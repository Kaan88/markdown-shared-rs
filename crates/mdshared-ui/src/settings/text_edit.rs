//! Shared shaped pointer and edit menu routing for the two settings forms.
//! The forms supply their own hit testing and row storage. The host owns the edit popup,
//! pointer capture, clock and wake-ups.

use super::{Drag, Form, Outcome, Row, Target, Value};
use crate::cursor::Cursor;
use crate::gfx::{Face, Gfx, Rect};
use crate::keys::Mods;
use crate::menu::Item;
use crate::textfield::{ContextAction, FieldHit, Pointer, TextField};
use std::time::Instant;

#[derive(Default)]
pub(super) struct TextPointer {
    /// Keep the click sequence on release, but never carry it to another row or scale.
    last: Option<(usize, f32, Pointer)>,
    pub(super) held: bool,
}

impl TextPointer {
    pub(super) fn start(&mut self, i: usize, scale: f32) -> &mut Pointer {
        if !matches!(self.last, Some((row, s, _)) if row == i && s == scale) {
            self.last = Some((i, scale, Pointer::system(scale)));
        }
        self.held = true;
        &mut self.last.as_mut().unwrap().2
    }

    pub(super) fn active(&self) -> Option<(usize, f32, &Pointer)> {
        let (i, scale, pointer) = self.last.as_ref()?;
        self.held.then_some((*i, *scale, pointer))
    }

    pub(super) fn pointer(&mut self) -> &mut Pointer {
        &mut self.last.as_mut().unwrap().2
    }
}

/// Use the same inset, face and size for both the hit and the drawing.
pub(super) fn hit(area: Rect, scale: f32) -> FieldHit {
    FieldHit {
        area,
        face: Face::Ui,
        size: 13.0,
        scale,
    }
}

pub(super) fn apply(row: &mut Row, field: &mut TextField, action: ContextAction) -> Outcome {
    let Row::Text { id, value, .. } = row else {
        return Outcome::None;
    };
    if !field.apply_context_action(action) {
        return Outcome::None;
    }
    match action {
        ContextAction::Copy => return Outcome::None,
        ContextAction::SelectAll => return Outcome::Redraw,
        _ if value == field.text() => return Outcome::Redraw,
        _ => {}
    }
    *value = field.text().to_owned();
    Outcome::Changed {
        id: *id,
        value: Value::Text(value.clone()),
    }
}

// Generate the same inherent API for both forms without exposing their private row indices.
macro_rules! text_edit_methods {
    () => {
        /// Routes a primary press after layout. Use instead of `press` for shaped text hits,
        /// Shift+click and multiple clicks. Pass the drawing scale and the event's time; the
        /// form counts text clicks, while `clicks` is forwarded to other controls. End native
        /// input method composition before forwarding a press in a text field. With a colour
        /// row's picker open, the press is the picker's, and its hex field takes it as a text
        /// row does (padding, double and triple click, Shift).
        pub fn pointer_press(
            &mut self,
            at: (f32, f32),
            mods: Mods,
            clicks: u8,
            now: Instant,
            gfx: &mut Gfx,
            scale: f32,
        ) -> Outcome {
            if let Some(o) = self.popup_text_press(at, mods, clicks, now, gfx, scale) {
                return o;
            }
            if let Some(i) = self.text_at(at) {
                let hit = hit(self.text_area(i), scale);
                if hit.cursor(at).is_some() {
                    self.focus_text(i);
                    let (state, field) = self.text_parts(i);
                    state.start(i, scale).press(field, gfx, hit, at, mods, now);
                    return Outcome::Redraw;
                }
            }
            self.press(at.0, at.1, mods, clicks)
        }

        /// Routes a move after `pointer_press`, even outside the form while the button is held.
        /// Reports a redraw for selection and hover, or a value change for other dragged controls.
        pub fn pointer_motion(
            &mut self,
            at: (f32, f32),
            mods: Mods,
            now: Instant,
            gfx: &mut Gfx,
        ) -> Outcome {
            if let Some(o) = self.popup_text_motion(at, now, gfx) {
                return o;
            }
            if let Some((i, hit)) = self.active_text_hit() {
                let (state, field) = self.text_parts(i);
                if state.pointer().pointer_move(field, gfx, hit, at, now) {
                    return Outcome::Redraw;
                }
                return Outcome::None;
            }
            self.cancel_if_inactive();
            self.other_motion(at, mods)
        }

        /// Ends the drag, using the release position even if no move was delivered there.
        /// Route this instead of `release` when using `pointer_press`.
        pub fn pointer_release(&mut self, at: (f32, f32), now: Instant, gfx: &mut Gfx) -> Outcome {
            if let Some(o) = self.popup_text_release(at, now, gfx) {
                return o;
            }
            if let Some((i, hit)) = self.active_text_hit() {
                let (state, field) = self.text_parts(i);
                let changed = state.pointer().pointer_move(field, gfx, hit, at, now);
                state.pointer().release();
                state.held = false;
                return if changed {
                    Outcome::Redraw
                } else {
                    Outcome::None
                };
            }
            self.cancel_if_inactive();
            self.release(at.0, at.1)
        }

        /// The next wake-up for horizontal text selection scrolling, if any. Recheck after
        /// routing input or a tick; an idle form does not need a timer.
        pub fn text_scroll_deadline(&self) -> Option<Instant> {
            if let Some(at) = self.popup_text_deadline() {
                return Some(at);
            }
            self.active_text_hit()?;
            self.text_pointer.active()?.2.deadline()
        }

        /// Advances edge scrolling at its deadline. Returns whether to redraw.
        pub fn tick_text_scroll(&mut self, now: Instant, gfx: &mut Gfx) -> bool {
            if let Some(redraw) = self.popup_text_tick(now, gfx) {
                return redraw;
            }
            let Some((i, hit)) = self.active_text_hit() else {
                self.cancel_if_inactive();
                return false;
            };
            let (state, field) = self.text_parts(i);
            state.pointer().tick(field, gfx, hit, now)
        }

        /// Stops the drag and breaks the click sequence. Call on lost window focus or capture,
        /// or when the drawing scale changes. Does not clear the selection.
        pub fn cancel_text_pointer(&mut self) {
            self.text_pointer = Default::default();
            self.popup_text_cancel();
        }

        /// Requests an I-beam over an editable text area or a cursor for another control.
        /// An open form popup takes input first, so underlying text never requests an I-beam;
        /// a colour picker's hex field does. `None` leaves the choice to the host (normally
        /// the arrow).
        pub fn pointer_cursor_at(&self, x: f32, y: f32) -> Option<Cursor> {
            if self.popup_open() {
                return self.popup_text_cursor(x, y);
            }
            self.text_at((x, y))
                .map(|_| Cursor::Text)
                .or_else(|| self.other_cursor(x, y))
        }

        /// Builds an edit menu for the enabled field at `at`, or the focused field for `None`.
        /// A successful call focuses that field without moving its caret or selection. While a
        /// form popup is open, returns `None`, except on an open colour picker's hex field (or
        /// for `None` while it has the keyboard): its menu, and the field takes the keyboard.
        /// The host draws the menu, routes input to it first and dismisses it if the form's
        /// focus or rows change.
        pub fn context_menu(&mut self, at: Option<(f32, f32)>) -> Option<Vec<Item<ContextAction>>> {
            self.cancel_text_pointer();
            if let Some(items) = self.popup_text_menu(at) {
                return Some(items);
            }
            if self.popup_open() {
                return None;
            }
            let i = match at {
                Some(at) => self.text_at(at),
                None => self.focused_text(),
            }?;
            self.focus_text(i);
            Some(self.text_parts(i).1.context_menu())
        }

        /// Applies a menu action to the focused text field, unless a form popup is open.
        /// Reports `Changed` only for a new text value, or `Redraw` if just the selection may
        /// have changed. Copy has no visible effect. Secret fields accept Paste and refuse
        /// Cut and Copy even if called directly. With a colour picker open, it goes to the
        /// picker's hex field when that has the keyboard: a colour pasted is reported as one
        /// typed (staged, as `Redraw`, in a picker with Apply and Cancel).
        pub fn apply_context_action(&mut self, action: ContextAction) -> Outcome {
            self.cancel_text_pointer();
            if let Some(o) = self.popup_text_action(action) {
                return o;
            }
            if self.popup_open() {
                return Outcome::None;
            }
            match self.focused_text() {
                Some(i) => self.apply_text(i, action),
                None => Outcome::None,
            }
        }

        fn active_text_hit(&self) -> Option<(usize, FieldHit)> {
            let (i, scale, _) = self.text_pointer.active()?;
            (!self.popup_open() && self.focused_text() == Some(i))
                .then(|| (i, hit(self.text_area(i), scale)))
        }

        fn cancel_if_inactive(&mut self) {
            if self.text_pointer.held {
                self.cancel_text_pointer();
            }
        }
    };
}

pub(super) use text_edit_methods;

impl Form {
    text_edit_methods!();

    /// Whether a dropdown or colour picker is taking input, even before it is drawn.
    pub fn popup_open(&self) -> bool {
        self.menu.is_some() || self.colour_open_row().is_some()
    }

    fn text_at(&self, at: (f32, f32)) -> Option<usize> {
        if self.popup_open() || self.bar().is_some_and(|(r, _)| r.contains(at.0, at.1)) {
            return None;
        }
        match self.target_at(at.0, at.1) {
            Some(Target::Text(i)) if hit(self.text_area(i), 1.0).cursor(at).is_some() => Some(i),
            _ => None,
        }
    }

    fn focused_text(&self) -> Option<usize> {
        self.focus
            .filter(|&i| matches!(self.rows.get(i), Some(Row::Text { .. })))
    }

    fn focus_text(&mut self, i: usize) {
        self.focus = Some(i);
        self.focus_visible = false;
        self.drag = Drag::None;
        self.pressed = None;
        self.menu_pressed = None;
    }

    fn text_parts(&mut self, i: usize) -> (&mut TextPointer, &mut TextField) {
        (&mut self.text_pointer, &mut self.fields[i])
    }

    fn apply_text(&mut self, i: usize, action: ContextAction) -> Outcome {
        apply(&mut self.rows[i], &mut self.fields[i], action)
    }

    fn other_motion(&mut self, at: (f32, f32), mods: Mods) -> Outcome {
        self.pointer_with(at.0, at.1, mods)
    }

    fn other_cursor(&self, x: f32, y: f32) -> Option<Cursor> {
        self.cursor_at(x, y)
    }
}

#[cfg(test)]
#[path = "text_edit_tests.rs"]
mod tests;
