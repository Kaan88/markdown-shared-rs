//! The paged settings dialog in motion (roadmap L10): [`Form::draw_animated`], the opt-in
//! beside [`Form::draw`], with the motion of the single-page form's
//! [`draw_animated`](super::super::Form::draw_animated) where the paged form has the same
//! pieces. A switch's knob slides and its fill fades in (the motion of
//! `toggle::draw_toggle_animated`); the chosen segment of a segmented choice slides to the one
//! chosen (a disabled row's paler fill slides the same way); a page's hover fill in the list
//! fades in and out by
//! [`document::HOVER`](crate::anim::document::HOVER); the open dropdown's hover row fades
//! (`Menu::draw_in_animated`).
//!
//! The caller owns the [`Animator`] and the frame (`begin_frame` before the draw, `end_frame`
//! after it for the next deadline), and names the form in it with `id`. The form keeps no
//! clock and no motion of its own: every value in flight lives in the animator, under names
//! made from `id`, the page, the row's index and the row's id, so a row replaced by another,
//! or the row at the same place on another page, starts at rest. The list's hover fills are
//! named from `id` and the page's place in the list.
//!
//! At rest, and on every frame of an animator that does not animate ([`Policy::Never`], or the
//! system asking for less motion), `draw_animated` draws exactly what `draw` draws, pixel for
//! pixel, and `draw` draws what it drew before this file was added (`frozen.rs`); the tests in
//! `motion_tests.rs` hold both to that. Under high contrast ([`Form::set_high_contrast`]) the
//! highlights (the list's hover fills, the menu's row, the chosen segment) change at once, as
//! the system's own controls do there; a switch's knob still moves.
//!
//! Deliberately not here:
//! - the dialog's entrance and exit, and the change of page (the caller fades the whole frame
//!   with `anim::pop`, or draws it under `overlay::draw_backdrop_presence`, as for any dialog);
//! - a row's hover fill and the scrollbar: the paged form draws neither (a row has no hover
//!   look, and a page never scrolls);
//! - the hover of the ×, the steppers, the buttons, a text field's edge and a dropdown's field,
//!   which change at once as they do in `draw` (the single-page form's Close button and
//!   steppers do the same);
//! - a slider: a number row the single-page form shows as a slider is a stepper here.
//!
//! [`Policy::Never`]: crate::anim::Policy::Never

use super::super::motion::Fx;
use super::Form;
use crate::anim::{Animator, AsId, Id};
use crate::gfx::{Painter, Rect};
use crate::theme::Palette as Colors;

impl Form {
    /// [`draw`](Self::draw) with motion (see the [module](self)): the switches slide, the
    /// chosen segments slide, the list's hover fills and the open dropdown's hover row fade.
    /// `anim` is the window's animator, with its frame begun; `id` names this form in it. At
    /// rest, and on every frame when the animator does not animate, this draws exactly what
    /// `draw` draws. After it, the animator's `end_frame` says when the next frame is due while
    /// something moves.
    pub fn draw_animated(
        &mut self,
        p: &mut Painter,
        pal: &Colors,
        window: Rect,
        caret_on: bool,
        anim: &mut Animator,
        id: impl AsId,
    ) {
        let mut fx = Fx::new(anim, Id::new(id), self.high_contrast);
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
