//! Hand-written line scanners for Kotlin, Swift, PowerShell and Dockerfile, which syntect's
//! bundled set lacks. Like the TOML scanner they cost no binary size beyond their own code (a
//! second syntax dump would) and nothing at start-up: the colours are looked up when a block in
//! one of these languages is first highlighted.
//!
//! One line at a time, with a small [`State`] carried from line to line (a block comment's depth,
//! a multi-line string, a here-string, a Dockerfile continuation or heredoc). Every scanner walks
//! its line once, left to right, and never rescans text it has already consumed, so the work is
//! linear in the input whatever it contains: an unterminated string, a million `\(` or `${`, or
//! `/*` nested without end. Colours of the tokens come from the same two themes as everything
//! else, picked by scope in [`Colors`].

use crate::incremental::Span;
use crate::scope_color;
use syntect::highlighting::Theme;

/// A language read by a scanner here rather than by syntect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Kotlin,
    Swift,
    PowerShell,
    Dockerfile,
}

impl Lang {
    /// By a fence-style name or file extension, case-insensitively: `kotlin` `kt` `kts`,
    /// `swift`, `powershell` `pwsh` `ps1` `psm1` `psd1`, `dockerfile` `docker` `containerfile`.
    pub fn from_token(token: &str) -> Option<Lang> {
        let token = token.trim();
        let is = |names: &[&str]| names.iter().any(|n| token.eq_ignore_ascii_case(n));
        if is(&["kotlin", "kt", "kts"]) {
            Some(Lang::Kotlin)
        } else if is(&["swift"]) {
            Some(Lang::Swift)
        } else if is(&["powershell", "pwsh", "ps1", "psm1", "psd1"]) {
            Some(Lang::PowerShell)
        } else if is(&["dockerfile", "docker", "containerfile"]) {
            Some(Lang::Dockerfile)
        } else {
            None
        }
    }

    /// By file name and extension: `Dockerfile`, `Containerfile`, `Dockerfile.dev`,
    /// `app.dockerfile`, `build.kts`, and so on.
    pub fn from_path(name: Option<&str>, extension: Option<&str>) -> Option<Lang> {
        if let Some(name) = name {
            let lower = name.to_ascii_lowercase();
            for base in ["dockerfile", "containerfile"] {
                if lower == base || lower.starts_with(&format!("{base}.")) {
                    return Some(Lang::Dockerfile);
                }
            }
        }
        extension.and_then(Lang::from_token)
    }

    /// By a `#!` line naming PowerShell.
    pub fn from_first_line(line: &str) -> Option<Lang> {
        let line = line.trim_start();
        (line.starts_with("#!") && (line.contains("pwsh") || line.contains("powershell")))
            .then_some(Lang::PowerShell)
    }

    pub fn name(self) -> &'static str {
        match self {
            Lang::Kotlin => "Kotlin",
            Lang::Swift => "Swift",
            Lang::PowerShell => "PowerShell",
            Lang::Dockerfile => "Dockerfile",
        }
    }
}

/// What a scanner carries from the end of one line to the start of the next.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum State {
    #[default]
    Normal,
    /// Inside a `/* */` (nesting depth) or a PowerShell `<# #>` (1).
    Block(u32),
    /// Inside a multi-line `"""` string; Swift's raw strings carry their number of `#`.
    Str { hashes: u8 },
    /// Inside a PowerShell here-string opened with this quote.
    Here(u8),
    /// A Dockerfile instruction continues from the line above.
    Continue,
    /// Inside a Dockerfile heredoc that ends at a line with this text.
    Heredoc(Box<str>),
}

/// The theme's colours for each kind of token.
#[derive(Clone, Copy)]
pub(crate) struct Colors {
    plain: [u8; 3],
    comment: [u8; 3],
    string: [u8; 3],
    escape: [u8; 3],
    number: [u8; 3],
    keyword: [u8; 3],
    constant: [u8; 3],
    type_: [u8; 3],
    function: [u8; 3],
    cmdlet: [u8; 3],
    annotation: [u8; 3],
    variable: [u8; 3],
}

impl Colors {
    pub(crate) fn new(theme: &Theme) -> Colors {
        Colors {
            plain: scope_color(theme, "source"),
            comment: scope_color(theme, "comment.line"),
            string: scope_color(theme, "string.quoted"),
            escape: scope_color(theme, "constant.character.escape"),
            number: scope_color(theme, "constant.numeric"),
            keyword: scope_color(theme, "keyword.control"),
            constant: scope_color(theme, "constant.language"),
            type_: scope_color(theme, "entity.name.class"),
            function: scope_color(theme, "entity.name.function"),
            cmdlet: scope_color(theme, "support.function"),
            annotation: scope_color(theme, "entity.other.attribute-name"),
            variable: scope_color(theme, "entity.name.tag"),
        }
    }
}

/// Colours a line left to right: whatever lies between painted pieces is plain, and neighbours of
/// one colour merge, so the spans are sorted, contiguous and cover the line.
struct Painter<'a> {
    out: &'a mut Vec<Span>,
    pos: usize,
    plain: [u8; 3],
}

impl Painter<'_> {
    fn paint(&mut self, start: usize, end: usize, color: [u8; 3]) {
        if start > self.pos {
            self.fill(start, self.plain);
        }
        self.fill(end, color);
    }

    fn fill(&mut self, end: usize, color: [u8; 3]) {
        if end <= self.pos {
            return;
        }
        match self.out.last_mut() {
            Some(last) if last.color == color && last.end as usize == self.pos => {
                last.end = end as u32;
            }
            _ => self.out.push(Span {
                start: self.pos as u32,
                end: end as u32,
                color,
            }),
        }
        self.pos = end;
    }
}

/// Spans for one line of text (without its line break), appended to `out`, and the state for the
/// next line. The spans cover the line from 0 to its length.
pub(crate) fn scan_line(
    lang: Lang,
    colors: &Colors,
    line: &str,
    state: &mut State,
    out: &mut Vec<Span>,
) {
    let mut p = Painter {
        out,
        pos: 0,
        plain: colors.plain,
    };
    match lang {
        Lang::Kotlin | Lang::Swift => cfamily(lang, colors, line, state, &mut p),
        Lang::PowerShell => powershell(colors, line, state, &mut p),
        Lang::Dockerfile => dockerfile(colors, line, state, &mut p),
    }
    p.fill(line.len(), colors.plain);
}

fn ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

fn ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}

fn ident_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && ident_byte(b[i]) {
        i += 1;
    }
    i
}

/// The length of the UTF-8 character starting at `i` (1 at the end of the text).
fn char_len(b: &[u8], i: usize) -> usize {
    let len = match b.get(i) {
        Some(c) if *c >= 0xf0 => 4,
        Some(c) if *c >= 0xe0 => 3,
        Some(c) if *c >= 0xc0 => 2,
        _ => 1,
    };
    len.min(b.len().saturating_sub(i)).max(1)
}

/// Past the number that starts at `i`: digits, letters (hex digits, suffixes), underscores, a
/// fraction, an exponent sign.
fn number_end(b: &[u8], i: usize) -> usize {
    let hex = b[i] == b'0' && matches!(b.get(i + 1), Some(b'x' | b'X' | b'b' | b'B'));
    let mut j = i;
    while j < b.len() {
        let c = b[j];
        if c.is_ascii_alphanumeric() || c == b'_' {
            let sign = !hex
                && matches!(c, b'e' | b'E')
                && matches!(b.get(j + 1), Some(b'+' | b'-'))
                && b.get(j + 2).is_some_and(u8::is_ascii_digit);
            j += if sign { 2 } else { 1 };
        } else if c == b'.' && b.get(j + 1).is_some_and(u8::is_ascii_digit) {
            j += 1;
        } else {
            break;
        }
    }
    j
}

/// Past the bracket that closes the group whose contents start at `from`, with its end found, or
/// the end of the text when it is not closed on this line.
fn close_group(b: &[u8], from: usize, open: u8, close: u8) -> (usize, bool) {
    let mut depth = 1u32;
    let mut j = from;
    while j < b.len() {
        if b[j] == open {
            depth += 1;
        } else if b[j] == close {
            depth -= 1;
            if depth == 0 {
                return (j + 1, true);
            }
        }
        j += 1;
    }
    (b.len(), false)
}

/// Past the end of a nestable `/* */` comment whose contents start at `from` at `depth`, and the
/// depth still open there.
fn skip_block(b: &[u8], from: usize, mut depth: u32) -> (usize, u32) {
    let mut j = from;
    while j < b.len() {
        if b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
            depth += 1;
            j += 2;
        } else if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
            depth -= 1;
            j += 2;
            if depth == 0 {
                return (j, 0);
            }
        } else {
            j += 1;
        }
    }
    (b.len(), depth)
}

fn find(b: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    b.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|k| k + from)
}

fn word_in(list: &[&str], word: &str) -> bool {
    word.len() <= 16 && list.contains(&word)
}

fn word_in_nocase(list: &[&str], word: &str) -> bool {
    word.len() <= 16 && list.iter().any(|k| k.eq_ignore_ascii_case(word))
}

// ---------------------------------------------------------------------------------------------
// Kotlin and Swift

const KOTLIN_KEYWORDS: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "do",
    "else",
    "for",
    "fun",
    "if",
    "in",
    "interface",
    "is",
    "object",
    "package",
    "return",
    "super",
    "this",
    "throw",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "when",
    "while",
    "by",
    "catch",
    "constructor",
    "finally",
    "import",
    "init",
    "abstract",
    "actual",
    "annotation",
    "companion",
    "const",
    "crossinline",
    "data",
    "enum",
    "expect",
    "external",
    "final",
    "infix",
    "inline",
    "inner",
    "internal",
    "lateinit",
    "noinline",
    "open",
    "operator",
    "out",
    "override",
    "private",
    "protected",
    "public",
    "reified",
    "sealed",
    "suspend",
    "tailrec",
    "vararg",
];

const SWIFT_KEYWORDS: &[&str] = &[
    "associatedtype",
    "class",
    "deinit",
    "enum",
    "extension",
    "fileprivate",
    "func",
    "import",
    "init",
    "inout",
    "internal",
    "let",
    "open",
    "operator",
    "private",
    "precedencegroup",
    "protocol",
    "public",
    "rethrows",
    "static",
    "struct",
    "subscript",
    "typealias",
    "var",
    "break",
    "case",
    "catch",
    "continue",
    "default",
    "defer",
    "do",
    "else",
    "fallthrough",
    "for",
    "guard",
    "if",
    "in",
    "repeat",
    "return",
    "throw",
    "switch",
    "where",
    "while",
    "as",
    "await",
    "is",
    "self",
    "Self",
    "super",
    "throws",
    "try",
    "async",
    "actor",
    "some",
    "any",
    "lazy",
    "final",
    "mutating",
    "nonmutating",
    "override",
    "required",
    "convenience",
    "weak",
    "unowned",
    "indirect",
    "dynamic",
    "willSet",
    "didSet",
    "nonisolated",
    "isolated",
    "consuming",
    "borrowing",
    "macro",
    "package",
];

const C_CONSTANTS: &[&str] = &["true", "false", "null", "nil"];

#[derive(Clone, Copy, PartialEq)]
enum Pending {
    None,
    Function,
    Type,
}

fn cfamily(lang: Lang, c: &Colors, line: &str, state: &mut State, p: &mut Painter) {
    let b = line.as_bytes();
    let n = b.len();
    let swift = lang == Lang::Swift;
    let keywords = if swift {
        SWIFT_KEYWORDS
    } else {
        KOTLIN_KEYWORDS
    };
    let mut i = 0;
    match *state {
        State::Block(depth) => {
            let (end, depth) = skip_block(b, 0, depth);
            p.paint(0, end, c.comment);
            *state = if depth > 0 {
                State::Block(depth)
            } else {
                State::Normal
            };
            i = end;
        }
        State::Str { hashes } => {
            let (end, closed) = string_body(lang, c, b, 0, 0, true, hashes, p);
            i = end;
            if closed {
                *state = State::Normal;
            }
        }
        _ => *state = State::Normal,
    }
    let mut pending = Pending::None;
    while i < n {
        let ch = b[i];
        match ch {
            b' ' | b'\t' | b'\r' => i += 1,
            b'/' if b.get(i + 1) == Some(&b'/') => {
                p.paint(i, n, c.comment);
                i = n;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let (end, depth) = skip_block(b, i + 2, 1);
                p.paint(i, end, c.comment);
                if depth > 0 {
                    *state = State::Block(depth);
                }
                i = end;
            }
            b'"' | b'#' => {
                // Swift raw strings start with `#`s; anything else with `#` is a directive.
                let mut hashes = 0;
                let mut q = i;
                if swift {
                    while b.get(q) == Some(&b'#') {
                        hashes += 1;
                        q += 1;
                    }
                }
                if b.get(q) == Some(&b'"') {
                    let multi = b[q..].starts_with(b"\"\"\"");
                    let open = q + if multi { 3 } else { 1 };
                    let (end, closed) =
                        string_body(lang, c, b, i, open, multi, hashes.min(255) as u8, p);
                    if multi && !closed {
                        *state = State::Str {
                            hashes: hashes.min(255) as u8,
                        };
                    }
                    i = end;
                } else if swift && ch == b'#' && b.get(i + 1).is_some_and(u8::is_ascii_alphabetic) {
                    let end = ident_end(b, i + 1);
                    p.paint(i, end, c.keyword);
                    i = end;
                } else {
                    // Past the `#`s, so that a run of them is looked at once.
                    i = q.max(i + 1);
                }
                pending = Pending::None;
            }
            b'\'' if !swift => {
                // A character literal: closes within a few bytes, or is just a quote.
                let from = i + 1 + usize::from(b.get(i + 1) == Some(&b'\\'));
                let stop = b[from.min(n)..n.min(from + 10)]
                    .iter()
                    .position(|x| *x == b'\'');
                match stop {
                    Some(k) => {
                        p.paint(i, from + k + 1, c.string);
                        i = from + k + 1;
                    }
                    None => i += 1,
                }
            }
            b'@' if b.get(i + 1).is_some_and(|x| ident_start(*x)) => {
                let mut end = ident_end(b, i + 1);
                // Kotlin's `@file:Name` and a dotted `@a.B`.
                while matches!(b.get(end), Some(b':' | b'.'))
                    && b.get(end + 1).is_some_and(|x| ident_start(*x))
                {
                    end = ident_end(b, end + 1);
                }
                p.paint(i, end, c.annotation);
                i = end;
            }
            b'0'..=b'9' => {
                let end = number_end(b, i);
                p.paint(i, end, c.number);
                i = end;
                pending = Pending::None;
            }
            _ if ident_start(ch) => {
                let end = ident_end(b, i);
                let word = &line[i..end];
                let declares = if word_in(&["fun", "func"], word) {
                    Pending::Function
                } else if word_in(
                    &[
                        "class",
                        "interface",
                        "object",
                        "struct",
                        "enum",
                        "protocol",
                        "extension",
                        "typealias",
                        "actor",
                        "associatedtype",
                    ],
                    word,
                ) {
                    Pending::Type
                } else {
                    Pending::None
                };
                let color = if word_in(keywords, word) {
                    Some(c.keyword)
                } else if declares == Pending::None && pending != Pending::None {
                    Some(if pending == Pending::Function {
                        c.function
                    } else {
                        c.type_
                    })
                } else if word_in(C_CONSTANTS, word) {
                    Some(c.constant)
                } else if ch.is_ascii_uppercase() {
                    Some(c.type_)
                } else {
                    None
                };
                if let Some(color) = color {
                    p.paint(i, end, color);
                }
                pending = declares;
                i = end;
            }
            _ => {
                i += 1;
                pending = Pending::None;
            }
        }
    }
}

/// A string literal's text from `open` (just past its opening quotes), painting from `start` (the
/// literal's first byte), as far as its closing quotes or the end of the line. `multi` is a `"""`
/// string, `hashes` Swift's raw-string `#`s. Interpolations (`\(..)`, `${..}`, `$name`) are
/// painted apart from the string. Returns where it stopped and whether the string closed.
#[allow(clippy::too_many_arguments)]
fn string_body(
    lang: Lang,
    c: &Colors,
    b: &[u8],
    start: usize,
    open: usize,
    multi: bool,
    hashes: u8,
    p: &mut Painter,
) -> (usize, bool) {
    let n = b.len();
    let swift = lang == Lang::Swift;
    let hashes = hashes as usize;
    let quotes: &[u8] = if multi { b"\"\"\"" } else { b"\"" };
    let mut seg = start;
    let mut i = open;
    while i < n {
        let ch = b[i];
        // An interpolation or escape: Swift `\#..(`, Kotlin `$` and (outside raw strings) `\`.
        let escaped = if swift {
            ch == b'\\' && b[i + 1..].iter().take(hashes).all(|x| *x == b'#') && i + hashes < n
        } else {
            ch == b'\\' && !multi
        };
        if escaped {
            let j = i + 1 + if swift { hashes } else { 0 };
            if swift && b.get(j) == Some(&b'(') {
                i = interpolation(c, b, seg, i, j + 1, b'(', b')', p);
                seg = i;
            } else {
                p.paint(seg, i, c.string);
                let end = (j + char_len(b, j)).min(n);
                p.paint(i, end, c.escape);
                i = end;
                seg = end;
            }
            continue;
        }
        if !swift && ch == b'$' {
            match b.get(i + 1) {
                Some(b'{') => {
                    i = interpolation(c, b, seg, i, i + 2, b'{', b'}', p);
                    seg = i;
                    continue;
                }
                Some(x) if ident_start(*x) => {
                    p.paint(seg, i, c.string);
                    let end = ident_end(b, i + 1);
                    p.paint(i, end, c.variable);
                    i = end;
                    seg = end;
                    continue;
                }
                _ => {}
            }
        }
        if ch == b'"'
            && b[i..].starts_with(quotes)
            && b[i + quotes.len()..]
                .iter()
                .take(hashes)
                .all(|x| *x == b'#')
            && i + quotes.len() + hashes <= n
            && b.len() - (i + quotes.len()) >= hashes
        {
            let end = i + quotes.len() + hashes;
            p.paint(seg, end, c.string);
            return (end, true);
        }
        i += 1;
    }
    p.paint(seg, n, c.string);
    (n, false)
}

/// An interpolation that starts at `at` with its contents from `inner`: the string up to it, the
/// delimiters, and the contents as plain code. Returns the position after it.
#[allow(clippy::too_many_arguments)]
fn interpolation(
    c: &Colors,
    b: &[u8],
    seg: usize,
    at: usize,
    inner: usize,
    open: u8,
    close: u8,
    p: &mut Painter,
) -> usize {
    p.paint(seg, at, c.string);
    p.paint(at, inner.min(b.len()), c.escape);
    let (end, closed) = close_group(b, inner, open, close);
    if closed {
        p.paint(inner, end - 1, c.plain);
        p.paint(end - 1, end, c.escape);
    } else {
        p.paint(inner, end, c.plain);
    }
    end
}

// ---------------------------------------------------------------------------------------------
// PowerShell

const PS_KEYWORDS: &[&str] = &[
    "begin",
    "break",
    "catch",
    "class",
    "continue",
    "data",
    "do",
    "dynamicparam",
    "else",
    "elseif",
    "end",
    "enum",
    "exit",
    "filter",
    "finally",
    "for",
    "foreach",
    "from",
    "function",
    "hidden",
    "if",
    "in",
    "param",
    "parallel",
    "process",
    "return",
    "static",
    "switch",
    "throw",
    "trap",
    "try",
    "until",
    "using",
    "var",
    "while",
    "workflow",
];

const PS_OPERATORS: &[&str] = &[
    "eq",
    "ne",
    "gt",
    "ge",
    "lt",
    "le",
    "like",
    "notlike",
    "match",
    "notmatch",
    "contains",
    "notcontains",
    "in",
    "notin",
    "is",
    "isnot",
    "as",
    "and",
    "or",
    "xor",
    "not",
    "band",
    "bor",
    "bxor",
    "bnot",
    "shl",
    "shr",
    "replace",
    "split",
    "join",
    "f",
    "ceq",
    "cne",
    "cgt",
    "cge",
    "clt",
    "cle",
    "clike",
    "cmatch",
    "ieq",
    "ine",
    "igt",
    "ilt",
];

fn powershell(c: &Colors, line: &str, state: &mut State, p: &mut Painter) {
    let b = line.as_bytes();
    let n = b.len();
    let mut i = 0;
    match *state {
        State::Block(_) => {
            let end = find(b, 0, b"#>").map_or(n, |k| k + 2);
            p.paint(0, end, c.comment);
            if end < n || line.ends_with("#>") {
                *state = State::Normal;
            }
            i = end;
        }
        State::Here(q) => {
            if b.first() == Some(&q) && b.get(1) == Some(&b'@') {
                p.paint(0, 2, c.string);
                *state = State::Normal;
                i = 2;
            } else {
                ps_string(c, b, 0, 0, q, true, p);
                i = n;
            }
        }
        _ => *state = State::Normal,
    }
    let mut pending = false;
    while i < n {
        let ch = b[i];
        match ch {
            b' ' | b'\t' | b'\r' => i += 1,
            b'#' => {
                p.paint(i, n, c.comment);
                i = n;
            }
            b'<' if b.get(i + 1) == Some(&b'#') => {
                let end = find(b, i + 2, b"#>").map_or(n, |k| k + 2);
                p.paint(i, end, c.comment);
                if find(b, i + 2, b"#>").is_none() {
                    *state = State::Block(1);
                }
                i = end;
            }
            b'@' if matches!(b.get(i + 1), Some(b'"' | b'\''))
                && b[i + 2..].iter().all(u8::is_ascii_whitespace) =>
            {
                p.paint(i, n, c.string);
                *state = State::Here(b[i + 1]);
                i = n;
            }
            b'"' | b'\'' => {
                i = ps_string(c, b, i, i + 1, ch, false, p).0;
                pending = false;
            }
            b'$' => {
                let next = b.get(i + 1).copied();
                match next {
                    Some(b'{') => {
                        let end = find(b, i + 2, b"}").map_or(n, |k| k + 1);
                        p.paint(i, end, c.variable);
                        i = end;
                    }
                    Some(x) if ident_start(x) || matches!(x, b'$' | b'?' | b'^') => {
                        let end = ps_variable_end(b, i + 1);
                        let name = &line[i + 1..end];
                        let color = if word_in_nocase(&["true", "false", "null"], name) {
                            c.constant
                        } else {
                            c.variable
                        };
                        p.paint(i, end, color);
                        i = end;
                    }
                    _ => i += 1,
                }
            }
            b'-' if b.get(i + 1).is_some_and(u8::is_ascii_alphabetic)
                && (i == 0 || !(ident_byte(b[i - 1]) || b[i - 1] == b'-')) =>
            {
                let end = b[i + 1..]
                    .iter()
                    .position(|x| !x.is_ascii_alphanumeric())
                    .map_or(n, |k| i + 1 + k);
                let word = &line[i + 1..end];
                let color = if word_in_nocase(PS_OPERATORS, word) {
                    c.keyword
                } else {
                    c.annotation
                };
                p.paint(i, end, color);
                i = end;
            }
            b'[' if b.get(i + 1).is_some_and(|x| x.is_ascii_alphabetic()) => {
                // `[type]` or the start of an attribute `[Name(`, up to a character that no type
                // name has.
                let mut j = i + 1;
                let mut depth = 0u32;
                // Type names are short; a longer run is not scanned to its end for each `[`.
                while j < n && j - i < 128 {
                    match b[j] {
                        b'[' => depth += 1,
                        b']' if depth == 0 => break,
                        b']' => depth -= 1,
                        x if ident_byte(x) || matches!(x, b'.' | b',' | b'`') => {}
                        _ => break,
                    }
                    j += 1;
                }
                match b.get(j) {
                    Some(b']') => {
                        p.paint(i, j + 1, c.type_);
                        i = j + 1;
                    }
                    Some(b'(') => {
                        p.paint(i, j, c.annotation);
                        i = j;
                    }
                    _ => i += 1,
                }
            }
            b'0'..=b'9' => {
                let end = number_end(b, i);
                p.paint(i, end, c.number);
                i = end;
                pending = false;
            }
            _ if ident_start(ch) => {
                let mut end = ident_end(b, i);
                // Verb-Noun cmdlets.
                let mut cmdlet = false;
                while b.get(end) == Some(&b'-')
                    && b.get(end + 1).is_some_and(u8::is_ascii_alphabetic)
                {
                    end = ident_end(b, end + 1);
                    cmdlet = true;
                }
                let word = &line[i..end];
                let color = if pending {
                    Some(c.function)
                } else if cmdlet {
                    Some(c.cmdlet)
                } else if word_in_nocase(PS_KEYWORDS, word) {
                    Some(c.keyword)
                } else {
                    None
                };
                if let Some(color) = color {
                    p.paint(i, end, color);
                }
                pending = !pending && word_in_nocase(&["function", "filter", "workflow"], word);
                i = end;
            }
            _ => {
                i += 1;
                pending = false;
            }
        }
    }
}

/// Past a variable name that starts at `i`: letters, digits, `_`, `?`, and `scope:` prefixes.
fn ps_variable_end(b: &[u8], i: usize) -> usize {
    if matches!(b[i], b'$' | b'?' | b'^') {
        return i + 1;
    }
    let mut j = ident_end(b, i);
    while b.get(j) == Some(&b':') && b.get(j + 1).is_some_and(|x| ident_start(*x)) {
        j = ident_end(b, j + 1);
    }
    j
}

/// A PowerShell string painted from `start`, its contents scanned from `from`; `here` strings
/// have no closing quote on the line. `"` strings have backtick escapes and `$` interpolation;
/// a doubled quote is an escaped quote. Returns where it ended and whether it closed.
fn ps_string(
    c: &Colors,
    b: &[u8],
    start: usize,
    from: usize,
    quote: u8,
    here: bool,
    p: &mut Painter,
) -> (usize, bool) {
    let n = b.len();
    let double = quote == b'"';
    let mut seg = start;
    let mut i = from;
    while i < n {
        let ch = b[i];
        if double && ch == b'`' {
            p.paint(seg, i, c.string);
            let end = (i + 1 + char_len(b, i + 1)).min(n);
            p.paint(i, end, c.escape);
            i = end;
            seg = end;
        } else if double && ch == b'$' {
            match b.get(i + 1) {
                Some(b'(') => {
                    i = interpolation(c, b, seg, i, i + 2, b'(', b')', p);
                    seg = i;
                }
                Some(b'{') => {
                    p.paint(seg, i, c.string);
                    let end = find(b, i + 2, b"}").map_or(n, |k| k + 1);
                    p.paint(i, end, c.variable);
                    i = end;
                    seg = end;
                }
                Some(x) if ident_start(*x) => {
                    p.paint(seg, i, c.string);
                    let end = ps_variable_end(b, i + 1);
                    p.paint(i, end, c.variable);
                    i = end;
                    seg = end;
                }
                _ => i += 1,
            }
        } else if ch == quote && !here {
            if b.get(i + 1) == Some(&quote) {
                i += 2;
            } else {
                p.paint(seg, i + 1, c.string);
                return (i + 1, true);
            }
        } else {
            i += 1;
        }
    }
    p.paint(seg, n, c.string);
    (n, false)
}

// ---------------------------------------------------------------------------------------------
// Dockerfile

const DOCKER_INSTRUCTIONS: &[&str] = &[
    "from",
    "run",
    "cmd",
    "label",
    "maintainer",
    "expose",
    "env",
    "add",
    "copy",
    "entrypoint",
    "volume",
    "user",
    "workdir",
    "arg",
    "onbuild",
    "stopsignal",
    "healthcheck",
    "shell",
];

fn dockerfile(c: &Colors, line: &str, state: &mut State, p: &mut Painter) {
    let b = line.as_bytes();
    let n = b.len();
    if let State::Heredoc(terminator) = state {
        let text = line.trim_end_matches('\r');
        if text == &**terminator || text.trim_start_matches('\t') == &**terminator {
            p.paint(0, n, c.constant);
            *state = State::Normal;
        } else {
            p.paint(0, n, c.string);
        }
        return;
    }
    let continued = *state == State::Continue;
    *state = State::Normal;
    let mut i = 0;
    while i < n && matches!(b[i], b' ' | b'\t') {
        i += 1;
    }
    if i < n && b[i] == b'#' {
        p.paint(i, n, c.comment);
        if continued {
            *state = State::Continue;
        }
        return;
    }
    if i >= n && continued {
        *state = State::Continue;
        return;
    }
    let mut instruction = String::new();
    if !continued {
        for _ in 0..2 {
            while i < n && matches!(b[i], b' ' | b'\t') {
                i += 1;
            }
            let end = ident_end(b, i);
            let word = &line[i..end];
            if !word_in_nocase(DOCKER_INSTRUCTIONS, word) {
                break;
            }
            p.paint(i, end, c.keyword);
            i = end;
            instruction = word.to_ascii_lowercase();
            if instruction != "onbuild" {
                break;
            }
        }
    }
    let heredocs = matches!(instruction.as_str(), "run" | "copy" | "add");
    let key_value = matches!(instruction.as_str(), "env" | "arg" | "label");
    let mut heredoc: Option<Box<str>> = None;
    while i < n {
        let ch = b[i];
        match ch {
            b'"' | b'\'' => {
                let mut seg = i;
                let mut j = i + 1;
                while j < n && b[j] != ch {
                    if ch == b'"' && b[j] == b'\\' {
                        j += 2;
                    } else if ch == b'"' && b[j] == b'$' {
                        let end = docker_variable_end(b, j);
                        if end > j + 1 {
                            p.paint(seg, j, c.string);
                            p.paint(j, end, c.variable);
                            seg = end;
                            j = end;
                        } else {
                            j += 1;
                        }
                    } else {
                        j += 1;
                    }
                }
                let end = (j + 1).min(n);
                p.paint(seg, end, c.string);
                i = end;
            }
            b'$' => {
                let end = docker_variable_end(b, i);
                if end > i + 1 {
                    p.paint(i, end, c.variable);
                    i = end;
                } else {
                    i += 1;
                }
            }
            b'-' if b.get(i + 1) == Some(&b'-')
                && b.get(i + 2).is_some_and(u8::is_ascii_alphabetic)
                && (i == 0 || matches!(b[i - 1], b' ' | b'\t')) =>
            {
                let mut end = i + 2;
                while end < n && (b[end].is_ascii_alphanumeric() || b[end] == b'-') {
                    end += 1;
                }
                p.paint(i, end, c.annotation);
                i = end;
            }
            b'<' if heredocs
                && heredoc.is_none()
                && b.get(i + 1) == Some(&b'<')
                && b.get(i + 2) != Some(&b'<') =>
            {
                let mut j = i + 2;
                if b.get(j) == Some(&b'-') {
                    j += 1;
                }
                let quote = matches!(b.get(j), Some(b'"' | b'\'')).then(|| b[j]);
                if quote.is_some() {
                    j += 1;
                }
                let end = ident_end(b, j);
                if end > j {
                    heredoc = Some(line[j..end].into());
                    let close = if quote.is_some() && b.get(end) == quote.as_ref() {
                        end + 1
                    } else {
                        end
                    };
                    p.paint(i, close, c.constant);
                    i = close;
                } else {
                    i += 2;
                }
            }
            _ if ident_start(ch) => {
                let end = ident_end(b, i);
                let word = &line[i..end];
                if instruction == "from" && word.eq_ignore_ascii_case("as") {
                    p.paint(i, end, c.keyword);
                } else if key_value && b.get(end) == Some(&b'=') {
                    p.paint(i, end, c.variable);
                }
                i = end;
            }
            _ => i += 1,
        }
    }
    if let Some(terminator) = heredoc {
        *state = State::Heredoc(terminator);
    } else if line.trim_end().ends_with('\\') {
        *state = State::Continue;
    }
}

/// Past the variable reference `$NAME` or `${...}` at `i`; `i + 1` when it is neither.
fn docker_variable_end(b: &[u8], i: usize) -> usize {
    match b.get(i + 1) {
        Some(b'{') => find(b, i + 2, b"}").map_or(b.len(), |k| k + 1),
        Some(x) if ident_start(*x) => ident_end(b, i + 1),
        _ => i + 1,
    }
}

#[cfg(test)]
#[path = "scan_tests.rs"]
mod tests;
