//! The cells a terminal's grid is made of, and the lines they are held in.
//!
//! [`Cell`] is one character with its colours and attributes; [`Line`] is a row of them, of the
//! grid or of the scrollback, with the two facts the rest of the crate needs about it: whether
//! it goes on in the next line (a soft wrap, see [`Line::wrapped`]) and whether its blank cells
//! at the end are padding. Everything here is plain data and arithmetic; nothing
//! draws, reads a clock or knows about a [`Screen`](crate::screen::Screen).
//!
//! The types are re-exported from [`crate::screen`], so the path of every one of them is what
//! it always was.

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Color {
    /// The terminal's own foreground or background.
    #[default]
    Default,
    /// One of the 256: 0 to 7 the standard colours, 8 to 15 their bright forms, 16 to 231 a
    /// 6 x 6 x 6 cube, 232 to 255 greys.
    Indexed(u8),
    /// Red, green, blue.
    Rgb(u8, u8, u8),
}

/// The attributes of a cell, as a set of flags.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Attrs(u8);

impl Attrs {
    pub const NONE: Attrs = Attrs(0);
    pub const BOLD: Attrs = Attrs(1);
    pub const DIM: Attrs = Attrs(2);
    pub const ITALIC: Attrs = Attrs(4);
    pub const UNDERLINE: Attrs = Attrs(8);
    pub const BLINK: Attrs = Attrs(16);
    /// Foreground and background change places when drawn.
    pub const INVERSE: Attrs = Attrs(32);
    /// The character is not drawn (it is still there, and is copied).
    pub const HIDDEN: Attrs = Attrs(64);
    pub const STRIKE: Attrs = Attrs(128);

    /// Whether every flag of `other` is set.
    pub fn contains(self, other: Attrs) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn with(self, other: Attrs) -> Attrs {
        Attrs(self.0 | other.0)
    }

    pub fn without(self, other: Attrs) -> Attrs {
        Attrs(self.0 & !other.0)
    }

    /// The flags as bits, in the order of the constants.
    pub fn bits(self) -> u8 {
        self.0
    }
}

/// One cell of the grid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    /// The character; a space where nothing was written.
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub attrs: Attrs,
    /// The cells the character takes: 1, or 2 for a wide character, whose second cell follows
    /// with a width of 0 (and a space, and the same colours) so that it is never drawn or
    /// copied on its own.
    pub width: u8,
}

impl Default for Cell {
    fn default() -> Cell {
        Cell::BLANK
    }
}

impl Cell {
    /// A cell nothing was written to, in the default colours.
    pub const BLANK: Cell = Cell {
        ch: ' ',
        fg: Color::Default,
        bg: Color::Default,
        attrs: Attrs::NONE,
        width: 1,
    };

    /// The second cell of a wide character.
    pub fn is_spacer(&self) -> bool {
        self.width == 0
    }
}

/// One line of the grid or of the scrollback.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Line {
    pub(crate) cells: Vec<Cell>,
    pub(crate) wrapped: bool,
    /// The last cell is blank because a wide character did not fit in it and was moved to the
    /// line below, not because a space was written there. A wrapped line's blank cells are
    /// otherwise part of the text, which is why this is remembered: joining such a line into
    /// another one drops this one cell and no other.
    padded: bool,
}

impl Line {
    pub(crate) fn blank(cols: usize, fill: Cell) -> Line {
        Line {
            cells: vec![fill; cols],
            wrapped: false,
            padded: false,
        }
    }

    /// A line of the cells given, going on in the next one when `wrapped`. The caller keeps the
    /// grid's habit: a wide character and its spacer cell travel together, and a line of the
    /// grid is as wide as the screen.
    pub(crate) fn from_cells(cells: Vec<Cell>, wrapped: bool) -> Line {
        Line {
            cells,
            wrapped,
            padded: false,
        }
    }

    /// Whether the last cell is blank only because a wide character was moved down a line. Such
    /// a cell is not a space of the text, and a caller joining the lines drops it.
    pub(crate) fn is_padded(&self) -> bool {
        self.padded
    }

    /// Says that the last cell is such a blank, or that it is not one any more.
    pub(crate) fn set_padded(&mut self, padded: bool) {
        self.padded = padded;
    }

    /// Takes the cells out and leaves the line empty: for a caller that joins this line into
    /// another one and has no use for it afterwards. The `wrapped` flag stays.
    pub(crate) fn take_cells(&mut self) -> Vec<Cell> {
        std::mem::take(&mut self.cells)
    }

    /// The cells the line holds. A line of the grid holds one per column; a line of the
    /// scrollback has lost its trailing blank cells, and kept the width of the screen it was
    /// written on. [`cell`](Self::cell) reads either without caring.
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The cell in column `col`; a blank one past the end of what the line holds.
    pub fn cell(&self, col: usize) -> Cell {
        self.cells.get(col).copied().unwrap_or(Cell::BLANK)
    }

    /// Whether the line ended because the text reached the right edge and went on in the next
    /// line, not because the program ended it: the two are one line of text to whoever copies
    /// them.
    pub fn wrapped(&self) -> bool {
        self.wrapped
    }

    /// The line's characters, without the trailing spaces.
    pub fn text(&self) -> String {
        let mut s: String = self
            .cells
            .iter()
            .filter(|c| !c.is_spacer())
            .map(|c| c.ch)
            .collect();
        s.truncate(s.trim_end_matches(' ').len());
        s
    }

    /// Before the cells `a..b` are overwritten: a wide character that lies half inside them
    /// loses its other half too, so no half is left standing.
    pub(crate) fn clean_edges(&mut self, a: usize, b: usize, blank: Cell) {
        if a > 0 && a < self.cells.len() && self.cells[a].is_spacer() {
            self.cells[a - 1] = blank;
        }
        if b < self.cells.len() && self.cells[b].is_spacer() {
            self.cells[b] = blank;
        }
    }

    pub(crate) fn erase(&mut self, a: usize, b: usize, blank: Cell) {
        let b = b.min(self.cells.len());
        if a >= b {
            return;
        }
        self.clean_edges(a, b, blank);
        self.cells[a..b].fill(blank);
        if b == self.cells.len() {
            self.wrapped = false;
        }
    }

    pub(crate) fn resize(&mut self, cols: usize) {
        self.cells.resize(cols, Cell::BLANK);
        // A wide character cut in two by the new edge goes altogether, and the cell it leaves
        // is a blank that is not a space of the text.
        if let Some(last) = self.cells.last_mut()
            && last.width == 2
        {
            *last = Cell::BLANK;
            self.padded = true;
        }
    }
}
