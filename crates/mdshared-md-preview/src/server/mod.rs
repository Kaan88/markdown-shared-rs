//! The loop that draws the page for an editor: in mdview's process (`mdview --preview-server`,
//! [`serve_stdio`]) or on a thread of the editor's own ([`serve`]).

mod scroll_sync;
#[cfg(test)]
mod tests;

use scroll_sync::{Sync, knots};

use crate::wire::{self, BlockLines, Copied, Pointer, Request, Update};
use mdshared_md_core::model::Document;
use mdshared_md_render::{DARK, DocumentRenderer, Gap, Jump, LIGHT, Options};
use std::io::{self, BufReader, BufWriter, Write};
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

/// Where a followed line goes: this far below the pane's top (logical px), as mdview puts a
/// line it jumps to.
const MARGIN: f32 = 24.0;

/// The state of one preview: the document, the pane and where the page is.
pub struct Server {
    renderer: DocumentRenderer,
    doc: Option<Document>,
    options: Options,
    /// The pane in device pixels; nothing is drawn until it is known.
    size: Option<(u32, u32)>,
    /// The page row at the pane's top, device pixels.
    top: f32,
    /// The caret's place, to show again after the text changes.
    follow: Option<(u32, u32)>,
    /// The source's line count, for the page's end in the scroll map.
    lines: u32,
    /// The editor's last screen of scrolling, `(tail, end)` in [`Request::Align`]'s units:
    /// over it the page goes on to its own bottom.
    end: Option<(f32, f32)>,
    /// The selection, two offsets into `Document::text`, anchor first, as mdview keeps it.
    selection: Option<(u32, u32)>,
    /// A press in progress: the range a drag extends from, and whether the pointer moved.
    press: Option<((u32, u32), bool)>,
    /// The selection's rectangles as the editor last got them.
    shown: Vec<[f32; 4]>,
    /// The selection (or the layout under it) changed since the editor last got it.
    selection_dirty: bool,
    /// The editor asked for the selection's text.
    copy: bool,
    /// The text the page was parsed from, as the editor sent it: where a task box's mark is.
    source: String,
    /// A task box was clicked: its mark's byte offset in `source` and its new state.
    task: Option<(u32, bool)>,
    /// The next text is the editor's answer to a tick: the page stays where it is, since the
    /// caret did not move to the box (a moved caret still comes as its own `Follow`).
    keep_top: bool,
    /// The answer to the last `Hover`, until it is sent.
    pointer: Option<Pointer>,
    /// The answer to the last `BlockAt`, until it is sent.
    block: Option<Option<BlockLines>>,
    /// Something changed since the last frame (or there has been none).
    dirty: bool,
    /// The editor's copy of the frame is gone: the next frame is sent whole.
    whole: bool,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    pub fn new() -> Server {
        Server {
            renderer: DocumentRenderer::new(),
            doc: None,
            options: Options::default(),
            size: None,
            top: 0.0,
            follow: None,
            lines: 1,
            end: None,
            selection: None,
            press: None,
            shown: Vec::new(),
            selection_dirty: false,
            copy: false,
            source: String::new(),
            task: None,
            keep_top: false,
            pointer: None,
            block: None,
            dirty: true,
            whole: true,
        }
    }

    /// Takes a request in. Nothing is drawn until [`frame`](Self::frame). False for
    /// [`Request::Quit`].
    pub fn apply(&mut self, req: Request) -> bool {
        match req {
            Request::Text { text, base_dir } => {
                let doc = mdshared_md_core::parse(&text);
                self.lines = text.bytes().filter(|&b| b == b'\n').count() as u32 + 1;
                if self.options.base_dir != base_dir {
                    self.options.base_dir = base_dir;
                }
                // A new text is laid out anew: the renderer's own check sees only the length.
                if self.size.is_some() {
                    self.renderer.layout(&doc, &self.options);
                }
                self.doc = Some(doc);
                self.source = text;
                // The offsets were into the old text.
                self.selection = None;
                self.press = None;
                self.selection_dirty = true;
                // A tick not answered yet was found in the old text, so its offset says nothing
                // of this one: it is dropped (the click does nothing), and this text is not the
                // editor's answer to it, so the page follows the caret as for any (audit 4, E7).
                if self.task.take().is_some() {
                    self.keep_top = false;
                }
                // The page shows the caret in the new text; it stays still while it does. After a
                // tick the reader is looking at the box, not the caret: the page stays.
                if std::mem::take(&mut self.keep_top) {
                    self.renderer.set_top(self.top);
                } else if let Some((line, column)) = self.follow {
                    self.go_to(line, column);
                }
            }
            Request::View {
                width,
                height,
                scale,
                dark,
            } => {
                // No larger than a frame can carry: a few hundred thousand px a side asked for
                // tens of gigabytes (audit 4, E10).
                let (width, height) = (width.min(wire::MAX_SIDE), height.min(wire::MAX_SIDE));
                let scale = if scale.is_finite() && scale > 0.1 {
                    scale.min(8.0)
                } else {
                    1.0
                };
                let options = Options {
                    width: width as f32 / scale,
                    scale,
                    theme: if dark { DARK } else { LIGHT },
                    max_height: None,
                    base_dir: self.options.base_dir.clone(),
                };
                let new_size = Some((width.max(1), height.max(1)));
                if new_size != self.size || !same(&options, &self.options) {
                    self.size = new_size;
                    self.options = options;
                    if let Some(doc) = &self.doc {
                        self.renderer.layout(doc, &self.options);
                    }
                    self.whole = true;
                    self.selection_dirty = true;
                    if let Some((line, column)) = self.follow {
                        self.go_to(line, column);
                    }
                }
            }
            Request::Follow { line, column } => {
                self.follow = Some((line, column));
                self.go_to(line, column);
            }
            Request::Scroll { by } => {
                if by.is_finite() {
                    self.top = (self.top + by).max(0.0);
                }
            }
            Request::Align { line, tail, end } => {
                self.end = (tail.is_finite() && end.is_finite()).then_some((tail, end));
                self.align(line);
            }
            Request::Press { x, y, clicks } => {
                self.press = None;
                if let Some(flip) = (clicks <= 1).then(|| self.task_at(x, y)).flatten() {
                    // A click on a task box ticks it in the editor's text, not a selection.
                    self.task = Some(flip);
                    self.keep_top = true;
                    if self.selection.take().is_some() {
                        self.selection_dirty = true;
                    }
                } else if let Some((doc, offset)) = self.offset_at(x, y) {
                    let anchor = match clicks {
                        0 | 1 => (offset, offset),
                        2 => mdshared_md_core::select::word_range(doc, offset),
                        _ => mdshared_md_core::select::block_range(doc, offset),
                    };
                    self.selection = Some(anchor);
                    self.press = Some((anchor, clicks > 1));
                    self.selection_dirty = true;
                }
            }
            Request::Drag { x, y } => {
                if let Some(((a, b), _)) = self.press
                    && let Some((_, offset)) = self.offset_at(x, y)
                {
                    let selection = if offset < a {
                        (b, offset)
                    } else if offset > b {
                        (a, offset)
                    } else {
                        (a, b)
                    };
                    if let Some((_, moved)) = self.press.as_mut() {
                        *moved |= offset != a;
                    }
                    if self.selection != Some(selection) {
                        self.selection = Some(selection);
                        self.selection_dirty = true;
                    }
                }
            }
            Request::Hover { x, y } => {
                self.pointer = Some(self.pointer_at(x, y));
            }
            Request::Release => {
                // A click that did not move clears the selection; a drag leaves it.
                if let Some((_, false)) = self.press.take()
                    && self.selection.take().is_some()
                {
                    self.selection_dirty = true;
                }
            }
            Request::SelectAll => {
                if let Some(doc) = &self.doc {
                    self.selection = Some((0, doc.text.len() as u32));
                    self.selection_dirty = true;
                }
            }
            Request::Unselect => {
                self.press = None;
                if self.selection.take().is_some() {
                    self.selection_dirty = true;
                }
            }
            Request::Copy => self.copy = true,
            Request::Gap { gap } => {
                let gap = gap.map(|(first_line, last_line, height)| Gap {
                    first_line,
                    last_line,
                    height: if height.is_finite() {
                        height.max(0.0)
                    } else {
                        0.0
                    },
                });
                let empty = Document::default();
                self.renderer
                    .set_gap(self.doc.as_ref().unwrap_or(&empty), gap);
            }
            Request::BlockAt { x, y } => self.block = Some(self.block_at(x, y)),
            Request::BlockOfLine { line } => self.block = Some(self.block_of_line(line)),
            Request::Unfollow => self.follow = None,
            Request::Quit => return false,
        }
        self.dirty = true;
        true
    }

    /// Moves the page so that source line `line` shows, only when that line is not in view
    /// already, so a caret moving within the pane leaves the page still.
    fn go_to(&mut self, line: u32, column: u32) {
        let (Some(doc), Some((_, height))) = (&self.doc, self.size) else {
            return;
        };
        let Some(block) = doc.block_at_line(line) else {
            return;
        };
        let text = doc.blocks[block as usize].text;
        let target = if text.is_empty() {
            Jump::Block(block)
        } else {
            Jump::Text(doc.text_at_line(line, column).unwrap_or(text.start))
        };
        let margin = MARGIN * self.options.scale;
        let height = height as f32;
        let Some(top) = self
            .renderer
            .jump_to(doc, &self.options, target, margin, height)
        else {
            return;
        };
        // The line's row is `top + margin`: in view, with a margin, the page stays.
        let row = top + margin;
        if row >= self.top + margin && row <= self.top + height - 2.0 * margin {
            self.renderer.set_top(self.top);
            return;
        }
        self.top = top.round();
    }

    /// Puts source line `line` (fractional) at the pane's top, as the editor's top is; over the
    /// editor's last screen the page goes on to its bottom ([`Sync::row`]).
    fn align(&mut self, line: f32) {
        if !line.is_finite() {
            return;
        }
        let Some(sync) = self.sync() else {
            return;
        };
        self.top = sync.row(line).round();
        self.renderer.set_top(self.top);
    }

    /// The source line at page row `top`: the inverse of [`align`](Self::align), so that the
    /// lead going from one side to the other moves neither.
    fn line_at(&self, top: f32) -> f32 {
        self.sync().map_or(1.0, |s| s.line(top))
    }

    fn sync(&self) -> Option<Sync> {
        let (doc, page) = (self.doc.as_ref()?, self.renderer.page_layout()?);
        let height = self.size.map_or(0.0, |(_, h)| h as f32);
        Some(Sync {
            knots: knots(doc, page.blocks(), self.lines),
            bottom: (page.total_height() - height).max(0.0),
            end: self.end,
        })
    }

    /// The text offset under pane point `x, y` (device pixels), with the document. Finding it
    /// may lay out the block there, which the next frame shows.
    fn offset_at(&mut self, x: f32, y: f32) -> Option<(&Document, u32)> {
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        let doc = self.doc.as_ref()?;
        let hit = self
            .renderer
            .hit_test(doc, &self.options, x, y + self.top)?;
        Some((doc, hit.offset))
    }

    /// The task box at `x, y` (pane pixels), when there is one and the text agrees with the page
    /// about every box: the byte offset of its mark in the text and the state a click gives it.
    fn task_at(&mut self, x: f32, y: f32) -> Option<(u32, bool)> {
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        let doc = self.doc.as_ref()?;
        let block = self
            .renderer
            .hit_test(doc, &self.options, x, y + self.top)?
            .task?;
        let boxes = mdshared_md_core::tasks::document_boxes(doc);
        let index = boxes.iter().position(|&(b, _)| b == block)?;
        let states: Vec<bool> = boxes.iter().map(|&(_, checked)| checked).collect();
        let (at, byte) =
            mdshared_md_core::tasks::toggle(self.source.as_bytes(), index, &states).ok()?;
        Some((at as u32, byte == b'x'))
    }

    /// The cursor for `x, y` (pane pixels): a hand over a task box, the text cursor over text.
    fn pointer_at(&mut self, x: f32, y: f32) -> Pointer {
        if !(x.is_finite() && y.is_finite()) {
            return Pointer::Arrow;
        }
        let Some(doc) = self.doc.as_ref() else {
            return Pointer::Arrow;
        };
        match self.renderer.hit_test(doc, &self.options, x, y + self.top) {
            Some(hit) if hit.task.is_some() => Pointer::Hand,
            Some(hit) if hit.on_text => Pointer::Text,
            _ => Pointer::Arrow,
        }
    }

    /// The top-level block at pane point `x, y` and its source lines, from the layout: a point
    /// in the side margins goes to the block beside it, one below the last block to none.
    fn block_at(&mut self, x: f32, y: f32) -> Option<BlockLines> {
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        let doc = self.doc.as_ref()?;
        let page = self.renderer.page_layout()?;
        let span = page.block_at(y + self.top)?;
        if y + self.top > span.y + span.height {
            return None;
        }
        let (first, next) = self.renderer.block_lines(doc, span.block)?;
        // The page's text column, as the renderer lays it out (`Pager::new`).
        let o = &self.options;
        let scale = o.scale.max(0.1);
        let width = o.width.max(120.0);
        let column = (width - 2.0 * o.theme.margin)
            .max(80.0)
            .min(o.theme.reading_width);
        let left = (width - column) / 2.0 * scale;
        let down = (y + self.top - span.y) / span.height.max(1.0);
        let across = (x - left) / (column * scale);
        Some(BlockLines {
            first,
            next,
            at: Some((down.clamp(0.0, 1.0), across.clamp(0.0, 1.0))),
        })
    }

    /// The top-level block that holds source line `line` (from 1): the last that starts on it
    /// or before.
    fn block_of_line(&self, line: u32) -> Option<BlockLines> {
        let doc = self.doc.as_ref()?;
        let page = self.renderer.page_layout()?;
        let span = page
            .blocks()
            .iter()
            .take_while(|s| {
                doc.blocks
                    .get(s.block as usize)
                    .is_some_and(|b| b.line <= line)
            })
            .last()?;
        let (first, next) = self.renderer.block_lines(doc, span.block)?;
        Some(BlockLines {
            first,
            next,
            at: None,
        })
    }

    /// The answer to the last [`Request::BlockAt`], for [`Update::block`].
    pub fn take_block(&mut self) -> Option<Option<BlockLines>> {
        self.block.take()
    }

    /// Where the room for the editor's box starts on the page, while there is one.
    pub fn gap_y(&self) -> Option<f32> {
        self.renderer.gap_y(self.doc.as_ref()?)
    }

    /// The answer to the last [`Request::Hover`], for [`Update::pointer`].
    pub fn take_pointer(&mut self) -> Option<Pointer> {
        self.pointer.take()
    }

    /// The task box clicked since the last answer, for [`Update::task`].
    pub fn take_task(&mut self) -> Option<(u32, bool)> {
        self.task.take()
    }

    /// The selected range, ordered, when it is not empty.
    fn selected(&self) -> Option<(u32, u32)> {
        let (a, b) = self.selection?;
        (a != b).then(|| (a.min(b), a.max(b)))
    }

    /// What an answer carries besides the frame: the selection's rectangles when they changed
    /// (`drew`: a frame was drawn, which can move the blocks under them), and the selected text
    /// when the editor asked for it.
    pub fn selection_answer(&mut self, drew: bool) -> (Option<Vec<[f32; 4]>>, Option<Copied>) {
        let copied = std::mem::take(&mut self.copy).then(|| match (self.selected(), &self.doc) {
            (Some((from, to)), Some(doc)) => Copied {
                text: doc
                    .text
                    .get(from as usize..to as usize)
                    .unwrap_or_default()
                    .to_string(),
                html: mdshared_md_core::rich::selection_html(doc, from, to).unwrap_or_default(),
            },
            _ => Copied::default(),
        });
        let look = self.selection_dirty || (drew && self.selected().is_some());
        self.selection_dirty = false;
        if !look {
            return (None, copied);
        }
        let rects: Vec<[f32; 4]> = match (self.selected(), &self.doc) {
            (Some((from, to)), Some(doc)) if self.size.is_some() => self
                .renderer
                .selection_rects(doc, &self.options, from, to)
                .into_iter()
                .map(|r| [r.x, r.y, r.w, r.h])
                .collect(),
            _ => Vec::new(),
        };
        if rects == self.shown {
            return (None, copied);
        }
        self.shown = rects.clone();
        (Some(rects), copied)
    }

    /// Draws what changed since the last frame. `None` when nothing did, or before the pane's
    /// size and a text are known.
    pub fn frame(&mut self) -> Option<Update> {
        if !self.dirty {
            return None;
        }
        let (Some(doc), Some(size)) = (&self.doc, self.size) else {
            return None;
        };
        self.dirty = false;
        let started = Instant::now();
        let frame = self
            .renderer
            .view_frame_sized(doc, &self.options, self.top, size);
        let (width, rows) = (frame.bitmap.width, frame.bitmap.height);
        let row = width as usize * 4;
        let band = if self.whole { None } else { frame.band };
        let pixels = match band {
            None => frame.bitmap.pixels.clone(),
            Some((_, 0)) if frame.shift == 0 => {
                self.top = frame.top;
                return None;
            }
            Some((first, count)) => {
                frame.bitmap.pixels[first as usize * row..(first + count) as usize * row].to_vec()
            }
        };
        let (top, shift) = (frame.top, frame.shift);
        self.top = top;
        self.whole = false;
        let total = self
            .renderer
            .page_layout()
            .map_or(0.0, |page| page.total_height());
        let line = self.line_at(top);
        Some(Update {
            width,
            height: rows,
            top,
            total,
            shift,
            band,
            pixels,
            micros: started.elapsed().as_micros().min(u32::MAX as u128) as u32,
            answers: 0,
            line,
            selection: None,
            copied: None,
            task: None,
            pointer: None,
            gap_y: None,
            block: None,
        })
    }
}

fn same(a: &Options, b: &Options) -> bool {
    a.width == b.width
        && (a.scale - b.scale).abs() < 0.001
        && a.theme.key() == b.theme.key()
        && a.base_dir == b.base_dir
}

/// Runs a preview until `requests` ends or asks to quit: each turn takes every request that is
/// waiting, then draws once and hands the update to `send` (false from `send` ends it too).
pub fn serve(requests: Receiver<Request>, mut send: impl FnMut(Update) -> bool) {
    let mut server = Server::new();
    while let Ok(first) = requests.recv() {
        if !server.apply(first) {
            return;
        }
        let mut answers = 1;
        while let Ok(more) = requests.try_recv() {
            if !server.apply(more) {
                return;
            }
            answers += 1;
        }
        // Every batch is answered, a frame or an ack, so the editor knows nothing is pending.
        let drawn = server.frame();
        let (selection, copied) = server.selection_answer(drawn.is_some());
        let update = Update {
            selection,
            copied,
            task: server.take_task(),
            pointer: server.take_pointer(),
            block: server.take_block(),
            gap_y: server.gap_y(),
            ..match drawn {
                Some(update) => Update { answers, ..update },
                None => Update::ack(answers),
            }
        };
        if !send(update) {
            return;
        }
    }
}

/// `mdview --preview-server`: [`serve`] over standard input and output, after
/// [`MAGIC`](wire::MAGIC). Ends with the input. Returns the exit code.
pub fn serve_stdio() -> i32 {
    let mut out = BufWriter::with_capacity(1 << 20, io::stdout().lock());
    if out
        .write_all(wire::MAGIC)
        .and_then(|_| out.flush())
        .is_err()
    {
        return 1;
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut input = BufReader::new(io::stdin().lock());
        while let Ok(Some(req)) = wire::read_request(&mut input) {
            if tx.send(req).is_err() {
                break;
            }
        }
    });
    serve(rx, |update| wire::write_update(&mut out, &update).is_ok());
    0
}
