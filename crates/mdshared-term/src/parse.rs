//! The byte-stream parser: what a program writes to a terminal, cut into printable characters,
//! control bytes and the escape sequences between them. It knows the *shape* of the sequences
//! and nothing of what they mean; the meaning is the [`Screen`](crate::Screen)'s.
//!
//! The states are those of Paul Williams' description of the DEC terminals' parser, reduced:
//! ground, escape, a control sequence (`ESC [`), an operating-system command (`ESC ]`), and the
//! other strings (`ESC P`, `ESC X`, `ESC ^`, `ESC _`), which are swallowed whole. Text is UTF-8,
//! decoded here, so a character cut in two by the end of a read is put together when the next
//! read comes; a byte that is not UTF-8 is the replacement character U+FFFD.
//!
//! Whatever the bytes are, the parser does not panic and does not grow: a control sequence has
//! at most [`MAX_PARAMS`] parameters and two intermediate bytes (more makes it ignored, as the
//! terminals do), and an operating-system command keeps at most [`MAX_OSC`] bytes.

/// The most parameters of one control sequence; further ones are dropped.
pub const MAX_PARAMS: usize = 32;
/// The most bytes of one operating-system command; further ones are dropped.
pub const MAX_OSC: usize = 4096;

/// What the parser found. The caller implements this; the [`Screen`](crate::Screen) is the
/// implementation that matters, and the tests have one that writes down what it is told.
pub trait Perform {
    /// A printable character.
    fn print(&mut self, c: char);
    /// A C0 control byte (below 0x20) that is acted on at once: line feed, carriage return,
    /// backspace, tab, bell, shift in and out.
    fn execute(&mut self, byte: u8);
    /// A control sequence, `ESC [ … final`. `private` is the marker after the bracket (`?`, `>`,
    /// `<` or `=`) or 0; `intermediates` are the bytes 0x20 to 0x2F before the final byte.
    fn csi(&mut self, params: &Params, private: u8, intermediates: &[u8], last: u8);
    /// An escape sequence that is not one of the above: `ESC intermediates… final`.
    fn esc(&mut self, intermediates: &[u8], last: u8);
    /// An operating-system command, without its `ESC ]` and its terminator.
    fn osc(&mut self, data: &[u8]);
}

/// The numeric parameters of a control sequence. A parameter left out is 0, which every sequence
/// reads as its default. Parameters are separated by `;`; ones joined by `:` are sub-parameters
/// of the one before them (`38:2::255:0:0`), and [`group_at`](Self::group_at) gives a parameter
/// with its sub-parameters as one slice.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    vals: [u16; MAX_PARAMS],
    /// Whether the value was joined to the one before it by a colon.
    colon: [bool; MAX_PARAMS],
    len: usize,
    /// More were given than fit; the digits that follow belong to none.
    full: bool,
}

impl Default for Params {
    fn default() -> Params {
        Params {
            vals: [0; MAX_PARAMS],
            colon: [false; MAX_PARAMS],
            len: 0,
            full: false,
        }
    }
}

impl Params {
    /// How many values there are, sub-parameters counted. `CSI m` has none; `CSI ;5m` has two.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Value `i`, or 0 when there is none.
    pub fn get(&self, i: usize) -> u16 {
        if i < self.len { self.vals[i] } else { 0 }
    }

    /// Value `i`, or `default` when it is absent or 0: how most sequences read a count.
    pub fn get_or(&self, i: usize, default: u16) -> u16 {
        match self.get(i) {
            0 => default,
            v => v,
        }
    }

    /// The parameter that starts at value `i` together with its colon-joined sub-parameters.
    /// Empty past the end. The next parameter starts at `i` plus the slice's length.
    pub fn group_at(&self, i: usize) -> &[u16] {
        if i >= self.len {
            return &[];
        }
        let mut end = i + 1;
        while end < self.len && self.colon[end] {
            end += 1;
        }
        &self.vals[i..end]
    }

    /// All values in order, sub-parameters included.
    pub fn values(&self) -> &[u16] {
        &self.vals[..self.len]
    }

    fn clear(&mut self) {
        self.len = 0;
        self.full = false;
    }

    fn digit(&mut self, d: u8) {
        if self.full {
            return;
        }
        if self.len == 0 {
            self.len = 1;
            self.vals[0] = 0;
            self.colon[0] = false;
        }
        let v = &mut self.vals[self.len - 1];
        *v = v.saturating_mul(10).saturating_add(d as u16);
    }

    fn separator(&mut self, colon: bool) {
        if self.len == 0 {
            // The parameter before the first separator was left out.
            self.len = 1;
            self.vals[0] = 0;
            self.colon[0] = false;
        }
        if self.len < MAX_PARAMS {
            self.vals[self.len] = 0;
            self.colon[self.len] = colon;
            self.len += 1;
        } else {
            self.full = true;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum State {
    #[default]
    Ground,
    /// After `ESC`, and after its intermediate bytes.
    Escape,
    /// After `ESC [`.
    Csi,
    /// After `ESC ]`.
    Osc,
    /// An `ESC` inside an operating-system command: `\` ends the command.
    OscEsc,
    /// Inside a string that is thrown away (`ESC P`, `ESC X`, `ESC ^`, `ESC _`).
    Str,
    /// An `ESC` inside such a string: `\` ends it.
    StrEsc,
}

/// The parser. It holds what it has read of a sequence or a character that is not finished, so
/// the bytes can be fed in pieces of any size: a piece at a time or all at once gives the same
/// calls.
#[derive(Clone, Debug, Default)]
pub struct Parser {
    state: State,
    params: Params,
    intermediates: [u8; 2],
    n_inter: usize,
    private: u8,
    /// The sequence being read is malformed; it is read to its end and not passed on.
    ignore: bool,
    osc: Vec<u8>,
    /// Continuation bytes still owed to the UTF-8 character being read.
    utf8_need: u8,
    utf8_cp: u32,
    /// The smallest code point a sequence of this length may encode; below it is an overlong
    /// form, which is refused.
    utf8_min: u32,
}

impl Parser {
    pub fn new() -> Parser {
        Parser::default()
    }

    /// Reads `bytes` and tells `to` what they hold.
    pub fn feed(&mut self, bytes: &[u8], to: &mut impl Perform) {
        for &b in bytes {
            self.byte(b, to);
        }
    }

    /// Whether the parser is between sequences: nothing is half read.
    pub fn at_rest(&self) -> bool {
        self.state == State::Ground && self.utf8_need == 0
    }

    fn enter_escape(&mut self) {
        self.state = State::Escape;
        self.n_inter = 0;
        self.ignore = false;
    }

    fn intermediate(&mut self, b: u8) {
        if self.n_inter < self.intermediates.len() {
            self.intermediates[self.n_inter] = b;
            self.n_inter += 1;
        } else {
            self.ignore = true;
        }
    }

    fn byte(&mut self, b: u8, to: &mut impl Perform) {
        // A character cut short by anything but its continuation is a bad one; the byte that
        // cut it is then read on its own.
        if self.utf8_need > 0 {
            if b & 0xC0 == 0x80 {
                self.utf8_cp = (self.utf8_cp << 6) | (b & 0x3F) as u32;
                self.utf8_need -= 1;
                if self.utf8_need == 0 {
                    let c = char::from_u32(self.utf8_cp)
                        .filter(|_| self.utf8_cp >= self.utf8_min)
                        .unwrap_or(char::REPLACEMENT_CHARACTER);
                    to.print(c);
                }
                return;
            }
            self.utf8_need = 0;
            to.print(char::REPLACEMENT_CHARACTER);
        }
        match self.state {
            State::Ground => self.ground(b, to),
            State::Escape => self.escape(b, to),
            State::Csi => self.csi(b, to),
            State::Osc => match b {
                0x07 => {
                    to.osc(&self.osc);
                    self.osc.clear();
                    self.state = State::Ground;
                }
                0x1B => self.state = State::OscEsc,
                0x18 | 0x1A => {
                    self.osc.clear();
                    self.state = State::Ground;
                }
                // The other controls have no place in a command and are dropped.
                0x00..=0x1F => {}
                _ => {
                    if self.osc.len() < MAX_OSC {
                        self.osc.push(b);
                    }
                }
            },
            State::OscEsc => {
                if b == b'\\' {
                    to.osc(&self.osc);
                    self.osc.clear();
                    self.state = State::Ground;
                } else {
                    // An escape that does not end the command begins a new sequence, and the
                    // command is given up.
                    self.osc.clear();
                    self.enter_escape();
                    self.escape(b, to);
                }
            }
            State::Str => match b {
                0x1B => self.state = State::StrEsc,
                0x18 | 0x1A => self.state = State::Ground,
                _ => {}
            },
            State::StrEsc => {
                if b == b'\\' {
                    self.state = State::Ground;
                } else {
                    self.enter_escape();
                    self.escape(b, to);
                }
            }
        }
    }

    fn ground(&mut self, b: u8, to: &mut impl Perform) {
        match b {
            0x1B => self.enter_escape(),
            0x00..=0x1F => to.execute(b),
            0x20..=0x7E => to.print(b as char),
            // DEL is padding.
            0x7F => {}
            0xC2..=0xDF => self.utf8_start(1, (b & 0x1F) as u32, 0x80),
            0xE0..=0xEF => self.utf8_start(2, (b & 0x0F) as u32, 0x800),
            0xF0..=0xF4 => self.utf8_start(3, (b & 0x07) as u32, 0x1_0000),
            // A continuation byte with nothing to continue, or a byte UTF-8 never uses.
            _ => to.print(char::REPLACEMENT_CHARACTER),
        }
    }

    fn utf8_start(&mut self, need: u8, cp: u32, min: u32) {
        self.utf8_need = need;
        self.utf8_cp = cp;
        self.utf8_min = min;
    }

    fn escape(&mut self, b: u8, to: &mut impl Perform) {
        match b {
            0x1B => self.enter_escape(),
            0x18 | 0x1A => self.state = State::Ground,
            0x00..=0x1F => to.execute(b),
            0x20..=0x2F => self.intermediate(b),
            0x7F => {}
            // After an intermediate every byte from 0x30 on is a final byte; before one, a few
            // of them open the longer sequences.
            b'[' if self.n_inter == 0 => {
                self.state = State::Csi;
                self.params.clear();
                self.private = 0;
            }
            b']' if self.n_inter == 0 => {
                self.state = State::Osc;
                self.osc.clear();
            }
            b'P' | b'X' | b'^' | b'_' if self.n_inter == 0 => self.state = State::Str,
            0x30..=0x7E => {
                if !self.ignore {
                    to.esc(&self.intermediates[..self.n_inter], b);
                }
                self.state = State::Ground;
            }
            // Not ASCII: no sequence continues with it. The sequence is given up and the byte
            // read as text.
            _ => {
                self.state = State::Ground;
                self.ground(b, to);
            }
        }
    }

    fn csi(&mut self, b: u8, to: &mut impl Perform) {
        match b {
            0x1B => self.enter_escape(),
            0x18 | 0x1A => self.state = State::Ground,
            0x00..=0x1F => to.execute(b),
            b'0'..=b'9' => {
                if self.n_inter > 0 {
                    self.ignore = true;
                } else {
                    self.params.digit(b - b'0');
                }
            }
            b';' | b':' => {
                if self.n_inter > 0 {
                    self.ignore = true;
                } else {
                    self.params.separator(b == b':');
                }
            }
            b'<'..=b'?' => {
                // A private marker comes first or not at all.
                if self.params.is_empty() && self.private == 0 && self.n_inter == 0 {
                    self.private = b;
                } else {
                    self.ignore = true;
                }
            }
            0x20..=0x2F => self.intermediate(b),
            0x40..=0x7E => {
                if !self.ignore {
                    to.csi(
                        &self.params,
                        self.private,
                        &self.intermediates[..self.n_inter],
                        b,
                    );
                }
                self.state = State::Ground;
            }
            0x7F => {}
            _ => {
                self.state = State::Ground;
                self.ground(b, to);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes down every call, one line each.
    #[derive(Default)]
    struct Log(Vec<String>);

    impl Perform for Log {
        fn print(&mut self, c: char) {
            match self.0.last_mut() {
                Some(last) if last.starts_with("print ") => last.push(c),
                _ => self.0.push(format!("print {c}")),
            }
        }
        fn execute(&mut self, byte: u8) {
            self.0.push(format!("exec {byte:02x}"));
        }
        fn csi(&mut self, params: &Params, private: u8, intermediates: &[u8], last: u8) {
            let mut groups = Vec::new();
            let mut i = 0;
            while i < params.len() {
                let g = params.group_at(i);
                i += g.len();
                let g: Vec<String> = g.iter().map(|v| v.to_string()).collect();
                groups.push(g.join(":"));
            }
            let private = if private == 0 {
                String::new()
            } else {
                (private as char).to_string()
            };
            self.0.push(format!(
                "csi {private}{}{}{}",
                groups.join(";"),
                String::from_utf8_lossy(intermediates),
                last as char
            ));
        }
        fn esc(&mut self, intermediates: &[u8], last: u8) {
            self.0.push(format!(
                "esc {}{}",
                String::from_utf8_lossy(intermediates),
                last as char
            ));
        }
        fn osc(&mut self, data: &[u8]) {
            self.0
                .push(format!("osc {}", String::from_utf8_lossy(data)));
        }
    }

    fn parsed(bytes: &[u8]) -> Vec<String> {
        let mut log = Log::default();
        let mut p = Parser::new();
        p.feed(bytes, &mut log);
        log.0
    }

    #[test]
    fn text_controls_and_sequences_are_told_apart() {
        assert_eq!(
            parsed(b"ab\r\n\x1b[1;31mc\x1b[m\x1b7\x1b(0\x1b]0;title\x07d"),
            [
                "print ab",
                "exec 0d",
                "exec 0a",
                "csi 1;31m",
                "print c",
                "csi m",
                "esc 7",
                "esc (0",
                "osc 0;title",
                "print d"
            ]
        );
    }

    #[test]
    fn parameters_keep_their_gaps_their_colons_and_their_limits() {
        assert_eq!(parsed(b"\x1b[;5H"), ["csi 0;5H"]);
        assert_eq!(parsed(b"\x1b[5;H"), ["csi 5;0H"]);
        assert_eq!(
            parsed(b"\x1b[38:2::255:0:10;1m"),
            ["csi 38:2:0:255:0:10;1m"]
        );
        assert_eq!(
            parsed(b"\x1b[?1049h\x1b[>c\x1b[!p\x1b[2 q"),
            ["csi ?1049h", "csi >c", "csi !p", "csi 2 q"]
        );
        assert_eq!(parsed(b"\x1b[?25$p"), ["csi ?25$p"]);
        // A number too large for 16 bits stops at the top; it does not wrap round.
        assert_eq!(parsed(b"\x1b[99999999999A"), ["csi 65535A"]);
        // More parameters than fit: the first ones are kept and the rest dropped.
        let mut many = b"\x1b[".to_vec();
        for i in 0..100 {
            many.extend_from_slice(format!("{i};").as_bytes());
        }
        many.push(b'm');
        let mut log = Log::default();
        Parser::new().feed(&many, &mut log);
        let line = &log.0[0];
        assert!(
            line.starts_with("csi 0;1;2;") && line.ends_with(";31m"),
            "{line}"
        );

        let mut p = Params::default();
        assert!(p.is_empty() && p.get(3) == 0 && p.group_at(0).is_empty());
        p.digit(4);
        p.separator(false);
        assert_eq!(
            (p.len(), p.get(0), p.get_or(1, 7), p.get_or(0, 7)),
            (2, 4, 7, 4)
        );
        assert_eq!(p.values(), [4, 0]);
    }

    #[test]
    fn malformed_sequences_are_read_to_their_end_and_dropped() {
        // A marker after a parameter, a digit after an intermediate, three intermediates.
        assert_eq!(parsed(b"\x1b[1?hX"), ["print X"]);
        assert_eq!(parsed(b"\x1b[ 1qX"), ["print X"]);
        assert_eq!(parsed(b"\x1b[ !\"qX"), ["print X"]);
        assert_eq!(parsed(b"\x1b !\"FX"), ["print X"]);
        // Cancel and substitute end a sequence with nothing done; an escape starts a new one.
        assert_eq!(parsed(b"\x1b[12\x18A"), ["print A"]);
        assert_eq!(parsed(b"\x1b[12\x1aA"), ["print A"]);
        assert_eq!(parsed(b"\x1b[12\x1b[3B"), ["csi 3B"]);
        // A control inside a sequence acts at once and the sequence goes on.
        assert_eq!(parsed(b"\x1b[1\n2A"), ["exec 0a", "csi 12A"]);
        // DEL is padding everywhere.
        assert_eq!(parsed(b"a\x7fb\x1b[\x7f2A"), ["print ab", "csi 2A"]);
    }

    #[test]
    fn commands_end_at_a_bell_or_a_string_terminator_and_other_strings_are_swallowed() {
        assert_eq!(parsed(b"\x1b]2;one\x1b\\x"), ["osc 2;one", "print x"]);
        assert_eq!(parsed(b"\x1b]2;one\x07x"), ["osc 2;one", "print x"]);
        // Given up at a cancel, and at an escape that is not the terminator.
        assert_eq!(parsed(b"\x1b]2;one\x18x"), ["print x"]);
        assert_eq!(parsed(b"\x1b]2;one\x1b[1mx"), ["csi 1m", "print x"]);
        // UTF-8 in a command is kept as its bytes.
        assert_eq!(parsed("\x1b]0;é\x07".as_bytes()), ["osc 0;é"]);
        // Device control, start of string, privacy message, application command.
        for open in *b"PX^_" {
            let mut bytes = vec![0x1b, open];
            bytes.extend_from_slice(b"1$r junk \x07 more\x1b\\ok");
            assert_eq!(parsed(&bytes), ["print ok"], "ESC {}", open as char);
        }
        assert_eq!(parsed(b"\x1bPjunk\x1b[1mok"), ["csi 1m", "print ok"]);
        // A command longer than the limit keeps its start.
        let mut long = b"\x1b]0;".to_vec();
        long.extend(std::iter::repeat_n(b'a', 3 * MAX_OSC));
        long.push(0x07);
        let out = parsed(&long);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), "osc ".len() + MAX_OSC);
    }

    #[test]
    fn utf8_is_decoded_and_what_is_not_utf8_is_the_replacement_character() {
        assert_eq!(parsed("aé€😀".as_bytes()), ["print aé€😀"]);
        // A lone continuation, a byte UTF-8 never uses, a start with no continuation.
        assert_eq!(parsed(b"\x80"), ["print \u{fffd}"]);
        assert_eq!(parsed(b"\xff"), ["print \u{fffd}"]);
        assert_eq!(parsed(b"\xc3a"), ["print \u{fffd}a"]);
        assert_eq!(parsed(b"\xe2\x82a"), ["print \u{fffd}a"]);
        // Overlong forms, a surrogate, past the last code point.
        assert_eq!(parsed(b"\xc0\xaf"), ["print \u{fffd}\u{fffd}"]);
        assert_eq!(parsed(b"\xe0\x80\xaf"), ["print \u{fffd}"]);
        assert_eq!(parsed(b"\xed\xa0\x80"), ["print \u{fffd}"]);
        assert_eq!(parsed(b"\xf4\x90\x80\x80"), ["print \u{fffd}"]);
        // A control that cuts a character short still acts.
        assert_eq!(parsed(b"\xe2\x82\x1b[1m"), ["print \u{fffd}", "csi 1m"]);
        assert_eq!(parsed(b"\xe2\n"), ["print \u{fffd}", "exec 0a"]);
    }

    #[test]
    fn bytes_fed_one_at_a_time_give_the_same_calls() {
        let stream = "plain \x1b[38;2;1;2;3mcolour\x1b[0m é€😀 \x1b]0;a title\x1b\\ \x1bPdcs\x1b\\ \x1b(0q\x1b(B\r\n\x1b[?25l"
            .as_bytes();
        let whole = parsed(stream);
        let mut log = Log::default();
        let mut p = Parser::new();
        for b in stream {
            p.feed(std::slice::from_ref(b), &mut log);
        }
        assert_eq!(log.0, whole);
        assert!(p.at_rest());
        p.feed(b"\x1b[", &mut log);
        assert!(!p.at_rest());
        let mut q = Parser::new();
        q.feed(b"\xe2", &mut log);
        assert!(!q.at_rest());
    }

    #[test]
    fn no_bytes_make_it_panic_or_grow() {
        // A small generator with a fixed seed: the same garbage every run.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut log = Log::default();
        let mut p = Parser::new();
        for _ in 0..200_000 {
            let r = next();
            // Mostly the bytes sequences are made of, so the states are all reached.
            let b = match r % 8 {
                0 => 0x1b,
                1 => {
                    let parts = b"[]P;:?0123456789mHJ\\\x07\x18";
                    parts[(r >> 8) as usize % parts.len()]
                }
                _ => (r >> 16) as u8,
            };
            p.feed(&[b], &mut log);
            log.0.clear();
            assert!(p.osc.len() <= MAX_OSC && p.params.len() <= MAX_PARAMS);
        }
    }
}
