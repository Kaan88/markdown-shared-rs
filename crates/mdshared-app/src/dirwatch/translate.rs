//! The `notify` event translation: which paths are the watched folder's entries, and which
//! names they are reported under.
//!
//! The callback this runs in touches no disk: the root the OS watcher was given is worked out
//! once, at start, so a hung mount cannot make `Drop` wait on a canonicalisation.

use super::RawChange;
use notify::EventKind;
use notify::event::{ModifyKind, RenameMode};
use std::path::{Path, PathBuf};

/// Rewrites the paths of `notify`'s events as the caller's folder, and drops everything that is
/// not an entry directly inside it.
///
/// Platforms spell a path their own way: macOS FSEvents reports `/private/var/...` for a folder
/// given as `/var/...`, and a relative folder may come back absolute. The root the OS watcher
/// was given is worked out once, at start (`std::path::absolute` on Windows and Linux,
/// `canonicalize` on macOS), and every event path is compared against it as it is: the callback
/// touches no disk, so a hung mount cannot make it wait. The path handed on is the caller's
/// folder joined with the event's own file name, so a [`Listing`](crate::dirlist::Listing) with
/// that folder matches it exactly. The file name itself is the platform's: on Windows it can be
/// the 8.3 short form, which is a different path to the list.
#[derive(Clone, Debug)]
pub(super) struct Translator {
    /// The folder as the caller gave it: what the report names.
    folder: PathBuf,
    /// What the OS watcher was given and what its events are built from.
    root: PathBuf,
}

impl Translator {
    pub(super) fn new(folder: PathBuf, root: PathBuf) -> Translator {
        Translator { folder, root }
    }

    /// Whether this path names the watched folder itself: it was deleted or renamed.
    fn is_folder(&self, path: &Path) -> bool {
        path == self.root
    }

    /// The event path rewritten as the caller's folder, when it is directly inside it.
    fn inside(&self, path: &Path) -> Option<PathBuf> {
        let name = path.file_name()?;
        (path.parent() == Some(self.root.as_path())).then(|| self.folder.join(name))
    }

    /// What one `notify` event means, as raw changes. Empty for an event that says nothing about
    /// a name inside the folder (an access, or an event with no path).
    pub(super) fn translate(&self, event: &notify::Event) -> Vec<RawChange> {
        if event.need_rescan() {
            return vec![RawChange::Rescan];
        }
        if matches!(
            event.kind,
            EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
        ) && event.paths.iter().any(|path| self.is_folder(path))
        {
            return vec![RawChange::Lost];
        }
        let mut out = Vec::with_capacity(event.paths.len());
        match &event.kind {
            EventKind::Create(_) => {
                out.extend(
                    event
                        .paths
                        .iter()
                        .filter_map(|p| self.inside(p))
                        .map(RawChange::Created),
                );
            }
            EventKind::Remove(_) => {
                out.extend(
                    event
                        .paths
                        .iter()
                        .filter_map(|p| self.inside(p))
                        .map(RawChange::Removed),
                );
            }
            EventKind::Modify(ModifyKind::Name(mode)) => match mode {
                RenameMode::From => out.extend(
                    event
                        .paths
                        .iter()
                        .filter_map(|p| self.inside(p))
                        .map(RawChange::RenamedFrom),
                ),
                RenameMode::To => out.extend(
                    event
                        .paths
                        .iter()
                        .filter_map(|p| self.inside(p))
                        .map(RawChange::RenamedTo),
                ),
                RenameMode::Both => match event.paths.as_slice() {
                    [from, to, ..] => match (self.inside(from), self.inside(to)) {
                        (Some(from), Some(to)) => out.push(RawChange::Renamed { from, to }),
                        (Some(from), None) => out.push(RawChange::RenamedFrom(from)),
                        (None, Some(to)) => out.push(RawChange::RenamedTo(to)),
                        (None, None) => {}
                    },
                    _ => out.extend(
                        event
                            .paths
                            .iter()
                            .filter_map(|p| self.inside(p))
                            .map(RawChange::RenamedTo),
                    ),
                },
                // A metadata read cannot distinguish the old spelling from the new one on a
                // case-insensitive volume. Only enumerating the folder gives its actual names.
                RenameMode::Any | RenameMode::Other => {
                    if event.paths.iter().any(|p| self.inside(p).is_some()) {
                        out.push(RawChange::Rescan);
                    }
                }
            },
            EventKind::Modify(_) => out.extend(
                event
                    .paths
                    .iter()
                    .filter_map(|p| self.inside(p))
                    .map(RawChange::Modified),
            ),
            // An access changes nothing a list shows, and an untyped event says nothing usable.
            EventKind::Access(_) | EventKind::Any | EventKind::Other => {}
        }
        out
    }
}
