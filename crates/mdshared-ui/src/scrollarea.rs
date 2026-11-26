//! A scroll area: one helper that ties a [`Viewport`] to both scrollbars, the clip, the wheel
//! and the keys, and that brings the focused control into view. It is the piece a dialog whose
//! body is taller than the window needs, and any pane, form or page of taller content.
//!
//! The application owns the rectangle, the content and the drawing:
//!
//! ```text
//! let rows = area.layout_column(view, &Flex::column().gap(8.0), &items);
//! for (i, r) in rows.iter().enumerate() {
//!     // Every control is offered each frame; the area reveals the focused one when it arrives.
//!     area.reveal_focus(&focus, i, (r.x as f64, r.y as f64), (r.w as f64, r.h as f64));
//! }
//! let saved = area.scrolled(&mut p, view);   // clip to `view` and move to content (0, 0)
//! for (r, item) in rows.iter().zip(&items) {
//!     draw_row(&mut p, *r, item);
//! }
//! saved.restore(&mut p);
//! draw_bars(&mut p, &pal, view, &area, hovered);
//! ```
//!
//! The offsets, the content's size and the view's size are kept in content pixels (logical
//! pixels in practice); every question and event answers in that one unit. The model draws
//! nothing and reads no clock, so a test moves it with numbers alone. Wheel deltas come in the
//! application's own unit, as [`host::Event::Wheel`](crate::host::Event::Wheel) gives them: a
//! notch away from the user (a positive `down`) scrolls up, so [`ScrollArea::wheel`] subtracts
//! both axes.
//!
//! The focus is passed in ([`ScrollArea::reveal_focus`]) rather than held: the id type is the
//! application's, as in [`Focus`]. `reveal` moves the offset at once, and `reveal_focus` only
//! when the focus arrives at a control, including coming back to one after the focus left the
//! area, so a wheel or a key is not undone on the next frame. An application that wants the
//! offset to glide keeps a
//! [`SmoothScroll`](crate::scroll::SmoothScroll) for each axis as [`viewport`] describes and
//! gives [`ScrollArea::set_offset`] what it draws each frame.
//!
//! Deliberately not here: drawing the body (each control has its own `draw_*`), hit testing
//! against the body (points go between the content and the screen with
//! [`ScrollArea::to_content`] and [`ScrollArea::to_area`]), a border or a background, the
//! wheel's own scaling (the caller turns notches into pixels), momentum, rubber-banding at the
//! ends, zoom, a thumb that is dragged (the application asks [`scroll::offset_for_thumb`] and
//! calls [`ScrollArea::set_offset`]), and accessibility of the content's place (roadmap K12).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::anim::{Animator, AsId};
use crate::focus::Focus;
use crate::gfx::{Painter, Rect};
use crate::keys::{KeyName, Mods};
use crate::layout::{Flex, Item};
use crate::scroll;
use crate::theme::Palette;
use crate::viewport::{Viewport, wheel_delta};

/// How far an arrow key and a line step move a scroll area, in content pixels. A page is the
/// view's length less this, so that a line of the last page stays in view.
pub const LINE: f64 = 24.0;

/// A scroll area's state: how large the content and the view are, and how far the view is
/// moved, all in content pixels. Build one, give it a rectangle each frame
/// ([`layout_column`](Self::layout_column), [`set_view`](Self::set_view),
/// [`set_content`](Self::set_content)), pass it the wheel, the keys and the focus, and draw
/// through [`scrolled`](Self::scrolled) and [`draw_bars`].
///
/// The offsets are always kept inside what the sizes allow, so a content that shrinks pulls
/// the view back in the moment it is told, and every answer is about the content as it is now.
/// A size that is not finite is 0, a negative one is 0, and a component of an offset or a wheel
/// that is not a number leaves that axis where it is.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct ScrollArea {
    viewport: Viewport,
    /// The hash of the focus (`Option<K>`) whose control was last revealed: `None` before the
    /// first reveal, and again once the focus is seen on something else. Kept so that the same
    /// focus is not revealed every frame.
    revealed: Option<u64>,
}

/// A number that is not negative, and 0 when it is not finite.
fn size(v: f64) -> f64 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

impl ScrollArea {
    /// An area with no content and no view, at the top left. Give it a view and a content before
    /// asking it anything.
    pub const fn new() -> ScrollArea {
        ScrollArea {
            viewport: Viewport {
                content: (0.0, 0.0),
                view: (0.0, 0.0),
                offset: (0.0, 0.0),
            },
            revealed: None,
        }
    }

    /// The size of everything there is to see, in content pixels.
    pub fn content(&self) -> (f64, f64) {
        self.viewport.content
    }

    /// The size of the window onto the content, in content pixels.
    pub fn view(&self) -> (f64, f64) {
        self.viewport.view
    }

    /// How far the view is moved from the content's top left, as it counts (clamped).
    pub fn offset(&self) -> (f64, f64) {
        self.viewport.at()
    }

    /// How far the view can move each way; 0 on an axis where the content fits.
    pub fn max(&self) -> (f64, f64) {
        self.viewport.max()
    }

    /// Whether the content is wider, and taller, than the view.
    pub fn scrolls(&self) -> (bool, bool) {
        self.viewport.scrolls()
    }

    /// The part of the content the view shows: its top left and its size, in content pixels.
    pub fn visible(&self) -> ((f64, f64), (f64, f64)) {
        self.viewport.visible()
    }

    /// The viewport this area keeps, for the questions it answers that are not repeated here
    /// (or for [`Viewport::bars`]).
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Tells the area the size of its window (its rectangle, less any border the caller keeps
    /// outside it), and pulls the offset inside the new limits. Returns whether the offset as it
    /// counts changed. A size that is negative or not finite is 0.
    pub fn set_view(&mut self, view: (f64, f64)) -> bool {
        let before = self.viewport.at();
        self.viewport.view = (size(view.0), size(view.1));
        self.pull_in(before)
    }

    /// Tells the area the size of its content, and pulls the offset inside the new limits.
    /// Returns whether the offset as it counts changed, which is what a content that shrank does
    /// while the same view stays. A size that is negative or not finite is 0.
    pub fn set_content(&mut self, content: (f64, f64)) -> bool {
        let before = self.viewport.at();
        self.viewport.content = (size(content.0), size(content.1));
        self.pull_in(before)
    }

    /// Puts the offset where `offset` says, kept inside the limits (also what an application
    /// gives after its [`SmoothScroll`](crate::scroll::SmoothScroll) glided). Returns whether the
    /// offset changed. A component that is not a number keeps that axis where it is.
    pub fn set_offset(&mut self, offset: (f64, f64)) -> bool {
        self.scroll_to(offset)
    }

    /// Moves the offset by `delta`, kept inside the limits. Returns whether it moved; a wheel
    /// that does not move the area is one for something else. A component that is not a number
    /// moves that axis not at all.
    pub fn scroll_by(&mut self, delta: (f64, f64)) -> bool {
        let step = |v: f64| if v.is_nan() { 0.0 } else { v };
        self.viewport.scroll_by((step(delta.0), step(delta.1)))
    }

    /// Moves the offset to `offset`, kept inside the limits. Returns whether it moved. A
    /// component that is not a number keeps that axis where it is.
    pub fn scroll_to(&mut self, offset: (f64, f64)) -> bool {
        let (x, y) = self.viewport.at();
        let keep = |v: f64, old: f64| if v.is_nan() { old } else { v };
        self.viewport
            .scroll_to((keep(offset.0, x), keep(offset.1, y)))
    }

    /// Takes a wheel's movement, `(across, down)` in the caller's unit, and scrolls by it: a
    /// notch away from the user is positive in [`host::Event::Wheel`](crate::host::Event::Wheel)
    /// and scrolls up, so both axes are subtracted. A wheel that only turns with Shift held
    /// scrolls across, as [`wheel_delta`] says (a notch away from the user then scrolls left).
    /// Returns whether the area moved.
    pub fn wheel(&mut self, across: f64, down: f64, shift: bool) -> bool {
        let (dx, dy) = wheel_delta(across, down, shift);
        self.scroll_by((-dx, -dy))
    }

    /// Takes a key that no control wanted and scrolls by it. Returns whether the area moved, so
    /// that the caller may pass an unhandled key on.
    ///
    /// Page Down and Space go down a page (the view's height less [`LINE`]); Page Up and
    /// Shift+Space go up one. Down and Up move a line, Right and Left a line across. Home and
    /// End go to the top and the bottom of the content, and with Ctrl to its top left and
    /// bottom right. Any key with Alt held is not a scroll key (it belongs to a menu), and so is
    /// every other key.
    pub fn key(&mut self, key: KeyName, mods: Mods) -> bool {
        if mods.alt {
            return false;
        }
        let (mx, my) = self.viewport.max();
        let page = (self.viewport.view.1 - LINE).max(LINE);
        let (x, y) = self.viewport.at();
        let to = match key {
            KeyName::PageDown => (x, y + page),
            KeyName::PageUp => (x, y - page),
            KeyName::Space if mods.shift => (x, y - page),
            KeyName::Space => (x, y + page),
            KeyName::Down => (x, y + LINE),
            KeyName::Up => (x, y - LINE),
            KeyName::Right => (x + LINE, y),
            KeyName::Left => (x - LINE, y),
            KeyName::Home if mods.ctrl => (0.0, 0.0),
            KeyName::End if mods.ctrl => (mx, my),
            KeyName::Home => (x, 0.0),
            KeyName::End => (x, my),
            _ => return false,
        };
        self.viewport.scroll_to(to)
    }

    /// Moves the offset the least that shows the content's rectangle at `at` of size `extent`
    /// whole, as [`Viewport::reveal`] does: one above or to the left goes to that edge, one
    /// below or to the right to the other, and one larger than the view shows its start. Returns
    /// whether the offset changed.
    pub fn reveal(&mut self, at: (f64, f64), extent: (f64, f64)) -> bool {
        let before = self.viewport.at();
        let to = self.viewport.reveal(at, extent);
        self.viewport.offset = to;
        to != before
    }

    /// Reveals the control with `id` when `focus` holds it there, the first time the focus comes
    /// to it: while the same control keeps the focus, giving it again moves nothing, so the wheel
    /// and the keys can scroll inside a control taller than the view instead of being pinned
    /// back. When the focus leaves for another control, the area sees it leave, and coming back
    /// to this one reveals it again.
    ///
    /// Call it every frame for each control the area lays out (or at least once a frame, with
    /// any `id`, while the focus is outside): the area compares [`Focus::focused`] with the
    /// focus it last revealed. That value is remembered as its hash, because [`ScrollArea`]
    /// holds no `K`, so `K` must be `Hash` as well as `Copy + Eq`.
    ///
    /// It does not reveal again after only a resize or a relayout of the same focus; call
    /// [`reveal`](Self::reveal) directly for that. `at` and `extent` are in content pixels, as
    /// [`layout_column`](Self::layout_column) and [`Flex::layout`] give them.
    pub fn reveal_focus<K: Copy + Eq + Hash>(
        &mut self,
        focus: &Focus<K>,
        id: K,
        at: (f64, f64),
        extent: (f64, f64),
    ) -> bool {
        let current = focus.focused();
        let mut h = DefaultHasher::new();
        current.hash(&mut h);
        let key = h.finish();
        if self.revealed == Some(key) {
            // Already revealed for this focus: the wheel and the keys are not undone.
            return false;
        }
        if current != Some(id) {
            // The focus is on another control (or nowhere): the area has seen it leave, so the
            // next time it comes to one of ours, that one is revealed again.
            self.revealed = None;
            return false;
        }
        self.revealed = Some(key);
        self.reveal(at, extent)
    }

    /// The rectangles of `items` laid out as a column as long as they want, and the area sized
    /// for it: the view is `area`'s size, the content is `area`'s width by
    /// [`Flex::natural`]'s height, and the rectangles are in content pixels with the content's
    /// top left at (0, 0).
    ///
    /// This is the dialog whose body is taller than its card: lay the controls out with
    /// `Flex::column()`, draw them through [`scrolled`](Self::scrolled), and the body scrolls
    /// rather than squeezing its flexible items into the view. The offset is kept where it was,
    /// pulled in when the column got shorter.
    pub fn layout_column(&mut self, area: Rect, flex: &Flex, items: &[Item]) -> Vec<Rect> {
        let height = flex.natural(items);
        // Both sizes are set before the offset is pulled in: a view that grew and a content
        // that grew in the same frame must not clamp the offset against the old content.
        let before = self.viewport.at();
        self.viewport.view = (size(f64::from(area.w)), size(f64::from(area.h)));
        self.viewport.content = (size(f64::from(area.w)), size(f64::from(height)));
        self.pull_in(before);
        flex.layout(Rect::new(0.0, 0.0, area.w, height), items)
    }

    /// Where the content's top left is on screen in `area`, and the content's whole size: the
    /// rectangle the content is drawn in, before the clip cuts it to `area`.
    pub fn content_rect(&self, area: Rect) -> Rect {
        let (x, y) = self.viewport.at();
        Rect::new(
            area.x - x as f32,
            area.y - y as f32,
            self.viewport.content.0 as f32,
            self.viewport.content.1 as f32,
        )
    }

    /// Where a point of the content is in `area` at the current offset.
    pub fn to_area(&self, area: Rect, point: (f64, f64)) -> (f32, f32) {
        self.viewport.to_area(area, point)
    }

    /// The point of the content under a point of `area`; `None` outside the area.
    pub fn to_content(&self, area: Rect, x: f32, y: f32) -> Option<(f64, f64)> {
        self.viewport.to_content(area, x, y)
    }

    /// Both scrollbars for `area`, the vertical first, each `None` when the content fits that
    /// way ([`Viewport::bars`]).
    #[allow(clippy::type_complexity)] // two optional (track, thumb) pairs
    pub fn bars(&self, area: Rect) -> (Option<(Rect, Rect)>, Option<(Rect, Rect)>) {
        self.viewport.bars(area)
    }

    /// Starts drawing the content: clips the painter to `area` and moves it so that content
    /// pixels are drawn directly, the content's top left at the area's top left when the offset
    /// is 0. Give the answer back to [`Scrolled::restore`] when the body is drawn.
    ///
    /// The clip is set before the move, so it stays the area; the move is rounded to whole
    /// device pixels, so the body stays as sharp scrolled as it is at rest. Sizes are drawn in
    /// content pixels; an offset beyond what an `f32` holds loses only what a screen cannot show.
    pub fn scrolled(&self, p: &mut Painter, area: Rect) -> Scrolled {
        let clip = p.clip(area);
        let (x, y) = self.viewport.at();
        let origin = p.offset(area.x - x as f32, area.y - y as f32);
        Scrolled { clip, origin }
    }

    /// Keeps the offset at `before`'s limits as they are now, and says whether it moved.
    fn pull_in(&mut self, before: (f64, f64)) -> bool {
        let after = self.viewport.at();
        self.viewport.offset = after;
        after != before
    }
}

/// What [`ScrollArea::scrolled`] changed in the painter, to be given back to [`restore`](Self::restore):
/// the clip and the origin as they were. `#[must_use]`: a painter left clipped and moved would
/// draw the rest of the frame into the wrong place.
#[derive(Clone, Copy, Debug)]
#[must_use = "give it back to Scrolled::restore, or the painter stays clipped and moved"]
pub struct Scrolled {
    clip: (i32, i32, i32, i32),
    origin: (i32, i32),
}

impl Scrolled {
    /// Puts the painter's origin and clip back as they were before
    /// [`ScrollArea::scrolled`].
    pub fn restore(self, p: &mut Painter) {
        p.restore_offset(self.origin);
        p.restore(self.clip);
    }
}

/// Draws both scrollbars of `area` over the body: the vertical one at its right, the horizontal
/// one along its bottom, each only when the content is longer than the view that way
/// ([`Viewport::bars`]). `active` is a bar being hovered or dragged, which draws it wider with a
/// faint track. Nothing is drawn when the content fits both ways.
pub fn draw_bars(p: &mut Painter, pal: &Palette, area: Rect, scroll: &ScrollArea, active: bool) {
    let (down, across) = scroll.bars(area);
    if let Some((track, thumb)) = down {
        scroll::draw_scrollbar(p, pal, track, thumb, active);
    }
    if let Some((track, thumb)) = across {
        scroll::draw_hscrollbar(p, pal, track, thumb, active);
    }
}

/// [`draw_bars`] with motion: each bar widens and its track fades in over a few frames when it
/// becomes `active`, and back. `id` names the area in `anim`; the two bars take `id` with
/// `"down"` and `"across"`. At rest, and whenever the animator does not animate, it draws exactly
/// what [`draw_bars`] draws.
pub fn draw_bars_animated(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    area: Rect,
    scroll: &ScrollArea,
    active: bool,
) {
    let id = id.as_id();
    let (down, across) = scroll.bars(area);
    if let Some((track, thumb)) = down {
        scroll::draw_scrollbar_animated(p, pal, anim, id.with("down"), track, thumb, active);
    }
    if let Some((track, thumb)) = across {
        scroll::draw_hscrollbar_animated(p, pal, anim, id.with("across"), track, thumb, active);
    }
}

#[cfg(test)]
#[path = "scrollarea_tests.rs"]
mod tests;
