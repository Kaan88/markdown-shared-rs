//! A splitter: one area cut into two panes with a handle between them that the pointer drags.
//! Where the cut is, what a press, a move and a release do to it, collapsing a pane, the reset
//! of a double click, and the cut as one settings value. State and arithmetic; the handle's
//! look is [`pane::draw_splitter`](crate::pane::draw_splitter), which [`draw`] calls for the
//! side-by-side case and mirrors for the stacked one.
//!
//! The caller owns the events and the cursor: it passes a press to [`Splitter::press`], moves
//! to [`Splitter::drag`], the release to [`Splitter::release`], shows the resize cursor while
//! [`Splitter::hit`] is true or a drag is on, and lays its two panes out in the rectangles
//! [`Splitter::rects`] gives, every frame. Nothing here remembers the area: each call is given
//! it, so a window that is resized between two calls needs no telling.
//!
//! Not `split`: [`mdshared_app::split`] is the cut of a document into notes. Deliberately not
//! here: more than two panes in one splitter (nest two), and a pane that animates open (ask
//! [`anim`](crate::anim) for the position and pass it on).

use crate::gfx::{Painter, Rect};
use crate::theme::Palette;

/// How wide the strip is that takes hold of the cut, centred on it: what
/// [`pane::draw_splitter`](crate::pane::draw_splitter) gives its own hit rectangle.
pub const GRIP: f32 = 6.0;

/// Which way the two panes lie.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Axis {
    /// Side by side: A on the left, B on the right, the handle a vertical line.
    Horizontal,
    /// One above the other: A on top, B below, the handle a horizontal line.
    Vertical,
}

/// One of the two panes: A is the left or the top one, B the right or the bottom one.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Side {
    A,
    B,
}

/// What the cut holds on to when the area changes size.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hold {
    /// A's share of the area, 0 to 1: both panes grow with the window. Two documents side by
    /// side.
    Ratio(f32),
    /// A's size in logical px: B takes what a larger window adds. A sidebar on the left.
    A(f32),
    /// B's size in logical px: A takes what a larger window adds. An inspector on the right,
    /// a terminal below.
    B(f32),
}

/// A cut of an area in two, and the drag that moves it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Splitter {
    hold: Hold,
    /// What [`reset`](Splitter::reset) goes back to.
    default: Hold,
    /// The least each pane is while it is open.
    min_a: f32,
    min_b: f32,
    /// Whether dragging far into a pane's minimum closes it.
    snap_a: bool,
    snap_b: bool,
    collapsed: Option<Side>,
    /// With the handle in hand: how far the pointer is from the cut, along the axis.
    grab: Option<f32>,
}

impl Splitter {
    /// A cut at `hold`, both panes at least 80 px while open, neither closing under a drag.
    pub const fn new(hold: Hold) -> Splitter {
        Splitter {
            hold,
            default: hold,
            min_a: 80.0,
            min_b: 80.0,
            snap_a: false,
            snap_b: false,
            collapsed: None,
            grab: None,
        }
    }

    /// The same with other least sizes for A and B.
    pub const fn min(mut self, min_a: f32, min_b: f32) -> Splitter {
        self.min_a = min_a;
        self.min_b = min_b;
        self
    }

    /// The same, with `side` closing when the handle is dragged more than half way into its
    /// least size (and opening again when it is dragged back out): the way a sidebar is
    /// dragged shut.
    pub const fn snaps(mut self, side: Side) -> Splitter {
        match side {
            Side::A => self.snap_a = true,
            Side::B => self.snap_b = true,
        }
        self
    }

    /// What the cut holds on to, with its number as it is now.
    pub fn hold(&self) -> Hold {
        self.hold
    }

    /// The pane that is closed, if one is.
    pub fn collapsed(&self) -> Option<Side> {
        self.collapsed
    }

    /// Whether the handle is in hand.
    pub fn dragging(&self) -> bool {
        self.grab.is_some()
    }

    /// The length of `area` along the axis, and where it starts.
    fn span(area: Rect, axis: Axis) -> (f32, f32) {
        match axis {
            Axis::Horizontal => (area.x, area.w.max(0.0)),
            Axis::Vertical => (area.y, area.h.max(0.0)),
        }
    }

    /// The cut kept clear of both least sizes in an area `len` long. An area too short for
    /// both gives each its share of what there is, by their least sizes.
    fn clamp(&self, at: f32, len: f32) -> f32 {
        let (min_a, min_b) = (clean(self.min_a), clean(self.min_b));
        if min_a + min_b >= len {
            return if min_a + min_b > 0.0 {
                (len * min_a / (min_a + min_b)).round()
            } else {
                (len / 2.0).round()
            };
        }
        clean(at).clamp(min_a, len - min_b).round()
    }

    /// Where the cut is in an area `len` long: A's size, in whole px. A closed A puts it at 0,
    /// a closed B at `len`.
    pub fn position(&self, len: f32) -> f32 {
        let len = clean(len);
        match self.collapsed {
            Some(Side::A) => 0.0,
            Some(Side::B) => len,
            None => {
                let at = match self.hold {
                    Hold::Ratio(r) => len * clean(r).min(1.0),
                    Hold::A(px) => px,
                    Hold::B(px) => len - clean(px),
                };
                self.clamp(at, len)
            }
        }
    }

    /// Pane A, the handle and pane B in `area`. The panes tile the area: they meet at the cut
    /// and neither gives room to the handle, which is the strip [`GRIP`] wide over their
    /// meeting edge, for hit testing and for [`draw`]. A closed pane is an empty rectangle at
    /// its side of the area, and the handle then lies at the area's edge, still there to be
    /// dragged back.
    pub fn rects(&self, area: Rect, axis: Axis) -> (Rect, Rect, Rect) {
        let (start, len) = Splitter::span(area, axis);
        let at = self.position(len);
        match axis {
            Axis::Horizontal => (
                Rect::new(area.x, area.y, at, area.h),
                Rect::new(start + at - GRIP / 2.0, area.y, GRIP, area.h),
                Rect::new(area.x + at, area.y, len - at, area.h),
            ),
            Axis::Vertical => (
                Rect::new(area.x, area.y, area.w, at),
                Rect::new(area.x, start + at - GRIP / 2.0, area.w, GRIP),
                Rect::new(area.x, area.y + at, area.w, len - at),
            ),
        }
    }

    /// Whether the point is on the handle (or the handle is in hand, wherever the pointer is):
    /// the caller shows the resize cursor.
    pub fn hit(&self, area: Rect, axis: Axis, x: f32, y: f32) -> bool {
        self.dragging() || self.rects(area, axis).1.contains(x, y)
    }

    /// A press of the primary button. On the handle it takes hold of it and returns true; the
    /// cut does not move until the pointer does. Anywhere else it returns false and the press
    /// is the panes'.
    pub fn press(&mut self, area: Rect, axis: Axis, x: f32, y: f32) -> bool {
        if !self.rects(area, axis).1.contains(x, y) {
            return false;
        }
        let (start, len) = Splitter::span(area, axis);
        let along = if axis == Axis::Horizontal { x } else { y };
        self.grab = Some(along - (start + self.position(len)));
        true
    }

    /// The pointer moved with the handle in hand: the cut follows it, stopping at each pane's
    /// least size. A pane that [`snaps`](Self::snaps) closes when the pointer is more than
    /// half way into its least size and opens again when it comes back. Returns whether
    /// anything changed (the caller lays out and redraws); false with no handle in hand.
    pub fn drag(&mut self, area: Rect, axis: Axis, x: f32, y: f32) -> bool {
        let Some(grab) = self.grab else {
            return false;
        };
        let (start, len) = Splitter::span(area, axis);
        let along = if axis == Axis::Horizontal { x } else { y };
        let want = along - grab - start;
        if !want.is_finite() {
            return false;
        }
        let before = (self.collapsed, self.position(len));
        self.collapsed = if self.snap_a && want < clean(self.min_a) / 2.0 {
            Some(Side::A)
        } else if self.snap_b && want > len - clean(self.min_b) / 2.0 {
            Some(Side::B)
        } else {
            None
        };
        if self.collapsed.is_none() {
            self.set_position(want, len);
        }
        before != (self.collapsed, self.position(len))
    }

    /// Lets go of the handle. Returns whether it was in hand: the moment to save the cut.
    pub fn release(&mut self) -> bool {
        self.grab.take().is_some()
    }

    /// Puts the cut `at` px from the start of an area `len` long (kept clear of the least
    /// sizes), in whatever the splitter holds on to, and opens a closed pane. For the keyboard
    /// (the position plus or minus a step) and for an animation of the cut.
    pub fn set_position(&mut self, at: f32, len: f32) {
        let len = clean(len);
        if !at.is_finite() {
            return;
        }
        self.collapsed = None;
        let at = self.clamp(at, len);
        self.hold = match self.hold {
            Hold::Ratio(r) if len <= 0.0 => Hold::Ratio(r),
            Hold::Ratio(_) => Hold::Ratio(at / len),
            Hold::A(_) => Hold::A(at),
            Hold::B(_) => Hold::B(len - at),
        };
    }

    /// Closes `side` if it is open and opens it if it is closed, at the size it had: the
    /// button and the shortcut that show and hide a sidebar. Closing one pane opens the other
    /// if that was the closed one.
    pub fn toggle_collapse(&mut self, side: Side) {
        self.collapsed = if self.collapsed == Some(side) {
            None
        } else {
            Some(side)
        };
    }

    /// Opens both panes with the cut where the splitter was made with: what a double click on
    /// the handle does.
    pub fn reset(&mut self) {
        self.hold = self.default;
        self.collapsed = None;
    }

    /// The cut as the value of one settings line: `"ratio:0.3"`, `"a:240"` or `"b:180"`, with
    /// `";a"` or `";b"` after it when that pane is closed (`"a:240;a"`: a sidebar of 240 px,
    /// hidden). The key of the line is the caller's.
    pub fn to_setting(&self) -> String {
        let cut = match self.hold {
            Hold::Ratio(r) => format!("ratio:{:.4}", clean(r).min(1.0)),
            Hold::A(px) => format!("a:{}", clean(px).round() as i64),
            Hold::B(px) => format!("b:{}", clean(px).round() as i64),
        };
        match self.collapsed {
            Some(Side::A) => cut + ";a",
            Some(Side::B) => cut + ";b",
            None => cut,
        }
    }

    /// Takes the cut from a line [`to_setting`](Self::to_setting) wrote. The line may be old
    /// or edited by hand: a cut of another kind than this splitter holds (a ratio where it
    /// keeps A's size) is skipped, since the application decides what its panes hold on to; a
    /// number that is not one is skipped; a ratio is kept within 0 to 1 and a size within 0 to
    /// 100,000; anything after the `;` that is not `a` or `b` means both panes are open.
    /// Whatever the number, [`rects`](Self::rects) keeps both panes at their least sizes.
    pub fn apply_setting(&mut self, value: &str) {
        let (cut, closed) = value.split_once(';').unwrap_or((value, ""));
        if let Some((kind, number)) = cut.split_once(':')
            && let Ok(n) = number.trim().parse::<f32>()
            && n.is_finite()
        {
            let px = n.clamp(0.0, 100_000.0).round();
            match (kind.trim(), self.hold) {
                ("ratio", Hold::Ratio(_)) => self.hold = Hold::Ratio(n.clamp(0.0, 1.0)),
                ("a", Hold::A(_)) => self.hold = Hold::A(px),
                ("b", Hold::B(_)) => self.hold = Hold::B(px),
                _ => {}
            }
        }
        self.collapsed = match closed.trim() {
            "a" => Some(Side::A),
            "b" => Some(Side::B),
            _ => None,
        };
    }
}

/// A size as the arithmetic can use it: a number that is not one, or is negative, is nothing.
fn clean(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// Draws the handle between the panes `rects` (from [`Splitter::rects`]): a hairline along the
/// cut, or the accent three device px wide when `hot` (the pointer is on it or it is in hand).
/// Side by side this is [`pane::draw_splitter`](crate::pane::draw_splitter) on pane A's inner
/// edge, pixel for pixel; stacked, it is the same look turned on its side. Nothing is drawn for
/// a handle at the very edge of the area with its pane closed and not `hot`.
pub fn draw(p: &mut Painter, pal: &Palette, rects: (Rect, Rect, Rect), axis: Axis, hot: bool) {
    let (a, _, b) = rects;
    if !hot && (a.w <= 0.0 || a.h <= 0.0 || b.w <= 0.0 || b.h <= 0.0) {
        return;
    }
    match axis {
        Axis::Horizontal => {
            crate::pane::draw_splitter(p, pal, a, true, hot);
        }
        Axis::Vertical => {
            let px = p.px();
            let y = a.bottom() - px;
            if hot {
                p.fill(Rect::new(a.x, y - px, a.w, 3.0 * px), pal.accent);
            } else {
                p.fill(Rect::new(a.x, y, a.w, px), pal.line);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Gfx;
    use crate::theme::LIGHT;
    use mdshared_text::Bitmap;

    const AREA: Rect = Rect::new(100.0, 50.0, 1000.0, 600.0);
    const H: Axis = Axis::Horizontal;
    const V: Axis = Axis::Vertical;

    #[test]
    fn the_panes_tile_the_area_and_the_handle_lies_over_the_cut() {
        let s = Splitter::new(Hold::Ratio(0.3));
        let (a, handle, b) = s.rects(AREA, H);
        assert_eq!(a, Rect::new(100.0, 50.0, 300.0, 600.0));
        assert_eq!(b, Rect::new(400.0, 50.0, 700.0, 600.0));
        assert_eq!(handle, Rect::new(397.0, 50.0, 6.0, 600.0));
        let (a, handle, b) = s.rects(AREA, V);
        assert_eq!(a, Rect::new(100.0, 50.0, 1000.0, 180.0));
        assert_eq!(b, Rect::new(100.0, 230.0, 1000.0, 420.0));
        assert_eq!(handle, Rect::new(100.0, 227.0, 1000.0, 6.0));
        assert_eq!(s.position(1000.0), 300.0);
        // The handle is hit on both sides of the cut, and nowhere else.
        assert!(s.hit(AREA, H, 397.0, 300.0) && s.hit(AREA, H, 402.9, 300.0));
        assert!(!s.hit(AREA, H, 396.9, 300.0) && !s.hit(AREA, H, 403.0, 300.0));
        assert!(!s.hit(AREA, H, 400.0, 49.0));
    }

    #[test]
    fn what_the_cut_holds_on_to_decides_who_gets_a_larger_window() {
        let wider = Rect { w: 1400.0, ..AREA };
        let ratio = Splitter::new(Hold::Ratio(0.25));
        assert_eq!(ratio.rects(AREA, H).0.w, 250.0);
        assert_eq!(ratio.rects(wider, H).0.w, 350.0);
        let sidebar = Splitter::new(Hold::A(240.0));
        assert_eq!(sidebar.rects(AREA, H).0.w, 240.0);
        assert_eq!(sidebar.rects(wider, H).0.w, 240.0);
        let inspector = Splitter::new(Hold::B(300.0));
        assert_eq!(inspector.rects(AREA, H).2.w, 300.0);
        assert_eq!(inspector.rects(wider, H).2.w, 300.0);
        assert_eq!(inspector.rects(wider, H).0.w, 1100.0);
        // The least sizes win over the number, and a window too small for both shares what
        // there is by them.
        let tight = Splitter::new(Hold::A(240.0)).min(200.0, 600.0);
        assert_eq!(tight.rects(Rect { w: 700.0, ..AREA }, H).0.w, 175.0);
        assert_eq!(tight.rects(Rect { w: 400.0, ..AREA }, H).0.w, 100.0);
        assert_eq!(Splitter::new(Hold::Ratio(0.01)).rects(AREA, H).0.w, 80.0);
        assert_eq!(Splitter::new(Hold::Ratio(7.0)).rects(AREA, H).0.w, 920.0);
        // No area, and numbers that are not numbers: empty rectangles, no panic.
        let none = Rect::new(0.0, 0.0, 0.0, 0.0);
        let (a, _, b) = Splitter::new(Hold::Ratio(f32::NAN)).rects(none, H);
        assert_eq!((a.w, b.w), (0.0, 0.0));
        let odd = Splitter::new(Hold::B(f32::INFINITY)).min(f32::NAN, -3.0);
        let (a, _, b) = odd.rects(AREA, V);
        assert_eq!(a.h + b.h, 600.0);
    }

    #[test]
    fn a_drag_moves_the_cut_by_as_much_as_the_pointer_and_stops_at_the_least_sizes() {
        let mut s = Splitter::new(Hold::Ratio(0.3)).min(200.0, 300.0);
        assert!(!s.drag(AREA, H, 500.0, 300.0), "no handle in hand");
        assert!(!s.press(AREA, H, 500.0, 300.0), "not on the handle");
        // Taken 2 px right of the cut: the cut stays 2 px left of the pointer.
        assert!(s.press(AREA, H, 402.0, 300.0));
        assert!(s.dragging());
        assert!(
            s.hit(AREA, H, 5.0, 5.0),
            "in hand, it is the handle everywhere"
        );
        assert!(
            !s.drag(AREA, H, 402.0, 350.0),
            "along the handle nothing changes"
        );
        assert!(s.drag(AREA, H, 552.0, 300.0));
        assert_eq!(s.rects(AREA, H).0.w, 450.0);
        assert_eq!(s.hold(), Hold::Ratio(0.45));
        // Past the least sizes it stops, and comes back with the pointer.
        assert!(s.drag(AREA, H, 150.0, 300.0));
        assert_eq!(s.rects(AREA, H).0.w, 200.0);
        assert!(!s.drag(AREA, H, 120.0, 300.0));
        assert!(s.drag(AREA, H, 5000.0, 300.0));
        assert_eq!(s.rects(AREA, H).2.w, 300.0);
        assert!(!s.drag(AREA, H, f32::NAN, 300.0));
        assert!(s.release());
        assert!(!s.release() && !s.dragging());
        // Stacked, and holding B's size: the same along y.
        let mut t = Splitter::new(Hold::B(200.0));
        assert!(t.press(AREA, V, 300.0, 450.0));
        assert!(t.drag(AREA, V, 300.0, 400.0));
        assert_eq!((t.hold(), t.rects(AREA, V).2.h), (Hold::B(250.0), 250.0));
        t.release();
        // The keyboard: a step from where it is.
        let mut k = Splitter::new(Hold::A(240.0));
        k.set_position(k.position(1000.0) + 16.0, 1000.0);
        assert_eq!(k.hold(), Hold::A(256.0));
        k.set_position(f32::NAN, 1000.0);
        k.set_position(-50.0, 1000.0);
        assert_eq!(k.hold(), Hold::A(80.0));
    }

    #[test]
    fn a_pane_that_snaps_closes_under_a_drag_and_a_toggle_keeps_its_size() {
        let mut s = Splitter::new(Hold::A(240.0))
            .min(160.0, 200.0)
            .snaps(Side::A);
        assert!(s.press(AREA, H, 340.0, 300.0));
        // Into the least size but not half way: it stops at 160.
        assert!(s.drag(AREA, H, 190.0, 300.0));
        assert_eq!((s.collapsed(), s.rects(AREA, H).0.w), (None, 160.0));
        // Past half of it: closed. The pane is empty and B has the whole area.
        assert!(s.drag(AREA, H, 170.0, 300.0));
        assert_eq!(s.collapsed(), Some(Side::A));
        let (a, handle, b) = s.rects(AREA, H);
        assert_eq!((a.w, b), (0.0, AREA));
        assert_eq!(handle.x, 97.0, "still there to be dragged back");
        // Back out: open again, at the pointer.
        assert!(s.drag(AREA, H, 400.0, 300.0));
        assert_eq!((s.collapsed(), s.rects(AREA, H).0.w), (None, 300.0));
        s.release();
        // The other pane does not snap: it stops.
        assert!(s.press(AREA, H, 400.0, 300.0));
        s.drag(AREA, H, 5000.0, 300.0);
        assert_eq!((s.collapsed(), s.rects(AREA, H).2.w), (None, 200.0));
        s.release();

        // The button: closed and open again at the size it had.
        let mut side = Splitter::new(Hold::A(240.0));
        side.toggle_collapse(Side::A);
        assert_eq!(
            (side.collapsed(), side.rects(AREA, H).0.w),
            (Some(Side::A), 0.0)
        );
        side.toggle_collapse(Side::A);
        assert_eq!((side.collapsed(), side.rects(AREA, H).0.w), (None, 240.0));
        side.toggle_collapse(Side::B);
        assert_eq!(side.rects(AREA, H).0, AREA);
        side.toggle_collapse(Side::A);
        assert_eq!(side.collapsed(), Some(Side::A));
        // A double click: both open, at the cut it was made with.
        side.set_position(500.0, 1000.0);
        side.toggle_collapse(Side::B);
        side.reset();
        assert_eq!((side.collapsed(), side.hold()), (None, Hold::A(240.0)));
        // A closed pane taken by its handle at the edge opens under the drag.
        let mut shut = Splitter::new(Hold::A(240.0)).snaps(Side::A);
        shut.toggle_collapse(Side::A);
        assert!(shut.press(AREA, H, 101.0, 300.0));
        assert!(shut.drag(AREA, H, 301.0, 300.0));
        assert_eq!((shut.collapsed(), shut.hold()), (None, Hold::A(200.0)));
    }

    #[test]
    fn the_cut_goes_through_one_settings_line() {
        for (s, line) in [
            (Splitter::new(Hold::Ratio(0.3)), "ratio:0.3000"),
            (Splitter::new(Hold::A(240.4)), "a:240"),
            (Splitter::new(Hold::B(180.0)), "b:180"),
        ] {
            assert_eq!(s.to_setting(), line);
            let mut back = Splitter::new(match s.hold() {
                Hold::Ratio(_) => Hold::Ratio(0.9),
                Hold::A(_) => Hold::A(1.0),
                Hold::B(_) => Hold::B(1.0),
            });
            back.apply_setting(line);
            assert_eq!(back.to_setting(), line);
            // Closed, and back.
            let mut closed = s;
            closed.toggle_collapse(Side::B);
            let line = closed.to_setting();
            assert!(line.ends_with(";b"));
            back.apply_setting(&line);
            assert_eq!(back.collapsed(), Some(Side::B));
            assert_eq!(back.to_setting(), line);
        }
        // Old or damaged lines change only what they say clearly.
        let fresh = || Splitter::new(Hold::A(240.0));
        for junk in [
            "",
            ";",
            "a",
            "a:",
            "a:wide",
            "a:NaN",
            "ratio:0.5",
            "b:300",
            ":",
            "a:1:2",
        ] {
            let mut s = fresh();
            s.apply_setting(junk);
            assert_eq!(
                (s.hold(), s.collapsed()),
                (Hold::A(240.0), None),
                "{junk:?}"
            );
        }
        let mut s = fresh();
        s.apply_setting(" a : 1e12 ; a ");
        assert_eq!(
            (s.hold(), s.collapsed()),
            (Hold::A(100_000.0), Some(Side::A))
        );
        s.apply_setting("a:-5;x");
        assert_eq!((s.hold(), s.collapsed()), (Hold::A(0.0), None));
        assert_eq!(s.rects(AREA, H).0.w, 80.0, "the least size still holds");
        let mut r = Splitter::new(Hold::Ratio(0.5));
        r.apply_setting("ratio:9");
        assert_eq!(r.hold(), Hold::Ratio(1.0));
    }

    fn at(bmp: &Bitmap, x: f32, y: f32) -> [u8; 3] {
        let o = (y as usize * bmp.width as usize + x as usize) * 4;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    }

    #[test]
    fn the_handle_draws_as_the_pane_splitter_does_and_the_same_on_its_side() {
        let area = Rect::new(0.0, 0.0, 200.0, 120.0);
        let s = Splitter::new(Hold::A(80.0)).min(20.0, 20.0);
        let shot = |draw: &dyn Fn(&mut Painter<'_>)| {
            let mut bmp = Bitmap::new(200, 120);
            let mut gfx = Gfx::new();
            {
                let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
                p.fill(area, LIGHT.pane);
                draw(&mut p);
            }
            bmp
        };
        for hot in [false, true] {
            let rects = s.rects(area, H);
            let ours = shot(&|p| draw(p, &LIGHT, rects, H, hot));
            let panes = shot(&|p| {
                crate::pane::draw_splitter(p, &LIGHT, rects.0, true, hot);
            });
            assert!(ours.pixels == panes.pixels, "hot {hot}");
            // Stacked: the same pixels with x and y changed over, on a square.
            let square = Rect::new(0.0, 0.0, 120.0, 120.0);
            let side = shot(&|p| draw(p, &LIGHT, s.rects(square, H), H, hot));
            let stacked = shot(&|p| draw(p, &LIGHT, s.rects(square, V), V, hot));
            for y in 0..120 {
                for x in 0..120 {
                    let (x, y) = (x as f32, y as f32);
                    assert_eq!(at(&stacked, x, y), at(&side, y, x), "hot {hot} at {x},{y}");
                }
            }
            let line = if hot { LIGHT.accent } else { LIGHT.line };
            assert_eq!(at(&stacked, 60.0, 79.0), line);
        }
        // A closed pane: no line at the window's edge until the pointer is on it.
        let mut shut = s;
        shut.toggle_collapse(Side::A);
        let blank = shot(&|_| ());
        assert!(shot(&|p| draw(p, &LIGHT, shut.rects(area, H), H, false)).pixels == blank.pixels);
        assert!(shot(&|p| draw(p, &LIGHT, shut.rects(area, H), H, true)).pixels != blank.pixels);
    }
}
