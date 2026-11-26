//! The screen of a terminal: a grid of cells that the bytes of a program are written into, the
//! lines that scrolled off its top, and the state the program switched on along the way.
//!
//! [`Screen::feed`] is the only way in. It takes the bytes read from the program (a shell, an
//! editor, anything that believes it talks to an xterm), and afterwards the screen is what that
//! program would see: [`Screen::cell`] and [`Screen::line`] read the grid, [`Screen::cursor`]
//! and [`Screen::modes`] the state, [`Screen::take_dirty`] says which rows changed since it was
//! last asked, and [`Screen::take_reply`] holds what the terminal owes the program in return (a
//! cursor position report, its identity). Nothing here draws, reads a key or owns a process.
//!
//! What it understands is the common core of the VT100 to VT220 terminals and xterm: see
//! [`Screen::feed`] for the list, and the crate's documentation for what is left out.

use crate::parse::{Params, Parser, Perform};
use crate::width::char_width;
use std::collections::VecDeque;
use std::ops::Range;

mod protocol;

/// The most columns or rows a screen has; [`Screen::new`] and [`Screen::resize`] bring larger
/// numbers down to it, and zero up to one.
pub const MAX_SIDE: usize = 2048;

/// The most bytes of replies kept for [`Screen::take_reply`]. A program that asks without end
/// while nobody takes the answers gets no more of them past this.
const MAX_REPLY: usize = 4096;
/// The most characters of a window title.
const MAX_TITLE: usize = 1024;

/// A colour as the program named it. What the sixteen and the 256 look like, and what "default"
/// is, belongs to whoever draws.
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
    cells: Vec<Cell>,
    wrapped: bool,
}

impl Line {
    fn blank(cols: usize, fill: Cell) -> Line {
        Line {
            cells: vec![fill; cols],
            wrapped: false,
        }
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
    fn clean_edges(&mut self, a: usize, b: usize, blank: Cell) {
        if a > 0 && a < self.cells.len() && self.cells[a].is_spacer() {
            self.cells[a - 1] = blank;
        }
        if b < self.cells.len() && self.cells[b].is_spacer() {
            self.cells[b] = blank;
        }
    }

    fn erase(&mut self, a: usize, b: usize, blank: Cell) {
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

    fn resize(&mut self, cols: usize) {
        self.cells.resize(cols, Cell::BLANK);
        // A wide character cut in two by the new edge goes altogether.
        if let Some(last) = self.cells.last_mut()
            && last.width == 2
        {
            *last = Cell::BLANK;
        }
    }
}

/// The shape a program asked the cursor to have.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Bar,
}

/// Where the cursor is and how it looks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cursor {
    /// From 0 at the top.
    pub row: usize,
    /// From 0 at the left. Always a column of the screen: after a character in the last
    /// column the cursor stays on it until the next one wraps.
    pub col: usize,
    /// The program has not hidden it.
    pub visible: bool,
    pub shape: CursorShape,
    pub blinking: bool,
}

/// Which mouse events a program asked for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum MouseMode {
    /// None: the mouse is the terminal's own, for selecting.
    #[default]
    Off,
    /// Presses and releases (mode 1000).
    Press,
    /// Those, and movement while a button is held (mode 1002).
    Drag,
    /// Those, and all movement (mode 1003).
    Motion,
}

/// The switches a program set, which change how input is sent to it
/// ([`input`](crate::input)) and how the screen behaves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Modes {
    /// Arrow keys send `ESC O A` instead of `ESC [ A` (DECCKM, mode 1).
    pub app_cursor: bool,
    /// The keypad sends its application sequences (`ESC =` and `ESC >`, mode 66).
    pub app_keypad: bool,
    /// Text that reaches the right edge goes on in the next line (DECAWM, mode 7).
    pub autowrap: bool,
    /// The cursor is shown (DECTCEM, mode 25).
    pub cursor_visible: bool,
    /// Cursor addresses count from the scroll region's top (DECOM, mode 6).
    pub origin: bool,
    /// Characters push the rest of the line right instead of overwriting (IRM, mode 4).
    pub insert: bool,
    /// A line feed also returns the carriage, and Enter sends both (LNM, mode 20).
    pub newline: bool,
    /// The alternate screen is shown (modes 47, 1047, 1049).
    pub alt_screen: bool,
    /// A paste is sent between `ESC [ 200 ~` and `ESC [ 201 ~` (mode 2004).
    pub bracketed_paste: bool,
    pub mouse: MouseMode,
    /// Mouse reports use the SGR form, which has no limit on the coordinates (mode 1006).
    pub mouse_sgr: bool,
    /// The program wants to be told when the window gains and loses the focus (mode 1004).
    pub focus: bool,
}

impl Default for Modes {
    fn default() -> Modes {
        Modes {
            app_cursor: false,
            app_keypad: false,
            autowrap: true,
            cursor_visible: true,
            origin: false,
            insert: false,
            newline: false,
            alt_screen: false,
            bracketed_paste: false,
            mouse: MouseMode::Off,
            mouse_sgr: false,
            focus: false,
        }
    }
}

/// The colours and attributes the next character is written with.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Pen {
    fg: Color,
    bg: Color,
    attrs: Attrs,
}

/// What `ESC 7` keeps and `ESC 8` brings back.
#[derive(Clone, Copy, Debug)]
struct Saved {
    row: usize,
    col: usize,
    pending: bool,
    pen: Pen,
    origin: bool,
    line_drawing: [bool; 2],
    shift: usize,
}

/// The DEC special graphics set, for the bytes 0x60 to 0x7E: the line-drawing characters that
/// full-screen programs frame their windows with.
const LINE_DRAWING: [char; 31] = [
    '◆', '▒', '␉', '␌', '␍', '␊', '°', '±', '␤', '␋', '┘', '┐', '┌', '└', '┼', '⎺', '⎻', '─', '⎼',
    '⎽', '├', '┤', '┴', '┬', '│', '≤', '≥', 'π', '≠', '£', '·',
];

/// A terminal's screen. See the [module](self).
///
/// **Lines have numbers.** Every line that was ever on the screen has an absolute number that
/// does not change while it scrolls up the screen and into the scrollback: the oldest line still
/// kept is [`first_line`](Self::first_line), the top row of the screen is
/// [`screen_start`](Self::screen_start), and [`line_at`](Self::line_at) reads any of them. A
/// selection ([`crate::select`]) and a view scrolled back hold these numbers, so they stay on
/// their text while output arrives. The numbers follow the text only when the whole screen
/// scrolls; a program that scrolls a region of it, or redraws it, moves text under them.
#[derive(Clone, Debug)]
pub struct Screen {
    cols: usize,
    rows: usize,
    /// The grid that is shown: the main one, or the alternate one.
    grid: Vec<Line>,
    /// The main grid while the alternate one is shown.
    main: Option<Vec<Line>>,
    /// The cursor's row in the main grid when the alternate one was entered.
    main_row: usize,
    scrollback: VecDeque<Line>,
    limit: usize,
    /// How many lines have left the scrollback's far end, ever.
    dropped: u64,
    row: usize,
    col: usize,
    /// A character was written in the last column: the next one wraps first.
    pending: bool,
    pen: Pen,
    saved: [Option<Saved>; 2],
    /// The scroll region, both rows inside it.
    top: usize,
    bottom: usize,
    tabs: Vec<bool>,
    modes: Modes,
    /// Whether the character sets G0 and G1 are the line-drawing one, and which is in use.
    line_drawing: [bool; 2],
    shift: usize,
    shape: CursorShape,
    blinking: bool,
    cursor_style_is_explicit: bool,
    /// The last character written, for "repeat".
    last: Option<char>,
    title: String,
    title_changed: bool,
    bell: bool,
    reply: Vec<u8>,
    dirty: Vec<bool>,
    parser: Parser,
}

/// The parser's calls, passed on to the screen. A type of its own so that the screen's public
/// face is `feed` and not the five calls.
struct Sink<'a>(&'a mut Screen);

impl Perform for Sink<'_> {
    fn print(&mut self, c: char) {
        self.0.print(c);
    }
    fn execute(&mut self, byte: u8) {
        self.0.execute(byte);
    }
    fn csi(&mut self, params: &Params, private: u8, intermediates: &[u8], last: u8) {
        self.0.csi(params, private, intermediates, last);
    }
    fn esc(&mut self, intermediates: &[u8], last: u8) {
        self.0.esc(intermediates, last);
    }
    fn osc(&mut self, data: &[u8]) {
        self.0.osc(data);
    }
}

fn default_tabs(cols: usize) -> Vec<bool> {
    (0..cols).map(|c| c > 0 && c % 8 == 0).collect()
}

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

impl Screen {
    /// An empty screen of `cols` by `rows` cells that keeps up to `scrollback` lines that
    /// scrolled off its top (0 keeps none). The sides are brought into 1 to [`MAX_SIDE`].
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Screen {
        let (cols, rows) = (cols.clamp(1, MAX_SIDE), rows.clamp(1, MAX_SIDE));
        Screen {
            cols,
            rows,
            grid: vec![Line::blank(cols, Cell::BLANK); rows],
            main: None,
            main_row: 0,
            scrollback: VecDeque::new(),
            limit: scrollback,
            dropped: 0,
            row: 0,
            col: 0,
            pending: false,
            pen: Pen::default(),
            saved: [None; 2],
            top: 0,
            bottom: rows - 1,
            tabs: default_tabs(cols),
            modes: Modes::default(),
            line_drawing: [false; 2],
            shift: 0,
            shape: CursorShape::Block,
            blinking: true,
            cursor_style_is_explicit: false,
            last: None,
            title: String::new(),
            title_changed: false,
            bell: false,
            reply: Vec::new(),
            dirty: vec![true; rows],
            parser: Parser::new(),
        }
    }

    /// Writes the program's output to the screen. The bytes may be cut anywhere, also inside a
    /// character or an escape sequence: the rest is waited for. No bytes make it panic.
    ///
    /// Understood:
    ///
    /// - text as UTF-8, wide characters in two cells; a combining mark or other character of no
    ///   width is dropped (the cell keeps its base character alone);
    /// - backspace, tab, line feed (also as vertical tab and form feed), carriage return, bell,
    ///   shift in and out with the DEC line-drawing set (`ESC ( 0`);
    /// - cursor movement: CUU CUD CUF CUB CNL CPL CHA HPA VPA CUP HVP, HPR VPR, tabs forward
    ///   and back (CHT CBT), tab stops (HTS TBC), index, reverse index and next line
    ///   (`ESC D`, `ESC M`, `ESC E`), save and restore (`ESC 7`, `ESC 8`, `CSI s`, `CSI u`);
    /// - erasing and editing: ED (also 3, the scrollback) EL ECH ICH DCH IL DL SU SD REP, the
    ///   screen alignment test (`ESC # 8`); erased cells take the current background;
    /// - the scroll region (DECSTBM) and origin mode;
    /// - SGR: bold, dim, italic, underline, blink, inverse, hidden, strike and their ends, the
    ///   16 colours and their bright forms, `38`/`48` with `5;n` and `2;r;g;b`, in the
    ///   semicolon and the colon forms;
    /// - modes: insert (4) and new line (20); private 1 (cursor keys), 6 (origin), 7 (wrap),
    ///   25 (cursor shown), 47, 1047 and 1049 (alternate screen), 1048, 66, 1000, 1002, 1003,
    ///   1004, 1006, 2004;
    /// - the cursor's shape (DECSCUSR), soft reset (DECSTR) and full reset (RIS);
    /// - the window title (OSC 0 and 2);
    /// - queries, answered through [`take_reply`](Self::take_reply): device attributes (DA1
    ///   and DA2), status and cursor position (DSR 5 and 6), a private mode's state (DECRQM).
    ///
    /// Everything else is read to its end and ignored.
    pub fn feed(&mut self, bytes: &[u8]) {
        let before = (self.row, self.col, self.modes.cursor_visible);
        let mut parser = std::mem::take(&mut self.parser);
        parser.feed(bytes, &mut Sink(self));
        self.parser = parser;
        // The cursor is drawn on its row: a row it left or reached has changed to the eye.
        if before != (self.row, self.col, self.modes.cursor_visible) {
            if let Some(d) = self.dirty.get_mut(before.0) {
                *d = true;
            }
            self.dirty[self.row] = true;
        }
    }

    // ---- reading ------------------------------------------------------------------------------

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// The cell at `row` and `col` of the screen; a blank one outside it.
    pub fn cell(&self, row: usize, col: usize) -> Cell {
        self.grid.get(row).map_or(Cell::BLANK, |l| l.cell(col))
    }

    /// Row `row` of the screen, from 0 at the top.
    pub fn line(&self, row: usize) -> Option<&Line> {
        self.grid.get(row)
    }

    /// The text of row `row`, without its trailing spaces; empty outside the screen.
    pub fn row_text(&self, row: usize) -> String {
        self.grid.get(row).map(Line::text).unwrap_or_default()
    }

    /// How many lines the scrollback holds.
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Line `i` of the scrollback, 0 being the oldest.
    pub fn scrollback_line(&self, i: usize) -> Option<&Line> {
        self.scrollback.get(i)
    }

    /// Forgets the scrollback. The lines on the screen keep their numbers.
    pub fn clear_scrollback(&mut self) {
        self.dropped += self.scrollback.len() as u64;
        self.scrollback.clear();
    }

    /// The absolute number of the oldest line still kept.
    pub fn first_line(&self) -> u64 {
        self.dropped
    }

    /// The absolute number of the screen's top row.
    pub fn screen_start(&self) -> u64 {
        self.dropped + self.scrollback.len() as u64
    }

    /// One past the absolute number of the screen's bottom row.
    pub fn end_line(&self) -> u64 {
        self.screen_start() + self.rows as u64
    }

    /// The line with the absolute number `line`, in the scrollback or on the screen; `None`
    /// for one that has been dropped or does not exist yet.
    pub fn line_at(&self, line: u64) -> Option<&Line> {
        let i = usize::try_from(line.checked_sub(self.dropped)?).ok()?;
        if i < self.scrollback.len() {
            self.scrollback.get(i)
        } else {
            self.grid.get(i - self.scrollback.len())
        }
    }

    pub fn cursor(&self) -> Cursor {
        Cursor {
            row: self.row,
            col: self.col,
            visible: self.modes.cursor_visible,
            shape: self.shape,
            blinking: self.blinking,
        }
    }

    /// Whether the program has chosen a cursor shape and blink with a nonzero DECSCUSR
    /// parameter. Zero (including an omitted parameter) and a full terminal reset return to
    /// the default. [`Self::cursor`] still reports the built-in blinking block at that default.
    pub fn cursor_style_is_explicit(&self) -> bool {
        self.cursor_style_is_explicit
    }

    pub fn modes(&self) -> Modes {
        self.modes
    }

    /// The window title the program set, or empty.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The title, if the program set it since this was last asked.
    pub fn take_title(&mut self) -> Option<String> {
        std::mem::take(&mut self.title_changed).then(|| self.title.clone())
    }

    /// Whether the program rang the bell since this was last asked.
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }

    /// The bytes the terminal owes the program, to be written to it: the answers to its
    /// queries, in the order they were asked. Empty when there are none.
    pub fn take_reply(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.reply)
    }

    /// The rows that changed since this was last asked, as ranges of rows in rising order, and
    /// marks them clean. A row the cursor left or entered counts as changed. A new screen, a
    /// resize and a switch between the main and the alternate screen change every row.
    pub fn take_dirty(&mut self) -> Vec<Range<usize>> {
        let mut out: Vec<Range<usize>> = Vec::new();
        for (r, d) in self.dirty.iter_mut().enumerate() {
            if std::mem::take(d) {
                match out.last_mut() {
                    Some(last) if last.end == r => last.end = r + 1,
                    _ => out.push(r..r + 1),
                }
            }
        }
        out
    }

    /// Marks every row changed: for a caller whose own picture of the screen is lost.
    pub fn mark_all_dirty(&mut self) {
        self.dirty.fill(true);
    }

    // ---- size ---------------------------------------------------------------------------------

    /// Changes the screen's size. Lines are not reflowed: a line longer than the new width is
    /// cut at it (the cells are gone, also if the screen widens again), a shorter one is padded.
    /// When rows go, blank and other lines below the cursor go first and then lines from the
    /// top, which join the scrollback on the main screen, so the cursor's line is always kept;
    /// when rows come, they come empty at the bottom (nothing is pulled back out of the
    /// scrollback). The scroll region becomes the whole screen and every row is marked changed.
    /// The scrollback's lines keep the width they were written with.
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
        let old = self.tabs.len();
        self.tabs.resize(cols, false);
        for (c, stop) in self.tabs.iter_mut().enumerate().skip(old) {
            *stop = c % 8 == 0;
        }
        (self.cols, self.rows) = (cols, rows);
        (self.top, self.bottom) = (0, rows - 1);
        self.row = self.row.min(rows - 1);
        self.col = self.col.min(cols - 1);
        self.pending = false;
        self.dirty = vec![true; rows];
    }

    // ---- writing ------------------------------------------------------------------------------

    /// A cell that was erased: blank, in the background the program has set.
    fn blank(&self) -> Cell {
        Cell {
            bg: self.pen.bg,
            ..Cell::BLANK
        }
    }

    fn push_scrollback(&mut self, mut line: Line) {
        if self.limit == 0 {
            self.dropped += 1;
            return;
        }
        if self.scrollback.len() >= self.limit {
            self.scrollback.pop_front();
            self.dropped += 1;
        }
        // Trailing blanks are not worth keeping, except in a line that goes on in the next:
        // there they are spaces of the text.
        while !line.wrapped && line.cells.last() == Some(&Cell::BLANK) {
            line.cells.pop();
        }
        line.cells.shrink_to_fit();
        self.scrollback.push_back(line);
    }

    /// Moves the lines of `top..=bottom` up by `n`; blank lines come in at the bottom. With
    /// `keep`, lines that leave the very top of the main screen join the scrollback.
    fn scroll_up(&mut self, top: usize, bottom: usize, n: usize, keep: bool) {
        if top > bottom || bottom >= self.rows {
            return;
        }
        let blank = self.blank();
        for _ in 0..n.min(bottom + 1 - top) {
            let line = self.grid.remove(top);
            if keep && top == 0 && !self.modes.alt_screen {
                self.push_scrollback(line);
            }
            self.grid.insert(bottom, Line::blank(self.cols, blank));
        }
        self.dirty[top..=bottom].fill(true);
    }

    /// Moves the lines of `top..=bottom` down by `n`; blank lines come in at the top.
    fn scroll_down(&mut self, top: usize, bottom: usize, n: usize) {
        if top > bottom || bottom >= self.rows {
            return;
        }
        let blank = self.blank();
        for _ in 0..n.min(bottom + 1 - top) {
            self.grid.remove(bottom);
            self.grid.insert(top, Line::blank(self.cols, blank));
        }
        self.dirty[top..=bottom].fill(true);
    }

    /// Down one line, scrolling the region when the cursor is on its last row.
    fn index(&mut self) {
        if self.row == self.bottom {
            self.scroll_up(self.top, self.bottom, 1, true);
        } else if self.row + 1 < self.rows {
            self.row += 1;
        }
    }

    fn reverse_index(&mut self) {
        if self.row == self.top {
            self.scroll_down(self.top, self.bottom, 1);
        } else if self.row > 0 {
            self.row -= 1;
        }
    }

    fn goto(&mut self, row: usize, col: usize) {
        self.row = row.min(self.rows - 1);
        self.col = col.min(self.cols - 1);
        self.pending = false;
    }

    /// To the row a program calls `row` (from 1), which in origin mode counts from the region's
    /// top and cannot leave the region.
    fn goto_addressed(&mut self, row: usize, col: usize) {
        let row = row.saturating_sub(1);
        if self.modes.origin {
            self.goto((self.top + row).min(self.bottom), col);
        } else {
            self.goto(row, col);
        }
    }

    fn up(&mut self, n: usize) {
        let stop = if self.row >= self.top { self.top } else { 0 };
        self.row = self.row.saturating_sub(n).max(stop);
        self.pending = false;
    }

    fn down(&mut self, n: usize) {
        let stop = if self.row <= self.bottom {
            self.bottom
        } else {
            self.rows - 1
        };
        self.row = self.row.saturating_add(n).min(stop);
        self.pending = false;
    }

    fn tab(&mut self) {
        let mut c = self.col + 1;
        while c < self.cols && !self.tabs[c] {
            c += 1;
        }
        self.col = c.min(self.cols - 1);
    }

    fn back_tab(&mut self) {
        let mut c = self.col;
        while c > 0 {
            c -= 1;
            if self.tabs[c] {
                break;
            }
        }
        self.col = c;
        self.pending = false;
    }

    fn erase(&mut self, row: usize, a: usize, b: usize) {
        let blank = self.blank();
        self.grid[row].erase(a, b, blank);
        self.dirty[row] = true;
    }

    /// Opens `n` blank cells at the cursor; the rest of the line moves right and what passes
    /// the edge is lost.
    fn insert_blanks(&mut self, n: usize) {
        let (row, col, cols, blank) = (self.row, self.col, self.cols, self.blank());
        let n = n.min(cols - col);
        let line = &mut self.grid[row];
        line.clean_edges(col, col, blank);
        line.cells[col..].rotate_right(n);
        line.cells[col..col + n].fill(blank);
        // A wide character whose second cell was pushed off the edge goes with it.
        if line.cells[cols - 1].width == 2 {
            line.cells[cols - 1] = blank;
        }
        self.dirty[row] = true;
    }

    fn wrap(&mut self) {
        self.grid[self.row].wrapped = true;
        self.col = 0;
        self.pending = false;
        self.index();
    }

    fn print(&mut self, c: char) {
        let c = if self.line_drawing[self.shift] && ('\x60'..='\x7e').contains(&c) {
            LINE_DRAWING[c as usize - 0x60]
        } else {
            c
        };
        let mut w = char_width(c) as usize;
        if w == 0 {
            return;
        }
        // A screen one column wide has no room for a wide character; it is kept in one cell.
        if w == 2 && self.cols < 2 {
            w = 1;
        }
        self.last = Some(c);
        if self.pending && self.modes.autowrap {
            self.wrap();
        }
        if w == 2 && self.col == self.cols - 1 {
            if self.modes.autowrap {
                // It does not fit in the last column: that one stays empty and it goes below.
                self.erase(self.row, self.col, self.cols);
                self.wrap();
            } else {
                self.col = self.cols - 2;
            }
        }
        self.pending = false;
        let (row, col, cols, blank) = (self.row, self.col, self.cols, self.blank());
        let cell = Cell {
            ch: c,
            fg: self.pen.fg,
            bg: self.pen.bg,
            attrs: self.pen.attrs,
            width: w as u8,
        };
        if self.modes.insert {
            self.insert_blanks(w);
        }
        let line = &mut self.grid[row];
        line.clean_edges(col, col + w, blank);
        line.cells[col] = cell;
        if w == 2 {
            line.cells[col + 1] = Cell {
                ch: ' ',
                width: 0,
                ..cell
            };
        }
        self.dirty[row] = true;
        if col + w >= cols {
            self.col = cols - 1;
            self.pending = self.modes.autowrap;
        } else {
            self.col = col + w;
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x07 => self.bell = true,
            0x08 => {
                self.col = self.col.saturating_sub(1);
                self.pending = false;
            }
            0x09 => self.tab(),
            0x0A..=0x0C => {
                self.pending = false;
                self.index();
                if self.modes.newline {
                    self.col = 0;
                }
            }
            0x0D => {
                self.col = 0;
                self.pending = false;
            }
            0x0E => self.shift = 1,
            0x0F => self.shift = 0,
            _ => {}
        }
    }

    fn save_cursor(&mut self) {
        self.saved[usize::from(self.modes.alt_screen)] = Some(Saved {
            row: self.row,
            col: self.col,
            pending: self.pending,
            pen: self.pen,
            origin: self.modes.origin,
            line_drawing: self.line_drawing,
            shift: self.shift,
        });
    }

    fn restore_cursor(&mut self) {
        match self.saved[usize::from(self.modes.alt_screen)] {
            Some(s) => {
                self.goto(s.row, s.col);
                self.pending = s.pending && self.col == self.cols - 1;
                self.pen = s.pen;
                self.modes.origin = s.origin;
                self.line_drawing = s.line_drawing;
                self.shift = s.shift;
            }
            // Nothing was saved: the state a terminal starts with.
            None => {
                self.goto(0, 0);
                self.pen = Pen::default();
                self.modes.origin = false;
                self.line_drawing = [false; 2];
                self.shift = 0;
            }
        }
    }

    /// Shows the alternate screen, empty, or goes back to the main one as it was left.
    fn alt_screen(&mut self, on: bool) {
        if on == self.modes.alt_screen {
            return;
        }
        if on {
            let fresh = vec![Line::blank(self.cols, Cell::BLANK); self.rows];
            self.main = Some(std::mem::replace(&mut self.grid, fresh));
            self.main_row = self.row;
        } else if let Some(main) = self.main.take() {
            self.grid = main;
        }
        self.modes.alt_screen = on;
        self.pending = false;
        self.dirty.fill(true);
    }
}
