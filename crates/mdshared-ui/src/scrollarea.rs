//! A scroll area: one helper that ties a [`Viewport`] to both scrollbars, the clip, the wheel
//! and the keys, and that brings the focused control into view. It is the piece a dialog whose
//! body is taller than the window needs, and any pane, form or page of taller content.
//!
//! The application owns the rectangle, the content and the drawing:
//!
//! ```text
//! let rows = area.layout_column(view, &Flex::column().gap(8.0), &items);
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
//! when the focus has come to another control, so a wheel or a key is not undone on the next
//! frame. An application that wants the offset to glide keeps a
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
    /// The hash of the focus id [`reveal_focus`](ScrollArea::reveal_focus) last brought into
    /// view; `None` before the first. Kept so that giving the same focus again moves nothing.
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
    /// to it: once that id has been revealed, giving it again while `focus` still holds it moves
    /// nothing, so the wheel and the keys can scroll inside a control taller than the view
    /// instead of being pinned back on the next frame. Another id reveals again. Returns whether
    /// the offset changed.
    ///
    /// `at` and `extent` are in content pixels, as [`layout_column`](Self::layout_column) and
    /// [`Flex::layout`] give them. The id is remembered as its hash, because [`ScrollArea`]
    /// holds no `K`; so `K` must be `Hash` as well as `Copy + Eq`.
    pub fn reveal_focus<K: Copy + Eq + Hash>(
        &mut self,
        focus: &Focus<K>,
        id: K,
        at: (f64, f64),
        extent: (f64, f64),
    ) -> bool {
        if !focus.is_focused(id) {
            return false;
        }
        let mut h = DefaultHasher::new();
        id.hash(&mut h);
        let key = h.finish();
        if self.revealed == Some(key) {
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
mod tests {
    use super::*;
    use crate::anim::{Clock, ManualClock};
    use crate::gfx::{Gfx, Rgb};
    use crate::theme::{DARK, LIGHT, Palette as Colors};
    use mdshared_text::Bitmap;

    const VIEW: (f64, f64) = (400.0, 200.0);

    fn area() -> ScrollArea {
        let mut a = ScrollArea::new();
        a.set_view(VIEW);
        a.set_content((1000.0, 2000.0));
        a
    }

    #[test]
    fn content_shorter_than_the_view_does_not_scroll() {
        let mut a = ScrollArea::new();
        assert!(!a.set_view(VIEW));
        assert!(!a.set_content((80.0, 60.0)));
        assert_eq!(a.max(), (0.0, 0.0));
        assert_eq!(a.scrolls(), (false, false));
        assert_eq!(a.offset(), (0.0, 0.0));
        assert!(!a.scroll_by((30.0, 30.0)));
        assert!(!a.wheel(0.0, 120.0, false));
        assert!(!a.key(KeyName::PageDown, Mods::default()));
        assert_eq!(a.bars(Rect::new(0.0, 0.0, 400.0, 200.0)), (None, None));
        // The whole content is in view, and no more.
        assert_eq!(a.visible(), ((0.0, 0.0), (80.0, 60.0)));
    }

    #[test]
    fn content_exactly_the_size_of_the_view_does_not_scroll_or_show_bars() {
        let mut a = ScrollArea::new();
        a.set_view(VIEW);
        a.set_content(VIEW);
        assert_eq!(a.max(), (0.0, 0.0));
        assert_eq!(a.scrolls(), (false, false));
        assert!(!a.set_offset((10.0, 10.0)));
        assert_eq!(a.offset(), (0.0, 0.0));
        assert_eq!(a.bars(Rect::new(0.0, 0.0, 400.0, 200.0)), (None, None));
        // A pixel over either way is a bar's worth of scroll.
        assert!(!a.set_content((400.0, 201.0)));
        assert_eq!(a.scrolls(), (false, true));
        assert!(!a.set_content((401.0, 201.0)));
        assert_eq!(a.scrolls(), (true, true));
    }

    #[test]
    fn content_that_shrinks_pulls_the_offset_back_in() {
        let mut a = area();
        assert!(a.set_offset((900.0, 1700.0)));
        assert_eq!(a.offset(), (600.0, 1700.0));
        // The content is now 500 by 400: the furthest offsets are 100 and 200.
        assert!(a.set_content((500.0, 400.0)));
        assert_eq!(a.offset(), (100.0, 200.0));
        // Told the same size again, nothing moves.
        assert!(!a.set_content((500.0, 400.0)));
        // A view grown past the content pulls it to the top left.
        assert!(a.set_view((900.0, 900.0)));
        assert_eq!(a.max(), (0.0, 0.0));
        assert_eq!(a.offset(), (0.0, 0.0));
    }

    #[test]
    fn a_wheel_notch_away_from_the_user_scrolls_up_and_shift_turns_it_across() {
        let mut a = area();
        a.set_offset((100.0, 200.0));
        // `host::Event::Wheel` gives a notch away from the user as positive: it scrolls up.
        assert!(a.wheel(0.0, 40.0, false));
        assert_eq!(a.offset(), (100.0, 160.0));
        // Towards the user is negative, and scrolls down.
        assert!(a.wheel(0.0, -120.0, false));
        assert_eq!(a.offset(), (100.0, 280.0));
        // Shift turns a vertical wheel across: away from the user scrolls left.
        assert!(a.wheel(0.0, 30.0, true));
        assert_eq!(a.offset(), (70.0, 280.0));
        // A trackpad's two-way movement is taken as it is, both axes subtracted.
        assert!(a.wheel(10.0, 20.0, true));
        assert_eq!(a.offset(), (60.0, 260.0));
        // At the top it does not move, so the caller may give the wheel to a parent.
        a.set_offset((0.0, 0.0));
        assert!(!a.wheel(0.0, 40.0, false));
        assert!(!a.wheel(40.0, 0.0, false));
        // The far end behaves the same way downwards, and a part that is not a number moves
        // nothing.
        a.set_offset((600.0, 1800.0));
        assert!(!a.wheel(0.0, -40.0, false));
        assert!(!a.wheel(f64::NAN, f64::NAN, false));
        assert_eq!(a.offset(), (600.0, 1800.0));
    }

    #[test]
    fn the_page_keys_move_by_a_page_less_a_line_and_home_and_end_go_to_the_ends() {
        let mut a = area();
        // A page is the view's 200 less one line of 24: 176.
        assert!(a.key(KeyName::PageDown, Mods::default()));
        assert_eq!(a.offset(), (0.0, 176.0));
        assert!(a.key(KeyName::PageUp, Mods::default()));
        assert_eq!(a.offset(), (0.0, 0.0));
        // Space pages down, Shift+Space up, exactly as Page Down and Page Up.
        assert!(a.key(KeyName::Space, Mods::default()));
        assert_eq!(a.offset(), (0.0, 176.0));
        assert!(a.key(
            KeyName::Space,
            Mods {
                shift: true,
                ..Mods::default()
            }
        ));
        assert_eq!(a.offset(), (0.0, 0.0));
        // A line at a time, and a line across.
        assert!(a.key(KeyName::Down, Mods::default()));
        assert!(a.key(KeyName::Down, Mods::default()));
        assert_eq!(a.offset(), (0.0, 48.0));
        assert!(a.key(KeyName::Right, Mods::default()));
        assert_eq!(a.offset(), (24.0, 48.0));
        assert!(a.key(KeyName::Up, Mods::default()));
        assert!(a.key(KeyName::Left, Mods::default()));
        assert_eq!(a.offset(), (0.0, 24.0));
        // End is the bottom, Home the top; with Ctrl, bottom right and top left.
        assert!(a.key(KeyName::End, Mods::default()));
        assert_eq!(a.offset(), (0.0, 1800.0));
        assert!(a.key(
            KeyName::End,
            Mods {
                ctrl: true,
                ..Mods::default()
            }
        ));
        assert_eq!(a.offset(), (600.0, 1800.0));
        assert!(a.key(
            KeyName::Home,
            Mods {
                ctrl: true,
                ..Mods::default()
            }
        ));
        assert_eq!(a.offset(), (0.0, 0.0));
        assert!(a.key(KeyName::End, Mods::default()));
        assert!(a.key(KeyName::Home, Mods::default()));
        assert_eq!(a.offset(), (0.0, 0.0));
        // Keys that are not scroll keys, and an Alt chord, are the caller's.
        assert!(!a.key(KeyName::Enter, Mods::default()));
        assert!(!a.key(
            KeyName::PageDown,
            Mods {
                alt: true,
                ..Mods::default()
            }
        ));
        assert_eq!(a.offset(), (0.0, 0.0));
    }

    #[test]
    fn reveal_moves_a_control_above_below_or_taller_than_the_view_into_view() {
        let mut a = area();
        a.set_offset((100.0, 100.0));
        // Already whole on screen: nothing moves.
        assert!(!a.reveal((150.0, 150.0), (50.0, 20.0)));
        assert_eq!(a.offset(), (100.0, 100.0));
        // Above and to the left: to that edge.
        assert!(a.reveal((40.0, 60.0), (60.0, 20.0)));
        assert_eq!(a.offset(), (40.0, 60.0));
        // Below and to the right: its end at the view's end.
        assert!(a.reveal((420.0, 380.0), (60.0, 30.0)));
        assert_eq!(a.offset(), (80.0, 210.0));
        // Taller than the view: its start.
        assert!(a.reveal((20.0, 900.0), (80.0, 260.0)));
        assert_eq!(a.offset(), (20.0, 900.0));
        // A rectangle past the far end stops at the content's end.
        assert!(a.reveal((940.0, 1900.0), (80.0, 100.0)));
        assert_eq!(a.offset(), (600.0, 1800.0));
        // A place that is not a number leaves that axis where it is.
        assert!(!a.reveal((f64::NAN, f64::NAN), (10.0, 10.0)));
        assert_eq!(a.offset(), (600.0, 1800.0));
    }

    #[test]
    fn reveal_focus_moves_only_for_the_control_that_holds_the_focus() {
        let mut a = area();
        let mut focus: Focus<u32> = Focus::new();
        focus.begin_frame();
        for id in [1, 2, 3] {
            focus.add(id);
        }
        focus.end_frame();
        focus.focus(2);
        assert!(!a.reveal_focus(&focus, 1, (0.0, 900.0), (50.0, 20.0)));
        assert_eq!(a.offset(), (0.0, 0.0));
        assert!(a.reveal_focus(&focus, 2, (0.0, 900.0), (50.0, 20.0)));
        assert_eq!(a.offset(), (0.0, 720.0));
        assert!(!a.reveal_focus(&focus, 2, (0.0, 760.0), (50.0, 20.0)));
    }

    #[test]
    fn reveal_focus_brings_a_control_in_once_and_does_not_pin_it_after_a_scroll() {
        let mut a = area();
        let mut focus: Focus<u32> = Focus::new();
        focus.begin_frame();
        for id in [1, 2] {
            focus.add(id);
        }
        focus.end_frame();
        focus.focus(2);
        // The first reveal moves it in; a control taller than the view shows its start.
        assert!(a.reveal_focus(&focus, 2, (0.0, 900.0), (50.0, 260.0)));
        assert_eq!(a.offset(), (0.0, 900.0));
        // Page Down scrolls inside it; the next frame's reveal for the same focus moves nothing.
        assert!(a.key(KeyName::PageDown, Mods::default()));
        assert_eq!(a.offset(), (0.0, 1076.0));
        assert!(!a.reveal_focus(&focus, 2, (0.0, 900.0), (50.0, 260.0)));
        assert_eq!(a.offset(), (0.0, 1076.0));
        // Even to the end of the content, where the control's start is far above the view.
        assert!(a.wheel(0.0, -1000.0, false));
        assert_eq!(a.offset(), (0.0, 1800.0));
        assert!(!a.reveal_focus(&focus, 2, (0.0, 900.0), (50.0, 260.0)));
        assert_eq!(a.offset(), (0.0, 1800.0));
        // Another control takes the focus: it is revealed in its turn.
        focus.focus(1);
        assert!(a.reveal_focus(&focus, 1, (0.0, 100.0), (50.0, 20.0)));
        assert_eq!(a.offset(), (0.0, 100.0));
    }

    #[test]
    fn layout_column_makes_the_content_as_long_as_the_items_want_and_keeps_them_still() {
        let mut a = ScrollArea::new();
        let flex = Flex::column().gap(10.0);
        let items = [Item::fixed(30.0), Item::fixed(40.0), Item::fixed(50.0)];
        let rects = a.layout_column(Rect::new(0.0, 0.0, 300.0, 120.0), &flex, &items);
        // 30 + 10 + 40 + 10 + 50.
        assert_eq!(a.content(), (300.0, 140.0));
        assert_eq!(a.view(), (300.0, 120.0));
        assert_eq!(
            rects,
            vec![
                Rect::new(0.0, 0.0, 300.0, 30.0),
                Rect::new(0.0, 40.0, 300.0, 40.0),
                Rect::new(0.0, 90.0, 300.0, 50.0),
            ]
        );
        // It scrolls by the 20 the content is longer, and the rectangles do not move: they are
        // in content pixels, and the painter's offset shows them elsewhere on screen.
        assert!(a.scroll_by((0.0, 50.0)));
        assert_eq!(a.offset(), (0.0, 20.0));
        let again = a.layout_column(Rect::new(0.0, 0.0, 300.0, 120.0), &flex, &items);
        assert_eq!(again, rects);
        assert_eq!(a.offset(), (0.0, 20.0));
        // A flexible item keeps its natural size: the column is laid out at its own length, not
        // squeezed into the 120 px view.
        let flex = Flex::column().gap(0.0);
        let items = [Item::flex(1.0).min(80.0), Item::fixed(60.0)];
        let rects = a.layout_column(Rect::new(0.0, 0.0, 300.0, 120.0), &flex, &items);
        assert_eq!(a.content(), (300.0, 140.0));
        assert_eq!(rects[0].h, 80.0);
        assert_eq!(rects[1].y, 80.0);
    }

    #[test]
    fn layout_column_keeps_the_offset_when_the_view_and_the_content_grow_together() {
        let mut a = ScrollArea::new();
        a.set_view((200.0, 200.0));
        a.set_content((200.0, 600.0));
        a.set_offset((0.0, 400.0));
        // A view of 300 and a content of 900 in one frame: clamping against the old content
        // first would pull 400 down to 300 and lose it.
        let flex = Flex::column();
        let items: Vec<Item> = (0..9).map(|_| Item::fixed(100.0)).collect();
        a.layout_column(Rect::new(0.0, 0.0, 300.0, 300.0), &flex, &items);
        assert_eq!(a.content(), (300.0, 900.0));
        assert_eq!(a.view(), (300.0, 300.0));
        assert_eq!(a.offset(), (0.0, 400.0));
    }

    #[test]
    fn points_go_between_the_content_and_the_area() {
        let mut a = area();
        a.set_offset((30.0, 40.0));
        let view = Rect::new(10.0, 20.0, 400.0, 200.0);
        assert_eq!(a.to_area(view, (130.0, 140.0)), (110.0, 120.0));
        assert_eq!(a.to_content(view, 110.0, 120.0), Some((130.0, 140.0)));
        assert_eq!(a.to_content(view, 5.0, 120.0), None);
        assert_eq!(
            a.content_rect(view),
            Rect::new(-20.0, -20.0, 1000.0, 2000.0)
        );
    }

    #[test]
    fn a_size_an_offset_or_a_point_that_is_not_a_number_does_not_panic() {
        let mut a = ScrollArea::new();
        a.set_view((f64::NAN, f64::INFINITY));
        assert_eq!(a.view(), (0.0, 0.0));
        a.set_content((-5.0, f64::NAN));
        assert_eq!(a.content(), (0.0, 0.0));
        assert!(!a.set_offset((f64::NAN, 3.0)));
        assert_eq!(a.offset(), (0.0, 0.0));
        // A view and a content of real sizes; a movement that is not a number does nothing,
        // and an infinite one goes as far as the content lets it.
        a.set_view((300.0, 200.0));
        a.set_content((600.0, 900.0));
        assert!(!a.scroll_by((f64::NAN, f64::NAN)));
        assert_eq!(a.offset(), (0.0, 0.0));
        assert!(a.scroll_by((f64::INFINITY, 0.0)));
        assert_eq!(a.offset(), (300.0, 0.0));
        a.set_offset((0.0, 0.0));
        assert!(!a.reveal((f64::INFINITY, f64::NEG_INFINITY), (10.0, 10.0)));
        assert_eq!(a.offset(), (0.0, 0.0));
    }

    #[test]
    fn a_component_that_is_not_a_number_keeps_its_axis_where_it_is() {
        let mut a = area();
        a.set_offset((100.0, 200.0));
        // An offset with a NaN part moves the other axis and leaves this one where it was.
        assert!(a.set_offset((f64::NAN, 300.0)));
        assert_eq!(a.offset(), (100.0, 300.0));
        assert!(a.scroll_to((f64::NAN, 250.0)));
        assert_eq!(a.offset(), (100.0, 250.0));
        assert!(a.scroll_by((f64::NAN, -50.0)));
        assert_eq!(a.offset(), (100.0, 200.0));
        // Nothing but NaN parts: nothing moves, and the answer says so.
        assert!(!a.scroll_by((f64::NAN, f64::NAN)));
        assert_eq!(a.offset(), (100.0, 200.0));
        assert!(!a.set_offset((f64::NAN, f64::NAN)));
        assert_eq!(a.offset(), (100.0, 200.0));
    }

    // ---- drawing ---------------------------------------------------------------------------

    const PAGE: Rect = Rect::new(0.0, 0.0, 160.0, 120.0);
    const VIEW_RECT: Rect = Rect::new(30.0, 20.0, 100.0, 80.0);

    fn shot(pal: &Colors, draw: impl FnOnce(&mut Painter<'_>)) -> Bitmap {
        let mut bmp = Bitmap::new(160, 120);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(PAGE, pal.page);
            draw(&mut p);
        }
        bmp
    }

    fn at(bmp: &Bitmap, x: f32, y: f32) -> Rgb {
        let o = (y as usize * bmp.width as usize + x as usize) * 4;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    }

    #[test]
    fn the_body_is_clipped_to_the_area() {
        let mut a = ScrollArea::new();
        a.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
        a.set_content((200.0, 400.0));
        a.set_offset((0.0, 50.0));
        let ink = [200, 30, 40];
        let mark = [20, 220, 90];
        let drawn = shot(&LIGHT, |p| {
            let saved = a.scrolled(p, VIEW_RECT);
            p.fill(Rect::new(0.0, 0.0, 200.0, 400.0), ink);
            // A mark whose place says whether the content was moved as well as clipped.
            p.fill(Rect::new(10.0, 60.0, 10.0, 10.0), mark);
            saved.restore(p);
        });
        // Inside the area the body shows, wherever the offset put it.
        assert_eq!(at(&drawn, 80.0, 60.0), ink);
        assert_eq!(at(&drawn, 31.0, 99.0), ink);
        // The translation: content (10, 60) is at screen (40, 30) at offset (0, 50).
        assert_eq!(at(&drawn, 45.0, 35.0), mark);
        // Outside it nothing was touched: the page keeps its pixels all round the area.
        for (x, y) in [(10.0, 10.0), (29.0, 60.0), (131.0, 60.0), (80.0, 19.0)] {
            assert_eq!(at(&drawn, x, y), LIGHT.page, "({x}, {y})");
        }
        assert_eq!(at(&drawn, 80.0, 101.0), LIGHT.page);
        // Without the clip the same fill would cover the whole bitmap.
        let unclipped = shot(&LIGHT, |p| {
            p.fill(Rect::new(0.0, 0.0, 200.0, 400.0), ink);
        });
        assert_eq!(at(&unclipped, 80.0, 10.0), ink);
    }

    #[test]
    fn after_restore_drawing_outside_the_area_is_unclipped_and_in_place() {
        let mut a = ScrollArea::new();
        a.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
        a.set_content((200.0, 400.0));
        a.set_offset((0.0, 50.0));
        let ink = [10, 200, 30];
        let inside = [200, 30, 40];
        let drawn = shot(&LIGHT, |p| {
            let saved = a.scrolled(p, VIEW_RECT);
            p.fill(Rect::new(0.0, 60.0, 10.0, 10.0), inside);
            saved.restore(p);
            // The page's own top left, outside the area, and a corner beyond its right edge:
            // both land where asked, unclipped and unmoved.
            p.fill(Rect::new(0.0, 0.0, 10.0, 10.0), ink);
            p.fill(Rect::new(150.0, 110.0, 10.0, 10.0), ink);
        });
        assert_eq!(at(&drawn, 5.0, 5.0), ink);
        assert_eq!(at(&drawn, 155.0, 115.0), ink);
        // The body drawn before the restore was clipped and moved as it should be.
        assert_eq!(at(&drawn, 35.0, 35.0), inside);
        assert_eq!(at(&drawn, 29.0, 20.0), LIGHT.page);
    }

    #[test]
    fn a_scrollbar_shows_only_where_the_content_is_longer() {
        let plain = shot(&LIGHT, |_| ());
        let bars = |content: (f64, f64), offset: (f64, f64)| {
            let mut a = ScrollArea::new();
            a.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
            a.set_content(content);
            a.set_offset(offset);
            shot(&LIGHT, |p| draw_bars(p, &LIGHT, VIEW_RECT, &a, false))
        };
        // It fits: no bar, and not one pixel changed.
        assert_eq!(bars((100.0, 60.0), (0.0, 0.0)).pixels, plain.pixels);
        // Down only: a bar at the right edge, where the vertical thumb is.
        let mut long = ScrollArea::new();
        long.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
        long.set_content((100.0, 400.0));
        let (down, across) = long.bars(VIEW_RECT);
        assert!(across.is_none());
        let (_, thumb) = down.unwrap();
        let down_only = bars((100.0, 400.0), (0.0, 0.0));
        assert_ne!(at(&down_only, thumb.cx(), thumb.cy()), LIGHT.page);
        assert_eq!(
            at(&down_only, VIEW_RECT.x + 5.0, VIEW_RECT.cy()),
            LIGHT.page
        );
        // Across only: a bar along the bottom, and none at the right edge where the vertical
        // thumb would be.
        let mut wide = ScrollArea::new();
        wide.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
        wide.set_content((400.0, 60.0));
        let (down, across) = wide.bars(VIEW_RECT);
        assert!(down.is_none());
        let (_, hthumb) = across.unwrap();
        let across_only = bars((400.0, 60.0), (0.0, 0.0));
        assert_ne!(at(&across_only, hthumb.cx(), hthumb.cy()), LIGHT.page);
        assert_eq!(
            at(&across_only, VIEW_RECT.right() - 6.0, VIEW_RECT.y + 5.0),
            LIGHT.page
        );
        // Both, scrolled to the far end with the bars active: each stops short of the corner.
        let mut b = ScrollArea::new();
        b.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
        b.set_content((400.0, 400.0));
        b.set_offset((300.0, 320.0));
        let (down, across) = b.bars(VIEW_RECT);
        let (_, dthumb) = down.unwrap();
        let (_, hthumb) = across.unwrap();
        let both = shot(&LIGHT, |p| draw_bars(p, &LIGHT, VIEW_RECT, &b, true));
        assert_ne!(at(&both, dthumb.cx(), dthumb.cy()), LIGHT.page);
        assert_ne!(at(&both, hthumb.cx(), hthumb.cy()), LIGHT.page);
        // The corner the two bars leave to each other shows the page, not either track.
        assert_eq!(
            at(&both, VIEW_RECT.right() - 6.0, VIEW_RECT.bottom() - 6.0),
            LIGHT.page
        );
    }

    #[test]
    fn a_scrolled_frame_shows_the_rows_at_the_offset() {
        const ROW: f32 = 20.0;
        let rows: Vec<Rgb> = (0..12).map(|i| [30 + i * 15, 60, 200 - i * 10]).collect();
        let mut a = ScrollArea::new();
        a.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
        a.set_content((100.0, 12.0 * f64::from(ROW)));
        a.set_offset((0.0, 2.0 * f64::from(ROW)));
        let drawn = shot(&LIGHT, |p| {
            let saved = a.scrolled(p, VIEW_RECT);
            for (i, ink) in rows.iter().enumerate() {
                p.fill(Rect::new(0.0, i as f32 * ROW, 100.0, ROW), *ink);
            }
            saved.restore(p);
        });
        // At offset 40 the top of the view is row 2; four rows of 20 fit in 80.
        for (i, ink) in rows.iter().enumerate().skip(2).take(4) {
            let y = VIEW_RECT.y + (i as f32 - 2.0) * ROW + ROW / 2.0;
            assert_eq!(at(&drawn, VIEW_RECT.cx(), y), *ink, "row {i}");
        }
        // Row 1 is above the view and row 6 below it: the page shows there.
        assert_eq!(at(&drawn, VIEW_RECT.cx(), VIEW_RECT.y - 1.0), LIGHT.page);
        assert_eq!(
            at(&drawn, VIEW_RECT.cx(), VIEW_RECT.bottom() + 1.0),
            LIGHT.page
        );
    }

    #[test]
    fn the_animated_bars_at_rest_draw_the_plain_ones_in_both_palettes() {
        for pal in [&LIGHT, &DARK] {
            let mut a = ScrollArea::new();
            a.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
            a.set_content((400.0, 400.0));
            let plain = shot(pal, |p| draw_bars(p, pal, VIEW_RECT, &a, true));
            let clock = ManualClock::new();
            let mut anim = Animator::new();
            let mut bmp = Bitmap::new(160, 120);
            let mut gfx = Gfx::new();
            {
                let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
                p.fill(PAGE, pal.page);
                anim.begin_frame(clock.now());
                draw_bars_animated(&mut p, pal, &mut anim, "area", VIEW_RECT, &a, true);
            }
            assert!(anim.end_frame().is_none(), "an idle bar asks for no frame");
            assert!(bmp.pixels == plain.pixels, "dark {}", pal.dark);
        }
    }

    /// A sheet for a person to judge: `cargo test -p mdshared-ui scrollarea_sheet -- --ignored`
    /// writes `target/specimen/scrollarea.png`: a dialog card whose body is taller than the
    /// card, at the top and scrolled down, light above and dark below.
    #[test]
    #[ignore]
    fn scrollarea_sheet() {
        use crate::gfx::Face;
        use crate::overlay::{DIALOG_FOOTER, DIALOG_PAD, draw_dialog_card};

        let (w, h) = (420.0f32, 300.0f32);
        let mut bmp = Bitmap::new((2.0 * (w + 20.0)) as u32, (2.0 * (h + 20.0)) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        for (k, pal) in [LIGHT, DARK].iter().enumerate() {
            let oy = k as f32 * (h + 20.0) + 10.0;
            for (j, offset) in [0.0f64, 220.0].iter().enumerate() {
                let ox = j as f32 * (w + 20.0) + 10.0;
                p.fill(
                    Rect::new(ox - 10.0, oy - 10.0, w + 20.0, h + 20.0),
                    pal.page,
                );
                let card = Rect::new(ox, oy, w, h);
                draw_dialog_card(&mut p, pal, card);
                p.text(
                    Face::Semibold,
                    15.0,
                    card.x + DIALOG_PAD,
                    card.y + DIALOG_PAD,
                    "Preferences",
                    pal.text,
                    card.w - 2.0 * DIALOG_PAD,
                );
                let body = Rect::new(
                    card.x + DIALOG_PAD,
                    card.y + DIALOG_PAD + 30.0,
                    card.w - 2.0 * DIALOG_PAD,
                    card.h - 2.0 * DIALOG_PAD - DIALOG_FOOTER - 14.0,
                );
                let mut area = ScrollArea::new();
                let flex = Flex::column().gap(8.0);
                let items: Vec<Item> = (0..12).map(|_| Item::fixed(36.0)).collect();
                let rects = area.layout_column(body, &flex, &items);
                area.set_offset((0.0, *offset));
                let saved = area.scrolled(&mut p, body);
                for (i, r) in rects.iter().enumerate() {
                    p.frame(*r, 4.0, pal.field, pal.line);
                    let label = format!("Setting {}", i + 1);
                    p.text(
                        Face::Ui,
                        13.0,
                        r.x + 10.0,
                        r.cy(),
                        &label,
                        pal.text,
                        r.w - 20.0,
                    );
                }
                saved.restore(&mut p);
                draw_bars(&mut p, pal, body, &area, true);
            }
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join("scrollarea.png")).unwrap();
    }
}
