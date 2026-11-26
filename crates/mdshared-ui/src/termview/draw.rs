//! Drawing a screen: each row on its own, clipped to its band, so that a row drawn alone is the
//! row of a whole frame pixel for pixel.

use super::boxes::{Piece, coverage, is_box_char, pieces};
use super::font::CellFont;
use super::metrics::Metrics;
use super::scheme::{CellColors, Scheme};
use super::view::View;
use crate::anim::{Animator, AsId};
use crate::gfx::{Painter, Rect, Rgb};
use mdshared_term::select::END;
use mdshared_term::{Attrs, Cell, CursorShape, Line, Screen, Selection};
use std::ops::Range;
use std::time::Duration;

/// Where the cursor is drawn: the view's row, the first column of the character under it, how
/// many cells that character takes, and its shape.
#[derive(Clone, Copy)]
struct CursorAt {
    row: usize,
    col: usize,
    cells: usize,
    shape: CursorShape,
}

fn cursor_at(screen: &Screen, view: &View) -> Option<CursorAt> {
    let c = screen.cursor();
    if !c.visible || !view.is_cursor_shown() {
        return None;
    }
    let row = view.cursor_row(screen)?;
    let line = screen.line(c.row);
    let (col, cells) = character_at(line, c.col, screen.cols());
    Some(CursorAt {
        row,
        col,
        cells,
        shape: c.shape,
    })
}

/// The first column of the character shown at `col`, and how many cells it takes (1, or 2 for
/// a wide character that fits).
fn character_at(line: Option<&Line>, col: usize, cols: usize) -> (usize, usize) {
    let cell = |c: usize| line.map_or(Cell::BLANK, |l| l.cell(c));
    let col = if col > 0 && cell(col).is_spacer() {
        col - 1
    } else {
        col
    };
    let cells = if cell(col).width == 2 && col + 1 < cols {
        2
    } else {
        1
    };
    (col, cells)
}

/// Whether column `col` lies in the selected span `a..=b` of a line (`b` may be [`END`]).
fn covers(span: Option<(usize, usize)>, col: usize) -> bool {
    span.is_some_and(|(a, b)| a <= col && (b == END || col <= b))
}

/// The colours of the character whose first column is `col` and which takes `cells` cells:
/// its own, then the selection over them, then a focused block cursor over that.
fn look(
    scheme: &Scheme,
    cell: &Cell,
    col: usize,
    cells: usize,
    span: Option<(usize, usize)>,
    block: Option<usize>,
) -> CellColors {
    let mut c = scheme.cell_colors(cell);
    if (col..col + cells).any(|k| covers(span, k)) {
        c.back = scheme.selection;
        if let Some(t) = scheme.selection_text {
            c.glyph = c.glyph.map(|_| t);
        }
    }
    if block == Some(col) {
        c.back = scheme.cursor;
        c.glyph = c.glyph.map(|_| scheme.cursor_text);
    }
    c
}

/// Draws the whole of `screen` into `area`: every row, and the background over what is left of
/// `area` below the last row and right of the last column. The grid starts at `area`'s top left
/// corner as the painter rounds it; rows and columns that do not fit are cut off by `area`.
///
/// Each cell is drawn as [`Scheme::cell_colors`] says, with the selection's colours over the
/// cells `selection` covers ([`Selection::span_on`], read in absolute line numbers, so it stays
/// on its text in the scrollback) and the cursor over its cell: a filled block with the
/// character in [`Scheme::cursor_text`], a bar at the cell's left edge or a line at its bottom,
/// as the program asked, and a hollow block in every shape while the view is not focused. A
/// cursor that the program hid, or that is scrolled out of the view, is not drawn. A wide
/// character takes its two cells; the cursor and the selection on either half cover both.
/// Box-drawing and block characters (U+2500 to U+259F) are drawn as shapes that join from cell
/// to cell ([`is_box_char`](super::is_box_char)); other characters come from `font`, bold and
/// italic as the cell says, underlined and struck through in the glyph's colour. A hidden cell
/// shows its background only.
///
/// `view` says how far back into the scrollback the view looks and whether it has the focus.
/// Nothing is changed but pixels; nothing is remembered between calls.
#[allow(clippy::too_many_arguments)] // drawing call: the grid, its look and its state
pub fn draw_screen(
    p: &mut Painter,
    font: &mut CellFont,
    scheme: &Scheme,
    metrics: &Metrics,
    area: Rect,
    screen: &Screen,
    selection: Option<&Selection>,
    view: View,
) {
    let (x0, y0, x1, y1) = p.dev_rect(area);
    let (_, ch) = metrics.cell_device();
    let below = y0 as i64 + screen.rows() as i64 * ch as i64;
    if below < y1 as i64 {
        let old = p.clip(area);
        p.fill_dev(x0, below as i32, x1, y1, scheme.background, 1.0);
        p.restore(old);
    }
    draw_rows(
        p,
        font,
        scheme,
        metrics,
        area,
        screen,
        selection,
        view,
        std::slice::from_ref(&(0..screen.rows())),
    );
}

/// Draws only the rows `rows` of the view (ranges of view rows, from 0 at the top; past the
/// screen's last row they are cut off), each over the band [`Metrics::row_rect`] gives it, the
/// padding at the right included. The pixels of every row drawn are those [`draw_screen`] draws
/// there, and nothing outside those bands is touched: each row is clipped to its band, so a
/// glyph that reaches past its row (an italic, a tall fallback glyph) is cut the same way in
/// both.
///
/// This is the cheap path: the application keeps last frame's bitmap, and after
/// [`Screen::feed`] draws only what [`View::damage`] makes of
/// [`Screen::take_dirty`](mdshared_term::Screen::take_dirty), then presents those bands.
/// Changes the screen cannot know of are the application's to add: a new selection (the rows
/// of the old and the new one, or everything), focus, the scroll offset, the scheme, a resize
/// (everything, with [`draw_screen`]).
#[allow(clippy::too_many_arguments)] // drawing call: the grid, its look, its state and the rows
pub fn draw_rows(
    p: &mut Painter,
    font: &mut CellFont,
    scheme: &Scheme,
    metrics: &Metrics,
    area: Rect,
    screen: &Screen,
    selection: Option<&Selection>,
    view: View,
    rows: &[Range<usize>],
) {
    let (ax0, ay0, ax1, ay1) = p.dev_rect(area);
    if ax1 <= ax0 || ay1 <= ay0 {
        return;
    }
    let (_, ch) = metrics.cell_device();
    let outer = p.clip(area);
    // The clip in force inside `area`, given back after each row.
    let in_area = p.clip(area);
    let cursor = cursor_at(screen, &view);
    let top = view.top_line(screen);
    for range in rows {
        for r in range.start..range.end.min(screen.rows()) {
            let y0 = ay0 as i64 + r as i64 * ch as i64;
            if y0 >= ay1 as i64 {
                break;
            }
            p.restore(in_area);
            let y0 = y0 as i32;
            let band = dev_to_rect(p, ax0, y0, ax1, y0 + ch);
            p.clip(band);
            // The band's clip, cut by the outer ones: what the text engine is given.
            let clip = p.clip(band);
            let row = Row {
                y0,
                ax0,
                metrics,
                cursor: cursor.filter(|c| c.row == r),
                focused: view.is_focused(),
            };
            row.draw(
                p,
                font,
                scheme,
                screen.line_at(top + r as u64),
                screen.cols(),
                selection,
                top + r as u64,
                clip,
            );
        }
    }
    p.restore(outer);
}

/// Logical px for a device rectangle of the painter (its offset taken off), which the painter
/// rounds back to exactly that rectangle.
fn dev_to_rect(p: &Painter, x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
    let (o, s) = (p.origin(), p.scale);
    Rect::new(
        (x0 - o.0) as f32 / s,
        (y0 - o.1) as f32 / s,
        (x1 - x0) as f32 / s,
        (y1 - y0) as f32 / s,
    )
}

/// One row being drawn: its top in device px, the grid's left edge, and the cursor if it is on
/// this row.
struct Row<'a> {
    y0: i32,
    ax0: i32,
    metrics: &'a Metrics,
    cursor: Option<CursorAt>,
    /// The view has the focus: a block cursor is filled, every cursor is hollow otherwise.
    focused: bool,
}

impl Row<'_> {
    #[allow(clippy::too_many_arguments)] // one row's inputs
    fn draw(
        &self,
        p: &mut Painter,
        font: &mut CellFont,
        scheme: &Scheme,
        line: Option<&Line>,
        cols: usize,
        selection: Option<&Selection>,
        abs: u64,
        clip: (i32, i32, i32, i32),
    ) {
        let (cw, ch) = self.metrics.cell_device();
        let (y0, y1) = (self.y0, self.y0 + ch);
        // Only the columns inside the clip are visited.
        let first = ((clip.0 - self.ax0).max(0) / cw) as usize;
        let last = ((((clip.2 - self.ax0).max(0) + cw - 1) / cw) as usize).min(cols);
        p.fill_dev(clip.0, y0, clip.2, y1, scheme.background, 1.0);
        if first >= last {
            return;
        }
        let span = selection.and_then(|s| s.span_on(abs));
        let focused_block = self
            .cursor
            .filter(|c| c.shape == CursorShape::Block && self.focused)
            .map(|c| c.col);
        let cell = |c: usize| line.map_or(Cell::BLANK, |l| l.cell(c));
        // A wide character starting just left of the clip still has its second cell inside.
        let start = if first > 0 && cell(first).is_spacer() {
            first - 1
        } else {
            first
        };

        // Backgrounds, in runs of one colour.
        let mut run: Option<(i32, Rgb)> = None;
        let mut col = start;
        while col < last {
            let (lead, cells) = character_at(line, col, cols);
            let c = look(scheme, &cell(lead), lead, cells, span, focused_block);
            let x = self.ax0 + (lead * cw as usize) as i32;
            match run {
                Some((_, color)) if color == c.back => {}
                Some((sx, color)) => {
                    if color != scheme.background {
                        p.fill_dev(sx, y0, x, y1, color, 1.0);
                    }
                    run = Some((x, c.back));
                }
                None => run = Some((x, c.back)),
            }
            col = lead + cells;
        }
        if let Some((sx, color)) = run
            && color != scheme.background
        {
            let end = self.ax0 + (col * cw as usize) as i32;
            p.fill_dev(sx, y0, end, y1, color, 1.0);
        }

        // Characters and their lines.
        let mut col = start;
        while col < last {
            let (lead, cells) = character_at(line, col, cols);
            col = lead + cells;
            let cl = cell(lead);
            let c = look(scheme, &cl, lead, cells, span, focused_block);
            let Some(fg) = c.glyph else {
                continue;
            };
            let x = self.ax0 + (lead * cw as usize) as i32;
            let w = cw * cells as i32;
            if is_box_char(cl.ch) {
                draw_box(p, cl.ch, x, y0, cw, ch, self.metrics.stroke(), fg);
            } else if cl.ch != ' ' {
                let style = (
                    cl.attrs.contains(Attrs::BOLD),
                    cl.attrs.contains(Attrs::ITALIC),
                );
                font.draw(
                    p.bmp,
                    clip,
                    cl.ch,
                    style,
                    self.metrics,
                    (x, w, y0 + self.metrics.baseline()),
                    fg,
                );
            }
            let t = self.metrics.stroke();
            if cl.attrs.contains(Attrs::UNDERLINE) {
                let uy = y0 + self.metrics.underline_y();
                p.fill_dev(x, uy, x + w, uy + t, fg, 1.0);
            }
            if cl.attrs.contains(Attrs::STRIKE) {
                let sy = y0 + self.metrics.strike_y();
                p.fill_dev(x, sy, x + w, sy + t, fg, 1.0);
            }
        }

        // The cursor's lines, over the characters.
        if let Some(c) = self.cursor {
            let x = self.ax0 + (c.col * cw as usize) as i32;
            let w = cw * c.cells as i32;
            let t = self.metrics.cursor_thickness();
            if !self.focused {
                let s = self.metrics.stroke();
                p.fill_dev(x, y0, x + w, y0 + s, scheme.cursor, 1.0);
                p.fill_dev(x, y1 - s, x + w, y1, scheme.cursor, 1.0);
                p.fill_dev(x, y0 + s, x + s, y1 - s, scheme.cursor, 1.0);
                p.fill_dev(x + w - s, y0 + s, x + w, y1 - s, scheme.cursor, 1.0);
            } else {
                match c.shape {
                    CursorShape::Block => {}
                    CursorShape::Bar => p.fill_dev(x, y0, x + t, y1, scheme.cursor, 1.0),
                    CursorShape::Underline => p.fill_dev(x, y1 - t, x + w, y1, scheme.cursor, 1.0),
                }
            }
        }
    }
}

/// Draws a box-drawing or block character in the cell at `x, y` (device px), `cw` by `ch`.
#[allow(clippy::too_many_arguments)] // drawing call: a place plus a style
fn draw_box(p: &mut Painter, c: char, x: i32, y: i32, cw: i32, ch: i32, light: i32, fg: Rgb) {
    let Some(shapes) = pieces(c, cw, ch, light) else {
        return;
    };
    for piece in &shapes {
        match *piece {
            Piece::Fill {
                x0,
                y0,
                x1,
                y1,
                alpha,
            } => p.fill_dev(x + x0, y + y0, x + x1, y + y1, fg, alpha),
            _ => {
                for py in 0..ch {
                    for px in 0..cw {
                        let a = coverage(piece, px, py, cw, ch);
                        if a > 0.0 {
                            p.fill_dev(x + px, y + py, x + px + 1, y + py + 1, fg, a);
                        }
                    }
                }
            }
        }
    }
}

/// Whether the cursor is to be shown this frame, with blinking: asks `anim` to
/// [`blink`](Animator::blink) `id` at `blink` on and off, but only while the cursor can blink
/// (the view has the focus, the program has not hidden the cursor nor asked for a steady one,
/// and the cursor is on the view). Otherwise, and when `blink` is `None` (the system's caret
/// blinking is off, `platform::caret_blink_time`) or motion is reduced, the cursor is shown and
/// nothing is asked, so the animator wants no frame for it and an idle window stays idle.
///
/// The blink starts on, the first time it is asked for. An application starts it again on each
/// key and on output that moves the cursor, as terminals do, with `anim.forget(id)`.
pub fn cursor_blinks_on(
    anim: &mut Animator,
    id: impl AsId,
    blink: Option<Duration>,
    screen: &Screen,
    view: View,
) -> bool {
    let c = screen.cursor();
    let can = view.is_focused()
        && view.is_cursor_shown()
        && c.visible
        && c.blinking
        && view.cursor_row(screen).is_some();
    match blink {
        Some(half) if can => anim.blink(id, half, half),
        _ => true,
    }
}

/// [`draw_screen`] with a cursor that blinks ([`cursor_blinks_on`]): while the cursor is off
/// it is not drawn and its cell is drawn as it is. At rest, with reduced motion, with blinking
/// off, without the focus, and while the cursor is on, the pixels are exactly
/// [`draw_screen`]'s.
///
/// The animator's deadline is the next change of the blink; at it, the application redraws the
/// cursor's row ([`View::cursor_row`]) with [`draw_rows_animated`], or the whole grid.
#[allow(clippy::too_many_arguments)] // drawing call: as `draw_screen`, with the animator
pub fn draw_screen_animated(
    p: &mut Painter,
    font: &mut CellFont,
    scheme: &Scheme,
    metrics: &Metrics,
    area: Rect,
    screen: &Screen,
    selection: Option<&Selection>,
    view: View,
    anim: &mut Animator,
    id: impl AsId,
    blink: Option<Duration>,
) {
    let on = cursor_blinks_on(anim, id, blink, screen, view);
    let view = view.cursor_shown(view.is_cursor_shown() && on);
    draw_screen(p, font, scheme, metrics, area, screen, selection, view);
}

/// [`draw_rows`] with a cursor that blinks, as [`draw_screen_animated`]. The animator is asked
/// whether or not the cursor's row is among `rows`, so a frame that redraws other rows keeps the
/// blink going.
#[allow(clippy::too_many_arguments)] // drawing call: as `draw_rows`, with the animator
pub fn draw_rows_animated(
    p: &mut Painter,
    font: &mut CellFont,
    scheme: &Scheme,
    metrics: &Metrics,
    area: Rect,
    screen: &Screen,
    selection: Option<&Selection>,
    view: View,
    rows: &[Range<usize>],
    anim: &mut Animator,
    id: impl AsId,
    blink: Option<Duration>,
) {
    let on = cursor_blinks_on(anim, id, blink, screen, view);
    let view = view.cursor_shown(view.is_cursor_shown() && on);
    draw_rows(
        p, font, scheme, metrics, area, screen, selection, view, rows,
    );
}
