//! Streams pulldown-cmark events into the document model. No event is kept after it is handled.

mod block;
mod inline;
mod markup;
mod math;
mod source_tasks;
mod text_links;

use block::{is_atx_heading, is_dash_run};
use math::{github_math, parse_with_checks};
use source_tasks::{Parsed, TaskEvents, TaskSource};
use text_links::WikiOpen;

use crate::autolink::{self, Entities, LinkKind, SWAP_PREFIX};
use crate::html::{self, Tag, Token, Tokenizer};
use crate::limits::{
    MAX_BLOCK_DEPTH, MAX_COLSPAN, MAX_DECODED_BYTES, MAX_DIAGRAM_BYTES, MAX_EQUATION_BYTES,
    MAX_HTML_DEPTH, MAX_HTML_LENGTH_PX, MAX_IMAGE_DIMENSION_PX, MAX_ROWSPAN, Oversize,
};
use crate::links::is_image_name;
use crate::model::*;
use crate::slug::Slugger;
use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, LinkType, Options, Parser, Tag as MdTag,
    TagEnd,
};
use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use unicase::UniCase;

pub(crate) fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_MATH
}

/// Parses Markdown into a document. A leading BOM is ignored. Text over what any file within the limit
/// can decode to yields an empty document with a warning, which also keeps every offset within `u32`.
pub fn parse(src: &str) -> Document {
    parse_wiki(src, &mut |_| WikiAs::Text)
}

/// [`parse`] with wiki-links. `wiki` is asked for each note name written as `[[Note]]`,
/// `[[Note|text]]`, `[[Note#Heading]]` or `[[folder/Note]]` in running text, and says whether
/// that is a link, a link to a note that is missing, or just text. It may be asked more than once
/// for a name. For `[[#Heading]]`, a link into the same document, the name is empty. The pass runs on what CommonMark left as plain
/// text, so `[[x]](url)`, `[[x]][ref]` and `[[x]]` with a definition for `x` keep their meaning,
/// and nothing in code or inside another link is touched.
pub fn parse_wiki(src: &str, wiki: &mut dyn FnMut(&str) -> WikiAs) -> Document {
    parse_wiki_embeds(src, &mut |note, embed| {
        if embed { WikiAs::Text } else { wiki(note) }
    })
}

/// [`parse_wiki`] that also asks about embeds: `wiki(name, true)` for `![[name]]`. Where `name`
/// ends in a picture extension (see [`crate::links::is_image_name`]) `Link` means the file is
/// there and the embed is the picture, as `![](name)` would be; for any other name `Link` makes
/// a link to the note, as `[[name]]` would. Anything else leaves the embed as written (a
/// `Missing` note too: an embed is never drawn as a missing link). An embed left as text is
/// remembered in `wiki_text` as `!name`.
pub fn parse_wiki_embeds(src: &str, wiki: &mut dyn FnMut(&str, bool) -> WikiAs) -> Document {
    parse_with_source(src, wiki, false).doc
}

/// Like parsing the page, but also find the source byte of each checkbox in the decoded text.
pub(crate) fn source_tasks(src: &str) -> Option<(usize, Vec<crate::tasks::Marker>)> {
    parse_with_source(src, &mut |_, _| WikiAs::Text, true).tasks
}

fn parse_with_source(
    input: &str,
    wiki: &mut dyn FnMut(&str, bool) -> WikiAs,
    track_tasks: bool,
) -> Parsed {
    let src = input.strip_prefix('\u{FEFF}').unwrap_or(input);
    if src.len() > MAX_DECODED_BYTES {
        return Parsed {
            doc: Document {
                warnings: vec![Warning::FileTooLarge],
                ..Document::default()
            },
            tasks: None,
        };
    }
    // YAML front matter becomes a table (or a yaml block) before anything else sees the source.
    let with_front;
    let mut front = Front::default();
    let mut task_source = track_tasks.then(|| TaskSource::new(src, input.len() - src.len()));
    let src = match crate::frontmatter::take(src) {
        Some((head, used)) => {
            front = Front {
                replacement_lines: count_lines(head.as_bytes()),
                original_lines: count_lines(&src.as_bytes()[..used]),
            };
            if let Some(source) = &mut task_source {
                source.skip_front(used, front.replacement_lines);
            }
            with_front = format!("{head}{}", &src[used..]);
            with_front.as_str()
        }
        None => src,
    };
    // CommonMark requires this, and pulldown-cmark leaves NUL in its text events.
    let no_nul;
    let src = if memchr::memchr(0, src.as_bytes()).is_some() {
        no_nul = src.replace('\0', "\u{FFFD}");
        no_nul.as_str()
    } else {
        src
    };
    // GitHub's `$`x`$` inline math is rewritten to `$x$` before parsing.
    let github;
    let src = match github_math(src) {
        Cow::Owned(rewritten) => {
            github = rewritten;
            github.as_str()
        }
        Cow::Borrowed(same) => same,
    };
    let protected = autolink::protect_url_delimiters(src);
    let restore = matches!(protected, Cow::Owned(_));
    if memchr::memchr(b'$', protected.as_bytes()).is_none() {
        let mut b = Builder::new(&protected, restore, false, front, task_source, &mut *wiki);
        for (event, range) in Parser::new_ext(&protected, options()).into_offset_iter() {
            b.event(event, range);
        }
        if b.rejected_alerts.is_empty() {
            return b.into_document();
        }
        let rejected = std::mem::take(&mut b.rejected_alerts);
        drop(b);
        return parse_with_checks(protected, restore, rejected, front, task_source, wiki);
    }
    parse_with_checks(protected, restore, Vec::new(), front, task_source, wiki)
}

#[derive(Clone, Copy, PartialEq)]
enum Origin {
    Md,
    /// Opened implicitly around inline content.
    Auto,
    Html(Tag),
}

#[derive(Clone, Copy, PartialEq)]
enum Node {
    Block(u32),
    Span(u32),
    /// Balances an event or tag that produced nothing, such as a collapsed nested strong.
    Nothing,
}

#[derive(Clone, Copy)]
struct Open {
    node: Node,
    origin: Origin,
}

/// An inline HTML tag left open at the end of an HTML block. It wraps the next blocks' inline
/// content until closed, which is how `<a href=x>`, a blank line, then an image is meant to work.
struct Ambient {
    tag: Tag,
    kind: SpanKind,
    depth: usize,
}

/// What replacing YAML front matter with a table did to the line count, so a block's line is still
/// the line of the file the reader would find it on.
#[derive(Clone, Copy, Default)]
struct Front {
    /// Lines of the Markdown that stands in for the front matter.
    replacement_lines: u32,
    /// Lines of the front matter in the file.
    original_lines: u32,
}

/// How far along one line of the file the next mark is ([`Builder::mark`]): a line longer
/// than this is a paragraph written as one line, which wraps to several on the page.
const MARK_GAP: usize = 64;

fn count_lines(bytes: &[u8]) -> u32 {
    // A plain loop: the compiler turns it into a vector count, which beats one search per line.
    bytes.iter().filter(|&&b| b == b'\n').count() as u32
}

struct Builder<'a> {
    src: &'a str,
    restore: bool,
    front: Front,
    task_source: Option<TaskSource<'a>>,
    task_events: Option<TaskEvents>,
    /// Where the event being handled starts in `src`: the place of any block it opens.
    event_at: usize,
    /// The last mark's place in `src`, with its line and the byte in it ([`Builder::mark`]).
    mark_src: usize,
    mark_line: u32,
    mark_col: u32,
    /// The line breaks of `src` not yet counted, the next one's offset (`usize::MAX` for none),
    /// and how many breaks lie before `counted_to`. Blocks open in order, so one walk over the
    /// breaks finds every block's line.
    breaks: memchr::Memchr<'a>,
    next_break: usize,
    counted_to: usize,
    counted: u32,
    doc: Document,
    stack: Vec<Open>,
    md_span_depth: usize,
    // At each truncation, the prefix of anchors already present must land no later
    // than the new end. Apply those limits in one backward pass at the end.
    anchor_cuts: Vec<(usize, u32)>,
    cur_block: u32,
    cur_span: u32,
    inline_open: bool,
    inline_block: u32,
    inline_start: usize,
    atom_end: usize,
    // Start of the text run still waiting for the autolink pass, and its escaped characters.
    run_start: Option<usize>,
    escapes: Vec<(usize, usize)>,
    link_hits: Vec<(usize, usize, LinkKind)>,
    /// Says what each `[[note]]` becomes.
    wiki: &'a mut dyn FnMut(&str, bool) -> WikiAs,
    /// A wiki-link changed the heading's text, so its anchor comes from the text shown.
    heading_wiki: bool,
    /// The span of a wiki-link whose label holds markup (`[[Note|a *b* c]]`) while its closing
    /// `]]` is still to come, or `NONE`.
    wiki_open: u32,
    /// A `<br clear>` was read: the next block that opens starts below the floats at that side.
    pending_clear: Option<Clear>,
    /// Set by `find_wiki` when the run ends inside such a label.
    wiki_pending: Option<WikiOpen>,
    link_depth: u32,
    strong_depth: u32,
    in_code_block: bool,
    html_buf: Option<String>,
    html_store: String,
    pre_open: bool,
    pre_first_newline: bool,
    ambient: Vec<Ambient>,
    ambient_spans_left: usize,
    open_html: [u32; Tag::SLOTS],
    /// Inside a `<rp>`, whose text only a browser with no ruby support shows. It ends with its
    /// closer, the next `<rt>` or `</ruby>`, or the block, so a stray `<rp>` cannot hide the rest.
    in_rp: bool,
    html_depth: usize,
    image_depth: u32,
    image_alt_start: usize,
    image_idx: u32,
    in_picture: bool,
    // The block the picture opened in, and how deep the stack was: a source counts only as a direct
    // child, and the picture ends when its block does.
    picture_block: u32,
    picture_depth: usize,
    picture_dark: StrRef,
    picture_light: StrRef,
    aligns: Vec<Alignment>,
    col: usize,
    header_row: bool,
    inline_limit: usize,
    slugger: Slugger,
    in_heading: bool,
    heading_text: String,
    // End of the last text event: the backslashes before it belong to that event.
    text_floor: usize,
    footnote_ids: HashMap<UniCase<String>, u32>,
    next_footnote_number: u32,
    // Events to ignore until a repeated footnote definition ends. The first definition wins.
    skip_depth: u32,
    // Text ranges of named entities in the current run, which a bare URL can't end on.
    entities: Vec<(usize, usize)>,
    block_depth: usize,
    // Implied table rows, cells and list items, innermost last. They exist only to give stray
    // content a legal home, so an empty one is dropped when it closes instead of drawing a gap.
    implied: Vec<u32>,
    // At most one: an alert can't nest. The block, and where its marker's `[` is in the source.
    open_alert: Option<(u32, usize)>,
    // Whether anything visible was added inside the open alert. GitHub keeps an alert whose body is
    // only empty inline HTML (`<i></i>`, `<a href=x></a>`, `<p></p>`, `<img>`), though nothing of it
    // reaches the model. A stray `<foo>` or a raw `<input>` adds nothing there either.
    alert_content: bool,
    rejected_alerts: Vec<usize>,
    // The last pass, or a document that can't hide a marker: a rejected one is written out instead.
    literal_alerts: bool,
}

impl<'a> Builder<'a> {
    fn new(
        src: &'a str,
        restore: bool,
        literal_alerts: bool,
        front: Front,
        task_source: Option<TaskSource<'a>>,
        wiki: &'a mut dyn FnMut(&str, bool) -> WikiAs,
    ) -> Self {
        let mut breaks = memchr::memchr_iter(b'\n', src.as_bytes());
        let next_break = breaks.next().unwrap_or(usize::MAX);
        Builder {
            src,
            restore,
            front,
            task_source,
            task_events: task_source.and_then(|source| source.events(src)),
            event_at: 0,
            mark_src: 0,
            mark_line: 0,
            mark_col: 0,
            breaks,
            next_break,
            counted_to: 0,
            counted: 0,
            doc: Document {
                text: String::with_capacity(src.len()),
                blocks: Vec::with_capacity(src.len() / 64),
                spans: Vec::with_capacity(src.len() / 64),
                ..Document::default()
            },
            stack: Vec::with_capacity(32),
            md_span_depth: 0,
            anchor_cuts: Vec::new(),
            cur_block: NONE,
            cur_span: NONE,
            inline_open: false,
            inline_block: NONE,
            inline_start: 0,
            atom_end: usize::MAX,
            run_start: None,
            escapes: Vec::new(),
            link_hits: Vec::new(),
            wiki,
            heading_wiki: false,
            wiki_open: NONE,
            pending_clear: None,
            wiki_pending: None,
            link_depth: 0,
            strong_depth: 0,
            in_code_block: false,
            html_buf: None,
            html_store: String::new(),
            pre_open: false,
            pre_first_newline: false,
            ambient: Vec::new(),
            ambient_spans_left: src.len() / 4 + 64,
            open_html: [0; Tag::SLOTS],
            in_rp: false,
            html_depth: 0,
            image_depth: 0,
            image_alt_start: 0,
            image_idx: 0,
            in_picture: false,
            picture_block: NONE,
            picture_depth: 0,
            picture_dark: StrRef::default(),
            picture_light: StrRef::default(),
            aligns: Vec::new(),
            col: 0,
            header_row: false,
            inline_limit: 0,
            slugger: Slugger::default(),
            in_heading: false,
            heading_text: String::new(),
            text_floor: 0,
            footnote_ids: HashMap::new(),
            next_footnote_number: 0,
            skip_depth: 0,
            entities: Vec::new(),
            block_depth: 0,
            implied: Vec::new(),
            open_alert: None,
            alert_content: false,
            rejected_alerts: Vec::new(),
            literal_alerts,
        }
    }

    /// The line of the file that byte `at` of `src` is on, counting from 1.
    fn line_of(&mut self, at: usize) -> u32 {
        let at = at.min(self.src.len());
        let before = if at >= self.counted_to {
            while self.next_break < at {
                self.counted += 1;
                self.next_break = self.breaks.next().unwrap_or(usize::MAX);
            }
            self.counted_to = at;
            self.counted
        } else {
            // A block that opens at an earlier place than the last: the walk stays where it is.
            self.counted - count_lines(&self.src.as_bytes()[at..self.counted_to])
        };
        let Front {
            replacement_lines,
            original_lines,
        } = self.front;
        if replacement_lines == 0 {
            before + 1
        } else if before < replacement_lines {
            1
        } else {
            before - replacement_lines + original_lines + 1
        }
    }

    /// Notes that what the file has at `at` is at `text` in the document's text
    /// ([`LineMark`]): once for each line a piece of text starts on, and again along a line
    /// every [`MARK_GAP`] bytes, so a file of short lines pays one mark a line.
    fn mark(&mut self, at: usize, text: usize) {
        let line = self.line_of(at);
        let col = if line == self.mark_line && at >= self.mark_src {
            if at - self.mark_src < MARK_GAP {
                return;
            }
            self.mark_col as usize + (at - self.mark_src)
        } else {
            let at = at.min(self.src.len());
            at - memchr::memrchr(b'\n', &self.src.as_bytes()[..at]).map_or(0, |i| i + 1)
        };
        (self.mark_src, self.mark_line, self.mark_col) = (at, line, col as u32);
        self.doc.marks.push(LineMark {
            line,
            col: col as u32,
            text: text as u32,
        });
    }

    /// The text of a code block: its lines are the file's, so each gets its mark.
    fn code_text(&mut self, t: &str, at: usize) {
        let start = self.doc.text.len();
        self.mark(at, start);
        self.push_str_restored(t);
        let pushed = &self.doc.text.as_bytes()[start..];
        if memchr::memchr(b'\n', &pushed[..pushed.len().saturating_sub(1)]).is_none() {
            return;
        }
        // Several lines in one piece: the line after each break. The place in the file is
        // not known exactly (an indented block drops its indent), the line is.
        let mut line = self.mark_line;
        let last = pushed.len() - 1;
        let breaks: Vec<usize> = memchr::memchr_iter(b'\n', &pushed[..last]).collect();
        for i in breaks {
            line += 1;
            self.doc.marks.push(LineMark {
                line,
                col: 0,
                text: (start + i + 1) as u32,
            });
        }
        // The next piece is on a later line, so it gets its own mark.
        self.mark_line = line;
        self.mark_src = usize::MAX;
    }

    fn event(&mut self, event: Event<'a>, range: Range<usize>) {
        self.event_at = range.start;
        if self.image_depth > 0 {
            return self.image_event(event);
        }
        if self.skip_depth > 0 {
            match event {
                Event::Start(_) => self.skip_depth += 1,
                Event::End(_) => self.skip_depth -= 1,
                // A reference inside a duplicate definition still counts on GitHub: it numbers the
                // note and puts it in the list, even though this definition's text is dropped.
                Event::FootnoteReference(label) => {
                    self.number_footnote(&label);
                }
                _ => {}
            }
            return;
        }
        if self.pre_open {
            return self.pre_event(event);
        }
        match event {
            Event::Start(tag) => self.start(tag, range),
            Event::End(TagEnd::HtmlBlock) => self.end_html_block(),
            Event::End(_) => self.end(),
            Event::Text(t) if self.in_code_block => self.code_text(&t, range.start),
            Event::Text(t) => self.md_text(&t, range),
            Event::Code(t) => {
                self.begin_inline();
                self.flush_run();
                self.note_heading_text(&t);
                let start = self.doc.text.len();
                self.mark(range.start, start);
                self.push_str_restored(&t);
                self.push_atom_like(SpanKind::Code, start);
            }
            Event::InlineMath(t) => self.math(&t, false),
            Event::DisplayMath(t) => self.math(&t, true),
            Event::Html(t) => match &mut self.html_buf {
                Some(buf) => {
                    if self.restore {
                        buf.push_str(&autolink::restore(&t));
                    } else {
                        buf.push_str(&t);
                    }
                }
                None => self.html_tokens(&autolink::restore(&t)),
            },
            Event::InlineHtml(t) => {
                if self.restore {
                    self.html_tokens(&autolink::restore(&t));
                } else {
                    self.html_tokens(&t);
                }
            }
            Event::FootnoteReference(label) => {
                self.begin_inline();
                let id = self.number_footnote(&label);
                if self.in_heading {
                    let number = self.doc.footnotes[id as usize].number.to_string();
                    self.heading_text.push_str(&number);
                }
                self.push_atom(SpanKind::FootnoteRef(id));
            }
            Event::SoftBreak => self.line_break(' '),
            Event::HardBreak => self.line_break('\n'),
            Event::Rule => {
                self.close_inline_for_block();
                // Under a marker line, `---` makes that line a setext heading, so no alert. Only
                // when no blank line separates them: after a blank line it is an ordinary rule.
                if self.first_in_open_alert()
                    && self.underlines_open_alert(range.start)
                    && self.src[range].trim_end().bytes().all(|c| c == b'-')
                {
                    self.reject_open_alert();
                }
                let b = self.open_block(BlockKind::ThematicBreak, Origin::Md);
                self.pop_open();
                self.finish_block(b);
            }
            Event::TaskListMarker(checked) => {
                self.begin_inline();
                self.mark_task_item();
                self.push_atom(SpanKind::Checkbox { checked });
                if let Some(tasks) = &mut self.task_events {
                    tasks.marker(self.doc.spans.len() - 1, range.end, checked);
                }
            }
        }
    }

    fn image_event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(_) => self.image_depth += 1,
            Event::End(_) => {
                self.image_depth -= 1;
                if self.image_depth == 0 {
                    let alt = StrRef {
                        start: self.image_alt_start as u32,
                        len: (self.doc.strings.len() - self.image_alt_start) as u32,
                    };
                    self.doc.images[self.image_idx as usize].alt = alt;
                }
            }
            Event::Text(t) | Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t) => {
                if self.restore {
                    self.doc.strings.push_str(&autolink::restore(&t));
                } else {
                    self.doc.strings.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak => self.doc.strings.push(' '),
            _ => {}
        }
    }

    /// Markdown inside a `<pre>` opened by an inline tag: a code block holds plain text only.
    fn pre_event(&mut self, event: Event<'a>) {
        match event {
            Event::Start(_) => self.push_open(Open {
                node: Node::Nothing,
                origin: Origin::Md,
            }),
            Event::End(_) => self.end(),
            Event::Text(t) | Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t) => {
                let restored = if self.restore {
                    autolink::restore(&t)
                } else {
                    Cow::Borrowed(&*t)
                };
                self.pre_text(&restored);
            }
            Event::SoftBreak | Event::HardBreak => self.pre_text("\n"),
            Event::Html(t) | Event::InlineHtml(t) => self.html_tokens(&autolink::restore(&t)),
            _ => {}
        }
    }

    fn start(&mut self, tag: MdTag<'_>, range: Range<usize>) {
        match tag {
            MdTag::Emphasis => self.open_md_span(SpanKind::Emphasis),
            MdTag::Strong => self.open_strong(Origin::Md),
            MdTag::Strikethrough => self.open_md_span(SpanKind::Strike),
            MdTag::Link {
                link_type,
                dest_url,
                title,
                ..
            } => {
                let dest = if matches!(link_type, LinkType::Email) {
                    self.pool_restored(&format!("mailto:{dest_url}"))
                } else {
                    self.pool_restored(&dest_url)
                };
                let title = self.pool_restored(&title);
                let idx = self.doc.links.len() as u32;
                self.doc.links.push(LinkInfo::new(dest, title));
                self.open_md_span(SpanKind::Link(idx));
            }
            MdTag::Image {
                dest_url, title, ..
            } => {
                self.begin_inline();
                let src = self.pool_restored(&dest_url);
                let title = self.pool_restored(&title);
                self.image_idx = self.doc.images.len() as u32;
                self.doc.images.push(ImageInfo {
                    src,
                    title,
                    ..ImageInfo::default()
                });
                self.image_alt_start = self.doc.strings.len();
                self.image_depth = 1;
                self.push_atom(SpanKind::Image(self.image_idx));
            }
            MdTag::Paragraph => {
                // `--` and `-` are setext underlines to GitHub, though pulldown-cmark reads them
                // as paragraph text. They underline the marker line, so this is not an alert.
                if self.first_in_open_alert()
                    && self.underlines_open_alert(range.start)
                    && is_dash_run(&self.src[range.clone()])
                {
                    self.reject_open_alert();
                }
                self.md_block(BlockKind::Paragraph { align: Align::None }, true, range)
            }
            MdTag::Heading { level, .. } => {
                // A setext underline turns what is above it into the heading, marker line included.
                // An ATX heading is a child instead, and so is a setext heading after a blank line.
                if self.first_in_open_alert()
                    && self.underlines_open_alert(range.start)
                    && !is_atx_heading(&self.src[range.start..])
                {
                    self.reject_open_alert();
                }
                self.md_block(
                    BlockKind::Heading {
                        level: level as u8,
                        align: Align::None,
                        slug: StrRef::default(),
                    },
                    true,
                    range,
                );
            }
            MdTag::BlockQuote(kind) => {
                let kind = kind.map(|k| match k {
                    BlockQuoteKind::Note => AlertKind::Note,
                    BlockQuoteKind::Tip => AlertKind::Tip,
                    BlockQuoteKind::Important => AlertKind::Important,
                    BlockQuoteKind::Warning => AlertKind::Warning,
                    BlockQuoteKind::Caution => AlertKind::Caution,
                });
                self.block_quote(kind, range);
            }
            MdTag::CodeBlock(kind) => {
                let info = match &kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or(""),
                    CodeBlockKind::Indented => "",
                };
                let lang = self.pool_restored(info);
                let kind = match info {
                    "mermaid" => BlockKind::Diagram {
                        lang,
                        oversize: None,
                    },
                    "math" => BlockKind::Math { oversize: None },
                    _ => BlockKind::CodeBlock { lang },
                };
                self.md_block(kind, false, range);
                self.in_code_block = true;
            }
            MdTag::HtmlBlock => {
                self.flush_run();
                let mut buf = std::mem::take(&mut self.html_store);
                buf.clear();
                self.html_buf = Some(buf);
            }
            MdTag::List(start) => {
                self.md_block(
                    BlockKind::List {
                        start: start.map(|n| n.min(u64::from(u32::MAX)) as u32),
                    },
                    false,
                    range,
                );
            }
            MdTag::Item => self.md_block(BlockKind::ListItem { task: false }, false, range),
            MdTag::FootnoteDefinition(label) => {
                // GitHub draws a footnote definition as a list item, so a marker line immediately
                // above one makes a plain quote, not an alert.
                if self.first_in_open_alert() && self.underlines_open_alert(range.start) {
                    self.reject_open_alert();
                }
                let id = self.footnote(&label);
                if self.doc.footnotes[id as usize].defined {
                    self.close_inline_for_block();
                    self.skip_depth = 1;
                    return;
                }
                // First definition wins. At the depth limit it gets no block of its own, so the
                // content joins an ancestor; `block` must then stay NONE rather than name that
                // ancestor, and `defined` is what keeps a later definition from replacing it.
                self.doc.footnotes[id as usize].defined = true;
                self.md_block(BlockKind::FootnoteDefinition { footnote: id }, false, range);
                let b = self.cur_block;
                if b != NONE
                    && matches!(
                        self.doc.blocks[b as usize].kind,
                        BlockKind::FootnoteDefinition { footnote } if footnote == id
                    )
                {
                    self.doc.footnotes[id as usize].block = b;
                }
            }
            MdTag::Table(aligns) => {
                self.aligns = aligns;
                self.md_block(BlockKind::Table, false, range);
            }
            MdTag::TableHead => {
                self.col = 0;
                self.header_row = true;
                self.md_block(BlockKind::TableRow { header: true }, false, range);
            }
            MdTag::TableRow => {
                self.col = 0;
                self.header_row = false;
                self.md_block(BlockKind::TableRow { header: false }, false, range);
            }
            MdTag::TableCell => {
                let align = match self.aligns.get(self.col) {
                    Some(Alignment::Left) => Align::Left,
                    Some(Alignment::Center) => Align::Center,
                    Some(Alignment::Right) => Align::Right,
                    _ => Align::None,
                };
                self.col += 1;
                let kind = BlockKind::TableCell {
                    align,
                    header: self.header_row,
                    colspan: 1,
                    rowspan: 1,
                };
                self.md_block(kind, true, range);
            }
            _ => {
                self.flush_run();
                self.push_open(Open {
                    node: Node::Nothing,
                    origin: Origin::Md,
                });
            }
        }
    }

    fn pool(&mut self, s: &str) -> StrRef {
        if s.is_empty() {
            return StrRef::default();
        }
        let start = self.doc.strings.len() as u32;
        self.doc.strings.push_str(s);
        StrRef {
            start,
            len: s.len() as u32,
        }
    }

    fn pool_restored(&mut self, s: &str) -> StrRef {
        if self.restore {
            let restored = autolink::restore(s);
            self.pool(&restored)
        } else {
            self.pool(s)
        }
    }

    fn footnote(&mut self, label: &str) -> u32 {
        // pulldown-cmark matches a reference to its definition ignoring case, so its labels must be
        // folded the same way here. The first spelling seen is the one kept for display.
        let key = UniCase::new(label.to_string());
        if let Some(&id) = self.footnote_ids.get(&key) {
            return id;
        }
        let label_ref = self.pool_restored(label);
        let id = self.doc.footnotes.len() as u32;
        self.doc.footnotes.push(Footnote {
            label: label_ref,
            number: 0,
            block: NONE,
            defined: false,
        });
        self.footnote_ids.insert(key, id);
        id
    }

    /// Numbers a footnote the first time any reference reaches it, in reference order as GitHub
    /// does, and returns its id. Duplicate definitions share the id, so they share the number.
    fn number_footnote(&mut self, label: &str) -> u32 {
        let id = self.footnote(label);
        let fnote = &mut self.doc.footnotes[id as usize];
        if fnote.number == 0 {
            self.next_footnote_number += 1;
            fnote.number = self.next_footnote_number;
        }
        id
    }

    fn mark_task_item(&mut self) {
        let mut b = self.cur_block;
        while b != NONE {
            let block = &mut self.doc.blocks[b as usize];
            if let BlockKind::ListItem { task } = &mut block.kind {
                *task = true;
                return;
            }
            b = block.parent;
        }
    }

    fn into_document(mut self) -> Parsed {
        self.flush_run();
        while let Some(open) = self.pop_open() {
            self.close(open);
        }
        let mut doc = self.doc;
        let mut last = doc.blocks.len().saturating_sub(1) as u32;
        for (i, anchor) in doc.anchors.iter_mut().enumerate().rev() {
            while let Some((_, limit)) = self.anchor_cuts.pop_if(|cut| cut.0 > i) {
                last = last.min(limit);
            }
            anchor.block = anchor.block.min(last);
        }
        if doc.blocks.is_empty() {
            doc.anchors.clear();
        }
        doc.text.shrink_to_fit();
        doc.strings.shrink_to_fit();
        doc.blocks.shrink_to_fit();
        doc.spans.shrink_to_fit();
        doc.links.shrink_to_fit();
        doc.images.shrink_to_fit();
        doc.headings.shrink_to_fit();
        doc.anchors.shrink_to_fit();
        doc.footnotes.shrink_to_fit();
        doc.block_attrs.shrink_to_fit();
        let tasks = self
            .task_source
            .zip(self.task_events)
            .and_then(|(source, events)| source.resolve(self.src, &doc, events));
        Parsed { doc, tasks }
    }
}

#[cfg(test)]
mod limit_tests {
    use super::*;

    #[test]
    fn repeated_block_drops_leave_one_deferred_anchor_cut() {
        let mut wiki = |_: &str, _: bool| WikiAs::Text;
        let mut b = Builder::new("", false, false, Front::default(), None, &mut wiki);
        for _ in 0..512 {
            let id = b.open_block(BlockKind::Paragraph { align: Align::None }, Origin::Auto);
            b.doc.anchors.push(Anchor {
                name: StrRef::default(),
                block: id + 1,
            });
            b.pop_open();
            b.finish_block(id);
            assert_eq!(b.anchor_cuts.as_slice(), &[(b.doc.anchors.len(), id)]);
        }
        // No anchor has been rewritten, not even the earliest. When block indices
        // are reused below, the deferred cut still applies only to the old anchors.
        assert!(b.doc.anchors.iter().all(|a| a.block == 1));
        for _ in 0..2 {
            let id = b.open_block(BlockKind::Plain, Origin::Md);
            b.pop_open();
            b.finish_block(id);
        }
        b.doc.anchors.push(Anchor {
            name: StrRef::default(),
            block: 1,
        });
        let doc = b.into_document().doc;
        assert!(doc.anchors[..512].iter().all(|a| a.block == 0));
        assert_eq!(doc.anchors[512].block, 1);
    }

    #[test]
    fn markdown_span_depth_is_bounded_and_closers_stay_balanced() {
        let depth = crate::limits::MAX_MD_SPAN_DEPTH;
        let doc = parse(&format!(
            "{}x{} *after*",
            "*a _b ".repeat(depth),
            " c_ d*".repeat(depth)
        ));
        let x = doc.text.find('x').unwrap() as u32;
        let wrappers = doc
            .spans
            .iter()
            .filter(|s| s.start <= x && x < s.end)
            .count();
        assert_eq!(wrappers, depth);
        let html = crate::rich::selection_html(&doc, 0, doc.text.len() as u32).unwrap();
        assert_eq!(html.matches("<em>").count(), depth + 1);
        assert!(html.ends_with(" <em>after</em></p>"));
    }
}
