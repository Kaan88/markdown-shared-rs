//! Builds inline spans, text and math within a block.

use super::text_links::is_named_entity;
use super::*;

impl<'a> Builder<'a> {
    // Inline content

    pub(super) fn enter_inline(&mut self) {
        self.inline_open = true;
        self.inline_block = self.cur_block;
        self.inline_start = self.doc.text.len();
        self.cur_span = NONE;
        self.in_heading = matches!(
            self.doc.blocks[self.cur_block as usize].kind,
            BlockKind::Heading { .. }
        );
        if self.in_heading {
            self.heading_text.clear();
        }
        // Every later block re-opens these, so a short file could otherwise ask for a span count
        // of tags times blocks. Real documents stay far under this budget.
        if self.ambient.len() > self.ambient_spans_left {
            self.ambient.clear();
            if !self.doc.warnings.contains(&Warning::UnclosedHtmlDropped) {
                self.doc.warnings.push(Warning::UnclosedHtmlDropped);
            }
        }
        self.ambient_spans_left -= self.ambient.len();
        for i in 0..self.ambient.len() {
            let (tag, kind) = (self.ambient[i].tag, self.ambient[i].kind);
            self.push_span(kind, Origin::Html(tag));
        }
    }

    /// One entry per tag: `<i><i>` looks the same as `<i>`, and a browser ends an open `<a>` when the
    /// next one starts. That bounds the list by the number of inline tags.
    fn carry_ambient(&mut self, tag: Tag, kind: SpanKind, depth: usize) {
        if let Some(i) = self.ambient.iter().position(|a| a.tag == tag) {
            self.ambient.remove(i);
        }
        self.ambient.push(Ambient { tag, kind, depth });
    }

    /// Makes sure text has somewhere to go: opens an anonymous block when no inline container is open.
    pub(super) fn begin_inline(&mut self) {
        if !self.inline_open {
            self.flush_run();
            self.open_block(BlockKind::Plain, Origin::Auto);
            self.enter_inline();
        }
    }

    pub(super) fn close_inline_for_block(&mut self) {
        self.flush_run();
        if !self.inline_open {
            return;
        }
        let closable = self
            .stack
            .iter()
            .rev()
            .find(|o| matches!(o.node, Node::Block(_)))
            .is_some_and(|o| o.origin != Origin::Md);
        if closable {
            self.close_inline_container();
        }
    }

    /// Pops through the open anonymous or HTML inline container, carrying its open HTML spans over.
    pub(super) fn close_inline_container(&mut self) {
        self.flush_run();
        let mut carried: Vec<(Tag, SpanKind)> = Vec::new();
        while let Some(open) = self.pop_open() {
            match open.node {
                Node::Span(s) => {
                    if let Origin::Html(tag) = open.origin {
                        carried.push((tag, self.doc.spans[s as usize].kind));
                    }
                    self.finish_span(s);
                }
                Node::Block(b) => {
                    self.finish_block(b);
                    self.discard_if_only_carried(b, carried.len());
                    break;
                }
                Node::Nothing => {}
            }
        }
        let depth = self.stack.len();
        self.ambient.clear();
        for (tag, kind) in carried.into_iter().rev() {
            self.carry_ambient(tag, kind, depth);
        }
    }

    /// A block tag inside Markdown inline content. The Markdown block's end event is still to come,
    /// so placeholders stay on the stack for it and for any Markdown span open here. The formatting
    /// open at this point carries on inside the new blocks, until its own end event or closer.
    ///
    /// A paragraph closes as it would in a browser. A table cell stays open and takes block
    /// children, with what it holds so far moved into the first of them.
    pub(super) fn split_md_inline(&mut self, is_cell: bool) {
        self.flush_run();
        let mut spans: Vec<(Origin, SpanKind)> = Vec::new();
        let mut placeholders = 0;
        let mut block = None;
        while let Some(open) = self.pop_open() {
            match open.node {
                Node::Span(s) => {
                    spans.push((open.origin, self.doc.spans[s as usize].kind));
                    self.finish_span(s);
                }
                Node::Block(b) => {
                    block = Some((open, b));
                    break;
                }
                Node::Nothing if open.origin == Origin::Md => placeholders += 1,
                Node::Nothing => {}
            }
        }
        let Some((open, b)) = block else {
            return;
        };
        if is_cell {
            self.move_cell_inline_to_child(b);
            self.inline_open = false;
            self.push_open(open);
        } else {
            self.finish_block(b);
            self.push_open(Open {
                node: Node::Nothing,
                origin: Origin::Md,
            });
        }
        let mut depth = self.stack.len();
        for _ in 0..placeholders {
            self.push_open(Open {
                node: Node::Nothing,
                origin: Origin::Md,
            });
        }
        for (origin, kind) in spans.into_iter().rev() {
            match origin {
                Origin::Html(tag) => self.carry_ambient(tag, kind, depth),
                Origin::Md => {
                    self.push_open(Open {
                        node: Node::Nothing,
                        origin: Origin::Md,
                    });
                    depth += 1;
                    self.carry_ambient(md_span_tag(kind), kind, depth);
                }
                Origin::Auto => {}
            }
        }
    }

    fn move_cell_inline_to_child(&mut self, cell: u32) {
        let block = self.doc.blocks[cell as usize];
        let text_end = self.doc.text.len();
        let spans_end = self.doc.spans.len();
        let own = &self.doc.text[block.text.start as usize..];
        let has_atom = self.doc.spans[block.spans.start as usize..]
            .iter()
            .any(is_atom);
        if own.trim().is_empty() && !has_atom {
            self.doc.text.truncate(block.text.start as usize);
            self.doc.spans.truncate(block.spans.start as usize);
            return;
        }
        let idx = self.doc.blocks.len() as u32;
        self.doc.blocks.push(Block {
            kind: BlockKind::Plain,
            parent: cell,
            end: idx + 1,
            text: TextRange::new(block.text.start as usize, text_end),
            spans: TextRange::new(block.spans.start as usize, spans_end),
            line: block.line,
        });
        if !self.doc.text.ends_with('\n') {
            self.doc.text.push('\n');
        }
    }

    /// An anonymous block holding nothing but tags carried onward would draw as an empty line.
    fn discard_if_only_carried(&mut self, b: u32, carried: usize) {
        // finish_block may already have dropped an empty paragraph.
        let Some(&block) = self.doc.blocks.get(b as usize) else {
            return;
        };
        if matches!(block.kind, BlockKind::Plain | BlockKind::Paragraph { .. })
            && block.text.is_empty()
            && block.spans.len() == carried
        {
            self.drop_last_block(b);
        }
    }

    pub(super) fn push_span(&mut self, kind: SpanKind, origin: Origin) -> u32 {
        if self.open_alert.is_some() {
            self.alert_content = true;
        }
        if origin == Origin::Md && self.md_span_depth >= crate::limits::MAX_MD_SPAN_DEPTH {
            self.push_open(Open {
                node: Node::Nothing,
                origin,
            });
            return NONE;
        }
        let at = self.doc.text.len() as u32;
        let idx = self.doc.spans.len() as u32;
        self.doc.spans.push(Span {
            start: at,
            end: at,
            parent: self.cur_span,
            kind,
        });
        self.cur_span = idx;
        self.push_open(Open {
            node: Node::Span(idx),
            origin,
        });
        match kind {
            SpanKind::Link(_) => self.link_depth += 1,
            SpanKind::Strong => self.strong_depth += 1,
            _ => {}
        }
        idx
    }

    pub(super) fn push_atom(&mut self, kind: SpanKind) {
        self.flush_run();
        if let Some(tasks) = &mut self.task_events {
            tasks.atom();
        }
        if self.open_alert.is_some() {
            self.alert_content = true;
        }
        let at = self.doc.text.len() as u32;
        self.atom_end = at as usize;
        self.doc.spans.push(Span {
            start: at,
            end: at,
            parent: self.cur_span,
            kind,
        });
    }

    /// A span whose text was already appended, starting at `start`.
    pub(super) fn push_atom_like(&mut self, kind: SpanKind, start: usize) {
        if self.open_alert.is_some() {
            self.alert_content = true;
        }
        let end = self.doc.text.len() as u32;
        self.doc.spans.push(Span {
            start: start as u32,
            end,
            parent: self.cur_span,
            kind,
        });
    }

    pub(super) fn open_md_span(&mut self, kind: SpanKind) {
        self.begin_inline();
        self.flush_run();
        self.push_span(kind, Origin::Md);
    }

    pub(super) fn open_strong(&mut self, origin: Origin) {
        self.begin_inline();
        self.flush_run();
        if self.strong_depth > 0 {
            self.push_open(Open {
                node: Node::Nothing,
                origin,
            });
        } else {
            self.push_span(SpanKind::Strong, origin);
        }
    }

    pub(super) fn math(&mut self, t: &str, display: bool) {
        // Unlike code, math reaches here with the file's line endings.
        let normalized;
        let t = if memchr::memchr(b'\r', t.as_bytes()).is_some() {
            normalized = t.replace("\r\n", "\n").replace('\r', "\n");
            normalized.as_str()
        } else {
            t
        };
        self.begin_inline();
        self.flush_run();
        self.note_heading_text(t);
        let start = self.doc.text.len();
        self.mark(self.event_at, start);
        self.push_str_restored(t);
        let oversize =
            (self.doc.text.len() - start > MAX_EQUATION_BYTES).then_some(Oversize::Equation);
        self.warn_oversize(oversize);
        self.push_atom_like(SpanKind::Math { display, oversize }, start);
    }

    pub(super) fn push_str_restored(&mut self, t: &str) {
        if self.restore {
            self.doc.text.push_str(&autolink::restore(t));
        } else {
            self.doc.text.push_str(t);
        }
    }

    pub(super) fn md_text(&mut self, t: &str, range: Range<usize>) {
        if self.in_rp {
            return;
        }
        let swapped = crate::emoji::replace(t);
        let t: &str = &swapped;
        let escaped = self.is_escaped(range.start);
        self.text_floor = range.end;
        // Text resuming after a block tag, as in ` done` after `</ul>`, starts a block of its own.
        if !self.inline_open && t.bytes().all(|c| c.is_ascii_whitespace()) {
            return;
        }
        self.begin_inline();
        self.note_heading_text(t);
        let at = self.doc.text.len();
        let t = if at == self.inline_start && at != self.atom_end {
            t.trim_start_matches(|c: char| c.is_ascii_whitespace())
        } else {
            t
        };
        if self.run_start.is_none() {
            self.run_start = Some(at);
        }
        self.mark(range.start, at);
        if escaped && let Some(c) = t.chars().next() {
            self.escapes.push((at, at + c.len_utf8()));
        }
        self.push_inline_text(t);
        self.note_task_text(range.start, at);
        if self.src.as_bytes()[range.start] == b'&' && is_named_entity(&self.src[range]) {
            self.entities.push((at, self.doc.text.len()));
        }
    }

    /// Sends only emitted prose to the task guard. Code and ruby readings use their
    /// existing active spans, including spans carried across inline blocks.
    pub(super) fn note_task_text(&mut self, source: usize, start: usize) {
        if let Some(tasks) = &mut self.task_events
            && !self.in_rp
            && self.open_html[Tag::Code.slot()] == 0
            && self.open_html[Tag::Kbd.slot()] == 0
            && (self.open_html[Tag::Rt.slot()] == 0 || self.open_html[Tag::Ruby.slot()] == 0)
        {
            tasks.text(source, self.inline_block, start, &self.doc.text[start..]);
        }
    }

    /// Appends inline text with whitespace runs collapsed as HTML would show them, so the renderer
    /// and find both see one space. A decoded `&#10;` also ends up here, because a newline in inline
    /// text is reserved to mean a hard break.
    pub(super) fn push_inline_text(&mut self, t: &str) {
        let b = t.as_bytes();
        let plain = !b.iter().any(|&c| matches!(c, b'\t' | b'\n' | b'\r'))
            && memchr::memmem::find(b, b"  ").is_none();
        let len = self.doc.text.len();
        let after_space =
            len > self.inline_start && len != self.atom_end && self.doc.text.ends_with(' ');
        if plain && !(after_space && b.first() == Some(&b' ')) {
            self.push_str_restored(t);
            return;
        }
        let restored = if self.restore {
            autolink::restore(t)
        } else {
            std::borrow::Cow::Borrowed(t)
        };
        let mut pending = false;
        for c in restored.chars() {
            if matches!(c, ' ' | '\t' | '\n' | '\r') {
                pending = true;
                continue;
            }
            if pending {
                self.push_collapsed_space();
                pending = false;
            }
            self.doc.text.push(c);
        }
        if pending {
            self.push_collapsed_space();
        }
    }

    /// Whether the character at `start` follows an unescaped backslash. The walk stops where the last
    /// text event ended: a backslash before that is already spoken for, and a long run of them
    /// emits an event per pair, so walking further would be quadratic.
    fn is_escaped(&self, start: usize) -> bool {
        let b = self.src.as_bytes();
        let mut i = start;
        let mut n = 0;
        while i > self.text_floor && b[i - 1] == b'\\' {
            n += 1;
            i -= 1;
        }
        n % 2 == 1
    }

    pub(super) fn line_break(&mut self, c: char) {
        // Between block tags, as in `<ul>` and `<li>` on separate lines of one paragraph.
        if !self.inline_open {
            return;
        }
        if c == ' ' && self.doc.text.ends_with('\n') && self.doc.text.len() > self.inline_start {
            return;
        }
        if self.run_start.is_none() {
            self.run_start = Some(self.doc.text.len());
        }
        self.doc.text.push(c);
    }
}

pub(super) fn is_atom(s: &Span) -> bool {
    matches!(
        s.kind,
        SpanKind::Image(_) | SpanKind::Checkbox { .. } | SpanKind::FootnoteRef(_)
    )
}

/// The tag a Markdown span counts as once it carries on past a block tag.
fn md_span_tag(kind: SpanKind) -> Tag {
    match kind {
        SpanKind::Strong => Tag::Strong,
        SpanKind::Strike => Tag::Strike,
        SpanKind::Link(_) => Tag::A,
        _ => Tag::Em,
    }
}
