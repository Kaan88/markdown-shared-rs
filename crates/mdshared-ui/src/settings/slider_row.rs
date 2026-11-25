//! The slider row of the settings dialog ([`Row::Slider`]): a whole number between two ends
//! with a step, shown as its value and unit and then a track with a thumb, the
//! [`slider`](crate::slider) widget's. It is a number row ([`Row::Number`]) drawn and moved
//! another way: the same fields, the same [`Value::Number`](super::Value::Number), the same
//! [`Outcome::Changed`] for every change.
//!
//! The model of the thumb is [`slider::Slider`](crate::slider::Slider), made from the row's
//! fields whenever it is needed, so the row's `value` is the one truth and nothing goes stale
//! after [`Form::set_value`] or [`Form::set_rows`]; only the grab of a drag under way is kept
//! (in the form's drag state) until the button goes up.
//!
//! Events, as in the rest of the form: Left and Right move one step (with Ctrl a page), Page Up
//! and Page Down a page (five steps, at least a tenth of the track), Home and End to the ends,
//! `+` and `-` one step, all as [`slider::Slider::step_by`](crate::slider::Slider::step_by)
//! moves. A press on the track moves the value under the pointer at once and takes hold of the
//! thumb; a press on the thumb takes hold where it is; the value follows the pointer until the
//! button goes up. [`Form::pointer`] reports each change of a drag as it happens;
//! [`Form::pointer_move`], whose answer is a `bool`, moves the thumb and leaves the report to
//! [`Form::release`].
//!
//! Deliberately not here:
//! - the wheel: it scrolls the form's body over a slider row as over every other row, so a
//!   turn of the wheel through a long form never changes a setting by accident (a number row
//!   does not take the wheel either);
//! - Up and Down, which move the keyboard over the rows in this form, as they do from a number
//!   row (a slider on its own takes them as less and more);
//! - fractions: the value is an `i32`, as a number row's; a range of more than about sixteen
//!   million loses whole numbers to the slider's `f32`;
//! - a slider in the paged form ([`pages`](super::pages)), which draws and moves a slider row
//!   as its stepper.

use super::motion::Fx;
use super::{Drag, Form, Look, Outcome, Row, Target, number_text};
use crate::gfx::{Face, Painter, Rect};
use crate::keys::KeyName;
use crate::slider::{self, Slider, Step};
use crate::theme::Palette as Colors;

/// The length of a slider row's track, logical px.
pub(super) const TRACK_W: f32 = 160.0;
/// The room between the value's text and the track.
pub(super) const GAP: f32 = 10.0;

/// `v` held between `min` and `max` and snapped to a multiple of `step` from `min`, as the
/// slider snaps it.
pub(super) fn snap(v: i32, min: i32, max: i32, step: i32) -> i32 {
    model(v, min, max, step).value().round() as i32
}

/// The slider of a row's fields.
fn model(value: i32, min: i32, max: i32, step: i32) -> Slider {
    Slider::new(min as f32, max.max(min) as f32, value as f32).step(step.max(1) as f32)
}

impl Row {
    /// A slider row: `value` between `min` and `max` (a `max` under `min` is `min`), moved one
    /// at a time; [`step`](Self::step) gives a bigger step, to whose multiples from `min` the
    /// value snaps, and [`unit`](Self::unit) the unit after the value. Its value comes and goes
    /// as [`Value::Number`](super::Value::Number), like a number row's.
    pub fn slider(id: u32, label: impl Into<String>, value: i32, min: i32, max: i32) -> Row {
        let max = max.max(min);
        Row::Slider {
            id,
            label: label.into(),
            help: String::new(),
            value: snap(value, min, max, 1),
            min,
            max,
            step: 1,
            unit: String::new(),
        }
    }
}

impl Form {
    /// The track of slider row `i`: the right part of its [`control_rect`](Self::control_rect),
    /// after the value's text. Empty for a row that is not a slider.
    pub fn slider_rect(&self, i: usize) -> Rect {
        if !matches!(self.rows.get(i), Some(Row::Slider { .. })) {
            return Rect::default();
        }
        let c = self.control_rect(i);
        Rect::new(c.right() - TRACK_W.min(c.w), c.y, TRACK_W.min(c.w), c.h)
    }

    /// The slider of row `i`, from its fields.
    fn slider_model(&self, i: usize) -> Option<Slider> {
        match self.rows.get(i) {
            Some(Row::Slider {
                value,
                min,
                max,
                step,
                ..
            }) => Some(model(*value, *min, *max, *step)),
            _ => None,
        }
    }

    /// Puts the slider's value into row `i`; true when it changed.
    fn put_slider(&mut self, i: usize, to: f32) -> bool {
        if let Some(Row::Slider {
            value, min, max, ..
        }) = self.rows.get_mut(i)
        {
            let next = (to.round() as i32).clamp(*min, *max);
            if next != *value {
                *value = next;
                return true;
            }
        }
        false
    }

    /// The slider row `i` moved by `step`.
    fn slider_step(&mut self, i: usize, step: Step) -> Outcome {
        let Some(mut s) = self.slider_model(i) else {
            return Outcome::Unused;
        };
        s.step_by(step);
        if self.put_slider(i, s.value()) {
            self.changed(i)
        } else {
            Outcome::None
        }
    }

    /// Left (`more` false) or Right on slider row `i`; with Ctrl (`big`) a page.
    pub(super) fn slider_key(&mut self, i: usize, more: bool, big: bool) -> Outcome {
        let step = match (more, big) {
            (false, false) => Step::Less,
            (true, false) => Step::More,
            (false, true) => Step::PageLess,
            (true, true) => Step::PageMore,
        };
        self.slider_step(i, step)
    }

    /// The other keys of slider row `i`: the pages, the ends, `+` and `-`.
    pub(super) fn slider_other_key(&mut self, i: usize, key: &KeyName) -> Outcome {
        let step = match key {
            KeyName::PageUp => Step::PageMore,
            KeyName::PageDown => Step::PageLess,
            KeyName::Home => Step::Min,
            KeyName::End => Step::Max,
            KeyName::Char('+' | '=') => Step::More,
            KeyName::Char('-') => Step::Less,
            _ => return Outcome::Unused,
        };
        self.slider_step(i, step)
    }

    /// A press on the track of slider row `i`.
    pub(super) fn slider_press(&mut self, i: usize, x: f32, y: f32) -> Outcome {
        self.focus = Some(i);
        let Some(mut s) = self.slider_model(i) else {
            return Outcome::Redraw;
        };
        s.press(self.slider_rect(i), x, y);
        let moved = self.put_slider(i, s.value());
        let now = match self.rows[i] {
            Row::Slider { value, .. } => value,
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
        if !matches!(self.rows.get(i), Some(Row::Slider { .. })) {
            // The rows were replaced under the drag.
            self.drag = Drag::None;
            return false;
        }
        let moved = s.drag(self.slider_rect(i), x, y);
        self.drag = Drag::Slider(i, s, reported);
        moved && self.put_slider(i, s.value())
    }

    /// The button went up with slider row `i`'s thumb in hand.
    pub(super) fn slider_release(&mut self, i: usize, reported: i32) -> Outcome {
        self.drag = Drag::None;
        self.pressed = None;
        match self.rows.get(i) {
            Some(Row::Slider { value, .. }) if *value != reported => self.changed(i),
            _ => Outcome::Redraw,
        }
    }

    /// The pointer moved: everything [`pointer_move`](Self::pointer_move) does, with the change
    /// of a slider row's value under a drag as [`Outcome::Changed`] at once (and `Redraw` for
    /// any other change of the view, `None` for none). A host with slider rows calls this
    /// instead of `pointer_move`; either is right for a form without them.
    pub fn pointer(&mut self, x: f32, y: f32) -> Outcome {
        if let Drag::Slider(i, ..) = self.drag {
            if !self.slide(x, y) {
                return Outcome::None;
            }
            if let (Drag::Slider(_, s, _), Some(Row::Slider { value, .. })) =
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
            Some(Row::Slider {
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
