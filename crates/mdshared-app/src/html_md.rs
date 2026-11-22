//! HTML to Markdown, for a paste of rich text: what a web page, a word processor or mdview put on
//! the clipboard becomes Markdown text. Headings, bold, italic, strikethrough, links, images,
//! lists (nested, numbered or not), inline code, code blocks, block quotes, rules and paragraphs
//! are written as their Markdown; any other tag keeps its text. A table is not made a Markdown
//! table: each row becomes one line of its cells' text joined by ` | `, whatever blocks the cells
//! hold.
//!
//! Hand-written for this one job, with no HTML library: a small tokenizer and a writer that keeps
//! the blocks it is in. It reads anything (clipboard contents are untrusted) and has no path that
//! can panic: every slice is taken with `get`, the depth of nesting it honours is capped, and the
//! output never passes [`cap`] of the input's length: past it the answer is `None`, and the paste
//! takes the plain text. Its time grows in step with the input.

/// Lists and quotes nested deeper than this are written at this depth.
const MAX_DEPTH: usize = 16;
/// Open inline elements (bold, links …) followed at once; more are kept as text.
const MAX_INLINE: usize = 64;

/// The most Markdown written for `len` bytes of HTML: eight times as much, and 64 KiB. Deep
/// nesting repeats its prefixes on every line; past this the input is not a document.
pub fn cap(len: usize) -> usize {
    len.saturating_mul(8).saturating_add(1 << 16)
}

/// The Markdown for an HTML fragment or document, or `None` when it has nothing Markdown can say
/// that its plain text does not: no heading, emphasis, link, image, list, quote, code or rule. A
/// paste then takes the clipboard's plain text, as it would without the conversion. Also `None`
/// for HTML that lays its text out as written (`white-space: pre` outside a `<pre>`, as code
/// editors copy their coloured text): its plain text is the better paste. And `None` when the
/// Markdown would be longer than [`cap`] of the HTML's length.
pub fn to_markdown(html: &str) -> Option<String> {
    let cap = cap(html.len());
    let mut w = Writer {
        cap,
        ..Writer::default()
    };
    let mut tokens = Tokens { s: html, at: 0 };
    while let Some(t) = tokens.next() {
        if w.over || w.out.len().saturating_add(w.line.len()) > cap {
            return None;
        }
        match t {
            Token::Text(t) => w.text(t),
            Token::Start {
                name,
                attrs,
                closed,
            } => {
                // A code block's own style is not a code editor's copy.
                if w.pre == 0
                    && !matches!(name.as_str(), "pre" | "code")
                    && attrs.style().is_some_and(|s| preformatted(&s))
                {
                    return None;
                }
                if is_raw(&name) && !closed {
                    tokens.skip_raw(&name);
                    continue;
                }
                w.start(&name, &attrs);
                if closed || is_void(&name) {
                    w.end(&name);
                }
            }
            Token::End(name) => w.end(&name),
        }
    }
    w.finish()
}

// ---- tokens ------------------------------------------------------------------------------------

enum Token<'a> {
    Text(&'a str),
    Start {
        name: String,
        attrs: Attrs,
        /// `<br/>`: written closed.
        closed: bool,
    },
    End(String),
}

#[derive(Default)]
struct Attrs(Vec<(String, String)>);

impl Attrs {
    fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// The `style` attribute, lower case and with its spaces taken out, for matching.
    fn style(&self) -> Option<String> {
        self.get("style").map(|s| {
            s.chars()
                .filter(|c| !c.is_whitespace())
                .flat_map(char::to_lowercase)
                .collect()
        })
    }
}

/// A style that lays text out as written, `white-space: pre` (not `pre-wrap`, which word
/// processors put on every run of text).
fn preformatted(style: &str) -> bool {
    style_value(style, "white-space") == Some("pre")
}

/// The value of the property `name` in a style as [`Attrs::style`] gives it, the last one
/// written. The whole name must match: Word's `mso-bidi-font-weight` is not `font-weight`.
fn style_value<'a>(style: &'a str, name: &str) -> Option<&'a str> {
    style
        .rsplit(';')
        .filter_map(|d| d.split_once(':'))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.trim_end_matches("!important"))
}

/// Whether a style makes its text bold (`Some(true)`), says it is not (`Some(false)`), or says
/// nothing of its weight.
fn style_bold(style: &str) -> Option<bool> {
    match style_value(style, "font-weight")? {
        "bold" | "bolder" => Some(true),
        "normal" | "lighter" => Some(false),
        n => n.parse::<u32>().ok().map(|n| n >= 600),
    }
}

fn style_italic(style: &str) -> bool {
    matches!(style_value(style, "font-style"), Some("italic" | "oblique"))
}

/// Elements whose content is not text to keep.
fn is_raw(name: &str) -> bool {
    matches!(
        name,
        "script" | "style" | "title" | "template" | "noscript" | "xml" | "svg" | "math"
    )
}

/// Elements that have no end tag.
fn is_void(name: &str) -> bool {
    matches!(
        name,
        "br" | "hr"
            | "img"
            | "meta"
            | "link"
            | "input"
            | "wbr"
            | "col"
            | "area"
            | "base"
            | "source"
            | "embed"
            | "param"
            | "track"
    )
}

struct Tokens<'a> {
    s: &'a str,
    /// Always on a character boundary of `s`.
    at: usize,
}

impl<'a> Tokens<'a> {
    fn rest(&self) -> &'a str {
        self.s.get(self.at..).unwrap_or("")
    }

    fn next(&mut self) -> Option<Token<'a>> {
        loop {
            let rest = self.rest();
            if rest.is_empty() {
                return None;
            }
            if !rest.starts_with('<') {
                let n = rest.find('<').unwrap_or(rest.len());
                self.at += n;
                return Some(Token::Text(rest.get(..n).unwrap_or("")));
            }
            let after = rest.get(1..).unwrap_or("");
            // A comment, a declaration or a processing instruction: nothing to keep.
            if let Some(c) = after.strip_prefix("!--") {
                let n = c.find("-->").map_or(c.len(), |i| i + 3);
                self.at += 4 + n;
                continue;
            }
            if after.starts_with('!') || after.starts_with('?') {
                let n = after.find('>').map_or(after.len(), |i| i + 1);
                self.at += 1 + n;
                continue;
            }
            return Some(self.tag(after));
        }
    }

    /// The tag `body` starts after `<`, or the `<` as text when no tag name follows it.
    fn tag(&mut self, after: &'a str) -> Token<'a> {
        let (end, body) = match after.strip_prefix('/') {
            Some(b) => (true, b),
            None => (false, after),
        };
        if !body.starts_with(|c: char| c.is_ascii_alphabetic()) {
            self.at += 1;
            return Token::Text("<");
        }
        let name_len = body
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(body.len());
        let name = body.get(..name_len).unwrap_or("").to_ascii_lowercase();
        let mut i = name_len;
        let mut attrs = Attrs::default();
        let mut closed = false;
        // Attributes, up to the `>` that is not inside quotes.
        loop {
            let tail = body.get(i..).unwrap_or("");
            i += tail.len() - tail.trim_start().len();
            let tail = body.get(i..).unwrap_or("");
            if tail.is_empty() {
                break;
            }
            if tail.starts_with('>') {
                i += 1;
                break;
            }
            if tail.starts_with('/') {
                closed = tail.get(1..).is_some_and(|t| t.starts_with('>'));
                i += 1;
                continue;
            }
            let key_len = tail
                .find(|c: char| c.is_whitespace() || c == '=' || c == '>' || c == '/')
                .unwrap_or(tail.len())
                .max(tail.chars().next().map_or(1, char::len_utf8));
            let key = tail.get(..key_len).unwrap_or("").to_ascii_lowercase();
            i += key_len;
            let tail = body.get(i..).unwrap_or("");
            let ws = tail.len() - tail.trim_start().len();
            let mut value = String::new();
            if body.get(i + ws..).is_some_and(|t| t.starts_with('=')) {
                i += ws + 1;
                let tail = body.get(i..).unwrap_or("");
                i += tail.len() - tail.trim_start().len();
                let tail = body.get(i..).unwrap_or("");
                let v = match tail.chars().next() {
                    Some(q @ ('"' | '\'')) => {
                        let inner = tail.get(1..).unwrap_or("");
                        let n = inner.find(q).unwrap_or(inner.len());
                        i += 1 + n + usize::from(n < inner.len());
                        inner.get(..n).unwrap_or("")
                    }
                    _ => {
                        let n = tail
                            .find(|c: char| c.is_whitespace() || c == '>')
                            .unwrap_or(tail.len());
                        i += n;
                        tail.get(..n).unwrap_or("")
                    }
                };
                value = decode(v);
            }
            if !end && attrs.0.len() < 32 {
                attrs.0.push((key, value));
            }
        }
        self.at += 1 + usize::from(end) + i.min(body.len());
        if end {
            Token::End(name)
        } else {
            Token::Start {
                name,
                attrs,
                closed,
            }
        }
    }

    /// Past the end tag of the raw element `name`, or to the end.
    fn skip_raw(&mut self, name: &str) {
        let rest = self.rest();
        let close = rest.match_indices("</").find(|(i, _)| {
            rest.get(i + 2..i + 2 + name.len())
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        });
        match close {
            Some((i, _)) => {
                let after = rest.get(i..).unwrap_or("");
                let n = after.find('>').map_or(after.len(), |j| j + 1);
                self.at += i + n;
            }
            None => self.at = self.s.len(),
        }
    }
}

/// Text with its character references made characters. An unknown or broken reference stays as
/// it was written.
fn decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(rest.get(..i).unwrap_or(""));
        let tail = rest.get(i + 1..).unwrap_or("");
        // Only the first bytes are looked at, so text full of `&` stays quick.
        let name = tail
            .bytes()
            .take(11)
            .position(|b| b == b';')
            .filter(|&n| n > 0)
            .and_then(|n| tail.get(..n));
        match name.and_then(|n| Some((n, reference(n)?))) {
            Some((n, c)) => {
                out.push(c);
                rest = tail.get(n.len() + 1..).unwrap_or("");
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

fn reference(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let n = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return Some(match char::from_u32(n) {
            Some('\0') | None => '\u{FFFD}',
            Some(c) => c,
        });
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "hellip" => '\u{2026}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201c}',
        "rdquo" => '\u{201d}',
        "laquo" => '\u{ab}',
        "raquo" => '\u{bb}',
        "bull" => '\u{2022}',
        "middot" => '\u{b7}',
        "copy" => '\u{a9}',
        "reg" => '\u{ae}',
        "trade" => '\u{2122}',
        "deg" => '\u{b0}',
        "times" => '\u{d7}',
        "euro" => '\u{20ac}',
        "shy" => '\u{ad}',
        "zwj" => '\u{200d}',
        "zwnj" => '\u{200c}',
        _ => return None,
    })
}

// ---- the writer --------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Wrap {
    Bold,
    Italic,
    /// Both at once, as a word processor's span can be.
    BoldItalic,
    Strike,
    Code,
    Link,
    /// An element that marks nothing, or of a kind already open: it closes, writing nothing.
    Plain,
}

struct Inline {
    wrap: Wrap,
    /// The element that opened it, so its end tag closes it.
    tag: String,
    /// Where its text starts in [`Writer::line`].
    start: usize,
    href: String,
}

struct List {
    ordered: bool,
    next: u64,
    /// The width of the marker of the item open in it ("- " is 2, "10. " is 4).
    width: usize,
}

/// A quote or a list that is written, in the order they were opened.
enum Nest {
    Quote,
    List(List),
}

/// A quote or a list that is open, written or not (past [`MAX_DEPTH`] of its kind it is not).
struct Frame {
    quote: bool,
    shown: bool,
}

/// Tags that end the block being read, at their start and at their end.
fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "nav"
            | "aside"
            | "main"
            | "figure"
            | "figcaption"
            | "address"
            | "details"
            | "summary"
            | "form"
            | "fieldset"
            | "center"
            | "dl"
            | "dt"
            | "dd"
            | "caption"
            | "table"
            | "body"
            | "html"
            | "tbody"
            | "thead"
            | "tfoot"
    )
}

/// Tags that start a line of their own: inside a table row only a space, inside `<pre>` a new
/// line of the code.
fn breaks_line(name: &str) -> bool {
    is_block(name)
        || matches!(
            name,
            "h1" | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "blockquote"
                | "ul"
                | "ol"
                | "li"
                | "pre"
                | "hr"
                | "br"
                | "tr"
        )
}

#[derive(Default)]
struct Writer {
    out: String,
    /// The most `out` may hold; `over` once something did not fit, and then nothing is written.
    cap: usize,
    over: bool,
    /// The text of the block being read, inline Markdown in it; `\n` is a hard break.
    line: String,
    inline: Vec<Inline>,
    /// Every quote and list open, innermost last, and those of them that are written.
    frames: Vec<Frame>,
    shown: Vec<Nest>,
    quotes_open: usize,
    lists_open: usize,
    /// An item was opened and its first block not yet written: that block takes the marker.
    item: bool,
    heading: usize,
    /// Inside `<pre>` (its depth), and its code's language.
    pre: usize,
    lang: String,
    /// Whether anything Markdown-specific was written.
    marked: bool,
    /// Whether the block last written was in a list, for tight lists.
    last_in_list: bool,
    /// The prefix of the lines of the block last written.
    last_prefix: String,
    /// Tables open; inside a table row, and the count of tables open when it started (a table
    /// inside one of its cells is read as text of the row).
    tables: usize,
    row: bool,
    row_table: usize,
    /// Cells read in the table row being read.
    cells: usize,
}

impl Writer {
    fn text(&mut self, raw: &str) {
        let text = decode(raw);
        if self.pre > 0 {
            self.line.push_str(&text);
            return;
        }
        let code = self.inline.iter().any(|i| i.wrap == Wrap::Code);
        let mut space = self.line.is_empty() || self.line.ends_with([' ', '\n']);
        let mut prev = self.line.chars().next_back();
        let mut chars = text.char_indices().peekable();
        while let Some((at, c)) = chars.next() {
            if c.is_whitespace() {
                if !space {
                    self.line.push(' ');
                    space = true;
                }
                prev = Some(' ');
                continue;
            }
            space = false;
            if !code {
                let next = chars.peek().map(|&(_, n)| n);
                let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
                let escape = match c {
                    '\\' | '*' | '`' | '[' | ']' => true,
                    // Inside a word (snake_case) it is no emphasis.
                    '_' => !(word(prev) && word(next)),
                    '<' => next.is_some_and(|n| n.is_ascii_alphabetic() || n == '/' || n == '!'),
                    '&' => entity_like(text.get(at + 1..).unwrap_or("")),
                    // A lone `~` after a space ("~5 min") can end no strikethrough.
                    '~' => next == Some('~') || prev.is_some_and(|p| !p.is_whitespace()),
                    _ => false,
                };
                if escape {
                    self.line.push('\\');
                }
            }
            self.line.push(c);
            prev = Some(c);
        }
    }

    /// A space between two blocks of a table row, which stay on its line.
    fn soft_break(&mut self) {
        if !self.line.is_empty() && !self.line.ends_with([' ', '\n']) {
            self.line.push(' ');
        }
    }

    fn start(&mut self, name: &str, attrs: &Attrs) {
        // Inside a code block only its lines and its language count.
        if self.pre > 0 {
            match name {
                "br" => self.line.push('\n'),
                "pre" => self.pre += 1,
                "code" if self.lang.is_empty() => self.lang = language(attrs),
                n if breaks_line(n) && !self.line.is_empty() && !self.line.ends_with('\n') => {
                    self.line.push('\n');
                }
                _ => {}
            }
            return;
        }
        if name == "table" {
            self.tables += 1;
        }
        if self.row {
            let own = self.tables == self.row_table;
            match name {
                // A row left open ends where the next one starts.
                "tr" if own => {
                    self.block();
                    self.cells = 0;
                    return;
                }
                "td" | "th" if own => {
                    self.cell();
                    return;
                }
                "td" | "th" => {
                    self.soft_break();
                    return;
                }
                n if breaks_line(n) => {
                    self.soft_break();
                    return;
                }
                _ => {}
            }
        }
        if is_block(name) {
            self.block();
            return;
        }
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.block();
                self.heading = name
                    .get(1..)
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(1)
                    .clamp(1, 6);
                self.marked = true;
            }
            "blockquote" => {
                self.block();
                self.enter(Nest::Quote);
                self.marked = true;
            }
            "ul" | "ol" => {
                self.block();
                let next = attrs
                    .get("start")
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .unwrap_or(1)
                    .min(999_999_999);
                self.enter(Nest::List(List {
                    ordered: name == "ol",
                    next,
                    width: 2,
                }));
                self.marked = true;
            }
            "li" => {
                self.block();
                if self.lists_open == 0 {
                    // An item copied without its list.
                    self.enter(Nest::List(List {
                        ordered: false,
                        next: 1,
                        width: 2,
                    }));
                }
                self.item = true;
                self.marked = true;
            }
            "pre" => {
                self.block();
                self.pre = 1;
                self.lang = language(attrs);
                self.marked = true;
            }
            "hr" => {
                self.block();
                let item = self.item;
                self.write_block("---", item, "", true);
                self.marked = true;
            }
            "br" => {
                if self.heading > 0 {
                    self.text(" ");
                } else {
                    let t = self.line.trim_end_matches(' ').len();
                    self.line.truncate(t);
                    self.line.push('\n');
                }
            }
            "tr" | "td" | "th" => {
                // A row starts; a cell copied without its row starts one too.
                self.block();
                self.row = true;
                self.row_table = self.tables;
                self.cells = 0;
                if name != "tr" {
                    self.cell();
                }
            }
            "img" => self.image(attrs),
            "b" | "strong" => {
                let style = attrs.style().unwrap_or_default();
                let wrap = match (style_bold(&style) != Some(false), style_italic(&style)) {
                    (true, true) => Wrap::BoldItalic,
                    (true, false) => Wrap::Bold,
                    (false, true) => Wrap::Italic,
                    (false, false) => Wrap::Plain,
                };
                self.open(wrap, name, attrs);
            }
            "i" | "em" | "cite" | "dfn" => self.open(Wrap::Italic, name, attrs),
            "s" | "del" | "strike" => self.open(Wrap::Strike, name, attrs),
            "code" | "kbd" | "samp" | "tt" => self.open(Wrap::Code, name, attrs),
            "a" => self.open(Wrap::Link, name, attrs),
            "span" | "font" => {
                // Word processors mark bold and italic with a style on a span.
                let style = attrs.style().unwrap_or_default();
                let wrap = match (style_bold(&style) == Some(true), style_italic(&style)) {
                    (true, true) => Wrap::BoldItalic,
                    (true, false) => Wrap::Bold,
                    (false, true) => Wrap::Italic,
                    (false, false) => Wrap::Plain,
                };
                self.open(wrap, name, attrs);
            }
            _ => {}
        }
    }

    /// A cell of the row being read starts: after the first, a ` | ` between it and the last.
    fn cell(&mut self) {
        if self.cells > 0 {
            self.text(" | ");
        }
        self.cells += 1;
    }

    fn end(&mut self, name: &str) {
        if self.pre > 0 {
            match name {
                "pre" => {
                    if self.pre == 1 {
                        self.code_block();
                    }
                    self.pre -= 1;
                }
                n if breaks_line(n) && !self.line.ends_with('\n') => self.line.push('\n'),
                _ => {}
            }
            return;
        }
        let table = name == "table";
        if table {
            self.tables = self.tables.saturating_sub(1);
        }
        if self.row {
            match name {
                // The row's own table, or one around it, ends.
                "tr" if self.tables == self.row_table => {
                    self.block();
                    self.row = false;
                    return;
                }
                "table" if self.tables < self.row_table => {
                    self.block();
                    self.row = false;
                    return;
                }
                "td" | "th" => return,
                n if breaks_line(n) => {
                    self.soft_break();
                    return;
                }
                _ => {}
            }
        }
        if is_block(name) {
            self.block();
            return;
        }
        match name {
            "tr" => self.block(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.block();
                self.heading = 0;
            }
            "blockquote" => {
                self.block();
                self.leave(true);
            }
            "ul" | "ol" => {
                self.block();
                self.item = false;
                self.leave(false);
            }
            "li" => {
                self.block();
                // An empty item still shows.
                if self.item {
                    self.write_block("", true, "", false);
                }
                self.item = false;
            }
            _ => self.close(name),
        }
    }

    /// A quote or a list opens inside those open.
    fn enter(&mut self, nest: Nest) {
        let quote = matches!(nest, Nest::Quote);
        let same = self
            .shown
            .iter()
            .filter(|n| matches!(n, Nest::Quote) == quote)
            .count();
        let shown = same < MAX_DEPTH;
        if shown {
            self.shown.push(nest);
        }
        self.frames.push(Frame { quote, shown });
        if quote {
            self.quotes_open += 1;
        } else {
            self.lists_open += 1;
        }
    }

    /// The innermost quote (or list) ends, and with it whatever was opened inside it and left
    /// open. Each frame looked at is closed, so a long input stays quick.
    fn leave(&mut self, quote: bool) {
        let open = if quote {
            self.quotes_open
        } else {
            self.lists_open
        };
        if open == 0 {
            return;
        }
        while let Some(f) = self.frames.pop() {
            if f.shown {
                self.shown.pop();
            }
            if f.quote {
                self.quotes_open = self.quotes_open.saturating_sub(1);
            } else {
                self.lists_open = self.lists_open.saturating_sub(1);
            }
            if f.quote == quote {
                break;
            }
        }
    }

    fn open(&mut self, wrap: Wrap, tag: &str, attrs: &Attrs) {
        if self.inline.len() >= MAX_INLINE {
            return;
        }
        let has = |w: Wrap| {
            self.inline.iter().any(|i| {
                i.wrap == w
                    || (i.wrap == Wrap::BoldItalic && matches!(w, Wrap::Bold | Wrap::Italic))
            })
        };
        // What is open already is not marked again.
        let wrap = match wrap {
            Wrap::BoldItalic => match (has(Wrap::Bold), has(Wrap::Italic)) {
                (true, true) => Wrap::Plain,
                (true, false) => Wrap::Italic,
                (false, true) => Wrap::Bold,
                (false, false) => Wrap::BoldItalic,
            },
            Wrap::Plain => Wrap::Plain,
            w if has(w) => Wrap::Plain,
            w => w,
        };
        self.inline.push(Inline {
            wrap,
            tag: tag.to_string(),
            start: self.line.len(),
            href: attrs.get("href").unwrap_or("").trim().to_string(),
        });
    }

    /// Closes the inline element `tag`, and any opened inside it and left open.
    fn close(&mut self, tag: &str) {
        let Some(at) = self.inline.iter().rposition(|i| i.tag == tag) else {
            return;
        };
        while self.inline.len() > at {
            if let Some(i) = self.inline.pop() {
                self.wrap(&i);
            }
        }
    }

    fn wrap(&mut self, i: &Inline) {
        if i.wrap == Wrap::Plain || !self.line.is_char_boundary(i.start) {
            return;
        }
        let inner = self.line.split_off(i.start);
        let core = inner.trim_matches([' ', '\n']);
        let lead = inner.len() - inner.trim_start_matches([' ', '\n']).len();
        let before = inner.get(..lead).unwrap_or("");
        let after = inner.get(lead + core.len()..).unwrap_or("");
        let marked = match i.wrap {
            _ if core.is_empty() => None,
            Wrap::Bold => Some(format!("**{core}**")),
            Wrap::Italic => Some(format!("*{core}*")),
            Wrap::BoldItalic => Some(format!("***{core}***")),
            Wrap::Strike => Some(format!("~~{core}~~")),
            Wrap::Code => {
                let fence = "`".repeat(longest_run(core, '`') + 1);
                let pad = if core.starts_with('`') || core.ends_with('`') {
                    " "
                } else {
                    ""
                };
                Some(format!("{fence}{pad}{core}{pad}{fence}"))
            }
            Wrap::Link => link_target(&i.href).map(|href| format!("[{core}]({href})")),
            Wrap::Plain => None,
        };
        match marked {
            Some(m) => {
                self.line.push_str(before);
                self.line.push_str(&m);
                self.line.push_str(after);
                self.marked = true;
            }
            None => self.line.push_str(&inner),
        }
    }

    fn image(&mut self, attrs: &Attrs) {
        let alt = attrs
            .get("alt")
            .unwrap_or("")
            .replace(['[', ']', '\n', '\r'], " ");
        let src = attrs
            .get("src")
            .map(str::trim)
            .filter(|s| !s.starts_with("data:"))
            .and_then(link_target);
        let Some(src) = src else {
            if !alt.trim().is_empty() {
                self.text(&alt);
            }
            return;
        };
        if !(self.line.is_empty() || self.line.ends_with([' ', '\n'])) {
            self.line.push(' ');
        }
        let alt = escape_entities(alt.trim());
        self.line.push_str(&format!("![{alt}]({src})"));
        self.marked = true;
    }

    /// Ends the block being read: its text is written.
    fn block(&mut self) {
        // Inline elements left open are marked in this block, and go on in the next from its
        // start: a link around a block is a link in it.
        let open = std::mem::take(&mut self.inline);
        for i in open.iter().rev() {
            self.wrap(i);
        }
        self.inline = open;
        for i in &mut self.inline {
            i.start = 0;
        }
        let line = std::mem::take(&mut self.line);
        let line = if self.heading > 0 {
            line.replace('\n', " ")
        } else {
            line
        };
        let text = line.trim_matches([' ', '\n']);
        if !text.is_empty() {
            let lead = if self.heading > 0 {
                format!("{} ", "#".repeat(self.heading))
            } else {
                String::new()
            };
            let item = self.item;
            self.write_block(text, item, &lead, false);
        }
    }

    /// A fenced code block of what `<pre>` held.
    fn code_block(&mut self) {
        let code = std::mem::take(&mut self.line);
        let code = code
            .trim_end_matches(['\n', '\r', ' '])
            .trim_start_matches(['\n', '\r']);
        let fence = "`".repeat((longest_run(code, '`') + 1).max(3));
        let text = format!("{fence}{}\n{code}\n{fence}", self.lang);
        self.lang.clear();
        let item = self.item;
        self.write_block(&text, item, "", true);
    }

    /// Adds `s` to the output, or nothing from now on once the output would pass its cap.
    fn put(&mut self, s: &str) {
        if self.over || self.out.len().saturating_add(s.len()) > self.cap {
            self.over = true;
            return;
        }
        self.out.push_str(s);
    }

    /// Writes one block: its lines, each with the prefixes of the quotes and lists it is in.
    /// `lead` goes before its first line (a heading's marks); `raw` lines are written as they are
    /// (a code block, a rule), others escaped and joined by hard breaks.
    fn write_block(&mut self, text: &str, item: bool, lead: &str, raw: bool) {
        if self.over {
            return;
        }
        let in_list = self.lists_open > 0;
        if !self.out.is_empty() {
            self.put("\n");
            if !(item && self.last_in_list) {
                // A blank line, inside the quotes and lists both blocks are in.
                let now = self.prefix(None);
                let shared: String = now
                    .chars()
                    .zip(self.last_prefix.chars())
                    .take_while(|(a, b)| a == b)
                    .map(|(a, _)| a)
                    .collect();
                self.put(shared.trim_end());
                self.put("\n");
            }
        }
        let mut marker = None;
        if item {
            let list = self.shown.iter_mut().rev().find_map(|n| match n {
                Nest::List(l) => Some(l),
                Nest::Quote => None,
            });
            if let Some(l) = list {
                let m = if l.ordered {
                    let m = format!("{}. ", l.next);
                    l.next = l.next.saturating_add(1);
                    m
                } else {
                    "- ".into()
                };
                l.width = m.len();
                marker = Some(m);
            }
            self.item = false;
        }
        let first = format!("{}{lead}", self.prefix(marker.as_deref()));
        let rest = self.prefix(None);
        for (k, l) in text.split('\n').enumerate() {
            if k == 0 {
                self.put(&first);
            } else {
                if !raw {
                    // A hard break.
                    self.put("\\");
                }
                self.put("\n");
                self.put(&rest);
            }
            if raw {
                self.put(l);
            } else if k == 0 && !lead.is_empty() {
                self.put(&escape_heading_end(l.trim()));
            } else {
                self.put(&escape_start(l.trim_start()));
            }
            if self.over {
                return;
            }
        }
        let end = self.out.trim_end_matches(' ').len();
        self.out.truncate(end);
        self.last_in_list = in_list;
        self.last_prefix = rest;
    }

    /// What every line of a block starts with: `> ` per quote and the indent of each list, in the
    /// order they were opened. `marker`: the first line of an item, whose marker takes the place
    /// of its list's indent.
    fn prefix(&self, marker: Option<&str>) -> String {
        let last_list = self.shown.iter().rposition(|n| matches!(n, Nest::List(_)));
        let mut p = String::new();
        for (k, n) in self.shown.iter().enumerate() {
            match n {
                Nest::Quote => p.push_str("> "),
                Nest::List(l) => match marker {
                    Some(m) if Some(k) == last_list => p.push_str(m),
                    _ => p.extend(std::iter::repeat_n(' ', l.width)),
                },
            }
        }
        p
    }

    fn finish(mut self) -> Option<String> {
        if self.pre > 0 {
            self.code_block();
        } else {
            self.block();
        }
        if self.over {
            return None;
        }
        let out = self.out.trim_end();
        (self.marked && !out.trim().is_empty()).then(|| out.to_string())
    }
}

/// The language a `<pre>` or `<code>` names in its class (`language-rust`, `lang-js`).
fn language(attrs: &Attrs) -> String {
    attrs
        .get("class")
        .unwrap_or("")
        .split_whitespace()
        .find_map(|c| {
            c.strip_prefix("language-")
                .or_else(|| c.strip_prefix("lang-"))
        })
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '#' | '_' | '.'))
        .take(32)
        .collect()
}

/// A link's target as Markdown can carry it, or `None` for one not worth a link (empty, a
/// script, a place in the page it was copied from).
fn link_target(href: &str) -> Option<String> {
    // Browsers take tabs and new lines out of a URL ("java\tscript:" is a script), so control
    // characters go before the scheme is looked at.
    let clean: String = href.chars().filter(|c| !c.is_control()).collect();
    let href = clean.trim();
    if href.is_empty()
        || href.starts_with('#')
        || href
            .get(..11)
            .is_some_and(|s| s.eq_ignore_ascii_case("javascript:"))
    {
        return None;
    }
    let mut out = String::with_capacity(href.len());
    for (at, c) in href.char_indices() {
        match c {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '&' if entity_like(href.get(at + 1..).unwrap_or("")) => out.push_str("\\&"),
            c => out.push(c),
        }
    }
    Some(out)
}

/// Whether text after a `&` would be read as a character reference (`copy;`, `#169;`, `#xA9;`),
/// so the `&` needs a backslash to stay itself. Looks at a few bytes only.
fn entity_like(after: &str) -> bool {
    let b = after.as_bytes();
    let mut i = 0;
    if b.first() == Some(&b'#') {
        i = 1;
        if matches!(b.get(1), Some(b'x' | b'X')) {
            i = 2;
        }
    }
    let start = i;
    while i - start < 32 && b.get(i).is_some_and(u8::is_ascii_alphanumeric) {
        i += 1;
    }
    i > start && b.get(i) == Some(&b';')
}

/// Text with every `&` that would read as a character reference escaped.
fn escape_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (at, c) in s.char_indices() {
        if c == '&' && entity_like(s.get(at + 1..).unwrap_or("")) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn longest_run(s: &str, c: char) -> usize {
    let (mut best, mut run) = (0, 0);
    for x in s.chars() {
        if x == c {
            run += 1;
            best = best.max(run);
        } else {
            run = 0;
        }
    }
    best
}

/// A heading's text, with a `#` run at its end escaped: unescaped, Markdown reads it as the
/// heading's closing marks ("Issue #" would show "Issue").
fn escape_heading_end(text: &str) -> String {
    let body = text.trim_end_matches('#');
    match text.get(body.len()..) {
        Some(marks) if !marks.is_empty() && (body.is_empty() || body.ends_with(' ')) => {
            format!("{body}\\{marks}")
        }
        _ => text.to_string(),
    }
}

/// A line of text that would read as a heading, a quote, a list item, a rule or the underline of
/// a heading at the start of a line, escaped so it stays text.
fn escape_start(line: &str) -> String {
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let after = line.get(digits..).unwrap_or("");
    if (1..=9).contains(&digits)
        && (after.starts_with(". ") || after.starts_with(") ") || after == "." || after == ")")
    {
        return format!("{}\\{after}", line.get(..digits).unwrap_or(""));
    }
    let trimmed = line.trim_end();
    let marks = line.starts_with('#')
        || line.starts_with('>')
        || line.starts_with("- ")
        || line.starts_with("+ ")
        || line == "+"
        || line.starts_with("---")
        || line.starts_with("===")
        // "-", "--", "-- -": a heading's underline or a rule.
        || (line.starts_with('-') && line.chars().all(|c| c == '-' || c == ' '))
        // "=", "==": a heading's underline.
        || (!trimmed.is_empty() && trimmed.chars().all(|c| c == '='));
    if marks {
        format!("\\{line}")
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests;
