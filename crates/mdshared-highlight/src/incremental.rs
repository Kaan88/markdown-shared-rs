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

use std::ops::Range;
use std::path::Path;
use std::time::Instant;
use syntect::parsing::SyntaxReference;

/// What a file is highlighted as.
#[derive(Clone, Copy)]
pub enum Language {
    /// A syntax from the bundled set.
    Syntax(&'static SyntaxReference),
    /// TOML, which the bundled set lacks: a line-wise version of the viewer's TOML scanner, its
    /// state being the multi-line string still open.
    Toml,
}

impl Language {
    /// By file name and extension (`Cargo.toml`, `main.rs`, `Makefile`, `.ts` read as JavaScript
    /// as the viewer does), then by the first line (`#!/bin/sh`, `<?xml`). `None` is plain text.
    pub fn for_path(path: &Path, first_line: &str) -> Option<Language> {
        let _ = (path, first_line);
        todo!()
    }

    /// By a fence-style name ("rust", "ts", "toml"), with the viewer's aliases.
    pub fn by_token(token: &str) -> Option<Language> {
        let _ = token;
        todo!()
    }

    /// The name for the status area: the syntax's own name ("Rust", "Markdown", "JavaScript"),
    /// or "TOML".
    pub fn name(self) -> &'static str {
        todo!()
    }

    pub fn limits(self) -> Limits {
        todo!()
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
    let _ = dark;
    todo!()
}

/// K: a checkpoint every 16 lines, more widely spaced in long files so that checkpoints stay
/// under about 4 MiB (K = max(16, lines / 3000)).
pub fn checkpoint_spacing(lines: usize) -> usize {
    (lines / 3000).max(16)
}

/// The highlighting state of one text.
pub struct LineHighlighter {
    language: Language,
    dark: bool,
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
        let _ = (total_lines, total_bytes);
        LineHighlighter { language, dark }
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn dark(&self) -> bool {
        self.dark
    }

    /// The file is over its limit: nothing will be coloured, and the calls below return at once.
    pub fn is_plain(&self) -> bool {
        todo!()
    }

    /// Switches the theme. Checkpoints are kept (they hold no colours); spans made from now on
    /// use the new theme.
    pub fn set_dark(&mut self, dark: bool) {
        let _ = dark;
        todo!()
    }

    /// The parse has reached the start of this line: spans can be made for any range starting at
    /// or before it without running lines above.
    pub fn frontier(&self) -> usize {
        todo!()
    }

    /// The parse has reached the end of the text.
    pub fn done(&self) -> bool {
        todo!()
    }

    /// Runs the parse forward from the frontier, storing checkpoints, until the end of the text
    /// or until a line finishes after `deadline`. True while there is more to do.
    pub fn advance<L: Lines + ?Sized>(&mut self, src: &L, deadline: Instant) -> bool {
        let _ = (src, deadline);
        todo!()
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
        let _ = (src, lines, deadline);
        todo!()
    }

    /// The text changed from `line` on and now has `total_lines` lines (`line_delta` more than
    /// before, negative for fewer). Checkpoints after `line` are dropped or shifted, and the
    /// frontier moves back to `line`. Used by editing (phase 2); tested from phase 1.
    pub fn invalidate_from(&mut self, line: usize, total_lines: usize, line_delta: isize) {
        let _ = (line, total_lines, line_delta);
        todo!()
    }
}
