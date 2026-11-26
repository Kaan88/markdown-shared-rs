//! Watches one folder and says what changed in it, one [`dirlist::Change`] per entry, so a
//! file manager's [`Listing`](crate::dirlist::Listing) can apply the report without reading the
//! folder again.
//!
//! The OS watcher ([`FileWatcher`](crate::watch::FileWatcher)) says only "something in this
//! folder". This module turns a burst of raw events into a short, settled report:
//!
//! - a creation followed by modifications is one [`Change::Created`](crate::dirlist::Change::Created);
//! - a creation followed by a removal is nothing at all (the list never saw it);
//! - a removal followed by a creation of the same path is a
//!   [`Change::Modified`](crate::dirlist::Change::Modified);
//! - a rename chain `a` to `b` to `c` is one rename from `a` to `c`;
//! - a burst of modifications is one [`Change::Modified`](crate::dirlist::Change::Modified).
//!
//! A report is ordered so that it can be applied top to bottom: anything that takes a name away
//! (a rename's old name, or a removal) comes before anything that puts an entry under that name
//! (a creation, a modification, or a rename's new name). A backup on save, which renames `f` to
//! `f~` and then writes a new `f`, is therefore a rename followed by a creation, and log
//! rotation moves the oldest file first. When the pending verdicts cannot be ordered — two
//! renames that swap their names, say — only the renames in the cycle are broken into a removal
//! and a modification, and each name's fate is read from the disk instead; a rename outside the
//! cycle is reported as a rename, so the selection can follow it.
//!
//! The work is split the way the rest of the repository splits a model from its edges:
//!
//! - [`Coalescer`] is the model. It has no clock and no disk: its events arrive with the
//!   [`Instant`] they happened at, and it answers [`Verdict`]s that name paths, so it can be
//!   tested with nothing but numbers and lists;
//! - [`DirWatcher`] is the edge. It watches one folder with `notify`, feeds the coalescer on its
//!   own thread, reads each changed path's details with [`Entry::at`](crate::dirlist::Entry::at),
//!   and wakes the application the way [`FileWatcher`](crate::watch::FileWatcher) does: one
//!   [`wake`](DirWatcher::new) call per settled report, and none again until
//!   [`woken`](DirWatcher::woken) is called.
//!
//! The root the OS watcher is given is worked out once, at start (`std::path::absolute` on
//! Windows and Linux, `canonicalize` on macOS), and every event path is compared against it as
//! it is: the callback touches no disk. The watcher then rewrites every path as the folder the
//! caller gave, joined with the event's own file name, so the paths match
//! [`Listing`](crate::dirlist::Listing)'s exact matching even where the platform reports another
//! spelling (`/private/var` for `/var` on macOS). An event whose parent is not that root is
//! dropped. A name inside the folder is still the platform's own: on Windows it may come back in
//! 8.3 short form, which is a different path to the list.
//!
//! Rename pairing differs by platform. Windows reports the old and the new name as two events,
//! Linux pairs them with an inotify cookie (and also reports a pair carrying both names), and
//! macOS FSEvents cannot pair them at all. A rename that cannot be paired falls back to a
//! removal of the name that went and a creation of the name that arrived: an unpaired old name
//! is a [`Change::Removed`](crate::dirlist::Change::Removed) when the quiet period ends, and an
//! unpaired new name is a [`Change::Created`](crate::dirlist::Change::Created) at once. macOS
//! reports a rename as one path with no side; that path is read as a modification, and the disk
//! says whether it is still there.
//!
//! When the OS watcher reports an overflow or asks for a rescan, [`Signal::Rescan`] is emitted
//! instead of the changes, because some events were lost and only a fresh read of the folder can
//! be trusted. When the watched folder itself is deleted or renamed, the callback only says the
//! folder may be gone; the worker confirms it (the folder is not there, or on Unix is not the
//! same folder) before [`Signal::Lost`] is emitted, and a folder that is still the same one is
//! a [`Signal::Rescan`] instead. (notify's Windows backend does not report that error at all: it
//! silently stops its watch, so a deleted folder is noticed there only by the next full read.)
//!
//! A report is settled after [`DEBOUNCE`] without a new event, but never later than
//! [`MAX_WAIT`] after its first event: one file written without pause cannot hold the rest of
//! the report back.
//!
//! Only entries directly inside the watched folder are reported. A rename out of the folder is
//! its removal, a rename into it its creation, and an event about any other path (a file's
//! contents, a subfolder's entries) is dropped.
//!
//! Deliberately not here:
//!
//! - no recursion: a subfolder's own entries are not watched, and F1's `Listing` reads a
//!   subfolder when it is expanded;
//! - no file operations and no undo (F3), no volumes (F4), no icons (F5);
//! - no drawing and no selection: the application owns both;
//! - no clock read inside [`Coalescer`], and no disk read inside it either; the watcher is what
//!   reads the clock and the disk.

mod order;
mod translate;

use crate::dirlist::{Change, Entry};
use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};
use order::{Item, settled};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use translate::Translator;

/// How long the coalescer waits for the events of one operation to stop before it reports them,
/// unless the caller asks for another quiet period with [`DirWatcher::with_debounce`].
pub const DEBOUNCE: Duration = Duration::from_millis(100);

/// The longest a report is held back, from its first event to its flush, even while more events
/// keep arriving: one file written without pause cannot starve the rest of the report.
pub const MAX_WAIT: Duration = Duration::from_secs(1);

/// One raw event, as a platform watcher saw it, before the quiet period settles it.
///
/// The variants mirror [`Change`], except that every one carries a path where a `Change` carries
/// the details read from the disk: reading those is the watcher's job, not the model's. The
/// platform adapter decides which variant a raw event is; [`DirWatcher`] builds them from
/// `notify`'s events, and a test builds them by hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawChange {
    /// A name appeared.
    Created(PathBuf),
    /// A name vanished.
    Removed(PathBuf),
    /// A name's details changed.
    Modified(PathBuf),
    /// A rename started at this old name; the matching [`RenamedTo`](Self::RenamedTo) follows.
    RenamedFrom(PathBuf),
    /// A rename arrived at this new name; it pairs with an earlier
    /// [`RenamedFrom`](Self::RenamedFrom), or is a creation when none is waiting.
    RenamedTo(PathBuf),
    /// A rename whose old and new names arrived together.
    Renamed {
        /// The name the entry had.
        from: PathBuf,
        /// The name the entry has now.
        to: PathBuf,
    },
    /// Some events were lost: the folder must be read again in full.
    Rescan,
    /// The watched folder itself may have been deleted or renamed: the worker confirms it
    /// before the application is told the watch is over.
    Lost,
}

/// What the coalescer decided happened to one entry, before its details are read.
///
/// A [`Verdict`] names paths only. [`DirWatcher`] turns each one into a [`Signal`] by reading
/// the path it names; the conversion is the private `signal_of`, and its edge cases are tested
/// with entries made by hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// An entry appeared, or a path was created that the list may already hold.
    Created(PathBuf),
    /// An entry is gone.
    Removed(PathBuf),
    /// An entry moved from one name to another.
    Renamed {
        /// The name the entry had.
        from: PathBuf,
        /// The name the entry has now.
        to: PathBuf,
    },
    /// An entry's details changed.
    Modified(PathBuf),
    /// Events were lost; the folder must be read again in full.
    Rescan,
    /// The watched folder may have been deleted or renamed; the watcher confirms it.
    Lost,
}

/// One report from a [`DirWatcher`], ready for a [`Listing`](crate::dirlist::Listing).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Signal {
    /// One settled change in the watched folder, to hand to
    /// [`Listing::apply`](crate::dirlist::Listing::apply).
    Changed(Change),
    /// Some events were lost. The application must read the folder again in full, as with
    /// [`read`](crate::dirlist::read) and
    /// [`Listing::set_entries`](crate::dirlist::Listing::set_entries), because the changes that
    /// were lost cannot be guessed.
    Rescan,
    /// The watch is over: the folder itself was deleted or renamed (confirmed by the worker),
    /// or the OS watcher failed. The application should drop the watcher and read the folder
    /// again, which will probably fail too; the list is what it was.
    Lost,
}

/// What is known about one path since the quiet period began.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Track {
    /// Created, and not removed or renamed since.
    Created,
    /// Modified, or removed and then created again.
    Modified,
    /// Removed, and not created again since.
    Removed,
    /// A rename landed here: `from` is the first name of the chain when the entry moved,
    /// and `None` when it was created (or came from outside the folder). A removal or a rename
    /// of this name reports the name itself as removed, because the list may hold the entry the
    /// landing replaced.
    Landed {
        /// The first name of the rename chain that ends here, when it moved.
        from: Option<PathBuf>,
    },
}

/// What a rename turns into at its destination.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Origin {
    /// The entry did not exist under its old name: it was created and then renamed.
    Fresh,
    /// The entry existed under this first name and moved.
    Moved(PathBuf),
}

/// A rename whose old name was seen and whose new name has not arrived yet.
#[derive(Clone, Debug, PartialEq, Eq)]
struct OpenFrom {
    /// The old name.
    path: PathBuf,
    /// What the new name will become.
    origin: Origin,
}

/// The coalescing itself: raw events with timestamps in, settled [`Verdict`]s out, and no I/O.
///
/// Feed every raw event with the [`Instant`] it was seen at through [`push`](Self::push), then
/// ask [`deadline`](Self::deadline) when the current burst will be ready and call
/// [`flush`](Self::flush) no earlier than that. A flush before the deadline gives nothing and
/// keeps the burst open, so an application may call it on any schedule.
///
/// The paths are compared exactly, as [`dirlist`](crate::dirlist) compares them: a path is inside
/// the folder when its parent is exactly the folder, the same spelling the folder's own reading
/// gives its entries.
#[derive(Clone, Debug)]
pub struct Coalescer {
    folder: PathBuf,
    debounce: Duration,
    max_wait: Duration,
    /// What is pending for each path, by path.
    paths: HashMap<PathBuf, Track>,
    /// Renames whose old name has been seen and whose new name has not, oldest first.
    open: VecDeque<OpenFrom>,
    /// Rename pairs already made from the two halves, so the platform's pair carrying both
    /// names is recognised as the same move.
    paired: Vec<(PathBuf, PathBuf)>,
    /// The time of the most recent accepted event; `None` when nothing is pending.
    last_at: Option<Instant>,
    /// The time of the first accepted event since the last flush.
    first_at: Option<Instant>,
    /// Events were lost, so nothing pending matters any more.
    rescan: bool,
    /// The watch is over; nothing pending matters any more.
    lost: bool,
}

impl Coalescer {
    /// A coalescer for entries directly inside `folder`, settling each burst `debounce` after
    /// its last event, and never later than [`MAX_WAIT`] after its first.
    ///
    /// A zero debounce settles at once: the next [`flush`](Self::flush) reports what it has,
    /// which is what a caller that wants no delay asks for.
    pub fn new(folder: impl Into<PathBuf>, debounce: Duration) -> Coalescer {
        Coalescer::with_max_wait(folder, debounce, MAX_WAIT)
    }

    /// [`new`](Self::new), with a longest wait of the caller's choosing, for a test or an
    /// application that wants a different ceiling.
    pub fn with_max_wait(
        folder: impl Into<PathBuf>,
        debounce: Duration,
        max_wait: Duration,
    ) -> Coalescer {
        Coalescer {
            folder: folder.into(),
            debounce,
            max_wait,
            paths: HashMap::new(),
            open: VecDeque::new(),
            paired: Vec::new(),
            last_at: None,
            first_at: None,
            rescan: false,
            lost: false,
        }
    }

    /// The folder whose entries are reported; its subfolders are not.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// Feeds one raw event seen at `at`.
    ///
    /// An event about a path that is not directly inside the folder is dropped and does not
    /// extend the quiet period. After a [`RawChange::Rescan`] every later event is dropped until
    /// [`flush`](Self::flush) reports the rescan, because the events that follow may be
    /// duplicates of what the fresh read will find anyway; a [`RawChange::Lost`] replaces
    /// whatever was pending, and needs no events after it.
    pub fn push(&mut self, change: RawChange, at: Instant) {
        if self.lost {
            return;
        }
        if let RawChange::Lost = change {
            self.lost = true;
            self.rescan = false;
            self.paths.clear();
            self.open.clear();
            self.paired.clear();
            self.touch(at);
            return;
        }
        if self.rescan {
            return;
        }
        match change {
            RawChange::Lost => {}
            RawChange::Rescan => {
                self.rescan = true;
                self.paths.clear();
                self.open.clear();
                self.paired.clear();
                self.touch(at);
            }
            RawChange::Created(path) => {
                if self.inside(&path) {
                    self.created(path);
                    self.touch(at);
                }
            }
            RawChange::Removed(path) => {
                if self.inside(&path) {
                    self.removed(path);
                    self.touch(at);
                }
            }
            RawChange::Modified(path) => {
                if self.inside(&path) {
                    self.modified(path);
                    self.touch(at);
                }
            }
            RawChange::RenamedFrom(path) => {
                if self.inside(&path) {
                    self.from(path);
                    self.touch(at);
                }
            }
            RawChange::RenamedTo(path) => {
                if self.inside(&path) {
                    self.to(path);
                    self.touch(at);
                }
            }
            RawChange::Renamed { from, to } => {
                let inside_from = self.inside(&from);
                let inside_to = self.inside(&to);
                if inside_from && inside_to {
                    if self.paired.contains(&(from.clone(), to.clone())) {
                        // The platform reported this rename as a pair and as two sides; the
                        // sides already paired, and this is the same move a second time.
                        return;
                    }
                    self.from(from);
                    self.to(to);
                } else if inside_from {
                    self.from(from);
                } else if inside_to {
                    self.to(to);
                } else {
                    return;
                }
                self.touch(at);
            }
        }
    }

    /// When the burst that is open now may be reported: the last accepted event plus the
    /// debounce, but never the first accepted event plus the longest wait. `None` when nothing
    /// is pending.
    ///
    /// A flush before this instant gives nothing. When the addition overflows an `Instant`,
    /// this is the last event's own time, so the burst is reported at once rather than never.
    pub fn deadline(&self) -> Option<Instant> {
        let last = self.last_at?;
        if !self.rescan && !self.lost && self.paths.is_empty() && self.open.is_empty() {
            return None;
        }
        let mut deadline = last.checked_add(self.debounce).unwrap_or(last);
        if let Some(first) = self.first_at
            && let Some(cap) = first.checked_add(self.max_wait)
        {
            deadline = deadline.min(cap);
        }
        Some(deadline)
    }

    /// Reports the settled burst as one verdict per entry, at `now`, and forgets it.
    ///
    /// `now` before [`deadline`](Self::deadline) gives an empty list and keeps the burst. At or
    /// after it, every pending path is reported once; a rename whose new name never arrived is
    /// reported as a removal of its old name.
    ///
    /// The report is ordered so that it can be applied top to bottom: anything that takes a name
    /// away comes before anything that puts an entry under that name. If no such order exists
    /// (two renames that swap their names), the renames are broken into a removal and a
    /// modification, and each name's fate is read from the disk instead.
    pub fn flush(&mut self, now: Instant) -> Vec<Verdict> {
        let Some(deadline) = self.deadline() else {
            return Vec::new();
        };
        if now < deadline {
            return Vec::new();
        }
        self.last_at = None;
        self.first_at = None;
        self.paired.clear();
        if self.lost {
            self.lost = false;
            self.paths.clear();
            self.open.clear();
            return vec![Verdict::Lost];
        }
        if self.rescan {
            self.rescan = false;
            self.paths.clear();
            self.open.clear();
            return vec![Verdict::Rescan];
        }
        let mut named: Vec<(PathBuf, Track)> = self.paths.drain().collect();
        // The map has no order of its own; sorting keeps the report the same for the same
        // events in every run.
        named.sort_by(|a, b| a.0.cmp(&b.0));
        let mut items: Vec<Item> = Vec::with_capacity(named.len() + self.open.len());
        for (path, track) in named {
            let verdict = match track {
                Track::Created => Verdict::Created(path),
                Track::Modified => Verdict::Modified(path),
                Track::Removed => Verdict::Removed(path),
                Track::Landed { from: Some(from) } if from == path => Verdict::Modified(path),
                Track::Landed { from: Some(from) } => Verdict::Renamed { from, to: path },
                Track::Landed { from: None } => Verdict::Created(path),
            };
            items.push(Item::new(verdict));
        }
        for open in self.open.drain(..) {
            // A rename whose new name never arrived: the old name is gone. When its old name
            // was itself the destination of an earlier rename, the entry is gone under that
            // first name too, and the list may still hold it.
            if let Origin::Moved(first) = &open.origin
                && first != &open.path
            {
                items.push(Item::new(Verdict::Removed(first.clone())));
            }
            items.push(Item::new(Verdict::Removed(open.path)));
        }
        settled(items)
    }

    /// Whether nothing is pending.
    pub fn is_idle(&self) -> bool {
        !self.rescan && !self.lost && self.paths.is_empty() && self.open.is_empty()
    }

    /// Whether `path` is directly inside the folder, as the folder spells it.
    fn inside(&self, path: &Path) -> bool {
        path.parent() == Some(self.folder.as_path())
    }

    /// Remembers that `at` is the newest accepted event, and the first of the burst if it is.
    ///
    /// A burst whose events cancel each other out leaves nothing pending; its times are
    /// forgotten too, so a later event starts its own burst instead of inheriting a wait that
    /// has nearly run out.
    fn touch(&mut self, at: Instant) {
        self.last_at = Some(match self.last_at {
            Some(last) if last > at => last,
            _ => at,
        });
        if self.first_at.is_none() {
            self.first_at = Some(at);
        }
        if self.is_idle() {
            self.last_at = None;
            self.first_at = None;
        }
    }

    /// A name appeared.
    fn created(&mut self, path: PathBuf) {
        match self.paths.insert(path.clone(), Track::Created) {
            // A creation after a removal of the same name: the entry is back, changed.
            Some(Track::Removed) => {
                self.paths.insert(path, Track::Modified);
            }
            // A rename was landing here and the name was created instead: the entry that was
            // moving is replaced, so its first name is gone.
            Some(Track::Landed { from: Some(from) }) => {
                self.paths.entry(from).or_insert(Track::Removed);
            }
            Some(Track::Landed { from: None })
            | Some(Track::Created)
            | Some(Track::Modified)
            | None => {}
        }
    }

    /// A name's details changed.
    fn modified(&mut self, path: PathBuf) {
        match self.paths.get_mut(&path) {
            // A removal and then a change of the same name is the entry back, changed.
            Some(track) => {
                if *track == Track::Removed {
                    *track = Track::Modified;
                }
            }
            None => {
                self.paths.insert(path, Track::Modified);
            }
        }
    }

    /// A name vanished.
    fn removed(&mut self, path: PathBuf) {
        match self.paths.remove(&path) {
            // Created and then removed: the list never saw it.
            Some(Track::Created) => {}
            // A rename landed here and the name is gone again. The list may hold the entry this
            // name had before the rename landed, so this name is removed too, as well as the
            // first name the entry came from.
            Some(Track::Landed { from }) => {
                self.paths.insert(path, Track::Removed);
                if let Some(from) = from {
                    self.paths.entry(from).or_insert(Track::Removed);
                }
            }
            Some(Track::Modified) | Some(Track::Removed) | None => {
                self.paths.insert(path, Track::Removed);
            }
        }
    }

    /// A rename left `path`.
    fn from(&mut self, path: PathBuf) {
        // Some platforms repeat an old name; it is the same move.
        if self.open.iter().any(|open| open.path == path) {
            return;
        }
        let track = self.paths.remove(&path);
        let origin = match track {
            // A name a rename landed on, or a creation, is vacated: the list may still hold the
            // entry that was at this name before the burst, so this name is removed too. The
            // collapsed rename no longer names it (a chain keeps only its first and last names,
            // and a creation moved on becomes a creation at the destination).
            Some(Track::Landed { from }) => {
                self.paths.insert(path.clone(), Track::Removed);
                from.map(Origin::Moved).unwrap_or(Origin::Fresh)
            }
            Some(Track::Created) | Some(Track::Removed) => {
                self.paths.insert(path.clone(), Track::Removed);
                Origin::Fresh
            }
            Some(Track::Modified) | None => Origin::Moved(path.clone()),
        };
        self.open.push_back(OpenFrom { path, origin });
    }

    /// A rename arrived at `path`.
    fn to(&mut self, path: PathBuf) {
        let origin = match self.open.pop_front() {
            Some(open) => {
                // Remember the pair, so the platform's event carrying both names is known to be
                // the same move.
                self.paired.push((open.path, path.clone()));
                open.origin
            }
            // No old name waiting: the entry was renamed into the folder from outside.
            None => Origin::Fresh,
        };
        let track = match origin {
            Origin::Fresh => Track::Landed { from: None },
            Origin::Moved(from) => Track::Landed { from: Some(from) },
        };
        self.put(path, track);
    }

    /// Stores `track` for `path`, recording the removal of an entry that was moving here when
    /// the new one replaces it.
    fn put(&mut self, path: PathBuf, track: Track) {
        let landed_from = match &track {
            Track::Landed { from } => from.clone(),
            Track::Created | Track::Modified | Track::Removed => None,
        };
        if let Some(Track::Landed { from: Some(from) }) = self.paths.insert(path, track)
            && landed_from.as_ref() != Some(&from)
        {
            self.paths.entry(from).or_insert(Track::Removed);
        }
    }
}

/// Reads one verdict into a signal, with `read` standing in for the disk.
///
/// A created entry that cannot be read is not reported: it is gone again, and the list never
/// saw it. A modified entry that cannot be read is removed, because the path it names is not
/// there any more. A rename whose new name cannot be read is a removal of the old name, for the
/// same reason.
fn signal_of(verdict: Verdict, read: &dyn Fn(&Path) -> Option<Entry>) -> Option<Signal> {
    match verdict {
        Verdict::Created(path) => read(&path).map(|entry| Signal::Changed(Change::Created(entry))),
        Verdict::Modified(path) => Some(Signal::Changed(match read(&path) {
            Some(entry) => Change::Modified(entry),
            None => Change::Removed(path),
        })),
        Verdict::Removed(path) => Some(Signal::Changed(Change::Removed(path))),
        Verdict::Renamed { from, to } => Some(Signal::Changed(match read(&to) {
            Some(entry) => Change::Renamed { from, to: entry },
            None => Change::Removed(from),
        })),
        Verdict::Rescan => Some(Signal::Rescan),
        Verdict::Lost => Some(Signal::Lost),
    }
}

/// One `notify` result as raw changes: an error means events may have been lost, so a rescan.
fn translation(result: notify::Result<notify::Event>, names: &Translator) -> Vec<RawChange> {
    match result {
        Ok(event) => names.translate(&event),
        Err(_) => vec![RawChange::Rescan],
    }
}

/// What the OS watcher is given, and what its events are built from: the folder made absolute
/// without touching a disk (Windows and Linux report the path it was given), and canonicalised
/// on macOS, where FSEvents reports the canonical path.
fn notify_root(folder: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::fs::canonicalize(folder).ok()
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::path::absolute(folder).ok()
    }
}

/// What a folder was, so the worker can tell a report about it from a report about a folder
/// that was deleted and made again under the same name.
#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
struct Identity {
    dev: u64,
    ino: u64,
}

/// What a folder was; Windows has no identity the watcher can compare.
#[cfg(not(unix))]
#[derive(Clone, Copy, Debug)]
struct Identity;

/// The identity of `path`, where the platform has one to compare.
fn identity(path: &Path) -> Option<Identity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::symlink_metadata(path).ok()?;
        Some(Identity {
            dev: meta.dev(),
            ino: meta.ino(),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// Confirms a folder-loss report in the worker, where a disk read is allowed.
///
/// `Lost` only when the folder is not there or is not the same folder that was watched; a
/// folder that is still the same one means the events were confused, and a full read (`Rescan`)
/// is the safe answer.
fn lost_signal(root: &Path, started: Option<Identity>) -> Signal {
    let meta = match std::fs::symlink_metadata(root) {
        Ok(meta) => meta,
        Err(_) => return Signal::Lost,
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Some(started) = started
            && (meta.dev() != started.dev || meta.ino() != started.ino)
        {
            return Signal::Lost;
        }
    }
    #[cfg(not(unix))]
    let _ = (meta, started);
    Signal::Rescan
}

/// Raw events waiting for the watcher's thread, and the signal that stops it.
#[derive(Default)]
struct Inbox {
    state: Mutex<InboxState>,
    ready: Condvar,
}

#[derive(Default)]
struct InboxState {
    /// Events in arrival order, each with the time it was seen.
    events: VecDeque<(RawChange, Instant)>,
    /// The watcher was dropped; the thread must end without reporting anything more.
    stop: bool,
}

impl Inbox {
    /// Queues one event, unless the watcher is being dropped.
    fn push(&self, change: RawChange, at: Instant) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stop {
            return;
        }
        state.events.push_back((change, at));
        self.ready.notify_one();
    }

    /// Tells the thread to end.
    fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.stop = true;
        self.ready.notify_all();
    }

    /// Whether the watcher has been dropped, so the thread must end without reporting anything
    /// more.
    fn stopped(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).stop
    }
}

/// The settled reports waiting for the application, and whether a wake has been sent for them.
#[derive(Default)]
struct Shared {
    /// Signals not taken yet, in the order they were settled.
    signals: Mutex<Vec<Signal>>,
    /// A wake has been sent and the application has not called [`DirWatcher::woken`] yet.
    pending: AtomicBool,
}

impl Shared {
    /// Adds a settled report and wakes once for it, as [`FileWatcher`](crate::watch::FileWatcher)
    /// does: further reports do not wake again until `woken`.
    fn deliver(&self, signals: Vec<Signal>, wake: &dyn Fn()) {
        if signals.is_empty() {
            return;
        }
        self.signals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(signals);
        if !self.pending.swap(true, Ordering::AcqRel) {
            wake();
        }
    }

    /// Resets the wake flag: the next report wakes again.
    fn woken(&self) {
        self.pending.store(false, Ordering::Release);
    }

    /// Takes the reports that are waiting, leaving none.
    fn take(&self) -> Vec<Signal> {
        std::mem::take(&mut *self.signals.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// Reads one path's entry; `Entry::at` in a real watcher, a made-up disk in a test.
type Read = dyn Fn(&Path) -> Option<Entry> + Send + Sync;

/// Watches one folder and reports what changed in it, settled, as [`Signal`]s.
///
/// [`DirWatcher::new`] starts the OS watcher and a thread of its own. That thread is the only
/// one that touches the [`Coalescer`], and it reads each changed path's details there with
/// [`Entry::at`](crate::dirlist::Entry::at), so neither the drawing thread nor the OS watcher's
/// callback ever waits for a disk. When a report is ready the thread calls the wake it was given
/// (once, until [`woken`](Self::woken) is called) and the application takes the report with
/// [`take`](Self::take).
///
/// Dropping the watcher does not wait for the thread: the thread may be in the middle of a read
/// on a share that never answers, and the application's thread must not wait for it. The drop
/// tells the thread to stop, and the thread ends by itself after the read it is in returns,
/// without reading anything else or reporting anything.
pub struct DirWatcher {
    folder: PathBuf,
    /// What the OS watcher was given and what its events are built from.
    root: PathBuf,
    /// The OS watcher; dropping it stops its callbacks.
    watcher: RecommendedWatcher,
    inbox: Arc<Inbox>,
    shared: Arc<Shared>,
}

impl DirWatcher {
    /// Watches `folder` with a [`DEBOUNCE`] quiet period, calling `wake` when a report is ready.
    ///
    /// `None` when the OS watcher cannot start or cannot watch the folder (no inotify instances
    /// left, or the folder cannot be opened); the application then falls back to reading the
    /// folder again on a slow poll. `wake` runs on the watcher's own thread: it must be cheap,
    /// must not block on anything the application holds, and must not panic.
    ///
    /// Every path in the report is this folder joined with the entry's own name, so the list's
    /// exact matching works wherever the platform spells paths another way. An entry's name is
    /// the platform's own; on Windows it may be the 8.3 short form, which is a different path to
    /// the list.
    pub fn new(
        folder: impl Into<PathBuf>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Option<DirWatcher> {
        DirWatcher::with_debounce(folder, DEBOUNCE, wake)
    }

    /// [`new`](Self::new), with a quiet period of the caller's choosing.
    ///
    /// A shorter period reports sooner and splits one operation into more reports; a longer one
    /// settles a save that writes several files. The report is settled at most [`MAX_WAIT`] after
    /// its first event whatever the quiet period.
    pub fn with_debounce(
        folder: impl Into<PathBuf>,
        debounce: Duration,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Option<DirWatcher> {
        DirWatcher::start(
            folder.into(),
            debounce,
            wake,
            Arc::new(|path: &Path| Entry::at(path)),
        )
    }

    /// Starts the watcher with the reader given: [`Entry::at`](crate::dirlist::Entry::at) in a
    /// real watcher, and a made-up disk in a test of this module.
    fn start(
        folder: PathBuf,
        debounce: Duration,
        wake: impl Fn() + Send + Sync + 'static,
        read: Arc<Read>,
    ) -> Option<DirWatcher> {
        let root = notify_root(&folder)?;
        let inbox = Arc::new(Inbox::default());
        let shared = Arc::new(Shared::default());
        let names = Translator::new(folder.clone(), root.clone());
        let for_events = Arc::clone(&inbox);
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let at = Instant::now();
            for change in translation(event, &names) {
                for_events.push(change, at);
            }
        })
        .ok()?;
        let mut watcher = watcher;
        if watcher.watch(&root, RecursiveMode::NonRecursive).is_err() {
            return None;
        }
        let identity = identity(&root);
        let coalescer = Coalescer::new(folder.clone(), debounce);
        let for_work = Arc::clone(&inbox);
        let for_report = Arc::clone(&shared);
        let for_missing = root.clone();
        // The handle is dropped at once: the thread is not waited for. See `DirWatcher`.
        _ = std::thread::Builder::new()
            .name("mdshared-dirwatch".to_string())
            .spawn(move || {
                run(
                    for_work,
                    for_report,
                    coalescer,
                    &wake,
                    read,
                    for_missing,
                    identity,
                )
            })
            .ok()?;
        Some(DirWatcher {
            folder,
            root,
            watcher,
            inbox,
            shared,
        })
    }

    /// The folder being watched, as it was given.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// The settled reports waiting, in the order they were settled, and forgets them.
    ///
    /// Call [`woken`](Self::woken) before this, as [`FileWatcher::woken`](crate::watch::FileWatcher::woken)
    /// is called, so a report settled during the call wakes again instead of being missed.
    pub fn take(&self) -> Vec<Signal> {
        self.shared.take()
    }

    /// Says the reports taken so far have been dealt with, so the next one wakes again.
    pub fn woken(&self) {
        self.shared.woken();
    }
}

impl Drop for DirWatcher {
    fn drop(&mut self) {
        // Stop the OS watch first, so its callback pushes nothing more, then tell the thread to
        // stop and do not wait for it: it may be inside a read on a share that never answers,
        // and this drop runs on the thread that draws.
        _ = self.watcher.unwatch(&self.root);
        self.inbox.stop();
    }
}

/// The watcher's thread: feed the coalescer, wait for the quiet period, read, report.
///
/// It waits on the inbox's condition variable until either an event arrives or the open burst's
/// deadline passes; the wait is for a duration, never a poll, so an idle folder costs nothing.
/// Once the watcher is dropped it ends without reading anything more or reporting anything.
fn run(
    inbox: Arc<Inbox>,
    shared: Arc<Shared>,
    mut coalescer: Coalescer,
    wake: &(dyn Fn() + Sync),
    read: Arc<Read>,
    root: PathBuf,
    started: Option<Identity>,
) {
    let mut state = inbox.state.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if state.stop {
            return;
        }
        while let Some((change, at)) = state.events.pop_front() {
            coalescer.push(change, at);
        }
        let now = Instant::now();
        let Some(deadline) = coalescer.deadline() else {
            state = inbox.ready.wait(state).unwrap_or_else(|e| e.into_inner());
            continue;
        };
        if deadline <= now {
            let verdicts = coalescer.flush(now);
            // The disk is read without the inbox held, so events keep arriving.
            drop(state);
            let mut signals = Vec::with_capacity(verdicts.len());
            for verdict in verdicts {
                // A read can be slow; stop between two of them rather than after the last.
                if inbox.stopped() {
                    return;
                }
                let signal = match verdict {
                    // The callback only says the folder may be gone; the worker, which may
                    // touch a disk, confirms it before the application is told the watch ended.
                    Verdict::Lost => Some(lost_signal(&root, started)),
                    other => signal_of(other, &*read),
                };
                if let Some(signal) = signal {
                    signals.push(signal);
                }
            }
            // Check the stop with the lock held, so a drop cannot land between the check and
            // the wake: the drop takes this lock to set the flag.
            state = inbox.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.stop {
                return;
            }
            shared.deliver(signals, wake);
            continue;
        }
        state = inbox
            .ready
            .wait_timeout(state, deadline - now)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}

#[cfg(test)]
#[path = "dirwatch_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "dirwatch_model_tests.rs"]
mod model_tests;

#[cfg(test)]
#[path = "dirwatch_watcher_tests.rs"]
mod watcher_tests;
