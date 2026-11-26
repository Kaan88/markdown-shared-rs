//! Joining and re-wrapping the lines of a screen when its width changes.
//!
//! A [`Line`] carries a `wrapped` flag: it is set when the text on that line reached the right
//! edge and went on in the line below, so the two are one line of text and not two. [`reflow`]
//! takes the kept lines of a screen and the place of its cursor, joins every run of wrapped lines
//! back into the one line of text it was, wraps each line of text again at a new width, and says
//! where the cursor landed. Nothing here reads or writes a [`Screen`](crate::screen::Screen), a
//! clock or an event: the screen hands its lines over and puts the result back, and the tests
//! call this directly.
//!
//! The rules, which are the ones xterm, VTE and kitty follow:
//!
//! - A line whose `wrapped` is false ends its line of text. A hard break therefore stays where
//!   it is, and the lines on either side of it are wrapped apart.
//! - The blank cells at the end of a line of text are not text: they are what the screen did not
//!   write yet, and they are dropped. The blank cells at the end of a line that goes on in the
//!   next one are text, though, because a space was written where the line broke off; only the
//!   one cell a wide character was moved out of is dropped, and [`Line`] says which line that
//!   happened on. So a wrap that fell on a space keeps it, and a wrap that fell on a wide
//!   character does not grow one. A blank cell the program gave a colour or another attribute is
//!   kept: it is something on the screen.
//! - A character two cells wide is never cut: when it does not fit in the last column of the
//!   row being built, that column stays empty and the character goes in the row below, exactly
//!   as writing it at the right edge does.
//! - The cursor is a place in the joined line of text rather than a place on the screen, so it
//!   stays on the same character. A place past the end of the text (where a prompt waits) is
//!   kept as far as the new width has columns for it, and a cursor that hangs past the last
//!   column of a full row keeps its pending wrap.
//!
//! Deliberately not here: the screen's scrollback limit, its scribe of absolute line numbers, the
//! alternate screen (a program's own picture is cut and padded where it is, not reflowed), and
//! anything about selections or views.

use crate::screen::{Cell, Line};

/// Where the cursor is after a reflow: its line in the reflowed buffer, its column, and whether
/// the last cell of that column is written, so that the next character wraps to the line below.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct CursorPlace {
    /// The index into the reflowed lines of the line the cursor is on. Always inside them.
    pub line: usize,
    /// The column of that line, always below the new width.
    pub col: usize,
    /// The cursor stands after the last cell of a full row: what `Screen::pending` means.
    pub pending: bool,
}

/// Re-wraps `lines` at `cols` columns, with the cursor at `cursor` (an index into `lines`), on
/// column `col` of that line, and `pending` set when it stands past the last cell of a full row.
///
/// Returns the reflowed lines (each exactly `cols` cells wide, every one but the last of its line
/// of text marked as wrapped) and where the cursor landed. The number of cells a line of text
/// holds can drop (the blank cells at the ends go), so the returned lines are usually fewer or
/// more than the input; the caller decides what to do with the ones that do not fit its screen.
///
/// Nothing here can fail or panic: `cols` is taken as at least one, a line of text that is empty
/// gives one empty line, a lone half of a wide character is treated as one cell wide, and a
/// cursor outside `lines` is reported on the first line.
pub(crate) fn reflow(
    mut lines: Vec<Line>,
    cursor: usize,
    col: usize,
    pending: bool,
    cols: usize,
) -> (Vec<Line>, CursorPlace) {
    let cols = cols.max(1);
    let mut out: Vec<Line> = Vec::with_capacity(lines.len());
    let mut place = CursorPlace::default();
    let mut i = 0;
    while i < lines.len() {
        // The run of written lines that make up one line of text, joined: each one but the last
        // is marked as going on in the next.
        let first = i;
        let mut cells: Vec<Cell> = Vec::new();
        // Where the cells of each written line begin in `cells`, for the cursor's place.
        let mut starts: Vec<usize> = Vec::new();
        loop {
            starts.push(cells.len());
            let line = &mut lines[i];
            let wrapped = line.wrapped();
            // A blank cell at the end of a line is not text. The blank cells at the end of a
            // wrapped line are, though: a space was written where the text broke off, and only
            // the one cell a wide character was moved out of is dropped.
            let keep = match (wrapped, line.is_padded()) {
                (true, false) => line.len(),
                (true, true) => line.len().saturating_sub(1),
                (false, _) => content_end(line.cells()),
            };
            let mut taken = line.take_cells();
            taken.truncate(keep);
            cells.append(&mut taken);
            if !wrapped || i + 1 >= lines.len() {
                break;
            }
            i += 1;
        }
        let content = cells.len();
        let base = out.len();
        // The cursor's place in this line of text, as a cell offset from its first one, when
        // the cursor is on this line. A cursor on the second cell of a wide character stands
        // after it.
        let wanted = (first..=i).contains(&cursor).then(|| {
            starts[cursor - first]
                + col
                + usize::from(pending || on_spacer(&cells, &starts, cursor - first, col))
        });
        let mut place_in_line: Option<(usize, usize, bool)> = None;
        // Wrap the joined line of text again, row by row, noting where the cursor lands.
        let mut at = 0;
        let mut row_index = 0;
        loop {
            let mut row = vec![Cell::BLANK; cols];
            let mut to = 0;
            let mut padded = false;
            while at < cells.len() {
                let wide = span(&cells, at) == 2;
                let mut w = if wide { 2 } else { 1 };
                if to + w > cols {
                    if to == 0 {
                        // One column of room for a wide character, as writing one gives it.
                        w = 1;
                    } else {
                        // Only a wide character leaves a cell empty, and only in the last
                        // column: it is marked as a blank that is not text. A row that is
                        // simply full has no such cell.
                        padded = wide && to + 1 == cols;
                        break;
                    }
                }
                // The cursor at this cell's offset stands before it.
                if wanted == Some(at) && place_in_line.is_none() {
                    place_in_line = Some((row_index, to, false));
                }
                row[to] = if usize::from(cells[at].width) == w {
                    cells[at]
                } else {
                    Cell {
                        width: w as u8,
                        ..cells[at]
                    }
                };
                if w == 2 {
                    row[to + 1] = cells[at + 1];
                }
                to += w;
                // A wide character squeezed into one column takes its spacer with it.
                at += if wide { 2 } else { 1 };
                // And a cursor on that spacer stands after the character.
                if wide && wanted == Some(at - 1) && place_in_line.is_none() {
                    place_in_line = Some(if to < cols {
                        (row_index, to, false)
                    } else {
                        (row_index, cols - 1, true)
                    });
                }
            }
            let more = at < cells.len();
            let mut line = Line::from_cells(row, more);
            line.set_padded(padded);
            out.push(line);
            if !more {
                // The cursor past the last cell of the text stands after it, and as far along
                // the row as the new width has columns for. When the row is full, that place is
                // its last column, with a wrap pending as it is after any full row.
                if let Some(offset) = wanted
                    && place_in_line.is_none()
                {
                    let col = to
                        .saturating_add(offset.saturating_sub(content))
                        .min(cols - 1);
                    place_in_line = Some((row_index, col, to >= cols));
                }
                break;
            }
            row_index += 1;
        }
        if let Some((line, col, pending)) = place_in_line {
            place = CursorPlace {
                line: base + line,
                col,
                pending,
            };
        }
        i += 1;
    }
    (out, place)
}

/// How many cells of `cells` are content: those up to the last one that is not a plain blank.
/// The blank cells after it are cells nothing was written to.
fn content_end(cells: &[Cell]) -> usize {
    cells
        .iter()
        .rposition(|cell| *cell != Cell::BLANK)
        .map_or(0, |at| at + 1)
}

/// How many cells the character at `at` takes: two when it and the spacer after it are both
/// there, and one otherwise (a loose spacer, or the first half of a wide character whose second
/// half is missing, is drawn as one cell of its own rather than breaking the grid).
fn span(cells: &[Cell], at: usize) -> usize {
    match cells[at].width {
        2 if at + 1 < cells.len() && cells[at + 1].width == 0 => 2,
        _ => 1,
    }
}

/// Whether column `col` of the joined piece that begins at `starts[piece]` is the second cell of
/// a wide character. A cursor there stands after that character, not inside it, because a
/// character is written whole or not at all.
fn on_spacer(cells: &[Cell], starts: &[usize], piece: usize, col: usize) -> bool {
    let start = starts[piece];
    let end = starts.get(piece + 1).copied().unwrap_or(cells.len());
    start + col < end && cells[start + col].is_spacer()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::Color;
    use crate::width::char_width;

    /// The cells of `text` as the screen writes them: a wide character and its spacer.
    fn cells_of(text: &str) -> Vec<Cell> {
        let mut out = Vec::new();
        for ch in text.chars() {
            let width = char_width(ch);
            out.push(Cell {
                ch,
                width,
                ..Cell::BLANK
            });
            if width == 2 {
                out.push(Cell {
                    ch: ' ',
                    width: 0,
                    ..Cell::BLANK
                });
            }
        }
        out
    }

    /// A written line of `text`, going on in the next when `wrapped`.
    fn line(text: &str, wrapped: bool) -> Line {
        Line::from_cells(cells_of(text), wrapped)
    }

    /// The text of each produced row, with the trailing spaces dropped.
    fn rows(lines: &[Line]) -> Vec<String> {
        lines.iter().map(Line::text).collect()
    }

    /// The wrapped flags of the produced rows, as `-` for a hard break and `>` for a wrap.
    fn flags(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|line| if line.wrapped() { '>' } else { '-' })
            .collect()
    }

    #[test]
    fn a_wrap_is_joined_and_wrapped_again_at_the_new_width() {
        // Two written lines that are one line of text of eight characters, wrapped at four.
        let (out, place) = reflow(
            vec![line("abcdef", true), line("gh", false)],
            1,
            2,
            false,
            4,
        );
        assert_eq!(rows(&out), ["abcd", "efgh"]);
        assert_eq!(flags(&out), ">-");
        // The cursor was past the last character of a full row: it keeps that place, and the
        // pending wrap that says the next character goes in the row below.
        assert_eq!(
            place,
            CursorPlace {
                line: 1,
                col: 3,
                pending: true
            }
        );
        // Wrapped wider again, the same text comes back, and the cursor is in the same place
        // in it (now a free cell of a row that is not full, so nothing is pending).
        let (out, place) = reflow(out, place.line, place.col, place.pending, 6);
        assert_eq!(rows(&out), ["abcdef", "gh"]);
        assert_eq!(flags(&out), ">-");
        assert_eq!(
            place,
            CursorPlace {
                line: 1,
                col: 2,
                pending: false
            }
        );
    }

    #[test]
    fn a_hard_break_ends_a_run_and_the_blank_cells_at_an_end_are_not_text() {
        // "ab   " is one line of text that ends in blank cells; the next line is its own.
        let (out, _) = reflow(
            vec![line("ab   ", false), line("cd  ", false)],
            0,
            0,
            false,
            8,
        );
        assert_eq!(rows(&out), ["ab", "cd"]);
        assert_eq!(flags(&out), "--");
        // The blank cells at the end of a wrapped line are a space written where the text
        // broke off, so they stay: the join is "ab  cd" and the line of text is not shortened.
        let (out, _) = reflow(vec![line("ab  ", true), line("cd", false)], 0, 3, false, 6);
        assert_eq!(rows(&out), ["ab  cd"]);
        // The one cell a wide character was moved out of is not a space of the text, and only
        // that one cell is dropped.
        let mut pad = line("ab ", true);
        pad.set_padded(true);
        let (out, _) = reflow(vec![pad, line("漢", false)], 0, 3, false, 8);
        assert_eq!(rows(&out), ["ab漢"]);
        // A blank cell before it was written where the text broke off, so it stays.
        let mut pad = line("ab  ", true);
        pad.set_padded(true);
        let (out, _) = reflow(vec![pad, line("漢", false)], 0, 3, false, 8);
        assert_eq!(rows(&out), ["ab 漢"]);
    }

    #[test]
    fn a_coloured_blank_is_content_and_is_kept() {
        let red = Cell {
            bg: Color::Indexed(1),
            ..Cell::BLANK
        };
        let cells = vec![cells_of("a")[0], red, red];
        let (out, _) = reflow(vec![Line::from_cells(cells, false)], 0, 0, false, 4);
        assert_eq!(out[0].len(), 4);
        assert_eq!(out[0].cells()[..3], [cells_of("a")[0], red, red]);
        assert_eq!(out[0].cell(3), Cell::BLANK);
    }

    #[test]
    fn a_wide_character_at_the_edge_moves_down_and_is_never_cut() {
        // At four columns "ab漢" fits whole; at three the last column stays empty for it.
        let (out, _) = reflow(vec![line("ab漢", false)], 0, 0, false, 3);
        assert_eq!(rows(&out), ["ab", "漢"]);
        assert_eq!(out[0].cell(2), Cell::BLANK);
        assert_eq!(out[1].cell(0).ch, '漢');
        assert!(out[1].cell(1).is_spacer());
        assert!(out[0].wrapped() && !out[1].wrapped());
        // One column is not wide enough for any wide character: it takes the column, as
        // writing one on a screen one column wide does.
        let (out, _) = reflow(vec![line("ab漢", false)], 0, 0, false, 1);
        assert_eq!(rows(&out), ["a", "b", "漢"]);
        assert_eq!(out[2].cell(0).width, 1);
        assert!(out.iter().all(|line| line.len() == 1));
        // A cursor on the second cell of a wide character stands after it: wrapped wider
        // again, the character is one cell and the cursor hangs past it, as it did.
        let (out, place) = reflow(vec![line("abcd漢", false)], 0, 5, true, 5);
        assert_eq!(rows(&out), ["abcd", "漢"]);
        assert_eq!(
            place,
            CursorPlace {
                line: 1,
                col: 2,
                pending: false
            },
            "after the character, not on its second cell"
        );
        let (out, place) = reflow(out, place.line, place.col, place.pending, 6);
        assert_eq!(rows(&out), ["abcd漢"]);
        assert_eq!(
            place,
            CursorPlace {
                line: 0,
                col: 5,
                pending: true
            }
        );
    }

    #[test]
    fn a_cursor_past_the_end_of_a_full_row_keeps_its_pending_wrap() {
        // Six columns of text, the cursor on the last one, the next character wrapping.
        let (out, place) = reflow(vec![line("abcdef", false)], 0, 5, true, 3);
        assert_eq!(rows(&out), ["abc", "def"]);
        assert_eq!(
            place,
            CursorPlace {
                line: 1,
                col: 2,
                pending: true
            },
            "the cursor is after the last character of a full row"
        );
        // A cursor past the end on a column the new width does not have is brought to the
        // last column, without a pending wrap when that cell is free.
        let (out, place) = reflow(vec![line("ab", false)], 0, 9, false, 4);
        assert_eq!(rows(&out), ["ab"]);
        assert_eq!(
            place,
            CursorPlace {
                line: 0,
                col: 3,
                pending: false
            }
        );
        // An empty line of text is still one row.
        let (out, place) = reflow(vec![line("", false)], 0, 0, false, 5);
        assert_eq!(out.len(), 1);
        assert_eq!(place, CursorPlace::default());
        // A cursor outside the lines does not panic.
        let (out, _) = reflow(vec![line("ab", false)], 99, 99, false, 5);
        assert_eq!(out.len(), 1);
    }
}
