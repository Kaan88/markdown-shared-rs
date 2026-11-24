//! Selecting text on a terminal: from a cell to a cell, through the scrollback and the screen
//! alike, by cells, words or whole lines, as a stream of text or as a rectangle.
//!
//! A selection holds absolute line numbers ([`Screen::first_line`]), so it stays on its text
//! while output scrolls the screen. It holds no reference to the screen: the calls that need to
//! read cells are given it. The caller turns its pointer events into [`Point`]s, starts a
//! selection on a press ([`Selection::start`], with the number of clicks), moves it on a drag
//! ([`Selection::extend`]), asks [`Selection::span_on`] for what to paint on each line, and
//! [`Selection::text`] for what to copy.
//!
//! What it does not know: when the program rewrites the cells under it. The numbers follow text
//! only when the whole screen scrolls, so a caller drops the selection when output changes a row
//! it covers ([`Screen::take_dirty`] against [`Selection::range`]), as terminals do.

use crate::screen::{Line, Screen};

/// A cell of the line space: an absolute line number and a column.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Point {
    pub line: u64,
    pub col: usize,
}

impl Point {
    pub const fn new(line: u64, col: usize) -> Point {
        Point { line, col }
    }
}

/// The column that stands for "to the end of the line", whatever its length.
pub const END: usize = usize::MAX;

/// The shape of a selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum SelectionKind {
    /// As text flows: from the first cell to the end of its line, whole lines between, and the
    /// last line up to the last cell.
    Stream,
    /// A rectangle of cells: the same columns on every line (Alt and drag, in most terminals).
    Block,
}

/// What a selection grows by.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Unit {
    Cell,
    Word,
    Line,
}

/// A selection in progress or made. See the [module](self).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selection {
    kind: SelectionKind,
    unit: Unit,
    /// The first and last cell of the unit that was pressed on.
    anchor: (Point, Point),
    /// The first and last cell of the unit the pointer is on now.
    head: (Point, Point),
}

/// The characters that belong to a word besides letters and digits: those of file paths, web
/// addresses and options, so a double click takes `~/src/a-b_c.rs`, `https://host/x?y=1&z=2#top`
/// or `--no-verify` whole.
pub const WORD_PUNCTUATION: &str = "_-./~:@%+#=?&";

/// What a character is to a double click: a blank, part of a word, or something else, which
/// is a word of its own cell.
fn class(c: char) -> u8 {
    if c == ' ' {
        0
    } else if c.is_alphanumeric() || WORD_PUNCTUATION.contains(c) {
        1
    } else {
        2
    }
}

/// The character shown in column `col`: for the second cell of a wide one, that character.
fn shown(line: &Line, col: usize) -> char {
    let cell = line.cell(col);
    if cell.is_spacer() && col > 0 {
        line.cell(col - 1).ch
    } else {
        cell.ch
    }
}

/// The first and last column of the character at `col`: both the same, or the two cells of a
/// wide character, whichever of them `col` is.
fn cell_span(line: Option<&Line>, col: usize) -> (usize, usize) {
    let Some(line) = line else {
        return (col, col);
    };
    let cell = line.cell(col);
    if cell.is_spacer() && col > 0 {
        (col - 1, col)
    } else if cell.width == 2 {
        (col, col + 1)
    } else {
        (col, col)
    }
}

impl Selection {
    /// A selection begun by a press at `at`. `clicks` is which press of a row of quick presses
    /// it is: the first selects by cells and selects nothing until it is dragged, the second
    /// takes the word under it and grows by words, the third (and later ones) the line and
    /// grows by lines.
    ///
    /// A word is a run of letters, digits and the characters of [`WORD_PUNCTUATION`], or a run
    /// of blanks, or one other character; it does not cross from one row to the next, also not
    /// where a line wrapped. A line is the whole line of text: a row together with the rows it
    /// wrapped from and into.
    pub fn start(screen: &Screen, at: Point, kind: SelectionKind, clicks: u8) -> Selection {
        let unit = match clicks {
            0 | 1 => Unit::Cell,
            2 => Unit::Word,
            _ => Unit::Line,
        };
        let extent = unit_at(screen, at, unit);
        Selection {
            kind,
            unit,
            anchor: extent,
            head: extent,
        }
    }

    /// Moves the end that is being dragged to `to`. The other end stays where the press was;
    /// the selection grows by the unit it was started with, to either side of the press.
    pub fn extend(&mut self, screen: &Screen, to: Point) {
        self.head = unit_at(screen, to, self.unit);
    }

    pub fn kind(&self) -> SelectionKind {
        self.kind
    }

    /// Nothing is selected: a single press that was not dragged off its cell.
    pub fn is_empty(&self) -> bool {
        self.unit == Unit::Cell && self.anchor == self.head
    }

    /// The first and the last cell selected, both inside the selection, in reading order. For a
    /// block these are its top left and bottom right corners. A last column of [`END`] means
    /// "through the end of the line".
    pub fn range(&self) -> (Point, Point) {
        match self.kind {
            SelectionKind::Stream => (
                self.anchor.0.min(self.head.0),
                self.anchor.1.max(self.head.1),
            ),
            SelectionKind::Block => {
                let ends = [self.anchor.0, self.anchor.1, self.head.0, self.head.1];
                let line = |f: fn(u64, u64) -> u64| ends.iter().map(|p| p.line).reduce(f);
                let col = |f: fn(usize, usize) -> usize| ends.iter().map(|p| p.col).reduce(f);
                (
                    Point::new(line(u64::min).unwrap_or(0), col(usize::min).unwrap_or(0)),
                    Point::new(line(u64::max).unwrap_or(0), col(usize::max).unwrap_or(0)),
                )
            }
        }
    }

    /// The columns selected on `line`: the first and the last, both inside, the last being
    /// [`END`] when the selection runs through the end of the line. `None` when the line has
    /// nothing selected. This is what a caller paints.
    pub fn span_on(&self, line: u64) -> Option<(usize, usize)> {
        if self.is_empty() {
            return None;
        }
        let (start, end) = self.range();
        if line < start.line || line > end.line {
            return None;
        }
        Some(match self.kind {
            SelectionKind::Block => (start.col, end.col),
            SelectionKind::Stream => (
                if line == start.line { start.col } else { 0 },
                if line == end.line { end.col } else { END },
            ),
        })
    }

    /// Whether the cell at `line` and `col` is selected.
    pub fn contains(&self, line: u64, col: usize) -> bool {
        self.span_on(line)
            .is_some_and(|(a, b)| a <= col && col <= b)
    }

    /// Cuts the selection to the lines the screen still keeps: the part that was in lines the
    /// scrollback has dropped is let go. Returns false when nothing of it is left (the caller
    /// then drops the selection).
    pub fn clip(&mut self, screen: &Screen) -> bool {
        let (first, end) = (screen.first_line(), screen.end_line());
        let (_, last) = self.range();
        if last.line < first || self.is_empty() {
            return false;
        }
        let stream = self.kind == SelectionKind::Stream;
        for p in [
            &mut self.anchor.0,
            &mut self.anchor.1,
            &mut self.head.0,
            &mut self.head.1,
        ] {
            if p.line < first {
                p.line = first;
                if stream {
                    p.col = 0;
                }
            } else if p.line >= end {
                p.line = end - 1;
                if stream {
                    p.col = END;
                }
            }
        }
        true
    }

    /// The selected text. Each row gives its selected characters without trailing spaces, and
    /// the rows are joined by a line feed; in a stream selection a row that wrapped into the
    /// next is joined to it with nothing between and keeps its spaces, so a long command comes
    /// out as the one line it was typed as. A block gives one line a row. Characters that are
    /// hidden by their attributes are copied like any other. Rows the scrollback has dropped
    /// are left out.
    pub fn text(&self, screen: &Screen) -> String {
        let mut out = String::new();
        if self.is_empty() {
            return out;
        }
        let (start, end) = self.range();
        let first = start.line.max(screen.first_line());
        let last = end.line.min(screen.end_line().saturating_sub(1));
        for n in first..=last {
            let (Some(line), Some((a, b))) = (screen.line_at(n), self.span_on(n)) else {
                continue;
            };
            let from = out.len();
            let through_end = b >= line.len().saturating_sub(1);
            for cell in line.cells().iter().skip(a).take((b - a).saturating_add(1)) {
                if !cell.is_spacer() {
                    out.push(cell.ch);
                }
            }
            let joined =
                self.kind == SelectionKind::Stream && line.wrapped() && through_end && n != last;
            if !joined {
                let kept = out[from..].trim_end_matches(' ').len();
                out.truncate(from + kept);
                if n != last {
                    out.push('\n');
                }
            }
        }
        out
    }
}

/// The first and last cell of the unit at `at`.
fn unit_at(screen: &Screen, at: Point, unit: Unit) -> (Point, Point) {
    let line = screen.line_at(at.line);
    match unit {
        Unit::Cell => {
            let (a, b) = cell_span(line, at.col);
            (Point::new(at.line, a), Point::new(at.line, b))
        }
        Unit::Word => {
            let Some(line) = line else {
                return (at, at);
            };
            let kind = class(shown(line, at.col));
            let (mut a, mut b) = cell_span(Some(line), at.col);
            // A character that is neither blank nor of a word is a word alone.
            if kind != 2 {
                while a > 0 && class(shown(line, a - 1)) == kind {
                    a -= 1;
                }
                while b + 1 < line.len() && class(shown(line, b + 1)) == kind {
                    b += 1;
                }
            }
            (Point::new(at.line, a), Point::new(at.line, b))
        }
        Unit::Line => {
            let (mut top, mut bottom) = (at.line, at.line);
            while top > screen.first_line() && screen.line_at(top - 1).is_some_and(Line::wrapped) {
                top -= 1;
            }
            while screen.line_at(bottom).is_some_and(Line::wrapped)
                && bottom + 1 < screen.end_line()
            {
                bottom += 1;
            }
            (Point::new(top, 0), Point::new(bottom, END))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A screen 12 wide and 4 high with a scrollback, fed `text`.
    fn screen(text: &str) -> Screen {
        let mut s = Screen::new(12, 4, 100);
        s.feed(text.as_bytes());
        s
    }

    fn drag(s: &Screen, from: (u64, usize), to: (u64, usize), kind: SelectionKind) -> Selection {
        let mut sel = Selection::start(s, Point::new(from.0, from.1), kind, 1);
        sel.extend(s, Point::new(to.0, to.1));
        sel
    }

    #[test]
    fn a_press_selects_nothing_and_a_drag_selects_from_cell_to_cell() {
        let s = screen("hello world\r\nsecond line\r\nthird");
        let mut sel = Selection::start(&s, Point::new(0, 6), SelectionKind::Stream, 1);
        assert!(sel.is_empty());
        assert_eq!(sel.text(&s), "");
        assert_eq!(sel.span_on(0), None);
        assert!(!sel.contains(0, 6));
        sel.extend(&s, Point::new(0, 8));
        assert!(!sel.is_empty());
        assert_eq!(sel.text(&s), "wor");
        assert_eq!(sel.range(), (Point::new(0, 6), Point::new(0, 8)));
        assert!(sel.contains(0, 6) && sel.contains(0, 8));
        assert!(!sel.contains(0, 5) && !sel.contains(0, 9) && !sel.contains(1, 7));
        // Dragged back past the press: the same ends in reading order.
        sel.extend(&s, Point::new(0, 2));
        assert_eq!(sel.text(&s), "llo w");
        // Over lines: the first from its cell to its end, the last up to its cell.
        sel.extend(&s, Point::new(2, 1));
        assert_eq!(sel.text(&s), "world\nsecond line\nth");
        assert_eq!(sel.span_on(0), Some((6, END)));
        assert_eq!(sel.span_on(1), Some((0, END)));
        assert_eq!(sel.span_on(2), Some((0, 1)));
        assert_eq!(sel.span_on(3), None);
        assert!(sel.contains(1, 500) && sel.contains(0, 11) && !sel.contains(2, 2));
        assert_eq!(sel.kind(), SelectionKind::Stream);
    }

    #[test]
    fn a_block_takes_the_same_columns_of_every_row() {
        let s = screen("abcdefgh\r\nij\r\nklmnopqr");
        let sel = drag(&s, (0, 2), (2, 4), SelectionKind::Block);
        assert_eq!(sel.text(&s), "cde\n\nmno");
        assert_eq!(sel.span_on(1), Some((2, 4)));
        assert!(sel.contains(1, 3) && !sel.contains(1, 5) && !sel.contains(1, 1));
        // From the bottom right to the top left it is the same rectangle.
        let back = drag(&s, (2, 4), (0, 2), SelectionKind::Block);
        assert_eq!(back.range(), sel.range());
        assert_eq!(back.text(&s), sel.text(&s));
        // And from the top right to the bottom left.
        let across = drag(&s, (0, 4), (2, 2), SelectionKind::Block);
        assert_eq!(across.text(&s), "cde\n\nmno");
    }

    #[test]
    fn a_second_click_takes_a_word_and_grows_by_words() {
        let s = screen("cd ~/src/a-b.rs  (x)");
        // 12 columns: "cd ~/src/a-b" wraps into ".rs  (x)".
        let word = |line: u64, col: usize| {
            Selection::start(&s, Point::new(line, col), SelectionKind::Stream, 2).text(&s)
        };
        assert_eq!(word(0, 0), "cd");
        assert_eq!(word(0, 1), "cd");
        assert_eq!(word(0, 5), "~/src/a-b");
        assert_eq!(word(1, 1), ".rs");
        // Blanks are a run of their own (which copies as nothing), and a bracket is alone.
        let blank = Selection::start(&s, Point::new(1, 3), SelectionKind::Stream, 2);
        assert_eq!(blank.range(), (Point::new(1, 3), Point::new(1, 4)));
        assert_eq!(word(1, 5), "(");
        assert_eq!(word(1, 6), "x");
        // Dragged, it grows by whole words to either side.
        let mut sel = Selection::start(&s, Point::new(0, 5), SelectionKind::Stream, 2);
        sel.extend(&s, Point::new(0, 1));
        assert_eq!(sel.text(&s), "cd ~/src/a-b");
        sel.extend(&s, Point::new(1, 6));
        assert_eq!(sel.text(&s), "~/src/a-b.rs  (x");
    }

    #[test]
    fn a_third_click_takes_the_line_with_the_rows_it_wrapped_over() {
        let s = screen("one\r\na long line that wraps twice..\r\nend");
        // Row 0 "one"; rows 1 to 3 the long line; then "end" scrolled row 0 off the top.
        assert_eq!(s.first_line(), 0);
        assert_eq!(s.screen_start(), 1);
        for line in 1..=3 {
            let sel = Selection::start(&s, Point::new(line, 4), SelectionKind::Stream, 3);
            assert_eq!(
                sel.text(&s),
                "a long line that wraps twice..",
                "from row {line}"
            );
            assert_eq!(sel.range(), (Point::new(1, 0), Point::new(3, END)));
        }
        let one = Selection::start(&s, Point::new(0, 1), SelectionKind::Stream, 3);
        assert_eq!(one.text(&s), "one");
        // Grown by lines: down from the first to the last.
        let mut sel = one;
        sel.extend(&s, Point::new(4, 0));
        assert_eq!(sel.text(&s), "one\na long line that wraps twice..\nend");
        assert!(!sel.is_empty());
    }

    #[test]
    fn wrapped_rows_copy_as_one_line_and_keep_their_spaces() {
        let s = screen("echo aaaaaa bbbbbb cccc\r\nnext");
        // "echo aaaaaa " ends its row with a space that belongs to the text.
        assert_eq!(s.row_text(0), "echo aaaaaa");
        let sel = drag(&s, (0, 0), (2, 3), SelectionKind::Stream);
        assert_eq!(sel.text(&s), "echo aaaaaa bbbbbb cccc\nnext");
        // A block does not join rows.
        let block = drag(&s, (0, 0), (1, 11), SelectionKind::Block);
        assert_eq!(block.text(&s), "echo aaaaaa\nbbbbbb cccc");
        // Ending in the middle of the first row, nothing is joined.
        let part = drag(&s, (0, 5), (0, 8), SelectionKind::Stream);
        assert_eq!(part.text(&s), "aaaa");
    }

    #[test]
    fn a_wide_character_is_selected_whole_from_either_of_its_cells() {
        let s = screen("a漢b");
        let from_second = drag(&s, (0, 2), (0, 3), SelectionKind::Stream);
        assert_eq!(from_second.range(), (Point::new(0, 1), Point::new(0, 3)));
        assert_eq!(from_second.text(&s), "漢b");
        let to_first = drag(&s, (0, 0), (0, 1), SelectionKind::Stream);
        assert_eq!(to_first.range().1, Point::new(0, 2));
        assert_eq!(to_first.text(&s), "a漢");
        let word = Selection::start(&s, Point::new(0, 2), SelectionKind::Stream, 2);
        assert_eq!(word.text(&s), "a漢b");
    }

    #[test]
    fn a_selection_stays_on_its_text_while_the_screen_scrolls_and_is_clipped_at_the_ring() {
        let mut s = Screen::new(12, 3, 4);
        s.feed(b"zero\r\none\r\ntwo");
        let mut sel = drag(&s, (0, 1), (1, 1), SelectionKind::Stream);
        assert_eq!(sel.text(&s), "ero\non");
        // Two more lines: the selected rows are in the scrollback now, under the same numbers.
        s.feed(b"\r\nthree\r\nfour");
        assert_eq!(s.screen_start(), 2);
        assert_eq!(sel.text(&s), "ero\non");
        assert!(sel.clip(&s));
        assert_eq!(sel.range().0, Point::new(0, 1));
        // The ring keeps four: after three more, line 0 is gone and the selection starts at 1.
        s.feed(b"\r\n5\r\n6\r\n7");
        assert_eq!(s.first_line(), 1);
        assert_eq!(sel.text(&s), "on");
        assert!(sel.clip(&s));
        assert_eq!(sel.range(), (Point::new(1, 0), Point::new(1, 1)));
        // And once all of it is gone, so is the selection.
        s.feed(b"\r\n8\r\n9");
        assert_eq!(sel.text(&s), "");
        assert!(!sel.clip(&s));
        // A point past the last line is brought in.
        let mut far = drag(&s, (s.screen_start(), 0), (9_999, 3), SelectionKind::Stream);
        assert!(far.clip(&s));
        assert_eq!(far.range().1, Point::new(s.end_line() - 1, END));
        // A point on no line at all selects without panicking.
        let nowhere = Selection::start(&s, Point::new(9_999, 3), SelectionKind::Stream, 2);
        assert_eq!(nowhere.text(&s), "");
        let mut empty = Selection::start(
            &s,
            Point::new(s.screen_start(), 0),
            SelectionKind::Stream,
            1,
        );
        assert!(!empty.clip(&s));
    }
}
