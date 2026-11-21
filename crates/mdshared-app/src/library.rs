//! The notes library: a `library` folder beside `settings.ini` whose `.md` files each hold one
//! group of notes. The notes themselves stay where they are and nothing is written to them; a
//! library file is a list of links, read again whenever it is saved, as the editor's saved
//! prompts file is.
//!
//! One file, one group. Its first level-1 heading names the group (its file name otherwise), a
//! level-2 heading or deeper starts a section, and every list item under them is a note:
//!
//! ```text
//! # Personal
//!
//! ## Money
//!
//! - [Taxes 2026](D:/misc/taxes.md)
//! - D:/misc/receipts.md
//! ```
//!
//! A line with no link and no path is left out, as is anything inside a code fence, so prose
//! around the lists is free. A link's text is the title; a line that is only a path takes its
//! title from the note's first heading, and its file name where there is none (such a line
//! needs `<` and `>` around a path with spaces in it, or it reads as prose). A relative path is
//! taken from the library folder. A note that is not on disk is listed as missing rather than
//! dropped, so a moved file is visible instead of silently gone.
//!
//! A group may name the folder its notes are written in with one `home:` line, `home: Personal`
//! or `home: D:/notes/work`: a path with no root is taken from the library folder as a note's
//! is. It is neither a heading nor a note, so a reader that does not know the line lists the
//! same notes as one that does; the first such line counts and a second is prose.
//!
//! [`read`] does the whole of it: the folder, each file, the titles and what is missing. It
//! touches the notes only for the titles it has to find, and never more than [`TITLE_BYTES`] of
//! one, so it stays off the launch path by being called when the library is first shown.

use crate::files::{self, FileStamp};
use std::path::{Path, PathBuf};

/// The folder of library files, beside `settings.ini`.
pub const DIR: &str = "library";

/// The file "add this note to the library" writes to when the folder has none.
pub const DEFAULT_FILE: &str = "notes.md";

/// A library file is read at most this far; a larger one is listed empty.
pub const MAX_FILE_BYTES: usize = 1024 * 1024;

/// Groups read at most, in name order: the count is of groups kept, not of files looked at. Only
/// a `.md` file in the folder is a library file, so anything else there is never a group and never
/// uses one of these up.
pub const MAX_FILES: usize = 512;

/// Notes kept per group.
pub const MAX_ENTRIES: usize = 2_000;

/// Bytes of a note read to find the heading its title comes from.
pub const TITLE_BYTES: usize = 8 * 1024;

/// What the first library file is written with.
pub const TEMPLATE: &str = "# Notes\n\
\n\
Each list item below is a note: a link, or a path on its own. A heading of level 2 is a\n\
section. Save this file and the library is read again. Lines that are not notes, like these,\n\
are left out.\n";

/// One note in the library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// What the list shows: the link's text, the note's first heading, or its file name.
    pub title: String,
    /// The note. Absolute, unless the library file gave a path with no root and the library
    /// folder is not known.
    pub path: PathBuf,
    /// The headings it is under, outermost first: `["Money", "2026"]` for a note under
    /// `### 2026` inside `## Money`. Empty for the notes written before any heading. Headings
    /// nest by their level, so a library file is a tree as deep as it is written.
    pub sections: Vec<String>,
    /// Its line in the library file, counted from 1, so a command can lead to it.
    pub line: usize,
    /// There is no file at `path`. Set by [`read`]; [`parse`] leaves it false.
    pub missing: bool,
}

/// One library file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// The file's first level-1 heading, or its file name without the extension.
    pub name: String,
    /// The folder its `home:` line names, resolved as a note's path is, or `None` when the file
    /// has no such line. Where new notes of the group are written is the app's business.
    pub home: Option<PathBuf>,
    pub file: PathBuf,
    /// What the file looked like when it was read, for "has it been saved since".
    pub stamp: FileStamp,
    /// Every heading path below the first level, in the order they are written, including the
    /// headings with no note under them yet: where a note can be put, and the tree a pane draws.
    pub sections: Vec<Vec<String>>,
    pub entries: Vec<Entry>,
}

/// Every group, in the order the folder gives (by file name).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Library {
    pub dir: PathBuf,
    pub groups: Vec<Group>,
    /// How many groups [`read`] left out because the folder holds more than [`MAX_FILES`] of
    /// them: 0 when it read every library file, so a pane can say "70 of 512 groups" or draw a
    /// warning without listing the folder again. It is a count, not the names: what was left out
    /// is every library file after the ones in `groups`, in name order.
    pub omitted: usize,
}

impl Library {
    /// The notes of every group, each with the group it is in.
    pub fn entries(&self) -> impl Iterator<Item = (&Group, &Entry)> {
        self.groups
            .iter()
            .flat_map(|g| g.entries.iter().map(move |e| (g, e)))
    }

    /// How many notes it holds.
    pub fn len(&self) -> usize {
        self.groups.iter().map(|g| g.entries.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether any library file has been saved, added or removed since it was read. A folder
    /// with more groups than [`MAX_FILES`] is not "changed" for merely still having them: the
    /// [`Library::omitted`] count is part of what was read.
    pub fn changed(&self) -> bool {
        let files = library_files(&self.dir);
        files.len() != self.groups.len() + self.omitted
            || files
                .iter()
                .zip(&self.groups)
                .any(|(f, g)| *f != g.file || files::stamp(f) != g.stamp)
    }
}

/// The `.md` files of a library folder, in name order: every candidate, before [`read`] caps the
/// groups it keeps. Only a `.md` file is a library file, so anything else in the folder is not
/// listed and costs no budget. A folder that is not there is empty, not an error.
fn library_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
                && p.is_file()
        })
        .collect();
    files.sort();
    files
}

/// Reads the whole library under `dir` (the `library` folder itself, not the settings folder):
/// every file, the title of every note that has none written for it, and which notes are not on
/// disk. A folder that is not there is an empty library, not an error. More than [`MAX_FILES`]
/// groups is not an unbounded read: the first ones in name order are kept and the rest are
/// counted in [`Library::omitted`], so a folder someone pointed at cannot cost the app more than
/// that many files and the next read picks the same ones.
pub fn read(dir: &Path) -> Library {
    let mut groups = Vec::new();
    let mut omitted = 0;
    for file in library_files(dir) {
        // The cap is on groups kept, not on the files looked at. A file beyond it is not read
        // and is only counted: it would be one group more.
        if groups.len() >= MAX_FILES {
            omitted += 1;
            continue;
        }
        let stamp = files::stamp(&file);
        let text = files::read_stable(&file, MAX_FILE_BYTES)
            .map(|l| files::decode(&l.bytes).0.into_owned())
            .unwrap_or_default();
        let Parsed {
            name,
            home,
            sections,
            mut entries,
        } = parse(&text, dir);
        let name = name.unwrap_or_else(|| {
            file.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string()
        });
        for entry in &mut entries {
            entry.missing = !entry.path.exists();
            if entry.title.is_empty() {
                entry.title = title_of(&entry.path);
            }
        }
        groups.push(Group {
            name,
            home,
            file,
            stamp,
            sections,
            entries,
        });
    }
    Library {
        dir: dir.to_path_buf(),
        groups,
        omitted,
    }
}

/// The title a note with none written for it goes by: its first heading, else its file name.
pub fn title_of(path: &Path) -> String {
    first_heading(path).unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string()
    })
}

/// The first `#` heading of a note, reading no more than [`TITLE_BYTES`] of it.
fn first_heading(path: &Path) -> Option<String> {
    let loaded = files::read_stable(path, TITLE_BYTES).ok().or_else(|| {
        // Too large to read whole: its start is enough for a heading.
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .ok()?
            .take(TITLE_BYTES as u64)
            .read_to_end(&mut bytes)
            .ok()?;
        Some(files::Loaded {
            bytes,
            stamp: files::stamp(path),
        })
    })?;
    let text = files::decode(&loaded.bytes).0.into_owned();
    let mut fence: Option<String> = None;
    for line in text.lines() {
        if let Some(marker) = fence_marker(line) {
            fence = match &fence {
                Some(open) if closes(&marker, open) => None,
                None => Some(marker),
                _ => fence,
            };
            continue;
        }
        if fence.is_none()
            && let Some(rest) = line.trim_start().strip_prefix('#')
        {
            let title = rest
                .trim_start_matches('#')
                .trim()
                .trim_end_matches('#')
                .trim();
            if !title.is_empty() {
                return Some(title.to_string());
            }
        }
    }
    None
}

/// The fence a line opens or closes, if it is one.
fn fence_marker(line: &str) -> Option<String> {
    let t = line.trim_start();
    let first = t.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let marker: String = t.chars().take_while(|&c| c == first).collect();
    (marker.len() >= 3).then_some(marker)
}

/// Whether `marker` closes the fence `open` opened: the same character, and no shorter.
fn closes(marker: &str, open: &str) -> bool {
    marker.starts_with(open) && marker.len() >= open.len()
}

/// What one library file holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    /// The file's first level-1 heading, if it has one.
    pub name: Option<String>,
    /// Its `home:` folder, resolved against the library folder, if it has one.
    pub home: Option<PathBuf>,
    /// Its other headings as paths, in the order they are written: `["Money"]`,
    /// `["Money", "2026"]`, `["Trips"]`.
    pub sections: Vec<Vec<String>>,
    pub entries: Vec<Entry>,
}

/// Reads one library file: its group name (its first level-1 heading), the folder its first
/// `home:` line names, its headings and its notes, in the order they are written. `base` is the
/// folder a path with no root is taken from. Nothing is read from disk here, so [`Entry::missing`]
/// is false and a note with no title written keeps an empty one; [`read`] fills both in.
pub fn parse(text: &str, base: &Path) -> Parsed {
    let mut name: Option<String> = None;
    let mut home: Option<PathBuf> = None;
    let mut sections: Vec<Vec<String>> = Vec::new();
    // The headings open at this line, with the level each was written at.
    let mut open: Vec<(usize, String)> = Vec::new();
    let mut entries: Vec<Entry> = Vec::new();
    let mut fence: Option<String> = None;
    for (n, line) in text.lines().enumerate() {
        if let Some(marker) = fence_marker(line) {
            fence = match &fence {
                Some(open) if closes(&marker, open) => None,
                None => Some(marker),
                _ => fence,
            };
            continue;
        }
        if fence.is_some() {
            continue;
        }
        if let Some(heading) = heading(line) {
            match heading {
                (1, text) if name.is_none() && !text.is_empty() => name = Some(text.to_string()),
                // A second level-1 heading closes the sections of the first.
                (1, _) => open.clear(),
                (_, "") => {}
                (level, text) => {
                    while open.last().is_some_and(|(l, _)| *l >= level) {
                        open.pop();
                    }
                    open.push((level, text.to_string()));
                    let path: Vec<String> = open.iter().map(|(_, t)| t.clone()).collect();
                    if !sections.contains(&path) {
                        sections.push(path);
                    }
                }
            }
            continue;
        }
        // The group's own folder, and only the first line that names one.
        if home.is_none()
            && let Some(folder) = home_line(line)
        {
            home = Some(resolve(&folder, base));
            continue;
        }
        if entries.len() >= MAX_ENTRIES {
            continue;
        }
        if let Some((title, path)) = note_line(line) {
            let path = resolve(&path, base);
            entries.push(Entry {
                title,
                path,
                sections: open.iter().map(|(_, t)| t.clone()).collect(),
                line: n + 1,
                missing: false,
            });
        }
    }
    Parsed {
        name,
        home,
        sections,
        entries,
    }
}

/// A heading line as its level and its text.
fn heading(line: &str) -> Option<(usize, &str)> {
    let rest = line.strip_prefix('#')?;
    let more = rest.len() - rest.trim_start_matches('#').len();
    let level = 1 + more;
    let text = rest[more..].trim().trim_end_matches('#').trim();
    (level == 1 || rest[more..].starts_with(' ') || text.is_empty()).then_some((level, text))
}

/// The folder a `home:` line names, as the line gives it. The key reads in any case, and the
/// value may be written in `<>` with `%20` in it, as a note's path may.
fn home_line(line: &str) -> Option<String> {
    let (key, value) = line.trim_start().split_once(':')?;
    if !key.trim().eq_ignore_ascii_case("home") {
        return None;
    }
    let (value, _) = unwrap_path(value);
    (!value.is_empty()).then_some(value)
}

/// A note line as its title (empty when it is only a path) and the path written on it: a list
/// item or a line of its own, holding `[Title](path)` or a path. Anything else is not a note.
fn note_line(line: &str) -> Option<(String, String)> {
    let t = line.trim();
    let t = t
        .strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))
        .or_else(|| t.strip_prefix("+ "))
        .unwrap_or(t)
        .trim();
    if t.is_empty() {
        return None;
    }
    if let Some(rest) = t.strip_prefix('[')
        && let Some((title, after)) = rest.split_once("](")
        && let Some(path) = after.strip_suffix(')')
    {
        let (path, _) = unwrap_path(path);
        return (!path.is_empty()).then(|| (title.trim().to_string(), path));
    }
    let (path, bracketed) = unwrap_path(t);
    looks_like_path(&path, bracketed).then_some((String::new(), path))
}

/// A path as the file holds it, and whether it was written in `<>`: the brackets are taken off,
/// and `%20` reads as a space, as a Markdown link's target does.
fn unwrap_path(text: &str) -> (String, bool) {
    let t = text.trim();
    match t.strip_prefix('<').and_then(|r| r.strip_suffix('>')) {
        Some(inner) => (inner.trim().replace("%20", " "), true),
        None => (t.replace("%20", " "), false),
    }
}

/// Whether a line with no link on it names a file, so that a sentence in a list is not taken
/// for one: it holds no character a path cannot, it has a folder separator or an extension of a
/// few letters, and it has no space unless it was written in `<>`.
fn looks_like_path(text: &str, bracketed: bool) -> bool {
    let extension = Path::new(text)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            (1..=8).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric())
        });
    !text.is_empty()
        && !text.contains(['*', '?', '"', '|', '\t'])
        && (bracketed || !text.contains(' '))
        && (text.contains('/') || text.contains('\\') || extension)
}

/// A path with no root, taken from the library folder. A Markdown link is written with forward
/// slashes; on Windows they become the separator the rest of the application spells paths with.
fn resolve(path: &str, base: &Path) -> PathBuf {
    let p = if cfg!(windows) {
        PathBuf::from(path.replace('/', "\\"))
    } else {
        PathBuf::from(path)
    };
    if p.is_absolute() || base.as_os_str().is_empty() {
        p
    } else {
        base.join(p)
    }
}

/// The line a note is written on: a link, with the path in `<>` when it holds a space.
pub fn link_line(title: &str, path: &Path) -> String {
    format!(
        "- [{}]({})",
        title.trim().replace(['[', ']'], ""),
        written_target(path)
    )
}

/// A path as a link's target: forward slashes, and `<>` around one a space would otherwise
/// break.
fn written_target(path: &Path) -> String {
    let written = path.to_string_lossy().replace('\\', "/");
    if written.contains(' ') {
        format!("<{written}>")
    } else {
        written
    }
}

/// A `home:` value as it is written: forward slashes, trimmed, and nothing when it has no text
/// or more than one line.
fn written_home(home: &Path) -> Option<String> {
    let written = home.to_string_lossy().replace('\\', "/");
    let written = written.trim();
    (!written.is_empty() && !written.contains(['\r', '\n'])).then(|| written.to_string())
}

/// Whether two paths, as a file writes them, are the same one: the same text under whichever
/// separator was used for a folder.
fn same_path(a: &str, b: &str) -> bool {
    a.trim().replace('\\', "/") == b.trim().replace('\\', "/")
}

/// The longest a folder name is made: a component, not the path, and a note's own file name
/// still has to fit beside it.
const FOLDER_NAME_MAX: usize = 64;

/// The names Windows keeps for a device, in any case and with any extension: no folder may be
/// called one.
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A name turned into the name of a folder or of a file's stem: a group, a section or a note. It is\n/// the one rule for all of them. Trimmed, the characters Windows forbids replaced by `_` (a `%`\n/// too, which the reader would take for the start of `%20`), a trailing dot or space taken off, a\n/// reserved device name made safe with a `_` after it (`COM7` is `COM7_`, `CON.txt` is\n/// `CON_.txt`), and the length capped so a note's file name fits in the path beside it. It reads\n/// and writes nothing: a name always comes out with something in it, an empty one as `Untitled`.
///
/// Two names may come out the same. Telling them apart needs the disk, so that is the app's job.
pub fn folder_name(name: &str) -> String {
    let replaced: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control()
                || matches!(
                    c,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '%'
                )
            {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut out: String = replaced
        .trim_end_matches([' ', '.'])
        .chars()
        .take(FOLDER_NAME_MAX)
        .collect();
    // The cap may leave a space or a dot at the end, which Windows would then take off itself.
    out.truncate(out.trim_end_matches([' ', '.']).len());
    if out.is_empty() {
        return "Untitled".to_string();
    }
    // Windows reads `CON.txt` as the device too, and ignores spaces before the dot, so the mark
    // goes straight after the device's own name.
    let (stem, rest) = out.split_at(out.find('.').unwrap_or(out.len()));
    if RESERVED_NAMES
        .iter()
        .any(|r| stem.trim_end().eq_ignore_ascii_case(r))
    {
        return format!("{}_{rest}", stem.trim_end());
    }
    out
}

/// The folder a note written under the heading path `section` belongs in, for a group whose
/// `home:` is `home`: that folder, then one [`folder_name`] per heading. It creates nothing.
pub fn note_folder(home: &Path, section: &[String]) -> PathBuf {
    let mut folder = home.to_path_buf();
    for name in section {
        folder.push(folder_name(name));
    }
    folder
}

/// A name as a writer writes it: trimmed, and nothing when it has no text or more than one
/// line.
fn clean(name: &str) -> Option<&str> {
    let name = name.trim();
    (!name.is_empty() && !name.contains(['\r', '\n'])).then_some(name)
}

/// A title as a link holds it: one line, with no bracket to end the link early.
fn link_title(title: &str) -> Option<String> {
    let title = clean(title)?.replace(['[', ']'], "");
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_string())
}

/// What a writer finds on one line of a library file.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Row {
    /// A heading, at the level it is written.
    Section { level: usize },
    /// A note.
    Note(NoteLine),
    /// A `home:` line: the group's folder, never a note.
    Home,
    /// A fence's own line, or a line inside one: never a heading and never a note.
    Code,
    /// Prose, a blank line, anything else.
    Other,
}

/// A note line taken apart, so a writer can change one part of it and leave the rest as it was
/// written.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NoteLine {
    /// What comes before it on the line: the indentation, and the list marker when it has one.
    lead: String,
    /// The link's text as written; empty for a path on its own.
    title: String,
    /// The target as written, `<>` and `%20` included; the path itself for a path on its own.
    target: String,
    /// It is written as a link rather than as a path on its own.
    link: bool,
}

/// A note line taken apart, or `None` when the line is not a note.
fn note_parts(line: &str) -> Option<NoteLine> {
    note_line(line)?;
    let start = line.len() - line.trim_start().len();
    let rest = &line[start..];
    let marker = if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
        2
    } else {
        0
    };
    let body = rest[marker..].trim();
    let lead = format!("{}{}", &line[..start], &rest[..marker]);
    if let Some(inner) = body.strip_prefix('[')
        && let Some((title, after)) = inner.split_once("](")
        && let Some(target) = after.strip_suffix(')')
    {
        return Some(NoteLine {
            lead,
            title: title.trim().to_string(),
            target: target.to_string(),
            link: true,
        });
    }
    Some(NoteLine {
        lead,
        title: String::new(),
        target: body.to_string(),
        link: false,
    })
}

/// A library file's text as its lines, each read once, and the line ending to write it back
/// with.
struct Text {
    lines: Vec<String>,
    rows: Vec<Row>,
    eol: &'static str,
}

impl Text {
    fn new(text: &str) -> Text {
        let mut rows = Vec::new();
        let mut fence: Option<String> = None;
        for line in text.lines() {
            if let Some(marker) = fence_marker(line) {
                fence = match &fence {
                    Some(open) if closes(&marker, open) => None,
                    None => Some(marker),
                    _ => fence,
                };
                rows.push(Row::Code);
            } else if fence.is_some() {
                rows.push(Row::Code);
            } else if let Some((level, _)) = heading(line) {
                rows.push(Row::Section { level });
            } else if home_line(line).is_some() {
                rows.push(Row::Home);
            } else {
                rows.push(match note_parts(line) {
                    Some(note) => Row::Note(note),
                    None => Row::Other,
                });
            }
        }
        Text {
            lines: text.lines().map(str::to_string).collect(),
            rows,
            eol: if text.contains("\r\n") { "\r\n" } else { "\n" },
        }
    }

    /// The whole file again, with the line ending it came with.
    fn into_text(self) -> String {
        let mut out = self.lines.join(self.eol);
        if !out.is_empty() {
            out.push_str(self.eol);
        }
        out
    }

    /// The first line that declares the group's home folder, if the file has one.
    fn home_row(&self) -> Option<usize> {
        self.rows.iter().position(|row| matches!(row, Row::Home))
    }

    /// Removes the line `at`, and a blank line either side of it that it leaves doubled.
    fn remove_row(&mut self, at: usize) {
        self.lines.remove(at);
        self.rows.remove(at);
        if at > 0
            && at < self.lines.len()
            && self.lines[at - 1].trim().is_empty()
            && self.lines[at].trim().is_empty()
        {
            self.lines.remove(at);
            self.rows.remove(at);
        }
    }

    /// Writes `line` under the heading path `section` (the end of the file when it is empty or
    /// not there yet) and gives the new text. A line goes after the last note written directly
    /// under that heading, before any heading below it, so the subsections keep their own notes.
    fn place(mut self, section: &[String], line: &str) -> String {
        let want: Vec<String> = section
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let found = (!want.is_empty())
            .then(|| heading_line(&self.lines, &want))
            .flatten();
        match found {
            Some((start, _)) => {
                let at = after_notes(&self.rows, start);
                self.lines.insert(at, line.to_string());
            }
            None if !want.is_empty() => {
                // The part of the path that is there already; the rest is written under it, and
                // the whole path at the end of the file when none of it is.
                let known = (1..want.len()).rev().find_map(|n| {
                    heading_line(&self.lines, &want[..n]).map(|(i, level)| (n, i, level))
                });
                let (from, mut at, mut level) = match known {
                    Some((n, i, level)) => (n, after_notes(&self.rows, i), level),
                    None => {
                        while self.lines.last().is_some_and(|l| l.trim().is_empty()) {
                            self.lines.pop();
                        }
                        (0, self.lines.len(), 1)
                    }
                };
                for name in &want[from..] {
                    level += 1;
                    self.lines.insert(at, String::new());
                    self.lines
                        .insert(at + 1, format!("{} {name}", "#".repeat(level)));
                    at += 2;
                }
                self.lines.insert(at, String::new());
                self.lines.insert(at + 1, line.to_string());
            }
            None => {
                if self.lines.last().is_some_and(|l| !l.trim().is_empty())
                    && self.lines.last().is_some_and(|l| note_line(l).is_none())
                {
                    self.lines.push(String::new());
                }
                self.lines.push(line.to_string());
            }
        }
        self.into_text()
    }
}

/// Where a line written directly under the heading on line `start` goes: after that heading's
/// own notes, before any heading below it, and never inside a code fence. Nothing is written
/// above the group's `home:` line, which stays directly under the heading.
fn after_notes(rows: &[Row], start: usize) -> usize {
    let end = rows
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, row)| matches!(row, Row::Section { .. }))
        .map_or(rows.len(), |(i, _)| i);
    let from = (start + 1..end)
        .find(|&i| matches!(rows[i], Row::Home))
        .map_or(start + 1, |i| i + 1);
    (from..end)
        .rev()
        .find(|&i| matches!(rows[i], Row::Note(_)))
        .map_or(from, |i| i + 1)
}

/// The line of the heading `want` names, with the level it is written at: the headings are
/// followed as a path, so `["Money", "2026"]` finds `### 2026` inside `## Money` and not a
/// `### 2026` under another section.
fn heading_line(lines: &[String], want: &[String]) -> Option<(usize, usize)> {
    let mut open: Vec<(usize, String)> = Vec::new();
    let mut fence: Option<String> = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some(marker) = fence_marker(line) {
            fence = match &fence {
                Some(o) if closes(&marker, o) => None,
                None => Some(marker),
                _ => fence,
            };
            continue;
        }
        if fence.is_some() {
            continue;
        }
        let Some((level, text)) = heading(line) else {
            continue;
        };
        if level == 1 {
            open.clear();
            continue;
        }
        if text.is_empty() {
            continue;
        }
        while open.last().is_some_and(|(l, _)| *l >= level) {
            open.pop();
        }
        open.push((level, text.to_string()));
        if open.len() == want.len()
            && open
                .iter()
                .zip(want)
                .all(|((_, t), w)| t.eq_ignore_ascii_case(w.trim()))
        {
            return Some((i, level));
        }
    }
    None
}

/// Writes a note into a library file's text under the heading path `section` (the end of the
/// file when it is empty or not there yet) and gives the new text. A note goes after the last
/// note written directly under that heading, before any heading below it, so the subsections
/// keep their own notes. The file's line ending is kept.
pub fn add_note(text: &str, section: &[String], title: &str, path: &Path) -> String {
    Text::new(text).place(section, &link_line(title, path))
}

/// Removes the note written on `line` and gives the new text; `None` when that line is not one
/// note, so a heading or a line inside a code fence is left alone. Only that line goes: the
/// blank lines around it stay where they are.
pub fn remove_note(text: &str, line: usize) -> Option<String> {
    let mut doc = Text::new(text);
    if !matches!(doc.rows.get(line.checked_sub(1)?), Some(Row::Note(_))) {
        return None;
    }
    doc.lines.remove(line - 1);
    doc.rows.remove(line - 1);
    Some(doc.into_text())
}

/// Rewrites the text of the link written on `line`, leaving its target exactly as it was
/// written; a line that is only a path becomes `- [Title](path)`. `None` when the line is not a
/// note, the title has no text, or the text would not change.
pub fn rename_note(text: &str, line: usize, title: &str) -> Option<String> {
    let title = link_title(title)?;
    let mut doc = Text::new(text);
    let Some(Row::Note(note)) = doc.rows.get(line.checked_sub(1)?) else {
        return None;
    };
    let new = format!("{}[{}]({})", note.lead, title, note.target);
    if doc.lines[line - 1] == new {
        return None;
    }
    doc.lines[line - 1] = new;
    Some(doc.into_text())
}

/// Rewrites the target of the note written on `line`, keeping its title and the way the line is
/// written otherwise. `None` when the line is not a note, or when it already names that path,
/// under whatever brackets and separators it was written with.
pub fn set_note_path(text: &str, line: usize, path: &Path) -> Option<String> {
    let mut doc = Text::new(text);
    let Some(Row::Note(note)) = doc.rows.get(line.checked_sub(1)?) else {
        return None;
    };
    let target = written_target(path);
    let said = unwrap_path(&note.target).0.replace('\\', "/");
    if said == unwrap_path(&target).0.replace('\\', "/") {
        return None;
    }
    let new = if note.link {
        format!("{}[{}]({})", note.lead, note.title, target)
    } else {
        format!("{}{}", note.lead, target)
    };
    doc.lines[line - 1] = new;
    Some(doc.into_text())
}

/// Rewrites the text of the section heading written on `line`, keeping its `#` count so its
/// nesting does not change. `None` when that line is not a section heading (a group's own `#`
/// heading is [`set_group_name`]'s), the name has no text, or the name would not change.
pub fn rename_section(text: &str, line: usize, name: &str) -> Option<String> {
    let name = clean(name)?;
    let mut doc = Text::new(text);
    let level = match doc.rows.get(line.checked_sub(1)?) {
        Some(Row::Section { level, .. }) if *level > 1 => *level,
        _ => return None,
    };
    let new = format!("{} {name}", "#".repeat(level));
    if doc.lines[line - 1] == new {
        return None;
    }
    doc.lines[line - 1] = new;
    Some(doc.into_text())
}

/// Writes a section heading under the heading path `parent` (the group itself when it is empty)
/// one level deeper than that heading, after its notes and before its subsections, and gives the
/// new text. `None` when that heading is not in the file or the name has no text.
pub fn add_section(text: &str, parent: &[String], name: &str) -> Option<String> {
    let name = clean(name)?;
    let mut doc = Text::new(text);
    let want: Vec<String> = parent
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let (at, level) = if want.is_empty() {
        match doc
            .rows
            .iter()
            .position(|row| matches!(row, Row::Section { level: 1, .. }))
        {
            Some(start) => (after_notes(&doc.rows, start), 1),
            None => {
                while doc.lines.last().is_some_and(|l| l.trim().is_empty()) {
                    doc.lines.pop();
                }
                (doc.lines.len(), 1)
            }
        }
    } else {
        let (start, level) = heading_line(&doc.lines, &want)?;
        (after_notes(&doc.rows, start), level)
    };
    // A blank line above the heading unless one is already there to be the gap.
    let mut add = Vec::new();
    if at > 0 && doc.lines[..at].last().is_none_or(|l| !l.trim().is_empty()) {
        add.push(String::new());
    }
    add.push(format!("{} {name}", "#".repeat(level + 1)));
    doc.lines.splice(at..at, add);
    Some(doc.into_text())
}

/// Removes the section heading written on `line` and everything under it, its subsections and
/// their notes included, and gives the new text. `None` when that line is not a section heading:
/// a group's own `#` heading is not one, and a group goes by removing its file.
pub fn remove_section(text: &str, line: usize) -> Option<String> {
    let mut doc = Text::new(text);
    let level = match doc.rows.get(line.checked_sub(1)?) {
        Some(Row::Section { level, .. }) if *level > 1 => *level,
        _ => return None,
    };
    let start = line - 1;
    let end = (start + 1..doc.rows.len())
        .find(|&i| matches!(doc.rows[i], Row::Section { level: l, .. } if l <= level))
        .unwrap_or(doc.rows.len());
    doc.lines.drain(start..end);
    if end == doc.rows.len() {
        while doc.lines.last().is_some_and(|l| l.trim().is_empty()) {
            doc.lines.pop();
        }
    }
    Some(doc.into_text())
}

/// Rewrites the file's first `#` heading, which is the group's name, and gives the new text.
/// `None` when the file has no `#` heading, the name has no text, or the name would not change.
pub fn set_group_name(text: &str, name: &str) -> Option<String> {
    let name = clean(name)?;
    let mut doc = Text::new(text);
    let at = doc
        .rows
        .iter()
        .position(|row| matches!(row, Row::Section { level: 1, .. }))?;
    let new = format!("# {name}");
    if doc.lines[at] == new {
        return None;
    }
    doc.lines[at] = new;
    Some(doc.into_text())
}

/// Writes the group's `home:` line directly under the file's first `#` heading, or rewrites the
/// line already there, and gives the new text; the file's line ending is kept. A path is written
/// with forward slashes, as a link's is. `None` when the file has no `#` heading, the path has no
/// text, or the file already says that path.
pub fn set_group_home(text: &str, home: &Path) -> Option<String> {
    let value = written_home(home)?;
    let mut doc = Text::new(text);
    if let Some(at) = doc.home_row() {
        if home_line(&doc.lines[at]).is_some_and(|said| same_path(&said, &value)) {
            return None;
        }
        doc.lines[at] = format!("home: {value}");
        return Some(doc.into_text());
    }
    let heading = doc
        .rows
        .iter()
        .position(|row| matches!(row, Row::Section { level: 1, .. }))?;
    // Directly under the heading, and the blank line a heading already has stays as the gap
    // after the new line; a file with none gets one.
    let mut add = vec![format!("home: {value}")];
    if doc
        .lines
        .get(heading + 1)
        .is_some_and(|l| !l.trim().is_empty())
    {
        add.push(String::new());
    }
    doc.lines.splice(heading + 1..heading + 1, add);
    Some(doc.into_text())
}

/// Removes the group's `home:` line, and any second one that would otherwise become it, and
/// gives the new text. `None` when the file has none.
pub fn clear_group_home(text: &str) -> Option<String> {
    let mut doc = Text::new(text);
    let mut removed = false;
    while let Some(at) = doc.home_row() {
        doc.remove_row(at);
        removed = true;
    }
    removed.then(|| doc.into_text())
}

/// The text of a new group file: [`TEMPLATE`] with its own name on it. `None` when the name has
/// no text.
pub fn new_group(name: &str) -> Option<String> {
    let name = clean(name)?;
    Some(TEMPLATE.replacen("# Notes", &format!("# {name}"), 1))
}

/// The text of a new group file that declares the folder its notes are written in, the line
/// written as [`set_group_home`] writes it. [`new_group`] is this with no folder, for a caller
/// that does not choose one.
pub fn new_group_with_home(name: &str, home: Option<&Path>) -> Option<String> {
    let text = new_group(name)?;
    Some(match home {
        Some(home) => set_group_home(&text, home).unwrap_or(text),
        None => text,
    })
}

/// Moves the note written on `line` to the heading path `to_path` of the same file, the line
/// exactly as it is written, and gives the new text. `None` when that line is not a note. Two
/// files are the caller's two calls.
pub fn move_note(text: &str, line: usize, to_path: &[String]) -> Option<String> {
    let doc = Text::new(text);
    if !matches!(doc.rows.get(line.checked_sub(1)?), Some(Row::Note(_))) {
        return None;
    }
    let moved = doc.lines[line - 1].clone();
    let without = remove_note(text, line)?;
    Some(Text::new(&without).place(to_path, &moved))
}

/// The line, counted from 1, of the section heading the path `section` names (`["Money",
/// "2026"]` is `### 2026` inside `## Money`), so a writer that takes a line can be given a
/// section by its path, and a command can lead to it. `None` when no such heading is written;
/// the first when the same path is written twice.
pub fn section_line(text: &str, section: &[String]) -> Option<usize> {
    let doc = Text::new(text);
    heading_line(&doc.lines, section).map(|(i, _)| i + 1)
}
#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mdshared-library-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_library_file_reads_as_a_group_of_notes_and_leaves_out_what_is_not_one() {
        let base = Path::new("/lib");
        let text = "# Personal\n\
            \n\
            Anything may be written here.\n\
            \n\
            ## Money\n\
            \n\
            - [Taxes 2026](D:/misc/taxes.md)\n\
            - D:/misc/receipts.md\n\
            - just a sentence about money\n\
            \n\
            ### 2025\n\
            \n\
            - [Last year](D:/misc/2025.md)\n\
            \n\
            ## Trips\n\
            \n\
            * [Japan](<D:/my notes/japan.md>)\n\
            + nearby.md\n\
            \n\
            ```\n\
            - [Not a note](D:/no.md)\n\
            ```\n";
        let Parsed {
            name,
            home: _,
            sections,
            entries,
        } = parse(text, base);
        assert_eq!(name.as_deref(), Some("Personal"));
        let seen: Vec<(&str, String, Vec<&str>)> = entries
            .iter()
            .map(|e| {
                (
                    e.title.as_str(),
                    e.path.to_string_lossy().replace('\\', "/"),
                    e.sections.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(
            seen,
            vec![
                ("Taxes 2026", "D:/misc/taxes.md".to_string(), vec!["Money"]),
                ("", "D:/misc/receipts.md".to_string(), vec!["Money"]),
                // A deeper heading nests inside the one above it.
                (
                    "Last year",
                    "D:/misc/2025.md".to_string(),
                    vec!["Money", "2025"]
                ),
                ("Japan", "D:/my notes/japan.md".to_string(), vec!["Trips"]),
                ("", "/lib/nearby.md".to_string(), vec!["Trips"]),
            ]
        );
        assert_eq!(entries[0].line, 7);
        assert!(entries.iter().all(|e| !e.missing));
        assert_eq!(
            sections,
            vec![
                vec!["Money".to_string()],
                vec!["Money".to_string(), "2025".to_string()],
                vec!["Trips".to_string()],
            ]
        );
        // A heading with no note under it yet is still a place a note can go.
        let empty = parse("# Notes\n\n## Later\n", base);
        assert_eq!(empty.sections, vec![vec!["Later".to_string()]]);
        assert!(empty.entries.is_empty());
    }

    #[test]
    fn the_library_folder_reads_as_groups_with_titles_and_what_is_missing() {
        let dir = scratch("read");
        let lib = dir.join("library");
        std::fs::create_dir_all(&lib).unwrap();
        let note = dir.join("kept.md");
        std::fs::write(&note, "\n# The kept note\n\ntext\n").unwrap();
        std::fs::write(dir.join("plain.md"), "no heading here\n").unwrap();
        std::fs::write(
            lib.join("a-work.md"),
            format!(
                "# Work\n\n## Now\n\n- {}\n- ../plain.md\n- {}\n",
                note.to_string_lossy().replace('\\', "/"),
                dir.join("gone.md").to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();
        std::fs::write(lib.join("b-rest.md"), "- nothing here\n").unwrap();
        std::fs::write(lib.join("notes.txt"), "- not read\n").unwrap();

        let library = read(&lib);
        assert_eq!(library.groups.len(), 2, "only the .md files");
        assert_eq!(library.groups[0].name, "Work");
        assert_eq!(
            library.groups[1].name, "b-rest",
            "no heading: the file name"
        );
        assert!(library.groups[1].entries.is_empty());
        let work = &library.groups[0].entries;
        assert_eq!(work.len(), 3);
        assert_eq!(work[0].title, "The kept note", "from its first heading");
        assert!(!work[0].missing);
        assert_eq!(work[1].title, "plain.md", "no heading: the file name");
        assert!(!work[1].missing, "a relative path is the library folder's");
        assert_eq!(work[2].title, "gone.md");
        assert!(work[2].missing);
        assert_eq!(library.len(), 3);
        assert!(!library.changed());

        // Saved again: the library knows it must be read again.
        std::fs::write(lib.join("a-work.md"), "# Work\n").unwrap();
        assert!(library.changed());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A heading path from the names given.
    fn at(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    /// A path as a test compares it: forward slashes, whatever the platform writes.
    fn written(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn a_note_is_added_under_its_section_and_reads_back() {
        let base = Path::new("/lib");
        let text = "# Personal\n\n## Money\n\n- [Taxes](D:/misc/taxes.md)\n\n## Trips\n\n- [Japan](D:/japan.md)\n";
        let out = add_note(
            text,
            &at(&["Money"]),
            "Receipts",
            Path::new("D:/misc/receipts.md"),
        );
        let entries = parse(&out, base).entries;
        let titles: Vec<&str> = entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, vec!["Taxes", "Receipts", "Japan"]);
        assert_eq!(entries[1].sections, at(&["Money"]));

        // A section that is not there yet is made at the end; a file with none takes it plainly.
        let out = add_note(&out, &at(&["Reading"]), "A book", Path::new("D:/book.md"));
        let entries = parse(&out, base).entries;
        assert_eq!(entries.last().unwrap().sections, at(&["Reading"]));
        let out = add_note("# Notes\n\nprose\n", &[], "One", Path::new("D:/one.md"));
        assert_eq!(out, "# Notes\n\nprose\n\n- [One](D:/one.md)\n");
        // A path with a space is written so that it reads back whole.
        let out = add_note(&out, &[], "Two", Path::new("D:/my notes/two.md"));
        let entries = parse(&out, base).entries;
        assert_eq!(
            entries
                .last()
                .unwrap()
                .path
                .to_string_lossy()
                .replace('\\', "/"),
            "D:/my notes/two.md"
        );
        // CRLF is kept.
        let out = add_note(
            "# Notes\r\n\r\n- [One](D:/one.md)\r\n",
            &[],
            "Two",
            Path::new("D:/two.md"),
        );
        assert!(out.ends_with("- [Two](D:/two.md)\r\n"), "{out:?}");
        assert!(!out.contains("\n\n"), "no bare line feed: {out:?}");
    }

    #[test]
    fn a_note_goes_under_a_nested_heading_and_a_missing_one_is_written_under_its_parent() {
        let base = Path::new("/lib");
        let text = "# Personal\n\
            \n\
            ## Money\n\
            \n\
            - [Taxes](D:/taxes.md)\n\
            \n\
            ### 2025\n\
            \n\
            - [Last year](D:/2025.md)\n\
            \n\
            ## Trips\n\
            \n\
            ### 2025\n\
            \n\
            - [Rome](D:/rome.md)\n";

        // The path picks the right "2025": the one inside Trips, not the one inside Money.
        let out = add_note(
            text,
            &at(&["Trips", "2025"]),
            "Vienna",
            Path::new("D:/v.md"),
        );
        let entries = parse(&out, base).entries;
        let seen: Vec<(&str, Vec<String>)> = entries
            .iter()
            .map(|e| (e.title.as_str(), e.sections.clone()))
            .collect();
        assert_eq!(
            seen,
            vec![
                ("Taxes", at(&["Money"])),
                ("Last year", at(&["Money", "2025"])),
                ("Rome", at(&["Trips", "2025"])),
                ("Vienna", at(&["Trips", "2025"])),
            ]
        );

        // A note under a heading that has subsections goes with that heading's own notes, not
        // at the end of its subsections.
        let out = add_note(text, &at(&["Money"]), "Receipts", Path::new("D:/r.md"));
        let entries = parse(&out, base).entries;
        assert_eq!(entries[1].title, "Receipts");
        assert_eq!(entries[1].sections, at(&["Money"]));

        // A heading that is not there yet is written under the part of the path that is.
        let out = add_note(
            text,
            &at(&["Money", "2026"]),
            "This year",
            Path::new("D:/n.md"),
        );
        let entries = parse(&out, base).entries;
        let this = entries.iter().find(|e| e.title == "This year").unwrap();
        assert_eq!(this.sections, at(&["Money", "2026"]));
        assert!(out.contains("### 2026"), "{out}");
        // Written inside Money, so the sections that follow are untouched.
        let sections = parse(&out, base).sections;
        assert_eq!(
            sections,
            vec![
                at(&["Money"]),
                at(&["Money", "2026"]),
                at(&["Money", "2025"]),
                at(&["Trips"]),
                at(&["Trips", "2025"]),
            ]
        );

        // None of the path there: the whole chain is written at the end.
        let out = add_note(
            "# Notes\n",
            &at(&["Reading", "2026"]),
            "A book",
            Path::new("D:/book.md"),
        );
        let parsed = parse(&out, base);
        assert_eq!(
            parsed.sections,
            vec![at(&["Reading"]), at(&["Reading", "2026"])]
        );
        assert_eq!(parsed.entries[0].sections, at(&["Reading", "2026"]));
    }

    /// A group with a section of its own, a subsection under it, and a second section.
    const SAMPLE: &str = "# Personal\n\
        \n\
        ## Money\n\
        \n\
        - [Taxes](D:/taxes.md)\n\
        \n\
        ### 2025\n\
        \n\
        - [Last year](D:/2025.md)\n\
        \n\
        ## Trips\n\
        \n\
        - [Japan](D:/japan.md)\n";

    #[test]
    fn a_note_is_removed_by_its_line_and_nothing_else_moves() {
        let text = "# Notes\n\n## A\n\n- [One](D:/one.md)\n- [Two](D:/two.md)\n";
        assert_eq!(
            remove_note(text, 5).unwrap(),
            "# Notes\n\n## A\n\n- [Two](D:/two.md)\n"
        );
        // The line numbers are the ones the parser gives, from the file's own text.
        assert_eq!(parse(text, Path::new("/lib")).entries[1].line, 6);
    }

    #[test]
    fn a_note_is_renamed_in_the_link_and_its_path_is_left_as_written() {
        assert_eq!(
            rename_note(SAMPLE, 5, "Taxes 2026").unwrap(),
            "# Personal\n\n## Money\n\n- [Taxes 2026](D:/taxes.md)\n\n### 2025\n\n- [Last year](D:/2025.md)\n\n## Trips\n\n- [Japan](D:/japan.md)\n"
        );
        // A path on its own becomes a link, with its brackets as they were.
        let text = "# Notes\n\n- <D:/my notes/a%20b.md>\n";
        assert_eq!(
            rename_note(text, 3, "A note").unwrap(),
            "# Notes\n\n- [A note](<D:/my notes/a%20b.md>)\n"
        );
        // A title is written without the brackets that would end the link early.
        assert_eq!(
            rename_note(text, 3, "a [bad] name").unwrap(),
            "# Notes\n\n- [a bad name](<D:/my notes/a%20b.md>)\n"
        );
    }

    #[test]
    fn a_note_path_is_rewritten_and_its_title_is_kept() {
        assert_eq!(
            set_note_path(SAMPLE, 5, Path::new("D:/misc/new taxes.md")).unwrap(),
            "# Personal\n\n## Money\n\n- [Taxes](<D:/misc/new taxes.md>)\n\n### 2025\n\n- [Last year](D:/2025.md)\n\n## Trips\n\n- [Japan](D:/japan.md)\n"
        );
        // A path on its own stays one: no title is invented for it.
        assert_eq!(
            set_note_path("# Notes\n\n- D:/old.md\n", 3, Path::new("D:/new.md")).unwrap(),
            "# Notes\n\n- D:/new.md\n"
        );
    }

    #[test]
    fn a_section_is_renamed_at_its_own_level_and_its_nesting_is_kept() {
        assert_eq!(
            rename_section(SAMPLE, 7, "2024").unwrap(),
            "# Personal\n\n## Money\n\n- [Taxes](D:/taxes.md)\n\n### 2024\n\n- [Last year](D:/2025.md)\n\n## Trips\n\n- [Japan](D:/japan.md)\n"
        );
        let parsed = parse(
            &rename_section(SAMPLE, 7, "2024").unwrap(),
            Path::new("/lib"),
        );
        assert_eq!(
            parsed.sections,
            vec![at(&["Money"]), at(&["Money", "2024"]), at(&["Trips"])],
            "the renamed heading is still under Money"
        );
        // A `##` stays a `##`, so the notes under it stay one level down.
        let out = rename_section(SAMPLE, 3, "Cash").unwrap();
        assert_eq!(
            out,
            "# Personal\n\n## Cash\n\n- [Taxes](D:/taxes.md)\n\n### 2025\n\n- [Last year](D:/2025.md)\n\n## Trips\n\n- [Japan](D:/japan.md)\n"
        );
        let parsed = parse(&out, Path::new("/lib"));
        assert_eq!(parsed.entries[1].sections, at(&["Cash", "2025"]));
    }

    #[test]
    fn a_section_is_added_after_its_parents_notes_and_before_its_subsections() {
        assert_eq!(
            add_section(SAMPLE, &at(&["Money"]), "2026").unwrap(),
            "# Personal\n\n## Money\n\n- [Taxes](D:/taxes.md)\n\n### 2026\n\n### 2025\n\n- [Last year](D:/2025.md)\n\n## Trips\n\n- [Japan](D:/japan.md)\n"
        );
        // The group itself is the parent of a top-level section.
        assert_eq!(
            add_section(SAMPLE, &[], "Later").unwrap(),
            "# Personal\n\n## Later\n\n## Money\n\n- [Taxes](D:/taxes.md)\n\n### 2025\n\n- [Last year](D:/2025.md)\n\n## Trips\n\n- [Japan](D:/japan.md)\n"
        );
        let parsed = parse(
            &add_section(SAMPLE, &at(&["Money", "2025"]), "March").unwrap(),
            Path::new("/lib"),
        );
        assert_eq!(
            parsed.sections[2],
            at(&["Money", "2025", "March"]),
            "March is written under 2025, before Trips"
        );
    }

    #[test]
    fn a_section_is_removed_with_the_notes_and_subsections_under_it() {
        // Money takes its subsection and that subsection's note with it.
        assert_eq!(
            remove_section(SAMPLE, 3).unwrap(),
            "# Personal\n\n## Trips\n\n- [Japan](D:/japan.md)\n"
        );
        // A subsection takes only what is its own.
        assert_eq!(
            remove_section(SAMPLE, 7).unwrap(),
            "# Personal\n\n## Money\n\n- [Taxes](D:/taxes.md)\n\n## Trips\n\n- [Japan](D:/japan.md)\n"
        );
        // The last section leaves no blank tail behind.
        assert_eq!(
            remove_section(SAMPLE, 11).unwrap(),
            "# Personal\n\n## Money\n\n- [Taxes](D:/taxes.md)\n\n### 2025\n\n- [Last year](D:/2025.md)\n"
        );
    }

    #[test]
    fn the_group_is_renamed_in_the_files_first_heading() {
        assert_eq!(
            set_group_name(SAMPLE, "Household").unwrap(),
            "# Household\n\n## Money\n\n- [Taxes](D:/taxes.md)\n\n### 2025\n\n- [Last year](D:/2025.md)\n\n## Trips\n\n- [Japan](D:/japan.md)\n"
        );
        let parsed = parse(
            &set_group_name(SAMPLE, "Household").unwrap(),
            Path::new("/lib"),
        );
        assert_eq!(parsed.name.as_deref(), Some("Household"));
        assert_eq!(parsed.entries[0].sections, at(&["Money"]), "nothing moved");
    }

    #[test]
    fn a_new_group_file_is_the_template_with_its_own_name() {
        let text = new_group("Work").unwrap();
        assert_eq!(text, TEMPLATE.replacen("# Notes", "# Work", 1));
        assert_eq!(
            parse(&text, Path::new("/lib")).name.as_deref(),
            Some("Work")
        );
        assert!(parse(&text, Path::new("/lib")).entries.is_empty());
        assert_eq!(new_group(" Notes "), Some(TEMPLATE.to_string()));
    }

    #[test]
    fn a_note_is_moved_between_the_sections_of_one_file() {
        let text = "# Notes\n\n## A\n\n- [One](D:/one.md)\n- [Two](D:/two.md)\n\n## B\n\n- [Three](D:/three.md)\n";
        let out = move_note(text, 5, &at(&["B"])).unwrap();
        assert_eq!(
            out,
            "# Notes\n\n## A\n\n- [Two](D:/two.md)\n\n## B\n\n- [Three](D:/three.md)\n- [One](D:/one.md)\n"
        );
        let entries = parse(&out, Path::new("/lib")).entries;
        assert_eq!(entries[2].title, "One", "last of B, where it was put");
        assert_eq!(entries[2].sections, at(&["B"]));
        // The line goes across exactly as it was written.
        let text = "# Notes\n\n## A\n\n- <D:/my notes/a%20b.md>\n\n## B\n";
        assert_eq!(
            move_note(text, 5, &at(&["B"])).unwrap(),
            "# Notes\n\n## A\n\n\n## B\n- <D:/my notes/a%20b.md>\n"
        );
    }

    #[test]
    fn a_crlf_file_stays_crlf() {
        let text = "# Notes\r\n\r\n## A\r\n\r\n- [One](D:/one.md)\r\n- [Two](D:/two.md)\r\n";
        let out = rename_note(text, 5, "Uno").unwrap();
        assert_eq!(
            out,
            "# Notes\r\n\r\n## A\r\n\r\n- [Uno](D:/one.md)\r\n- [Two](D:/two.md)\r\n"
        );
        assert!(
            !out.replace("\r\n", "").contains('\n'),
            "no bare line feed: {out:?}"
        );
        // A heading added to it is written the same way.
        assert_eq!(
            add_section(text, &at(&["A"]), "B").unwrap(),
            "# Notes\r\n\r\n## A\r\n\r\n- [One](D:/one.md)\r\n- [Two](D:/two.md)\r\n\r\n### B\r\n"
        );
        assert_eq!(
            set_group_name(text, "Home").unwrap(),
            "# Home\r\n\r\n## A\r\n\r\n- [One](D:/one.md)\r\n- [Two](D:/two.md)\r\n"
        );
    }

    #[test]
    fn nothing_inside_a_code_fence_is_taken_for_a_note_or_a_heading() {
        let text = "# Notes\n\
            \n\
            ## Money\n\
            \n\
            ```\n\
            - [Fake](D:/fake.md)\n\
            ## Fake heading\n\
            ```\n\
            \n\
            ### 2025\n";

        // A fenced line is refused by the writers that work on one line.
        assert_eq!(remove_note(text, 6), None);
        assert_eq!(rename_note(text, 6, "Real"), None);
        assert_eq!(set_note_path(text, 6, Path::new("D:/real.md")), None);
        assert_eq!(move_note(text, 6, &at(&["Money"])), None);
        assert_eq!(rename_section(text, 7, "Real"), None);
        assert_eq!(remove_section(text, 7), None);

        // And a new section goes above the fence rather than between its lines.
        assert_eq!(
            add_section(text, &at(&["Money"]), "2026").unwrap(),
            "# Notes\n\n## Money\n\n### 2026\n\n```\n- [Fake](D:/fake.md)\n## Fake heading\n```\n\n### 2025\n"
        );
        let parsed = parse(
            &add_section(text, &at(&["Money"]), "2026").unwrap(),
            Path::new("/lib"),
        );
        assert_eq!(parsed.entries.len(), 0);
        assert_eq!(
            parsed.sections,
            vec![
                at(&["Money"]),
                at(&["Money", "2026"]),
                at(&["Money", "2025"])
            ]
        );
    }

    #[test]
    fn a_writer_refuses_a_line_that_is_not_what_it_expects() {
        // A heading where a note was wanted, a note where a heading was, no line at all.
        assert_eq!(remove_note(SAMPLE, 1), None);
        assert_eq!(remove_note(SAMPLE, 0), None);
        assert_eq!(remove_note(SAMPLE, 99), None);
        assert_eq!(rename_note(SAMPLE, 3, "x"), None);
        assert_eq!(set_note_path(SAMPLE, 3, Path::new("D:/x.md")), None);
        assert_eq!(move_note(SAMPLE, 3, &at(&["Trips"])), None);
        assert_eq!(rename_section(SAMPLE, 5, "x"), None);
        assert_eq!(remove_section(SAMPLE, 5), None);
        assert_eq!(add_section(SAMPLE, &at(&["Nope"]), "x"), None);
        // The group's own heading belongs to set_group_name, not to the section writers.
        assert_eq!(rename_section(SAMPLE, 1, "x"), None);
        assert_eq!(remove_section(SAMPLE, 1), None);
        // Nothing to change, or nothing to name it.
        assert_eq!(rename_note(SAMPLE, 5, "Taxes"), None);
        assert_eq!(rename_note(SAMPLE, 5, "  "), None);
        assert_eq!(set_note_path(SAMPLE, 5, Path::new("D:/taxes.md")), None);
        // The same note written another way is still the same note.
        assert_eq!(
            set_note_path(
                "# Notes\n\n- <D:/my notes/a.md>\n",
                3,
                Path::new("D:/my notes/a.md")
            ),
            None
        );
        assert_eq!(
            set_note_path("# Notes\n\n- D:\\one.md\n", 3, Path::new("D:/one.md")),
            None
        );
        assert_eq!(rename_section(SAMPLE, 7, "2025"), None);
        assert_eq!(rename_section(SAMPLE, 7, ""), None);
        assert_eq!(add_section(SAMPLE, &[], " "), None);
        assert_eq!(set_group_name(SAMPLE, "Personal"), None);
        assert_eq!(set_group_name("- no heading here\n", "x"), None);
        assert_eq!(new_group(""), None);
    }

    #[test]
    fn every_writer_reads_back_as_the_library_it_wrote() {
        let dir = scratch("writers");
        let lib = dir.join("library");
        std::fs::create_dir_all(&lib).unwrap();
        let file = lib.join("notes.md");
        let base = lib.as_path();

        let notes = |text: &str| -> Vec<(String, Vec<String>)> {
            parse(text, base)
                .entries
                .iter()
                .map(|e| (e.title.clone(), e.sections.clone()))
                .collect()
        };
        let line_of = |text: &str, title: &str| -> usize {
            parse(text, base)
                .entries
                .iter()
                .find(|e| e.title == title)
                .unwrap()
                .line
        };
        let section_line = |text: &str, want: &[String]| -> usize {
            let lines: Vec<String> = text.lines().map(str::to_string).collect();
            heading_line(&lines, want).unwrap().0 + 1
        };

        let mut text = new_group("Personal").unwrap();
        text = add_section(&text, &[], "Money").unwrap();
        text = add_note(
            &text,
            &at(&["Money"]),
            "Taxes",
            Path::new("D:/misc/taxes.md"),
        );
        text = add_note(
            &text,
            &at(&["Money"]),
            "Receipts",
            Path::new("D:/misc/receipts.md"),
        );
        text = add_section(&text, &at(&["Money"]), "2025").unwrap();
        text = add_note(
            &text,
            &at(&["Money", "2025"]),
            "Last year",
            Path::new("D:/misc/2025.md"),
        );
        assert_eq!(
            notes(&text),
            vec![
                ("Taxes".to_string(), at(&["Money"])),
                ("Receipts".to_string(), at(&["Money"])),
                ("Last year".to_string(), at(&["Money", "2025"])),
            ]
        );
        std::fs::write(&file, &text).unwrap();
        let library = read(&lib);
        assert_eq!(library.groups.len(), 1);
        assert_eq!(library.groups[0].name, "Personal");
        assert_eq!(library.groups[0].entries.len(), 3);

        // Every write is checked the way the app checks it: the new text is parsed again and
        // read back.
        text = rename_note(&text, line_of(&text, "Taxes"), "Taxes 2026").unwrap();
        assert_eq!(
            notes(&text),
            vec![
                ("Taxes 2026".to_string(), at(&["Money"])),
                ("Receipts".to_string(), at(&["Money"])),
                ("Last year".to_string(), at(&["Money", "2025"])),
            ]
        );

        text = set_note_path(
            &text,
            line_of(&text, "Receipts"),
            Path::new("D:/misc/2025 receipts.md"),
        )
        .unwrap();
        assert_eq!(
            parse(&text, base).entries[1].path,
            Path::new("D:/misc/2025 receipts.md")
        );

        text = move_note(&text, line_of(&text, "Last year"), &at(&["Money"])).unwrap();
        assert_eq!(
            notes(&text),
            vec![
                ("Taxes 2026".to_string(), at(&["Money"])),
                ("Receipts".to_string(), at(&["Money"])),
                ("Last year".to_string(), at(&["Money"])),
            ]
        );

        let at_2025 = section_line(&text, &at(&["Money", "2025"]));
        text = rename_section(&text, at_2025, "2024").unwrap();
        assert_eq!(
            parse(&text, base).sections,
            vec![at(&["Money"]), at(&["Money", "2024"])]
        );

        text = add_section(&text, &at(&["Money"]), "2026").unwrap();
        text = remove_section(&text, section_line(&text, &at(&["Money", "2024"]))).unwrap();
        assert_eq!(
            parse(&text, base).sections,
            vec![at(&["Money"]), at(&["Money", "2026"])]
        );

        text = remove_note(&text, line_of(&text, "Receipts")).unwrap();
        text = set_group_name(&text, "Household").unwrap();
        std::fs::write(&file, &text).unwrap();

        let library = read(&lib);
        assert_eq!(library.groups[0].name, "Household");
        let seen: Vec<(&str, String, Vec<String>)> = library.groups[0]
            .entries
            .iter()
            .map(|e| {
                (
                    e.title.as_str(),
                    e.path.to_string_lossy().replace('\\', "/"),
                    e.sections.clone(),
                )
            })
            .collect();
        assert_eq!(
            seen,
            vec![
                ("Taxes 2026", "D:/misc/taxes.md".to_string(), at(&["Money"])),
                ("Last year", "D:/misc/2025.md".to_string(), at(&["Money"])),
            ]
        );
        assert_eq!(
            library.groups[0].sections,
            vec![at(&["Money"]), at(&["Money", "2026"])]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_section_is_found_by_its_path_and_its_line_leads_to_the_writers() {
        let text = "# G\n\n## Money\n\n### 2026\n\n- [A](D:/a.md)\n\n## Trips\n\n### 2026\n\n```\n## Not\n```\n";
        let path = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(section_line(text, &path(&["Money"])), Some(3));
        assert_eq!(section_line(text, &path(&["Money", "2026"])), Some(5));
        assert_eq!(section_line(text, &path(&["Trips", "2026"])), Some(11));
        assert_eq!(
            section_line(text, &path(&["money"])),
            Some(3),
            "as the writers match"
        );
        assert_eq!(
            section_line(text, &path(&["2026"])),
            None,
            "a path, not a name"
        );
        assert_eq!(
            section_line(text, &path(&["Not"])),
            None,
            "a fence holds no heading"
        );
        assert_eq!(section_line(text, &[]), None);
        // The line is what the section writers take.
        let renamed = rename_section(
            text,
            section_line(text, &path(&["Trips"])).unwrap(),
            "Travel",
        );
        assert!(renamed.unwrap().contains("## Travel\n"));
        let crlf = text.replace('\n', "\r\n");
        assert_eq!(section_line(&crlf, &path(&["Trips", "2026"])), Some(11));
    }

    #[test]
    fn a_group_home_is_read_from_the_file_and_taken_from_the_library_folder() {
        let base = Path::new("/lib");
        let parsed = parse(
            "# Personal\n\nhome: Personal\n\n- [Taxes](D:/misc/taxes.md)\n",
            base,
        );
        assert_eq!(
            parsed.home.as_deref().map(written),
            Some("/lib/Personal".to_string())
        );
        assert_eq!(parsed.name.as_deref(), Some("Personal"), "not in the name");
        assert_eq!(parsed.entries.len(), 1, "and not a note");
        assert_eq!(parsed.entries[0].title, "Taxes");
        assert!(parsed.entries[0].sections.is_empty(), "and not a section");
    }

    #[test]
    fn a_group_home_with_a_root_is_kept_as_it_is() {
        let parsed = parse("# Work\n\nhome: D:/notes/work\n", Path::new("/lib"));
        assert_eq!(
            parsed.home.as_deref().map(written),
            Some("D:/notes/work".to_string())
        );
        assert!(
            parsed.entries.is_empty(),
            "a path on its own is not taken for a note on this line"
        );
        // The same folder, written with the other separator, is the same folder.
        let parsed = parse("# Work\nhome: D:\\notes\\work\n", Path::new("/lib"));
        assert_eq!(
            parsed.home.as_deref().map(written),
            Some("D:/notes/work".to_string())
        );
    }

    #[test]
    fn only_the_first_home_line_counts() {
        let parsed = parse("# P\n\nhome: First\n\nhome: Second\n", Path::new("/lib"));
        assert_eq!(
            parsed.home.as_deref().map(written),
            Some("/lib/First".to_string())
        );
        assert!(parsed.entries.is_empty(), "the second is prose");
    }

    #[test]
    fn a_home_line_inside_a_code_fence_is_not_read() {
        let fenced = "# P\n\n```\nhome: Fenced\n```\n";
        assert_eq!(parse(fenced, Path::new("/lib")).home, None);
        let both = "# P\n\n```\nhome: Fenced\n```\n\nhome: Real\n";
        assert_eq!(
            parse(both, Path::new("/lib")).home.as_deref().map(written),
            Some("/lib/Real".to_string())
        );
    }

    #[test]
    fn a_file_with_a_home_line_reads_as_the_same_notes_and_sections() {
        let base = Path::new("/lib");
        let without = "# Personal\n\
            \n\
            ## Money\n\
            \n\
            - [Taxes](D:/taxes.md)\n\
            \n\
            ### 2025\n\
            \n\
            - [Last year](D:/2025.md)\n\
            \n\
            ## Trips\n\
            \n\
            - [Japan](D:/japan.md)\n";
        // A reader that does not know the line is the same reader otherwise: it is ignored as
        // content, so the notes, the sections and the name are the ones the file has without it.
        let with = without.replacen("# Personal\n\n", "# Personal\n\nhome: Personal\n\n", 1);
        let a = parse(&with, base);
        let b = parse(without, base);
        assert_eq!(
            a.home.as_deref().map(written),
            Some("/lib/Personal".to_string())
        );
        assert_eq!(b.home, None);
        assert_eq!(a.name, b.name);
        assert_eq!(a.sections, b.sections);
        let seen = |parsed: &Parsed| -> Vec<(String, String, Vec<String>)> {
            parsed
                .entries
                .iter()
                .map(|e| (e.title.clone(), written(&e.path), e.sections.clone()))
                .collect()
        };
        assert_eq!(seen(&a), seen(&b));
        assert_eq!(a.entries.len(), 3);
    }

    #[test]
    fn the_group_home_is_written_under_the_heading_and_rewritten_there() {
        assert_eq!(
            set_group_home("# Personal\n", Path::new("Personal")).unwrap(),
            "# Personal\nhome: Personal\n"
        );
        // The blank line the heading already has stays as the gap after the new line.
        assert_eq!(
            set_group_home("# Personal\n\n## Money\n", Path::new("Personal")).unwrap(),
            "# Personal\nhome: Personal\n\n## Money\n"
        );
        // A line that is there is rewritten where it is, and nothing else moves.
        assert_eq!(
            set_group_home(
                "# Personal\nhome: Old\n\n## Money\n",
                Path::new("New/Place")
            )
            .unwrap(),
            "# Personal\nhome: New/Place\n\n## Money\n"
        );
        // Line endings the file did not have are not invented.
        assert_eq!(
            set_group_home("# Notes\nprose\n", Path::new("Notes")).unwrap(),
            "# Notes\nhome: Notes\n\nprose\n"
        );
        // An absolute path is written with forward slashes, as a link's is.
        assert_eq!(
            set_group_home("# Work\n", Path::new("D:\\notes\\work")).unwrap(),
            "# Work\nhome: D:/notes/work\n"
        );
        // Notes and sections written under the group stay below the line.
        let text = set_group_home("# P\n", Path::new("P")).unwrap();
        assert_eq!(
            add_note(&text, &[], "One", Path::new("D:/one.md")),
            "# P\nhome: P\n\n- [One](D:/one.md)\n"
        );
        assert_eq!(
            add_section(&text, &[], "Money").unwrap(),
            "# P\nhome: P\n\n## Money\n"
        );
    }

    #[test]
    fn setting_a_group_home_refuses_an_unchanged_path_or_no_heading() {
        assert_eq!(set_group_home("# P\nhome: P\n", Path::new("P")), None);
        assert_eq!(
            set_group_home("# P\nhome: <D:/my notes/x>\n", Path::new("D:/my notes/x")),
            None
        );
        assert_eq!(
            set_group_home("# P\nhome: D:\\notes\\work\n", Path::new("D:/notes/work")),
            None
        );
        assert_eq!(set_group_home("- no heading\n", Path::new("P")), None);
        assert_eq!(set_group_home("# P\nhome: x\n", Path::new("")), None);
    }

    #[test]
    fn the_group_home_is_cleared_and_leaves_no_double_gap() {
        assert_eq!(
            clear_group_home("# P\nhome: X\n\n## M\n").unwrap(),
            "# P\n\n## M\n"
        );
        assert_eq!(
            clear_group_home("# P\n\nhome: X\n\n## M\n").unwrap(),
            "# P\n\n## M\n"
        );
        // A second line would otherwise become the home; both go.
        assert_eq!(
            clear_group_home("# P\nhome: A\nhome: B\n\n## M\n").unwrap(),
            "# P\n\n## M\n"
        );
        assert_eq!(clear_group_home("# P\n\n## M\n"), None);
        // What was under the line stays under it.
        assert_eq!(
            clear_group_home("# P\nhome: X\n\n- [One](D:/one.md)\n").unwrap(),
            "# P\n\n- [One](D:/one.md)\n"
        );
    }

    #[test]
    fn a_crlf_file_keeps_its_line_endings_with_a_home_line() {
        let text = "# Notes\r\n\r\n## A\r\n\r\n- [One](D:/one.md)\r\n";
        let out = set_group_home(text, Path::new("Notes")).unwrap();
        assert_eq!(
            out,
            "# Notes\r\nhome: Notes\r\n\r\n## A\r\n\r\n- [One](D:/one.md)\r\n"
        );
        assert!(
            !out.replace("\r\n", "").contains('\n'),
            "no bare line feed: {out:?}"
        );
        assert_eq!(clear_group_home(&out).unwrap(), text);
    }

    #[test]
    fn a_folder_name_replaces_what_windows_forbids_and_is_never_empty() {
        assert_eq!(folder_name(" Money: 2026? "), "Money_ 2026_");
        assert_eq!(folder_name("a/b\\c<d>e\"f|g*h?i"), "a_b_c_d_e_f_g_h_i");
        assert_eq!(folder_name("a\tb\nc"), "a_b_c");
        // A device name is made safe, in any case and with a trailing dot.
        assert_eq!(folder_name("CON"), "CON_");
        assert_eq!(folder_name("con."), "con_");
        assert_eq!(folder_name("com1"), "com1_");
        assert_eq!(folder_name("lpt9"), "lpt9_");
        assert_eq!(folder_name("Console"), "Console", "not a device name");
        // Only dots, only spaces, nothing: a folder still needs a name.
        assert_eq!(folder_name("..."), "Untitled");
        assert_eq!(folder_name(" .. "), "Untitled");
        assert_eq!(folder_name(""), "Untitled");
        assert_eq!(folder_name("2026."), "2026");
        // A long name is capped, and the cap leaves no dot or space at the end.
        assert_eq!(folder_name(&"a".repeat(200)), "a".repeat(64));
        assert_eq!(
            folder_name(&format!("{}.", "x".repeat(100))),
            "x".repeat(64)
        );
    }

    #[test]
    fn a_percent_sign_is_replaced_because_the_reader_takes_percent_20_for_a_space() {
        assert_eq!(folder_name("50%20off"), "50_20off");
        assert_eq!(folder_name("100%"), "100_");
        // The name as a link's file reads back as the file that was written: nothing in it is
        // decoded into another name.
        let name = folder_name("50%20off");
        let line = link_line("Sale", Path::new(&format!("D:/notes/{name}.md")));
        let parsed = parse(&format!("# G\n\n{line}\n"), Path::new("D:/lib"));
        assert_eq!(
            parsed.entries[0].path.file_name().and_then(|n| n.to_str()),
            Some("50_20off.md")
        );
    }

    #[test]
    fn every_device_name_is_made_safe_in_any_case_and_with_any_extension() {
        for name in RESERVED_NAMES {
            assert_eq!(folder_name(name), format!("{name}_"), "{name}");
            let lower = name.to_ascii_lowercase();
            assert_eq!(folder_name(&lower), format!("{lower}_"), "{lower}");
            // Windows reads CON.txt as the device too, so the mark goes after the device's name.
            assert_eq!(
                folder_name(&format!("{name}.txt")),
                format!("{name}_.txt"),
                "{name}.txt"
            );
            assert_eq!(
                folder_name(&format!("{name} ")),
                format!("{name}_"),
                "{name} "
            );
        }
        assert_eq!(folder_name("COM10"), "COM10", "COM10 is not a device name");
        assert_eq!(folder_name("COM"), "COM");
        assert_eq!(folder_name("Console.txt"), "Console.txt");
    }
    #[test]
    fn a_note_folder_follows_the_heading_path_from_the_home() {
        let home = Path::new("D:/notes/Personal");
        assert_eq!(written(&note_folder(home, &[])), "D:/notes/Personal");
        assert_eq!(
            written(&note_folder(home, &at(&["Money"]))),
            "D:/notes/Personal/Money"
        );
        assert_eq!(
            written(&note_folder(home, &at(&["Money", "2025", "Taxes?"]))),
            "D:/notes/Personal/Money/2025/Taxes_"
        );
        // No home known is still a folder path, and nothing is created.
        assert_eq!(
            note_folder(Path::new(""), &at(&["Money"])),
            PathBuf::from("Money")
        );
    }

    #[test]
    fn a_new_group_file_can_declare_its_folder() {
        let text = new_group_with_home("Work", Some(Path::new("Work"))).unwrap();
        assert!(text.starts_with("# Work\nhome: Work\n\n"), "{text:?}");
        let parsed = parse(&text, Path::new("/lib"));
        assert_eq!(parsed.name.as_deref(), Some("Work"));
        assert_eq!(
            parsed.home.as_deref().map(written),
            Some("/lib/Work".to_string())
        );
        assert!(parsed.entries.is_empty());
        assert_eq!(
            new_group_with_home("Work", None).unwrap(),
            new_group("Work").unwrap(),
            "no folder is today's new group"
        );
        assert_eq!(new_group_with_home(" ", Some(Path::new("x"))), None);
    }

    #[test]
    fn a_group_written_with_a_home_and_two_nested_notes_reads_back_as_the_library() {
        let dir = scratch("home");
        let lib = dir.join("library");
        std::fs::create_dir_all(&lib).unwrap();
        let home = lib.join("Personal");

        let mut text = new_group_with_home("Personal", Some(Path::new("Personal"))).unwrap();
        assert!(
            text.starts_with("# Personal\nhome: Personal\n\n"),
            "{text:?}"
        );
        text = add_section(&text, &[], "Money").unwrap();
        text = add_section(&text, &at(&["Money"]), "2025").unwrap();
        text = add_note(
            &text,
            &at(&["Money"]),
            "Receipts",
            &note_folder(&home, &at(&["Money"])).join("receipts.md"),
        );
        text = add_note(
            &text,
            &at(&["Money", "2025"]),
            "Last year",
            &note_folder(&home, &at(&["Money", "2025"])).join("last year.md"),
        );
        std::fs::write(lib.join("personal.md"), &text).unwrap();

        let library = read(&lib);
        assert_eq!(library.groups.len(), 1);
        let group = &library.groups[0];
        assert_eq!(group.name, "Personal");
        assert_eq!(group.home.as_deref(), Some(home.as_path()));
        let seen: Vec<(&str, String, Vec<String>)> = group
            .entries
            .iter()
            .map(|e| (e.title.as_str(), written(&e.path), e.sections.clone()))
            .collect();
        assert_eq!(
            seen,
            vec![
                (
                    "Receipts",
                    written(&home.join("Money").join("receipts.md")),
                    at(&["Money"]),
                ),
                (
                    "Last year",
                    written(&home.join("Money").join("2025").join("last year.md")),
                    at(&["Money", "2025"]),
                ),
            ]
        );
        // Each note sits in the folder its heading path names.
        for entry in &group.entries {
            assert_eq!(
                written(entry.path.parent().unwrap()),
                written(&note_folder(&home, &entry.sections))
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A library folder holding `count` groups, named `g000` and up, written in the order given
    /// so that a test can prove the reader sorts the folder itself rather than trusting it.
    fn groups_folder(name: &str, count: usize, backwards: bool) -> PathBuf {
        let dir = scratch(name);
        let lib = dir.join("library");
        std::fs::create_dir_all(&lib).unwrap();
        let mut order: Vec<usize> = (0..count).collect();
        if backwards {
            order.reverse();
        }
        for n in order {
            std::fs::write(
                lib.join(format!("g{n:03}.md")),
                format!("# Group {n:03}\n\n- [Note {n:03}](D:/notes/{n:03}.md)\n"),
            )
            .unwrap();
        }
        lib
    }

    #[test]
    fn a_library_of_six_hundred_files_reads_the_first_five_hundred_and_twelve_and_counts_the_rest()
    {
        let lib = groups_folder("cap", 600, false);
        let library = read(&lib);
        assert_eq!(MAX_FILES, 512, "the cap the pane words the count with");
        assert_eq!(library.groups.len(), MAX_FILES);
        assert_eq!(library.omitted, 600 - MAX_FILES, "left out, and counted");
        assert_eq!(library.groups[0].name, "Group 000", "the first by name");
        assert_eq!(library.groups[MAX_FILES - 1].name, "Group 511");
        assert_eq!(library.len(), MAX_FILES, "one note in each group");
        // It is still the same library: the extra files are not a change to read again for.
        assert!(!library.changed());
        let _ = std::fs::remove_dir_all(lib.parent().unwrap());
    }

    #[test]
    fn a_library_of_seventy_files_reads_all_of_them_and_counts_nothing() {
        let lib = groups_folder("seventy", 70, false);
        let library = read(&lib);
        assert_eq!(library.groups.len(), 70);
        assert_eq!(library.omitted, 0, "nothing is left out under the cap");
        assert_eq!(library.groups[69].name, "Group 069");
        assert!(!library.changed());
        let _ = std::fs::remove_dir_all(lib.parent().unwrap());
    }

    #[test]
    fn a_file_that_is_not_a_library_file_does_not_use_up_the_cap() {
        let lib = groups_folder("others", 70, false);
        // Six hundred other files, sorting before every group, and a folder that merely ends in
        // `.md`: neither is a library file, so neither may take a group's place.
        for n in 0..600 {
            std::fs::write(lib.join(format!("a{n:03}.txt")), "not a library file\n").unwrap();
        }
        std::fs::create_dir(lib.join("z-folder.md")).unwrap();
        let library = read(&lib);
        assert_eq!(library.groups.len(), 70, "every group is still read");
        assert_eq!(library.omitted, 0);
        assert_eq!(library.groups[0].name, "Group 000");
        let _ = std::fs::remove_dir_all(lib.parent().unwrap());
    }

    #[test]
    fn two_reads_of_one_folder_give_the_same_groups_in_the_same_order() {
        // Written back to front: the order has to come from the reader's sort, not the folder.
        let lib = groups_folder("same-twice", 600, true);
        let first = read(&lib);
        let second = read(&lib);
        assert_eq!(first, second);
        let names: Vec<&str> = second.groups.iter().map(|g| g.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "the groups are in name order");
        let _ = std::fs::remove_dir_all(lib.parent().unwrap());
    }
}
