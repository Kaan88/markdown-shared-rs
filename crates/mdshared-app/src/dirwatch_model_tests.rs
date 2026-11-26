//! The tests that catch a report applied in the wrong order, or a report that loses a name, on a
//! made-up disk and a `Listing`: no watcher, no thread, no real folder.
//!
//! Each case names the events the platforms give, resolves each verdict through the made-up
//! disk (instead of `Entry::at`), applies it to the list, and compares the list with the disk.
//! The random model does the same over a run of operations that take time, so a report split by
//! the longest wait is applied before the operations that follow it, exactly as the watcher
//! would report it.

use super::*;
use crate::dirlist::{Entry, Kind, Listing};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The folder every test here watches.
fn folder() -> PathBuf {
    PathBuf::from("/model")
}

/// A path directly inside the watched folder.
fn p(name: &str) -> PathBuf {
    folder().join(name)
}

/// An entry for `name`, whose size is the disk's generation of it, so a lost modification shows.
fn entry(name: &str, generation: u64) -> Entry {
    Entry {
        path: p(name),
        name: name.to_string(),
        kind: Kind::File,
        size: generation,
        modified: None,
        created: None,
        hidden: false,
    }
}

/// The shown names and generations of a list, in its order.
fn shown(listing: &Listing) -> Vec<(String, u64)> {
    listing
        .visible()
        .map(|entry| (entry.name.clone(), entry.size))
        .collect()
}

/// A list showing exactly this disk.
fn listing_of(disk: &BTreeMap<String, u64>) -> Listing {
    let mut listing = Listing::new();
    listing.set_folder(Some(folder()));
    listing.set_entries(
        disk.iter()
            .map(|(name, generation)| entry(name, *generation))
            .collect(),
    );
    listing
}

/// The names and generations the disk holds, in the order a list sorts them.
fn want(disk: &BTreeMap<String, u64>) -> Vec<(String, u64)> {
    disk.iter()
        .map(|(name, generation)| (name.clone(), *generation))
        .collect()
}

/// Applies every verdict to `listing`, reading through `disk`.
fn apply(listing: &mut Listing, verdicts: Vec<Verdict>, disk: &BTreeMap<String, u64>) {
    let read = |path: &Path| {
        let name = path.file_name()?.to_str()?;
        disk.get(name).map(|generation| entry(name, *generation))
    };
    for verdict in verdicts {
        match verdict {
            Verdict::Rescan | Verdict::Lost => panic!("no overflow in a made-up folder"),
            other => {
                if let Some(Signal::Changed(change)) = signal_of(other, &read) {
                    listing.apply(change);
                }
            }
        }
    }
}

/// The verdicts of these events, all seen a millisecond apart and settled after the quiet
/// period.
fn settle(events: &[RawChange]) -> Vec<Verdict> {
    let mut coalescer = Coalescer::new(folder(), Duration::from_millis(10));
    let at = Instant::now();
    for (i, event) in events.iter().cloned().enumerate() {
        coalescer.push(event, at + Duration::from_millis(i as u64));
    }
    coalescer.flush(at + Duration::from_millis(events.len() as u64 + 100))
}

/// A backup on save: the old file is renamed out of the way, then a new file of the same name
/// is written.
#[test]
fn a_backup_on_save_keeps_the_new_file_and_the_backup() {
    let disk = BTreeMap::from([("f".to_string(), 2), ("f~".to_string(), 1)]);
    let mut listing = listing_of(&BTreeMap::from([("f".to_string(), 1)]));
    let verdicts = settle(&[
        RawChange::RenamedFrom(p("f")),
        RawChange::RenamedTo(p("f~")),
        RawChange::Created(p("f")),
    ]);
    apply(&mut listing, verdicts, &disk);
    assert_eq!(shown(&listing), want(&disk));
}

/// Log rotation: every file moves one name up, oldest first, and a new file is written.
#[test]
fn log_rotation_moves_the_oldest_file_first() {
    let disk = BTreeMap::from([
        ("f".to_string(), 4),
        ("f.1".to_string(), 1),
        ("f.2".to_string(), 2),
        ("f.3".to_string(), 3),
    ]);
    let mut listing = listing_of(&disk);
    let verdicts = settle(&[
        RawChange::RenamedFrom(p("f.2")),
        RawChange::RenamedTo(p("f.3")),
        RawChange::RenamedFrom(p("f.1")),
        RawChange::RenamedTo(p("f.2")),
        RawChange::RenamedFrom(p("f")),
        RawChange::RenamedTo(p("f.1")),
        RawChange::Created(p("f")),
    ]);
    apply(&mut listing, verdicts, &disk);
    assert_eq!(shown(&listing), want(&disk));
}

/// A swap of two names through a temporary one.
#[test]
fn a_swap_through_a_temporary_name_keeps_both_entries() {
    let disk = BTreeMap::from([("a".to_string(), 2), ("b".to_string(), 1)]);
    let mut listing = listing_of(&disk);
    let verdicts = settle(&[
        RawChange::RenamedFrom(p("a")),
        RawChange::RenamedTo(p("t")),
        RawChange::RenamedFrom(p("b")),
        RawChange::RenamedTo(p("a")),
        RawChange::RenamedFrom(p("t")),
        RawChange::RenamedTo(p("b")),
    ]);
    apply(&mut listing, verdicts, &disk);
    assert_eq!(shown(&listing), want(&disk));
}

/// A rename onto an existing name, and then a removal of that name: the list must lose both
/// names, and in particular the name the rename landed on.
#[test]
fn a_rename_onto_an_existing_name_that_is_then_removed_removes_both() {
    let disk = BTreeMap::new();
    let mut listing = listing_of(&BTreeMap::from([
        ("a".to_string(), 1),
        ("b".to_string(), 2),
    ]));
    let verdicts = settle(&[
        RawChange::RenamedFrom(p("a")),
        RawChange::RenamedTo(p("b")),
        RawChange::Removed(p("b")),
    ]);
    apply(&mut listing, verdicts, &disk);
    assert_eq!(shown(&listing), want(&disk));
}

/// The same, for a name that was created and then renamed onto an existing one, as Windows
/// reports it: the created name, the removal of the overwritten name, the two rename halves,
/// and then the removal of the landing name.
#[test]
fn a_created_name_renamed_onto_an_existing_one_then_removed_removes_both() {
    let disk = BTreeMap::new();
    let mut listing = listing_of(&BTreeMap::from([("b".to_string(), 2)]));
    let verdicts = settle(&[
        RawChange::Created(p("t")),
        RawChange::Removed(p("b")),
        RawChange::RenamedFrom(p("t")),
        RawChange::RenamedTo(p("b")),
        RawChange::Removed(p("b")),
    ]);
    apply(&mut listing, verdicts, &disk);
    assert_eq!(shown(&listing), want(&disk));
}

/// The same, for a rename into the folder onto an existing name that is then removed, as inotify
/// reports it: only the new name, and then its removal.
#[test]
fn a_name_moved_in_onto_an_existing_one_then_removed_removes_both() {
    let disk = BTreeMap::new();
    let mut listing = listing_of(&BTreeMap::from([("b".to_string(), 2)]));
    let verdicts = settle(&[RawChange::RenamedTo(p("b")), RawChange::Removed(p("b"))]);
    apply(&mut listing, verdicts, &disk);
    assert_eq!(shown(&listing), want(&disk));
}

/// A rename onto an existing name whose result is then renamed again, as inotify reports it:
/// the two halves of each rename, then the pair carrying both names.
#[test]
fn a_rename_onto_an_existing_name_that_moves_again_keeps_the_chain() {
    let disk = BTreeMap::from([("c".to_string(), 1)]);
    let mut listing = listing_of(&BTreeMap::from([
        ("a".to_string(), 1),
        ("b".to_string(), 2),
    ]));
    let verdicts = settle(&[
        RawChange::RenamedFrom(p("a")),
        RawChange::RenamedTo(p("b")),
        RawChange::Renamed {
            from: p("a"),
            to: p("b"),
        },
        RawChange::RenamedFrom(p("b")),
        RawChange::RenamedTo(p("c")),
        RawChange::Renamed {
            from: p("b"),
            to: p("c"),
        },
    ]);
    assert!(
        verdicts.contains(&Verdict::Renamed {
            from: p("a"),
            to: p("c")
        }),
        "the chain keeps its first and last names: {verdicts:?}"
    );
    apply(&mut listing, verdicts, &disk);
    assert_eq!(shown(&listing), want(&disk));
}

/// A chain through a name the list holds: the intermediate name's entry must be removed too,
/// because the first rename replaced it.
#[test]
fn a_rename_chain_removes_the_name_it_passed_through() {
    let disk = BTreeMap::from([("c".to_string(), 1)]);
    let mut listing = listing_of(&BTreeMap::from([
        ("a".to_string(), 1),
        ("b".to_string(), 2),
    ]));
    let verdicts = settle(&[
        RawChange::RenamedFrom(p("a")),
        RawChange::RenamedTo(p("b")),
        RawChange::RenamedFrom(p("b")),
        RawChange::RenamedTo(p("c")),
    ]);
    apply(&mut listing, verdicts, &disk);
    assert_eq!(shown(&listing), want(&disk));
}

/// A chain whose last rename leaves the folder: the first name is gone too, and the list may
/// still hold it.
#[test]
fn a_chain_that_leaves_the_folder_removes_its_first_name_too() {
    let disk = BTreeMap::new();
    let mut listing = listing_of(&BTreeMap::from([("a".to_string(), 1)]));
    let verdicts = settle(&[
        RawChange::RenamedFrom(p("a")),
        RawChange::RenamedTo(p("b")),
        RawChange::RenamedFrom(p("b")),
    ]);
    apply(&mut listing, verdicts, &disk);
    assert_eq!(shown(&listing), want(&disk));
}

// The model test: random operations on a made-up disk, the raw events each platform would give,
// reports split by the longest wait, and a list that must end up equal to the disk.

/// How a platform reports the events of one operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    /// Windows: create, remove, modify; a rename onto an existing name removes that name first,
    /// then the old and the new name of the rename.
    Windows,
    /// inotify: create, remove, modify; a rename's old and new names and then a pair carrying
    /// both. An overwritten target's removal is not reported.
    Inotify,
    /// macOS FSEvents: every path an operation touches is a modification, including the old
    /// path of a rename; the disk decides what happened to it.
    Mac,
}

/// A made-up folder: the names on the disk, each with a generation, and the raw events the
/// operations would have produced.
struct Model {
    disk: BTreeMap<String, u64>,
    events: Vec<RawChange>,
    generation: u64,
    style: Style,
}

impl Model {
    fn new(names: &[&str], style: Style) -> Model {
        let disk = names
            .iter()
            .enumerate()
            .map(|(i, name)| ((*name).to_string(), i as u64 + 1))
            .collect();
        Model {
            disk,
            events: Vec::new(),
            generation: names.len() as u64,
            style,
        }
    }

    fn has(&self, name: &str) -> bool {
        self.disk.contains_key(name)
    }

    /// A creation, reported only when the name was free (a file manager does not create over a
    /// name that is there).
    fn create(&mut self, name: &str) {
        if !self.has(name) {
            self.generation += 1;
            self.disk.insert(name.to_string(), self.generation);
            self.events.push(match self.style {
                Style::Mac => RawChange::Modified(p(name)),
                Style::Windows | Style::Inotify => RawChange::Created(p(name)),
            });
        }
    }

    fn remove(&mut self, name: &str) {
        if self.disk.remove(name).is_some() {
            self.events.push(match self.style {
                Style::Mac => RawChange::Modified(p(name)),
                Style::Windows | Style::Inotify => RawChange::Removed(p(name)),
            });
        }
    }

    fn modify(&mut self, name: &str) {
        if self.has(name) {
            self.generation += 1;
            self.disk.insert(name.to_string(), self.generation);
            self.events.push(RawChange::Modified(p(name)));
        }
    }

    /// A rename inside the folder, overwriting the target.
    fn rename(&mut self, from: &str, to: &str) -> bool {
        let Some(generation) = self.disk.remove(from) else {
            return false;
        };
        let overwritten = self.disk.insert(to.to_string(), generation).is_some();
        match self.style {
            Style::Windows => {
                if overwritten {
                    self.events.push(RawChange::Removed(p(to)));
                }
                self.rename_halves(from, to);
            }
            Style::Inotify => {
                self.rename_halves(from, to);
                self.events.push(RawChange::Renamed {
                    from: p(from),
                    to: p(to),
                });
            }
            Style::Mac => {
                self.events.push(RawChange::Modified(p(from)));
                self.events.push(RawChange::Modified(p(to)));
            }
        }
        true
    }

    fn rename_halves(&mut self, from: &str, to: &str) {
        self.events.push(RawChange::RenamedFrom(p(from)));
        self.events.push(RawChange::RenamedTo(p(to)));
    }

    /// A rename into the folder from outside it.
    fn move_in(&mut self, name: &str) -> bool {
        if self.has(name) {
            return false;
        }
        self.generation += 1;
        self.disk.insert(name.to_string(), self.generation);
        self.events.push(match self.style {
            Style::Mac => RawChange::Modified(p(name)),
            Style::Windows | Style::Inotify => RawChange::RenamedTo(p(name)),
        });
        true
    }

    /// A rename out of the folder.
    fn move_out(&mut self, name: &str) -> bool {
        if self.disk.remove(name).is_none() {
            return false;
        }
        self.events.push(match self.style {
            Style::Mac => RawChange::Modified(p(name)),
            Style::Windows | Style::Inotify => RawChange::RenamedFrom(p(name)),
        });
        true
    }
}

/// A small linear congruential generator, so every run of a seed is the same run.
fn next(seed: &mut u64) -> usize {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*seed >> 33) as usize
}

/// One run: the coalescer, the list and the disk, with a clock moved by hand.
struct Run {
    coalescer: Coalescer,
    listing: Listing,
    disk: BTreeMap<String, u64>,
    start: Instant,
    label: String,
}

impl Run {
    /// Flushes every report that is due at `now_ms`, resolves it through the disk as it is at
    /// that moment, applies it, and requires the list to equal the disk.
    fn flush_due(&mut self, now_ms: u64) {
        let now = self.start + Duration::from_millis(now_ms);
        while self
            .coalescer
            .deadline()
            .is_some_and(|deadline| deadline <= now)
        {
            let verdicts = self.coalescer.flush(now);
            let mut signals = Vec::new();
            for verdict in verdicts {
                match verdict {
                    Verdict::Rescan | Verdict::Lost => {
                        panic!("no overflow in a made-up folder")
                    }
                    other => {
                        let disk = &self.disk;
                        let read = |path: &Path| {
                            let name = path.file_name()?.to_str()?;
                            disk.get(name).map(|generation| entry(name, *generation))
                        };
                        if let Some(signal) = signal_of(other, &read) {
                            signals.push(signal);
                        }
                    }
                }
            }
            for signal in signals {
                match signal {
                    Signal::Changed(change) => {
                        self.listing.apply(change);
                    }
                    Signal::Rescan | Signal::Lost => panic!("no overflow in a made-up folder"),
                }
            }
            assert_eq!(
                shown(&self.listing),
                want(&self.disk),
                "{}: after a flush at {now_ms} ms",
                self.label
            );
        }
    }
}

/// Runs one seed: operations that take time on the disk, the events of the given style, and the
/// list must equal the disk at every report and at the end.
fn one_seed(seed_number: u64, style: Style) {
    let names = ["a", "b", "c", "d"];
    let mut seed = seed_number.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut model = Model::new(&names, style);
    let mut run = Run {
        coalescer: Coalescer::with_max_wait(
            folder(),
            Duration::from_millis(50),
            Duration::from_secs(1),
        ),
        listing: listing_of(&model.disk),
        disk: model.disk.clone(),
        start: Instant::now(),
        label: format!("seed {seed_number}, {style:?}"),
    };

    let steps = 1 + next(&mut seed) % 20;
    let mut now = 0u64;
    let mut description = Vec::new();
    for _ in 0..steps {
        run.flush_due(now);
        now += (next(&mut seed) % 250) as u64;
        run.flush_due(now);

        let a = names[next(&mut seed) % names.len()];
        let b = names[next(&mut seed) % names.len()];
        let c = names[next(&mut seed) % names.len()];
        match next(&mut seed) % 9 {
            0 => {
                model.create(a);
                description.push(format!("create {a}"));
            }
            1 => {
                model.remove(a);
                description.push(format!("remove {a}"));
            }
            2 => {
                model.modify(a);
                description.push(format!("modify {a}"));
            }
            3 => {
                if a != b && model.rename(a, b) {
                    description.push(format!("rename {a}->{b}"));
                }
            }
            4 => {
                // A swap through a temporary name, when both names are there.
                if a != b && model.has(a) && model.has(b) {
                    model.rename(a, "t");
                    model.rename(b, a);
                    model.rename("t", b);
                    description.push(format!("swap {a}<->{b}"));
                }
            }
            5 => {
                // A backup on save: the old name out of the way, then a new file of that name.
                if model.has(a) {
                    let backup = format!("{a}~");
                    model.rename(a, &backup);
                    model.create(a);
                    description.push(format!("backup {a}"));
                }
            }
            6 => {
                // A chain through a name.
                if a != b && b != c && model.rename(a, b) {
                    model.rename(b, c);
                    description.push(format!("chain {a}->{b}->{c}"));
                }
            }
            7 => {
                if model.move_in(a) {
                    description.push(format!("move in {a}"));
                }
            }
            _ => {
                if model.move_out(a) {
                    description.push(format!("move out {a}"));
                }
            }
        }

        run.label = format!("seed {seed_number}, {style:?}: {}", description.join(", "));
        run.disk = model.disk.clone();
        let at = run.start + Duration::from_millis(now);
        for event in model.events.drain(..) {
            run.coalescer.push(event, at);
        }
    }

    now += 5_000;
    run.flush_due(now);
    assert!(
        run.coalescer.is_idle(),
        "seed {seed_number}, {style:?}: {}",
        description.join(", ")
    );
}

#[test]
fn random_operations_end_with_the_list_equal_to_the_disk() {
    for seed in 0..2_000 {
        for style in [Style::Windows, Style::Inotify, Style::Mac] {
            one_seed(seed, style);
        }
    }
}
