//! A text button that looks like a link: the accent colour, an underline while the pointer is
//! over it, the hand cursor, and the focus ring when the keyboard is on it. The model is just
//! the text and the questions a caller lays it out with; the drawing is a free function that
//! changes nothing.
//!
//! The caller owns the events, the hit, the pointer and the keyboard. It asks [`hit`] whether a
//! press belongs to the link, draws with the flags it knows ([`Look`]), and calls [`key`] with
//! the key it received: Enter or Space answers `true` and the caller does the link's work. A
//! disabled link is drawn quiet, and the caller leaves it out of its own hits, as it does with
//! every other disabled control.
//!
//! Deliberately not here: opening the address, which is the application's own business; motion
//! (the underline and the colours are there on the frame they are drawn, and no frame is asked
//! for); the pointer's tracking and the focus model, which the caller runs and passes in as
//! [`Look`] flags; a hover delay; and the accessible name and role. The later accessibility
//! layer (roadmap W8) will report the role hyperlink or button with Invoke, the link's text as
//! its name, and [`link_rect`]'s rectangle as its bounds.

use crate::cursor::Cursor;
use crate::focus::draw_ring;
use crate::gfx::{Face, Painter, Rect, mix};
use crate::keys::{KeyName, Mods};
use crate::theme::Palette;

/// The height a link wants: the line of text with a little room above and below. A caller that
/// stacks links can step by this. It is not a minimum: a shorter rectangle still draws its text
/// centred in it.
pub const LINK_H: f32 = 22.0;

/// The size of a link's text, in logical px. The same size as a dialog button's label and the
/// interface's ordinary text.
pub const LINK_TEXT: f32 = 13.0;

/// The corner radius of the focus ring around a link, in logical px. It is small because the
/// link itself has no frame: the ring follows the line of text, not a button's box.
const RADIUS: f32 = 3.0;

/// How much of the text colour a pressed link mixes into the accent: enough to read as a second
/// state without turning the link into ordinary text.
const PRESS: f32 = 0.3;

/// A link: its text, and nothing else. The caller keeps one beside the data the link belongs
/// to, asks it for the rectangle it needs, and passes the text to [`draw_link`].
///
/// The text is kept as it was given, spaces and all; measuring and drawing do not trim it. An
/// empty link measures zero wide and draws nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Link {
    text: String,
}

impl Link {
    /// A link that shows `text`. Anything that becomes a `String` will do, as with the other
    /// models here.
    pub fn new(text: impl Into<String>) -> Link {
        Link { text: text.into() }
    }

    /// The text the link shows.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The width of this link's text, in logical px, in the face and size [`draw_link`] uses.
    /// An empty link is zero wide. A painter is needed because the width depends on the fonts
    /// the window has.
    pub fn measure(&self, p: &mut Painter) -> f32 {
        measure(p, &self.text)
    }

    /// The rectangle this link wants with its top left at `x`, `y`: as wide as its measured
    /// text and [`LINK_H`] high. The text is centred vertically in it.
    pub fn rect(&self, p: &mut Painter, x: f32, y: f32) -> Rect {
        link_rect(p, x, y, &self.text)
    }

    /// Whether a point is on this link, by [`hit`](crate::link::hit): the rectangle answers, not
    /// the text's own pixels, so the whole line is easy to press.
    pub fn hit(&self, r: Rect, x: f32, y: f32) -> bool {
        hit(r, x, y)
    }

    /// The cursor to show over this link: [`Cursor::Hand`], always. A disabled link is left out
    /// of the caller's hits, so this is not asked for it.
    pub fn cursor(&self) -> Cursor {
        cursor()
    }

    /// What this link does with a key, by [`key`](crate::link::key): `true` for Enter and Space
    /// with neither Ctrl nor Alt held, `false` for everything else.
    pub fn key(&self, k: KeyName, mods: Mods) -> bool {
        key(k, mods)
    }
}

/// The width of `text` in the face and size a link draws it, in logical px. Empty text is zero
/// wide. Trailing spaces are not counted (the painter measures the laid-out line, as parley's
/// `Layout::width` does), so `"Open "` is as wide as `"Open"`.
pub fn measure(p: &mut Painter, text: &str) -> f32 {
    p.measure(Face::Ui, LINK_TEXT, text)
}

/// The rectangle a link of `text` wants with its top left at `x`, `y`: as wide as the measured
/// text and [`LINK_H`] high. The numbers are passed through, including a NaN or an infinite
/// `x` or `y`; drawing such a rectangle draws nothing, and a hit on it is false.
pub fn link_rect(p: &mut Painter, x: f32, y: f32, text: &str) -> Rect {
    Rect::new(x, y, measure(p, text), LINK_H)
}

/// Whether the point `x`, `y` is on the link in `r`. The left and top edges belong to the link
/// and the right and bottom edges do not, as with [`Rect::contains`](crate::gfx::Rect::contains).
/// NaN and infinite points are outside it.
pub fn hit(r: Rect, x: f32, y: f32) -> bool {
    r.contains(x, y)
}

/// The cursor a link asks for: the hand, which the system draws as a pointer over something a
/// click activates.
pub fn cursor() -> Cursor {
    Cursor::Hand
}

/// Whether the key activates a link: Enter and Space do, with or without Shift, when neither Ctrl
/// nor Alt is held (as `toggle` and `dropdown` read them); every other key does not. The caller
/// decides whether the keyboard is on the link at all, and leaves a disabled link out of the keys
/// as it leaves it out of the hits; this only says what the activation keys are.
pub fn key(key: KeyName, mods: Mods) -> bool {
    !mods.ctrl && !mods.alt && matches!(key, KeyName::Enter | KeyName::Space)
}

/// How a link is drawn just now, from the caller's own state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Look {
    /// The pointer is over it: the text is underlined.
    pub hovered: bool,
    /// A press that started on the link is held: the underline stays and the accent deepens.
    pub pressed: bool,
    /// The keyboard is on it: a focus ring lies just outside the rectangle.
    pub focused: bool,
    /// It cannot be used: drawn in the faint colour, with no underline, ring or press look.
    pub disabled: bool,
}

/// Draws the link `text` in `r` with the colours and marks `look` asks for, and changes nothing.
///
/// At rest the text is the accent colour, which is the colour the toolkit uses for links. With
/// the pointer over it the text is underlined. Pressed, the accent is mixed a little towards the
/// text colour and the underline stays. Focused, [`draw_ring`] draws the accent ring one pixel
/// outside the rectangle, so it reaches three pixels out. Disabled, the text is the faint colour
/// and the underline and ring are left out even when the other flags are set: no mark promises
/// something the link cannot do.
///
/// Text wider than `r` is elided with an ellipsis at `r`'s width, and the underline covers what
/// is shown. An empty text, a rectangle with a side that is not positive or not finite, or one
/// whose numbers are not finite draws nothing at all. Nothing is drawn outside `r` except the
/// focus ring of a focused link.
pub fn draw_link(p: &mut Painter, pal: &Palette, r: Rect, text: &str, look: Look) {
    if text.is_empty() || [r.x, r.y, r.w, r.h].iter().any(|v| !v.is_finite()) {
        return;
    }
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    let colour = if look.disabled {
        pal.faint
    } else if look.pressed {
        mix(pal.accent, pal.text, PRESS)
    } else {
        pal.accent
    };
    if look.focused && !look.disabled {
        draw_ring(p, pal, r, RADIUS);
    }
    // What fits is drawn as it is; what does not is shortened once, so that the underline can be
    // drawn under exactly the characters on screen. The clip keeps an ellipsis that is itself
    // wider than a very narrow `r` inside it.
    let saved = p.clip(r);
    if p.measure(Face::Ui, LINK_TEXT, text) > r.w {
        let shown = p.elide(Face::Ui, LINK_TEXT, text, r.w);
        p.text(Face::Ui, LINK_TEXT, r.x, r.cy(), &shown, colour, r.w);
        if !look.disabled && (look.hovered || look.pressed) {
            p.underline_range(
                Face::Ui,
                LINK_TEXT,
                r.x,
                r.cy(),
                &shown,
                0..shown.len(),
                colour,
            );
        }
    } else {
        p.text(Face::Ui, LINK_TEXT, r.x, r.cy(), text, colour, r.w);
        if !look.disabled && (look.hovered || look.pressed) {
            p.underline_range(
                Face::Ui,
                LINK_TEXT,
                r.x,
                r.cy(),
                text,
                0..text.len(),
                colour,
            );
        }
    }
    p.restore(saved);
}

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;
