//! The arithmetic of a grid of cells of one size, filled row by row: an icon view, an album
//! wall, a sheet of thumbnails. How many columns fit, which cells the viewport shows, where
//! one is, which one a point falls in, where a key moves the cursor, what offset brings a cell
//! into view, and which cells a rubber band touches. It mirrors [`rows::Rows`](crate::rows) and
//! like it draws nothing and knows nothing of an event loop: the caller keeps the count and
//! the offset, draws each cell inside the rectangle [`Grid::rect`] names, and owns the hits
//! and the keyboard. The selection is a [`select::Selection`](crate::select::Selection) over
//! the same indices.
//!
//! It refuses cells of different sizes, groups with headings between them, and a grid that
//! fills column by column.

use std::ops::Range;

use crate::gfx::Rect;
use crate::scroll;

/// A key that moves the cursor of a grid. A page is the rows the viewport holds whole, never
/// fewer than one.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Nav {
    Left,
    Right,
    Up,
    Down,
    /// The first cell of the grid, and the last.
    Home,
    End,
    PageUp,
    PageDown,
}

/// The rectangle between two corners, whichever way round they are: the rubber band from where
/// the press was to where the pointer is.
pub fn band_rect(from: (f32, f32), to: (f32, f32)) -> Rect {
    Rect::new(
        from.0.min(to.0),
        from.1.min(to.1),
        (from.0 - to.0).abs(),
        (from.1 - to.1).abs(),
    )
}

/// A grid of `count` cells seen through a viewport `view_w` by `view_h`, scrolled down by
/// `scroll`. A cell's pitch is its size plus any gap the caller wants between cells; the
/// caller draws the cell itself inside its rectangle, leaving the gap free.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Grid {
    /// The pitch of one cell across and down.
    pub cell_w: f32,
    pub cell_h: f32,
    /// The size of the grid's viewport.
    pub view_w: f32,
    pub view_h: f32,
    /// How many cells there are.
    pub count: usize,
    /// The offset the grid is scrolled by, from the top of the first row. Answers clamp it, as
    /// the answers of [`Rows`](crate::rows::Rows) do.
    pub scroll: f32,
    /// Whether the columns are widened to fill the viewport: the width left over after as many
    /// columns as fit is shared among them, so the cells reach both sides. Off, the columns
    /// keep their pitch and the spare width is empty on the right.
    pub stretch: bool,
}

impl Grid {
    /// A grid at its top, its columns at their own pitch.
    pub fn new(cell_w: f32, cell_h: f32, view_w: f32, view_h: f32, count: usize) -> Grid {
        Grid {
            cell_w,
            cell_h,
            view_w,
            view_h,
            count,
            scroll: 0.0,
            stretch: false,
        }
    }

    /// The same grid with its columns widened to fill the viewport.
    pub fn stretched(mut self) -> Grid {
        self.stretch = true;
        self
    }

    /// How many columns fit across the viewport: at least one, so a viewport narrower than a
    /// cell still shows a column (cut off) and every answer below has something to divide by.
    pub fn columns(&self) -> usize {
        if self.cell_w <= 0.0 || !self.view_w.is_finite() {
            return 1;
        }
        ((self.view_w / self.cell_w).floor() as usize).max(1)
    }

    /// The pitch of a column as drawn: the cell's own, or wider when the grid is
    /// [`stretch`](Self::stretch)ed.
    pub fn pitch_w(&self) -> f32 {
        if self.stretch && self.view_w > self.cell_w * self.columns() as f32 {
            self.view_w / self.columns() as f32
        } else {
            self.cell_w
        }
    }

    /// How many rows the cells fill; the last may be short.
    pub fn row_count(&self) -> usize {
        self.count.div_ceil(self.columns())
    }

    /// The height of all the rows together.
    pub fn content(&self) -> f32 {
        self.row_count() as f32 * self.cell_h
    }

    /// The furthest the grid can be scrolled; 0 when every row fits.
    pub fn max_scroll(&self) -> f32 {
        (self.content() - self.view_h).max(0.0)
    }

    /// `scroll` kept between the top of the grid and [`max_scroll`](Self::max_scroll).
    pub fn clamp_scroll(&self, scroll: f32) -> f32 {
        scroll.clamp(0.0, self.max_scroll())
    }

    /// The rows the viewport touches: the first, and one past the last.
    fn visible_rows(&self) -> Range<usize> {
        let rows = self.row_count();
        if rows == 0 || self.cell_h <= 0.0 || self.view_h <= 0.0 {
            return 0..0;
        }
        let scroll = self.clamp_scroll(self.scroll);
        let first = ((scroll / self.cell_h).floor() as usize).min(rows);
        let end = (((scroll + self.view_h) / self.cell_h).ceil() as usize).clamp(first, rows);
        first..end
    }

    /// The cells the viewport touches: the first, and one past the last, whole rows of them
    /// (the last row of the grid may be short). Empty for an empty grid. What the caller draws.
    pub fn visible(&self) -> Range<usize> {
        let rows = self.visible_rows();
        let cols = self.columns();
        (rows.start * cols).min(self.count)..(rows.end * cols).min(self.count)
    }

    /// The row and the column of cell `i`.
    pub fn place(&self, i: usize) -> (usize, usize) {
        (i / self.columns(), i % self.columns())
    }

    /// Where cell `i` is in `area` at the current offset, whether or not it is on screen: one
    /// pitch across and down. A cell past the last is answered like any other; the caller asks
    /// [`visible`](Self::visible) first.
    pub fn rect(&self, area: Rect, i: usize) -> Rect {
        let (row, col) = self.place(i);
        let w = self.pitch_w();
        Rect::new(
            area.x + col as f32 * w,
            area.y + row as f32 * self.cell_h - self.scroll,
            w,
            self.cell_h,
        )
    }

    /// Which cell a point falls in, or `None` outside `area`, to the right of the last column,
    /// or where the last row has no cell. The scroll is taken into account, so the answer is
    /// the cell drawn there. The gap a caller leaves inside a cell is part of the cell here; a
    /// caller that wants presses in the gaps to miss tests its own inner rectangle.
    pub fn cell_at(&self, area: Rect, x: f32, y: f32) -> Option<usize> {
        if self.cell_h <= 0.0 || !area.contains(x, y) {
            return None;
        }
        let w = self.pitch_w();
        if w <= 0.0 {
            return None;
        }
        let (col, row) = ((x - area.x) / w, (y - area.y + self.scroll) / self.cell_h);
        if !col.is_finite() || !row.is_finite() || col < 0.0 || row < 0.0 {
            return None;
        }
        let (col, row) = (col.floor() as usize, row.floor() as usize);
        if col >= self.columns() {
            return None;
        }
        let i = row.checked_mul(self.columns())?.checked_add(col)?;
        (i < self.count).then_some(i)
    }

    /// The overlay scrollbar's track and thumb for `area`, or `None` when every row fits.
    pub fn thumb(&self, area: Rect) -> Option<(Rect, Rect)> {
        scroll::scrollbar(
            area,
            self.content(),
            self.view_h,
            self.clamp_scroll(self.scroll),
        )
    }

    /// How many rows a page key moves: the whole rows the viewport holds, at least one.
    pub fn page(&self) -> usize {
        if self.cell_h <= 0.0 {
            return 1;
        }
        ((self.view_h / self.cell_h).floor() as usize).max(1)
    }

    /// Where `nav` moves the cursor from `cursor`. `None` for an empty grid. With no cursor
    /// the forward keys start at the first cell and the others at the last; a cursor past the
    /// last cell counts as the last one.
    ///
    /// Left and Right step through the cells in order, from the end of a row to the start of
    /// the next, and stop at the first and last cell. Up and Down keep the column; Down from
    /// the row above a short last row, in a column that row does not have, goes to the last
    /// cell (so Down always reaches the end), and Up from the first row or Down from the last
    /// stays. The page keys move by [`page`](Self::page) rows the same way.
    pub fn nav(&self, cursor: Option<usize>, nav: Nav) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        let last = self.count - 1;
        let cols = self.columns();
        let Some(at) = cursor.map(|c| c.min(last)) else {
            return Some(match nav {
                Nav::Right | Nav::Down | Nav::Home | Nav::PageDown => 0,
                Nav::Left | Nav::Up | Nav::End | Nav::PageUp => last,
            });
        };
        // Down by `rows` rows in the same column, as far as there are rows.
        let down = |rows: usize| {
            let (row, col) = (at / cols, at % cols);
            let to_row = (row + rows).min(last / cols);
            if to_row == row {
                at
            } else {
                (to_row * cols + col).min(last)
            }
        };
        let up = |rows: usize| at - rows.min(at / cols) * cols;
        Some(match nav {
            Nav::Left => at.saturating_sub(1),
            Nav::Right => (at + 1).min(last),
            Nav::Up => up(1),
            Nav::Down => down(1),
            Nav::Home => 0,
            Nav::End => last,
            Nav::PageUp => up(self.page()),
            Nav::PageDown => down(self.page()),
        })
    }

    /// The offset that shows the row of cell `i` whole, moved only when it is not whole
    /// already: a row above the viewport goes to its top, one below it to its bottom. The
    /// answer stays within the grid.
    pub fn reveal(&self, i: usize) -> f32 {
        let top = (i / self.columns()) as f32 * self.cell_h;
        let bottom = top + self.cell_h;
        let scroll = self.clamp_scroll(self.scroll);
        let to = if top < scroll {
            top
        } else if bottom > scroll + self.view_h {
            bottom - self.view_h
        } else {
            scroll
        };
        self.clamp_scroll(to)
    }

    /// The cells a rubber band touches, in order: every cell whose rectangle the band overlaps,
    /// on screen or not. `band` is in the coordinates of `area` and of this frame (see
    /// [`band_rect`]); a caller whose grid scrolls while the band is held keeps the press point
    /// in the content's coordinates (its `y` plus the scroll at the press) and takes the scroll
    /// off again each frame, so the band's fixed corner stays on the cell it started on. A
    /// band with no area still touches the cell it lies in, so a press that has not moved yet
    /// selects what is under it.
    ///
    /// As with [`cell_at`](Self::cell_at) the whole pitch is the cell; a caller whose cells
    /// have wide gaps filters the answer by its own inner rectangles.
    pub fn band(&self, area: Rect, band: Rect) -> Vec<usize> {
        let (w, h) = (self.pitch_w(), self.cell_h);
        let finite = [band.x, band.y, band.w, band.h]
            .iter()
            .all(|v| v.is_finite());
        if self.count == 0 || w <= 0.0 || h <= 0.0 || !finite {
            return Vec::new();
        }
        let cols = self.columns();
        // The band in the grid's own coordinates: from its first cell's corner, unscrolled.
        let (x0, y0) = (band.x - area.x, band.y - area.y + self.scroll);
        let (x1, y1) = (x0 + band.w.max(0.0), y0 + band.h.max(0.0));
        if x1 < 0.0 || y1 < 0.0 || x0 >= cols as f32 * w {
            return Vec::new();
        }
        let first = |v: f32, pitch: f32| (v.max(0.0) / pitch).floor() as usize;
        // A far edge exactly on a boundary does not touch the cell beyond it, unless the band
        // has no extent there at all.
        let end = |from: f32, to: f32, pitch: f32, limit: usize| {
            let cells = if to > from {
                (to / pitch).ceil() as usize
            } else {
                (to.max(0.0) / pitch).floor() as usize + 1
            };
            cells.min(limit)
        };
        let (c0, c1) = (first(x0, w), end(x0, x1, w, cols));
        let (r0, r1) = (first(y0, h), end(y0, y1, h, self.row_count()));
        let mut out = Vec::new();
        for row in r0..r1 {
            for col in c0..c1 {
                let i = row * cols + col;
                if i < self.count {
                    out.push(i);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(10.0, 200.0, 350.0, 250.0);

    /// 23 cells of 100 by 100 in a view 350 by 250: three columns, eight rows, the last with
    /// two cells.
    fn grid() -> Grid {
        Grid::new(100.0, 100.0, 350.0, 250.0, 23)
    }

    #[test]
    fn the_cells_fill_rows_of_as_many_columns_as_fit() {
        let g = grid();
        assert_eq!((g.columns(), g.row_count(), g.pitch_w()), (3, 8, 100.0));
        assert_eq!((g.content(), g.max_scroll(), g.page()), (800.0, 550.0, 2));
        assert_eq!(g.visible(), 0..9, "two rows and the top of a third");
        assert_eq!(g.rect(AREA, 0), Rect::new(10.0, 200.0, 100.0, 100.0));
        assert_eq!(g.rect(AREA, 4), Rect::new(110.0, 300.0, 100.0, 100.0));
        assert_eq!((g.place(4), g.place(22)), ((1, 1), (7, 1)));
        assert_eq!(g.thumb(AREA), scroll::scrollbar(AREA, 800.0, 250.0, 0.0));

        let end = Grid { scroll: 550.0, ..g };
        assert_eq!(end.visible(), 15..23);
        assert_eq!(end.rect(AREA, 22).bottom(), AREA.bottom());
        let middle = Grid { scroll: 100.0, ..g };
        assert_eq!(middle.visible(), 3..12);
        // An offset past either end is clamped for the range, as in a list.
        assert_eq!(Grid { scroll: 9e9, ..g }.visible(), 15..23);
        assert_eq!(Grid { scroll: -50.0, ..g }.visible(), 0..9);

        // Stretched, the three columns share the 350 px.
        let wide = g.stretched();
        assert!((wide.pitch_w() - 350.0 / 3.0).abs() < 1e-4);
        assert!((wide.rect(AREA, 2).right() - AREA.right()).abs() < 1e-3);
        assert_eq!(wide.columns(), 3);
        // A view narrower than a cell still has a column; one that fits everything has no
        // scroll.
        let narrow = Grid::new(100.0, 100.0, 60.0, 250.0, 5);
        assert_eq!((narrow.columns(), narrow.row_count()), (1, 5));
        assert_eq!(narrow.stretched().pitch_w(), 100.0);
        let all = Grid::new(100.0, 100.0, 1000.0, 400.0, 7);
        assert_eq!(
            (all.columns(), all.row_count(), all.max_scroll()),
            (10, 1, 0.0)
        );
        assert_eq!(all.visible(), 0..7);
    }

    #[test]
    fn a_point_falls_in_the_cell_drawn_there_or_in_none() {
        let g = Grid {
            scroll: 150.0,
            ..grid()
        };
        assert_eq!(
            g.cell_at(AREA, 10.0, 200.0),
            Some(3),
            "the second row, half scrolled"
        );
        assert_eq!(g.cell_at(AREA, 110.0, 249.9), Some(4));
        assert_eq!(g.cell_at(AREA, 110.0, 250.0), Some(7));
        assert_eq!(g.cell_at(AREA, 309.9, 449.9), Some(11));
        // Right of the last column, and outside the area.
        assert_eq!(g.cell_at(AREA, 310.0, 300.0), None);
        assert_eq!(g.cell_at(AREA, 9.9, 300.0), None);
        assert_eq!(g.cell_at(AREA, 100.0, 199.9), None);
        assert_eq!(g.cell_at(AREA, 100.0, 450.0), None);
        // The short last row: its third place has no cell.
        let end = Grid {
            scroll: 550.0,
            ..grid()
        };
        assert_eq!(end.cell_at(AREA, 150.0, 440.0), Some(22));
        assert_eq!(end.cell_at(AREA, 250.0, 440.0), None);
        // Every visible cell is found at its own middle.
        for i in g.visible() {
            let r = g.rect(AREA, i);
            if AREA.contains(r.cx(), r.cy()) {
                assert_eq!(g.cell_at(AREA, r.cx(), r.cy()), Some(i));
            }
        }
        // Stretched, the hit follows the wider columns.
        let wide = grid().stretched();
        assert_eq!(wide.cell_at(AREA, 10.0 + 120.0, 210.0), Some(1));
        assert_eq!(wide.cell_at(AREA, 10.0 + 340.0, 210.0), Some(2));
        assert_eq!(
            Grid::new(0.0, 0.0, 100.0, 100.0, 5).cell_at(AREA, 20.0, 210.0),
            None
        );
    }

    #[test]
    fn the_keys_move_in_two_directions_and_stop_at_the_edges() {
        let g = grid();
        // No cursor.
        assert_eq!(g.nav(None, Nav::Right), Some(0));
        assert_eq!(g.nav(None, Nav::Down), Some(0));
        assert_eq!(g.nav(None, Nav::Left), Some(22));
        assert_eq!(g.nav(None, Nav::PageUp), Some(22));
        // Along the order, across the ends of rows.
        assert_eq!(g.nav(Some(0), Nav::Left), Some(0));
        assert_eq!(g.nav(Some(2), Nav::Right), Some(3));
        assert_eq!(g.nav(Some(3), Nav::Left), Some(2));
        assert_eq!(g.nav(Some(22), Nav::Right), Some(22));
        // Up and down keep the column.
        assert_eq!(g.nav(Some(4), Nav::Down), Some(7));
        assert_eq!(g.nav(Some(4), Nav::Up), Some(1));
        assert_eq!(g.nav(Some(1), Nav::Up), Some(1), "the first row holds");
        assert_eq!(g.nav(Some(19), Nav::Down), Some(22));
        // The last row has two cells: from the third column above it, Down goes to the last.
        assert_eq!(g.nav(Some(20), Nav::Down), Some(22));
        assert_eq!(g.nav(Some(22), Nav::Down), Some(22), "the last row holds");
        assert_eq!(g.nav(Some(21), Nav::Down), Some(21));
        // Pages of two rows.
        assert_eq!(g.nav(Some(4), Nav::PageDown), Some(10));
        assert_eq!(g.nav(Some(10), Nav::PageUp), Some(4));
        assert_eq!(g.nav(Some(4), Nav::PageUp), Some(1));
        assert_eq!(g.nav(Some(17), Nav::PageDown), Some(22));
        assert_eq!(g.nav(Some(16), Nav::PageDown), Some(22));
        assert_eq!(
            (g.nav(Some(9), Nav::Home), g.nav(Some(9), Nav::End)),
            (Some(0), Some(22))
        );
        // A cursor past the end counts as the last cell; an empty grid has no answer.
        assert_eq!(g.nav(Some(99), Nav::Left), Some(21));
        let empty = Grid { count: 0, ..g };
        for nav in [Nav::Left, Nav::Down, Nav::Home, Nav::End] {
            assert_eq!(empty.nav(Some(3), nav), None);
        }
        assert_eq!((empty.visible(), empty.row_count()), (0..0, 0));
        // One row: Up and Down stay.
        let row = Grid::new(100.0, 100.0, 1000.0, 400.0, 7);
        assert_eq!(
            (row.nav(Some(3), Nav::Down), row.nav(Some(3), Nav::Up)),
            (Some(3), Some(3))
        );
    }

    #[test]
    fn reveal_moves_only_when_the_row_is_not_whole() {
        let g = grid();
        assert_eq!(g.reveal(0), 0.0);
        assert_eq!(g.reveal(5), 0.0, "the second row is whole");
        assert_eq!(g.reveal(6), 50.0, "the third row comes up to the bottom");
        assert_eq!(g.reveal(22), 550.0);
        let down = Grid { scroll: 320.0, ..g };
        assert_eq!(
            down.reveal(9),
            300.0,
            "a row cut at the top goes to the top"
        );
        assert_eq!(down.reveal(12), 320.0);
        assert_eq!(down.reveal(0), 0.0);
        // Revealing where each key leads keeps the cursor on screen all the way down.
        let mut g = grid();
        let mut cursor = Some(0);
        for _ in 0..12 {
            cursor = g.nav(cursor, Nav::Down);
            g.scroll = g.reveal(cursor.unwrap());
            let r = g.rect(AREA, cursor.unwrap());
            assert!(r.y >= AREA.y - 0.01 && r.bottom() <= AREA.bottom() + 0.01);
        }
        assert_eq!(cursor, Some(21));
    }

    #[test]
    fn a_rubber_band_takes_every_cell_it_touches() {
        let g = grid();
        let band = |a: (f32, f32), b: (f32, f32)| g.band(AREA, band_rect(a, b));
        // Inside one cell, and a press that has not moved.
        assert_eq!(band((120.0, 210.0), (180.0, 280.0)), vec![1]);
        assert_eq!(band((120.0, 210.0), (120.0, 210.0)), vec![1]);
        // Across two columns and two rows, drawn from any corner.
        let four = vec![0, 1, 3, 4];
        assert_eq!(band((50.0, 250.0), (150.0, 350.0)), four);
        assert_eq!(band((150.0, 350.0), (50.0, 250.0)), four);
        assert_eq!(band((150.0, 250.0), (50.0, 350.0)), four);
        // Ending exactly on a boundary does not take the cell beyond it.
        assert_eq!(band((50.0, 250.0), (110.0, 300.0)), vec![0]);
        assert_eq!(band((50.0, 250.0), (110.1, 300.1)), four);
        // Reaching outside the area on every side: everything in the rows it covers.
        assert_eq!(
            band((-500.0, 100.0), (900.0, 390.0)),
            vec![0, 1, 2, 3, 4, 5]
        );
        // To the right of the columns, or above the grid: nothing.
        assert_eq!(band((320.0, 250.0), (340.0, 350.0)), Vec::<usize>::new());
        assert_eq!(band((50.0, 100.0), (150.0, 150.0)), Vec::<usize>::new());
        // Scrolled: the band is in this frame's coordinates, the cells are the ones drawn there.
        let scrolled = Grid { scroll: 550.0, ..g };
        assert_eq!(
            scrolled.band(AREA, band_rect((20.0, 290.0), (300.0, 440.0))),
            vec![18, 19, 20, 21, 22],
            "the short last row gives the cells it has"
        );
        // Below the screen too: the band is not cut to the viewport.
        assert_eq!(
            band((20.0, 210.0), (30.0, 5000.0)),
            vec![0, 3, 6, 9, 12, 15, 18, 21]
        );
        // Against the plain test of each cell's rectangle, for many bands.
        let mut seed: u32 = 7;
        let mut next = move |span: f32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / 16_777_216.0 * span
        };
        for _ in 0..500 {
            let g = Grid {
                scroll: next(550.0).floor(),
                ..grid()
            };
            let a = (next(450.0) - 40.0, next(350.0) + 150.0);
            let b = (next(450.0) - 40.0, next(350.0) + 150.0);
            let band = band_rect(a, b);
            let plain: Vec<usize> = (0..g.count)
                .filter(|&i| {
                    let r = g.rect(AREA, i);
                    band.x < r.right()
                        && band.right() > r.x
                        && band.y < r.bottom()
                        && band.bottom() > r.y
                })
                .collect();
            assert_eq!(g.band(AREA, band), plain, "{band:?} at {}", g.scroll);
        }
        // Nonsense in, nothing out.
        assert!(
            g.band(AREA, Rect::new(f32::NAN, 0.0, 10.0, 10.0))
                .is_empty()
        );
        assert!(Grid { count: 0, ..g }.band(AREA, AREA).is_empty());
    }
}
