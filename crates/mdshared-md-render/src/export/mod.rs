//! A document as one self-contained HTML file: what "Export as HTML" saves and what "Print" hands
//! to the browser.
//!
//! The HTML is made from the parsed document model, never from the source, so it holds what the
//! page shows. It carries its own style sheet (the page's light colours, font stacks and reading
//! width; a print style sheet turns the page white and avoids breaking code and tables), and no
//! scripts of the document's own.
//!
//! - **Images.** A local image is embedded as a `data:` URI when it is at most [`MAX_EMBED`] and
//!   the file stays under [`MAX_TOTAL`]; a larger one is linked by its `file://` address, which
//!   works on this machine only. A remote image is linked when the caller says the reader loaded
//!   it, and is its alt text otherwise. Nothing is downloaded here.
//! - **Code** keeps the page's highlight colours as inline styles.
//! - **Formulas and diagrams** are drawn by the engine process, as the page does, and embedded as
//!   PNG pictures at twice their size (so they stay sharp), with the source as their alt text.
//!   When the engine is missing or rejects the source, the source stays in a code box, labelled.
//! - Everything written from the document is escaped, links keep only `http`, `https`, `mailto`
//!   and relative addresses, and a `data:` image must be an image.

mod pieces;
#[cfg(test)]
mod tests;

use pieces::*;

use crate::highlight;
use crate::image::{self, DecodedImage};
use crate::theme::{Color, Theme};
use mdshared_md_core::model::{
    Align, BlockKind, Dir, Document, ImageAlign, ImageInfo, Length, ListMarker, NONE, SpanKind,
};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

/// The largest local image that is embedded in the file.
pub const MAX_EMBED: u64 = 4 * 1024 * 1024;
/// The most image bytes embedded in one file; later images are linked.
pub const MAX_TOTAL: u64 = 24 * 1024 * 1024;

pub struct ExportOptions<'a> {
    /// The `<title>`. Empty takes the first heading, then "Document".
    pub title: &'a str,
    /// The page's colours and font lists; the reading width comes from it too.
    pub theme: Theme,
    /// Where relative image paths start. `None` leaves them as their alt text.
    pub base_dir: Option<&'a Path>,
    /// The reader has loaded this document's remote images, so they are linked.
    pub remote_images: bool,
    /// For the browser to print: every `<details>` is open, and the page asks the browser to open
    /// its print dialog once it has loaded.
    pub print: bool,
}

/// The whole document as an HTML page.
pub fn export_html(doc: &Document, opts: &ExportOptions) -> String {
    let mut w = Writer {
        doc,
        opts,
        out: String::with_capacity(doc.text.len() * 2 + 4096),
        embedded: 0,
        pictures: HashMap::new(),
        anchors: HashMap::new(),
    };
    for a in &doc.anchors {
        w.anchors.entry(a.block).or_default().push(doc.str(a.name));
    }
    w.blocks(NONE);
    w.footnotes();
    let body = std::mem::take(&mut w.out);
    let title = if opts.title.trim().is_empty() {
        doc.headings
            .first()
            .map(|h| doc.str(h.title).trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Document".to_string())
    } else {
        opts.title.trim().to_string()
    };
    let mut page = String::with_capacity(body.len() + 6000);
    page.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    page.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    page.push_str("<meta name=\"generator\" content=\"mdview\">\n<title>");
    escape_into(&mut page, &title, false);
    page.push_str("</title>\n<style>\n");
    page.push_str(&stylesheet(&opts.theme));
    page.push_str("</style>\n</head>\n<body>\n<main>\n");
    page.push_str(&body);
    page.push_str("</main>\n");
    if opts.print {
        // The browser's own print dialog, once the images are in. Without scripts (a locked-down
        // browser) the page is still there to print with Ctrl+P.
        page.push_str(
            "<script>window.addEventListener('load',function(){setTimeout(function(){window.print()},300)})</script>\n",
        );
    }
    page.push_str("</body>\n</html>\n");
    page
}

struct Writer<'a> {
    doc: &'a Document,
    opts: &'a ExportOptions<'a>,
    out: String,
    /// Image bytes embedded so far.
    embedded: u64,
    /// Formulas and diagrams already drawn, by what they were drawn from.
    pictures: HashMap<(u8, String), Option<Picture>>,
    /// Explicit anchors by block.
    anchors: HashMap<u32, Vec<&'a str>>,
}

#[derive(Clone)]
struct Picture {
    uri: String,
    /// Logical size: the raster is twice that.
    width: f32,
    height: f32,
    /// From the top to the text baseline.
    baseline: f32,
    /// The same drawn for the dark page, when it differs and could be drawn.
    dark: Option<String>,
}

impl<'a> Writer<'a> {
    fn blocks(&mut self, parent: u32) {
        for c in self.doc.children(parent) {
            self.block(c);
        }
    }

    fn block(&mut self, i: u32) {
        let doc = self.doc;
        let b = doc.blocks[i as usize];
        if let Some(names) = self.anchors.remove(&i) {
            for name in names {
                self.out.push_str("<a id=\"");
                escape_into(&mut self.out, name, false);
                self.out.push_str("\"></a>");
            }
        }
        let attrs = self.attrs(i, None);
        match b.kind {
            BlockKind::Paragraph { align } => {
                let attrs = self.attrs(i, Some(align));
                self.holder(i, &format!("<p{attrs}>"), "</p>\n", true);
            }
            BlockKind::Plain => {
                let inline = matches!(
                    doc.blocks.get(b.parent as usize).map(|p| p.kind),
                    Some(BlockKind::ListItem { .. } | BlockKind::TableCell { .. })
                );
                if inline {
                    self.holder(i, "", "", false);
                } else {
                    self.holder(i, &format!("<div{attrs}>"), "</div>\n", true);
                }
            }
            BlockKind::Heading { level, align, slug } => {
                let n = level.clamp(1, 6);
                let mut open = format!("<h{n}");
                if slug.len > 0 {
                    open.push_str(" id=\"");
                    escape_into(&mut open, doc.str(slug), false);
                    open.push('"');
                }
                open.push_str(&self.attrs(i, Some(align)));
                open.push('>');
                self.holder(i, &open, &format!("</h{n}>\n"), true);
            }
            BlockKind::BlockQuote => {
                self.holder(
                    i,
                    &format!("<blockquote{attrs}>\n"),
                    "</blockquote>\n",
                    false,
                );
            }
            BlockKind::Alert(kind) => {
                let name = kind.label();
                let open = format!(
                    "<div class=\"alert alert-{}\"{attrs}>\n<p class=\"alert-title\">{name}</p>\n",
                    name.to_ascii_lowercase()
                );
                self.holder(i, &open, "</div>\n", false);
            }
            BlockKind::List { start } => {
                let marker = doc.attrs_of(i).and_then(|a| a.marker);
                let style = marker_style(marker);
                let open = match start {
                    None => format!("<ul{attrs}{style}>\n"),
                    Some(n) if n != 1 => format!("<ol start=\"{n}\"{attrs}{style}>\n"),
                    Some(_) => format!("<ol{attrs}{style}>\n"),
                };
                let close = if start.is_some() {
                    "</ol>\n"
                } else {
                    "</ul>\n"
                };
                self.holder(i, &open, close, false);
            }
            BlockKind::ListItem { task } => {
                let marker = doc.attrs_of(i).and_then(|a| a.marker);
                let style = marker_style(marker);
                let open = if task {
                    format!("<li class=\"task\"{style}>")
                } else {
                    format!("<li{style}>")
                };
                self.holder(i, &open, "</li>\n", false);
            }
            BlockKind::CodeBlock { lang } => {
                let lang = doc.str(lang).trim().to_string();
                self.code(i, &lang, None);
            }
            BlockKind::Diagram { lang, oversize } => {
                let lang = doc.str(lang).trim().to_string();
                let label = if lang.is_empty() { "diagram" } else { &lang };
                let source = doc.block_text(i).trim();
                let picture = match oversize {
                    None if !source.is_empty() => self.diagram(source),
                    _ => None,
                };
                match picture {
                    Some(p) => {
                        self.figure_picture(&p, source, "diagram");
                    }
                    None => {
                        let why =
                            oversize.map_or("Could not be drawn".to_string(), |o| o.to_string());
                        let label = label.to_string();
                        self.code(i, &label, Some(&why));
                    }
                }
            }
            BlockKind::Math { oversize } => {
                let source = doc.block_text(i).trim();
                let picture = match oversize {
                    None if !source.is_empty() => self.formula(source, true),
                    _ => None,
                };
                match picture {
                    Some(p) => self.figure_picture(&p, source, "math"),
                    None => {
                        let why =
                            oversize.map_or("Could not be drawn".to_string(), |o| o.to_string());
                        self.code(i, "math", Some(&why));
                    }
                }
            }
            BlockKind::ThematicBreak => self.out.push_str("<hr>\n"),
            BlockKind::Table => {
                // A table is placed by `align` (its own, or that of the `<div>`, `<center>` or
                // `<figure>` around it), which a browser does with auto margins.
                let own = doc.attrs_of(i).map_or(Align::None, |a| a.align);
                let align = if own == Align::None {
                    self.inherited_align(i)
                } else {
                    own
                };
                let margin = match align {
                    Align::Center => "margin-left:auto;margin-right:auto;",
                    Align::Right => "margin-left:auto;margin-right:0;",
                    _ => "",
                };
                let attrs = self.attrs_styled(i, None, margin);
                self.holder(i, &format!("<table{attrs}>\n"), "</table>\n", false);
            }
            BlockKind::TableRow { .. } => self.holder(i, "<tr>", "</tr>\n", false),
            BlockKind::TableCell {
                align,
                header,
                colspan,
                rowspan,
            } => {
                let tag = if header { "th" } else { "td" };
                let mut open = format!("<{tag}");
                if colspan > 1 {
                    let _ = write!(open, " colspan=\"{colspan}\"");
                }
                if rowspan > 1 {
                    let _ = write!(open, " rowspan=\"{rowspan}\"");
                }
                // A cell that states no `align` or `valign` takes its row's, and `width` is the
                // column's asked width.
                let own = doc.attrs_of(i);
                let row = doc.attrs_of(b.parent);
                let align = if align == Align::None {
                    row.map_or(Align::None, |a| a.align)
                } else {
                    align
                };
                let mut style = String::new();
                if let Some(css) = align_css(align) {
                    style.push_str(css);
                }
                match own.and_then(|a| a.width) {
                    Some(Length::Px(p)) => {
                        let _ = write!(style, "width:{p}px;");
                    }
                    Some(Length::Percent(p)) => {
                        let _ = write!(style, "width:{p}%;");
                    }
                    None => {}
                }
                if let Some(v) = own.and_then(|a| a.valign).or(row.and_then(|a| a.valign)) {
                    style.push_str(match v {
                        mdshared_md_core::model::VAlign::Top => "vertical-align:top;",
                        mdshared_md_core::model::VAlign::Middle => "vertical-align:middle;",
                        mdshared_md_core::model::VAlign::Bottom => "vertical-align:bottom;",
                        mdshared_md_core::model::VAlign::Baseline => "vertical-align:baseline;",
                    });
                }
                if !style.is_empty() {
                    let _ = write!(open, " style=\"{style}\"");
                }
                open.push('>');
                self.holder(i, &open, &format!("</{tag}>"), false);
            }
            BlockKind::Details { open } => {
                let open_attr = if open || self.opts.print { " open" } else { "" };
                self.holder(
                    i,
                    &format!("<details{open_attr}{attrs}>\n"),
                    "</details>\n",
                    false,
                );
            }
            BlockKind::Summary => self.holder(i, "<summary>", "</summary>\n", false),
            // Written in the list at the end, where the page puts them.
            BlockKind::FootnoteDefinition { .. } => {}
            BlockKind::Div { align } => {
                let attrs = self.attrs(i, Some(align));
                self.holder(i, &format!("<div{attrs}>\n"), "</div>\n", false);
            }
            BlockKind::DefTerm => self.holder(i, "<div class=\"dt\">", "</div>\n", false),
            BlockKind::DefDesc => self.holder(i, "<div class=\"dd\">", "</div>\n", false),
            BlockKind::Figure { align } => {
                let attrs = self.attrs(i, Some(align));
                self.holder(i, &format!("<figure{attrs}>\n"), "</figure>\n", false);
            }
            BlockKind::FigCaption => self.holder(i, "<figcaption>", "</figcaption>\n", false),
        }
    }

    /// A block's tags around what it holds: its own inline text, then its child blocks. A block
    /// with nothing in it writes nothing when `skip_empty`.
    fn holder(&mut self, i: u32, open: &str, close: &str, skip_empty: bool) {
        let doc = self.doc;
        let b = doc.blocks[i as usize];
        let mark = self.out.len();
        self.out.push_str(open);
        let body = self.out.len();
        // Only a leaf has inline text of its own; a container's text is its children's.
        if b.end > i + 1 {
            self.blocks(i);
        } else {
            self.inline(i);
        }
        if skip_empty && self.out.len() == body {
            self.out.truncate(mark);
            return;
        }
        self.out.push_str(close);
    }

    /// The `align` of the nearest `<div>`, `<center>` or `<figure>` around block `i` that states
    /// one: what a table with no `align` of its own is placed by.
    fn inherited_align(&self, i: u32) -> Align {
        let mut b = i;
        loop {
            let block = &self.doc.blocks[b as usize];
            if let BlockKind::Figure { align } | BlockKind::Div { align } = block.kind
                && align != Align::None
            {
                return align;
            }
            if block.parent == NONE {
                return Align::None;
            }
            b = block.parent;
        }
    }

    /// ` dir`, ` style` of a block from its alignment and its HTML attributes.
    fn attrs(&self, i: u32, align: Option<Align>) -> String {
        self.attrs_styled(i, align, "")
    }

    /// `attrs` with `extra` CSS declarations added to the style.
    fn attrs_styled(&self, i: u32, align: Option<Align>, extra: &str) -> String {
        let mut s = String::new();
        let mut style = String::from(extra);
        if let Some(a) = self.doc.attrs_of(i) {
            match a.dir {
                Some(Dir::Rtl) => s.push_str(" dir=\"rtl\""),
                Some(Dir::Ltr) => s.push_str(" dir=\"ltr\""),
                Some(Dir::Auto) => s.push_str(" dir=\"auto\""),
                None => {}
            }
            // A table is placed by margins (see `BlockKind::Table`), not by `text-align`.
            let table = matches!(self.doc.blocks[i as usize].kind, BlockKind::Table);
            if let Some(css) = align_css(a.align).filter(|_| !table) {
                style.push_str(css);
            }
            if let Some(Length::Px(p)) = a.width {
                let _ = write!(style, "width:{p}px;");
            } else if let Some(Length::Percent(p)) = a.width {
                let _ = write!(style, "width:{p}%;");
            }
        }
        if let Some(css) = align.and_then(align_css) {
            style.push_str(css);
        }
        if !style.is_empty() {
            let _ = write!(s, " style=\"{style}\"");
        }
        s
    }

    // ---- code, formulas, diagrams ----------------------------------------------------------

    /// A code box; `note` follows the language in the corner, for source shown in place of a
    /// picture.
    fn code(&mut self, i: u32, lang: &str, note: Option<&str>) {
        let source = self.doc.block_text(i).trim_end_matches('\n');
        let label = match (lang.is_empty(), note) {
            (false, Some(n)) => format!("{lang} \u{b7} {n}"),
            (false, None) => lang.to_string(),
            (true, Some(n)) => n.to_string(),
            (true, None) => String::new(),
        };
        self.out.push_str("<pre");
        if !label.is_empty() {
            self.out.push_str(" data-lang=\"");
            escape_into(&mut self.out, &label, false);
            self.out.push('"');
        }
        self.out.push_str("><code>");
        // Coloured for the light page, and for the dark one too where the two lists agree: the
        // dark colour rides along as `--d`, which the dark style rules put to use.
        let tokens = if lang.is_empty() || note.is_some() {
            None
        } else {
            highlight::tokens(source, Some(lang), false)
        };
        match tokens {
            Some(tokens) => {
                let dark = highlight::tokens(source, Some(lang), true);
                highlighted(&mut self.out, source, &tokens, dark.as_deref());
            }
            None => escape_into(&mut self.out, source, false),
        }
        self.out.push_str("</code></pre>\n");
    }

    fn figure_picture(&mut self, p: &Picture, alt: &str, class: &str) {
        let _ = write!(self.out, "<p class=\"{class}\">");
        self.picture_imgs(p, "", "", alt);
        self.out.push_str("</p>\n");
    }

    /// The `<img>` of a drawn picture: one, or a light one and a dark one of which the page
    /// shows the one the reader's browser asks for.
    fn picture_imgs(&mut self, p: &Picture, class: &str, style: &str, alt: &str) {
        let both = [("lt", Some(&p.uri)), ("dk", p.dark.as_ref())];
        for (scheme, uri) in both {
            let Some(uri) = uri else {
                continue;
            };
            self.out.push_str("<img");
            let scheme = if p.dark.is_some() { scheme } else { "" };
            let classes = [class, scheme].join(" ");
            if !classes.trim().is_empty() {
                let _ = write!(self.out, " class=\"{}\"", classes.trim());
            }
            let _ = write!(
                self.out,
                " src=\"{uri}\" width=\"{}\" height=\"{}\"",
                px(p.width),
                px(p.height)
            );
            if !style.is_empty() {
                let _ = write!(self.out, " style=\"{style}\"");
            }
            self.out.push_str(" alt=\"");
            escape_into(&mut self.out, alt, false);
            self.out.push_str("\">");
        }
    }

    /// A formula drawn by the engine, once per source and style.
    fn formula(&mut self, source: &str, display: bool) -> Option<Picture> {
        let key = (u8::from(display), source.to_string());
        if let Some(found) = self.pictures.get(&key) {
            return found.clone();
        }
        let size = self.opts.theme.body_size * if display { 1.15 } else { 1.0 } * 2.0;
        let drawn = crate::engine::render_math(source, display, size, crate::theme::LIGHT.text);
        let dark = crate::engine::render_math(source, display, size, crate::theme::DARK.text);
        let picture = with_dark(drawn.and_then(|d| picture_of(&d)), dark);
        self.pictures.insert(key, picture.clone());
        picture
    }

    fn diagram(&mut self, source: &str) -> Option<Picture> {
        let key = (2, source.to_string());
        if let Some(found) = self.pictures.get(&key) {
            return found.clone();
        }
        let drawn = crate::engine::render_mermaid(source, 2.0, false);
        let dark = crate::engine::render_mermaid(source, 2.0, true);
        let picture = with_dark(drawn.and_then(|d| picture_of(&d)), dark);
        self.pictures.insert(key, picture.clone());
        picture
    }

    // ---- inline content --------------------------------------------------------------------

    fn inline(&mut self, i: u32) {
        let b = self.doc.blocks[i as usize];
        self.run(b.spans.start, b.spans.end, NONE, b.text.start, b.text.end);
    }

    /// Text `from..to` of a block with the spans among `ss..se` whose parent is `parent`.
    fn run(&mut self, ss: u32, se: u32, parent: u32, from: u32, to: u32) {
        let doc = self.doc;
        let mut at = from;
        for s in ss..se {
            let span = doc.spans[s as usize];
            // A span with no parent in this block is at its top, whatever it was carried from.
            let top = parent == NONE && !(ss..se).contains(&span.parent);
            if span.parent != parent && !top {
                continue;
            }
            let atom = matches!(
                span.kind,
                SpanKind::Image(_) | SpanKind::Checkbox { .. } | SpanKind::FootnoteRef(_)
            );
            if atom {
                if span.start < at || span.start > to {
                    continue;
                }
                self.text(at, span.start);
                at = span.start;
                self.atom(span.kind);
                continue;
            }
            let (cs, ce) = (span.start.max(from), span.end.min(to));
            // A span with no text of its own is written when it holds a picture: a link around
            // an image alone.
            let holds_atom = cs == ce
                && (ss..se).any(|k| {
                    let inner = doc.spans[k as usize];
                    inner.parent == s && matches!(inner.kind, SpanKind::Image(_))
                });
            if (cs >= ce && !holds_atom) || cs < at {
                continue;
            }
            self.text(at, cs);
            at = ce;
            if let SpanKind::Math {
                display,
                oversize: None,
            } = span.kind
            {
                let source = doc.text[cs as usize..ce as usize].trim();
                if let Some(p) = self.formula(source, display) {
                    // The baseline sits on the text's: the part of the picture below it hangs down.
                    let style =
                        format!("vertical-align:-{}px", px((p.height - p.baseline).max(0.0)));
                    self.picture_imgs(&p, "math", &style, source);
                    continue;
                }
            }
            let (open, close) = self.span_tags(span.kind);
            self.out.push_str(&open);
            match span.kind {
                // A formula that is not drawn is its source, as code.
                SpanKind::Math { .. } => self.text(cs, ce),
                _ => self.run(ss, se, s, cs, ce),
            }
            self.out.push_str(&close);
        }
        self.text(at, to);
    }

    fn text(&mut self, from: u32, to: u32) {
        if from < to
            && let Some(t) = self.doc.text.get(from as usize..to as usize)
        {
            escape_into(&mut self.out, t, true);
        }
    }

    fn atom(&mut self, kind: SpanKind) {
        match kind {
            SpanKind::Checkbox { checked } => {
                self.out.push_str(if checked {
                    "<input type=\"checkbox\" checked disabled> "
                } else {
                    "<input type=\"checkbox\" disabled> "
                });
            }
            SpanKind::FootnoteRef(n) => {
                let number = self.doc.footnotes.get(n as usize).map_or(0, |f| f.number);
                let _ = write!(
                    self.out,
                    "<sup class=\"fnref\"><a href=\"#fn-{number}\" id=\"fnref-{number}\">{number}</a></sup>"
                );
            }
            SpanKind::Image(n) => {
                if let Some(info) = self.doc.images.get(n as usize).copied() {
                    self.image(&info);
                }
            }
            _ => {}
        }
    }

    fn span_tags(&self, kind: SpanKind) -> (String, String) {
        let tag = |t: &str| (format!("<{t}>"), format!("</{t}>"));
        match kind {
            SpanKind::Emphasis => tag("em"),
            SpanKind::Strong => tag("strong"),
            SpanKind::Strike => tag("del"),
            SpanKind::Underline => tag("u"),
            SpanKind::Code | SpanKind::Math { .. } => tag("code"),
            SpanKind::Kbd => tag("kbd"),
            SpanKind::Sub => tag("sub"),
            SpanKind::Sup => tag("sup"),
            SpanKind::Mark => tag("mark"),
            SpanKind::Small => tag("small"),
            SpanKind::Quote => tag("q"),
            SpanKind::Ruby => tag("ruby"),
            SpanKind::RubyNote => tag("rt"),
            SpanKind::Abbr(title) => {
                let mut open = String::from("<abbr title=\"");
                escape_into(&mut open, self.doc.str(title), false);
                open.push_str("\">");
                (open, "</abbr>".to_string())
            }
            SpanKind::Link(n) => match self.doc.links.get(n as usize) {
                Some(link) if !link.wiki && safe_href(self.doc.str(link.dest)) => {
                    let mut open = String::from("<a href=\"");
                    escape_into(&mut open, self.doc.str(link.dest).trim(), false);
                    open.push('"');
                    let title = self.doc.str(link.title);
                    if !title.is_empty() {
                        open.push_str(" title=\"");
                        escape_into(&mut open, title, false);
                        open.push('"');
                    }
                    open.push('>');
                    (open, "</a>".to_string())
                }
                _ => (String::new(), String::new()),
            },
            SpanKind::Image(_) | SpanKind::FootnoteRef(_) | SpanKind::Checkbox { .. } => {
                (String::new(), String::new())
            }
        }
    }

    // ---- images ----------------------------------------------------------------------------

    fn image(&mut self, info: &ImageInfo) {
        let doc = self.doc;
        let alt = doc.str(info.alt);
        let (light, dark) = (doc.str(info.light_src), doc.str(info.dark_src));
        let plain = doc.str(info.src);
        // A picture made for each scheme is written twice, and the page shows the one the
        // reader's browser asks for; any other image once.
        let pick = |themed: &'a str| if themed.is_empty() { plain } else { themed };
        let Some(uri) = self.image_uri(pick(light).trim()) else {
            escape_into(&mut self.out, alt, true);
            return;
        };
        let dark_uri = if light.is_empty() && dark.is_empty() {
            None
        } else {
            self.image_uri(pick(dark).trim()).filter(|d| *d != uri)
        };
        match dark_uri {
            Some(dark_uri) => {
                self.image_tag(info, &uri, "lt");
                self.image_tag(info, &dark_uri, "dk");
            }
            None => self.image_tag(info, &uri, ""),
        }
    }

    /// One `<img>` for `info` with the address `uri`; `scheme` is its class when there is a
    /// twin for the other colour scheme.
    fn image_tag(&mut self, info: &ImageInfo, uri: &str, scheme: &str) {
        let doc = self.doc;
        let alt = doc.str(info.alt);
        self.out.push_str("<img");
        if !scheme.is_empty() {
            let _ = write!(self.out, " class=\"{scheme}\"");
        }
        self.out.push_str(" src=\"");
        escape_into(&mut self.out, uri, false);
        self.out.push_str("\" alt=\"");
        escape_into(&mut self.out, alt, false);
        self.out.push('"');
        let title = doc.str(info.title);
        if !title.is_empty() {
            self.out.push_str(" title=\"");
            escape_into(&mut self.out, title, false);
            self.out.push('"');
        }
        let mut style = String::new();
        match info.width {
            Some(Length::Px(p)) => {
                let _ = write!(self.out, " width=\"{p}\"");
            }
            Some(Length::Percent(p)) => {
                let _ = write!(style, "width:{p}%;");
            }
            None => {}
        }
        match info.height {
            Some(Length::Px(p)) => {
                let _ = write!(self.out, " height=\"{p}\"");
            }
            Some(Length::Percent(_)) | None => {}
        }
        style.push_str(match info.align {
            ImageAlign::None => "",
            ImageAlign::Left => "float:left;margin:0 12px 8px 0;",
            ImageAlign::Right => "float:right;margin:0 0 8px 12px;",
            ImageAlign::Top => "vertical-align:top;",
            ImageAlign::Middle => "vertical-align:middle;",
            ImageAlign::Bottom => "vertical-align:bottom;",
        });
        if !style.is_empty() {
            let _ = write!(self.out, " style=\"{style}\"");
        }
        self.out.push('>');
    }

    /// What the `src` of an `<img>` is for this image, or `None` when it has no picture.
    fn image_uri(&mut self, src: &str) -> Option<String> {
        if src.is_empty() {
            return None;
        }
        let lower = src.get(..11).unwrap_or(src).to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            return self.opts.remote_images.then(|| src.to_string());
        }
        if lower.starts_with("data:image/") {
            return Some(src.to_string());
        }
        let (path, bytes) = image::local_paths(src, self.opts.base_dir)
            .find_map(|path| image::read(&path).map(|bytes| (path, bytes)))?;
        let mime = image_mime(&bytes)?;
        let len = bytes.len() as u64;
        if len <= MAX_EMBED && self.embedded + len <= MAX_TOTAL {
            self.embedded += len;
            return Some(format!("data:{mime};base64,{}", base64(&bytes)));
        }
        Some(file_url(&path))
    }

    // ---- footnotes -------------------------------------------------------------------------

    /// The footnotes the text refers to, numbered, after the page, as the viewer lists them.
    fn footnotes(&mut self) {
        let doc = self.doc;
        let mut notes: Vec<_> = doc
            .footnotes
            .iter()
            .filter(|f| f.number > 0 && f.defined)
            .collect();
        if notes.is_empty() {
            return;
        }
        notes.sort_by_key(|f| f.number);
        self.out
            .push_str("<section class=\"footnotes\">\n<hr>\n<ol>\n");
        for note in notes {
            let n = note.number;
            let _ = write!(self.out, "<li id=\"fn-{n}\">");
            if note.block != NONE {
                if doc.blocks[note.block as usize].end > note.block + 1 {
                    self.blocks(note.block);
                } else {
                    self.inline(note.block);
                }
            }
            let _ = writeln!(
                self.out,
                " <a class=\"back\" href=\"#fnref-{n}\">\u{21a9}</a></li>"
            );
        }
        self.out.push_str("</ol>\n</section>\n");
    }
}
