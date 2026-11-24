//! What an application may ask of the host during a call: the [`Cx`], and the state it shares
//! with the loop.

use std::time::Instant;

use mdshared_app::backdrop::Backdrop;
use winit::window::Window;

use super::Proxy;
use super::frame::{Damage, Deadlines, Visibility};
use crate::anim::Animator;
use crate::corners::Corners;
use crate::cursor::Cursor;
use crate::frost;
use crate::gfx::{Painter, Rect};
use crate::keys::Mods;
use crate::theme::Palette;
use crate::window;

/// The host's state that a [`Cx`] reads and writes.
pub(super) struct Shared {
    pub(super) title: String,
    pub(super) scale: f32,
    pub(super) size: (f32, f32),
    pub(super) focused: bool,
    pub(super) maximized: bool,
    pub(super) vis: Visibility,
    pub(super) mods: Mods,
    pub(super) dark: bool,
    pub(super) corners: Corners,
    pub(super) high_contrast: bool,
    pub(super) damage: Damage,
    /// What the last frame said was in motion.
    pub(super) animated: Damage,
    pub(super) deadlines: Deadlines,
    pub(super) cursor: Cursor,
    pub(super) snap: Option<Rect>,
    pub(super) backdrop: Backdrop,
    pub(super) pal: Palette,
    pub(super) exit: bool,
    /// A call asked for a redraw or said the accessibility tree changed.
    pub(super) touched: bool,
}

/// The host's side of the loop that an [`App`](super::App) may touch during a call: what it asks
/// for here takes effect when the call returns.
pub struct Cx<'a, M: 'static> {
    pub(super) window: &'a Window,
    pub(super) anim: &'a mut Animator,
    pub(super) proxy: &'a Proxy<M>,
    pub(super) st: &'a mut Shared,
    pub(super) now: Instant,
    /// During [`App::draw`](super::App::draw): the frame's device rectangle, and whether the
    /// chrome is frosted.
    pub(super) drawing: Option<((i32, i32, i32, i32), bool)>,
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

    /// During [`App::draw`](super::App::draw): `r` holds something in motion, so the animator's
    /// next deadline redraws it. Rectangles are joined; a frame that names none has the whole
    /// window redrawn at the deadline. Outside a draw it does nothing.
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

    /// The window's animator. Use it during [`App::draw`](super::App::draw), where the host has
    /// begun its frame; its deadline then wakes the loop by itself (see the
    /// [module](super)). Outside a draw no frame is begun: a value given a new target there is
    /// seen at the next frame, and an [`Animator::wake_at`] asked there wakes nothing until some
    /// frame is drawn, since the host reads the animator's deadline only when a frame ends. For
    /// a timer asked from an event, use [`Cx::wake_at`].
    pub fn anim(&mut self) -> &mut Animator {
        self.anim
    }

    /// Asks for [`Event::Wake`](super::Event::Wake) at `at` (the earliest of all asked for
    /// wins), whether or not anything is drawn. It comes once; ask again for a later one. For a
    /// tooltip's delay, a toast's end, a `--seconds` limit.
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

    /// Whether the window has the keyboard. True for the first frame, which is drawn before the
    /// window is shown and as the system will show it (see the [module](super)).
    pub fn focused(&self) -> bool {
        self.st.focused
    }

    /// Whether the window is maximized.
    pub fn maximized(&self) -> bool {
        self.st.maximized
    }

    /// Whether high contrast is on, as the host last read it
    /// ([`Event::Contrast`](super::Event::Contrast)). Always false off Windows, where the host
    /// does not read it.
    pub fn high_contrast(&self) -> bool {
        self.st.high_contrast
    }

    /// The modifiers held now.
    pub fn mods(&self) -> Mods {
        self.st.mods
    }

    /// The window's title, as last set.
    pub fn title(&self) -> &str {
        &self.st.title
    }

    /// A proxy that sends [`App::message`](super::App::message)s from any thread.
    pub fn proxy(&self) -> Proxy<M> {
        self.proxy.clone()
    }

    /// The winit window itself, for what the host does not wrap (the IME's cursor area, the
    /// monitor). Do not make it visible or change its decorations.
    pub fn window(&self) -> &Window {
        self.window
    }

    /// Starts moving the window with the pointer: call it from the
    /// [`Event::Press`](super::Event::Press) of the left button on the title row. Nothing when
    /// the system refuses.
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

    /// Ends the loop after this call: the window closes and [`run`](super::run) returns. A
    /// message a worker sends after this is dropped ([`Proxy::send`]).
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
    /// layouts over it ([`snap`](crate::snap)); `None` while it is not there to press (a dialog
    /// over it). Sent to the system only on a change, and again after a change of scale.
    pub fn set_maximize_button(&mut self, rect: Option<Rect>) {
        self.st.snap = rect;
    }

    /// The backdrop wanted behind the chrome: Mica, Acrylic or `Off` (the default). `pal` is the
    /// palette the chrome is drawn in: its flat colour is what is taken out of the chrome
    /// ([`frost::key_colour`]). A change of either redraws the whole window. The backdrop is used
    /// only on Windows, never under high contrast, only while the system shows one, and only
    /// while the window has the keyboard; off Windows nothing is shown (see the
    /// [module](super)).
    pub fn set_backdrop(&mut self, backdrop: Backdrop, pal: &Palette) {
        if self.st.backdrop != backdrop || self.st.pal != *pal {
            self.st.backdrop = backdrop;
            self.st.pal = *pal;
            self.st.damage.all();
        }
    }

    /// Whether the chrome is frosted in the frame being drawn ([`Cx::frost`] marks it then).
    /// Always false off Windows and outside a draw.
    pub fn frosted(&self) -> bool {
        self.drawing.is_some_and(|(_, on)| on)
    }

    /// During [`App::draw`](super::App::draw), after drawing a chrome region `r` (logical px)
    /// flat and before drawing anything over it (a menu, a tooltip): lets the backdrop through
    /// it ([`frost::mark`]), within the frame's damage. Nothing while the chrome is not
    /// frosted, or outside a draw.
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

    /// The dark title-bar mode of Windows; see [`Config::dark`](super::Config::dark).
    pub fn set_dark(&mut self, dark: bool) {
        if self.st.dark != dark {
            self.st.dark = dark;
            super::run::style(self.window, dark, self.st.corners);
        }
    }

    /// Something in the accessibility tree changed without a redraw: the host asks
    /// [`App::accessibility`](super::App) again after this call. Nothing without `a11y`.
    pub fn accessibility_changed(&mut self) {
        self.st.touched = true;
    }
}
