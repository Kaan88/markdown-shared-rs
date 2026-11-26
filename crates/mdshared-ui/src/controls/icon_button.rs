//! An icon button: a square button that shows one glyph from [`crate::glyph`] and carries the
//! text of its tooltip. It is drawn as a subtle [`crate::controls::button`] (no fill until the
//! pointer is over it), so a toolbar of them reads as one row.
//!
//! The tooltip's timing is [`crate::tooltip`]'s and its drawing is
//! [`crate::overlay::Bubble::tip`]; [`IconButton::over`] builds the [`Over`] a caller passes to
//! [`Tooltip::hover`](crate::tooltip::Tooltip::hover) with the button's own text and rectangle.
//! Nothing here reads a clock.
//!
//! Deliberately not here: the tooltip's delay and its drawing, the pointer's tracking, the
//! keyboard focus (the caller passes [`Look`](crate::controls::button::Look) flags) and the
//! accessible node, which the caller builds with [`crate::a11y::button`], naming it with the
//! tooltip's text.

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

/// An icon button: the glyph it shows and the text of its tooltip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconButton {
    icon: &'static str,
    tip: String,
}

impl IconButton {
    /// A button with `icon` (a path from [`crate::glyph`]) and no tooltip text.
    pub fn new(icon: &'static str) -> IconButton {
        IconButton {
            icon,
            tip: String::new(),
        }
    }

    /// The same button with `text` as its tooltip (and, for a screen reader, its name).
    pub fn tip(mut self, text: impl Into<String>) -> IconButton {
        self.tip = text.into();
        self
    }

    /// The glyph the button shows.
    pub fn icon(&self) -> &'static str {
        self.icon
    }

    /// The tooltip's text ("" for none).
    pub fn tip_text(&self) -> &str {
        &self.tip
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
    /// Ctrl or Alt.
    pub fn key(&self, k: &KeyName, mods: Mods) -> bool {
        button::key(k, mods)
    }

    /// The tooltip this button shows while the pointer rests on it: its key (the caller's) as
    /// `key`, its text, and its rectangle as the anchor. Pass it to
    /// [`Tooltip::hover`](crate::tooltip::Tooltip::hover) or
    /// [`hover_at`](crate::tooltip::Tooltip::hover_at); draw the tip with
    /// [`Bubble::tip`](crate::overlay::Bubble::tip) or, for its fade,
    /// [`Bubble::draw_presence`](crate::overlay::Bubble::draw_presence). It is built even when
    /// the text is empty, so the caller's own `Option` decides whether a tip exists.
    pub fn over<K>(&self, key: K, r: Rect) -> Over<K> {
        Over {
            key,
            text: self.tip.clone(),
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
