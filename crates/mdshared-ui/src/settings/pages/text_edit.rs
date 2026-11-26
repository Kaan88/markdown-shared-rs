//! Connects the paged form's geometry, focus and rows to the shared text editing path.

use super::super::text_edit::{TextPointer, apply, hit, text_edit_methods};
use super::{Focus, Form, Outcome, Row, Target, is_text};
use crate::cursor::Cursor;
use crate::gfx::Gfx;
use crate::keys::Mods;
use crate::menu::Item;
use crate::textfield::{ContextAction, FieldHit, TextField};
use std::time::Instant;

impl Form {
    text_edit_methods!();

    /// Whether a form popup (currently a dropdown) is taking input, even before it is drawn.
    pub fn popup_open(&self) -> bool {
        self.menu.is_some()
    }

    fn text_at(&self, at: (f32, f32)) -> Option<usize> {
        if self.popup_open() {
            return None;
        }
        match self.target_at(at.0, at.1) {
            Some(Target::Text(i)) if hit(self.text_area(i), 1.0).cursor(at).is_some() => Some(i),
            _ => None,
        }
    }

    fn focused_text(&self) -> Option<usize> {
        match self.focus {
            Focus::Control(i)
                if self.kind(i).is_some_and(is_text)
                    && self.item(i).is_some_and(|it| !it.is_disabled()) =>
            {
                Some(i)
            }
            _ => None,
        }
    }

    fn focus_text(&mut self, i: usize) {
        self.focus = Focus::Control(i);
        self.focus_visible = false;
        self.drag = None;
        self.pressed = None;
        self.menu_pressed = None;
    }

    fn text_parts(&mut self, i: usize) -> (&mut TextPointer, &mut TextField) {
        (
            &mut self.text_pointer,
            &mut self.slots[self.current][i].field,
        )
    }

    fn apply_text(&mut self, i: usize, action: ContextAction) -> Outcome {
        let c = self.current;
        let row: &mut Row = &mut self.pages[c].items[i].row;
        apply(row, &mut self.slots[c][i].field, action)
    }

    fn other_motion(&mut self, at: (f32, f32), _: Mods) -> Outcome {
        if self.pointer_move(at.0, at.1) {
            Outcome::Redraw
        } else {
            Outcome::None
        }
    }

    fn other_cursor(&self, _: f32, _: f32) -> Option<Cursor> {
        None
    }
}
