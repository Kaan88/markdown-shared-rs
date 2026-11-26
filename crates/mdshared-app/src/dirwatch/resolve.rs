//! Resolves settled path verdicts and checks whether the watched folder still exists.
//! This is the worker's disk side; the notification callback never reads metadata.

use super::{Change, Entry, Signal, Verdict};
use std::path::Path;

/// Reads one verdict into a signal, with `read` standing in for the disk.
///
/// A created entry that cannot be read is not reported: it is gone again, and the list never
/// saw it. A modified entry that cannot be read is removed, because the path it names is not
/// there any more. A rename whose new name cannot be read is a removal of the old name, for the
/// same reason.
pub(super) fn signal_of(verdict: Verdict, read: &dyn Fn(&Path) -> Option<Entry>) -> Option<Signal> {
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
