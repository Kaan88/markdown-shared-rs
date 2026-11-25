//! Number rows shown as sliders ([`Form::show_as_slider`]): a [`Row::Number`] whose id the form
//! was given is drawn as its value and unit, then a track with a thumb (the
//! [`slider`](crate::slider) widget's), instead of a stepper. Nothing else about the row
//! changes: it is built with [`Row::number`] (its `step` and `unit` too), its value comes and
//! goes as [`Value::Number`](super::Value::Number), and every change is an
//! [`Outcome::Changed`]. A form never told of a slider draws and answers as it always has.
//!
//! The marks are ids, kept by the form: they outlast [`Form::set_rows`], so a row rebuilt with
//! the same id is still a slider, and an id with no number row marks nothing until one comes.
//! Only a drag under way ends at `set_rows` (the thumb in hand might now be another row's).
//!
//! The value is whole numbers in `i64` arithmetic, so the keys are exact over the whole `i32`
//! range. The thumb's place is a fraction of the track held in an `f32` (the slider widget's
//! model, on a track from 0 to 1): a press or a drag lands on the whole number nearest that
//! fraction, so over a span of more than about sixteen million (2^24) a pointer cannot reach
//! every number, which no track a few hundred pixels long could show anyway.
//!
//! Events, as in the rest of the form: Left and Right move one step (with Ctrl a page), Page Up
//! and Page Down a page (five steps, or a tenth of the span if that is more), Home and End to
//! the ends, `+` and `-` one step; a value moved by a key lands on a multiple of the step from
//! the low end (or on the high end). A press on the track moves the value under the pointer at
//! once and takes hold of the thumb; a press on the thumb takes hold where it is; the value
//! follows the pointer until the button goes up. [`Form::pointer`] reports each change of a
//! drag as it happens; [`Form::pointer_move`], whose answer is a `bool`, moves the thumb and
//! leaves the report to [`Form::release`]. A dropdown opened by the keys during a drag lets the
//! thumb go.
//!
//! Deliberately not here:
//! - the wheel: it scrolls the form's body over a slider as over every other row, so a turn of
//!   the wheel through a long form never changes a setting by accident (a stepper does not take
//!   the wheel either);
//! - Up and Down, which move the keyboard over the rows in this form, as they do from a
//!   stepper (a slider on its own takes them as less and more);
//! - a slider in the paged form ([`pages`](super::pages)), which draws such a row as its
//!   stepper: the marks are this form's.

use super::motion::Fx;
use super::{Drag, Form, Look, Outcome, Row, Target, number_text};
use crate::gfx::{Face, Painter, Rect};
use crate::keys::KeyName;
use crate::slider::{self, Slider};
use crate::theme::Palette as Colors;

/// The length of a slider's track, logical px.
pub(super) const TRACK_W: f32 = 160.0;
/// The room between the value's text and the track.
pub(super) const GAP: f32 = 10.0;

/// A number row's fields as the slider reads them: its ends in order (a row built by hand with
/// `min` over `max` has them swapped), its step (at least 1) and its value.
#[derive(Clone, Copy, Debug)]
struct Span {
    lo: i64,
    hi: i64,
    step: i64,
    value: i64,
}

impl Span {
    fn of(row: &Row) -> Option<Span> {
        match row {
            Row::Number {
                value,
                min,
                max,
                step,
                ..
            } => Some(Span {
                lo: i64::from((*min).min(*max)),
                hi: i64::from((*min).max(*max)),
                step: i64::from((*step).max(1)),
                value: i64::from(*value),
            }),
            _ => None,
        }
    }

    /// `v` on a multiple of the step from the low end, inside the ends.
    fn on_grid(&self, v: i64) -> i32 {
        let k = ((v - self.lo) as f64 / self.step as f64).round() as i64;
        let v = self.lo.saturating_add(k.saturating_mul(self.step));
        v.clamp(self.lo, self.hi) as i32
    }

    /// Where the value is along the track, 0 to 1.
    fn fraction(&self) -> f32 {
        let span = self.hi - self.lo;
        if span <= 0 {
            return 0.0;
        }
        ((self.value - self.lo) as f64 / span as f64).clamp(0.0, 1.0) as f32
    }

    /// The whole number at a place along the track, on the step's grid.
    fn at(&self, fraction: f32) -> i32 {
        let t = if fraction.is_nan() {
            0.0
        } else {
            f64::from(fraction).clamp(0.0, 1.0)
        };
        let v = self.lo + (t * (self.hi - self.lo) as f64).round() as i64;
        self.on_grid(v)
    }
}

impl Form {
    /// Draws the number row `id` as a slider (see the [module](self)) from the next layout on,
    /// and moves it as one: its value stays a [`Value::Number`](super::Value::Number), its
    /// `step` is the slider's. The mark is kept by id through [`set_rows`](Self::set_rows), and
    /// marks a number row of that id added later. Returns whether the form has such a number
    /// row now; another kind of row with that id is drawn as it was.
    pub fn show_as_slider(&mut self, id: u32) -> bool {
        if !self.sliders.contains(&id) {
            self.sliders.push(id);
        }
        self.rows
            .iter()
            .any(|r| matches!(r, Row::Number { id: k, .. } if *k == id))
    }

    /// Whether row `i` is a number row shown as a slider.
    pub(super) fn is_slider(&self, i: usize) -> bool {
        matches!(self.rows.get(i), Some(Row::Number { id, .. }) if self.sliders.contains(id))
    }

    /// The track of slider row `i`: the right part of its [`control_rect`](Self::control_rect),
    /// after the value's text. Empty for a row that is not shown as a slider.
    pub fn slider_rect(&self, i: usize) -> Rect {
        if !self.is_slider(i) {
            return Rect::default();
        }
        let c = self.control_rect(i);
        let w = TRACK_W.min(c.w).max(0.0);
        Rect::new(c.right() - w, c.y, w, c.h)
    }

    /// The thumb of slider row `i`: the slider widget on a track from 0 to 1, at the row's
    /// place along it.
    fn slider_model(&self, i: usize) -> Option<Slider> {
        if !self.is_slider(i) {
            return None;
        }
        Span::of(&self.rows[i]).map(|s| Slider::new(0.0, 1.0, s.fraction()))
    }

    /// Puts `to` into number row `i`; true when it changed.
    fn put_slider(&mut self, i: usize, to: i32) -> bool {
        if let Some(Row::Number { value, .. }) = self.rows.get_mut(i)
            && *value != to
        {
            *value = to;
            return true;
        }
        false
    }

    /// The value of slider row `i` changed by `how` from its span.
    fn slider_to(&mut self, i: usize, how: impl Fn(&Span) -> i32) -> Outcome {
        let Some(span) = self.rows.get(i).and_then(Span::of) else {
            return Outcome::Unused;
        };
        if self.put_slider(i, how(&span)) {
            self.changed(i)
        } else {
            Outcome::None
        }
    }

    /// Left (`more` false) or Right on slider row `i`; with Ctrl (`big`) a page.
    pub(super) fn slider_key(&mut self, i: usize, more: bool, big: bool) -> Outcome {
        let dir = if more { 1 } else { -1 };
        self.slider_to(i, |s| {
            let by = if big { page(s) } else { s.step };
            s.on_grid(s.value + dir * by)
        })
    }

    /// The other keys of slider row `i`: the pages, the ends, `+` and `-`.
    pub(super) fn slider_other_key(&mut self, i: usize, key: &KeyName) -> Outcome {
        match key {
            KeyName::PageUp => self.slider_to(i, |s| s.on_grid(s.value + page(s))),
            KeyName::PageDown => self.slider_to(i, |s| s.on_grid(s.value - page(s))),
            KeyName::Home => self.slider_to(i, |s| s.lo as i32),
            KeyName::End => self.slider_to(i, |s| s.hi as i32),
            KeyName::Char('+' | '=') => self.slider_key(i, true, false),
            KeyName::Char('-') => self.slider_key(i, false, false),
            _ => Outcome::Unused,
        }
    }

    /// A press on the track of slider row `i`.
    pub(super) fn slider_press(&mut self, i: usize, x: f32, y: f32) -> Outcome {
        self.focus = Some(i);
        let (Some(mut s), Some(span)) = (self.slider_model(i), self.rows.get(i).and_then(Span::of))
        else {
            return Outcome::Redraw;
        };
        let before = s.value();
        s.press(self.slider_rect(i), x, y);
        let moved = s.value() != before && self.put_slider(i, span.at(s.value()));
        let now = match self.rows[i] {
            Row::Number { value, .. } => value,
            _ => 0,
        };
        self.drag = Drag::Slider(i, s, now);
        if moved {
            self.changed(i)
        } else {
            Outcome::Redraw
        }
    }

    /// The pointer moved with a slider's thumb in hand: true when the value changed.
    pub(super) fn slide(&mut self, x: f32, y: f32) -> bool {
        let Drag::Slider(i, mut s, reported) = self.drag else {
            return false;
        };
        let Some(span) = self.is_slider(i).then(|| Span::of(&self.rows[i])).flatten() else {
            self.drag = Drag::None;
            return false;
        };
        let moved = s.drag(self.slider_rect(i), x, y);
        self.drag = Drag::Slider(i, s, reported);
        moved && self.put_slider(i, span.at(s.value()))
    }

    /// The button went up with slider row `i`'s thumb in hand.
    pub(super) fn slider_release(&mut self, i: usize, reported: i32) -> Outcome {
        self.drag = Drag::None;
        self.pressed = None;
        match self.rows.get(i) {
            Some(Row::Number { value, .. }) if self.is_slider(i) && *value != reported => {
                self.changed(i)
            }
            _ => Outcome::Redraw,
        }
    }

    /// The pointer moved: everything [`pointer_move`](Self::pointer_move) does, with the change
    /// of a slider's value under a drag as [`Outcome::Changed`] at once (and `Redraw` for any
    /// other change of the view, `None` for none). A host with sliders calls this instead of
    /// `pointer_move`; either is right for a form without them.
    pub fn pointer(&mut self, x: f32, y: f32) -> Outcome {
        if let Some(o) = self.colour_pointer(x, y) {
            return o;
        }
        if matches!(self.drag, Drag::Number(_)) {
            return self.drag_number_pointer(x, None);
        }
        if let Drag::Slider(i, ..) = self.drag {
            if !self.slide(x, y) {
                return Outcome::None;
            }
            if let (Drag::Slider(_, s, _), Some(Row::Number { value, .. })) =
                (self.drag, self.rows.get(i))
            {
                self.drag = Drag::Slider(i, s, *value);
            }
            return self.changed(i);
        }
        if self.pointer_move(x, y) {
            Outcome::Redraw
        } else {
            Outcome::None
        }
    }

    /// Draws slider row `i`'s control: the value and its unit, then the slider. With `fx` the
    /// thumb glides ([`slider::draw_animated`]).
    pub(super) fn draw_slider_row(
        &self,
        p: &mut Painter,
        pal: &Colors,
        i: usize,
        ring_on: bool,
        fx: Option<&mut Fx<'_>>,
    ) {
        let (
            Some(s),
            Some(Row::Number {
                id, value, unit, ..
            }),
        ) = (self.slider_model(i), self.rows.get(i))
        else {
            return;
        };
        let track = self.slider_rect(i);
        if let Some(Look::Slider(_)) = self.looks.get(i) {
            p.text_right(
                Face::Ui,
                13.0,
                track.x - GAP,
                track.cy(),
                &number_text(*value, unit),
                pal.text,
            );
        }
        let held = matches!(self.drag, Drag::Slider(k, ..) if k == i);
        let look = slider::Look {
            hot: held || self.hover == Some(Target::Slider(i)),
            pressed: held,
            focused: ring_on,
            ..slider::Look::default()
        };
        match fx {
            Some(fx) => fx.slider(p, pal, (i, *id), track, &s, look),
            None => slider::draw(p, pal, track, &s, look),
        }
    }
}

/// A page of a slider: five steps, or a tenth of the span if that is more.
fn page(s: &Span) -> i64 {
    (s.step * 5).max((s.hi - s.lo + 9) / 10)
}
