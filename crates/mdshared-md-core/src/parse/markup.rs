//! Turns embedded HTML and its attributes into the document model.

use super::*;

impl<'a> Builder<'a> {
    // Embedded HTML

    pub(super) fn end_html_block(&mut self) {
        let Some(buf) = self.html_buf.take() else {
            return;
        };
        self.html_tokens(&buf);
        self.html_store = buf;
        self.flush_run();
        // An unclosed `<picture>` ends with the HTML block that held it, as in a browser, so its
        // sources cannot leak onto a later image.
        self.leave_picture();
        if self.pre_open {
            self.close_html(Tag::Pre);
            self.pre_open = false;
        }
        // Only an anonymous container ends with the block. An explicit `<p>` or `<h1>` stays open for
        // the next HTML block, and a following Markdown block closes it the way a browser would.
        let top_block = self
            .stack
            .iter()
            .rev()
            .find(|o| matches!(o.node, Node::Block(_)));
        if self.inline_open && top_block.is_some_and(|o| o.origin == Origin::Auto) {
            self.close_inline_container();
        }
    }

    pub(super) fn html_tokens(&mut self, html: &str) {
        for tok in Tokenizer::new(html) {
            if self.pre_open {
                self.pre_token(tok);
                continue;
            }
            match tok {
                Token::Text(t) => self.html_text(t),
                Token::Ignored(_) => {}
                Token::Close { name, raw } => self.html_close(name, raw),
                // Self-closing syntax means nothing on HTML elements, so it is ignored as a browser does.
                Token::Open {
                    name, attrs, raw, ..
                } => self.html_open(name, attrs, raw),
            }
        }
    }

    fn pre_token(&mut self, tok: Token<'_>) {
        match tok {
            Token::Text(t) => self.pre_text(&html::decode_entities(t)),
            Token::Close { name, .. } if html::classify_tag(name) == Tag::Pre => {
                self.close_html(Tag::Pre);
                self.pre_open = false;
            }
            _ => self.pre_first_newline = false,
        }
    }

    pub(super) fn pre_text(&mut self, t: &str) {
        let s = if self.pre_first_newline {
            t.strip_prefix('\n').unwrap_or(t)
        } else {
            t
        };
        self.doc.text.push_str(s);
        self.pre_first_newline = false;
    }

    /// Text inside HTML collapses whitespace as a browser would. Whitespace between block tags
    /// produces nothing.
    fn html_text(&mut self, t: &str) {
        if self.in_rp {
            return;
        }
        let decoded = html::decode_entities(t);
        self.collapsed_text(&decoded);
    }

    fn collapsed_text(&mut self, t: &str) {
        if !self.inline_open && t.bytes().all(|c| c.is_ascii_whitespace()) {
            return;
        }
        self.begin_inline();
        self.flush_run();
        self.note_heading_text(t);
        let start = self.doc.text.len();
        let mut pending_space = false;
        for c in t.chars() {
            if c.is_ascii_whitespace() {
                pending_space = true;
                continue;
            }
            if pending_space {
                self.push_collapsed_space();
                pending_space = false;
            }
            self.doc.text.push(c);
        }
        if pending_space {
            self.push_collapsed_space();
        }
        self.note_task_text(self.event_at, start);
    }

    pub(super) fn push_collapsed_space(&mut self) {
        let len = self.doc.text.len();
        // An image or checkbox has no text of its own, yet a space on either side of it still shows.
        let after_atom = len == self.atom_end;
        if after_atom || (len != self.inline_start && !self.doc.text.ends_with([' ', '\n'])) {
            self.doc.text.push(' ');
        }
    }

    fn html_open(&mut self, name: &str, attrs: &str, raw: &str) {
        let tag = html::classify_tag(name);
        // An image, source or break with no text still counts as body for an alert, as on GitHub, and
        // so does a tag the sanitizer keeps with no element here, such as `<span>`.
        if self.open_alert.is_some()
            && (matches!(tag, Tag::Img | Tag::Source | Tag::Br) || html::is_inert_tag(name))
        {
            self.alert_content = true;
        }
        match tag {
            Tag::Literal => return self.collapsed_text(raw),
            // A browser stops nesting too. The text stays, in the element that is already open.
            _ if self.html_depth >= MAX_HTML_DEPTH && tag.nests() => return,
            _ => {}
        }
        let anchor = html::attr(attrs, "id").or_else(|| html::attr(attrs, "name"));
        match tag {
            Tag::Unknown => {}
            Tag::Ruby => {
                self.in_rp = false;
                self.open_html_span(tag, SpanKind::Ruby);
            }
            Tag::Rp => {
                self.flush_run();
                self.in_rp = true;
            }
            Tag::Rt => {
                self.in_rp = false;
                self.open_html_span(tag, SpanKind::RubyNote);
            }
            Tag::Strong => self.open_strong(Origin::Html(tag)),
            Tag::Em => self.open_html_span(tag, SpanKind::Emphasis),
            Tag::Underline => self.open_html_span(tag, SpanKind::Underline),
            Tag::Strike => self.open_html_span(tag, SpanKind::Strike),
            Tag::Code => self.open_html_span(tag, SpanKind::Code),
            Tag::Kbd => self.open_html_span(tag, SpanKind::Kbd),
            Tag::Sub => self.open_html_span(tag, SpanKind::Sub),
            Tag::Sup => self.open_html_span(tag, SpanKind::Sup),
            Tag::Mark => self.open_html_span(tag, SpanKind::Mark),
            Tag::Abbr => {
                // A browser marks only an `<abbr>` that has a title; a bare one is plain text.
                if let Some(title) = html::attr(attrs, "title").filter(|t| !t.trim().is_empty()) {
                    let title = self.pool(&title);
                    self.open_html_span(tag, SpanKind::Abbr(title));
                }
            }
            Tag::Small => self.open_html_span(tag, SpanKind::Small),
            Tag::Q => {
                self.open_html_span(tag, SpanKind::Quote);
                self.push_inline_text("\u{201c}");
            }
            Tag::A => self.html_link(attrs),
            Tag::Img => self.html_image(attrs),
            Tag::Br => {
                // `<br clear>` ends the block it is in, and the next block starts below the
                // floats; where the block cannot be ended (a heading, a list item's own text)
                // it is the break it always was.
                let cleared = html::attr(attrs, "clear")
                    .and_then(|v| parse_clear(&v))
                    .is_some_and(|side| self.clear_break(side));
                if !cleared {
                    self.begin_inline();
                    self.flush_run();
                    self.doc.text.push('\n');
                }
            }
            // GitHub's sanitizer removes a raw input outside a task list, so nothing is drawn.
            Tag::Input => {}
            Tag::Picture => {
                self.in_picture = true;
                self.picture_block = self.cur_block;
                self.picture_depth = self.stack.len();
                self.picture_dark = StrRef::default();
                self.picture_light = StrRef::default();
            }
            Tag::Source if self.in_picture && self.stack.len() == self.picture_depth => {
                self.html_source(attrs)
            }
            Tag::Source => {}
            _ => self.html_open_block(tag, attrs),
        }
        if let Some(a) = anchor
            && !a.is_empty()
        {
            let name = self.pool(&a);
            // With no text open, an anchor belongs to whatever block comes next.
            let block = if self.inline_open {
                self.inline_block
            } else {
                self.doc.blocks.len() as u32
            };
            self.doc.anchors.push(Anchor { name, block });
        }
    }

    fn html_open_block(&mut self, tag: Tag, attrs: &str) {
        // A browser drops these with no table open, text kept, and does not end the paragraph.
        if matches!(tag, Tag::Tr | Tag::Td | Tag::Th) && self.open_html[Tag::Table.slot()] == 0 {
            return;
        }
        let align = html_align(attrs);
        self.flush_run();
        if self.inline_open && !self.make_room_for_html_block(tag) {
            return;
        }
        let origin = Origin::Html(tag);
        let kind = match tag {
            Tag::Hr => {
                let b = self.open_block(BlockKind::ThematicBreak, origin);
                self.note_block_attrs(b, tag, attrs);
                self.pop_open();
                self.finish_block(b);
                return;
            }
            Tag::TableSection => return,
            Tag::P => BlockKind::Paragraph { align },
            Tag::H(level) => BlockKind::Heading {
                level,
                align,
                slug: StrRef::default(),
            },
            Tag::Div | Tag::Section => BlockKind::Div { align },
            Tag::Dt | Tag::Dd => {
                // `<dt>a<dd>b<dt>c` is complete HTML: each opener ends the item before it.
                self.close_implied(&[Tag::Dt, Tag::Dd], &[Tag::Section, Tag::Div]);
                if tag == Tag::Dt {
                    BlockKind::DefTerm
                } else {
                    BlockKind::DefDesc
                }
            }
            Tag::Figure => BlockKind::Figure { align },
            Tag::FigCaption => BlockKind::FigCaption,
            Tag::Center => BlockKind::Div {
                align: Align::Center,
            },
            Tag::Blockquote => BlockKind::BlockQuote,
            Tag::Ul => BlockKind::List { start: None },
            Tag::Ol => {
                let start = html::attr(attrs, "start")
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    .unwrap_or(1);
                BlockKind::List { start: Some(start) }
            }
            Tag::Li => {
                self.close_implied(&[Tag::Li], &[Tag::Ul, Tag::Ol]);
                let list = BlockKind::List { start: None };
                if !self.parent_is(Tag::Ul) && !self.flattened(list) {
                    self.open_block(list, Origin::Auto);
                }
                BlockKind::ListItem { task: false }
            }
            Tag::Table => BlockKind::Table,
            Tag::Tr => {
                self.close_implied(&[Tag::Tr], &[Tag::Table]);
                if !self.parent_is(Tag::Table) {
                    return;
                }
                BlockKind::TableRow { header: false }
            }
            Tag::Td | Tag::Th => {
                self.close_implied(&[Tag::Td, Tag::Th], &[Tag::Tr, Tag::Table]);
                if self.parent_is(Tag::Table) {
                    self.open_implied_row();
                }
                if !self.parent_is(Tag::Tr) {
                    return;
                }
                BlockKind::TableCell {
                    align,
                    header: tag == Tag::Th,
                    colspan: parse_span(attrs, "colspan", MAX_COLSPAN),
                    rowspan: parse_span(attrs, "rowspan", MAX_ROWSPAN),
                }
            }
            Tag::Details => BlockKind::Details {
                open: html::attrs(attrs).any(|(k, _)| k.eq_ignore_ascii_case("open")),
            },
            Tag::Summary => BlockKind::Summary,
            Tag::Pre => {
                let lang = html::attr(attrs, "lang").unwrap_or_default();
                let lang = self.pool(&lang);
                let b = self.open_block(BlockKind::CodeBlock { lang }, origin);
                self.note_block_attrs(b, tag, attrs);
                self.pre_open = true;
                self.pre_first_newline = true;
                return;
            }
            _ => return,
        };
        if self.flattened(kind) {
            return self.push_open(Open {
                node: Node::Nothing,
                origin,
            });
        }
        let b = self.open_block(kind, origin);
        self.note_block_attrs(b, tag, attrs);
        if matches!(tag, Tag::P | Tag::H(_)) {
            self.enter_inline();
        }
    }

    /// A `<br clear>`: ends the inline content open, if it can be ended, and has the next block
    /// start below the floats at `side`. False when the content has to stay whole.
    fn clear_break(&mut self, side: Clear) -> bool {
        self.flush_run();
        if self.inline_open && !self.make_room_for_html_block(Tag::Br) {
            return false;
        }
        self.pending_clear = Some(side);
        true
    }

    /// Ends whatever inline content is open so that a block can start. False when the tag has to be
    /// ignored instead: a Markdown heading stays whole, and GitHub's outline keeps it as one line.
    fn make_room_for_html_block(&mut self, tag: Tag) -> bool {
        let innermost = self.stack.iter().rev().find_map(|o| match o.node {
            Node::Block(b) => Some((b, o.origin)),
            _ => None,
        });
        let Some((b, origin)) = innermost else {
            return true;
        };
        if origin != Origin::Md {
            self.close_inline_container();
            return true;
        }
        match self.doc.blocks[b as usize].kind {
            _ if tag == Tag::TableSection => false,
            BlockKind::Paragraph { .. } => {
                self.split_md_inline(false);
                true
            }
            BlockKind::TableCell { .. } => {
                self.split_md_inline(true);
                true
            }
            _ => false,
        }
    }

    /// Presentation attributes that have no room in `BlockKind`. Nearly every block has none, and
    /// `Block` is the structure that most affects a document's memory.
    fn note_block_attrs(&mut self, block: u32, tag: Tag, attrs: &str) {
        if attrs.trim().is_empty() {
            return;
        }
        let own_align = matches!(
            tag,
            Tag::P
                | Tag::H(_)
                | Tag::Div
                | Tag::Section
                | Tag::Center
                | Tag::Td
                | Tag::Th
                | Tag::Figure
        );
        let length = |key: &str| html::attr(attrs, key).and_then(|v| parse_length(&v));
        let a = BlockAttrs {
            block,
            dir: html::attr(attrs, "dir").and_then(|v| parse_dir(&v)),
            align: if own_align {
                Align::None
            } else {
                html_align(attrs)
            },
            valign: html::attr(attrs, "valign").and_then(|v| parse_valign(&v)),
            width: length("width"),
            height: length("height"),
            marker: match tag {
                Tag::Ul | Tag::Ol | Tag::Li => {
                    html::attr(attrs, "type").and_then(|v| parse_marker(&v))
                }
                _ => None,
            },
        };
        let empty = BlockAttrs {
            block,
            ..BlockAttrs::default()
        };
        if a != empty {
            self.doc.block_attrs.push(a);
        }
    }

    /// Closes an open `targets` element if one is found before a `boundaries` element or Markdown.
    fn close_implied(&mut self, targets: &[Tag], boundaries: &[Tag]) {
        if targets.iter().all(|t| self.open_html[t.slot()] == 0) {
            return;
        }
        let mut found = None;
        for (i, o) in self.stack.iter().enumerate().rev() {
            match o.origin {
                Origin::Md => break,
                Origin::Html(t) if targets.contains(&t) => {
                    found = Some(i);
                    break;
                }
                Origin::Html(t) if boundaries.contains(&t) => break,
                _ => {}
            }
        }
        if let Some(i) = found {
            self.pop_to(i);
        }
    }

    pub(super) fn pop_to(&mut self, index: usize) {
        self.flush_run();
        while self.stack.len() > index {
            let open = self.pop_open().expect("length checked by the loop");
            self.close(open);
        }
    }

    /// Closes the innermost `tag` opened by HTML, without crossing anything Markdown opened.
    fn close_html(&mut self, tag: Tag) -> bool {
        if self.open_html[tag.slot()] == 0 {
            return false;
        }
        let mut found = None;
        for (i, o) in self.stack.iter().enumerate().rev() {
            match o.origin {
                Origin::Md => break,
                Origin::Html(t) if t == tag => {
                    found = Some(i);
                    break;
                }
                _ => {}
            }
        }
        match found {
            Some(i) => {
                self.pop_to(i);
                true
            }
            None => false,
        }
    }

    fn html_close(&mut self, name: &str, raw: &str) {
        let tag = html::classify_tag(name);
        match tag {
            Tag::Literal => return self.collapsed_text(raw),
            Tag::Picture => {
                self.leave_picture();
                return;
            }
            Tag::Rp => {
                self.in_rp = false;
                return;
            }
            Tag::Unknown
            | Tag::TableSection
            | Tag::Hr
            | Tag::Br
            | Tag::Img
            | Tag::Input
            | Tag::Source => return,
            _ => {}
        }
        // A closer for an inline element never reaches past a block; a closer for a block may close
        // inline containers on the way.
        let is_span = matches!(
            tag,
            Tag::A
                | Tag::Strong
                | Tag::Em
                | Tag::Underline
                | Tag::Strike
                | Tag::Code
                | Tag::Kbd
                | Tag::Sub
                | Tag::Sup
                | Tag::Mark
                | Tag::Small
                | Tag::Q
                | Tag::Ruby
                | Tag::Rt
                | Tag::Abbr
        );
        if is_span {
            self.close_html_span(tag);
        } else {
            self.close_html(tag);
        }
    }

    fn close_html_span(&mut self, tag: Tag) {
        if tag == Tag::Q && self.open_html[tag.slot()] > 0 {
            self.push_inline_text("\u{201d}");
        }
        if tag == Tag::Ruby {
            self.in_rp = false;
        }
        if self.open_html[tag.slot()] > 0 {
            let mut found = None;
            for (i, o) in self.stack.iter().enumerate().rev() {
                if matches!(o.node, Node::Block(_)) || o.origin == Origin::Md {
                    break;
                }
                if o.origin == Origin::Html(tag) {
                    found = Some(i);
                    break;
                }
            }
            if let Some(i) = found {
                self.pop_to(i);
            }
        }
        if let Some(i) = self.ambient.iter().rposition(|a| a.tag == tag) {
            self.ambient.remove(i);
        }
    }

    fn open_html_span(&mut self, tag: Tag, kind: SpanKind) {
        self.begin_inline();
        self.flush_run();
        self.push_span(kind, Origin::Html(tag));
    }

    fn html_link(&mut self, attrs: &str) {
        let Some(href) = html::attr(attrs, "href").filter(|h| html::safe_link_url(h)) else {
            return;
        };
        let dest = self.pool(&href);
        let title = html::attr(attrs, "title").unwrap_or_default();
        let title = self.pool(&title);
        let idx = self.doc.links.len() as u32;
        self.doc.links.push(LinkInfo::new(dest, title));
        self.open_html_span(Tag::A, SpanKind::Link(idx));
    }

    fn html_image(&mut self, attrs: &str) {
        let src = html::attr(attrs, "src")
            .filter(|s| html::safe_image_url(s))
            .unwrap_or_default();
        let alt = html::attr(attrs, "alt").unwrap_or_default();
        // GitHub keeps an image whose source is missing or was stripped, and its alt text shows.
        if src.is_empty() && alt.is_empty() {
            return;
        }
        self.begin_inline();
        let src = self.pool(&src);
        let alt = self.pool(&alt);
        let title = html::attr(attrs, "title").unwrap_or_default();
        let title = self.pool(&title);
        let mut width = html::attr(attrs, "width").and_then(|v| parse_length(&v));
        let mut height = html::attr(attrs, "height").and_then(|v| parse_length(&v));
        clamp_image_size(&mut width, &mut height);
        let idx = self.doc.images.len() as u32;
        self.doc.images.push(ImageInfo {
            src,
            alt,
            title,
            width,
            height,
            dark_src: self.picture_dark,
            light_src: self.picture_light,
            align: html::attr(attrs, "align").map_or(ImageAlign::None, |v| parse_image_align(&v)),
        });
        self.push_atom(SpanKind::Image(idx));
        self.leave_picture();
    }

    /// A `<picture>` shows one image, and a `<source>` means nothing outside one.
    pub(super) fn leave_picture(&mut self) {
        self.in_picture = false;
        self.picture_block = NONE;
        self.picture_depth = 0;
        self.picture_dark = StrRef::default();
        self.picture_light = StrRef::default();
    }

    fn html_source(&mut self, attrs: &str) {
        let Some(media) = html::attr(attrs, "media") else {
            return;
        };
        let Some(srcset) = html::attr(attrs, "srcset") else {
            return;
        };
        let first = srcset
            .split(',')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .next()
            .unwrap_or("");
        if first.is_empty() || !html::safe_image_url(first) {
            return;
        }
        let compact: String = media
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        // A browser takes the first matching source; a later one never replaces it.
        if compact.contains("prefers-color-scheme:dark") && self.picture_dark.len == 0 {
            self.picture_dark = self.pool(first);
        } else if compact.contains("prefers-color-scheme:light") && self.picture_light.len == 0 {
            self.picture_light = self.pool(first);
        }
    }
}

fn html_align(attrs: &str) -> Align {
    match html::attr(attrs, "align") {
        Some(a) if a.eq_ignore_ascii_case("left") => Align::Left,
        Some(a) if a.eq_ignore_ascii_case("center") => Align::Center,
        Some(a) if a.eq_ignore_ascii_case("right") => Align::Right,
        Some(a) if a.eq_ignore_ascii_case("justify") => Align::Justify,
        _ => Align::None,
    }
}

/// The `clear` of a `<br>`: `left`, `right`, `all` or `both`, in any case. Anything else, an empty
/// value too, clears nothing.
fn parse_clear(v: &str) -> Option<Clear> {
    match v.trim().to_ascii_lowercase().as_str() {
        "left" => Some(Clear::Left),
        "right" => Some(Clear::Right),
        "all" | "both" => Some(Clear::Both),
        _ => None,
    }
}

fn parse_dir(v: &str) -> Option<Dir> {
    match v.trim().to_ascii_lowercase().as_str() {
        "ltr" => Some(Dir::Ltr),
        "rtl" => Some(Dir::Rtl),
        "auto" => Some(Dir::Auto),
        _ => None,
    }
}

fn parse_valign(v: &str) -> Option<VAlign> {
    match v.trim().to_ascii_lowercase().as_str() {
        "top" => Some(VAlign::Top),
        "middle" | "center" => Some(VAlign::Middle),
        "bottom" => Some(VAlign::Bottom),
        "baseline" => Some(VAlign::Baseline),
        _ => None,
    }
}

/// `type` is case-sensitive here, unlike the other attributes: `a` and `A` are different lists.
fn parse_marker(v: &str) -> Option<ListMarker> {
    match v.trim() {
        "1" => Some(ListMarker::Decimal),
        "a" => Some(ListMarker::LowerAlpha),
        "A" => Some(ListMarker::UpperAlpha),
        "i" => Some(ListMarker::LowerRoman),
        "I" => Some(ListMarker::UpperRoman),
        t if t.eq_ignore_ascii_case("disc") => Some(ListMarker::Disc),
        t if t.eq_ignore_ascii_case("circle") => Some(ListMarker::Circle),
        t if t.eq_ignore_ascii_case("square") => Some(ListMarker::Square),
        _ => None,
    }
}

fn parse_image_align(v: &str) -> ImageAlign {
    match v.trim().to_ascii_lowercase().as_str() {
        "left" => ImageAlign::Left,
        "right" => ImageAlign::Right,
        "top" => ImageAlign::Top,
        "middle" | "center" => ImageAlign::Middle,
        "bottom" => ImageAlign::Bottom,
        _ => ImageAlign::None,
    }
}

/// `colspan` or `rowspan` under the HTML rules: leading digits count, zero or nothing means 1, and a
/// value over the limit is clamped. Layout can then divide by it and size from it.
fn parse_span(attrs: &str, key: &str, max: u16) -> u16 {
    let Some(v) = html::attr(attrs, key) else {
        return 1;
    };
    let v = v.trim();
    // Chromium accepts a leading `+` here, as in `colspan="+2"`.
    let v = v.strip_prefix('+').unwrap_or(v);
    let digits = v.bytes().take_while(u8::is_ascii_digit).count();
    let n = match v[..digits].parse::<u64>() {
        Ok(n) => n,
        Err(_) if digits > 0 => u64::MAX,
        Err(_) => 1,
    };
    n.clamp(1, u64::from(max)) as u16
}

/// A length as Chromium reads one from an HTML attribute: leading digits, an optional fraction,
/// then an optional `%` immediately after; any other suffix is ignored (`10em` is 10). No sign and
/// no exponent, so `+5`, `.5` and `1e3` give no length or just their leading digit. `50 %` is 50px,
/// because the space stops the percent form.
fn parse_length(v: &str) -> Option<Length> {
    let v = v.trim_start();
    let b = v.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    if b.get(i) == Some(&b'.') {
        let mut j = i + 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        i = j;
    }
    let number: f64 = v[..i].parse().ok()?;
    if v[i..].starts_with('%') {
        return Some(Length::Percent(number.min(1000.0) as u16));
    }
    Some(Length::Px((number as u32).min(MAX_HTML_LENGTH_PX)))
}

/// GitHub caps an image's pixel width and height at 2000, scaling both together so the picture
/// keeps its shape. A percentage is left alone, as is one dimension with nothing to scale against.
fn clamp_image_size(width: &mut Option<Length>, height: &mut Option<Length>) {
    let side = |l: Option<Length>| match l {
        Some(Length::Px(n)) => Some(f64::from(n)),
        _ => None,
    };
    let max = f64::from(MAX_IMAGE_DIMENSION_PX);
    let (w, h) = (side(*width), side(*height));
    if let (Some(w), Some(h)) = (w, h)
        && (w > max || h > max)
    {
        let scale = (max / w.max(h)).min(1.0);
        *width = Some(Length::Px((w * scale).round() as u32));
        *height = Some(Length::Px((h * scale).round() as u32));
        return;
    }
    if matches!(*width, Some(Length::Px(n)) if n > MAX_IMAGE_DIMENSION_PX) {
        *width = Some(Length::Px(MAX_IMAGE_DIMENSION_PX));
    }
    if matches!(*height, Some(Length::Px(n)) if n > MAX_IMAGE_DIMENSION_PX) {
        *height = Some(Length::Px(MAX_IMAGE_DIMENSION_PX));
    }
}
