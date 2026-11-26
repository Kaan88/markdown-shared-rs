//! A folder's entries as a file manager's list keeps them: the name, kind, size, times and
//! hidden flag of each entry, sorted by any column with the folders first, narrowed by a text
//! filter and a show-hidden switch, and kept in step with the disk change by change.
//!
//! The model and the read are separate, because reading a folder is the slow part and a folder
//! that is slow to read (a network drive, a drive that went away) must never hold up the window:
//!
//! - [`read`] reads a folder on the calling thread and can be stopped between two entries;
//!   [`read_checked`] is the same read with the folder's own error kept;
//! - [`FolderRead`] runs the read on a worker thread ([`Job`](crate::job::Job)) and hands the
//!   entries over once, on the thread that owns the list, with the error kept too;
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
//! calls [`Listing::apply`] with one [`Change`]. Only that entry moves, by binary search in the
//! sorted rows, and nothing is read again; [`Listing::set_entries`] with a fresh [`read`]
//! remains the fallback for a folder that changed in a way the watcher did not report.
//!
//! Paths are compared exactly, as the platform spelled them: no case folding and no tidying of
//! separators. Two names that differ only in case are two entries where the file system says
//! so. That puts one rule on the watcher that feeds [`Listing::apply`]: its paths must be
//! spelled the way the folder spelled them, with no `\\?\` prefix and no 8.3 short name (on
//! Windows both can name an entry the read returned under another spelling), and on macOS
//! `/var` and `/private/var` are two paths, not one. When the list has a
//! [folder](Listing::folder), a change is applied only when the path it names sits directly in
//! that folder: a rename out of the folder is a removal, a rename into it a creation, and the
//! folder itself is not one of its entries.
//!
//! Links are never followed. An entry that is a symbolic link or a Windows junction is
//! [`Kind::Other`], so a link to a dead share or a loop cannot stall the read of the folder or
//! send it anywhere else; the system reports a junction as a link too, and what it points at is
//! the follow that would cost the read. A link to a folder is therefore not a [`Kind::Folder`]
//! here, and an application that opens one must follow the link itself, off the thread that
//! draws, as it would follow any slow path. The hidden flag and the times are the link's own,
//! not its target's.
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
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs::{self, FileType, Metadata};
use std::hash::{Hash, Hasher};
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
    /// Neither a file nor a folder, and nothing this module follows: a symbolic link (to a
    /// file, a folder, a dead path or itself), a Windows junction, a device, a socket, a pipe.
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
    /// are never added up, which would read the whole tree. A link's size is the link's own.
    pub size: u64,
    /// When the entry last changed, where the system can say. `None` where it cannot. A link's
    /// own time, never its target's.
    pub modified: Option<SystemTime>,
    /// When the entry was created, where the system can say (some Linux file systems cannot).
    /// `None` where it cannot.
    pub created: Option<SystemTime>,
    /// Whether the system hides it: the hidden attribute on Windows (which a hidden junction
    /// such as `C:\Documents and Settings` carries), a leading dot elsewhere. The link's own
    /// flag, never its target's.
    pub hidden: bool,
}

impl Entry {
    /// The entry for `path`, or `None` when it cannot be read: it does not exist, its folder
    /// cannot be opened, or the system refuses its details.
    ///
    /// A link is not followed: the details are the link's own (`symlink_metadata`), so a dead
    /// link still has an entry and a link to a folder is [`Kind::Other`].
    ///
    /// This is how an application turns a path that a watcher reports into the entry to hand to
    /// [`Listing::apply`]. It reads the disk, so it belongs off the thread that draws; a single
    /// entry's details are quick, but a dead network path answers slowly.
    pub fn at(path: impl AsRef<Path>) -> Option<Entry> {
        Self::at_checked(path.as_ref()).ok()
    }

    /// Like `at`, without losing the reason a metadata read failed.
    pub(crate) fn at_checked(path: &Path) -> io::Result<Entry> {
        let meta = fs::symlink_metadata(path)?;
        let name = name_of(path);
        Ok(entry_of(path.to_path_buf(), name, &meta.file_type(), &meta))
    }
}

/// The last part of a path as a name, or the whole path when it has no last part (a root).
fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Builds an entry from a path and the details the system gave for the entry itself.
///
/// `file_type` is what the folder scan gave, so a link is known to be a link without opening
/// anything, and it is [`Kind::Other`]: the scan calls a Windows junction a link as well, and
/// asking what a link points at is the follow this module refuses. Nothing here follows a link.
fn entry_of(path: PathBuf, name: String, file_type: &FileType, meta: &Metadata) -> Entry {
    let kind = if file_type.is_symlink() {
        Kind::Other
    } else if file_type.is_dir() {
        Kind::Folder
    } else if file_type.is_file() {
        Kind::File
    } else {
        Kind::Other
    };
    let hidden = is_hidden(&name, meta);
    Entry {
        path,
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

/// One entry of a folder, from the scan's own record of it, or `None` when the system will not
/// give its details (it was deleted between being listed and being asked for, or its name is not
/// one the system will open).
///
/// Nothing here follows a link: [`DirEntry::metadata`](fs::DirEntry::metadata) reports the link
/// itself, which on Windows costs no extra read because the folder scan already carried it.
fn read_entry(item: &fs::DirEntry) -> Option<Entry> {
    let file_type = item.file_type().ok()?;
    let meta = item.metadata().ok()?;
    Some(entry_of(
        item.path(),
        item.file_name().to_string_lossy().into_owned(),
        &file_type,
        &meta,
    ))
}

/// The entries of `dir` in the folder's own order, as far as `cancel` lets the read go.
///
/// `cancel` is asked before each entry; once it says true the read stops and what it has read so
/// far is returned. It cannot interrupt a single system call, so a `read_dir` that hangs on the
/// network keeps that thread there; the window is not held up because the read runs on a worker
/// (see [`FolderRead`]).
///
/// An entry that cannot be read (it was deleted between being listed and its details being asked
/// for, its name is not one the system will open) is skipped, so the answer is always usable. A
/// folder that cannot be opened gives nothing; [`read_checked`] keeps the error instead.
pub fn read(dir: &Path, cancel: &dyn Fn() -> bool) -> Vec<Entry> {
    read_checked(dir, cancel).unwrap_or_default()
}

/// [`read`], with the folder's own error kept: a folder that cannot be opened (it is gone, it is
/// not a folder, it may not be listed) is an `Err`, so the application can tell it from a folder
/// that is simply empty.
///
/// An entry that cannot be read is still skipped rather than failing the whole read, and a
/// cancel still ends the read and returns what it had.
pub fn read_checked(dir: &Path, cancel: &dyn Fn() -> bool) -> io::Result<Vec<Entry>> {
    read_with(dir, cancel, &read_entry)
}

/// [`read_checked`], with the step that turns one scanned item into an entry given by the
/// caller: the real one is [`read_entry`], and a test gives one that reads nothing to show that
/// the read goes on past an entry the system would not describe.
fn read_with(
    dir: &Path,
    cancel: &dyn Fn() -> bool,
    entry_of_item: &dyn Fn(&fs::DirEntry) -> Option<Entry>,
) -> io::Result<Vec<Entry>> {
    let listing = fs::read_dir(dir)?;
    let mut entries = Vec::new();
    for item in listing {
        if cancel() {
            break;
        }
        // An entry the system could not hand over at all is skipped, like an unreadable one.
        let Ok(item) = item else {
            continue;
        };
        if let Some(entry) = entry_of_item(&item) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

/// A folder read on a worker thread, whose entries the window takes when they are ready.
///
/// [`FolderRead::start`] makes the thread; the wake it is given is called once when the work
/// ends, so the window can ask [`FolderRead::take`] or [`FolderRead::take_checked`] then.
/// Nothing blocks the caller, and the read is abandoned with [`FolderRead::cancel`] or by
/// dropping this handle, without waiting for the thread: a cancelled read delivers nothing, even
/// when its thread had already read something.
pub struct FolderRead {
    dir: PathBuf,
    job: Job<io::Result<Vec<Entry>>>,
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
            read_checked(&for_work, &|| cancel.cancelled())
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
    /// entries it had read are thrown away, so neither take gives anything.
    pub fn cancel(&self) {
        self.job.cancel();
    }

    /// The folder and its entries, once the read has ended: `None` while it still runs, `None`
    /// again after they were taken, and `None` for a cancelled read, whose entries are dropped
    /// here rather than handed over half read. A folder that could not be opened gives an empty
    /// list here, as [`read`] does; use [`take_checked`](Self::take_checked) to see the error.
    ///
    /// A read is handed over once: whichever of the two takes is called first takes it.
    pub fn take(&mut self) -> Option<(PathBuf, Vec<Entry>)> {
        if self.job.cancelled() {
            self.job.take();
            return None;
        }
        let entries = self.job.take()?.unwrap_or_default();
        Some((self.dir.clone(), entries))
    }

    /// [`take`](Self::take), with the folder's own error kept: an `Err` says the folder could
    /// not be opened, so the application can tell it from a folder that is empty.
    pub fn take_checked(&mut self) -> Option<(PathBuf, io::Result<Vec<Entry>>)> {
        if self.job.cancelled() {
            self.job.take();
            return None;
        }
        Some((self.dir.clone(), self.job.take()?))
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
/// decides, then the name in [`natural_cmp`]'s order, then the path's bytes. The last two make
/// the order total, so two runs of the same entries always come out the same way. Times sort
/// with "unknown" (`None`) before any time.
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
///
/// With a [folder](Listing::folder), only a path directly inside it is applied: a rename out of
/// the folder is a [`Removed`](Change::Removed), a rename into it a
/// [`Created`](Change::Created), and anything else outside is ignored.
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

mod remap;

use remap::Edit;
pub use remap::Remap;

/// An index from a path to an entry's place in a [`Listing`], so one change finds its entry
/// without reading the list.
///
/// Keyed by a hash of the path, so no path is copied; a bucket holds the few places whose paths
/// hash alike and the paths themselves decide.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Index {
    buckets: HashMap<u64, Vec<usize>>,
}

impl Index {
    /// The index of every entry, by path.
    fn of(entries: &[Entry]) -> Index {
        let mut index = Index::default();
        for (i, entry) in entries.iter().enumerate() {
            index.add(&entry.path, i);
        }
        index
    }

    fn hash(path: &Path) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut hasher);
        hasher.finish()
    }

    /// The place of the entry at exactly `path`.
    fn find(&self, entries: &[Entry], path: &Path) -> Option<usize> {
        self.buckets
            .get(&Self::hash(path))?
            .iter()
            .copied()
            .find(|&i| entries[i].path == path)
    }

    fn add(&mut self, path: &Path, index: usize) {
        self.buckets
            .entry(Self::hash(path))
            .or_default()
            .push(index);
    }

    fn remove(&mut self, path: &Path, index: usize) {
        let hash = Self::hash(path);
        if let Some(bucket) = self.buckets.get_mut(&hash) {
            bucket.retain(|&i| i != index);
            if bucket.is_empty() {
                self.buckets.remove(&hash);
            }
        }
    }
}

/// The entries of one folder, filtered and sorted: what a file manager's list shows.
///
/// The list holds every entry read ([`all`](Self::all), in the order the folder gave them) and
/// the rows it shows ([`visible`](Self::visible), filtered and sorted). Every method that changes
/// the rows returns a [`Remap`] that says where each old row went.
///
/// A new list is sorted by name, ascending; folders come first; hidden entries are left out; and
/// the filter is empty.
///
/// Nothing here reads the disk or the clock. What it holds is given to it: the entries from
/// [`read`] or [`FolderRead`], and the one entry of a [`Change`] the application read with
/// [`Entry::at`].
///
/// Each entry's lower-cased name is made once, when the entry comes in, and is the key the
/// filter and the name column compare; a change finds, removes and re-inserts its entry by
/// binary search in the sorted rows instead of sorting them again.
#[derive(Clone, Debug, PartialEq)]
pub struct Listing {
    folder: Option<PathBuf>,
    /// Every entry, in the order the folder gave them; a removal puts the last entry in the
    /// place of the one that went, so the order is the folder's only until something changes.
    entries: Vec<Entry>,
    /// The name of each entry in lower case, parallel to `entries`: the filter's key and the
    /// name sort's key.
    lower: Vec<String>,
    index: Index,
    /// Every entry, sorted: the hidden switch and the filter only choose among these rows, so
    /// neither of them sorts anything.
    order: Vec<usize>,
    /// The rows shown, as places in `entries`: a subsequence of `order`.
    view: Vec<usize>,
    filter: String,
    /// `filter` in lower case, which is what the entries' names are compared against.
    needle: String,
    show_hidden: bool,
    sort: Sort,
}

impl Listing {
    /// An empty list, by name, ascending, hidden entries left out.
    pub fn new() -> Listing {
        Listing {
            folder: None,
            entries: Vec::new(),
            lower: Vec::new(),
            index: Index::default(),
            order: Vec::new(),
            view: Vec::new(),
            filter: String::new(),
            needle: String::new(),
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
        let old = std::mem::take(&mut self.view);
        self.folder = folder;
        self.entries.clear();
        self.lower.clear();
        self.index = Index::default();
        self.order.clear();
        Remap::exact(vec![None; old.len()])
    }

    /// Replaces every entry with a fresh reading of the folder, keeping the filter and the sort.
    pub fn set_entries(&mut self, entries: Vec<Entry>) -> Remap {
        let old = std::mem::take(&mut self.view);
        let lower: Vec<String> = entries.iter().map(|e| e.name.to_lowercase()).collect();
        let order = self.order_of(&entries, &lower);
        let view: Vec<usize> = order
            .iter()
            .copied()
            .filter(|&i| self.shows_entry(&entries[i], &lower[i]))
            .collect();
        // The rows of the new list are matched to the old ones by path, borrowed both ways, so
        // no path is copied for the remap.
        let mut at: HashMap<&Path, usize> = HashMap::with_capacity(view.len());
        for (row, &i) in view.iter().enumerate() {
            at.insert(entries[i].path.as_path(), row);
        }
        let new_of = old
            .iter()
            .map(|&i| at.get(self.entries[i].path.as_path()).copied())
            .collect();
        self.index = Index::of(&entries);
        self.entries = entries;
        self.lower = lower;
        self.order = order;
        self.view = view;
        Remap::exact(new_of)
    }

    /// Applies one change to the entries, keeping everything else: only the entry the change
    /// names is added, removed, replaced or moved, by binary search in the rows, and nothing is
    /// read from the disk.
    ///
    /// With a [folder](Self::folder), a change whose path is not directly inside it is ignored,
    /// except that a rename out of the folder removes its entry and a rename into it creates
    /// one. A rename's remap follows the entry to its new path, so a selection stays on it: the
    /// old row of `from` maps to the new row of `to`, though the two paths differ.
    ///
    /// Paths are matched exactly, as the folder spelled them, so the watcher must hand over its
    /// own spelling: no `\\?\` prefix, no 8.3 short name, and on macOS `/var` and
    /// `/private/var` are two paths. A path spelled otherwise is a path the list does not hold,
    /// and the change is applied as a creation or ignored.
    pub fn apply(&mut self, change: Change) -> Remap {
        let rows = self.view.len();
        // The row edits the change makes, in the numbering of the moment each was made, and the
        // few old rows whose answer the edits cannot give.
        let mut edits: Vec<Edit> = Vec::new();
        let mut named: Vec<(usize, Option<usize>)> = Vec::new();
        match change {
            Change::Created(entry) | Change::Modified(entry) => {
                if self.inside(&entry.path) {
                    let (_, replaced) = self.put(entry, &mut edits);
                    if let Some((old, new)) = replaced {
                        named.push((old, Some(new)));
                    }
                }
            }
            Change::Removed(path) => {
                if self.inside(&path)
                    && let Some(i) = self.index.find(&self.entries, &path)
                {
                    self.take_out(i, &mut edits);
                }
            }
            Change::Renamed { from, to } => {
                // The row the entry had before anything moves, for the selection to follow it.
                let from_row = self
                    .inside(&from)
                    .then(|| self.index.find(&self.entries, &from))
                    .flatten()
                    .and_then(|i| self.position_of(i).ok());
                let inside_to = self.inside(&to.path);
                if inside_to && let Some(j) = self.index.find(&self.entries, &to.path) {
                    // A name the list already shows is replaced by the new entry.
                    self.take_out(j, &mut edits);
                }
                if let Some(i) = self.index.find(&self.entries, &from) {
                    self.take_out(i, &mut edits);
                }
                if inside_to {
                    let (new, replaced) = self.put(to, &mut edits);
                    if let Some((old, moved)) = replaced {
                        named.push((old, Some(moved)));
                    }
                    if let Some(row) = from_row {
                        named.push((row, self.position_of(new).ok()));
                    }
                }
            }
        }
        Remap::edited(rows, edits, named)
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
    /// or filtered out). Paths are compared exactly, as the folder spelled them.
    pub fn row_of(&self, path: &Path) -> Option<usize> {
        let i = self.index.find(&self.entries, path)?;
        self.position_of(i).ok()
    }

    /// The shown entries, top to bottom: filter and sort applied.
    pub fn visible(&self) -> impl Iterator<Item = &Entry> + '_ {
        self.view.iter().map(|&i| &self.entries[i])
    }

    /// Every entry read, in the order the folder gave them until a change; a removal moves the
    /// last entry into the place of the one that went.
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
        let old = std::mem::take(&mut self.view);
        self.filter = filter.to_string();
        self.needle = filter.to_lowercase();
        // The rows were sorted already, and a filter keeps their order: the ones that stay are
        // a subsequence of them, so nothing is sorted again for a keystroke.
        self.view = self
            .order
            .iter()
            .copied()
            .filter(|&i| self.shows(i))
            .collect();
        self.remap_entries(&old)
    }

    /// Whether hidden entries are shown.
    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    /// Shows or hides the entries the system hides (see [`Entry::hidden`]).
    pub fn set_show_hidden(&mut self, show: bool) -> Remap {
        let old = std::mem::take(&mut self.view);
        self.show_hidden = show;
        self.view = self
            .order
            .iter()
            .copied()
            .filter(|&i| self.shows(i))
            .collect();
        self.remap_entries(&old)
    }

    /// The current sort.
    pub fn sort(&self) -> Sort {
        self.sort
    }

    /// Sorts by `sort`, folders first.
    pub fn set_sort(&mut self, sort: Sort) -> Remap {
        let old = std::mem::take(&mut self.view);
        self.sort = sort;
        self.order = self.order_of(&self.entries, &self.lower);
        self.view = self
            .order
            .iter()
            .copied()
            .filter(|&i| self.shows(i))
            .collect();
        self.remap_entries(&old)
    }

    /// A click on a column's title: sorted by another column, the list is now sorted by this one,
    /// ascending; sorted by this one, the direction turns round. Folders stay first.
    pub fn toggle_sort(&mut self, column: Column) -> Remap {
        let descending = self.sort.column == column && !self.sort.descending;
        self.set_sort(Sort { column, descending })
    }

    /// Whether the entry at `path` sits directly in the list's folder (or the list has no
    /// folder, which applies everything). The folder itself is not one of its entries.
    fn inside(&self, path: &Path) -> bool {
        match self.folder.as_deref() {
            None => true,
            Some(folder) => path.parent() == Some(folder),
        }
    }

    /// Whether the entry at place `i` passes the filter and the hidden switch.
    fn shows(&self, i: usize) -> bool {
        self.shows_entry(&self.entries[i], &self.lower[i])
    }

    fn shows_entry(&self, entry: &Entry, lower: &str) -> bool {
        (self.show_hidden || !entry.hidden)
            && (self.needle.is_empty() || lower.contains(&self.needle))
    }

    /// Every entry, in the list's sort.
    fn order_of(&self, entries: &[Entry], lower: &[String]) -> Vec<usize> {
        let mut order: Vec<usize> = (0..entries.len()).collect();
        let sort = self.sort;
        order.sort_by(|&a, &b| compare(&entries[a], &lower[a], &entries[b], &lower[b], sort));
        order
    }

    /// The place of entry `i` in the rows, by binary search; `Err` is where it would go.
    fn position_of(&self, i: usize) -> Result<usize, usize> {
        self.view.binary_search_by(|&other| self.order_by(other, i))
    }

    /// The place of entry `i` in the sorted order, by binary search; `Err` is where it would go.
    fn place_in_order(&self, i: usize) -> Result<usize, usize> {
        self.order
            .binary_search_by(|&other| self.order_by(other, i))
    }

    /// The comparison of the entry at place `other` with the entry at place `i`.
    fn order_by(&self, other: usize, i: usize) -> Ordering {
        compare(
            &self.entries[other],
            &self.lower[other],
            &self.entries[i],
            &self.lower[i],
            self.sort,
        )
    }

    /// Takes entry `i` out of the sorted order.
    fn order_remove(&mut self, i: usize) {
        if let Ok(pos) = self.place_in_order(i) {
            self.order.remove(pos);
        }
    }

    /// Puts entry `i` into the sorted order, by binary search.
    fn order_insert(&mut self, i: usize) {
        let at = self.place_in_order(i).unwrap_or_else(|at| at);
        self.order.insert(at, i);
    }

    /// Takes entry `i` out of the rows, if it is in them, and says which row went.
    fn view_remove(&mut self, i: usize) -> Option<usize> {
        let pos = self.position_of(i).ok()?;
        self.view.remove(pos);
        Some(pos)
    }

    /// Puts entry `i` into the rows, by binary search, if it passes the filter and the switches,
    /// and says which row it took.
    fn view_insert(&mut self, i: usize) -> Option<usize> {
        if !self.shows(i) {
            return None;
        }
        let at = self.position_of(i).unwrap_or_else(|at| at);
        self.view.insert(at, i);
        Some(at)
    }

    /// Adds or replaces `entry`, returning its place in `entries`, recording the rows it moved
    /// in `edits`, and, when it replaced an entry that was shown, the row the entry itself went
    /// from and to (its own row is not something the edits can answer: they removed it).
    fn put(&mut self, entry: Entry, edits: &mut Vec<Edit>) -> (usize, Option<(usize, usize)>) {
        let lower = entry.name.to_lowercase();
        if let Some(i) = self.index.find(&self.entries, &entry.path) {
            let old = self.view_remove(i);
            if let Some(pos) = old {
                edits.push(Edit::Removed(pos));
            }
            self.order_remove(i);
            self.entries[i] = entry;
            self.lower[i] = lower;
            self.order_insert(i);
            let new = self.view_insert(i);
            if let Some(at) = new {
                edits.push(Edit::Inserted(at));
            }
            (i, old.zip(new))
        } else {
            self.entries.push(entry);
            self.lower.push(lower);
            let i = self.entries.len() - 1;
            self.index.add(&self.entries[i].path, i);
            self.order_insert(i);
            if let Some(at) = self.view_insert(i) {
                edits.push(Edit::Inserted(at));
            }
            (i, None)
        }
    }

    /// Removes entry `i` from the entries, the keys and the index, and its row from the rows,
    /// recording the row in `edits`.
    fn take_out(&mut self, i: usize, edits: &mut Vec<Edit>) {
        let last = self.entries.len() - 1;
        if let Some(pos) = self.view_remove(i) {
            edits.push(Edit::Removed(pos));
        }
        self.order_remove(i);
        if last != i {
            // The last entry takes this place: point its places in the order and in the rows at
            // its new one before the entries move. Its row number is the one the removal above
            // already moved it to.
            if let Ok(pos) = self.place_in_order(last) {
                self.order[pos] = i;
            }
            if let Ok(pos) = self.position_of(last) {
                self.view[pos] = i;
            }
        }
        let removed = self.entries.swap_remove(i);
        self.lower.swap_remove(i);
        self.index.remove(&removed.path, i);
        if last != i {
            self.index.remove(&self.entries[i].path, last);
            self.index.add(&self.entries[i].path, i);
        }
    }

    /// Where each of `old`'s places is, when the entries did not move (a filter, a switch or a
    /// sort): the rows were rebuilt, and only their places changed.
    fn remap_entries(&self, old: &[usize]) -> Remap {
        let mut row_of = vec![usize::MAX; self.entries.len()];
        for (row, &i) in self.view.iter().enumerate() {
            row_of[i] = row;
        }
        Remap::exact(
            old.iter()
                .map(|&i| row_of.get(i).copied().filter(|&row| row != usize::MAX))
                .collect(),
        )
    }
}

impl Default for Listing {
    fn default() -> Listing {
        Listing::new()
    }
}

/// Compares two entries under `sort`: folders first, then the column, then the name and the
/// path, so that the order is total and two runs of the same entries agree.
///
/// The name column compares the cached lower-case names, which is [`natural_cmp`] on the names
/// themselves; the tiebreak then separates names that differ only in case, and the path settles
/// anything left (none, where every path is unique).
fn compare(a: &Entry, a_lower: &str, b: &Entry, b_lower: &str, sort: Sort) -> Ordering {
    match (a.kind == Kind::Folder, b.kind == Kind::Folder) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }
    let by_column = match sort.column {
        Column::Name => natural_cmp(a_lower, b_lower),
        Column::Kind => a.kind.cmp(&b.kind),
        Column::Size => a.size.cmp(&b.size),
        Column::Modified => a.modified.cmp(&b.modified),
        Column::Created => a.created.cmp(&b.created),
    };
    let by_column = if sort.descending {
        by_column.reverse()
    } else {
        by_column
    };
    by_column
        .then_with(|| natural_cmp(&a.name, &b.name))
        .then_with(|| a.path.cmp(&b.path))
}

#[cfg(test)]
#[path = "dirlist_tests.rs"]
mod tests;
