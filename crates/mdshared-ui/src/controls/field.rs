//! A framed single-line text field, built on [`crate::textfield::TextField`]: the box, an
//! optional leading icon, a clear button while there is text, and an error state with a message
//! under the box. It owns the text, the caret and the selection through the field; the caller
//! owns the keyboard and the focus, as it does for the field itself.
//!
//! The caller passes every key to [`FramedField::key`] while the field has the focus: the editing
//! keys go to the [`TextField`], and Escape may clear the whole text when the caller has turned
//! that on ([`FramedField::set_escape_clears`]). It passes presses inside the box to
//! [`FramedField::press`], which answers whether the clear button took it, and it keeps the
//! focus on the field in either case: nothing here moves the focus, so clearing a field never
//! hands the keyboard away.
//!
//! Escape is **off** by default because it already means "close" in this toolkit's dialogs,
//! dropdowns, colour picker and command palette; a search field that wants Windows' clear-on-
//! Escape turns it on and an empty field's Escape still reaches the caller.
//!
//! Deliberately not here: the caret's blink ([`FramedField::draw_blinking`] takes it from an
//! [`Animator`](crate::anim::Animator), as [`TextField::draw`] does), the clipboard's contents
//! (the field reads it when `clipboard` is true, as it always has), an input method's preedit
//! (the field has it), the label or name above the box (the caller's), and the accessible nodes,
//! which the caller builds with [`crate::a11y::text_input`] and [`crate::a11y::button`].

use crate::anim::{Animator, AsId};
use crate::gfx::{Face, Painter, Rect, mix};
use crate::glyph;
use crate::keys::{KeyName, Mods};
use crate::textfield::{FieldColors, TextField};
use crate::theme::Palette;
use std::time::Duration;

/// The height of the box, in logical px. The error message adds [`ERROR_H`] under it.
pub const HEIGHT: f32 = 32.0;

/// The band an error message takes under the box, in logical px. [`FramedField::height`] adds it
/// only when there is a message, so a caller can lay out the rows below either way.
pub const ERROR_H: f32 = 18.0;

/// The room a leading icon takes at the left of the box, in logical px.
pub const ICON_W: f32 = 30.0;

/// The side of the clear button's square, in logical px. It is [`CLEAR_PAD`] from the right
/// edge, vertically centred.
pub const CLEAR: f32 = 22.0;

/// The room between the clear button and the box's right edge, in logical px.
pub const CLEAR_PAD: f32 = 4.0;

/// The room left of the text, and right of it when the field has no icon, in logical px. The
/// clear button's room is reserved whether or not it shows, so the text does not shift when the
/// first character is typed.
pub const PAD: f32 = 10.0;

/// The corner radius of the frame, in logical px.
pub const RADIUS: f32 = 5.0;

/// The size of the text, in logical px. The same as a button's label.
pub const TEXT: f32 = 13.0;

/// The size of the error message, in logical px.
pub const ERROR_TEXT: f32 = 12.0;

/// What a press inside the box hit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    /// The point is off the box (the error message's band is not the box).
    Outside,
    /// The text area: place the caret, as the field's own `press` does.
    Text,
    /// The clear button: the text was cleared; keep the focus where it was.
    Cleared,
}

/// What a key did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Not this field's key (Enter, Tab, Up, Down, a shortcut, or Escape with nothing to clear):
    /// the caller's.
    Unused,
    /// An editing key; `true` when the text changed, `false` when only the caret or the selection
    /// moved.
    Used(bool),
    /// Escape cleared the text ([`FramedField::set_escape_clears`]).
    Cleared,
}

/// A framed text field: the [`TextField`] that holds the text, an optional leading icon, a
/// placeholder and an error message. The icon is a path from [`crate::glyph`].
#[derive(Clone, Default)]
pub struct FramedField {
    field: TextField,
    icon: Option<&'static str>,
    placeholder: String,
    error: Option<String>,
    escape_clears: bool,
}

impl FramedField {
    /// An empty field with no icon, no placeholder, no error, and Escape left to the caller.
    pub fn new() -> FramedField {
        FramedField::default()
    }

    /// The same field with `icon` (a path from [`crate::glyph`]) before the text.
    pub fn with_icon(mut self, icon: &'static str) -> FramedField {
        self.icon = Some(icon);
        self
    }

    /// The same field with `text` drawn, in the placeholder colour, while the text is empty.
    pub fn with_placeholder(mut self, text: impl Into<String>) -> FramedField {
        self.placeholder = text.into();
        self
    }

    /// The text the field holds.
    pub fn text(&self) -> &str {
        self.field.text()
    }

    /// The field the text is kept in, for its caret, selection, preedit and secret flag.
    pub fn field(&self) -> &TextField {
        &self.field
    }

    /// The field, to change the text, the caret or the secret flag directly.
    pub fn field_mut(&mut self) -> &mut TextField {
        &mut self.field
    }

    /// Whether there is no text (so there is no clear button and the placeholder shows).
    pub fn is_empty(&self) -> bool {
        self.field.is_empty()
    }

    /// Empties the text, as the clear button does. `true` when there was text.
    pub fn clear(&mut self) -> bool {
        let had = !self.field.is_empty();
        self.field.clear();
        had
    }

    /// The icon before the text, if any.
    pub fn icon(&self) -> Option<&'static str> {
        self.icon
    }

    /// The placeholder drawn while the field is empty.
    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    /// Sets the placeholder drawn while the field is empty.
    pub fn set_placeholder(&mut self, text: impl Into<String>) {
        self.placeholder = text.into();
    }

    /// The error message shown under the box, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Sets the error message shown under the box. An error turns the frame the danger colour and
    /// draws the message under it, in the band [`FramedField::height`] makes room for.
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.error = Some(message.into());
    }

    /// Removes the error message, and the band under the box with it.
    pub fn clear_error(&mut self) {
        self.error = None;
    }

    /// Whether Escape clears the text; off by default (see the module doc).
    pub fn escape_clears(&self) -> bool {
        self.escape_clears
    }

    /// Turns Escape into "clear this field" (and leaves it to the caller when the field is
    /// already empty). Off by default because Escape closes dialogs and popups in this toolkit.
    pub fn set_escape_clears(&mut self, on: bool) {
        self.escape_clears = on;
    }

    /// The height the field wants: [`HEIGHT`], plus [`ERROR_H`] when there is an error message.
    pub fn height(&self) -> f32 {
        HEIGHT + if self.error.is_some() { ERROR_H } else { 0.0 }
    }

    /// Where the leading icon is drawn: [`ICON_W`] wide at the left of `r`, or `None` when the
    /// field has no icon.
    pub fn icon_rect(&self, r: Rect) -> Option<Rect> {
        self.icon.map(|_| Rect::new(r.x, r.y, ICON_W.min(r.w), r.h))
    }

    /// Where the text is drawn: beside the icon (or at [`PAD`] from the left) and short of the
    /// clear button's reserved room at the right. The same rectangle whether or not there is
    /// text, so the text does not shift when the clear button appears or goes.
    pub fn text_rect(&self, r: Rect) -> Rect {
        let left = if self.icon.is_some() { ICON_W } else { PAD };
        let right = PAD + CLEAR + CLEAR_PAD;
        Rect::new(r.x + left, r.y, (r.w - left - right).max(0.0), r.h)
    }

    /// The clear button's square, or `None` while the field is empty (it is not drawn then). The
    /// caller still leaves it out of its hits while the field is drawn disabled.
    pub fn clear_rect(&self, r: Rect) -> Option<Rect> {
        (!self.field.is_empty()).then(|| {
            Rect::new(
                r.right() - CLEAR_PAD - CLEAR,
                r.cy() - CLEAR / 2.0,
                CLEAR,
                CLEAR,
            )
        })
    }

    /// The band the error message is drawn in, under `r`, or `None` when there is no message.
    pub fn error_rect(&self, r: Rect) -> Option<Rect> {
        self.error
            .as_ref()
            .map(|_| Rect::new(r.x, r.bottom(), r.w, ERROR_H))
    }

    /// A press at `x`, `y` inside the box: on the clear button it empties the text and answers
    /// [`Hit::Cleared`]; elsewhere inside the box it answers [`Hit::Text`], and the caller places
    /// the caret with the field's own `press`. Off the box it answers [`Hit::Outside`] and
    /// changes nothing. The caller keeps the focus on the field for both `Text` and `Cleared`.
    pub fn press(&mut self, r: Rect, x: f32, y: f32) -> Hit {
        if !r.contains(x, y) {
            return Hit::Outside;
        }
        if let Some(c) = self.clear_rect(r)
            && c.contains(x, y)
        {
            self.field.clear();
            return Hit::Cleared;
        }
        Hit::Text
    }

    /// A key press; `typed` is the text the key event produced. Editing keys go to the
    /// [`TextField`] (`clipboard` lets Ctrl+C, Ctrl+X and Ctrl+V read and write the clipboard, as
    /// the field does). Escape clears the text when [`FramedField::set_escape_clears`] is on and
    /// there is text; with an empty field it is [`Outcome::Unused`], so the caller's own Escape
    /// still works. Everything the field does not take is [`Outcome::Unused`].
    pub fn key(
        &mut self,
        key: &KeyName,
        mods: Mods,
        typed: Option<&str>,
        clipboard: bool,
    ) -> Outcome {
        if self.escape_clears && !mods.ctrl && !mods.alt && *key == KeyName::Escape {
            return if self.clear() {
                Outcome::Cleared
            } else {
                Outcome::Unused
            };
        }
        match self.field.handle_key(key, mods, typed, clipboard) {
            Some(changed) => Outcome::Used(changed),
            None => Outcome::Unused,
        }
    }
}

/// How a framed field is drawn just now, from the caller's own state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Look {
    /// The keyboard is on it: the frame turns the accent colour and a thicker line runs along
    /// the bottom edge. A disabled field shows nothing of it.
    pub focused: bool,
    /// It cannot be used: the frame is the palette's line, the text and icon are faint, and the
    /// clear button is not drawn.
    pub disabled: bool,
    /// The pointer is over the clear button: it takes the hover fill and the text colour.
    pub hovered_clear: bool,
    /// A press on the clear button is held: the deeper fill.
    pub pressed_clear: bool,
}

/// Draws the field's box in `r`, its icon, its text (or placeholder), the clear button while
/// there is text, and the error message under the box in the band [`FramedField::height`] makes
/// room for. `caret_on` is [`TextField::draw`]'s: the caret is drawn only when the field is
/// focused and `caret_on`. It changes nothing in the field's text, but the field scrolls to keep
/// its caret in view, as [`TextField::draw`] does.
///
/// The frame is the palette's `field` with an edge mixed from the line and the text; focused, the
/// edge and a 2 px line along the bottom are the accent; with an error, both are the danger
/// colour (an error shows whatever the focus). The text is the placeholder colour while empty.
/// Nothing is drawn outside `r` except the error message under it.
pub fn draw(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    field: &mut FramedField,
    look: Look,
    caret_on: bool,
) {
    if !sane(r) {
        return;
    }
    let error = field.error.is_some();
    let edge = if look.disabled {
        pal.line
    } else if error {
        pal.danger
    } else if look.focused {
        pal.accent
    } else {
        mix(pal.line, pal.text, 0.3)
    };
    p.frame(r, RADIUS, pal.field, edge);
    if look.focused && !look.disabled {
        let px = 2.0f32.min((r.h / 2.0).max(0.0));
        p.fill(
            Rect::new(
                r.x + RADIUS.min(r.w / 2.0),
                (r.bottom() - 1.0 - px).max(r.y),
                (r.w - 2.0 * RADIUS.min(r.w / 2.0)).max(0.0),
                px,
            ),
            edge,
        );
    }
    if let Some(icon) = field.icon
        && let Some(ir) = field.icon_rect(r)
    {
        let ink = if look.disabled { pal.faint } else { pal.muted };
        p.icon(icon, 16.0, ir.cx(), ir.cy(), ink);
    }
    if let Some(c) = field.clear_rect(r)
        && !look.disabled
    {
        if look.pressed_clear {
            p.rbox(c, 4.0, pal.press);
        } else if look.hovered_clear {
            p.rbox(c, 4.0, pal.hover);
        }
        let ink = if look.hovered_clear || look.pressed_clear {
            pal.text
        } else {
            pal.muted
        };
        p.icon(glyph::CLOSE, 10.0, c.cx(), c.cy(), ink);
    }
    let colors = FieldColors {
        text: if look.disabled { pal.faint } else { pal.text },
        placeholder: if look.disabled { pal.faint } else { pal.muted },
        selection: pal.selection,
        caret: if look.disabled { pal.faint } else { pal.text },
    };
    let area = field.text_rect(r);
    let focused = look.focused && !look.disabled;
    let FramedField {
        field: tf,
        placeholder,
        ..
    } = &mut *field;
    tf.draw(
        p,
        area,
        Face::Ui,
        TEXT,
        &colors,
        placeholder,
        focused,
        caret_on,
    );
    if let Some(message) = field.error.as_deref() {
        let cy = r.bottom() + ERROR_H / 2.0;
        p.icon(glyph::WARN, 12.0, r.x + 7.0, cy, pal.danger);
        p.text(
            Face::Ui,
            ERROR_TEXT,
            r.x + 17.0,
            cy,
            message,
            pal.danger,
            (r.w - 17.0).max(0.0),
        );
    }
}

/// [`draw`] with a caret that blinks at the system's rate: `caret_on` is
/// [`TextField::blink_caret`] of the same `anim`, `id`, `blink` and `focused`. Draws exactly what
/// [`draw`] draws with that `caret_on`, and the loop waits for
/// [`Animator::end_frame`](crate::anim::Animator::end_frame), which wakes it only while the field
/// is focused and its caret blinks.
#[allow(clippy::too_many_arguments)] // drawing call: a rectangle, the model and its style
pub fn draw_blinking(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    blink: Option<Duration>,
    r: Rect,
    field: &mut FramedField,
    look: Look,
) {
    let caret_on = field
        .field
        .blink_caret(anim, id, blink, look.focused && !look.disabled);
    draw(p, pal, r, field, look, caret_on);
}

/// Whether a rectangle can be drawn at all: every number finite and both sides positive.
fn sane(r: Rect) -> bool {
    [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) && r.w > 0.0 && r.h > 0.0
}

#[cfg(test)]
#[path = "field_tests.rs"]
mod tests;
