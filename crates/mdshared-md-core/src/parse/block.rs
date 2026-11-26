//! Builds and closes document blocks, including alerts and headings.

use super::inline::is_atom;
use super::*;

impl<'a> Builder<'a> {
    pub(super) fn push_open(&mut self, open: Open) {
        if let Origin::Html(tag) = open.origin {
            self.open_html[tag.slot()] += 1;
            self.html_depth += 1;
        }
        self.stack.push(open);
    }

    pub(super) fn pop_open(&mut self) -> Option<Open> {
        let open = self.stack.pop()?;
        if let Origin::Html(tag) = open.origin {
            self.open_html[tag.slot()] -= 1;
            self.html_depth -= 1;
        }
        Some(open)
    }

    pub(super) fn md_block(&mut self, kind: BlockKind, inline: bool, range: Range<usize>) {
        self.close_inline_for_block();
        self.inline_limit = range.end;
        // A cell whose table was flattened away still has inline content to hold.
        let kind = match kind {
            BlockKind::TableCell { .. } if !self.parent_is(Tag::Tr) => BlockKind::Plain,
            _ => kind,
        };
        if self.flattened(kind) {
            return self.push_open(Open {
                node: Node::Nothing,
                origin: Origin::Md,
            });
        }
        self.open_block(kind, Origin::Md);
        if inline {
            self.enter_inline();
        }
    }

    /// GitHub turns a marked quote into an alert only if it holds something, and only outside a
    /// quote, list item, `<div>` or `<details>`, as measured with `gh api markdown`. That includes a
    /// footnote, since GitHub draws one as a list item.
    pub(super) fn block_quote(&mut self, alert: Option<AlertKind>, range: Range<usize>) {
        let marker = alert.and_then(|_| {
            let b = self.src.as_bytes();
            let at = range.start + memchr::memchr(b'[', &b[range.start..range.end])?;
            b[at..].starts_with(b"[!").then_some(at)
        });
        let (Some(alert), Some(marker)) = (alert, marker) else {
            let kind = alert.map_or(BlockKind::BlockQuote, BlockKind::Alert);
            return self.md_block(kind, false, range);
        };
        self.close_inline_for_block();
        if self.alert_allowed_here() {
            self.alert_content = false;
            self.md_block(BlockKind::Alert(alert), false, range);
            // Unless it was flattened, deep inside tags that don't rule out an alert.
            if matches!(self.doc.blocks.last(), Some(b) if matches!(b.kind, BlockKind::Alert(_))) {
                self.open_alert = Some((self.cur_block, marker));
            }
        } else {
            self.rejected_alerts.push(marker);
            self.md_block(BlockKind::BlockQuote, false, range);
            if self.literal_alerts {
                self.push_marker_paragraph(marker);
            }
        }
    }

    /// Walked from the innermost entry: the first Markdown container or rejecting HTML tag ends it,
    /// and a table cell resets the `<div>`, list and quote ancestors outside it. Only `<details>`
    /// still rejects inside a cell. That is how GitHub behaves, measured with `gh api markdown`.
    fn alert_allowed_here(&self) -> bool {
        if self
            .stack
            .iter()
            .any(|o| matches!((o.origin, o.node), (Origin::Html(Tag::Details), _)))
        {
            return false;
        }
        for o in self.stack.iter().rev() {
            match (o.origin, o.node) {
                (Origin::Html(Tag::Td | Tag::Th), _) => return true,
                (Origin::Html(tag), _) => {
                    if matches!(tag, Tag::Div | Tag::Blockquote | Tag::Li) {
                        return false;
                    }
                }
                (Origin::Md, Node::Block(b)) => {
                    if matches!(
                        self.doc.blocks[b as usize].kind,
                        BlockKind::BlockQuote
                            | BlockKind::Alert(_)
                            | BlockKind::ListItem { .. }
                            | BlockKind::FootnoteDefinition { .. }
                    ) {
                        return false;
                    }
                }
                (Origin::Md, Node::Nothing) => {}
                _ => {}
            }
        }
        true
    }

    pub(super) fn first_in_open_alert(&self) -> bool {
        matches!(self.open_alert, Some((b, _)) if b as usize + 1 == self.doc.blocks.len())
    }

    /// Whether a block starting at `start` touches the open alert's marker line. A blank quote line
    /// between them means it is an ordinary child block; otherwise a setext underline or thematic
    /// break rewrites the marker line itself, which leaves a plain quote on GitHub.
    pub(super) fn underlines_open_alert(&self, start: usize) -> bool {
        let Some((_, marker)) = self.open_alert else {
            return false;
        };
        let from = self.src[marker..]
            .find('\n')
            .map_or(self.src.len(), |i| marker + i + 1);
        if from >= start {
            return true;
        }
        let region = &self.src[from..start];
        let parts: Vec<&str> = region.split('\n').collect();
        // The last piece is only the prefix of the line `start` sits on, not a complete line.
        !parts[..parts.len().saturating_sub(1)]
            .iter()
            .any(|line| is_blank_quote_line(line))
    }

    pub(super) fn reject_open_alert(&mut self) {
        let Some((b, marker)) = self.open_alert.take() else {
            return;
        };
        self.rejected_alerts.push(marker);
        if self.literal_alerts {
            self.doc.blocks[b as usize].kind = BlockKind::BlockQuote;
            self.push_marker_paragraph(marker);
        }
    }

    /// Only for a document that is keeping its result, since hiding the marker reads better: this
    /// can't join the marker to a paragraph that follows it.
    fn push_marker_paragraph(&mut self, marker: usize) {
        let tail = &self.src[marker..];
        let text = &tail[..tail.find(']').map_or(0, |i| i + 1)];
        let p = self.open_block(BlockKind::Paragraph { align: Align::None }, Origin::Md);
        self.doc.text.push_str(text);
        self.pop_open();
        self.finish_block(p);
    }

    pub(super) fn parent_is(&self, tag: Tag) -> bool {
        self.cur_block != NONE
            && match self.doc.blocks[self.cur_block as usize].kind {
                BlockKind::Table => tag == Tag::Table,
                BlockKind::TableRow { .. } => tag == Tag::Tr,
                BlockKind::List { .. } => tag == Tag::Ul,
                _ => false,
            }
    }

    /// Whether a container goes without a block of its own. Rows, cells and items follow their
    /// parent, so a table or list either fits whole or leaves no part of itself behind.
    pub(super) fn flattened(&self, kind: BlockKind) -> bool {
        match kind {
            BlockKind::Paragraph { .. }
            | BlockKind::Plain
            | BlockKind::Heading { .. }
            | BlockKind::CodeBlock { .. }
            | BlockKind::Diagram { .. }
            | BlockKind::Math { .. }
            | BlockKind::ThematicBreak => false,
            BlockKind::ListItem { .. } => !self.parent_is(Tag::Ul),
            BlockKind::TableRow { .. } => !self.parent_is(Tag::Table),
            BlockKind::TableCell { .. } => !self.parent_is(Tag::Tr),
            _ => self.block_depth >= MAX_BLOCK_DEPTH,
        }
    }

    /// Keeps the shape `BlockKind` promises when content arrives where a table or list has no room
    /// for it. The implied parents count as real tags, so a later `<tr>` or `</table>` ends them.
    fn fit_parent(&mut self, kind: BlockKind) {
        if self.cur_block == NONE
            || matches!(
                kind,
                BlockKind::TableRow { .. }
                    | BlockKind::TableCell { .. }
                    | BlockKind::ListItem { .. }
            )
        {
            return;
        }
        match self.doc.blocks[self.cur_block as usize].kind {
            BlockKind::Table => {
                self.open_implied_row();
                self.open_implied_cell();
            }
            BlockKind::TableRow { .. } => self.open_implied_cell(),
            BlockKind::List { .. } => {
                let list = Node::Block(self.cur_block);
                let at = self.stack.iter().rposition(|o| o.node == list);
                match at {
                    // A list that only stood in for a missing one ends at the first thing that isn't
                    // an item, because in a browser there was no list to stay inside.
                    Some(i) if self.stack[i].origin == Origin::Auto => self.pop_to(i),
                    _ => {
                        let b = self
                            .open_block(BlockKind::ListItem { task: false }, Origin::Html(Tag::Li));
                        self.implied.push(b);
                        self.doc.block_attrs.push(BlockAttrs {
                            block: b,
                            marker: Some(ListMarker::None),
                            ..BlockAttrs::default()
                        });
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn open_implied_row(&mut self) {
        let b = self.open_block(BlockKind::TableRow { header: false }, Origin::Html(Tag::Tr));
        self.implied.push(b);
    }

    fn open_implied_cell(&mut self) {
        let cell = BlockKind::TableCell {
            align: Align::None,
            header: false,
            colspan: 1,
            rowspan: 1,
        };
        let b = self.open_block(cell, Origin::Html(Tag::Td));
        self.implied.push(b);
    }

    /// Closes everything down to and including the innermost entry a Markdown event opened. A
    /// placeholder left by `split_md_inline` stands for something already closed, so HTML blocks
    /// opened after it stay open for what follows, as they would in a browser.
    pub(super) fn end(&mut self) {
        self.flush_run();
        self.in_code_block = false;
        let md = self.stack.iter().rposition(|o| o.origin == Origin::Md);
        let keep_from = md
            .filter(|&i| self.stack[i].node == Node::Nothing)
            .and_then(|i| {
                let above = &self.stack[i + 1..];
                let html_block = above.iter().rposition(|o| match o.node {
                    Node::Block(b) => {
                        matches!(o.origin, Origin::Html(_))
                            && !matches!(
                                self.doc.blocks[b as usize].kind,
                                BlockKind::CodeBlock { .. }
                            )
                    }
                    _ => false,
                })?;
                Some((i, i + 1 + html_block + 1))
            });
        if let Some((placeholder, keep)) = keep_from {
            while self.stack.len() > keep {
                if let Some(open) = self.pop_open() {
                    self.close(open);
                }
            }
            self.stack.remove(placeholder);
        } else {
            while let Some(open) = self.pop_open() {
                self.close(open);
                if open.origin == Origin::Md {
                    break;
                }
            }
        }
        let depth = self.stack.len();
        self.ambient.retain(|a| a.depth <= depth);
    }

    pub(super) fn close(&mut self, open: Open) {
        match open.node {
            Node::Block(b) => self.finish_block(b),
            Node::Span(s) => self.finish_span(s),
            Node::Nothing => {}
        }
    }

    pub(super) fn finish_span(&mut self, s: u32) {
        let end = self.doc.text.len() as u32;
        let span = &mut self.doc.spans[s as usize];
        span.end = end;
        self.cur_span = span.parent;
        if self.wiki_open == s {
            self.wiki_open = NONE;
        }
        match span.kind {
            SpanKind::Link(_) => self.link_depth -= 1,
            SpanKind::Strong => self.strong_depth -= 1,
            _ => {}
        }
    }

    pub(super) fn open_block(&mut self, kind: BlockKind, origin: Origin) -> u32 {
        self.in_rp = false;
        // A block an HTML tag opened is content even when it ends empty, as GitHub keeps the alert.
        if self.open_alert.is_some() && !matches!(origin, Origin::Md) {
            self.alert_content = true;
        }
        self.fit_parent(kind);
        self.block_depth += 1;
        let idx = self.doc.blocks.len() as u32;
        let text = self.doc.text.len() as u32;
        let spans = self.doc.spans.len() as u32;
        let line = self.line_of(self.event_at);
        self.doc.blocks.push(Block {
            kind,
            parent: self.cur_block,
            end: idx + 1,
            text: TextRange {
                start: text,
                end: text,
            },
            spans: TextRange {
                start: spans,
                end: spans,
            },
            line,
        });
        self.push_open(Open {
            node: Node::Block(idx),
            origin,
        });
        self.cur_block = idx;
        if let Some(side) = self.pending_clear.take() {
            self.doc.clears.push(ClearAt { block: idx, side });
        }
        idx
    }

    pub(super) fn finish_block(&mut self, b: u32) {
        if matches!(self.open_alert, Some((a, _)) if a == b) {
            if self.doc.blocks.len() == b as usize + 1 && !self.alert_content {
                self.reject_open_alert();
            }
            self.open_alert = None;
            self.alert_content = false;
        }
        self.block_depth -= 1;
        self.in_rp = false;
        if self.in_picture && self.picture_block == b {
            self.leave_picture();
        }
        let text_end = self.doc.text.len();
        let spans_end = self.doc.spans.len() as u32;
        let blocks_end = self.doc.blocks.len() as u32;
        let block = &mut self.doc.blocks[b as usize];
        block.end = blocks_end;
        block.text.end = text_end as u32;
        let has_children = blocks_end > b + 1;
        // A container's range would otherwise cover its children's spans too.
        block.spans.end = if has_children {
            block.spans.start
        } else {
            spans_end
        };
        self.cur_block = block.parent;
        let kind = block.kind;
        let start = block.text.start as usize;
        match kind {
            BlockKind::Paragraph { .. } => self.promote_display_math(b),
            BlockKind::Heading { .. } => self.finish_heading(b),
            BlockKind::Diagram { lang, .. } => {
                let oversize = if text_end - start > MAX_DIAGRAM_BYTES {
                    Some(Oversize::Diagram)
                } else if crate::limits::unsupported_diagram(&self.doc.text[start..text_end]) {
                    Some(Oversize::UnsupportedDiagram)
                } else {
                    None
                };
                self.doc.blocks[b as usize].kind = BlockKind::Diagram { lang, oversize };
                // Only a size is worth a warning; an unsupported family says so where it is drawn.
                self.warn_oversize(oversize.filter(|o| *o == Oversize::Diagram));
            }
            BlockKind::Math { .. } => {
                let oversize =
                    (text_end - start > MAX_EQUATION_BYTES).then_some(Oversize::Equation);
                self.doc.blocks[b as usize].kind = BlockKind::Math { oversize };
                self.warn_oversize(oversize);
            }
            _ => {}
        }
        let leaf = matches!(
            kind,
            BlockKind::Paragraph { .. }
                | BlockKind::Plain
                | BlockKind::Heading { .. }
                | BlockKind::CodeBlock { .. }
                | BlockKind::Diagram { .. }
                | BlockKind::Math { .. }
        ) || (matches!(kind, BlockKind::TableCell { .. }) && !has_children);
        if leaf && text_end > start && !self.doc.text.ends_with('\n') {
            // Keeps find from matching across block boundaries. Not part of any block's own range.
            self.doc.text.push('\n');
        }
        if self.inline_open && self.inline_block == b {
            self.inline_open = false;
        }
        if matches!(kind, BlockKind::CodeBlock { .. }) {
            self.pre_open = false;
        }
        // An implied row, cell or item that never got content would draw an empty line or a phantom
        // column. GitHub moves such stray content out of the table; keeping the shape without the
        // empty node is the closest this model can do. Empty inline tags such as `<b></b>` leave an
        // empty container child, so the whole subtree goes: no text of its own and no atom anywhere.
        if self.implied.last() == Some(&b) {
            self.implied.pop();
            let block = self.doc.blocks[b as usize];
            let empty = block.text.is_empty()
                && !self.doc.spans[block.spans.start as usize..]
                    .iter()
                    .any(is_atom);
            if empty {
                self.drop_last_block(b);
                return;
            }
        }
        // `<a id="x"></a>` alone on a line parses as a paragraph that would only add a gap. A browser
        // gives an empty paragraph no height. Anchors noted against it land on the next block.
        let block = self.doc.blocks[b as usize];
        if matches!(block.kind, BlockKind::Paragraph { .. })
            && !has_children
            && block.text.is_empty()
            && !self.doc.spans[block.spans.usize()].iter().any(is_atom)
        {
            self.drop_last_block(b);
        }
    }

    pub(super) fn drop_last_block(&mut self, b: u32) {
        let block = self.doc.blocks[b as usize];
        self.doc.spans.truncate(block.spans.start as usize);
        self.doc.blocks.truncate(b as usize);
        while self.doc.block_attrs.last().is_some_and(|a| a.block >= b) {
            self.doc.block_attrs.pop();
        }
        // A clear the dropped block held belongs to whatever block takes its place.
        while let Some(c) = self.doc.clears.pop_if(|c| c.block >= b) {
            self.pending_clear = Some(c.side);
        }
        // An empty implied container can hold an empty heading or an anchor; both would point past
        // the end of the tree once it goes.
        while self.doc.headings.last().is_some_and(|h| h.block >= b) {
            self.doc.headings.pop();
        }
        // An anchor noted for the dropped container, or for the block that would have followed it,
        // belongs to whatever block takes its place.
        for a in &mut self.doc.anchors {
            a.block = a.block.min(b);
        }
        // A definition with no text can itself have been what the empty container held.
        for f in &mut self.doc.footnotes {
            if f.block != NONE && f.block >= b {
                f.block = NONE;
            }
        }
    }

    pub(super) fn warn_oversize(&mut self, oversize: Option<Oversize>) {
        if let Some(o) = oversize {
            self.doc.warnings.push(Warning::Oversize(o));
        }
    }

    fn promote_display_math(&mut self, b: u32) {
        let block = self.doc.blocks[b as usize];
        if block.spans.len() != 1 {
            return;
        }
        let span = self.doc.spans[block.spans.start as usize];
        if let SpanKind::Math {
            display: true,
            oversize,
        } = span.kind
            && span.start == block.text.start
            && span.end == block.text.end
        {
            self.doc.spans.pop();
            let block = &mut self.doc.blocks[b as usize];
            block.spans.end = block.spans.start;
            block.kind = BlockKind::Math { oversize };
        }
    }

    fn finish_heading(&mut self, b: u32) {
        let block = self.doc.blocks[b as usize];
        let BlockKind::Heading { level, align, .. } = block.kind else {
            return;
        };
        // The outline shows one line, so a hard break in the heading reads as a space there.
        let raw = self.doc.text[block.text.usize()].trim();
        // With a wiki-link in it, the anchor comes from the text shown, not the brackets.
        let shown = std::mem::take(&mut self.heading_wiki).then(|| raw.to_string());
        let title = if raw.contains('\n') {
            let flat = raw.replace('\n', " ");
            self.pool(&flat)
        } else {
            let start = self.doc.strings.len() as u32;
            self.doc.strings.push_str(raw);
            StrRef {
                start,
                len: raw.len() as u32,
            }
        };
        let slug = match shown {
            Some(shown) => self.slugger.slug(&shown),
            None => self.slugger.slug(&self.heading_text),
        };
        self.in_heading = false;
        let slug = self.pool(&slug);
        self.doc.blocks[b as usize].kind = BlockKind::Heading { level, align, slug };
        self.doc.headings.push(Heading {
            level,
            block: b,
            title,
            slug,
        });
    }

    /// Slugs come from the heading's text before whitespace collapses, as GitHub's do.
    pub(super) fn note_heading_text(&mut self, t: &str) {
        if !self.in_heading {
            return;
        }
        if self.restore {
            self.heading_text.push_str(&autolink::restore(t));
        } else {
            self.heading_text.push_str(t);
        }
    }
}

/// A line inside a quote that shows nothing: just the `>` markers, or a lone HTML comment, which
/// GitHub ignores. Such a line separates a marker from the block after it.
fn is_blank_quote_line(line: &str) -> bool {
    let mut s = line.trim();
    while let Some(rest) = s.strip_prefix('>') {
        s = rest.trim_start();
    }
    s.is_empty() || (s.starts_with("<!--") && s.ends_with("-->"))
}

/// ATX headings need one to six `#` followed by a space or the end of the line. `#x` is a setext
/// heading's text, not an ATX heading, and that distinction picks the right alert rule.
pub(super) fn is_atx_heading(s: &str) -> bool {
    let indent = s.len() - s.trim_start_matches(' ').len();
    if indent > 3 {
        return false;
    }
    let s = &s[indent..];
    let hashes = s.bytes().take_while(|&c| c == b'#').count();
    (1..=6).contains(&hashes)
        && s.as_bytes()
            .get(hashes)
            .is_none_or(|&c| c == b' ' || c == b'\t')
}

/// `-`, `--` and longer runs are setext underlines to GitHub, though pulldown-cmark only makes a
/// heading of `---` and longer.
pub(super) fn is_dash_run(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty() && t.bytes().all(|c| c == b'-')
}
