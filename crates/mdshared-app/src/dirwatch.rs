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
//! Rename pairing differs by platform. Windows reports the old and the new name as two events,
//! Linux pairs them with an inotify cookie (and also reports a pair carrying both names), and
//! macOS FSEvents cannot pair them at all. A rename that cannot be paired falls back to a
//! removal of the name that went and a creation of the name that arrived: an unpaired old name
//! is a [`Change::Removed`](crate::dirlist::Change::Removed) when the quiet period ends, and an
//! unpaired new name is a [`Change::Created`](crate::dirlist::Change::Created) at once. A
//! platform that reports a rename with one path and no side reads as a removal plus a creation
//! of that path, which is a modification once the path's details are read.
//!
//! When the OS watcher reports an overflow or asks for a rescan, [`Signal::Rescan`] is emitted
//! instead of the changes, because some events were lost and only a fresh read of the folder can
//! be trusted.
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

use crate::dirlist::{Change, Entry};
use notify::event::{ModifyKind, RenameMode};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long the coalescer waits for the events of one operation to stop before it reports them,
/// unless the caller asks for another quiet period with [`DirWatcher::with_debounce`].
pub const DEBOUNCE: Duration = Duration::from_millis(100);

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
    /// A rename is landing here under `from`, and the entry has not moved on since.
    Moved {
        /// The first name of the rename chain that ends here.
        from: PathBuf,
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
    /// What is pending for each path, by path.
    paths: HashMap<PathBuf, Track>,
    /// Renames whose old name has been seen and whose new name has not, oldest first.
    open: VecDeque<OpenFrom>,
    /// The time of the most recent accepted event; `None` when nothing is pending.
    last_at: Option<Instant>,
    /// Events were lost, so nothing pending matters any more.
    rescan: bool,
}

impl Coalescer {
    /// A coalescer for entries directly inside `folder`, settling each burst `debounce` after
    /// its last event.
    ///
    /// A zero debounce settles at once: the next [`flush`](Self::flush) reports what it has,
    /// which is what a caller that wants no delay asks for.
    pub fn new(folder: impl Into<PathBuf>, debounce: Duration) -> Coalescer {
        Coalescer {
            folder: folder.into(),
            debounce,
            paths: HashMap::new(),
            open: VecDeque::new(),
            last_at: None,
            rescan: false,
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
    /// duplicates of what the fresh read will find anyway.
    pub fn push(&mut self, change: RawChange, at: Instant) {
        if self.rescan {
            return;
        }
        match change {
            RawChange::Rescan => {
                self.rescan = true;
                self.paths.clear();
                self.open.clear();
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
                    if self.paths.get(&to) == Some(&Track::Moved { from: from.clone() }) {
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
    /// debounce. `None` when nothing is pending.
    ///
    /// A flush before this instant gives nothing. When the addition overflows an `Instant`,
    /// this is the last event's own time, so the burst is reported at once rather than never.
    pub fn deadline(&self) -> Option<Instant> {
        let last = self.last_at?;
        if !self.rescan && self.paths.is_empty() && self.open.is_empty() {
            return None;
        }
        Some(last.checked_add(self.debounce).unwrap_or(last))
    }

    /// Reports the settled burst as one verdict per entry, at `now`, and forgets it.
    ///
    /// `now` before [`deadline`](Self::deadline) gives an empty list and keeps the burst. At or
    /// after it, every pending path is reported once; a rename whose new name never arrived is
    /// reported as a removal of its old name; the list is ordered by path, and a removal comes
    /// before anything else at the same path, so a path removed and created again (which is a
    /// modification) is applied the way it happened.
    pub fn flush(&mut self, now: Instant) -> Vec<Verdict> {
        let Some(deadline) = self.deadline() else {
            return Vec::new();
        };
        if now < deadline {
            return Vec::new();
        }
        self.last_at = None;
        if self.rescan {
            self.rescan = false;
            self.paths.clear();
            self.open.clear();
            return vec![Verdict::Rescan];
        }
        // The primary path of each verdict, a rank that puts a removal first at one path, and
        // the verdict itself.
        let mut settled: Vec<(PathBuf, u8, Verdict)> =
            Vec::with_capacity(self.paths.len() + self.open.len());
        for (path, track) in self.paths.drain() {
            let (rank, verdict) = match track {
                Track::Created => (1, Verdict::Created(path.clone())),
                Track::Modified => (1, Verdict::Modified(path.clone())),
                Track::Removed => (0, Verdict::Removed(path.clone())),
                Track::Moved { from } => (
                    1,
                    Verdict::Renamed {
                        from,
                        to: path.clone(),
                    },
                ),
            };
            settled.push((path, rank, verdict));
        }
        for open in self.open.drain(..) {
            settled.push((open.path.clone(), 0, Verdict::Removed(open.path)));
        }
        settled.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut out: Vec<Verdict> = Vec::with_capacity(settled.len());
        let mut last_removed: Option<PathBuf> = None;
        for (_, _, verdict) in settled {
            if let Verdict::Removed(path) = &verdict {
                if last_removed.as_ref() == Some(path) {
                    continue;
                }
                last_removed = Some(path.clone());
            }
            out.push(verdict);
        }
        out
    }

    /// Whether nothing is pending.
    pub fn is_idle(&self) -> bool {
        !self.rescan && self.paths.is_empty() && self.open.is_empty()
    }

    /// Whether `path` is directly inside the folder, as the folder spells it.
    fn inside(&self, path: &Path) -> bool {
        path.parent() == Some(self.folder.as_path())
    }

    /// Remembers that `at` is the newest accepted event.
    fn touch(&mut self, at: Instant) {
        self.last_at = Some(match self.last_at {
            Some(last) if last > at => last,
            _ => at,
        });
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
            Some(Track::Moved { from }) => {
                self.paths.entry(from).or_insert(Track::Removed);
            }
            Some(Track::Created) | Some(Track::Modified) | None => {}
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
            // A rename whose destination was removed: the entry is gone under its first name.
            Some(Track::Moved { from }) => {
                self.paths.entry(from).or_insert(Track::Removed);
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
            // The entry was created and then renamed: its destination is a creation.
            Some(Track::Created) | Some(Track::Removed) => Origin::Fresh,
            // A chain: keep the first name.
            Some(Track::Moved { from }) => Origin::Moved(from),
            Some(Track::Modified) | None => Origin::Moved(path.clone()),
        };
        self.open.push_back(OpenFrom { path, origin });
    }

    /// A rename arrived at `path`.
    fn to(&mut self, path: PathBuf) {
        let track = match self.open.pop_front() {
            Some(open) => match open.origin {
                Origin::Fresh => Track::Created,
                Origin::Moved(from) => Track::Moved { from },
            },
            // No old name waiting: the entry was renamed into the folder from outside.
            None => Track::Created,
        };
        self.put(path, track);
    }

    /// Stores `track` for `path`, recording the removal of an entry that was moving here when
    /// the new one replaces it.
    fn put(&mut self, path: PathBuf, track: Track) {
        let landed_from = match &track {
            Track::Moved { from } => Some(from.clone()),
            Track::Created | Track::Modified | Track::Removed => None,
        };
        if let Some(Track::Moved { from }) = self.paths.insert(path, track)
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
    }
}

/// What a `notify` event means, as raw changes. Empty for an event that says nothing about a
/// name inside the folder (an access, or an event with no path).
///
/// A platform that cannot pair a rename's two names reports it as one event carrying one path;
/// that becomes a removal and a creation of the same path, which the coalescer settles as a
/// modification of it.
fn raw_changes(event: &notify::Event) -> Vec<RawChange> {
    if event.need_rescan() {
        return vec![RawChange::Rescan];
    }
    let mut out = Vec::with_capacity(event.paths.len());
    match &event.kind {
        EventKind::Create(_) => out.extend(event.paths.iter().cloned().map(RawChange::Created)),
        EventKind::Remove(_) => out.extend(event.paths.iter().cloned().map(RawChange::Removed)),
        EventKind::Modify(ModifyKind::Name(mode)) => match mode {
            RenameMode::From => out.extend(event.paths.iter().cloned().map(RawChange::RenamedFrom)),
            RenameMode::To => out.extend(event.paths.iter().cloned().map(RawChange::RenamedTo)),
            RenameMode::Both => match event.paths.as_slice() {
                [from, to, ..] => out.push(RawChange::Renamed {
                    from: from.clone(),
                    to: to.clone(),
                }),
                _ => out.extend(event.paths.iter().cloned().map(RawChange::RenamedTo)),
            },
            // macOS FSEvents pairs no names: the one path it gives is both sides, which is a
            // removal and a creation of it.
            RenameMode::Any | RenameMode::Other => {
                for path in &event.paths {
                    out.push(RawChange::Removed(path.clone()));
                    out.push(RawChange::Created(path.clone()));
                }
            }
        },
        EventKind::Modify(_) => out.extend(event.paths.iter().cloned().map(RawChange::Modified)),
        // An access changes nothing a list shows, and an untyped event says nothing usable.
        EventKind::Access(_) | EventKind::Any | EventKind::Other => {}
    }
    out
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

/// Watches one folder and reports what changed in it, settled, as [`Signal`]s.
///
/// [`DirWatcher::new`] starts the OS watcher and a thread of its own. That thread is the only
/// one that touches the [`Coalescer`], and it reads each changed path's details there with
/// [`Entry::at`](crate::dirlist::Entry::at), so neither the drawing thread nor the OS watcher's
/// callback ever waits for a disk. When a report is ready the thread calls the wake it was given
/// (once, until [`woken`](Self::woken) is called) and the application takes the report with
/// [`take`](Self::take).
///
/// The thread ends when this value is dropped, and joins before the drop returns.
pub struct DirWatcher {
    folder: PathBuf,
    /// The OS watcher; dropped after the thread is joined.
    watcher: RecommendedWatcher,
    inbox: Arc<Inbox>,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl DirWatcher {
    /// Watches `folder` with a [`DEBOUNCE`] quiet period, calling `wake` when a report is ready.
    ///
    /// `None` when the OS watcher cannot start or cannot watch the folder (no inotify instances
    /// left, or the folder cannot be opened); the application then falls back to reading the
    /// folder again on a slow poll. `wake` runs on the watcher's own thread: it must be cheap,
    /// must not block on anything the application holds, and must not panic.
    ///
    /// Give the folder as an absolute path, spelled exactly as
    /// [`dirlist::read`](crate::dirlist::read) is given it. The paths in the report are the
    /// folder joined with each name, so a relative folder is reported relative on Windows and
    /// Linux but absolute on macOS; an absolute one is reported the same way everywhere.
    pub fn new(
        folder: impl Into<PathBuf>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Option<DirWatcher> {
        DirWatcher::with_debounce(folder, DEBOUNCE, wake)
    }

    /// [`new`](Self::new), with a quiet period of the caller's choosing.
    ///
    /// A shorter period reports sooner and splits one operation into more reports; a longer one
    /// settles a save that writes several files. Nothing here caps the period.
    pub fn with_debounce(
        folder: impl Into<PathBuf>,
        debounce: Duration,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Option<DirWatcher> {
        let folder = folder.into();
        let inbox = Arc::new(Inbox::default());
        let shared = Arc::new(Shared::default());
        let watched = Arc::clone(&inbox);
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let at = Instant::now();
            match event {
                Ok(event) => {
                    for change in raw_changes(&event) {
                        watched.push(change, at);
                    }
                }
                // The watcher could not say what happened, so events may have been lost.
                Err(_) => watched.push(RawChange::Rescan, at),
            }
        })
        .ok()?;
        let mut watcher = watcher;
        if watcher.watch(&folder, RecursiveMode::NonRecursive).is_err() {
            return None;
        }
        let coalescer = Coalescer::new(folder.clone(), debounce);
        let for_work = Arc::clone(&inbox);
        let for_report = Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name("mdshared-dirwatch".to_string())
            .spawn(move || run(for_work, for_report, coalescer, &wake))
            .ok()?;
        Some(DirWatcher {
            folder,
            watcher,
            inbox,
            shared,
            worker: Some(worker),
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
        // Release the watch first, so the OS watcher stops queueing events; the thread is then
        // told to end and joined, and the field itself is dropped after this body.
        _ = self.watcher.unwatch(&self.folder);
        self.inbox.stop();
        if let Some(worker) = self.worker.take() {
            _ = worker.join();
        }
    }
}

/// The watcher's thread: feed the coalescer, wait for the quiet period, read, report.
///
/// It waits on the inbox's condition variable until either an event arrives or the open burst's
/// deadline passes; the wait is for a duration, never a poll, so an idle folder costs nothing.
fn run(inbox: Arc<Inbox>, shared: Arc<Shared>, mut coalescer: Coalescer, wake: &dyn Fn()) {
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
            let signals: Vec<Signal> = verdicts
                .into_iter()
                .filter_map(|verdict| signal_of(verdict, &|path| Entry::at(path)))
                .collect();
            shared.deliver(signals, wake);
            state = inbox.state.lock().unwrap_or_else(|e| e.into_inner());
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
