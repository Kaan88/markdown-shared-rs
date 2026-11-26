//! The tests of the watcher's edge: waking, the drop that must not wait, the thread's stop, and
//! one real folder on Windows.

use super::*;
use crate::dirlist::Kind;
#[cfg(windows)]
use crate::dirlist::{Listing, read};
use std::time::Duration;

/// The folder the made-up tests watch.
fn folder() -> PathBuf {
    PathBuf::from("/watched")
}

/// A path directly inside the watched folder.
fn p(name: &str) -> PathBuf {
    folder().join(name)
}

// The wake contract shared with the application.

/// A folder that is still the same folder is not lost: the events were confused, so re-read.
#[test]
fn a_folder_that_is_still_there_is_not_lost() {
    let dir = std::env::temp_dir().join(format!("mdshared-dirwatch-still-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let started = identity(&dir);
    assert_eq!(lost_signal(&dir, started), Signal::Rescan);
    _ = std::fs::remove_dir_all(&dir);
}

/// A folder that is gone is lost.
#[test]
fn a_folder_that_is_gone_is_lost() {
    let dir = std::env::temp_dir().join(format!("mdshared-dirwatch-gone-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&dir);
    assert_eq!(lost_signal(&dir, None), Signal::Lost);
}

/// A folder made again under the same name is not the folder that was watched, so it is lost.
#[cfg(unix)]
#[test]
fn a_folder_made_again_under_the_same_name_is_lost() {
    let dir = std::env::temp_dir().join(format!("mdshared-dirwatch-again-{}", std::process::id()));
    let other =
        std::env::temp_dir().join(format!("mdshared-dirwatch-again-o-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&dir);
    _ = std::fs::remove_dir_all(&other);
    std::fs::create_dir_all(&dir).unwrap();
    let started = identity(&dir);
    std::fs::rename(&dir, &other).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(lost_signal(&dir, started), Signal::Lost);
    _ = std::fs::remove_dir_all(&dir);
    _ = std::fs::remove_dir_all(&other);
}

#[test]
fn a_burst_of_reports_wakes_once_until_woken() {
    let wakes = AtomicBool::new(false);
    let shared = Shared::default();
    let wake = || wakes.store(true, Ordering::Release);
    shared.deliver(vec![Signal::Rescan], &wake);
    assert!(
        wakes.swap(false, Ordering::AcqRel),
        "the first report wakes"
    );
    shared.deliver(vec![Signal::Rescan], &wake);
    assert!(
        !wakes.load(Ordering::Acquire),
        "no second wake before woken"
    );
    assert_eq!(shared.take(), vec![Signal::Rescan, Signal::Rescan]);
    shared.woken();
    shared.deliver(vec![Signal::Rescan], &wake);
    assert!(
        wakes.swap(false, Ordering::AcqRel),
        "after woken it wakes again"
    );
    assert_eq!(shared.take(), vec![Signal::Rescan]);
}

#[test]
fn an_empty_report_does_not_wake() {
    let wakes = AtomicBool::new(false);
    let shared = Shared::default();
    let wake = || wakes.store(true, Ordering::Release);
    shared.deliver(Vec::new(), &wake);
    assert!(!wakes.load(Ordering::Acquire));
    assert!(shared.take().is_empty());
}

/// An entry for `path`, as `Entry::at` would read it.
fn entry_at(path: PathBuf) -> Entry {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    Entry {
        path,
        name,
        kind: Kind::File,
        size: 1,
        modified: None,
        created: None,
        hidden: false,
    }
}

/// Abandons the watcher while the reader is blocked, and measures the drop.
#[test]
fn dropping_the_watcher_does_not_wait_for_a_slow_read() {
    let dir = std::env::temp_dir().join(format!("mdshared-dirwatch-drop-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();

    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let release = std::sync::Mutex::new(release_rx);
    let read: Arc<Read> = Arc::new(move |path: &Path| {
        started_tx.send(()).ok();
        release.lock().unwrap().recv().ok();
        Some(entry_at(path.to_path_buf()))
    });
    let (wake_tx, wake_rx) = std::sync::mpsc::sync_channel(4);
    let watcher = DirWatcher::start(
        dir.clone(),
        Duration::from_millis(20),
        move || {
            let _ = wake_tx.try_send(());
        },
        read,
    )
    .expect("the watcher starts");

    std::fs::write(dir.join("a.txt"), "x").unwrap();
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the report is being read");

    // Release the read a little after the drop, from another thread, so a drop that joins
    // would be measured waiting for it.
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        release_tx.send(()).ok();
    });
    let before = Instant::now();
    drop(watcher);
    let waited = before.elapsed();
    release.join().ok();

    assert!(
        waited < Duration::from_millis(300),
        "the drop waited {waited:?} for the read"
    );
    assert!(
        wake_rx.try_recv().is_err(),
        "nothing is reported after the watcher is dropped"
    );
    _ = std::fs::remove_dir_all(&dir);
}

/// The thread reads no path after the watcher is dropped, even when a report had several.
#[test]
fn a_stopped_watcher_reads_no_more_paths() {
    let at = Instant::now();
    let inbox = Arc::new(Inbox::default());
    let shared = Arc::new(Shared::default());
    inbox.push(RawChange::Created(p("a.txt")), at);
    inbox.push(
        RawChange::Created(p("b.txt")),
        at + Duration::from_millis(1),
    );
    let coalescer = Coalescer::new(folder(), Duration::from_millis(10));

    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&reads);
    let release = std::sync::Mutex::new(release_rx);
    let read: Arc<Read> = Arc::new(move |path: &Path| {
        counted.fetch_add(1, Ordering::AcqRel);
        started_tx.send(()).ok();
        release.lock().unwrap().recv().ok();
        Some(entry_at(path.to_path_buf()))
    });
    let wake = || {};
    let for_work = Arc::clone(&inbox);
    let for_report = Arc::clone(&shared);
    std::thread::spawn(move || run(for_work, for_report, coalescer, &wake, read, folder(), None));

    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the first read starts");
    inbox.stop();
    release_tx.send(()).ok();
    std::thread::sleep(Duration::from_millis(200));

    assert_eq!(
        reads.load(Ordering::Acquire),
        1,
        "the second path was not read"
    );
    assert!(shared.take().is_empty(), "nothing was reported");
}

// The watcher on a real folder. Windows only: the other systems have their own rename
// machinery and are type-checked, not run, from the machine this was written on.

/// A folder of its own under the temporary folder, removed when dropped.
#[cfg(windows)]
struct Dir(PathBuf);

#[cfg(windows)]
impl Dir {
    fn new(name: &str) -> Dir {
        let dir =
            std::env::temp_dir().join(format!("mdshared-dirwatch-{name}-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }
}

#[cfg(windows)]
impl Drop for Dir {
    fn drop(&mut self) {
        _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The shown names of a list, in its order.
#[cfg(windows)]
fn names(listing: &Listing) -> Vec<String> {
    listing.visible().map(|entry| entry.name.clone()).collect()
}

#[cfg(windows)]
#[test]
fn a_folder_on_disk_reports_creates_renames_and_removes_that_apply_to_the_listing() {
    use std::sync::mpsc::channel;

    let dir = Dir::new("disk");
    std::fs::write(dir.0.join("keep.txt"), "keep").unwrap();
    std::fs::write(dir.0.join("gone.txt"), "gone").unwrap();
    std::fs::write(dir.0.join("moving.txt"), "moving").unwrap();

    let (send, receive) = channel();
    let watcher = DirWatcher::with_debounce(dir.0.clone(), Duration::from_millis(40), move || {
        let _ = send.send(());
    })
    .expect("the watcher starts");

    let mut listing = Listing::new();
    listing.set_folder(Some(dir.0.clone()));
    listing.set_entries(read(&dir.0, &|| false));

    // Let anything the watch saw before this point drain away.
    let settle_end = std::time::Instant::now() + Duration::from_millis(300);
    while std::time::Instant::now() < settle_end {
        watcher.woken();
        _ = watcher.take();
        std::thread::sleep(Duration::from_millis(10));
    }

    std::fs::write(dir.0.join("made.txt"), "made").unwrap();
    std::fs::rename(dir.0.join("moving.txt"), dir.0.join("moved.txt")).unwrap();
    std::fs::rename(dir.0.join("keep.txt"), dir.0.join("kept.txt")).unwrap();
    std::fs::remove_file(dir.0.join("gone.txt")).unwrap();

    let mut changes = 0;
    let mut saw_create = false;
    let mut saw_rename = false;
    let mut saw_remove = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut quiet_since = std::time::Instant::now();
    while std::time::Instant::now() < deadline {
        watcher.woken();
        let signals = watcher.take();
        if signals.is_empty() {
            if quiet_since.elapsed() > Duration::from_millis(600) {
                break;
            }
            _ = receive.recv_timeout(Duration::from_millis(25));
            continue;
        }
        quiet_since = std::time::Instant::now();
        for signal in signals {
            match signal {
                Signal::Changed(change) => {
                    match &change {
                        Change::Created(_) => saw_create = true,
                        Change::Removed(_) => saw_remove = true,
                        Change::Renamed { .. } => saw_rename = true,
                        Change::Modified(_) => {}
                    }
                    listing.apply(change);
                    changes += 1;
                }
                Signal::Rescan => panic!("a small burst of changes is not an overflow"),
                Signal::Lost => panic!("the folder is still there"),
            }
        }
    }

    assert!(changes >= 3, "the report arrived: {changes} changes");
    assert!(saw_create, "a created file was reported");
    assert!(saw_rename, "a rename was paired");
    assert!(saw_remove, "a removal was reported");

    let mut fresh = Listing::new();
    fresh.set_folder(Some(dir.0.clone()));
    fresh.set_entries(read(&dir.0, &|| false));
    let applied: Vec<Entry> = listing.visible().cloned().collect();
    let reread: Vec<Entry> = fresh.visible().cloned().collect();
    assert_eq!(
        applied, reread,
        "applying the report gives the same list as reading the folder again"
    );
    assert_eq!(
        names(&listing),
        vec![
            "kept.txt".to_string(),
            "made.txt".to_string(),
            "moved.txt".to_string()
        ]
    );

    drop(watcher);
}
