//! Finds and writes bare links and wiki-links in a plain text run.

use super::*;

impl<'a> Builder<'a> {
    /// The autolink pass, run on each finished text run so that links become ordinary spans in order.
    pub(super) fn flush_run(&mut self) {
        let Some(start) = self.run_start.take() else {
            return;
        };
        let mut escapes = std::mem::take(&mut self.escapes);
        let mut named = std::mem::take(&mut self.entities);
        let start = if self.wiki_open != NONE {
            self.close_wiki_label(start, &mut escapes, &mut named)
        } else {
            start
        };
        let wiki = if self.link_depth == 0 {
            self.find_wiki(start, &escapes)
        } else {
            Vec::new()
        };
        if self.link_depth == 0
            && (!wiki.is_empty() || autolink::may_contain_link(&self.doc.text[start..]))
        {
            let mut hits = std::mem::take(&mut self.link_hits);
            hits.clear();
            // An escaped character is a wall: nothing links across it or through it, as in cmark-gfm
            // where the escape splits the text node. A wiki-link is one too.
            let merged: Vec<(usize, usize)>;
            let walls: &[(usize, usize)] = if wiki.is_empty() {
                &escapes
            } else {
                let mut all = escapes.clone();
                all.extend(wiki.iter().map(|w| (w.start, w.end)));
                all.sort_unstable();
                merged = all;
                &merged
            };
            let mut pos = start;
            for &(a, b) in walls {
                if a >= pos {
                    let entities = Entities::new(&named, pos);
                    autolink::find_links_into(&self.doc.text[pos..a], pos, entities, &mut hits);
                }
                pos = pos.max(b);
            }
            let entities = Entities::new(&named, pos);
            autolink::find_links_into(&self.doc.text[pos..], pos, entities, &mut hits);
            if wiki.is_empty() {
                for &(s, e, kind) in &hits {
                    let dest_text = kind.destination(&self.doc.text[s..e]);
                    let dest = self.pool(&dest_text);
                    let idx = self.doc.links.len() as u32;
                    self.doc.links.push(LinkInfo::new(dest, StrRef::default()));
                    self.doc.spans.push(Span {
                        start: s as u32,
                        end: e as u32,
                        parent: self.cur_span,
                        kind: SpanKind::Link(idx),
                    });
                }
            } else {
                self.rewrite_run(start, wiki, &hits);
            }
            self.link_hits = hits;
        }
        if let Some(open) = self.wiki_pending.take() {
            self.open_wiki_label(open);
        }
        escapes.clear();
        self.escapes = escapes;
        named.clear();
        self.entities = named;
    }

    /// The wiki-links in the text run from `start` to the end of the text, in order. A pair of
    /// brackets counts when neither is escaped, no `!` stands before it (an embed stays text),
    /// the inside holds no bracket or hard break, `parse_target` accepts it, it is not a web
    /// address, and the application wants a link for that note. One pass over the run: a search
    /// for the closing pair ends at the next bracket, and the next opening pair cannot start
    /// before that bracket.
    fn find_wiki(&mut self, start: usize, escapes: &[(usize, usize)]) -> Vec<WikiHit> {
        let mut out = Vec::new();
        if memchr::memmem::find(&self.doc.text.as_bytes()[start..], b"[[").is_none() {
            return out;
        }
        // Spans are in order of their start, so the last one says whether any begins inside the
        // run; then the run is not plain text and is left alone.
        if self
            .doc
            .spans
            .last()
            .is_some_and(|s| s.start as usize > start)
        {
            return out;
        }
        let escaped = |at: usize| {
            let after = escapes.partition_point(|&(a, _)| a <= at);
            after > 0 && at < escapes[after - 1].1
        };
        let mut i = start;
        loop {
            let b = self.doc.text.as_bytes();
            let Some(open) = memchr::memmem::find(&b[i..], b"[[").map(|at| at + i) else {
                break;
            };
            let inner = open + 2;
            let close = b[inner..]
                .iter()
                .position(|c| matches!(c, b'[' | b']' | b'\n'))
                .map(|at| at + inner);
            let Some(close) = close else {
                let bare =
                    !(escaped(open) || escaped(open + 1) || (open > 0 && b[open - 1] == b'!'));
                if bare && self.wiki_open == NONE {
                    self.wiki_pending = self.wiki_label_start(open);
                }
                break;
            };
            let closed = b[close] == b']' && b.get(close + 1) == Some(&b']');
            // A `!` right before is an embed when it is in this run and not escaped; one that is
            // not stays text, brackets and all.
            let bang = open > 0 && b[open - 1] == b'!';
            let embed = bang && open > start && !escaped(open - 1);
            let plain = !(escaped(open) || escaped(open + 1) || escaped(close))
                && !escaped(close + 1)
                && !(bang && !embed);
            if !closed || !plain {
                // `[[[x]]` opens at its second bracket: the next pair may start one before a
                // bracket that directly follows this pair.
                i = match (b[close], close == inner) {
                    (b'[', true) => open + 1,
                    (b'[', false) => close,
                    _ => close + 1,
                };
                continue;
            }
            i = close + 2;
            let text = self.doc.text[inner..close].to_string();
            let Some(target) = mdshared_app::wikilink::parse_target(&text) else {
                continue;
            };
            if target.note.contains("://") {
                continue;
            }
            let dest = match target.heading {
                Some(heading) => format!("{}#{heading}", target.note),
                None => target.note.to_string(),
            };
            let picture = embed && target.heading.is_none() && is_image_name(target.note);
            let answer = (self.wiki)(target.note, embed);
            let (kind, missing) = match answer {
                WikiAs::Link if picture => {
                    // A size (`|200`, `|200x100`) is not a label; the picture keeps its own.
                    let sized = |l: &str| {
                        l.split('x')
                            .all(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
                    };
                    let alt = target.label.filter(|l| !sized(l)).unwrap_or("");
                    (HitKind::Image(alt.to_string()), false)
                }
                // A picture's name with a heading names no note.
                WikiAs::Link if embed && is_image_name(target.note) => continue,
                WikiAs::Link => (HitKind::Link, false),
                // An embed is never drawn as a missing link.
                WikiAs::Missing if !embed => (HitKind::Link, true),
                WikiAs::Missing => continue,
                WikiAs::Text => {
                    let note = if embed {
                        format!("!{}", target.note)
                    } else {
                        target.note.to_string()
                    };
                    let note = self.pool(&note);
                    self.doc.wiki_text.push(note);
                    continue;
                }
            };
            out.push(WikiHit {
                start: if embed { open - 1 } else { open },
                end: close + 2,
                shown: target.display(),
                dest,
                missing,
                kind,
            });
        }
        out
    }

    /// The run ends inside `[[Note|label` and the label goes on after markup: the source after the
    /// run reaches a `]]` on the same line, through nothing that could end the label early (a
    /// bracket, a backslash, a tag, a line end, a code span). What the link would be, to open
    /// when the run is finished; `None` leaves the text as it is.
    fn wiki_label_start(&mut self, open: usize) -> Option<WikiOpen> {
        let inner = open + 2;
        let head = &self.doc.text[inner..];
        let pipe = head.find('|')?;
        let target = mdshared_app::wikilink::parse_target(head)?;
        if target.note.contains("://") {
            return None;
        }
        let label_head = &head[pipe + 1..];
        let floor = self.text_floor;
        let before = self.src.get(..floor)?;
        // The source must really end where this run does (an HTML text has no such place).
        if !before.ends_with(if label_head.is_empty() {
            "|"
        } else {
            label_head
        }) {
            return None;
        }
        if !wiki_label_closes(&self.src.as_bytes()[floor..]) {
            return None;
        }
        let dest = match target.heading {
            Some(heading) => format!("{}#{heading}", target.note),
            None => target.note.to_string(),
        };
        let note = target.note.to_string();
        let missing = match (self.wiki)(&note, false) {
            WikiAs::Link => false,
            WikiAs::Missing => true,
            WikiAs::Text => {
                let note = self.pool(&note);
                self.doc.wiki_text.push(note);
                return None;
            }
        };
        Some(WikiOpen {
            tail: self.doc.text.len() - open,
            prefix: 2 + pipe + 1,
            dest,
            missing,
        })
    }

    /// Takes `[[Note|` off the end of the finished run and opens the link there; the label's
    /// markup that follows becomes its children and `close_wiki_label` ends it at the `]]`.
    fn open_wiki_label(&mut self, w: WikiOpen) {
        let at = self.doc.text.len() - w.tail;
        if self.doc.spans.last().is_some_and(|s| s.start as usize > at) {
            // A bare address in the label's start made a span after the opening: leave the text.
            return;
        }
        self.cut_text(at, w.prefix);
        let dest = self.pool(&w.dest);
        let idx = self.doc.links.len() as u32;
        self.doc.links.push(LinkInfo {
            wiki: true,
            missing: w.missing,
            ..LinkInfo::new(dest, StrRef::default())
        });
        let span = self.push_span(SpanKind::Link(idx), Origin::Auto);
        self.doc.spans[span as usize].start = at as u32;
        self.wiki_open = span;
        self.heading_wiki |= self.in_heading;
    }

    /// With a markup label open and a run that holds its `]]`: takes the brackets out, ends the
    /// link there and returns where the rest of the run begins. Any other run, or one whose
    /// brackets are inside nested markup, leaves everything as it is.
    fn close_wiki_label(
        &mut self,
        start: usize,
        escapes: &mut [(usize, usize)],
        named: &mut [(usize, usize)],
    ) -> usize {
        let idx = self.wiki_open;
        if self.stack.last().map(|o| o.node) != Some(Node::Span(idx)) {
            return start;
        }
        let b = self.doc.text.as_bytes();
        let Some(at) = b[start..]
            .iter()
            .position(|c| matches!(c, b'[' | b']'))
            .map(|at| at + start)
        else {
            return start;
        };
        let walled = escapes.iter().any(|&(a, e)| a < at + 2 && at < e);
        if walled || b[at] != b']' || b.get(at + 1) != Some(&b']') {
            return start;
        }
        self.cut_text(at, 2);
        for (a, e) in escapes.iter_mut().chain(named.iter_mut()) {
            if *a >= at + 2 {
                *a -= 2;
                *e -= 2;
            }
        }
        let open = self.pop_open().expect("the link is on the stack");
        self.close(open);
        self.doc.spans[idx as usize].end = at as u32;
        at
    }

    /// Removes `n` bytes of text at `at` and moves the marks after them back.
    fn cut_text(&mut self, at: usize, n: usize) {
        self.doc.text.replace_range(at..at + n, "");
        for m in self.doc.marks.iter_mut().rev() {
            if (m.text as usize) <= at {
                break;
            }
            m.text = (m.text as usize - n).max(at) as u32;
        }
    }

    /// Writes the run from `start` again with each wiki-link replaced by the text it shows, and
    /// records the wiki-links and the bare-URL `hits` (found in the run as it was) as spans at
    /// their new places, in order.
    fn rewrite_run(&mut self, start: usize, wiki: Vec<WikiHit>, hits: &[(usize, usize, LinkKind)]) {
        let tail = self.doc.text.split_off(start);
        let piece = |a: usize, b: usize| &tail[a - start..b - start];
        let mut hits = hits.iter().peekable();
        let mut pos = start;
        let link = |this: &mut Self, dest: &str, at: usize, missing: Option<bool>| {
            let dest = this.pool(dest);
            let idx = this.doc.links.len() as u32;
            this.doc.links.push(LinkInfo {
                wiki: missing.is_some(),
                missing: missing.unwrap_or(false),
                ..LinkInfo::new(dest, StrRef::default())
            });
            this.doc.spans.push(Span {
                start: at as u32,
                end: this.doc.text.len() as u32,
                parent: this.cur_span,
                kind: SpanKind::Link(idx),
            });
        };
        let mut wiki = wiki.into_iter().peekable();
        loop {
            let next_hit = hits.peek().map(|h| h.0);
            let next_wiki = wiki.peek().map(|w| w.start);
            match (next_hit, next_wiki) {
                (Some(h), w) if w.is_none_or(|w| h < w) => {
                    let &(s, e, kind) = hits.next().expect("peeked");
                    self.doc.text.push_str(piece(pos, s));
                    let at = self.doc.text.len();
                    self.doc.text.push_str(piece(s, e));
                    link(self, &kind.destination(piece(s, e)), at, None);
                    pos = e;
                }
                (_, Some(_)) => {
                    let w = wiki.next().expect("peeked");
                    self.doc.text.push_str(piece(pos, w.start));
                    match &w.kind {
                        HitKind::Link => {
                            let at = self.doc.text.len();
                            self.doc.text.push_str(&w.shown);
                            link(self, &w.dest, at, Some(w.missing));
                        }
                        HitKind::Image(alt) => {
                            let src = self.pool(&w.dest);
                            let alt = self.pool(alt);
                            let idx = self.doc.images.len() as u32;
                            self.doc.images.push(ImageInfo {
                                src,
                                alt,
                                ..ImageInfo::default()
                            });
                            self.atom_end = self.doc.text.len();
                            self.push_atom_like(SpanKind::Image(idx), self.doc.text.len());
                        }
                    }
                    pos = w.end;
                }
                _ => break,
            }
        }
        self.doc.text.push_str(piece(pos, start + tail.len()));
        self.heading_wiki |= self.in_heading;
    }
}

/// A wiki-link found in a text run: its place in the text as written, what it shows instead, the
/// destination kept for it (`Note#Heading`), and whether its note is missing.
struct WikiHit {
    start: usize,
    end: usize,
    shown: String,
    dest: String,
    missing: bool,
    kind: HitKind,
}

/// What a [`WikiHit`] becomes.
enum HitKind {
    /// A link showing the label or the note's name.
    Link,
    /// A picture, with this alt text (often empty).
    Image(String),
}

/// A wiki-link label that goes on after the end of a text run, to open when the run is done: how
/// far its `[[` is from the end of the text, how long `[[Note|` is, where it points.
pub(super) struct WikiOpen {
    tail: usize,
    prefix: usize,
    dest: String,
    missing: bool,
}

/// Whether `rest`, the source after the start of a wiki-link label, reaches `]]` on its line with
/// nothing in between that could end or change the label: a bracket, a backslash, a tag, an
/// unfinished code span. A `]]` inside a code span does not count.
fn wiki_label_closes(rest: &[u8]) -> bool {
    let mut i = 0;
    let mut code: Option<usize> = None;
    while i < rest.len() {
        match rest[i] {
            b'`' => {
                let n = rest[i..].iter().take_while(|&&c| c == b'`').count();
                code = match code {
                    None => Some(n),
                    Some(k) if k == n => None,
                    k => k,
                };
                i += n;
                continue;
            }
            b'\\' | b'<' | b'\n' | b'[' => return false,
            b']' if code.is_none() => return rest.get(i + 1) == Some(&b']'),
            _ => {}
        }
        i += 1;
    }
    false
}

/// `&name;` with letters only. GitHub's URL trimming, working on the source, stops at anything else.
pub(super) fn is_named_entity(source: &str) -> bool {
    source
        .strip_prefix('&')
        .and_then(|s| s.strip_suffix(';'))
        .is_some_and(|name| !name.is_empty() && name.bytes().all(|c| c.is_ascii_alphabetic()))
}
