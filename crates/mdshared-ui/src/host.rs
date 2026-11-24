//! The window host: the winit loop, the softbuffer present and the timers of ONE window, so
//! that an application writes its model, its drawing and its reactions, and not the loop that
//! every application here has rewritten (roadmap W1, at Purge's scope). Behind the feature
//! `host`; with `a11y` too, on Windows, the host also serves the application's AccessKit tree to
//! UI Automation.
//!
//! **Who owns what.** The application implements [`App`] and keeps its models, its hits and its
//! state; [`run`] owns winit's `ApplicationHandler`, the softbuffer surface, the [`Bitmap`] the
//! frame is drawn into, the [`Gfx`] (fonts and glyph caches) and the window's one
//! [`Animator`]. Nothing is called back into a widget: the host hands the application plain
//! [`Event`]s, and the application tells the host what it wants through the [`Cx`] it is given
//! with each one (a redraw of a rectangle, a cursor, a wake-up, the window dragged).
//!
//! **Events.** winit's window events are reduced to the toolkit's own, in logical px: the
//! pointer moving, a press (with its click count, for a double click) and a release, the wheel
//! (a notch is [`WHEEL_LINE`] px, Shift turns it sideways by
//! [`viewport::wheel_delta`](crate::viewport::wheel_delta)), a key press as a
//! [`keys::KeyName`](crate::keys::KeyName) with the modifiers and the text it typed, the text
//! and composition of an input method, the focus, the scale, the size, the system's theme, and
//! the close request. A worker's reply arrives through [`App::message`], sent from any thread
//! with the [`Proxy`] that [`Cx::proxy`] gives.
//!
//! **Frames.** The application asks for a redraw of a rectangle ([`Cx::request_redraw`]); the
//! rectangles asked for before a frame are joined into the one rectangle that holds them all,
//! the frame draws that rectangle only ([`App::draw`] is given it, and the painter is clipped to
//! it), and only that rectangle is sent to the window (softbuffer's `present_with_damage`). The
//! bitmap keeps the rest of the last frame. A resize, a change of scale, the backdrop turning
//! on or off, and the window coming back from being covered repaint everything. A window that
//! is covered (winit's `Occluded`) is not drawn until it is seen again.
//!
//! **One wake-up.** The animator's deadline ([`Animator::end_frame`]), which already holds a
//! caret's blink ([`textfield::TextField::blink_caret`](crate::textfield::TextField::blink_caret))
//! and anything else in motion, and the application's own timers ([`Cx::wake_at`], a tooltip's
//! delay) are folded into one `ControlFlow::WaitUntil`; with neither, the loop waits for an event
//! (`ControlFlow::Wait`) and an idle window does no work. When the animator's deadline comes,
//! the host redraws what the last frame said was moving ([`Cx::animating`]; the whole window
//! if it said nothing). When a timer comes, the application gets [`Event::Wake`] and decides.
//!
//! **The window.** Undecorated, with the size and the minimum size of the [`Config`], the
//! system's shadow on Windows, and the dark title-bar mode and corners of the config. A press
//! on the 8 px band at the edge of a restored window resizes it ([`window::edge_at`]) and shows
//! the resize cursor there; the title row is the application's, which drags the window by
//! [`Cx::drag_window`] and gets snap layouts by [`Cx::set_maximize_button`]. A backdrop (Mica
//! or Acrylic, [`Cx::set_backdrop`]) is asked for through [`frost::Frost`], which refuses it
//! when the system shows none (transparency off, battery saver, high contrast), and the chrome
//! the application marks with [`Cx::frost`] lets it through.
//!
//! **The first frame.** The window is created hidden, the first frame is drawn and presented to
//! it, then it is shown and the frame presented again (a frame presented to a hidden window
//! shows nothing): mdview's rules, which use no DWM cloak. So it never shows white.
//!
//! **Accessibility** (features `host` and `a11y`, Windows). The AccessKit adapter is made with
//! `with_mixed_handlers` before the window is shown. The application gives its tree by
//! [`App::accessibility`]; the host asks for it before the first frame and after every call that
//! asked for a redraw or said [`Cx::accessibility_changed`], keeps the latest for the system's
//! first request (served at once, on the thread that asks), and hands it to the adapter when an
//! assistive technology is listening. A request from one (Invoke, Toggle, Focus, ScrollIntoView)
//! is [`Event::Action`].
//!
//! Deliberately not here: tabs and several windows (Purge's scope: one window); the
//! client-drawn shadow of Linux ([`shadow`](crate::shadow)) and the rounded corners it cuts,
//! which mdview wires itself; the window's place and size kept between runs
//! ([`window_state`](crate::window_state) is the application's to call); a menu bar, a tray, a
//! file drop, touch, pinch and pen; key releases; regions of the window with their own damage
//! (the damage is one rectangle); a GPU. On Linux the backdrop is asked of KDE
//! ([`frost::BlurSystem`]) but the frame carries no alpha with stock softbuffer (mdview's
//! patched one has `set_premultiplied_alpha`), so it does not show there. Tested: everything pure (the damage, the deadlines, the
//! click count, the reduction of events) in `host_tests.rs`; the window parts by the example
//! `cargo run -p mdshared-ui --features host --example host`, compiled and run headless for a
//! few seconds on Windows, not seen by a person; Linux and macOS only compiled.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mdshared_app::backdrop::Backdrop;
use mdshared_text::Bitmap;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{ResizeDirection, Window, WindowId};

use crate::anim::Animator;
use crate::corners::Corners;
use crate::cursor::{Cursor, CursorState};
use crate::frost::{self, Frost};
use crate::gfx::{Gfx, Painter, Rect};
use crate::keys::{KeyName, Mods};
use crate::textfield::Preedit;
use crate::theme::{LIGHT, Palette};
use crate::{chrome, platform, snap, window};

/// One notch of a mouse wheel, in logical px: what a wheel that reports lines (winit's
/// `LineDelta`) moves per line. mdview counts a notch as this much too.
pub const WHEEL_LINE: f32 = 48.0;

/// How far apart, in logical px each way, two presses may be and still count as one double
/// click (Windows' `SM_CXDOUBLECLK` is 4 device px).
const CLICK_SLOP: f32 = 4.0;

// ---- events ----------------------------------------------------------------------------------

/// A mouse button. Buttons beyond these five are not reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

/// A key pressed (or repeated while held).
#[derive(Clone, Debug, PartialEq)]
pub struct KeyPress {
    /// The key as shortcuts name it ([`keys::key_name`](crate::keys::key_name)); `None` for a key
    /// the shortcut strings have no name for (a modifier alone, a media key).
    pub name: Option<KeyName>,
    /// The modifiers held.
    pub mods: Mods,
    /// The text the key typed, as winit gives it (control characters included: a caller that
    /// edits passes it to [`TextField::handle_key`](crate::textfield::TextField::handle_key),
    /// which drops them).
    pub text: Option<String>,
    /// True when the key is held and the system repeats it.
    pub repeat: bool,
}

/// What happened to the window, in the toolkit's terms. Positions and sizes are logical px from
/// the top-left of the window's client area.
///
/// The enum is `non_exhaustive`: a `match` on it needs a `_` arm, so a variant added later (or
/// one that only a feature brings) breaks no application.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Event {
    /// The pointer moved to `(x, y)`. Sent on the resize band too, where a press resizes.
    PointerMove { x: f32, y: f32 },
    /// The pointer left the window.
    PointerLeave,
    /// A button went down at `(x, y)`. `clicks` is 1, then 2 for a double click, 3 for a triple,
    /// counted while the presses of one button come within the system's double-click time and
    /// a few px of each other. Not sent for a press on the resize band of a restored window,
    /// which resizes it.
    Press {
        x: f32,
        y: f32,
        button: Button,
        clicks: u32,
    },
    /// A button went up at `(x, y)`.
    Release { x: f32, y: f32, button: Button },
    /// The wheel or a touchpad moved the view: `dx` across, `dy` down, logical px, positive when
    /// the content should move right or down (winit's sign: a wheel turned towards the user is
    /// positive `dy`). A vertical wheel with Shift held arrives as `dx`. `precise` is true for a
    /// touchpad's pixel deltas and false for a wheel's notches ([`WHEEL_LINE`] each).
    Wheel { dx: f32, dy: f32, precise: bool },
    /// A key went down.
    Key(KeyPress),
    /// Text an input method committed (an accent typed with a dead key, Chinese or Japanese
    /// input). Input methods are on only on macOS ([`window::created`]).
    Text(String),
    /// What an input method is composing, `None` when it stopped.
    Preedit(Option<Preedit>),
    /// The modifiers held changed ([`Cx::mods`] has them too).
    Modifiers(Mods),
    /// The window got (`true`) or lost the keyboard.
    Focus(bool),
    /// The scale changed: a monitor of another density, or the system's setting. The whole
    /// window is redrawn after it.
    Scale(f32),
    /// The window's size changed, logical px. The whole window is redrawn after it.
    Resize { w: f32, h: f32 },
    /// The system's light or dark mode changed.
    Theme { dark: bool },
    /// The time asked for by [`Cx::wake_at`] came; the instant is now. Ask again for a later one.
    Wake(Instant),
    /// The window's close button of the system, Alt+F4 or the taskbar asked to close. The
    /// window stays open unless the application calls [`Cx::exit`].
    CloseRequested,
    /// An assistive technology asked for an action (features `host` and `a11y`, Windows).
    #[cfg(all(feature = "a11y", windows))]
    Action(accesskit::ActionRequest),
}

// ---- the application -------------------------------------------------------------------------

/// What an application gives the host. The host calls these on the loop's thread, one at a
/// time; each gets a [`Cx`] to ask the host for things.
pub trait App: 'static {
    /// A message from a worker thread, sent with [`Proxy::send`].
    type Message: Send + 'static;

    /// The window exists (hidden, before its first frame): a place to start workers with
    /// [`Cx::proxy`], ask for a backdrop, read the scale. Nothing by default.
    fn start(&mut self, cx: &mut Cx<'_, Self::Message>) {
        let _ = cx;
    }

    /// Something happened. The application updates its models and asks for a redraw of what
    /// changed.
    fn event(&mut self, cx: &mut Cx<'_, Self::Message>, event: Event);

    /// Draws the frame's damage: `damage` (logical px) is the rectangle that must be painted,
    /// every pixel of it (the bitmap holds the last frame there, the background included), and
    /// the painter is clipped to it. Drawing outside it is cut off. A redraw asked for here
    /// comes in the next frame.
    fn draw(&mut self, cx: &mut Cx<'_, Self::Message>, p: &mut Painter, damage: Rect);

    /// A worker's message arrived. Nothing by default.
    fn message(&mut self, cx: &mut Cx<'_, Self::Message>, message: Self::Message) {
        let _ = (cx, message);
    }

    /// The window's accessibility tree, whole (features `host` and `a11y`, Windows). Bounds are
    /// in device px of the client area ([`access_rect`]). By default a window node with the
    /// window's title and nothing in it.
    #[cfg(all(feature = "a11y", windows))]
    fn accessibility(&mut self, cx: &mut Cx<'_, Self::Message>) -> accesskit::TreeUpdate {
        let root = accesskit::NodeId(0);
        let mut node = accesskit::Node::new(accesskit::Role::Window);
        node.set_label(cx.title().to_string());
        let mut info = accesskit::TreeInfo::new(root);
        info.toolkit_name = Some("mdshared-ui".into());
        accesskit::TreeUpdate {
            nodes: vec![(root, node)],
            tree: Some(info),
            tree_id: accesskit::TreeId::ROOT,
            focus: root,
        }
    }
}

/// A rectangle in logical px as AccessKit's bounds: device px of the client area. A scale that
/// is not a positive number counts as 1.
#[cfg(all(feature = "a11y", windows))]
pub fn access_rect(r: Rect, scale: f32) -> accesskit::Rect {
    let s = f64::from(clean_scale(f64::from(scale)));
    accesskit::Rect {
        x0: f64::from(r.x) * s,
        y0: f64::from(r.y) * s,
        x1: f64::from(r.x + r.w) * s,
        y1: f64::from(r.y + r.h) * s,
    }
}

/// How the window is made. Built with [`Config::new`] and the builder methods; every one has a
/// default.
#[derive(Clone, Debug)]
pub struct Config {
    title: String,
    size: (f32, f32),
    min_size: (f32, f32),
    app_id: String,
    icon_png: Option<&'static [u8]>,
    dark: bool,
    corners: Corners,
}

impl Config {
    /// A window titled `title`, 900 x 640 logical px, at least 520 x 360, light title bar,
    /// round corners, the title as its Linux app id.
    pub fn new(title: &str) -> Config {
        Config {
            title: title.to_string(),
            size: (900.0, 640.0),
            min_size: (520.0, 360.0),
            app_id: title.to_string(),
            icon_png: None,
            dark: false,
            corners: Corners::Round,
        }
    }

    /// The size it opens at, logical px. Not a positive number: the default.
    pub fn size(mut self, w: f32, h: f32) -> Config {
        self.size = (positive(w, 900.0), positive(h, 640.0));
        self
    }

    /// The smallest it may be made, logical px. Not a positive number: the default.
    pub fn min_size(mut self, w: f32, h: f32) -> Config {
        self.min_size = (positive(w, 520.0), positive(h, 360.0));
        self
    }

    /// The Linux app id and X11 class ([`window::identify`]); the desktop entry repeats it.
    pub fn app_id(mut self, id: &str) -> Config {
        self.app_id = id.to_string();
        self
    }

    /// A PNG for the X11 window icon ([`window::identify`]); ignored elsewhere.
    pub fn icon_png(mut self, png: &'static [u8]) -> Config {
        self.icon_png = Some(png);
        self
    }

    /// The dark title-bar mode of Windows (the system's own parts: the shadow's edge, the
    /// window's menu). [`Cx::set_dark`] changes it later.
    pub fn dark(mut self, dark: bool) -> Config {
        self.dark = dark;
        self
    }

    /// The window's corners on Windows 11 ([`corners::set_window_corners`](crate::corners::set_window_corners)).
    pub fn corners(mut self, corners: Corners) -> Config {
        self.corners = corners;
        self
    }
}

fn positive(v: f32, default: f32) -> f32 {
    if v.is_finite() && v > 0.0 { v } else { default }
}

// ---- the proxy -------------------------------------------------------------------------------

/// What reaches the loop from outside it.
enum Wake<M> {
    Message(M),
    #[cfg(all(feature = "a11y", windows))]
    Access(accesskit_winit::Event),
}

#[cfg(all(feature = "a11y", windows))]
impl<M> From<accesskit_winit::Event> for Wake<M> {
    fn from(e: accesskit_winit::Event) -> Wake<M> {
        Wake::Access(e)
    }
}

/// Sends messages to the application from any thread: a worker's replies. Clone it freely.
pub struct Proxy<M: 'static>(EventLoopProxy<Wake<M>>);

impl<M: 'static> Clone for Proxy<M> {
    fn clone(&self) -> Proxy<M> {
        Proxy(self.0.clone())
    }
}

impl<M: 'static> Proxy<M> {
    /// Hands `message` to the loop, which passes it to [`App::message`] and wakes if it was
    /// waiting. False when the loop has ended (the window closed): a worker stops then.
    pub fn send(&self, message: M) -> bool {
        self.0.send_event(Wake::Message(message)).is_ok()
    }
}

// ---- the context -----------------------------------------------------------------------------

/// The host's side of the loop that an [`App`] may touch during a call: what it asks for here
/// takes effect when the call returns.
pub struct Cx<'a, M: 'static> {
    window: &'a Window,
    anim: &'a mut Animator,
    proxy: &'a Proxy<M>,
    st: &'a mut Shared,
    now: Instant,
    /// During [`App::draw`]: the frame's device rectangle, and whether the chrome is frosted.
    drawing: Option<((i32, i32, i32, i32), bool)>,
}

impl<M: 'static> Cx<'_, M> {
    /// Asks for `r` (logical px) to be drawn again. Rectangles asked for before the frame are
    /// joined. One that is empty or not made of numbers asks for nothing.
    pub fn request_redraw(&mut self, r: Rect) {
        self.st.damage.add(r);
        self.st.touched = true;
    }

    /// Asks for the whole window to be drawn again.
    pub fn request_redraw_all(&mut self) {
        self.st.damage.all();
        self.st.touched = true;
    }

    /// During [`App::draw`]: `r` holds something in motion, so the animator's next deadline
    /// redraws it. Rectangles are joined; a frame that names none has the whole window redrawn
    /// at the deadline. Outside a draw it does nothing.
    pub fn animating(&mut self, r: Rect) {
        if self.drawing.is_some() {
            self.st.animated.add(r);
        }
    }

    /// The cursor the application wants where the pointer is. The host shows it (and the resize
    /// cursors on the edge band), telling the window only on a change.
    pub fn set_cursor(&mut self, cursor: Cursor) {
        self.st.cursor = cursor;
    }

    /// The window's animator, with a frame begun during [`App::draw`]. Its deadline wakes the
    /// loop by itself; see the [module](self).
    pub fn anim(&mut self) -> &mut Animator {
        self.anim
    }

    /// Asks for [`Event::Wake`] at `at` (the earliest of all asked for wins). It comes once; ask
    /// again for a later one. For a tooltip's delay, a toast's end, a `--seconds` limit.
    pub fn wake_at(&mut self, at: Instant) {
        self.st.deadlines.wake_at(at);
    }

    /// The time the host read for this call (a frame's time during a draw): what a model is
    /// given instead of reading a clock.
    pub fn now(&self) -> Instant {
        self.now
    }

    /// The scale: device px per logical px.
    pub fn scale(&self) -> f32 {
        self.st.scale
    }

    /// The client area, logical px.
    pub fn size(&self) -> (f32, f32) {
        self.st.size
    }

    /// Whether the window has the keyboard.
    pub fn focused(&self) -> bool {
        self.st.focused
    }

    /// Whether the window is maximized.
    pub fn maximized(&self) -> bool {
        self.st.maximized
    }

    /// The modifiers held now.
    pub fn mods(&self) -> Mods {
        self.st.mods
    }

    /// The window's title, as last set.
    pub fn title(&self) -> &str {
        &self.st.title
    }

    /// A proxy that sends [`App::message`]s from any thread.
    pub fn proxy(&self) -> Proxy<M> {
        self.proxy.clone()
    }

    /// The winit window itself, for what the host does not wrap (the IME's cursor area, the
    /// monitor). Do not make it visible or change its decorations.
    pub fn window(&self) -> &Window {
        self.window
    }

    /// Starts moving the window with the pointer: call it from the [`Event::Press`] of the left
    /// button on the title row. Nothing when the system refuses.
    pub fn drag_window(&mut self) {
        let _ = self.window.drag_window();
    }

    /// Does what a double click on the title row does on this system
    /// ([`window::title_double_click`]).
    pub fn title_double_click(&mut self) {
        window::title_double_click(self.window);
    }

    /// Maximizes the window, or restores it when it is maximized (the caption's middle button).
    pub fn toggle_maximize(&mut self) {
        self.window.set_maximized(!self.window.is_maximized());
    }

    /// Minimizes the window.
    pub fn minimize(&mut self) {
        self.window.set_minimized(true);
    }

    /// Ends the loop after this call: the window closes and [`run`] returns.
    pub fn exit(&mut self) {
        self.st.exit = true;
    }

    /// Sets the window's title (the taskbar's, and the default accessibility tree's).
    pub fn set_title(&mut self, title: &str) {
        if self.st.title != title {
            self.st.title = title.to_string();
            self.window.set_title(title);
        }
    }

    /// Where the maximize caption button is drawn (logical px), so Windows 11 shows the snap
    /// layouts over it ([`snap`]); `None` while it is not there to press (a dialog over it).
    /// Sent to the system only on a change, and again after a change of scale.
    pub fn set_maximize_button(&mut self, rect: Option<Rect>) {
        self.st.snap = rect;
    }

    /// The backdrop wanted behind the chrome: Mica, Acrylic or `Off` (the default). `pal` is the
    /// palette the chrome is drawn in: its flat colour is what is taken out of the chrome
    /// ([`frost::key_colour`]). The backdrop is used only while the system shows one and the
    /// window has the keyboard; when it turns on or off the whole window is redrawn.
    pub fn set_backdrop(&mut self, backdrop: Backdrop, pal: &Palette) {
        self.st.backdrop = backdrop;
        self.st.pal = *pal;
    }

    /// Whether the chrome is frosted in the frame being drawn ([`Cx::frost`] marks it then).
    pub fn frosted(&self) -> bool {
        self.drawing.is_some_and(|(_, on)| on)
    }

    /// During [`App::draw`], after drawing a chrome region `r` (logical px) flat and before
    /// drawing anything over it (a menu, a tooltip): lets the backdrop through it
    /// ([`frost::mark`]), within the frame's damage. Nothing while the chrome is not frosted, or
    /// outside a draw.
    pub fn frost(&mut self, p: &mut Painter, r: Rect) {
        let Some((clip, true)) = self.drawing else {
            return;
        };
        let d = p.dev_rect(r);
        let cut = (
            d.0.max(clip.0),
            d.1.max(clip.1),
            d.2.min(clip.2),
            d.3.min(clip.3),
        );
        if cut.0 < cut.2 && cut.1 < cut.3 {
            frost::mark(p.bmp, cut);
        }
    }

    /// The dark title-bar mode of Windows; see [`Config::dark`].
    pub fn set_dark(&mut self, dark: bool) {
        if self.st.dark != dark {
            self.st.dark = dark;
            style(self.window, dark, self.st.corners);
        }
    }

    /// Something in the accessibility tree changed without a redraw: the host asks
    /// [`App::accessibility`] again after this call. Nothing without `a11y`.
    pub fn accessibility_changed(&mut self) {
        self.st.touched = true;
    }
}

// ---- pure parts ------------------------------------------------------------------------------

/// The rectangles asked for before the next frame, joined into the one that holds them all.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Damage {
    rect: Option<Rect>,
    full: bool,
}

impl Damage {
    /// Adds `r`. A rectangle with a side that is not a positive number, or with a corner that
    /// is not a number, adds nothing.
    fn add(&mut self, r: Rect) {
        let finite = r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite();
        if !finite || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        self.rect = Some(match self.rect {
            None => r,
            Some(o) => {
                let (x0, y0) = (o.x.min(r.x), o.y.min(r.y));
                let (x1, y1) = (o.right().max(r.right()), o.bottom().max(r.bottom()));
                Rect::new(x0, y0, x1 - x0, y1 - y0)
            }
        });
    }

    /// The whole window.
    fn all(&mut self) {
        self.full = true;
    }

    fn is_empty(&self) -> bool {
        !self.full && self.rect.is_none()
    }

    /// Joins `other` in.
    fn merge(&mut self, other: Damage) {
        self.full |= other.full;
        if let Some(r) = other.rect {
            self.add(r);
        }
    }

    /// The damage in device px of a `w` x `h` bitmap at `scale`: x0, y0, x1, y1, rounded
    /// outwards and cut to the bitmap. `None` when nothing of it is in the bitmap.
    fn device(&self, scale: f32, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
        if w == 0 || h == 0 {
            return None;
        }
        if self.full {
            return Some((0, 0, w, h));
        }
        let r = self.rect?;
        let s = clean_scale(f64::from(scale));
        // `as` saturates: a rectangle far outside the bitmap is cut to it.
        let x0 = ((r.x * s).floor().max(0.0) as u32).min(w);
        let y0 = ((r.y * s).floor().max(0.0) as u32).min(h);
        let x1 = ((r.right() * s).ceil().max(0.0) as u32).min(w);
        let y1 = ((r.bottom() * s).ceil().max(0.0) as u32).min(h);
        (x0 < x1 && y0 < y1).then_some((x0, y0, x1, y1))
    }
}

/// The two things that wake the loop: the animator's deadline and the application's timer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Deadlines {
    anim: Option<Instant>,
    timer: Option<Instant>,
}

/// Which of the [`Deadlines`] came.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Due {
    anim: bool,
    timer: bool,
}

impl Deadlines {
    /// Asks for the timer at `at`; the earliest asked for wins.
    fn wake_at(&mut self, at: Instant) {
        self.timer = Some(self.timer.map_or(at, |t| t.min(at)));
    }

    /// When the loop must wake next: the earlier of the two. The animator's is left out while
    /// the window is covered (`occluded`), since nothing is drawn then. `None`: wait for an
    /// event.
    fn next(&self, occluded: bool) -> Option<Instant> {
        let anim = self.anim.filter(|_| !occluded);
        match (anim, self.timer) {
            (Some(a), Some(t)) => Some(a.min(t)),
            (a, t) => a.or(t),
        }
    }

    /// Takes the deadlines that have come by `now`. A covered window's animator deadline is
    /// dropped instead: the window is drawn whole when it is seen again.
    fn take_due(&mut self, now: Instant, occluded: bool) -> Due {
        let anim = self.anim.is_some_and(|a| a <= now);
        let timer = self.timer.is_some_and(|t| t <= now);
        if anim || occluded {
            self.anim = None;
        }
        if timer {
            self.timer = None;
        }
        Due {
            anim: anim && !occluded,
            timer,
        }
    }
}

/// Counts the presses of one button that make a double or triple click.
#[derive(Clone, Copy, Debug, Default)]
struct Clicks {
    last: Option<(Instant, (f32, f32), Button)>,
    count: u32,
}

impl Clicks {
    /// A press of `button` at `at` (logical px) at `now`: 1 for a first press, one more for
    /// each that follows within `interval` and [`CLICK_SLOP`] px of the one before.
    fn press(&mut self, now: Instant, at: (f32, f32), button: Button, interval: Duration) -> u32 {
        let again = self.last.is_some_and(|(t, p, b)| {
            b == button
                && now.saturating_duration_since(t) <= interval
                && (at.0 - p.0).abs() <= CLICK_SLOP
                && (at.1 - p.1).abs() <= CLICK_SLOP
        });
        self.count = if again {
            self.count.saturating_add(1)
        } else {
            1
        };
        self.last = Some((now, at, button));
        self.count
    }
}

/// A scale that is a positive number, else 1.
fn clean_scale(s: f64) -> f32 {
    if s.is_finite() && s > 0.0 {
        s as f32
    } else {
        1.0
    }
}

/// A device position as logical px. A position that is not a number is 0.
fn logical(x: f64, y: f64, scale: f32) -> (f32, f32) {
    let s = f64::from(clean_scale(f64::from(scale)));
    let c = |v: f64| if v.is_finite() { (v / s) as f32 } else { 0.0 };
    (c(x), c(y))
}

/// winit's button as the toolkit's, `None` for the others.
fn button(b: MouseButton) -> Option<Button> {
    Some(match b {
        MouseButton::Left => Button::Left,
        MouseButton::Right => Button::Right,
        MouseButton::Middle => Button::Middle,
        MouseButton::Back => Button::Back,
        MouseButton::Forward => Button::Forward,
        MouseButton::Other(_) => return None,
    })
}

/// A wheel's movement as [`Event::Wheel`]: notches times [`WHEEL_LINE`], or a touchpad's device
/// px as logical px; Shift turns a vertical wheel sideways.
fn wheel(delta: MouseScrollDelta, scale: f32, shift: bool) -> Event {
    let (x, y, precise) = match delta {
        MouseScrollDelta::LineDelta(x, y) => (
            f64::from(x) * f64::from(WHEEL_LINE),
            f64::from(y) * f64::from(WHEEL_LINE),
            false,
        ),
        MouseScrollDelta::PixelDelta(p) => {
            let (x, y) = logical(p.x, p.y, scale);
            (f64::from(x), f64::from(y), true)
        }
    };
    let (dx, dy) = crate::viewport::wheel_delta(x, y, shift);
    Event::Wheel {
        dx: dx as f32,
        dy: dy as f32,
        precise,
    }
}

/// Copies the device rectangle `r` (x0, y0, x1, y1, inside both) of `src`, BGRA bytes in rows of
/// `stride` pixels, into `dst`, pixels of `0xAARRGGBB` in rows of the same stride.
fn copy_rect(src: &[u8], dst: &mut [u32], stride: usize, r: (usize, usize, usize, usize)) {
    let (x0, y0, x1, y1) = r;
    let rows = (dst.len() / stride.max(1)).min(src.len() / (stride * 4).max(1));
    let (x1, y1) = (x1.min(stride), y1.min(rows));
    if x0 >= x1 {
        return;
    }
    for y in y0..y1 {
        let from = &src[(y * stride + x0) * 4..(y * stride + x1) * 4];
        let to = &mut dst[y * stride + x0..y * stride + x1];
        for (d, px) in to.iter_mut().zip(from.as_chunks::<4>().0) {
            *d = u32::from_le_bytes(*px);
        }
    }
}

// ---- the loop --------------------------------------------------------------------------------

/// The host's state that a [`Cx`] reads and writes.
struct Shared {
    title: String,
    scale: f32,
    size: (f32, f32),
    focused: bool,
    maximized: bool,
    occluded: bool,
    mods: Mods,
    dark: bool,
    corners: Corners,
    damage: Damage,
    /// What the last frame said was in motion.
    animated: Damage,
    deadlines: Deadlines,
    cursor: Cursor,
    snap: Option<Rect>,
    backdrop: Backdrop,
    pal: Palette,
    exit: bool,
    /// A call asked for a redraw or said the accessibility tree changed.
    touched: bool,
}

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
        occluded: false,
        mods: Mods::default(),
        dark: config.dark,
        corners: config.corners,
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
fn style(w: &Window, dark: bool, corners: Corners) {
    #[cfg(windows)]
    if let Some(h) = hwnd(w) {
        platform::style_window(h, dark);
        crate::corners::set_window_corners(h, corners);
    }
    #[cfg(not(windows))]
    let _ = (w, dark, corners);
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

    #[cfg(not(all(feature = "a11y", windows)))]
    fn refresh_tree(&mut self) {}

    /// The window's size and scale as the window says them now.
    fn read_window(&mut self) {
        let Some(win) = self.win.as_ref() else {
            return;
        };
        let w = &win.window;
        self.st.scale = clean_scale(w.scale_factor());
        let s = w.inner_size();
        self.st.size = logical(f64::from(s.width), f64::from(s.height), self.st.scale);
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

    /// Brings the backdrop in line with the application's wish and the focus. True when the
    /// chrome turned frosted or flat: everything is drawn again.
    fn sync_frost(&mut self) -> bool {
        let want = self.st.backdrop;
        if !self.frost.concerns(want) {
            return false;
        }
        let Some(win) = self.win.as_mut() else {
            return false;
        };
        // TODO(wk/hc): turn the backdrop off by `tokens::allowed_backdrop` once branch wk/hc is
        // on this base; until then `platform::backdrop_available` refuses it under high
        // contrast on Windows, which `Frost::sync` asks.
        #[cfg(windows)]
        let mut sys = frost::WindowSystem(hwnd(&win.window));
        #[cfg(not(windows))]
        let mut sys = frost::BlurSystem(&win.window);
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

    /// Draws the damage and presents it.
    fn frame(&mut self, now: Instant) {
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
        let Some(dev) = self.st.damage.device(scale, w, h) else {
            self.st.damage = Damage::default();
            return;
        };
        self.st.damage = Damage::default();
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
        present(win, dev, self.frost.key());
        // A redraw asked for while drawing belongs to the next frame; the tree is not asked
        // again for it.
        self.st.touched = false;
        self.sync_window();
    }
}

/// Copies the device rectangle `dev` of the bitmap to the window and presents that rectangle:
/// all of it when the window's buffer does not hold the last frame (its age is not 1).
fn present(win: &mut Win, dev: (u32, u32, u32, u32), key: Option<&frost::Key>) {
    let (w, h) = (win.bmp.width, win.bmp.height);
    let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else {
        return;
    };
    if win.surface.resize(nw, nh).is_err() {
        return;
    }
    let Ok(mut buffer) = win.surface.buffer_mut() else {
        return;
    };
    let r = if buffer.age() == 1 { dev } else { (0, 0, w, h) };
    let r = (r.0 as usize, r.1 as usize, r.2 as usize, r.3 as usize);
    copy_rect(&win.bmp.pixels, &mut buffer, w as usize, r);
    if let Some(key) = key {
        frost::key_out(key, &mut buffer, w as usize, r);
    }
    let (Some(rw), Some(rh)) = (
        NonZeroU32::new((r.2 - r.0) as u32),
        NonZeroU32::new((r.3 - r.1) as u32),
    ) else {
        return;
    };
    let rect = softbuffer::Rect {
        x: r.0 as u32,
        y: r.1 as u32,
        width: rw,
        height: rh,
    };
    let _ = buffer.present_with_damage(&[rect]);
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
        self.st.focused = window.has_focus();
        self.anim.set_system_reduced(platform::motion_reduced());
        let now = Instant::now();
        self.call(now, |app, cx| app.start(cx));
        self.refresh_tree();
        // The first frame, to the hidden window; then shown, and presented again.
        self.st.damage.all();
        self.frame(now);
        window.set_visible(true);
        if let Some(win) = self.win.as_mut() {
            let (w, h) = (win.bmp.width, win.bmp.height);
            present(win, (0, 0, w, h), self.frost.key());
        }
        if self.st.exit {
            el.exit();
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, wake: Wake<A::Message>) {
        let now = Instant::now();
        match wake {
            Wake::Message(m) => self.call(now, |app, cx| app.message(cx, m)),
            #[cfg(all(feature = "a11y", windows))]
            Wake::Access(e) => match e.window_event {
                accesskit_winit::WindowEvent::InitialTreeRequested => self.refresh_tree(),
                accesskit_winit::WindowEvent::ActionRequested(req) => {
                    self.call(now, |app, cx| app.event(cx, Event::Action(req)));
                }
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
        let window = win.window.clone();
        let now = Instant::now();
        let scale = self.st.scale;
        let ev = match event {
            WindowEvent::CloseRequested => Some(Event::CloseRequested),
            WindowEvent::RedrawRequested => {
                // One the loop did not ask for is the system's: the window needs all its
                // pixels again, from the bitmap (mdview's rule), after drawing any damage.
                let expose = !std::mem::take(&mut self.requested);
                let s = window.inner_size();
                if expose && (win.bmp.width, win.bmp.height) != (s.width, s.height) {
                    self.st.damage.all();
                }
                self.frame(now);
                if expose && let Some(win) = self.win.as_mut() {
                    let (w, h) = (win.bmp.width, win.bmp.height);
                    present(win, (0, 0, w, h), self.frost.key());
                }
                None
            }
            WindowEvent::Resized(_) => {
                self.read_window();
                self.st.damage.all();
                // Bounds may have moved with the layout: the tree is asked again.
                self.st.touched = true;
                let (w, h) = self.st.size;
                Some(Event::Resize { w, h })
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                self.read_window();
                self.st.damage.all();
                // The snap rectangle is sent in device px: again at the new scale; and the
                // accessibility tree's bounds are device px too.
                self.snap_sent = None;
                self.st.touched = true;
                Some(Event::Scale(self.st.scale))
            }
            WindowEvent::ThemeChanged(t) => Some(Event::Theme {
                dark: t == winit::window::Theme::Dark,
            }),
            WindowEvent::Occluded(occluded) => {
                self.st.occluded = occluded;
                if !occluded {
                    self.st.damage.all();
                }
                None
            }
            WindowEvent::Focused(focused) => {
                self.st.focused = focused;
                if focused {
                    // The one place the system's wish for less motion is read again.
                    self.anim.set_system_reduced(platform::motion_reduced());
                }
                if self.st.backdrop != Backdrop::Off {
                    self.st.damage.all();
                }
                if !focused {
                    self.cursor.forget();
                }
                Some(Event::Focus(focused))
            }
            WindowEvent::ModifiersChanged(m) => {
                self.st.mods = Mods::from_winit(m.state());
                Some(Event::Modifiers(self.st.mods))
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = logical(position.x, position.y, scale);
                let (x, y) = self.pointer;
                Some(Event::PointerMove { x, y })
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor.forget();
                Some(Event::PointerLeave)
            }
            WindowEvent::MouseInput {
                state, button: b, ..
            } => {
                let Some(b) = button(b) else {
                    return;
                };
                let (x, y) = self.pointer;
                match state {
                    ElementState::Pressed => {
                        if b == Button::Left
                            && let Some(dir) = self.edge(self.pointer)
                        {
                            let _ = window.drag_resize_window(dir);
                            return;
                        }
                        let clicks = self.clicks.press(now, (x, y), b, self.double_click);
                        Some(Event::Press {
                            x,
                            y,
                            button: b,
                            clicks,
                        })
                    }
                    ElementState::Released => Some(Event::Release { x, y, button: b }),
                }
            }
            WindowEvent::MouseWheel { delta, .. } => Some(wheel(delta, scale, self.st.mods.shift)),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } if event.state.is_pressed() => Some(Event::Key(KeyPress {
                name: crate::keys::key_name(&event.logical_key),
                mods: self.st.mods,
                text: event.text.as_ref().map(|t| t.to_string()),
                repeat: event.repeat,
            })),
            WindowEvent::Ime(winit::event::Ime::Commit(text)) => Some(Event::Text(text)),
            WindowEvent::Ime(winit::event::Ime::Preedit(text, cursor)) => {
                Some(Event::Preedit(Preedit::new(text, cursor)))
            }
            WindowEvent::Ime(winit::event::Ime::Enabled | winit::event::Ime::Disabled) => {
                Some(Event::Preedit(None))
            }
            _ => None,
        };
        if let Some(ev) = ev {
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
        let due = self.st.deadlines.take_due(now, self.st.occluded);
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
            && !self.st.occluded
            && !self.requested
            && let Some(win) = self.win.as_ref()
        {
            self.requested = true;
            win.window.request_redraw();
        }
        el.set_control_flow(match self.st.deadlines.next(self.st.occluded) {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        });
    }
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
