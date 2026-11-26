//! The tests of [`super`], the coalescer and the watcher that reports what changed in one folder.

use super::*;
use crate::dirlist::Kind;
#[cfg(windows)]
use crate::dirlist::{Listing, read};
use notify::event::{CreateKind, Flag, RemoveKind};

/// The folder every pure test watches.
fn folder() -> PathBuf {
    PathBuf::from("/watched")
}

/// A path directly inside the watched folder.
fn p(name: &str) -> PathBuf {
    folder().join(name)
}

/// A path outside the watched folder.
fn elsewhere(name: &str) -> PathBuf {
    PathBuf::from("/other").join(name)
}

/// A coalescer with a 50 ms quiet period.
fn coalescer() -> Coalescer {
    Coalescer::new(folder(), Duration::from_millis(50))
}

/// Feeds `change` as if it was seen `ms` after `at`.
fn push(coalescer: &mut Coalescer, at: Instant, ms: u64, change: RawChange) {
    coalescer.push(change, at + Duration::from_millis(ms));
}

/// Flushes as if `ms` had passed since `at`.
fn settle(coalescer: &mut Coalescer, at: Instant, ms: u64) -> Vec<Verdict> {
    coalescer.flush(at + Duration::from_millis(ms))
}

#[test]
fn a_create_followed_by_modifies_is_one_creation() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::Created(p("a.txt")));
    push(&mut c, t, 10, RawChange::Modified(p("a.txt")));
    push(&mut c, t, 20, RawChange::Modified(p("a.txt")));
    assert_eq!(settle(&mut c, t, 1_000), vec![Verdict::Created(p("a.txt"))]);
}

#[test]
fn a_create_followed_by_a_remove_is_nothing() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::Created(p("a.txt")));
    push(&mut c, t, 10, RawChange::Removed(p("a.txt")));
    assert_eq!(settle(&mut c, t, 1_000), Vec::new());
    assert!(c.is_idle(), "nothing is left pending");
    assert_eq!(c.deadline(), None, "and nothing more will come");
}

#[test]
fn a_remove_followed_by_a_create_is_a_modification() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::Removed(p("a.txt")));
    push(&mut c, t, 10, RawChange::Created(p("a.txt")));
    assert_eq!(
        settle(&mut c, t, 1_000),
        vec![Verdict::Modified(p("a.txt"))]
    );
}

#[test]
fn a_remove_followed_by_a_modification_is_a_modification() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::Removed(p("a.txt")));
    push(&mut c, t, 10, RawChange::Modified(p("a.txt")));
    assert_eq!(
        settle(&mut c, t, 1_000),
        vec![Verdict::Modified(p("a.txt"))]
    );
}

#[test]
fn a_rename_chain_is_one_rename_from_the_first_name_to_the_last() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::RenamedFrom(p("a")));
    push(&mut c, t, 1, RawChange::RenamedTo(p("b")));
    push(&mut c, t, 2, RawChange::RenamedFrom(p("b")));
    push(&mut c, t, 3, RawChange::RenamedTo(p("c")));
    assert_eq!(
        settle(&mut c, t, 1_000),
        vec![Verdict::Renamed {
            from: p("a"),
            to: p("c")
        }]
    );
}

#[test]
fn a_burst_of_modifications_is_one_modification() {
    let t = Instant::now();
    let mut c = coalescer();
    for ms in 0..50 {
        push(&mut c, t, ms, RawChange::Modified(p("a.txt")));
    }
    assert_eq!(
        settle(&mut c, t, 1_000),
        vec![Verdict::Modified(p("a.txt"))]
    );
}

#[test]
fn an_overflow_gives_a_rescan_and_drops_what_was_pending() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::Created(p("a.txt")));
    push(&mut c, t, 1, RawChange::Rescan);
    // Events after the rescan are moot: the fresh read will find their outcome anyway.
    push(&mut c, t, 2, RawChange::Created(p("b.txt")));
    assert_eq!(settle(&mut c, t, 1_000), vec![Verdict::Rescan]);
    assert!(c.is_idle());
    assert_eq!(settle(&mut c, t, 2_000), Vec::new());
}

#[test]
fn an_overflow_with_nothing_else_still_gives_a_rescan() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::Rescan);
    assert_eq!(settle(&mut c, t, 1_000), vec![Verdict::Rescan]);
}

#[test]
fn changes_outside_the_folder_are_dropped() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::Created(elsewhere("a.txt")));
    push(&mut c, t, 1, RawChange::Removed(elsewhere("b.txt")));
    push(&mut c, t, 2, RawChange::Modified(elsewhere("c.txt")));
    push(&mut c, t, 3, RawChange::RenamedFrom(elsewhere("d")));
    push(&mut c, t, 4, RawChange::RenamedTo(elsewhere("e")));
    push(
        &mut c,
        t,
        5,
        RawChange::Renamed {
            from: elsewhere("f"),
            to: elsewhere("g"),
        },
    );
    assert_eq!(c.deadline(), None, "no dropped event opened a burst");
    assert_eq!(settle(&mut c, t, 1_000), Vec::new());
}

#[test]
fn the_folder_itself_is_not_one_of_its_entries() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::Created(folder()));
    assert_eq!(settle(&mut c, t, 100), Vec::new());
    push(&mut c, t, 200, RawChange::Modified(folder()));
    assert_eq!(settle(&mut c, t, 300), Vec::new());
    push(&mut c, t, 400, RawChange::Removed(folder()));
    assert_eq!(settle(&mut c, t, 500), Vec::new());
}

#[test]
fn a_rename_out_of_the_folder_is_a_removal() {
    let t = Instant::now();
    let mut c = coalescer();
    push(
        &mut c,
        t,
        0,
        RawChange::Renamed {
            from: p("a"),
            to: elsewhere("a"),
        },
    );
    assert_eq!(settle(&mut c, t, 1_000), vec![Verdict::Removed(p("a"))]);
}

#[test]
fn a_rename_into_the_folder_is_a_creation() {
    let t = Instant::now();
    let mut c = coalescer();
    push(
        &mut c,
        t,
        0,
        RawChange::Renamed {
            from: elsewhere("a"),
            to: p("a"),
        },
    );
    assert_eq!(settle(&mut c, t, 1_000), vec![Verdict::Created(p("a"))]);
}

#[test]
fn an_unpaired_old_name_is_a_removal_at_the_end_of_the_quiet_period() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::RenamedFrom(p("a")));
    assert_eq!(settle(&mut c, t, 1_000), vec![Verdict::Removed(p("a"))]);
}

#[test]
fn an_unpaired_new_name_is_a_creation() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::RenamedTo(p("b")));
    assert_eq!(settle(&mut c, t, 1_000), vec![Verdict::Created(p("b"))]);
}

#[test]
fn a_paired_rename_carries_both_names() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::RenamedFrom(p("a")));
    push(&mut c, t, 1, RawChange::RenamedTo(p("b")));
    assert_eq!(
        settle(&mut c, t, 1_000),
        vec![Verdict::Renamed {
            from: p("a"),
            to: p("b")
        }]
    );
}

#[test]
fn a_creation_that_is_renamed_is_a_creation_at_the_new_name() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::Created(p("a")));
    push(&mut c, t, 1, RawChange::RenamedFrom(p("a")));
    push(&mut c, t, 2, RawChange::RenamedTo(p("b")));
    assert_eq!(settle(&mut c, t, 1_000), vec![Verdict::Created(p("b"))]);
}

#[test]
fn a_modified_entry_that_is_renamed_keeps_its_first_name() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::Modified(p("a")));
    push(&mut c, t, 1, RawChange::RenamedFrom(p("a")));
    push(&mut c, t, 2, RawChange::RenamedTo(p("b")));
    assert_eq!(
        settle(&mut c, t, 1_000),
        vec![Verdict::Renamed {
            from: p("a"),
            to: p("b")
        }]
    );
}

#[test]
fn a_renamed_entry_that_is_removed_is_a_removal_of_its_first_name() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::RenamedFrom(p("a")));
    push(&mut c, t, 1, RawChange::RenamedTo(p("b")));
    push(&mut c, t, 2, RawChange::Removed(p("b")));
    assert_eq!(settle(&mut c, t, 1_000), vec![Verdict::Removed(p("a"))]);
}

#[test]
fn a_rename_reported_both_as_halves_and_as_a_pair_is_one_rename() {
    // Linux reports From, To and then a pair carrying both names.
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::RenamedFrom(p("a")));
    push(&mut c, t, 1, RawChange::RenamedTo(p("b")));
    push(
        &mut c,
        t,
        2,
        RawChange::Renamed {
            from: p("a"),
            to: p("b"),
        },
    );
    assert_eq!(
        settle(&mut c, t, 1_000),
        vec![Verdict::Renamed {
            from: p("a"),
            to: p("b")
        }]
    );
}

#[test]
fn a_rename_onto_a_name_that_was_moving_keeps_the_first_entries_removal() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::RenamedFrom(p("a")));
    push(&mut c, t, 1, RawChange::RenamedTo(p("b")));
    push(&mut c, t, 2, RawChange::RenamedFrom(p("c")));
    push(&mut c, t, 3, RawChange::RenamedTo(p("b")));
    assert_eq!(
        settle(&mut c, t, 1_000),
        vec![
            Verdict::Removed(p("a")),
            Verdict::Renamed {
                from: p("c"),
                to: p("b")
            }
        ]
    );
}

#[test]
fn a_name_created_after_it_was_renamed_away_is_a_creation_beside_the_rename() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::RenamedFrom(p("a")));
    push(&mut c, t, 1, RawChange::Created(p("a")));
    push(&mut c, t, 2, RawChange::RenamedTo(p("b")));
    assert_eq!(
        settle(&mut c, t, 1_000),
        vec![
            Verdict::Created(p("a")),
            Verdict::Renamed {
                from: p("a"),
                to: p("b")
            }
        ]
    );
}

#[test]
fn nothing_is_reported_before_the_quiet_period_ends() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::Created(p("a.txt")));
    push(&mut c, t, 10, RawChange::Modified(p("a.txt")));
    assert_eq!(c.deadline(), Some(t + Duration::from_millis(60)));
    assert_eq!(settle(&mut c, t, 59), Vec::new(), "still settling");
    assert_eq!(
        settle(&mut c, t, 60),
        vec![Verdict::Created(p("a.txt"))],
        "the deadline itself is late enough"
    );
    assert_eq!(c.deadline(), None);
}

#[test]
fn a_zero_quiet_period_settles_at_the_next_flush() {
    let t = Instant::now();
    let mut c = Coalescer::new(folder(), Duration::ZERO);
    push(&mut c, t, 0, RawChange::Modified(p("a.txt")));
    assert_eq!(c.deadline(), Some(t));
    assert_eq!(settle(&mut c, t, 0), vec![Verdict::Modified(p("a.txt"))]);
}

#[test]
fn a_report_names_each_path_once_and_lists_it_in_path_order() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::Modified(p("c.txt")));
    push(&mut c, t, 1, RawChange::Created(p("a.txt")));
    push(&mut c, t, 2, RawChange::Removed(p("b.txt")));
    assert_eq!(
        settle(&mut c, t, 1_000),
        vec![
            Verdict::Created(p("a.txt")),
            Verdict::Removed(p("b.txt")),
            Verdict::Modified(p("c.txt"))
        ]
    );
}

#[test]
fn a_long_random_sequence_settles_the_same_way_twice_and_never_panics() {
    let names = ["a", "b", "c", "d"];
    let mut seed = 0x5eed_u64;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) as usize
    };
    let mut events = Vec::new();
    for _ in 0..500 {
        let kind = next() % 7;
        let a = p(names[next() % names.len()]);
        let b = p(names[next() % names.len()]);
        events.push(match kind {
            0 => RawChange::Created(a),
            1 => RawChange::Removed(a),
            2 => RawChange::Modified(a),
            3 => RawChange::RenamedFrom(a),
            4 => RawChange::RenamedTo(a),
            5 => RawChange::Renamed { from: a, to: b },
            _ => RawChange::Rescan,
        });
    }
    // End on several names at once, so the last report carries more than one verdict.
    events.push(RawChange::Created(p("a")));
    events.push(RawChange::Created(p("b")));
    events.push(RawChange::Created(p("c")));
    let t = Instant::now();
    let mut first = coalescer();
    let mut second = coalescer();
    let mut one = Vec::new();
    let mut other = Vec::new();
    for (i, event) in events.iter().cloned().enumerate() {
        let at = t + Duration::from_millis(i as u64);
        first.push(event.clone(), at);
        second.push(event, at);
        // Let each burst of twenty-five events settle, so a report often carries more than
        // one entry and the order of the report is checked too.
        if i % 25 == 24 || i + 1 == events.len() {
            one.extend(settle(&mut first, t, i as u64 + 100));
            other.extend(settle(&mut second, t, i as u64 + 100));
        }
    }
    assert_eq!(one, other, "the same events settle the same way");
}

// The reading of a verdict, with entries made by hand instead of read from a disk.

/// An entry for `path`, as [`Entry::at`] would read it.
fn entry(path: PathBuf) -> Entry {
    Entry {
        name: path.file_name().unwrap().to_string_lossy().into_owned(),
        path,
        kind: Kind::File,
        size: 7,
        modified: None,
        created: None,
        hidden: false,
    }
}

/// A reader that knows these entries and no others.
fn knows(entries: Vec<Entry>) -> impl Fn(&Path) -> Option<Entry> {
    move |path| entries.iter().find(|entry| entry.path == path).cloned()
}

#[test]
fn a_created_entry_is_read_into_a_created_change() {
    let path = p("a.txt");
    let signal = signal_of(
        Verdict::Created(path.clone()),
        &knows(vec![entry(path.clone())]),
    );
    assert_eq!(signal, Some(Signal::Changed(Change::Created(entry(path)))));
}

#[test]
fn a_created_entry_that_vanished_is_not_reported() {
    let signal = signal_of(Verdict::Created(p("a.txt")), &knows(Vec::new()));
    assert_eq!(signal, None);
}

#[test]
fn a_modified_entry_is_read_into_a_modified_change() {
    let path = p("a.txt");
    let signal = signal_of(
        Verdict::Modified(path.clone()),
        &knows(vec![entry(path.clone())]),
    );
    assert_eq!(signal, Some(Signal::Changed(Change::Modified(entry(path)))));
}

#[test]
fn a_modified_entry_that_vanished_is_removed() {
    let path = p("a.txt");
    let signal = signal_of(Verdict::Modified(path.clone()), &knows(Vec::new()));
    assert_eq!(signal, Some(Signal::Changed(Change::Removed(path))));
}

#[test]
fn a_removal_is_reported_without_reading_anything() {
    let path = p("a.txt");
    let reads_nothing = |_: &Path| -> Option<Entry> { panic!("a removal reads nothing") };
    assert_eq!(
        signal_of(Verdict::Removed(path.clone()), &reads_nothing),
        Some(Signal::Changed(Change::Removed(path)))
    );
}

#[test]
fn a_rename_is_read_at_its_new_name() {
    let from = p("a.txt");
    let to = p("b.txt");
    let signal = signal_of(
        Verdict::Renamed {
            from: from.clone(),
            to: to.clone(),
        },
        &knows(vec![entry(to.clone())]),
    );
    assert_eq!(
        signal,
        Some(Signal::Changed(Change::Renamed {
            from,
            to: entry(to)
        }))
    );
}

#[test]
fn a_rename_whose_new_name_vanished_is_a_removal_of_the_old_name() {
    let from = p("a.txt");
    let to = p("b.txt");
    let signal = signal_of(
        Verdict::Renamed {
            from: from.clone(),
            to,
        },
        &knows(Vec::new()),
    );
    assert_eq!(signal, Some(Signal::Changed(Change::Removed(from))));
}

#[test]
fn a_rescan_is_a_rescan() {
    let reads_nothing = |_: &Path| -> Option<Entry> { panic!("a rescan reads nothing") };
    assert_eq!(
        signal_of(Verdict::Rescan, &reads_nothing),
        Some(Signal::Rescan)
    );
}

// The translation of a `notify` event into raw changes.

#[test]
fn a_rescan_event_is_a_rescan() {
    let event = notify::Event::new(EventKind::Other).set_flag(Flag::Rescan);
    assert_eq!(raw_changes(&event), vec![RawChange::Rescan]);
}

#[test]
fn a_create_event_is_a_creation_of_each_of_its_paths() {
    let event = notify::Event::new(EventKind::Create(CreateKind::File))
        .add_path(p("a.txt"))
        .add_path(p("b.txt"));
    assert_eq!(
        raw_changes(&event),
        vec![
            RawChange::Created(p("a.txt")),
            RawChange::Created(p("b.txt"))
        ]
    );
}

#[test]
fn a_remove_event_is_a_removal() {
    let event = notify::Event::new(EventKind::Remove(RemoveKind::File)).add_path(p("a.txt"));
    assert_eq!(raw_changes(&event), vec![RawChange::Removed(p("a.txt"))]);
}

#[test]
fn a_plain_modify_event_is_a_modification() {
    let event = notify::Event::new(EventKind::Modify(ModifyKind::Any)).add_path(p("a.txt"));
    assert_eq!(raw_changes(&event), vec![RawChange::Modified(p("a.txt"))]);
}

#[test]
fn the_two_halves_of_a_rename_become_from_and_to() {
    let from_event =
        notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From))).add_path(p("a"));
    let to_event =
        notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To))).add_path(p("b"));
    assert_eq!(
        raw_changes(&from_event),
        vec![RawChange::RenamedFrom(p("a"))]
    );
    assert_eq!(raw_changes(&to_event), vec![RawChange::RenamedTo(p("b"))]);
}

#[test]
fn a_paired_rename_becomes_one_change_with_both_names() {
    let event = notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
        .add_path(p("a"))
        .add_path(p("b"));
    assert_eq!(
        raw_changes(&event),
        vec![RawChange::Renamed {
            from: p("a"),
            to: p("b")
        }]
    );
}

#[test]
fn an_unpaired_rename_becomes_a_removal_and_a_creation_of_its_path() {
    let event =
        notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any))).add_path(p("a"));
    assert_eq!(
        raw_changes(&event),
        vec![RawChange::Removed(p("a")), RawChange::Created(p("a"))]
    );
}

#[test]
fn an_access_or_untyped_event_says_nothing() {
    let access = notify::Event::new(EventKind::Access(notify::event::AccessKind::Open(
        notify::event::AccessMode::Read,
    )))
    .add_path(p("a.txt"));
    let any = notify::Event::new(EventKind::Any).add_path(p("a.txt"));
    let other = notify::Event::new(EventKind::Other).add_path(p("a.txt"));
    assert_eq!(raw_changes(&access), Vec::new());
    assert_eq!(raw_changes(&any), Vec::new());
    assert_eq!(raw_changes(&other), Vec::new());
}

// The wake contract shared with the application.

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
