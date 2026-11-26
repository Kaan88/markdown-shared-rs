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
use super::frame::{Damage, Deadlines, Visibility, minimized, next_wake, present, wants_frame};
use super::input::{Clicks, button, clean_scale, click_slop, logical, wheel};
use super::timers::NamedTimers;
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
    /// The application's named timers ([`Cx::timer_at`]).
    timers: NamedTimers,
    st: Shared,
    cursor: CursorState,
    frost: Frost,
    /// The pointer, logical px.
    pointer: (f32, f32),
    /// The pointer has been over the window since it last left: until then `pointer` is not
    /// where it is, and no resize cursor is shown for it.
    pointer_seen: bool,
    clicks: Clicks,
    double_click: Duration,
    /// The double-click rectangle, device px.
    double_click_size: (u32, u32),
    /// What [`snap`] was last told; `None` before the first time and after a change of scale.
    snap_sent: Option<Option<Rect>>,
    /// The loop asked the window for a redraw and has not had it yet.
    requested: bool,
    #[cfg(all(feature = "a11y", windows))]
    access: Shared11y,
}

/// The accessibility tree, shared with the adapter's activation handler, which asks for it
/// inside the window procedure (on Windows, the window's own thread).
#[cfg(all(feature = "a11y", windows))]
type Shared11y = std::sync::Arc<std::sync::Mutex<Access>>;

#[cfg(all(feature = "a11y", windows))]
struct Access {
    tree: crate::a11y::Tree,
    /// The node with the keyboard, as the application last answered.
    focus: accesskit::NodeId,
    /// The application was not asked while nobody listened: the tree may be behind.
    stale: bool,
}

/// Opens the window `config` describes and runs `app` in it until it calls [`Cx::exit`]. Call it
/// on the main thread (macOS requires it), once per process. Fails when winit cannot make its
/// event loop or the loop ends with an error.
pub fn run<A: App>(config: Config, app: A) -> Result<(), winit::error::EventLoopError> {
    let collect = app.damage_regions();
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
        damage: Damage::with_pieces(collect),
        animated: Damage::with_pieces(collect),
        deadlines: Deadlines::default(),
        cursor: Cursor::Default,
        snap: None,
        backdrop: Backdrop::Off,
        veil: None,
        pal: LIGHT,
        exit: false,
        touched: false,
        show: false,
        shown: false,
        no_resize: Vec::new(),
    };
    let mut host = Host {
        app,
        config,
        proxy,
        win: None,
        gfx: Gfx::new(),
        anim: Animator::new(),
        timers: NamedTimers::default(),
        st,
        cursor: CursorState::new(),
        frost: Frost::default(),
        pointer: (0.0, 0.0),
        pointer_seen: false,
        clicks: Clicks::default(),
        double_click: platform::double_click_time(),
        double_click_size: platform::double_click_size(),
        snap_sent: None,
        requested: false,
        #[cfg(all(feature = "a11y", windows))]
        access: {
            let root = crate::a11y::id("mdshared-ui host window");
            std::sync::Arc::new(std::sync::Mutex::new(Access {
                tree: crate::a11y::Tree::new(root),
                focus: root,
                stale: true,
            }))
        },
    };
    event_loop.run_app(&mut host)
}

/// Serves the whole tree at once when an assistive technology first asks, inside the window
/// procedure (the loop's proxy alone would answer with a placeholder first). A tree not kept up to
/// date while nobody listened is served as it is, and the loop is woken to ask for the nodes
/// again.
#[cfg(all(feature = "a11y", windows))]
struct Latest<M: 'static> {
    access: Shared11y,
    wake: winit::event_loop::EventLoopProxy<Wake<M>>,
}

#[cfg(all(feature = "a11y", windows))]
impl<M: Send + 'static> accesskit::ActivationHandler for Latest<M> {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        let access = self.access.lock().ok()?;
        if access.stale || access.tree.is_empty() {
            let _ = self.wake.send_event(Wake::Refresh);
        }
        // Never built yet (asked while the window is still being made): nothing, so that the
        // first update the loop gives, which then holds every node and the tree's info, is the
        // adapter's first tree. A tree served here and the updates after it must agree.
        if access.tree.is_empty() {
            return None;
        }
        Some(access.tree.full(access.focus))
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

/// `d` (device px, x0, y0, x1, y1) as a rectangle in logical px at `scale`.
fn region_rect(d: (u32, u32, u32, u32), scale: f32) -> Rect {
    Rect::new(
        d.0 as f32 / scale,
        d.1 as f32 / scale,
        (d.2 - d.0) as f32 / scale,
        (d.3 - d.1) as f32 / scale,
    )
}

/// Paints `pieces` (device px) into `bmp`, one call each, with the painter clipped to that
/// piece: the loop [`Host::frame`] runs for an application that draws regions. `draw` is given
/// the painter, the animator, the piece in logical px and its device clip. The animator's group
/// is reset before each call, as `begin_frame` does for the one-call frame, so a group set in
/// one region's draw does not leak into the next.
#[allow(clippy::too_many_arguments)] // a drawing loop: bitmap, animator, timers and pieces
pub(super) fn paint_regions(
    bmp: &mut Bitmap,
    gfx: &mut Gfx,
    anim: &mut Animator,
    timers: &mut NamedTimers,
    key: Option<crate::frost::Key>,
    scale: f32,
    pieces: &[(u32, u32, u32, u32)],
    mut draw: impl FnMut(&mut Painter, &mut Animator, &mut NamedTimers, Rect, (i32, i32, i32, i32)),
) {
    for &dev in pieces {
        let clip = (dev.0 as i32, dev.1 as i32, dev.2 as i32, dev.3 as i32);
        let region = region_rect(dev, scale);
        anim.ungroup();
        let mut p = Painter::new(bmp, gfx, scale);
        p.set_frost_key(key);
        p.clip(region);
        draw(&mut p, anim, timers, region, clip);
    }
}

/// Fires the named timers of one turn: the names due when the turn began (earliest first, set
/// order at one instant), each only while it is still waiting and still due — a callback may
/// have cancelled or moved it. Anything set during the turn, even at `now`, is not in the
/// snapshot and waits for the next turn, so a callback that re-arms itself cannot spin the
/// loop. `fire` answers whether to go on (false stops, for an application that exited). True
/// when at least one timer fired.
pub(super) fn fire_due_timers(
    timers: &mut NamedTimers,
    now: Instant,
    mut fire: impl FnMut(&mut NamedTimers, &str) -> bool,
) -> bool {
    let mut fired = false;
    for name in timers.due_at(now) {
        if !timers.take_if_due(&name, now) {
            continue;
        }
        fired = true;
        if !fire(timers, &name) {
            break;
        }
    }
    fired
}

/// Whether the window can be hidden with `set_visible` and drawn again later. On Wayland a
/// window is a surface of its own and winit's `set_visible` is a no-op there, so hiding it
/// would only freeze it: [`Cx::hide_window`](super::cx::Cx::hide_window) does nothing and the
/// window keeps drawing.
pub(super) fn can_hide(w: &Window) -> bool {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        return !matches!(
            w.window_handle().map(|h| h.as_raw()),
            Ok(RawWindowHandle::Wayland(_))
        );
    }
    #[cfg(any(not(unix), target_os = "macos"))]
    {
        let _ = w;
        true
    }
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
            timers: &mut self.timers,
            proxy: &self.proxy,
            st: &mut self.st,
            now,
            drawing: None,
        };
        f(&mut self.app, &mut cx);
        self.after();
    }

    /// After a call: the cursor, the snap rectangle, the accessibility tree. The tree waits for
    /// the frame when one is coming, so that it reads the layout that frame draws.
    fn after(&mut self) {
        self.sync_window();
        if std::mem::take(&mut self.st.show) {
            self.show(Instant::now());
        }
        let frame_coming = wants_frame(self.st.vis.hidden(), &self.st.damage);
        if self.st.touched && !frame_coming {
            self.refresh_tree(false);
        }
    }

    /// Shows a window the application hid: draw and present it while it is still hidden, then
    /// show it, then read the keyboard. Called from `after`, which the host runs after every
    /// call, so the draw happens before the window is on screen.
    fn show(&mut self, now: Instant) {
        if !self.st.vis.app_hidden() {
            return;
        }
        self.st.vis.set_app_hidden(false);
        self.st.touched = true;
        self.reveal(now);
    }

    /// Draws the whole window and presents it to a window that is still hidden, then shows it
    /// and presents it again, so its client area is never seen undrawn. The first call (the
    /// launch) also builds the accessibility tree before showing. Then the keyboard is read
    /// again: a window hidden from launch has none.
    fn reveal(&mut self, now: Instant) {
        let first = !self.st.shown;
        self.st.shown = true;
        // Shown again, assume the system activates it, as launch does: the chrome is drawn
        // frosted from the first presented frame instead of flipping once the focus arrives.
        let was = self.st.focused;
        let assumed = if first { was } else { true };
        self.st.focused = assumed;
        self.st.damage.all();
        self.frame(now);
        if first {
            self.refresh_tree(true);
        }
        if let Some(win) = self.win.as_ref() {
            win.window.set_visible(true);
        }
        self.present_all();
        let Some(win) = self.win.as_ref() else {
            return;
        };
        let has = win.window.has_focus();
        self.st.focused = has;
        if has != assumed {
            // The frame drew the other focus, and the chrome may look different by it.
            self.st.damage.all();
        }
        if has != was {
            self.call(now, |app, cx| app.event(cx, Event::Focus(has)));
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

    /// The resize direction at `at` (logical px): only for a restored window drawn by us, with
    /// the pointer seen, and not over the snap rectangle or a place the application keeps.
    fn edge(&self, at: (f32, f32)) -> Option<ResizeDirection> {
        if self.st.maximized || chrome::NATIVE_FRAME || !self.pointer_seen {
            return None;
        }
        let kept = |r: &Rect| r.contains(at.0, at.1);
        if self.st.snap.as_ref().is_some_and(kept) || self.st.no_resize.iter().any(kept) {
            return None;
        }
        window::edge_at(at.0, at.1, self.st.size.0, self.st.size.1)
    }

    /// Asks the application for its nodes and gives the adapter what changed: only once an
    /// assistive technology has asked (Windows' adapter stays active from then on), unless `always` (once, before the window is shown, so the
    /// first request is served a real tree). Not asked, the tree is marked stale.
    #[cfg(all(feature = "a11y", windows))]
    fn refresh_tree(&mut self, always: bool) {
        self.st.touched = false;
        let Some(win) = self.win.as_mut() else {
            return;
        };
        let Ok(mut access) = self.access.lock() else {
            return;
        };
        let access = &mut *access;
        let Win {
            window, adapter, ..
        } = win;
        let mut cx = Cx {
            window,
            anim: &mut self.anim,
            timers: &mut self.timers,
            proxy: &self.proxy,
            st: &mut self.st,
            now: Instant::now(),
            drawing: None,
        };
        let app = &mut self.app;
        let mut build = || {
            access.tree.begin();
            access.focus = app.accessibility(&mut cx, &mut access.tree);
            access.stale = false;
            access.tree.end(access.focus)
        };
        if always {
            let update = build();
            adapter.update_if_active(|| update);
        } else {
            let mut built = false;
            adapter.update_if_active(|| {
                built = true;
                build()
            });
            if !built {
                access.stale = true;
            }
        }
        // A redraw asked for while the nodes were built is the application's business; the
        // tree is not asked again for it.
        self.st.touched = false;
    }

    /// No adapter here: the tree is not asked for.
    #[cfg(not(all(feature = "a11y", windows)))]
    fn refresh_tree(&mut self, _always: bool) {
        self.st.touched = false;
    }

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
    /// tells the application. `colours`: the watch woke, which it also does when the contrast
    /// theme's colours change with the switch still on, so the application is told anyway.
    fn read_contrast(&mut self, now: Instant, colours: bool) {
        let hc = read_high_contrast();
        if hc != self.st.high_contrast || (colours && hc) {
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
        self.frost.set_veil(self.st.veil);
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
        let damage = self.st.damage.take();
        let Some(union) = damage.device(scale, w, h) else {
            // The damage fell wholly outside the window: no frame, but `after` kept the tree
            // for one, so it is built now.
            if self.st.touched {
                self.refresh_tree(false);
            }
            return;
        };
        let pieces = if damage.collects() {
            damage.pieces(scale, w, h)
        } else {
            Vec::new()
        };
        // One call per rectangle only when the application asked for it and there is more than
        // one; otherwise the old one-call frame with the union, unchanged.
        let regioned = pieces.len() > 1;
        self.st.animated.reset();
        let Some(win) = self.win.as_mut() else {
            return;
        };
        self.anim.begin_frame(now);
        if regioned {
            let key = self.frost.key().copied();
            let frosted = key.is_some();
            let Win { window, bmp, .. } = win;
            let st = &mut self.st;
            let proxy = &self.proxy;
            let app = &mut self.app;
            paint_regions(
                bmp,
                &mut self.gfx,
                &mut self.anim,
                &mut self.timers,
                key,
                scale,
                &pieces,
                |p, anim, timers, region, clip| {
                    let mut cx = Cx {
                        window,
                        anim: &mut *anim,
                        timers: &mut *timers,
                        proxy,
                        st: &mut *st,
                        now,
                        drawing: Some((clip, frosted)),
                    };
                    app.draw(&mut cx, p, region);
                },
            );
        } else {
            let clip = (
                union.0 as i32,
                union.1 as i32,
                union.2 as i32,
                union.3 as i32,
            );
            let damage = region_rect(union, scale);
            let Win { window, bmp, .. } = win;
            let mut p = Painter::new(bmp, &mut self.gfx, scale);
            p.set_frost_key(self.frost.key().copied());
            p.clip(damage);
            let mut cx = Cx {
                window,
                anim: &mut self.anim,
                timers: &mut self.timers,
                proxy: &self.proxy,
                st: &mut self.st,
                now,
                drawing: Some((clip, self.frost.key().is_some())),
            };
            self.app.draw(&mut cx, &mut p, damage);
        }
        self.st.deadlines.anim = self.anim.end_frame();
        let presented: &[(u32, u32, u32, u32)] = if regioned { &pieces } else { &[union] };
        present(&mut win.surface, &win.bmp, presented, self.frost.key());
        self.sync_window();
        // Asked for before the frame (and kept for it by `after`), or said during it: the tree
        // reads the layout just drawn. A redraw asked for while drawing does not count.
        if self.st.touched {
            self.refresh_tree(false);
        }
    }

    /// Presents the whole bitmap again, as it is.
    fn present_all(&mut self) {
        if let Some(win) = self.win.as_mut() {
            let (w, h) = (win.bmp.width, win.bmp.height);
            present(
                &mut win.surface,
                &win.bmp,
                &[(0, 0, w, h)],
                self.frost.key(),
            );
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
                    // Minimized or covered: nothing to lay out, and the size kept is the last
                    // real one. A tree kept for a frame that will not come now is built at
                    // once. A window the application hid is not OS-hidden: its new size is
                    // still delivered, so it can lay out while away.
                    let tell = self.st.vis.app_hidden() && !self.st.vis.os_hidden();
                    self.after();
                    if tell {
                        let (w, h) = self.st.size;
                        return Some(Event::Resize { w, h });
                    }
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
                self.read_contrast(now, false);
                Event::Theme {
                    dark: t == winit::window::Theme::Dark,
                }
            }
            WindowEvent::Occluded(occluded) => {
                if self.st.vis.set_occluded(occluded) {
                    self.st.damage.all();
                }
                // Hidden now, a tree kept for a frame is built at once.
                self.after();
                return None;
            }
            WindowEvent::Focused(focused) => {
                let changed = self.st.focused != focused;
                self.st.focused = focused;
                if focused {
                    // The places the system's wish for less motion, and the contrast, are read
                    // again.
                    self.anim.set_system_reduced(platform::motion_reduced());
                    self.read_contrast(now, false);
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
                self.pointer_seen = true;
                let (x, y) = self.pointer;
                Event::PointerMove { x, y }
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer_seen = false;
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
            Latest {
                access: self.access.clone(),
                wake: self.proxy.0.clone(),
            },
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
        // The first frame, to the hidden window; then the tree, read from what it laid out;
        // then shown, and presented again. An application that hid the window in `start` (a
        // launch to the tray) is not shown; one that showed itself there has already been
        // drawn and shown by `after`.
        if !self.st.vis.app_hidden() && !self.st.shown {
            self.reveal(now);
        }
        // The contrast switch reaches no winit event on Windows: the host watches it (a no-op
        // elsewhere). After the first frame: it waits a few ms for its window.
        let proxy = self.proxy.0.clone();
        let motion = proxy.clone();
        platform::watch_motion_reduced(move || {
            let _ = motion.send_event(Wake::Motion);
        });
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
            Wake::Contrast => self.read_contrast(now, true),
            Wake::Motion => {
                self.anim.set_system_reduced(platform::motion_reduced());
                // What moves now snaps, or what was still starts: the frame says which.
                self.st.damage.all();
            }
            #[cfg(all(feature = "a11y", windows))]
            // Through `after`, so that a frame already asked for is drawn first and the
            // nodes read its layout.
            Wake::Refresh => {
                self.st.touched = true;
                self.after();
            }
            #[cfg(all(feature = "a11y", windows))]
            Wake::Access(e) => match e.window_event {
                accesskit_winit::WindowEvent::ActionRequested(req) => {
                    self.call(now, |app, cx| app.event(cx, Event::Action(req)));
                }
                // Not sent while the direct handler (`Latest`) is installed: it answers the
                // first request, or sends `Wake::Refresh` when the tree was never built. Kept
                // for an adapter that sends it.
                accesskit_winit::WindowEvent::InitialTreeRequested => self.refresh_tree(false),
                // Not sent on Windows (accesskit_winit drops the deactivation handler there):
                // once asked, the tree is built for the life of the window.
                accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
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
        self.call(now, |app, cx| app.raw(cx, &event));
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
        // A show asked for from a draw (where `after` does not run) is done here.
        if std::mem::take(&mut self.st.show) {
            self.show(now);
        }
        let hidden = self.st.vis.hidden();
        let due = self.st.deadlines.take_due(now, hidden);
        if due.anim {
            let moving = self.st.animated.take();
            let (w, h) = self
                .win
                .as_ref()
                .map_or((1, 1), |win| (win.bmp.width, win.bmp.height));
            // Motion named wholly outside the window would give a frame with nothing to draw,
            // and so no new deadline: the animation would stop. The whole window is drawn.
            if moving.is_empty() || moving.device(self.st.scale, w, h).is_none() {
                self.st.damage.all();
            } else {
                self.st.damage.merge(moving);
            }
        }
        if due.timer {
            self.call(now, |app, cx| app.event(cx, Event::Wake(now)));
        }
        // Named timers come whether or not the window is seen: the names due when the turn
        // begins, each still waiting and due when its turn comes. Anything set meanwhile,
        // even at `now`, waits for the next turn, so re-arming from a callback cannot spin.
        let fired = if let Some(win) = self.win.as_ref() {
            let window = &win.window;
            let st = &mut self.st;
            let anim = &mut self.anim;
            let proxy = &self.proxy;
            let app = &mut self.app;
            fire_due_timers(&mut self.timers, now, |timers, name| {
                {
                    let mut cx = Cx {
                        window,
                        anim: &mut *anim,
                        timers: &mut *timers,
                        proxy,
                        st: &mut *st,
                        now,
                        drawing: None,
                    };
                    app.timer_fired(&mut cx, name);
                }
                !st.exit
            })
        } else {
            false
        };
        if fired {
            self.after();
        }
        if self.st.exit {
            el.exit();
            return;
        }
        // An application that hid or showed the window in a timer callback changed this.
        let hidden = self.st.vis.hidden();
        if wants_frame(hidden, &self.st.damage)
            && !self.requested
            && let Some(win) = self.win.as_ref()
        {
            self.requested = true;
            win.window.request_redraw();
        }
        let next = next_wake(&self.st.deadlines, &self.timers, hidden);
        el.set_control_flow(match next {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        });
    }
}
