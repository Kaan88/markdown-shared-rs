//! Changing a screen's size: the cut-and-pad [`resize`](Screen::resize) and the
//! [`resize_reflow`](Screen::resize_reflow) that joins and wraps the lines again.
//!
//! Both are called by the application on a window resize and change nothing else: the scroll
//! region, the tab stops, the rows marked changed and the switch between the two calls are in
//! `Screen::resize` and `Screen::resize_reflow`, which live here; the joining and wrapping
//! itself is the private [`reflow`](crate::reflow) module.

use super::*;
use crate::reflow;

/// Brings `grid` to `rows` lines. Lines are taken from below the row `keep` first (the bottom of
/// a screen is usually empty) and then from the top, and those from the top are returned, oldest
/// first, for the scrollback; `keep` follows its line.
fn fit_rows(grid: &mut Vec<Line>, rows: usize, cols: usize, keep: &mut usize) -> Vec<Line> {
    let mut off = Vec::new();
    while grid.len() > rows {
        if grid.len() - 1 > *keep {
            grid.pop();
        } else {
            off.push(grid.remove(0));
            *keep = keep.saturating_sub(1);
        }
    }
    while grid.len() < rows {
        grid.push(Line::blank(cols, Cell::BLANK));
    }
    off
}

// ---- size -------------------------------------------------------------------------------------

impl Screen {
    /// Changes the screen's size. Lines are not reflowed: a line longer than the new width is
    /// cut at it (the cells are gone, also if the screen widens again), a shorter one is padded.
    /// When rows go, blank and other lines below the cursor go first and then lines from the
    /// top, which join the scrollback on the main screen, so the cursor's line is always kept;
    /// when rows come, they come empty at the bottom (nothing is pulled back out of the
    /// scrollback). The scroll region becomes the whole screen and every row is marked changed.
    /// The scrollback's lines keep the width they were written with.
    ///
    /// This is the call for a caller that wants a width change to cut, as a terminal did before
    /// reflow existed; [`resize_reflow`](Self::resize_reflow) is the same change with the primary
    /// screen and its scrollback joined and wrapped again.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        let (cols, rows) = (cols.clamp(1, MAX_SIDE), rows.clamp(1, MAX_SIDE));
        if (cols, rows) == (self.cols, self.rows) {
            return;
        }
        let alt = self.modes.alt_screen;
        let mut row = self.row;
        let off = fit_rows(&mut self.grid, rows, cols, &mut row);
        self.row = row;
        if !alt {
            for line in off {
                self.push_scrollback(line);
            }
        }
        if let Some(mut main) = self.main.take() {
            let mut keep = self.main_row;
            for line in fit_rows(&mut main, rows, cols, &mut keep) {
                self.push_scrollback(line);
            }
            self.main_row = keep;
            for line in &mut main {
                line.resize(cols);
            }
            self.main = Some(main);
        }
        for line in &mut self.grid {
            line.resize(cols);
        }
        self.fit_tabs(cols);
        (self.cols, self.rows) = (cols, rows);
        (self.top, self.bottom) = (0, rows - 1);
        self.row = self.row.min(rows - 1);
        self.col = self.col.min(cols - 1);
        self.pending = false;
        self.dirty = vec![true; rows];
    }

    /// Changes the screen's size and re-wraps it: when the width changes, the primary screen and
    /// its scrollback are joined back into the lines of text they were (see [`Line::wrapped`])
    /// and wrapped again at the new width, as VTE and kitty do, so that a line longer than
    /// the screen is not cut and nothing is lost when the screen widens again.
    ///
    /// - A hard line break, one whose line is not marked as wrapped, stays where it is.
    /// - Only the cells a line holds text in ([`Line::text_len`]) are text, so the padding of a
    ///   row does not grow a space each time the width changes, whatever its colour, while a
    ///   space written where the line broke off is kept.
    /// - A character two cells wide is never cut: when it does not fit in the last column of the
    ///   row, that column stays empty and the character goes in the row below.
    /// - The cursor stays on the same character of the same line of text; a cursor past the end
    ///   of that text keeps its place as far as the new width has columns for it, and a cursor
    ///   that hangs past the right edge keeps its pending wrap. The saved cursor of `ESC 7`,
    ///   `CSI s` and mode 1048 follows its text the same way.
    /// - The grid stays at the end of the buffer, so the rows the new wrapping saved come back
    ///   out of the scrollback and the rows it added go into it, up to the scrollback's limit
    ///   (the oldest go when the new wrapping needs more lines than it keeps). The cursor's line
    ///   stays on the screen: it keeps its row where the text above it wraps to the same height,
    ///   and when more than a screenful of text ends up below it, it is put on the top row and
    ///   the lines below the screen are lost, as the lines below the cursor are when rows go.
    /// - The scroll region becomes the whole screen, the tab stops reach the new columns and
    ///   every row is marked changed.
    ///
    /// The alternate screen is not reflowed: it is a picture the program drew cell by cell, and
    /// it is cut and padded exactly as [`resize`](Self::resize) does, which is also what a change
    /// of height alone gives. The primary screen behind it is a text, though, and is joined and
    /// wrapped again as if it were shown, its cursor and scrollback included, so that leaving the
    /// alternate screen gives it back re-wrapped rather than cut.
    ///
    /// A selection cannot survive the re-wrap: its line numbers and columns point into a buffer
    /// that was joined and wrapped again, so [`Selection::follow`](crate::select::Selection::follow)
    /// answers false after it and the caller drops the selection, as it does when a program
    /// rewrites the text.
    pub fn resize_reflow(&mut self, cols: usize, rows: usize) {
        self.resize_reflow_anchored(cols, rows, None);
    }

    /// [`resize_reflow`](Self::resize_reflow), and it says where the content that was at
    /// absolute line `anchor` is after it (`None` for one the reflow did not keep, or when no
    /// anchor was given): what a caller that was scrolled back to a line needs to stay on it,
    /// the numbers having moved with the text.
    ///
    /// A caller that resizes a window while it is being dragged should call this once, when the
    /// drag ends, and [`resize`](Self::resize) while it is going on: each reflow joins and
    /// wraps the whole scrollback again, and a scrollback that grows past its limit at every
    /// intermediate width loses its oldest lines for good.
    pub fn resize_reflow_anchored(
        &mut self,
        cols: usize,
        rows: usize,
        anchor: Option<u64>,
    ) -> Option<u64> {
        let (cols, rows) = (cols.clamp(1, MAX_SIDE), rows.clamp(1, MAX_SIDE));
        if (cols, rows) == (self.cols, self.rows) {
            return anchor;
        }
        // A new height alone leaves every line of text as it was.
        if cols == self.cols {
            self.resize(cols, rows);
            return anchor;
        }
        let alt = self.modes.alt_screen;
        if alt {
            // The picture the program drew is cut and padded, and what falls off its top is
            // not history.
            let mut row = self.row;
            let _ = fit_rows(&mut self.grid, rows, cols, &mut row);
            self.row = row;
            for line in &mut self.grid {
                line.resize(cols);
            }
        }
        // The buffer that is the primary one: the grid when it is shown, the stored one when
        // the alternate screen is. Its cursor's column is the saved one, which is where the
        // program left it, unless it never saved one.
        let saved = self.saved[0].map(|s| (s.row, s.col, s.pending));
        // An anchor is a line of the buffer on screen, which the alternate screen never is.
        let anchor = if alt {
            None
        } else {
            anchor.and_then(|line| usize::try_from(line.checked_sub(self.dropped)?).ok())
        };
        let (grid, cursor) = if alt {
            let (col, pending) = self.saved[0].map_or((self.col, false), |s| (s.col, s.pending));
            let main = self.main.take().unwrap_or_default();
            (main, (self.main_row, col, pending))
        } else {
            (
                std::mem::take(&mut self.grid),
                (self.row, self.col, self.pending),
            )
        };
        let out = self.reflow_buffer(grid, cursor, saved, anchor, cols, rows);
        if alt {
            self.main = Some(out.grid);
            self.main_row = out.row;
        } else {
            self.grid = out.grid;
            self.row = out.row;
            self.col = out.col;
            self.pending = out.pending && self.modes.autowrap;
        }
        if let (Some(saved), Some((row, col, pending))) = (self.saved[0].as_mut(), out.saved) {
            saved.row = row;
            saved.col = col;
            saved.pending = pending;
        }
        self.fit_tabs(cols);
        (self.cols, self.rows) = (cols, rows);
        (self.top, self.bottom) = (0, rows - 1);
        if alt {
            self.row = self.row.min(rows - 1);
            self.col = self.col.min(cols - 1);
            self.pending = false;
        }
        self.dirty.clear();
        self.dirty.resize(rows, true);
        self.reflows = self.reflows.wrapping_add(1);
        out.anchor.map(|line| self.dropped + line as u64)
    }

    /// Re-wraps one buffer, its scrollback first, at `cols` by `rows`: the grid that is shown,
    /// or the main screen while the alternate one is. `cursor` and `saved` are places in `grid`
    /// (a row, a column and whether the place hangs past a full row), and `anchor` is a line of
    /// the buffer the caller wants to find again. The result gives the grid that fits, where the
    /// cursor is on it, where the saved place is, and where the anchor went. The lines that do
    /// not fit the grid go into the scrollback.
    fn reflow_buffer(
        &mut self,
        grid: Vec<Line>,
        cursor: (usize, usize, bool),
        saved: Option<(usize, usize, bool)>,
        anchor: Option<usize>,
        cols: usize,
        rows: usize,
    ) -> Reflowed {
        let mut lines: Vec<Line> = self.scrollback.drain(..).collect();
        let first = lines.len();
        lines.extend(grid);
        let last = lines.len().saturating_sub(1);
        let spot = |(row, col, pending): (usize, usize, bool)| reflow::Spot {
            line: (first + row).min(last),
            col,
            pending,
        };
        let anchor_spot = anchor.map(|line| reflow::Spot {
            line: line.min(last),
            col: 0,
            pending: false,
        });
        let (mut lines, places) = reflow::reflow(
            lines,
            [Some(spot(cursor)), saved.map(spot), anchor_spot],
            cols,
        );
        let place = places[0].unwrap_or_default();
        // The content ends at its last line, or at the cursor's line when that is below it: the
        // empty rows after that are not content and are dropped. The grid shows the bottom of
        // the content, so the lines the new wrapping added above it go into the scrollback and
        // none below it is lost. Only when the content below the cursor is taller than the
        // screen does the cursor's line end up above the grid, and then its row is the top one
        // while the text it is on is history: nothing is lost, and a caller that scrolls back
        // can show it.
        let mut end = place.line;
        for (i, line) in lines.iter().enumerate().skip(place.line) {
            if line.text_len > 0 {
                end = i;
            }
        }
        lines.truncate(end + 1);
        let start = end.saturating_add(1).saturating_sub(rows);
        let mut grid = lines.split_off(start);
        for line in lines {
            self.push_scrollback(line);
        }
        while grid.len() < rows {
            grid.push(Line::blank(cols, Cell::BLANK));
        }
        Reflowed {
            row: place.line.saturating_sub(start),
            col: place.col,
            pending: place.pending,
            saved: places[1].map(|p| {
                (
                    p.line.saturating_sub(start).min(rows.saturating_sub(1)),
                    p.col,
                    p.pending,
                )
            }),
            anchor: places[2].map(|p| p.line),
            grid,
        }
    }

    /// How many times the width of the screen was reflowed. A selection made under an older
    /// count cannot know where its text went, and is dropped.
    pub(crate) fn reflows(&self) -> u64 {
        self.reflows
    }

    /// Brings the tab stops to `cols` columns: the ones that are there stay, the new ones are
    /// every eighth column, as a terminal starts with them.
    pub(super) fn fit_tabs(&mut self, cols: usize) {
        let old = self.tabs.len();
        self.tabs.resize(cols, false);
        for (c, stop) in self.tabs.iter_mut().enumerate().skip(old) {
            *stop = c % 8 == 0;
        }
    }
}

/// What a re-wrap of one buffer gave: the grid that fits the screen, and where the cursor and
/// the saved place are on it.
struct Reflowed {
    grid: Vec<Line>,
    row: usize,
    col: usize,
    pending: bool,
    saved: Option<(usize, usize, bool)>,
    /// The line the caller's anchor is on now, when it was given one.
    anchor: Option<usize>,
}
