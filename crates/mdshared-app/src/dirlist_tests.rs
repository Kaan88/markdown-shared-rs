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

/// Makes a folder link pointing at `target`; false where the machine does not allow it.
fn link_dir(target: &Path, link: &Path) -> bool {
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link).is_ok()
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
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

/// Sets the hidden attribute on the link itself, never on what it points at.
#[cfg(windows)]
fn hide_link(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, GetFileAttributesW, SetFileAttributesW,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a NUL-terminated wide string that outlives both calls, and neither call
    // keeps the pointer.
    let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
    assert_ne!(attributes, u32::MAX, "the link is there: {path:?}");
    // SAFETY: the same NUL-terminated string, still alive; the call only reads it.
    let done = unsafe { SetFileAttributesW(wide.as_ptr(), attributes | FILE_ATTRIBUTE_HIDDEN) };
    assert_ne!(done, 0, "the link's own attribute is set: {path:?}");
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
fn the_systems_way_of_hiding_a_file_is_read() {
    let d = Dir::new("hidden-read");
    d.file(".dot", b"x");
    d.file("plain", b"x");
    #[cfg(windows)]
    {
        let plain = d.0.join("plain");
        make_hidden(&plain);
    }
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
fn links_are_listed_as_other_and_never_followed() {
    let d = Dir::new("links");
    d.dir("real");
    if !link_dir(Path::new("real"), &d.0.join("to-folder")) {
        eprintln!("skipped: this machine does not let a test make a symbolic link");
        return;
    }
    if !link_dir(Path::new("missing"), &d.0.join("broken"))
        || !link_dir(Path::new("loop"), &d.0.join("loop"))
    {
        eprintln!("skipped: a link to a missing path or to itself could not be made");
        return;
    }
    let entries = read(&d.0, &|| false);
    let kind = |name: &str| entries.iter().find(|e| e.name == name).map(|e| e.kind);
    assert_eq!(
        kind("to-folder"),
        Some(Kind::Other),
        "a link to a folder is not followed"
    );
    assert_eq!(
        kind("broken"),
        Some(Kind::Other),
        "a dead link is still listed"
    );
    assert_eq!(kind("loop"), Some(Kind::Other), "a loop is not entered");
    assert_eq!(kind("real"), Some(Kind::Folder));
    assert_eq!(entries.len(), 4, "nothing was skipped: {entries:?}");
}

#[cfg(windows)]
#[test]
fn a_hidden_junction_is_listed_and_hidden() {
    let d = Dir::new("junction");
    let target = d.dir("target");
    let link = d.0.join("junction");
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if !made {
        eprintln!("skipped: mklink /J is not available here");
        return;
    }
    hide_link(&link);
    let entries = read(&d.0, &|| false);
    let junction = entries
        .iter()
        .find(|e| e.name == "junction")
        .expect("the junction is listed");
    assert!(
        junction.hidden,
        "the link's own hidden attribute is what hides it"
    );
    assert_eq!(
        junction.kind,
        Kind::Other,
        "a junction is a reparse point, which the scan reports as a link, so it is not followed"
    );
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

    // Size, the folders first and then the files by size; equal sizes fall back to the name, so
    // the order is the same however the folder listed them.
    listing.set_sort(Sort {
        column: Column::Size,
        descending: false,
    });
    assert_eq!(
        names(&listing),
        ["adir", "zdir", "dev", "file2", "file1", "file10"]
    );
    listing.set_sort(Sort {
        column: Column::Size,
        descending: true,
    });
    assert_eq!(
        names(&listing),
        ["adir", "zdir", "file10", "file1", "file2", "dev"]
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

    // Created: unknown for every entry, so the name decides every tie.
    listing.set_sort(Sort {
        column: Column::Created,
        descending: false,
    });
    assert_eq!(
        names(&listing),
        ["adir", "zdir", "dev", "file1", "file2", "file10"]
    );

    // Kind: folders first, then file, then other.
    listing.set_sort(Sort {
        column: Column::Kind,
        descending: false,
    });
    assert_eq!(
        names(&listing),
        ["adir", "zdir", "file1", "file2", "file10", "dev"]
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
    // Equal sizes, so the name tiebreak is what orders the files under the size column.
    d.file("a.txt", b"11");
    d.file("b.txt", b"11");
    d.file("c.txt", b"11");
    d.file("m.txt", b"11");
    d.dir("s");
    let before = read(&d.0, &|| false);

    // The disk changes in the five ways a list must follow.
    let created = d.file("d.txt", b"11");
    fs::remove_file(d.0.join("b.txt")).unwrap();
    fs::rename(d.0.join("a.txt"), d.0.join("z.txt")).unwrap();
    fs::write(d.0.join("m.txt"), b"more").unwrap();
    fs::remove_file(d.0.join("c.txt")).unwrap();
    fs::create_dir(d.0.join("c.txt")).unwrap();

    let changes = [
        Change::Created(Entry::at(&created).unwrap()),
        Change::Removed(d.0.join("b.txt")),
        Change::Renamed {
            from: d.0.join("a.txt"),
            to: Entry::at(d.0.join("z.txt")).unwrap(),
        },
        Change::Modified(Entry::at(d.0.join("m.txt")).unwrap()),
        Change::Modified(Entry::at(d.0.join("c.txt")).unwrap()),
    ];
    let after = read(&d.0, &|| false);

    // Applied one by one, the list is the one a fresh reading gives, under every column and
    // both directions.
    for column in [
        Column::Name,
        Column::Kind,
        Column::Size,
        Column::Modified,
        Column::Created,
    ] {
        for descending in [false, true] {
            let sort = Sort { column, descending };
            let mut incremental = Listing::new();
            incremental.set_folder(Some(d.0.clone()));
            incremental.set_entries(before.clone());
            incremental.set_sort(sort);
            for change in changes.clone() {
                incremental.apply(change);
            }
            let mut fresh = Listing::new();
            fresh.set_folder(Some(d.0.clone()));
            fresh.set_entries(after.clone());
            fresh.set_sort(sort);
            assert_eq!(
                incremental.visible().cloned().collect::<Vec<_>>(),
                fresh.visible().cloned().collect::<Vec<_>>(),
                "{column:?}, descending {descending}"
            );
        }
    }
    assert_eq!(
        names(&freshly_read(&d.0)),
        ["c.txt", "s", "d.txt", "m.txt", "z.txt"]
    );
}

/// A fresh reading of the folder, by name.
fn freshly_read(dir: &Path) -> Listing {
    let mut listing = Listing::new();
    listing.set_entries(read(dir, &|| false));
    listing
}

#[test]
fn a_rename_onto_a_listed_name_replaces_it() {
    let mut listing = Listing::new();
    listing.set_entries(vec![
        entry("a", Kind::File, 1, None),
        entry("b", Kind::File, 2, None),
    ]);
    let remap = listing.apply(Change::Renamed {
        from: PathBuf::from("/made/up/a"),
        to: entry("b", Kind::Folder, 0, None),
    });
    assert_eq!(names(&listing), ["b"], "the name is listed once");
    assert_eq!(listing.entry(0).unwrap().kind, Kind::Folder);
    assert_eq!(
        remap.get(0),
        Some(0),
        "the row of \"a\" follows to the new \"b\""
    );
    assert_eq!(remap.get(1), None, "the old \"b\" was replaced");
}

#[test]
fn a_change_outside_the_folder_is_ignored() {
    let folder = PathBuf::from("/made/up");
    let elsewhere = |name: &str| Entry {
        path: PathBuf::from("/elsewhere").join(name),
        name: name.to_string(),
        kind: Kind::File,
        size: 1,
        modified: None,
        created: None,
        hidden: false,
    };
    let mut listing = Listing::new();
    listing.set_folder(Some(folder.clone()));
    listing.set_entries(vec![entry("a", Kind::File, 1, None)]);

    // A creation outside changes nothing.
    let remap = listing.apply(Change::Created(elsewhere("b")));
    assert_eq!(listing.len(), 1);
    assert!(remap.unchanged());
    // A removal of a path outside changes nothing.
    listing.apply(Change::Removed(PathBuf::from("/elsewhere/a")));
    assert_eq!(listing.len(), 1);
    // The folder itself is not one of its entries.
    listing.apply(Change::Removed(folder.clone()));
    assert_eq!(listing.len(), 1);

    // A rename out of the folder is a removal.
    let remap = listing.apply(Change::Renamed {
        from: folder.join("a"),
        to: elsewhere("a"),
    });
    assert!(listing.is_empty());
    assert_eq!(remap.get(0), None);

    // A rename into the folder is a creation.
    listing.apply(Change::Renamed {
        from: PathBuf::from("/elsewhere/b"),
        to: entry("b", Kind::File, 1, None),
    });
    assert_eq!(names(&listing), ["b"]);
}

#[test]
fn a_cancelled_read_reads_nothing() {
    let d = Dir::new("cancel-none");
    d.file("a.txt", b"x");
    assert!(read(&d.0, &|| true).is_empty());
    assert!(read_checked(&d.0, &|| true).unwrap().is_empty());
}

#[test]
fn a_cancel_part_way_stops_the_read() {
    let d = Dir::new("cancel-part");
    for i in 0..5 {
        d.file(&format!("f{i}"), b"x");
    }
    let asked = Cell::new(0);
    let entries = read(&d.0, &|| {
        let n = asked.get();
        asked.set(n + 1);
        n >= 2
    });
    assert_eq!(entries.len(), 2, "{entries:?}");
}

#[test]
fn a_cancelled_folder_read_hands_nothing_over() {
    let d = Dir::new("cancel-worker");
    d.file("a.txt", b"x");
    // Cancelled before the thread could notice, the handle still gives nothing, even though the
    // work may have read the folder to its end.
    let mut job = FolderRead::start(d.0.clone(), || {}).unwrap();
    job.cancel();
    wait_until(|| !job.running());
    assert!(job.take().is_none());
}

#[test]
fn a_folder_read_on_a_worker_hands_its_entries_over_once() {
    let d = Dir::new("worker");
    d.file("a.txt", b"x");
    let mut job = FolderRead::start(d.0.clone(), || {}).unwrap();
    assert_eq!(job.dir(), d.0.as_path());
    wait_until(|| !job.running());
    let (dir, entries) = job.take().expect("the read ends with its entries");
    assert_eq!(dir, d.0);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "a.txt");
    assert!(job.take().is_none(), "a result is handed over once");

    // A folder that cannot be opened is an error through the checked take.
    let mut job = FolderRead::start(d.0.join("missing"), || {}).unwrap();
    wait_until(|| !job.running());
    let (_, result) = job.take_checked().expect("the read ends");
    assert!(result.is_err(), "a folder that cannot be opened says so");
}

#[test]
fn a_folder_that_cannot_be_opened_says_so_and_a_vanished_entry_is_skipped() {
    let d = Dir::new("unreadable");
    let kept = d.file("kept.txt", b"x");
    let other = d.file("other.txt", b"y");
    // One entry, whose details are asked for after it has gone: the skip path itself, without
    // depending on the order the folder would list its entries in. On Windows the folder scan's
    // own record still describes a deleted entry, so the entry is asked of the disk only here.
    #[cfg(not(windows))]
    {
        let gone = d.file("gone.txt", b"z");
        let mut items = fs::read_dir(&d.0).unwrap();
        let item = items
            .find(|item| {
                item.as_ref()
                    .is_ok_and(|item| item.file_name() == "gone.txt")
            })
            .expect("the folder lists gone.txt")
            .unwrap();
        fs::remove_file(&gone).unwrap();
        assert!(
            read_entry(&item).is_none(),
            "an entry that vanished is skipped"
        );
    }
    // One entry whose details cannot be had is skipped, and the others still arrive.
    let partial = read_with(&d.0, &|| false, &|item| {
        if item.file_name() == "other.txt" {
            None
        } else {
            read_entry(item)
        }
    })
    .unwrap();
    let names: Vec<String> = partial.iter().map(|e| e.name.clone()).collect();
    assert_eq!(
        names,
        ["kept.txt"],
        "the unreadable one is skipped, the other stays"
    );
    assert!(kept.is_file() && other.is_file());

    // A folder that cannot be opened is an error, and reads as nothing.
    let missing = d.0.join("missing");
    assert!(read_checked(&missing, &|| false).is_err());
    assert!(read(&missing, &|| false).is_empty());
    assert!(Entry::at(&missing).is_none());
}

#[test]
fn two_paths_that_differ_only_in_case_are_two_entries() {
    let upper = entry("A.txt", Kind::File, 1, None);
    let lower = entry("a.txt", Kind::File, 1, None);
    let mut listing = Listing::new();
    listing.set_entries(vec![upper.clone(), lower.clone()]);
    assert_eq!(listing.len(), 2, "two paths, two rows");
    // The names owe their order to the path's bytes once the case-folding key ties.
    assert_eq!(listing.row_of(&upper.path), Some(0));
    assert_eq!(listing.row_of(&lower.path), Some(1));
    listing.apply(Change::Removed(upper.path.clone()));
    assert_eq!(listing.len(), 1, "one path removed, one left");
    assert_eq!(listing.row_of(&lower.path), Some(0));
}

#[test]
fn two_names_that_differ_only_in_case_are_two_entries() {
    let d = Dir::new("case");
    d.file("A.txt", b"a");
    let second = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(d.0.join("a.txt"));
    if second.is_err() {
        eprintln!("skipped: this file system does not tell A.txt from a.txt");
        return;
    }
    let mut listing = Listing::new();
    listing.set_entries(read(&d.0, &|| false));
    assert_eq!(names(&listing), ["A.txt", "a.txt"], "one entry each");
    assert_eq!(listing.row_of(&d.0.join("A.txt")), Some(0));
    assert_eq!(listing.row_of(&d.0.join("a.txt")), Some(1));
    // A change for one of them moves one entry only.
    let remap = listing.apply(Change::Removed(d.0.join("A.txt")));
    assert_eq!(remap.get(0), None);
    assert_eq!(names(&listing), ["a.txt"]);
}

#[test]
fn a_remap_answers_nothing_for_a_row_that_never_was() {
    let mut listing = Listing::new();
    listing.set_entries(vec![
        entry("a", Kind::File, 1, None),
        entry("b", Kind::File, 2, None),
    ]);
    // A change's remap follows edits; a row past the list's old end has no answer.
    let remap = listing.apply(Change::Created(entry("c", Kind::File, 3, None)));
    assert_eq!(remap.len(), 2);
    assert_eq!(remap.get(2), None, "there was no third row");
    assert_eq!(remap.get(usize::MAX), None, "nor a row at the end of usize");
    assert_eq!(remap.mapping()(2), None);
    // A rebuilt view's remap is the same.
    let remap = listing.set_sort(Sort {
        column: Column::Size,
        descending: true,
    });
    assert_eq!(remap.len(), 3);
    assert_eq!(remap.get(3), None);
    assert_eq!(remap.get(usize::MAX), None);
}

#[test]
fn a_filter_keeps_the_sorted_order_and_sorts_nothing_again() {
    let mut entries = Vec::with_capacity(100_000);
    for i in 0..100_000 {
        entries.push(entry(
            &format!("file{i:06}"),
            Kind::File,
            (i % 97) as u64,
            at((i % 1000) as u64),
        ));
    }
    let mut listing = Listing::new();
    listing.set_entries(entries);
    listing.set_sort(Sort {
        column: Column::Size,
        descending: true,
    });
    let before: Vec<String> = names(&listing);

    // A narrowed filter keeps the rows it shows in the order they had.
    listing.set_filter("file00000");
    let narrowed: Vec<String> = names(&listing);
    let kept: Vec<String> = before
        .iter()
        .filter(|name| name.contains("file00000"))
        .cloned()
        .collect();
    assert_eq!(narrowed, kept, "a filter keeps the order");

    // A keystroke filters; it does not sort again. A hundred widenings and narrowings of a
    // hundred thousand rows stay far inside the bound (a re-sort each time would not).
    let started = Instant::now();
    for _ in 0..50 {
        listing.set_filter("");
        listing.set_filter("file");
    }
    let elapsed = started.elapsed();
    listing.set_filter("");
    assert_eq!(
        names(&listing),
        before,
        "clearing the filter restores the order"
    );
    eprintln!("100 filter changes on 100,000 entries took {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(4),
        "100 filter changes on 100,000 entries took {elapsed:?}"
    );
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

#[test]
fn a_thousand_changes_on_a_hundred_thousand_entries_are_quick() {
    let mut entries = Vec::with_capacity(100_000);
    for i in 0..100_000 {
        entries.push(entry(
            &format!("file{i:06}"),
            Kind::File,
            (i % 97) as u64,
            at((i % 1000) as u64),
        ));
    }
    let mut listing = Listing::new();
    listing.set_entries(entries);
    let started = Instant::now();
    for k in 0..1_000 {
        listing.apply(Change::Created(entry(
            &format!("a{k:04}"),
            Kind::File,
            5,
            at(1),
        )));
    }
    let elapsed = started.elapsed();
    assert_eq!(listing.len(), 101_000);
    eprintln!("1,000 changes on 100,000 entries took {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(2),
        "1,000 changes on 100,000 entries took {elapsed:?}"
    );
}
