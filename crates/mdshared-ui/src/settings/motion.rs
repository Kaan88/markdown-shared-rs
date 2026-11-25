//! The settings dialog in motion (roadmap L10): [`Form::draw_animated`], the opt-in beside
//! [`Form::draw`]. A switch's knob slides and its fill fades in; the chosen segment of a
//! segmented choice slides to the one chosen; a row's hover fill, a category's and the open
//! dropdown's hover row fade in and out; the scrollbar widens and its track fades in under
//! the pointer; the thumb of a number row shown as a slider ([`Form::show_as_slider`])
//! glides to a value set by a key or a click. Every motion
//! is the toolkit's own for that widget (`toggle::draw_toggle_animated`,
//! `scroll::draw_scrollbar_animated`, `slider::draw_animated`, `Menu::draw_in_animated`), and
//! the fades are [`document::HOVER`].
//!
//! The caller owns the [`Animator`] and the frame (`begin_frame` before the draw, `end_frame`
//! after it for the next deadline), and names the form in it with `id`. The form keeps no
//! clock and no motion of its own: every value in flight lives in the animator, under names
//! made from `id`, the row's index and the row's id, so a row replaced by another starts at
//! rest.
//!
//! At rest, and on every frame of an animator that does not animate ([`Policy::Never`], or the
//! system asking for less motion), `draw_animated` draws exactly what `draw` draws, pixel for
//! pixel; the tests in `motion_tests.rs` hold it to that. Under high contrast
//! ([`Form::set_high_contrast`]) the highlights (the hover fills, the scrollbar, the menu's
//! row, the chosen segment) change at once, as the system's own controls do there; a switch's
//! knob and a slider's thumb still move.
//!
//! Deliberately not here:
//! - the dialog's entrance and exit (the caller fades the whole frame with `anim::pop` or
//!   draws it under `overlay::draw_backdrop_presence`, as for any dialog);
//! - the stepper's and the Close button's hover, which change at once as they do in `draw`;
//! - a check box: the form draws none (the dropdown's menu marks its chosen option, as the
//!   menu draws it);
//! - smooth scrolling of the body: the wheel moves it as in `draw`.
//!
//! [`Policy::Never`]: crate::anim::Policy::Never

use super::Form;
use crate::anim::{Animator, AsId, Id, document, fluent};
use crate::gfx::{Painter, Rect};
use crate::menu::Menu;
use crate::scroll;
use crate::slider::{self, Slider};
use crate::theme::Palette as Colors;

/// What [`Form::draw_animated`] hands to the drawing: the animator, the form's name in it, and
/// whether highlights change at once.
pub(super) struct Fx<'a> {
    anim: &'a mut Animator,
    id: Id,
    high_contrast: bool,
}

impl<'a> Fx<'a> {
    /// The motion of a form named `id` in `anim`; highlights change at once under
    /// `high_contrast`. The paged form (`settings::pages`) draws with this too.
    pub(super) fn new(anim: &'a mut Animator, id: Id, high_contrast: bool) -> Fx<'a> {
        Fx {
            anim,
            id,
            high_contrast,
        }
    }

    /// The same motion with every name under `part` as well: the paged form names the values
    /// of a page's rows under the page, so a row of another page at the same index and with
    /// the same id starts at rest.
    pub(super) fn under(&mut self, part: impl std::hash::Hash) -> Fx<'_> {
        Fx {
            anim: &mut *self.anim,
            id: self.id.with(part),
            high_contrast: self.high_contrast,
        }
    }
}

impl Fx<'_> {
    /// How far row `i`'s hover fill has come in, 0 to 1.
    pub(super) fn row(&mut self, i: usize, row_id: Option<u32>, hovered: bool) -> f32 {
        if self.high_contrast {
            return f32::from(u8::from(hovered));
        }
        let name = self.id.with(("row", i, row_id));
        self.anim.toggle(name, hovered, document::HOVER)
    }

    /// How far category `c`'s hover fill has come in, 0 to 1.
    pub(super) fn category(&mut self, c: usize, hovered: bool) -> f32 {
        if self.high_contrast {
            return f32::from(u8::from(hovered));
        }
        let name = self.id.with(("category", c));
        self.anim.toggle(name, hovered, document::HOVER)
    }

    /// The body's scrollbar.
    pub(super) fn scrollbar(
        &mut self,
        p: &mut Painter,
        pal: &Colors,
        track: Rect,
        thumb: Rect,
        active: bool,
    ) {
        if self.high_contrast {
            scroll::draw_scrollbar(p, pal, track, thumb, active);
        } else {
            let name = self.id.with("scrollbar");
            scroll::draw_scrollbar_animated(p, pal, self.anim, name, track, thumb, active);
        }
    }

    /// The open dropdown's menu, `row` the row it belongs to.
    pub(super) fn menu(
        &mut self,
        p: &mut Painter,
        pal: &Colors,
        window: Rect,
        row: usize,
        menu: &mut Menu<usize>,
    ) {
        if self.high_contrast {
            menu.draw_in(p, pal, window);
        } else {
            menu.draw_in_animated(p, pal, window, self.anim, self.id.with(("menu", row)));
        }
    }

    /// A toggle row's switch in `r`: the knob slides by [`fluent::MOVE`], as
    /// `toggle::draw_toggle_animated` slides it. `row` is the row's index and id.
    #[allow(clippy::too_many_arguments)] // drawing call: a rectangle plus the switch's looks
    pub(super) fn toggle(
        &mut self,
        p: &mut Painter,
        pal: &Colors,
        row: (usize, u32),
        r: Rect,
        on: bool,
        hovered: bool,
        pressed: bool,
    ) {
        let t = self
            .anim
            .toggle(self.id.with(("toggle", row.0, row.1)), on, fluent::MOVE);
        if t <= 0.0 || t >= 1.0 {
            crate::toggle::switch(p, pal, r, t >= 1.0, hovered, pressed);
        } else {
            crate::toggle::switch_at(p, pal, r, t, hovered, pressed);
        }
    }

    /// Where the chosen segment's fill is on its way to `to` (the chosen segment of the
    /// control `ctrl`), or `None` when it is there. Held as the place inside the control, so a
    /// scroll moves it with the row and is not a motion.
    pub(super) fn pill(
        &mut self,
        i: usize,
        row_id: Option<u32>,
        ctrl: Rect,
        to: Rect,
    ) -> Option<Rect> {
        if self.high_contrast {
            return None;
        }
        let target = (to.x - ctrl.x, to.w);
        let name = self.id.with(("pill", i, row_id));
        let (x, w) = self.anim.value(name, target, fluent::MOVE);
        (x, w)
            .ne(&target)
            .then(|| Rect::new(ctrl.x + x, to.y, w, to.h))
    }

    /// A slider row's slider in `r`, by `slider::draw_animated`. `row` is the row's index and
    /// id.
    pub(super) fn slider(
        &mut self,
        p: &mut Painter,
        pal: &Colors,
        row: (usize, u32),
        r: Rect,
        s: &Slider,
        look: slider::Look,
    ) {
        let name = self.id.with(("slider", row.0, row.1));
        slider::draw_animated(p, pal, self.anim, name, r, s, look);
    }
}

impl Form {
    /// [`draw`](Self::draw) with motion (see the [module](self)): the switches slide, the
    /// chosen segments slide, the hover fills and the scrollbar fade, a slider row's thumb
    /// glides. `anim` is the window's animator, with its frame begun; `id` names this form in
    /// it. At rest, and on every frame when the animator does not animate, this draws exactly
    /// what `draw` draws. After it, the animator's `end_frame` says when the next frame is due
    /// while something moves.
    pub fn draw_animated(
        &mut self,
        p: &mut Painter,
        pal: &Colors,
        window: Rect,
        caret_on: bool,
        anim: &mut Animator,
        id: impl AsId,
    ) {
        let mut fx = Fx {
            anim,
            id: Id::new(id),
            high_contrast: self.high_contrast,
        };
        self.draw_with(p, pal, window, caret_on, Some(&mut fx));
    }

    /// Whether the system's contrast theme is on: [`draw_animated`](Self::draw_animated) then
    /// changes highlights at once. Pass the host's `Cx::high_contrast` or
    /// [`crate::platform::system_high_contrast`]. It changes nothing that [`draw`](Self::draw)
    /// draws. Returns true when it changed, so the caller redraws.
    pub fn set_high_contrast(&mut self, on: bool) -> bool {
        let changed = self.high_contrast != on;
        self.high_contrast = on;
        changed
    }
}
