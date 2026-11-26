//! The size of one cell of the grid and the geometry that follows from it: how many columns and
//! rows fit a rectangle, which cell lies under a point, and where a cell is.

use crate::gfx::Rect;

/// The size of the cells of a grid, in whole device pixels, with the font size and the display
/// scale they were made for.
///
/// Every cell of the grid is exactly as wide and as tall as every other in device pixels, so a
/// line drawn to a cell's edge meets the line of the cell beside it with no gap and no overlap,
/// and the grid does not drift by fractions of a pixel across 200 columns. The grid starts at
/// the top left corner of the rectangle it is given, rounded to the device pixel as the painter
/// rounds it; what is left over at the right and the bottom (less than a cell) is padding in the
/// background colour.
///
/// Made by [`CellFont::metrics`](super::CellFont::metrics), which measures the font, or by
/// [`Metrics::fixed`] for a cell size given by hand. All the questions take logical px, as the
/// rest of the toolkit does, and give logical px back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    font_size: f32,
    scale: f32,
    /// Device px.
    cell_w: i32,
    cell_h: i32,
    /// Device px from the top of a cell down to the baseline of its text.
    baseline: i32,
}

/// The largest side of a cell in device px; anything larger is brought down to it.
const MAX_CELL: i32 = 1024;

/// A finite, positive number, or `fallback`.
fn sane(v: f32, fallback: f32) -> f32 {
    if v.is_finite() && v > 0.0 {
        v
    } else {
        fallback
    }
}

impl Metrics {
    /// Cells `cell_w` by `cell_h` logical px for text of `font_size` logical px on a display of
    /// `scale`. The cell is rounded to whole device pixels and is at least one; the baseline is
    /// put at four fifths of its height. A scale or a font size that is not a finite positive
    /// number counts as 1 (and a cell side as 1 logical px), so nonsense gives a small grid,
    /// never a panic.
    pub fn fixed(font_size: f32, scale: f32, cell_w: f32, cell_h: f32) -> Metrics {
        let scale = sane(scale, 1.0).min(64.0);
        let side = |v: f32| ((sane(v, 1.0) * scale).round() as i32).clamp(1, MAX_CELL);
        let (w, h) = (side(cell_w), side(cell_h));
        Metrics::from_device(font_size, scale, w, h, (h as f32 * 0.8).round() as i32)
    }

    /// The metrics of cells already measured in device px.
    pub(super) fn from_device(
        font_size: f32,
        scale: f32,
        w: i32,
        h: i32,
        baseline: i32,
    ) -> Metrics {
        let (w, h) = (w.clamp(1, MAX_CELL), h.clamp(1, MAX_CELL));
        Metrics {
            font_size: sane(font_size, 1.0).min(512.0),
            scale: sane(scale, 1.0).min(64.0),
            cell_w: w,
            cell_h: h,
            baseline: baseline.clamp(0, h),
        }
    }

    /// The font size the cells were made for, in logical px.
    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    /// The display scale the cells were made for. Drawing with a painter of another scale is
    /// allowed (the cells keep their device size), but the logical answers here are then off.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// The width of a cell in logical px (a whole number of device px divided by the scale).
    pub fn cell_w(&self) -> f32 {
        self.cell_w as f32 / self.scale
    }

    /// The height of a cell in logical px.
    pub fn cell_h(&self) -> f32 {
        self.cell_h as f32 / self.scale
    }

    /// The width and height of a cell in device px.
    pub fn cell_device(&self) -> (i32, i32) {
        (self.cell_w, self.cell_h)
    }

    /// The font size in device px.
    pub(super) fn font_device(&self) -> f32 {
        self.font_size * self.scale
    }

    /// Device px from the top of a cell to the baseline.
    pub(super) fn baseline(&self) -> i32 {
        self.baseline
    }

    /// The thickness of a thin line in device px: underlines, strike-through, light box lines,
    /// the outline of the hollow cursor. A twelfth of the font size, at least one pixel.
    pub(super) fn stroke(&self) -> i32 {
        ((self.font_device() / 12.0).round() as i32).clamp(1, (self.cell_h / 4).max(1))
    }

    /// The width of the bar cursor and the height of the underline cursor in device px: an
    /// eighth of the font size, at least one pixel.
    pub(super) fn cursor_thickness(&self) -> i32 {
        ((self.font_device() / 8.0).round() as i32).clamp(1, self.cell_w.min(self.cell_h))
    }

    /// Device px from the top of a cell to the top of an underline.
    pub(super) fn underline_y(&self) -> i32 {
        let below = ((self.font_device() * 0.1).round() as i32).max(1);
        (self.baseline + below)
            .min(self.cell_h - self.stroke())
            .max(0)
    }

    /// Device px from the top of a cell to the top of a strike-through line: three tenths of the
    /// font size above the baseline, as [`Painter::strikethrough_range`] puts it.
    ///
    /// [`Painter::strikethrough_range`]: crate::gfx::Painter::strikethrough_range
    pub(super) fn strike_y(&self) -> i32 {
        (self.baseline - (0.3 * self.font_device()).round() as i32).clamp(0, self.cell_h - 1)
    }

    /// The device rectangle `area` covers, with the painter at no offset: x0, y0, x1, y1. A
    /// rectangle with a side that is not finite covers nothing.
    fn device(&self, area: Rect) -> (i32, i32, i32, i32) {
        let ok = [area.x, area.y, area.w, area.h]
            .iter()
            .all(|v| v.is_finite());
        if !ok {
            return (0, 0, 0, 0);
        }
        let d = |v: f32| (v * self.scale).round() as i32;
        let (x0, y0) = (d(area.x), d(area.y));
        (x0, y0, d(area.right()).max(x0), d(area.bottom()).max(y0))
    }

    /// How many whole columns and rows fit `area`: the size to give
    /// [`Screen::resize_reflow`](mdshared_term::Screen::resize_reflow) (or `resize`, which
    /// cuts its lines instead of wrapping them again) and the pseudo-terminal when the window
    /// changes. Zero when not even one cell fits (a `Screen` makes that one); zero for a
    /// rectangle with a side that is NaN or infinite.
    pub fn grid_size(&self, area: Rect) -> (usize, usize) {
        let (x0, y0, x1, y1) = self.device(area);
        (
            ((x1 - x0) / self.cell_w) as usize,
            ((y1 - y0) / self.cell_h) as usize,
        )
    }

    /// The cell under the point `x, y` of `area`, as (row, column) from the top left: for
    /// mouse reports and for starting a selection. `None` outside `area`, in the padding that is
    /// left over at its right and bottom, and for a point that is not a number. The cell is the
    /// one whose device pixels hold the point, so the answer agrees with what is drawn.
    pub fn cell_at(&self, area: Rect, x: f32, y: f32) -> Option<(usize, usize)> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let (x0, y0, x1, y1) = self.device(area);
        let (px, py) = ((x * self.scale).floor(), (y * self.scale).floor());
        if px < x0 as f32 || py < y0 as f32 || px >= x1 as f32 || py >= y1 as f32 {
            return None;
        }
        let (cols, rows) = self.grid_size(area);
        let col = ((px - x0 as f32) as i64 / self.cell_w as i64) as usize;
        let row = ((py - y0 as f32) as i64 / self.cell_h as i64) as usize;
        (col < cols && row < rows).then_some((row, col))
    }

    /// The cell nearest to the point `x, y` in a grid of `cols` by `rows` at the top left of
    /// `area`, as (row, column): a point outside takes the cell at the edge it is past. For a
    /// drag that leaves the grid while selecting or while a program reports the mouse. A grid
    /// of no columns or no rows gives (0, 0); a coordinate that is NaN counts as the grid's top
    /// or left edge.
    pub fn cell_clamped(
        &self,
        area: Rect,
        x: f32,
        y: f32,
        cols: usize,
        rows: usize,
    ) -> (usize, usize) {
        let (x0, y0, _, _) = self.device(area);
        let index = |v: f32, start: i32, side: i32, count: usize| {
            let at = ((v * self.scale).floor() - start as f32) / side as f32;
            if at.is_nan() || at < 0.0 || count == 0 {
                0
            } else {
                (at.floor() as usize).min(count - 1)
            }
        };
        (
            index(y, y0, self.cell_h, rows),
            index(x, x0, self.cell_w, cols),
        )
    }

    /// The rectangle of the cell at `row` and `col` of a grid at the top left of `area`, in
    /// logical px, whether or not it lies inside `area`. A wide character takes this cell and
    /// the next.
    pub fn cell_rect(&self, area: Rect, row: usize, col: usize) -> Rect {
        let (x0, y0, _, _) = self.device(area);
        let x = x0 as f64 + col as f64 * self.cell_w as f64;
        let y = y0 as f64 + row as f64 * self.cell_h as f64;
        let s = self.scale as f64;
        Rect::new((x / s) as f32, (y / s) as f32, self.cell_w(), self.cell_h())
    }

    /// The band of row `row` across the whole width of `area` (its padding at the right
    /// included), in logical px: what [`draw_rows`](super::draw_rows) repaints for that row, and
    /// so the damage an application reports for it.
    pub fn row_rect(&self, area: Rect, row: usize) -> Rect {
        let (x0, y0, x1, _) = self.device(area);
        let y = y0 as f64 + row as f64 * self.cell_h as f64;
        let s = self.scale as f64;
        Rect::new(
            (x0 as f64 / s) as f32,
            (y / s) as f32,
            ((x1 - x0) as f64 / s) as f32,
            self.cell_h(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_cell_is_rounded_to_whole_device_pixels() {
        let m = Metrics::fixed(14.0, 1.5, 8.4, 17.0);
        assert_eq!(m.cell_device(), (13, 26));
        assert_eq!(m.cell_w(), 13.0 / 1.5);
        assert_eq!(m.cell_h(), 26.0 / 1.5);
        assert_eq!(m.font_size(), 14.0);
        assert_eq!(m.scale(), 1.5);
    }

    #[test]
    fn nonsense_sizes_give_a_small_grid_and_no_panic() {
        for v in [f32::NAN, f32::INFINITY, -3.0, 0.0] {
            let m = Metrics::fixed(v, v, v, v);
            assert_eq!(m.cell_device(), (1, 1));
            assert_eq!(m.scale(), 1.0);
            assert_eq!(m.font_size(), 1.0);
            let r = Rect::new(v, v, v, v);
            assert_eq!(m.grid_size(r), (0, 0));
            assert_eq!(m.cell_at(r, 1.0, 1.0), None);
            let _ = m.cell_clamped(r, v, v, 10, 10);
            let _ = m.cell_rect(r, 3, 3);
        }
        let m = Metrics::fixed(14.0, 1.0, 1e9, 1e9);
        assert_eq!(m.cell_device(), (MAX_CELL, MAX_CELL));
    }

    #[test]
    fn the_grid_size_counts_whole_cells_only() {
        let m = Metrics::fixed(14.0, 1.0, 8.0, 16.0);
        assert_eq!(m.grid_size(Rect::new(0.0, 0.0, 800.0, 480.0)), (100, 30));
        assert_eq!(m.grid_size(Rect::new(3.0, 5.0, 807.4, 495.4)), (100, 30));
        assert_eq!(m.grid_size(Rect::new(3.0, 5.0, 807.9, 495.9)), (101, 31));
        assert_eq!(m.grid_size(Rect::new(0.0, 0.0, 7.4, 15.4)), (0, 0));
        assert_eq!(m.grid_size(Rect::new(0.0, 0.0, -50.0, 20.0)), (0, 1));
        // At a scale of 1.5 the cell is 12 by 24 device px.
        let m = Metrics::fixed(14.0, 1.5, 8.0, 16.0);
        assert_eq!(m.grid_size(Rect::new(0.0, 0.0, 800.0, 480.0)), (100, 30));
    }

    /// The plain way: walk every cell rectangle and see which holds the point.
    fn cell_by_search(m: &Metrics, area: Rect, x: f32, y: f32) -> Option<(usize, usize)> {
        let (cols, rows) = m.grid_size(area);
        let s = m.scale();
        let (px, py) = ((x * s).floor(), (y * s).floor());
        let inside = px >= (area.x * s).round()
            && py >= (area.y * s).round()
            && px < (area.right() * s).round()
            && py < (area.bottom() * s).round();
        if !inside {
            return None;
        }
        for row in 0..rows {
            for col in 0..cols {
                let r = m.cell_rect(area, row, col);
                let (x0, y0) = ((r.x * s).round(), (r.y * s).round());
                let (w, h) = m.cell_device();
                if px >= x0 && px < x0 + w as f32 && py >= y0 && py < y0 + h as f32 {
                    return Some((row, col));
                }
            }
        }
        None
    }

    #[test]
    fn the_cell_under_a_point_agrees_with_a_search_of_every_cell_rectangle() {
        let mut seed: u32 = 7;
        let mut next = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 8) as f32 / (1 << 24) as f32
        };
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let m = Metrics::fixed(13.0, scale, 7.8, 15.6);
            let area = Rect::new(10.3, 20.7, 203.0, 151.0);
            for _ in 0..600 {
                let (x, y) = (next() * 240.0, next() * 200.0);
                assert_eq!(
                    m.cell_at(area, x, y),
                    cell_by_search(&m, area, x, y),
                    "{scale} {x},{y}"
                );
            }
        }
    }

    #[test]
    fn a_point_outside_the_grid_takes_the_nearest_cell_when_clamped() {
        let m = Metrics::fixed(14.0, 1.0, 8.0, 16.0);
        let area = Rect::new(100.0, 50.0, 800.0, 480.0);
        assert_eq!(m.cell_clamped(area, 0.0, 0.0, 80, 24), (0, 0));
        assert_eq!(m.cell_clamped(area, 5000.0, 5000.0, 80, 24), (23, 79));
        assert_eq!(m.cell_clamped(area, 117.0, 83.0, 80, 24), (2, 2));
        assert_eq!(m.cell_clamped(area, 117.0, 83.0, 0, 24), (2, 0));
        assert_eq!(m.cell_clamped(area, f32::NAN, f32::NAN, 80, 24), (0, 0));
        // Inside, the clamped answer is the plain one.
        assert_eq!(m.cell_at(area, 117.0, 83.0), Some((2, 2)));
        // The padding at the right is no cell, but clamps to the last column.
        let area = Rect::new(0.0, 0.0, 85.0, 40.0);
        assert_eq!(m.cell_at(area, 82.0, 5.0), None);
        assert_eq!(m.cell_clamped(area, 82.0, 5.0, 10, 2), (0, 9));
    }

    #[test]
    fn cells_and_rows_are_placed_from_the_rounded_corner() {
        let m = Metrics::fixed(14.0, 1.0, 8.0, 16.0);
        let area = Rect::new(10.4, 20.6, 100.0, 100.0);
        assert_eq!(m.cell_rect(area, 2, 3), Rect::new(34.0, 53.0, 8.0, 16.0));
        assert_eq!(m.row_rect(area, 1), Rect::new(10.0, 37.0, 100.0, 16.0));
        // Far down a grid the rectangle is still exact.
        let r = m.cell_rect(Rect::new(0.0, 0.0, 1.0, 1.0), 2000, 2000);
        assert_eq!((r.x, r.y), (16000.0, 32000.0));
    }

    #[test]
    fn the_lines_of_a_cell_stay_inside_it() {
        for (size, h) in [(6.0, 7.0), (14.0, 17.0), (30.0, 36.0), (14.0, 3.0)] {
            let m = Metrics::fixed(size, 1.0, size * 0.6, h);
            let (_, ch) = m.cell_device();
            assert!(m.stroke() >= 1);
            assert!(m.underline_y() >= 0 && m.underline_y() + m.stroke() <= ch.max(m.stroke()));
            assert!(m.strike_y() >= 0 && m.strike_y() < ch);
            assert!(m.cursor_thickness() >= 1);
        }
    }
}
