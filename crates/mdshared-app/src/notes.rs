//! The notes under a folder: which files there are, which links each holds, where a wiki-link
//! leads and which notes link to a given one (its backlinks).
//!
//! [`NoteIndex::build`] walks the folder within the bounds of a [`Config`] and reads each note
//! with [`wikilink::scan`], not with a Markdown parser. A second build that is handed the first
//! re-reads only the files whose size or time changed, and [`NoteIndex::update_file`] brings one
//! file up to date. [`Indexer`] does the builds on a thread of its own, started with the first
//! folder; it waits on a channel, so it costs nothing while nothing changes.
//!
//! Resolving a wiki-link ([`NoteIndex::resolve`]): a bare name is looked for in the linking
//! file's folder, then anywhere under the root, the file nearest the root winning; a path is
//! tried from the root, then from the linking file's folder, then as the end of any note's path.
//! Names compare without case. Ties go to the earlier extension in [`Config::extensions`], then
//! to the path that sorts first.

use crate::files::{self, FileStamp};
use crate::paths::strip_verbatim;
use crate::wikilink::{self, LinkSyntax};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

/// Which files are notes and how much a build may read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// File extensions of notes, lowercase, without the dot. A name written without one is tried
    /// with each in this order.
    pub extensions: Vec<String>,
    /// Notes listed at most. The walk goes level by level, so the ones left out are the deepest.
    pub max_files: usize,
    /// A larger file is listed, and links to it resolve, but its own links are not read.
    pub max_file_bytes: u64,
    /// Bytes of note text read for links at most, over all files.
    pub max_total_bytes: u64,
    /// Folder levels below the root that are entered; the root's own files are level 0.
    pub max_depth: usize,
    /// Directory entries looked at before the walk stops, whatever they are.
    pub max_entries: usize,
    /// Links kept per file.
    pub max_links_per_file: usize,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            extensions: vec!["md".into(), "markdown".into()],
            max_files: 20_000,
            max_file_bytes: 2 * 1024 * 1024,
            max_total_bytes: 256 * 1024 * 1024,
            max_depth: 16,
            max_entries: 200_000,
            max_links_per_file: 2_000,
        }
    }
}

/// The most of a line that is kept as a link's context, in bytes.
const MAX_CONTEXT: usize = 160;

/// Whether a folder tree leaves this entry out: a name starting with a dot, the hidden attribute
/// on Windows, and the build folders `target` and `node_modules`. The Files tree of
/// `mdshared-ui` applies the same rule.
pub fn skips_entry(name: &str, is_dir: bool, meta: &std::fs::Metadata) -> bool {
    name.starts_with('.') || is_hidden(meta) || (is_dir && skips_folder_name(name))
}

fn skips_folder_name(name: &str) -> bool {
    name.starts_with('.') || name == "target" || name == "node_modules"
}

fn is_hidden(_meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        _meta.file_attributes() & 0x2 != 0
    }
    #[cfg(not(windows))]
    false
}

/// A link written in a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteLink {
    /// A wiki-link; otherwise a Markdown link or reference definition with a relative path.
    pub wiki: bool,
    /// The note a wiki-link names, as written; a Markdown link's decoded path.
    pub target: String,
    /// A wiki-link's heading text, or a Markdown link's fragment, as written.
    pub heading: Option<String>,
    /// Line of the file the link is on, from 0.
    pub line: u32,
    /// Byte offset of the link in the file's decoded text.
    pub offset: usize,
    /// The link's line, trimmed and cut to a bounded length.
    pub context: String,
    /// The nearest heading above the link.
    pub section: Option<String>,
    /// The target's last path component, lowercase and without a note extension: what a note's
    /// name must equal for a wiki-link to reach it.
    name_key: String,
}

/// One file of the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub path: PathBuf,
    /// The path below the root, with `/` between the parts.
    pub rel: String,
    /// `rel` lowercase and without its extension: what wiki-links compare with.
    key: String,
    /// `rel` as files are told apart: lowercase on Windows.
    ident: String,
    /// Position of the extension in `Config::extensions`.
    ext_rank: usize,
    stamp: FileStamp,
    /// False when the file was too large, over the total, or unreadable: `links` is then empty.
    pub scanned: bool,
    pub links: Vec<NoteLink>,
}

impl Note {
    /// The file name without its extension.
    pub fn name(&self) -> &str {
        let file = self.rel.rsplit('/').next().unwrap_or(&self.rel);
        file.rsplit_once('.').map_or(file, |(stem, _)| stem)
    }

    fn name_key(&self) -> &str {
        self.key.rsplit('/').next().unwrap_or(&self.key)
    }

    fn dir_key(&self) -> &str {
        dir_of(&self.key)
    }

    fn depth(&self) -> usize {
        self.key.matches('/').count()
    }
}

fn dir_of(key: &str) -> &str {
    key.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// What a build did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub files: usize,
    pub folders: usize,
    /// Directory entries looked at.
    pub entries: usize,
    /// Files read and scanned by this build.
    pub read_files: usize,
    /// Files taken over unchanged from the previous index.
    pub reused_files: usize,
    /// Bytes of the files whose links the index holds.
    pub scanned_bytes: u64,
    pub links: usize,
    /// A limit stopped the walk, left files out or left some unread.
    pub truncated: bool,
}

/// A link to a note from another one.
#[derive(Debug, Clone, Copy)]
pub struct Backlink<'a> {
    pub from: &'a Note,
    pub link: &'a NoteLink,
}

#[derive(Debug, Clone)]
pub struct NoteIndex {
    root: PathBuf,
    /// The root as text with `/`, and the same as files are told apart.
    root_display: String,
    root_ident: String,
    config: Config,
    /// Sorted by path without case.
    notes: Vec<Arc<Note>>,
    /// Name key to notes, best first: nearest the root, then extension order, then path order.
    by_name: HashMap<String, Vec<u32>>,
    by_ident: HashMap<String, u32>,
    stats: Stats,
}

impl NoteIndex {
    /// Walks `root` and reads its notes. `previous`, when it is an index of the same root, lends
    /// every file whose size and time are unchanged, so only changed files are read again.
    /// `None` when `cancelled` answered true; it is asked once per folder and per file.
    pub fn build(
        root: &Path,
        config: &Config,
        previous: Option<&NoteIndex>,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<NoteIndex> {
        let mut index = NoteIndex {
            root: root.to_path_buf(),
            root_display: path_display(root),
            root_ident: ident_of(&path_display(root)),
            config: config.clone(),
            notes: Vec::new(),
            by_name: HashMap::new(),
            by_ident: HashMap::new(),
            stats: Stats::default(),
        };
        let previous = previous.filter(|p| p.root_ident == index.root_ident && p.config == *config);
        let mut stats = Stats::default();
        let mut queue = VecDeque::from([(root.to_path_buf(), String::new(), 0usize)]);
        'walk: while let Some((dir, rel_dir, depth)) = queue.pop_front() {
            if cancelled() {
                return None;
            }
            let Ok(read) = std::fs::read_dir(&dir) else {
                continue;
            };
            stats.folders += 1;
            let mut entries = Vec::new();
            for entry in read.flatten() {
                stats.entries += 1;
                if stats.entries > config.max_entries {
                    stats.truncated = true;
                    break;
                }
                // The entry's own type and attributes: a link to a folder is not followed, so
                // the walk cannot loop.
                let (Ok(kind), Ok(meta)) = (entry.file_type(), entry.metadata()) else {
                    continue;
                };
                let name = entry.file_name().to_string_lossy().into_owned();
                if kind.is_symlink() || skips_entry(&name, kind.is_dir(), &meta) {
                    continue;
                }
                if kind.is_dir() || extension_rank(&name, config).is_some() {
                    entries.push((name, kind.is_dir(), meta));
                }
            }
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, is_dir, meta) in entries {
                let rel = if rel_dir.is_empty() {
                    name.clone()
                } else {
                    format!("{rel_dir}/{name}")
                };
                if is_dir {
                    if depth < config.max_depth {
                        queue.push_back((dir.join(&name), rel, depth + 1));
                    } else {
                        stats.truncated = true;
                    }
                    continue;
                }
                if index.notes.len() >= config.max_files {
                    stats.truncated = true;
                    break 'walk;
                }
                if cancelled() {
                    return None;
                }
                let stamp = FileStamp::Present {
                    modified: meta.modified().ok(),
                    size: meta.len(),
                };
                let kept = previous
                    .and_then(|p| p.by_ident.get(&ident_of(&rel)))
                    .map(|&i| &previous.expect("checked").notes[i as usize])
                    .filter(|n| n.stamp == stamp && n.rel == rel);
                let note = match kept {
                    Some(note)
                        if note.scanned
                            || meta.len() > config.max_file_bytes
                            || stats.scanned_bytes + meta.len() > config.max_total_bytes =>
                    {
                        stats.reused_files += 1;
                        note.clone()
                    }
                    _ => {
                        let room = stats.scanned_bytes + meta.len() <= config.max_total_bytes;
                        Arc::new(read_note(dir.join(&name), rel, stamp, room, config))
                    }
                };
                if note.scanned {
                    stats.scanned_bytes += meta.len();
                    if kept.is_none_or(|k| !Arc::ptr_eq(k, &note)) {
                        stats.read_files += 1;
                    }
                } else {
                    stats.truncated |= meta.len() > 0;
                }
                index.notes.push(note);
            }
            if stats.truncated && stats.entries > config.max_entries {
                break;
            }
        }
        index.stats = stats;
        index.finish();
        Some(index)
    }

    /// Sorts the notes and rebuilds the look-up tables and the counts that follow from them.
    fn finish(&mut self) {
        self.notes
            .sort_by(|a, b| (&a.key, &a.rel).cmp(&(&b.key, &b.rel)));
        self.by_ident = self
            .notes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.ident.clone(), i as u32))
            .collect();
        self.by_name.clear();
        for (i, n) in self.notes.iter().enumerate() {
            self.by_name
                .entry(n.name_key().to_string())
                .or_default()
                .push(i as u32);
        }
        let notes = &self.notes;
        for list in self.by_name.values_mut() {
            list.sort_by(|&a, &b| {
                let (a, b) = (&notes[a as usize], &notes[b as usize]);
                (a.depth(), a.ext_rank, &a.key, &a.rel).cmp(&(
                    b.depth(),
                    b.ext_rank,
                    &b.key,
                    &b.rel,
                ))
            });
        }
        self.stats.files = self.notes.len();
        self.stats.links = self.notes.iter().map(|n| n.links.len()).sum();
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Every note, in path order.
    pub fn notes(&self) -> impl Iterator<Item = &Note> {
        self.notes.iter().map(|n| &**n)
    }

    /// The note at `path`, if the index lists it.
    pub fn note(&self, path: &Path) -> Option<&Note> {
        let ident = self.ident_below_root(path)?;
        self.by_ident.get(&ident).map(|&i| &*self.notes[i as usize])
    }

    /// Whether `path` is below the root, by spelling.
    pub fn covers(&self, path: &Path) -> bool {
        self.rel_below_root(path).is_some()
    }

    /// `path` below the root, with `/` between the parts, spelled as given.
    fn rel_below_root(&self, path: &Path) -> Option<String> {
        let full = path_display(path);
        let at = self.root_display.len();
        if full.len() <= at
            || !full.is_char_boundary(at)
            || ident_of(&full[..at]) != self.root_ident
        {
            return None;
        }
        let rest = &full[at..];
        if self.root_display.ends_with('/') {
            return Some(rest.to_string());
        }
        rest.strip_prefix('/').map(str::to_string)
    }

    fn ident_below_root(&self, path: &Path) -> Option<String> {
        self.rel_below_root(path).map(|rel| ident_of(&rel))
    }

    /// Where a wiki-link's note part leads, written in the note at `from` (which need not be in
    /// the index, or under the root). The module documentation has the order of the search.
    pub fn resolve(&self, from: Option<&Path>, target: &str) -> Option<&Note> {
        let wanted = Wanted::new(target, &self.config)?;
        let from_dir = from
            .and_then(|p| self.ident_below_root(p))
            .map(|ident| dir_of(&ident).to_lowercase());
        self.resolve_wanted(from_dir.as_deref(), &wanted)
    }

    fn resolve_wanted(&self, from_dir: Option<&str>, wanted: &Wanted) -> Option<&Note> {
        let candidates = self.by_name.get(wanted.name())?;
        let pick = |fits: &dyn Fn(&Note) -> bool| {
            candidates
                .iter()
                .map(|&i| &*self.notes[i as usize])
                .find(|n| wanted.ext.is_none_or(|e| e == n.ext_rank) && fits(n))
        };
        if !wanted.key.contains('/') {
            return from_dir
                .and_then(|dir| pick(&|n| n.dir_key() == dir))
                .or_else(|| pick(&|_| true));
        }
        let rooted = normalize("", &wanted.key);
        if let Some(key) = &rooted
            && let Some(note) = pick(&|n| n.key == *key)
        {
            return Some(note);
        }
        if let Some(key) = from_dir.and_then(|dir| normalize(dir, &wanted.key))
            && let Some(note) = pick(&|n| n.key == key)
        {
            return Some(note);
        }
        let tail = format!("/{}", rooted?);
        pick(&|n| n.key.ends_with(&tail))
    }

    /// The note a relative Markdown link in the note at `from` names: `path` is the link's
    /// decoded path, extension included.
    pub fn resolve_relative(&self, from: &Path, path: &str) -> Option<&Note> {
        let from = self.ident_below_root(from)?;
        let ident = normalize(dir_of(&from), &ident_of(&path.replace('\\', "/")))?;
        self.by_ident.get(&ident).map(|&i| &*self.notes[i as usize])
    }

    /// The links to the note at `path` from other notes, in path order and then line order, at
    /// most `max`. A note's links to itself do not count.
    pub fn backlinks(&self, path: &Path, max: usize) -> Vec<Backlink<'_>> {
        let Some(target) = self.note(path) else {
            return Vec::new();
        };
        let name = target.name_key();
        let mut out = Vec::new();
        for from in self.notes.iter().map(|n| &**n) {
            if std::ptr::eq(from, target) {
                continue;
            }
            let from_dir = from.dir_key();
            for link in &from.links {
                if link.name_key != name {
                    continue;
                }
                let hit = if link.wiki {
                    Wanted::new(&link.target, &self.config)
                        .and_then(|w| self.resolve_wanted(Some(from_dir), &w))
                } else {
                    normalize(dir_of(&from.ident), &ident_of(&link.target))
                        .and_then(|ident| self.by_ident.get(&ident))
                        .map(|&i| &*self.notes[i as usize])
                };
                if hit.is_some_and(|n| std::ptr::eq(n, target)) {
                    if out.len() == max {
                        return out;
                    }
                    out.push(Backlink { from, link });
                }
            }
        }
        out
    }

    /// Brings one file up to date after it was written, created or removed: reads it again, adds
    /// it or drops it. False when nothing changed (also for a path the index does not cover).
    /// The file and total byte limits apply; the file count limit does not, for one file.
    pub fn update_file(&mut self, path: &Path) -> bool {
        let Some(given) = self.rel_below_root(path) else {
            return false;
        };
        let old = self.by_ident.get(&ident_of(&given)).copied();
        let stamp = files::stamp(path);
        // A file not listed yet is spelled as the caller gave it.
        let rel = old.map_or(given, |i| self.notes[i as usize].rel.clone());
        let listed = stamp != FileStamp::Missing
            && path.is_file()
            && extension_rank(&rel, &self.config).is_some()
            && !rel
                .rsplit_once('/')
                .is_some_and(|(dirs, _)| dirs.split('/').any(skips_folder_name))
            && !rel.rsplit('/').next().is_some_and(|n| n.starts_with('.'));
        match (old, listed) {
            (None, false) => return false,
            (Some(i), false) => {
                self.stats.scanned_bytes -= scanned_size(&self.notes[i as usize]);
                self.notes.remove(i as usize);
            }
            (Some(i), true) if self.notes[i as usize].stamp == stamp => return false,
            (old, true) => {
                if let Some(i) = old {
                    self.stats.scanned_bytes -= scanned_size(&self.notes[i as usize]);
                }
                let size = match stamp {
                    FileStamp::Present { size, .. } => size,
                    FileStamp::Missing => 0,
                };
                let room = self.stats.scanned_bytes + size <= self.config.max_total_bytes;
                let note = Arc::new(read_note(
                    path.to_path_buf(),
                    rel,
                    stamp,
                    room,
                    &self.config,
                ));
                self.stats.scanned_bytes += scanned_size(&note);
                match old {
                    Some(i) => self.notes[i as usize] = note,
                    None => self.notes.push(note),
                }
            }
        }
        self.finish();
        true
    }
}

fn scanned_size(note: &Note) -> u64 {
    match note.stamp {
        FileStamp::Present { size, .. } if note.scanned => size,
        _ => 0,
    }
}

/// A wiki-link's note part, ready to compare: lowercase, `/` between parts, a note extension
/// taken off and remembered.
struct Wanted {
    key: String,
    ext: Option<usize>,
}

impl Wanted {
    fn new(target: &str, config: &Config) -> Option<Wanted> {
        let lower = target.trim().replace('\\', "/").to_lowercase();
        let lower = lower.trim_start_matches('/');
        let (key, ext) = match extension_rank(lower, config) {
            Some(rank) => (
                &lower[..lower.len() - config.extensions[rank].len() - 1],
                Some(rank),
            ),
            None => (lower, None),
        };
        let wanted = Wanted {
            key: key.to_string(),
            ext,
        };
        (!wanted.name().is_empty()).then_some(wanted)
    }

    fn name(&self) -> &str {
        self.key.rsplit('/').next().unwrap_or(&self.key)
    }
}

/// The position in `Config::extensions` of a name's extension, compared without case.
fn extension_rank(name: &str, config: &Config) -> Option<usize> {
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() || stem.ends_with('/') {
        return None;
    }
    config
        .extensions
        .iter()
        .position(|e| e.eq_ignore_ascii_case(ext))
}

/// Joins `path` onto `dir` (both with `/`), resolving `.` and `..`. `None` when it climbs above
/// the root.
fn normalize(dir: &str, path: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

/// A path as text with `/` between the parts, no verbatim prefix and no trailing separator.
fn path_display(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut s = strip_verbatim(&s)
        .unwrap_or_else(|| s.into_owned())
        .replace('\\', "/");
    while s.len() > 1 && s.ends_with('/') && !s.ends_with(":/") {
        s.pop();
    }
    s
}

/// How files are told apart: by spelling, without case on Windows.
fn ident_of(rel: &str) -> String {
    if cfg!(windows) {
        rel.to_lowercase()
    } else {
        rel.to_string()
    }
}

fn read_note(path: PathBuf, rel: String, stamp: FileStamp, room: bool, config: &Config) -> Note {
    let size = match stamp {
        FileStamp::Present { size, .. } => size,
        FileStamp::Missing => 0,
    };
    let bytes = (room && size <= config.max_file_bytes)
        .then(|| std::fs::read(&path).ok())
        .flatten()
        // The file may have grown since it was listed.
        .filter(|b| b.len() as u64 <= config.max_file_bytes);
    let links = bytes
        .as_deref()
        .map(|b| scan_links(&files::decode(b).0, config));
    let ext_rank = extension_rank(&rel, config).unwrap_or(0);
    let ext_len = rel.rsplit_once('.').map_or(0, |(_, e)| e.len() + 1);
    Note {
        path,
        key: rel[..rel.len() - ext_len].to_lowercase(),
        ident: ident_of(&rel),
        rel,
        ext_rank,
        stamp,
        scanned: links.is_some(),
        links: links.unwrap_or_default(),
    }
}

/// The links of one note's text that can name another note: wiki-links with a note part, and
/// Markdown links and definitions whose destination is a relative path to a note file.
pub fn scan_links(text: &str, config: &Config) -> Vec<NoteLink> {
    let mut out = Vec::new();
    wikilink::scan(text, |found| {
        let (wiki, target, heading) = match found.syntax {
            LinkSyntax::Wiki { .. } => match wikilink::parse_target(found.target) {
                Some(t) if !t.note.is_empty() => {
                    (true, t.note.to_string(), t.heading.map(str::to_string))
                }
                _ => return true,
            },
            LinkSyntax::Inline | LinkSyntax::Definition => {
                match wikilink::relative_destination(found.target) {
                    Some((path, fragment)) if extension_rank(&path, config).is_some() => {
                        (false, path.replace('\\', "/"), fragment.map(str::to_string))
                    }
                    _ => return true,
                }
            }
        };
        let name_key = match Wanted::new(&target, config) {
            Some(w) => w.name().to_string(),
            None => return true,
        };
        out.push(NoteLink {
            wiki,
            target,
            heading,
            line: found.line,
            offset: found.offset,
            context: context_of(found.line_text),
            section: found.section.map(|s| cut(s, MAX_CONTEXT).to_string()),
            name_key,
        });
        out.len() < config.max_links_per_file
    });
    out
}

fn context_of(line: &str) -> String {
    let line = line.trim();
    let short = cut(line, MAX_CONTEXT);
    if short.len() < line.len() {
        format!("{}…", short.trim_end())
    } else {
        short.to_string()
    }
}

fn cut(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Where a wiki-link's note part leads when only the linking file's folder may be looked at: the
/// one folder the target names (the linking file's own for a bare name) is listed once, nothing
/// is walked. For an application with no root folder open.
pub fn resolve_in_folder(dir: &Path, target: &str, config: &Config) -> Option<PathBuf> {
    let wanted = Wanted::new(target, config)?;
    let written = target.trim().replace('\\', "/");
    let mut folder = dir.to_path_buf();
    if let Some((dirs, _)) = written.trim_start_matches('/').rsplit_once('/') {
        for part in dirs.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    folder.pop();
                }
                other => folder.push(other),
            }
        }
    }
    let mut best: Option<(usize, String)> = None;
    for entry in std::fs::read_dir(&folder)
        .ok()?
        .flatten()
        .take(config.max_entries)
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(rank) = extension_rank(&name, config) else {
            continue;
        };
        let stem = &name[..name.len() - config.extensions[rank].len() - 1];
        if stem.to_lowercase() != wanted.name()
            || wanted.ext.is_some_and(|e| e != rank)
            || !entry.file_type().is_ok_and(|t| t.is_file())
        {
            continue;
        }
        if best.as_ref().is_none_or(|b| (rank, &name) < (b.0, &b.1)) {
            best = Some((rank, name));
        }
    }
    best.map(|(_, name)| folder.join(name))
}

enum Job {
    /// Index this folder from nothing.
    Build(PathBuf, u64),
    /// Walk the current folder again, reading only what changed.
    Refresh,
}

struct Shared {
    /// Raised by every `set_root`: a build for an older number stops and its result is dropped.
    generation: AtomicU64,
    ready: Mutex<Option<Arc<NoteIndex>>>,
}

/// Builds and refreshes a [`NoteIndex`] on a thread of its own. The thread starts with the first
/// folder and sleeps on a channel between jobs.
pub struct Indexer {
    config: Config,
    wake: Arc<dyn Fn() + Send + Sync>,
    shared: Arc<Shared>,
    jobs: Option<Sender<Job>>,
    root: Option<PathBuf>,
}

impl Indexer {
    /// `wake` is called on the indexer's thread each time a new index is ready to
    /// [`take`](Self::take).
    pub fn new(config: Config, wake: impl Fn() + Send + Sync + 'static) -> Indexer {
        Indexer {
            config,
            wake: Arc::new(wake),
            shared: Arc::new(Shared {
                generation: AtomicU64::new(0),
                ready: Mutex::new(None),
            }),
            jobs: None,
            root: None,
        }
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Indexes `root` from now on, or nothing. A build under way for another folder is cancelled
    /// and an index not yet taken is dropped. The same folder again does nothing.
    pub fn set_root(&mut self, root: Option<PathBuf>) {
        if root == self.root {
            return;
        }
        let generation = self.shared.generation.fetch_add(1, Ordering::AcqRel) + 1;
        *self.shared.ready.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.root = root.clone();
        if let Some(root) = root {
            self.send(Job::Build(root, generation));
        }
    }

    /// Asks for the current folder to be walked again, after something in it changed. Requests
    /// that pile up while a build runs are served by one walk.
    pub fn refresh(&mut self) {
        if self.root.is_some() {
            self.send(Job::Refresh);
        }
    }

    /// The newest finished index, once.
    pub fn take(&self) -> Option<Arc<NoteIndex>> {
        self.shared
            .ready
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    fn send(&mut self, job: Job) {
        if self.jobs.is_none() {
            let (send, receive) = channel();
            let (config, wake, shared) =
                (self.config.clone(), self.wake.clone(), self.shared.clone());
            let spawned = std::thread::Builder::new()
                .name("note-index".into())
                .spawn(move || work(receive, config, wake, shared));
            if spawned.is_err() {
                return;
            }
            self.jobs = Some(send);
        }
        if let Some(jobs) = &self.jobs {
            _ = jobs.send(job);
        }
    }
}

impl Drop for Indexer {
    fn drop(&mut self) {
        // Stops a build under way; closing the channel ends the thread.
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
    }
}

fn work(
    jobs: Receiver<Job>,
    config: Config,
    wake: Arc<dyn Fn() + Send + Sync>,
    shared: Arc<Shared>,
) {
    let mut current: Option<Arc<NoteIndex>> = None;
    let mut root: Option<(PathBuf, u64)> = None;
    while let Ok(mut job) = jobs.recv() {
        // Only the newest request matters, and several refreshes are one.
        let mut rebuild = false;
        loop {
            if let Job::Build(path, generation) = job {
                root = Some((path, generation));
                current = None;
                rebuild = true;
            }
            match jobs.try_recv() {
                Ok(next) => job = next,
                Err(_) => break,
            }
        }
        let live = || shared.generation.load(Ordering::Acquire);
        let Some((path, generation)) = root.as_ref().filter(|r| r.1 == live()) else {
            continue;
        };
        let cancelled = || live() != *generation;
        let previous = if rebuild { None } else { current.as_deref() };
        let Some(index) = NoteIndex::build(path, &config, previous, &cancelled) else {
            continue;
        };
        let index = Arc::new(index);
        current = Some(index.clone());
        {
            let mut ready = shared.ready.lock().unwrap_or_else(|e| e.into_inner());
            if cancelled() {
                continue;
            }
            *ready = Some(index);
        }
        wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

    /// A folder of its own under the temporary folder, removed when dropped.
    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Dir {
            let dir =
                std::env::temp_dir().join(format!("mdshared-notes-{name}-{}", std::process::id()));
            _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Dir(dir.canonicalize().unwrap())
        }

        fn write(&self, rel: &str, text: &str) -> PathBuf {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
            path
        }

        fn index(&self) -> NoteIndex {
            self.index_with(&Config::default())
        }

        fn index_with(&self, config: &Config) -> NoteIndex {
            NoteIndex::build(&self.0, config, None, &|| false).unwrap()
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            _ = fs::remove_dir_all(&self.0);
        }
    }

    fn rels(index: &NoteIndex) -> Vec<&str> {
        index.notes().map(|n| n.rel.as_str()).collect()
    }

    #[test]
    fn the_walk_lists_notes_and_skips_what_the_files_tree_skips() {
        let d = Dir::new("walk");
        d.write("a.md", "");
        d.write("B.MARKDOWN", "");
        d.write("notes.txt", "");
        d.write("sub/deep/c.md", "");
        d.write(".hidden/x.md", "");
        d.write("target/x.md", "");
        d.write("node_modules/pkg/readme.md", "");
        d.write("sub/.draft.md", "");
        let index = d.index();
        assert_eq!(rels(&index), ["a.md", "B.MARKDOWN", "sub/deep/c.md"]);
        let stats = index.stats();
        assert_eq!((stats.files, stats.folders, stats.truncated), (3, 3, false));
        assert!(index.covers(&d.0.join("sub/new.md")));
        assert!(!index.covers(d.0.parent().unwrap()));
    }

    #[test]
    fn a_bare_name_resolves_in_the_same_folder_then_nearest_the_root() {
        let d = Dir::new("resolve");
        let top = d.write("Top.md", "");
        d.write("a/Note.md", "");
        let in_b = d.write("b/Note.md", "");
        d.write("b/deep/Note.md", "");
        d.write("b/deep/Only.markdown", "");
        d.write("b/deep/Only.md", "");
        d.write("z/Both.markdown", "");
        d.write("y/Both.md", "");
        let index = d.index();
        let to =
            |from: Option<&Path>, target: &str| index.resolve(from, target).map(|n| n.rel.as_str());
        // The same folder wins; otherwise the nearest the root, then the path that sorts first.
        assert_eq!(to(Some(&in_b), "Note"), Some("b/Note.md"));
        assert_eq!(
            to(Some(&d.0.join("b/deep/x.md")), "note"),
            Some("b/deep/Note.md")
        );
        assert_eq!(to(Some(&top), "NOTE"), Some("a/Note.md"));
        assert_eq!(to(None, "Note"), Some("a/Note.md"));
        // `.md` before `.markdown` at the same depth, wherever they sort.
        assert_eq!(to(Some(&top), "Only"), Some("b/deep/Only.md"));
        assert_eq!(to(Some(&top), "Both"), Some("y/Both.md"));
        assert_eq!(
            to(Some(&top), "Only.markdown"),
            Some("b/deep/Only.markdown")
        );
        assert_eq!(to(Some(&top), "Missing"), None);
        assert_eq!(to(Some(&top), ""), None);
        assert_eq!(to(Some(&top), "a/"), None);
        // A file outside the root has no folder of its own here.
        assert_eq!(
            to(Some(&d.0.parent().unwrap().join("x.md")), "Note"),
            Some("a/Note.md")
        );
    }

    #[test]
    fn a_path_resolves_from_the_root_then_the_folder_then_as_a_tail() {
        let d = Dir::new("paths");
        d.write("deep/Note.md", "");
        d.write("a/deep/Note.md", "");
        d.write("a/b/deep/Other.md", "");
        d.write("a/Side.md", "");
        let from = d.write("a/From.md", "");
        let index = d.index();
        let to = |target: &str| index.resolve(Some(&from), target).map(|n| n.rel.as_str());
        assert_eq!(to("deep/Note"), Some("deep/Note.md"));
        assert_eq!(to("/deep/Note"), Some("deep/Note.md"));
        assert_eq!(to("a\\deep\\note.MD"), Some("a/deep/Note.md"));
        assert_eq!(to("./deep/Note"), Some("deep/Note.md"));
        assert_eq!(to("b/deep/Other"), Some("a/b/deep/Other.md"));
        assert_eq!(to("../deep/Note"), Some("deep/Note.md"));
        assert_eq!(to("deep/Other"), Some("a/b/deep/Other.md"));
        assert_eq!(to("../../deep/Note"), None);
        assert_eq!(to("nowhere/Note"), None);
    }

    #[test]
    fn backlinks_count_wiki_links_and_relative_markdown_links() {
        let d = Dir::new("backlinks");
        let target = d.write(
            "notes/Target.md",
            "# Target\n\n[[Target]] links to itself.\n",
        );
        d.write(
            "Index.md",
            "# Index\n\n## Reading\n\n- see [[Target#Part|the target]] first\n- `[[Target]]` in code\n\n```\n[[Target]]\n```\n",
        );
        d.write(
            "notes/Other.md",
            "Plain [link](Target.md#part) and [up](../Index.md).\n\n[ref]: ./Target.md\n",
        );
        d.write(
            "notes/Web.md",
            "[x](https://e.com/Target.md) [y](/Target.md) [[Targets]]\n",
        );
        d.write(
            "Target.md",
            "Another note of the same name. [[notes/Target]]\n",
        );
        let index = d.index();
        // (file, line, context, section, heading) of each backlink
        #[allow(clippy::type_complexity)]
        let found: Vec<(&str, u32, &str, Option<&str>, Option<&str>)> = index
            .backlinks(&target, 100)
            .iter()
            .map(|b| {
                (
                    b.from.rel.as_str(),
                    b.link.line,
                    b.link.context.as_str(),
                    b.link.section.as_deref(),
                    b.link.heading.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                (
                    "notes/Other.md",
                    0,
                    "Plain [link](Target.md#part) and [up](../Index.md).",
                    None,
                    Some("part")
                ),
                ("notes/Other.md", 2, "[ref]: ./Target.md", None, None),
                (
                    "Target.md",
                    0,
                    "Another note of the same name. [[notes/Target]]",
                    None,
                    None
                ),
            ]
        );
        // `[[Target]]` from the root's Index.md names the root's Target.md, the nearer one.
        let top: Vec<&str> = index
            .backlinks(&d.0.join("Target.md"), 100)
            .iter()
            .map(|b| b.link.context.as_str())
            .collect();
        assert_eq!(top, ["- see [[Target#Part|the target]] first"]);
        assert_eq!(index.backlinks(&d.0.join("Index.md"), 100).len(), 1);
        assert_eq!(index.backlinks(&target, 1).len(), 1);
        assert!(index.backlinks(&d.0.join("gone.md"), 100).is_empty());
    }

    #[test]
    fn one_file_is_brought_up_to_date() {
        let d = Dir::new("update");
        let target = d.write("Target.md", "");
        let a = d.write("A.md", "nothing yet\n");
        let mut index = d.index();
        assert!(index.backlinks(&target, 10).is_empty());
        assert!(!index.update_file(&a), "an unchanged file changes nothing");

        fs::write(&a, "now [[Target]] is linked, and more text\n").unwrap();
        assert!(index.update_file(&a));
        assert_eq!(index.backlinks(&target, 10).len(), 1);

        let b = d.write("sub/B.md", "[t](../Target.md)\n");
        assert!(index.update_file(&b));
        assert_eq!(rels(&index), ["A.md", "sub/B.md", "Target.md"]);
        assert_eq!(index.backlinks(&target, 10).len(), 2);
        assert_eq!(index.resolve(Some(&a), "b").map(|n| n.name()), Some("B"));

        fs::remove_file(&a).unwrap();
        assert!(index.update_file(&a));
        assert_eq!(index.backlinks(&target, 10).len(), 1);
        assert_eq!(index.stats().files, 2);

        // Files the walk would not list are not added.
        assert!(!index.update_file(&d.write(".git/x.md", "[[Target]]")));
        assert!(!index.update_file(&d.write("target/x.md", "[[Target]]")));
        assert!(!index.update_file(&d.write("notes.txt", "[[Target]]")));
        assert!(!index.update_file(&d.0.parent().unwrap().join("outside.md")));
    }

    #[test]
    fn a_second_build_reads_only_what_changed() {
        let d = Dir::new("rebuild");
        let target = d.write("Target.md", "");
        let a = d.write("A.md", "[[Target]]\n");
        d.write("B.md", "[[Target]]\n");
        let first = d.index();
        assert_eq!(
            (first.stats().read_files, first.stats().reused_files),
            (3, 0)
        );

        fs::write(&a, "no longer a link to the target\n").unwrap();
        d.write("C.md", "[[Target]] from a new file\n");
        fs::remove_file(d.0.join("B.md")).unwrap();
        let second = NoteIndex::build(&d.0, &Config::default(), Some(&first), &|| false).unwrap();
        assert_eq!(
            (second.stats().read_files, second.stats().reused_files),
            (2, 1)
        );
        let from: Vec<&str> = second
            .backlinks(&target, 10)
            .iter()
            .map(|b| b.from.rel.as_str())
            .collect();
        assert_eq!(from, ["C.md"]);
    }

    #[test]
    fn the_limits_bound_the_walk_and_the_reading() {
        let d = Dir::new("limits");
        for i in 0..10 {
            d.write(&format!("n{i}.md"), "[[n0]] some text to count\n");
        }
        d.write("a/b/c/deep.md", "[[n0]]\n");
        d.write("big.md", &"[[n0]] ".repeat(100));
        let config = |change: &dyn Fn(&mut Config)| {
            let mut c = Config::default();
            change(&mut c);
            c
        };

        let few = d.index_with(&config(&|c| c.max_files = 4));
        assert_eq!((few.stats().files, few.stats().truncated), (4, true));

        let shallow = d.index_with(&config(&|c| c.max_depth = 2));
        assert!(shallow.note(&d.0.join("a/b/c/deep.md")).is_none());
        assert!(shallow.stats().truncated);
        assert!(d.index().note(&d.0.join("a/b/c/deep.md")).is_some());

        // A file over the size limit is listed and resolvable, but its links are not read.
        let small = d.index_with(&config(&|c| c.max_file_bytes = 100));
        let big = small.note(&d.0.join("big.md")).unwrap();
        assert!(!big.scanned && big.links.is_empty());
        assert!(small.resolve(None, "big").is_some());
        assert!(small.stats().truncated);

        let budget = d.index_with(&config(&|c| c.max_total_bytes = 60));
        // n0 and n1 (26 bytes each) and the 7 bytes of deep.md fit; nothing else does.
        assert_eq!(budget.stats().read_files, 3);
        assert!(budget.stats().scanned_bytes <= 60);
        assert_eq!(budget.stats().files, 12);

        let entries = d.index_with(&config(&|c| c.max_entries = 5));
        assert!(entries.stats().files <= 5 && entries.stats().truncated);

        let links = d.index_with(&config(&|c| c.max_links_per_file = 7));
        assert_eq!(links.note(&d.0.join("big.md")).unwrap().links.len(), 7);
    }

    #[test]
    fn a_cancelled_build_gives_nothing() {
        let d = Dir::new("cancel");
        d.write("a.md", "");
        assert!(NoteIndex::build(&d.0, &Config::default(), None, &|| true).is_none());
    }

    #[test]
    fn a_name_resolves_in_one_folder_without_an_index() {
        let d = Dir::new("folder");
        d.write("Note.markdown", "");
        d.write("Note.md", "");
        d.write("sub/Deep.md", "");
        d.write("sub/more/Deeper.md", "");
        d.write("Folder.md/x.md", "");
        let config = Config::default();
        let to = |dir: &str, target: &str| {
            resolve_in_folder(&d.0.join(dir), target, &config).map(|p| {
                p.strip_prefix(&d.0)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
        };
        assert_eq!(to("", "note"), Some("Note.md".into()));
        assert_eq!(to("", "Note.markdown"), Some("Note.markdown".into()));
        assert_eq!(to("", "sub/deep"), Some("sub/Deep.md".into()));
        assert_eq!(to("sub", "../Note"), Some("Note.md".into()));
        // Nothing below the folder is searched, and a folder is not a note.
        assert_eq!(to("", "Deeper"), None);
        assert_eq!(to("", "Folder"), None);
        assert_eq!(to("gone", "Note"), None);
    }

    fn wait_for(indexer: &Indexer, woken: &Receiver<()>) -> Arc<NoteIndex> {
        woken
            .recv_timeout(Duration::from_secs(10))
            .expect("the indexer wakes");
        indexer.take().expect("an index is ready after a wake")
    }

    #[test]
    fn the_indexer_builds_refreshes_and_changes_folder_off_thread() {
        let d = Dir::new("indexer");
        let target = d.write("Target.md", "");
        d.write("A.md", "[[Target]]\n");
        let other = Dir::new("indexer-other");
        other.write("Elsewhere.md", "");

        let (send, woken) = channel();
        let mut indexer = Indexer::new(Config::default(), move || _ = send.send(()));
        assert!(indexer.take().is_none());
        indexer.refresh();
        assert!(
            woken.recv_timeout(Duration::from_millis(100)).is_err(),
            "no folder, no work"
        );

        indexer.set_root(Some(d.0.clone()));
        let first = wait_for(&indexer, &woken);
        assert_eq!(first.backlinks(&target, 10).len(), 1);
        assert!(indexer.take().is_none(), "an index is handed over once");

        d.write("B.md", "[[Target]]\n");
        indexer.refresh();
        let second = wait_for(&indexer, &woken);
        assert_eq!(second.backlinks(&target, 10).len(), 2);
        assert_eq!(second.stats().reused_files, 2);

        indexer.set_root(Some(other.0.clone()));
        let third = wait_for(&indexer, &woken);
        assert_eq!(third.root(), other.0);
        assert_eq!(rels(&third), ["Elsewhere.md"]);

        indexer.set_root(None);
        indexer.refresh();
        assert!(woken.recv_timeout(Duration::from_millis(100)).is_err());
        assert!(indexer.take().is_none());
    }

    /// `cargo test --release -p mdshared-app notes::tests::two_thousand -- --nocapture` prints
    /// the times.
    #[test]
    fn two_thousand_notes_are_indexed_well_under_a_second() {
        let d = Dir::new("many");
        for i in 0..2000 {
            let mut text = format!("# Note {i}\n\nSome words about note {i}.\n\n");
            for k in 1..=5 {
                text.push_str(&format!(
                    "- [[note-{}]] and [md](../f{}/note-{}.md)\n",
                    (i + k * 7) % 2000,
                    (i + k) % 20,
                    (i + k) % 2000
                ));
            }
            text.push_str("\n```\n[[note-0]]\n```\n");
            d.write(&format!("f{}/note-{i}.md", i % 20), &text);
        }
        let started = Instant::now();
        let index = d.index();
        let built = started.elapsed();
        let stats = index.stats();
        assert_eq!((stats.files, stats.read_files), (2000, 2000));
        assert_eq!(stats.links, 20_000);

        let started = Instant::now();
        let again = NoteIndex::build(&d.0, &Config::default(), Some(&index), &|| false).unwrap();
        let refreshed = started.elapsed();
        assert_eq!(again.stats().reused_files, 2000);

        let target = d.0.join("f0/note-0.md");
        let started = Instant::now();
        let links = index.backlinks(&target, usize::MAX);
        let queried = started.elapsed();
        // Five wiki-links and five Markdown links name each note.
        assert_eq!(links.len(), 10);
        println!(
            "2000 notes, {} bytes, {} links: build {built:?}, unchanged rebuild {refreshed:?}, backlinks of one note {queried:?}",
            stats.scanned_bytes, stats.links
        );
        if !cfg!(debug_assertions) {
            assert!(built < Duration::from_secs(1), "{built:?}");
        }
    }
}
