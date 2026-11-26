//! Resolves settled path verdicts and checks whether the watched folder still exists.
//! This is the worker's disk side; the notification callback never reads metadata.

use super::{Change, Entry, Signal, Verdict};
use std::{io, path::Path};

/// The followed folder's identity, not the identity of a symlink used to reach it.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Identity {
    dev: u64,
    ino: u64,
}

/// The current Windows backend does not use a filesystem identity for loss checks.
#[cfg(not(unix))]
pub(super) type Identity = ();

#[cfg(unix)]
pub(super) fn identity(path: &Path) -> Option<Identity> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    meta.is_dir().then(|| Identity {
        dev: meta.dev(),
        ino: meta.ino(),
    })
}

/// On Unix, decline registration if the directory disappeared or its identity changed during
/// setup. Otherwise a live watcher could be registered against a different directory.
pub(super) fn register<T>(
    root: &Path,
    watch: impl FnOnce() -> io::Result<T>,
) -> io::Result<(T, Option<Identity>)> {
    #[cfg(unix)]
    {
        let before = identity(root)
            .ok_or_else(|| io::Error::other("cannot identify the directory to watch"))?;
        let watcher = watch()?;
        if identity(root) != Some(before) {
            return Err(io::Error::other(
                "watched directory changed during registration",
            ));
        }
        Ok((watcher, Some(before)))
    }
    #[cfg(not(unix))]
    {
        let _ = root;
        Ok((watch()?, None))
    }
}

/// Confirms a possible loss. Report `Lost` if the root can no longer be verified: a vanished
/// or replaced target cannot be recovered by applying entry changes to this watch.
pub(super) fn lost_signal(root: &Path, started: Option<Identity>) -> Signal {
    #[cfg(unix)]
    let still_watched =
        identity(root).is_some_and(|current| started.is_none_or(|id| id == current));
    #[cfg(not(unix))]
    let still_watched = {
        let _ = started;
        std::fs::metadata(root).is_ok_and(|meta| meta.is_dir())
    };
    if still_watched {
        Signal::Rescan
    } else {
        Signal::Lost
    }
}

/// Reads through the absolute watch root, then restores the caller's path spelling for the list.
pub(super) fn read_from_root(root: &Path, path: &Path) -> io::Result<Entry> {
    let name = path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
    let mut entry = Entry::at_checked(&root.join(name))?;
    entry.path = path.to_owned();
    Ok(entry)
}

/// Reads one verdict into a signal, with `read` standing in for the disk.
///
/// Only `NotFound` proves absence. Other errors request a rescan rather than losing a row.
pub(super) fn signal_of(
    verdict: Verdict,
    read: &dyn Fn(&Path) -> io::Result<Entry>,
) -> Option<Signal> {
    let resolve = |result: io::Result<Entry>, found: fn(Entry) -> Change, missing| match result {
        Ok(entry) => Some(Signal::Changed(found(entry))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => missing,
        Err(_) => Some(Signal::Rescan),
    };
    match verdict {
        Verdict::Created(path) => resolve(read(&path), Change::Created, None),
        Verdict::Modified(path) => resolve(
            read(&path),
            Change::Modified,
            Some(Signal::Changed(Change::Removed(path))),
        ),
        Verdict::Removed(path) => Some(Signal::Changed(Change::Removed(path))),
        Verdict::Renamed { from, to } => Some(match read(&to) {
            Ok(entry) => Signal::Changed(Change::Renamed { from, to: entry }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Signal::Changed(Change::Removed(from)),
            Err(_) => Signal::Rescan,
        }),
        Verdict::Rescan => Some(Signal::Rescan),
        Verdict::Lost => Some(Signal::Lost),
    }
}
