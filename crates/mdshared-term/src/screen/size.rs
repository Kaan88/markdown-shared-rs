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
    /// and wrapped again at the new width, as xterm, VTE and kitty do, so that a line longer than
    /// the screen is not cut and nothing is lost when the screen widens again.
    ///
    /// - A hard line break, one whose line is not marked as wrapped, stays where it is.
    /// - The blank cells at the end of a line are not text and are dropped, so the padding of a
    ///   row does not grow a space each time the width changes. The blank cells at the end of a
    ///   line that goes on in the next one are text, though (a space written where the line
    ///   broke off), and are kept; only the one cell a wide character was moved out of is
    ///   dropped. A blank cell the program gave a colour is something on the screen and is kept.
    /// - A character two cells wide is never cut: when it does not fit in the last column of the
    ///   row, that column stays empty and the character goes in the row below.
    /// - The cursor stays on the same character of the same line of text; a cursor past the end
    ///   of that text keeps its place as far as the new width has columns for it, and a cursor
    ///   that hangs past the right edge keeps its pending wrap.
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
    /// of height alone gives. The two are the same call then, so a caller that wants no reflow at
    /// all uses [`resize`](Self::resize).
    ///
    /// A selection cannot survive the re-wrap: its line numbers and columns point into a buffer
    /// that was joined and wrapped again, so [`Selection::follow`](crate::select::Selection::follow)
    /// answers false after it and the caller drops the selection, as it does when a program
    /// rewrites the text.
    pub fn resize_reflow(&mut self, cols: usize, rows: usize) {
        let (cols, rows) = (cols.clamp(1, MAX_SIDE), rows.clamp(1, MAX_SIDE));
        if (cols, rows) == (self.cols, self.rows) {
            return;
        }
        // A picture drawn cell by cell is not a text to re-wrap, and a new height alone leaves
        // every line of text as it was.
        if self.modes.alt_screen || cols == self.cols {
            self.resize(cols, rows);
            return;
        }
        let cursor = self.scrollback.len() + self.row;
        let grid = std::mem::take(&mut self.grid);
        let mut lines: Vec<Line> = self.scrollback.drain(..).collect();
        lines.extend(grid);
        let (mut lines, place) = reflow::reflow(lines, cursor, self.col, self.pending, cols);
        // The grid shows the end of the buffer, or the cursor's line at its top when the text
        // below it is longer than the screen: then the lines past the bottom are lost, as the
        // lines below the cursor are when rows go.
        let start = place.line.min(lines.len().saturating_sub(rows));
        let mut grid = lines.split_off(start);
        for line in lines {
            self.push_scrollback(line);
        }
        grid.truncate(rows);
        while grid.len() < rows {
            grid.push(Line::blank(cols, Cell::BLANK));
        }
        let row = place.line - start;
        self.grid = grid;
        self.fit_tabs(cols);
        (self.cols, self.rows) = (cols, rows);
        (self.top, self.bottom) = (0, rows - 1);
        self.row = row.min(rows - 1);
        self.col = place.col.min(cols - 1);
        self.pending = place.pending && self.modes.autowrap;
        self.dirty = vec![true; rows];
        self.reflows = self.reflows.wrapping_add(1);
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
