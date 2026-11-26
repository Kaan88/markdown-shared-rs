//! What the parser's escape sequences mean: the `impl Screen` that turns a control sequence,
//! an escape sequence or an operating system command into cells and state.
//!
//! Split from the screen itself only to keep the files readable; every method here is a private
//! call from `Screen::feed`, which owns the parser and hands the calls on.

use super::*;

impl Screen {
    pub(super) fn csi(&mut self, p: &Params, private: u8, inter: &[u8], last: u8) {
        let n = p.get_or(0, 1) as usize;
        match (private, inter, last) {
            (0, [], b'@') => {
                self.insert_blanks(n);
                self.pending = false;
            }
            (0, [], b'A') => self.up(n),
            (0, [], b'B' | b'e') => self.down(n),
            (0, [], b'C' | b'a') => self.goto(self.row, self.col.saturating_add(n)),
            (0, [], b'D') => self.goto(self.row, self.col.saturating_sub(n)),
            (0, [], b'E') => {
                self.down(n);
                self.col = 0;
            }
            (0, [], b'F') => {
                self.up(n);
                self.col = 0;
            }
            (0, [], b'G' | b'`') => self.goto(self.row, n - 1),
            (0, [], b'H' | b'f') => {
                self.goto_addressed(n, p.get_or(1, 1) as usize - 1);
            }
            (0, [], b'I') => (0..n.min(self.cols)).for_each(|_| self.tab()),
            (0, [], b'J') => self.erase_display(p.get(0)),
            (0, [], b'K') => match p.get(0) {
                0 => self.erase(self.row, self.col, self.cols),
                1 => self.erase(self.row, 0, self.col + 1),
                2 => self.erase(self.row, 0, self.cols),
                _ => {}
            },
            (0, [], b'L') => {
                if (self.top..=self.bottom).contains(&self.row) {
                    self.scroll_down(self.row, self.bottom, n);
                    self.col = 0;
                    self.pending = false;
                }
            }
            (0, [], b'M') => {
                if (self.top..=self.bottom).contains(&self.row) {
                    self.scroll_up(self.row, self.bottom, n, false);
                    self.col = 0;
                    self.pending = false;
                }
            }
            (0, [], b'P') => {
                let (row, col, blank) = (self.row, self.col, self.blank());
                let n = n.min(self.cols - col);
                let line = &mut self.grid[row];
                line.clean_edges(col, col + n, blank);
                line.cells[col..].rotate_left(n);
                let cols = line.cells.len();
                line.cells[cols - n..].fill(blank);
                line.wrapped = false;
                self.dirty[row] = true;
                self.pending = false;
            }
            (0, [], b'S') => self.scroll_up(self.top, self.bottom, n, true),
            (0, [], b'T') => self.scroll_down(self.top, self.bottom, n),
            (0, [], b'X') => {
                let end = self.col.saturating_add(n).min(self.cols);
                // Erasing inside a line does not end a wrapped line; only its tail does.
                let wrapped = self.grid[self.row].wrapped;
                self.erase(self.row, self.col, end);
                self.grid[self.row].wrapped = wrapped && end < self.cols;
            }
            (0, [], b'Z') => (0..n.min(self.cols)).for_each(|_| self.back_tab()),
            (0, [], b'b') => {
                if let Some(c) = self.last {
                    (0..n).for_each(|_| self.print(c));
                }
            }
            (0, [], b'c') if p.get(0) == 0 => self.reply(b"\x1b[?1;2c"),
            (b'>', [], b'c') if p.get(0) == 0 => self.reply(b"\x1b[>0;0;0c"),
            (0, [], b'd') => self.goto_addressed(n, self.col),
            (0, [], b'g') => match p.get(0) {
                0 => self.tabs[self.col] = false,
                3 => self.tabs.fill(false),
                _ => {}
            },
            (0, [], b'h' | b'l') => {
                for &mode in p.values() {
                    match mode {
                        4 => self.modes.insert = last == b'h',
                        20 => self.modes.newline = last == b'h',
                        _ => {}
                    }
                }
            }
            (b'?', [], b'h' | b'l') => {
                for &mode in p.values() {
                    self.private_mode(mode, last == b'h');
                }
            }
            (0, [], b'm') => self.sgr(p),
            (0, [], b'n') => match p.get(0) {
                5 => self.reply(b"\x1b[0n"),
                6 => {
                    let row = if self.modes.origin {
                        self.row.saturating_sub(self.top)
                    } else {
                        self.row
                    };
                    let report = format!("\x1b[{};{}R", row + 1, self.col + 1);
                    self.reply(report.as_bytes());
                }
                _ => {}
            },
            (b'?', [b'$'], b'p') => {
                let mode = p.get(0);
                let state = match self.private_mode_state(mode) {
                    Some(true) => 1,
                    Some(false) => 2,
                    None => 0,
                };
                let report = format!("\x1b[?{mode};{state}$y");
                self.reply(report.as_bytes());
            }
            (0, [b'!'], b'p') => self.soft_reset(),
            (0, [b' '], b'q') => {
                let style = p.get(0);
                self.shape = match style {
                    3 | 4 => CursorShape::Underline,
                    5 | 6 => CursorShape::Bar,
                    _ => CursorShape::Block,
                };
                // Odd numbers blink, and so does 0, the default.
                self.blinking = style == 0 || style % 2 == 1;
                self.dirty[self.row] = true;
            }
            (0, [], b'r') => {
                let top = n - 1;
                let bottom = match p.get(1) as usize {
                    0 => self.rows - 1,
                    b => (b - 1).min(self.rows - 1),
                };
                if top < bottom {
                    (self.top, self.bottom) = (top, bottom);
                    self.goto_addressed(1, 0);
                }
            }
            (0, [], b's') => self.save_cursor(),
            (0, [], b'u') => self.restore_cursor(),
            _ => {}
        }
    }

    pub(super) fn erase_display(&mut self, mode: u16) {
        match mode {
            0 => {
                self.erase(self.row, self.col, self.cols);
                for r in self.row + 1..self.rows {
                    self.erase(r, 0, self.cols);
                }
            }
            1 => {
                for r in 0..self.row {
                    self.erase(r, 0, self.cols);
                }
                self.erase(self.row, 0, self.col + 1);
            }
            2 => {
                for r in 0..self.rows {
                    self.erase(r, 0, self.cols);
                }
            }
            3 => self.clear_scrollback(),
            _ => {}
        }
    }

    pub(super) fn private_mode(&mut self, mode: u16, on: bool) {
        match mode {
            1 => self.modes.app_cursor = on,
            6 => {
                self.modes.origin = on;
                self.goto_addressed(1, 0);
            }
            7 => {
                self.modes.autowrap = on;
                self.pending &= on;
            }
            25 => self.modes.cursor_visible = on,
            47 | 1047 => self.alt_screen(on),
            66 => self.modes.app_keypad = on,
            1000 => self.modes.mouse = if on { MouseMode::Press } else { MouseMode::Off },
            1002 => self.modes.mouse = if on { MouseMode::Drag } else { MouseMode::Off },
            1003 => {
                self.modes.mouse = if on {
                    MouseMode::Motion
                } else {
                    MouseMode::Off
                }
            }
            1004 => self.modes.focus = on,
            1006 => self.modes.mouse_sgr = on,
            1048 => {
                if on {
                    self.save_cursor();
                } else {
                    self.restore_cursor();
                }
            }
            1049 => {
                if on {
                    self.save_cursor();
                    self.alt_screen(true);
                } else {
                    self.alt_screen(false);
                    self.restore_cursor();
                }
            }
            2004 => self.modes.bracketed_paste = on,
            _ => {}
        }
    }

    /// Whether a private mode is set, or `None` for one this screen does not know.
    pub(super) fn private_mode_state(&self, mode: u16) -> Option<bool> {
        let m = &self.modes;
        Some(match mode {
            1 => m.app_cursor,
            6 => m.origin,
            7 => m.autowrap,
            25 => m.cursor_visible,
            47 | 1047 | 1049 => m.alt_screen,
            66 => m.app_keypad,
            1000 => m.mouse == MouseMode::Press,
            1002 => m.mouse == MouseMode::Drag,
            1003 => m.mouse == MouseMode::Motion,
            1004 => m.focus,
            1006 => m.mouse_sgr,
            2004 => m.bracketed_paste,
            _ => return None,
        })
    }

    pub(super) fn sgr(&mut self, p: &Params) {
        if p.is_empty() {
            self.pen = Pen::default();
            return;
        }
        let byte = |v: u16| v.min(255) as u8;
        let mut i = 0;
        while i < p.len() {
            let g = p.group_at(i);
            i += g.len();
            let a = &mut self.pen.attrs;
            match g[0] {
                0 => self.pen = Pen::default(),
                1 => *a = a.with(Attrs::BOLD),
                2 => *a = a.with(Attrs::DIM),
                3 => *a = a.with(Attrs::ITALIC),
                // `4:0` is "no underline"; the other styles of underline are one here.
                4 if g.get(1) == Some(&0) => *a = a.without(Attrs::UNDERLINE),
                4 | 21 => *a = a.with(Attrs::UNDERLINE),
                5 | 6 => *a = a.with(Attrs::BLINK),
                7 => *a = a.with(Attrs::INVERSE),
                8 => *a = a.with(Attrs::HIDDEN),
                9 => *a = a.with(Attrs::STRIKE),
                22 => *a = a.without(Attrs::BOLD).without(Attrs::DIM),
                23 => *a = a.without(Attrs::ITALIC),
                24 => *a = a.without(Attrs::UNDERLINE),
                25 => *a = a.without(Attrs::BLINK),
                27 => *a = a.without(Attrs::INVERSE),
                28 => *a = a.without(Attrs::HIDDEN),
                29 => *a = a.without(Attrs::STRIKE),
                v @ 30..=37 => self.pen.fg = Color::Indexed(v as u8 - 30),
                39 => self.pen.fg = Color::Default,
                v @ 40..=47 => self.pen.bg = Color::Indexed(v as u8 - 40),
                49 => self.pen.bg = Color::Default,
                v @ 90..=97 => self.pen.fg = Color::Indexed(v as u8 - 90 + 8),
                v @ 100..=107 => self.pen.bg = Color::Indexed(v as u8 - 100 + 8),
                which @ (38 | 48) => {
                    let colour = if g.len() > 1 {
                        // The colon form: everything is in this one parameter. With six
                        // values the one after the 2 names a colour space and is skipped.
                        match g[1] {
                            5 if g.len() >= 3 => Some(Color::Indexed(byte(g[2]))),
                            2 if g.len() >= 6 => {
                                Some(Color::Rgb(byte(g[3]), byte(g[4]), byte(g[5])))
                            }
                            2 if g.len() >= 5 => {
                                Some(Color::Rgb(byte(g[2]), byte(g[3]), byte(g[4])))
                            }
                            _ => None,
                        }
                    } else {
                        // The semicolon form: the parameters that follow belong to it.
                        match p.get(i) {
                            5 => {
                                i += 2;
                                Some(Color::Indexed(byte(p.get(i - 1))))
                            }
                            2 => {
                                i += 4;
                                Some(Color::Rgb(
                                    byte(p.get(i - 3)),
                                    byte(p.get(i - 2)),
                                    byte(p.get(i - 1)),
                                ))
                            }
                            _ => {
                                i += 1;
                                None
                            }
                        }
                    };
                    if let Some(colour) = colour {
                        if which == 38 {
                            self.pen.fg = colour;
                        } else {
                            self.pen.bg = colour;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn esc(&mut self, inter: &[u8], last: u8) {
        match (inter, last) {
            ([], b'7') => self.save_cursor(),
            ([], b'8') => self.restore_cursor(),
            ([], b'D') => {
                self.pending = false;
                self.index();
            }
            ([], b'E') => {
                self.pending = false;
                self.col = 0;
                self.index();
            }
            ([], b'H') => self.tabs[self.col] = true,
            ([], b'M') => {
                self.pending = false;
                self.reverse_index();
            }
            ([], b'c') => self.reset(),
            ([], b'=') => self.modes.app_keypad = true,
            ([], b'>') => self.modes.app_keypad = false,
            ([b'('], set) => self.line_drawing[0] = set == b'0',
            ([b')'], set) => self.line_drawing[1] = set == b'0',
            ([b'#'], b'8') => {
                let fill = Cell {
                    ch: 'E',
                    ..Cell::BLANK
                };
                for line in &mut self.grid {
                    *line = Line::blank(self.cols, fill);
                }
                (self.top, self.bottom) = (0, self.rows - 1);
                self.goto(0, 0);
                self.dirty.fill(true);
            }
            _ => {}
        }
    }

    pub(super) fn osc(&mut self, data: &[u8]) {
        let Some(at) = data.iter().position(|&b| b == b';') else {
            return;
        };
        if matches!(&data[..at], b"0" | b"2") {
            let title: String = String::from_utf8_lossy(&data[at + 1..])
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_TITLE)
                .collect();
            self.title = title;
            self.title_changed = true;
        }
    }
}
