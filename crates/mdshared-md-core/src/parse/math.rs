//! Checks math delimiters and reparses ambiguous math and alerts.

use super::*;

/// GitHub writes inline math as ``$`x`$`` so that Markdown cannot touch the formula. This rewrites
/// each such pair on one line to `$x$`, outside fenced code. A formula containing `$` or a backtick
/// is left alone. Documents without the two-character opener are not copied.
pub(super) fn github_math(src: &str) -> Cow<'_, str> {
    if memchr::memmem::find(src.as_bytes(), b"$`").is_none() {
        return Cow::Borrowed(src);
    }
    let mut out = String::with_capacity(src.len());
    let mut fence: Option<(char, usize)> = None;
    let mut changed = false;
    for line in src.split_inclusive('\n') {
        let indent = line.len() - line.trim_start_matches(' ').len();
        let rest = line[indent..].trim_end_matches(['\n', '\r']);
        let marker = rest.chars().next().filter(|c| matches!(c, '`' | '~'));
        let run = marker.map_or(0, |m| rest.chars().take_while(|&c| c == m).count());
        match (fence, marker) {
            (Some((m, n)), Some(c))
                if indent < 4 && c == m && run >= n && rest[run..].trim().is_empty() =>
            {
                fence = None;
            }
            (None, Some(c))
                if indent < 4 && run >= 3 && !(c == '`' && rest[run..].contains('`')) =>
            {
                fence = Some((c, run));
            }
            _ => {}
        }
        if fence.is_some() || marker.is_some() && run >= 3 || !line.contains("$`") {
            out.push_str(line);
            continue;
        }
        let mut at = 0;
        while let Some(open) = line[at..].find("$`").map(|i| at + i) {
            let body = open + 2;
            let close = line[body..].find("`$").map(|i| body + i);
            match close {
                Some(close) if close > body && !line[body..close].contains(['`', '$']) => {
                    out.push_str(&line[at..open]);
                    out.push('$');
                    out.push_str(&line[body..close]);
                    out.push('$');
                    at = close + 2;
                    changed = true;
                }
                _ => {
                    out.push_str(&line[at..body]);
                    at = body;
                }
            }
        }
        out.push_str(&line[at..]);
    }
    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(src)
    }
}

/// Passes that may hide rejected delimiters before the rest are shown as literal text. The first
/// usually settles everything, so only a hostile input reaches the limit.
const MATH_PASSES: usize = 4;

/// pulldown-cmark pairs `$` with no regard for what surrounds it, and whatever Markdown sits between
/// a bad pair is already lost by the time the event arrives. It also takes an alert marker wherever
/// it finds one, and the line it takes may have belonged to a paragraph. So rejected delimiters are
/// hidden from the parser and the source is parsed again. `rejected` holds those already known.
pub(super) fn parse_with_checks(
    mut src: Cow<'_, str>,
    mut restore: bool,
    rejected: Vec<usize>,
    front: Front,
    wiki: &mut dyn FnMut(&str, bool) -> WikiAs,
) -> Document {
    // Hiding swaps in characters from U+E003 upward. A document that already uses any character
    // from that private-use block would have its own text rewritten by `restore`, so such a
    // document takes the literal pass instead.
    let can_hide = memchr::memmem::find(src.as_bytes(), SWAP_PREFIX).is_none();
    if can_hide && !rejected.is_empty() {
        src = Cow::Owned(hide_delimiters(&src, &rejected));
        restore = true;
    }
    let passes = if can_hide { MATH_PASSES } else { 0 };
    for _ in 0..passes {
        match build_checked(&src, restore, false, front, wiki) {
            Ok(doc) => return doc,
            Err(rejected) => {
                src = Cow::Owned(hide_delimiters(&src, &rejected));
                restore = true;
            }
        }
    }
    match build_checked(&src, restore, true, front, wiki) {
        Ok(doc) => doc,
        Err(_) => unreachable!("the literal pass rejects nothing"),
    }
}

/// With `literal`, a rejected span or alert becomes plain text. Otherwise the first rejected span stops
/// the build, and the rest of the walk only collects the `$` offsets to hide.
fn build_checked(
    src: &str,
    restore: bool,
    literal: bool,
    front: Front,
    wiki: &mut dyn FnMut(&str, bool) -> WikiAs,
) -> Result<Document, Vec<usize>> {
    let mut b = Builder::new(src, restore, literal, front, wiki);
    let mut gate = MathGate {
        boundary: true,
        inline: 0..src.len(),
    };
    let mut rejected = Vec::new();
    let mut settled_until = 0;
    for (event, range) in Parser::new_ext(src, options()).into_offset_iter() {
        let is_math = matches!(event, Event::InlineMath(_) | Event::DisplayMath(_));
        let accepted = !is_math || gate.accepts(src, &range);
        gate.note(&event, &range);
        if accepted {
            if rejected.is_empty() {
                b.event(event, range);
            }
        } else if literal {
            b.event(Event::Text(src[range.clone()].into()), range);
        } else if range.start >= settled_until {
            // The rest of this block's pairs are unreliable now, because each came from pairing
            // with whatever the rejected ones used up. They all get decided here, at once.
            rejected.extend(rejected_dollars(src, gate.inline.clone()));
            rejected.push(range.start);
            settled_until = gate.inline.end;
        }
    }
    rejected.append(&mut b.rejected_alerts);
    if rejected.is_empty() || literal {
        Ok(b.into_document())
    } else {
        rejected.sort_unstable();
        rejected.dedup();
        Err(rejected)
    }
}

/// Each offset is at a `$` or at the `[` of an alert marker.
fn hide_delimiters(src: &str, offsets: &[usize]) -> String {
    let mut out = String::with_capacity(src.len() + offsets.len() * 2);
    let mut pos = 0;
    for &at in offsets {
        out.push_str(&src[pos..at]);
        out.push(if src.as_bytes()[at] == b'[' {
            autolink::HIDDEN_BRACKET
        } else {
            autolink::HIDDEN_DOLLAR
        });
        pos = at + 1;
    }
    out.push_str(&src[pos..]);
    out
}

/// GitHub's rule for what may touch a math span, measured against `gh api markdown` since it is not
/// documented. See docs/COVERAGE.md for where this differs.
struct MathGate {
    /// Whether whatever came just before the next event allows a span to open there.
    boundary: bool,
    /// Source of the innermost block that holds inline content.
    inline: Range<usize>,
}

impl MathGate {
    fn note(&mut self, event: &Event<'_>, range: &Range<usize>) {
        self.boundary = match event {
            Event::Text(t) => t
                .as_bytes()
                .last()
                .is_none_or(|c| c.is_ascii_whitespace() || *c == b'('),
            // `**$x$**` and `~~$x$~~` render, yet `*$x$*`, `***$x$***` and `[$x$](u)` do not.
            Event::Start(MdTag::Strong | MdTag::Strikethrough) => self.boundary,
            Event::Start(MdTag::Emphasis | MdTag::Link { .. } | MdTag::Image { .. }) => false,
            Event::InlineMath(_) | Event::DisplayMath(_) => false,
            Event::InlineHtml(t) | Event::Html(t) => match Tokenizer::new(t).next() {
                Some(Token::Close { .. }) => true,
                Some(Token::Open { name, .. }) => html::classify_tag(name) == Tag::Br,
                _ => false,
            },
            _ => true,
        };
        if let Event::Start(
            MdTag::Paragraph | MdTag::Heading { .. } | MdTag::TableCell | MdTag::Item,
        ) = event
        {
            self.inline = range.clone();
        }
    }

    fn accepts(&self, src: &str, range: &Range<usize>) -> bool {
        self.boundary && may_follow_math(src.as_bytes(), range.end)
    }
}

/// `$c$-d` and `$c$é` render, while `$c$d`, `$c$_` and a span ending an emphasis do not.
fn may_follow_math(b: &[u8], at: usize) -> bool {
    match b.get(at) {
        Some(b'_') => false,
        Some(b'*') => b.get(at + 1) == Some(&b'*'),
        Some(c) => !c.is_ascii_alphanumeric(),
        None => true,
    }
}

/// `MathGate`'s rule for the source text before `at`, for use without events.
fn may_precede_math(src: &str, at: usize, start: usize) -> bool {
    let b = src.as_bytes();
    if at <= start {
        return true;
    }
    match b[at - 1] {
        c if c.is_ascii_whitespace() => true,
        b'(' | b'`' => true,
        c @ (b'*' | b'~' | b'_') => {
            let run = b[start..at].iter().rev().take_while(|&&x| x == c).count();
            let closes_a_span = at - run > start && b[at - run - 1].is_ascii_alphanumeric();
            match c {
                b'*' => run == 2 || (run == 1 && closes_a_span),
                b'~' => run <= 2,
                _ => run == 1 && closes_a_span,
            }
        }
        b'>' => {
            // The longest tag that counts is `<br />`, and `</x>` needs only the slash to be seen.
            let from = start.max(at.saturating_sub(16));
            memchr::memrchr(b'<', &b[from..at]).is_some_and(|i| {
                let tag = &b[from + i + 1..at - 1];
                tag.first() == Some(&b'/')
                    || tag.trim_ascii_end().eq_ignore_ascii_case(b"br")
                    || tag
                        .strip_suffix(b"/")
                        .is_some_and(|t| t.trim_ascii_end().eq_ignore_ascii_case(b"br"))
            })
        }
        _ => false,
    }
}

/// Goes through one block's `$` delimiters in a single sweep, using the parser's pairing rule plus
/// GitHub's: a `$` can open if a non-space follows, it can close if a non-space precedes, and the next
/// delimiter is the only one it can pair with. Returns the offsets of every `$` not in a good pair.
///
/// An opener that fails takes only itself out, because its partner may still open a good span: in
/// `y$. **$a$**` the first pair would be `$. **$`, and `$a$` has to survive. A closer that fails takes
/// both, since GitHub shows all three of `$x$y$` literally.
fn rejected_dollars(src: &str, block: Range<usize>) -> Vec<usize> {
    let b = src.as_bytes();
    let d = math_delimiters(b, block.clone());
    let double_at = |k: usize| d.get(k + 1).is_some_and(|&next| next == d[k] + 1);
    let mut out = Vec::new();
    let mut k = 0;
    while k < d.len() {
        let width = if double_at(k) { 2 } else { 1 };
        let closer = k + width;
        let pairs = closer < d.len()
            && (width == 1 || double_at(closer))
            && !b[d[k] + width].is_ascii_whitespace()
            && !b[d[closer] - 1].is_ascii_whitespace();
        if pairs && may_precede_math(src, d[k], block.start) {
            if !may_follow_math(b, d[closer] + width) {
                out.extend(d[k..closer + width].iter().copied());
            }
            k = closer + width;
        } else {
            out.extend(d[k..closer].iter().copied());
            k = closer;
        }
    }
    out
}

/// Offsets of each `$`, leaving out escaped ones and those in code spans.
fn math_delimiters(b: &[u8], block: Range<usize>) -> Vec<usize> {
    const MAX_FENCE: usize = 32;
    let mut out = Vec::new();
    let mut no_closer = [false; MAX_FENCE];
    let mut i = block.start;
    while i < block.end {
        match b[i] {
            b'\\' => i += 2,
            b'`' => {
                let run = b[i..block.end].iter().take_while(|&&c| c == b'`').count();
                i += run;
                // Remembering a length with no closer keeps this linear on unmatched runs.
                if run < MAX_FENCE && !no_closer[run] {
                    match closing_backticks(&b[i..block.end], run) {
                        Some(end) => i += end,
                        None => no_closer[run] = true,
                    }
                }
            }
            b'$' => {
                out.push(i);
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

fn closing_backticks(b: &[u8], run: usize) -> Option<usize> {
    let mut i = 0;
    while let Some(at) = memchr::memchr(b'`', &b[i..]) {
        let start = i + at;
        let len = b[start..].iter().take_while(|&&c| c == b'`').count();
        if len == run {
            return Some(start + len);
        }
        i = start + len;
    }
    None
}
