//! Number rows whose value is dragged ([`Form::make_draggable`]): a [`Row::Number`] whose id the
//! form was given is still drawn as its stepper (minus, the value with its unit, plus), and its
//! buttons and keys work as before; in addition, a press on the value's text followed by a
//! horizontal drag changes the value, as the number fields of a drawing or a sound program do.
//! Nothing about the row changes: it is built with [`Row::number`] (its `step` too), its value
//! comes and goes as [`Value::Number`](super::Value::Number), and every change is an
//! [`Outcome::Changed`]. A form never told of a draggable row draws and answers as it always
//! has, and so does a row of a told form whose id was not named.
//!
//! The marks are ids, kept by the form as the slider marks are
//! ([`Form::show_as_slider`]): they outlast [`Form::set_rows`], so a row rebuilt with the same id
//! is still draggable, and an id with no number row marks nothing until one comes. A drag under
//! way ends at `set_rows` (the row in hand might now be another one), with no report. A row
//! that is also shown as a slider is a slider: its thumb is what is dragged, not its text.
//!
//! The drag: the value is the value at the press plus one `step` for every
//! [`PX_PER_STEP`] (4) logical px the pointer has moved to the right of where it was pressed,
//! less one step for every 4 px to the left. With Shift held it is one step for every
//! [`FINE_PX_PER_STEP`] (16) px: the step is never divided, so the value stays on the numbers the
//! stepper's buttons reach. Pressing or letting go of Shift during the drag does not make the
//! value jump: the drag carries on from the value and the place where the pointer was then. The
//! value is held inside the row's ends (a row built by hand with `min` over `max` is read with
//! its ends swapped); the place, not the value, follows the pointer, so after a drag past an
//! end the pointer must come back over that stretch before the value moves away from the end.
//! The arithmetic is whole numbers in `i64`, so it neither overflows nor panics over the whole
//! `i32` range, and a pointer at a place that is not a number moves nothing.
//!
//! Reporting, as for a slider's drag: [`Form::pointer`] (and [`Form::pointer_with`], which also
//! says whether Shift is down now) reports each change as an [`Outcome::Changed`] when it
//! happens; [`Form::pointer_move`], whose answer is a `bool`, moves the value and leaves the
//! report to [`Form::release`]. Escape during the drag puts back the value the drag started
//! from (an `Outcome::Changed` when it had moved) and ends the drag; the release that follows
//! does nothing. The release ends it too. Other keys during a drag act as they do without one;
//! the next move of the pointer puts the value back where the pointer says.
//!
//! The pointer's shape: [`Form::cursor_at`] answers [`Cursor::ResizeEw`] over a draggable value
//! and all through its drag (wherever the pointer is then), and `None` elsewhere, so the host
//! lists it first among its candidates: `cursor::pick([form.cursor_at(x, y), ..])`, then
//! [`CursorState::set`](crate::cursor::CursorState::set). `Cursor::ResizeEw` is winit's
//! `CursorIcon::EwResize` ([`Cursor::icon`]), which winit shows on Windows as the system's
//! `IDC_SIZEWE`; a host that sets the cursor itself loads that one
//! (`LoadCursorW(null, IDC_SIZEWE)`).
//!
//! No clock is read and no timer runs: the value moves only when the pointer does.
//!
//! Deliberately not here:
//! - a vertical drag, or acceleration with speed: the change is a function of where the
//!   pointer is, so a slow drag and a fast one over the same stretch give the same value;
//! - the wheel, which scrolls the form's body over a stepper as it always has;
//! - a look of its own: the row is drawn as its stepper, during a drag too (the cursor is the
//!   sign that it can be dragged);
//! - the paged form ([`pages`](super::pages)), which has no such marks: its rows ignore drags.

use super::{Drag, Form, Outcome, Row, Target};
use crate::cursor::Cursor;
use crate::keys::{KeyName, Mods};

/// Logical px the pointer moves for one step of a dragged value.
pub const PX_PER_STEP: f32 = 4.0;
/// Logical px the pointer moves for one step with Shift held: a quarter of the pace.
pub const FINE_PX_PER_STEP: f32 = 16.0;

/// A number row's value in hand: what the drag needs between events.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct Held {
    /// The row's index.
    row: usize,
    /// The value at the press, put back by Escape.
    start: i32,
    /// Where the pointer was when the drag last began counting (the press, or the last change
    /// of Shift), and the value then.
    anchor_x: f32,
    anchor: i64,
    /// Shift was down at the last move.
    fine: bool,
    /// The value last reported in an [`Outcome::Changed`].
    reported: i32,
}

/// A number row's ends in order and its step (at least 1), as the drag reads them.
fn span(row: &Row) -> Option<(i64, i64, i64)> {
    match row {
        Row::Number { min, max, step, .. } => Some((
            i64::from((*min).min(*max)),
            i64::from((*min).max(*max)),
            i64::from((*step).max(1)),
        )),
        _ => None,
    }
}

/// The value with the pointer at `x`, for a drag that counts from `anchor` at `anchor_x`: one
/// `step` per `px` logical px (towards zero), held to `lo..=hi`. `None` for a place that is not a
/// number.
fn value_at(
    (lo, hi, step): (i64, i64, i64),
    anchor: i64,
    anchor_x: f32,
    x: f32,
    fine: bool,
) -> Option<i64> {
    let px = if fine { FINE_PX_PER_STEP } else { PX_PER_STEP };
    let d = f64::from(x) - f64::from(anchor_x);
    if d.is_nan() {
        return None;
    }
    // `as` saturates, so an infinite place is a very long way, not a panic.
    let steps = (d / f64::from(px)).trunc() as i64;
    Some(
        anchor
            .saturating_add(steps.saturating_mul(step))
            .clamp(lo, hi),
    )
}

impl Form {
    /// Lets the value of the number row `id` be dragged left and right (see the
    /// [module](self)); the row is still drawn and stepped as a stepper. The mark is kept by id
    /// through [`set_rows`](Self::set_rows), and marks a number row of that id added later.
    /// Returns whether the form has such a number row now; another kind of row with that id
    /// ignores drags as before.
    pub fn make_draggable(&mut self, id: u32) -> bool {
        if !self.draggable.contains(&id) {
            self.draggable.push(id);
        }
        self.rows
            .iter()
            .any(|r| matches!(r, Row::Number { id: k, .. } if *k == id))
    }

    /// Whether row `i` is a number row whose value is dragged (and not a slider).
    fn is_draggable(&self, i: usize) -> bool {
        matches!(self.rows.get(i), Some(Row::Number { id, .. }) if self.draggable.contains(id))
            && !self.is_slider(i)
    }

    /// Whether `(x, y)` is on the value's text of a draggable row, as the form is laid out now.
    fn on_draggable_value(&self, x: f32, y: f32) -> Option<usize> {
        match self.target_at(x, y) {
            Some(Target::Row(i))
                if self.is_draggable(i) && self.stepper_parts(i).1.contains(x, y) =>
            {
                Some(i)
            }
            _ => None,
        }
    }

    /// A press on the control of row `i` at `(x, y)`: takes hold of the value when it is a
    /// draggable row's value. The form's press has already focused the row.
    pub(super) fn drag_number_press(&mut self, i: usize, x: f32, y: f32, mods: Mods) {
        if self.on_draggable_value(x, y) != Some(i) {
            return;
        }
        if let Some(Row::Number { value, .. }) = self.rows.get(i) {
            self.drag = Drag::Number(Held {
                row: i,
                start: *value,
                anchor_x: x,
                anchor: i64::from(*value),
                fine: mods.shift,
                reported: *value,
            });
        }
    }

    /// The pointer moved to `x` with a value in hand (`fine`: Shift is down, or `None` when the
    /// caller does not say, which keeps the pace it had): true when the value changed.
    pub(super) fn drag_number_to(&mut self, x: f32, fine: Option<bool>) -> bool {
        let Drag::Number(mut h) = self.drag else {
            return false;
        };
        let Some(sp) = self.rows.get(h.row).and_then(span) else {
            self.drag = Drag::None;
            return false;
        };
        let now = match self.rows[h.row] {
            Row::Number { value, .. } => value,
            _ => return false,
        };
        // Carry on from here at the new pace, with no jump; a place that is not a number cannot
        // anchor it, so the pace waits for one that is.
        if let Some(fine) = fine
            && fine != h.fine
            && x.is_finite()
        {
            h.anchor_x = x;
            h.anchor = i64::from(now);
            h.fine = fine;
        }
        self.drag = Drag::Number(h);
        let Some(to) = value_at(sp, h.anchor, h.anchor_x, x, h.fine) else {
            return false;
        };
        let to = to as i32;
        if to == now {
            return false;
        }
        if let Some(Row::Number { value, .. }) = self.rows.get_mut(h.row) {
            *value = to;
        }
        true
    }

    /// The pointer moved with a value in hand, reported as it happens: `Changed` when the value
    /// moved, `None` when it did not.
    pub(super) fn drag_number_pointer(&mut self, x: f32, fine: Option<bool>) -> Outcome {
        if !self.drag_number_to(x, fine) {
            return Outcome::None;
        }
        let Drag::Number(mut h) = self.drag else {
            return Outcome::None;
        };
        if let Some(Row::Number { value, .. }) = self.rows.get(h.row) {
            h.reported = *value;
        }
        self.drag = Drag::Number(h);
        self.changed(h.row)
    }

    /// The pointer moved, with whether Shift is down now: [`pointer`](Self::pointer), with the
    /// fine pace of a dragged number row following Shift (one step per 16 px instead of 4)
    /// during the drag as well as at its press. Use it instead of `pointer` where the host knows
    /// the modifier keys at a move; with `pointer` the pace stays the one the press began with.
    pub fn pointer_with(&mut self, x: f32, y: f32, mods: Mods) -> Outcome {
        if matches!(self.drag, Drag::Number(_)) {
            return self.drag_number_pointer(x, Some(mods.shift));
        }
        self.pointer(x, y)
    }

    /// The button went up with a value in hand: `Changed` when it moved since the last report,
    /// `None` when it is where the press found it (a click on the value answers as without the
    /// mark), `Redraw` otherwise.
    pub(super) fn drag_number_release(&mut self) -> Outcome {
        let Drag::Number(h) = self.drag else {
            return Outcome::None;
        };
        self.drag = Drag::None;
        self.pressed = None;
        match self.rows.get(h.row) {
            Some(Row::Number { value, .. }) if *value != h.reported => self.changed(h.row),
            Some(Row::Number { value, .. }) if *value != h.start => Outcome::Redraw,
            _ => Outcome::None,
        }
    }

    /// A key with a value in hand: Escape puts back the value the drag started from and ends
    /// the drag (`Changed` when it had moved, `Redraw` when not). `None` for any other key, or
    /// with no drag: the form handles it as it always has.
    pub(super) fn drag_number_key(&mut self, key: &KeyName) -> Option<Outcome> {
        let Drag::Number(h) = self.drag else {
            return None;
        };
        if *key != KeyName::Escape {
            return None;
        }
        self.drag = Drag::None;
        self.pressed = None;
        let mut moved = false;
        if let Some(Row::Number { value, .. }) = self.rows.get_mut(h.row)
            && *value != h.start
        {
            *value = h.start;
            moved = true;
        }
        Some(if moved {
            self.changed(h.row)
        } else {
            Outcome::Redraw
        })
    }

    /// The cursor the form asks for at `(x, y)`: [`Cursor::ResizeEw`] over the value of a
    /// draggable number row and all through its drag, wherever the pointer is; `None` (no
    /// opinion: the host's own choice, usually the arrow) everywhere else, while a dropdown or a
    /// colour picker is open (its card may cover the rows), before the first layout, and always
    /// in a form with no draggable row. See the [module](self) for how a host passes it on.
    pub fn cursor_at(&self, x: f32, y: f32) -> Option<Cursor> {
        if matches!(self.drag, Drag::Number(_)) {
            return Some(Cursor::ResizeEw);
        }
        if self.menu.is_some() || self.colour_open_row().is_some() || self.draggable.is_empty() {
            return None;
        }
        self.on_draggable_value(x, y).map(|_| Cursor::ResizeEw)
    }
}
