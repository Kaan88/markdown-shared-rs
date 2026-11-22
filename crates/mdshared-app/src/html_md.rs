//! HTML to Markdown, for a paste of rich text: what a web page, a word processor or mdview put on
//! the clipboard becomes Markdown text. Headings, bold, italic, strikethrough, links, images,
//! lists (nested, numbered or not), inline code, code blocks, block quotes, rules and paragraphs
//! are written as their Markdown; any other tag keeps its text. A table's cells are kept as text,
//! a row to a line.
//!
//! Hand-written for this one job, with no HTML library: a small tokenizer and a writer that keeps
//! the blocks it is in. It reads anything (clipboard contents are untrusted) and has no path that
//! can panic: every slice is taken with `get`, and the depth of nesting it honours is capped so a
//! hostile input cannot make the output grow much faster than the input.

/// Lists and quotes nested deeper than this are written at this depth.
const MAX_DEPTH: usize = 16;
/// Open inline elements (bold, links …) followed at once; more are kept as text.
const MAX_INLINE: usize = 64;

/// The Markdown for an HTML fragment or document, or `None` when it has nothing Markdown can say
/// that its plain text does not: no heading, emphasis, link, image, list, quote, code or rule. A
/// paste then takes the clipboard's plain text, as it would without the conversion. Also `None`
/// for HTML that lays its text out as written (`white-space: pre`, as code editors copy their
/// coloured text): its plain text is the better paste.
pub fn to_markdown(html: &str) -> Option<String> {
    let mut w = Writer::default();
    let mut tokens = Tokens { s: html, at: 0 };
    // Deep nesting repeats its prefixes on every line; past this the input is not a document.
    let cap = html.len().saturating_mul(8).saturating_add(1 << 16);
    while let Some(t) = tokens.next() {
        if w.out.len() > cap {
            return None;
        }
        match t {
            Token::Text(t) => w.text(t),
            Token::Start {
                name,
                attrs,
                closed,
            } => {
                if attrs.style().is_some_and(|s| preformatted(&s)) {
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
    style
        .match_indices("white-space:pre")
        .any(|(i, m)| !style.get(i + m.len()..).is_some_and(|t| t.starts_with('-')))
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
        let name = tail
            .find(';')
            .filter(|&n| n > 0 && n <= 10)
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

#[derive(Default)]
struct Writer {
    out: String,
    /// The text of the block being read, inline Markdown in it; `\n` is a hard break.
    line: String,
    inline: Vec<Inline>,
    quote: usize,
    lists: Vec<List>,
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
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
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
                let next = chars.peek().copied();
                let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
                let escape = match c {
                    '\\' | '*' | '`' | '[' | ']' => true,
                    // Inside a word (snake_case) it is no emphasis.
                    '_' => !(word(prev) && word(next)),
                    '<' => next.is_some_and(|n| n.is_ascii_alphabetic() || n == '/' || n == '!'),
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

    fn start(&mut self, name: &str, attrs: &Attrs) {
        // Inside a code block only its lines and its language count.
        if self.pre > 0 {
            match name {
                "br" => self.line.push('\n'),
                "pre" => self.pre += 1,
                "code" if self.lang.is_empty() => self.lang = language(attrs),
                _ => {}
            }
            return;
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
                self.quote += 1;
                self.marked = true;
            }
            "ul" | "ol" => {
                self.block();
                let next = attrs
                    .get("start")
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .unwrap_or(1)
                    .min(999_999_999);
                self.lists.push(List {
                    ordered: name == "ol",
                    next,
                    width: 2,
                });
                self.marked = true;
            }
            "li" => {
                self.block();
                if self.lists.is_empty() {
                    // An item copied without its list.
                    self.lists.push(List {
                        ordered: false,
                        next: 1,
                        width: 2,
                    });
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
            "tr" => {
                self.block();
                self.cells = 0;
            }
            "td" | "th" => {
                if self.cells > 0 {
                    self.text(" | ");
                }
                self.cells += 1;
            }
            "img" => self.image(attrs),
            "b" | "strong" => {
                let normal = attrs.style().is_some_and(|s| {
                    s.contains("font-weight:normal") || s.contains("font-weight:400")
                });
                self.open(if normal { Wrap::Plain } else { Wrap::Bold }, name, attrs);
            }
            "i" | "em" | "cite" | "dfn" => self.open(Wrap::Italic, name, attrs),
            "s" | "del" | "strike" => self.open(Wrap::Strike, name, attrs),
            "code" | "kbd" | "samp" | "tt" => self.open(Wrap::Code, name, attrs),
            "a" => self.open(Wrap::Link, name, attrs),
            "span" | "font" => {
                // Word processors mark bold and italic with a style on a span.
                let style = attrs.style().unwrap_or_default();
                let bold = ["bold", "600", "700", "800", "900"]
                    .iter()
                    .any(|w| style.contains(&format!("font-weight:{w}")));
                let wrap = if bold {
                    Wrap::Bold
                } else if style.contains("font-style:italic") {
                    Wrap::Italic
                } else {
                    Wrap::Plain
                };
                self.open(wrap, name, attrs);
            }
            _ => {}
        }
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
                "div" | "p" | "li" | "tr" if !self.line.ends_with('\n') => self.line.push('\n'),
                _ => {}
            }
            return;
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
                self.quote = self.quote.saturating_sub(1);
            }
            "ul" | "ol" => {
                self.block();
                self.item = false;
                self.lists.pop();
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

    fn open(&mut self, wrap: Wrap, tag: &str, attrs: &Attrs) {
        if self.inline.len() >= MAX_INLINE {
            return;
        }
        let same = wrap != Wrap::Plain && self.inline.iter().any(|i| i.wrap == wrap);
        self.inline.push(Inline {
            wrap: if same { Wrap::Plain } else { wrap },
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
                self.wrap(i);
            }
        }
    }

    fn wrap(&mut self, i: Inline) {
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
        self.line.push_str(&format!("![{}]({src})", alt.trim()));
        self.marked = true;
    }

    /// Ends the block being read: its text is written.
    fn block(&mut self) {
        let line = std::mem::take(&mut self.line);
        // Inline elements left open go on in the next block, from its start.
        for i in &mut self.inline {
            i.start = 0;
        }
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

    /// Writes one block: its lines, each with the prefixes of the quotes and lists it is in.
    /// `lead` goes before its first line (a heading's marks); `raw` lines are written as they are
    /// (a code block, a rule), others escaped and joined by hard breaks.
    fn write_block(&mut self, text: &str, item: bool, lead: &str, raw: bool) {
        let in_list = !self.lists.is_empty();
        if !self.out.is_empty() {
            self.out.push('\n');
            if !(item && self.last_in_list) {
                // A blank line, inside the quotes and lists both blocks are in.
                let now = self.prefix(false);
                let shared: String = now
                    .chars()
                    .zip(self.last_prefix.chars())
                    .take_while(|(a, b)| a == b)
                    .map(|(a, _)| a)
                    .collect();
                self.out.push_str(shared.trim_end());
                self.out.push('\n');
            }
        }
        let depth = self.lists.len().min(MAX_DEPTH);
        let mut marker = String::new();
        if item {
            if let Some(l) = self.lists.get_mut(depth.saturating_sub(1)) {
                marker = if l.ordered {
                    let m = format!("{}. ", l.next);
                    l.next = l.next.saturating_add(1);
                    m
                } else {
                    "- ".into()
                };
                l.width = marker.len();
            }
            self.item = false;
        }
        let first = format!("{}{marker}{lead}", self.prefix(item));
        let rest = self.prefix(false);
        for (k, l) in text.split('\n').enumerate() {
            if k == 0 {
                self.out.push_str(&first);
            } else {
                if !raw {
                    // A hard break.
                    self.out.push('\\');
                }
                self.out.push('\n');
                self.out.push_str(&rest);
            }
            if raw {
                self.out.push_str(l);
            } else if k == 0 && !lead.is_empty() {
                self.out.push_str(l.trim_start());
            } else {
                self.out.push_str(&escape_start(l.trim_start()));
            }
        }
        let end = self.out.trim_end_matches(' ').len();
        self.out.truncate(end);
        self.last_in_list = in_list;
        self.last_prefix = rest;
    }

    /// What every line of a block starts with: `> ` per quote, then the indent of the lists it
    /// is in. `item`: the first line of an item, whose own marker comes after.
    fn prefix(&self, item: bool) -> String {
        let mut p = "> ".repeat(self.quote.min(MAX_DEPTH));
        let depth = self.lists.len().min(MAX_DEPTH);
        let own = if item { depth.saturating_sub(1) } else { depth };
        for l in self.lists.iter().take(own) {
            p.extend(std::iter::repeat_n(' ', l.width));
        }
        p
    }

    fn finish(mut self) -> Option<String> {
        while let Some(i) = self.inline.pop() {
            self.wrap(i);
        }
        if self.pre > 0 {
            self.code_block();
        } else {
            self.block();
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
    let href = href.trim();
    if href.is_empty()
        || href.starts_with('#')
        || href
            .get(..11)
            .is_some_and(|s| s.eq_ignore_ascii_case("javascript:"))
    {
        return None;
    }
    let mut out = String::with_capacity(href.len());
    for c in href.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    Some(out)
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

/// A line of text that would read as a heading, a quote, a list item or a rule at the start of
/// a line, escaped so it stays text.
fn escape_start(line: &str) -> String {
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let after = line.get(digits..).unwrap_or("");
    if (1..=9).contains(&digits)
        && (after.starts_with(". ") || after.starts_with(") ") || after == "." || after == ")")
    {
        return format!("{}\\{after}", line.get(..digits).unwrap_or(""));
    }
    let marks = line.starts_with('#')
        || line.starts_with('>')
        || line.starts_with("- ")
        || line.starts_with("+ ")
        || line == "-"
        || line == "+"
        || line.starts_with("---")
        || line.starts_with("===");
    if marks {
        format!("\\{line}")
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests;
