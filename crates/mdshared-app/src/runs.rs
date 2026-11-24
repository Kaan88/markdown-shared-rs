//! A run that writes notes: its record, the order of the write, and undo
//! (`docs/ai-library-plan.md` 4.3, 4.4, 4.5).
//!
//! Nothing here can overwrite, move, rename or delete a file it did not write. A note and a
//! picture are created with [`crate::files::create_new`]: a name that is taken is refused
//! before a byte is written, and the bytes reach the name by a rename that never replaces a file.
//! A folder is only ever created, inside the library folder or the group's folder. The one
//! deletion is undo's, and it goes only to what the record says this run created (`created`: set
//! after the fact, except for a note whose name was free when the run began, which is marked
//! before it is written and put right after; never a file the run found there), only inside the
//! library folder or the home the group's library file declares now, only a regular file with the
//! extension a run writes, only while it is byte for byte the one it wrote, and only when no
//! library file lists it any more.
//!
//! The order is the one of 4.3, and every prefix of it is a state that can be lived with: the
//! record first, then folders, pictures, notes, the read-back, the links, the closed record, and
//! the Recycle Bin last (not built: phase T). Nothing is linked before it exists, and the source
//! document is never touched.

use crate::files::create_new;
use crate::library::{self, Parsed};
use crate::paths::same_path;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The folder of the records, inside the library folder. The reader takes only the `.md` files
/// directly in the library folder, so nothing in here is ever read as a group.
pub const RUNS_DIR: &str = ".runs";

/// The newest records kept, and the days kept when they are more.
pub const KEEP_RUNS: usize = 20;
pub const KEEP_DAYS: u64 = 90;

/// A record bigger than this is not one of ours.
const RECORD_MAX: u64 = 4 * 1024 * 1024;

/// The record format this program writes and reads. A record of a later version is not read, not
/// acted on, and never deleted: it is shown as written by a newer version.
pub const RECORD_VERSION: u32 = 1;

/// The most of one file undo reads to compare it with its recorded digest, whatever the record
/// says about it. A note or a source of more is not checked, so it is not removed and undo is not
/// run against it: a path in a record is not allowed to make the window read without end.
pub const DIGEST_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// The most library files undo reads to find out whether one still lists a note.
const LISTING_MAX_FILES: usize = 4096;

/// The pictures a run copies: the only extensions undo removes besides a note's `.md`.
const PICTURE_EXTENSIONS: [&str; 10] = [
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "avif", "tif", "tiff",
];

// ---- time -----------------------------------------------------------------------------------------

/// Seconds since 1970 and `2026-11-21T14:03:11Z`, in UTC.
pub fn now_utc() -> (u64, String) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    (secs, utc(secs))
}

/// `YYYY-MM-DDTHH:MM:SSZ` for seconds since 1970, UTC.
pub fn utc(secs: u64) -> String {
    let (y, m, d, hh, mm, ss) = civil(secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// The day number and the seconds of the day, as a civil date and time, UTC.
fn civil(secs: u64) -> (i64, u32, u32, u64, u64, u64) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, rem / 3600, rem / 60 % 60, rem % 60)
}

/// The seconds since 1970 of a `YYYY-MM-DDTHH:MM:SSZ` stamp, when it reads as one.
fn secs_of(stamp: &str) -> Option<u64> {
    let b = stamp.as_bytes();
    if b.len() != 20
        || b.get(4) != Some(&b'-')
        || b.get(7) != Some(&b'-')
        || b.get(10) != Some(&b'T')
        || b.get(13) != Some(&b':')
        || b.get(16) != Some(&b':')
        || b.get(19) != Some(&b'Z')
    {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<u64> { stamp.get(from..to)?.parse().ok() };
    let (y, m, d) = (num(0, 4)? as i64, num(5, 7)? as u32, num(8, 10)? as u32);
    let (hh, mm, ss) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    let days = days_from_civil(y, m, d);
    if days < 0 {
        return None;
    }
    Some(days as u64 * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// Days since 1970 of a civil date, UTC.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The name a record has at `secs`: `2026-11-21T14-03-11Z-split`. A colon is not a character
/// Windows allows in a file name, so the clock is written with hyphens. The `started` field
/// inside keeps the ISO stamp.
fn stamp(secs: u64, kind: Kind) -> String {
    let name = utc(secs).replace(':', "-");
    let stem = name.strip_suffix('Z').unwrap_or(&name);
    format!("{stem}Z-{}", kind.as_str())
}

// ---- the record -----------------------------------------------------------------------------------

/// What a run was: a split into notes, or a move of notes that exist (5.12).
///
/// A kind is part of the file format (D-072): a version that has no word for one lists the record
/// and never touches it, so adding one is a change older copies see only as "written by a newer
/// version". [`Kind::Move`] was added after the format's first version and did not need a new
/// `version`: the older readers already treat an unknown kind that way. [`Kind::Markdown`] came
/// the same way (phase 5, `docs/ai-library-plan.md` 5.8): one note whose text a model wrote, from
/// a text file that is never changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Split,
    Move,
    /// A text file rewritten in place (`docs/ai-library-plan.md` 5.6, D-074): the record holds
    /// the original text of every file the run overwrote (D-076).
    Rewrite,

    Markdown,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Split => "split",
            Kind::Move => "move",
            Kind::Rewrite => "rewrite",

            Kind::Markdown => "markdown",
        }
    }

    fn parse(text: &str) -> Option<Kind> {
        match text {
            "split" => Some(Kind::Split),
            "move" => Some(Kind::Move),
            "rewrite" => Some(Kind::Rewrite),

            "markdown" => Some(Kind::Markdown),
            _ => None,
        }
    }
}

/// Where a run is in the order of 4.3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// The record is made; nothing else was done.
    Writing,
    /// The notes are written, but a library file could not be updated.
    LinksPending,
    /// Everything was done and the record was closed.
    Done,
    /// The run was cancelled between files.
    Stopped,
    /// Undo ran.
    Undone,
    /// Undo ran and could not do all of it: a library file kept its lines, or the bin would
    /// not take a note. Undo is offered again; Finish is not, since the owner asked for the run
    /// to go (audit 4, A F2 and F3). A version before this one reads the word as a later
    /// version's and leaves the record alone.
    UndoPending,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Writing => "writing",
            State::LinksPending => "links pending",
            State::Done => "done",
            State::Stopped => "stopped",
            State::Undone => "undone",
            State::UndoPending => "undo pending",
        }
    }

    fn parse(text: &str) -> Option<State> {
        match text {
            "writing" => Some(State::Writing),
            "links pending" => Some(State::LinksPending),
            "done" => Some(State::Done),
            "stopped" => Some(State::Stopped),
            "undone" => Some(State::Undone),
            "undo pending" => Some(State::UndoPending),
            _ => None,
        }
    }

    /// The state whose word is the longest: what a record is measured in when it must still
    /// read back after whatever state it is closed with.
    fn longest() -> State {
        [
            State::Writing,
            State::LinksPending,
            State::Done,
            State::Stopped,
            State::Undone,
            State::UndoPending,
        ]
        .into_iter()
        .max_by_key(|s| s.as_str().len())
        .unwrap_or(State::LinksPending)
    }

    /// A run Finish can take further: one whose notes are written and whose links were not all
    /// written, or that stopped before they were. Not one an undo has begun on.
    pub fn finishable(self) -> bool {
        matches!(self, State::Writing | State::LinksPending | State::Stopped)
    }

    /// A run that can be undone, or finished.
    pub fn live(self) -> bool {
        !matches!(self, State::Undone)
    }
}

/// What was sent to the AI, when anything was. A split at headings sends nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sent {
    pub connection: String,
    pub model: String,
    pub bytes: u64,
    pub requests: u64,
}

/// The document that was split: its path, its length and its digest, so step 8 and undo can tell
/// whether it is still the file that was read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub path: PathBuf,
    pub len: u64,
    pub sha256: String,
    /// Undo refuses while this is true and the file is not back. Nothing sets it before phase T.
    pub recycled: bool,
}

/// The group the notes went to: its name, its library file, whether the run made it, and its
/// folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// The name the owner sees, which is what a new library file's heading holds.
    pub name: String,
    pub file: PathBuf,
    /// In a [`Request`]: the file is to be made. In a [`Record`]: this run made it, which is set
    /// only after the file was created, so a file that was there is never marked.
    pub created: bool,
    pub home: PathBuf,
}

/// One note the record holds: where it went, what it holds, and the line that lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteRec {
    pub path: PathBuf,
    /// This run created the file. False for a note that was there already with the same bytes
    /// (it is linked, and undo takes its line out, but never the file) and for one that was not
    /// written. Set after the write; a record with no such field says false.
    pub created: bool,
    pub sha256: String,
    /// The lines of the source, 1-based and inclusive.
    pub ranges: Vec<(usize, usize)>,
    pub heading_shift: u8,
    pub title_added: bool,
    pub sections: Vec<String>,
    /// The link line as it was written into the library file.
    pub line: String,
}

/// One picture the run copied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureRec {
    pub path: PathBuf,
    /// This run created the file: see [`NoteRec::created`].
    pub created: bool,
    pub sha256: String,
}

/// What a run's links did to one library file: the text it held before them, and the digest of
/// the text they left, so undo can give the file back byte for byte while nothing else changed
/// it since: its blank lines, its line ends and its last line as they were (review of the audit 4
/// fixes, S3). Kept only when the add was exactly the run's links on that text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Added {
    pub file: PathBuf,
    /// The file's text before the run's first add, without a byte order mark.
    pub before: String,
    /// The digest of the text the run's last add left in it, without a byte order mark.
    pub written: String,
}

/// One file a [`Kind::Rewrite`] run overwrote: the original text (D-076), and the digests
/// before and after, so undo can tell the rewritten file from one changed since.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Original {
    pub path: PathBuf,
    /// The file's digest before the run.
    pub sha256: String,
    /// The digest this run left in the file. Undo puts the original back only while the file
    /// still holds this text.
    pub written: String,
    /// The original's length in bytes.
    pub len: u64,
    /// The original began with a UTF-8 byte order mark; undo writes it back with one.
    pub bom: bool,
    /// The original text, as it was read.
    pub text: String,
}

/// One run's record, in memory with absolute paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub version: u32,
    pub kind: Kind,
    pub started: String,
    /// Not written: what `started` means as a number, for the list's age.
    pub started_secs: u64,
    pub state: State,
    pub instruction: String,
    pub sent: Option<Sent>,
    pub source: Source,
    pub group: Group,
    /// The folders this run made, parents first: not the folders its notes use, only the ones
    /// that were not there when it started.
    pub folders: Vec<PathBuf>,
    pub notes: Vec<NoteRec>,
    pub pictures: Vec<PictureRec>,
    /// The section headings the run wrote into a library file, outermost first, so undo can take
    /// them out when nothing is under them.
    pub sections_added: Vec<Vec<String>>,
    /// What a [`Kind::Move`] did: the files it moved, the lines it rewrote and the folders it
    /// removed. `None` for a split, whose record has no such part.
    pub moved: Option<crate::moves::Moved>,
    /// What a [`Kind::Rewrite`] overwrote: each file's original text (D-076). Empty for every
    /// other kind, and for a record written before this field existed.
    pub originals: Vec<Original>,
    /// What the run's links did to each library file ([`Added`]). Empty for a run that wrote
    /// no link, and for a record written before this field existed: undo then takes the lines
    /// out as [`with_removals`] does.
    pub added: Vec<Added>,
}

/// A record read from disk.
#[derive(Clone, Debug)]
pub struct Run {
    pub record: Record,
    pub path: PathBuf,
    pub library: PathBuf,
}

// ---- paths in a record -----------------------------------------------------------------------------

pub(crate) fn stored(path: &Path, library: &Path) -> String {
    stored_on(path, library, cfg!(windows))
}

pub(crate) fn resolved(text: &str, library: &Path) -> PathBuf {
    resolved_on(text, library, cfg!(windows))
}

/// A record is written with `/` between the parts of a path, so a library carried between
/// machines reads the same everywhere. Only Windows separates with `\`: everywhere else a
/// backslash is an ordinary character of a file's name, and turning it into a separator would
/// make the record name a different file. `windows` is the platform, taken as an argument so a
/// test can try both on either (and it is why the record's paths broke on Linux and a Mac).
fn stored_on(path: &Path, library: &Path, windows: bool) -> String {
    let shown = path.strip_prefix(library).unwrap_or(path);
    let text = shown.to_string_lossy();
    if windows {
        text.replace('\\', "/")
    } else {
        text.into_owned()
    }
}

/// What [`stored_on`] wrote, read back on the platform that is reading it.
fn resolved_on(text: &str, library: &Path, windows: bool) -> PathBuf {
    let p = if windows {
        PathBuf::from(text.replace('/', "\\"))
    } else {
        PathBuf::from(text)
    };
    if p.is_absolute() || library.as_os_str().is_empty() {
        p
    } else {
        library.join(p)
    }
}

// ---- JSON ------------------------------------------------------------------------------------------

/// A small JSON value, enough for a record and no more. Nothing here recurses without a depth
/// bound, so a hostile file cannot exhaust the stack; the release build aborts on a panic, so
/// every index is checked.
pub(crate) enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub(crate) fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(crate) fn str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn num(&self) -> Option<f64> {
        match self {
            Json::Num(n) if n.is_finite() => Some(*n),
            _ => None,
        }
    }

    pub(crate) fn uint(&self) -> Option<u64> {
        let n = self.num()?;
        (n >= 0.0 && n <= u64::MAX as f64).then_some(n as u64)
    }

    pub(crate) fn usize(&self) -> Option<usize> {
        let n = self.uint()?;
        usize::try_from(n).ok()
    }

    pub(crate) fn bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub(crate) fn arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }
}

const JSON_DEPTH: u8 = 32;

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Reader<'a> {
        Reader {
            b: text.as_bytes(),
            at: 0,
        }
    }

    fn skip(&mut self) {
        while let Some(&c) = self.b.get(self.at) {
            if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' {
                self.at += 1;
            } else {
                break;
            }
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.skip();
        if self.b.get(self.at) == Some(&c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn word(&mut self, word: &str) -> bool {
        self.skip();
        let end = self.at.saturating_add(word.len());
        if self.b.get(self.at..end) == Some(word.as_bytes()) {
            self.at = end;
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: u8) -> Option<Json> {
        if depth == 0 {
            return None;
        }
        self.skip();
        match *self.b.get(self.at)? {
            b'n' => self.word("null").then_some(Json::Null),
            b't' => self.word("true").then_some(Json::Bool(true)),
            b'f' => self.word("false").then_some(Json::Bool(false)),
            b'"' => self.string().map(Json::Str),
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                self.skip();
                if self.eat(b']') {
                    return Some(Json::Arr(items));
                }
                loop {
                    items.push(self.value(depth - 1)?);
                    if self.eat(b']') {
                        return Some(Json::Arr(items));
                    }
                    if !self.eat(b',') {
                        return None;
                    }
                }
            }
            b'{' => {
                self.at += 1;
                let mut items: Vec<(String, Json)> = Vec::new();
                self.skip();
                if self.eat(b'}') {
                    return Some(Json::Obj(items));
                }
                loop {
                    self.skip();
                    if self.b.get(self.at) != Some(&b'"') {
                        return None;
                    }
                    let key = self.string()?;
                    if !self.eat(b':') {
                        return None;
                    }
                    let value = self.value(depth - 1)?;
                    items.push((key, value));
                    if self.eat(b'}') {
                        return Some(Json::Obj(items));
                    }
                    if !self.eat(b',') {
                        return None;
                    }
                }
            }
            _ => self.number(),
        }
    }

    fn number(&mut self) -> Option<Json> {
        self.skip();
        let start = self.at;
        if self.b.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        let mut digits = false;
        while self.b.get(self.at).is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
            digits = true;
        }
        if self.b.get(self.at) == Some(&b'.') {
            self.at += 1;
            while self.b.get(self.at).is_some_and(|c| c.is_ascii_digit()) {
                self.at += 1;
            }
        }
        if matches!(self.b.get(self.at), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.b.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            while self.b.get(self.at).is_some_and(|c| c.is_ascii_digit()) {
                self.at += 1;
            }
        }
        if !digits {
            return None;
        }
        let text = std::str::from_utf8(self.b.get(start..self.at)?).ok()?;
        text.parse().ok().map(Json::Num)
    }

    fn string(&mut self) -> Option<String> {
        if self.b.get(self.at) != Some(&b'"') {
            return None;
        }
        self.at += 1;
        let mut out = String::new();
        loop {
            let c = *self.b.get(self.at)?;
            self.at += 1;
            match c {
                b'"' => return Some(out),
                b'\\' => {
                    let e = *self.b.get(self.at)?;
                    self.at += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let ch = if (0xD800..0xDC00).contains(&hi) {
                                // A surrogate pair, when the second half is there.
                                let save = self.at;
                                if self.b.get(self.at) == Some(&b'\\')
                                    && self.b.get(self.at + 1) == Some(&b'u')
                                {
                                    self.at += 2;
                                    match self.hex4() {
                                        Some(lo) if (0xDC00..0xE000).contains(&lo) => {
                                            char::from_u32(
                                                0x1_0000 + ((hi - 0xD800) << 10) + (lo - 0xDC00),
                                            )
                                            .unwrap_or('\u{FFFD}')
                                        }
                                        _ => {
                                            self.at = save;
                                            '\u{FFFD}'
                                        }
                                    }
                                } else {
                                    '\u{FFFD}'
                                }
                            } else {
                                char::from_u32(hi).unwrap_or('\u{FFFD}')
                            };
                            out.push(ch);
                        }
                        _ => return None,
                    }
                }
                c if c < 0x20 => return None,
                c if c < 0x80 => out.push(c as char),
                _ => {
                    // A UTF-8 character's bytes, from its first byte's length.
                    let len = if c < 0xE0 {
                        2
                    } else if c < 0xF0 {
                        3
                    } else {
                        4
                    };
                    let from = self.at - 1;
                    let end = from + len;
                    let bytes = self.b.get(from..end)?;
                    out.push_str(std::str::from_utf8(bytes).ok()?);
                    self.at = end;
                }
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let mut n = 0u32;
        for _ in 0..4 {
            let c = *self.b.get(self.at)?;
            self.at += 1;
            n = n * 16 + (c as char).to_digit(16)?;
        }
        Some(n)
    }
}

pub(crate) fn parse_json(text: &str) -> Option<Json> {
    let mut r = Reader::new(text);
    let value = r.value(JSON_DEPTH)?;
    r.skip();
    Some(value)
}

pub(crate) fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// ---- the record's files ----------------------------------------------------------------------------

impl Record {
    /// The JSON this record is written as, with the library folder's paths relative to it.
    pub fn to_json(&self, library: &Path) -> String {
        if self.kind == Kind::Move {
            return crate::moves::record_json(self, library);
        }
        let mut s = String::new();
        s.push_str("{\n");
        s.push_str(&format!("  \"version\": {},\n", self.version));
        s.push_str(&format!("  \"kind\": \"{}\",\n", esc(self.kind.as_str())));
        s.push_str(&format!("  \"started\": \"{}\",\n", esc(&self.started)));
        s.push_str(&format!("  \"state\": \"{}\",\n", esc(self.state.as_str())));
        s.push_str(&format!(
            "  \"instruction\": \"{}\",\n",
            esc(&self.instruction)
        ));
        match &self.sent {
            Some(sent) => s.push_str(&format!(
                "  \"sent\": {{\"connection\": \"{}\", \"model\": \"{}\", \"bytes\": {}, \"requests\": {}}},\n",
                esc(&sent.connection),
                esc(&sent.model),
                sent.bytes,
                sent.requests
            )),
            None => s.push_str("  \"sent\": null,\n"),
        }
        s.push_str(&format!(
            "  \"source\": {{\"path\": \"{}\", \"len\": {}, \"sha256\": \"{}\", \"recycled\": {}}},\n",
            esc(&stored(&self.source.path, library)),
            self.source.len,
            esc(&self.source.sha256),
            self.source.recycled
        ));
        s.push_str(&format!(
            "  \"group\": {{\"name\": \"{}\", \"file\": \"{}\", \"created\": {}, \"home\": \"{}\"}},\n",
            esc(&self.group.name),
            esc(&stored(&self.group.file, library)),
            self.group.created,
            esc(&stored(&self.group.home, library))
        ));
        let folders: Vec<String> = self
            .folders
            .iter()
            .map(|f| format!("\"{}\"", esc(&stored(f, library))))
            .collect();
        s.push_str(&format!("  \"folders\": [{}],\n", folders.join(", ")));
        s.push_str("  \"notes\": [");
        for (i, n) in self.notes.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            let ranges: Vec<String> = n
                .ranges
                .iter()
                .map(|(a, b)| format!("[{a}, {b}]"))
                .collect();
            let sections: Vec<String> = n
                .sections
                .iter()
                .map(|x| format!("\"{}\"", esc(x)))
                .collect();
            s.push_str(&format!(
                "\n    {{\"path\": \"{}\", \"created\": {}, \"sha256\": \"{}\", \"ranges\": [{}], \"heading_shift\": {}, \"title_added\": {}, \"sections\": [{}], \"line\": \"{}\"}}",
                esc(&stored(&n.path, library)),
                n.created,
                esc(&n.sha256),
                ranges.join(", "),
                n.heading_shift,
                n.title_added,
                sections.join(", "),
                esc(&n.line)
            ));
        }
        if !self.notes.is_empty() {
            s.push_str("\n  ");
        }
        s.push_str("],\n");
        let headings: Vec<String> = self
            .sections_added
            .iter()
            .map(|path| {
                let names: Vec<String> = path.iter().map(|x| format!("\"{}\"", esc(x))).collect();
                format!("[{}]", names.join(", "))
            })
            .collect();
        s.push_str(&format!(
            "  \"sections_added\": [{}],\n",
            headings.join(", ")
        ));
        let pictures: Vec<String> = self
            .pictures
            .iter()
            .map(|p| {
                format!(
                    "{{\"path\": \"{}\", \"created\": {}, \"sha256\": \"{}\"}}",
                    esc(&stored(&p.path, library)),
                    p.created,
                    esc(&p.sha256)
                )
            })
            .collect();
        s.push_str(&format!("  \"pictures\": [{}],\n", pictures.join(", ")));
        let added: Vec<String> = self
            .added
            .iter()
            .map(|a| {
                format!(
                    "{{\"file\": \"{}\", \"before\": \"{}\", \"written\": \"{}\"}}",
                    esc(&stored(&a.file, library)),
                    esc(&a.before),
                    esc(&a.written)
                )
            })
            .collect();
        s.push_str(&format!("  \"added\": [{}],\n", added.join(", ")));
        let originals: Vec<String> = self
            .originals
            .iter()
            .map(|o| {
                format!(
                    "{{\"path\": \"{}\", \"sha256\": \"{}\", \"written\": \"{}\", \"len\": {}, \"bom\": {}, \"text\": \"{}\"}}",
                    esc(&stored(&o.path, library)),
                    esc(&o.sha256),
                    esc(&o.written),
                    o.len,
                    o.bom,
                    esc(&o.text)
                )
            })
            .collect();
        s.push_str(&format!("  \"originals\": [{}]\n", originals.join(", ")));
        s.push_str("}\n");
        s
    }

    /// The record a JSON text holds, its paths resolved against `library`; `None` when it is not
    /// a record this program wrote.
    pub fn from_json(text: &str, library: &Path) -> Option<Record> {
        let json = parse_json(text)?;
        let version = u32::try_from(json.get("version")?.uint()?).ok()?;
        if version > RECORD_VERSION {
            return None;
        }
        let kind = Kind::parse(json.get("kind")?.str()?)?;
        let started = json.get("started")?.str()?.to_string();
        let started_secs = secs_of(&started).unwrap_or(0);
        let state = State::parse(json.get("state")?.str()?)?;
        let instruction = json
            .get("instruction")
            .and_then(Json::str)
            .unwrap_or_default()
            .to_string();
        if kind == Kind::Move {
            return crate::moves::record_from_json(
                &json,
                library,
                (version, started, started_secs, state, instruction),
            );
        }
        let sent = json.get("sent").and_then(|s| {
            Some(Sent {
                connection: s.get("connection")?.str()?.to_string(),
                model: s
                    .get("model")
                    .and_then(Json::str)
                    .unwrap_or_default()
                    .to_string(),
                bytes: s.get("bytes").and_then(Json::uint).unwrap_or(0),
                requests: s.get("requests").and_then(Json::uint).unwrap_or(0),
            })
        });
        let src = json.get("source")?;
        let source = Source {
            path: resolved(src.get("path")?.str()?, library),
            len: src.get("len")?.uint()?,
            sha256: src
                .get("sha256")
                .and_then(Json::str)
                .unwrap_or_default()
                .to_string(),
            recycled: src.get("recycled").and_then(Json::bool).unwrap_or(false),
        };
        let g = json.get("group")?;
        let file = resolved(g.get("file")?.str()?, library);
        let group = Group {
            name: g
                .get("name")
                .and_then(Json::str)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    file.file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default()
                }),
            file,
            created: g.get("created").and_then(Json::bool).unwrap_or(false),
            home: resolved(
                g.get("home").and_then(Json::str).unwrap_or_default(),
                library,
            ),
        };
        let folders = json
            .get("folders")
            .and_then(Json::arr)
            .map(|a| {
                a.iter()
                    .filter_map(Json::str)
                    .map(|s| resolved(s, library))
                    .collect()
            })
            .unwrap_or_default();
        let notes = json
            .get("notes")
            .and_then(Json::arr)
            .map(|a| {
                a.iter()
                    .filter_map(|n| {
                        let path = resolved(n.get("path")?.str()?, library);
                        let ranges = n
                            .get("ranges")
                            .and_then(Json::arr)
                            .map(|r| {
                                r.iter()
                                    .filter_map(|pair| {
                                        let pair = pair.arr()?;
                                        let a = pair.first()?.usize()?;
                                        let b = pair.get(1)?.usize()?;
                                        (a >= 1 && a <= b).then_some((a, b))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let sections = n
                            .get("sections")
                            .and_then(Json::arr)
                            .map(|s| s.iter().filter_map(Json::str).map(str::to_string).collect())
                            .unwrap_or_default();
                        Some(NoteRec {
                            path,
                            created: n.get("created").and_then(Json::bool).unwrap_or(false),
                            sha256: n
                                .get("sha256")
                                .and_then(Json::str)
                                .unwrap_or_default()
                                .to_string(),
                            ranges,
                            heading_shift: n
                                .get("heading_shift")
                                .and_then(Json::uint)
                                .unwrap_or(0)
                                .min(8) as u8,
                            title_added: n.get("title_added").and_then(Json::bool).unwrap_or(false),
                            sections,
                            line: n
                                .get("line")
                                .and_then(Json::str)
                                .unwrap_or_default()
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let sections_added = json
            .get("sections_added")
            .and_then(Json::arr)
            .map(|a| {
                a.iter()
                    .filter_map(|path| {
                        let names: Vec<String> = path
                            .arr()?
                            .iter()
                            .filter_map(Json::str)
                            .map(str::to_string)
                            .collect();
                        (!names.is_empty()).then_some(names)
                    })
                    .take(64)
                    .collect()
            })
            .unwrap_or_default();
        let pictures = json
            .get("pictures")
            .and_then(Json::arr)
            .map(|a| {
                a.iter()
                    .filter_map(|p| {
                        Some(PictureRec {
                            path: resolved(p.get("path")?.str()?, library),
                            created: p.get("created").and_then(Json::bool).unwrap_or(false),
                            sha256: p
                                .get("sha256")
                                .and_then(Json::str)
                                .unwrap_or_default()
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let originals = json
            .get("originals")
            .and_then(Json::arr)
            .map(|a| {
                a.iter()
                    .filter_map(|o| {
                        let text = o.get("text")?.str()?.to_string();
                        Some(Original {
                            path: resolved(o.get("path")?.str()?, library),
                            sha256: o
                                .get("sha256")
                                .and_then(Json::str)
                                .unwrap_or_default()
                                .to_string(),
                            written: o
                                .get("written")
                                .and_then(Json::str)
                                .unwrap_or_default()
                                .to_string(),
                            len: o.get("len").and_then(Json::uint).unwrap_or(0),
                            bom: o.get("bom").and_then(Json::bool).unwrap_or(false),
                            text,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let added = json
            .get("added")
            .and_then(Json::arr)
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        Some(Added {
                            file: resolved(x.get("file")?.str()?, library),
                            before: x.get("before")?.str()?.to_string(),
                            written: x.get("written")?.str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Record {
            version,
            kind,
            started,
            started_secs,
            state,
            instruction,
            sent,
            source,
            group,
            folders,
            notes,
            pictures,
            sections_added,
            moved: None,
            originals,
            added,
        })
    }

    /// Keeps what an add of `links` did to `file`, read before it (`before`) and now: only when
    /// the file now holds exactly those links added to that text, so the record never holds a
    /// text the run did not start from. A second add to a file that still held what the first
    /// left keeps the first text, so undo goes back to before the run's first add. Nothing is
    /// kept when the record would no longer read back with it ([`RECORD_MAX`]).
    fn keep_added(&mut self, file: &Path, before: Option<String>, links: &[Link], library: &Path) {
        let Some(before) = before else {
            return;
        };
        let Some(now) = library_text_of(file) else {
            return;
        };
        if with_links_added(&before, links) != now {
            return;
        }
        let written = crate::update::sha256_hex(now.as_bytes());
        let was = crate::update::sha256_hex(before.as_bytes());
        let at = self.added.iter().position(|a| same_path(&a.file, file));
        let entry = match at.and_then(|i| self.added.get(i)) {
            Some(a) if a.written.eq_ignore_ascii_case(&was) => Added {
                file: file.to_path_buf(),
                before: a.before.clone(),
                written,
            },
            _ => Added {
                file: file.to_path_buf(),
                before,
                written,
            },
        };
        self.added.retain(|a| !same_path(&a.file, file));
        let mut probe = self.clone();
        probe.added.push(entry.clone());
        if reads_back(&mut probe, library) {
            self.added.push(entry);
        }
    }

    /// Writes the record as a new file in `<library>/.runs`, and gives its path. A name that is
    /// taken is another run's record: the next number is used, and no file is ever replaced.
    pub fn create(&self, library: &Path) -> io::Result<PathBuf> {
        let dir = library.join(RUNS_DIR);
        std::fs::create_dir_all(&dir)?;
        let stem = stamp(self.started_secs, self.kind);
        let text = self.to_json(library);
        for n in 1..=99 {
            let name = if n == 1 {
                format!("{stem}.json")
            } else {
                format!("{stem}-{n}.json")
            };
            let path = dir.join(name);
            match create_new(&path, text.as_bytes()) {
                Ok(()) => return Ok(path),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "the runs folder holds that name already",
        ))
    }

    /// Writes the record again in place. The file is the run's own, named when step 1 made it,
    /// so this replaces it with the same run's newer state.
    pub fn close(&self, path: &Path, library: &Path) -> io::Result<()> {
        crate::settings::write_atomic(path, &self.to_json(library))
    }

    /// Appends one line to the `<same name>.done` beside the record.
    pub fn note_done(&self, path: &Path, line: &str) {
        let done = path.with_extension("done");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&done)
        {
            let _ = writeln!(file, "{} {line}", utc(now_utc().0));
        }
    }

    /// The group's name as the list shows it: its library file's stem.
    pub fn group_name(&self) -> String {
        self.group
            .file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The source's name.
    pub fn source_name(&self) -> String {
        self.source
            .path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Whether this is a rewrite's record (5.6, D-074).
    pub fn is_rewrite(&self) -> bool {
        self.kind == Kind::Rewrite
    }
}

/// A text file's before and after text, and whether its original began with a UTF-8 byte order
/// mark: one file of a rewrite run (5.6, D-074).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewriteFile {
    pub path: PathBuf,
    /// The text the file held when the review was made.
    pub before: String,
    /// The text to write: what the owner accepted.
    pub after: String,
    pub bom: bool,
}

/// Starts a rewrite run's record, before its first file is touched. The record goes in the
/// library's `.runs` (D-076); its `source` is the first file, and its originals grow as files
/// are written.
pub fn start_rewrite(
    library: &Path,
    instruction: &str,
    sent: Option<Sent>,
    first: &Path,
) -> io::Result<(PathBuf, Record)> {
    let bytes = read_regular(first, DIGEST_MAX_BYTES, false).unwrap_or_default();
    let (secs, started) = now_utc();
    let record = Record {
        version: RECORD_VERSION,
        kind: Kind::Rewrite,
        started,
        started_secs: secs,
        state: State::Writing,
        instruction: instruction.to_string(),
        sent,
        source: Source {
            path: first.to_path_buf(),
            len: bytes.len() as u64,
            sha256: crate::update::sha256_hex(&bytes),
            recycled: false,
        },
        group: Group {
            name: String::new(),
            file: PathBuf::new(),
            created: false,
            home: PathBuf::new(),
        },
        folders: Vec::new(),
        notes: Vec::new(),
        pictures: Vec::new(),
        sections_added: Vec::new(),
        moved: None,
        originals: Vec::new(),
        added: Vec::new(),
    };
    let path = record.create(library)?;
    Ok((path, record))
}

/// Writes one file of a rewrite run (`file.after`) and keeps `file.before` in the record first
/// (D-074, D-076): the record is written before the file, so a crash between them leaves the
/// original where undo can find it.
///
/// The file is rewritten only while it still holds `file.before` to the byte: a file changed
/// since the review is refused, not overwritten. Only UTF-8 text is taken, because the record
/// holds the original as text; a file with a byte order mark keeps it. `Error` says why.
pub fn rewrite_into(
    record_path: &Path,
    record: &mut Record,
    library: &Path,
    file: &RewriteFile,
) -> Result<(), String> {
    if record.kind != Kind::Rewrite {
        return Err("This record is not a rewrite's.".to_string());
    }
    let bytes = read_regular(&file.path, DIGEST_MAX_BYTES, false)
        .ok_or_else(|| "The file could not be read. It was not rewritten.".to_string())?;
    let (bom, body) = match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(rest) => (true, rest),
        None => (false, &bytes[..]),
    };
    let now = std::str::from_utf8(body)
        .map_err(|_| "The file is not UTF-8 text. It was not rewritten.".to_string())?;
    if bom != file.bom || now != file.before {
        return Err(
            "The file changed since it was read. Nothing was taken from the review.".to_string(),
        );
    }
    // A record this program could not read back would keep the original where no undo finds
    // it (audit 4, B2 and D2): it is refused before the file is touched.
    if !rewrite_fits(record, library, file) {
        return Err(RECORD_FULL.to_string());
    }
    let after = encoded(&file.after, file.bom);
    record.originals.push(original_of(file));
    // Whatever fails after this point takes the original out of the record again, in memory and
    // on disk, so the record never names a file it did not rewrite (audit 4, B6).
    if let Err(e) = record.close(record_path, library) {
        record.originals.pop();
        return Err(format!(
            "The record could not be written, so nothing was: {e}"
        ));
    }
    if let Err(e) = crate::files::replace(&file.path, &after) {
        record.originals.pop();
        let _ = record.close(record_path, library);
        return Err(format!("The file could not be written: {e}"));
    }
    Ok(())
}

/// What [`rewrite_into`] says when the record has no room for one more original: the caller
/// closes this record and starts another for the file.
pub const RECORD_FULL: &str =
    "The run's record has no room for this file's original. Nothing was written.";

/// Whether the record, with `file`'s original added, is still one this program reads back
/// ([`RECORD_MAX`]). A rewrite run whose next file does not fit is given a new record
/// ([`start_rewrite`]) for it, so that every original stays where undo can find it.
///
/// It is measured in the longest state the record can be closed with, not the one it has now: a
/// record that fits as "writing" and passes the cap as "undo pending" would lose its undo the
/// moment an undo was refused (review of the audit 4 fixes, S1).
pub fn rewrite_fits(record: &Record, library: &Path, file: &RewriteFile) -> bool {
    let mut with = record.clone();
    with.originals.push(original_of(file));
    reads_back(&mut with, library)
}

/// Whether `record` is one this program reads back ([`RECORD_MAX`]) in whatever state it is
/// closed with. Its state is left as the longest.
fn reads_back(record: &mut Record, library: &Path) -> bool {
    record.state = State::longest();
    record.to_json(library).len() as u64 <= RECORD_MAX
}

/// The record's entry for one file of a rewrite: its original text, and the digests before and
/// after.
fn original_of(file: &RewriteFile) -> Original {
    let before = encoded(&file.before, file.bom);
    let after = encoded(&file.after, file.bom);
    Original {
        path: file.path.clone(),
        sha256: crate::update::sha256_hex(&before),
        written: crate::update::sha256_hex(&after),
        len: before.len() as u64,
        bom: file.bom,
        text: file.before.clone(),
    }
}

/// A text as the bytes a file holds: a byte order mark when it had one, then UTF-8.
fn encoded(text: &str, bom: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 3);
    if bom {
        out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    out.extend_from_slice(text.as_bytes());
    out
}

/// Reads one record. `None` when the file is not a record of ours; nothing here can panic.
pub fn load(path: &Path, library: &Path) -> Option<Record> {
    inspect(path, library).ok()
}

/// A record file this version could not use: another version wrote it, or it is not a record.
/// It is listed and never acted on, and never deleted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unread {
    pub path: PathBuf,
    /// A later version of this program wrote it: a version this one does not know, or a kind or
    /// a state it has no word for.
    pub newer: bool,
    /// When it was written, from its name (`2026-11-21T14:03:11Z`), or empty.
    pub started: String,
}

/// Reads one record; `Err(newer)` when it cannot be used, saying whether a later version wrote it.
fn inspect(path: &Path, library: &Path) -> Result<Record, bool> {
    let meta = std::fs::metadata(path).map_err(|_| false)?;
    if !meta.is_file() || meta.len() > RECORD_MAX {
        return Err(false);
    }
    let text = std::fs::read_to_string(path).map_err(|_| false)?;
    match Record::from_json(&text, library) {
        Some(record) => Ok(record),
        None => Err(written_by_newer(&text)),
    }
}

/// Whether a text that is not a record of this version is one of a later version: valid JSON
/// whose version is past [`RECORD_VERSION`], or whose kind or state this version has no word for.
fn written_by_newer(text: &str) -> bool {
    let Some(json) = parse_json(text) else {
        return false;
    };
    let version = json.get("version").and_then(Json::uint);
    version.is_some_and(|v| v > u64::from(RECORD_VERSION))
        || (version.is_some()
            && (json
                .get("kind")
                .and_then(Json::str)
                .is_some_and(|k| Kind::parse(k).is_none())
                || json
                    .get("state")
                    .and_then(Json::str)
                    .is_some_and(|s| State::parse(s).is_none())))
}

/// The time a record's file name holds: `2026-11-21T14-03-11Z-split.json`, as an ISO stamp.
fn name_stamp(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parts = (|| {
        let head = stem.get(..20)?;
        Some(format!(
            "{}:{}:{}",
            head.get(..13)?,
            head.get(14..16)?,
            head.get(17..)?
        ))
    })();
    parts
        .filter(|iso| secs_of(iso).is_some())
        .unwrap_or_default()
}

/// Every record in `library/.runs`, newest first. The folder is read only when this is asked
/// for: never at start.
pub fn list(library: &Path) -> Vec<Run> {
    list_all(library).0
}

/// [`list`], and with it the record files this version cannot use, newest first by the time in
/// their names, so a window can say that they are there.
pub fn list_all(library: &Path) -> (Vec<Run>, Vec<Unread>) {
    let dir = library.join(RUNS_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return (Vec::new(), Vec::new());
    };
    let mut runs: Vec<Run> = Vec::new();
    let mut unread: Vec<Unread> = Vec::new();
    for path in entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("json"))
        })
    {
        match inspect(&path, library) {
            Ok(record) => runs.push(Run {
                record,
                path,
                library: library.to_path_buf(),
            }),
            Err(newer) => unread.push(Unread {
                started: name_stamp(&path),
                path,
                newer,
            }),
        }
    }
    runs.sort_by(|a, b| b.record.started.cmp(&a.record.started));
    unread.sort_by(|a, b| b.started.cmp(&a.started).then_with(|| a.path.cmp(&b.path)));
    (runs, unread)
}

/// Deletes the records older than the newest [`KEEP_RUNS`] and older than [`KEEP_DAYS`]. Their
/// notes and their `.done` files beside them go only when the record goes. A file that does not
/// load as a record of this version is not counted among the newest and is never deleted: it may
/// be the record of a later version, and its undo with it.
pub fn prune(library: &Path, now: u64) {
    let dir = library.join(RUNS_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut all: Vec<(u64, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("json"))
        })
        .filter_map(|p| {
            let record = load(&p, library)?;
            Some((record.started_secs, p))
        })
        .collect();
    all.sort_by_key(|(secs, _)| std::cmp::Reverse(*secs));
    for (i, (secs, path)) in all.into_iter().enumerate() {
        let old = i >= KEEP_RUNS && (now.saturating_sub(secs)) > KEEP_DAYS * 86_400;
        if old {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(path.with_extension("done"));
        }
    }
}

// ---- what to write --------------------------------------------------------------------------------

/// One note the run writes: the exact bytes, where they go, and the line that lists it.
#[derive(Clone, Debug)]
pub struct NewNote {
    pub path: PathBuf,
    pub text: String,
    /// The same note as the plan made it from the source's lines. Step 5 compares the file with
    /// this: in the program it is the same text as `text`; a writer that wrote anything else is
    /// caught here.
    pub from_source: String,
    pub title: String,
    /// The note's path as the library file links it: relative to the library when it is inside.
    pub link_path: PathBuf,
    pub sections: Vec<String>,
    pub ranges: Vec<(usize, usize)>,
    pub heading_shift: u8,
    pub title_added: bool,
    /// The file was there with these bytes when the plan was made (3.9).
    pub already: bool,
}

/// One picture the run copies.
#[derive(Clone, Debug)]
pub struct NewPicture {
    pub source: PathBuf,
    /// The folder it goes in; its name is made from the source's own name.
    pub folder: PathBuf,
}

/// Everything a write does, worked out before step 1.
#[derive(Clone, Debug)]
pub struct Request {
    pub library: PathBuf,
    pub source: PathBuf,
    pub instruction: String,
    pub sent: Option<Sent>,
    pub group: Group,
    /// The folders to make, parents first, all of them inside the library or the group's folder.
    pub folders: Vec<PathBuf>,
    pub notes: Vec<NewNote>,
    pub pictures: Vec<NewPicture>,
}

/// A link as the library file holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub path: PathBuf,
    pub title: String,
    pub sections: Vec<String>,
    pub line: String,
}

impl Link {
    pub fn new(note: &NewNote) -> Link {
        Link {
            path: note.link_path.clone(),
            title: note.title.clone(),
            sections: note.sections.clone(),
            line: library::link_line(&note.title, &note.link_path),
        }
    }
}

/// A link undo takes out: the line as it was recorded, so a line the owner edited is kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkRemoval {
    pub path: PathBuf,
    pub sections: Vec<String>,
    pub line: String,
}

/// What the notes and the library write through: the window's own gate, which refuses a library
/// file that is open with unsaved changes, re-reads it, checks the result and writes it at once.
pub trait Gate {
    /// Adds every link of one library file in one atomic write.
    fn add_links(&mut self, file: &Path, links: &[Link]) -> Result<(), String>;

    /// Takes every removal out of one library file in one atomic write, and then the section
    /// headings the run wrote that have nothing under them.
    fn remove_links(
        &mut self,
        file: &Path,
        links: &[LinkRemoval],
        headings: &[Vec<String>],
    ) -> Result<(), String>;

    /// [`Gate::remove_links`] for a library file whose add the record holds (`added`). A gate
    /// that composes with [`with_removals_added`] gives the file back byte for byte while it
    /// still holds what the add wrote (review of the audit 4 fixes, S3); one that does not takes
    /// the lines out as `remove_links` does.
    fn remove_links_added(
        &mut self,
        file: &Path,
        links: &[LinkRemoval],
        headings: &[Vec<String>],
        added: &Added,
    ) -> Result<(), String> {
        let _ = added;
        self.remove_links(file, links, headings)
    }
}

/// Why one file was left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub path: PathBuf,
    pub why: String,
}

impl Failure {
    fn new(path: &Path, why: impl Into<String>) -> Failure {
        Failure {
            path: path.to_path_buf(),
            why: why.into(),
        }
    }
}

/// What a write did.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub record: Option<Record>,
    pub record_path: Option<PathBuf>,
    /// Step 1 failed: nothing at all was done.
    pub start_error: Option<String>,
    pub notes_written: Vec<PathBuf>,
    pub notes_present: Vec<PathBuf>,
    pub pictures_written: Vec<PathBuf>,
    pub note_failures: Vec<Failure>,
    pub picture_failures: Vec<Failure>,
    pub check_failures: Vec<Failure>,
    pub folder_failures: Vec<Failure>,
    /// The group's library file could not be made.
    pub group_failure: Option<Failure>,
    /// The library files that could not be updated, and why.
    pub link_failures: Vec<Failure>,
    /// The library files that were.
    pub linked: Vec<PathBuf>,
    /// Step 7 was reached.
    pub closed: bool,
    pub close_error: Option<String>,
}

impl Report {
    /// How many notes are on disk: written now, or there already.
    pub fn notes(&self) -> usize {
        self.notes_written.len() + self.notes_present.len()
    }

    /// Whether the run counts as done, even with a file or two left out.
    pub fn state(&self) -> Option<State> {
        self.record.as_ref().map(|r| r.state)
    }
}

// ---- the write -----------------------------------------------------------------------------------

/// Splits `req` into its files, in the order of 4.3, with the clock read now.
pub fn write(req: &Request, gate: &mut dyn Gate) -> Report {
    let (secs, started) = now_utc();
    write_at(req, gate, secs, &started)
}

/// [`write`] with the clock given, so a test can know the record's name.
pub fn write_at(req: &Request, gate: &mut dyn Gate, secs: u64, started: &str) -> Report {
    write_of(req, Kind::Split, gate, secs, started)
}

/// A text file's Markdown note (5.8): the same order of 4.3, with [`Kind::Markdown`] in the
/// record. Its notes hold the text a model wrote, not line ranges of the source, so the caller
/// composes each note and passes the same text as [`NewNote::from_source`]; the source is read for
/// its digest and never written. Undo takes the note and its line out as it does for a split.
pub fn write_note(req: &Request, gate: &mut dyn Gate) -> Report {
    let (secs, started) = now_utc();
    write_of(req, Kind::Markdown, gate, secs, &started)
}

fn write_of(req: &Request, kind: Kind, gate: &mut dyn Gate, secs: u64, started: &str) -> Report {
    let mut report = Report::default();
    // The source as it is now: its length and digest go in the record, and step 8 compares the
    // file with them before it would move it anywhere.
    let source = match std::fs::read(&req.source) {
        Ok(bytes) => Source {
            path: req.source.clone(),
            len: bytes.len() as u64,
            sha256: crate::update::sha256_hex(&bytes),
            recycled: false,
        },
        Err(e) => {
            report.start_error = Some(format!("The run could not start: {e}"));
            return report;
        }
    };
    if let Some(why) = outside(req) {
        report.start_error = Some(format!("The run could not start: {why}"));
        return report;
    }
    // 1. The record, state "writing", holding the whole plan. No folder and no group file is
    // marked as made by this run until it was made. A note whose name is free now is marked as
    // the run's before it is written, so that a run that dies while it writes its notes leaves a
    // record that owns them (audit 4, A F9 and W5): undo removes such a note only while it holds,
    // byte for byte, the text this run planned for it, and never a note it finds missing. Step 4
    // puts the mark right for every note once it knows, and the record is written again after
    // step 5.
    let mut record = planned(req, kind, secs, started, source);
    let record_path = match record.create(&req.library) {
        Ok(path) => path,
        Err(e) => {
            report.start_error = Some(format!("The run could not start: {e}"));
            return report;
        }
    };
    report.record_path = Some(record_path.clone());
    record.note_done(&record_path, "started");

    // 2. Folders. A folder that cannot be made fails the notes that needed it, at 4.
    for folder in &req.folders {
        if !inside_any(folder, req) {
            report.folder_failures.push(Failure::new(
                folder,
                "it is outside the library and the group's folder",
            ));
            continue;
        }
        // The folders that are not there yet, deepest first: the ones this run makes. A folder
        // that was there is never in the record, so undo never removes it.
        let mut missing: Vec<PathBuf> = Vec::new();
        let mut at = Some(folder.as_path());
        while let Some(dir) = at {
            if dir.symlink_metadata().is_ok() || !inside_any(dir, req) {
                break;
            }
            missing.push(dir.to_path_buf());
            at = dir.parent();
        }
        let made = std::fs::create_dir_all(folder);
        for dir in missing.into_iter().rev() {
            if dir.is_dir() && !record.folders.iter().any(|f| same_path(f, &dir)) {
                record.folders.push(dir);
            }
        }
        if let Err(e) = made {
            report
                .folder_failures
                .push(Failure::new(folder, e.to_string()));
        }
    }

    // 3. Pictures: each copied to a new file, never over one.
    for picture in &req.pictures {
        let bytes = match std::fs::read(&picture.source) {
            Ok(bytes) => bytes,
            Err(e) => {
                report
                    .picture_failures
                    .push(Failure::new(&picture.source, e.to_string()));
                continue;
            }
        };
        let stem = picture
            .source
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "picture".to_string());
        let ext = picture
            .source
            .extension()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        match create_copy(&picture.folder, &stem, &ext, &bytes) {
            Ok((path, true)) => {
                record.pictures.push(PictureRec {
                    path: path.clone(),
                    created: true,
                    sha256: crate::update::sha256_hex(&bytes),
                });
                report.pictures_written.push(path.clone());
                record.note_done(&record_path, &format!("picture {path:?}"));
            }
            Ok((path, false)) => {
                // The same bytes are already there: the picture counts as copied, and the file
                // is not written again. It is not this run's file, so undo leaves it.
                record.pictures.push(PictureRec {
                    path: path.clone(),
                    created: false,
                    sha256: crate::update::sha256_hex(&bytes),
                });
                report.pictures_written.push(path);
            }
            Err(e) => {
                report
                    .picture_failures
                    .push(Failure::new(&picture.source, e.to_string()));
            }
        }
    }

    // The folders and the pictures are the run's from here on: a run that dies while it writes
    // its notes leaves a record that owns them too (audit 4, W5).
    if !record.folders.is_empty() || !record.pictures.is_empty() {
        let _ = record.close(&record_path, &req.library);
    }
    if dies_at("before the notes") {
        return report;
    }

    // 4. Notes: each created at the name the plan gave it, never over a file. A note is the
    // run's only when this write made it; step 1's mark is put right for every other one.
    let mut written: Vec<usize> = Vec::new();
    for (i, note) in req.notes.iter().enumerate() {
        if let Some(rec) = record.notes.get_mut(i) {
            rec.created = false;
        }
        if !inside_any(&note.path, req) {
            report.note_failures.push(Failure::new(
                &note.path,
                "it is outside the library and the group's folder",
            ));
            continue;
        }
        match create_new(&note.path, note.text.as_bytes()) {
            Ok(()) => {
                written.push(i);
                if let Some(rec) = record.notes.get_mut(i) {
                    rec.created = true;
                }
                report.notes_written.push(note.path.clone());
                record.note_done(&record_path, &format!("note {}", shown(&note.path)));
                if dies_at("after a note") {
                    return report;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                // 3.9: the same name with the same bytes is the note, already there. A file
                // with other bytes is never touched, and the note is reported.
                match std::fs::read(&note.path) {
                    Ok(bytes) if bytes == note.text.as_bytes() => {
                        written.push(i);
                        report.notes_present.push(note.path.clone());
                    }
                    Ok(_) => report.note_failures.push(Failure::new(
                        &note.path,
                        "a file of that name is there with other contents",
                    )),
                    Err(e) => report
                        .note_failures
                        .push(Failure::new(&note.path, e.to_string())),
                }
            }
            Err(e) => report
                .note_failures
                .push(Failure::new(&note.path, e.to_string())),
        }
    }

    // 5. The read-back: each note read again and compared with the lines it was made from. A
    // note that does not read back is neither linked nor counted, and stops the Recycle Bin
    // step ([`recycle_source`]).
    let mut sound: Vec<usize> = Vec::new();
    for &i in &written {
        let note = &req.notes[i];
        match verify(&note.path, &note.from_source) {
            Ok(()) => sound.push(i),
            Err(why) => {
                report.check_failures.push(Failure::new(&note.path, why));
                record.note_done(&record_path, &format!("check failed {}", shown(&note.path)));
            }
        }
    }

    // What the run has made so far goes into the record now, before any library file is touched:
    // a run that stops after this point can be undone, and undo knows exactly which folders and
    // notes are its own. A failure to write it again is reported by step 7, which tries once more.
    let _ = record.close(&record_path, &req.library);

    // 6. The library: the group's file, then each file's links in one atomic write.
    if !sound.is_empty() {
        let group_ready = if req.group.created {
            match create_group(req) {
                Ok(()) => {
                    record.group.created = true;
                    let _ = record.close(&record_path, &req.library);
                    record.note_done(&record_path, &format!("group {}", shown(&req.group.file)));
                    true
                }
                Err(e) => {
                    report.group_failure = Some(Failure::new(&req.group.file, e.to_string()));
                    false
                }
            }
        } else {
            true
        };
        if group_ready {
            let links: Vec<Link> = sound.iter().map(|&i| Link::new(&req.notes[i])).collect();
            // The headings the file does not have yet are the run's own, for undo.
            let before = std::fs::read_to_string(&req.group.file).ok().map(|text| {
                library::parse(&text, req.group.file.parent().unwrap_or(Path::new("")))
            });
            let mut added: Vec<Vec<String>> = Vec::new();
            for note in sound.iter().map(|&i| &req.notes[i]) {
                for n in 1..=note.sections.len() {
                    let path: Vec<String> = note.sections[..n].to_vec();
                    let known = before
                        .as_ref()
                        .is_some_and(|p| p.sections.iter().any(|s| sections_eq(s, &path)));
                    if !known && !added.iter().any(|s| sections_eq(s, &path)) {
                        added.push(path);
                    }
                }
            }
            let text_before = library_text_of(&req.group.file);
            match gate.add_links(&req.group.file, &links) {
                Ok(()) => {
                    record.sections_added = added;
                    record.keep_added(&req.group.file, text_before, &links, &req.library);
                    report.linked.push(req.group.file.clone());
                    record.note_done(
                        &record_path,
                        &format!("links {} in {}", links.len(), shown(&req.group.file)),
                    );
                }
                Err(why) => report
                    .link_failures
                    .push(Failure::new(&req.group.file, why)),
            }
        }
    }

    // 7. The record is closed. A run that could not update a library file says so.
    record.state = if report.link_failures.is_empty() && report.group_failure.is_none() {
        State::Done
    } else {
        State::LinksPending
    };
    match record.close(&record_path, &req.library) {
        Ok(()) => {
            report.closed = true;
            record.note_done(&record_path, record.state.as_str());
        }
        Err(e) => report.close_error = Some(e.to_string()),
    }
    // What the run wrote, when it knows: the notes that were sound and linked keep the digest
    // of their bytes; a note that failed keeps the digest it was planned with.
    if report.closed {
        prune(&req.library, secs);
    }
    report.record = Some(record);
    report
}

#[cfg(test)]
thread_local! {
    /// The step a test makes [`write_of`] stop at, as a process that died there would: what is
    /// on disk then is what a crash leaves.
    static DIES_AT: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

/// Whether a test asked [`write_of`] to stop at `step`; never in a build of the program.
fn dies_at(step: &str) -> bool {
    #[cfg(test)]
    {
        DIES_AT.with(|d| d.get() == Some(step))
    }
    #[cfg(not(test))]
    {
        let _ = step;
        false
    }
}

/// Step 1's record of a write: the whole plan, state "writing", with every note whose name is
/// free now marked as the run's ([`write_of`] puts the marks right as it writes).
fn planned(req: &Request, kind: Kind, secs: u64, started: &str, source: Source) -> Record {
    Record {
        version: RECORD_VERSION,
        kind,
        started: started.to_string(),
        started_secs: secs,
        state: State::Writing,
        instruction: req.instruction.clone(),
        sent: req.sent.clone(),
        source,
        group: Group {
            created: false,
            ..req.group.clone()
        },
        folders: Vec::new(),
        notes: req
            .notes
            .iter()
            .map(|n| NoteRec {
                path: n.path.clone(),
                created: n.path.symlink_metadata().is_err(),
                // The digest of the bytes this note will hold, known before they are written.
                sha256: crate::update::sha256_hex(n.text.as_bytes()),
                ranges: n.ranges.clone(),
                heading_shift: n.heading_shift,
                title_added: n.title_added,
                sections: n.sections.clone(),
                line: library::link_line(&n.title, &n.link_path),
            })
            .collect(),
        pictures: Vec::new(),
        sections_added: Vec::new(),
        moved: None,
        originals: Vec::new(),
        added: Vec::new(),
    }
}

/// The text a note must hold, read from disk; `Err` says it does not read back the same.
pub fn verify(path: &Path, expected: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes == expected.as_bytes() {
        Ok(())
    } else {
        Err("does not read back the same".to_string())
    }
}

/// Writes `bytes` as `stem.ext`, or `stem 2.ext` when that is taken, and never over a file. The
/// second value is false when a file with exactly these bytes was there already.
fn create_copy(folder: &Path, stem: &str, ext: &str, bytes: &[u8]) -> io::Result<(PathBuf, bool)> {
    for n in 1..=999 {
        let name = if ext.is_empty() {
            if n == 1 {
                stem.to_string()
            } else {
                format!("{stem} {n}")
            }
        } else if n == 1 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem} {n}.{ext}")
        };
        let path = folder.join(name);
        match create_new(&path, bytes) {
            Ok(()) => return Ok((path, true)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                if std::fs::read(&path).is_ok_and(|old| old == bytes) {
                    return Ok((path, false));
                }
            }
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "every name for it is taken",
    ))
}

/// Makes the group's library file, refusing when the library already holds as many library files
/// as the reader takes. The file is created exclusively: an existing file is never replaced.
fn create_group(req: &Request) -> io::Result<()> {
    if let Ok(entries) = std::fs::read_dir(&req.library) {
        let count = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("md"))
            })
            .count();
        if count >= library::MAX_FILES {
            return Err(io::Error::other(format!(
                "The library already has {} files.",
                library::MAX_FILES
            )));
        }
    }
    let name = req.group.name.trim();
    let home = written_home(&req.group.home, &req.library);
    let text = library::set_group_home(&format!("# {name}\n"), &home)
        .ok_or_else(|| io::Error::other("the group has no name"))?;
    let read = library::parse(&text, &req.library);
    if read.name.as_deref() != Some(name) || read.home.is_none() {
        return Err(io::Error::other(
            "the file would not read back as its notes",
        ));
    }
    create_new(&req.group.file, text.as_bytes())
}

/// A folder as a library file writes it: relative to the library when it is inside it, so the
/// library can be moved as a whole.
fn written_home(home: &Path, library: &Path) -> PathBuf {
    match home.strip_prefix(library) {
        Ok(inside) if !inside.as_os_str().is_empty() => inside.to_path_buf(),
        _ => home.to_path_buf(),
    }
}

impl Group {
    /// The name the library file is made from: its file name without `.md`.
    pub fn file_stem(&self) -> String {
        self.file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

pub(crate) fn shown(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Whether `path` is in the library folder or in the group's folder. Nothing is written outside
/// those, and undo removes nothing outside them either: the one rule, in one place.
///
/// No library folder, or no home, is nothing: an empty path is not a folder that holds
/// everything. A path is compared as the disk compares names (part by part, in the platform's
/// case), after the disk has been asked where its links lead.
pub(crate) fn inside(library: &Path, home: &Path, path: &Path) -> bool {
    if library.as_os_str().is_empty() {
        return false;
    }
    // Compared in one spelling, whatever their length (audit 4, N2).
    let path = crate::paths::comparable(path);
    let library = crate::paths::comparable(library);
    if under(&path, &library) && !same_path(&path, &library) {
        return true;
    }
    if home.as_os_str().is_empty() {
        return false;
    }
    under(&path, &crate::paths::comparable(home))
}

/// Whether `path` is `base` or lies below it, part by part, with the case rule of
/// [`same_path`]. No disk is asked.
pub(crate) fn under(path: &Path, base: &Path) -> bool {
    let mut parts = path.components();
    base.components().all(|b| {
        parts
            .next()
            .is_some_and(|p| same_path(Path::new(p.as_os_str()), Path::new(b.as_os_str())))
    })
}

/// [`inside`] for a request's two folders.
fn inside_any(path: &Path, req: &Request) -> bool {
    inside(&req.library, &req.group.home, path)
}

/// Why the request must not be written, or `None`.
fn outside(req: &Request) -> Option<String> {
    for folder in &req.folders {
        if !inside_any(folder, req) {
            return Some(format!(
                "{} is outside the library and the group's folder.",
                folder.display()
            ));
        }
    }
    for note in &req.notes {
        if !inside_any(&note.path, req) {
            return Some(format!(
                "{} is outside the library and the group's folder.",
                note.path.display()
            ));
        }
    }
    for picture in &req.pictures {
        if !inside_any(&picture.folder, req) {
            return Some(format!(
                "{} is outside the library and the group's folder.",
                picture.folder.display()
            ));
        }
    }
    // The group's file is a library file: a `.md` directly in the library folder, whatever the
    // home is. A home that is the library folder itself waived the check before (audit 4, N1).
    let library_file = Reach {
        library: req.library.clone(),
        home: None,
    }
    .is_library_file(&req.group.file);
    if !library_file {
        return Some(format!(
            "{} is outside the library folder.",
            req.group.file.display()
        ));
    }
    None
}

/// The library text with every link added, in the order given, one call of the library's own
/// writer per link. A note under no section goes in the group's own notes, before the first
/// section heading; those are written first, so a heading another link makes is made below them.
/// Undo removes them in the reverse order.
pub fn with_links_added(text: &str, links: &[Link]) -> String {
    let mut out = text.to_string();
    let mut ordered: Vec<&Link> = links.iter().filter(|l| l.sections.is_empty()).collect();
    ordered.extend(links.iter().filter(|l| !l.sections.is_empty()));
    for link in ordered {
        out = library::add_note_in_group(&out, &link.sections, &link.title, &link.path);
    }
    out
}

/// Whether `new` is exactly `old`'s notes plus `links`, each under its section: the check step 6
/// makes before an atomic write. A count is not enough. `base` resolves a link's written path the
/// way the library file does.
pub fn check_links_added(old: &Parsed, new: &Parsed, links: &[Link], base: &Path) -> bool {
    let same = |e: &library::Entry, l: &Link| {
        let target = link_target(&l.path, base);
        same_path(&e.path, &target)
            && sections_eq(&e.sections, &l.sections)
            && e.title.trim() == l.title.trim().replace(['[', ']'], "").trim()
    };
    if new.entries.len() != old.entries.len() + links.len() {
        return false;
    }
    // Every note the file had is still there, once per occurrence.
    let mut left: Vec<&library::Entry> = new.entries.iter().collect();
    for e in &old.entries {
        let at = left.iter().position(|n| {
            same_path(&n.path, &e.path) && n.sections == e.sections && n.title == e.title
        });
        match at {
            Some(i) => {
                left.remove(i);
            }
            None => return false,
        }
    }
    // And each planned link is among what is left, under its section.
    for l in links {
        let at = left.iter().position(|n| same(n, l));
        match at {
            Some(i) => {
                left.remove(i);
            }
            None => return false,
        }
    }
    left.is_empty()
}

/// The library text with every recorded link taken out: only a line that is still exactly the
/// recorded line under the recorded section goes. `None` when not one of them was found.
pub fn with_links_removed(text: &str, links: &[LinkRemoval], base: &Path) -> Option<String> {
    let mut out = text.to_string();
    let mut removed = false;
    for link in links {
        let parsed = library::parse(&out, base);
        let target = link_target(&link.path, base);
        let at = parsed.entries.iter().find(|e| {
            same_path(&e.path, &target)
                && sections_eq(&e.sections, &link.sections)
                && out
                    .lines()
                    .nth(e.line.saturating_sub(1))
                    .is_some_and(|line| line.trim_end() == link.line.trim_end())
        });
        if let Some(e) = at
            && let Some(new) = library::remove_note(&out, e.line)
        {
            out = new;
            removed = true;
        }
    }
    removed.then_some(out)
}

/// Whether `new` is `old` without exactly the removals: the check step 2 of undo makes.
pub fn check_links_removed(
    old: &Parsed,
    new: &Parsed,
    removals: &[LinkRemoval],
    base: &Path,
) -> bool {
    if new.entries.len() + removals.len() != old.entries.len() {
        return false;
    }
    let mut left: Vec<&library::Entry> = old.entries.iter().collect();
    for e in &new.entries {
        let at = left.iter().position(|n| {
            same_path(&n.path, &e.path) && n.sections == e.sections && n.title == e.title
        });
        match at {
            Some(i) => {
                left.remove(i);
            }
            None => return false,
        }
    }
    // What is left is the removals, each of them, under its section.
    for l in removals {
        let target = link_target(&l.path, base);
        let at = left
            .iter()
            .position(|n| same_path(&n.path, &target) && sections_eq(&n.sections, &l.sections));
        match at {
            Some(i) => {
                left.remove(i);
            }
            None => return false,
        }
    }
    left.is_empty()
}

/// The library text with the section headings the run wrote taken out, when nothing is under
/// them: deepest first, and only a heading whose path and place are still there.
///
/// "Nothing under it" means no note under it or under a heading inside it, and no line of
/// anything else before the next heading of its level or above (or the end of the file): a
/// sub-heading, a paragraph or a list of the owner's keeps the heading, so what they wrote is not
/// left under the section before it (audit 3, C6; audit 4, A F5).
pub fn with_headings_removed(text: &str, headings: &[Vec<String>], base: &Path) -> Option<String> {
    let mut out = text.to_string();
    let mut changed = false;
    let mut ordered = headings.to_vec();
    ordered.sort_by_key(|h| std::cmp::Reverse(h.len()));
    for heading in ordered {
        let parsed = library::parse(&out, base);
        let under = parsed
            .entries
            .iter()
            .any(|e| sections_eq(&e.sections[..e.sections.len().min(heading.len())], &heading));
        if under {
            continue;
        }
        let Some(line) = library::section_line(&out, &heading) else {
            continue;
        };
        let Some(text_line) = out.lines().nth(line.saturating_sub(1)) else {
            continue;
        };
        let Some(level) = atx_level(text_line) else {
            continue;
        };
        let owned = out
            .lines()
            .skip(line)
            .find(|l| !l.trim().is_empty())
            .is_some_and(|next| atx_level(next).is_none_or(|l| l > level));
        if owned {
            continue;
        }
        let mut lines: Vec<&str> = out.lines().collect();
        if line == 0 || line > lines.len() {
            continue;
        }
        lines.remove(line - 1);
        // The file's own line ending, found the way `crate::library`'s writers find it,
        // so a CRLF file stays CRLF and a file that ended with no newline keeps none.
        let eol = if out.contains("\r\n") { "\r\n" } else { "\n" };
        let mut new = lines.join(eol);
        if out.ends_with('\n') {
            new.push_str(eol);
        }
        out = new;
        changed = true;
    }
    changed.then_some(out)
}

/// The level of an ATX heading line (`## Tokyo` is 2), the way Markdown reads one: at most three
/// spaces before one to six `#`, then a space or the end of the line.
fn atx_level(line: &str) -> Option<usize> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let hashes = rest.len() - rest.trim_start_matches('#').len();
    let after = &rest[hashes..];
    ((1..=6).contains(&hashes) && (after.is_empty() || after.starts_with([' ', '\t'])))
        .then_some(hashes)
}

pub(crate) fn sections_eq(a: &[String], b: &[String]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.trim().eq_ignore_ascii_case(y.trim()))
}

/// A link's path as the library file resolves it.
pub(crate) fn link_target(path: &Path, base: &Path) -> PathBuf {
    resolved(&path.to_string_lossy(), base)
}

// ---- step 8: the original in the Recycle Bin (5.4) ----------------------------------------------

/// What puts a file in the Recycle Bin. The application's is the system's bin
/// (`mdshared_ui::platform::recycle`, behind its feature `recycle`); a test's records the call
/// and touches no bin.
pub trait Bin {
    /// Puts the file at `path` in the bin; `Err` says why it did not go, and it is then where it
    /// was.
    fn recycle(&mut self, path: &Path) -> Result<(), String>;
}

/// The "bin" that removes the file: what undo did before the Recycle Bin was built (D-071), and
/// what [`undo`] still does.
pub struct Delete;

impl Bin for Delete {
    fn recycle(&mut self, path: &Path) -> Result<(), String> {
        std::fs::remove_file(path).map_err(|e| e.to_string())
    }
}

/// Step 8 of 4.3: the source of a written split goes to the Recycle Bin, last, and only when the
/// run is whole and the source is still the file that was read. Every ticked note was written or
/// was there already, read back the same and was linked; the record is closed as "done"; and the
/// source has the length and SHA-256 the record holds. Then `bin` takes it, and the record says
/// `recycled`, so undo asks for it back before it removes a note (4.5, rule 1).
///
/// `Err` is the reason in words that follow "could not be put in the Recycle Bin: ", and the
/// source is then where it was. A record that could not be written again after the bin took the
/// source is reported in `report.close_error`; undo still refuses, because the source is gone.
pub fn recycle_source(
    report: &mut Report,
    req: &Request,
    library: &Path,
    bin: &mut dyn Bin,
) -> Result<(), String> {
    let (Some(record), Some(record_path)) = (report.record.as_mut(), report.record_path.clone())
    else {
        return Err("the run did not start".to_string());
    };
    let whole = report.start_error.is_none()
        && report.note_failures.is_empty()
        && report.picture_failures.is_empty()
        && report.check_failures.is_empty()
        && report.folder_failures.is_empty()
        && report.group_failure.is_none()
        && report.link_failures.is_empty()
        && report.notes_written.len() + report.notes_present.len() == req.notes.len()
        && !req.notes.is_empty()
        && report.linked.iter().any(|f| same_path(f, &req.group.file));
    if !whole {
        return Err("not every note was written, read back and listed".to_string());
    }
    if record.kind != Kind::Split || record.state != State::Done || !report.closed {
        return Err("the run's record is not closed as done".to_string());
    }
    if !same_path(&record.source.path, &req.source) {
        return Err("the run's record names another file".to_string());
    }
    match source_state(&record.source) {
        SourceState::Same => {}
        SourceState::Gone => return Err("it is not where it was".to_string()),
        SourceState::Changed => {
            return Err("it has changed since the notes were cut from it".to_string());
        }
        SourceState::TooLarge => return Err("it is too large to be checked".to_string()),
    }
    bin.recycle(&record.source.path)?;
    record.source.recycled = true;
    if let Err(e) = record.close(&record_path, library) {
        report.close_error = Some(e.to_string());
    }
    record.note_done(&record_path, "recycled");
    Ok(())
}

/// The groups whose library files list `path` now, by their names: read from the files, as undo
/// reads them. The report says a recycled source that is itself a note "will show as missing".
pub fn listed_in(library: &Path, path: &Path) -> Vec<String> {
    let listed = list_library(library);
    let mut groups: Vec<String> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for l in named_in(&listed, path) {
        if seen.iter().any(|f| same_path(f, &l.file)) {
            continue;
        }
        seen.push(l.file.clone());
        let name = read_regular(&l.file, library::MAX_FILE_BYTES as u64, true)
            .and_then(|bytes| library::parse(&crate::files::decode(&bytes).0, library).name)
            .unwrap_or_else(|| {
                l.file
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        groups.push(name);
    }
    groups
}

// ---- undo ------------------------------------------------------------------------------------------

/// What undo would do to one run, worked out from the record and the disk as they are now.
#[derive(Clone, Debug, Default)]
pub struct UndoPlan {
    /// Why undo cannot run at all.
    pub refusal: Option<String>,
    /// The links to take out, per library file.
    pub links: Vec<(PathBuf, Vec<LinkRemoval>)>,
    /// Lines that are gone already: nothing to do.
    pub links_gone: usize,
    /// Notes whose line the owner edited or moved since: the line is kept, and so is the note.
    pub links_kept: Vec<PathBuf>,
    /// Notes that stay because a library line still lists them after this run's lines are out.
    pub held: Vec<PathBuf>,
    /// Files the run found there already, with the bytes it would have written: they are linked,
    /// undo takes their lines out, and the files are not its own to remove.
    pub before: Vec<PathBuf>,
    /// The files this run wrote, with the digest they had: removed only while it matches.
    pub notes: Vec<PathBuf>,
    pub pictures: Vec<PathBuf>,
    /// Files changed since: kept, and listed.
    pub kept: Vec<PathBuf>,
    /// The notes among them, and the pictures, so the question can name each kind (5.9).
    pub notes_kept: Vec<PathBuf>,
    pub pictures_kept: Vec<PathBuf>,
    pub gone: Vec<PathBuf>,
    /// The group's file, when the run made it.
    pub group_file: Option<PathBuf>,
    /// The folders the run made, deepest first.
    pub folders: Vec<PathBuf>,
    /// A rewrite's originals that would be put back: the file still holds what the run wrote.
    pub restore: Vec<PathBuf>,
    /// A rewrite's originals changed since the run: kept, and listed.
    pub restore_kept: Vec<PathBuf>,
    /// A rewrite's originals that are gone already: nothing to put back.
    pub restore_gone: usize,
    /// Notes and pictures this record marks as its own that a run started later names as its
    /// own too: this run died before it wrote them, and the later run found the names free and
    /// wrote them. They are that run's, and stay with their lines (review of the audit 4 fixes,
    /// S2).
    pub later: Vec<PathBuf>,
}

impl UndoPlan {
    /// Whether anything would happen.
    pub fn acts(&self) -> bool {
        self.refusal.is_none()
            && (!self.notes.is_empty()
                || !self.pictures.is_empty()
                || !self.links.is_empty()
                || self.group_file.is_some()
                || !self.folders.is_empty()
                || !self.restore.is_empty())
    }

    /// The question's sentence: what goes, and what stays.
    pub fn sentence(&self, group: &str) -> String {
        if let Some(why) = &self.refusal {
            return why.clone();
        }
        if !self.restore.is_empty() || !self.restore_kept.is_empty() || self.restore_gone > 0 {
            // Each file is named where it is: a rewrite's record may name a file anywhere, so the
            // question says which (audit 4, A F8).
            let mut s = if self.restore.is_empty() {
                "Nothing of this run is left to put back.".to_string()
            } else {
                let named: Vec<String> = self
                    .restore
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();
                format!(
                    "Puts back the original of {} this run rewrote: {}.",
                    count(self.restore.len(), "file", "files"),
                    named.join(", ")
                )
            };
            if !self.restore_kept.is_empty() {
                s.push_str(&format!(
                    " {} changed since and {} kept.",
                    count(self.restore_kept.len(), "file was", "files were"),
                    if self.restore_kept.len() == 1 {
                        "is"
                    } else {
                        "are"
                    }
                ));
            }
            if self.restore_gone > 0 {
                s.push_str(&format!(
                    " {} gone already.",
                    count(self.restore_gone, "file is", "files are")
                ));
            }
            return s;
        }
        let mut goes: Vec<String> = Vec::new();
        if !self.notes.is_empty() {
            goes.push(count(self.notes.len(), "note", "notes"));
        }
        if !self.pictures.is_empty() {
            goes.push(count(self.pictures.len(), "picture", "pictures"));
        }
        let mut s = if goes.is_empty() && self.links.is_empty() {
            "Nothing of this run is left to remove.".to_string()
        } else {
            let mut s;
            if goes.is_empty() {
                s = "Removes the lines this run added".to_string();
            } else {
                s = format!("Removes {} this run wrote", goes.join(" and "));
            }
            if !self.links.is_empty() {
                let lines: usize = self.links.iter().map(|(_, l)| l.len()).sum();
                if goes.is_empty() {
                    s.push_str(&format!(": {lines} in {group}"));
                } else {
                    s.push_str(&format!(
                        ", with {} in {group}",
                        count(lines, "line", "lines")
                    ));
                }
            }
            if !self.folders.is_empty() {
                s.push_str(&format!(
                    ", and {} if {} empty",
                    count(self.folders.len(), "folder", "folders"),
                    if self.folders.len() == 1 {
                        "it is"
                    } else {
                        "they are"
                    }
                ));
            }
            s.push('.');
            s
        };
        if !self.kept.is_empty() {
            let mut said: Vec<String> = Vec::new();
            if !self.notes_kept.is_empty() {
                said.push(format!(
                    "{} changed since",
                    count(self.notes_kept.len(), "note was", "notes were")
                ));
            }
            if !self.pictures_kept.is_empty() {
                said.push(format!(
                    "{} changed since",
                    count(self.pictures_kept.len(), "picture was", "pictures were")
                ));
            }
            s.push_str(&format!(
                " {} and {} kept.",
                said.join(", "),
                if self.kept.len() == 1 { "is" } else { "are" }
            ));
        }
        if !self.links_kept.is_empty() {
            s.push_str(&format!(
                " {} edited or moved since and {} kept, with {}.",
                count(self.links_kept.len(), "line was", "lines were"),
                if self.links_kept.len() == 1 {
                    "is"
                } else {
                    "are"
                },
                if self.links_kept.len() == 1 {
                    "its note"
                } else {
                    "their notes"
                }
            ));
        }
        let other: Vec<&PathBuf> = self
            .held
            .iter()
            .filter(|h| !self.links_kept.iter().any(|k| same_path(k, h)))
            .collect();
        if !other.is_empty() {
            s.push_str(&if other.len() == 1 {
                " 1 note stays, because another line in the library lists it.".to_string()
            } else {
                format!(
                    " {} notes stay, because other lines in the library list them.",
                    other.len()
                )
            });
        }
        if !self.before.is_empty() {
            s.push_str(&format!(
                " {} there before this run and {} kept.",
                count(self.before.len(), "file was", "files were"),
                if self.before.len() == 1 { "is" } else { "are" }
            ));
        }
        if !self.later.is_empty() {
            s.push_str(&format!(
                " {} a later run's and {}.",
                count(self.later.len(), "file is", "files are"),
                if self.later.len() == 1 {
                    "stays"
                } else {
                    "stay"
                }
            ));
        }
        s
    }
}

fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// The text of a regular file of at most `max` bytes, read whole and no further; `None` when it
/// is not a regular file, is bigger, or cannot be read. With `follow` a link is followed, as it
/// is for a source document; without it a link is not a regular file, which is what undo wants
/// of anything it is about to delete.
pub(crate) fn read_regular(path: &Path, max: u64, follow: bool) -> Option<Vec<u8>> {
    use std::io::Read;
    let meta = if follow {
        std::fs::metadata(path)
    } else {
        std::fs::symlink_metadata(path)
    }
    .ok()?;
    if !meta.is_file() || meta.len() > max {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= max).then_some(bytes)
}

pub(crate) fn digest_of(path: &Path) -> Option<String> {
    let bytes = read_regular(path, DIGEST_MAX_BYTES, false)?;
    Some(crate::update::sha256_hex(&bytes))
}

pub(crate) fn has_parent_dir(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

pub(crate) fn has_extension(path: &Path, allowed: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| allowed.iter().any(|a| e.eq_ignore_ascii_case(a)))
}

/// Where the document a run was cut from stands now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceState {
    /// The file has the recorded length and digest.
    Same,
    Gone,
    /// There is a file, and it is not the recorded one.
    Changed,
    /// It is the recorded length, and too large to be compared.
    TooLarge,
}

fn source_state(source: &Source) -> SourceState {
    let Ok(meta) = std::fs::metadata(&source.path) else {
        return SourceState::Gone;
    };
    if !meta.is_file() || meta.len() != source.len {
        return SourceState::Changed;
    }
    if source.len > DIGEST_MAX_BYTES {
        return SourceState::TooLarge;
    }
    match read_regular(&source.path, DIGEST_MAX_BYTES, true) {
        Some(bytes) if crate::update::sha256_hex(&bytes).eq_ignore_ascii_case(&source.sha256) => {
            SourceState::Same
        }
        Some(_) => SourceState::Changed,
        None => SourceState::Gone,
    }
}

/// Why undo does not run when the original is not the file the run was cut from. The notes may
/// be the only copy of its text, and undo is the one thing here that deletes.
fn source_refusal(record: &Record, state: SourceState) -> String {
    let name = record.source_name();
    if record.source.recycled {
        return format!("Put \u{2018}{name}\u{2019} back from the Recycle Bin first, then undo.");
    }
    let what = match state {
        SourceState::Gone => format!(
            "\u{2018}{name}\u{2019}, the file these notes were cut from, is not where it was"
        ),
        SourceState::Changed => {
            format!("\u{2018}{name}\u{2019} is not the file these notes were cut from any more")
        }
        _ => format!("\u{2018}{name}\u{2019} is too large to be checked against this run"),
    };
    format!(
        "{what}, so these notes may be the only copy of its text. Undo will not remove them. \
         Put the original back, or delete the notes yourself if you do not need them."
    )
}

// ---- what a record may reach -------------------------------------------------------------------

/// What a record may name, and so what undo and finish may touch for it: the library folder, and
/// the home the group's library file declares now. A record is a file on disk like any other, so
/// it is not given more than write access to the library folder gives: a path in it is never
/// taken as it stands, and the home is read from the library file and not from the record.
pub(crate) struct Reach {
    pub(crate) library: PathBuf,
    pub(crate) home: Option<PathBuf>,
}

impl Reach {
    /// The reach of `record` in `library`, or the sentence that says which path of the record is
    /// outside it, so that none of the record is acted on.
    fn of(record: &Record, library: &Path) -> Result<Reach, String> {
        if !library.is_absolute() {
            return Err("The library folder is not known, so this run is not undone.".to_string());
        }
        let reach = Reach {
            library: library.to_path_buf(),
            home: declared_home(&record.group.file, library),
        };
        // A group's file that is not there declares no folder, so a note in a folder outside the
        // library cannot be checked against one: the sentence says that, and not that the record
        // names a place it should not (audit 4, W2). Nothing is acted on either way.
        let missing = record.group.file.symlink_metadata().is_err();
        let outside = |path: &Path| {
            if missing {
                format!(
                    "The group's library file {} is not there, so the folder {} is in cannot be \
                     checked, and this run is not undone or finished. Its notes are on disk as \
                     they were; make the group again with that folder, or remove them yourself.",
                    record.group.file.display(),
                    path.display()
                )
            } else {
                format!(
                    "{} is outside the library and the group's folder, so this run is not undone.",
                    path.display()
                )
            }
        };
        let foreign = |path: &Path| {
            format!(
                "{} is not a file a run writes, so this run is not undone.",
                path.display()
            )
        };
        let file = record.group.file.as_path();
        if !reach.is_library_file(file) {
            return Err(outside(file));
        }
        for folder in &record.folders {
            if !reach.allows(folder) {
                return Err(outside(folder));
            }
        }
        for note in &record.notes {
            if !reach.allows(&note.path) {
                return Err(outside(&note.path));
            }
            if !has_extension(&note.path, &["md"]) {
                return Err(foreign(&note.path));
            }
        }
        for picture in &record.pictures {
            if !reach.allows(&picture.path) {
                return Err(outside(&picture.path));
            }
            if !has_extension(&picture.path, &PICTURE_EXTENSIONS) {
                return Err(foreign(&picture.path));
            }
        }
        Ok(reach)
    }

    /// A path with no `..`, inside the library folder (not the folder itself) or inside the home.
    pub(crate) fn allows(&self, path: &Path) -> bool {
        !has_parent_dir(path)
            && inside(
                &self.library,
                self.home.as_deref().unwrap_or(Path::new("")),
                path,
            )
    }

    /// A `.md` file directly in the library folder: where a group's library file is, and the one
    /// place undo writes besides the files a run made.
    pub(crate) fn is_library_file(&self, path: &Path) -> bool {
        !has_parent_dir(path)
            && has_extension(path, &["md"])
            && path.parent().is_some_and(|dir| {
                same_path(
                    &crate::paths::canonical_or_absolute(dir),
                    &crate::paths::canonical_or_absolute(&self.library),
                )
            })
    }
}

/// The folder the group's library file names with `home:` as it reads now; `None` when the file
/// is not there, names none, names one with a `..` in it, or names the library folder or a folder
/// that holds it.
pub(crate) fn declared_home(group_file: &Path, library: &Path) -> Option<PathBuf> {
    let bytes = read_regular(group_file, library::MAX_FILE_BYTES as u64, true)?;
    let text = crate::files::decode(&bytes).0.into_owned();
    let home = library::parse(&text, library).home?;
    if home.as_os_str().is_empty() || has_parent_dir(&home) {
        return None;
    }
    let held = under(
        &crate::paths::canonical_or_absolute(library),
        &crate::paths::canonical_or_absolute(&home),
    );
    (!held).then_some(home)
}

// ---- what the library lists --------------------------------------------------------------------

/// One note line of one library file, as the file holds it now.
pub(crate) struct Listing {
    pub(crate) file: PathBuf,
    pub(crate) path: PathBuf,
    /// The name of the file path leads to, in lower case: the cheap test before the real one.
    pub(crate) name: String,
    pub(crate) sections: Vec<String>,
    /// The line's text, as it is in the file.
    pub(crate) line: String,
}

/// Every note line of every library file, read from the files and not from what the window has
/// in memory.
pub(crate) struct Listed {
    /// False when the library could not be read in full, so that what is not listed may only be
    /// what was not seen.
    pub(crate) complete: bool,
    pub(crate) entries: Vec<Listing>,
}

/// Reads the library folder again: every `.md` file directly in it, as the reader takes them, a
/// file of more than the reader's cap as the reader takes it (it lists nothing). The reader keeps
/// at most [`library::MAX_FILES`] files; this looks at more, so a note listed by the 513th file is
/// still seen, and says it is not complete past [`LISTING_MAX_FILES`].
pub(crate) fn list_library(library: &Path) -> Listed {
    let mut listed = Listed {
        complete: true,
        entries: Vec::new(),
    };
    let Ok(dir) = std::fs::read_dir(library) else {
        listed.complete = false;
        return listed;
    };
    let mut files: Vec<PathBuf> = dir
        .flatten()
        .map(|e| e.path())
        .filter(|p| has_extension(p, &["md"]) && p.is_file())
        .collect();
    files.sort();
    if files.len() > LISTING_MAX_FILES {
        listed.complete = false;
        files.truncate(LISTING_MAX_FILES);
    }
    for file in files {
        let too_large =
            std::fs::metadata(&file).is_ok_and(|m| m.len() > library::MAX_FILE_BYTES as u64);
        if too_large {
            continue;
        }
        let Some(bytes) = read_regular(&file, library::MAX_FILE_BYTES as u64, true) else {
            listed.complete = false;
            continue;
        };
        let text = crate::files::decode(&bytes).0.into_owned();
        let lines: Vec<&str> = text.lines().collect();
        for e in library::parse(&text, library).entries {
            listed.entries.push(Listing {
                name: lower_name(&e.path),
                line: lines
                    .get(e.line.saturating_sub(1))
                    .copied()
                    .unwrap_or_default()
                    .to_string(),
                file: file.clone(),
                path: e.path,
                sections: e.sections,
            });
        }
    }
    listed
}

pub(crate) fn lower_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// The lines of listed that name path.
pub(crate) fn named_in<'a>(listed: &'a Listed, path: &Path) -> Vec<&'a Listing> {
    let name = lower_name(path);
    listed
        .entries
        .iter()
        .filter(|l| l.name == name && same_file(&l.path, path))
        .collect()
}

/// Whether two paths are one file: spelled alike, or, when their names are alike, leading to the
/// same place once the disk is asked. A mistake in the first direction keeps a note; one in the
/// other would delete a note a line still lists, so the disk is asked when in doubt.
pub(crate) fn same_file(a: &Path, b: &Path) -> bool {
    if same_path(a, b) {
        return true;
    }
    match (a.file_name(), b.file_name()) {
        (Some(x), Some(y)) if same_path(Path::new(x), Path::new(y)) => same_path(
            &crate::paths::canonical_or_absolute(a),
            &crate::paths::canonical_or_absolute(b),
        ),
        _ => false,
    }
}

/// Works out what undo would do, reading the disk and never writing it.
pub fn prepare(record: &Record, library: &Path) -> UndoPlan {
    plan_of(record, library).0
}

fn plan_of(record: &Record, library: &Path) -> (UndoPlan, Option<Reach>) {
    let mut plan = UndoPlan::default();
    if record.state == State::Undone {
        plan.refusal = Some("This run is undone already.".to_string());
        return (plan, None);
    }
    // A move holds files that were carried from one place to another, not files a run made: the
    // rules below would read it as a split with no notes. It has its own undo (`moves::undo`).
    if record.kind == Kind::Move {
        plan.refusal = Some("A move is undone by its own command.".to_string());
        return (plan, None);
    }
    // A rewrite touches only the files it names, and puts back what each held: there is no
    // library in it, and nothing is created or removed. Each file goes back only while it still
    // holds the text this run wrote.
    // A file the run rewrote twice has two originals: it goes back to the first one, while it
    // holds what the last one wrote (audit 4, W1).
    if record.kind == Kind::Rewrite {
        for (first, last) in rewrite_chains(record) {
            match digest_of(&first.path) {
                None => plan.restore_gone += 1,
                Some(d) if d.eq_ignore_ascii_case(&last.written) => {
                    plan.restore.push(first.path.clone())
                }
                Some(d) if d.eq_ignore_ascii_case(&first.sha256) => plan.restore_gone += 1,
                Some(_) => plan.restore_kept.push(first.path.clone()),
            }
        }
        return (plan, None);
    }
    // Every path the record names must be one a write could have made, and is checked against
    // the library folder and the home the group's library file declares now, never against the
    // record's own word (4.1, 6.2). One that is not makes it a record this program did not
    // write: none of it is acted on, and nothing is removed.
    let reach = match Reach::of(record, library) {
        Ok(reach) => reach,
        Err(why) => {
            plan.refusal = Some(why);
            return (plan, None);
        }
    };
    // The original. Undo is the one thing that deletes, so it does not run unless the notes can
    // not be the only copy: the original is where it was, with the length and the digest it had.
    // A source in the Recycle Bin must be back for the same reason.
    let state = source_state(&record.source);
    if state != SourceState::Same {
        plan.refusal = Some(source_refusal(record, state));
        return (plan, None);
    }
    let listed = list_library(library);
    if !listed.complete {
        plan.refusal = Some(
            "The library could not be read in full, so there is no telling which notes a line \
             still lists. Nothing was removed."
                .to_string(),
        );
        return (plan, None);
    }
    // What a later run wrote under the names this one marked: not this run's to take.
    let later =
        if record.notes.iter().any(|n| n.created) || record.pictures.iter().any(|p| p.created) {
            claimed_later(record, library)
        } else {
            Vec::new()
        };
    let later_run_wrote = |path: &Path| later.iter().any(|p| same_path(p, path));
    // Each recorded note: its line, then its file.
    let mut removals: Vec<LinkRemoval> = Vec::new();
    for note in &record.notes {
        if note.created && later_run_wrote(&note.path) {
            plan.later.push(note.path.clone());
            continue;
        }
        let named = named_in(&listed, &note.path);
        let in_group: Vec<&&Listing> = named
            .iter()
            .filter(|l| same_path(&l.file, &record.group.file))
            .collect();
        let exact = in_group.iter().any(|l| {
            sections_eq(&l.sections, &note.sections) && l.line.trim_end() == note.line.trim_end()
        });
        // The file first: a note changed since is kept, and so is its line.
        let mut file_goes = false;
        if !note.created {
            plan.before.push(note.path.clone());
        } else if note.path.symlink_metadata().is_err() {
            plan.gone.push(note.path.clone());
        } else if digest_of(&note.path).is_some_and(|d| d.eq_ignore_ascii_case(&note.sha256)) {
            file_goes = true;
        } else {
            plan.kept.push(note.path.clone());
            plan.notes_kept.push(note.path.clone());
            continue;
        }
        if exact {
            removals.push(LinkRemoval {
                path: note.path.clone(),
                sections: note.sections.clone(),
                line: note.line.clone(),
            });
        } else if in_group.is_empty() {
            // Not in the file: the line is gone, or the notes were never linked.
            plan.links_gone += 1;
        } else {
            // The line is there and is not the recorded one, or is under another section: the
            // owner edited or moved it, and it is kept.
            plan.links_kept.push(note.path.clone());
        }
        // Whatever still lists the note after the removals keeps it: the line of the owner's,
        // a line in another library file, a second line of the same file.
        let left = named.len() - usize::from(exact);
        if file_goes {
            if left == 0 {
                plan.notes.push(note.path.clone());
            } else {
                plan.held.push(note.path.clone());
            }
        }
    }
    for picture in &record.pictures {
        if picture.created && later_run_wrote(&picture.path) {
            plan.later.push(picture.path.clone());
        } else if !picture.created {
            plan.before.push(picture.path.clone());
        } else if picture.path.symlink_metadata().is_err() {
            plan.gone.push(picture.path.clone());
        } else if digest_of(&picture.path).is_some_and(|d| d.eq_ignore_ascii_case(&picture.sha256))
        {
            plan.pictures.push(picture.path.clone());
        } else {
            plan.kept.push(picture.path.clone());
            plan.pictures_kept.push(picture.path.clone());
        }
    }
    if !removals.is_empty() {
        plan.links = vec![(record.group.file.clone(), removals)];
    }
    // The group's file when this run made it, and the folders this run made, deepest first.
    if record.group.created && record.group.file.is_file() {
        plan.group_file = Some(record.group.file.clone());
    }
    let mut folders = record.folders.clone();
    folders.sort_by_key(|f| std::cmp::Reverse(f.components().count()));
    plan.folders = folders;
    (plan, Some(reach))
}

/// The notes and pictures that the runs started after `record` name, from the records in
/// `library/.runs` that are not undone. A run that died before its notes had marked them as its
/// own (A F9); a later run of the same plan found the names free and wrote them, so they are that
/// run's; or it found them there with its bytes and listed them, and counts on them (review of
/// the audit 4 fixes, S2). Undo of the earlier run leaves them. Only the records named as
/// started later are read, and not a rewrite's or a move's, which make no note: undo's question
/// does not read megabytes of originals for this. Two runs started in the same second are not
/// told apart.
fn claimed_later(record: &Record, library: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(library.join(RUNS_DIR)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
        if !path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("json"))
        {
            continue;
        }
        if !secs_of(&name_stamp(&path)).is_some_and(|s| s > record.started_secs) {
            continue;
        }
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let kind = stem.get(21..).unwrap_or_default();
        if kind.starts_with(Kind::Rewrite.as_str()) || kind.starts_with(Kind::Move.as_str()) {
            continue;
        }
        let Ok(other) = inspect(&path, library) else {
            continue;
        };
        if !other.state.live() || other.started_secs <= record.started_secs {
            continue;
        }
        out.extend(other.notes.iter().map(|n| n.path.clone()));
        out.extend(other.pictures.iter().map(|p| p.path.clone()));
    }
    out
}

/// What undo did.
#[derive(Clone, Debug, Default)]
pub struct UndoReport {
    pub refusal: Option<String>,
    pub plan: Option<UndoPlan>,
    pub removed_notes: Vec<PathBuf>,
    pub removed_pictures: Vec<PathBuf>,
    pub removed_links: usize,
    pub kept: Vec<PathBuf>,
    pub links_kept: Vec<PathBuf>,
    /// Notes this run did not create: they were there already, and stay.
    pub before: Vec<PathBuf>,
    /// Notes and pictures a later run wrote under names this run had marked: they stay
    /// ([`UndoPlan::later`]).
    pub later: Vec<PathBuf>,
    /// Notes kept because a library file still lists them, whether the line could not be taken
    /// out, the owner edited or moved it, or another file lists the note too.
    pub held_back: Vec<PathBuf>,
    pub group_removed: bool,
    pub group_kept: bool,
    pub folders_removed: Vec<PathBuf>,
    pub folders_kept: Vec<PathBuf>,
    /// Library files that could not be updated, and why.
    pub failures: Vec<Failure>,
    /// Notes and pictures the bin would not take ([`undo_with`]), and why: they are kept.
    pub bin_failures: Vec<Failure>,
    /// A rewrite's originals put back.
    pub restored: Vec<PathBuf>,
    /// A rewrite's originals changed since the run: kept.
    pub restore_kept: Vec<PathBuf>,
    /// The record was marked "undone".
    pub marked: bool,
    pub mark_error: Option<String>,
}

/// The library text with the run's lines taken out and then the section headings the run wrote
/// that have nothing under them: both removals in one text, `Some` when either changed
/// anything. The one composition every gate uses, the window's and the tests', so that what the
/// tests check is what the program does.
pub fn with_removals(
    text: &str,
    links: &[LinkRemoval],
    headings: &[Vec<String>],
    base: &Path,
) -> Option<String> {
    let without = with_links_removed(text, links, base);
    let swept = with_headings_removed(without.as_deref().unwrap_or(text), headings, base);
    swept
        .or(without)
        .map(|new| without_new_trailing_blanks(text, new))
}

/// [`with_removals`] for a library file whose add the record holds. While the file holds byte
/// for byte the text the add left (`added.written`) and taking the run's lines out leaves the
/// notes and the sections it had before, the text it had before comes back whole: its blank
/// lines, its line ends and its last line as they were (review of the audit 4 fixes, S3).
/// Otherwise (the file changed since, or some of the run's lines stay) the lines are taken out
/// as `with_removals` does. A byte order mark at the start of `text` is kept.
pub fn with_removals_added(
    text: &str,
    links: &[LinkRemoval],
    headings: &[Vec<String>],
    base: &Path,
    added: &Added,
) -> Option<String> {
    let removed = with_removals(text, links, headings, base)?;
    let (bom, body) = match text.strip_prefix('\u{FEFF}') {
        Some(rest) => ("\u{FEFF}", rest),
        None => ("", text),
    };
    let unchanged = crate::update::sha256_hex(body.as_bytes()).eq_ignore_ascii_case(&added.written);
    if unchanged {
        let was = library::parse(&added.before, base);
        let now = library::parse(&removed, base);
        let same_notes = was.entries.len() == now.entries.len()
            && was.entries.iter().zip(&now.entries).all(|(a, b)| {
                same_path(&a.path, &b.path) && a.sections == b.sections && a.title == b.title
            });
        let same_sections = was.sections.len() == now.sections.len()
            && was
                .sections
                .iter()
                .zip(&now.sections)
                .all(|(a, b)| sections_eq(a, b));
        if same_notes && same_sections {
            return Some(format!("{bom}{}", added.before));
        }
    }
    Some(removed)
}

/// A library file's text as its writers read it: the bytes without a UTF-8 byte order mark,
/// when they are UTF-8 and no bigger than the reader takes; `None` otherwise.
fn library_text_of(file: &Path) -> Option<String> {
    let bytes = read_regular(file, library::MAX_FILE_BYTES as u64, true)?;
    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    String::from_utf8(body.to_vec()).ok()
}

/// `new` without the blank lines at its end when `old` ended with a line of text: a blank line
/// that is last only because the lines after it were taken out, as the one a run's first link
/// is written below (audit 4, C11). Blank lines the file ended with before are kept.
fn without_new_trailing_blanks(old: &str, new: String) -> String {
    let ended_blank = |t: &str| t.lines().last().is_some_and(|l| l.trim().is_empty());
    if ended_blank(old) || !ended_blank(&new) {
        return new;
    }
    let eol = if new.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<&str> = new.lines().collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let mut out = lines.join(eol);
    if !out.is_empty() {
        out.push_str(eol);
    }
    out
}

/// Undoes one run: the links, then the files, then the group's file, then the folders, and the
/// record is marked "undone" and kept. A run whose links could not be taken out is not undone:
/// the notes those lines name stay, and the record says "links pending" so it can be tried again.
/// A note is removed only when, after the links step, no library file lists it: the library is
/// read again for that, so whatever kept a line (an edit, a move, a refusal) keeps its note.
pub fn undo(path: &Path, record: &Record, library: &Path, gate: &mut dyn Gate) -> UndoReport {
    undo_with(path, record, library, gate, &mut Delete)
}

/// [`undo`], with the notes and the pictures it removes handed to `bin`: the Recycle Bin, once
/// it is built (4.5, rule 3; D-071). A file the bin refuses is kept, and listed with the reason
/// in `bin_failures`.
pub fn undo_with(
    path: &Path,
    record: &Record,
    library: &Path,
    gate: &mut dyn Gate,
    bin: &mut dyn Bin,
) -> UndoReport {
    let mut report = UndoReport::default();
    let (plan, reach) = plan_of(record, library);
    if let Some(why) = &plan.refusal {
        report.refusal = Some(why.clone());
        report.plan = Some(plan);
        return report;
    }
    if record.kind == Kind::Rewrite {
        return undo_rewrite(path, record, library, plan, report);
    }
    let Some(reach) = reach else {
        report.refusal = plan.refusal.clone();
        report.plan = Some(plan);
        return report;
    };
    // 2. The links first, so the pane never shows a link to a file that is gone.
    for (file, removals) in &plan.links {
        let headings = if same_path(file, &record.group.file) {
            record.sections_added.as_slice()
        } else {
            &[]
        };
        let removed = match record.added.iter().find(|a| same_path(&a.file, file)) {
            Some(added) => gate.remove_links_added(file, removals, headings, added),
            None => gate.remove_links(file, removals, headings),
        };
        match removed {
            Ok(()) => report.removed_links += removals.len(),
            Err(why) => report.failures.push(Failure::new(file, why)),
        }
    }
    // 3. The library again, from the files: a note is removed only if no library file lists it
    //    now. The lines the links step could not take out, a line moved or edited since the
    //    plan was made and a second library file that lists the note all keep it here.
    let listed = list_library(library);
    for held in &plan.held {
        report.held_back.push(held.clone());
    }
    let created_note = |p: &Path| {
        record
            .notes
            .iter()
            .find(|n| n.created && same_path(&n.path, p))
    };
    for note in &plan.notes {
        let still_listed = !listed.complete || !named_in(&listed, note).is_empty();
        if still_listed {
            report.held_back.push(note.clone());
            continue;
        }
        let matches = created_note(note).is_some_and(|n| {
            reach.allows(note)
                && has_extension(note, &["md"])
                && digest_of(note).is_some_and(|d| d.eq_ignore_ascii_case(&n.sha256))
        });
        if !matches {
            report.kept.push(note.clone());
            continue;
        }
        match bin.recycle(note) {
            Ok(()) => report.removed_notes.push(note.clone()),
            Err(why) => {
                report.bin_failures.push(Failure::new(note, why));
                report.kept.push(note.clone());
            }
        }
    }
    for picture in &plan.pictures {
        let matches = record
            .pictures
            .iter()
            .find(|p| p.created && same_path(&p.path, picture))
            .is_some_and(|p| {
                reach.allows(picture)
                    && has_extension(picture, &PICTURE_EXTENSIONS)
                    && digest_of(picture).is_some_and(|d| d.eq_ignore_ascii_case(&p.sha256))
            });
        if !matches {
            report.kept.push(picture.clone());
            continue;
        }
        match bin.recycle(picture) {
            Ok(()) => report.removed_pictures.push(picture.clone()),
            Err(why) => {
                report.bin_failures.push(Failure::new(picture, why));
                report.kept.push(picture.clone());
            }
        }
    }
    // 4. The group's library file, only when this run made it and nothing but its heading and
    //    its `home:` line is left in it. Not while a note the bin would not take is still on
    //    disk: its `home:` is what lets the next undo reach that note (audit 4, A F2).
    if let Some(file) = &plan.group_file {
        if !report.bin_failures.is_empty() {
            report.group_kept = true;
        } else if reach.is_library_file(file)
            && file.symlink_metadata().is_ok_and(|m| m.is_file())
            && only_heading_and_home(file)
        {
            if std::fs::remove_file(file).is_ok() {
                report.group_removed = true;
            } else {
                report.group_kept = true;
            }
        } else {
            report.group_kept = true;
        }
    }
    // 5. The folders this run made, only while empty, and never a link to one.
    for folder in &plan.folders {
        let real = folder
            .symlink_metadata()
            .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink());
        if real && reach.allows(folder) && std::fs::remove_dir(folder).is_ok() {
            report.folders_removed.push(folder.clone());
        } else {
            report.folders_kept.push(folder.clone());
        }
    }
    // 6. The record is marked and stays in the list, but only when its links and its notes
    //    went: a run whose lines are still in a library file, or whose note the bin would not
    //    take, is not undone, and undo can be pressed again (audit 4, A F2).
    let mut marked = record.clone();
    marked.state = if report.failures.is_empty() && report.bin_failures.is_empty() {
        State::Undone
    } else {
        State::UndoPending
    };
    match marked.close(path, library) {
        Ok(()) => report.marked = true,
        Err(e) => report.mark_error = Some(e.to_string()),
    }
    report.kept.extend(plan.kept.iter().cloned());
    report.links_kept = plan.links_kept.clone();
    report.before = plan.before.clone();
    report.later = plan.later.clone();
    report.plan = Some(plan);
    report
}

/// Undo of a rewrite (5.6, D-074): puts each original back while the file still holds what the
/// run wrote, and keeps any file changed since. Nothing is removed, and nothing is written to a
/// path the record does not name.
fn undo_rewrite(
    path: &Path,
    record: &Record,
    library: &Path,
    plan: UndoPlan,
    mut report: UndoReport,
) -> UndoReport {
    report.kept.extend(plan.restore_kept.iter().cloned());
    report.restore_kept = plan.restore_kept.clone();
    for (first, last) in rewrite_chains(record) {
        if !plan.restore.iter().any(|p| same_path(p, &first.path)) {
            continue;
        }
        // Read again just before writing: the file may have changed since the plan was made.
        let same = digest_of(&first.path).is_some_and(|d| d.eq_ignore_ascii_case(&last.written));
        if !same {
            report.restore_kept.push(first.path.clone());
            report.kept.push(first.path.clone());
            continue;
        }
        let bytes = encoded(&first.text, first.bom);
        match crate::files::replace(&first.path, &bytes) {
            Ok(()) => report.restored.push(first.path.clone()),
            Err(e) => report
                .failures
                .push(Failure::new(&first.path, e.to_string())),
        }
    }
    let mut marked = record.clone();
    marked.state = if report.failures.is_empty() {
        State::Undone
    } else {
        State::UndoPending
    };
    match marked.close(path, library) {
        Ok(()) => report.marked = true,
        Err(e) => report.mark_error = Some(e.to_string()),
    }
    report.plan = Some(plan);
    report
}

/// A rewrite's originals, one pair per file in the order they were first written: the original
/// undo puts back, and the last one (whose `written` the file holds when nothing changed it
/// since). For a file rewritten more than once the first is the earliest original of the unbroken
/// chain that ends at the last, each one written over what the one before it wrote: a text the
/// owner put in the file between two of the run's writes is the farthest back undo goes, so an
/// edit of the owner's is never undone with the run.
fn rewrite_chains(record: &Record) -> Vec<(&Original, &Original)> {
    let mut chains: Vec<(&Original, &Original)> = Vec::new();
    for original in &record.originals {
        match chains
            .iter_mut()
            .find(|(first, _)| same_path(&first.path, &original.path))
        {
            Some((first, last)) => {
                if !original.sha256.eq_ignore_ascii_case(&last.written) {
                    *first = original;
                }
                *last = original;
            }
            None => chains.push((original, original)),
        }
    }
    chains
}

/// Whether a library file holds nothing but its group's heading and its `home:` line.
fn only_heading_and_home(path: &Path) -> bool {
    let Some(bytes) = read_regular(path, library::MAX_FILE_BYTES as u64, false) else {
        return false;
    };
    let text = crate::files::decode(&bytes).0.into_owned();
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines.len() == 2
        && lines[0].starts_with("# ")
        && lines[1]
            .split_once(':')
            .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case("home"))
}

// ---- finishing -----------------------------------------------------------------------------

/// What Finish did: the links it added, and the record it closed.
#[derive(Clone, Debug, Default)]
pub struct FinishReport {
    pub added: usize,
    /// Notes the library file lists already: nothing to add for them.
    pub present: usize,
    /// Notes that are not on disk with the bytes the run wrote: gone, or changed since. They
    /// cannot be written again from the record, and are left out (audit 3, C4).
    pub left_out: Vec<PathBuf>,
    pub failures: Vec<Failure>,
    pub state: Option<State>,
    pub error: Option<String>,
}

/// Finishes a run that stopped before its links were written: every recorded note that is on
/// disk with the right bytes is linked, and the record is closed. A note that is not there
/// cannot be written again from the record (it holds ranges, not text), and is left out.
pub fn finish(path: &Path, record: &Record, library: &Path, gate: &mut dyn Gate) -> FinishReport {
    let mut report = FinishReport::default();
    if !record.state.live() {
        report.error = Some("This run is undone already.".to_string());
        return report;
    }
    if record.state == State::UndoPending {
        report.error =
            Some("This run is being undone: Undo it again to take the rest of it out.".to_string());
        return report;
    }
    if record.kind == Kind::Move {
        report.error = Some("A move has nothing to finish: it is put back or undone.".to_string());
        return report;
    }
    if record.kind == Kind::Rewrite {
        report.error = Some(
            "A rewrite has nothing to finish: its files are written or left alone.".to_string(),
        );
        return report;
    }
    // Finish writes into the group's library file, so it reaches no further than undo does.
    if let Err(why) = Reach::of(record, library) {
        report.error = Some(why);
        return report;
    }
    let present = read_regular(&record.group.file, library::MAX_FILE_BYTES as u64, true)
        .map(|b| library::parse(&crate::files::decode(&b).0, library));
    let mut links: Vec<Link> = Vec::new();
    // The headings the file does not have yet, as the write works them out: the run's own, so
    // undo takes them out again with its lines (audit 4, A F4).
    let mut added: Vec<Vec<String>> = Vec::new();
    for note in &record.notes {
        if !digest_of(&note.path).is_some_and(|d| d.eq_ignore_ascii_case(&note.sha256)) {
            report.left_out.push(note.path.clone());
            continue;
        }
        let already = present.as_ref().is_some_and(|p| {
            p.entries
                .iter()
                .any(|e| same_path(&e.path, &note.path) && sections_eq(&e.sections, &note.sections))
        });
        if already {
            report.present += 1;
            continue;
        }
        for n in 1..=note.sections.len() {
            let path: Vec<String> = note.sections[..n].to_vec();
            let known = present
                .as_ref()
                .is_some_and(|p| p.sections.iter().any(|s| sections_eq(s, &path)));
            if !known
                && !added.iter().any(|s| sections_eq(s, &path))
                && !record.sections_added.iter().any(|s| sections_eq(s, &path))
            {
                added.push(path);
            }
        }
        // The link is the recorded line, with the path as the line spells it: the same text a
        // write of the run would have put in the file, so that undo finds it as the run's own.
        let (title, written) = link_parts(&note.line);
        let path = if title.is_empty() || written.is_empty() {
            note.path.clone()
        } else {
            PathBuf::from(written)
        };
        links.push(Link {
            path,
            title,
            sections: note.sections.clone(),
            line: note.line.clone(),
        });
    }
    let mut closed = record.clone();
    if !links.is_empty() {
        let text_before = library_text_of(&record.group.file);
        match gate.add_links(&record.group.file, &links) {
            Ok(()) => {
                report.added = links.len();
                closed.sections_added.extend(added);
                closed.keep_added(&record.group.file, text_before, &links, library);
            }
            Err(why) => report.failures.push(Failure::new(&record.group.file, why)),
        }
    }
    closed.state = if report.failures.is_empty() {
        State::Done
    } else {
        State::LinksPending
    };
    match closed.close(path, library) {
        Ok(()) => report.state = Some(closed.state),
        Err(e) => report.error = Some(e.to_string()),
    }
    report
}

/// A recorded link line as its title and its path text: `- [Title](<path>)`.
fn link_parts(line: &str) -> (String, String) {
    let t = line.trim().trim_start_matches("- ").trim();
    if let Some(rest) = t.strip_prefix('[')
        && let Some((title, after)) = rest.split_once("](")
        && let Some(path) = after.strip_suffix(')')
    {
        let path = path.trim();
        let path = path
            .strip_prefix('<')
            .and_then(|p| p.strip_suffix('>'))
            .unwrap_or(path);
        return (title.to_string(), path.replace("%20", " "));
    }
    (String::new(), t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn scratch(tag: &str) -> PathBuf {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("mdshared-runs-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        dir
    }

    /// Audit 4, N2: a note whose path is 260 characters or more is inside its library; the
    /// long path is not read as outside it for its spelling.
    #[cfg(windows)]
    #[test]
    fn a_long_path_inside_the_library_is_inside_it() {
        let library = scratch("long");
        let note = library
            .join("a".repeat(100))
            .join("b".repeat(100))
            .join(format!("{}.md", "c".repeat(60)));
        write_file(&note, "# Long\n");
        assert!(note.as_os_str().len() >= 260, "{}", note.display());
        assert!(inside(&library, Path::new(""), &note));
        assert!(!inside(&library.join("a"), Path::new(""), &note));
        let _ = std::fs::remove_dir_all(&library);
    }

    fn write_file(path: &Path, text: &str) {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).expect("its folder");
        }
        std::fs::write(path, text).expect("a file");
    }

    /// The window's gate, without a window: the same library writers, the same checks, one
    /// atomic write. A test can make it refuse.
    struct TestGate {
        refusals: Vec<PathBuf>,
        added: Vec<(PathBuf, usize)>,
        removed: Vec<(PathBuf, usize)>,
        /// A gate that has not opted in to [`Gate::remove_links_added`]: the trait's own.
        plain: bool,
    }

    impl TestGate {
        fn new() -> TestGate {
            TestGate {
                refusals: Vec::new(),
                added: Vec::new(),
                removed: Vec::new(),
                plain: false,
            }
        }
    }

    impl Gate for TestGate {
        fn add_links(&mut self, file: &Path, links: &[Link]) -> Result<(), String> {
            if self.refusals.iter().any(|p| same_path(p, file)) {
                return Err("The group's library file has unsaved changes.".to_string());
            }
            let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
            let base = file.parent().unwrap_or(Path::new(""));
            let new = with_links_added(&text, links);
            if !check_links_added(
                &library::parse(&text, base),
                &library::parse(&new, base),
                links,
                base,
            ) {
                return Err(format!(
                    "the file would not read back as its notes: old {:?} new {:?} links {:?} text {:?}",
                    library::parse(&text, base)
                        .entries
                        .iter()
                        .map(|e| (&e.path, &e.sections, &e.title))
                        .collect::<Vec<_>>(),
                    library::parse(&new, base)
                        .entries
                        .iter()
                        .map(|e| (&e.path, &e.sections, &e.title))
                        .collect::<Vec<_>>(),
                    links
                        .iter()
                        .map(|l| (&l.path, &l.sections, &l.title))
                        .collect::<Vec<_>>(),
                    new
                ));
            }
            crate::settings::write_atomic(file, &new).map_err(|e| e.to_string())?;
            self.added.push((file.to_path_buf(), links.len()));
            Ok(())
        }

        fn remove_links(
            &mut self,
            file: &Path,
            links: &[LinkRemoval],
            headings: &[Vec<String>],
        ) -> Result<(), String> {
            if self.refusals.iter().any(|p| same_path(p, file)) {
                return Err("The group's library file has unsaved changes.".to_string());
            }
            self.remove_with(file, links, |text, base| {
                with_removals(text, links, headings, base)
            })
        }

        fn remove_links_added(
            &mut self,
            file: &Path,
            links: &[LinkRemoval],
            headings: &[Vec<String>],
            added: &Added,
        ) -> Result<(), String> {
            if self.plain {
                return self.remove_links(file, links, headings);
            }
            if self.refusals.iter().any(|p| same_path(p, file)) {
                return Err("The group's library file has unsaved changes.".to_string());
            }
            self.remove_with(file, links, |text, base| {
                with_removals_added(text, links, headings, base, added)
            })
        }
    }

    impl TestGate {
        /// One removal, composed by `change`, checked and written as the window's gate does.
        fn remove_with(
            &mut self,
            file: &Path,
            links: &[LinkRemoval],
            change: impl FnOnce(&str, &Path) -> Option<String>,
        ) -> Result<(), String> {
            let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
            let base = file.parent().unwrap_or(Path::new(""));
            let new = change(&text, base).ok_or_else(|| "the file was not changed".to_string())?;
            if !check_links_removed(
                &library::parse(&text, base),
                &library::parse(&new, base),
                links,
                base,
            ) {
                return Err("the file would not read back as its notes".to_string());
            }
            crate::settings::write_atomic(file, &new).map_err(|e| e.to_string())?;
            self.removed.push((file.to_path_buf(), links.len()));
            Ok(())
        }
    }

    /// A plan of three notes in `library/Trip`, and a new group `Trip.md`.
    fn plan(library: &Path, source: &Path, notes: &[(&str, &str, &[&str])]) -> Request {
        let home = library.join("Trip");
        let mut items = Vec::new();
        for (i, (name, text, sections)) in notes.iter().enumerate() {
            let sections: Vec<String> = sections.iter().map(|s| s.to_string()).collect();
            let folder = library::note_folder(&home, &sections);
            let path = folder.join(format!("{name}.md"));
            items.push(NewNote {
                link_path: path
                    .strip_prefix(library)
                    .map(Path::to_path_buf)
                    .unwrap_or(path.clone()),
                path,
                text: text.to_string(),
                from_source: text.to_string(),
                title: name.to_string(),
                sections,
                ranges: vec![(i + 1, i + 1)],
                heading_shift: 0,
                title_added: false,
                already: false,
            });
        }
        let mut folders: Vec<PathBuf> = items
            .iter()
            .filter_map(|n| n.path.parent().map(Path::to_path_buf))
            .collect();
        folders.push(home.clone());
        folders.sort_by_key(|f| f.components().count());
        folders.dedup();
        Request {
            library: library.to_path_buf(),
            source: source.to_path_buf(),
            instruction: String::new(),
            sent: None,
            group: Group {
                name: "Trip".to_string(),
                file: library.join("Trip.md"),
                created: true,
                home,
            },
            folders,
            notes: items,
            pictures: Vec::new(),
        }
    }

    fn source(dir: &Path) -> PathBuf {
        let path = dir.join("Trip journal.md");
        write_file(&path, "# Japan\n\none\n\n## Asakusa\n\ntwo\n");
        path
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn a_write_makes_the_folders_the_notes_the_links_and_the_record() {
        let dir = scratch("ok");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(
            &library,
            &source,
            &[
                ("Asakusa", "# Asakusa\n", &["Tokyo"]),
                ("Nara", "# Nara\n", &[]),
            ],
        );
        let mut gate = TestGate::new();
        let report = write_at(
            &req,
            &mut gate,
            secs_of("2026-11-21T14:03:11Z").expect("the stamp"),
            "2026-11-21T14:03:11Z",
        );
        assert_eq!(report.start_error, None);
        assert_eq!(report.notes(), 2);
        assert!(
            report.note_failures.is_empty(),
            "{:?}",
            report.note_failures
        );
        assert!(
            report.check_failures.is_empty(),
            "{:?}",
            report.check_failures
        );
        assert!(
            report.link_failures.is_empty(),
            "{:?}",
            report.link_failures
        );
        assert_eq!(report.state(), Some(State::Done));
        assert_eq!(read(&library.join("Trip/Tokyo/Asakusa.md")), "# Asakusa\n");
        let group = read(&library.join("Trip.md"));
        assert!(group.starts_with("# Trip\n"), "{group}");
        assert!(group.contains("home:"), "{group}");
        assert!(
            group.contains("- [Asakusa](Trip/Tokyo/Asakusa.md)"),
            "{group}"
        );
        // The record, its done file, and the digest of what was written.
        let record_path = report.record_path.expect("a record");
        assert!(record_path.ends_with("2026-11-21T14-03-11Z-split.json"));
        let text = read(&record_path);
        assert!(text.contains("\"state\": \"done\""), "{text}");
        assert!(text.contains("\"kind\": \"split\""), "{text}");
        let loaded = load(&record_path, &library).expect("the record reads");
        assert_eq!(loaded.notes.len(), 2);
        assert_eq!(
            loaded.notes[0].sha256,
            crate::update::sha256_hex(b"# Asakusa\n")
        );
        assert_eq!(loaded.state, State::Done);
        assert_eq!(
            loaded.source.sha256,
            crate::update::sha256_hex(&std::fs::read(&source).expect("the source"))
        );
        assert!(record_path.with_extension("done").is_file());
        // The source is untouched.
        assert_eq!(read(&source), "# Japan\n\none\n\n## Asakusa\n\ntwo\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_markdown_run_writes_the_model_text_and_undo_takes_it_out_again() {
        let dir = scratch("markdown");
        let library = dir.join("library");
        std::fs::create_dir_all(library.join("Notes")).expect("the library");
        let source = dir.join("pasta notes.txt");
        write_file(&source, "PASTA\n\nboil it.\n");
        let group = library.join("Notes.md");
        write_file(&group, "# Notes\nhome: Notes\n");
        let note = library.join("Notes/pasta-notes.md");
        let model = "# Pasta\n\n- Boil it.\n";
        let req = Request {
            library: library.clone(),
            source: source.clone(),
            instruction: "Format this text as Markdown".to_string(),
            sent: Some(Sent {
                connection: "claude-code".to_string(),
                model: "a model".to_string(),
                bytes: 20,
                requests: 1,
            }),
            group: Group {
                name: "Notes".to_string(),
                file: group.clone(),
                created: false,
                home: library.join("Notes"),
            },
            folders: Vec::new(),
            notes: vec![NewNote {
                link_path: PathBuf::from("Notes/pasta-notes.md"),
                path: note.clone(),
                text: model.to_string(),
                from_source: model.to_string(),
                title: "pasta notes".to_string(),
                sections: Vec::new(),
                ranges: Vec::new(),
                heading_shift: 0,
                title_added: false,
                already: false,
            }],
            pictures: Vec::new(),
        };
        let mut gate = TestGate::new();
        let report = write_note(&req, &mut gate);
        assert_eq!(report.start_error, None);
        assert_eq!(report.notes(), 1);
        assert!(
            report.note_failures.is_empty(),
            "{:?}",
            report.note_failures
        );
        assert_eq!(report.state(), Some(State::Done));
        // The note holds the model's text, and the group lists it in its own notes.
        assert_eq!(read(&note), model);
        assert_eq!(
            read(&group),
            "# Notes\nhome: Notes\n\n- [pasta notes](Notes/pasta-notes.md)\n"
        );
        // The source was read and never written.
        assert_eq!(read(&source), "PASTA\n\nboil it.\n");
        let record_path = report.record_path.clone().expect("a record");
        let record_name = record_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        assert!(record_name.ends_with("-markdown.json"), "{record_path:?}");
        let text = read(&record_path);
        assert!(text.contains("\"kind\": \"markdown\""), "{text}");
        let loaded = load(&record_path, &library).expect("the record reads");
        assert_eq!(loaded.kind, Kind::Markdown);
        assert_eq!(
            loaded.notes[0].sha256,
            crate::update::sha256_hex(model.as_bytes())
        );
        assert!(loaded.notes[0].created);
        // Undo: the line first, then the note, and the source is still there.
        let plan = prepare(&loaded, &library);
        assert!(plan.refusal.is_none(), "{:?}", plan.refusal);
        assert!(plan.acts(), "something to undo");
        let mut gate = TestGate::new();
        let undone = undo(&record_path, &loaded, &library, &mut gate);
        assert_eq!(undone.refusal, None);
        assert_eq!(undone.removed_notes, vec![note.clone()]);
        assert!(!note.exists());
        // The blank line the link was written after stays, as it does for a split's undo.
        assert_eq!(read(&group).trim_end(), "# Notes\nhome: Notes");
        assert_eq!(read(&source), "PASTA\n\nboil it.\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_markdown_run_refuses_a_name_that_is_taken_with_other_bytes() {
        let dir = scratch("markdown-taken");
        let library = dir.join("library");
        std::fs::create_dir_all(library.join("Notes")).expect("the library");
        let source = dir.join("pasta notes.txt");
        write_file(&source, "PASTA\n");
        let group = library.join("Notes.md");
        write_file(&group, "# Notes\nhome: Notes\n");
        let note = library.join("Notes/pasta-notes.md");
        write_file(&note, "# Somebody else's note\n");
        let req = Request {
            library: library.clone(),
            source,
            instruction: String::new(),
            sent: None,
            group: Group {
                name: "Notes".to_string(),
                file: group.clone(),
                created: false,
                home: library.join("Notes"),
            },
            folders: Vec::new(),
            notes: vec![NewNote {
                link_path: PathBuf::from("Notes/pasta-notes.md"),
                path: note.clone(),
                text: "# Pasta\n".to_string(),
                from_source: "# Pasta\n".to_string(),
                title: "pasta notes".to_string(),
                sections: Vec::new(),
                ranges: Vec::new(),
                heading_shift: 0,
                title_added: false,
                already: false,
            }],
            pictures: Vec::new(),
        };
        let mut gate = TestGate::new();
        let report = write_note(&req, &mut gate);
        assert_eq!(report.notes(), 0);
        assert_eq!(report.note_failures.len(), 1, "{:?}", report.note_failures);
        // The file is not replaced and no line is written for it.
        assert_eq!(read(&note), "# Somebody else's note\n");
        assert_eq!(read(&group), "# Notes\nhome: Notes\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_one_leaves_nothing_at_all_when_the_record_cannot_be_written() {
        let dir = scratch("step1");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        // A file where the records' folder would go.
        write_file(&library.join(RUNS_DIR), "not a folder");
        let source = source(&dir);
        let req = plan(&library, &source, &[("Asakusa", "# Asakusa\n", &[])]);
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 1, "2026-11-21T14:03:11Z");
        let why = report.start_error.clone().expect("the run did not start");
        assert!(why.starts_with("The run could not start: "), "{why}");
        // Nothing: no folder, no note, no group, no link.
        let names: Vec<String> = std::fs::read_dir(&library)
            .expect("the library")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![RUNS_DIR], "{names:?}");
        assert!(!library.join("Trip").exists());
        assert!(gate.added.is_empty());
        assert_eq!(read(&source), "# Japan\n\none\n\n## Asakusa\n\ntwo\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_two_leaves_the_notes_that_needed_a_folder_it_could_not_make() {
        let dir = scratch("step2");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        // The group's folder is a file: no folder can be made inside it.
        write_file(&library.join("Trip"), "in the way");
        let source = source(&dir);
        let req = plan(&library, &source, &[("Asakusa", "# Asakusa\n", &["Tokyo"])]);
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 2, "2026-11-21T14:03:11Z");
        assert!(!report.folder_failures.is_empty());
        assert_eq!(report.notes(), 0);
        assert_eq!(report.note_failures.len(), 1);
        assert!(report.linked.is_empty(), "nothing is linked");
        // What is left: the file that was in the way, and a record saying what failed.
        assert_eq!(read(&library.join("Trip")), "in the way");
        let record = read(&report.record_path.clone().expect("a record"));
        assert!(record.contains("\"state\": \"done\""), "{record}");
        // The library file was made and holds no link.
        let group = read(&library.join("Trip.md"));
        assert!(!group.contains("Asakusa"), "{group}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_three_leaves_the_picture_out_and_writes_the_note_anyway() {
        let dir = scratch("step3");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let mut req = plan(&library, &source, &[("Asakusa", "# Asakusa\n", &[])]);
        req.pictures.push(NewPicture {
            source: dir.join("missing.png"),
            folder: library.join("Trip"),
        });
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 3, "2026-11-21T14:03:11Z");
        assert_eq!(report.picture_failures.len(), 1);
        assert!(!report.notes_written.is_empty());
        assert!(library.join("Trip/Asakusa.md").is_file());
        assert!(!library.join("Trip/missing.png").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_four_writes_the_others_and_never_replaces_the_file_that_was_there() {
        let dir = scratch("step4");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(
            &library,
            &source,
            &[("One", "# One\n", &[]), ("Two", "# Two\n", &[])],
        );
        // A file of the same name, holding the owner's words.
        write_file(&library.join("Trip/One.md"), "# Mine\n");
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 4, "2026-11-21T14:03:11Z");
        assert_eq!(report.note_failures.len(), 1);
        assert!(report.note_failures[0].why.contains("other contents"));
        assert_eq!(
            read(&library.join("Trip/One.md")),
            "# Mine\n",
            "never replaced"
        );
        assert_eq!(read(&library.join("Trip/Two.md")), "# Two\n");
        // Only the note that was written is linked.
        let group = read(&library.join("Trip.md"));
        assert!(!group.contains("One"), "{group}");
        assert!(group.contains("Two"), "{group}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_five_reports_a_note_that_does_not_read_back_and_leaves_the_file() {
        let dir = scratch("step5");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let mut req = plan(&library, &source, &[("One", "# Written\n", &[])]);
        // The text that was written and the lines it was made from do not agree: the check
        // catches what a broken writer would do.
        req.notes[0].from_source = "# Made from the source\n".to_string();
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 5, "2026-11-21T14:03:11Z");
        assert_eq!(report.check_failures.len(), 1);
        assert_eq!(report.check_failures[0].why, "does not read back the same");
        // What is left: the note as it was written, unlinked.
        assert_eq!(read(&library.join("Trip/One.md")), "# Written\n");
        let group = read(&library.join("Trip.md"));
        assert!(!group.contains("[One]"), "{group}");
        assert!(gate.added.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_six_leaves_the_notes_unlinked_and_the_record_says_links_pending() {
        let dir = scratch("step6");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(
            &library,
            &source,
            &[("One", "# One\n", &[]), ("Two", "# Two\n", &[])],
        );
        let mut gate = TestGate::new();
        gate.refusals.push(library.join("Trip.md"));
        let report = write_at(&req, &mut gate, 6, "2026-11-21T14:03:11Z");
        assert_eq!(report.state(), Some(State::LinksPending));
        assert_eq!(report.link_failures.len(), 1);
        // The notes are there, and the library file names neither.
        assert!(library.join("Trip/One.md").is_file());
        assert!(library.join("Trip/Two.md").is_file());
        let group = read(&library.join("Trip.md"));
        assert!(!group.contains("[One]"), "{group}");
        assert!(!group.contains("[Two]"), "{group}");
        // Finish links them, and closes the record.
        let record_path = report.record_path.clone().expect("a record");
        let record = load(&record_path, &library).expect("the record");
        let mut gate = TestGate::new();
        let done = finish(&record_path, &record, &library, &mut gate);
        assert_eq!(done.added, 2, "{:?}", done.failures);
        assert_eq!(done.state, Some(State::Done));
        let group = read(&library.join("Trip.md"));
        assert!(group.contains("[One]"), "{group}");
        let again = load(&record_path, &library).expect("the record");
        assert_eq!(again.state, State::Done);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_seven_leaves_the_run_whole_and_its_undo_refused_for_the_reason() {
        let dir = scratch("step7");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let runs = library.join(RUNS_DIR);
        std::fs::create_dir_all(&runs).expect("the runs folder");
        // A folder where the record's temporary name will be: the record can be made, and not
        // written again.
        std::fs::create_dir_all(runs.join("2026-11-21T14-03-11Z-split.json.tmp"))
            .expect("a folder in the way");
        let source = source(&dir);
        let req = plan(&library, &source, &[("One", "# One\n", &[])]);
        let mut gate = TestGate::new();
        let secs = secs_of("2026-11-21T14:03:11Z").expect("the stamp");
        let report = write_at(&req, &mut gate, secs, "2026-11-21T14:03:11Z");
        assert!(report.close_error.is_some(), "the close failed");
        assert!(!report.closed);
        // The run still counts: the note, the link, and the record at "writing".
        assert!(library.join("Trip/One.md").is_file());
        assert!(read(&library.join("Trip.md")).contains("[One]"));
        let record_path = report.record_path.clone().expect("a record");
        let record = load(&record_path, &library).expect("the record");
        assert_eq!(record.state, State::Writing);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_source_in_the_recycle_bin_that_is_not_back_refuses_undo_and_keeps_every_note() {
        let dir = scratch("undo-source");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(&library, &source, &[("One", "# One\n", &[])]);
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 7, "2026-11-21T14:03:11Z");
        let record_path = report.record_path.clone().expect("a record");
        let mut record = load(&record_path, &library).expect("the record");
        // As if phase T had put the source away.
        record.source.recycled = true;
        record.source.sha256 = crate::update::sha256_hex(b"another file");
        let plan = prepare(&record, &library);
        let why = plan.refusal.clone().expect("refused");
        assert_eq!(
            why,
            "Put \u{2018}Trip journal.md\u{2019} back from the Recycle Bin first, then undo."
        );
        assert!(plan.notes.is_empty(), "nothing would be removed");
        let mut gate = TestGate::new();
        let done = undo(&record_path, &record, &library, &mut gate);
        assert!(done.refusal.is_some());
        assert!(library.join("Trip/One.md").is_file(), "the note stays");
        assert!(read(&library.join("Trip.md")).contains("[One]"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A bin of the test's own: a folder it moves files into, and the calls it was given. No
    /// system bin is touched.
    struct TestBin {
        into: PathBuf,
        calls: Vec<PathBuf>,
        refuse: Option<String>,
    }

    impl TestBin {
        fn new(dir: &Path) -> TestBin {
            let into = dir.join("bin");
            std::fs::create_dir_all(&into).expect("the bin");
            TestBin {
                into,
                calls: Vec::new(),
                refuse: None,
            }
        }
        /// Where `path` went.
        fn held(&self, path: &Path) -> PathBuf {
            self.into.join(path.file_name().unwrap_or_default())
        }
    }

    impl Bin for TestBin {
        fn recycle(&mut self, path: &Path) -> Result<(), String> {
            self.calls.push(path.to_path_buf());
            if let Some(why) = &self.refuse {
                return Err(why.clone());
            }
            std::fs::rename(path, self.held(path)).map_err(|e| e.to_string())
        }
    }

    /// A written run of one note, ready for step 8.
    fn written(tag: &str) -> (PathBuf, PathBuf, PathBuf, Request, Report) {
        let dir = scratch(tag);
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(
            &library,
            &source,
            &[("One", "# One\n", &[]), ("Two", "# Two\n", &["Tokyo"])],
        );
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 9, "2026-11-21T14:03:11Z");
        assert_eq!(report.state(), Some(State::Done));
        (dir, library, source, req, report)
    }

    #[test]
    fn step_eight_puts_the_source_in_the_bin_last_and_the_record_says_so() {
        let (dir, library, source, req, mut report) = written("step8");
        let mut bin = TestBin::new(&dir);
        assert_eq!(
            recycle_source(&mut report, &req, &library, &mut bin),
            Ok(())
        );
        assert_eq!(bin.calls, vec![source.clone()]);
        assert!(!source.exists(), "the original is in the bin");
        assert!(bin.held(&source).is_file());
        // The record on disk says so, and undo asks for it back before anything goes.
        let record_path = report.record_path.clone().expect("a record");
        let record = load(&record_path, &library).expect("the record");
        assert!(record.source.recycled);
        assert_eq!(record.state, State::Done);
        assert!(read(&record_path.with_extension("done")).contains("recycled"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_eight_is_cancelled_by_a_note_that_was_not_written() {
        let dir = scratch("step8-note");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(
            &library,
            &source,
            &[("One", "# One\n", &[]), ("Two", "# Two\n", &[])],
        );
        // The owner's file where the second note would go: that note is not written.
        write_file(&library.join("Trip/Two.md"), "# Mine\n");
        let mut gate = TestGate::new();
        let mut report = write_at(&req, &mut gate, 10, "2026-11-21T14:03:11Z");
        assert_eq!(report.note_failures.len(), 1);
        let mut bin = TestBin::new(&dir);
        let why = recycle_source(&mut report, &req, &library, &mut bin).expect_err("cancelled");
        assert_eq!(why, "not every note was written, read back and listed");
        assert!(bin.calls.is_empty(), "the bin is never asked");
        assert!(source.is_file(), "the original is where it was");
        let record =
            load(&report.record_path.clone().expect("a record"), &library).expect("the record");
        assert!(!record.source.recycled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_eight_is_cancelled_by_a_note_that_does_not_read_back() {
        let dir = scratch("step8-check");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let mut req = plan(&library, &source, &[("One", "# Written\n", &[])]);
        req.notes[0].from_source = "# Made from the source\n".to_string();
        let mut gate = TestGate::new();
        let mut report = write_at(&req, &mut gate, 11, "2026-11-21T14:03:11Z");
        let mut bin = TestBin::new(&dir);
        assert!(recycle_source(&mut report, &req, &library, &mut bin).is_err());
        assert!(bin.calls.is_empty());
        assert!(source.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_eight_is_cancelled_when_the_library_file_was_not_updated() {
        let dir = scratch("step8-links");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(&library, &source, &[("One", "# One\n", &[])]);
        let mut gate = TestGate::new();
        gate.refusals.push(library.join("Trip.md"));
        let mut report = write_at(&req, &mut gate, 12, "2026-11-21T14:03:11Z");
        let mut bin = TestBin::new(&dir);
        assert!(recycle_source(&mut report, &req, &library, &mut bin).is_err());
        assert!(bin.calls.is_empty());
        assert!(source.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_eight_is_cancelled_by_a_source_changed_between_reading_and_writing() {
        let (dir, library, source, req, mut report) = written("step8-changed");
        // The owner saved the document while the notes were being written.
        write_file(&source, "# Japan\n\none, and a line more\n");
        let mut bin = TestBin::new(&dir);
        let why = recycle_source(&mut report, &req, &library, &mut bin).expect_err("cancelled");
        assert_eq!(why, "it has changed since the notes were cut from it");
        assert!(bin.calls.is_empty());
        assert_eq!(read(&source), "# Japan\n\none, and a line more\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn step_eight_that_the_bin_refuses_leaves_the_source_and_the_record_as_they_were() {
        let (dir, library, source, req, mut report) = written("step8-refused");
        let mut bin = TestBin::new(&dir);
        bin.refuse = Some("the item is too large for the Recycle Bin".to_string());
        let why = recycle_source(&mut report, &req, &library, &mut bin).expect_err("refused");
        assert_eq!(why, "the item is too large for the Recycle Bin");
        assert!(source.is_file());
        let record =
            load(&report.record_path.clone().expect("a record"), &library).expect("the record");
        assert!(!record.source.recycled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_is_refused_while_the_recycled_source_is_away_and_runs_once_it_is_back() {
        let (dir, library, source, req, mut report) = written("step8-undo");
        let mut bin = TestBin::new(&dir);
        recycle_source(&mut report, &req, &library, &mut bin).expect("recycled");
        let record_path = report.record_path.clone().expect("a record");
        let record = load(&record_path, &library).expect("the record");
        // Away: refused, and every note stays.
        let mut gate = TestGate::new();
        let done = undo(&record_path, &record, &library, &mut gate);
        assert_eq!(
            done.refusal.as_deref(),
            Some("Put \u{2018}Trip journal.md\u{2019} back from the Recycle Bin first, then undo.")
        );
        assert!(library.join("Trip/One.md").is_file());
        // Restored by the owner: undo runs.
        std::fs::rename(bin.held(&source), &source).expect("restored");
        let mut gate = TestGate::new();
        let done = undo(&record_path, &record, &library, &mut gate);
        assert_eq!(done.refusal, None);
        assert_eq!(done.removed_notes.len(), 2);
        assert!(!library.join("Trip/One.md").exists());
        assert!(source.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_with_a_bin_hands_it_the_notes_and_keeps_one_it_refuses() {
        let (dir, library, _source, _req, report) = written("undo-bin");
        let record_path = report.record_path.clone().expect("a record");
        let record = load(&record_path, &library).expect("the record");
        let mut bin = TestBin::new(&dir);
        let mut gate = TestGate::new();
        let done = undo_with(&record_path, &record, &library, &mut gate, &mut bin);
        assert_eq!(done.refusal, None);
        assert_eq!(done.removed_notes.len(), 2);
        assert_eq!(bin.calls.len(), 2, "both notes went through the bin");
        assert!(bin.held(&library.join("Trip/One.md")).is_file());
        assert!(!library.join("Trip/One.md").exists());

        // A bin that refuses keeps the note, and says why.
        let (dir2, library2, _s, _r, report2) = written("undo-bin-refused");
        let record_path2 = report2.record_path.clone().expect("a record");
        let record2 = load(&record_path2, &library2).expect("the record");
        let mut bin2 = TestBin::new(&dir2);
        bin2.refuse = Some("blocked by policy".to_string());
        let mut gate = TestGate::new();
        let done = undo_with(&record_path2, &record2, &library2, &mut gate, &mut bin2);
        assert!(done.removed_notes.is_empty());
        assert_eq!(done.bin_failures.len(), 2);
        assert_eq!(done.bin_failures[0].why, "blocked by policy");
        assert!(library2.join("Trip/One.md").is_file(), "kept");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn listed_in_names_the_groups_that_list_a_file() {
        let dir = scratch("listed-in");
        let library = dir.join("library");
        let note = dir.join("Trip journal.md");
        write_file(&note, "# Japan\n");
        write_file(
            &library.join("Trips.md"),
            &format!("# Trips\n\n- [Journal]({})\n", note.display()),
        );
        write_file(&library.join("Other.md"), "# Other\n\n- [Else](else.md)\n");
        assert_eq!(listed_in(&library, &note), vec!["Trips".to_string()]);
        assert!(listed_in(&library, &dir.join("nowhere.md")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_takes_the_links_out_first_then_the_notes_the_group_and_the_folders() {
        let dir = scratch("undo");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(
            &library,
            &source,
            &[("One", "# One\n", &["Tokyo"]), ("Two", "# Two\n", &[])],
        );
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 8, "2026-11-21T14:03:11Z");
        assert_eq!(report.state(), Some(State::Done));
        let record_path = report.record_path.clone().expect("a record");
        let record = load(&record_path, &library).expect("the record");
        let mut gate = TestGate::new();
        let done = undo(&record_path, &record, &library, &mut gate);
        assert_eq!(done.refusal, None);
        assert_eq!(done.removed_notes.len(), 2);
        assert_eq!(done.removed_links, 2);
        assert!(done.group_removed);
        assert_eq!(done.folders_removed.len(), 2, "{:?}", done.folders_removed);
        // Everything the run made is gone; the source is where it was.
        assert!(!library.join("Trip").exists());
        assert!(!library.join("Trip.md").exists());
        assert!(source.is_file());
        // The record is marked and stays.
        let again = load(&record_path, &library).expect("the record stays");
        assert_eq!(again.state, State::Undone);
        assert!(prepare(&again, &library).refusal.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_keeps_a_note_the_owner_changed_and_a_link_the_owner_edited() {
        let dir = scratch("undo-kept");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(
            &library,
            &source,
            &[("One", "# One\n", &[]), ("Two", "# Two\n", &[])],
        );
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 9, "2026-11-21T14:03:11Z");
        let record_path = report.record_path.clone().expect("a record");
        // The owner changes one note and edits the other's line in the library file.
        write_file(&library.join("Trip/One.md"), "# One, changed\n");
        let group = read(&library.join("Trip.md"));
        let edited = group.replace("[Two]", "[Two, renamed]");
        write_file(&library.join("Trip.md"), &edited);
        let record = load(&record_path, &library).expect("the record");
        let plan = prepare(&record, &library);
        assert_eq!(plan.kept.len(), 1, "{:?}", plan.kept);
        assert_eq!(plan.notes_kept.len(), 1, "{:?}", plan.notes_kept);
        assert_eq!(plan.links_kept.len(), 1, "{:?}", plan.links_kept);
        let words = plan.sentence("Trip");
        assert!(
            words.contains("1 note was changed since and is kept."),
            "{words}"
        );
        assert!(
            words.contains("1 line was edited or moved since and is kept, with its note."),
            "{words}"
        );
        let mut gate = TestGate::new();
        let done = undo(&record_path, &record, &library, &mut gate);
        // The line the owner edited is kept, and a note a library line still lists is kept with
        // it: no line is left naming a file that undo removed.
        assert!(done.removed_notes.is_empty(), "{:?}", done.removed_notes);
        assert_eq!(read(&library.join("Trip/Two.md")), "# Two\n");
        assert_eq!(read(&library.join("Trip/One.md")), "# One, changed\n");
        let group = read(&library.join("Trip.md"));
        assert!(
            group.contains("[One]"),
            "the kept note's line stays: {group}"
        );
        assert!(group.contains("[Two, renamed]"), "{group}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `prepare` reads the disk and `undo` reads it again, which looks like the same question
    /// asked twice until the file changes between them: the editor asks, the owner saves over a
    /// note while the question is on screen, and the answer is already stale. The second hash is
    /// the one that decides, so a note written to between the two calls is kept.
    #[test]
    fn a_note_changed_after_undo_was_worked_out_is_still_kept() {
        let dir = scratch("undo-raced");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(
            &library,
            &source,
            &[("One", "# One\n", &[]), ("Two", "# Two\n", &[])],
        );
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 9, "2026-11-21T14:03:11Z");
        let record_path = report.record_path.clone().expect("a record");
        let record = load(&record_path, &library).expect("the record");
        // Worked out while both notes are untouched: undo would remove both.
        let plan = prepare(&record, &library);
        assert_eq!(plan.notes.len(), 2, "{:?}", plan.notes);
        assert!(plan.kept.is_empty(), "{:?}", plan.kept);
        // The owner saves over one of them before pressing Undo.
        let one = library.join("Trip").join("One.md");
        write_file(&one, "# One, saved while the question was up\n");
        let mut gate = TestGate::new();
        let done = undo(&record_path, &record, &library, &mut gate);
        assert_eq!(
            done.removed_notes,
            vec![library.join("Trip/Two.md")],
            "only the note that is still ours goes"
        );
        assert_eq!(
            read(&one),
            "# One, saved while the question was up\n",
            "the note written between the two reads is kept, whatever the plan said"
        );
        assert!(
            done.kept.iter().any(|p| same_path(p, &one)),
            "{:?}",
            done.kept
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_leaves_a_group_file_that_holds_more_than_its_heading_and_its_home() {
        let dir = scratch("undo-group");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(&library, &source, &[("One", "# One\n", &[])]);
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 10, "2026-11-21T14:03:11Z");
        let record_path = report.record_path.clone().expect("a record");
        // The owner writes something of their own into the group's file.
        let group = format!("{}Mine to keep.\n", read(&library.join("Trip.md")));
        write_file(&library.join("Trip.md"), &group);
        let record = load(&record_path, &library).expect("the record");
        let mut gate = TestGate::new();
        let done = undo(&record_path, &record, &library, &mut gate);
        assert!(done.group_kept && !done.group_removed);
        let group = read(&library.join("Trip.md"));
        assert!(group.contains("Mine to keep."), "{group}");
        assert!(!group.contains("[One]"), "{group}");
        // The folder held no file of the owner's own, so it went.
        assert!(done.folders_removed.contains(&library.join("Trip")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A record is a file in `.runs`, and undo deletes and writes what it names. One that names
    /// a path outside the library folder and the group's folder is not a record this program
    /// wrote (the write refuses those, 4.1 and 6.2), so undo acts on none of it.
    #[test]
    fn undo_removes_nothing_when_the_record_names_a_path_outside_the_library_and_the_groups_folder()
    {
        let dir = scratch("undo-outside");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(&library, &source, &[("One", "# One\n", &[])]);
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 13, "2026-11-21T14:03:11Z");
        let record_path = report.record_path.clone().expect("a record");
        let good = load(&record_path, &library).expect("the record");
        // The owner's own files, with the digests a hostile record would carry: what it takes is
        // the path, not the hash.
        let mine = dir.join("My own notes.md");
        write_file(&mine, "words I wrote\n");
        let own_group = dir.join("My own group.md");
        write_file(&own_group, "# Mine\nhome: Mine\n");
        let outside_folder = dir.join("work");
        std::fs::create_dir_all(&outside_folder).expect("the folder");
        let digest = |text: &str| crate::update::sha256_hex(text.as_bytes());
        // The record a hostile `.runs` file holds: the owner's own note, with its real digest.
        let mut note = good.clone();
        note.notes[0].path = mine.clone();
        note.notes[0].sha256 = digest("words I wrote\n");
        // Undo refuses, and removes neither the owner's file nor the run's own note, which it
        // would otherwise take.
        let mut gate = TestGate::new();
        let done = undo(&record_path, &note, &library, &mut gate);
        assert!(done.refusal.is_some());
        assert!(gate.removed.is_empty(), "{:?}", gate.removed);
        assert_eq!(
            read(&mine),
            "words I wrote\n",
            "the owner's file is untouched"
        );
        assert_eq!(
            read(&library.join("Trip/One.md")),
            "# One\n",
            "and the run's own note is not removed either"
        );
        assert!(read(&library.join("Trip.md")).contains("[One]"));
        assert_eq!(
            load(&record_path, &library).expect("the record").state,
            State::Done
        );
        // The same refusal for a picture, a folder and the group's library file, and nothing is
        // planned for any of them either: the confinement is decided before any removal.
        let mut picture = good.clone();
        picture.pictures.push(PictureRec {
            path: mine.clone(),
            created: true,
            sha256: digest("words I wrote\n"),
        });
        let mut folder = good.clone();
        folder.folders = vec![outside_folder.clone()];
        let mut group = good.clone();
        group.group.file = own_group.clone();
        for record in [&note, &picture, &folder, &group] {
            let plan = prepare(record, &library);
            let why = plan.refusal.clone().expect("refused");
            assert!(
                why.ends_with(
                    "is outside the library and the group's folder, so this run is not undone."
                ),
                "{why}"
            );
            assert!(plan.notes.is_empty(), "{:?}", plan.notes);
            assert!(plan.pictures.is_empty(), "{:?}", plan.pictures);
            assert!(plan.folders.is_empty(), "{:?}", plan.folders);
            assert!(plan.links.is_empty(), "{:?}", plan.links);
            assert!(plan.group_file.is_none(), "{:?}", plan.group_file);
        }
        assert_eq!(read(&own_group), "# Mine\nhome: Mine\n");
        assert!(outside_folder.is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A library file the owner's editor wrote with CRLF comes back with CRLF: undo's own
    /// removal of a section heading keeps the file's endings, as every writer in
    /// `crate::library` does.
    #[test]
    fn undo_keeps_the_line_endings_of_a_crlf_library_file() {
        let dir = scratch("undo-crlf");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        // A group that is there already, so undo keeps its file and takes the heading out of it.
        write_file(&library.join("Trip.md"), "# Trip\nhome: Trip\n");
        let mut req = plan(&library, &source, &[("One", "# One\n", &["Tokyo"])]);
        req.group.created = false;
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 14, "2026-11-21T14:03:11Z");
        let record_path = report.record_path.clone().expect("a record");
        let lf = read(&library.join("Trip.md"));
        assert!(lf.contains("\n\n## Tokyo\n"), "{lf:?}");
        // The file with the endings the owner's editor gives it.
        write_file(&library.join("Trip.md"), &lf.replace('\n', "\r\n"));
        let record = load(&record_path, &library).expect("the record");
        let mut gate = TestGate::new();
        let done = undo(&record_path, &record, &library, &mut gate);
        assert_eq!(done.removed_links, 1);
        assert_eq!(done.removed_notes.len(), 1);
        let after = String::from_utf8(
            std::fs::read(library.join("Trip.md")).expect("the group's file stays"),
        )
        .expect("utf-8");
        assert!(
            !after.replace("\r\n", "").contains('\n'),
            "every line keeps CRLF: {after:?}"
        );
        assert!(after.starts_with("# Trip\r\nhome: Trip\r\n"), "{after:?}");
        assert!(!after.contains("## Tokyo"), "{after:?}");
        assert!(!after.contains("[One]"), "{after:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The group's library file is open with unsaved changes, so the gate refuses to take the
    /// lines out. The notes must stay, because their lines are still in the file, and the record
    /// must not say "undone": the press can be made again.
    #[test]
    fn an_undo_whose_links_could_not_be_taken_out_keeps_the_notes_and_is_not_undone() {
        let dir = scratch("undo-refused");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(
            &library,
            &source,
            &[("One", "# One\n", &[]), ("Two", "# Two\n", &[])],
        );
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 12, "2026-11-21T14:03:11Z");
        let record_path = report.record_path.clone().expect("a record");
        let record = load(&record_path, &library).expect("the record");
        let mut gate = TestGate::new();
        gate.refusals.push(library.join("Trip.md"));
        let done = undo(&record_path, &record, &library, &mut gate);
        assert_eq!(done.removed_links, 0);
        assert!(done.removed_notes.is_empty(), "{:?}", done.removed_notes);
        assert_eq!(done.held_back.len(), 2, "{:?}", done.held_back);
        assert_eq!(done.failures.len(), 1, "{:?}", done.failures);
        // Exactly what is left: both notes, both lines, and a record that can be tried again.
        assert_eq!(read(&library.join("Trip/One.md")), "# One\n");
        assert_eq!(read(&library.join("Trip/Two.md")), "# Two\n");
        let group = read(&library.join("Trip.md"));
        assert!(group.contains("[One]"), "{group}");
        assert!(group.contains("[Two]"), "{group}");
        let again = load(&record_path, &library).expect("the record stays");
        // "Undo pending", not "links pending": the owner asked for the run to go, so Finish is
        // not what comes next (audit 4, A F3).
        assert_eq!(again.state, State::UndoPending);
        assert!(again.state.live(), "Undo is offered again");
        assert!(!again.state.finishable());
        let mut gate = TestGate::new();
        let finished = finish(&record_path, &again, &library, &mut gate);
        assert!(finished.error.is_some(), "{finished:?}");
        assert_eq!(
            load(&record_path, &library).expect("the record").state,
            State::UndoPending
        );
        // With the file saved or closed, the same press finishes the job.
        let mut gate = TestGate::new();
        let done = undo(&record_path, &again, &library, &mut gate);
        assert!(done.failures.is_empty(), "{:?}", done.failures);
        assert_eq!(done.removed_links, 2);
        assert_eq!(done.removed_notes.len(), 2);
        assert!(done.marked);
        assert!(!library.join("Trip/One.md").exists());
        assert!(!library.join("Trip/Two.md").exists());
        assert_eq!(
            load(&record_path, &library)
                .expect("the record stays")
                .state,
            State::Undone
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_record_reads_back_with_its_paths_and_survives_a_hostile_file() {
        let dir = scratch("json");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let record = Record {
            version: 1,
            kind: Kind::Split,
            started: "2026-11-21T14:03:11Z".to_string(),
            started_secs: secs_of("2026-11-21T14:03:11Z").expect("the stamp"),
            state: State::Done,
            instruction: "one note per chapter".to_string(),
            sent: None,
            source: Source {
                path: dir.join("A \"quoted\" \\ file.md"),
                len: 12,
                sha256: "abc".to_string(),
                recycled: false,
            },
            group: Group {
                name: "Group".to_string(),
                file: library.join("Group.md"),
                created: true,
                home: library.join("Group"),
            },
            folders: vec![library.join("Group").join("Tokyo")],
            notes: vec![NoteRec {
                path: library.join("Group").join("Tokyo").join("One.md"),
                created: true,
                sha256: "def".to_string(),
                ranges: vec![(1, 5), (7, 9)],
                heading_shift: 1,
                title_added: false,
                sections: vec!["Tokyo".to_string()],
                line: "- [One](Group/Tokyo/One.md)".to_string(),
            }],
            pictures: Vec::new(),
            sections_added: vec![vec!["Tokyo".to_string()]],
            moved: None,
            originals: Vec::new(),
            added: Vec::new(),
        };
        let text = record.to_json(&library);
        let back = Record::from_json(&text, &library).expect("it reads");
        // The source's name holds a quote and a backslash. On Windows a backslash separates
        // the parts of a path; everywhere else it is a character of the name and must survive
        // the round trip as one.
        assert_eq!(back, record);
        assert!(text.contains("\"path\": \"Group/Tokyo/One.md\""), "{text}");
        // Nothing that is not a record, and no depth that can end the process.
        assert!(Record::from_json("", &library).is_none());
        assert!(Record::from_json("{}", &library).is_none());
        assert!(Record::from_json("[1,2,3]", &library).is_none());
        let deep = "[".repeat(10_000) + &"]".repeat(10_000);
        assert!(Record::from_json(&deep, &library).is_none());
        assert!(Record::from_json("{\"version\":1,\"kind\":\"split\"", &library).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_written_into_a_record_reads_back_as_the_same_path() {
        // A `Path` is the host's: only this machine's rule decides what is absolute and where
        // one part ends, so the round trip is tried with the host's own paths and names. What
        // *is* tried both ways is the one thing the record decides for itself — which
        // character separates the parts in the text it writes.
        let library = scratch("paths").join("library");
        let group = library.join("Group");
        // On Windows `\` separates the parts, so a name cannot hold one; everywhere else it is
        // an ordinary character of the name.
        let odd = if cfg!(windows) {
            "A 'quoted' file.md"
        } else {
            "A \\ file.md"
        };
        for name in ["One.md", odd] {
            let path = group.join(name);
            let text = stored(&path, &library);
            assert_eq!(resolved(&text, &library), path, "from {text}");
        }
        // The parts are written with `/` between them whatever this platform separates with,
        // so a library carried to another machine reads the same there.
        assert_eq!(stored(&group.join("One.md"), &library), "Group/One.md");

        // And the rule itself, both ways, on whatever machine runs this. Only a platform that
        // separates with `\` may turn one into `/`: where a backslash is part of a name,
        // writing it as `/` would make the record name a different file. A `Path` on Windows
        // reads `/` and `\` alike and would hide that, so the text is compared directly.
        let both = Path::new("/library/Group/A \\ file.md");
        let root = Path::new("/library");
        assert_eq!(stored_on(both, root, false), "Group/A \\ file.md");
        assert_eq!(stored_on(both, root, true), "Group/A / file.md");
        assert_eq!(
            resolved_on("Group/One.md", root, false),
            root.join("Group/One.md")
        );
        assert_eq!(
            resolved_on("Group/One.md", root, true),
            root.join("Group\\One.md")
        );
    }

    #[test]
    fn the_clock_writes_iso_and_a_name_windows_takes() {
        let (_, stamp_text) = now_utc();
        assert_eq!(stamp_text.len(), 20, "{stamp_text}");
        assert!(stamp_text.ends_with('Z'));
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(1_800_000_000), "2027-01-15T08:00:00Z");
        assert_eq!(secs_of("2027-01-15T08:00:00Z"), Some(1_800_000_000));
        assert_eq!(secs_of("not a stamp"), None);
        let name = stamp(1_800_000_000, Kind::Split);
        assert_eq!(name, "2027-01-15T08-00-00Z-split");
        assert!(!name.contains(':'), "{name}");
    }

    #[test]
    fn a_newer_record_sorts_first_and_the_old_ones_go_after_a_run() {
        let dir = scratch("list");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(&library, &source, &[("One", "# One\n", &[])]);
        let mut gate = TestGate::new();
        let a = write_at(&req, &mut gate, 1, "2026-11-21T14:00:01Z");
        let b = write_at(&req, &mut gate, 2, "2026-11-21T14:00:02Z");
        let runs = list(&library);
        assert_eq!(runs.len(), 2);
        assert_eq!(
            runs[0].path,
            b.record_path.clone().expect("the second record")
        );
        assert_eq!(runs[1].path, a.record_path.expect("the first record"));
        // Pruning keeps the newest twenty and everything of the last ninety days.
        let base = runs[0].record.clone();
        let mut paths = Vec::new();
        for secs in 1..=21u64 {
            let old = Record {
                started: utc(secs),
                started_secs: secs,
                ..base.clone()
            };
            paths.push(old.create(&library).expect("an old record"));
        }
        let now = 200 * 86_400;
        prune(&library, now);
        for path in &paths[..3] {
            assert!(!path.exists(), "the three oldest went");
        }
        for path in &paths[3..] {
            assert!(path.exists(), "the newest twenty stayed");
        }
        assert!(runs[0].path.exists(), "the new ones stayed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_link_is_added_under_its_section_and_read_back_exactly() {
        let base = if cfg!(windows) {
            Path::new(r"C:\lib")
        } else {
            Path::new("/lib")
        };
        let text = "# Group\n\nhome: Group\n";
        let link = Link {
            path: PathBuf::from("Group/Tokyo/One.md"),
            title: "One".to_string(),
            sections: vec!["Tokyo".to_string()],
            line: "- [One](Group/Tokyo/One.md)".to_string(),
        };
        let new = with_links_added(text, std::slice::from_ref(&link));
        assert!(new.contains("## Tokyo"), "{new}");
        assert!(new.contains(&link.line), "{new}");
        let old_p = library::parse(text, base);
        let new_p = library::parse(&new, base);
        assert!(check_links_added(
            &old_p,
            &new_p,
            std::slice::from_ref(&link),
            base,
        ));
        // A line that was edited is not removed.
        let edited = new.replace("[One]", "[One, edited]");
        let removal = LinkRemoval {
            path: link.path.clone(),
            sections: link.sections.clone(),
            line: link.line.clone(),
        };
        assert_eq!(
            with_links_removed(&edited, std::slice::from_ref(&removal), base),
            None
        );
        let removed =
            with_links_removed(&new, std::slice::from_ref(&removal), base).expect("removed");
        assert!(!removed.contains("[One]"), "{removed}");
    }

    #[test]
    fn nothing_is_written_outside_the_library_or_the_groups_folder() {
        let dir = scratch("outside");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let mut req = plan(&library, &source, &[("One", "# One\n", &[])]);
        req.notes[0].path = dir.join("Somewhere else.md");
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 11, "2026-11-21T14:03:11Z");
        let why = report.start_error.expect("refused");
        assert!(why.contains("outside the library"), "{why}");
        assert!(!dir.join("Somewhere else.md").exists());
        assert!(!library.join(RUNS_DIR).exists(), "not even a record");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A path with `..` in it is not taken as it stands, even where folding it would land inside
    /// the library: a run never writes one, so a record that holds one is not a record of a run.
    #[test]
    fn a_record_path_with_dots_in_it_is_refused_even_when_it_folds_to_a_path_inside() {
        let dir = scratch("dots-inside");
        let library = dir.join("library");
        write_file(&library.join("Trip/One.md"), "# One\n");
        let reach = Reach {
            library: library.clone(),
            home: None,
        };
        assert!(reach.allows(&library.join("Trip/One.md")));
        assert!(
            !reach.allows(&library.join("Trip/Other/../One.md")),
            "it names the note, and still it is refused"
        );
        assert!(!reach.allows(&library.join("..").join("library").join("Trip/One.md")));
        assert!(
            !reach.allows(&library),
            "the library folder itself is not a path of a run"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// File names compare as the disk compares them: `C:\Notes` and `c:\notes` are one folder on
    /// Windows and macOS, so a record that spells the library in another case is not refused for
    /// it, and `Trip` is not `Trips`.
    #[test]
    fn a_path_is_under_a_folder_part_by_part_in_the_case_the_disk_uses() {
        assert!(under(Path::new("/lib/Trip/One.md"), Path::new("/lib")));
        assert!(under(Path::new("/lib"), Path::new("/lib")));
        assert!(!under(Path::new("/library/One.md"), Path::new("/lib")));
        assert!(!under(Path::new("/lib"), Path::new("/lib/Trip")));
        if crate::paths::NAMES_IGNORE_CASE {
            assert!(under(Path::new("/LIB/trip/One.md"), Path::new("/lib/Trip")));
        } else {
            assert!(!under(
                Path::new("/LIB/trip/One.md"),
                Path::new("/lib/Trip")
            ));
        }
    }

    // ---- a rewrite's record (5.6, D-074, D-076) -------------------------------------------

    /// A rewrite of one file: the record made, the file written, and the record closed.
    fn rewrite_one(tag: &str, before: &str, after: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = scratch(tag);
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let file = dir.join("notes.md");
        write_file(&file, before);
        let (path, mut record) =
            start_rewrite(&library, "shorten it", None, &file).expect("a record");
        rewrite_into(
            &path,
            &mut record,
            &library,
            &RewriteFile {
                path: file.clone(),
                before: before.to_string(),
                after: after.to_string(),
                bom: false,
            },
        )
        .expect("the write");
        record.state = State::Done;
        record.close(&path, &library).expect("closed");
        (dir, library, path)
    }

    #[test]
    fn a_rewrite_record_holds_the_original_and_undo_puts_it_back() {
        let (dir, library, path) = rewrite_one("rewrite", "one\ntwo\n", "ONE\nTWO\n");
        assert_eq!(read(&dir.join("notes.md")), "ONE\nTWO\n");
        let text = std::fs::read_to_string(&path).expect("the record");
        assert!(text.contains("\"kind\": \"rewrite\""), "{text}");
        assert!(text.contains("\"originals\""), "{text}");
        assert!(
            text.contains("one\\ntwo\\n"),
            "the original is in the record"
        );
        let record = load(&path, &library).expect("it reads");
        assert_eq!(record.kind, Kind::Rewrite);
        assert_eq!(record.originals.len(), 1);
        assert_eq!(record.originals[0].text, "one\ntwo\n");
        assert!(!record.originals[0].bom);
        // Undo puts the text back, and marks the record.
        let plan = prepare(&record, &library);
        assert!(plan.refusal.is_none(), "{:?}", plan.refusal);
        assert_eq!(plan.restore, vec![dir.join("notes.md")]);
        assert!(plan.acts());
        let mut gate = TestGate::new();
        let report = undo(&path, &record, &library, &mut gate);
        assert_eq!(report.refusal, None);
        assert_eq!(report.restored, vec![dir.join("notes.md")]);
        assert_eq!(read(&dir.join("notes.md")), "one\ntwo\n");
        let after = load(&path, &library).expect("it still reads");
        assert_eq!(after.state, State::Undone);
        // A second undo says it is done already.
        let again = prepare(&after, &library);
        assert_eq!(
            again.refusal.as_deref(),
            Some("This run is undone already.")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rewrite_undo_keeps_a_file_changed_since_the_run() {
        let (dir, library, path) = rewrite_one("rewrite-kept", "one\n", "ONE\n");
        let file = dir.join("notes.md");
        write_file(&file, "the owner's newer text\n");
        let record = load(&path, &library).expect("it reads");
        let plan = prepare(&record, &library);
        assert!(plan.restore.is_empty());
        assert_eq!(plan.restore_kept, vec![file.clone()]);
        assert!(!plan.acts(), "nothing to put back");
        assert!(
            plan.sentence("").contains("1 file was"),
            "{}",
            plan.sentence("")
        );
        let mut gate = TestGate::new();
        let report = undo(&path, &record, &library, &mut gate);
        assert_eq!(read(&file), "the owner's newer text\n", "kept to the byte");
        assert_eq!(report.restored.len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rewrite_refuses_a_file_changed_before_it_is_written() {
        let dir = scratch("rewrite-changed");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let file = dir.join("notes.md");
        write_file(&file, "one\n");
        let (path, mut record) =
            start_rewrite(&library, "shorten it", None, &file).expect("a record");
        write_file(&file, "somebody else\n");
        let why = rewrite_into(
            &path,
            &mut record,
            &library,
            &RewriteFile {
                path: file.clone(),
                before: "one\n".to_string(),
                after: "ONE\n".to_string(),
                bom: false,
            },
        )
        .expect_err("changed");
        assert!(why.contains("changed since"), "{why}");
        assert_eq!(read(&file), "somebody else\n");
        let record = load(&path, &library).expect("the record reads");
        assert!(record.originals.is_empty(), "nothing was recorded");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rewrite_keeps_the_byte_order_mark_it_found() {
        let dir = scratch("rewrite-bom");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let file = dir.join("notes.md");
        std::fs::write(&file, [0xEF, 0xBB, 0xBF, b'o', b'n', b'e', b'\n']).expect("a file");
        let (path, mut record) =
            start_rewrite(&library, "shorten it", None, &file).expect("a record");
        rewrite_into(
            &path,
            &mut record,
            &library,
            &RewriteFile {
                path: file.clone(),
                before: "one\n".to_string(),
                after: "ONE\n".to_string(),
                bom: true,
            },
        )
        .expect("the write");
        assert_eq!(
            std::fs::read(&file).expect("bytes"),
            [0xEF, 0xBB, 0xBF, b'O', b'N', b'E', b'\n']
        );
        let mut gate = TestGate::new();
        let undone = undo(&path, &record, &library, &mut gate);
        assert_eq!(undone.restored, vec![file.clone()]);
        assert_eq!(
            std::fs::read(&file).expect("bytes"),
            [0xEF, 0xBB, 0xBF, b'o', b'n', b'e', b'\n']
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rewrite_record_reads_originals_back_from_its_json() {
        let (dir, library, path) = rewrite_one("rewrite-json", "a \"quoted\" line\n", "A\n");
        let record = load(&path, &library).expect("it reads");
        assert_eq!(record.originals.len(), 1);
        assert_eq!(record.originals[0].text, "a \"quoted\" line\n");
        assert_eq!(
            record.originals[0].len as usize,
            "a \"quoted\" line\n".len()
        );
        let json = record.to_json(&library);
        let back = Record::from_json(&json, &library).expect("it reads");
        assert_eq!(back.originals, record.originals);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rewrite_record_is_not_confined_to_the_library_its_files_are_anywhere() {
        // A file outside the library folder is the owner's choice in the task window, and undo
        // follows the record's own path: the confinement of 4.1 is for notes a run makes.
        let dir = scratch("rewrite-outside");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let file = dir.join("outside/notes.md");
        write_file(&file, "one\n");
        let (path, mut record) =
            start_rewrite(&library, "shorten it", None, &file).expect("a record");
        rewrite_into(
            &path,
            &mut record,
            &library,
            &RewriteFile {
                path: file.clone(),
                before: "one\n".to_string(),
                after: "ONE\n".to_string(),
                bom: false,
            },
        )
        .expect("the write");
        record.state = State::Done;
        record.close(&path, &library).expect("closed");
        let record = load(&path, &library).expect("it reads");
        let mut gate = TestGate::new();
        let undone = undo(&path, &record, &library, &mut gate);
        assert_eq!(undone.refusal, None);
        assert_eq!(read(&file), "one\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- audit 4 -------------------------------------------------------------------------

    fn rewrite_file(path: &Path, before: &str, after: &str) -> RewriteFile {
        RewriteFile {
            path: path.to_path_buf(),
            before: before.to_string(),
            after: after.to_string(),
            bom: false,
        }
    }

    /// A F2 (W4): a note the bin refuses at undo keeps the run live ("undo pending") and keeps
    /// the group's file that reaches it, so a second undo takes the note out.
    #[test]
    fn a_note_the_bin_refuses_keeps_the_run_to_be_undone_again() {
        let dir = scratch("a4-bin-refuses");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(&library, &source, &[("One", "# One\n", &[])]);
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 3, "2026-11-21T14:03:11Z");
        let path = report.record_path.clone().expect("a record");
        let record = load(&path, &library).expect("the record");
        let mut bin = TestBin::new(&dir);
        bin.refuse = Some("the file is in use".to_string());
        let done = undo_with(&path, &record, &library, &mut gate, &mut bin);
        assert_eq!(done.bin_failures.len(), 1);
        assert!(library.join("Trip/One.md").is_file(), "the note is kept");
        assert!(
            library.join("Trip.md").is_file(),
            "the group's file stays while a note of the run is on disk"
        );
        assert!(done.group_kept);
        let again = load(&path, &library).expect("the record");
        assert_eq!(again.state, State::UndoPending);
        assert_eq!(prepare(&again, &library).refusal, None);
        // The bin takes it the second time.
        let mut bin = TestBin::new(&dir);
        let done = undo_with(&path, &again, &library, &mut gate, &mut bin);
        assert!(done.bin_failures.is_empty(), "{:?}", done.bin_failures);
        assert_eq!(done.removed_notes, vec![library.join("Trip/One.md")]);
        assert!(!library.join("Trip/One.md").exists());
        assert!(done.group_removed, "{done:?}");
        assert_eq!(
            load(&path, &library).expect("the record").state,
            State::Undone
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A F4 (W6): Finish records the headings it wrote, so undo takes them out with the lines,
    /// and the group's file the run made goes too.
    #[test]
    fn finish_records_its_headings_and_undo_takes_them_and_the_groups_file_out() {
        let dir = scratch("a4-finish-headings");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(&library, &source, &[("One", "# One\n", &["Tokyo"])]);
        let mut refusing = TestGate::new();
        refusing.refusals.push(library.join("Trip.md"));
        let report = write_at(&req, &mut refusing, 3, "2026-11-21T14:03:11Z");
        assert_eq!(report.state(), Some(State::LinksPending));
        let path = report.record_path.clone().expect("a record");
        let record = load(&path, &library).expect("the record");
        assert!(record.sections_added.is_empty());
        let mut gate = TestGate::new();
        let finished = finish(&path, &record, &library, &mut gate);
        assert_eq!(finished.added, 1, "{:?}", finished.failures);
        let record = load(&path, &library).expect("the record");
        assert_eq!(record.sections_added, vec![vec!["Tokyo".to_string()]]);
        assert!(read(&library.join("Trip.md")).contains("## Tokyo"));
        let done = undo(&path, &record, &library, &mut gate);
        assert_eq!(done.removed_notes.len(), 1);
        assert!(done.group_removed, "{done:?}");
        assert!(!library.join("Trip.md").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Audit 3 C4: Finish says what it left out, and links nothing for a note that is gone.
    #[test]
    fn finish_says_which_notes_it_left_out() {
        let dir = scratch("a4-finish-left-out");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(
            &library,
            &source,
            &[("One", "# One\n", &[]), ("Two", "# Two\n", &[])],
        );
        let mut refusing = TestGate::new();
        refusing.refusals.push(library.join("Trip.md"));
        let report = write_at(&req, &mut refusing, 3, "2026-11-21T14:03:11Z");
        let path = report.record_path.clone().expect("a record");
        let record = load(&path, &library).expect("the record");
        std::fs::remove_file(library.join("Trip/Two.md")).expect("gone");
        let mut gate = TestGate::new();
        let finished = finish(&path, &record, &library, &mut gate);
        assert_eq!(finished.added, 1);
        assert_eq!(finished.present, 0);
        assert_eq!(finished.left_out, vec![library.join("Trip/Two.md")]);
        // Finished again, the note it linked is counted as there already.
        let record = load(&path, &library).expect("the record");
        let again = finish(&path, &record, &library, &mut gate);
        assert_eq!(again.added, 0);
        assert_eq!(again.present, 1);
        assert_eq!(again.left_out, vec![library.join("Trip/Two.md")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A F5 (audit 3, C6): a heading the run wrote stays while the owner wrote anything under it,
    /// and goes when nothing is.
    #[test]
    fn a_heading_the_owner_wrote_under_is_kept() {
        let base = Path::new("C:/lib");
        let tokyo = vec!["Tokyo".to_string()];
        let sub = "# Trip\nhome: Trip\n\n## Tokyo\n\n### My own plans\n\n";
        assert_eq!(
            with_headings_removed(sub, std::slice::from_ref(&tokyo), base),
            None
        );
        let prose = "# Trip\nhome: Trip\n\n## Tokyo\n\nTrains leave at nine.\n\n## Kyoto\n";
        assert_eq!(
            with_headings_removed(prose, std::slice::from_ref(&tokyo), base),
            None
        );
        let empty = "# Trip\nhome: Trip\n\n## Tokyo\n\n## Kyoto\n";
        assert_eq!(
            with_headings_removed(empty, std::slice::from_ref(&tokyo), base).as_deref(),
            Some("# Trip\nhome: Trip\n\n\n## Kyoto\n")
        );
        let last = "# Trip\nhome: Trip\n\n## Tokyo\n\n";
        assert_eq!(
            with_headings_removed(last, std::slice::from_ref(&tokyo), base).as_deref(),
            Some("# Trip\nhome: Trip\n\n\n")
        );
        // A deeper heading of the run's own, emptied too, goes first and its parent after it.
        let nested = "# Trip\n\n## Japan\n\n### Tokyo\n\n";
        let both = [
            vec!["Japan".to_string()],
            vec!["Japan".to_string(), "Tokyo".to_string()],
        ];
        assert_eq!(
            with_headings_removed(nested, &both, base).as_deref(),
            Some("# Trip\n\n\n\n")
        );
    }

    /// C11: undo of a note added to a group that was there leaves its file as it was, byte for
    /// byte, and not with the blank line the first link was written below.
    #[test]
    fn undo_leaves_a_group_file_it_did_not_make_as_it_was() {
        let base = Path::new("C:/lib");
        let before = "# Recipes\nhome: Recipes\n";
        let link = Link {
            path: PathBuf::from("Recipes/pasta.md"),
            title: "pasta".to_string(),
            sections: Vec::new(),
            line: library::link_line("pasta", Path::new("Recipes/pasta.md")),
        };
        let written = with_links_added(before, std::slice::from_ref(&link));
        assert_ne!(written, before);
        let removal = LinkRemoval {
            path: link.path.clone(),
            sections: Vec::new(),
            line: link.line.clone(),
        };
        assert_eq!(
            with_removals(&written, std::slice::from_ref(&removal), &[], base).as_deref(),
            Some(before)
        );
        // A file that ends with blank lines of its own keeps them.
        let owned = format!("{written}\n");
        assert_eq!(
            with_removals(&owned, &[removal], &[], base).as_deref(),
            Some("# Recipes\nhome: Recipes\n\n\n")
        );
    }

    /// B2 and D2: a rewrite's record that would pass the reader's cap is refused before the file
    /// is touched, so every original the record holds can be read back and undone.
    #[test]
    fn a_rewrite_record_never_grows_past_what_can_be_read_back() {
        let dir = scratch("a4-record-full");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        // Short lines escape to two bytes each: twenty files of 200 KB pass 4 MiB.
        let text: String = "a\r\n".repeat(66_000);
        let mut files = Vec::new();
        for i in 0..20 {
            let file = dir.join(format!("f{i}.txt"));
            write_file(&file, &text);
            files.push(file);
        }
        let (path, mut record) =
            start_rewrite(&library, "shorten", None, &files[0]).expect("a record");
        let mut refused = None;
        for (i, file) in files.iter().enumerate() {
            let one = rewrite_file(file, &text, "gone\n");
            let fits = rewrite_fits(&record, &library, &one);
            match rewrite_into(&path, &mut record, &library, &one) {
                Ok(()) => assert!(fits),
                Err(why) => {
                    assert!(!fits);
                    assert_eq!(why, RECORD_FULL);
                    assert_eq!(read(file), text, "the file is not touched");
                    refused = Some(i);
                    break;
                }
            }
        }
        let refused = refused.expect("the cap is reached before the twentieth file");
        assert!(refused > 0);
        assert!(std::fs::metadata(&path).expect("the record").len() <= RECORD_MAX);
        let (runs, unread) = list_all(&library);
        assert!(unread.is_empty());
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].record.originals.len(), refused);
        let mut gate = TestGate::new();
        let done = undo(&path, &runs[0].record, &library, &mut gate);
        assert_eq!(done.restored.len(), refused);
        assert_eq!(read(&files[0]), text);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// B6 and N8: a file that cannot be written leaves no original in the record, in memory or
    /// on disk, and no temporary beside it; a `<name>.tmp` of the owner's is never touched (D1).
    #[test]
    fn a_rewrite_that_cannot_write_its_file_leaves_no_trace() {
        let dir = scratch("a4-rewrite-fails");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let file = dir.join("notes.txt");
        write_file(&file, "one\n");
        write_file(&dir.join("notes.txt.tmp"), "the owner's\n");
        let (path, mut record) = start_rewrite(&library, "x", None, &file).expect("a record");
        let mut p = std::fs::metadata(&file).expect("there").permissions();
        p.set_readonly(true);
        std::fs::set_permissions(&file, p).expect("read-only");
        let failed = rewrite_into(
            &path,
            &mut record,
            &library,
            &rewrite_file(&file, "one\n", "ONE\n"),
        );
        assert!(failed.is_err());
        assert!(record.originals.is_empty());
        assert!(
            load(&path, &library)
                .expect("the record")
                .originals
                .is_empty()
        );
        assert_eq!(read(&file), "one\n");
        assert_eq!(read(&dir.join("notes.txt.tmp")), "the owner's\n");
        let mut p = std::fs::metadata(&file).expect("there").permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        p.set_readonly(false);
        std::fs::set_permissions(&file, p).expect("writable");
        rewrite_into(
            &path,
            &mut record,
            &library,
            &rewrite_file(&file, "one\n", "ONE\n"),
        )
        .expect("the write");
        assert_eq!(read(&file), "ONE\n");
        assert_eq!(read(&dir.join("notes.txt.tmp")), "the owner's\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// W1: a file rewritten twice in one record goes back to what it held before the run, and
    /// the run is undone.
    #[test]
    fn a_file_rewritten_twice_in_one_record_is_undone_to_its_first_text() {
        let dir = scratch("a4-twice");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let file = dir.join("notes.txt");
        write_file(&file, "zero\n");
        let (path, mut record) = start_rewrite(&library, "x", None, &file).expect("a record");
        rewrite_into(
            &path,
            &mut record,
            &library,
            &rewrite_file(&file, "zero\n", "one\n"),
        )
        .expect("first");
        rewrite_into(
            &path,
            &mut record,
            &library,
            &rewrite_file(&file, "one\n", "two\n"),
        )
        .expect("second");
        record.state = State::Done;
        record.close(&path, &library).expect("closed");
        let record = load(&path, &library).expect("it reads");
        let plan = prepare(&record, &library);
        assert_eq!(plan.restore, vec![file.clone()]);
        assert!(plan.restore_kept.is_empty());
        let mut gate = TestGate::new();
        let done = undo(&path, &record, &library, &mut gate);
        assert_eq!(done.restored, vec![file.clone()]);
        assert_eq!(read(&file), "zero\n");
        assert_eq!(
            load(&path, &library).expect("the record").state,
            State::Undone
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// W1: a text the owner put in the file between two of the run's writes is as far back as
    /// undo goes.
    #[test]
    fn undo_of_a_file_rewritten_twice_stops_at_an_edit_of_the_owners() {
        let dir = scratch("a4-twice-edited");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let file = dir.join("notes.txt");
        write_file(&file, "zero\n");
        let (path, mut record) = start_rewrite(&library, "x", None, &file).expect("a record");
        rewrite_into(
            &path,
            &mut record,
            &library,
            &rewrite_file(&file, "zero\n", "one\n"),
        )
        .expect("first");
        write_file(&file, "one, and the owner's line\n");
        rewrite_into(
            &path,
            &mut record,
            &library,
            &rewrite_file(&file, "one, and the owner's line\n", "two\n"),
        )
        .expect("second");
        let mut gate = TestGate::new();
        let done = undo(&path, &record, &library, &mut gate);
        assert_eq!(done.restored, vec![file.clone()]);
        assert_eq!(read(&file), "one, and the owner's line\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A F8: the question names each file a rewrite would put back, where it is.
    #[test]
    fn the_rewrite_question_names_its_files() {
        let dir = scratch("a4-named");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let file = dir.join("elsewhere/notes.txt");
        write_file(&file, "one\n");
        let (path, mut record) = start_rewrite(&library, "x", None, &file).expect("a record");
        rewrite_into(
            &path,
            &mut record,
            &library,
            &rewrite_file(&file, "one\n", "ONE\n"),
        )
        .expect("the write");
        let sentence = prepare(&record, &library).sentence("");
        assert!(sentence.contains(&file.display().to_string()), "{sentence}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// T6: the rewrite question's words, whole: what it puts back, the file kept and the file
    /// gone.
    #[test]
    fn the_rewrite_question_says_what_it_puts_back_in_these_words() {
        let dir = scratch("review-t6");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let files: Vec<PathBuf> = ["a.txt", "b.txt", "c.txt", "d.txt"]
            .iter()
            .map(|n| dir.join("elsewhere").join(n))
            .collect();
        for f in &files {
            write_file(f, "one\n");
        }
        let (path, mut record) = start_rewrite(&library, "x", None, &files[0]).expect("a record");
        for f in &files {
            rewrite_into(
                &path,
                &mut record,
                &library,
                &rewrite_file(f, "one\n", "ONE\n"),
            )
            .expect("the write");
        }
        let one = |p: &PathBuf| p.display().to_string();
        assert_eq!(
            prepare(&record, &library).sentence(""),
            format!(
                "Puts back the original of 4 files this run rewrote: {}, {}, {}, {}.",
                one(&files[0]),
                one(&files[1]),
                one(&files[2]),
                one(&files[3])
            )
        );
        write_file(&files[1], "changed\n");
        std::fs::remove_file(&files[2]).expect("gone");
        write_file(&files[3], "one\n");
        assert_eq!(
            prepare(&record, &library).sentence(""),
            format!(
                "Puts back the original of 1 file this run rewrote: {}. 1 file was changed since \
                 and is kept. 2 files are gone already.",
                one(&files[0])
            )
        );
        write_file(&files[0], "changed\n");
        write_file(&files[2], "changed\n");
        assert_eq!(
            prepare(&record, &library).sentence(""),
            "Nothing of this run is left to put back. 3 files were changed since and are kept. \
             1 file is gone already."
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// W2: a run whose group file was never made, its home outside the library, says so, and
    /// neither undo nor finish acts on it.
    #[test]
    fn a_run_whose_group_file_was_never_made_says_so() {
        let dir = scratch("a4-no-group-file");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let mut req = plan(&library, &source, &[("One", "# One\n", &[])]);
        let home = dir.join("Notes/Trip");
        req.group.home = home.clone();
        req.folders = vec![home.clone()];
        req.notes[0].path = home.join("One.md");
        req.notes[0].link_path = home.join("One.md");
        // The library is full, so the group's file is not made.
        for i in 0..library::MAX_FILES {
            write_file(&library.join(format!("g{i:03}.md")), "# G\n");
        }
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 3, "2026-11-21T14:03:11Z");
        assert!(report.group_failure.is_some());
        assert!(home.join("One.md").is_file());
        let path = report.record_path.clone().expect("a record");
        let record = load(&path, &library).expect("the record");
        let why = prepare(&record, &library).refusal.expect("refused");
        assert!(why.contains("is not there"), "{why}");
        let finished = finish(&path, &record, &library, &mut gate);
        assert!(
            finished
                .error
                .as_deref()
                .is_some_and(|e| e.contains("is not there")),
            "{finished:?}"
        );
        assert!(home.join("One.md").is_file(), "nothing is removed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// N1: a group's file that is not directly in the library folder is refused, also when the
    /// home is the library folder itself.
    #[test]
    fn a_group_file_outside_the_library_folder_is_refused_whatever_the_home() {
        let dir = scratch("a4-group-outside");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let mut req = plan(&library, &source, &[("One", "# One\n", &[])]);
        req.group.home = library.clone();
        req.group.file = dir.join("elsewhere/Trip.md");
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 3, "2026-11-21T14:03:11Z");
        assert!(
            report
                .start_error
                .as_deref()
                .is_some_and(|e| e.contains("outside the library folder")),
            "{:?}",
            report.start_error
        );
        assert!(!dir.join("elsewhere/Trip.md").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A F9 and W5: the record made before anything is written owns every note whose name is
    /// free, so a run that dies while it writes leaves a record whose undo can take them out;
    /// a name that is taken is never the run's.
    #[test]
    fn the_first_record_owns_the_notes_whose_names_are_free() {
        let dir = scratch("a4-planned");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(
            &library,
            &source,
            &[("One", "# One\n", &[]), ("Two", "# Two\n", &[])],
        );
        write_file(&library.join("Trip/Two.md"), "# Two\n");
        let src = Source {
            path: source.clone(),
            len: 0,
            sha256: String::new(),
            recycled: false,
        };
        let record = planned(&req, Kind::Split, 3, "2026-11-21T14:03:11Z", src.clone());
        assert!(record.notes[0].created, "a free name is the run's");
        assert!(!record.notes[1].created, "a taken name is never the run's");
        // And a run that finished puts the marks right: Two was there with the same bytes.
        let mut gate = TestGate::new();
        let report = write_at(&req, &mut gate, 3, "2026-11-21T14:03:11Z");
        let record = report.record.expect("the record");
        assert!(record.notes[0].created);
        assert!(!record.notes[1].created);
        let _ = std::fs::remove_dir_all(&dir);

        // A note whose name was free but that could not be written is not the run's either.
        let dir = scratch("a4-planned-failed");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = self::source(&dir);
        let req = plan(
            &library,
            &source,
            &[
                ("One", "# One\n", &[]),
                ("Three", "# Three\n", &["Blocked"]),
            ],
        );
        // A file where the note's folder would go.
        write_file(&library.join("Trip/Blocked"), "a file of the owner's\n");
        let record = planned(&req, Kind::Split, 3, "2026-11-21T14:03:11Z", src);
        assert!(record.notes[1].created, "its name is free");
        let report = write_at(&req, &mut gate, 3, "2026-11-21T14:03:11Z");
        assert_eq!(report.note_failures.len(), 1, "{:?}", report.note_failures);
        let record = report.record.expect("the record");
        assert!(record.notes[0].created);
        assert!(!record.notes[1].created, "it was never written");
        assert_eq!(
            read(&library.join("Trip/Blocked")),
            "a file of the owner's\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// W5: a run that dies after it made its folders, before its notes, leaves a record that
    /// owns those folders, so undo takes them away again.
    #[test]
    fn a_run_that_dies_before_its_notes_owns_the_folders_it_made() {
        let dir = scratch("a4-dies-folders");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(&library, &source, &[("One", "# One\n", &["Tokyo"])]);
        DIES_AT.with(|d| d.set(Some("before the notes")));
        let report = write_at(&req, &mut TestGate::new(), 3, "2026-11-21T14:03:11Z");
        DIES_AT.with(|d| d.set(None));
        assert!(library.join("Trip/Tokyo").is_dir());
        assert!(!library.join("Trip/Tokyo/One.md").exists());
        let path = report.record_path.expect("a record");
        let record = load(&path, &library).expect("the record a crash leaves");
        assert_eq!(record.state, State::Writing);
        assert!(
            record
                .folders
                .iter()
                .any(|f| same_path(f, &library.join("Trip/Tokyo"))),
            "{:?}",
            record.folders
        );
        let done = undo(&path, &record, &library, &mut TestGate::new());
        assert_eq!(done.refusal, None);
        assert!(!library.join("Trip").exists(), "{done:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A F9: a run that dies after it wrote a note, before the record was written again, leaves
    /// a record that owns the note: undo takes it out, and does not look for the note it never
    /// wrote.
    #[test]
    fn a_run_that_dies_while_it_writes_its_notes_can_be_undone() {
        let dir = scratch("a4-dies-notes");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let req = plan(
            &library,
            &source,
            &[("One", "# One\n", &[]), ("Two", "# Two\n", &[])],
        );
        DIES_AT.with(|d| d.set(Some("after a note")));
        let report = write_at(&req, &mut TestGate::new(), 3, "2026-11-21T14:03:11Z");
        DIES_AT.with(|d| d.set(None));
        assert!(library.join("Trip/One.md").is_file());
        assert!(!library.join("Trip/Two.md").exists());
        let path = report.record_path.expect("a record");
        let record = load(&path, &library).expect("the record a crash leaves");
        let done = undo(&path, &record, &library, &mut TestGate::new());
        assert_eq!(done.refusal, None);
        assert_eq!(done.removed_notes, vec![library.join("Trip/One.md")]);
        assert!(!library.join("Trip/One.md").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A F14 on the write's own check: a note's path with a `..` after a folder that is not
    /// there is where the `..` leads, so one that leads out of the library is outside it.
    #[test]
    fn a_dot_dot_after_a_missing_folder_is_where_it_leads() {
        let dir = scratch("a4-dotdot");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let none = Path::new("");
        let escape = library
            .join("Trip")
            .join("..")
            .join("..")
            .join("elsewhere")
            .join("x.md");
        assert!(!inside(&library, none, &escape));
        let back_in = library.join("Trip").join("..").join("Tokyo").join("x.md");
        assert!(inside(&library, none, &back_in));
        // The library folder itself, reached by `..`, is not inside it.
        assert!(!inside(&library, none, &library.join("Trip").join("..")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// D1: undo of a rewrite puts the original back through `files::replace`, so a file of the
    /// owner's called `<name>.tmp` beside it is never touched.
    #[test]
    fn undo_of_a_rewrite_leaves_the_owners_tmp_file_alone() {
        let dir = scratch("a4-undo-tmp");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let file = dir.join("notes.txt");
        write_file(&file, "one\n");
        let (path, mut record) = start_rewrite(&library, "x", None, &file).expect("a record");
        rewrite_into(
            &path,
            &mut record,
            &library,
            &rewrite_file(&file, "one\n", "ONE\n"),
        )
        .expect("the write");
        write_file(&dir.join("notes.txt.tmp"), "the owner's\n");
        let done = undo(&path, &record, &library, &mut TestGate::new());
        assert_eq!(done.restored, vec![file.clone()]);
        assert_eq!(read(&file), "one\n");
        assert_eq!(read(&dir.join("notes.txt.tmp")), "the owner's\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A F2 for a rewrite: a file undo could not write back keeps the run to be undone again
    /// ("undo pending"), and the next undo, once it can, finishes it.
    #[test]
    fn a_rewrite_undo_that_cannot_write_its_file_is_left_to_be_undone_again() {
        let dir = scratch("a4-undo-ro");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let file = dir.join("notes.txt");
        write_file(&file, "one\n");
        let (path, mut record) = start_rewrite(&library, "x", None, &file).expect("a record");
        rewrite_into(
            &path,
            &mut record,
            &library,
            &rewrite_file(&file, "one\n", "ONE\n"),
        )
        .expect("the write");
        let set_readonly = |on: bool| {
            let mut p = std::fs::metadata(&file).expect("there").permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            p.set_readonly(on);
            std::fs::set_permissions(&file, p).expect("its permissions");
        };
        set_readonly(true);
        let done = undo(&path, &record, &library, &mut TestGate::new());
        assert_eq!(done.failures.len(), 1, "{done:?}");
        assert_eq!(read(&file), "ONE\n");
        let again = load(&path, &library).expect("the record");
        assert_eq!(again.state, State::UndoPending);
        assert!(again.state.live() && !again.state.finishable());
        set_readonly(false);
        let done = undo(&path, &again, &library, &mut TestGate::new());
        assert_eq!(done.restored, vec![file.clone()]);
        assert_eq!(read(&file), "one\n");
        assert_eq!(
            load(&path, &library).expect("the record").state,
            State::Undone
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// N8: a rewrite whose record cannot be written writes nothing, and leaves no original in
    /// the record it holds.
    #[test]
    fn a_rewrite_whose_record_cannot_be_written_writes_nothing() {
        let dir = scratch("a4-record-fails");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let file = dir.join("notes.txt");
        write_file(&file, "one\n");
        let (path, mut record) = start_rewrite(&library, "x", None, &file).expect("a record");
        // A folder where the record is: it cannot be written over.
        std::fs::remove_file(&path).expect("the record goes");
        std::fs::create_dir_all(path.join("in the way")).expect("a folder");
        let failed = rewrite_into(
            &path,
            &mut record,
            &library,
            &rewrite_file(&file, "one\n", "ONE\n"),
        );
        assert!(
            failed
                .as_ref()
                .is_err_and(|e| e.contains("The record could not be written")),
            "{failed:?}"
        );
        assert!(record.originals.is_empty());
        assert_eq!(read(&file), "one\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A F11a: the question counts a run's lines in words, beside its notes: "with 1 line", not
    /// "their 1 lines".
    #[test]
    fn the_undo_question_counts_the_lines_in_words() {
        let none: &[&str] = &[];
        for (notes, words) in [
            (
                vec![("One", "# One\n", none)],
                "Removes 1 note this run wrote, with 1 line in Trip",
            ),
            (
                vec![("One", "# One\n", none), ("Two", "# Two\n", none)],
                "Removes 2 notes this run wrote, with 2 lines in Trip",
            ),
        ] {
            let dir = scratch("a4-sentence");
            let library = dir.join("library");
            std::fs::create_dir_all(&library).expect("the library");
            let source = source(&dir);
            let req = plan(&library, &source, &notes);
            let report = write_at(&req, &mut TestGate::new(), 3, "2026-11-21T14:03:11Z");
            let path = report.record_path.expect("a record");
            let record = load(&path, &library).expect("the record");
            let sentence = prepare(&record, &library).sentence("Trip");
            assert!(sentence.starts_with(words), "{sentence}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    // ---- the review of the audit 4 fixes -----------------------------------------------------

    /// S1: a rewrite's record that just fits when its file is written still reads back in every
    /// state it can be closed with, "undo pending" among them, so a refused undo can be tried
    /// again.
    #[test]
    fn a_rewrite_record_that_just_fits_still_reads_back_after_its_undo_is_refused() {
        let dir = scratch("review-s1");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let file = dir.join("notes.txt");
        write_file(&file, "one\n");
        let (path, mut record) = start_rewrite(&library, "x", None, &file).expect("a record");
        // The longest original that still fits: one more byte would not.
        let fits = |n: usize| {
            rewrite_fits(
                &record,
                &library,
                &rewrite_file(&file, &"a".repeat(n), "b\n"),
            )
        };
        let (mut low, mut high) = (0usize, RECORD_MAX as usize);
        while low + 1 < high {
            let mid = (low + high) / 2;
            if fits(mid) { low = mid } else { high = mid }
        }
        assert!(fits(low) && !fits(low + 1), "{low}");
        let before = "a".repeat(low);
        write_file(&file, &before);
        rewrite_into(
            &path,
            &mut record,
            &library,
            &rewrite_file(&file, &before, "b\n"),
        )
        .expect("the write");
        assert_eq!(load(&path, &library).map(|r| r.state), Some(State::Writing));
        // Every state the record can be closed with reads back.
        for state in [
            State::Writing,
            State::LinksPending,
            State::Done,
            State::Stopped,
            State::Undone,
            State::UndoPending,
        ] {
            let mut closed = record.clone();
            closed.state = state;
            closed.close(&path, &library).expect("written");
            assert_eq!(
                load(&path, &library).map(|r| r.state),
                Some(state),
                "{} bytes",
                std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
            );
        }
        record.close(&path, &library).expect("written");
        // And the probe's way: an undo that cannot write the file leaves "undo pending", which
        // is read back and undone again once it can.
        let set_readonly = |on: bool| {
            let mut p = std::fs::metadata(&file).expect("there").permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            p.set_readonly(on);
            std::fs::set_permissions(&file, p).expect("its permissions");
        };
        set_readonly(true);
        let done = undo(&path, &record, &library, &mut TestGate::new());
        set_readonly(false);
        assert_eq!(done.failures.len(), 1, "{done:?}");
        let again = load(&path, &library).expect("the record reads back");
        assert_eq!(again.state, State::UndoPending);
        let done = undo(&path, &again, &library, &mut TestGate::new());
        assert_eq!(done.restored, vec![file.clone()]);
        assert_eq!(read(&file), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// S2: a run that died before its notes left a record that marks them as its own. A later
    /// run of the same plan wrote them; undo of the dead run leaves that run's notes and lines,
    /// and the later run's undo takes them out.
    #[test]
    fn undo_of_a_dead_run_leaves_the_notes_a_later_run_wrote() {
        let dir = scratch("review-s2");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let none: &[&str] = &[];
        let req = plan(
            &library,
            &source,
            &[("One", "# One\n", none), ("Two", "# Two\n", none)],
        );
        // The clock as a run reads it: the record's name and its `started` say the same time.
        let at = |secs: u64| (secs, utc(secs));
        let (secs, started) = at(1_795_000_000);
        DIES_AT.with(|d| d.set(Some("before the notes")));
        let dead = write_at(&req, &mut TestGate::new(), secs, &started);
        DIES_AT.with(|d| d.set(None));
        let dead_path = dead.record_path.expect("a record");
        let dead_record = load(&dead_path, &library).expect("the record a crash leaves");
        assert!(dead_record.notes.iter().all(|n| n.created));
        let (secs, started) = at(1_795_000_060);
        let later = write_at(&req, &mut TestGate::new(), secs, &started);
        assert_eq!(later.state(), Some(State::Done));
        let later_path = later.record_path.expect("a record");
        let one = library.join("Trip/One.md");
        let two = library.join("Trip/Two.md");
        let group = read(&library.join("Trip.md"));
        assert!(one.is_file() && two.is_file());
        let plan_of_dead = prepare(&dead_record, &library);
        assert!(plan_of_dead.notes.is_empty(), "{plan_of_dead:?}");
        assert!(plan_of_dead.links.is_empty(), "{plan_of_dead:?}");
        assert_eq!(plan_of_dead.later.len(), 2, "{plan_of_dead:?}");
        let sentence = plan_of_dead.sentence("Trip");
        assert!(
            sentence.contains("2 files are a later run's and stay."),
            "{sentence}"
        );
        let done = undo(&dead_path, &dead_record, &library, &mut TestGate::new());
        assert!(done.removed_notes.is_empty(), "{done:?}");
        assert_eq!(done.removed_links, 0, "{done:?}");
        assert!(one.is_file() && two.is_file());
        assert_eq!(read(&library.join("Trip.md")), group);
        // The later run is still its own, and its undo takes its notes and lines.
        let later_record = load(&later_path, &library).expect("the later record");
        assert_eq!(later_record.state, State::Done);
        let done = undo(&later_path, &later_record, &library, &mut TestGate::new());
        assert_eq!(done.removed_notes.len(), 2, "{done:?}");
        assert!(!one.exists() && !two.exists());
        // An undone run names nothing any more.
        assert!(prepare(&dead_record, &library).later.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// S3: undo gives a library file back byte for byte when it still holds what the run's add
    /// wrote, whatever its shape: the owner's blank lines at its end, a link written above a
    /// heading, no newline at its end, CRLF or mixed line ends, a heading the run made.
    #[test]
    fn undo_gives_the_library_file_back_byte_for_byte_whatever_its_shape() {
        let none: &[&str] = &[];
        let shapes = [
            "# Trip\n",
            "# Trip\n\n- [a](a.md)\n\n",
            "# Trip\n\n## S\n\n- [b](b.md)\n\n",
            "# Trip\n\n## S\n\n- [b](b.md)\n",
            "# Trip",
            "# Trip\n\n- [a](a.md)",
            "# Trip\r\n\r\n- [a](a.md)\r\n",
            "# Trip\r\n\r\n- [a](a.md)\r\n\r\n",
            "# Trip\n\n- [a](a.md)\r\n## S\r\n",
            "# Trip\n\n\n\n",
            "# Trip\nhome: Trip\n\nA line of the owner's.\n\n## S\n\n### Deeper\n\n- [b](b.md)\n\n\n",
        ];
        let notes: [&[(&str, &str, &[&str])]; 3] = [
            &[("One", "# One\n", none)],
            &[("One", "# One\n", none), ("Two", "# Two\n", &["S"])],
            &[("Two", "# Two\n", &["New", "Deeper"])],
        ];
        let mut wrong = Vec::new();
        for (i, shape) in shapes.iter().enumerate() {
            for (j, notes) in notes.iter().enumerate() {
                let dir = scratch("review-s3");
                let library = dir.join("library");
                std::fs::create_dir_all(&library).expect("the library");
                let source = source(&dir);
                let group = library.join("Trip.md");
                write_file(&group, shape);
                let mut req = plan(&library, &source, notes);
                req.group.created = false;
                let report = write_at(&req, &mut TestGate::new(), 3, "2026-11-21T14:03:11Z");
                assert_eq!(report.state(), Some(State::Done), "{i} {j} {report:?}");
                assert_ne!(read(&group), *shape, "{i} {j}: the links are in");
                let path = report.record_path.expect("a record");
                let record = load(&path, &library).expect("the record");
                let done = undo(&path, &record, &library, &mut TestGate::new());
                assert!(done.failures.is_empty(), "{i} {j} {done:?}");
                assert_eq!(done.removed_notes.len(), notes.len(), "{i} {j} {done:?}");
                let back = std::fs::read(&group).expect("the file");
                if back != shape.as_bytes() {
                    wrong.push(format!(
                        "{i} {j}: {shape:?} came back as {:?}",
                        String::from_utf8_lossy(&back)
                    ));
                }
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// S3, the rest: what the add did is in the record and read back; a file changed since, a
    /// line that stays, a gate that has not opted in and a record without it all take the lines
    /// out as before, and the owner's change is kept.
    #[test]
    fn undo_takes_the_lines_out_as_before_when_the_file_changed_since_the_add() {
        let none: &[&str] = &[];
        let shape = "# Trip\n\nA line of the owner's.\n\n";
        let setup = |tag: &str| {
            let dir = scratch(tag);
            let library = dir.join("library");
            std::fs::create_dir_all(&library).expect("the library");
            let source = source(&dir);
            let group = library.join("Trip.md");
            write_file(&group, shape);
            let mut req = plan(
                &library,
                &source,
                &[("One", "# One\n", none), ("Two", "# Two\n", none)],
            );
            req.group.created = false;
            let report = write_at(&req, &mut TestGate::new(), 3, "2026-11-21T14:03:11Z");
            assert_eq!(report.state(), Some(State::Done), "{report:?}");
            let path = report.record_path.expect("a record");
            let record = load(&path, &library).expect("the record");
            (dir, library, group, path, record)
        };
        // The record holds the text before the add and the digest of the one after it.
        let (dir, library, group, path, record) = setup("review-s3-record");
        assert_eq!(record.added.len(), 1, "{record:?}");
        assert!(same_path(&record.added[0].file, &group));
        assert_eq!(record.added[0].before, shape);
        assert_eq!(
            record.added[0].written,
            crate::update::sha256_hex(read(&group).as_bytes())
        );
        assert_eq!(
            Record::from_json(&record.to_json(&library), &library),
            Some(record.clone())
        );
        let done = undo(&path, &record, &library, &mut TestGate::new());
        assert_eq!(done.removed_links, 2);
        assert_eq!(read(&group), shape);
        let _ = std::fs::remove_dir_all(&dir);
        // A line of the owner's added since: the run's lines go, the owner's line stays, and the
        // text before the run is not put back over it.
        let (dir, library, group, path, record) = setup("review-s3-changed");
        let changed = read(&group).replace(
            "A line of the owner's.",
            "A line of the owner's.\nAnd one more.",
        );
        std::fs::write(&group, &changed).expect("the owner's edit");
        let done = undo(&path, &record, &library, &mut TestGate::new());
        assert_eq!(done.removed_links, 2, "{done:?}");
        let now = read(&group);
        assert!(now.contains("And one more."), "{now:?}");
        assert!(!now.contains("[One]") && !now.contains("[Two]"), "{now:?}");
        let _ = std::fs::remove_dir_all(&dir);
        // A note changed since keeps its line: the file is not the one before the run.
        let (dir, library, group, path, record) = setup("review-s3-kept");
        std::fs::write(library.join("Trip/Two.md"), "# Two, edited\n").expect("the edit");
        let done = undo(&path, &record, &library, &mut TestGate::new());
        assert_eq!(done.removed_links, 1, "{done:?}");
        let now = read(&group);
        assert!(!now.contains("[One]") && now.contains("[Two]"), "{now:?}");
        let _ = std::fs::remove_dir_all(&dir);
        // A gate that has not opted in, and a record from before the field, take the lines out
        // the way they did, through `remove_links`.
        for old in [false, true] {
            let (dir, library, group, path, mut record) = setup("review-s3-plain");
            let mut gate = TestGate::new();
            if old {
                record.added.clear();
            } else {
                gate.plain = true;
            }
            let heuristic = with_removals(
                &read(&group),
                &record
                    .notes
                    .iter()
                    .map(|n| LinkRemoval {
                        path: n.path.clone(),
                        sections: n.sections.clone(),
                        line: n.line.clone(),
                    })
                    .collect::<Vec<_>>(),
                &record.sections_added,
                &library,
            )
            .expect("the lines are there");
            let done = undo(&path, &record, &library, &mut gate);
            assert_eq!(done.removed_links, 2, "{done:?}");
            assert_eq!(read(&group), heuristic);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// S3 for Finish: the links Finish writes are recorded the same way, and undo gives the file
    /// back byte for byte.
    #[test]
    fn undo_after_finish_gives_the_library_file_back_byte_for_byte() {
        let none: &[&str] = &[];
        let dir = scratch("review-s3-finish");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let group = library.join("Trip.md");
        let shape = "# Trip\r\n\r\n## S\r\n\r\n- [b](b.md)\r\n\r\n";
        write_file(&group, shape);
        let mut req = plan(
            &library,
            &source,
            &[("One", "# One\n", none), ("Two", "# Two\n", &["S"])],
        );
        req.group.created = false;
        let mut gate = TestGate::new();
        gate.refusals.push(group.clone());
        let report = write_at(&req, &mut gate, 3, "2026-11-21T14:03:11Z");
        assert_eq!(report.state(), Some(State::LinksPending));
        let path = report.record_path.expect("a record");
        let record = load(&path, &library).expect("the record");
        assert!(record.added.is_empty());
        let done = finish(&path, &record, &library, &mut TestGate::new());
        assert_eq!(done.added, 2, "{done:?}");
        let record = load(&path, &library).expect("the record");
        assert_eq!(record.added.len(), 1, "{record:?}");
        let done = undo(&path, &record, &library, &mut TestGate::new());
        assert_eq!(done.removed_links, 2, "{done:?}");
        assert_eq!(std::fs::read(&group).expect("the file"), shape.as_bytes());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// S3: a library file whose text would take the record past what is read back is not kept
    /// in it; the record still reads back, and undo takes the lines out as before.
    #[test]
    fn a_library_file_too_big_for_the_record_is_not_kept_in_it() {
        let none: &[&str] = &[];
        let dir = scratch("review-s3-big");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let group = library.join("Trip.md");
        // Each control character is six bytes in the record: 700,000 of them pass 4 MiB.
        let shape = format!("# Trip\n\n{}\n", "\u{1}".repeat(700_000));
        write_file(&group, &shape);
        let mut req = plan(&library, &source, &[("One", "# One\n", none)]);
        req.group.created = false;
        let report = write_at(&req, &mut TestGate::new(), 3, "2026-11-21T14:03:11Z");
        assert_eq!(
            report.state(),
            Some(State::Done),
            "{:?}",
            report.link_failures
        );
        let path = report.record_path.expect("a record");
        assert!(std::fs::metadata(&path).expect("the record").len() <= RECORD_MAX);
        let record = load(&path, &library).expect("the record reads back");
        assert!(record.added.is_empty());
        let done = undo(&path, &record, &library, &mut TestGate::new());
        assert_eq!(done.removed_links, 1, "{done:?}");
        assert!(!read(&group).contains("[One]"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// S3, the keeping itself: a second add to a file that still holds what the first left
    /// keeps the text before the first, a file changed between them starts again from the
    /// second, and a file that does not hold exactly the links on the text read before keeps
    /// nothing.
    #[test]
    fn what_an_add_did_is_kept_only_when_the_file_holds_exactly_it() {
        let dir = scratch("review-s3-keep");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let file = library.join("Trip.md");
        let link = |name: &str| Link {
            path: PathBuf::from(format!("Trip/{name}.md")),
            title: name.to_string(),
            sections: Vec::new(),
            line: format!("- [{name}](Trip/{name}.md)"),
        };
        let mut record = Record {
            version: 1,
            kind: Kind::Split,
            started: "2026-11-21T14:03:11Z".to_string(),
            started_secs: 3,
            state: State::Writing,
            instruction: String::new(),
            sent: None,
            source: Source {
                path: dir.join("Trip.md"),
                len: 0,
                sha256: String::new(),
                recycled: false,
            },
            group: Group {
                name: "Trip".to_string(),
                file: file.clone(),
                created: false,
                home: library.join("Trip"),
            },
            folders: Vec::new(),
            notes: Vec::new(),
            pictures: Vec::new(),
            sections_added: Vec::new(),
            moved: None,
            originals: Vec::new(),
            added: Vec::new(),
        };
        let t0 = "# Trip\n\nThe owner's line.\n\n";
        let t1 = with_links_added(t0, &[link("One")]);
        write_file(&file, &t1);
        record.keep_added(&file, Some(t0.to_string()), &[link("One")], &library);
        assert_eq!(record.added.len(), 1);
        assert_eq!(record.added[0].before, t0);
        let t2 = with_links_added(&t1, &[link("Two")]);
        write_file(&file, &t2);
        record.keep_added(&file, Some(t1.clone()), &[link("Two")], &library);
        assert_eq!(record.added.len(), 1, "{:?}", record.added);
        assert_eq!(record.added[0].before, t0, "the text before the first add");
        assert_eq!(
            record.added[0].written,
            crate::update::sha256_hex(t2.as_bytes())
        );
        // Changed between the adds: the first text is not what this one started from.
        let t2b = format!("{t2}The owner again.\n");
        let t3 = with_links_added(&t2b, &[link("Three")]);
        write_file(&file, &t3);
        record.keep_added(&file, Some(t2b.clone()), &[link("Three")], &library);
        assert_eq!(record.added.len(), 1);
        assert_eq!(record.added[0].before, t2b);
        // The file holds something else than the links on the text read before: nothing kept.
        let mut other = record.clone();
        other.added.clear();
        write_file(&file, &format!("{t3}And a line written meanwhile.\n"));
        other.keep_added(&file, Some(t2b), &[link("Three")], &library);
        assert!(other.added.is_empty(), "{:?}", other.added);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// S2, a run that died after a note: the later run found that note there with its bytes
    /// and listed it, and the picture too; it counts on them, so undo of the dead run leaves
    /// them as well as the note the later run wrote.
    #[test]
    fn undo_of_a_dead_run_leaves_what_a_later_run_found_there_and_listed() {
        let dir = scratch("review-s2-found");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).expect("the library");
        let source = source(&dir);
        let none: &[&str] = &[];
        let mut req = plan(
            &library,
            &source,
            &[("One", "# One\n", none), ("Two", "# Two\n", none)],
        );
        let picture = dir.join("map.png");
        std::fs::write(&picture, b"a picture").expect("the picture");
        req.pictures.push(NewPicture {
            source: picture,
            folder: library.join("Trip"),
        });
        let at = |secs: u64| (secs, utc(secs));
        let (secs, started) = at(1_795_000_000);
        DIES_AT.with(|d| d.set(Some("after a note")));
        let dead = write_at(&req, &mut TestGate::new(), secs, &started);
        DIES_AT.with(|d| d.set(None));
        let dead_path = dead.record_path.expect("a record");
        let dead_record = load(&dead_path, &library).expect("the record a crash leaves");
        let one = library.join("Trip/One.md");
        let two = library.join("Trip/Two.md");
        let map = library.join("Trip/map.png");
        assert!(one.is_file() && !two.exists() && map.is_file());
        assert!(dead_record.pictures.iter().all(|p| p.created));
        let (secs, started) = at(1_795_000_060);
        let later = write_at(&req, &mut TestGate::new(), secs, &started);
        assert_eq!(later.state(), Some(State::Done), "{later:?}");
        let later_record = load(&later.record_path.expect("a record"), &library).expect("it");
        assert!(!later_record.notes[0].created && later_record.notes[1].created);
        assert!(later_record.pictures.iter().all(|p| !p.created));
        let plan_of_dead = prepare(&dead_record, &library);
        assert!(plan_of_dead.notes.is_empty(), "{plan_of_dead:?}");
        assert!(plan_of_dead.pictures.is_empty(), "{plan_of_dead:?}");
        assert_eq!(plan_of_dead.later.len(), 3, "{plan_of_dead:?}");
        let group = read(&library.join("Trip.md"));
        let done = undo(&dead_path, &dead_record, &library, &mut TestGate::new());
        assert!(done.removed_notes.is_empty(), "{done:?}");
        assert!(done.removed_pictures.is_empty(), "{done:?}");
        assert_eq!(done.later.len(), 3, "{done:?}");
        assert!(one.is_file() && two.is_file() && map.is_file());
        assert_eq!(read(&library.join("Trip.md")), group);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
