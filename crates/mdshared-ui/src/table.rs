//! The columns of a table: how wide each is in a given width, which one a point in the header
//! falls in and whether it is on an edge that resizes, the drag of such an edge, which column
//! the rows are sorted by, the rectangle of a cell, and the header's drawing. The rows
//! themselves are a [`rows::Rows`](crate::rows::Rows) the caller keeps beside it: this module
//! knows columns and nothing of what is in them.
//!
//! The model ([`Table`]) holds state and answers questions; [`draw_header`] draws from it and
//! changes nothing. The caller owns the events: it asks [`Table::hit`] where the pointer is
//! (to pick the resize cursor), passes a press to [`Table::press`], moves to
//! [`Table::drag`], the release to [`Table::release`], and calls
//! [`Table::toggle_sort`] itself when a press and its release were on the same title. Sorting
//! the rows is the caller's too: the table only says by which column and which way.
//!
//! Deliberately not here: columns that can be hidden or put in another order, a column that
//! shrinks below its minimum, cells that span columns, and a header taller than one line. A
//! table too wide for its view is wider than its view: the caller scrolls it sideways (it gives
//! the header and the rows a rectangle whose left edge is moved by the scroll) or clips it.

use crate::gfx::{Face, Painter, Rect, mix};
use crate::glyph;
use crate::theme::Palette;

/// The height a header wants.
pub const HEADER_H: f32 = 30.0;
/// What a cell keeps free on each side of its text.
pub const CELL_PAD: f32 = 10.0;
/// How far on either side of a column's right edge a press takes hold of the edge.
pub const EDGE_GRIP: f32 = 4.0;
/// The size of a header's title and of the text [`draw_cell_text`] draws.
const TITLE_TEXT: f32 = 12.5;
const SORT_ICON: f32 = 12.0;

/// Where the content of a column sits in its cells.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    /// For numbers: sizes, counts, lengths of time.
    Right,
}

/// One column. `id` is the caller's name for it, which the saved widths and the sort are kept
/// by, so the columns can be listed in another order, or one can be added, without the saved
/// line meaning something else.
#[derive(Clone, PartialEq, Debug)]
pub struct Column {
    pub id: u32,
    pub title: String,
    /// The width the column has before any spare width is shared out. A drag of its edge
    /// changes this.
    pub width: f32,
    /// The least a drag or a saved line can make it.
    pub min: f32,
    /// Its share of the width the columns do not use: 0 takes none, and of two columns with 1
    /// and 2 the second takes twice as much. A table with no such column leaves the spare
    /// width empty on the right.
    pub flex: f32,
    pub align: Align,
    /// Whether a click on its title sorts by it.
    pub sortable: bool,
    /// Whether its right edge can be dragged.
    pub resizable: bool,
}

impl Column {
    /// A left-aligned column `width` wide that can be sorted by and resized, no narrower than
    /// 40 px, taking no spare width.
    pub fn new(id: u32, title: impl Into<String>, width: f32) -> Column {
        Column {
            id,
            title: title.into(),
            width,
            min: 40.0,
            flex: 0.0,
            align: Align::Left,
            sortable: true,
            resizable: true,
        }
    }

    pub fn min(mut self, min: f32) -> Column {
        self.min = min;
        self
    }

    pub fn flex(mut self, flex: f32) -> Column {
        self.flex = flex;
        self
    }

    pub fn align(mut self, align: Align) -> Column {
        self.align = align;
        self
    }

    /// A column whose title does nothing when clicked (an icon column, say).
    pub fn unsortable(mut self) -> Column {
        self.sortable = false;
        self
    }

    /// A column whose edge cannot be dragged.
    pub fn fixed(mut self) -> Column {
        self.resizable = false;
        self
    }
}

/// Where a column is across the table: from the table's own left edge, whatever the view shows
/// of it.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Span {
    pub x: f32,
    pub w: f32,
}

/// Which column the rows are sorted by, and which way.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Sort {
    /// A [`Column::id`].
    pub column: u32,
    pub descending: bool,
}

/// What a point in the header is on. The number is the column's index, as in
/// [`Table::columns`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum HeaderHit {
    /// The title of a column: a click sorts by it, if it is [`sortable`](Column::sortable).
    Title(usize),
    /// The right edge of a column that can be resized: a drag changes its width, and the
    /// pointer over it is the east-west resize cursor.
    Edge(usize),
}

/// An edge held by the pointer.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Drag {
    column: usize,
    /// Where the pointer was when it took hold, and the column's width then.
    from_x: f32,
    from_w: f32,
}

/// The columns of one table, their widths as last laid out, the sort and an edge in hand.
#[derive(Clone, PartialEq, Debug)]
pub struct Table {
    columns: Vec<Column>,
    spans: Vec<Span>,
    /// The width [`layout`](Table::layout) was last asked for.
    view_w: f32,
    sort: Option<Sort>,
    drag: Option<Drag>,
}

impl Table {
    /// A table of these columns, not sorted, laid out for no width yet: call
    /// [`layout`](Self::layout) before asking where anything is.
    pub fn new(columns: Vec<Column>) -> Table {
        let mut t = Table {
            columns,
            spans: Vec::new(),
            view_w: 0.0,
            sort: None,
            drag: None,
        };
        t.layout(0.0);
        t
    }

    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// The index of the column with this id.
    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.columns.iter().position(|c| c.id == id)
    }

    /// Lays the columns out for a view `view_w` wide and returns the width of the table. Call
    /// it whenever the view's width may have changed; every other answer is from the last
    /// call.
    ///
    /// Each column gets its width, never less than its minimum. Width left over is shared
    /// among the columns with a [`flex`](Column::flex), by their shares. When the columns are
    /// wider than the view, those with a flex give up what is missing, by their shares and
    /// down to their minimums; what is still missing makes the table wider than the view (the
    /// returned width says by how much). Edges land on whole pixels, so the hairlines between
    /// columns do not blur.
    pub fn layout(&mut self, view_w: f32) -> f32 {
        self.view_w = view_w;
        let mut widths: Vec<f32> = self
            .columns
            .iter()
            .map(|c| clean(c.width).max(clean(c.min)))
            .collect();
        let spare = view_w - widths.iter().sum::<f32>();
        if spare > 0.0 {
            let shares: f32 = self.columns.iter().map(|c| clean(c.flex)).sum();
            if shares > 0.0 {
                for (w, c) in widths.iter_mut().zip(&self.columns) {
                    *w += spare * clean(c.flex) / shares;
                }
            }
        } else if spare < 0.0 {
            // Taken in rounds: a column that reaches its minimum drops out and the others
            // give more.
            let mut missing = -spare;
            for _ in 0..self.columns.len() {
                let shares: f32 = (self.columns.iter().zip(&widths))
                    .filter(|(c, w)| **w > clean(c.min))
                    .map(|(c, _)| clean(c.flex))
                    .sum();
                if shares <= 0.0 || missing <= 0.01 {
                    break;
                }
                let round = missing;
                for (w, c) in widths.iter_mut().zip(&self.columns) {
                    let room = *w - clean(c.min);
                    if room > 0.0 {
                        let take = (round * clean(c.flex) / shares).min(room);
                        *w -= take;
                        missing -= take;
                    }
                }
            }
        }
        // Whole-pixel edges; the rounding error of one column goes to the next.
        self.spans.clear();
        let mut x = 0.0f32;
        let mut edge = 0.0f32;
        for w in widths {
            edge += w;
            let right = edge.round().max(x);
            self.spans.push(Span { x, w: right - x });
            x = right;
        }
        x
    }

    /// Where each column is, as last laid out.
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// The width of the table as last laid out.
    pub fn width(&self) -> f32 {
        self.spans.last().map_or(0.0, |s| s.x + s.w)
    }

    /// The rectangle of column `k` in `row`: a row of the list, or the header. `row.x` is
    /// where the table's left edge is (the view's left edge, less the sideways scroll); the
    /// row's width is not used. A column that does not exist gets an empty rectangle at the
    /// table's right edge.
    pub fn cell(&self, row: Rect, k: usize) -> Rect {
        match self.spans.get(k) {
            Some(s) => Rect::new(row.x + s.x, row.y, s.w, row.h),
            None => Rect::new(row.x + self.width(), row.y, 0.0, row.h),
        }
    }

    /// The column a point's `x` falls in, in a row or header whose table starts at `row.x`.
    pub fn column_at(&self, row: Rect, x: f32) -> Option<usize> {
        let at = x - row.x;
        self.spans.iter().position(|s| at >= s.x && at < s.x + s.w)
    }

    /// What the point is on in the header drawn at `header` (`header.x` is the table's left
    /// edge, as for [`cell`](Self::cell)): an edge that can be dragged, within
    /// [`EDGE_GRIP`] of it on either side, before a title; `None` outside the header's height
    /// or past the last column. While an edge is in hand the answer is that edge wherever the
    /// pointer is, so the cursor does not flicker during a drag.
    pub fn hit(&self, header: Rect, x: f32, y: f32) -> Option<HeaderHit> {
        if let Some(d) = self.drag {
            return Some(HeaderHit::Edge(d.column));
        }
        if y < header.y || y >= header.bottom() {
            return None;
        }
        let at = x - header.x;
        // The nearest edge wins, so a narrow column between two edges keeps both reachable.
        let edge = (self.spans.iter().enumerate())
            .filter(|(k, _)| self.columns[*k].resizable)
            .map(|(k, s)| (k, (at - (s.x + s.w)).abs()))
            .filter(|(_, d)| *d <= EDGE_GRIP)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((k, _)) = edge {
            return Some(HeaderHit::Edge(k));
        }
        self.column_at(header, x).map(HeaderHit::Title)
    }

    /// A press of the primary button in the header. On an edge it takes hold of it (and
    /// returns that edge); on a title it changes nothing and returns the title, for the caller
    /// to remember until the release.
    ///
    /// Taking hold of an edge makes every column's own width what it is laid out at just now,
    /// and takes the flex from the column whose edge it is: from here on that column is as
    /// wide as the user made it, and the spare width goes to the others.
    pub fn press(&mut self, header: Rect, x: f32, y: f32) -> Option<HeaderHit> {
        let hit = self.hit(header, x, y)?;
        if let HeaderHit::Edge(k) = hit {
            for (c, s) in self.columns.iter_mut().zip(&self.spans) {
                c.width = s.w;
            }
            self.columns[k].flex = 0.0;
            self.drag = Some(Drag {
                column: k,
                from_x: x,
                from_w: self.columns[k].width,
            });
        }
        Some(hit)
    }

    /// The pointer moved to `x` with an edge in hand: the column is as much wider as the
    /// pointer has gone right since the press, never narrower than its minimum. The table is
    /// laid out again. Returns whether a width changed (the caller redraws); false with no edge
    /// in hand.
    pub fn drag(&mut self, x: f32) -> bool {
        let Some(d) = self.drag else {
            return false;
        };
        let c = &mut self.columns[d.column];
        let w = (d.from_w + (x - d.from_x)).round().max(clean(c.min));
        if !w.is_finite() || w == c.width {
            return false;
        }
        c.width = w;
        let before = self.spans.clone();
        self.layout(self.view_w);
        before != self.spans
    }

    /// Lets go of the edge. Returns whether one was in hand: the moment to save the widths.
    pub fn release(&mut self) -> bool {
        self.drag.take().is_some()
    }

    /// The index of the column whose edge is in hand.
    pub fn dragging(&self) -> Option<usize> {
        self.drag.map(|d| d.column)
    }

    /// Makes column `k` exactly `width` wide (its minimum at least) and takes its flex: for a
    /// double click on its edge, with the width of its widest content as the caller measured
    /// it. The table is laid out again.
    pub fn set_width(&mut self, k: usize, width: f32) {
        if let Some(c) = self.columns.get_mut(k) {
            if width.is_finite() {
                c.width = width.round().max(clean(c.min));
                c.flex = 0.0;
            }
            self.layout(self.view_w);
        }
    }

    /// The sort, if the rows are sorted by a column.
    pub fn sort(&self) -> Option<Sort> {
        self.sort
    }

    /// Sets the sort as it is: for the application's default, and for a saved one.
    pub fn set_sort(&mut self, sort: Option<Sort>) {
        self.sort = sort.filter(|s| self.index_of(s.column).is_some());
    }

    /// A click on the title of column `k`: sorted by another column or not at all, the rows
    /// are now sorted by this one, ascending; sorted by this one, the direction turns round.
    /// Returns the new sort for the caller to sort by, or `None` (and nothing changes) for a
    /// column that is not [`sortable`](Column::sortable) or does not exist.
    pub fn toggle_sort(&mut self, k: usize) -> Option<Sort> {
        let c = self.columns.get(k).filter(|c| c.sortable)?;
        let descending = matches!(self.sort, Some(s) if s.column == c.id && !s.descending);
        self.sort = Some(Sort {
            column: c.id,
            descending,
        });
        self.sort
    }

    /// The widths and the sort as the value of one settings line:
    /// `"1:240,2:90,3:120;sort=2:desc"`. Each column is `id:width` in whole px; the part after
    /// the `;` is there only when the table is sorted, and ends in `asc` or `desc`. The key of
    /// the line is the caller's (`files.columns`, say), written with
    /// [`mdshared_app::settings::Writer::line`].
    pub fn to_setting(&self) -> String {
        let mut out = String::new();
        for c in &self.columns {
            if !out.is_empty() {
                out.push(',');
            }
            out.push_str(&format!("{}:{}", c.id, clean(c.width).round() as i64));
        }
        if let Some(s) = self.sort {
            let way = if s.descending { "desc" } else { "asc" };
            out.push_str(&format!(";sort={}:{way}", s.column));
        }
        out
    }

    /// Takes the widths and the sort from a line [`to_setting`](Self::to_setting) wrote, and
    /// lays the table out again. The line may be old or edited by hand, so everything in it is
    /// optional: a column it does not name keeps its width, an id the table does not have is
    /// skipped, a width that is not a number or is not finite is skipped, one below the
    /// column's minimum becomes the minimum, and a sort by a column that is gone, or that
    /// cannot be sorted by, is no sort. A line without a `sort=` part leaves the sort as it
    /// is. A column that takes spare width still takes it: its saved width is what it starts
    /// from.
    pub fn apply_setting(&mut self, value: &str) {
        let (widths, rest) = value.split_once(';').unwrap_or((value, ""));
        for part in widths.split(',') {
            let Some((id, w)) = part.split_once(':') else {
                continue;
            };
            let (Ok(id), Ok(w)) = (id.trim().parse::<u32>(), w.trim().parse::<f32>()) else {
                continue;
            };
            if !w.is_finite() {
                continue;
            }
            if let Some(c) = self.columns.iter_mut().find(|c| c.id == id) {
                // A width no screen has is a damaged line, not a wish.
                c.width = w.round().clamp(clean(c.min), 100_000.0);
            }
        }
        for part in rest.split(';') {
            let Some(sort) = part.trim().strip_prefix("sort=") else {
                continue;
            };
            let (id, way) = sort.split_once(':').unwrap_or((sort, "asc"));
            self.sort = id.trim().parse::<u32>().ok().and_then(|id| {
                let c = self.columns.iter().find(|c| c.id == id && c.sortable)?;
                Some(Sort {
                    column: c.id,
                    descending: way.trim().eq_ignore_ascii_case("desc"),
                })
            });
        }
        self.layout(self.view_w);
    }
}

/// A width or share as the arithmetic can use it: a number that is not one, or is negative, is
/// nothing.
fn clean(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// Draws the header of `table` in `header` (`header.x` is the table's left edge, `header.w`
/// the width to paint the bar over): the bar and the line under it, each title in its column
/// by the column's alignment, the sort mark beside the title of the column the rows are sorted
/// by ([`glyph::SORT`] ascending, [`glyph::SORT_DESC`] descending), a short hairline at each
/// edge, and the look of `hot`: a title under the pointer is filled quiet (darker while
/// `pressed`), an edge under the pointer or in hand is drawn in the accent. It records no hit
/// and changes no state; clip to the view first when the table is wider than it.
pub fn draw_header(
    p: &mut Painter,
    pal: &Palette,
    table: &Table,
    header: Rect,
    hot: Option<HeaderHit>,
    pressed: bool,
) {
    let px = p.px();
    p.fill(header, pal.bar);
    let sorted = table.sort();
    for (k, c) in table.columns().iter().enumerate() {
        let cell = table.cell(header, k);
        if cell.w <= 0.0 {
            continue;
        }
        if hot == Some(HeaderHit::Title(k)) && c.sortable {
            let fill = mix(pal.bar, pal.text, if pressed { 0.14 } else { 0.08 });
            p.fill(cell, fill);
        }
        let mark = sorted.filter(|s| s.column == c.id);
        let mark_w = if mark.is_some() { SORT_ICON + 6.0 } else { 0.0 };
        let old = p.clip(cell);
        let room = (cell.w - 2.0 * CELL_PAD - mark_w).max(0.0);
        let text_w = p.measure(Face::Ui, TITLE_TEXT, &c.title).min(room);
        let x = match c.align {
            Align::Left => cell.x + CELL_PAD,
            Align::Center => cell.x + (cell.w - text_w - mark_w) / 2.0,
            Align::Right => cell.right() - CELL_PAD - text_w - mark_w,
        };
        let colour = if mark.is_some() { pal.text } else { pal.muted };
        p.text(Face::Ui, TITLE_TEXT, x, cell.cy(), &c.title, colour, room);
        if let Some(s) = mark {
            let icon = if s.descending {
                glyph::SORT_DESC
            } else {
                glyph::SORT
            };
            let at = x + text_w + 6.0 + SORT_ICON / 2.0;
            p.icon(icon, SORT_ICON, at, cell.cy(), pal.muted);
        }
        p.restore(old);
        // The edge: a hairline clear of the top and bottom, or the accent over the whole
        // height when it is the one that would move.
        let edge = cell.right() - px;
        if hot == Some(HeaderHit::Edge(k)) {
            p.fill(
                Rect::new(edge - px, header.y, 3.0 * px, header.h),
                pal.accent,
            );
        } else {
            p.fill(
                Rect::new(edge, header.y + 7.0, px, (header.h - 14.0).max(0.0)),
                pal.line,
            );
        }
    }
    p.fill(
        Rect::new(header.x, header.bottom() - px, header.w, px),
        pal.line,
    );
}

/// Draws `text` in a cell by an alignment, [`CELL_PAD`] clear of its sides, elided to what
/// fits, in the interface face at the header's size: the plain cell most columns are. A cell
/// with an icon, a badge or two lines is the caller's own drawing in the same rectangle.
pub fn draw_cell_text(
    p: &mut Painter,
    cell: Rect,
    align: Align,
    text: &str,
    colour: crate::gfx::Rgb,
) {
    let room = cell.w - 2.0 * CELL_PAD;
    if room <= 0.0 {
        return;
    }
    let w = p.measure(Face::Ui, TITLE_TEXT, text).min(room);
    let x = match align {
        Align::Left => cell.x + CELL_PAD,
        Align::Center => cell.x + (cell.w - w) / 2.0,
        Align::Right => cell.right() - CELL_PAD - w,
    };
    p.text(Face::Ui, TITLE_TEXT, x, cell.cy(), text, colour, room);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Gfx;
    use crate::theme::{DARK, LIGHT};
    use mdshared_text::Bitmap;

    const NAME: u32 = 1;
    const SIZE: u32 = 2;
    const DATE: u32 = 3;
    const ICON: u32 = 4;

    /// Name (takes the spare width), Size (numbers), Modified.
    fn files() -> Table {
        Table::new(vec![
            Column::new(NAME, "Name", 200.0).min(80.0).flex(1.0),
            Column::new(SIZE, "Size", 90.0).align(Align::Right),
            Column::new(DATE, "Modified", 140.0),
        ])
    }

    fn widths(t: &Table) -> Vec<f32> {
        t.spans().iter().map(|s| s.w).collect()
    }

    const HEADER: Rect = Rect::new(20.0, 50.0, 600.0, HEADER_H);

    #[test]
    fn spare_width_goes_to_the_columns_that_ask_for_it_and_too_little_comes_from_them() {
        let mut t = files();
        // Exactly what the columns want.
        assert_eq!(t.layout(430.0), 430.0);
        assert_eq!(widths(&t), vec![200.0, 90.0, 140.0]);
        // More: the flexible column takes it all.
        assert_eq!(t.layout(600.0), 600.0);
        assert_eq!(widths(&t), vec![370.0, 90.0, 140.0]);
        assert_eq!(t.spans()[2], Span { x: 460.0, w: 140.0 });
        assert_eq!((t.width(), t.columns().len()), (600.0, 3));
        // Less: it gives, down to its minimum, and then the table is wider than the view.
        assert_eq!(t.layout(400.0), 400.0);
        assert_eq!(widths(&t), vec![170.0, 90.0, 140.0]);
        assert_eq!(t.layout(250.0), 310.0);
        assert_eq!(widths(&t), vec![80.0, 90.0, 140.0]);
        // Two that share, one to two; with no such column the spare width stays empty.
        let mut two = Table::new(vec![
            Column::new(1, "A", 100.0).flex(1.0),
            Column::new(2, "B", 100.0).flex(2.0),
            Column::new(3, "C", 100.0),
        ]);
        two.layout(600.0);
        assert_eq!(widths(&two), vec![200.0, 300.0, 100.0]);
        two.layout(240.0);
        assert_eq!(
            widths(&two),
            vec![80.0, 60.0, 100.0],
            "the larger share gives more"
        );
        two.layout(150.0);
        assert_eq!(widths(&two), vec![40.0, 40.0, 100.0]);
        let mut none = Table::new(vec![Column::new(1, "A", 100.0), Column::new(2, "B", 50.0)]);
        assert_eq!(none.layout(400.0), 150.0);
        // Edges are whole pixels and the columns tile, whatever the shares.
        let mut thirds = Table::new(vec![
            Column::new(1, "A", 10.0).min(10.0).flex(1.0),
            Column::new(2, "B", 10.0).min(10.0).flex(1.0),
            Column::new(3, "C", 10.0).min(10.0).flex(1.0),
        ]);
        thirds.layout(100.0);
        assert_eq!(widths(&thirds), vec![33.0, 34.0, 33.0]);
        for pair in thirds.spans().windows(2) {
            assert_eq!(pair[0].x + pair[0].w, pair[1].x);
        }
        // No columns, and numbers that are not numbers.
        let mut empty = Table::new(vec![]);
        assert_eq!((empty.layout(300.0), empty.width()), (0.0, 0.0));
        assert_eq!(empty.hit(HEADER, 30.0, 60.0), None);
        let mut odd = Table::new(vec![
            Column::new(1, "A", f32::NAN).min(-5.0).flex(f32::INFINITY),
            Column::new(2, "B", 50.0),
        ]);
        assert_eq!(odd.layout(f32::NAN), 50.0);
        assert_eq!(widths(&odd), vec![0.0, 50.0]);
    }

    #[test]
    fn cells_follow_the_columns_and_the_sideways_scroll() {
        let mut t = files();
        t.layout(600.0);
        let row = Rect::new(20.0, 100.0, 600.0, 24.0);
        assert_eq!(t.cell(row, 0), Rect::new(20.0, 100.0, 370.0, 24.0));
        assert_eq!(t.cell(row, 2), Rect::new(480.0, 100.0, 140.0, 24.0));
        assert_eq!(t.cell(row, 9), Rect::new(620.0, 100.0, 0.0, 24.0));
        assert_eq!(t.column_at(row, 20.0), Some(0));
        assert_eq!(t.column_at(row, 389.9), Some(0));
        assert_eq!(t.column_at(row, 390.0), Some(1));
        assert_eq!(t.column_at(row, 619.9), Some(2));
        assert_eq!(t.column_at(row, 620.0), None);
        assert_eq!(t.column_at(row, 19.9), None);
        // Scrolled 100 px to the right: the row's left edge is 100 px further left.
        let scrolled = Rect { x: -80.0, ..row };
        assert_eq!(t.cell(scrolled, 1).x, 290.0);
        assert_eq!(t.column_at(scrolled, 300.0), Some(1));
        assert_eq!((t.index_of(DATE), t.index_of(99)), (Some(2), None));
    }

    #[test]
    fn a_point_in_the_header_is_on_a_title_or_on_an_edge() {
        let mut t = files();
        t.layout(600.0);
        let y = HEADER.cy();
        // Name ends at 370 (390 on screen), Size at 460 (480), Modified at 600 (620).
        assert_eq!(t.hit(HEADER, 100.0, y), Some(HeaderHit::Title(0)));
        assert_eq!(t.hit(HEADER, 385.9, y), Some(HeaderHit::Title(0)));
        assert_eq!(t.hit(HEADER, 386.0, y), Some(HeaderHit::Edge(0)));
        assert_eq!(t.hit(HEADER, 394.0, y), Some(HeaderHit::Edge(0)));
        assert_eq!(t.hit(HEADER, 394.1, y), Some(HeaderHit::Title(1)));
        assert_eq!(t.hit(HEADER, 478.0, y), Some(HeaderHit::Edge(1)));
        // The last edge is reachable from outside the last column too.
        assert_eq!(t.hit(HEADER, 622.0, y), Some(HeaderHit::Edge(2)));
        assert_eq!(t.hit(HEADER, 625.0, y), None);
        assert_eq!(t.hit(HEADER, 100.0, HEADER.y - 0.1), None);
        assert_eq!(t.hit(HEADER, 100.0, HEADER.bottom()), None);
        // An edge that cannot be dragged is its title.
        let mut fixed = Table::new(vec![
            Column::new(ICON, "", 28.0).min(28.0).fixed().unsortable(),
            Column::new(NAME, "Name", 100.0),
        ]);
        fixed.layout(128.0);
        assert_eq!(fixed.hit(HEADER, 47.0, y), Some(HeaderHit::Title(0)));
        assert_eq!(fixed.hit(HEADER, 49.0, y), Some(HeaderHit::Title(1)));
        assert_eq!(fixed.press(HEADER, 47.0, y), Some(HeaderHit::Title(0)));
        assert_eq!(fixed.dragging(), None);
        // Two edges 6 px apart: the nearer one.
        let mut narrow = Table::new(vec![
            Column::new(1, "A", 100.0),
            Column::new(2, "B", 6.0).min(6.0),
        ]);
        narrow.layout(106.0);
        assert_eq!(narrow.hit(HEADER, 122.0, y), Some(HeaderHit::Edge(0)));
        assert_eq!(narrow.hit(HEADER, 124.0, y), Some(HeaderHit::Edge(1)));
    }

    #[test]
    fn dragging_an_edge_resizes_its_column_and_stops_at_the_minimum() {
        let mut t = files();
        t.layout(600.0);
        let y = HEADER.cy();
        assert!(!t.drag(300.0), "no edge in hand");
        assert!(!t.release());
        // The edge of Size, at 480 on screen.
        assert_eq!(t.press(HEADER, 479.0, y), Some(HeaderHit::Edge(1)));
        assert_eq!(t.dragging(), Some(1));
        // Wherever the pointer goes, it is still that edge.
        assert_eq!(t.hit(HEADER, 5.0, 900.0), Some(HeaderHit::Edge(1)));
        assert!(t.drag(509.0));
        assert_eq!(
            widths(&t),
            vec![340.0, 120.0, 140.0],
            "Name gives what Size takes"
        );
        assert!(!t.drag(509.0), "no change, no redraw");
        // Far to the left: its minimum of 40, and Name takes the rest.
        assert!(t.drag(100.0));
        assert_eq!(widths(&t), vec![420.0, 40.0, 140.0]);
        assert!(!t.drag(f32::NAN));
        assert!(t.release());
        assert_eq!((t.dragging(), t.release()), (None, false));
        assert_eq!(t.hit(HEADER, 5.0, 900.0), None);

        // The edge of the flexible column itself: from then on it is as wide as it was made,
        // and with no other flexible column the spare width is empty.
        assert_eq!(t.press(HEADER, 440.0, y), Some(HeaderHit::Edge(0)));
        assert!(t.drag(340.0));
        t.release();
        assert_eq!(widths(&t), vec![320.0, 40.0, 140.0]);
        assert_eq!(t.columns()[0].flex, 0.0);
        assert_eq!(t.layout(900.0), 500.0);
        // A double click on an edge: the caller measured 73.4 px of content.
        t.set_width(1, 73.4);
        assert_eq!(widths(&t), vec![320.0, 73.0, 140.0]);
        t.set_width(1, 3.0);
        assert_eq!(widths(&t)[1], 40.0);
        t.set_width(9, 50.0);
        t.set_width(1, f32::NAN);
        assert_eq!(widths(&t)[1], 40.0);
    }

    #[test]
    fn a_click_on_a_title_sorts_by_it_and_a_second_turns_it_round() {
        let mut t = Table::new(vec![
            Column::new(ICON, "", 28.0).unsortable(),
            Column::new(NAME, "Name", 200.0),
            Column::new(SIZE, "Size", 90.0),
        ]);
        assert_eq!(t.sort(), None);
        let asc = Sort {
            column: NAME,
            descending: false,
        };
        assert_eq!(t.toggle_sort(1), Some(asc));
        assert_eq!(
            t.toggle_sort(1),
            Some(Sort {
                descending: true,
                ..asc
            })
        );
        assert_eq!(t.toggle_sort(1), Some(asc));
        // Another column starts ascending, whichever way the last one was.
        t.toggle_sort(1);
        assert_eq!(
            t.toggle_sort(2),
            Some(Sort {
                column: SIZE,
                descending: false
            })
        );
        // A column that cannot be sorted by, or is not there: nothing changes.
        assert_eq!(t.toggle_sort(0), None);
        assert_eq!(t.toggle_sort(7), None);
        assert_eq!(t.sort().map(|s| s.column), Some(SIZE));
        t.set_sort(Some(Sort {
            column: 99,
            descending: true,
        }));
        assert_eq!(t.sort(), None);
        t.set_sort(Some(asc));
        assert_eq!(t.sort(), Some(asc));
    }

    #[test]
    fn widths_and_sort_go_through_one_settings_line() {
        let mut t = files();
        t.layout(600.0);
        assert_eq!(t.to_setting(), "1:200,2:90,3:140");
        t.press(HEADER, 479.0, HEADER.cy());
        t.drag(509.0);
        t.release();
        t.toggle_sort(1);
        t.toggle_sort(1);
        let line = t.to_setting();
        // The widths saved are the columns' own; Name, which gives and takes, is laid out from
        // its 370.
        assert_eq!(line, "1:370,2:120,3:140;sort=2:desc");
        // Into a fresh table: the same columns, the same sort.
        let mut back = files();
        back.layout(600.0);
        back.apply_setting(&line);
        assert_eq!(widths(&back), widths(&t));
        assert_eq!(back.sort(), t.sort());
        assert_eq!(back.to_setting(), line);
        // Through the settings file's own writer and reader.
        let mut w = mdshared_app::settings::Writer::new("files");
        w.line("columns", &line);
        let text = w.finish();
        let (_, value) = mdshared_app::settings::pairs(&text)
            .find(|(k, _)| k.ends_with("columns"))
            .unwrap();
        assert_eq!(value, line);
    }

    #[test]
    fn a_damaged_settings_line_changes_only_what_it_says_clearly() {
        let fresh = || {
            let mut t = files();
            t.layout(430.0);
            t
        };
        for junk in [
            "",
            ";",
            ",,,",
            "sort=",
            "1",
            "1:",
            ":200",
            "x:200",
            "1:wide",
            "1:NaN,2:inf",
            "99:500",
            "1:200:300",
            ";sort=99:desc",
            ";sort=x",
            ";;sort",
            "\u{0}\u{ffff}:\n",
        ] {
            let mut t = fresh();
            t.apply_setting(junk);
            assert_eq!(widths(&t), vec![200.0, 90.0, 140.0], "{junk:?}");
            assert_eq!(t.sort(), None, "{junk:?}");
        }
        let mut t = fresh();
        // One good part among bad ones; a width below the minimum; spaces; a sort with no way.
        t.apply_setting("7:1, 2 : 10 ,1:1e9,3:150.6;sort= 3 ");
        assert_eq!(t.columns()[1].width, 40.0);
        assert_eq!(t.columns()[0].width, 100_000.0);
        assert_eq!(t.columns()[2].width, 151.0);
        assert_eq!(
            t.sort(),
            Some(Sort {
                column: DATE,
                descending: false
            })
        );
        // No sort part: the sort stays. A sort by a column that cannot be sorted by: none.
        t.apply_setting("3:140");
        assert!(t.sort().is_some());
        let mut icons = Table::new(vec![Column::new(ICON, "", 28.0).unsortable()]);
        icons.apply_setting("4:28;sort=4:asc");
        assert_eq!(icons.sort(), None);
    }

    fn shot(pal: &Palette, t: &Table, hot: Option<HeaderHit>, pressed: bool) -> Bitmap {
        let mut bmp = Bitmap::new(660, 100);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, 660.0, 100.0), pal.page);
            draw_header(&mut p, pal, t, HEADER, hot, pressed);
        }
        bmp
    }

    fn at(bmp: &Bitmap, x: f32, y: f32) -> [u8; 3] {
        let o = (y as usize * bmp.width as usize + x as usize) * 4;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    }

    /// How many pixels of the rectangle are not `c`.
    fn ink(bmp: &Bitmap, r: Rect, c: [u8; 3]) -> usize {
        let mut n = 0;
        for y in r.y as usize..r.bottom() as usize {
            for x in r.x as usize..r.right() as usize {
                n += usize::from(at(bmp, x as f32, y as f32) != c);
            }
        }
        n
    }

    #[test]
    fn the_header_draws_its_bar_titles_edges_and_the_sort_mark() {
        for pal in [&LIGHT, &DARK] {
            let mut t = files();
            t.layout(600.0);
            let plain = shot(pal, &t, None, false);
            // The bar, the line under it, and nothing outside the header.
            assert_eq!(at(&plain, 200.0, 53.0), pal.bar);
            assert_eq!(at(&plain, 200.0, 79.0), pal.line);
            assert_eq!(at(&plain, 200.0, 49.0), pal.page);
            assert_eq!(at(&plain, 200.0, 80.0), pal.page);
            assert_eq!(at(&plain, 19.0, 60.0), pal.page);
            // A hairline at each column's right edge, clear of the top.
            for edge in [389.0, 479.0, 619.0] {
                assert_eq!(at(&plain, edge, 65.0), pal.line, "edge at {edge}");
                assert_eq!(at(&plain, edge, 52.0), pal.bar);
            }
            // Titles: Name at the left of its column, Size at the right of its own.
            let name = t.cell(HEADER, 0);
            let size = t.cell(HEADER, 1);
            let left_half = |c: Rect| Rect::new(c.x + 2.0, c.y + 4.0, c.w / 2.0 - 4.0, c.h - 8.0);
            let right_half =
                |c: Rect| Rect::new(c.cx() + 2.0, c.y + 4.0, c.w / 2.0 - 6.0, c.h - 8.0);
            assert!(ink(&plain, left_half(name), pal.bar) > 20);
            assert_eq!(ink(&plain, right_half(name), pal.bar), 0);
            assert!(ink(&plain, right_half(size), pal.bar) > 20);
            assert_eq!(ink(&plain, left_half(size), pal.bar), 0);

            // Sorted by Name: a mark after the title, another one when turned round, and the
            // other columns as they were.
            t.toggle_sort(0);
            let asc = shot(pal, &t, None, false);
            t.toggle_sort(0);
            let desc = shot(pal, &t, None, false);
            assert!(asc.pixels != plain.pixels && desc.pixels != asc.pixels);
            for y in 50..80 {
                for x in 391..640 {
                    let (x, y) = (x as f32, y as f32);
                    assert_eq!(at(&asc, x, y), at(&plain, x, y));
                    assert_eq!(at(&desc, x, y), at(&plain, x, y));
                }
            }
            // A title under the pointer is filled, darker while pressed; an edge is the accent
            // over the header's whole height.
            let hover = shot(pal, &t, Some(HeaderHit::Title(1)), false);
            let press = shot(pal, &t, Some(HeaderHit::Title(1)), true);
            let quiet = at(&hover, 420.0, 53.0);
            assert!(quiet != pal.bar && at(&press, 420.0, 53.0) != quiet);
            assert_eq!(at(&hover, 300.0, 53.0), pal.bar);
            let edge = shot(pal, &t, Some(HeaderHit::Edge(1)), false);
            for y in [50.0, 65.0, 78.0] {
                assert_eq!(at(&edge, 479.0, y), pal.accent);
            }
            assert_eq!(at(&edge, 389.0, 65.0), pal.line);
        }
        // A column that cannot be sorted by shows no hover.
        let mut icons = Table::new(vec![Column::new(ICON, "", 28.0).unsortable().fixed()]);
        icons.layout(28.0);
        let a = shot(&LIGHT, &icons, None, false);
        let b = shot(&LIGHT, &icons, Some(HeaderHit::Title(0)), true);
        assert!(a.pixels == b.pixels);
    }

    #[test]
    fn cell_text_sits_by_its_alignment_and_stays_inside_the_cell() {
        let cell = Rect::new(100.0, 20.0, 200.0, 30.0);
        let draw = |align: Align, text: &str| {
            let mut bmp = Bitmap::new(400, 70);
            let mut gfx = Gfx::new();
            {
                let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
                p.fill(Rect::new(0.0, 0.0, 400.0, 70.0), [255, 255, 255]);
                draw_cell_text(&mut p, cell, align, text, [0, 0, 0]);
            }
            let white = [255, 255, 255];
            let xs: Vec<usize> = (0..400)
                .filter(|&x| (0..70).any(|y| at(&bmp, x as f32, y as f32) != white))
                .collect();
            (*xs.first().unwrap() as f32, *xs.last().unwrap() as f32)
        };
        let (l0, l1) = draw(Align::Left, "12.3 MB");
        let (c0, c1) = draw(Align::Center, "12.3 MB");
        let (r0, r1) = draw(Align::Right, "12.3 MB");
        assert!((l0 - 110.0).abs() <= 2.0, "{l0}");
        assert!((r1 - 290.0).abs() <= 2.0, "{r1}");
        assert!(((c0 + c1) / 2.0 - 200.0).abs() <= 2.0, "{c0}..{c1}");
        assert!(l1 < r1 && l0 < c0 && c0 < r0);
        // Too long: elided, and still inside the padding.
        let long = "a very long file name that cannot possibly fit in two hundred pixels.flac";
        for align in [Align::Left, Align::Center, Align::Right] {
            let (x0, x1) = draw(align, long);
            assert!(x0 >= 108.0 && x1 <= 292.0, "{align:?}: {x0}..{x1}");
        }
        // No room at all: nothing drawn, nothing panics.
        let mut bmp = Bitmap::new(50, 50);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        draw_cell_text(
            &mut p,
            Rect::new(5.0, 5.0, 12.0, 20.0),
            Align::Right,
            "x",
            [9, 9, 9],
        );
        assert!(bmp.pixels.iter().all(|&b| b == 0));
    }
}
