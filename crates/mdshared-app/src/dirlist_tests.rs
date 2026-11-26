use super::*;
use std::cell::Cell;
use std::fs;
use std::time::{Duration, Instant};

/// A folder of its own under the temporary folder, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let dir =
            std::env::temp_dir().join(format!("mdshared-dirlist-{name}-{}", std::process::id()));
        _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn file(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }

    fn dir(&self, rel: &str) -> PathBuf {
        let path = self.0.join(rel);
        fs::create_dir_all(&path).unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        _ = fs::remove_dir_all(&self.0);
    }
}

fn names(listing: &Listing) -> Vec<String> {
    listing.visible().map(|e| e.name.clone()).collect()
}

/// An entry of a made-up folder, for the sorts and the filters, which are pure arithmetic.
fn entry(name: &str, kind: Kind, size: u64, modified: Option<SystemTime>) -> Entry {
    Entry {
        path: PathBuf::from("/made/up").join(name),
        name: name.to_string(),
        kind,
        size,
        modified,
        created: None,
        hidden: false,
    }
}

fn at(secs: u64) -> Option<SystemTime> {
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
}

/// Waits for a worker to end, up to five seconds, so a broken one fails a test instead of
/// hanging it.
fn wait_until(mut done: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < end, "the worker did not finish in time");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Marks `path` hidden with the system's own tool; the attribute is what Windows hides by.
#[cfg(windows)]
fn make_hidden(path: &Path) {
    let status = std::process::Command::new("attrib")
        .arg("+h")
        .arg(path)
        .status()
        .expect("attrib runs");
    assert!(status.success(), "attrib +h {path:?} failed");
}

#[test]
fn reading_a_folder_gives_its_entries_and_their_kinds() {
    let d = Dir::new("read");
    d.dir("sub");
    d.file("a.txt", b"hello");
    d.file("noext", b"");
    let mut listing = Listing::new();
    let remap = listing.set_folder(Some(d.0.clone()));
    assert!(remap.is_empty(), "the first view has no old rows");
    let remap = listing.set_entries(read(&d.0, &|| false));
    assert!(remap.is_empty());

    assert_eq!(listing.folder(), Some(d.0.as_path()));
    assert_eq!(names(&listing), ["sub", "a.txt", "noext"]);
    assert_eq!(listing.len(), 3);
    assert!(!listing.is_empty());
    assert_eq!(listing.entry(3), None);
    assert_eq!(listing.path_at(3), None);

    let sub = listing.entry(0).unwrap();
    assert_eq!(sub.kind, Kind::Folder);
    assert_eq!(sub.path, d.0.join("sub"));
    assert_eq!(sub.name, "sub");
    assert!(!sub.hidden);

    let file = listing.entry(1).unwrap();
    assert_eq!(file.kind, Kind::File);
    assert_eq!(file.path, d.0.join("a.txt"));
    assert_eq!(file.size, 5);
    assert!(file.modified.is_some(), "the file was just written");
    assert!(!file.hidden);

    assert_eq!(listing.row_of(&d.0.join("a.txt")), Some(1));
    assert_eq!(listing.row_of(&d.0.join("noext")), Some(2));
    assert_eq!(listing.row_of(&d.0.join("missing")), None);
    assert_eq!(
        listing.all().len(),
        3,
        "every entry is kept, not only the shown ones"
    );
    assert_eq!(listing.sort(), Sort::DEFAULT);
    assert_eq!(listing.filter(), "");
    assert!(!listing.show_hidden());
}

#[test]
fn the_systems_way_of_hiding_an_entry_is_read() {
    let d = Dir::new("hidden-read");
    d.file(".dot", b"x");
    let plain = d.file("plain", b"x");
    #[cfg(windows)]
    make_hidden(&plain);
    let entries = read(&d.0, &|| false);
    let named = |name: &str| {
        entries
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} was not read: {entries:?}"))
    };
    #[cfg(windows)]
    {
        assert!(named("plain").hidden, "the attribute hides it");
        assert!(
            !named(".dot").hidden,
            "Windows hides by attribute, not by a leading dot"
        );
    }
    #[cfg(not(windows))]
    {
        assert!(named(".dot").hidden, "a leading dot hides it");
        assert!(!named("plain").hidden);
    }
}

#[test]
fn every_column_sorts_with_the_folders_first_and_names_naturally() {
    let entries = vec![
        entry("file10", Kind::File, 300, at(30)),
        entry("file2", Kind::File, 100, at(10)),
        entry("zdir", Kind::Folder, 0, at(20)),
        entry("adir", Kind::Folder, 0, at(40)),
        entry("file1", Kind::File, 200, at(20)),
        entry("dev", Kind::Other, 0, None),
    ];
    let mut listing = Listing::new();
    listing.set_entries(entries);

    // Name, ascending: folders first, then the names in their natural order.
    assert_eq!(
        names(&listing),
        ["adir", "zdir", "dev", "file1", "file2", "file10"]
    );
    // Descending: folders stay first, and each group turns round.
    listing.set_sort(Sort {
        column: Column::Name,
        descending: true,
    });
    assert_eq!(
        names(&listing),
        ["zdir", "adir", "file10", "file2", "file1", "dev"]
    );

    // Size, the folders first and then the files by size; the equal folders keep the order
    // the folder gave them, which is what a stable sort promises.
    listing.set_sort(Sort {
        column: Column::Size,
        descending: false,
    });
    assert_eq!(
        names(&listing),
        ["zdir", "adir", "dev", "file2", "file1", "file10"]
    );
    listing.set_sort(Sort {
        column: Column::Size,
        descending: true,
    });
    assert_eq!(
        names(&listing),
        ["zdir", "adir", "file10", "file1", "file2", "dev"]
    );

    // Modified, with an unknown time first and the folders by their own times.
    listing.set_sort(Sort {
        column: Column::Modified,
        descending: false,
    });
    assert_eq!(
        names(&listing),
        ["zdir", "adir", "dev", "file2", "file1", "file10"]
    );

    // Created: unknown for every entry, so each group keeps the read order.
    listing.set_sort(Sort {
        column: Column::Created,
        descending: false,
    });
    assert_eq!(
        names(&listing),
        ["zdir", "adir", "file10", "file2", "file1", "dev"]
    );

    // Kind: folders first, then file, then other.
    listing.set_sort(Sort {
        column: Column::Kind,
        descending: false,
    });
    assert_eq!(
        names(&listing),
        ["zdir", "adir", "file10", "file2", "file1", "dev"]
    );
}

#[test]
fn a_click_on_a_column_turns_it_round_and_folders_stay_first() {
    let mut listing = Listing::new();
    listing.set_entries(vec![
        entry("b", Kind::File, 1, None),
        entry("a", Kind::File, 1, None),
        entry("d", Kind::Folder, 1, None),
    ]);
    // A new list is sorted by name, ascending, so the first click turns it round.
    listing.toggle_sort(Column::Name);
    assert!(listing.sort().descending);
    assert_eq!(names(&listing), ["d", "b", "a"]);
    listing.toggle_sort(Column::Name);
    assert_eq!(
        listing.sort(),
        Sort::DEFAULT,
        "the second click is ascending"
    );
    assert_eq!(names(&listing), ["d", "a", "b"]);
    listing.toggle_sort(Column::Name);
    assert!(listing.sort().descending);
    // Another column starts ascending, whatever the old direction was.
    let remap = listing.toggle_sort(Column::Size);
    assert_eq!(
        listing.sort(),
        Sort {
            column: Column::Size,
            descending: false
        }
    );
    assert!(!remap.is_empty(), "three rows were shown before");
    assert!(!listing.is_empty());
}

#[test]
fn filtering_and_the_hidden_switch_narrow_the_list() {
    let mut secret = entry(".secret", Kind::File, 3, None);
    secret.hidden = true;
    let mut hidden_dir = entry(".hidden-dir", Kind::Folder, 0, None);
    hidden_dir.hidden = true;
    let mut listing = Listing::new();
    listing.set_entries(vec![
        entry("Report.TXT", Kind::File, 1, None),
        entry("notes.txt", Kind::File, 2, None),
        entry("picture.png", Kind::File, 4, None),
        secret,
        hidden_dir,
    ]);

    assert_eq!(
        names(&listing),
        ["notes.txt", "picture.png", "Report.TXT"],
        "hidden entries are left out"
    );
    // The filter ignores case and matches anywhere in the name.
    listing.set_filter("txt");
    assert_eq!(names(&listing), ["notes.txt", "Report.TXT"]);
    assert_eq!(listing.filter(), "txt");
    listing.set_filter("zzz");
    assert!(names(&listing).is_empty(), "nothing matches");
    assert!(listing.is_empty());
    assert_eq!(listing.len(), 0);

    // The switch shows the hidden ones, and the filter still applies to them.
    listing.set_filter("");
    let remap = listing.set_show_hidden(true);
    assert!(listing.show_hidden());
    assert_eq!(
        names(&listing),
        [
            ".hidden-dir",
            ".secret",
            "notes.txt",
            "picture.png",
            "Report.TXT"
        ]
    );
    // The old row of "notes.txt" was 0, and it is row 2 now.
    assert_eq!(remap.get(0), Some(2));
    assert!(!remap.unchanged());
    assert_eq!(remap.len(), 3);

    // A filter narrows along with the switch, and the remap says what was dropped.
    let remap = listing.set_filter("secret");
    assert_eq!(names(&listing), [".secret"]);
    assert_eq!(remap.get(0), None, "the hidden folder is filtered out");
    assert_eq!(remap.get(1), Some(0), "the hidden file is the only row");
    listing.set_show_hidden(false);
    assert!(
        names(&listing).is_empty(),
        "a hidden match is not shown while the switch is off"
    );
}

#[test]
fn applying_a_change_gives_the_same_list_as_a_full_read() {
    let d = Dir::new("apply");
    d.file("a.txt", b"one");
    d.file("b.txt", b"two");
    d.file("m.txt", b"three");
    d.dir("s");

    let mut listing = Listing::new();
    listing.set_folder(Some(d.0.clone()));
    listing.set_entries(read(&d.0, &|| false));
    assert_eq!(names(&listing), ["s", "a.txt", "b.txt", "m.txt"]);

    // The disk changes the four ways a watcher reports.
    let created = d.file("c.txt", b"four");
    fs::remove_file(d.0.join("b.txt")).unwrap();
    fs::rename(d.0.join("a.txt"), d.0.join("z.txt")).unwrap();
    fs::write(d.0.join("m.txt"), b"three and more").unwrap();
    // And one the watcher never reported as a create: a change to a file not in the list.
    let missed = d.file("n.txt", b"new");

    let remap = listing.apply(Change::Created(Entry::at(&created).unwrap()));
    assert_eq!(remap.len(), 4);
    assert!(!remap.unchanged(), "a row was added");
    listing.apply(Change::Removed(d.0.join("b.txt")));
    let remap = listing.apply(Change::Renamed {
        from: d.0.join("a.txt"),
        to: Entry::at(d.0.join("z.txt")).unwrap(),
    });
    // "a.txt" was row 1; "z.txt" is the last row of the files now.
    assert_eq!(remap.get(1), Some(3));
    listing.apply(Change::Modified(Entry::at(d.0.join("m.txt")).unwrap()));
    let remap = listing.apply(Change::Modified(Entry::at(&missed).unwrap()));
    // "z.txt", the last row, was pushed down by the file the watcher missed.
    assert_eq!(remap.get(3), Some(4));
    assert_eq!(remap.len(), 4);

    let mut fresh = Listing::new();
    fresh.set_entries(read(&d.0, &|| false));
    assert_eq!(
        listing.all().len(),
        fresh.all().len(),
        "every entry is either applied or read"
    );
    assert_eq!(
        listing.visible().cloned().collect::<Vec<_>>(),
        fresh.visible().cloned().collect::<Vec<_>>(),
        "the applied list is the one a full read gives"
    );
    assert_eq!(names(&listing), ["s", "c.txt", "m.txt", "n.txt", "z.txt"]);
    assert_eq!(listing.entry(2).unwrap().size, 14, "the change was applied");
}

#[test]
fn a_cancelled_read_delivers_nothing() {
    let d = Dir::new("cancel");
    for i in 0..5 {
        d.file(&format!("f{i}"), b"x");
    }
    // Asked before the first entry, the flag stops the read at once.
    assert!(read(&d.0, &|| true).is_empty());

    // Raised after two entries, it stops the read there: a dead folder is abandoned.
    let asked = Cell::new(0);
    let entries = read(&d.0, &|| {
        let n = asked.get();
        asked.set(n + 1);
        n >= 2
    });
    assert_eq!(entries.len(), 2, "{entries:?}");

    // The worker's handle throws away what a cancelled read had read, even when its thread
    // has already finished.
    let mut job = FolderRead::start(d.0.clone(), || {}).unwrap();
    assert_eq!(job.dir(), d.0.as_path());
    job.cancel();
    wait_until(|| !job.running());
    assert!(
        job.take().is_none(),
        "a cancelled read delivers nothing, though it may have read entries"
    );
}

#[test]
fn a_folder_read_on_a_worker_hands_its_entries_over_once() {
    let d = Dir::new("worker");
    d.file("a.txt", b"x");
    let mut job = FolderRead::start(d.0.clone(), || {}).unwrap();
    wait_until(|| !job.running());
    let (dir, entries) = job.take().expect("the read ends with its entries");
    assert_eq!(dir, d.0);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "a.txt");
    assert!(job.take().is_none(), "a result is handed over once");
}

#[test]
fn unreadable_entries_are_skipped_without_a_panic() {
    let d = Dir::new("unreadable");
    d.file("kept.txt", b"x");
    d.dir("gone");
    // The folder is deleted between being listed and its details being asked for: its
    // entry cannot be read, and the read goes on.
    let first = Cell::new(true);
    let entries = read(&d.0, &|| {
        if first.replace(false) {
            let _ = fs::remove_dir_all(d.0.join("gone"));
        }
        false
    });
    assert!(
        entries.iter().all(|e| e.name != "gone"),
        "an entry that vanished is skipped: {entries:?}"
    );
    assert!(entries.iter().any(|e| e.name == "kept.txt"));

    // A folder that cannot be opened, and a path that cannot be read, give nothing.
    assert!(read(&d.0.join("missing"), &|| false).is_empty());
    assert!(Entry::at(d.0.join("missing")).is_none());
}

#[test]
fn a_re_sort_and_a_change_say_where_each_row_went() {
    let mut listing = Listing::new();
    listing.set_entries(vec![
        entry("a", Kind::File, 3, at(1)),
        entry("b", Kind::File, 2, at(2)),
        entry("c", Kind::File, 1, at(3)),
    ]);
    let remap = listing.set_sort(Sort {
        column: Column::Size,
        descending: false,
    });
    assert_eq!(
        (remap.get(0), remap.get(1), remap.get(2)),
        (Some(2), Some(1), Some(0))
    );
    assert_eq!(remap.mapping()(0), Some(2));
    assert_eq!(remap.get(3), None, "there was no fourth row");
    assert!(!remap.unchanged());

    // A rename follows the entry to its new row.
    let remap = listing.apply(Change::Renamed {
        from: PathBuf::from("/made/up/c"),
        to: entry("z", Kind::File, 1, at(3)),
    });
    // The old row 0 was "c": the entry is "z" now, and the row follows it there. "b" and
    // "a" keep their rows.
    assert_eq!(remap.get(0), Some(0));
    assert_eq!(remap.get(1), Some(1));
    assert_eq!(remap.get(2), Some(2));
    assert_eq!(
        listing
            .visible()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        ["z", "b", "a"]
    );
    // The path of the old entry is gone, the new one is found.
    assert_eq!(listing.row_of(Path::new("/made/up/c")), None);
    assert_eq!(listing.row_of(Path::new("/made/up/z")), Some(0));

    // A removal was in the old view and is in none now.
    let remap = listing.apply(Change::Removed(PathBuf::from("/made/up/b")));
    assert_eq!(remap.get(0), Some(0));
    assert_eq!(remap.get(1), None);
    assert_eq!(remap.get(2), Some(1));
}

#[test]
fn a_change_to_an_entry_not_listed_inserts_it_and_one_already_there_replaces_it() {
    let mut listing = Listing::new();
    listing.set_entries(vec![entry("a", Kind::File, 1, None)]);
    let remap = listing.apply(Change::Modified(entry("a", Kind::File, 9, None)));
    assert_eq!(listing.len(), 1);
    assert_eq!(listing.entry(0).unwrap().size, 9);
    assert!(remap.unchanged());
    listing.apply(Change::Created(entry("a", Kind::Folder, 0, None)));
    assert_eq!(listing.len(), 1, "the same path is not listed twice");
    assert_eq!(listing.entry(0).unwrap().kind, Kind::Folder);
    listing.apply(Change::Removed(PathBuf::from("/made/up/none")));
    assert_eq!(
        listing.len(),
        1,
        "removing what is not there changes nothing"
    );
}
