//! The tests of the watcher's edge: waking, the drop that must not wait, the thread's stop, and
//! one real folder on Windows.

use super::queues::LIMIT;
#[cfg(unix)]
use super::resolve::identity;
use super::worker::{Worker, feed_batch};
use super::*;
use crate::dirlist::Kind;
#[cfg(windows)]
use crate::dirlist::{Listing, read};
use std::sync::atomic::{AtomicBool, Ordering};
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
    let dir = Dir::new("still");
    #[cfg(unix)]
    let started = identity(&dir.0);
    #[cfg(not(unix))]
    let started = None;
    assert_eq!(lost_signal(&dir.0, started), Signal::Rescan);
}

/// A folder that is gone is lost.
#[test]
fn a_folder_that_is_gone_is_lost() {
    let dir = Dir::new("gone");
    assert_eq!(lost_signal(&dir.0.join("absent"), None), Signal::Lost);
}

/// A folder made again under the same name is not the folder that was watched, so it is lost.
#[cfg(unix)]
#[test]
fn a_folder_made_again_under_the_same_name_is_lost() {
    let sandbox = Dir::new("again");
    let dir = sandbox.0.join("watched");
    let other = sandbox.0.join("old");
    std::fs::create_dir(&dir).unwrap();
    let started = identity(&dir);
    std::fs::rename(&dir, &other).unwrap();
    std::fs::create_dir(&dir).unwrap();
    assert_eq!(lost_signal(&dir, started), Signal::Lost);
}

#[cfg(unix)]
#[test]
fn losing_a_watched_symlinks_target_loses_the_watch() {
    let dir = Dir::new("link-target");
    let target = dir.0.join("target");
    std::fs::create_dir(&target).unwrap();
    let link = dir.0.join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let started = identity(&link);
    assert_eq!(started, identity(&target));
    assert!(started.is_some());
    std::fs::remove_dir(&target).unwrap();
    assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
    assert_eq!(lost_signal(&link, started), Signal::Lost);
}

#[cfg(unix)]
#[test]
fn a_root_replaced_during_registration_is_rejected() {
    let dir = Dir::new("register-replaced");
    let root = dir.0.join("watched");
    std::fs::create_dir(&root).unwrap();
    let result = register(&root, || {
        std::fs::rename(&root, dir.0.join("old")).unwrap();
        std::fs::create_dir(&root).unwrap();
        Ok(())
    });
    assert!(result.is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn inotify_reports_loss_when_the_target_of_a_watched_link_is_deleted() {
    let dir = Dir::new("inotify-link-target");
    let target = dir.0.join("target");
    let link = dir.0.join("link");
    std::fs::create_dir(&target).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let watcher = DirWatcher::new(&link, move || {
        send.send(()).ok();
    })
    .unwrap();
    std::fs::remove_dir(&target).unwrap();
    receive.recv_timeout(Duration::from_secs(5)).unwrap();
    watcher.woken();
    assert_eq!(watcher.take(), [Signal::Lost]);
}

#[test]
fn a_burst_of_reports_wakes_once_until_woken() {
    let wakes = AtomicBool::new(false);
    let shared = Shared::default();
    let wake = || wakes.store(true, Ordering::Release);
    let a = Signal::Changed(Change::Removed(p("a")));
    let b = Signal::Changed(Change::Removed(p("b")));
    shared.deliver(vec![a.clone()], &wake);
    assert!(
        wakes.swap(false, Ordering::AcqRel),
        "the first report wakes"
    );
    shared.deliver(vec![b.clone()], &wake);
    assert!(
        !wakes.load(Ordering::Acquire),
        "no second wake before woken"
    );
    assert_eq!(shared.take(), vec![a, b]);
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

#[test]
fn raw_overflow_collapses_the_backlog_without_hiding_folder_loss() {
    let inbox = Inbox::default();
    let at = Instant::now();
    for i in 0..5_000 {
        inbox.push(RawChange::Modified(p(&format!("{i}"))), at, None);
    }
    assert_eq!(
        inbox.wait_for_batch(None).unwrap(),
        [(RawChange::Rescan, at, None)]
    );
    inbox.push(RawChange::Rescan, at, None);
    inbox.push(RawChange::Lost, at, None);
    inbox.push(RawChange::Created(p("late")), at, None);
    assert_eq!(
        inbox.wait_for_batch(None).unwrap(),
        [(RawChange::Lost, at, None)]
    );
}

#[test]
fn a_burst_cannot_grow_without_bound_across_detached_batches() {
    let at = Instant::now();
    let mut c = Coalescer::new(folder(), Duration::ZERO);
    let mut seen = 0;
    for batch in 0..2 {
        let events = (0..LIMIT / 2 + 1)
            .map(|i| (RawChange::Modified(p(&format!("{batch}-{i}"))), at, None))
            .collect();
        feed_batch(&mut c, events, &mut seen);
    }
    assert_eq!(c.flush(at), vec![Verdict::Rescan]);
}

#[test]
fn a_stalled_ui_gets_one_rescan_or_a_lost_watch_instead_of_a_growing_queue() {
    let shared = Shared::default();
    let wake = || {};
    for round in 0..2 {
        for i in 0..5_000 {
            shared.deliver(
                vec![Signal::Changed(Change::Removed(p(&format!("{i}"))))],
                &wake,
            );
        }
        if round == 1 {
            shared.deliver(vec![Signal::Lost], &wake);
            shared.deliver(vec![Signal::Rescan], &wake);
        }
        assert_eq!(
            shared.take(),
            vec![if round == 0 {
                Signal::Rescan
            } else {
                Signal::Lost
            }]
        );
        shared.woken();
    }
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

/// Abandons the watcher while the reader is blocked, before opening the gate.
#[test]
fn dropping_the_watcher_does_not_wait_for_a_slow_read() {
    let sandbox = Dir::new("drop");
    let dir = sandbox.0.canonicalize().unwrap();

    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel::<()>(1);
    let release = std::sync::Mutex::new(release_rx);
    let read: Arc<Read> = Arc::new(move |path: &Path| {
        started_tx.send(()).ok();
        release.lock().unwrap().recv().ok();
        Ok(entry_at(path.to_path_buf()))
    });
    let (wake_tx, wake_rx) = std::sync::mpsc::sync_channel(4);
    let watcher = DirWatcher::start(
        dir.clone(),
        notify_root(&dir).unwrap(),
        Duration::from_millis(20),
        move || {
            let _ = wake_tx.try_send(());
        },
        read,
        None,
    )
    .expect("the watcher starts");

    watcher
        .inbox
        .push(RawChange::Created(dir.join("a.txt")), Instant::now(), None);
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the report is being read");

    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let dropping = std::thread::spawn(move || {
        drop(watcher);
        done_tx.send(()).ok();
    });
    let done = done_rx.recv_timeout(Duration::from_secs(5));
    drop(release_tx);
    done.expect("dropping returns while the read is still blocked");
    dropping.join().unwrap();
    assert_eq!(
        wake_rx.recv_timeout(Duration::from_secs(5)),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected),
        "the worker exits without waking"
    );
}

#[test]
fn dropping_the_watcher_returns_before_native_teardown_can_finish() {
    use std::sync::mpsc::{Receiver, Sender, channel};

    struct GatedBackend(Sender<()>, Receiver<()>);
    impl Drop for GatedBackend {
        fn drop(&mut self) {
            self.0.send(()).ok();
            self.1.recv().ok();
        }
    }

    let (started, teardown) = channel();
    let (release, gate) = channel();
    let (mut backend, (), _permit) =
        backend::start(move || Ok((GatedBackend(started, gate), ())), |_| Ok(())).unwrap();
    let owner_thread = backend.thread.take().unwrap();
    let inbox = Arc::new(Inbox::default());
    let watcher = DirWatcher {
        folder: folder(),
        _backend: backend,
        inbox: Arc::clone(&inbox),
        shared: Arc::new(Shared::default()),
    };
    let (done, dropped) = channel();
    let dropping = std::thread::spawn(move || {
        drop(watcher);
        done.send(()).ok();
    });
    teardown.recv_timeout(Duration::from_secs(5)).unwrap();
    let result = dropped.recv_timeout(Duration::from_secs(5));
    drop(release);
    result.expect("dropping returns without waiting for native teardown");
    assert!(inbox.stopped());
    dropping.join().unwrap();
    owner_thread.join().unwrap();
}

#[test]
fn dropping_a_watcher_with_a_stalled_scan_keeps_the_slot_until_both_threads_exit() {
    use std::sync::mpsc::channel;

    static SLOTS: backend::Slots = backend::Slots::new(1);
    let start = || backend::start_with_slots(&SLOTS, || Ok(((), ())), |_| Ok(()));
    let (mut owner, (), permit) = start().unwrap();
    let restart = owner.restart_handle();
    let owner_thread = owner.thread.take().unwrap();
    let inbox = Arc::new(Inbox::default());
    let shared = Arc::new(Shared::default());
    let now = Instant::now();
    let mut periodic = Periodic::new(Duration::from_secs(1), now, None).unwrap();
    periodic.make_due(now);
    let mut worker = Worker {
        inbox: Arc::clone(&inbox),
        shared: Arc::clone(&shared),
        coalescer: Coalescer::new(folder(), DEBOUNCE),
        read: Arc::new(|_| panic!("no incremental events")),
        root: folder(),
        started: None,
        periodic: Some(periodic),
    };
    let watcher = DirWatcher {
        folder: folder(),
        _backend: owner,
        inbox,
        shared,
    };
    let (entered, entering) = channel();
    let (release, gate) = channel::<()>();
    let for_ack = Arc::clone(&worker.inbox);
    let worker_thread = permit
        .spawn(move || {
            worker.run_with_scan(
                &|| panic!("a stopped watcher cannot report"),
                &|| {
                    let for_ack = Arc::clone(&for_ack);
                    restart.request(move |result| for_ack.restart_finished(result.is_ok()));
                },
                &|_| {
                    entered.send(()).unwrap();
                    _ = gate.recv();
                    Ok(Some(Default::default()))
                },
            );
        })
        .unwrap();

    entering.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(watcher);
    owner_thread.join().unwrap();
    assert_eq!(start().err().unwrap().kind(), io::ErrorKind::WouldBlock);
    drop(release);
    worker_thread.join().unwrap();
    assert!(start().is_ok());
}

/// The thread reads no path after the watcher is dropped, even when a report had several.
#[test]
fn a_stopped_watcher_reads_no_more_paths() {
    let at = Instant::now();
    let inbox = Arc::new(Inbox::default());
    let shared = Arc::new(Shared::default());
    inbox.push(RawChange::Created(p("a.txt")), at, None);
    inbox.push(
        RawChange::Created(p("b.txt")),
        at + Duration::from_millis(1),
        None,
    );
    let coalescer = Coalescer::new(folder(), Duration::from_millis(10));

    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel::<()>(1);
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&reads);
    let release = std::sync::Mutex::new(release_rx);
    let read: Arc<Read> = Arc::new(move |path: &Path| {
        counted.fetch_add(1, Ordering::AcqRel);
        started_tx.send(()).ok();
        release.lock().unwrap().recv().ok();
        Ok(entry_at(path.to_path_buf()))
    });
    let wake = || {};
    let for_work = Arc::clone(&inbox);
    let for_report = Arc::clone(&shared);
    let (done, exited) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        Worker {
            inbox: for_work,
            shared: for_report,
            coalescer,
            read,
            root: folder(),
            started: None,
            periodic: None,
        }
        .run(&wake, || {});
        done.send(()).ok();
    });

    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the first read starts");
    inbox.stop();
    drop(release_tx);
    exited.recv_timeout(Duration::from_secs(5)).unwrap();
    worker.join().unwrap();

    assert_eq!(
        reads.load(Ordering::Acquire),
        1,
        "the second path was not read"
    );
    assert!(shared.take().is_empty(), "nothing was reported");
}

/// A folder of its own under the temporary folder, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        Self::new_in(&std::env::temp_dir(), name)
    }

    fn new_in(parent: &Path, name: &str) -> Dir {
        let base = format!("mdshared-dirwatch-{name}-{}", std::process::id());
        for attempt in 0..1024 {
            let dir = if attempt == 0 {
                parent.join(&base)
            } else {
                parent.join(format!("{base}-{attempt}"))
            };
            match std::fs::create_dir(&dir) {
                Ok(()) => return Dir(dir),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("cannot claim {}: {e}", dir.display()),
            }
        }
        panic!("no unused temporary directory for {base}");
    }
}

#[test]
fn a_temp_directory_collision_leaves_the_existing_contents_alone() {
    let parent = Dir::new("collision-sandbox");
    let occupied = parent
        .0
        .join(format!("mdshared-dirwatch-probe-{}", std::process::id()));
    std::fs::create_dir(&occupied).unwrap();
    std::fs::write(occupied.join("keep"), b"untouched").unwrap();
    let claimed = Dir::new_in(&parent.0, "probe");
    assert_ne!(claimed.0, occupied);
    drop(claimed);
    assert_eq!(std::fs::read(occupied.join("keep")).unwrap(), b"untouched");
}

impl Drop for Dir {
    fn drop(&mut self) {
        _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_relative_report_reads_metadata_under_the_captured_root() {
    let dir = Dir::new("absolute-reader");
    std::fs::write(dir.0.join("a.txt"), "contents").unwrap();
    let relative = PathBuf::from("not-the-watch-root").join("a.txt");
    let entry = read_from_root(&dir.0, &relative).unwrap();
    assert_eq!(entry.path, relative);
    assert_eq!(entry.size, 8);
}

#[test]
fn a_silent_backend_still_recovers_the_listing_and_gets_a_restart_request() {
    let dir = Dir::new("silent-backend");
    std::fs::write(dir.0.join("arrived"), "new").unwrap();
    let inbox = Arc::new(Inbox::default());
    let shared = Arc::new(Shared::default());
    let (restart, restarted) = std::sync::mpsc::channel();
    let (notify, notified) = std::sync::mpsc::channel();
    let interval = Duration::from_secs(3_600);
    let now = Instant::now();
    let mut periodic = Periodic::new(interval, now, Some(vec![entry_at(p("gone"))])).unwrap();
    periodic.make_due(now);
    let worker = Worker {
        inbox: Arc::clone(&inbox),
        shared: Arc::clone(&shared),
        coalescer: Coalescer::new(folder(), DEBOUNCE),
        read: Arc::new(|_| panic!("the fake backend sent no incremental events")),
        root: dir.0.clone(),
        started: None,
        periodic: Some(periodic),
    };
    let for_ack = Arc::clone(&inbox);
    let thread = std::thread::spawn(move || {
        worker.run(
            &|| {
                _ = notify.send(());
            },
            || {
                _ = restart.send(());
                for_ack.restart_finished(true);
            },
        )
    });
    let wake = notified.recv_timeout(Duration::from_secs(5));
    inbox.stop();
    thread.join().unwrap();
    wake.unwrap();
    restarted.try_recv().unwrap();
    let mut listing = crate::dirlist::Listing::new();
    listing.set_folder(Some(folder()));
    listing.set_entries(vec![entry_at(p("gone"))]);
    let signals = shared.take();
    assert_eq!(signals.len(), 2);
    for signal in signals {
        let Signal::Changed(change) = signal else {
            panic!("expected a list change, got {signal:?}")
        };
        listing.apply(change);
    }
    assert_eq!(
        listing
            .visible()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>(),
        [p("arrived")]
    );
}

#[test]
fn reconcile_builder_defaults_and_explicit_disable_are_platform_appropriate() {
    let builder = DirWatcher::builder(folder());
    assert_eq!(
        builder.interval,
        cfg!(windows).then(|| Duration::from_secs(30))
    );
    assert!(
        builder
            .reconcile_interval(Some(Duration::ZERO))
            .interval
            .is_none()
    );
    assert!(
        DirWatcher::builder(folder())
            .reconcile_interval(None)
            .interval
            .is_none()
    );
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

    std::fs::write(dir.0.join("made.txt"), "made").unwrap();
    std::fs::rename(dir.0.join("moving.txt"), dir.0.join("moved.txt")).unwrap();
    std::fs::rename(dir.0.join("keep.txt"), dir.0.join("kept.txt")).unwrap();
    std::fs::remove_file(dir.0.join("gone.txt")).unwrap();

    let mut changes = 0;
    let mut saw_create = false;
    let mut saw_rename = false;
    let mut saw_remove = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let expected = ["kept.txt", "made.txt", "moved.txt"];
    loop {
        watcher.woken();
        let signals = watcher.take();
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
        let remaining = deadline.saturating_duration_since(Instant::now());
        if (saw_create && saw_rename && saw_remove && names(&listing) == expected)
            || remaining.is_zero()
            || receive.recv_timeout(remaining).is_err()
        {
            break;
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
    assert_eq!(names(&listing), expected);

    drop(watcher);
}
