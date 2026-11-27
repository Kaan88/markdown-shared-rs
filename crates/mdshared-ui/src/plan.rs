//! The plan screen (`docs/ai-library-plan.md` 5.3): the card that shows a proposed set of notes,
//! lets the owner read and change it, and writes nothing until they say so. It is the Replace in
//! files confirmation (`replace_ui`) grown up, with the same parts: a ticked scrolling list, the
//! preview of the row the cursor is on, an options row, a footer, and `Space` to tick.
//!
//! This file is the state and the events: what is selected, ticked and being renamed, where each
//! part of the card is ([`geo`]), what a click or a key does, and the files each note would be
//! written as. `plan/draw.rs` draws it. The arithmetic of the notes, the rest, the warnings and
//! the counts is `mdshared_app::split` and nothing here repeats it; the arithmetic of a list of
//! equal rows is [`crate::rows`]. The headings come from the application (mdedit finds them with
//! its function list), and so does the key: [`PlanScreen::key`] takes the key's name and whether
//! it was the context-menu key. A plan is read from the document and written nowhere: the
//! footer's primary button only records that it was pressed ([`PlanScreen::write_pressed`]); the
//! writer, the record and undo are the next job.
//!
//! Nothing here exists until a plan is opened, so it is nothing on the launch path.

use crate::cursor::Cursor;
use crate::gfx::{Gfx, Rect};
use crate::keys::{KeyName, Mods};
use crate::menu::{Item, Menu};
use crate::overlay::{DIALOG_FOOTER, DIALOG_PAD, dialog_card};
use crate::rows::{Nav, Rows};
use crate::scroll::offset_for_thumb;
use crate::textfield::{EditLabels, FieldArea, FieldInput, FieldOutcome, TextField};
use mdshared_app::library::{self, folder_name, note_folder};
use mdshared_app::split::answer::Dropped;
use mdshared_app::split::{
    self, Fix, Heading, Levels, Lines, NoteLine, Plan, Rest, SPLIT_LEVELS, Totals, Warning,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

pub mod demo;
mod draw;

// ---- the numbers measured from the mockups (5.3) -------------------------------------------------

/// The card the screen asks for; `dialog_card` shrinks it to the window.
pub const CARD_W: f32 = 1200.0;
pub const CARD_H: f32 = 736.0;
pub const HEAD_H: f32 = 104.0;
/// The line under the header: a warning, or a notice, when there is one.
pub const NOTICE_H: f32 = 34.0;
pub const COVER_H: f32 = 64.0;
pub const STRIP_H: f32 = 18.0;
/// The list's width, and the card width below which the preview is dropped.
pub const LIST_W: f32 = 600.0;
pub const PREVIEW_MIN_CARD: f32 = 860.0;
pub const ROW_H: f32 = 44.0;
pub const LIST_HEAD: f32 = 40.0;
pub const PREVIEW_HEAD: f32 = 72.0;
/// The options row of a card with nothing in it: it takes no room, so an application that puts
/// no box in it draws the card as it always did.
pub const OPTIONS_H: f32 = 0.0;
/// The options row with the Recycle Bin box in it (5.4), when the application offers the box.
pub const OPTIONS_ROW_H: f32 = 44.0;
/// A line of the preview.
pub const LINE_H: f32 = 19.0;
/// The width of the preview's gutter of line numbers.
pub const GUTTER: f32 = 64.0;
/// The least a ticked note takes of the strip, in device pixels.
pub const MIN_BAND: f32 = 2.0;
/// Separators are drawn between the notes of the strip only when each is at least this wide.
pub const SEPARATE_AT: f32 = 6.0;

// ---- small texts -----------------------------------------------------------------------------------

/// 1,284. The shared [`mdshared_app::fmt::count`].
pub fn thousands(n: usize) -> String {
    mdshared_app::fmt::count(n as u64)
}

/// 1.9 KB. The shared [`mdshared_app::fmt::size`] up to a gigabyte; from there this one goes on
/// in megabytes, as it always has (a plan's note is never that large, and what a caller was
/// given for one does not change).
pub fn size_text(bytes: usize) -> String {
    if bytes < 1024 * 1024 * 1024 {
        mdshared_app::fmt::size(bytes as u64)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// "1 line", "34 lines".
pub fn lines_text(n: usize) -> String {
    if n == 1 {
        "1 line".to_string()
    } else {
        format!("{} lines", thousands(n))
    }
}

/// FNV-1a: what "the same bytes" means for a file that is already there.
pub fn fingerprint(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ---- the library the notes would go into -------------------------------------------------------------

/// What is on disk at a path: its length and its fingerprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    pub len: u64,
    pub hash: u64,
}

/// Looks a path up on disk: `None` when nothing is there.
pub type Reader = Box<dyn Fn(&Path) -> Option<Found>>;

/// The most of a file that is read to tell whether it is the note: a bigger file is another file.
const READ_MAX: u64 = 8 * 1024 * 1024;

/// The real reader. It reads, never writes.
#[allow(dead_code)] // the Split into notes command opens the screen with it (`App::plan_open_active`)
pub fn fs_reader() -> Reader {
    Box::new(|path| {
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_file() || meta.len() > READ_MAX {
            return Some(Found {
                len: u64::MAX,
                hash: 0,
            });
        }
        let bytes = std::fs::read(path).ok()?;
        Some(Found {
            len: bytes.len() as u64,
            hash: fingerprint(&bytes),
        })
    })
}

/// A group the notes go to: its name, its library file and its folder. The file is carried and
/// never looked up by name again: two groups of one name are two files, and the notes and their
/// links must go to the one that was chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dest {
    pub name: String,
    /// The group's library file.
    pub file: PathBuf,
    pub home: PathBuf,
    /// The group does not exist yet: writing makes it.
    pub is_new: bool,
}

impl Dest {
    /// The group `name` in the library at `root`; its folder is named by the library's one rule.
    pub fn new(root: &Path, name: &str, is_new: bool) -> Dest {
        let stem = folder_name(name);
        Dest {
            name: name.trim().to_string(),
            file: root.join(format!("{stem}.md")),
            home: root.join(stem),
            is_new,
        }
    }
}

/// Where one note would be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub path: PathBuf,
    /// The same name is there with the same bytes (3.9).
    pub already: bool,
    /// The name is taken by other bytes, or by an earlier note: the file is called this instead.
    pub renamed: Option<String>,
}

/// What a row has to say in place of its size (5.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mark {
    Already,
    Renamed(String),
    Moved,
    /// What the checks of an AI's answer changed about the note (5.2).
    Fixed(Fix),
}

impl Mark {
    pub fn text(&self) -> String {
        match self {
            Mark::Already => "already there".to_string(),
            Mark::Renamed(name) => format!("will be written as \u{2018}{name}\u{2019}"),
            Mark::Moved | Mark::Fixed(Fix::CodeBlock) => {
                "boundary moved to keep a code block whole".to_string()
            }
            Mark::Fixed(Fix::Table) => "boundary moved to keep a table whole".to_string(),
            Mark::Fixed(Fix::Paragraph) => {
                "boundary moved to a paragraph\u{2019}s edge".to_string()
            }
            Mark::Fixed(Fix::Overlap) => "overlapped the note above: starts after it".to_string(),
        }
    }
}

// ---- the strip ------------------------------------------------------------------------------------------

/// One stretch of the coverage strip: a note, or a gap between notes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    /// The row it belongs to: a note's index, or the rest's (the number of notes) for a gap.
    pub row: usize,
    pub x0: f32,
    pub x1: f32,
    pub note: bool,
    pub ticked: bool,
}

/// The widths of stretches of `lines` lines in `total` pixels: in proportion to their lines, and a
/// stretch that `keep` is at least `min` wide, the rest sharing out what is left. A plan with more
/// such stretches than the room holds gives up the minimum and is a plain proportion.
pub fn widths(lines: &[f32], keep: &[bool], total: f32, min: f32) -> Vec<f32> {
    let n = lines.len();
    let sum: f32 = lines.iter().sum();
    if n == 0 || sum <= 0.0 || total <= 0.0 {
        return vec![0.0; n];
    }
    let mut pinned = vec![false; n];
    loop {
        let fixed = pinned.iter().filter(|&&p| p).count() as f32 * min;
        let free: f32 = lines
            .iter()
            .zip(&pinned)
            .filter(|(_, p)| !**p)
            .map(|(l, _)| *l)
            .sum();
        if fixed >= total || free <= 0.0 {
            break;
        }
        let scale = (total - fixed) / free;
        let mut grew = false;
        for k in 0..n {
            if !pinned[k] && keep.get(k).copied().unwrap_or(false) && lines[k] * scale < min {
                pinned[k] = true;
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    let fixed = pinned.iter().filter(|&&p| p).count() as f32 * min;
    if fixed >= total {
        pinned.iter_mut().for_each(|p| *p = false);
    }
    let fixed = pinned.iter().filter(|&&p| p).count() as f32 * min;
    let free: f32 = lines
        .iter()
        .zip(&pinned)
        .filter(|(_, p)| !**p)
        .map(|(l, _)| *l)
        .sum();
    let scale = if free > 0.0 {
        (total - fixed) / free
    } else {
        0.0
    };
    let mut out: Vec<f32> = lines
        .iter()
        .zip(&pinned)
        .map(|(l, p)| if *p { min } else { l * scale })
        .collect();
    let got: f32 = out.iter().sum();
    if got > 0.0 && (got - total).abs() > 0.001 {
        let k = total / got;
        out.iter_mut().for_each(|w| *w *= k);
    }
    out
}

/// The bands of the strip for `plan` in `width` pixels, in line order: each note, and each gap
/// between them that holds some text. A stretch between two notes of blank lines only is not a
/// gap: it joins the note before it (or the first note), so a plan of many notes is a strip and
/// not a row of hairlines. Edges are whole pixels, so a band takes the columns it owns and the
/// bands tile the strip.
pub fn strip_bands(plan: &Plan, width: f32) -> Vec<Band> {
    type Span = (usize, usize, bool, bool);
    let notes = plan.notes();
    let mut spans: Vec<Span> = Vec::new();
    let mut carry = 0usize;
    let mut gap = |spans: &mut Vec<Span>, from: usize, to: usize| {
        if to <= from {
            return;
        }
        if plan.has_text(from..to) {
            spans.push((notes.len(), to - from, false, false));
        } else if let Some(last) = spans.last_mut() {
            last.1 += to - from;
        } else {
            carry += to - from;
        }
    };
    let mut at = 0usize;
    for (i, n) in notes.iter().enumerate() {
        let r = n.range();
        gap(&mut spans, at, r.start);
        spans.push((i, r.len(), true, n.ticked()));
        at = r.end.max(at);
    }
    gap(&mut spans, at, plan.lines());
    if let Some(first) = spans.first_mut() {
        first.1 += carry;
    }
    let lines: Vec<f32> = spans.iter().map(|s| s.1 as f32).collect();
    // Every note keeps its minimum, ticked or not, so that a tick changes the columns of its
    // own note and no other.
    let keep: Vec<bool> = spans.iter().map(|s| s.2).collect();
    let w = widths(&lines, &keep, width, MIN_BAND);
    let mut out = Vec::with_capacity(spans.len());
    let mut cum = 0.0f32;
    let mut left = 0.0f32;
    for (k, s) in spans.iter().enumerate() {
        cum += w.get(k).copied().unwrap_or(0.0);
        let right = if k + 1 == spans.len() {
            width.round()
        } else {
            cum.round().max(left)
        };
        out.push(Band {
            row: s.0,
            x0: left,
            x1: right,
            note: s.2,
            ticked: s.3,
        });
        left = right;
    }
    out
}

/// Whether the strip draws a separator at every note's left edge: only when each note is wide
/// enough for it to be a line between bands and not most of them.
pub fn separated(bands: &[Band]) -> bool {
    bands
        .iter()
        .filter(|b| b.note)
        .all(|b| b.x1 - b.x0 >= SEPARATE_AT)
}

// ---- where the parts of the card are -----------------------------------------------------------------------

/// The card for a `w` x `h` window.
pub fn card(w: f32, h: f32) -> Rect {
    dialog_card(CARD_W, CARD_H, w, h)
}

/// Where the parts of the card go.
#[derive(Clone, Copy, Debug)]
pub struct Geo {
    pub card: Rect,
    pub header: Rect,
    pub notice: Option<Rect>,
    pub cover: Rect,
    pub strip: Rect,
    pub list_head: Rect,
    pub list: Rect,
    /// The whole right column, when the card is wide enough for it.
    pub preview: Option<Rect>,
    pub preview_head: Rect,
    pub preview_body: Rect,
    /// The options row, when the card has something in it (the Recycle Bin box).
    pub options: Option<Rect>,
}

/// The parts of `card`; `notice` makes room for the line under the header.
pub fn geo(card: Rect, notice: bool) -> Geo {
    geo_with(card, notice, false)
}

/// [`geo`], and `options` makes room for the options row above the footer.
pub fn geo_with(card: Rect, notice: bool, options: bool) -> Geo {
    let pad = DIALOG_PAD;
    let footer = Rect::new(card.x, card.bottom() - DIALOG_FOOTER, card.w, DIALOG_FOOTER);
    let options_h = if options { OPTIONS_ROW_H } else { OPTIONS_H };
    let options = Rect::new(
        card.x + pad,
        footer.y - options_h,
        (card.w - 2.0 * pad).max(0.0),
        options_h,
    );
    let header = Rect::new(card.x, card.y, card.w, HEAD_H);
    let mut y = header.bottom();
    let notice = notice.then(|| {
        let r = Rect::new(card.x, y, card.w, NOTICE_H);
        y += NOTICE_H;
        r
    });
    let cover = Rect::new(card.x, y, card.w, COVER_H);
    y += COVER_H;
    let strip = Rect::new(
        card.x + pad,
        cover.y + 30.0,
        (card.w - 2.0 * pad).max(0.0),
        STRIP_H,
    );
    let body = Rect::new(card.x, y, card.w, (options.y - y).max(0.0));
    let wide = card.w >= PREVIEW_MIN_CARD;
    let list_w = if wide { LIST_W } else { card.w };
    let list_head = Rect::new(body.x, body.y, list_w, LIST_HEAD.min(body.h));
    let list = Rect::new(
        body.x,
        list_head.bottom(),
        list_w,
        (body.h - list_head.h).max(0.0),
    );
    let preview = wide.then(|| Rect::new(card.x + LIST_W, body.y, card.w - LIST_W, body.h));
    let pv = preview.unwrap_or_default();
    let preview_head = Rect::new(pv.x, pv.y, pv.w, PREVIEW_HEAD.min(pv.h));
    let preview_body = Rect::new(
        pv.x,
        preview_head.bottom(),
        pv.w,
        (pv.h - preview_head.h).max(0.0),
    );
    Geo {
        card,
        header,
        notice,
        cover,
        strip,
        list_head,
        list,
        preview,
        preview_head,
        preview_body,
        options: (options.h > 0.0).then_some(options),
    }
}

// ---- state ----------------------------------------------------------------------------------------------

/// What the document is, for the header's first column.
#[derive(Clone, Debug, Default)]
pub struct Source {
    pub name: String,
    pub lines: usize,
    pub bytes: usize,
    /// The file the document was read from. `None` for a plan a demonstration made.
    pub path: Option<PathBuf>,
    /// The file's length and SHA-256 when the plan was made: the notes are cut from that file,
    /// so a plan that is shown again refuses to write when the file has changed since.
    pub len: u64,
    pub sha256: String,
    /// "Windows-1254, written as UTF-8", when the file was not UTF-8.
    pub encoding: Option<String>,
}

/// What opens a plan.
pub struct Init<'a> {
    /// The document, shared: the plan keeps it for as long as it is shown.
    pub doc: Arc<dyn Lines>,
    pub name: &'a str,
    /// The file on disk the notes are made from; `None` for a demonstration.
    pub path: Option<&'a Path>,
    pub headings: &'a [Heading],
    /// The level to cut at; `None` for the shallowest that occurs at least twice.
    pub level: Option<u8>,
    pub encoding: Option<String>,
    pub dest: Dest,
    /// The other groups the notes could go to.
    pub groups: Vec<Dest>,
    /// The library's folder: a new group is made in it.
    pub root: PathBuf,
    pub reader: Reader,
}

/// The two scrolling parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pane {
    List,
    Preview,
}

/// What the pointer can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PHit {
    Card,
    Tick(usize),
    Row(usize),
    Strip(usize),
    TickAll,
    UntickAll,
    Level(usize),
    Dest,
    Cancel,
    Write,
    Show,
    /// A line of the preview, by its place in the lines shown.
    Line(usize),
    Preview,
    Thumb(Pane),
    Track(Pane),
    Field,
    /// A button or a link of the AI (5.2): the header's, or the notice's.
    Ai(AiAct),
    /// The Recycle Bin box and its label (5.4).
    Recycle,
    /// "Keep them in a note called 'The rest'", beside a box that is disabled for want of it.
    KeepRest,
}

/// What an event asks of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Nothing,
    Close,
    Write,
    /// The window asks the AI, stops it, or asks again (5.2). Only an application that offers
    /// the AI ([`PlanScreen::ai_offer`], or a notice it set with links) is ever given one.
    Ai(AiAct),
}

/// What the AI's buttons and links ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AiAct {
    /// "Ask the AI…": the window opens its instruction panel.
    Ask,
    /// Stop the request in flight.
    Stop,
    /// Send the same request again.
    RunAgain,
    /// Back to the cut at headings. The screen does this itself; the window is told.
    AtHeadings,
}

/// A plan that came from a model: what it was asked, and through what, for the header and the
/// record; and what the checks dropped, for the line above the list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AiCut {
    pub instruction: String,
    /// "Claude API · Claude Opus 5.5".
    pub via: String,
    /// The connection and the model, as the record keeps them.
    pub connection: String,
    pub model: String,
    /// What was sent: bytes, and in how many requests.
    pub bytes: u64,
    pub requests: u64,
    /// Only the headings were sent (the outline with the paragraphs was over the limit).
    pub headings_only: bool,
    pub dropped: Dropped,
}

/// What the text field in front of a row or the button is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    Rename(usize),
    Section(usize),
    Group,
}

pub struct Edit {
    pub kind: EditKind,
    pub field: TextField,
    /// A click inside the field, as an x from the left of its text, for the next draw to turn
    /// into a caret (only a painter knows where the letters are).
    pub click: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MAct {
    MoveTo,
    Merge,
    Toggle,
    /// A section of the plan, by its place in `Pop::sections`.
    Section(usize),
    NewSection,
    /// Start a note at this source line (0-based).
    NewNote(usize),
    Group(usize),
    NewGroup,
}

pub struct Pop {
    pub menu: Menu<MAct>,
    /// The row it is about.
    pub row: usize,
    pub sections: Vec<Vec<String>>,
    pub anchor: (f32, f32),
}

#[derive(Clone, Copy, Debug)]
pub struct Drag {
    pane: Pane,
    grab: f32,
}

/// "This drive has no Recycle Bin": the source is not on a local fixed drive (5.4).
pub const RECYCLE_NO_BIN: &str = "This drive has no Recycle Bin";
/// "Only for one document at a time": a plan over several files (5.4).
pub const RECYCLE_ONE_DOCUMENT: &str = "Only for one document at a time";
/// The one-click answer to lines in no note (5.4).
pub const KEEP_THE_REST: &str = "Keep them in a note called \u{2018}The rest\u{2019}";
/// What the options row says beside the box when it is ticked and allowed (5.4).
pub const RECYCLE_TICKED: &str =
    "Every line is in a note, so nothing is lost. It can be restored from the Recycle Bin.";

/// "The Recycle Bin is not available here: {why}": the system refuses (5.4).
pub fn recycle_unavailable(why: &str) -> String {
    format!("The Recycle Bin is not available here: {why}")
}

/// The Recycle Bin box of the options row (5.4): put the original in the bin after the notes are
/// written. The card shows it only when the application puts one here; it holds what the owner
/// asked for and what the application found against it, and the card works out the rest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecycleBox {
    /// The owner ticked it. Kept while the box is disabled, so the strip can say what stands in
    /// the way (5.3: the warning colour, "while the Recycle Bin box is wanted").
    pub wanted: bool,
    /// Why the application will not offer it whatever is ticked, in the card's words: the drive
    /// has no bin, the plan is over several files, the system refuses. `None` when nothing does.
    pub refused: Option<String>,
}

/// Why the Recycle Bin box is disabled (5.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecycleWhy {
    /// The application's reason ([`RecycleBox::refused`]); no click answers it.
    Refused(String),
    /// This many non-blank lines are in no ticked note; ticking the rest answers it.
    Lines(usize),
}

impl RecycleWhy {
    /// The reason as the options row says it, without its link.
    pub fn text(&self) -> String {
        match self {
            RecycleWhy::Refused(why) => why.clone(),
            RecycleWhy::Lines(1) => "1 line is in no note.".to_string(),
            RecycleWhy::Lines(n) => format!("{} lines are in no note.", thousands(*n)),
        }
    }
}

/// The line under the header: what the plan says about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    /// A warning (the warning colour) or a plain notice.
    pub warn: bool,
    /// The note its "Show" link selects.
    pub show: Option<usize>,
    /// Links after it, each an [`Act::Ai`] for the window: "Stop", "Run again".
    pub links: Vec<(AiAct, String)>,
}

pub struct PlanScreen {
    pub plan: Plan,
    headings: Vec<Heading>,
    pub levels: Levels,
    pub source: Source,
    pub dest: Dest,
    pub groups: Vec<Dest>,
    root: PathBuf,
    reader: Reader,
    disk: HashMap<PathBuf, Option<Found>>,
    pub targets: Vec<Target>,
    pub rest: Rest,
    pub totals: Totals,
    pub warnings: Vec<Warning>,
    /// The selected row as it will be written, line by line.
    pub preview: Vec<NoteLine>,
    pub notice: Option<Notice>,
    /// The selected row: a note, or the rest (one past the last note).
    pub sel: usize,
    pub scroll: f32,
    pub pscroll: f32,
    pub edit: Option<Edit>,
    pub menu: Option<Pop>,
    pub hover: Option<PHit>,
    pub pressed: Option<PHit>,
    /// Counts every change of what the screen shows: the frame signature of the overlay.
    pub tick: u64,
    /// Something was ticked, renamed or moved: Escape keeps the plan.
    pub edited: bool,
    /// How many times "Write the notes" was pressed. It writes nothing yet.
    pub write_pressed: u32,
    /// Why nothing can be written, when a library file involved has unsaved changes or the
    /// group's folder cannot be written.
    pub blocked: Option<String>,
    /// The destination `blocked` was worked out for: the window works it out again when the
    /// group the notes go to changes (C2).
    pub blocked_for: Option<Dest>,
    pub win: (f32, f32),
    pub scale: f32,
    pub spots: Vec<(PHit, Rect)>,
    pub bands: Vec<Band>,
    pub bands_w: f32,
    drag: Option<Drag>,
    /// The row that was selected when the press began: a second click on it renames.
    press_sel: usize,
    /// The rename field's mouse editing and edit menu, on the
    /// [`pointer_press`](PlanScreen::pointer_press) route; the old [`press`](PlanScreen::press)
    /// route never reaches it.
    input: FieldInput,
    /// Where the rename field was last drawn, while one is open.
    field_at: Option<FieldArea>,
    /// The header offers "Ask the AI…" (5.2). Off unless the application turns it on, so the
    /// card of an application with no AI draws as it always did.
    pub ai_offer: bool,
    /// The plan came from a model; `None` for a cut at headings.
    pub ai_cut: Option<AiCut>,
    /// A line the application puts above the list, before any warning: a request in flight, an
    /// answer that could not be read, a refusal.
    pub ai_notice: Option<Notice>,
    /// The Recycle Bin box (5.4). `None` unless the application puts one here, so the card of an
    /// application that does not offer it draws as it always did, with no options row.
    pub recycle: Option<RecycleBox>,
}

/// How many names are tried for a note before the first is used anyway.
const NAME_TRIES: usize = 10_000;

fn lookup(
    disk: &mut HashMap<PathBuf, Option<Found>>,
    reader: &Reader,
    path: &Path,
) -> Option<Found> {
    if let Some(found) = disk.get(path) {
        return *found;
    }
    let found = reader(path);
    disk.insert(path.to_path_buf(), found);
    found
}

impl PlanScreen {
    pub fn new(init: Init) -> PlanScreen {
        let level = init.level.unwrap_or_else(|| {
            split::levels(init.headings)
                .default_level()
                .unwrap_or(SPLIT_LEVELS[1])
        });
        let plan = split::plan(init.doc, init.name, init.headings, level);
        let mut s = PlanScreen {
            levels: split::levels(init.headings),
            headings: init.headings.to_vec(),
            source: Source {
                name: init.name.to_string(),
                lines: plan.lines(),
                bytes: plan.document_bytes(),
                path: init.path.map(Path::to_path_buf),
                len: 0,
                sha256: String::new(),
                encoding: init.encoding,
            },
            plan,
            dest: init.dest,
            groups: init.groups,
            root: init.root,
            reader: init.reader,
            disk: HashMap::new(),
            targets: Vec::new(),
            rest: Rest::default(),
            totals: Totals::default(),
            warnings: Vec::new(),
            preview: Vec::new(),
            notice: None,
            sel: 0,
            scroll: 0.0,
            pscroll: 0.0,
            edit: None,
            menu: None,
            hover: None,
            pressed: None,
            tick: 0,
            edited: false,
            write_pressed: 0,
            blocked: None,
            blocked_for: None,
            win: (1280.0, 800.0),
            scale: 1.0,
            spots: Vec::new(),
            bands: Vec::new(),
            bands_w: -1.0,
            drag: None,
            press_sel: 0,
            input: FieldInput::new(),
            field_at: None,
            ai_offer: false,
            ai_cut: None,
            ai_notice: None,
            recycle: None,
        };
        s.untick_what_is_there();
        s.refresh();
        s
    }

    /// The notes that are already in the group's folder, with the same bytes, come unticked (3.9).
    fn untick_what_is_there(&mut self) {
        self.retarget();
        let there: Vec<usize> = self
            .targets
            .iter()
            .enumerate()
            .filter(|(_, t)| t.already)
            .map(|(i, _)| i)
            .collect();
        for i in there {
            self.plan.set_ticked(i, false);
        }
    }

    /// Takes in what a change of the plan changed: the counts, the warnings, the files, the
    /// preview and the strip.
    fn refresh(&mut self) {
        self.rest = self.plan.rest();
        self.totals = self.plan.totals();
        self.warnings = self.plan.warnings();
        self.retarget();
        self.sel = self.sel.min(self.plan.notes().len());
        self.notice = self.make_notice();
        self.bands_w = -1.0;
        self.fill_preview();
        self.tick += 1;
    }

    /// A change the owner made: the plan is worth keeping when Escape closes the screen.
    fn changed(&mut self) {
        self.edited = true;
        self.refresh();
    }

    fn fill_preview(&mut self) {
        self.preview = if self.sel < self.plan.notes().len() {
            self.plan.note_lines(self.sel)
        } else {
            self.plan.rest_lines()
        };
        let max = (self.preview.len() as f32 * LINE_H).max(0.0);
        self.pscroll = self.pscroll.clamp(0.0, max);
    }

    /// The file each note would be written as, and which of them are there already.
    pub fn retarget(&mut self) {
        // A list of paths, compared the way the disk compares them: `Notes.md` and `notes.md`
        // are one file here, and the second note must not be given the first one's name (C5).
        let mut taken: Vec<PathBuf> = Vec::new();
        let mut next: HashMap<PathBuf, usize> = HashMap::new();
        let mut out = Vec::with_capacity(self.plan.notes().len());
        for i in 0..self.plan.notes().len() {
            let Some(n) = self.plan.note(i) else { break };
            let folder = note_folder(&self.dest.home, n.sections());
            let stem = folder_name(&split::note_slug(n.title()));
            let ticked = n.ticked();
            let first = folder.join(format!("{stem}.md"));
            let start = next.get(&first).copied().filter(|&k| k > 8).unwrap_or(1);
            let mut text: Option<(u64, usize)> = None;
            let mut chosen: Option<(PathBuf, String, usize, bool)> = None;
            for k in start..=NAME_TRIES {
                let name = if k == 1 {
                    format!("{stem}.md")
                } else {
                    format!("{stem} {k}.md")
                };
                let path = folder.join(&name);
                if taken
                    .iter()
                    .any(|p| mdshared_app::paths::same_path(p, &path))
                {
                    continue;
                }
                match lookup(&mut self.disk, &self.reader, &path) {
                    None => {
                        chosen = Some((path, name, k, false));
                        break;
                    }
                    Some(found) => {
                        let (hash, len) = *text.get_or_insert_with(|| {
                            let t = self.plan.note_text(i);
                            (fingerprint(t.as_bytes()), t.len())
                        });
                        if found.len == len as u64 && found.hash == hash {
                            chosen = Some((path, name, k, true));
                            break;
                        }
                    }
                }
            }
            let (path, name, k, already) =
                chosen.unwrap_or_else(|| (first.clone(), format!("{stem}.md"), 1, false));
            next.insert(first, k + 1);
            if ticked {
                taken.push(path.clone());
            }
            out.push(Target {
                path,
                already,
                renamed: (k > 1 && !already).then_some(name),
            });
        }
        self.targets = out;
    }

    fn make_notice(&self) -> Option<Notice> {
        let notes = self.plan.notes();
        if let Some(n) = &self.ai_notice {
            return Some(n.clone());
        }
        if let Some(text) = self.ai_cut.as_ref().and_then(|c| c.dropped.text()) {
            let more = self.warnings.len();
            return Some(Notice {
                text: if more > 0 {
                    format!("{text} (and {more} more)")
                } else {
                    text
                },
                warn: true,
                show: None,
                links: Vec::new(),
            });
        }
        if let Some(w) = self.warnings.first() {
            let (text, show) = match w {
                Warning::NoSplit { notes: 0 } => (
                    "No note was cut: the document has no heading at this level.".to_string(),
                    None,
                ),
                Warning::NoSplit { .. } => {
                    ("Only one note: the document was not split.".into(), None)
                }
                Warning::OneNoteHoldsMost { note, lines, total } => (
                    format!(
                        "One note holds {} of the document's {} lines.",
                        thousands(*lines),
                        thousands(*total)
                    ),
                    Some(*note),
                ),
                Warning::ManyShortNotes { notes: n } => (
                    format!("{} notes are only one or two lines long.", n.len()),
                    n.first().copied(),
                ),
                Warning::NotAtAHeading { notes: n } => (
                    if n.len() == 1 {
                        "1 note does not start at a heading, as the others do.".to_string()
                    } else {
                        format!(
                            "{} notes do not start at a heading, as the others do.",
                            n.len()
                        )
                    },
                    n.first().copied(),
                ),
                Warning::SameTitle { title, notes: n } => (
                    format!(
                        "{} notes are called \u{201C}{title}\u{201D}: the later files get a number.",
                        n.len()
                    ),
                    n.first().copied(),
                ),
            };
            let more = self.warnings.len().saturating_sub(1);
            let text = if more > 0 {
                format!("{text} (and {more} more)")
            } else {
                text
            };
            return Some(Notice {
                text,
                warn: true,
                show,
                links: Vec::new(),
            });
        }
        let there = self.targets.iter().filter(|t| t.already).count();
        if there > 0 && !notes.is_empty() {
            let all = there == notes.len();
            let text = if all {
                format!(
                    "All {} notes are already in this folder, with the same text. They are unticked.",
                    notes.len()
                )
            } else {
                format!(
                    "{there} of the {} notes are already in this folder, with the same text. They are unticked.",
                    notes.len()
                )
            };
            return Some(Notice {
                text,
                warn: false,
                show: self.targets.iter().position(|t| t.already),
                links: Vec::new(),
            });
        }
        None
    }

    // ---- what the rows say ----

    /// How many rows the list has: the notes and the rest.
    pub fn row_count(&self) -> usize {
        self.plan.notes().len() + 1
    }

    /// Whether `row` is the rest.
    pub fn is_rest(&self, row: usize) -> bool {
        row >= self.plan.notes().len()
    }

    pub fn ticked(&self, row: usize) -> bool {
        match self.plan.note(row) {
            Some(n) => n.ticked(),
            None => self.plan.rest_ticked(),
        }
    }

    /// What a note's row says in place of its size, if anything.
    pub fn mark(&self, i: usize) -> Option<Mark> {
        let t = self.targets.get(i)?;
        if t.already {
            return Some(Mark::Already);
        }
        if let Some(name) = &t.renamed {
            return Some(Mark::Renamed(name.clone()));
        }
        if let Some(fix) = self.plan.note(i).and_then(|n| n.fix()) {
            return Some(Mark::Fixed(fix));
        }
        self.plan
            .note(i)
            .filter(|n| n.boundary_moved())
            .map(|_| Mark::Moved)
    }

    /// The notes that have a mark on them, for F7.
    pub fn marked(&self) -> Vec<usize> {
        (0..self.plan.notes().len())
            .filter(|&i| self.mark(i).is_some())
            .collect()
    }

    /// `library\Japan 2025\Tokyo\Asakusa.md`, as the preview shows it: the library folder's name
    /// and then the path inside it. A note outside the library (a group whose folder is
    /// elsewhere, 5.1) shows its own path: it is where the note is written, and `library\` in
    /// front of `D:\notes\work` would name a path that is nowhere.
    fn written_path(&self, path: &Path) -> String {
        let sep = std::path::MAIN_SEPARATOR;
        let Ok(rel) = path.strip_prefix(&self.root) else {
            return path.display().to_string();
        };
        let mut s = library::DIR.to_string();
        for part in rel.components() {
            s.push(sep);
            s.push_str(&part.as_os_str().to_string_lossy());
        }
        s
    }

    /// `library\Japan 2025\Tokyo\Asakusa.md`, as the preview shows it.
    pub fn target_text(&self, i: usize) -> String {
        match self.targets.get(i) {
            Some(t) => self.written_path(&t.path),
            None => String::new(),
        }
    }

    /// `library\Japan 2025\`: the folder of the group.
    pub fn dest_text(&self) -> String {
        let mut s = self.written_path(&self.dest.home);
        s.push(std::path::MAIN_SEPARATOR);
        s
    }

    /// The library folder the plan writes into.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Why "Write the notes" is disabled, or `None` when it can be pressed.
    pub fn problem(&self) -> Option<String> {
        if self.totals.notes == 0 {
            return Some("Nothing is ticked.".to_string());
        }
        self.blocked.clone()
    }

    // ---- the Recycle Bin box (5.4) ----

    /// Why the box is disabled, or `None` when it can be ticked: the application's reason
    /// first, since no click answers it; then the lines in no ticked note.
    pub fn recycle_why(&self) -> Option<RecycleWhy> {
        let b = self.recycle.as_ref()?;
        if let Some(why) = &b.refused {
            return Some(RecycleWhy::Refused(why.clone()));
        }
        (!self.totals.covered()).then_some(RecycleWhy::Lines(self.totals.lines_in_no_note))
    }

    /// Whether the write puts the original in the Recycle Bin: the box is there, ticked, and
    /// nothing stands against it now.
    pub fn recycle_on(&self) -> bool {
        self.recycle.as_ref().is_some_and(|b| b.wanted) && self.recycle_why().is_none()
    }

    /// The box is ticked and something stands against it: the gaps are drawn in the warning
    /// colour (5.3), and so is the reason.
    pub fn recycle_blocked(&self) -> bool {
        self.recycle.as_ref().is_some_and(|b| b.wanted) && self.recycle_why().is_some()
    }

    /// A click on the box: ticks or unticks it, only while it can be ticked.
    pub fn toggle_recycle(&mut self) {
        if self.recycle_why().is_some() {
            return;
        }
        if let Some(b) = self.recycle.as_mut() {
            b.wanted = !b.wanted;
            self.tick += 1;
        }
    }

    /// "Keep them in a note called 'The rest'": ticks the rest, so every line is in a note.
    pub fn keep_the_rest(&mut self) {
        if !self.plan.rest_ticked() {
            self.plan.set_rest_ticked(true);
            self.changed();
        }
    }

    /// The footer's sentence: what pressing the button does, in one sentence (5.3, 5.4).
    pub fn footer_sentence(&self) -> String {
        if self.recycle_on() {
            format!(
                "Writes one .md per ticked note into {}, then puts {} in the Recycle Bin.",
                self.dest_text(),
                self.source.name
            )
        } else {
            format!(
                "Writes one .md per ticked note into {}. The source document is not changed.",
                self.dest_text()
            )
        }
    }

    // ---- the plan, changed ----

    pub fn toggle(&mut self, row: usize) {
        if self.is_rest(row) {
            self.plan.set_rest_ticked(!self.plan.rest_ticked());
        } else if let Some(n) = self.plan.note(row) {
            let on = !n.ticked();
            self.plan.set_ticked(row, on);
        }
        self.changed();
    }

    pub fn tick_all(&mut self) {
        self.plan.tick_all();
        self.changed();
    }

    pub fn untick_all(&mut self) {
        self.plan.untick_all();
        self.changed();
    }

    pub fn rename(&mut self, i: usize, title: &str) {
        self.plan.rename(i, title);
        self.changed();
    }

    pub fn move_to(&mut self, i: usize, sections: &[&str]) {
        self.plan.move_to_section(i, sections);
        self.changed();
    }

    pub fn merge(&mut self, i: usize) {
        if let Some(k) = self.plan.merge_with_above(i) {
            self.sel = k;
            self.changed();
            self.reveal();
        }
    }

    /// Starts a note at the 0-based source `line`, and selects it.
    pub fn new_note(&mut self, line: usize) {
        if let Some(i) = self.plan.new_note_at(line) {
            self.sel = i;
            self.pscroll = 0.0;
            self.changed();
            self.reveal();
        }
    }

    /// Cuts the document at another level: the notes are made again, and what was changed in
    /// the old ones is gone.
    pub fn set_level(&mut self, level: u8) {
        if level == self.plan.level() && self.ai_cut.is_none() {
            return;
        }
        self.ai_cut = None;
        self.plan = split::plan(
            self.plan.document().clone(),
            &self.source.name,
            &self.headings,
            level,
        );
        self.sel = 0;
        self.scroll = 0.0;
        self.pscroll = 0.0;
        self.untick_what_is_there();
        self.edited = true;
        self.refresh();
    }

    /// The document's headings, as the plan was opened with them.
    pub fn headings(&self) -> &[Heading] {
        &self.headings
    }

    /// Shows the plan a model's answer made (`split::answer::plan`) in place of the one shown:
    /// the same document, the same group. Notes already in the group's folder come unticked, as
    /// they do for a cut at headings, and the plan counts as changed, so Escape keeps it.
    pub fn set_ai_plan(&mut self, plan: Plan, cut: AiCut) {
        self.plan = plan;
        self.ai_cut = Some(cut);
        self.ai_notice = None;
        self.edit = None;
        self.menu = None;
        self.sel = 0;
        self.scroll = 0.0;
        self.pscroll = 0.0;
        self.untick_what_is_there();
        self.edited = true;
        self.refresh();
    }

    /// Sets the line the application puts above the list, or takes it away.
    pub fn set_ai_notice(&mut self, notice: Option<Notice>) {
        self.ai_notice = notice;
        self.notice = self.make_notice();
        self.tick += 1;
    }

    /// Sends the notes to another group.
    pub fn set_dest(&mut self, dest: Dest) {
        self.dest = dest;
        self.disk.clear();
        self.changed();
    }

    // ---- the list ----

    pub fn geo(&self) -> Geo {
        geo_with(
            card(self.win.0, self.win.1),
            self.notice.is_some(),
            self.recycle.is_some(),
        )
    }

    pub fn rows(&self) -> Rows {
        let g = self.geo();
        Rows {
            row_h: ROW_H,
            view: g.list.h,
            count: self.row_count(),
            scroll: self.scroll,
        }
    }

    pub fn prows(&self) -> Rows {
        let g = self.geo();
        Rows {
            row_h: LINE_H,
            view: (g.preview_body.h - 8.0).max(0.0),
            count: self.preview.len(),
            scroll: self.pscroll,
        }
    }

    /// Scrolls the list so the selected row shows.
    pub fn reveal(&mut self) {
        let rows = self.rows();
        self.scroll = rows.reveal(self.sel);
    }

    pub fn select(&mut self, row: usize) {
        let row = row.min(self.plan.notes().len());
        if row != self.sel {
            self.sel = row;
            self.pscroll = 0.0;
            self.fill_preview();
        }
        self.reveal();
        self.tick += 1;
    }

    /// The row a strip pixel column belongs to.
    pub fn band_row(&self, px: f32) -> Option<usize> {
        self.bands
            .iter()
            .find(|b| px >= b.x0 && px < b.x1)
            .or_else(|| self.bands.last().filter(|b| px >= b.x1 && px <= b.x1 + 0.5))
            .map(|b| b.row)
    }

    pub fn bands_for(&mut self, width: f32) {
        if (self.bands_w - width).abs() > 0.01 {
            self.bands = strip_bands(&self.plan, width);
            self.bands_w = width;
        }
    }

    // ---- events ----

    /// The window's size changed or the screen opens: where things are comes from it.
    pub fn resize(&mut self, w: f32, h: f32) {
        if self.win != (w, h) {
            self.win = (w, h);
            self.reveal();
            self.tick += 1;
        }
    }

    pub fn hit_at(&self, x: f32, y: f32) -> Option<PHit> {
        let g = self.geo();
        if !g.card.contains(x, y) {
            return None;
        }
        if let Some((hit, _)) = self.spots.iter().rev().find(|(_, r)| r.contains(x, y)) {
            return Some(*hit);
        }
        if g.strip.contains(x, y) {
            let px = (x - g.strip.x) * self.scale;
            return self.band_row(px).map(PHit::Strip);
        }
        if g.list.contains(x, y) {
            let rows = self.rows();
            return Some(match rows.row_at(g.list, x, y) {
                Some(i) => {
                    let row = rows.rect(g.list, i);
                    if x < row.x + 40.0 {
                        PHit::Tick(i)
                    } else {
                        PHit::Row(i)
                    }
                }
                None => PHit::Card,
            });
        }
        if g.preview_body.contains(x, y) && g.preview.is_some() {
            let rows = self.prows();
            let area = Rect::new(
                g.preview_body.x,
                g.preview_body.y + 4.0,
                g.preview_body.w,
                rows.view,
            );
            return Some(match rows.row_at(area, x, y) {
                Some(k) => PHit::Line(k),
                None => PHit::Preview,
            });
        }
        Some(PHit::Card)
    }

    /// The pointer moved: a thumb in hand follows it, an open menu lights its rows, and the
    /// hover follows it.
    ///
    /// Deprecated with [`press`](Self::press): [`pointer_motion`](Self::pointer_motion) is
    /// this with the rename field's drag.
    #[deprecated(
        note = "an open rename field takes only a click here (at the next draw): use `pointer_press`, `pointer_motion`, `pointer_release`, `secondary_press` and `key_with_menu`, which give it the drag, the double and triple click, edge scrolling and the edit menu"
    )]
    pub fn pointer(&mut self, x: f32, y: f32) {
        if let Some(d) = self.drag {
            self.drag_to(d, y);
            return;
        }
        if let Some(pop) = self.menu.as_mut() {
            if pop.menu.pointer(x, y) {
                self.tick += 1;
            }
            return;
        }
        let hit = self.hit_at(x, y).filter(|h| *h != PHit::Card);
        if hit != self.hover {
            self.hover = hit;
            self.tick += 1;
        }
    }

    fn drag_to(&mut self, d: Drag, y: f32) {
        let g = self.geo();
        let (area, rows) = match d.pane {
            Pane::List => (g.list, self.rows()),
            Pane::Preview => (
                Rect::new(
                    g.preview_body.x,
                    g.preview_body.y + 4.0,
                    g.preview_body.w,
                    g.preview_body.h - 8.0,
                ),
                self.prows(),
            ),
        };
        if let Some((track, thumb)) = rows.thumb(area) {
            let off = offset_for_thumb(track, thumb.h, y - d.grab, rows.content(), rows.view);
            match d.pane {
                Pane::List => self.scroll = rows.clamp_scroll(off),
                Pane::Preview => self.pscroll = rows.clamp_scroll(off),
            }
            self.tick += 1;
        }
    }

    /// A press: an open menu chooses or closes, a press outside an open rename field takes
    /// the name as it stands, a thumb is grabbed, a track pages, and a control is pressed
    /// (it acts on the [`release`](Self::release) over it).
    ///
    /// Deprecated: a press in an open rename field only places the caret here, at the next
    /// draw (no drag, no double or triple click, no edge scrolling, no edit menu).
    /// [`pointer_press`](Self::pointer_press) is this with all of it. The plan's applications
    /// move when their pin passes this, or allow the warning to keep this behaviour (roadmap
    /// L15).
    #[deprecated(
        note = "an open rename field takes only a click here (at the next draw): use `pointer_press`, `pointer_motion`, `pointer_release`, `secondary_press` and `key_with_menu`, which give it the drag, the double and triple click, edge scrolling and the edit menu"
    )]
    pub fn press(&mut self, x: f32, y: f32) {
        self.tick += 1;
        if let Some(pop) = self.menu.as_ref() {
            if pop.menu.contains(x, y) {
                self.pressed = Some(PHit::Card);
            } else {
                self.menu = None;
            }
            return;
        }
        let hit = self.hit_at(x, y);
        if let Some(e) = self.edit.as_mut() {
            if hit == Some(PHit::Field) {
                e.click = Some(x);
                return;
            }
            self.commit_edit();
        }
        self.press_sel = self.sel;
        self.pressed = hit;
        match hit {
            Some(PHit::Thumb(pane)) => {
                let g = self.geo();
                let (area, rows) = self.pane_area(pane, &g);
                if let Some((_, thumb)) = rows.thumb(area) {
                    self.drag = Some(Drag {
                        pane,
                        grab: y - thumb.y,
                    });
                }
            }
            Some(PHit::Track(pane)) => {
                let g = self.geo();
                let (area, rows) = self.pane_area(pane, &g);
                if let Some((_, thumb)) = rows.thumb(area) {
                    let page = if y < thumb.y { -rows.view } else { rows.view };
                    let to = rows.clamp_scroll(rows.scroll + page);
                    match pane {
                        Pane::List => self.scroll = to,
                        Pane::Preview => self.pscroll = to,
                    }
                }
            }
            _ => {}
        }
    }

    fn pane_area(&self, pane: Pane, g: &Geo) -> (Rect, Rows) {
        match pane {
            Pane::List => (g.list, self.rows()),
            Pane::Preview => (
                Rect::new(
                    g.preview_body.x,
                    g.preview_body.y + 4.0,
                    g.preview_body.w,
                    (g.preview_body.h - 8.0).max(0.0),
                ),
                self.prows(),
            ),
        }
    }

    /// The button went up: the control the press began on acts when the pointer is still on
    /// it. A drag of the rename field begun on the [`pointer_press`](Self::pointer_press)
    /// route ends here too (without the last stretch to `(x, y)`:
    /// [`pointer_release`](Self::pointer_release) has that).
    pub fn release(&mut self, x: f32, y: f32) -> Act {
        self.tick += 1;
        self.drag = None;
        if self.input.is_dragging() {
            self.input.cancel();
        }
        let Some(pressed) = self.pressed.take() else {
            return Act::Nothing;
        };
        if let Some(pop) = self.menu.as_mut() {
            pop.menu.pointer(x, y);
            if pressed == PHit::Card
                && let Some(act) = pop.menu.chosen()
            {
                self.choose(act);
            }
            return Act::Nothing;
        }
        if self.hit_at(x, y) != Some(pressed) {
            return Act::Nothing;
        }
        match pressed {
            PHit::Tick(i) => {
                self.select(i);
                self.toggle(i);
            }
            PHit::Row(i) => {
                if i == self.press_sel && !self.is_rest(i) && self.edit.is_none() {
                    self.start_rename();
                } else {
                    self.select(i);
                }
            }
            PHit::Strip(row) => self.select(row),
            PHit::TickAll => self.tick_all(),
            PHit::UntickAll => self.untick_all(),
            PHit::Level(k) => {
                if let Some(&l) = SPLIT_LEVELS.get(k) {
                    self.set_level(l);
                }
            }
            PHit::Dest => self.open_dest_menu(),
            PHit::Show => {
                if let Some(i) = self.notice.as_ref().and_then(|n| n.show) {
                    self.select(i);
                }
            }
            PHit::Cancel => return Act::Close,
            PHit::Write => return self.write(),
            PHit::Recycle => self.toggle_recycle(),
            PHit::KeepRest => self.keep_the_rest(),
            PHit::Ai(AiAct::AtHeadings) => {
                // The level the cut at headings had: an AI plan keeps it (`answer::plan`).
                let level = self.plan.level();
                self.set_level(level);
                return Act::Ai(AiAct::AtHeadings);
            }
            PHit::Ai(act) => return Act::Ai(act),
            _ => {}
        }
        Act::Nothing
    }

    /// Drops a press under way, for when the window lost the pointer (`Focus(false)`, or the
    /// host's `Event::PressLost`), where a [`release`](Self::release) would act on what is
    /// under the pointer: a pressed tick box, row, button or menu item does nothing, and a
    /// scrollbar's thumb is let go with the scroll where the drag left it. The selection, the
    /// plan, an open menu and a rename under way stay as they are. True when there was a press
    /// (the screen's `tick` moves then, so the overlay draws again without its pressed look).
    /// On the [`pointer_press`](Self::pointer_press) route it also ends a drag in the rename
    /// field and closes the field's edit menu.
    pub fn cancel(&mut self) -> bool {
        let thumb = self.drag.take().is_some();
        let control = self.pressed.take().is_some();
        let field = self.input.is_dragging();
        let menu = self.input.cancel();
        let held = thumb || control || field || menu;
        if held {
            self.tick += 1;
        }
        held
    }

    // ---- the pointer route, with all of the rename field's mouse editing --------------------
    //
    // `pointer_press`, `secondary_press`, `pointer_motion`, `pointer_release`, `key_with_menu`,
    // `text_deadline`, `tick_text` and `cursor_at` together: an open rename field (a note's, a
    // section's, the new group's) gets a field's whole mouse editing (`textfield::FieldInput`),
    // and the rest of the screen answers as `press`, `pointer`, `release` and `key` do.

    /// Moves `tick` when the rename field's input changed what it draws; true then.
    fn field_moved(&mut self, o: FieldOutcome) -> bool {
        let moved = o.redraw();
        if moved {
            self.tick += 1;
        }
        moved
    }

    /// A primary (left) press at `at`, on the route that gives an open rename field all of a
    /// field's mouse editing: a click places the caret where the shaped text puts it (at once,
    /// not at the next draw), Shift extends the selection, a double click selects a word and a
    /// triple click everything, a drag selects and scrolls the text past the field's ends
    /// ([`text_deadline`](Self::text_deadline)), and a press anywhere in the field's frame
    /// counts. `now` is the event's time and `gfx` the one the screen is drawn with.
    ///
    /// With the field's edit menu open, the press goes to the menu (choosing an item, or
    /// closing the menu when outside it) and no further. Everywhere else it is
    /// [`press`](Self::press): a press outside the field takes the name as it stands, a row is
    /// selected, a button is pressed.
    ///
    /// Use it with [`pointer_motion`](Self::pointer_motion),
    /// [`pointer_release`](Self::pointer_release), [`secondary_press`](Self::secondary_press)
    /// and [`key_with_menu`](Self::key_with_menu) in place of `press`, `pointer`, `release`
    /// and `key`; wait for [`text_deadline`](Self::text_deadline) and call
    /// [`tick_text`](Self::tick_text); call [`cancel`](Self::cancel) when the window loses the
    /// keyboard. [`draw`](Self::draw) draws the edit menu over everything.
    // The rest of the screen is still `press`'s; its click-only field part never runs from
    // here, since a press in the field's frame is the input's.
    #[allow(deprecated)]
    pub fn pointer_press(&mut self, at: (f32, f32), mods: Mods, now: Instant, gfx: &mut Gfx) {
        if let (Some(e), Some(area)) = (self.edit.as_mut(), self.field_at) {
            let menu = self.input.menu_open();
            let o = self.input.press(&mut e.field, gfx, area, at, mods, now);
            if menu || o.taken() {
                self.tick += 1;
                return;
            }
        } else {
            self.input.cancel();
        }
        self.press(at.0, at.1);
    }

    /// A secondary (right) press at `at`: on an open rename field it ends a drag, moves the
    /// caret to the press when nothing is selected, and opens the field's edit menu there
    /// (Cut, Copy, Paste, Select all, in the words of [`set_edit_labels`](Self::set_edit_labels)).
    /// An edit menu open before is closed first, and the press goes no further. True when it
    /// was taken; false leaves it to the application (a row's menu, say).
    pub fn secondary_press(&mut self, at: (f32, f32), now: Instant, gfx: &mut Gfx) -> bool {
        let (Some(e), Some(area)) = (self.edit.as_mut(), self.field_at) else {
            return false;
        };
        let o = self.input.secondary_press(&mut e.field, gfx, area, at, now);
        if o.taken() {
            self.tick += 1;
        }
        o.taken()
    }

    /// The pointer moved to `at`, on the [`pointer_press`](Self::pointer_press) route: an open
    /// edit menu lights its rows, a press held in the rename field extends the selection
    /// (wherever the pointer is), and otherwise it is [`pointer`](Self::pointer).
    #[allow(deprecated)]
    pub fn pointer_motion(&mut self, at: (f32, f32), now: Instant, gfx: &mut Gfx) {
        if let (Some(e), Some(area)) = (self.edit.as_mut(), self.field_at) {
            let o = self.input.pointer_move(&mut e.field, gfx, area, at, now);
            if o.taken() {
                self.field_moved(o);
                return;
            }
        }
        self.pointer(at.0, at.1);
    }

    /// The primary button went up at `at`, on the [`pointer_press`](Self::pointer_press)
    /// route: a press held in the rename field ends there (the run of clicks goes on), an open
    /// edit menu takes it, and otherwise it is [`release`](Self::release).
    pub fn pointer_release(&mut self, at: (f32, f32), now: Instant, gfx: &mut Gfx) -> Act {
        if let (Some(e), Some(area)) = (self.edit.as_mut(), self.field_at) {
            let o = self.input.release(&mut e.field, gfx, area, at, now);
            if o.taken() {
                self.field_moved(o);
                return Act::Nothing;
            }
        }
        self.release(at.0, at.1)
    }

    /// A key, on the [`pointer_press`](Self::pointer_press) route: with a rename field open,
    /// its edit menu takes every key first (Up, Down, Enter, Escape, an access key; Escape
    /// closes the menu and leaves the rename open), and Shift+F10 or the Menu key
    /// (`menu_key`) opens the menu at the caret. Every other key is [`key`](Self::key).
    pub fn key_with_menu(
        &mut self,
        name: Option<&KeyName>,
        menu_key: bool,
        mods: Mods,
        typed: Option<&str>,
        gfx: &mut Gfx,
    ) -> Act {
        let shift_f10 =
            matches!(name, Some(KeyName::F(10))) && mods.shift && !mods.ctrl && !mods.alt;
        if let (Some(e), Some(area)) = (self.edit.as_mut(), self.field_at)
            && (self.input.menu_open() || shift_f10 || menu_key)
        {
            let open = Mods {
                shift: true,
                ..Mods::default()
            };
            if !self.input.menu_open() {
                // Shift+F10 or the Menu key: open it at the caret.
                self.input
                    .key(&mut e.field, gfx, area, &KeyName::F(10), open, None);
            } else if let Some(n) = name {
                self.input.key(&mut e.field, gfx, area, n, mods, typed);
            }
            // A key with no name while the menu is open: the menu drops it, as it drops text.
            self.tick += 1;
            return Act::Nothing;
        }
        self.key(name, menu_key, mods, typed)
    }

    /// When [`tick_text`](Self::tick_text) is due: the next step of a drag held past the
    /// rename field's end, or the edit menu's submenu delay. `None` while nothing waits.
    pub fn text_deadline(&self) -> Option<Instant> {
        self.input.deadline()
    }

    /// Does what was due at `now` (see [`text_deadline`](Self::text_deadline)). True when
    /// something moved (and `tick` with it); early or repeated calls do nothing.
    pub fn tick_text(&mut self, now: Instant, gfx: &mut Gfx) -> bool {
        let (Some(e), Some(area)) = (self.edit.as_mut(), self.field_at) else {
            return false;
        };
        let o = self.input.tick(&mut e.field, gfx, area, now);
        self.field_moved(o)
    }

    /// The cursor to show at `at` on the [`pointer_press`](Self::pointer_press) route: the
    /// I-beam over an open rename field and all through its drag, the arrow over an open edit
    /// menu, and `None` (the application's choice) elsewhere.
    pub fn cursor_at(&self, at: (f32, f32)) -> Option<Cursor> {
        let area = self.field_at?;
        self.edit.as_ref()?;
        self.input.cursor_at(area, at)
    }

    /// Words the rename field's edit menu by `labels` from the next time it opens (English
    /// until then).
    pub fn set_edit_labels(&mut self, labels: EditLabels) {
        self.input.set_labels(labels);
    }

    /// Whether the rename field's edit menu is open.
    pub fn edit_menu_open(&self) -> bool {
        self.input.menu_open()
    }

    /// "Write the notes": records that it was pressed, and nothing else. The writer is the next
    /// job.
    fn write(&mut self) -> Act {
        if self.problem().is_some() {
            return Act::Nothing;
        }
        self.write_pressed += 1;
        self.tick += 1;
        Act::Write
    }

    pub fn wheel(&mut self, x: f32, y: f32, dy: f32) {
        self.tick += 1;
        if let Some(pop) = self.menu.as_mut() {
            pop.menu.scroll_by(dy);
            return;
        }
        let g = self.geo();
        if g.preview.is_some_and(|r| r.contains(x, y)) {
            let rows = self.prows();
            self.pscroll = rows.clamp_scroll(self.pscroll + dy);
        } else {
            let rows = self.rows();
            self.scroll = rows.clamp_scroll(self.scroll + dy);
            // A name being typed stays in view: the list scrolls only as far as keeps its row
            // whole (audit 4, N5).
            if let Some(EditKind::Rename(k) | EditKind::Section(k)) =
                self.edit.as_ref().map(|e| e.kind)
            {
                self.scroll = self.rows().reveal(k);
            }
        }
    }

    /// A right click: the row's menu, or the preview line's. True when it was taken.
    pub fn right_press(&mut self, x: f32, y: f32) -> bool {
        self.tick += 1;
        if self.edit.is_some() {
            self.commit_edit();
        }
        match self.hit_at(x, y) {
            Some(PHit::Row(i) | PHit::Tick(i)) => {
                self.select(i);
                self.open_row_menu(i, (x, y));
                true
            }
            Some(PHit::Line(k)) => {
                let line = self
                    .preview
                    .get(k)
                    .and_then(|l| l.source)
                    .map(|s| s.saturating_sub(1));
                if let Some(line) = line {
                    let items = vec![Item::new(MAct::NewNote(line), "Start a new note here")];
                    self.menu = Some(Pop {
                        menu: Menu::new(items, x, y, false),
                        row: self.sel,
                        sections: Vec::new(),
                        anchor: (x, y),
                    });
                }
                true
            }
            _ => false,
        }
    }

    fn open_row_menu(&mut self, row: usize, at: (f32, f32)) {
        let ticked = self.ticked(row);
        let toggle = if ticked { "Untick" } else { "Tick" };
        let items = if self.is_rest(row) {
            vec![Item::new(MAct::Toggle, toggle)]
        } else {
            vec![
                Item::new(MAct::MoveTo, "Move to a section\u{2026}"),
                Item::new(MAct::Merge, "Merge with the note above").enabled(row > 0),
                Item::new(MAct::Toggle, toggle).sep(),
            ]
        };
        self.menu = Some(Pop {
            menu: Menu::new(items, at.0, at.1, false),
            row,
            sections: Vec::new(),
            anchor: at,
        });
    }

    fn open_dest_menu(&mut self) {
        let Some((_, r)) = self.spots.iter().find(|(h, _)| *h == PHit::Dest).copied() else {
            return;
        };
        let mut items: Vec<Item<MAct>> = Vec::new();
        for (i, g) in self.groups.iter().enumerate() {
            items.push(Item::new(MAct::Group(i), g.name.clone()).checked(
                !self.dest.is_new && mdshared_app::paths::same_path(&g.file, &self.dest.file),
            ));
        }
        items.push(Item::new(MAct::NewGroup, "New group\u{2026}").sep());
        self.menu = Some(Pop {
            menu: Menu::new(items, r.x, r.bottom() + 2.0, false),
            row: self.sel,
            sections: Vec::new(),
            anchor: (r.x, r.bottom() + 2.0),
        });
    }

    fn open_section_menu(&mut self, row: usize, anchor: (f32, f32)) {
        let sections = self.plan.sections();
        let mut items: Vec<Item<MAct>> = Vec::new();
        for (i, s) in sections.iter().enumerate() {
            let label = if s.is_empty() {
                "No section".to_string()
            } else {
                s.join(" \u{203A} ")
            };
            let here = self
                .plan
                .note(row)
                .is_some_and(|n| n.sections() == s.as_slice());
            items.push(Item::new(MAct::Section(i), label).checked(here));
        }
        items.push(Item::new(MAct::NewSection, "New section\u{2026}").sep());
        self.menu = Some(Pop {
            menu: Menu::new(items, anchor.0, anchor.1, false),
            row,
            sections,
            anchor,
        });
    }

    fn choose(&mut self, act: MAct) {
        let Some(pop) = self.menu.take() else { return };
        let row = pop.row;
        match act {
            MAct::MoveTo => self.open_section_menu(row, pop.anchor),
            MAct::Merge => self.merge(row),
            MAct::Toggle => self.toggle(row),
            MAct::Section(i) => {
                if let Some(path) = pop.sections.get(i) {
                    let names: Vec<&str> = path.iter().map(String::as_str).collect();
                    self.move_to(row, &names);
                }
            }
            MAct::NewSection => {
                self.edit = Some(Edit {
                    kind: EditKind::Section(row),
                    field: TextField::new(),
                    click: None,
                });
            }
            MAct::NewNote(line) => self.new_note(line),
            MAct::Group(i) => {
                if let Some(g) = self.groups.get(i).cloned() {
                    self.set_dest(g);
                }
            }
            MAct::NewGroup => {
                self.edit = Some(Edit {
                    kind: EditKind::Group,
                    field: TextField::new(),
                    click: None,
                });
            }
        }
        self.tick += 1;
    }

    /// F2, or a second click: the name of the selected note in a field over its row.
    pub fn start_rename(&mut self) {
        let row = self.sel;
        let Some(n) = self.plan.note(row) else { return };
        let mut field = TextField::new();
        field.set_text(n.title());
        field.select_all();
        self.edit = Some(Edit {
            kind: EditKind::Rename(row),
            field,
            click: None,
        });
        self.reveal();
        self.tick += 1;
    }

    /// Takes the field's text where it was meant for: a name, a section or a group.
    pub fn commit_edit(&mut self) {
        let Some(e) = self.edit.take() else { return };
        let text = e.field.text().trim().to_string();
        match e.kind {
            EditKind::Rename(i) => {
                if !text.is_empty() {
                    self.rename(i, &text);
                }
            }
            EditKind::Section(i) => {
                if !text.is_empty() {
                    self.move_to(i, &[text.as_str()]);
                }
            }
            EditKind::Group => {
                if !text.is_empty() {
                    let dest = Dest::new(&self.root, &text, true);
                    self.set_dest(dest);
                }
            }
        }
        self.tick += 1;
    }

    pub fn cancel_edit(&mut self) {
        self.edit = None;
        self.tick += 1;
    }

    /// The next (or previous) row with a mark on it, from the selected one; F7.
    pub fn step_mark(&mut self, forward: bool) {
        let marked = self.marked();
        let to = if forward {
            marked
                .iter()
                .copied()
                .find(|&i| i > self.sel)
                .or(marked.first().copied())
        } else {
            marked
                .iter()
                .rev()
                .copied()
                .find(|&i| i < self.sel)
                .or(marked.last().copied())
        };
        if let Some(i) = to {
            self.select(i);
        }
    }

    /// A key: its name, `None` for a key without one (the context-menu key has none, so
    /// `menu_key` says it was that one).
    pub fn key(
        &mut self,
        name: Option<&KeyName>,
        menu_key: bool,
        mods: Mods,
        typed: Option<&str>,
    ) -> Act {
        self.tick += 1;
        let name = name.cloned();
        if let Some(pop) = self.menu.as_mut() {
            match name {
                Some(KeyName::Escape) => self.menu = None,
                Some(KeyName::Up) => pop.menu.step(false),
                Some(KeyName::Down) => pop.menu.step(true),
                Some(KeyName::Enter | KeyName::Space) => {
                    if let Some(act) = pop.menu.chosen() {
                        self.choose(act);
                    }
                }
                _ => {}
            }
            return Act::Nothing;
        }
        if self.edit.is_some() {
            match name {
                Some(KeyName::Enter) => self.commit_edit(),
                Some(KeyName::Escape) => self.cancel_edit(),
                Some(n) => {
                    if let Some(e) = self.edit.as_mut() {
                        e.field.handle_key(&n, mods, typed, true);
                    }
                }
                // A key with no name of its own still types its text, as it does in any other
                // field (audit 4, N6).
                None => {
                    if let Some(t) = typed.filter(|t| !t.chars().any(char::is_control))
                        && crate::keys::types_text(mods)
                        && let Some(e) = self.edit.as_mut()
                    {
                        e.field.insert(t);
                    }
                }
            }
            return Act::Nothing;
        }
        let at = {
            let g = self.geo();
            let r = self.rows().rect(g.list, self.sel);
            (r.x + 60.0, r.bottom())
        };
        if menu_key || (mods.shift && matches!(name, Some(KeyName::F(10)))) {
            let row = self.sel;
            self.open_row_menu(row, at);
            return Act::Nothing;
        }
        let nav = match name {
            Some(KeyName::Escape) => return Act::Close,
            Some(KeyName::Enter) => return self.write(),
            Some(KeyName::Space) => {
                let row = self.sel;
                self.toggle(row);
                return Act::Nothing;
            }
            Some(KeyName::F(2)) => {
                self.start_rename();
                return Act::Nothing;
            }
            Some(KeyName::F(7)) => {
                self.step_mark(!mods.shift);
                return Act::Nothing;
            }
            Some(KeyName::Up) => Nav::Up,
            Some(KeyName::Down) => Nav::Down,
            Some(KeyName::Home) => Nav::Home,
            Some(KeyName::End) => Nav::End,
            Some(KeyName::PageUp) => Nav::PageUp,
            Some(KeyName::PageDown) => Nav::PageDown,
            _ => return Act::Nothing,
        };
        if let Some(to) = self.rows().nav(Some(self.sel), nav) {
            self.select(to);
        }
        Act::Nothing
    }

    /// What the frame of the screen depends on besides its tick.
    pub fn sig(&self) -> (u64, Option<PHit>, Option<PHit>) {
        (self.tick, self.hover, self.pressed)
    }
}

#[cfg(test)]
// These test the old `press` and `pointer` route as well as the new one.
#[allow(deprecated)]
mod tests {
    use super::demo::{japan, log, many, recipes};
    use super::*;
    use crate::gfx::{Face, Gfx, Painter};
    use crate::theme::{DARK, LIGHT, Palette};

    /// `thousands` and `size_text` as they were written here before they were handed to
    /// `mdshared_app::fmt`: what they give must not have changed for any caller.
    #[test]
    fn the_shared_formatters_give_what_this_module_gave() {
        let old_thousands = |n: usize| {
            let digits = n.to_string();
            let mut out = String::new();
            for (i, c) in digits.chars().enumerate() {
                if i > 0 && (digits.len() - i).is_multiple_of(3) {
                    out.push(',');
                }
                out.push(c);
            }
            out
        };
        let old_size = |bytes: usize| {
            let b = bytes as f64;
            if bytes < 1024 {
                format!("{bytes} B")
            } else if bytes < 1024 * 1024 {
                format!("{:.1} KB", b / 1024.0)
            } else {
                format!("{:.1} MB", b / (1024.0 * 1024.0))
            }
        };
        let mut n: usize = 0;
        for step in 0..4000usize {
            assert_eq!(thousands(n), old_thousands(n), "{n}");
            assert_eq!(size_text(n), old_size(n), "{n}");
            // Small numbers one by one, then growing past a terabyte.
            n = if step < 1100 {
                n + 1
            } else {
                n + n / 97 + 1013
            };
        }
        for edge in [
            1023,
            1024,
            1_048_575,
            1_048_576,
            1_073_741_823,
            1_073_741_824,
            usize::MAX,
        ] {
            assert_eq!(thousands(edge), old_thousands(edge));
            assert_eq!(size_text(edge), old_size(edge));
        }
    }
    use mdshared_app::split::Text;
    use mdshared_text::Bitmap;

    /// The document's ATX headings, found with a plain scan of its own that skips code fences
    /// (the card is given its headings; mdedit finds them with its function list, which is not a
    /// dependency of this crate).
    fn headings_of(doc: &Text) -> Vec<Heading> {
        let run = |s: &str| -> Option<(u8, usize)> {
            let c = *s.as_bytes().first()?;
            if c != b'`' && c != b'~' {
                return None;
            }
            let n = s.bytes().take_while(|&b| b == c).count();
            (n >= 3).then_some((c, n))
        };
        let mut out = Vec::new();
        let mut open: Option<(u8, usize)> = None;
        let mut body = String::new();
        for i in 0..doc.len_lines() {
            doc.line_text(i, &mut body);
            let indent = body
                .len()
                .saturating_sub(body.trim_start_matches([' ', '\t']).len());
            let t = body.trim_start_matches([' ', '\t']).trim_end();
            if let Some((ch, n)) = open {
                if indent < 4
                    && let Some((c, k)) = run(t)
                    && c == ch
                    && k >= n
                    && t.get(k..).is_some_and(|r| r.trim().is_empty())
                {
                    open = None;
                }
                continue;
            }
            if indent < 4
                && let Some(r) = run(t)
            {
                open = Some(r);
                continue;
            }
            if indent >= 4 {
                continue;
            }
            let hashes = t.bytes().take_while(|&b| b == b'#').count();
            if (1..=6).contains(&hashes)
                && (t.len() == hashes
                    || t.get(hashes..).is_some_and(|r| r.starts_with([' ', '\t'])))
                && !t.get(hashes..).unwrap_or("").trim().is_empty()
            {
                out.push(Heading {
                    line: i,
                    level: hashes as u8,
                });
            }
        }
        out
    }

    fn nothing() -> Reader {
        Box::new(|_| None)
    }

    fn screen_in(text: &str, level: Option<u8>, root: &Path, reader: Reader) -> PlanScreen {
        let doc = Text::new(text);
        let h = headings_of(&doc);
        PlanScreen::new(Init {
            doc: Arc::new(doc),
            name: "Doc.md",
            path: None,
            headings: &h,
            level,
            encoding: None,
            dest: Dest::new(root, "Group", false),
            groups: vec![Dest::new(root, "Other", false)],
            root: root.to_path_buf(),
            reader,
        })
    }

    fn screen_of(text: &str, level: Option<u8>) -> PlanScreen {
        screen_in(text, level, Path::new("library"), nothing())
    }

    fn render(s: &mut PlanScreen, pal: &Palette, w: u32, h: u32) -> Bitmap {
        let mut bmp = Bitmap::new(w, h);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, w as f32, h as f32), pal.page);
            s.draw(&mut p, pal, w as f32, h as f32, true);
        }
        bmp
    }

    fn pixel(b: &Bitmap, x: u32, y: u32) -> [u8; 3] {
        let o = ((y * b.width + x) * 4) as usize;
        [b.pixels[o], b.pixels[o + 1], b.pixels[o + 2]]
    }

    fn key(s: &mut PlanScreen, k: KeyName) -> Act {
        s.key(Some(&k), false, Mods::default(), None)
    }

    fn doc_line_blank(doc: &dyn Lines, i: usize) -> bool {
        let mut t = String::new();
        doc.line_text(i, &mut t);
        t.trim().is_empty()
    }

    /// The non-blank lines in no ticked note, counted from the document and the notes' ranges.
    fn lines_left_out(s: &PlanScreen) -> usize {
        let doc = s.plan.document();
        (0..s.plan.lines())
            .filter(|&i| !doc_line_blank(&**doc, i))
            .filter(|&i| {
                !s.plan
                    .notes()
                    .iter()
                    .any(|n| n.ticked() && n.range().contains(&i))
            })
            .count()
    }

    #[test]
    fn the_card_is_laid_out_as_measured_and_drops_its_preview_when_narrow() {
        let g = geo(card(1280.0, 800.0), false);
        assert_eq!((g.card.w, g.card.h), (1200.0, 736.0));
        assert_eq!(g.header.h, 104.0);
        assert_eq!(g.cover.h, 64.0);
        assert_eq!(g.strip.h, 18.0);
        assert_eq!(g.list.w, 600.0, "the list is 600 wide");
        assert_eq!(g.preview_head.h, 72.0);
        assert!(g.preview.is_some_and(|p| p.w == 600.0));
        assert_eq!(DIALOG_FOOTER, 72.0);
        assert_eq!(ROW_H, 44.0);
        let small = geo(card(800.0, 600.0), false);
        assert!(small.card.w < 860.0);
        assert!(small.preview.is_none(), "no preview below 860 px of card");
        assert_eq!(small.list.w, small.card.w, "the list takes the width");
        let wide = geo(card(1280.0, 800.0), true);
        assert!(wide.notice.is_some_and(|n| n.h == NOTICE_H));
        assert_eq!(wide.cover.y, g.cover.y + NOTICE_H);
    }

    #[test]
    fn a_three_line_note_in_a_five_thousand_line_document_is_at_least_two_pixels() {
        let mut text = String::from("## tiny\n\nx\n\n## big\n\n");
        for i in 0..5_000 {
            text.push_str(&format!("line {i}\n"));
        }
        let s = screen_of(&text, Some(2));
        assert!(s.plan.lines() >= 5_000);
        assert_eq!(s.plan.note(0).expect("a note").lines(), 3);
        let bands = strip_bands(&s.plan, 1152.0);
        let tiny = bands
            .iter()
            .find(|b| b.row == 0 && b.note)
            .expect("its band");
        assert!(
            tiny.x1 - tiny.x0 >= 2.0,
            "a 3-line note took {} px of the strip",
            tiny.x1 - tiny.x0
        );
        // Proportionally it would have been under a pixel: 3 / 5,000 of 1,152.
    }

    #[test]
    fn the_bands_tile_the_strip_whatever_the_plan() {
        for (text, level) in [
            (japan(), Some(2)),
            (japan(), Some(1)),
            (recipes(), None),
            (log(), Some(2)),
            (many(), None),
            (String::from("no headings here\njust text\n"), None),
        ] {
            let mut s = screen_of(&text, level);
            if s.plan.notes().len() > 3 {
                s.toggle(1);
                s.toggle(2);
            }
            for width in [1.0, 40.0, 800.0, 1152.0, 2304.0] {
                let bands = strip_bands(&s.plan, width);
                let mut at = 0.0f32;
                for b in &bands {
                    assert_eq!(b.x0, at, "a gap or an overlap before {b:?} at {width}");
                    assert!(b.x1 >= b.x0);
                    at = b.x1;
                }
                if !bands.is_empty() {
                    assert_eq!(at, width, "the bands end where the strip does");
                }
            }
        }
        // Past two thousand ticked notes the minimum cannot be kept, and it is a plain proportion.
        let w = widths(&vec![1.0; 600], &vec![true; 600], 1152.0, 2.0);
        assert!((w.iter().sum::<f32>() - 1152.0).abs() < 0.01);
    }

    #[test]
    fn blank_lines_between_two_notes_are_not_a_gap_of_their_own() {
        let s = screen_of(&recipes(), None);
        let notes = s.plan.notes().len();
        assert!(notes > 5);
        let bands = strip_bands(&s.plan, 1152.0);
        assert_eq!(
            bands.len(),
            notes,
            "one band a note, and none for the blank lines"
        );
        assert!(bands.iter().all(|b| b.note));
        assert_eq!(bands[0].x0, 0.0);
        assert_eq!(bands[notes - 1].x1, 1152.0);
        // A stretch with words in it is a gap, and is the rest's.
        let j = screen_of(&japan(), Some(2));
        assert!(strip_bands(&j.plan, 1152.0).iter().any(|b| !b.note));
    }

    #[test]
    fn a_click_on_a_band_selects_the_row_that_owns_that_line() {
        let mut s = screen_of(&japan(), Some(2));
        let g = s.geo();
        s.bands_for(g.strip.w);
        let rest = s.plan.notes().len();
        let mut gaps = 0;
        let mut notes = 0;
        for b in s.bands.clone() {
            let x = g.strip.x + (b.x0 + b.x1) / 2.0;
            let y = g.strip.cy();
            s.press(x, y);
            s.release(x, y);
            assert_eq!(s.sel, b.row, "band {b:?}");
            if b.note {
                notes += 1;
                let first = s.plan.note(b.row).expect("a note").range().start;
                assert_eq!(s.plan.note_at(first), Some(b.row), "the row owns the line");
            } else {
                gaps += 1;
                assert_eq!(s.sel, rest, "a gap is the rest");
                assert!(!s.preview.is_empty(), "its lines are in the preview");
            }
        }
        assert!(notes == 23 && gaps >= 3, "{notes} notes and {gaps} gaps");
        // The list follows: the selected row is on screen.
        let rows = s.rows();
        s.press(g.strip.x + 2.0, g.strip.cy());
        s.release(g.strip.x + 2.0, g.strip.cy());
        assert!(rows.visible().contains(&0) || s.scroll == rows.reveal(s.sel));
    }

    #[test]
    fn unticked_or_not_a_note_keeps_its_columns_and_a_tick_changes_only_its_own() {
        let mut s = screen_of(&japan(), Some(2));
        let pal = LIGHT;
        let before = render(&mut s, &pal, 1280, 800);
        let g = s.geo();
        let (sx0, sy0, sx1, sy1) = (
            g.strip.x as u32,
            g.strip.y as u32,
            g.strip.right() as u32,
            g.strip.bottom() as u32,
        );
        let bands = s.bands.clone();
        let edges = |s: &PlanScreen| -> Vec<(usize, f32, f32)> {
            s.bands.iter().map(|b| (b.row, b.x0, b.x1)).collect()
        };
        let before_edges = edges(&s);
        let k = 6usize;
        s.toggle(k);
        let after = render(&mut s, &pal, 1280, 800);
        assert_eq!(edges(&s), before_edges, "a tick moves no band");
        let band = bands
            .iter()
            .find(|b| b.note && b.row == k)
            .copied()
            .expect("the band");
        let (c0, c1) = (sx0 + band.x0 as u32, sx0 + band.x1 as u32);
        for x in sx0..sx1 {
            let differs = (sy0..sy1).any(|y| pixel(&before, x, y) != pixel(&after, x, y));
            if x > c0 && x < c1 {
                assert!(differs, "column {x} of the ticked note did not change");
            } else if x < c0 || x >= c1 {
                assert!(!differs, "column {x} outside the note changed");
            }
        }
        // A note too small to see keeps its two pixels ticked or not, so ticking it moves
        // nothing else either.
        let mut text = String::from("## tiny\n\nx\n\n## big\n\n");
        for i in 0..4_000 {
            text.push_str(&format!("line {i}\n"));
        }
        text.push_str("\n## last\n\nlast words\n");
        let mut t = screen_of(&text, Some(2));
        let w = t.geo().strip.w;
        let edges = |b: &[Band]| -> Vec<(usize, f32, f32)> {
            b.iter().map(|b| (b.row, b.x0, b.x1)).collect()
        };
        let before = edges(&strip_bands(&t.plan, w));
        t.toggle(0);
        assert_eq!(
            before,
            edges(&strip_bands(&t.plan, w)),
            "unticking the three-line note moved the other bands"
        );
    }

    #[test]
    fn the_rest_counts_the_non_blank_lines_outside_ticked_notes_after_every_change() {
        let mut s = screen_of(&japan(), Some(2));
        let check = |s: &PlanScreen, what: &str| {
            assert_eq!(s.rest.lines(), lines_left_out(s), "after {what}");
            let want = if s.rest.ticked() {
                0
            } else {
                lines_left_out(s)
            };
            assert_eq!(s.totals.lines_in_no_note, want, "the footer after {what}");
            assert_eq!(s.plan.check(), Ok(()), "after {what}");
        };
        check(&s, "opening");
        s.toggle(2);
        check(&s, "an untick");
        s.toggle(2);
        check(&s, "a tick");
        s.untick_all();
        check(&s, "unticking all");
        assert!(s.rest.lines() > 200, "most of the document is left out");
        s.tick_all();
        s.toggle(s.plan.notes().len());
        check(&s, "ticking the rest");
        s.toggle(s.plan.notes().len());
        s.merge(5);
        check(&s, "a merge");
        s.merge(0);
        check(&s, "a merge of the first note, which is nothing");
        let line = s.plan.note(3).expect("a note").range().start + 2;
        s.new_note(line);
        check(&s, "a new note");
        s.new_note(0);
        check(&s, "a new note in the preamble");
        s.toggle(0);
        check(&s, "an untick of the new note");
    }

    #[test]
    fn a_merge_and_a_new_note_keep_every_line_in_exactly_one_note() {
        let mut s = screen_of(&japan(), Some(2));
        let covered = |s: &PlanScreen| -> Vec<usize> {
            let mut v = vec![0usize; s.plan.lines()];
            for n in s.plan.notes() {
                for l in n.range() {
                    v[l] += 1;
                }
            }
            v
        };
        let before: usize = covered(&s).iter().filter(|&&c| c > 0).count();
        let notes = s.plan.notes().len();
        s.merge(4);
        assert_eq!(s.plan.notes().len(), notes - 1, "a merge joins two notes");
        assert!(covered(&s).iter().all(|&c| c <= 1), "after a merge");
        s.new_note(s.plan.note(10).expect("a note").range().start + 3);
        assert_eq!(s.plan.notes().len(), notes, "a new note divides one");
        let c = covered(&s);
        assert!(c.iter().all(|&c| c <= 1), "after a new note");
        assert!(
            c.iter().filter(|&&c| c > 0).count() >= before,
            "no line was lost"
        );
        assert_eq!(s.plan.check(), Ok(()));
    }

    #[test]
    fn a_rename_changes_the_file_and_the_link_and_not_a_heading_from_the_source() {
        let mut s = screen_of(&japan(), Some(2));
        let first_line = |s: &PlanScreen, i: usize| s.plan.note_lines(i)[0].text.clone();
        assert_eq!(first_line(&s, 0), "# Planning the route\n");
        let old = s.targets[0].path.clone();
        s.rename(0, "A new name?");
        let t = &s.targets[0].path;
        assert_ne!(*t, old);
        assert_eq!(
            t.file_name().and_then(|n| n.to_str()),
            Some("a-new-name_.md"),
            "the file is named by the one rule of the library"
        );
        assert!(
            s.target_text(0).ends_with("a-new-name_.md"),
            "the link shows it"
        );
        assert_eq!(
            first_line(&s, 0),
            "# Planning the route\n",
            "a heading that came from the document keeps its words"
        );
        // A note that starts with no heading has the app's title above it, and that follows.
        let mut p = screen_of(&japan(), Some(2));
        p.new_note(p.plan.note(2).expect("a note").range().start + 3);
        let k = p
            .plan
            .note_at(p.plan.note(2).expect("a note").range().end)
            .unwrap_or(3);
        let added = (0..p.plan.notes().len())
            .find(|&i| p.plan.note(i).is_some_and(|n| n.title_added()))
            .unwrap_or(k);
        let was = first_line(&p, added);
        assert!(was.starts_with("# Lines "), "{was}");
        p.rename(added, "Mine");
        assert_eq!(first_line(&p, added), "# Mine\n");
    }

    #[test]
    fn a_name_taken_by_other_bytes_is_numbered_and_the_same_bytes_are_already_there() {
        let first = screen_of(&japan(), Some(2));
        let mut table: HashMap<PathBuf, Found> = HashMap::new();
        let text0 = first.plan.note_text(0);
        table.insert(
            first.targets[0].path.clone(),
            Found {
                len: text0.len() as u64,
                hash: fingerprint(text0.as_bytes()),
            },
        );
        table.insert(first.targets[1].path.clone(), Found { len: 5, hash: 1 });
        let reader: Reader = Box::new(move |p| table.get(p).copied());
        let s = screen_in(&japan(), Some(2), Path::new("library"), reader);
        assert!(s.targets[0].already, "same name, same bytes");
        assert!(
            !s.plan.note(0).expect("a note").ticked(),
            "and it comes unticked"
        );
        assert_eq!(s.mark(0), Some(Mark::Already));
        assert_eq!(
            s.mark(0).map(|m| m.text()),
            Some("already there".to_string())
        );
        assert_eq!(
            s.targets[1].renamed.as_deref(),
            Some("packing-and-money 2.md")
        );
        assert_eq!(
            s.mark(1).map(|m| m.text()),
            Some("will be written as \u{2018}packing-and-money 2.md\u{2019}".to_string())
        );
        assert!(s.plan.note(1).expect("a note").ticked(), "and looks normal");
        assert_eq!(s.mark(2), None);
        // Two notes of one name in one plan: the second is numbered, until the first is unticked.
        let mut t = screen_of("## Same\n\na\n\n## Same\n\nb\n", Some(2));
        assert_eq!(t.targets[1].renamed.as_deref(), Some("same 2.md"));
        t.toggle(0);
        assert_eq!(t.targets[1].renamed, None, "the name is free again");
    }

    #[test]
    fn a_boundary_moved_to_keep_a_code_block_whole_says_so_on_its_row() {
        let mut s = screen_of(&log(), Some(2));
        let inside = 38;
        s.new_note(inside);
        let moved: Vec<usize> = (0..s.plan.notes().len())
            .filter(|&i| s.mark(i) == Some(Mark::Moved))
            .collect();
        assert_eq!(moved.len(), 1);
        assert_eq!(
            Mark::Moved.text(),
            "boundary moved to keep a code block whole"
        );
        assert_eq!(s.marked(), moved);
    }

    #[test]
    fn the_keys_are_those_of_the_replace_confirmation_and_the_added_ones() {
        let mut s = screen_of(&japan(), Some(2));
        assert_eq!(s.sel, 0);
        key(&mut s, KeyName::Down);
        key(&mut s, KeyName::Down);
        assert_eq!(s.sel, 2);
        key(&mut s, KeyName::Up);
        assert_eq!(s.sel, 1);
        key(&mut s, KeyName::End);
        assert_eq!(s.sel, 23, "End is the rest");
        key(&mut s, KeyName::Home);
        assert_eq!(s.sel, 0);
        key(&mut s, KeyName::PageDown);
        assert!(s.sel > 1);
        key(&mut s, KeyName::Home);
        assert!(s.plan.note(0).expect("a note").ticked());
        key(&mut s, KeyName::Space);
        assert!(!s.plan.note(0).expect("a note").ticked(), "Space unticks");
        key(&mut s, KeyName::Space);
        assert!(s.plan.note(0).expect("a note").ticked(), "and ticks");
        assert_eq!(key(&mut s, KeyName::Enter), Act::Write);
        assert_eq!(s.write_pressed, 1);
        assert_eq!(key(&mut s, KeyName::Escape), Act::Close);
        // F2 renames in place, Enter keeps the name, Escape drops it.
        key(&mut s, KeyName::F(2));
        assert!(matches!(
            s.edit.as_ref().map(|e| e.kind),
            Some(EditKind::Rename(0))
        ));
        s.edit.as_mut().expect("the field").field.set_text("Route");
        assert_eq!(
            key(&mut s, KeyName::Escape),
            Act::Nothing,
            "Escape ends the edit only"
        );
        assert!(s.edit.is_none());
        assert_eq!(
            s.plan.note(0).expect("a note").title(),
            "Planning the route"
        );
        key(&mut s, KeyName::F(2));
        s.edit.as_mut().expect("the field").field.set_text("Route");
        key(&mut s, KeyName::Enter);
        assert_eq!(s.plan.note(0).expect("a note").title(), "Route");
        assert_eq!(s.write_pressed, 1, "Enter in the field writes nothing");
        // F7 and Shift+F7 walk the rows with a mark on them.
        let mut m = screen_of(&log(), Some(2));
        m.new_note(38);
        m.select(0);
        key(&mut m, KeyName::F(7));
        let at = m.marked()[0];
        assert_eq!(m.sel, at);
        m.key(
            Some(&KeyName::F(7)),
            false,
            Mods {
                shift: true,
                ..Mods::default()
            },
            None,
        );
        assert_eq!(m.sel, at, "the only mark: it stays");
    }

    #[test]
    fn the_row_menu_opens_from_the_keyboard_and_merges_and_moves_and_unticks() {
        let mut s = screen_of(&japan(), Some(2));
        key(&mut s, KeyName::Down);
        key(&mut s, KeyName::Down);
        let notes = s.plan.notes().len();
        s.key(None, true, Mods::default(), None);
        let labels: Vec<String> = s
            .menu
            .as_ref()
            .expect("the menu")
            .menu
            .items
            .iter()
            .map(|i| i.label.clone())
            .collect();
        assert_eq!(
            labels,
            [
                "Move to a section\u{2026}",
                "Merge with the note above",
                "Untick"
            ]
        );
        // Down, Down, Enter: Merge with the note above.
        key(&mut s, KeyName::Down);
        key(&mut s, KeyName::Down);
        key(&mut s, KeyName::Enter);
        assert_eq!(s.plan.notes().len(), notes - 1);
        assert_eq!(s.sel, 1, "the note that is left");
        assert_eq!(s.plan.check(), Ok(()));
        // Shift+F10 is the same menu; Untick is its last row.
        s.key(
            Some(&KeyName::F(10)),
            false,
            Mods {
                shift: true,
                ..Mods::default()
            },
            None,
        );
        assert!(s.menu.is_some());
        key(&mut s, KeyName::Up);
        key(&mut s, KeyName::Enter);
        assert!(!s.plan.note(1).expect("a note").ticked());
        // Move to a section: the menu of the plan's sections, then one of them.
        s.key(None, true, Mods::default(), None);
        key(&mut s, KeyName::Down);
        key(&mut s, KeyName::Enter);
        let pop = s.menu.as_ref().expect("the sections");
        assert!(pop.menu.items.iter().any(|i| i.label == "Tokyo"));
        assert!(
            pop.menu
                .items
                .iter()
                .any(|i| i.label == "New section\u{2026}")
        );
        let tokyo = pop
            .menu
            .items
            .iter()
            .position(|i| i.label == "Tokyo")
            .unwrap_or(0);
        for _ in 0..=tokyo {
            key(&mut s, KeyName::Down);
        }
        key(&mut s, KeyName::Enter);
        assert_eq!(
            s.plan.note(1).expect("a note").sections(),
            ["Tokyo".to_string()]
        );
        assert!(
            s.target_text(1).contains("Tokyo"),
            "the file goes in its folder"
        );
    }

    #[test]
    fn a_right_click_on_a_line_of_the_preview_starts_a_note_there() {
        let mut s = screen_of(&japan(), Some(2));
        let g = s.geo();
        let notes = s.plan.notes().len();
        let k = (3..s.preview.len())
            .find(|&k| s.preview[k].source.is_some() && !s.preview[k].text.trim().is_empty())
            .expect("a line of the first note that has words");
        let clicked = s.preview[k].source.expect("a source line");
        let y = g.preview_body.y + 4.0 + k as f32 * LINE_H + LINE_H / 2.0;
        assert!(s.right_press(g.preview_body.x + 100.0, y));
        let menu = &s.menu.as_ref().expect("the menu").menu;
        assert_eq!(menu.items.len(), 1);
        assert_eq!(menu.items[0].label, "Start a new note here");
        key(&mut s, KeyName::Down);
        key(&mut s, KeyName::Enter);
        assert_eq!(s.plan.notes().len(), notes + 1);
        assert_eq!(s.sel, 1, "the new note is selected");
        assert_eq!(
            s.plan.note(1).expect("a note").first_line(),
            clicked,
            "it starts at the line that was clicked"
        );
        assert_eq!(s.plan.check(), Ok(()));
    }

    #[test]
    fn a_second_click_on_the_selected_row_renames_it() {
        let mut s = screen_of(&japan(), Some(2));
        let g = s.geo();
        let rows = s.rows();
        let r = rows.rect(g.list, 2);
        let (x, y) = (r.x + 200.0, r.cy());
        s.press(x, y);
        s.release(x, y);
        assert_eq!(s.sel, 2);
        assert!(s.edit.is_none(), "the first click selects");
        s.press(x, y);
        s.release(x, y);
        assert!(matches!(
            s.edit.as_ref().map(|e| e.kind),
            Some(EditKind::Rename(2))
        ));
        // A click elsewhere takes the name as it stands.
        s.edit.as_mut().expect("the field").field.set_text("Hello");
        let other = rows.rect(g.list, 4);
        s.press(other.x + 200.0, other.cy());
        s.release(other.x + 200.0, other.cy());
        assert!(s.edit.is_none());
        assert_eq!(s.plan.note(2).expect("a note").title(), "Hello");
    }

    #[test]
    fn the_tick_box_and_tick_all_and_untick_all_change_the_plan() {
        let mut s = screen_of(&japan(), Some(2));
        let g = s.geo();
        let r = s.rows().rect(g.list, 3);
        let (x, y) = (r.x + 28.0, r.cy());
        s.press(x, y);
        s.release(x, y);
        assert!(!s.plan.note(3).expect("a note").ticked());
        assert_eq!(s.sel, 3);
        render(&mut s, &LIGHT, 1280, 800);
        let spot = |s: &PlanScreen, h: PHit| {
            s.spots
                .iter()
                .find(|(k, _)| *k == h)
                .map(|(_, r)| *r)
                .expect("a control")
        };
        let r = spot(&s, PHit::UntickAll);
        s.press(r.cx(), r.cy());
        s.release(r.cx(), r.cy());
        assert_eq!(s.totals.notes, 0);
        let r = spot(&s, PHit::TickAll);
        s.press(r.cx(), r.cy());
        s.release(r.cx(), r.cy());
        assert_eq!(s.totals.notes, 23);
    }

    #[test]
    fn a_cancelled_press_does_nothing() {
        let mut s = screen_of(&japan(), Some(2));
        assert!(!s.cancel(), "nothing held");
        let g = s.geo();
        let r = s.rows().rect(g.list, 3);
        let (x, y) = (r.x + 28.0, r.cy());
        s.press(x, y);
        let tick = s.tick;
        assert!(s.cancel());
        assert!(s.tick > tick, "drawn again");
        assert!(!s.cancel(), "once");
        assert_eq!(s.release(x, y), Act::Nothing);
        assert!(
            s.plan.note(3).expect("a note").ticked(),
            "the tick box did not flip"
        );
        // A thumb in hand is let go: a move after the cancel scrolls nothing.
        s.drag = Some(Drag {
            pane: Pane::List,
            grab: 0.0,
        });
        assert!(s.cancel());
        let scroll = s.scroll;
        s.pointer(x, y + 300.0);
        assert_eq!(s.scroll, scroll);
    }

    // ---- the pointer route with the rename field's whole mouse editing ----

    /// A screen with a rename of row 2 open, drawn once with the returned `Gfx` (which places
    /// the field); `None` on a system with no font for the name.
    fn renaming() -> Option<(PlanScreen, Gfx)> {
        let mut s = screen_of(&japan(), Some(2));
        s.sel = 2;
        s.start_rename();
        let mut gfx = Gfx::new();
        let mut bmp = Bitmap::new(1280, 800);
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            s.draw(&mut p, &LIGHT, 1280.0, 800.0, true);
        }
        let area = s.field_at?;
        let text = s.edit.as_ref()?.field.text().to_string();
        let stops = {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.caret_stops(Face::Ui, 13.5, &text)
        };
        if text.is_empty() || stops.last().is_none_or(|&w| w <= 0.0) {
            eprintln!("skipped: no advancing glyph for the test text in the system fonts");
            return None;
        }
        assert!(area.frame().w > 0.0);
        Some((s, gfx))
    }

    fn field(s: &PlanScreen) -> &TextField {
        &s.edit.as_ref().expect("a rename").field
    }

    #[test]
    fn the_new_route_gives_the_rename_field_its_clicks_drag_and_padding() {
        let Some((mut s, mut gfx)) = renaming() else {
            return;
        };
        let t0 = Instant::now();
        let ms = std::time::Duration::from_millis;
        let area = s.field_at.unwrap();
        let name = field(&s).text().to_string();
        let text = area.text().area;
        // A press in the frame's padding, left of the text: the caret at the start, at once.
        let pad = (area.frame().x + 3.0, area.frame().cy());
        assert!(!text.contains(pad.0, pad.1));
        s.pointer_press(pad, Mods::default(), t0, &mut gfx);
        assert_eq!(s.pointer_release(pad, t0, &mut gfx), Act::Nothing);
        assert!(!field(&s).has_selection());
        // A drag from there to the far end selects the whole name.
        let end = (text.right() + 40.0, text.cy());
        s.pointer_press(pad, Mods::default(), t0 + ms(2000), &mut gfx);
        s.pointer_motion(end, t0 + ms(2010), &mut gfx);
        assert_eq!(field(&s).selected_text(), name);
        s.pointer_release(end, t0 + ms(2020), &mut gfx);
        // Three presses at one place: the caret, a word, everything.
        let mid = (text.x + 2.0, text.cy());
        for (k, dt) in [(1, 4000), (2, 4010), (3, 4020)] {
            s.pointer_press(mid, Mods::default(), t0 + ms(dt), &mut gfx);
            s.pointer_release(mid, t0 + ms(dt), &mut gfx);
            if k == 3 {
                assert_eq!(field(&s).selected_text(), name, "the triple click");
            }
        }
        assert!(s.edit.is_some(), "still renaming");
        // The old release ends a drag of the new route.
        s.pointer_press(mid, Mods::default(), t0 + ms(9000), &mut gfx);
        assert!(s.input.is_dragging());
        s.release(mid.0, mid.1);
        assert!(!s.input.is_dragging());
        // A press outside the field takes the name, as `press` does.
        s.pointer_press((5.0, 5.0), Mods::default(), t0 + ms(12000), &mut gfx);
        assert!(s.edit.is_none());
        assert_eq!(s.plan.note(2).expect("a note").title(), name);
    }

    #[test]
    fn a_right_press_in_the_rename_field_opens_its_edit_menu_which_takes_the_keys_first() {
        let Some((mut s, mut gfx)) = renaming() else {
            return;
        };
        let t = Instant::now();
        let area = s.field_at.unwrap();
        let at = (area.text().area.x + 2.0, area.text().area.cy());
        assert!(s.secondary_press(at, t, &mut gfx));
        assert!(s.edit_menu_open());
        assert!(
            field(&s).has_selection(),
            "a right press keeps the selection"
        );
        // The screen draws it over everything: the frame differs from one without it.
        let with = render(&mut s, &LIGHT, 1280, 800).pixels;
        // Escape closes the menu and leaves the rename open.
        assert_eq!(
            s.key_with_menu(
                Some(&KeyName::Escape),
                false,
                Mods::default(),
                None,
                &mut gfx
            ),
            Act::Nothing
        );
        assert!(!s.edit_menu_open());
        assert!(s.edit.is_some());
        assert_ne!(with, render(&mut s, &LIGHT, 1280, 800).pixels);
        // The Menu key opens it at the caret; a cancel closes it.
        s.key_with_menu(None, true, Mods::default(), None, &mut gfx);
        assert!(s.edit_menu_open());
        assert!(s.cancel());
        assert!(!s.edit_menu_open());
        // Escape with the menu closed is `key`'s: the rename is dropped.
        s.key_with_menu(
            Some(&KeyName::Escape),
            false,
            Mods::default(),
            None,
            &mut gfx,
        );
        assert!(s.edit.is_none());
        // No rename: a right press is the application's.
        assert!(!s.secondary_press(at, t, &mut gfx));
        // The menu does not outlive the rename it was for.
        let Some((mut s, mut gfx)) = renaming() else {
            return;
        };
        s.secondary_press(at, t, &mut gfx);
        assert!(s.edit_menu_open());
        s.edit = None;
        render(&mut s, &LIGHT, 1280, 800);
        assert!(!s.edit_menu_open());
    }

    #[test]
    fn the_new_route_without_a_rename_is_the_old_one_and_takes_nonsense() {
        let mut s = screen_of(&japan(), Some(2));
        let mut gfx = Gfx::new();
        let t = Instant::now();
        let g = s.geo();
        let r = s.rows().rect(g.list, 3);
        let (x, y) = (r.x + 28.0, r.cy());
        s.pointer_press((x, y), Mods::default(), t, &mut gfx);
        assert_eq!(s.pointer_release((x, y), t, &mut gfx), Act::Nothing);
        assert!(
            !s.plan.note(3).expect("a note").ticked(),
            "the tick box flipped"
        );
        assert_eq!(s.cursor_at((x, y)), None);
        for v in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1e9, 1e9] {
            s.pointer_press((v, v), Mods::default(), t, &mut gfx);
            s.pointer_motion((v, v), t, &mut gfx);
            s.pointer_release((v, v), t, &mut gfx);
            s.secondary_press((v, v), t, &mut gfx);
            s.tick_text(t, &mut gfx);
            s.cancel();
        }
        if let Some((mut s, mut gfx)) = renaming() {
            for v in [f32::NAN, f32::INFINITY, -1e9] {
                s.secondary_press((v, v), t, &mut gfx);
                s.pointer_motion((v, v), t, &mut gfx);
                s.tick_text(t, &mut gfx);
                let _ = s.cursor_at((v, v));
            }
        }
    }

    /// Audit 4, W10: the rename field's spot is what shows of it. With its row half under the
    /// list's head, a click on "Tick all" ticks all and does not land in the field.
    #[test]
    fn a_rename_field_half_out_of_the_list_takes_no_click_meant_for_tick_all() {
        let mut s = screen_of(&japan(), Some(2));
        let spot = |s: &PlanScreen, h: PHit| {
            s.spots
                .iter()
                .find(|(k, _)| *k == h)
                .map(|(_, r)| *r)
                .expect("a control")
        };
        render(&mut s, &LIGHT, 1280, 800);
        let r = spot(&s, PHit::UntickAll);
        s.press(r.cx(), r.cy());
        s.release(r.cx(), r.cy());
        assert_eq!(s.totals.notes, 0);
        key(&mut s, KeyName::Down);
        key(&mut s, KeyName::F(2));
        assert!(matches!(
            s.edit.as_ref().map(|e| e.kind),
            Some(EditKind::Rename(1))
        ));
        s.scroll = ROW_H * 1.5;
        render(&mut s, &LIGHT, 1280, 800);
        let g = s.geo();
        let field = spot(&s, PHit::Field);
        assert!(
            field.y >= g.list.y && field.bottom() <= g.list.bottom(),
            "{field:?}"
        );
        // A point of "Tick all" the whole field would have covered.
        let all = spot(&s, PHit::TickAll);
        let row = s.rows().rect(g.list, 1);
        let (x, y) = (all.cx(), g.list.y - 2.0);
        assert!(all.contains(x, y) && row.contains(x, y), "{all:?} {row:?}");
        s.press(x, y);
        s.release(x, y);
        assert_eq!(s.totals.notes, 23, "Tick all took the click");
    }

    /// Audit 4, N5: the wheel does not scroll a name being typed out of view.
    #[test]
    fn the_wheel_keeps_the_name_being_typed_in_view() {
        let mut s = screen_of(&japan(), Some(2));
        render(&mut s, &LIGHT, 1280, 800);
        key(&mut s, KeyName::F(2));
        let g = s.geo();
        s.wheel(g.list.cx(), g.list.cy(), ROW_H * 10.0);
        let r = s.rows().rect(g.list, 0);
        assert!(r.y >= g.list.y && r.bottom() <= g.list.bottom(), "{r:?}");
        // With no name being typed, it scrolls.
        key(&mut s, KeyName::Escape);
        s.wheel(g.list.cx(), g.list.cy(), ROW_H * 10.0);
        assert!(s.scroll > ROW_H, "{}", s.scroll);
    }

    /// Audit 4, N6: text from a key with no name (a dead key's letter) goes into the name.
    #[test]
    fn a_key_with_no_name_types_its_text_into_the_name() {
        let mut s = screen_of(&japan(), Some(2));
        key(&mut s, KeyName::F(2));
        s.edit.as_mut().expect("the field").field.set_text("Caf");
        s.key(None, false, Mods::default(), Some("\u{e9}"));
        assert_eq!(
            s.edit.as_ref().expect("the field").field.text(),
            "Caf\u{e9}"
        );
        // A control character is no text.
        s.key(None, false, Mods::default(), Some("\u{7}"));
        assert_eq!(
            s.edit.as_ref().expect("the field").field.text(),
            "Caf\u{e9}"
        );
    }

    #[test]
    fn the_level_switch_cuts_the_document_again() {
        let mut s = screen_of(&japan(), Some(2));
        assert_eq!(s.plan.notes().len(), 23);
        render(&mut s, &LIGHT, 1280, 800);
        let hash = s
            .spots
            .iter()
            .find(|(k, _)| *k == PHit::Level(0))
            .map(|(_, r)| *r)
            .expect("the # segment");
        s.press(hash.cx(), hash.cy());
        s.release(hash.cx(), hash.cy());
        assert_eq!(s.plan.level(), 1);
        assert_eq!(s.plan.notes().len(), 7, "six parts and the afterword");
        assert_eq!(s.sel, 0);
        assert_eq!(s.plan.check(), Ok(()));
    }

    #[test]
    fn write_the_notes_is_disabled_with_its_reason_when_nothing_is_ticked_and_writes_nothing() {
        let dir = std::env::temp_dir().join(format!("mdedit-plan-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        let mut s = screen_in(&japan(), Some(2), &dir, fs_reader());
        assert_eq!(s.problem(), None);
        render(&mut s, &LIGHT, 1280, 800);
        let write = s
            .spots
            .iter()
            .find(|(k, _)| *k == PHit::Write)
            .map(|(_, r)| *r)
            .expect("the button takes the pointer");
        s.press(write.cx(), write.cy());
        assert_eq!(s.release(write.cx(), write.cy()), Act::Write);
        assert_eq!(s.write_pressed, 1, "it records that it was pressed");
        assert_eq!(
            std::fs::read_dir(&dir).expect("the folder").count(),
            0,
            "and writes nothing"
        );
        s.untick_all();
        assert_eq!(s.problem().as_deref(), Some("Nothing is ticked."));
        render(&mut s, &LIGHT, 1280, 800);
        assert!(
            s.spots.iter().all(|(k, _)| *k != PHit::Write),
            "a disabled button takes no pointer"
        );
        assert_eq!(key(&mut s, KeyName::Enter), Act::Nothing);
        assert_eq!(s.write_pressed, 1);
        s.blocked = Some("The folder cannot be written.".into());
        s.toggle(0);
        assert_eq!(
            s.problem().as_deref(),
            Some("The folder cannot be written.")
        );
        assert_eq!(key(&mut s, KeyName::Enter), Act::Nothing);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_disabled_primary_button_is_the_plain_frame_the_replace_screen_draws() {
        for pal in [LIGHT, DARK] {
            let mut s = screen_of(&japan(), Some(2));
            s.untick_all();
            let shot = render(&mut s, &pal, 1280, 800);
            let g = s.geo();
            let w = crate::overlay::dialog_button_w(
                &mut Painter::new(&mut Bitmap::new(2, 2), &mut Gfx::new(), 1.0),
                "Write the notes",
            );
            let by = g.card.bottom() - DIALOG_FOOTER + (DIALOG_FOOTER - 32.0) / 2.0;
            let r = Rect::new(g.card.right() - DIALOG_PAD - w, by, w, 32.0);
            let mut want = Bitmap::new(1280, 800);
            {
                let mut gfx = Gfx::new();
                let mut p = Painter::new(&mut want, &mut gfx, 1.0);
                p.fill(Rect::new(0.0, 0.0, 1280.0, 800.0), pal.popup);
                p.frame(r, 6.0, pal.field, pal.line);
                p.text_center(Face::Semibold, 13.0, r, "Write the notes", pal.faint);
            }
            let mut painted = false;
            for y in r.y as u32..r.bottom() as u32 {
                for x in r.x as u32..r.right() as u32 {
                    // The card's footer is a little darker than the popup it is drawn on, so
                    // the comparison is of the button's own edge and label, not its corners.
                    let inside = x > r.x as u32 + 3
                        && x + 3 < r.right() as u32
                        && y > r.y as u32 + 3
                        && y + 3 < r.bottom() as u32;
                    if inside {
                        assert_eq!(pixel(&shot, x, y), pixel(&want, x, y), "({x}, {y})");
                        painted |= pixel(&want, x, y) != pal.popup;
                    }
                }
            }
            assert!(painted, "the button has a fill and a label");
        }
    }

    #[test]
    fn a_name_being_typed_over_a_row_is_drawn() {
        for pal in [LIGHT, DARK] {
            let mut s = screen_of(&japan(), Some(2));
            s.select(2);
            s.start_rename();
            s.edit.as_mut().expect("the field").field.set_text("");
            let empty = render(&mut s, &pal, 1280, 800);
            s.edit
                .as_mut()
                .expect("the field")
                .field
                .set_text("Mmmm typed");
            let typed = render(&mut s, &pal, 1280, 800);
            let g = s.geo();
            let r = s.rows().rect(g.list, 2);
            let field = Rect::new(r.x + 46.0, r.y + 6.0, 300.0, 32.0);
            let mut differing = 0;
            for y in field.y as u32..field.bottom() as u32 {
                for x in field.x as u32..field.right() as u32 {
                    if pixel(&empty, x, y) != pixel(&typed, x, y) {
                        differing += 1;
                    }
                }
            }
            assert!(
                differing > 40,
                "typing drew {differing} pixels in the name field"
            );
            // And the field is a box of its own over the row: its edge is the accent.
            assert_eq!(
                pixel(&typed, field.x as u32 + 40, field.y as u32 + 2).len(),
                3
            );
        }
    }

    #[test]
    fn a_heading_whose_level_changed_has_its_old_hash_struck_through_before_the_new_ones() {
        for pal in [LIGHT, DARK] {
            let mut s = screen_of(&japan(), Some(2));
            let first = s.plan.note_lines(0);
            assert_eq!(
                (first[0].removed, first[0].text.as_str()),
                (1, "# Planning the route\n")
            );
            let shot = render(&mut s, &pal, 1280, 800);
            // What a pixel of the text colour looks like in this bitmap.
            let mut one = Bitmap::new(4, 4);
            {
                let mut gfx = Gfx::new();
                let mut p = Painter::new(&mut one, &mut gfx, 1.0);
                p.fill(Rect::new(0.0, 0.0, 4.0, 4.0), pal.text);
            }
            let ink = pixel(&one, 1, 1);
            let g = s.geo();
            let pv = g.preview.expect("the preview");
            let (x0, y0) = (pv.x as u32 + GUTTER as u32 + 6, g.preview_body.y as u32 + 4);
            // The struck hash is drawn faint, so a run of the text colour across its cell is the
            // line through it; the same cell of a line whose level did not change has none.
            let run = |y: u32| (x0..x0 + 7).filter(|&x| pixel(&shot, x, y) == ink).count();
            let struck = (y0..y0 + LINE_H as u32).map(run).max().unwrap_or(0);
            assert!(struck >= 6, "{struck} px of the strike across the old hash");
            let plain_y0 = y0 + 2 * LINE_H as u32;
            let plain = (plain_y0..plain_y0 + LINE_H as u32)
                .map(run)
                .max()
                .unwrap_or(0);
            assert!(plain < 6, "an unchanged line has no strike ({plain})");
        }
    }

    /// A group can live outside the library folder ("Set the group's folder…", 5.1). The plan
    /// then shows where the notes really go, never `library\` in front of a path that is not in
    /// the library: `library\D:\notes\work\…` names nothing at all.
    #[test]
    fn a_group_outside_the_library_shows_its_own_path_and_the_header_and_rows_agree() {
        let outside = if cfg!(windows) {
            PathBuf::from(r"D:\notes\work")
        } else {
            PathBuf::from("/notes/work")
        };
        let mut s = screen_of(&japan(), Some(2));
        s.set_dest(Dest {
            name: "Work".to_string(),
            file: PathBuf::from("Work.md"),
            home: outside.clone(),
            is_new: false,
        });
        let dest = s.dest_text();
        let mut want = outside.display().to_string();
        want.push(std::path::MAIN_SEPARATOR);
        assert_eq!(dest, want, "the header shows the group's own folder");
        let target = s.target_text(0);
        assert!(
            target.starts_with(&outside.display().to_string()),
            "the row shows the note's own path: {target}"
        );
        for text in [&dest, &target] {
            assert!(
                !text.contains(library::DIR),
                "no library prefix on a path outside it: {text}"
            );
        }
        // A group inside the library still reads the way 5.3 draws it.
        let inside = screen_of(&japan(), Some(2));
        assert!(
            inside.dest_text().starts_with(library::DIR),
            "{}",
            inside.dest_text()
        );
        assert!(
            inside.target_text(0).starts_with(library::DIR),
            "{}",
            inside.target_text(0)
        );
    }

    // ---- the AI (5.2, phase 4b) ----

    /// The plan the answer `json` makes of the Japan journal, on its screen.
    fn ai_screen(json: &str) -> PlanScreen {
        let mut s = screen_of(&japan(), Some(2));
        let answer = split::answer::read(json).expect("the answer reads");
        let (plan, dropped) = split::answer::plan(
            s.plan.document().clone(),
            "Doc.md",
            s.headings(),
            2,
            &answer,
            &[],
        );
        s.set_ai_plan(
            plan,
            AiCut {
                instruction: "one note per part".into(),
                via: "Claude API".into(),
                dropped,
                ..AiCut::default()
            },
        );
        s
    }

    fn spot(s: &PlanScreen, hit: PHit) -> Option<Rect> {
        s.spots.iter().find(|(h, _)| *h == hit).map(|(_, r)| *r)
    }

    fn click(s: &mut PlanScreen, r: Rect) -> Act {
        s.press(r.cx(), r.cy());
        s.release(r.cx(), r.cy())
    }

    #[test]
    fn ask_the_ai_is_offered_only_when_the_application_turns_it_on() {
        let mut off = screen_of(&japan(), Some(2));
        let before = render(&mut off, &LIGHT, 1280, 800);
        assert!(spot(&off, PHit::Ai(AiAct::Ask)).is_none());
        let mut on = screen_of(&japan(), Some(2));
        on.ai_offer = true;
        let after = render(&mut on, &LIGHT, 1280, 800);
        assert_ne!(before.pixels, after.pixels, "the button is drawn");
        let ask = spot(&on, PHit::Ai(AiAct::Ask)).expect("the button");
        assert_eq!(click(&mut on, ask), Act::Ai(AiAct::Ask));
        // The level switch is still there, left of it.
        let hash = spot(&on, PHit::Level(2)).expect("the ### segment");
        assert!(hash.right() < ask.x);
    }

    #[test]
    fn a_plan_from_the_ai_replaces_the_cut_and_escape_keeps_it() {
        // Parts: Planning (line 10), Tokyo (17), the rest; ask for two notes over them.
        let mut s = ai_screen(r#"{"notes":[{"from":10,"to":30,"title":"x"},{"from":40,"to":60}]}"#);
        assert_eq!(s.plan.notes().len(), 2);
        assert!(s.edited, "a model's plan is kept by Escape");
        assert_eq!(s.sel, 0);
        assert_eq!(s.plan.check(), Ok(()));
        render(&mut s, &LIGHT, 1280, 800);
        // The header quotes the instruction and offers the cut at headings back.
        let back = spot(&s, PHit::Ai(AiAct::AtHeadings)).expect("At headings");
        assert_eq!(click(&mut s, back), Act::Ai(AiAct::AtHeadings));
        assert!(s.ai_cut.is_none());
        assert_eq!(s.plan.notes().len(), 23);
        assert_eq!(s.plan.level(), 2);
    }

    #[test]
    fn a_moved_or_overlapping_note_says_so_on_its_row() {
        let text = japan();
        let doc = Text::new(text.as_str());
        // A start in the middle of a paragraph, and a note that overlaps the one before.
        let para = (0..doc.len_lines())
            .find(|&i| {
                i > 20
                    && !doc_line_blank(&doc, i)
                    && !doc_line_blank(&doc, i - 1)
                    && !doc_line_blank(&doc, i + 1)
            })
            .expect("a line inside a paragraph");
        let json = format!(
            r#"{{"notes":[{{"from":{},"to":{}}},{{"from":{},"to":{}}}]}}"#,
            para + 1,
            para + 30,
            para + 20,
            para + 60
        );
        let s = ai_screen(&json);
        let marks: Vec<Option<Mark>> = (0..s.plan.notes().len()).map(|i| s.mark(i)).collect();
        assert_eq!(
            marks,
            [
                Some(Mark::Fixed(Fix::Paragraph)),
                Some(Mark::Fixed(Fix::Overlap))
            ]
        );
        assert_eq!(
            Mark::Fixed(Fix::Overlap).text(),
            "overlapped the note above: starts after it"
        );
        assert_eq!(
            Mark::Fixed(Fix::Paragraph).text(),
            "boundary moved to a paragraph\u{2019}s edge"
        );
        assert_eq!(
            Mark::Fixed(Fix::CodeBlock).text(),
            Mark::Moved.text(),
            "one sentence for one thing"
        );
        assert_eq!(s.marked(), [0, 1], "F7 visits them");
    }

    #[test]
    fn the_notes_dropped_are_the_line_above_the_list() {
        let mut json = String::from(r#"{"notes":["#);
        for k in 0..230 {
            if k > 0 {
                json.push(',');
            }
            json.push_str(&format!(r#"{{"from":{},"to":{}}}"#, k * 2 + 1, k * 2 + 1));
        }
        json.push_str(r#",{"from":"x","to":2}]}"#);
        let s = ai_screen(&json);
        let n = s.notice.clone().expect("a notice");
        assert!(n.warn);
        assert!(
            n.text.starts_with("Dropped ") && n.text.contains("31 past the first 200"),
            "{}",
            n.text
        );
        assert!(n.links.is_empty());
    }

    #[test]
    fn a_notice_the_application_sets_comes_first_and_its_links_act() {
        let mut s = screen_of(&log(), Some(2));
        assert!(s.notice.as_ref().is_some_and(|n| n.warn), "a warning first");
        s.set_ai_notice(Some(Notice {
            text: "The AI\u{2019}s answer could not be read.".into(),
            warn: true,
            show: None,
            links: vec![
                (AiAct::RunAgain, "Run again".into()),
                (AiAct::AtHeadings, "Split at headings instead".into()),
            ],
        }));
        assert_eq!(
            s.notice.as_ref().map(|n| n.text.as_str()),
            Some("The AI\u{2019}s answer could not be read.")
        );
        render(&mut s, &DARK, 1280, 800);
        let again = spot(&s, PHit::Ai(AiAct::RunAgain)).expect("Run again");
        let instead = spot(&s, PHit::Ai(AiAct::AtHeadings)).expect("the other link");
        assert!(again.right() <= instead.x);
        assert_eq!(click(&mut s, again), Act::Ai(AiAct::RunAgain));
        s.set_ai_notice(None);
        assert!(
            s.notice
                .as_ref()
                .is_some_and(|n| n.text.starts_with("One note holds")),
            "the warning is back"
        );
    }

    // ---- the Recycle Bin box (5.4) ----

    /// The Japan journal cut at `##`: lines in no note until the rest is ticked.
    fn with_box(refused: Option<&str>) -> PlanScreen {
        let mut s = screen_of(&japan(), Some(2));
        s.recycle = Some(RecycleBox {
            wanted: false,
            refused: refused.map(str::to_string),
        });
        s
    }

    #[test]
    fn recycle_a_card_without_the_box_is_drawn_as_it_always_was() {
        let mut plain = screen_of(&japan(), Some(2));
        let before = render(&mut plain, &LIGHT, 1280, 800);
        assert!(plain.geo().options.is_none(), "no options row");
        assert!(spot(&plain, PHit::Recycle).is_none());
        assert!(!plain.recycle_on());
        assert_eq!(plain.recycle_why(), None);
        // The list ends where the footer begins, as before phase T.
        let g = plain.geo();
        assert_eq!(g.list.bottom(), g.card.bottom() - DIALOG_FOOTER);
        let mut again = screen_of(&japan(), Some(2));
        assert!(
            render(&mut again, &LIGHT, 1280, 800).pixels == before.pixels,
            "the same card, pixel for pixel"
        );
    }

    #[test]
    fn recycle_the_box_takes_a_row_above_the_footer() {
        let s = with_box(None);
        let g = s.geo();
        let row = g.options.expect("the options row");
        assert_eq!(row.h, OPTIONS_ROW_H);
        assert_eq!(row.bottom(), g.card.bottom() - DIALOG_FOOTER);
        assert_eq!(g.list.bottom(), row.y, "the list gives the row its room");
    }

    #[test]
    fn recycle_is_disabled_by_lines_in_no_note_and_keep_the_rest_answers_it() {
        let mut s = with_box(None);
        let gaps = s.totals.lines_in_no_note;
        assert!(gaps > 0, "the journal has lines between its notes");
        assert_eq!(s.recycle_why(), Some(RecycleWhy::Lines(gaps)));
        assert_eq!(RecycleWhy::Lines(18).text(), "18 lines are in no note.");
        assert_eq!(RecycleWhy::Lines(1).text(), "1 line is in no note.");
        let _ = render(&mut s, &LIGHT, 1280, 800);
        // Disabled: the box is not a place to click, and a click there ticks nothing.
        assert!(spot(&s, PHit::Recycle).is_none());
        s.toggle_recycle();
        assert!(!s.recycle.as_ref().expect("the box").wanted);
        // The one-click answer: the rest is ticked, and the box can be ticked.
        let link = spot(&s, PHit::KeepRest).expect("the answer");
        assert_eq!(click(&mut s, link), Act::Nothing);
        assert!(s.plan.rest_ticked());
        assert_eq!(s.recycle_why(), None);
        let _ = render(&mut s, &LIGHT, 1280, 800);
        assert!(spot(&s, PHit::KeepRest).is_none(), "nothing left to answer");
        let tick = spot(&s, PHit::Recycle).expect("the box");
        click(&mut s, tick);
        assert!(s.recycle_on());
        click(&mut s, tick);
        assert!(!s.recycle_on(), "a second click unticks it");
    }

    #[test]
    fn recycle_is_disabled_by_the_applications_reason_whatever_is_ticked() {
        for why in [
            RECYCLE_NO_BIN.to_string(),
            RECYCLE_ONE_DOCUMENT.to_string(),
            recycle_unavailable("a policy on this PC deletes files instead"),
        ] {
            let mut s = with_box(Some(&why));
            s.keep_the_rest();
            assert!(s.totals.covered());
            assert_eq!(s.recycle_why(), Some(RecycleWhy::Refused(why.clone())));
            let _ = render(&mut s, &LIGHT, 1280, 800);
            assert!(spot(&s, PHit::Recycle).is_none());
            assert!(spot(&s, PHit::KeepRest).is_none(), "no click answers it");
            s.toggle_recycle();
            assert!(!s.recycle_on());
        }
        assert_eq!(
            recycle_unavailable("off"),
            "The Recycle Bin is not available here: off"
        );
    }

    #[test]
    fn recycle_ticked_says_nothing_is_lost_and_the_footer_says_what_happens() {
        let mut s = with_box(None);
        s.keep_the_rest();
        assert!(
            s.footer_sentence()
                .ends_with("The source document is not changed.")
        );
        s.toggle_recycle();
        assert!(s.recycle_on());
        assert_eq!(
            s.footer_sentence(),
            format!(
                "Writes one .md per ticked note into {}, then puts {} in the Recycle Bin.",
                s.dest_text(),
                s.source.name
            )
        );
        // The ticked row and the unticked row differ by the sentence beside the box.
        let ticked = render(&mut s, &LIGHT, 1280, 800);
        let row = s.geo().options.expect("the row");
        s.toggle_recycle();
        let unticked = render(&mut s, &LIGHT, 1280, 800);
        let differing = (row.y as u32 + 4..row.bottom() as u32 - 4)
            .flat_map(|y| (row.x as u32 + 400..row.right() as u32).map(move |x| (x, y)))
            .filter(|&(x, y)| pixel(&ticked, x, y) != pixel(&unticked, x, y))
            .count();
        assert!(differing > 100, "the sentence drew {differing} pixels");
    }

    #[test]
    fn recycle_wanted_and_then_blocked_turns_the_gaps_to_the_warning_colour() {
        for pal in [LIGHT, DARK] {
            let mut s = with_box(None);
            s.keep_the_rest();
            s.toggle_recycle();
            // The owner unticks the rest and a note: their lines are in no note, and the box
            // stands disabled with the wish kept.
            let rest = s.plan.notes().len();
            s.toggle(rest);
            s.toggle(2);
            s.select(0);
            assert!(s.recycle_blocked());
            assert!(
                !s.recycle_on(),
                "nothing goes to the bin while lines are left"
            );
            let warned = render(&mut s, &pal, 1280, 800);
            let band = s
                .bands
                .iter()
                .find(|b| b.row == 2 && b.note)
                .copied()
                .expect("the unticked note's band");
            let g = s.geo();
            let x = (g.strip.x + (band.x0 + band.x1) / 2.0) as u32;
            let y = g.strip.cy() as u32;
            // The bitmap holds its bytes blue first.
            let bgr = |c: [u8; 3]| [c[2], c[1], c[0]];
            let warn = bgr(crate::gfx::mix(pal.popup, pal.warn, 0.75));
            assert_eq!(pixel(&warned, x, y), warn);
            let gap = s
                .bands
                .iter()
                .find(|b| !b.note && b.x1 - b.x0 >= 2.0)
                .copied()
                .expect("a gap between notes");
            let gx = (g.strip.x + (gap.x0 + gap.x1) / 2.0) as u32;
            assert_eq!(
                pixel(&warned, gx, y),
                warn,
                "a gap is the warning colour too"
            );
            // Not wanted: the same lines are the plain track.
            s.recycle.as_mut().expect("the box").wanted = false;
            let calm = render(&mut s, &pal, 1280, 800);
            assert_ne!(pixel(&calm, x, y), warn);
            assert_eq!(pixel(&calm, x, y), bgr(super::draw::strip_colors(&pal).1));
        }
    }
}
