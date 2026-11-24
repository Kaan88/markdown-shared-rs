//! The loop: winit's `ApplicationHandler` for the one window, which reduces its events, calls
//! the application, draws the damage and decides when to wake.

use std::rc::Rc;
use std::time::{Duration, Instant};

use mdshared_app::backdrop::Backdrop;
use mdshared_text::Bitmap;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{ResizeDirection, Window, WindowId};

use super::cx::{Cx, Shared};
use super::frame::{Damage, Deadlines, Visibility, minimized, present};
use super::input::{Clicks, button, clean_scale, click_slop, logical, wheel};
use super::{App, Button, Config, Event, KeyPress, Proxy, Wake};
use crate::anim::Animator;
use crate::corners::Corners;
use crate::cursor::{Cursor, CursorState};
use crate::frost::Frost;
use crate::gfx::{Gfx, Painter, Rect};
use crate::keys::Mods;
use crate::textfield::Preedit;
use crate::theme::LIGHT;
use crate::{chrome, platform, snap, window};

/// The window and what draws into it, once it exists.
struct Win {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    bmp: Bitmap,
    #[cfg(all(feature = "a11y", windows))]
    adapter: accesskit_winit::Adapter,
}

struct Host<A: App> {
    app: A,
    config: Config,
    proxy: Proxy<A::Message>,
    win: Option<Win>,
    gfx: Gfx,
    anim: Animator,
    st: Shared,
    cursor: CursorState,
    frost: Frost,
    /// The pointer, logical px.
    pointer: (f32, f32),
    clicks: Clicks,
    double_click: Duration,
    /// The double-click rectangle, device px.
    double_click_size: (u32, u32),
    /// What [`snap`] was last told; `None` before the first time and after a change of scale.
    snap_sent: Option<Option<Rect>>,
    /// The loop asked the window for a redraw and has not had it yet.
    requested: bool,
    #[cfg(all(feature = "a11y", windows))]
    latest: std::sync::Arc<std::sync::Mutex<Option<accesskit::TreeUpdate>>>,
}

/// Opens the window `config` describes and runs `app` in it until it calls [`Cx::exit`]. Call it
/// on the main thread (macOS requires it), once per process. Fails when winit cannot make its
/// event loop or the loop ends with an error.
pub fn run<A: App>(config: Config, app: A) -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::<Wake<A::Message>>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = Proxy(event_loop.create_proxy());
    let st = Shared {
        title: config.title.clone(),
        scale: 1.0,
        size: config.size,
        focused: false,
        maximized: false,
        vis: Visibility::default(),
        mods: Mods::default(),
        dark: config.dark,
        corners: config.corners,
        high_contrast: false,
        damage: Damage::default(),
        animated: Damage::default(),
        deadlines: Deadlines::default(),
        cursor: Cursor::Default,
        snap: None,
        backdrop: Backdrop::Off,
        pal: LIGHT,
        exit: false,
        touched: false,
    };
    let mut host = Host {
        app,
        config,
        proxy,
        win: None,
        gfx: Gfx::new(),
        anim: Animator::new(),
        st,
        cursor: CursorState::new(),
        frost: Frost::default(),
        pointer: (0.0, 0.0),
        clicks: Clicks::default(),
        double_click: platform::double_click_time(),
        double_click_size: platform::double_click_size(),
        snap_sent: None,
        requested: false,
        #[cfg(all(feature = "a11y", windows))]
        latest: Default::default(),
    };
    event_loop.run_app(&mut host)
}

/// Serves the latest tree at once when an assistive technology first asks, on whichever thread
/// asks (the loop's proxy alone would answer with a placeholder first).
#[cfg(all(feature = "a11y", windows))]
struct Latest(std::sync::Arc<std::sync::Mutex<Option<accesskit::TreeUpdate>>>);

#[cfg(all(feature = "a11y", windows))]
impl accesskit::ActivationHandler for Latest {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        self.0.lock().ok()?.clone()
    }
}

#[cfg(windows)]
fn hwnd(w: &Window) -> Option<isize> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match w.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

/// The dark title-bar mode and the corners (Windows; nothing elsewhere).
pub(super) fn style(w: &Window, dark: bool, corners: Corners) {
    #[cfg(windows)]
    if let Some(h) = hwnd(w) {
        platform::style_window(h, dark);
        crate::corners::set_window_corners(h, corners);
    }
    #[cfg(not(windows))]
    let _ = (w, dark, corners);
}

/// Whether high contrast is on, read where it is cheap: Windows (`SPI_GETHIGHCONTRAST`). Off
/// Windows the portal read can wait half a second, and the host shows no backdrop there to turn
/// off, so it is not read (the application reads it itself).
fn read_high_contrast() -> bool {
    #[cfg(windows)]
    return platform::system_high_contrast();
    #[cfg(not(windows))]
    false
}

impl<A: App> Host<A> {
    /// Calls the application with a [`Cx`], then does what it asked.
    fn call(&mut self, now: Instant, f: impl FnOnce(&mut A, &mut Cx<'_, A::Message>)) {
        let Some(win) = self.win.as_ref() else {
            return;
        };
        let mut cx = Cx {
            window: &win.window,
            anim: &mut self.anim,
            proxy: &self.proxy,
            st: &mut self.st,
            now,
            drawing: None,
        };
        f(&mut self.app, &mut cx);
        self.after();
    }

    /// After a call: the cursor, the snap rectangle, the accessibility tree.
    fn after(&mut self) {
        self.sync_window();
        if std::mem::take(&mut self.st.touched) {
            self.refresh_tree();
        }
    }

    /// The cursor and the snap rectangle, after a call or a frame.
    fn sync_window(&mut self) {
        self.sync_cursor();
        if let Some(win) = self.win.as_ref()
            && self.snap_sent != Some(self.st.snap)
        {
            self.snap_sent = Some(self.st.snap);
            snap::set_maximize_button_rect(&win.window, self.st.snap);
        }
    }

    /// The resize cursor on the edge band, else the application's.
    fn sync_cursor(&mut self) {
        let Some(win) = self.win.as_ref() else {
            return;
        };
        let want = self
            .edge(self.pointer)
            .map_or(self.st.cursor, Cursor::of_resize);
        if let Some(icon) = self.cursor.set(want) {
            win.window.set_cursor(icon);
        }
    }

    /// The resize direction at `at` (logical px): only for a restored window drawn by us.
    fn edge(&self, at: (f32, f32)) -> Option<ResizeDirection> {
        if self.st.maximized || chrome::NATIVE_FRAME {
            return None;
        }
        window::edge_at(at.0, at.1, self.st.size.0, self.st.size.1)
    }

    #[cfg(all(feature = "a11y", windows))]
    fn refresh_tree(&mut self) {
        let Some(win) = self.win.as_ref() else {
            return;
        };
        let mut cx = Cx {
            window: &win.window,
            anim: &mut self.anim,
            proxy: &self.proxy,
            st: &mut self.st,
            now: Instant::now(),
            drawing: None,
        };
        let tree = self.app.accessibility(&mut cx);
        self.st.touched = false;
        if let Ok(mut l) = self.latest.lock() {
            *l = Some(tree.clone());
        }
        if let Some(win) = self.win.as_mut() {
            win.adapter.update_if_active(|| tree);
        }
    }

    /// No adapter here: the tree is not asked for.
    #[cfg(not(all(feature = "a11y", windows)))]
    fn refresh_tree(&mut self) {}

    /// The window's size, scale and state as the window says them now.
    fn read_window(&mut self) {
        let Some(win) = self.win.as_ref() else {
            return;
        };
        let w = &win.window;
        self.st.scale = clean_scale(w.scale_factor());
        let s = w.inner_size();
        if self
            .st
            .vis
            .set_minimized(minimized((s.width, s.height), w.is_minimized()))
        {
            // Seen again after being minimized.
            self.st.damage.all();
        }
        if s.width > 0 && s.height > 0 {
            self.st.size = logical(f64::from(s.width), f64::from(s.height), self.st.scale);
        }
        self.st.maximized = w.is_maximized();
        if let Some(mhz) = w
            .current_monitor()
            .and_then(|m| m.refresh_rate_millihertz())
            .filter(|&r| r > 0)
        {
            self.anim
                .set_frame_interval(Duration::from_secs_f64(1000.0 / f64::from(mhz)));
        }
    }

    /// Reads high contrast again; on a switch, turns the backdrop by it, redraws everything and
    /// tells the application.
    fn read_contrast(&mut self, now: Instant) {
        let hc = read_high_contrast();
        if hc != self.st.high_contrast {
            self.st.high_contrast = hc;
            self.st.damage.all();
            self.call(now, |app, cx| app.event(cx, Event::Contrast(hc)));
        }
    }

    /// Brings the backdrop in line with the application's wish, the contrast and the focus.
    /// True when the chrome turned frosted or flat: everything is drawn again.
    #[cfg(windows)]
    fn sync_frost(&mut self) -> bool {
        let want = crate::tokens::allowed_backdrop(self.st.backdrop, self.st.high_contrast);
        if !self.frost.concerns(want) {
            return false;
        }
        let Some(win) = self.win.as_mut() else {
            return false;
        };
        let mut sys = crate::frost::WindowSystem(hwnd(&win.window));
        let changed = self
            .frost
            .sync(want, self.st.focused, &self.st.pal, &mut sys);
        if changed {
            // Every mark goes: the chrome puts its own back when it is drawn frosted.
            for px in win.bmp.pixels.as_chunks_mut::<4>().0 {
                px[3] = 255;
            }
        }
        changed
    }

    /// No backdrop off Windows: the frame is presented without alpha (stock softbuffer), so
    /// keyed-out chrome would be black. The chrome stays flat.
    #[cfg(not(windows))]
    fn sync_frost(&mut self) -> bool {
        false
    }

    /// Draws the damage and presents it. Nothing while the window cannot be seen: the damage
    /// waits.
    fn frame(&mut self, now: Instant) {
        if self.st.vis.hidden() {
            return;
        }
        let Some(win) = self.win.as_mut() else {
            return;
        };
        let s = win.window.inner_size();
        let (w, h) = (s.width.max(1), s.height.max(1));
        if win.bmp.width != w || win.bmp.height != h {
            win.bmp = Bitmap::new(w, h);
            self.st.damage.all();
        }
        if self.sync_frost() {
            self.st.damage.all();
        }
        let scale = self.st.scale;
        let damage = std::mem::take(&mut self.st.damage);
        let Some(dev) = damage.device(scale, w, h) else {
            return;
        };
        self.st.animated = Damage::default();
        let Some(win) = self.win.as_mut() else {
            return;
        };
        let clip = (dev.0 as i32, dev.1 as i32, dev.2 as i32, dev.3 as i32);
        let damage = Rect::new(
            dev.0 as f32 / scale,
            dev.1 as f32 / scale,
            (dev.2 - dev.0) as f32 / scale,
            (dev.3 - dev.1) as f32 / scale,
        );
        self.anim.begin_frame(now);
        {
            let Win { window, bmp, .. } = win;
            let mut p = Painter::new(bmp, &mut self.gfx, scale);
            p.clip(damage);
            let mut cx = Cx {
                window,
                anim: &mut self.anim,
                proxy: &self.proxy,
                st: &mut self.st,
                now,
                drawing: Some((clip, self.frost.key().is_some())),
            };
            self.app.draw(&mut cx, &mut p, damage);
        }
        self.st.deadlines.anim = self.anim.end_frame();
        present(&mut win.surface, &win.bmp, dev, self.frost.key());
        // A redraw asked for while drawing belongs to the next frame; the tree is not asked
        // again for it.
        self.st.touched = false;
        self.sync_window();
    }

    /// Presents the whole bitmap again, as it is.
    fn present_all(&mut self) {
        if let Some(win) = self.win.as_mut() {
            let (w, h) = (win.bmp.width, win.bmp.height);
            present(&mut win.surface, &win.bmp, (0, 0, w, h), self.frost.key());
        }
    }

    /// winit's event as the toolkit's, after what the host does with it itself. `None` when
    /// the application is not told.
    fn reduce(&mut self, now: Instant, event: WindowEvent) -> Option<Event> {
        let window = self.win.as_ref()?.window.clone();
        let scale = self.st.scale;
        Some(match event {
            WindowEvent::CloseRequested => Event::CloseRequested,
            WindowEvent::RedrawRequested => {
                // One the loop did not ask for is the system's: the window needs all its
                // pixels again, from the bitmap (mdview's rule), after drawing any damage.
                let expose = !std::mem::take(&mut self.requested);
                if self.st.vis.hidden() {
                    return None;
                }
                let s = window.inner_size();
                let bmp = self.win.as_ref().map(|w| (w.bmp.width, w.bmp.height));
                if expose && bmp != Some((s.width, s.height)) {
                    self.st.damage.all();
                }
                self.frame(now);
                if expose {
                    self.present_all();
                }
                return None;
            }
            WindowEvent::Resized(_) => {
                self.read_window();
                if self.st.vis.hidden() {
                    // Minimized: nothing to lay out, and the size kept is the last real one.
                    return None;
                }
                self.st.damage.all();
                // Bounds may have moved with the layout: the tree is asked again.
                self.st.touched = true;
                let (w, h) = self.st.size;
                Event::Resize { w, h }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                self.read_window();
                self.st.damage.all();
                // The snap rectangle is sent in device px: again at the new scale; and the
                // accessibility tree's bounds are device px too.
                self.snap_sent = None;
                self.st.touched = true;
                Event::Scale(self.st.scale)
            }
            WindowEvent::ThemeChanged(t) => {
                self.read_contrast(now);
                Event::Theme {
                    dark: t == winit::window::Theme::Dark,
                }
            }
            WindowEvent::Occluded(occluded) => {
                if self.st.vis.set_occluded(occluded) {
                    self.st.damage.all();
                }
                return None;
            }
            WindowEvent::Focused(focused) => {
                let changed = self.st.focused != focused;
                self.st.focused = focused;
                if focused {
                    // The places the system's wish for less motion, and the contrast, are read
                    // again.
                    self.anim.set_system_reduced(platform::motion_reduced());
                    self.read_contrast(now);
                }
                if changed && self.st.backdrop != Backdrop::Off {
                    self.st.damage.all();
                }
                if !focused {
                    self.cursor.forget();
                }
                Event::Focus(focused)
            }
            WindowEvent::ModifiersChanged(m) => {
                self.st.mods = Mods::from_winit(m.state());
                Event::Modifiers(self.st.mods)
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = logical(position.x, position.y, scale);
                let (x, y) = self.pointer;
                Event::PointerMove { x, y }
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor.forget();
                Event::PointerLeave
            }
            WindowEvent::MouseInput {
                state, button: b, ..
            } => {
                let b = button(b)?;
                let (x, y) = self.pointer;
                match state {
                    ElementState::Pressed => {
                        if b == Button::Left
                            && let Some(dir) = self.edge(self.pointer)
                        {
                            let _ = window.drag_resize_window(dir);
                            return None;
                        }
                        let slop = click_slop(self.double_click_size, scale);
                        let clicks = self.clicks.press(now, (x, y), b, self.double_click, slop);
                        Event::Press {
                            x,
                            y,
                            button: b,
                            clicks,
                        }
                    }
                    ElementState::Released => Event::Release { x, y, button: b },
                }
            }
            WindowEvent::MouseWheel { delta, .. } => wheel(delta, scale, self.st.mods.shift),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state.is_pressed() => Event::Key(KeyPress {
                name: crate::keys::key_name(&event.logical_key),
                mods: self.st.mods,
                text: event.text.as_ref().map(|t| t.to_string()),
                repeat: event.repeat,
            }),
            WindowEvent::Ime(winit::event::Ime::Commit(text)) => Event::Text(text),
            WindowEvent::Ime(winit::event::Ime::Preedit(text, cursor)) => {
                Event::Preedit(Preedit::new(text, cursor))
            }
            WindowEvent::Ime(winit::event::Ime::Enabled | winit::event::Ime::Disabled) => {
                Event::Preedit(None)
            }
            _ => return None,
        })
    }
}

impl<A: App> ApplicationHandler<Wake<A::Message>> for Host<A> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_some() {
            return;
        }
        let c = &self.config;
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title(c.title.clone())
            .with_decorations(false)
            .with_visible(false)
            .with_inner_size(LogicalSize::new(f64::from(c.size.0), f64::from(c.size.1)))
            .with_min_inner_size(LogicalSize::new(
                f64::from(c.min_size.0),
                f64::from(c.min_size.1),
            ));
        #[cfg(windows)]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs = attrs.with_undecorated_shadow(true);
        }
        let attrs = window::identify(attrs, &c.app_id, c.icon_png);
        // Shown, the window is activated (winit's `with_active`, true unless set): the first
        // frame is drawn as it will be seen, focused, so the chrome does not flip at launch.
        let will_be_active = attrs.active;
        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("mdshared-ui host: no window: {e}");
                el.exit();
                return;
            }
        };
        window::created(&window);
        style(&window, self.st.dark, self.st.corners);
        // Before the window is shown: AccessKit panics on a visible one.
        #[cfg(all(feature = "a11y", windows))]
        let adapter = accesskit_winit::Adapter::with_mixed_handlers(
            el,
            &window,
            Latest(self.latest.clone()),
            self.proxy.0.clone(),
        );
        let surface = softbuffer::Context::new(window.clone())
            .and_then(|context| softbuffer::Surface::new(&context, window.clone()));
        let surface = match surface {
            Ok(s) => s,
            Err(e) => {
                eprintln!("mdshared-ui host: no surface: {e}");
                el.exit();
                return;
            }
        };
        self.win = Some(Win {
            window: window.clone(),
            surface,
            bmp: Bitmap::new(1, 1),
            #[cfg(all(feature = "a11y", windows))]
            adapter,
        });
        self.read_window();
        // Hidden, the window cannot be minimized; a zero size reported for it is not one.
        self.st.vis = Visibility::default();
        self.st.focused = will_be_active || window.has_focus();
        self.st.high_contrast = read_high_contrast();
        self.anim.set_system_reduced(platform::motion_reduced());
        let now = Instant::now();
        self.call(now, |app, cx| app.start(cx));
        self.refresh_tree();
        // The first frame, to the hidden window; then shown, and presented again.
        self.st.damage.all();
        self.frame(now);
        window.set_visible(true);
        self.present_all();
        // The system decides about the focus; if the window did not get it, the frame drawn
        // focused is drawn again (winit sends no `Focused(false)` for a focus it never gave).
        if self.st.focused && !window.has_focus() {
            self.st.focused = false;
            if self.st.backdrop != Backdrop::Off {
                self.st.damage.all();
            }
            self.call(now, |app, cx| app.event(cx, Event::Focus(false)));
        }
        // The contrast switch reaches no winit event on Windows: the host watches it (a no-op
        // elsewhere). After the first frame: it waits a few ms for its window.
        let proxy = self.proxy.0.clone();
        platform::watch_contrast(move || {
            let _ = proxy.send_event(Wake::Contrast);
        });
        if self.st.exit {
            el.exit();
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, wake: Wake<A::Message>) {
        if self.st.exit {
            // Winding down: what arrives now is dropped (`Proxy::send`'s doc).
            return;
        }
        let now = Instant::now();
        match wake {
            Wake::Message(m) => self.call(now, |app, cx| app.message(cx, m)),
            Wake::Contrast => self.read_contrast(now),
            #[cfg(all(feature = "a11y", windows))]
            Wake::Access(e) => match e.window_event {
                accesskit_winit::WindowEvent::ActionRequested(req) => {
                    self.call(now, |app, cx| app.event(cx, Event::Action(req)));
                }
                // The adapter is made with a direct activation handler (`Latest`), so the first
                // tree is never asked for through the loop; and nothing is kept for an
                // assistive technology that leaves. Named so the match stays exhaustive.
                accesskit_winit::WindowEvent::InitialTreeRequested
                | accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
            },
        }
        if self.st.exit {
            el.exit();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(win) = self.win.as_mut() else {
            return;
        };
        #[cfg(all(feature = "a11y", windows))]
        win.adapter.process_event(&win.window, &event);
        #[cfg(not(all(feature = "a11y", windows)))]
        let _ = win;
        let now = Instant::now();
        if let Some(ev) = self.reduce(now, event) {
            self.call(now, |app, cx| app.event(cx, ev));
        }
        if self.st.exit {
            // Ended here, not in `about_to_wait`: a close from the system alone (the taskbar's
            // "Close window") is followed by no other event (mdview's finding).
            el.exit();
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        let hidden = self.st.vis.hidden();
        let due = self.st.deadlines.take_due(now, hidden);
        if due.anim {
            let moving = self.st.animated;
            if moving.is_empty() {
                self.st.damage.all();
            } else {
                self.st.damage.merge(moving);
            }
        }
        if due.timer {
            self.call(now, |app, cx| app.event(cx, Event::Wake(now)));
        }
        if self.st.exit {
            el.exit();
            return;
        }
        if !self.st.damage.is_empty()
            && !hidden
            && !self.requested
            && let Some(win) = self.win.as_ref()
        {
            self.requested = true;
            win.window.request_redraw();
        }
        el.set_control_flow(match self.st.deadlines.next(hidden) {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        });
    }
}
