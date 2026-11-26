//! The tests that catch a report applied in the wrong order, and a report that loses a name, on
//! a made-up disk and a `Listing`: no watcher, no thread, no real folder.
//!
//! Each case names the events the platforms give, resolves each verdict through the made-up
//! disk (instead of `Entry::at`), applies it to the list, and compares the list with the disk.

use super::*;
use crate::dirlist::{Entry, Kind, Listing};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// The folder every test here watches.
fn folder() -> PathBuf {
    PathBuf::from("/model")
}

/// A path directly inside the watched folder.
fn p(name: &str) -> PathBuf {
    folder().join(name)
}

/// An entry for `name`, as `Entry::at` would read it.
fn entry(name: &str) -> Entry {
    Entry {
        path: p(name),
        name: name.to_string(),
        kind: Kind::File,
        size: name.len() as u64,
        modified: None,
        created: None,
        hidden: false,
    }
}

/// A list showing exactly these names, read as if from the folder.
fn listing_of(names: &[&str]) -> Listing {
    let mut listing = Listing::new();
    listing.set_folder(Some(folder()));
    listing.set_entries(names.iter().map(|name| entry(name)).collect());
    listing
}

/// A reader that knows exactly these names and no others.
fn disk_of<'a>(names: &'a [String]) -> impl Fn(&Path) -> Option<Entry> + 'a {
    move |path| {
        let name = path.file_name()?.to_str()?;
        names.contains(&name.to_string()).then(|| entry(name))
    }
}

/// The shown names of a list, in its order.
fn shown(listing: &Listing) -> Vec<String> {
    listing.visible().map(|entry| entry.name.clone()).collect()
}

/// Applies every verdict to `listing`, reading through `disk`.
fn apply(listing: &mut Listing, verdicts: Vec<Verdict>, disk: &[String]) {
    let read = disk_of(disk);
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
    let disk = vec!["f".to_string(), "f~".to_string()];
    let mut listing = listing_of(&["f"]);
    let verdicts = settle(&[
        RawChange::RenamedFrom(p("f")),
        RawChange::RenamedTo(p("f~")),
        RawChange::Created(p("f")),
    ]);
    apply(&mut listing, verdicts, &disk);
    assert_eq!(shown(&listing), disk);
}

/// Log rotation: every file moves one name up, oldest first, and a new file is written.
#[test]
fn log_rotation_moves_the_oldest_file_first() {
    let disk = vec![
        "f".to_string(),
        "f.1".to_string(),
        "f.2".to_string(),
        "f.3".to_string(),
    ];
    let mut listing = listing_of(&["f", "f.1", "f.2", "f.3"]);
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
    assert_eq!(shown(&listing), disk);
}

/// A swap of two names through a temporary one.
#[test]
fn a_swap_through_a_temporary_name_keeps_both_entries() {
    let disk = vec!["a".to_string(), "b".to_string()];
    let mut listing = listing_of(&["a", "b"]);
    let verdicts = settle(&[
        RawChange::RenamedFrom(p("a")),
        RawChange::RenamedTo(p("t")),
        RawChange::RenamedFrom(p("b")),
        RawChange::RenamedTo(p("a")),
        RawChange::RenamedFrom(p("t")),
        RawChange::RenamedTo(p("b")),
    ]);
    apply(&mut listing, verdicts, &disk);
    assert_eq!(shown(&listing), disk);
}

/// A rename onto an existing name, and then a removal of that name: the list must lose both
/// names, and in particular the name the rename landed on.
#[test]
fn a_rename_onto_an_existing_name_that_is_then_removed_removes_both() {
    let disk: Vec<String> = Vec::new();
    let mut listing = listing_of(&["a", "b"]);
    let verdicts = settle(&[
        RawChange::RenamedFrom(p("a")),
        RawChange::RenamedTo(p("b")),
        RawChange::Removed(p("b")),
    ]);
    apply(&mut listing, verdicts, &disk);
    assert_eq!(shown(&listing), disk);
}

/// A rename onto an existing name whose result is then renamed again, as inotify reports it:
/// the two halves of each rename, then the pair carrying both names.
#[test]
fn a_rename_onto_an_existing_name_that_moves_again_keeps_the_chain() {
    let disk = vec!["c".to_string()];
    let mut listing = listing_of(&["a", "b"]);
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
    assert_eq!(shown(&listing), disk);
}

/// A chain through a name the list holds: the intermediate name's entry must be removed too,
/// because the first rename replaced it.
#[test]
fn a_rename_chain_removes_the_name_it_passed_through() {
    let disk = vec!["c".to_string()];
    let mut listing = listing_of(&["a", "b"]);
    let verdicts = settle(&[
        RawChange::RenamedFrom(p("a")),
        RawChange::RenamedTo(p("b")),
        RawChange::RenamedFrom(p("b")),
        RawChange::RenamedTo(p("c")),
    ]);
    apply(&mut listing, verdicts, &disk);
    assert_eq!(shown(&listing), disk);
}

// The model test: random operations on a made-up disk, the raw events the two platforms would
// give, and a list that must end up equal to the disk.

/// How a platform reports a rename.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    /// Windows: the old name, then the new name, as two events.
    Windows,
    /// inotify: the old name, the new name, and a pair carrying both.
    Inotify,
}

/// A made-up folder: the names on the disk, each with a generation, and the raw events the
/// operations so far would have produced.
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

    /// A creation, reported only when the name was free (a create of an existing name is not
    /// something a file manager does).
    fn create(&mut self, name: &str) {
        if !self.has(name) {
            self.generation += 1;
            self.disk.insert(name.to_string(), self.generation);
            self.events.push(RawChange::Created(p(name)));
        }
    }

    fn remove(&mut self, name: &str) {
        if self.disk.remove(name).is_some() {
            self.events.push(RawChange::Removed(p(name)));
        }
    }

    fn modify(&mut self, name: &str) {
        if self.has(name) {
            self.generation += 1;
            self.disk.insert(name.to_string(), self.generation);
            self.events.push(RawChange::Modified(p(name)));
        }
    }

    /// A rename, overwriting the target. The overwritten target's removal is not reported:
    /// that is what both platforms do, and it is why the report must say it.
    fn rename(&mut self, from: &str, to: &str) -> bool {
        let Some(generation) = self.disk.remove(from) else {
            return false;
        };
        self.disk.insert(to.to_string(), generation);
        self.events.push(RawChange::RenamedFrom(p(from)));
        self.events.push(RawChange::RenamedTo(p(to)));
        if self.style == Style::Inotify {
            self.events.push(RawChange::Renamed {
                from: p(from),
                to: p(to),
            });
        }
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

/// Runs one seed: random operations on the disk, the events of the given style, then the
/// coalescer, the list and the disk must agree.
fn one_seed(seed_number: u64, style: Style) {
    let names = ["a", "b", "c", "d"];
    let mut seed = seed_number.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut model = Model::new(&names, style);
    let before: Vec<&str> = model.disk.keys().map(String::as_str).collect();
    let mut listing = listing_of(&before);

    let steps = 1 + next(&mut seed) % 8;
    let mut description = Vec::new();
    for _ in 0..steps {
        let a = names[next(&mut seed) % names.len()];
        let b = names[next(&mut seed) % names.len()];
        let c = names[next(&mut seed) % names.len()];
        match next(&mut seed) % 7 {
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
                if model.rename(a, b) {
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
            _ => {
                // A chain through a name.
                if a != b && b != c && model.rename(a, b) {
                    model.rename(b, c);
                    description.push(format!("chain {a}->{b}->{c}"));
                }
            }
        }
    }

    let verdicts = settle(&model.events);
    let disk: Vec<String> = model.disk.keys().cloned().collect();
    apply(&mut listing, verdicts, &disk);
    assert_eq!(
        shown(&listing),
        disk,
        "seed {seed_number}, {style:?}: {}",
        description.join(", ")
    );
}

#[test]
fn random_operations_end_with_the_list_equal_to_the_disk() {
    for seed in 0..200 {
        one_seed(seed, Style::Windows);
        one_seed(seed, Style::Inotify);
    }
}
