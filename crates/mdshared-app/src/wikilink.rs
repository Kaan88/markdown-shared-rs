//! Wiki-links (`[[Note#Heading|shown text]]`) and a light scan of a Markdown file for the links it
//! holds, without parsing the document.
//!
//! [`parse_target`] splits what stands between the brackets. [`scan`] walks a file's text line by
//! line and reports every wiki-link, inline link and reference definition outside fenced code and
//! code spans; it is what the note index reads files with, and what an editor can complete and
//! navigate from. It reads less carefully than a Markdown parser: see [`scan`] for where it
//! differs.

/// What stands between `[[` and `]]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WikiTarget<'a> {
    /// The note's name or path, without `#heading`. Empty for a link into the same document.
    pub note: &'a str,
    /// The text after `#`, as written. `None` without a `#` or with nothing after it.
    pub heading: Option<&'a str>,
    /// The text after `|`. `None` without a `|` or with nothing after it.
    pub label: Option<&'a str>,
}

impl<'a> WikiTarget<'a> {
    /// What the link shows when it has no label: the note's name, `Note > Heading` with a
    /// heading, and the heading alone for a link into the same document.
    pub fn display(&self) -> String {
        if let Some(label) = self.label {
            return label.to_string();
        }
        match (self.note.is_empty(), self.heading) {
            (false, Some(h)) => format!("{} > {}", self.note, h.replace('#', " > ")),
            (true, Some(h)) => h.replace('#', " > "),
            _ => self.note.to_string(),
        }
    }

    /// The heading a jump should land on: the last one of a `#a#b` path. A block reference
    /// (`#^id`) names no heading.
    pub fn last_heading(&self) -> Option<&'a str> {
        let h = self.heading?.rsplit('#').next()?.trim();
        (!h.is_empty() && !h.starts_with('^')).then_some(h)
    }
}

/// Splits the inside of a wiki-link. The first `|` (also written `\|`, which a table needs) ends
/// the target and the first `#` in the target starts the heading. `None` when nothing names a
/// note or a heading, or when the text holds a bracket or a line break.
pub fn parse_target(inner: &str) -> Option<WikiTarget<'_>> {
    if inner
        .bytes()
        .any(|c| matches!(c, b'[' | b']' | b'\n' | b'\r'))
    {
        return None;
    }
    let (target, label) = match inner.find('|') {
        Some(at) => {
            let target = &inner[..at];
            (
                target.strip_suffix('\\').unwrap_or(target),
                Some(inner[at + 1..].trim()),
            )
        }
        None => (inner, None),
    };
    let (note, heading) = match target.find('#') {
        Some(at) => (&target[..at], Some(target[at + 1..].trim())),
        None => (target, None),
    };
    let note = note.trim();
    let heading = heading.filter(|h| !h.is_empty());
    if note.is_empty() && heading.is_none() {
        return None;
    }
    Some(WikiTarget {
        note,
        heading,
        label: label.filter(|l| !l.is_empty()),
    })
}

/// Which syntax a scanned link was written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkSyntax {
    /// `[[target]]`; `embed` for `![[target]]`.
    Wiki { embed: bool },
    /// `[text](destination)`.
    Inline,
    /// `[label]: destination` at the start of a line.
    Definition,
}

/// One link [`scan`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found<'a> {
    pub syntax: LinkSyntax,
    /// The inside of a wiki-link, or a Markdown link's destination, as written.
    pub target: &'a str,
    /// Line number, from 0.
    pub line: u32,
    /// Byte offset of the link's first character in the whole text.
    pub offset: usize,
    /// The whole line, without its line break.
    pub line_text: &'a str,
    /// The text of the nearest ATX heading above the link.
    pub section: Option<&'a str>,
}

/// Reports the links in `text`, in order, until `found` returns false.
///
/// Skipped: fenced code blocks (backtick and tilde fences, also inside a quote), code spans that
/// open and close on one line, and anything after a backslash. Not understood, and so read as
/// ordinary text: indented code blocks, code spans that run over a line break, HTML, and
/// reference links (their definitions are reported instead). The work is linear in the text
/// except for backtick runs, which cost the square of their number on one line; a line has at
/// most about 1,400 runs of distinct lengths per megabyte.
pub fn scan<'a>(text: &'a str, mut found: impl FnMut(Found<'a>) -> bool) {
    let mut fence: Option<(u8, usize)> = None;
    let mut section: Option<&'a str> = None;
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut offset = 0;
    for (line_no, raw) in text.split('\n').enumerate() {
        let line_start = offset;
        offset += raw.len() + 1;
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let body = strip_quote(line);
        if let Some((mark, len)) = fence {
            if closes_fence(body, mark, len) {
                fence = None;
            }
            continue;
        }
        if let Some(opened) = opens_fence(body) {
            fence = Some(opened);
            continue;
        }
        if let Some(title) = atx_title(body) {
            section = Some(title);
        }
        let b = line.as_bytes();
        if memchr(b'[', b).is_none() {
            continue;
        }
        let mut report = |syntax, target: &'a str, at: usize| {
            found(Found {
                syntax,
                target,
                line: line_no as u32,
                offset: line_start + at,
                line_text: line,
                section,
            })
        };
        if let Some((target, at)) = definition(line)
            && !report(LinkSyntax::Definition, target, at)
        {
            return;
        }
        runs.clear();
        let mut run_cursor = 0;
        let mut ticks_listed = false;
        let mut open_brackets = 0usize;
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'\\' => i += 2,
                b'`' => {
                    if !ticks_listed {
                        list_backtick_runs(b, i, &mut runs);
                        ticks_listed = true;
                    }
                    let len = b[i..].iter().take_while(|&&c| c == b'`').count();
                    // A code span ends at the next run of the same length; without one the
                    // backticks are literal.
                    while runs.get(run_cursor).is_some_and(|r| r.0 < i + len) {
                        run_cursor += 1;
                    }
                    let closer = runs[run_cursor..].iter().find(|r| r.1 == len).map(|r| r.0);
                    i = closer.unwrap_or(i) + len;
                }
                b'[' if b.get(i + 1) == Some(&b'[') => {
                    let inner_start = i + 2;
                    match wiki_end(b, inner_start) {
                        Some(end) => {
                            let embed = i > 0 && b[i - 1] == b'!';
                            let at = if embed { i - 1 } else { i };
                            if !report(LinkSyntax::Wiki { embed }, &line[inner_start..end], at) {
                                return;
                            }
                            i = end + 2;
                        }
                        None => {
                            open_brackets += 1;
                            i += 1;
                        }
                    }
                }
                b'[' => {
                    open_brackets += 1;
                    i += 1;
                }
                b']' if open_brackets > 0 && b.get(i + 1) == Some(&b'(') => {
                    open_brackets -= 1;
                    match destination(b, i + 2) {
                        Some((from, to, after)) => {
                            if !report(LinkSyntax::Inline, &line[from..to], from) {
                                return;
                            }
                            i = after;
                        }
                        None => i += 2,
                    }
                }
                b']' => {
                    open_brackets = open_brackets.saturating_sub(1);
                    i += 1;
                }
                _ => i += 1,
            }
        }
    }
}

fn memchr(needle: u8, hay: &[u8]) -> Option<usize> {
    hay.iter().position(|&c| c == needle)
}

/// The line without leading blanks and block-quote markers.
fn strip_quote(line: &str) -> &str {
    line.trim_start_matches([' ', '\t', '>'])
}

fn opens_fence(body: &str) -> Option<(u8, usize)> {
    let mark = *body.as_bytes().first()?;
    if mark != b'`' && mark != b'~' {
        return None;
    }
    let len = body.bytes().take_while(|&c| c == mark).count();
    // A backtick fence's info string cannot hold a backtick: that line is a code span.
    (len >= 3 && !(mark == b'`' && body[len..].contains('`'))).then_some((mark, len))
}

fn closes_fence(body: &str, mark: u8, len: usize) -> bool {
    let n = body.bytes().take_while(|&c| c == mark).count();
    n >= len && body[n..].trim().is_empty()
}

fn atx_title(body: &str) -> Option<&str> {
    let level = body.bytes().take_while(|&c| c == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &body[level..];
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let title = rest.trim().trim_end_matches('#').trim_end();
    (!title.is_empty()).then_some(title)
}

/// Every backtick run from `from` on: (offset, length).
fn list_backtick_runs(b: &[u8], from: usize, out: &mut Vec<(usize, usize)>) {
    let mut i = from;
    while i < b.len() {
        if b[i] == b'`' {
            let len = b[i..].iter().take_while(|&&c| c == b'`').count();
            out.push((i, len));
            i += len;
        } else {
            i += 1;
        }
    }
}

/// The offset of the `]]` that closes a wiki-link whose inside starts at `from`. A bracket inside
/// means this is not one.
fn wiki_end(b: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i < b.len() {
        match b[i] {
            b']' => return (b.get(i + 1) == Some(&b']') && i > from).then_some(i),
            b'[' => return None,
            _ => i += 1,
        }
    }
    None
}

/// A longer destination is not read as one, which keeps a line of unclosed `](` linear.
const MAX_DESTINATION: usize = 1024;

/// An inline link's destination after `](`: its range, and where the scan goes on.
fn destination(b: &[u8], from: usize) -> Option<(usize, usize, usize)> {
    let b = &b[..b.len().min(from + MAX_DESTINATION)];
    let mut i = from;
    while b.get(i).is_some_and(|c| matches!(c, b' ' | b'\t')) {
        i += 1;
    }
    if b.get(i) == Some(&b'<') {
        let start = i + 1;
        let end = start + b[start..].iter().position(|&c| c == b'>' || c == b'<')?;
        return (b[end] == b'>').then_some((start, end, end + 1));
    }
    let start = i;
    let mut depth = 0usize;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'(' => depth += 1,
            b')' if depth == 0 => break,
            b')' => depth -= 1,
            b' ' | b'\t' => break,
            _ => {}
        }
        i += 1;
    }
    let end = i.min(b.len());
    (end > start && end < b.len()).then_some((start, end, end))
}

/// `[label]: destination` with at most three spaces before it: the destination and its offset.
fn definition(line: &str) -> Option<(&str, usize)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 || !line[indent..].starts_with('[') || line[indent..].starts_with("[^") {
        return None;
    }
    let close = indent + line[indent..].find("]:")?;
    if line[indent + 1..close].contains(['[', ']']) || close == indent + 1 {
        return None;
    }
    let rest = &line[close + 2..];
    let dest = rest.trim_start();
    let at = close + 2 + (rest.len() - dest.len());
    let dest = match dest.strip_prefix('<') {
        Some(angle) => &angle[..angle.find('>')?],
        None => dest.split([' ', '\t']).next()?,
    };
    let at = if line[at..].starts_with('<') {
        at + 1
    } else {
        at
    };
    (!dest.is_empty()).then_some((dest, at))
}

/// A relative link destination split into its decoded path and its fragment (still as written).
/// `None` for anything with a scheme, a drive, a leading slash or backslash, or no path at all:
/// only a path relative to the linking file can name a neighbouring note.
pub fn relative_destination(dest: &str) -> Option<(String, Option<&str>)> {
    let dest = dest.trim();
    let (rest, fragment) = match dest.split_once('#') {
        Some((a, f)) => (a, Some(f)),
        None => (dest, None),
    };
    let path = rest.split('?').next().unwrap_or(rest);
    if path.is_empty() || path.starts_with(['/', '\\']) {
        return None;
    }
    if let Some(colon) = path.find(':')
        && path[..colon]
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
    {
        return None;
    }
    let decoded = percent_decode(path);
    (!decoded.starts_with(['/', '\\'])).then_some((decoded, fragment))
}

fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| char::from(c).to_digit(16);
        match (b[i], b.get(i + 1).copied(), b.get(i + 2).copied()) {
            (b'%', Some(h), Some(l)) if hex(h).is_some() && hex(l).is_some() => {
                out.push((hex(h).unwrap_or(0) * 16 + hex(l).unwrap_or(0)) as u8);
                i += 3;
            }
            (c, ..) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(inner: &str) -> (String, Option<String>, Option<String>) {
        let t = parse_target(inner).expect(inner);
        (
            t.note.to_string(),
            t.heading.map(str::to_string),
            t.label.map(str::to_string),
        )
    }

    #[test]
    fn a_target_splits_into_note_heading_and_label() {
        let s = |v: &str| Some(v.to_string());
        assert_eq!(target("Note"), ("Note".into(), None, None));
        assert_eq!(target(" Note | shown "), ("Note".into(), None, s("shown")));
        assert_eq!(target("Note#Heading"), ("Note".into(), s("Heading"), None));
        assert_eq!(
            target("Note#Heading|text"),
            ("Note".into(), s("Heading"), s("text"))
        );
        assert_eq!(target("#Heading"), (String::new(), s("Heading"), None));
        assert_eq!(target("folder/Note"), ("folder/Note".into(), None, None));
        // A table needs the pipe escaped; a second pipe or `#` belongs to the label.
        assert_eq!(target("Note\\|a|b #c"), ("Note".into(), None, s("a|b #c")));
        assert_eq!(target("Note#"), ("Note".into(), None, None));
        assert_eq!(target("Note|"), ("Note".into(), None, None));
        for bad in ["", "  ", "#", "|x", "a[b", "a]b", "a\nb"] {
            assert_eq!(parse_target(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn display_text_and_the_heading_to_jump_to() {
        let show = |inner: &str| parse_target(inner).unwrap().display();
        assert_eq!(show("Note"), "Note");
        assert_eq!(show("Note|shown"), "shown");
        assert_eq!(show("Note#Heading"), "Note > Heading");
        assert_eq!(show("#Heading"), "Heading");
        assert_eq!(show("folder/Note"), "folder/Note");
        fn last(inner: &str) -> Option<&str> {
            parse_target(inner).unwrap().last_heading()
        }
        assert_eq!(last("Note#A#B"), Some("B"));
        assert_eq!(last("Note#^block"), None);
        assert_eq!(last("Note"), None);
    }

    fn links(text: &str) -> Vec<(LinkSyntax, String, u32)> {
        let mut out = Vec::new();
        scan(text, |f| {
            out.push((f.syntax, f.target.to_string(), f.line));
            true
        });
        out
    }

    #[test]
    fn the_scan_finds_wiki_links_inline_links_and_definitions() {
        let wiki = LinkSyntax::Wiki { embed: false };
        assert_eq!(
            links(
                "See [[One]] and [[Two#H|two]].\n\n![[Pic.png]] [x](other.md#h) [y](<a b.md>)\n[ref]: ../up.md \"t\"\n"
            ),
            vec![
                (wiki, "One".into(), 0),
                (wiki, "Two#H|two".into(), 0),
                (LinkSyntax::Wiki { embed: true }, "Pic.png".into(), 2),
                (LinkSyntax::Inline, "other.md#h".into(), 2),
                (LinkSyntax::Inline, "a b.md".into(), 2),
                (LinkSyntax::Definition, "../up.md".into(), 3),
            ]
        );
        // Parentheses in a destination balance; a title is not part of it.
        assert_eq!(
            links("[a](b(1).md \"t\") [c](d.md)"),
            vec![
                (LinkSyntax::Inline, "b(1).md".into(), 0),
                (LinkSyntax::Inline, "d.md".into(), 0)
            ]
        );
    }

    #[test]
    fn the_scan_skips_code_and_escapes() {
        let text = "\
`[[span]]` and ``[[two ` ticks]]`` then [[real]]
```md
[[fenced]] [x](fenced.md)
```
> ~~~
> [[quoted fence]]
> ~~~
\\[[escaped]] [[]] [[a[b]] `open [[literal]]
~~~~
```
[[still fenced]]
~~~~
[[after]]
";
        let found: Vec<String> = links(text).into_iter().map(|l| l.1).collect();
        assert_eq!(found, ["real", "literal", "after"]);
    }

    #[test]
    fn the_scan_reports_where_a_link_is() {
        let text = "# Top\r\n\r\ntext [[A]]\r\n## Part two ##\r\n- item [[B]] here\r\n";
        let mut out = Vec::new();
        scan(text, |f| {
            out.push((
                f.line,
                f.section,
                f.line_text,
                &text[f.offset..f.offset + 5],
            ));
            true
        });
        assert_eq!(
            out,
            vec![
                (2, Some("Top"), "text [[A]]", "[[A]]"),
                (4, Some("Part two"), "- item [[B]] here", "[[B]]"),
            ]
        );
    }

    #[test]
    fn the_scan_stops_when_asked() {
        let mut n = 0;
        scan("[[a]] [[b]] [[c]]", |_| {
            n += 1;
            n < 2
        });
        assert_eq!(n, 2);
    }

    #[test]
    fn hostile_input_is_scanned_quickly() {
        let started = std::time::Instant::now();
        let mut n = 0usize;
        let mut count = |text: &str| {
            scan(text, |_| {
                n += 1;
                true
            })
        };
        count(&"[[".repeat(100_000));
        count(&"[[a]]".repeat(100_000));
        count(&"[".repeat(100_000));
        count(&"[a](".repeat(100_000));
        count(&"]]".repeat(100_000));
        count(&"`[[a]] ".repeat(100_000));
        // Backtick runs of every length up to 600 on one line, none of which closes.
        let mut ticks = String::new();
        for len in 1..600 {
            ticks.push_str(&"`".repeat(len));
            ticks.push_str("[[x]] ");
        }
        count(&ticks);
        assert!(n >= 100_000);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_relative_destination_is_decoded_and_others_are_refused() {
        assert_eq!(
            relative_destination("a%20b.md#h"),
            Some(("a b.md".to_string(), Some("h")))
        );
        assert_eq!(
            relative_destination("../x.md?q=1"),
            Some(("../x.md".to_string(), None))
        );
        for other in [
            "https://e.com/a.md",
            "mailto:a@b",
            "C:/a.md",
            "/abs.md",
            "\\\\host\\a.md",
            "%2F%2Fhost/a.md",
            "#only",
            "",
        ] {
            assert_eq!(relative_destination(other), None, "{other:?}");
        }
    }
}
