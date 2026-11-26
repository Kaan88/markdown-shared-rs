//! The tests of [`super`], the coalescer and the watcher that reports what changed in one folder.

use super::*;
use crate::dirlist::Kind;
use notify::EventKind;
use notify::event::{CreateKind, Flag, ModifyKind, RemoveKind, RenameMode};

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
        vec![
            Verdict::Removed(p("b")),
            Verdict::Renamed {
                from: p("a"),
                to: p("c")
            }
        ]
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
    assert_eq!(
        settle(&mut c, t, 1_000),
        vec![Verdict::Removed(p("a")), Verdict::Created(p("b"))]
    );
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
    assert_eq!(
        settle(&mut c, t, 1_000),
        vec![Verdict::Removed(p("a")), Verdict::Removed(p("b"))]
    );
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
            Verdict::Renamed {
                from: p("a"),
                to: p("b")
            },
            Verdict::Created(p("a"))
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
fn a_burst_that_cancels_out_leaves_no_stale_wait() {
    let t = Instant::now();
    let mut c = Coalescer::with_max_wait(
        folder(),
        Duration::from_millis(50),
        Duration::from_millis(100),
    );
    // A creation and its removal are nothing at all; the burst is over.
    push(&mut c, t, 0, RawChange::Created(p("a.txt")));
    push(&mut c, t, 10, RawChange::Removed(p("a.txt")));
    assert!(c.is_idle());
    assert_eq!(c.deadline(), None);
    // A later event must get its own quiet period, not the old burst's longest wait.
    push(&mut c, t, 500, RawChange::Created(p("b.txt")));
    assert_eq!(c.deadline(), Some(t + Duration::from_millis(550)));
    assert_eq!(settle(&mut c, t, 549), Vec::new(), "still its own burst");
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
fn a_report_names_each_path_once_in_a_stable_order() {
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
fn only_the_renames_in_a_cycle_are_broken_into_removals_and_modifications() {
    let t = Instant::now();
    let mut c = coalescer();
    // A swap through a temporary name is a cycle: a->t, b->a, t->b.
    push(&mut c, t, 0, RawChange::RenamedFrom(p("a")));
    push(&mut c, t, 1, RawChange::RenamedTo(p("t")));
    push(&mut c, t, 2, RawChange::RenamedFrom(p("b")));
    push(&mut c, t, 3, RawChange::RenamedTo(p("a")));
    push(&mut c, t, 4, RawChange::RenamedFrom(p("t")));
    push(&mut c, t, 5, RawChange::RenamedTo(p("b")));
    // An unrelated rename must stay a rename, so the selection can follow it.
    push(&mut c, t, 6, RawChange::RenamedFrom(p("c")));
    push(&mut c, t, 7, RawChange::RenamedTo(p("d")));
    let verdicts = settle(&mut c, t, 1_000);
    assert!(
        verdicts.contains(&Verdict::Renamed {
            from: p("c"),
            to: p("d")
        }),
        "an unrelated rename is not broken: {verdicts:?}"
    );
    assert!(
        !verdicts.contains(&Verdict::Modified(p("d"))),
        "and it is not read from the disk either: {verdicts:?}"
    );
}

#[test]
fn twenty_thousand_removals_and_renames_settle_well_under_a_second() {
    let t = Instant::now();
    let mut c = Coalescer::with_max_wait(
        folder(),
        Duration::from_secs(3_600),
        Duration::from_secs(7_200),
    );
    for i in 0..10_000u64 {
        let at = t + Duration::from_millis(i);
        c.push(RawChange::RenamedFrom(p(&format!("a{i}"))), at);
        c.push(RawChange::RenamedTo(p(&format!("b{i}"))), at);
        c.push(RawChange::Removed(p(&format!("c{i}"))), at);
    }
    let before = Instant::now();
    let verdicts = c.flush(t + Duration::from_secs(7_200));
    let elapsed = before.elapsed();
    assert_eq!(verdicts.len(), 20_000);
    assert!(
        elapsed < Duration::from_secs(5),
        "the report took {elapsed:?}, which is not well under a second"
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

/// The translation of one event for the pure folder `/watched`; it exists in no file system, so
/// the translator compares paths lexically.
fn translate(event: &notify::Event) -> Vec<RawChange> {
    Translator::new(folder(), folder()).translate(event)
}

/// A `notify` result that is an error.
fn error() -> notify::Result<notify::Event> {
    Err(notify::Error::generic("the watch failed"))
}

#[test]
fn a_rescan_event_is_a_rescan() {
    let event = notify::Event::new(EventKind::Other).set_flag(Flag::Rescan);
    assert_eq!(translate(&event), vec![RawChange::Rescan]);
}

#[test]
fn a_create_event_is_a_creation_of_each_of_its_paths() {
    let event = notify::Event::new(EventKind::Create(CreateKind::File))
        .add_path(p("a.txt"))
        .add_path(p("b.txt"));
    assert_eq!(
        translate(&event),
        vec![
            RawChange::Created(p("a.txt")),
            RawChange::Created(p("b.txt"))
        ]
    );
}

#[test]
fn a_remove_event_is_a_removal() {
    let event = notify::Event::new(EventKind::Remove(RemoveKind::File)).add_path(p("a.txt"));
    assert_eq!(translate(&event), vec![RawChange::Removed(p("a.txt"))]);
}

#[test]
fn a_plain_modify_event_is_a_modification() {
    let event = notify::Event::new(EventKind::Modify(ModifyKind::Any)).add_path(p("a.txt"));
    assert_eq!(translate(&event), vec![RawChange::Modified(p("a.txt"))]);
}

#[test]
fn the_two_halves_of_a_rename_become_from_and_to() {
    let from_event =
        notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From))).add_path(p("a"));
    let to_event =
        notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To))).add_path(p("b"));
    assert_eq!(translate(&from_event), vec![RawChange::RenamedFrom(p("a"))]);
    assert_eq!(translate(&to_event), vec![RawChange::RenamedTo(p("b"))]);
}

#[test]
fn a_paired_rename_becomes_one_change_with_both_names() {
    let event = notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
        .add_path(p("a"))
        .add_path(p("b"));
    assert_eq!(
        translate(&event),
        vec![RawChange::Renamed {
            from: p("a"),
            to: p("b")
        }]
    );
}

#[test]
fn a_case_only_rename_without_sides_requires_a_rescan() {
    // On a case-insensitive volume, both spellings would pass a metadata lookup.
    let mut c = coalescer();
    let at = Instant::now();
    for (mode, name) in [(RenameMode::Any, "Report"), (RenameMode::Other, "report")] {
        let event = notify::Event::new(EventKind::Modify(ModifyKind::Name(mode))).add_path(p(name));
        for change in translate(&event) {
            c.push(change, at);
        }
    }
    assert_eq!(settle(&mut c, at, 1_000), vec![Verdict::Rescan]);
}

#[test]
fn three_complete_renames_can_repeat_the_first_pair() {
    let at = Instant::now();
    let mut c = coalescer();
    for (from, to) in [("a", "b"), ("b", "a"), ("a", "b")] {
        c.push(
            RawChange::Renamed {
                from: p(from),
                to: p(to),
            },
            at,
        );
    }
    assert!(settle(&mut c, at, 1_000).contains(&Verdict::Renamed {
        from: p("a"),
        to: p("b"),
    }));
}

fn settle_backend_renames(events: &[(RenameMode, usize, &[&str])]) -> Vec<Verdict> {
    let at = Instant::now();
    let mut c = coalescer();
    let names = Translator::new(folder(), folder());
    for &(mode, cookie, paths) in events {
        let mut event =
            notify::Event::new(EventKind::Modify(ModifyKind::Name(mode))).set_tracker(cookie);
        for name in paths {
            event = event.add_path(p(name));
        }
        for (change, cookie) in translation(Ok(event), &names) {
            c.push_tracked(change, at, cookie);
        }
    }
    settle(&mut c, at, 1_000)
}

#[test]
fn unrelated_inotify_moves_do_not_transfer_the_selection() {
    let verdicts =
        settle_backend_renames(&[(RenameMode::From, 1, &["a"]), (RenameMode::To, 2, &["b"])]);
    assert_eq!(
        verdicts,
        vec![Verdict::Created(p("b")), Verdict::Removed(p("a"))]
    );
}

#[test]
fn interleaved_inotify_moves_pair_by_cookie_even_without_both() {
    let verdicts = settle_backend_renames(&[
        (RenameMode::From, 1, &["a"]),
        (RenameMode::From, 2, &["b"]),
        (RenameMode::To, 2, &["c"]),
        (RenameMode::Both, 2, &["b", "c"]),
        (RenameMode::To, 1, &["d"]),
    ]);
    assert_eq!(verdicts.len(), 2);
    for (from, to) in [("a", "d"), ("b", "c")] {
        assert!(verdicts.contains(&Verdict::Renamed {
            from: p(from),
            to: p(to)
        }));
    }
}

#[test]
fn a_rename_out_of_the_folder_becomes_a_removal_on_its_own() {
    let event = notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
        .add_path(p("a"))
        .add_path(elsewhere("a"));
    assert_eq!(translate(&event), vec![RawChange::RenamedFrom(p("a"))]);
}

#[test]
fn a_rename_into_the_folder_becomes_a_creation_on_its_own() {
    let event = notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
        .add_path(elsewhere("a"))
        .add_path(p("a"));
    assert_eq!(translate(&event), vec![RawChange::RenamedTo(p("a"))]);
}

#[test]
fn an_event_about_the_folder_itself_is_a_lost_watch() {
    for kind in [
        EventKind::Remove(RemoveKind::Folder),
        EventKind::Modify(ModifyKind::Name(RenameMode::From)),
    ] {
        let event = notify::Event::new(kind).add_path(folder());
        assert_eq!(translate(&event), vec![RawChange::Lost]);
    }
}

#[test]
fn an_error_from_the_os_watcher_is_a_rescan() {
    let names = Translator::new(folder(), folder());
    assert_eq!(
        translation(error(), &names).collect::<Vec<_>>(),
        vec![(RawChange::Rescan, None)]
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
    assert_eq!(translate(&access), Vec::new());
    assert_eq!(translate(&any), Vec::new());
    assert_eq!(translate(&other), Vec::new());
}

#[test]
fn an_event_path_is_rewritten_as_the_callers_folder() {
    // The root is what the OS watcher was given and the folder is what the caller named, so
    // this needs no disk at all: nothing here exists.
    let names = Translator::new(PathBuf::from("/given"), PathBuf::from("/root"));
    let event = notify::Event::new(EventKind::Create(CreateKind::File))
        .add_path(PathBuf::from("/root/a.txt"));
    assert_eq!(
        names.translate(&event),
        vec![RawChange::Created(PathBuf::from("/given/a.txt"))],
        "the path is the folder given, joined with the event's own name"
    );
    // The caller's spelling is not what the watcher reports: a path under it is not the folder.
    let under_given = notify::Event::new(EventKind::Create(CreateKind::File))
        .add_path(PathBuf::from("/given/a.txt"));
    assert_eq!(names.translate(&under_given), Vec::new());
    let outside = notify::Event::new(EventKind::Create(CreateKind::File))
        .add_path(PathBuf::from("/elsewhere.txt"));
    assert_eq!(names.translate(&outside), Vec::new());
}

#[test]
fn the_folder_given_is_what_a_canonicalised_root_reports() {
    // macOS reports the canonical path; the report still names the caller's folder.
    let names = Translator::new(PathBuf::from("/var/x"), PathBuf::from("/private/var/x"));
    let event = notify::Event::new(EventKind::Create(CreateKind::File))
        .add_path(PathBuf::from("/private/var/x/a.txt"));
    assert_eq!(
        names.translate(&event),
        vec![RawChange::Created(PathBuf::from("/var/x/a.txt"))],
        "the folder given is what the report names"
    );
}

#[test]
fn a_report_is_flushed_after_the_longest_wait_even_while_events_keep_coming() {
    let t = Instant::now();
    let mut c = Coalescer::with_max_wait(
        folder(),
        Duration::from_millis(50),
        Duration::from_millis(200),
    );
    // An event every 20 ms: the quiet period never ends, but the wait does.
    for ms in (0..10).map(|i| i * 20) {
        push(&mut c, t, ms, RawChange::Modified(p("a.txt")));
        let quiet = (ms + 50).min(200);
        assert_eq!(
            c.deadline(),
            Some(t + Duration::from_millis(quiet)),
            "at {ms} ms the burst is due at {quiet} ms"
        );
    }
    assert_eq!(
        settle(&mut c, t, 199),
        Vec::new(),
        "still waiting for the ceiling"
    );
    assert_eq!(settle(&mut c, t, 200), vec![Verdict::Modified(p("a.txt"))]);
}

#[test]
fn a_lost_watch_clears_what_was_pending() {
    let t = Instant::now();
    let mut c = coalescer();
    push(&mut c, t, 0, RawChange::Created(p("a.txt")));
    push(&mut c, t, 1, RawChange::Lost);
    // Events after the loss are moot: the folder is gone.
    push(&mut c, t, 2, RawChange::Created(p("b.txt")));
    assert_eq!(settle(&mut c, t, 1_000), vec![Verdict::Lost]);
    assert!(c.is_idle());
    assert_eq!(settle(&mut c, t, 2_000), Vec::new());
}

#[test]
fn a_lost_verdict_is_a_lost_signal() {
    let reads_nothing = |_: &Path| -> Option<Entry> { panic!("a lost watch reads nothing") };
    assert_eq!(signal_of(Verdict::Lost, &reads_nothing), Some(Signal::Lost));
}
