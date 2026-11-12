//! Line-by-line highlighting for an editor, alongside the viewer's whole-block [`crate::tokens`].
//! It uses the same syntax set, the same two themes and the same language names, and changes
//! nothing the viewer calls.
//!
//! The parse state (syntect's `ParseState` and the `ScopeStack`, not the `HighlightState`) is
//! stored at the start of every K-th line, K = [`checkpoint_spacing`]; colours for any lines are
//! made by running forward from the checkpoint at or before them. Work is cut into time-boxed
//! calls that stop after the line during which the deadline passes. Numbers and limits come from
//! the editor's spike "highlight" (`markdown-editor-rs/spikes/highlight/RESULTS.md`).
//!
//! No threads and no text type: the text is read through [`Lines`], so this module needs no rope.

use super::{scope_color, syntax, syntaxes, theme};
use std::ops::Range;
use std::path::Path;
use std::time::Instant;
use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference};

/// What a file is highlighted as.
#[derive(Clone, Copy)]
pub enum Language {
    /// A syntax from the bundled set.
    Syntax(&'static SyntaxReference),
    /// TOML, which the bundled set lacks: a line-wise version of the viewer's TOML scanner, its
    /// state being the multi-line string still open.
    Toml,
}

/// A syntax worth highlighting with: not hidden, and not the plain-text one.
fn usable(syntax: Option<&'static SyntaxReference>) -> Option<Language> {
    syntax
        .filter(|s| !s.hidden && s.name != "Plain Text")
        .map(Language::Syntax)
}

impl Language {
    /// By file name and extension (`Cargo.toml`, `main.rs`, `Makefile`, `.ts` read as JavaScript
    /// as the viewer does), then by the first line (`#!/bin/sh`, `<?xml`). `None` is plain text.
    pub fn for_path(path: &Path, first_line: &str) -> Option<Language> {
        let set = syntaxes();
        let name = path.file_name().and_then(|n| n.to_str());
        let extension = path.extension().and_then(|e| e.to_str());
        if extension.is_some_and(|e| e.eq_ignore_ascii_case("toml")) || name == Some("Cargo.lock") {
            return Some(Language::Toml);
        }
        if let Some(found) = extension.and_then(|e| usable(syntax(set, e))) {
            return Some(found);
        }
        if let Some(found) = name.and_then(|n| usable(set.find_syntax_by_extension(n))) {
            return Some(found);
        }
        usable(set.find_syntax_by_first_line(first_line))
    }

    /// By a fence-style name ("rust", "ts", "toml"), with the viewer's aliases.
    pub fn by_token(token: &str) -> Option<Language> {
        if token.trim().eq_ignore_ascii_case("toml") {
            return Some(Language::Toml);
        }
        usable(syntax(syntaxes(), token))
    }

    /// The name for the status area: the syntax's own name ("Rust", "Markdown", "JavaScript"),
    /// or "TOML".
    pub fn name(self) -> &'static str {
        match self {
            Language::Syntax(syntax) => syntax.name.as_str(),
            Language::Toml => "TOML",
        }
    }

    pub fn limits(self) -> Limits {
        match self {
            Language::Syntax(syntax) if syntax.name == "Markdown" => Limits::MARKDOWN,
            _ => Limits::CODE,
        }
    }
}

impl PartialEq for Language {
    fn eq(&self, other: &Language) -> bool {
        match (self, other) {
            (Language::Syntax(a), Language::Syntax(b)) => std::ptr::eq(*a, *b),
            (Language::Toml, Language::Toml) => true,
            _ => false,
        }
    }
}

impl std::fmt::Debug for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Sizes over which text stays plain (the default foreground), from the spike's numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// A longer line is not parsed: it gets no spans and the parse state passes over it
    /// unchanged.
    pub max_line_chars: usize,
    /// A larger file is not highlighted at all.
    pub max_file_bytes: usize,
}

impl Limits {
    /// Code syntaxes: 1,000 characters a line, 2 MiB a file.
    pub const CODE: Limits = Limits {
        max_line_chars: 1_000,
        max_file_bytes: 2 << 20,
    };
    /// Markdown, whose syntect grammar is about eight times slower: 200 characters, 300 KiB.
    pub const MARKDOWN: Limits = Limits {
        max_line_chars: 200,
        max_file_bytes: 300 << 10,
    };

    pub fn allows_file(self, bytes: usize) -> bool {
        bytes <= self.max_file_bytes
    }
}

/// A coloured run of one line, in byte offsets of the line's text (line break excluded).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub color: [u8; 3],
}

/// The text being highlighted, read one line at a time.
pub trait Lines {
    fn len_lines(&self) -> usize;
    /// Replaces `out` with line `index`'s text, without its line break. The highlighter adds the
    /// `\n` the syntax set expects.
    fn line(&self, index: usize, out: &mut String);
}

/// The theme's plain text colour, for lines that are not (or not yet) coloured, so text does not
/// change colour when its spans arrive.
pub fn foreground(dark: bool) -> [u8; 3] {
    // The same fallback as `tokens`.
    theme(dark)
        .settings
        .foreground
        .map_or([40, 40, 40], |c| [c.r, c.g, c.b])
}

/// K: a checkpoint every 16 lines, more widely spaced in long files so that checkpoints stay
/// under about 4 MiB (K = max(16, lines / 3000)).
pub fn checkpoint_spacing(lines: usize) -> usize {
    (lines / 3000).max(16)
}

/// Colours of the line-wise TOML scanner, taken from the theme as the viewer's scanner does.
#[derive(Clone, Copy)]
struct TomlColors {
    plain: [u8; 3],
    comment: [u8; 3],
    string: [u8; 3],
    constant: [u8; 3],
    key: [u8; 3],
    table: [u8; 3],
}

impl TomlColors {
    fn new(dark: bool) -> TomlColors {
        let theme = theme(dark);
        TomlColors {
            plain: scope_color(theme, "source"),
            comment: scope_color(theme, "comment.line"),
            string: scope_color(theme, "string.quoted"),
            constant: scope_color(theme, "constant.numeric"),
            key: scope_color(theme, "entity.name.tag"),
            table: scope_color(theme, "keyword.control"),
        }
    }
}

/// One line of TOML. The colours of the line's bytes equal what the viewer's whole-text scanner
/// gives them; `open` is the multi-line string delimiter still open at the start of the line, and
/// at the end of it.
fn toml_line(
    colors: &TomlColors,
    line: &str,
    open: &mut Option<&'static str>,
    out: &mut Vec<Span>,
) {
    let end = line.len();
    let bytes = line.as_bytes();
    let mut push = |start: usize, stop: usize, color: [u8; 3]| {
        if start >= stop {
            return;
        }
        match out.last_mut() {
            Some(last) if last.color == color && last.end as usize == start => {
                last.end = stop as u32;
            }
            _ => out.push(Span {
                start: start as u32,
                end: stop as u32,
                color,
            }),
        }
    };
    let mut value = false;
    if open.is_none() {
        if line.trim_start().starts_with('[') {
            // A table header, up to a trailing comment.
            let stop = line.find('#').unwrap_or(end);
            push(0, stop, colors.table);
            push(stop, end, colors.comment);
            return;
        }
    } else {
        value = true;
    }
    let mut at = 0;
    while at < end {
        if let Some(delimiter) = *open {
            match line[at..end].find(delimiter) {
                Some(i) => {
                    push(at, at + i + delimiter.len(), colors.string);
                    at += i + delimiter.len();
                    *open = None;
                }
                None => {
                    push(at, end, colors.string);
                    at = end;
                }
            }
            continue;
        }
        let c = bytes[at];
        if c == b'#' {
            push(at, end, colors.comment);
            break;
        }
        if c == b'"' || c == b'\'' {
            let triple: &'static str = if c == b'"' { "\"\"\"" } else { "'''" };
            if line[at..end].starts_with(triple) {
                push(at, at + 3, colors.string);
                at += 3;
                *open = Some(triple);
                continue;
            }
            // To the closing quote on this line; a basic string may escape one.
            let mut stop = at + 1;
            while stop < end && bytes[stop] != c {
                stop += if c == b'"' && bytes[stop] == b'\\' {
                    2
                } else {
                    1
                };
            }
            let stop = (stop + 1).min(end);
            push(at, stop, if value { colors.string } else { colors.key });
            at = stop;
            continue;
        }
        if c == b'=' && !value {
            value = true;
            push(at, at + 1, colors.plain);
            at += 1;
            continue;
        }
        let word =
            |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'+' | b'.' | b':');
        if word(c) {
            let mut stop = at;
            while stop < end && word(bytes[stop]) {
                stop += 1;
            }
            let text = &line[at..stop];
            let color = if !value {
                colors.key
            } else if matches!(text, "true" | "false" | "inf" | "nan")
                || text.as_bytes()[0].is_ascii_digit()
                || matches!(text.as_bytes()[0], b'+' | b'-')
            {
                colors.constant
            } else {
                colors.plain
            };
            push(at, stop, color);
            at = stop;
            continue;
        }
        // Punctuation and spaces, one character at a time so UTF-8 stays whole.
        let len = line[at..end].chars().next().map_or(1, char::len_utf8);
        push(at, at + len, colors.plain);
        at += len;
    }
}

/// The parse state at the start of a line. A stored checkpoint is this without `hl`.
#[derive(Clone)]
enum Run {
    Syntax {
        parse: ParseState,
        scopes: ScopeStack,
        /// The highlight state for `scopes`, when it is known to match them. It is rebuilt from
        /// `scopes` when absent, and dropped by lines run only to advance the parse.
        hl: Option<HighlightState>,
    },
    Toml {
        open: Option<&'static str>,
    },
}

impl Run {
    fn initial(language: Language) -> Run {
        match language {
            Language::Syntax(syntax) => Run::Syntax {
                parse: ParseState::new(syntax),
                scopes: ScopeStack::new(),
                hl: None,
            },
            Language::Toml => Run::Toml { open: None },
        }
    }

    /// What a checkpoint stores.
    fn snapshot(&self) -> Run {
        match self {
            Run::Syntax { parse, scopes, .. } => Run::Syntax {
                parse: parse.clone(),
                scopes: scopes.clone(),
                hl: None,
            },
            Run::Toml { open } => Run::Toml { open: *open },
        }
    }

    fn forget_colors(&mut self) {
        if let Run::Syntax { hl, .. } = self {
            *hl = None;
        }
    }
}

/// Everything that decides how a line is run but is not state of the text.
struct Engine {
    language: Language,
    dark: bool,
    max_line_chars: usize,
    highlighter: Highlighter<'static>,
    toml: TomlColors,
    foreground: [u8; 3],
}

impl Engine {
    fn new(language: Language, dark: bool) -> Engine {
        Engine {
            language,
            dark,
            max_line_chars: language.limits().max_line_chars,
            highlighter: Highlighter::new(theme(dark)),
            toml: TomlColors::new(dark),
            foreground: foreground(dark),
        }
    }

    fn set_dark(&mut self, dark: bool) {
        self.dark = dark;
        self.highlighter = Highlighter::new(theme(dark));
        self.toml = TomlColors::new(dark);
        self.foreground = foreground(dark);
    }

    /// Runs one line, whose text is in `buf` without its break. Spans are made when `out` is
    /// given. A line over the limit leaves `run` as it was, and so does one syntect fails on;
    /// both come out plain.
    fn step(&self, run: &mut Run, buf: &mut String, out: Option<&mut Vec<Span>>) {
        let len = buf.len();
        if len > self.max_line_chars && buf.chars().count() > self.max_line_chars {
            return;
        }
        match run {
            Run::Toml { open } => {
                let mut scratch = Vec::new();
                toml_line(&self.toml, buf, open, out.unwrap_or(&mut scratch));
            }
            Run::Syntax { parse, scopes, hl } => {
                let backup = parse.clone();
                buf.push('\n');
                let ops = match parse.parse_line(buf, syntaxes()) {
                    Ok(ops) => ops,
                    Err(_) => {
                        *parse = backup;
                        buf.pop();
                        return;
                    }
                };
                if let Some(out) = out {
                    let state = hl.get_or_insert_with(|| {
                        HighlightState::new(&self.highlighter, scopes.clone())
                    });
                    let mut pos = 0;
                    let mut push = |start: usize, stop: usize, color: [u8; 3]| {
                        if start >= stop {
                            return;
                        }
                        match out.last_mut() {
                            Some(last) if last.color == color => last.end = stop as u32,
                            _ => out.push(Span {
                                start: start as u32,
                                end: stop as u32,
                                color,
                            }),
                        }
                    };
                    for (style, piece) in
                        HighlightIterator::new(state, &ops, buf, &self.highlighter)
                    {
                        let start = pos;
                        pos += piece.len();
                        let stop = pos.min(len);
                        let c = style.foreground;
                        push(start, stop, [c.r, c.g, c.b]);
                    }
                    // A parser that leaves a gap at the end keeps the text readable.
                    push(pos.min(len), len, self.foreground);
                } else {
                    *hl = None;
                }
                for (_, op) in &ops {
                    let _ = scopes.apply(op);
                }
                buf.pop();
            }
        }
    }
}

/// The highlighting state of one text.
pub struct LineHighlighter {
    engine: Engine,
    plain: bool,
    total_lines: usize,
    /// K, fixed when the highlighter is made.
    spacing: usize,
    /// The state at the start of line `i * spacing`, for every such line up to the frontier.
    checkpoints: Vec<Run>,
    /// The state has reached the start of this line.
    frontier: usize,
    /// The state at `frontier`; `None` only while a call is running, or when plain.
    run: Option<Run>,
    buf: String,
}

impl LineHighlighter {
    /// `total_lines` and `total_bytes` describe the text; a text over the language's
    /// `max_file_bytes` is plain from the start ([`is_plain`](Self::is_plain)).
    pub fn new(
        language: Language,
        dark: bool,
        total_lines: usize,
        total_bytes: usize,
    ) -> LineHighlighter {
        let plain = !language.limits().allows_file(total_bytes);
        let (checkpoints, run) = if plain {
            (Vec::new(), None)
        } else {
            let run = Run::initial(language);
            (vec![run.snapshot()], Some(run))
        };
        LineHighlighter {
            engine: Engine::new(language, dark),
            plain,
            total_lines,
            spacing: checkpoint_spacing(total_lines),
            checkpoints,
            frontier: 0,
            run,
            buf: String::new(),
        }
    }

    pub fn language(&self) -> Language {
        self.engine.language
    }

    pub fn dark(&self) -> bool {
        self.engine.dark
    }

    /// The file is over its limit: nothing will be coloured, and the calls below return at once.
    pub fn is_plain(&self) -> bool {
        self.plain
    }

    /// Switches the theme. Checkpoints are kept (they hold no colours); spans made from now on
    /// use the new theme.
    pub fn set_dark(&mut self, dark: bool) {
        if self.engine.dark == dark {
            return;
        }
        self.engine.set_dark(dark);
        if let Some(run) = &mut self.run {
            run.forget_colors();
        }
    }

    /// The parse has reached the start of this line: spans can be made for any range starting at
    /// or before it without running lines above.
    pub fn frontier(&self) -> usize {
        if self.plain {
            self.total_lines
        } else {
            self.frontier
        }
    }

    /// The parse has reached the end of the text.
    pub fn done(&self) -> bool {
        self.plain || self.frontier >= self.total_lines
    }

    /// The number of stored checkpoints (one per K lines passed).
    pub fn checkpoint_count(&self) -> usize {
        self.checkpoints.len()
    }

    /// K, the lines between checkpoints.
    pub fn spacing(&self) -> usize {
        self.spacing
    }

    /// Runs line `pos` with `run`, which is the state at its start. When `lead`, the state is the
    /// frontier's, so the frontier moves on and a checkpoint is stored at a multiple of K.
    fn run_line<L: Lines + ?Sized>(
        &mut self,
        src: &L,
        run: &mut Run,
        pos: usize,
        lead: bool,
        out: Option<&mut Vec<Span>>,
    ) {
        src.line(pos, &mut self.buf);
        self.engine.step(run, &mut self.buf, out);
        if lead {
            let next = pos + 1;
            self.frontier = next;
            if next < self.total_lines
                && next.is_multiple_of(self.spacing)
                && next / self.spacing == self.checkpoints.len()
            {
                self.checkpoints.push(run.snapshot());
            }
        }
    }

    /// Runs the parse forward from the frontier, storing checkpoints, until the end of the text
    /// or until a line finishes after `deadline`. True while there is more to do.
    pub fn advance<L: Lines + ?Sized>(&mut self, src: &L, deadline: Instant) -> bool {
        if self.done() {
            return false;
        }
        let mut run = self
            .run
            .take()
            .expect("a highlighter that is not plain has a state");
        while self.frontier < self.total_lines {
            let pos = self.frontier;
            self.run_line(src, &mut run, pos, true, None);
            if Instant::now() >= deadline {
                break;
            }
        }
        self.run = Some(run);
        !self.done()
    }

    /// Spans for each line of `lines` (clamped to the text). `None` when `lines.start` is past
    /// the frontier, or when `deadline` passed first (checkpoints made on the way are kept, so the
    /// next call starts further on). A line's spans are sorted, do not overlap and cover its text
    /// from 0 to its length; an empty `Vec` is a plain line (over the line limit, or no colour
    /// change), drawn in [`foreground`].
    pub fn spans<L: Lines + ?Sized>(
        &mut self,
        src: &L,
        lines: Range<usize>,
        deadline: Instant,
    ) -> Option<Vec<Vec<Span>>> {
        let start = lines.start.min(self.total_lines);
        let end = lines.end.min(self.total_lines).max(start);
        if self.plain {
            return Some(vec![Vec::new(); end - start]);
        }
        if start > self.frontier {
            return None;
        }
        if start == end {
            return Some(Vec::new());
        }
        let (mut pos, mut run, mut lead) = if start == self.frontier {
            let run = self
                .run
                .take()
                .expect("a highlighter that is not plain has a state");
            (start, run, true)
        } else {
            let at = start / self.spacing;
            (at * self.spacing, self.checkpoints[at].clone(), false)
        };
        let mut result = Vec::with_capacity(end - start);
        let mut finished = true;
        while pos < end {
            if pos == self.frontier {
                lead = true;
            }
            if pos >= start {
                let mut line = Vec::new();
                self.run_line(src, &mut run, pos, lead, Some(&mut line));
                result.push(line);
            } else {
                self.run_line(src, &mut run, pos, lead, None);
            }
            pos += 1;
            if pos < end && Instant::now() >= deadline {
                finished = false;
                break;
            }
        }
        if lead {
            self.run = Some(run);
        }
        finished.then_some(result)
    }

    /// The text changed from `line` on and now has `total_lines` lines (`line_delta` more than
    /// before, negative for fewer). Checkpoints after `line` are dropped, and the frontier moves
    /// back to the last one kept. Used by editing (phase 2); tested from phase 1.
    ///
    /// Checkpoints are not shifted: lines after the change are run again, which keeps the stored
    /// states exact. Comparing with shifted old states to stop early is left for phase 2.
    pub fn invalidate_from(&mut self, line: usize, total_lines: usize, line_delta: isize) {
        let _ = line_delta;
        self.total_lines = total_lines;
        if self.plain {
            return;
        }
        if self.frontier > line {
            // The state at the start of `line` is still right, so a checkpoint at `line` stays.
            let keep = (line / self.spacing + 1).min(self.checkpoints.len());
            self.checkpoints.truncate(keep);
            let last = self.checkpoints.len() - 1;
            self.frontier = last * self.spacing;
            self.run = Some(self.checkpoints[last].clone());
        }
        if self.frontier > total_lines {
            // The text became shorter than the state's position (a checkpoint past its end).
            let keep = (total_lines / self.spacing + 1).min(self.checkpoints.len());
            self.checkpoints.truncate(keep);
            let last = self.checkpoints.len() - 1;
            self.frontier = last * self.spacing;
            self.run = Some(self.checkpoints[last].clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens;
    use std::time::Duration;

    struct Text(Vec<String>);

    impl Text {
        fn new(source: &str) -> Text {
            Text(source.split('\n').map(str::to_owned).collect())
        }
    }

    impl Lines for Text {
        fn len_lines(&self) -> usize {
            self.0.len()
        }
        fn line(&self, index: usize, out: &mut String) {
            out.clear();
            out.push_str(&self.0[index]);
        }
    }

    fn far() -> Instant {
        Instant::now() + Duration::from_secs(120)
    }

    fn rust() -> Language {
        Language::by_token("rust").unwrap()
    }

    fn highlighter(language: Language, dark: bool, text: &Text) -> LineHighlighter {
        let bytes = text.0.iter().map(|l| l.len() + 1).sum();
        LineHighlighter::new(language, dark, text.len_lines(), bytes)
    }

    fn all_spans(language: Language, dark: bool, text: &Text) -> Vec<Vec<Span>> {
        let mut h = highlighter(language, dark, text);
        h.spans(text, 0..text.len_lines(), far()).unwrap()
    }

    /// The colour of each byte of a line, the foreground where there are no spans.
    fn byte_colors(spans: &[Span], len: usize, fg: [u8; 3]) -> Vec<[u8; 3]> {
        let mut out = vec![fg; len];
        let mut at = 0;
        for span in spans {
            assert_eq!(span.start as usize, at, "contiguous: {spans:?}");
            assert!(span.end > span.start, "not empty: {spans:?}");
            out[span.start as usize..span.end as usize].fill(span.color);
            at = span.end as usize;
        }
        if !spans.is_empty() {
            assert_eq!(at, len, "covers the line: {spans:?}");
        }
        out
    }

    /// Every byte of the text must get the colour `tokens` gives it.
    fn assert_matches_tokens(source: &str, name: &str, dark: bool) {
        let language = Language::by_token(name).unwrap();
        let text = Text::new(source);
        let spans = all_spans(language, dark, &text);
        let expected = tokens(source, Some(name), dark).unwrap();
        let mut offset = 0;
        for (index, line) in text.0.iter().enumerate() {
            let got = byte_colors(&spans[index], line.len(), foreground(dark));
            for (i, color) in got.iter().enumerate() {
                let at = offset + i;
                let token = expected.iter().find(|t| t.range.contains(&at)).unwrap();
                assert_eq!(
                    *color, token.color,
                    "{name} dark={dark} line {index} byte {i} of {line:?}"
                );
            }
            offset += line.len() + 1;
        }
    }

    const RUST: &str = "//! doc\nuse std::fmt;\n\n/* block\n   comment */\nstruct P<'a> { x: &'a str, n: u32 }\n\nimpl P<'_> {\n    fn go(&self) -> String {\n        let s = r#\"raw \"quoted\" text\"#;\n        let t = \"a\\\"b\";\n        println!(\"{} {}\", s, self.n + 0x1f);\n        format!(\"{t}\") // tail\n    }\n}\n";
    const MARKDOWN: &str = "# Title\n\nSome *emphasis*, **bold**, `code` and a [link](http://x.y).\n\n- one\n- two\n  - nested\n\n> quote\n\n```rust\nfn main() { let x = 1; }\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```json\n{\"k\": [1, true, null]}\n```\n\nEnd.\n";
    const JSON: &str = "{\n  \"name\": \"x\",\n  \"list\": [1, 2.5, true, null],\n  \"nested\": {\n    \"a\": \"b\\n\"\n  }\n}\n";
    const PYTHON: &str = "import os\n\n@decorator\ndef f(a, b=2):\n    \"\"\"Doc\n    string.\"\"\"\n    x = [i for i in range(10) if i % 2]\n    return f'{a}' + \"s\"  # note\n\nclass C(object):\n    pass\n";

    #[test]
    fn a_line_by_line_pass_gives_the_viewers_colours() {
        for dark in [false, true] {
            assert_matches_tokens(RUST, "rust", dark);
            assert_matches_tokens(MARKDOWN, "markdown", dark);
            assert_matches_tokens(JSON, "json", dark);
            assert_matches_tokens(PYTHON, "python", dark);
        }
    }

    fn generated_rust(lines: usize) -> String {
        let mut s = String::new();
        let mut count = 0;
        let mut n = 0;
        while count < lines {
            let piece = match n % 7 {
                0 => format!("/// Doc for item {n}\n"),
                1 => format!("pub fn item_{n}(a: u32, b: &str) -> u32 {{\n"),
                2 => format!("    let text = \"value {n}\"; // note\n"),
                3 => "    /* a\n       block */\n".to_string(),
                4 => format!("    if a > {n} {{ return a * 2 + b.len() as u32; }}\n"),
                5 => "    let raw = r#\"x \"y\" z\"#;\n    a\n}\n".to_string(),
                _ => format!("struct S{n} {{ field: Vec<u8> }}\n\n"),
            };
            count += piece.matches('\n').count();
            s.push_str(&piece);
            n += 1;
        }
        s
    }

    #[test]
    fn a_checkpoint_resume_equals_one_pass() {
        let source = generated_rust(5000);
        let text = Text::new(&source);
        for dark in [false, true] {
            let mut h = highlighter(rust(), dark, &text);
            while h.advance(&text, far()) {}
            assert!(h.done());
            let resumed = h.spans(&text, 3000..3060, far()).unwrap();
            let mut fresh = highlighter(rust(), dark, &text);
            let whole = fresh.spans(&text, 0..3060, far()).unwrap();
            assert_eq!(resumed.len(), 60);
            assert_eq!(resumed[..], whole[3000..]);
            assert!(resumed.iter().any(|l| l.len() > 1));
            // A start that is not a multiple of K.
            let odd = h.spans(&text, 3001..3002, far()).unwrap();
            assert_eq!(odd[0], whole[3001]);
        }
        assert_eq!(checkpoint_spacing(5000), 16);
        assert_eq!(checkpoint_spacing(300_000), 100);
        assert_eq!(checkpoint_spacing(10), 16);
        let h = highlighter(rust(), false, &text);
        assert_eq!(h.spacing(), 16);
    }

    #[test]
    fn checkpoints_are_stored_at_multiples_of_k() {
        let source = generated_rust(200);
        let text = Text::new(&source);
        let mut h = highlighter(rust(), false, &text);
        assert_eq!(h.checkpoint_count(), 1);
        while h.advance(&text, far()) {}
        assert_eq!(h.checkpoint_count(), (text.len_lines() - 1) / 16 + 1);
    }

    #[test]
    fn a_line_over_the_limit_is_plain_and_leaves_the_state() {
        let long = format!("    let v = {};", "1 + ".repeat(300) + "1");
        assert!(long.chars().count() > 1000);
        let with = Text(vec![
            "fn main() {".into(),
            long,
            "    let y = \"s\"; // c".into(),
            "}".into(),
        ]);
        let without = Text(vec![
            "fn main() {".into(),
            "    let y = \"s\"; // c".into(),
            "}".into(),
        ]);
        let a = all_spans(rust(), false, &with);
        let b = all_spans(rust(), false, &without);
        assert!(a[1].is_empty());
        assert!(!b[1].is_empty());
        assert_eq!(a[0], b[0]);
        assert_eq!(a[2], b[1]);
        assert_eq!(a[3], b[2]);
        // Characters are counted, not bytes: 600 two-byte characters are within the limit.
        let wide = Text(vec![format!("// {}", "é".repeat(600))]);
        assert!(wide.0[0].len() > 1000);
        assert!(!all_spans(rust(), false, &wide)[0].is_empty());
    }

    #[test]
    fn a_file_over_the_limit_is_plain() {
        let mut h = LineHighlighter::new(rust(), false, 10, Limits::CODE.max_file_bytes + 1);
        assert!(h.is_plain());
        assert!(h.done());
        let text = Text::new(&"a\n".repeat(9));
        assert!(!h.advance(&text, far()));
        let spans = h.spans(&text, 2..7, far()).unwrap();
        assert_eq!(spans, vec![Vec::<Span>::new(); 5]);
        assert_eq!(h.frontier(), 10);
        let md = Language::by_token("markdown").unwrap();
        assert!(!LineHighlighter::new(md, false, 10, 300 << 10).is_plain());
        assert!(LineHighlighter::new(md, false, 10, (300 << 10) + 1).is_plain());
    }

    #[test]
    fn set_dark_changes_colours_and_keeps_the_frontier() {
        let source = generated_rust(100);
        let text = Text::new(&source);
        let mut h = highlighter(rust(), false, &text);
        let light = h.spans(&text, 0..40, far()).unwrap();
        let frontier = h.frontier();
        assert_eq!(frontier, 40);
        h.set_dark(true);
        assert!(h.dark());
        assert_eq!(h.frontier(), frontier);
        let dark = h.spans(&text, 0..40, far()).unwrap();
        assert_ne!(light, dark);
        let fresh = all_spans(rust(), true, &text);
        assert_eq!(dark[..], fresh[..40]);
        // From the frontier itself, whose running state held the light theme's styles.
        let mut h = highlighter(rust(), false, &text);
        h.spans(&text, 0..40, far()).unwrap();
        h.set_dark(true);
        let next = h.spans(&text, 40..60, far()).unwrap();
        assert_eq!(next[..], fresh[40..60]);
    }

    type Edit = Box<dyn Fn(&mut Vec<String>)>;

    #[test]
    fn invalidate_then_rerun_equals_a_fresh_highlighter() {
        let source = generated_rust(400);
        let old = Text::new(&source);
        let cases: Vec<(usize, Edit, isize)> = vec![
            (100, Box::new(|v| v[100] = "    let q = \"open".into()), 0),
            (
                100,
                Box::new(|v| {
                    v[100] = "/* start".into();
                    v.insert(101, "more".into());
                }),
                1,
            ),
            (
                37,
                Box::new(|v| {
                    v.drain(37..40);
                }),
                -3,
            ),
            (16, Box::new(|v| v[16] = "fn x() {".into()), 0),
            (5, Box::new(|v| v.insert(5, "\"\"\"".into())), 1),
            (399, Box::new(|v| v.push("tail".into())), 1),
        ];
        for (line, edit, delta) in cases {
            let mut h = highlighter(rust(), false, &old);
            while h.advance(&old, far()) {}
            let mut lines = old.0.clone();
            edit(&mut lines);
            let new = Text(lines);
            assert_eq!(new.0.len() as isize - old.0.len() as isize, delta);
            h.invalidate_from(line, new.len_lines(), delta);
            assert!(h.frontier() <= line);
            assert_eq!(h.frontier() % 16, 0);
            let rerun = h.spans(&new, 0..new.len_lines(), far()).unwrap();
            assert_eq!(rerun, all_spans(rust(), false, &new), "edit at {line}");
            assert!(h.done());
        }
        // A partly run highlighter keeps what it has.
        let mut h = highlighter(rust(), false, &old);
        h.spans(&old, 0..50, far()).unwrap();
        h.invalidate_from(200, old.len_lines(), 0);
        assert_eq!(h.frontier(), 50);
    }

    #[test]
    fn deadlines_stop_after_one_line_and_keep_progress() {
        let source = generated_rust(300);
        let text = Text::new(&source);
        let mut h = highlighter(rust(), false, &text);
        let past = Instant::now();
        assert!(h.advance(&text, past));
        assert_eq!(h.frontier(), 1);
        assert!(h.spans(&text, 0..10, past).is_none());
        // Run from a checkpoint behind the frontier: nothing new is learnt.
        assert_eq!(h.frontier(), 1);
        // Past the frontier: nothing runs.
        assert!(h.spans(&text, 5..10, past).is_none());
        assert_eq!(h.frontier(), 1);
        // Each call from the frontier moves one line on and then stops.
        for _ in 0..3 {
            let f = h.frontier();
            assert!(h.spans(&text, f..f + 3, past).is_none());
            assert_eq!(h.frontier(), f + 1);
        }
        let f = h.frontier();
        assert_eq!(h.spans(&text, f..f + 1, past).unwrap().len(), 1);
        assert_eq!(h.frontier(), f + 1);
        let fresh = all_spans(rust(), false, &text);
        let f = h.frontier();
        assert_eq!(
            h.spans(&text, f - 3..f, far()).unwrap()[..],
            fresh[f - 3..f]
        );
    }

    const TOML: &str = "# comment\n[package]\nname = \"mdview\" # trailing\nedition = 2024\ntext = \"\"\"\nmany # not a comment\n\"\"\"\n\u{f6}n = true\n";
    const TOML_MULTI: &str = "title = 'TOML'\nliteral = '''\nline # one \"q\"\n  [not.a.table]\nend''' # after\nbasic = \"\"\"\nhas \\\"escape\\\" and \"\"quotes\"\" \"\"\" \nrest = [1, 2.5e3, -4, +5, 0x1F, 1979-05-27T07:32:00Z]\nlist = [\n  \"a\", 'b', # inner\n]\n[[arr.of.tables]] # trailing\n  key.sub = { x = true, y = \"s\\\\\" }\nbad = \"unterminated\\\nkey with space = inf\n\u{fc}ml = \"\u{e7}\u{15f}\" # t\u{fc}rk\u{e7}e\n\n";

    #[test]
    fn toml_line_wise_equals_the_viewers_scanner() {
        for source in [TOML, TOML_MULTI] {
            for dark in [false, true] {
                let text = Text::new(source);
                let spans = all_spans(Language::Toml, dark, &text);
                let expected = tokens(source, Some("toml"), dark).unwrap();
                let mut offset = 0;
                for (index, line) in text.0.iter().enumerate() {
                    let got = byte_colors(&spans[index], line.len(), [0; 3]);
                    for (i, color) in got.iter().enumerate() {
                        let token = expected
                            .iter()
                            .find(|t| t.range.contains(&(offset + i)))
                            .unwrap();
                        assert_eq!(*color, token.color, "line {index} byte {i} of {line:?}");
                    }
                    offset += line.len() + 1;
                }
            }
        }
        // And from a checkpoint inside a multi-line string.
        let text = Text::new(&TOML_MULTI.repeat(20));
        let mut h = highlighter(Language::Toml, false, &text);
        while h.advance(&text, far()) {}
        let all = all_spans(Language::Toml, false, &text);
        for start in [17, 33, 49] {
            let got = h.spans(&text, start..start + 6, far()).unwrap();
            assert_eq!(got[..], all[start..start + 6]);
        }
    }

    #[test]
    fn languages_are_found_by_path_and_first_line() {
        let find = |p: &str, first: &str| Language::for_path(Path::new(p), first);
        assert_eq!(find("main.rs", ""), Language::by_token("rust"));
        assert_eq!(find("src/main.rs", "").unwrap().name(), "Rust");
        assert_eq!(find("README.md", "").unwrap().name(), "Markdown");
        assert_eq!(find("Cargo.toml", ""), Some(Language::Toml));
        assert_eq!(find("Cargo.lock", ""), Some(Language::Toml));
        assert_eq!(find("a.TOML", ""), Some(Language::Toml));
        for ts in ["x.ts", "x.tsx", "x.jsx", "x.mjs", "x.cjs", "x.js"] {
            assert_eq!(find(ts, "").unwrap().name(), "JavaScript", "{ts}");
        }
        assert_eq!(find("script.py", "").unwrap().name(), "Python");
        let bash = find("deploy", "#!/bin/bash").unwrap();
        assert!(
            bash.name().to_lowercase().contains("bash"),
            "{}",
            bash.name()
        );
        assert_eq!(find("notes.txt", ""), None);
        assert_eq!(find("notes.txt", "just text"), None);
        assert_eq!(find("plain", "hello"), None);
        assert_eq!(find("Makefile", "").unwrap().name(), "Makefile");
        assert_eq!(Language::Toml.name(), "TOML");
        assert_eq!(Language::Toml.limits(), Limits::CODE);
        assert_eq!(find("a.md", "").unwrap().limits(), Limits::MARKDOWN);
        assert_eq!(find("a.rs", "").unwrap().limits(), Limits::CODE);
        assert_eq!(Language::by_token("ts"), Language::by_token("js"));
        assert_eq!(Language::by_token("TOML"), Some(Language::Toml));
        assert_eq!(Language::by_token("nope"), None);
        assert_eq!(Language::by_token("text"), None);
    }

    #[test]
    fn foregrounds_are_the_themes_and_differ() {
        assert_ne!(foreground(false), foreground(true));
    }

    #[test]
    fn spans_are_contiguous_and_on_char_boundaries_for_turkish_and_cjk() {
        let source = "let ad = \"\u{130}stanbul'da \u{e7}ok g\u{fc}zel \u{15f}ehir \u{131}\u{11f}\u{15f}\"; // t\u{fc}rk\u{e7}e yorum\nlet \u{540d}\u{524d} = \"\u{65e5}\u{672c}\u{8a9e}\u{306e}\u{30c6}\u{30ad}\u{30b9}\u{30c8}\u{3001}\u{6f22}\u{5b57}\"; // \u{30b3}\u{30e1}\u{30f3}\u{30c8} \u{4e2d}\u{6587}\nfn \u{95a2}\u{6570}() -> u8 { '\u{fc}' as u8 }\n\"\u{1c5}\" + \"\u{1f600} emoji\"\n";
        let text = Text::new(source);
        for language in [
            rust(),
            Language::by_token("python").unwrap(),
            Language::Toml,
        ] {
            let spans = all_spans(language, false, &text);
            for (line, spans) in text.0.iter().zip(&spans) {
                let mut at = 0;
                for span in spans {
                    assert_eq!(span.start as usize, at);
                    assert!(span.end > span.start);
                    assert!(
                        line.is_char_boundary(span.start as usize),
                        "{line:?} {span:?}"
                    );
                    assert!(
                        line.is_char_boundary(span.end as usize),
                        "{line:?} {span:?}"
                    );
                    at = span.end as usize;
                }
                if !line.is_empty() {
                    assert_eq!(at, line.len(), "{language:?} {line:?}");
                }
            }
        }
    }

    #[test]
    fn empty_and_one_line_texts_work() {
        let text = Text::new("");
        let mut h = highlighter(rust(), false, &text);
        assert_eq!(
            h.spans(&text, 0..5, far()).unwrap(),
            vec![Vec::<Span>::new()]
        );
        assert!(h.done());
        assert!(!h.advance(&text, far()));
        assert_eq!(h.spans(&text, 7..9, far()), Some(Vec::new()));
    }
}
