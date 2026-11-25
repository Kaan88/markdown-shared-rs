//! An expander: a header with a chevron and a title, and a body that opens and closes under it.
//!
//! The model is one flag. [`Expander`] holds whether the body is open, toggles it on a click or
//! on Space or Enter (with neither Ctrl nor Alt held), and answers whether a point is on the
//! header. The height of the body is asked of the [`Animator`], which moves it between 0 and the
//! height the caller measured, and stops asking for frames once it has arrived. Under reduced
//! motion the animator jumps to the end, so the same calls need no special case.
//!
//! The caller owns the events and the content: it passes a press or a key to [`Expander`], draws
//! the body itself inside the rectangle [`draw_expander_animated`] returns, and lays out what
//! comes after it at [`height`]. Nothing here records a hit, calls back, or reads a clock.
//!
//! A disabled expander is drawn quiet with [`Look::disabled`]; the caller must leave it out of
//! its hits and its keys as well, because [`Expander::header_hit`] and [`Expander::key`] know
//! nothing of the `Look`.
//!
//! The chevron points right while closed and down while open. [`draw_header`] and
//! [`draw_expander_animated`] swap the two glyphs; [`draw_expander_turning`], the opt-in
//! animated draw, turns the chevron between them as the body opens and closes (the tree's
//! disclosure, `glyph::draw_disclosure`), and at rest is the same pixels as the swap.
//!
//! Deliberately not here: the body's content (the caller draws it); the header's icons besides
//! the chevron; a turning chevron in `draw_header` and `draw_expander_animated` (they keep the
//! swap, so what they draw does not change); and the accessibility layer.
//! For a screen reader the caller builds the header with `a11y::expander` (feature `a11y`), a
//! button that expands or collapses; [`Expander::expanded`] is what maps to UIA's
//! ExpandCollapse state.

use crate::anim::{Animator, AsId, Id, fluent};
use crate::focus::draw_ring;
use crate::gfx::{Face, Painter, Rect};
use crate::glyph;
use crate::keys::{KeyName, Mods};
use crate::theme::Palette;

/// The height of the header, and the room the chevron and title have.
pub const HEADER_H: f32 = 32.0;
/// The space from the header's left edge to the chevron.
const PAD: f32 = 12.0;
/// The chevron's square, and the gap between it and the title.
const CHEVRON: f32 = 16.0;
const GAP: f32 = 8.0;
/// The corner radius of the header's hover and focus fill.
const RADIUS: f32 = 6.0;
/// The size of the title.
const TITLE: f32 = 12.5;

/// Whether an expander's body is open, and the events that change it. The body itself and its
/// measured height stay with the caller.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Expander {
    /// The body is open.
    pub expanded: bool,
}

impl Expander {
    /// An expander whose body is open or closed.
    pub fn new(expanded: bool) -> Expander {
        Expander { expanded }
    }

    /// Opens a closed body or closes an open one, and answers the new state.
    pub fn toggle(&mut self) -> bool {
        self.expanded = !self.expanded;
        self.expanded
    }

    /// Opens or closes the body, and answers whether that changed anything.
    pub fn set(&mut self, expanded: bool) -> bool {
        let changed = self.expanded != expanded;
        self.expanded = expanded;
        changed
    }

    /// Whether `x, y` lies on `header`. A point on the header's right or bottom edge is outside,
    /// as for every rectangle here; a rectangle or a point that is not a number is never hit.
    /// This does not know whether the expander is disabled: the caller leaves a disabled one out
    /// of its hits.
    pub fn header_hit(&self, header: Rect, x: f32, y: f32) -> bool {
        if [header.x, header.y, header.w, header.h, x, y]
            .iter()
            .any(|v| !v.is_finite())
        {
            return false;
        }
        header.w > 0.0 && header.h > 0.0 && header.contains(x, y)
    }

    /// Gives `key` to the expander: Space or Enter, with neither Ctrl nor Alt held, toggles the
    /// body and answers true. Every other key changes nothing and answers false, so the caller
    /// can pass keys on to the application's shortcuts. This does not know whether the expander
    /// is disabled: the caller leaves a disabled one out of its keys.
    pub fn key(&mut self, key: &KeyName, mods: Mods) -> bool {
        if mods.ctrl || mods.alt || !matches!(key, KeyName::Space | KeyName::Enter) {
            return false;
        }
        self.toggle();
        true
    }
}

/// The header's rectangle at the top of `area`: the full width and [`HEADER_H`] tall, or as tall
/// as `area` when that is shorter. A width that is not a number is taken as zero; a height that
/// is not a number is taken as [`HEADER_H`].
pub fn header_rect(area: Rect) -> Rect {
    let x = if area.x.is_finite() { area.x } else { 0.0 };
    let y = if area.y.is_finite() { area.y } else { 0.0 };
    let w = if area.w.is_finite() {
        area.w.max(0.0)
    } else {
        0.0
    };
    let h = if area.h.is_finite() {
        area.h.clamp(0.0, HEADER_H)
    } else {
        HEADER_H
    };
    Rect::new(x, y, w, h)
}

/// How much of the body is visible now: from 0 while closed to `full_h` while open, entering
/// at the Fluent pace for something appearing and leaving at the pace for something going away.
/// `id` names this expander in `anim` (the body's height is a value under that name, so the
/// same id can carry the widget's other values); `full_h` is the height the caller measured,
/// and a height that is negative or not a number is taken as zero. Under reduced motion the
/// answer is 0 or `full_h` at once, as the animator makes every value its target.
pub fn body_height(anim: &mut Animator, id: impl AsId, expanded: bool, full_h: f32) -> f32 {
    shown_height(anim, body_id(id), expanded, full_h)
}

/// The height the whole expander takes in a layout: the header, plus as much of the body as is
/// showing. `full_h` is the body's measured height.
pub fn height(anim: &mut Animator, id: impl AsId, expanded: bool, full_h: f32) -> f32 {
    HEADER_H + shown_height(anim, body_id(id), expanded, full_h)
}

/// The animated value lives under a part of the expander's name, so a widget that uses its own
/// name for other values does not collide with the body's height.
fn body_id(id: impl AsId) -> Id {
    Id::new(id).with("body")
}

fn shown_height(anim: &mut Animator, id: Id, expanded: bool, full_h: f32) -> f32 {
    let full_h = if full_h.is_finite() {
        full_h.max(0.0)
    } else {
        0.0
    };
    let shown = anim.toggle(
        id,
        expanded,
        if expanded {
            fluent::ENTER
        } else {
            fluent::EXIT
        },
    );
    let h = shown * full_h;
    if h.is_finite() {
        h.clamp(0.0, full_h)
    } else {
        full_h
    }
}

/// What an expander's header looks like just now, from the caller's own state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Look {
    /// The pointer is over the header.
    pub hovered: bool,
    /// The header is held down.
    pub pressed: bool,
    /// The keyboard is on the header; a ring follows its edge.
    pub focused: bool,
    /// It cannot be changed: drawn quiet, with no hover, press or focus mark.
    pub disabled: bool,
}

/// Draws the header in `rect`: the chevron pointing right while closed and down while open, then
/// the title. The pointer gives it a hover or press fill, the keyboard a ring inside its edge,
/// and a disabled header is quiet. At rest, and when it is disabled, the header draws no
/// background: whatever is behind it (a page, a pane or a popup) shows through. It draws nothing
/// outside `rect` and changes no state. A rectangle with a side that is not positive or not
/// finite draws nothing.
pub fn draw_header(
    p: &mut Painter,
    pal: &Palette,
    rect: Rect,
    title: &str,
    expanded: bool,
    look: Look,
) {
    header_turned(p, pal, rect, title, if expanded { 1.0 } else { 0.0 }, look);
}

/// [`draw_header`] with its chevron `open` of the way through its turn (0 right, 1 down; see
/// [`glyph::draw_disclosure`]). At 0 and 1 the chevron is the plain right and down glyph, so
/// `draw_header` is this at 0 or 1.
fn header_turned(p: &mut Painter, pal: &Palette, rect: Rect, title: &str, open: f32, look: Look) {
    if [rect.x, rect.y, rect.w, rect.h]
        .iter()
        .any(|v| !v.is_finite())
        || rect.w <= 0.0
        || rect.h <= 0.0
    {
        return;
    }
    let ink = if look.disabled {
        pal.faint
    } else if look.pressed || look.hovered {
        pal.text
    } else {
        pal.muted
    };
    let fill = if look.disabled {
        None
    } else if look.pressed {
        Some(pal.press)
    } else if look.hovered {
        Some(pal.hover)
    } else {
        None
    };
    if let Some(fill) = fill {
        p.rbox(rect, RADIUS, fill);
    }
    if look.focused && !look.disabled {
        // Two px in, so the line that starts one px outside the rectangle it is given stays
        // inside the header: a ring that is cut off by the header's own edge would look broken.
        draw_ring(p, pal, rect.inset(2.0), RADIUS - 2.0);
    }
    let cx = rect.x + PAD + CHEVRON / 2.0;
    glyph::draw_disclosure(p, cx, rect.cy(), CHEVRON, open, ink);
    let x = rect.x + PAD + CHEVRON + GAP;
    let max = rect.right() - PAD - x;
    if max > 0.0 {
        p.text(Face::Semibold, TITLE, x, rect.cy(), title, ink, max);
    }
}

/// Draws the header and answers the rectangle the body should be drawn into this frame: the
/// header's width, under the header that was drawn (shorter than [`HEADER_H`] when the area is),
/// as tall as [`body_height`] says. The caller passes that rectangle to
/// [`Painter::clip`](crate::gfx::Painter::clip), draws the body, and restores the clip. The
/// rectangle never reaches past the bottom of `area`, and is empty at the top of a body that is
/// closed. `id` names this expander in `anim`, as in [`body_height`].
#[allow(clippy::too_many_arguments)] // a widget call: area, label, state and the body's height
pub fn draw_expander_animated(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    area: Rect,
    title: &str,
    expanded: bool,
    look: Look,
    full_h: f32,
) -> Rect {
    let open = if expanded { 1.0 } else { 0.0 };
    expander_turned(p, pal, anim, id, area, title, expanded, open, look, full_h)
}

/// [`draw_expander_animated`] whose chevron also turns, from right to down as the body opens
/// and back as it closes, by [`glyph::disclosure_turn`] under the expander's `id`. At rest it
/// is `draw_expander_animated` pixel for pixel (the plain right or down chevron), and it
/// answers the same rectangle for the body. The turn takes as long as the body's height (167
/// ms each way); under reduced motion both are at their end at once. Opt in by calling this
/// instead of `draw_expander_animated`.
#[allow(clippy::too_many_arguments)] // a widget call: area, label, state and the body's height
pub fn draw_expander_turning(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    area: Rect,
    title: &str,
    expanded: bool,
    look: Look,
    full_h: f32,
) -> Rect {
    let id = Id::new(id);
    let open = glyph::disclosure_turn(anim, id, expanded);
    expander_turned(p, pal, anim, id, area, title, expanded, open, look, full_h)
}

#[allow(clippy::too_many_arguments)] // the two public draws above, with the chevron's turn
fn expander_turned(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    area: Rect,
    title: &str,
    expanded: bool,
    open: f32,
    look: Look,
    full_h: f32,
) -> Rect {
    let id = Id::new(id);
    let shown = shown_height(anim, body_id(id), expanded, full_h);
    let head = header_rect(area);
    header_turned(p, pal, head, title, open, look);
    // Under the header that was actually drawn, which is shorter than HEADER_H when the area is.
    let room = if area.h.is_finite() {
        (area.bottom() - head.bottom()).max(0.0)
    } else {
        0.0
    };
    let h = shown.min(room);
    Rect::new(head.x, head.bottom(), head.w, h)
}

#[cfg(test)]
#[path = "expander_tests.rs"]
mod tests;
