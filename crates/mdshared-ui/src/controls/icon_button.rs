//! An icon button: a square button that shows one glyph from [`crate::glyph`] and carries its
//! name, which is also the text of its tooltip. It is drawn as a subtle
//! [`crate::controls::button`] (no fill until the pointer is over it), so a toolbar of them
//! reads as one row.
//!
//! The name is required rather than optional: a button with only a glyph has no other name for
//! a screen reader, so [`IconButton::new`] takes it, [`IconButton::name`] gives it back, and
//! [`IconButton::over`] puts it in the tooltip. An empty name is allowed but is a poor one; a
//! caller with nothing to say about a square of pixels has the wrong control.
//!
//! The tooltip's timing is [`crate::tooltip`]'s and its drawing is
//! [`crate::overlay::Bubble::tip`]; [`IconButton::over`] builds the [`Over`] a caller passes to
//! [`Tooltip::hover`](crate::tooltip::Tooltip::hover) with the button's own text and rectangle.
//! Nothing here reads a clock.
//!
//! Deliberately not here: the tooltip's delay and its drawing, the pointer's tracking, the
//! keyboard focus (the caller passes [`Look`](crate::controls::button::Look) flags) and the
//! accessible node, which the caller builds with [`crate::a11y::button`], naming it with
//! [`IconButton::name`].

use crate::gfx::{Painter, Rect};
use crate::keys::{KeyName, Mods};
use crate::theme::Palette;
use crate::tooltip::Over;

use super::button::{self, Kind, Look};

/// The side of an icon button's square, in logical px. The same as a push button's height, so a
/// row of both lines up.
pub const SIZE: f32 = button::HEIGHT;

/// The size of the glyph, in logical px.
pub const ICON: f32 = 16.0;

/// An icon button: the glyph it shows and its name (which is its tooltip's text too).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconButton {
    icon: &'static str,
    name: String,
}

impl IconButton {
    /// A button with `icon` (a path from [`crate::glyph`]), named `name` for a tooltip and a
    /// screen reader alike. An empty name is kept as it is given.
    pub fn new(icon: &'static str, name: impl Into<String>) -> IconButton {
        IconButton {
            icon,
            name: name.into(),
        }
    }

    /// The glyph the button shows.
    pub fn icon(&self) -> &'static str {
        self.icon
    }

    /// The button's name: its tooltip's text, and the name a screen reader announces.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The button's square with its top left at `x`, `y`: [`SIZE`] on each side.
    pub fn rect(&self, x: f32, y: f32) -> Rect {
        rect(x, y)
    }

    /// Whether a point is on this button, by [`button::hit`].
    pub fn hit(&self, r: Rect, x: f32, y: f32) -> bool {
        button::hit(r, x, y)
    }

    /// What this button does with a key, by [`button::key`]: `true` for Enter and Space without
    /// Ctrl, Alt or the Mac's Control. A repeat is the caller's to ignore.
    pub fn key(&self, k: &KeyName, mods: Mods) -> bool {
        button::key(k, mods)
    }

    /// The tooltip this button shows while the pointer rests on it: its key (the caller's) as
    /// `key`, its name as the text, and its rectangle as the anchor. Pass it to
    /// [`Tooltip::hover`](crate::tooltip::Tooltip::hover) or
    /// [`hover_at`](crate::tooltip::Tooltip::hover_at); draw the tip with
    /// [`Bubble::tip`](crate::overlay::Bubble::tip) or, for its fade,
    /// [`Bubble::draw_presence`](crate::overlay::Bubble::draw_presence). An empty name gives an
    /// empty tip, which a caller may choose not to show.
    pub fn over<K>(&self, key: K, r: Rect) -> Over<K> {
        Over {
            key,
            text: self.name.clone(),
            anchor: r,
        }
    }
}

/// The square an icon button wants with its top left at `x`, `y`: [`SIZE`] on each side.
pub fn rect(x: f32, y: f32) -> Rect {
    Rect::new(x, y, SIZE, SIZE)
}

/// Draws `icon` centred in `r`, over the subtle button box that `look` asks for, and changes
/// nothing. Disabled, it is the quiet field frame with a faint glyph, as every other disabled
/// button is; focused, the ring lies outside `r`.
pub fn draw(p: &mut Painter, pal: &Palette, r: Rect, icon: &str, look: Look) {
    let (_, ink) = button::draw_box(p, pal, r, Kind::Subtle, look);
    if [r.x, r.y, r.w, r.h].iter().any(|v| !v.is_finite()) || r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    p.icon(icon, ICON, r.cx(), r.cy(), ink);
}

#[cfg(test)]
#[path = "icon_button_tests.rs"]
mod tests;
