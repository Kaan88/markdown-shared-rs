//! Watches the folders that hold the open files, so a change on disk reaches the application without a
//! poll. Parent folders rather than the files themselves, because editors save by writing a new
//! file and renaming it over the old one, which ends a watch on the file itself.
//!
//! The watcher thread never touches the UI: it calls `wake`, and the shell checks the files on its
//! own thread. Wakes are coalesced, so a burst of events costs one check.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

/// Shared by the watcher thread and the UI thread.
struct Shared {
    /// The open files' folders and names, compared against each event's paths.
    files: std::sync::Mutex<BTreeSet<PathBuf>>,
    /// The folders whose listing is followed.
    listed: std::sync::Mutex<BTreeSet<PathBuf>>,
    /// A wake has been sent and the shell has not called `woken` yet.
    pending: AtomicBool,
}

pub struct FileWatcher {
    watcher: RecommendedWatcher,
    shared: Arc<Shared>,
    /// Every folder the OS watcher follows now.
    folders: BTreeSet<PathBuf>,
    /// The parents of the followed files.
    file_folders: BTreeSet<PathBuf>,
    /// The folders whose listing is followed.
    listed: BTreeSet<PathBuf>,
}

impl FileWatcher {
    /// `None` when the OS watcher cannot start (no inotify instances left, for example); the shell
    /// then falls back to a slow poll.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Option<FileWatcher> {
        let shared = Arc::new(Shared {
            files: Default::default(),
            listed: Default::default(),
            pending: AtomicBool::new(false),
        });
        let seen = shared.clone();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else {
                return;
            };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            let ours = {
                let files = seen.files.lock().unwrap_or_else(|e| e.into_inner());
                event.paths.iter().any(|path| files.contains(path))
            } || {
                // Anything created, removed or renamed directly inside a listed folder.
                let listed = seen.listed.lock().unwrap_or_else(|e| e.into_inner());
                !listed.is_empty()
                    && matches!(
                        event.kind,
                        EventKind::Create(_)
                            | EventKind::Remove(_)
                            | EventKind::Modify(notify::event::ModifyKind::Name(_))
                    )
                    && event
                        .paths
                        .iter()
                        .any(|path| path.parent().is_some_and(|dir| listed.contains(dir)))
            };
            if ours && !seen.pending.swap(true, Ordering::AcqRel) {
                wake();
            }
        })
        .ok()?;
        Some(FileWatcher {
            watcher,
            shared,
            folders: BTreeSet::new(),
            file_folders: BTreeSet::new(),
            listed: BTreeSet::new(),
        })
    }

    /// Follows exactly these files from now on. Folders no longer needed are released.
    pub fn set_files<'a>(&mut self, paths: impl IntoIterator<Item = &'a Path>) {
        let files: BTreeSet<PathBuf> = paths.into_iter().map(Path::to_path_buf).collect();
        self.file_folders = files
            .iter()
            .filter_map(|p| p.parent().map(Path::to_path_buf))
            .collect();
        self.rewatch();
        *self.shared.files.lock().unwrap_or_else(|e| e.into_inner()) = files;
    }

    /// Follows the listing of exactly these folders from now on (a folder tree's root and its
    /// expanded folders): an entry created, removed or renamed directly inside one wakes. A
    /// change to the contents of a file there does not, unless the file is one of
    /// [`set_files`](Self::set_files).
    pub fn set_folders<'a>(&mut self, folders: impl IntoIterator<Item = &'a Path>) {
        self.listed = folders.into_iter().map(Path::to_path_buf).collect();
        self.rewatch();
        *self.shared.listed.lock().unwrap_or_else(|e| e.into_inner()) = self.listed.clone();
    }

    /// Watches the folders the files and the listing need, and releases the rest.
    fn rewatch(&mut self) {
        let wanted: BTreeSet<PathBuf> = self.file_folders.union(&self.listed).cloned().collect();
        for gone in self.folders.difference(&wanted) {
            _ = self.watcher.unwatch(gone);
        }
        let mut watched = BTreeSet::new();
        for folder in &wanted {
            if self.folders.contains(folder)
                || self
                    .watcher
                    .watch(folder, RecursiveMode::NonRecursive)
                    .is_ok()
            {
                watched.insert(folder.clone());
            }
        }
        self.folders = watched;
    }

    /// The shell calls this before it checks the files, so a change during the check wakes again.
    pub fn woken(&self) {
        self.shared.pending.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    #[test]
    fn a_save_by_rename_wakes_and_other_files_do_not() {
        let dir = std::env::temp_dir().join(format!("mdshared-watch-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let file = dir.join("a.md");
        std::fs::write(&file, "# a\n").unwrap();
        let (send, receive) = channel();
        let mut watcher = FileWatcher::new(move || _ = send.send(())).expect("watcher");
        watcher.set_files([file.as_path()]);

        std::fs::write(dir.join("other.md"), "x").unwrap();
        assert!(
            receive.recv_timeout(Duration::from_millis(300)).is_err(),
            "a file that is not open does not wake"
        );

        // An editor's atomic save: write a temporary file, then rename it over the open one.
        let temp = dir.join(".a.md.tmp");
        std::fs::write(&temp, "# b\n").unwrap();
        std::fs::rename(&temp, &file).unwrap();
        receive
            .recv_timeout(Duration::from_secs(2))
            .expect("the rename wakes");
        // Until the shell says it has looked, further events do not wake again.
        std::fs::write(&file, "# c\n").unwrap();
        assert!(receive.recv_timeout(Duration::from_millis(300)).is_err());
        watcher.woken();
        std::fs::remove_file(&file).unwrap();
        receive
            .recv_timeout(Duration::from_secs(2))
            .expect("a delete wakes");

        watcher.woken();
        watcher.set_files([]);
        std::fs::write(&file, "# d\n").unwrap();
        assert!(
            receive.recv_timeout(Duration::from_millis(300)).is_err(),
            "nothing is watched after set_files([])"
        );
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_listed_folder_wakes_for_new_and_removed_entries_only() {
        let dir = std::env::temp_dir().join(format!("mdshared-watch-list-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let dir = dir.canonicalize().unwrap();
        let old = dir.join("old.md");
        std::fs::write(
            &old, "# old
",
        )
        .unwrap();
        let (send, receive) = channel();
        let mut watcher = FileWatcher::new(move || _ = send.send(())).expect("watcher");
        watcher.set_folders([dir.as_path()]);

        std::fs::write(
            &old,
            "# changed
",
        )
        .unwrap();
        std::fs::write(dir.join("sub").join("deep.md"), "x").unwrap();
        assert!(
            receive.recv_timeout(Duration::from_millis(300)).is_err(),
            "contents, and entries of a folder that is not listed, do not wake"
        );

        std::fs::write(
            dir.join("new.md"),
            "# new
",
        )
        .unwrap();
        receive
            .recv_timeout(Duration::from_secs(2))
            .expect("a new entry wakes");
        std::thread::sleep(Duration::from_millis(200));
        while receive.try_recv().is_ok() {}
        watcher.woken();
        std::fs::rename(dir.join("new.md"), dir.join("renamed.md")).unwrap();
        receive
            .recv_timeout(Duration::from_secs(2))
            .expect("a rename wakes");
        std::thread::sleep(Duration::from_millis(200));
        watcher.woken();
        std::fs::remove_file(&old).unwrap();
        receive
            .recv_timeout(Duration::from_secs(2))
            .expect("a removal wakes");

        std::thread::sleep(Duration::from_millis(200));
        while receive.try_recv().is_ok() {}
        watcher.woken();
        watcher.set_folders([]);
        std::fs::write(dir.join("later.md"), "x").unwrap();
        assert!(
            receive.recv_timeout(Duration::from_millis(300)).is_err(),
            "nothing is listed after set_folders([])"
        );
        _ = std::fs::remove_dir_all(&dir);
    }
}
