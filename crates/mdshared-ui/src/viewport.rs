//! A view that scrolls both ways: a table wider than its pane, a terminal with long lines, a
//! picture larger than its window. It holds the content's size, the view's size and the two
//! offsets as `f64`, and answers the questions a two-way view asks: how far each way it can
//! go, the offset that shows a rectangle, where a point of the content is on screen and back,
//! and both scrollbars ([`scroll::scrollbars`]).
//!
//! It draws nothing and owns no motion: a view that glides keeps a
//! [`SmoothScroll`](crate::scroll::SmoothScroll) for each axis (named `id.with("x")` and
//! `id.with("y")`) and gives their offsets to [`Viewport::offset`] each frame. The wheel's
//! convention is in [`wheel_delta`]: a vertical wheel with Shift held scrolls across.
//!
//! Deliberately not here: zoom (a picture viewer scales its content and gives the scaled size
//! here); momentum and rubber-banding at the edges; and rows (a list that also scrolls across
//! keeps [`rows::Rows`](crate::rows::Rows) for down and this for across, or this alone).

use crate::gfx::Rect;
use crate::scroll;

/// Turns a wheel's movement into a movement of the view, both `(across, down)` in the same
/// unit: as it is, except that a wheel that only turns (no sideways part) with Shift held
/// scrolls across, as Windows and the Linux desktops do. A part that is not a number is no
/// movement.
pub fn wheel_delta(across: f64, down: f64, shift: bool) -> (f64, f64) {
    let clean = |v: f64| if v.is_finite() { v } else { 0.0 };
    let (across, down) = (clean(across), clean(down));
    if shift && across == 0.0 {
        (down, 0.0)
    } else {
        (across, down)
    }
}

/// A two-way view of content: sizes and offsets are `(across, down)` in one unit of the
/// caller's choice.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Viewport {
    /// The size of everything there is to see.
    pub content: (f64, f64),
    /// The size of the window onto it.
    pub view: (f64, f64),
    /// How far the view is moved from the content's top left. Answers clamp it, so an
    /// offset left past either end does not change what they say.
    pub offset: (f64, f64),
}

/// A number, not negative.
fn size(v: f64) -> f64 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

impl Viewport {
    /// A view of `content` through `view`, at the top left.
    pub fn new(content: (f64, f64), view: (f64, f64)) -> Viewport {
        Viewport {
            content,
            view,
            offset: (0.0, 0.0),
        }
    }

    /// How far each way the view can move; 0 on an axis where the content fits.
    pub fn max(&self) -> (f64, f64) {
        (
            (size(self.content.0) - size(self.view.0)).max(0.0),
            (size(self.content.1) - size(self.view.1)).max(0.0),
        )
    }

    /// `offset` kept between the top left and [`max`](Self::max); a part that is not a
    /// number is 0.
    pub fn clamp(&self, offset: (f64, f64)) -> (f64, f64) {
        let (mx, my) = self.max();
        let c = |v: f64, m: f64| if v.is_nan() { 0.0 } else { v.clamp(0.0, m) };
        (c(offset.0, mx), c(offset.1, my))
    }

    /// The offset as it counts, clamped.
    pub fn at(&self) -> (f64, f64) {
        self.clamp(self.offset)
    }

    /// Sets the offset (clamped); returns whether it moved.
    pub fn scroll_to(&mut self, offset: (f64, f64)) -> bool {
        let to = self.clamp(offset);
        let moved = to != self.at();
        self.offset = to;
        moved
    }

    /// Moves the offset by `delta` (clamped); returns whether it moved.
    pub fn scroll_by(&mut self, delta: (f64, f64)) -> bool {
        let (x, y) = self.at();
        self.scroll_to((x + delta.0, y + delta.1))
    }

    /// Whether the content is wider, and taller, than the view: which ways it scrolls.
    pub fn scrolls(&self) -> (bool, bool) {
        let (mx, my) = self.max();
        (mx > 0.0, my > 0.0)
    }

    /// The part of the content the view shows: its top left and its size, in the content's
    /// unit.
    pub fn visible(&self) -> ((f64, f64), (f64, f64)) {
        let (x, y) = self.at();
        let w = size(self.view.0).min(size(self.content.0));
        let h = size(self.view.1).min(size(self.content.1));
        ((x, y), (w, h))
    }

    /// The offset that shows the content's rectangle at `at` of size `size` whole, moved on
    /// each axis only as far as needed: one off to the left or above goes to that edge, one off
    /// to the right or below to the other. On an axis where it is larger than the view, its
    /// start is shown.
    pub fn reveal(&self, at: (f64, f64), extent: (f64, f64)) -> (f64, f64) {
        let (x, y) = self.at();
        let axis = |start: f64, len: f64, offset: f64, view: f64| {
            let (start, len, view) = (start, size(len), size(view));
            if !start.is_finite() {
                offset
            } else if start < offset || len > view {
                start
            } else if start + len > offset + view {
                start + len - view
            } else {
                offset
            }
        };
        self.clamp((
            axis(at.0, extent.0, x, self.view.0),
            axis(at.1, extent.1, y, self.view.1),
        ))
    }

    /// Where a point of the content is in `area` at the current offset.
    pub fn to_area(&self, area: Rect, point: (f64, f64)) -> (f32, f32) {
        let (x, y) = self.at();
        (area.x + (point.0 - x) as f32, area.y + (point.1 - y) as f32)
    }

    /// The point of the content under a point of `area`; `None` outside the area.
    pub fn to_content(&self, area: Rect, x: f32, y: f32) -> Option<(f64, f64)> {
        if !area.contains(x, y) {
            return None;
        }
        let (ox, oy) = self.at();
        Some((f64::from(x - area.x) + ox, f64::from(y - area.y) + oy))
    }

    /// Both scrollbars for `area`, the vertical first ([`scroll::scrollbars`]): each `None`
    /// when the content fits that way. Very large sizes lose only precision a bar cannot show.
    #[allow(clippy::type_complexity)] // two optional (track, thumb) pairs
    pub fn bars(&self, area: Rect) -> (Option<(Rect, Rect)>, Option<(Rect, Rect)>) {
        let (x, y) = self.at();
        scroll::scrollbars(
            area,
            (size(self.content.0) as f32, size(self.content.1) as f32),
            (size(self.view.0) as f32, size(self.view.1) as f32),
            (x as f32, y as f32),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(10.0, 20.0, 400.0, 300.0);

    fn wide() -> Viewport {
        Viewport::new((1000.0, 2000.0), (400.0, 300.0))
    }

    #[test]
    fn a_view_moves_as_far_as_the_content_reaches() {
        let mut v = wide();
        assert_eq!(v.max(), (600.0, 1700.0));
        assert_eq!(v.scrolls(), (true, true));
        assert!(v.scroll_by((50.0, 100.0)));
        assert_eq!(v.at(), (50.0, 100.0));
        assert!(v.scroll_to((1e9, -5.0)));
        assert_eq!(v.at(), (600.0, 0.0));
        assert!(!v.scroll_by((10.0, 0.0)));
        v.offset = (f64::NAN, f64::INFINITY);
        assert_eq!(v.at(), (0.0, 1700.0));
        let fits = Viewport::new((100.0, 100.0), (400.0, 300.0));
        assert_eq!(fits.max(), (0.0, 0.0));
        assert_eq!(fits.scrolls(), (false, false));
        assert_eq!(fits.visible(), ((0.0, 0.0), (100.0, 100.0)));
    }

    #[test]
    fn shift_turns_the_wheel_sideways() {
        assert_eq!(wheel_delta(0.0, 40.0, true), (40.0, 0.0));
        assert_eq!(wheel_delta(0.0, 40.0, false), (0.0, 40.0));
        // A trackpad that moves both ways is taken as it is.
        assert_eq!(wheel_delta(3.0, 40.0, true), (3.0, 40.0));
        assert_eq!(wheel_delta(f64::NAN, 5.0, false), (0.0, 5.0));
    }

    #[test]
    fn reveal_moves_each_axis_only_as_far_as_needed() {
        let mut v = wide();
        v.offset = (100.0, 100.0);
        // Already whole on screen.
        assert_eq!(v.reveal((150.0, 150.0), (50.0, 50.0)), (100.0, 100.0));
        // Off to the right and above.
        assert_eq!(v.reveal((480.0, 50.0), (60.0, 20.0)), (140.0, 50.0));
        // Larger than the view: its start.
        assert_eq!(v.reveal((300.0, 900.0), (500.0, 10.0)), (300.0, 610.0));
        // Past the end stays within the content.
        assert_eq!(v.reveal((990.0, 1990.0), (100.0, 100.0)), (600.0, 1700.0));
        assert_eq!(v.reveal((f64::NAN, 10.0), (1.0, 1.0)), (100.0, 10.0));
    }

    #[test]
    fn points_go_between_the_content_and_the_area() {
        let mut v = wide();
        v.offset = (30.0, 40.0);
        assert_eq!(v.to_area(AREA, (130.0, 140.0)), (110.0, 120.0));
        assert_eq!(v.to_content(AREA, 110.0, 120.0), Some((130.0, 140.0)));
        assert_eq!(v.to_content(AREA, 5.0, 120.0), None);
        assert_eq!(v.visible(), ((30.0, 40.0), (400.0, 300.0)));
    }

    #[test]
    fn both_bars_leave_the_corner_to_each_other() {
        let v = wide();
        let (down, across) = v.bars(AREA);
        let (dt, _) = down.unwrap();
        let (at, _) = across.unwrap();
        assert!(
            dt.bottom() <= at.y + 0.01,
            "the vertical track stops above the horizontal"
        );
        assert!(
            at.right() <= dt.x + 0.01,
            "the horizontal track stops left of the vertical"
        );
        // One way only: the bar takes the whole side.
        let tall = Viewport::new((300.0, 2000.0), (400.0, 300.0));
        let (down, across) = tall.bars(AREA);
        assert!(across.is_none());
        assert_eq!(down, scroll::scrollbar(AREA, 2000.0, 300.0, 0.0));
    }

    /// A sheet for a person to judge: `cargo test -p mdshared-ui viewport_sheet -- --ignored`
    /// writes `target/specimen/viewport.png`: a grid larger than its view both ways with both
    /// bars at rest, the same with them active, and grids that scroll only across or only down;
    /// light above, dark below.
    #[test]
    #[ignore]
    fn viewport_sheet() {
        use crate::gfx::{Face, Gfx, Painter};
        use crate::scroll::{draw_hscrollbar, draw_scrollbar};
        use crate::theme::{DARK, LIGHT};
        use mdshared_text::Bitmap;
        let (cell, pane, gap) = (60.0f32, (220.0f32, 160.0f32), 20.0f32);
        let view = (f64::from(pane.0), f64::from(pane.1));
        let cases = [
            (Viewport::new((900.0, 700.0), view), (180.0, 240.0), false),
            (Viewport::new((900.0, 700.0), view), (680.0, 540.0), true),
            (Viewport::new((900.0, 150.0), view), (300.0, 0.0), false),
            (Viewport::new((200.0, 700.0), view), (0.0, 120.0), false),
        ];
        let w = gap + cases.len() as f32 * (pane.0 + gap);
        let half = pane.1 + 2.0 * gap;
        let mut bmp = Bitmap::new(w as u32, (2.0 * half) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        for (k, pal) in [LIGHT, DARK].iter().enumerate() {
            let top = k as f32 * half;
            p.fill(Rect::new(0.0, top, w, half), pal.page);
            for (i, (v, at, active)) in cases.iter().enumerate() {
                let mut v = *v;
                v.scroll_to(*at);
                let area = Rect::new(gap + i as f32 * (pane.0 + gap), top + gap, pane.0, pane.1);
                let border = Rect::new(area.x - 1.0, area.y - 1.0, area.w + 2.0, area.h + 2.0);
                p.frame(border, 0.0, pal.pane, pal.line);
                let clip = p.clip(area);
                // The content ends where it ends, inside a view larger than it.
                let (ex, ey) = v.to_area(area, (0.0, 0.0));
                let edge = Rect::new(ex, ey, v.content.0 as f32, v.content.1 as f32);
                let inner = p.clip(edge);
                let ((x0, y0), (vw, vh)) = v.visible();
                let (c0, r0) = ((x0 / 60.0) as i32, (y0 / 60.0) as i32);
                for r in r0..=((y0 + vh) / 60.0) as i32 {
                    for c in c0..=((x0 + vw) / 60.0) as i32 {
                        let (cx, cy) = (f64::from(c) * 60.0, f64::from(r) * 60.0);
                        if cx >= v.content.0 || cy >= v.content.1 {
                            continue;
                        }
                        let (sx, sy) = v.to_area(area, (cx, cy));
                        let r1 = Rect::new(sx + 1.0, sy + 1.0, cell - 2.0, cell - 2.0);
                        let fill = if (r + c) % 2 == 0 {
                            pal.hover
                        } else {
                            pal.pane
                        };
                        p.frame(r1, 4.0, fill, pal.line);
                        let label = format!("{c},{r}");
                        p.text_center(Face::Ui, 12.0, r1, &label, pal.muted);
                    }
                }
                p.restore(inner);
                let (down, across) = v.bars(area);
                if let Some((t, th)) = down {
                    draw_scrollbar(&mut p, pal, t, th, *active);
                }
                if let Some((t, th)) = across {
                    draw_hscrollbar(&mut p, pal, t, th, *active);
                }
                p.restore(clip);
            }
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join("viewport.png")).unwrap();
    }
}
