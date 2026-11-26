//! Resolves settled path verdicts and checks whether the watched folder still exists.
//! This is the worker's disk side; the notification callback never reads metadata.

use super::{Change, Entry, Signal, Verdict};
use std::{io, path::Path};

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
