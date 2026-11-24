//! The window host: the winit loop, the softbuffer present and the timers of ONE window, so
//! that an application writes its model, its drawing and its reactions, and not the loop that
//! every application here has rewritten (roadmap W1, at Purge's scope). Behind the feature
//! `host`; with `a11y` too, on Windows, the host also serves the application's AccessKit tree to
//! UI Automation.
//!
//! **Who owns what.** The application implements [`App`] and keeps its models, its hits and its
//! state; [`run`] owns winit's `ApplicationHandler`, the softbuffer surface, the bitmap the frame
//! is drawn into, the [`Gfx`](crate::gfx::Gfx) (fonts and glyph caches) and the window's one
//! [`Animator`](crate::anim::Animator). Nothing is called back into a widget: the host hands the
//! application plain [`Event`]s, and the application tells the host what it wants through the
//! [`Cx`] it is given with each one (a redraw of a rectangle, a cursor, a wake-up, the window
//! dragged).
//!
//! **Events.** winit's window events are reduced to the toolkit's own, in logical px: the
//! pointer moving, a press (with its click count, for a double click) and a release, the wheel
//! (a notch is [`WHEEL_LINE`] px, Shift turns it sideways by
//! [`viewport::wheel_delta`](crate::viewport::wheel_delta)), a key press as a
//! [`keys::KeyName`](crate::keys::KeyName) with the modifiers and the text it typed, the text
//! and composition of an input method, the focus, the scale, the size, the system's theme and
//! contrast, and the close request. A worker's reply arrives through [`App::message`], sent from
//! any thread with the [`Proxy`] that [`Cx::proxy`] gives.
//!
//! **Frames.** The application asks for a redraw of a rectangle ([`Cx::request_redraw`]); the
//! rectangles asked for before a frame are joined into the one rectangle that holds them all,
//! the frame draws that rectangle only ([`App::draw`] is given it, and the painter is clipped to
//! it), and only that rectangle is sent to the window (softbuffer's `present_with_damage`). The
//! bitmap keeps the rest of the last frame. A resize, a change of scale, a change of the backdrop
//! or of its palette, and the window coming back from being covered or minimized repaint
//! everything. A window that is covered (winit's `Occluded`) or minimized (a size of zero, or
//! `is_minimized`; winit 0.30 never says `Occluded` on Windows) is not drawn until it is seen
//! again.
//!
//! **Every animated value on every frame.** A frame that draws only its damage still ends the
//! animator's frame, and [`Animator::end_frame`](crate::anim::Animator::end_frame) forgets every
//! name the frame did not ask for: a value that is not asked for in a frame starts afresh the
//! next time, and a blink stops. So [`App::draw`] must ask for every animated value on every
//! frame, whatever the damage (the drawing outside the damage is clipped and costs little), or
//! name the others with [`Animator::keep`](crate::anim::Animator::keep). The host does not skip
//! ending a partial frame instead: the deadline of the next frame comes from `end_frame` alone,
//! and the commonest partial frame (a caret's blink, which draws the field only) needs it.
//!
//! **One wake-up.** The animator's deadline (`end_frame`), which already holds a caret's blink
//! ([`TextField::blink_caret`](crate::textfield::TextField::blink_caret)) and anything else in
//! motion, and the application's own timers ([`Cx::wake_at`], a tooltip's delay) are folded into
//! one `ControlFlow::WaitUntil`; with neither, the loop waits for an event
//! (`ControlFlow::Wait`) and an idle window does no work. When the animator's deadline comes,
//! the host redraws what the last frame said was moving ([`Cx::animating`]; the whole window if
//! it said nothing). When a timer comes, the application gets [`Event::Wake`] and decides.
//!
//! **The window.** Undecorated, with the size and the minimum size of the [`Config`], the
//! system's shadow on Windows, and the dark title-bar mode and corners of the config. A press
//! on the 8 px band at the edge of a restored window resizes it
//! ([`window::edge_at`](crate::window::edge_at)) and shows the resize cursor there; the title row
//! is the application's, which drags the window by [`Cx::drag_window`] and gets snap layouts by
//! [`Cx::set_maximize_button`].
//!
//! **The backdrop** (Windows only). Mica or Acrylic ([`Cx::set_backdrop`]) is asked for through
//! [`frost::Frost`](crate::frost::Frost): never under high contrast
//! ([`tokens::allowed_backdrop`](crate::tokens::allowed_backdrop), with the contrast the host
//! watches), and refused when the system shows none (transparency off, battery saver). The chrome
//! the application marks with [`Cx::frost`] lets it through. Off Windows there is no backdrop:
//! the frame is presented without alpha (stock softbuffer presents Xrgb8888), so keyed-out
//! chrome would show black; the host reports none ([`Cx::frosted`] is false) until it can
//! present with alpha.
//!
//! **The first frame.** The window is created hidden, the first frame is drawn and presented to
//! it, then it is shown and the frame presented again (a frame presented to a hidden window
//! shows nothing): mdview's rules, which use no DWM cloak. So it never shows white. The first
//! frame is drawn as focused, since a window winit shows is activated (`with_active`, true by
//! default); if it turns out not to have the keyboard once shown, it is drawn again unfocused.
//!
//! **Accessibility** (features `host` and `a11y`). The application gives its tree by
//! [`App::accessibility`] on every system. On Windows the AccessKit adapter is made with
//! `with_mixed_handlers` before the window is shown; the host asks for the tree before the
//! first frame, after every call that asked for a redraw or said [`Cx::accessibility_changed`],
//! and after a resize or a change of scale; keeps the latest for the system's first request
//! (served at once, on the thread that asks); and hands it to the adapter when an assistive
//! technology is listening. A request from one (Invoke, Toggle, Focus, ScrollIntoView) is
//! [`Event::Action`]. Elsewhere there is no adapter yet and the tree is not asked for.
//!
//! Deliberately not here: tabs and several windows (Purge's scope: one window); the
//! client-drawn shadow of Linux ([`shadow`](crate::shadow)) and the rounded corners it cuts,
//! which mdview wires itself; a backdrop off Windows (above); the window's place and size kept
//! between runs ([`window_state`](crate::window_state) is the application's to call); a menu
//! bar, a tray, a file drop, touch, pinch and pen; key releases; regions of the window with
//! their own damage (the damage is one rectangle); a GPU. Tested: everything pure (the damage,
//! the deadlines, the covered and minimized window, the click count, the reduction of events) in
//! `host_tests.rs`; the window parts by the example
//! `cargo run -p mdshared-ui --features host --example host`, compiled and run headless for a
//! few seconds on Windows, not seen by a person; Linux and macOS only compiled.
//!
//! The parts: `cx` (what the application may ask), `input` (winit's input reduced), `frame`
//! (the damage, the deadlines, the present) and `run` (the loop).

use std::time::Instant;

use winit::event_loop::EventLoopProxy;

use crate::corners::Corners;
use crate::gfx::{Painter, Rect};
use crate::keys::{KeyName, Mods};
use crate::textfield::Preedit;

mod cx;
mod frame;
mod input;
mod run;

pub use cx::Cx;
pub use run::run;

/// One notch of a mouse wheel, in logical px: what a wheel that reports lines (winit's
/// `LineDelta`) moves per line. mdview counts a notch as this much too.
pub const WHEEL_LINE: f32 = 48.0;

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
    /// A button went down at `(x, y)`. `clicks` is 1, then 2 for a double click and 3 for a
    /// triple, counted while the presses of one button come within the system's double-click
    /// time and its double-click rectangle (`SM_CXDOUBLECLK` by `SM_CYDOUBLECLK` device px on
    /// Windows, centred on the press before). It stops at 3: a fourth press in the same run is
    /// 3 again. Not sent for a press on the resize band of a restored window, which resizes it.
    Press {
        x: f32,
        y: f32,
        button: Button,
        clicks: u32,
    },
    /// A button went up at `(x, y)`.
    Release { x: f32, y: f32, button: Button },
    /// The wheel or a touchpad moved the view: `dx` across, `dy` down, logical px, positive when
    /// the content should move right or down (winit's sign: a wheel turned away from the user,
    /// to scroll up, gives a positive `dy`). A vertical wheel with Shift held arrives as `dx`.
    /// `precise` is true for a touchpad's pixel deltas and false for a wheel's notches
    /// ([`WHEEL_LINE`] each).
    Wheel { dx: f32, dy: f32, precise: bool },
    /// A key went down.
    Key(KeyPress),
    /// Text an input method committed (an accent typed with a dead key, Chinese or Japanese
    /// input). Input methods are on only on macOS ([`window::created`](crate::window::created)).
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
    /// High contrast was switched on (`true`) or off: the application makes its tokens again
    /// ([`tokens`](crate::tokens)). The host has already turned the backdrop off or on by it
    /// and redraws everything. Windows only: there the host watches the contrast itself
    /// ([`platform::watch_contrast`](crate::platform::watch_contrast), whose one wake it takes,
    /// so an application of the host does not call it) and reads it again on a change of theme
    /// and when the window gains the keyboard. Elsewhere it is never sent: the portal read can
    /// wait half a second, so the application reads
    /// [`platform::system_high_contrast`](crate::platform::system_high_contrast) itself, off the
    /// loop's thread.
    Contrast(bool),
    /// The time asked for by [`Cx::wake_at`] came; the instant is now. Ask again for a later one.
    Wake(Instant),
    /// The window's close button of the system, Alt+F4 or the taskbar asked to close. The
    /// window stays open unless the application calls [`Cx::exit`].
    CloseRequested,
    /// An assistive technology asked for an action (features `host` and `a11y`; only Windows
    /// has an adapter that sends any).
    #[cfg(feature = "a11y")]
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
    ///
    /// **Ask for every animated value on every frame, whatever the damage** (or name it with
    /// [`Animator::keep`](crate::anim::Animator::keep)): the animator's frame ends after every
    /// draw, partial ones included, and forgets the names a frame did not ask for. A value not
    /// asked for in a frame that drew only a caret starts afresh, and a blink that is not asked
    /// for stops. The simplest way is to draw the whole window every time and let the clip cut
    /// it to the damage.
    fn draw(&mut self, cx: &mut Cx<'_, Self::Message>, p: &mut Painter, damage: Rect);

    /// A worker's message arrived. Nothing by default.
    fn message(&mut self, cx: &mut Cx<'_, Self::Message>, message: Self::Message) {
        let _ = (cx, message);
    }

    /// The window's accessibility tree, whole (feature `a11y`). Bounds are in device px of the
    /// client area ([`access_rect`]). By default a window node with the window's title and
    /// nothing in it. Asked for on Windows only, where the adapter serves it.
    #[cfg(feature = "a11y")]
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
#[cfg(feature = "a11y")]
pub fn access_rect(r: Rect, scale: f32) -> accesskit::Rect {
    let s = f64::from(input::clean_scale(f64::from(scale)));
    accesskit::Rect {
        x0: f64::from(r.x) * s,
        y0: f64::from(r.y) * s,
        x1: f64::from(r.x + r.w) * s,
        y1: f64::from(r.y + r.h) * s,
    }
}

// ---- the config ------------------------------------------------------------------------------

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

    /// The Linux app id and X11 class ([`window::identify`](crate::window::identify)); the
    /// desktop entry repeats it.
    pub fn app_id(mut self, id: &str) -> Config {
        self.app_id = id.to_string();
        self
    }

    /// A PNG for the X11 window icon ([`window::identify`](crate::window::identify)); ignored
    /// elsewhere.
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

    /// The window's corners on Windows 11
    /// ([`corners::set_window_corners`](crate::corners::set_window_corners)).
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
    /// The contrast watch saw high contrast switch (Windows).
    Contrast,
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
    ///
    /// After [`Cx::exit`] the loop is winding down: a message sent then may still answer true,
    /// but it is dropped, not delivered. A worker that must know its last message arrived has
    /// the application say so before it calls `exit`.
    pub fn send(&self, message: M) -> bool {
        self.0.send_event(Wake::Message(message)).is_ok()
    }
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
