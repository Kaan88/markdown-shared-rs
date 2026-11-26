//! A folder's entries as a file manager's list keeps them: the name, kind, size, times and
//! hidden flag of each entry, sorted by any column with the folders first, narrowed by a text
//! filter and a show-hidden switch, and kept in step with the disk change by change.
//!
//! The model and the read are separate, because reading a folder is the slow part and a folder
//! that is slow to read (a network drive, a drive that went away) must never hold up the window:
//!
//! - [`read`] reads a folder on the calling thread and can be stopped between two entries;
//! - [`FolderRead`] runs [`read`] on a worker thread ([`Job`](crate::job::Job)) and hands the
//!   entries over once, on the thread that owns the list;
//! - [`Listing`] holds what was read and answers the questions a list asks (how many rows, the
//!   entry of a row, the row of an entry) without touching the disk;
//! - [`Entry::at`] reads one path's details, for a change the watcher reports by path.
//!
//! The application owns the events and the selection. Every method of [`Listing`] that changes
//! the rows returns a [`Remap`], which says where each old row went; the application hands it to
//! `mdshared_ui::select::Selection::remap`, and the selection follows its entries through a new
//! sort, a filter, and a create, remove, rename or change:
//!
//! ```text
//! let remap = listing.toggle_sort(Column::Size);
//! selection.remap(remap.mapping());
//! ```
//!
//! How the list is kept in step with the disk: the application's watcher (roadmap F2) says what
//! changed, the application reads the new details with [`Entry::at`] where it needs them, and
//! calls [`Listing::apply`] with one [`Change`]. Only that entry moves, nothing is read again;
//! [`Listing::set_entries`] with a fresh [`read`] remains the fallback for a folder that changed
//! in a way the watcher did not report.
//!
//! Deliberately not here:
//!
//! - no drawing: the rows are the application's to draw, with its own columns and fonts;
//! - no watcher: F2 reports what changed, this module only applies it;
//! - no file operations (F3), no volumes (F4), no icons or thumbnails (F5);
//! - no recursive folder size: a folder's size is what the system reports for it, usually zero,
//!   and this module never walks a folder to add up its contents;
//! - no clock and no settings line: the sort, the filter and the switches are the caller's state
//!   and it saves them the way it saves everything else.

use crate::fmt::natural_cmp;
use crate::job::Job;
use crate::paths::{NAMES_IGNORE_CASE, strip_verbatim};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The `FILE_ATTRIBUTE_HIDDEN` bit of a Windows entry's attributes.
#[cfg(windows)]
const HIDDEN_ATTRIBUTE: u32 = 0x2;

/// What an entry of a folder is, as far as a list needs to know.
///
/// The order of the variants is the order [the kind column](Column::Kind) sorts by: folders, then
/// files, then everything else. Folders come first in every sort anyway (see [`Listing`]).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Kind {
    /// A folder. Asking a system for its details gives no contents: its size is not added up.
    Folder,
    /// A plain file.
    File,
    /// Neither a file nor a folder: a device, a socket, a pipe, or a link whose target is gone.
    Other,
}

/// One entry of a folder, with everything a list shows of it.
///
/// The fields are public so that an application, or a test, can hold an entry read earlier and
/// hand it back to [`Listing::apply`]; the module itself builds them with [`Entry::at`] and
/// [`read`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The full path, as the folder named it.
    pub path: PathBuf,
    /// The last part of the path, as a string. A name the system holds in bytes that are not
    /// UTF-8 is kept with the replacement character, so it can be shown but may not open again.
    pub name: String,
    /// A folder, a file, or something else.
    pub kind: Kind,
    /// The size in bytes as the system reports it. A folder is usually zero here: its contents
    /// are never added up, which would read the whole tree.
    pub size: u64,
    /// When the entry last changed, where the system can say. `None` where it cannot.
    pub modified: Option<SystemTime>,
    /// When the entry was created, where the system can say (some Linux file systems cannot).
    /// `None` where it cannot.
    pub created: Option<SystemTime>,
    /// Whether the system hides it: the hidden attribute on Windows, a leading dot elsewhere.
    pub hidden: bool,
}

impl Entry {
    /// The entry for `path`, or `None` when it cannot be read: it does not exist, its folder
    /// cannot be opened, or the system refuses its details.
    ///
    /// This is how an application turns a path that a watcher reports into the entry to hand to
    /// [`Listing::apply`]. It reads the disk, so it belongs off the thread that draws; a single
    /// entry's details are quick, but a dead network path answers slowly.
    pub fn at(path: impl AsRef<Path>) -> Option<Entry> {
        let path = path.as_ref();
        let meta = fs::metadata(path).ok()?;
        Some(entry_of(path, &meta))
    }
}

/// Builds an entry from a path and the details the system gave for it.
fn entry_of(path: &Path, meta: &Metadata) -> Entry {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let kind = if meta.is_dir() {
        Kind::Folder
    } else if meta.is_file() {
        Kind::File
    } else {
        Kind::Other
    };
    let hidden = is_hidden(&name, meta);
    Entry {
        path: path.to_path_buf(),
        name,
        kind,
        size: meta.len(),
        modified: meta.modified().ok(),
        created: meta.created().ok(),
        hidden,
    }
}

/// Whether the system hides an entry: the hidden attribute on Windows, a leading dot elsewhere.
#[cfg(windows)]
fn is_hidden(name: &str, meta: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    let _ = name;
    meta.file_attributes() & HIDDEN_ATTRIBUTE != 0
}

/// Whether the system hides an entry: the hidden attribute on Windows, a leading dot elsewhere.
#[cfg(not(windows))]
fn is_hidden(name: &str, meta: &Metadata) -> bool {
    let _ = meta;
    name.starts_with('.')
}

/// The entries of `dir` in the folder's own order, as far as `cancel` lets the read go.
///
/// `cancel` is asked before each entry; once it says true the read stops and what it has read so
/// far is returned. It cannot interrupt a single system call, so a `read_dir` that hangs on the
/// network keeps that thread there; the window is not held up because the read runs on a worker
/// (see [`FolderRead`]).
///
/// An entry that cannot be read (it was deleted between being listed and its details being asked
/// for, its name is not one the system will open, a folder's contents may not be listed) is
/// skipped, so the answer is always usable. A folder that cannot be opened gives nothing.
pub fn read(dir: &Path, cancel: &dyn Fn() -> bool) -> Vec<Entry> {
    let Ok(listing) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for entry in listing {
        if cancel() {
            break;
        }
        // An entry the system could not hand over at all is skipped, like an unreadable one.
        let Ok(entry) = entry else {
            continue;
        };
        if let Some(entry) = Entry::at(entry.path()) {
            entries.push(entry);
        }
    }
    entries
}

/// A folder read on a worker thread, whose entries the window takes when they are ready.
///
/// [`FolderRead::start`] makes the thread; the wake it is given is called once when the work
/// ends, so the window can ask [`FolderRead::take`] then. Nothing blocks the caller, and the
/// read is abandoned with [`FolderRead::cancel`] or by dropping this handle, without waiting for
/// the thread: a cancelled read delivers nothing, even when its thread had already read
/// something.
pub struct FolderRead {
    dir: PathBuf,
    job: Job<Vec<Entry>>,
}

impl FolderRead {
    /// Starts reading `dir` on a worker thread. `wake` is called from that thread once the work
    /// has ended; it must be cheap, must not block on anything the caller holds, and must not
    /// panic, as a [`Job`](crate::job::Job)'s wake.
    ///
    /// The error is the thread's, when one cannot be made: nothing was read then.
    pub fn start(
        dir: impl Into<PathBuf>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> io::Result<FolderRead> {
        let dir = dir.into();
        let for_work = dir.clone();
        let job = Job::start(wake, move |cancel, _| {
            read(&for_work, &|| cancel.cancelled())
        })?;
        Ok(FolderRead { dir, job })
    }

    /// The folder being read.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Whether the read is still going. False once the work has ended (or was cancelled and
    /// ended), so a caller that sees false has had its last wake.
    pub fn running(&self) -> bool {
        self.job.running()
    }

    /// Asks the read to stop. It returns at once and the read stops between two entries; the
    /// entries it had read are thrown away, so [`FolderRead::take`] gives `None`.
    pub fn cancel(&self) {
        self.job.cancel();
    }

    /// The folder and its entries, once the read has ended: `None` while it still runs, `None`
    /// again after they were taken, and `None` for a cancelled read, whose entries are dropped
    /// here rather than handed over half read.
    pub fn take(&mut self) -> Option<(PathBuf, Vec<Entry>)> {
        if self.job.cancelled() {
            self.job.take();
            return None;
        }
        self.job.take().map(|entries| (self.dir.clone(), entries))
    }
}

impl Drop for FolderRead {
    fn drop(&mut self) {
        self.job.cancel();
    }
}

/// The column a list is sorted by.
///
/// Folders come first whatever the column and the direction; among the rest, a column's own order
/// decides, and entries whose keys are equal keep the order of [`Listing::all`]. Times sort with
/// "unknown" (`None`) before any time.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Column {
    /// The name, in [`natural_cmp`]'s order, so `"file2"` comes before `"file10"`.
    Name,
    /// The [`Kind`], in its own order: folders, then files, then others.
    Kind,
    /// The size in bytes.
    Size,
    /// The time last changed.
    Modified,
    /// The time created.
    Created,
}

/// How a list is sorted: by one column, upwards or downwards.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sort {
    /// The column whose values are compared.
    pub column: Column,
    /// Largest (or last) first. Folders stay first either way.
    pub descending: bool,
}

impl Sort {
    /// The sort a new list has: by name, ascending.
    pub const DEFAULT: Sort = Sort {
        column: Column::Name,
        descending: false,
    };
}

/// One change on the disk, as the application's watcher (roadmap F2) reports it.
///
/// Each variant carries what the list needs to change one entry in place; nothing is re-read.
/// A [`Change::Modified`] for a path the list does not hold inserts it, because a save the
/// watcher reported as a change may be a file it never reported being created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// An entry appeared. An entry already listed under the same path takes its place.
    Created(Entry),
    /// An entry is gone. Nothing happens when the path was not listed.
    Removed(PathBuf),
    /// An entry moved or changed its name; `to` is the entry read at its new path.
    Renamed {
        /// The path the entry had in the list.
        from: PathBuf,
        /// The entry as it is now.
        to: Entry,
    },
    /// An entry's details changed. An entry not listed under the path is inserted.
    Modified(Entry),
}

/// Where the rows of a list went after it changed: for each old row, its new one, or `None` when
/// the entry is gone or was filtered out.
///
/// This is what an application hands to the toolkit's `select::Selection::remap` (as
/// [`Remap::mapping`]) so that a selection follows its entries through a new sort, a filter, or a
/// change on the disk.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Remap {
    new_of: Vec<Option<usize>>,
}

impl Remap {
    /// The new row of the old row `old`, or `None` when that entry is not shown any more.
    pub fn get(&self, old: usize) -> Option<usize> {
        self.new_of.get(old).copied().flatten()
    }

    /// How many old rows this remap speaks for: the length of the list before it changed.
    pub fn len(&self) -> usize {
        self.new_of.len()
    }

    /// Whether there were no rows before the change.
    pub fn is_empty(&self) -> bool {
        self.new_of.is_empty()
    }

    /// Whether every old row is where it was, and none was lost.
    pub fn unchanged(&self) -> bool {
        self.new_of
            .iter()
            .enumerate()
            .all(|(old, new)| *new == Some(old))
    }

    /// The remap as the function `Selection::remap` takes, for `selection.remap(remap.mapping())`.
    pub fn mapping(&self) -> impl Fn(usize) -> Option<usize> + '_ {
        |old| self.get(old)
    }
}

/// The entries of one folder, filtered and sorted: what a file manager's list shows.
///
/// The list holds every entry read ([`all`](Self::all), in the folder's own order) and the rows
/// it shows ([`visible`](Self::visible), filtered and sorted). Every method that changes the rows
/// returns a [`Remap`] that says where each old row went.
///
/// A new list is sorted by name, ascending; folders come first; hidden entries are left out; and
/// the filter is empty.
///
/// Nothing here reads the disk or the clock. What it holds is given to it: the entries from
/// [`read`] or [`FolderRead`], and the one entry of a [`Change`] the application read with
/// [`Entry::at`].
#[derive(Clone, Debug, PartialEq)]
pub struct Listing {
    folder: Option<PathBuf>,
    /// Every entry read, in the order the folder gave them. The view indexes into this.
    entries: Vec<Entry>,
    /// The rows shown, as indices into `entries`, filtered and sorted.
    view: Vec<usize>,
    filter: String,
    show_hidden: bool,
    sort: Sort,
}

impl Listing {
    /// An empty list, by name, ascending, hidden entries left out.
    pub fn new() -> Listing {
        Listing {
            folder: None,
            entries: Vec::new(),
            view: Vec::new(),
            filter: String::new(),
            show_hidden: false,
            sort: Sort::DEFAULT,
        }
    }

    /// The folder the entries were read from, if the caller has said.
    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    /// Names the folder the list belongs to and forgets every entry: for a list that shows
    /// another folder now. Its filter, its sort and its switches stay.
    pub fn set_folder(&mut self, folder: Option<PathBuf>) -> Remap {
        let old = self.view_paths();
        self.folder = folder;
        self.entries.clear();
        self.rebuild();
        self.remap_from(&old)
    }

    /// Replaces every entry with a fresh reading of the folder, keeping the filter and the sort.
    pub fn set_entries(&mut self, entries: Vec<Entry>) -> Remap {
        let old = self.view_paths();
        self.entries = entries;
        self.rebuild();
        self.remap_from(&old)
    }

    /// Applies one change to the entries, keeping everything else: only the entry the change
    /// names is added, removed, replaced or moved, and nothing is read from the disk.
    ///
    /// A rename's remap follows the entry to its new path, so a selection stays on it: the old
    /// row of `from` maps to the new row of `to`, though the two paths differ.
    pub fn apply(&mut self, change: Change) -> Remap {
        let old = self.view_paths();
        let mut renamed = None;
        match change {
            Change::Created(entry) | Change::Modified(entry) => self.put(entry),
            Change::Removed(path) => {
                let gone = path_key(&path);
                self.entries.retain(|entry| path_key(&entry.path) != gone);
            }
            Change::Renamed { from, to } => {
                let gone = path_key(&from);
                let moved = path_key(&to.path);
                self.entries.retain(|entry| path_key(&entry.path) != gone);
                self.put(to);
                renamed = Some((gone, moved));
            }
        }
        self.rebuild();
        let mut remap = self.remap_from(&old);
        if let Some((gone, moved)) = renamed {
            // The old path is not in the list any more, so the path matching would lose the
            // row: it is where the entry went under its new name.
            if let Some(old_row) = old.iter().position(|path| path_key(path) == gone) {
                remap.new_of[old_row] = self.row_of_key(&moved);
            }
        }
        remap
    }

    /// How many rows are shown.
    pub fn len(&self) -> usize {
        self.view.len()
    }

    /// Whether no row is shown (an empty folder, or every entry filtered out).
    pub fn is_empty(&self) -> bool {
        self.view.is_empty()
    }

    /// The entry of shown row `row`, counting from the top; `None` past the last row.
    pub fn entry(&self, row: usize) -> Option<&Entry> {
        self.view.get(row).map(|&i| &self.entries[i])
    }

    /// The path of shown row `row`; `None` past the last row.
    pub fn path_at(&self, row: usize) -> Option<&Path> {
        self.entry(row).map(|entry| entry.path.as_path())
    }

    /// The shown row of the entry at `path`, or `None` when no row shows it (not read, hidden,
    /// or filtered out). Paths are compared as the system compares names: without case where
    /// the system ignores it.
    pub fn row_of(&self, path: &Path) -> Option<usize> {
        self.row_of_key(&path_key(path))
    }

    /// The shown row of the entry whose path key is `wanted`.
    fn row_of_key(&self, wanted: &str) -> Option<usize> {
        self.view
            .iter()
            .position(|&i| path_key(&self.entries[i].path) == wanted)
    }

    /// The shown entries, top to bottom: filter and sort applied.
    pub fn visible(&self) -> impl Iterator<Item = &Entry> + '_ {
        self.view.iter().map(|&i| &self.entries[i])
    }

    /// Every entry read, in the folder's own order, before the filter and the sort.
    pub fn all(&self) -> &[Entry] {
        &self.entries
    }

    /// The text the filter holds; empty when there is none.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Shows only the entries whose name contains `filter`, ignoring case. An empty filter
    /// shows every entry the hidden switch allows.
    pub fn set_filter(&mut self, filter: &str) -> Remap {
        let old = self.view_paths();
        self.filter = filter.to_string();
        self.rebuild();
        self.remap_from(&old)
    }

    /// Whether hidden entries are shown.
    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    /// Shows or hides the entries the system hides (see [`Entry::hidden`]).
    pub fn set_show_hidden(&mut self, show: bool) -> Remap {
        let old = self.view_paths();
        self.show_hidden = show;
        self.rebuild();
        self.remap_from(&old)
    }

    /// The current sort.
    pub fn sort(&self) -> Sort {
        self.sort
    }

    /// Sorts by `sort`, folders first.
    pub fn set_sort(&mut self, sort: Sort) -> Remap {
        let old = self.view_paths();
        self.sort = sort;
        self.rebuild();
        self.remap_from(&old)
    }

    /// A click on a column's title: sorted by another column, the list is now sorted by this one,
    /// ascending; sorted by this one, the direction turns round. Folders stay first.
    pub fn toggle_sort(&mut self, column: Column) -> Remap {
        let descending = self.sort.column == column && !self.sort.descending;
        self.set_sort(Sort { column, descending })
    }

    /// Puts an entry in, replacing the entry with the same path when there is one.
    fn put(&mut self, entry: Entry) {
        let wanted = path_key(&entry.path);
        match self
            .entries
            .iter_mut()
            .find(|held| path_key(&held.path) == wanted)
        {
            Some(held) => *held = entry,
            None => self.entries.push(entry),
        }
    }

    /// Builds the view from the entries: the filter, the hidden switch and the sort.
    fn rebuild(&mut self) {
        let needle = self.filter.to_lowercase();
        let mut view = Vec::with_capacity(self.entries.len());
        for (i, entry) in self.entries.iter().enumerate() {
            if !self.show_hidden && entry.hidden {
                continue;
            }
            if !needle.is_empty() && !entry.name.to_lowercase().contains(&needle) {
                continue;
            }
            view.push(i);
        }
        let sort = self.sort;
        view.sort_by(|&a, &b| compare(&self.entries[a], &self.entries[b], sort));
        self.view = view;
    }

    /// The paths of the shown rows, in order: what the next view is matched against.
    fn view_paths(&self) -> Vec<PathBuf> {
        self.view
            .iter()
            .map(|&i| self.entries[i].path.clone())
            .collect()
    }

    /// Where each of `old`'s rows is in the view as it is now.
    fn remap_from(&self, old: &[PathBuf]) -> Remap {
        if old.is_empty() {
            return Remap::default();
        }
        let mut at = HashMap::with_capacity(self.view.len());
        for (row, &i) in self.view.iter().enumerate() {
            at.insert(path_key(&self.entries[i].path), row);
        }
        Remap {
            new_of: old
                .iter()
                .map(|path| at.get(&path_key(path)).copied())
                .collect(),
        }
    }
}

impl Default for Listing {
    fn default() -> Listing {
        Listing::new()
    }
}

/// Compares two entries under `sort`: folders first, then the column, then the direction.
///
/// The comparison is a stable sort's key: two entries that come out equal keep the order of
/// [`Listing::all`], which is the order the folder gave them.
fn compare(a: &Entry, b: &Entry, sort: Sort) -> Ordering {
    match (a.kind == Kind::Folder, b.kind == Kind::Folder) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }
    let by_column = match sort.column {
        Column::Name => natural_cmp(&a.name, &b.name),
        Column::Kind => a.kind.cmp(&b.kind),
        Column::Size => a.size.cmp(&b.size),
        Column::Modified => a.modified.cmp(&b.modified),
        Column::Created => a.created.cmp(&b.created),
    };
    if sort.descending {
        by_column.reverse()
    } else {
        by_column
    }
}

/// The key two spellings of one path share: no verbatim prefix, no trailing separator, `\` on
/// Windows, and without case where the system ignores it.
fn path_key(path: &Path) -> String {
    let text = path.to_string_lossy();
    let mut key = strip_verbatim(&text).unwrap_or_else(|| text.into_owned());
    if cfg!(windows) {
        key = key.replace('/', "\\");
    }
    while key.len() > 1 && (key.ends_with('\\') || key.ends_with('/')) && !key.ends_with(":\\") {
        key.pop();
    }
    if NAMES_IGNORE_CASE {
        key.to_lowercase()
    } else {
        key
    }
}

#[cfg(test)]
#[path = "dirlist_tests.rs"]
mod tests;
