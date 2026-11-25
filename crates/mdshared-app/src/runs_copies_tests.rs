//! The record of "Copy them in" ([`Kind::Copies`]) and its undo: it takes away only the copies
//! it made, while they are the bytes it wrote and nothing else lists them, and a picture copy
//! only while its original is still there as it was.

use super::*;
use crate::moves::copy_in::{self, Wanted};

struct Run1 {
    dir: PathBuf,
    library: PathBuf,
    group: PathBuf,
    record_path: PathBuf,
    record: Record,
    before: String,
    copy: PathBuf,
    linked: PathBuf,
    picture: PathBuf,
    original_picture: PathBuf,
}

impl Drop for Run1 {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A run that copies `trip.md` (with the picture it shows) into the group's folder, and links
/// `kept.md`, a file already in the library, where it is.
fn run(tag: &str) -> Run1 {
    let dir = scratch(tag);
    let dir = crate::paths::canonical_or_absolute(&dir);
    let library = dir.join("library");
    let group = library.join("trips.md");
    write_file(&group, "# Trips\n");
    let linked = library.join("Old").join("kept.md");
    write_file(&linked, "# Kept\n");
    let md = dir.join("desk").join("trip.md");
    write_file(&md, "# Trip\n\n![Beach](img/beach.png)\n");
    let original_picture = dir.join("desk").join("img").join("beach.png");
    write_file(&original_picture, "PNG beach");
    let home = library.join("Trips");
    let wanted = [
        Wanted {
            from: md.clone(),
            dir: home.clone(),
        },
        Wanted {
            from: linked.clone(),
            dir: home.clone(),
        },
    ];
    let plan = copy_in::plan(&wanted, std::slice::from_ref(&library));
    let written = copy_in::write(&plan, &library, &home).expect("written");
    let copy = home.join("trip.md");
    let picture = home.join("img").join("beach.png");
    let links: Vec<Link> = plan
        .iter()
        .map(|p| {
            let title = p.from.file_stem().unwrap().to_string_lossy().into_owned();
            Link {
                path: p.target().to_path_buf(),
                title: title.clone(),
                sections: Vec::new(),
                line: library::link_line(&title, p.target()),
            }
        })
        .collect();
    let before = read(&group);
    std::fs::write(&group, with_links_added(&before, &links)).expect("the add");
    let g = Group {
        name: "Trips".to_string(),
        file: group.clone(),
        created: false,
        home: home.clone(),
    };
    let (record_path, record) =
        copies_record(&library, "", g, &links, &before, &written).expect("made");
    Run1 {
        dir,
        library,
        group,
        record_path,
        record,
        before,
        copy,
        linked,
        picture,
        original_picture,
    }
}

#[test]
fn a_run_of_copies_reads_back_with_its_copies_and_their_originals() {
    let r = run("copies-record");
    assert_eq!(r.record.kind, Kind::Copies);
    let loaded = load(&r.record_path, &r.library).expect("it reads back");
    assert_eq!(loaded.kind, Kind::Copies);
    let copy = loaded
        .notes
        .iter()
        .find(|n| same_path(&n.path, &r.copy))
        .expect("the copy");
    assert!(copy.created && !copy.sha256.is_empty());
    let linked = loaded
        .notes
        .iter()
        .find(|n| same_path(&n.path, &r.linked))
        .expect("the file linked where it is");
    assert!(!linked.created);
    assert_eq!(loaded.pictures.len(), 1);
    let p = &loaded.pictures[0];
    assert!(same_path(&p.path, &r.picture));
    assert!(same_path(
        p.original.as_deref().expect("its original"),
        &r.original_picture
    ));
    assert_eq!(p.original_sha256, r.record.pictures[0].original_sha256);
}

#[test]
fn undo_takes_away_the_copies_and_leaves_the_owner_s_files() {
    let r = run("copies-undo");
    let loaded = load(&r.record_path, &r.library).expect("it reads back");
    let plan = prepare(&loaded, &r.library);
    assert!(plan.refusal.is_none(), "{:?}", plan.refusal);
    let mut gate = TestGate::new();
    let report = undo(&r.record_path, &loaded, &r.library, &mut gate);
    assert!(!r.copy.exists(), "{report:?}");
    assert!(!r.picture.exists(), "{report:?}");
    assert!(
        !r.library.join("Trips").exists(),
        "the folders it made go too"
    );
    assert_eq!(read(&r.group), r.before);
    assert_eq!(
        read(&r.linked),
        "# Kept\n",
        "a file linked where it is is never touched"
    );
    assert_eq!(read(&r.original_picture), "PNG beach");
}

#[test]
fn a_copy_of_any_extension_is_taken_away_by_undo() {
    // A run of copies is the one kind whose notes may be other than `.md`.
    let dir = crate::paths::canonical_or_absolute(&scratch("copies-pdf"));
    let library = dir.join("library");
    let group = library.join("docs.md");
    write_file(&group, "# Docs\n");
    let from = dir.join("desk").join("menu.pdf");
    write_file(&from, "%PDF-1.4\n");
    let home = library.join("Docs");
    let plan = copy_in::plan(
        &[Wanted {
            from: from.clone(),
            dir: home.clone(),
        }],
        std::slice::from_ref(&library),
    );
    let written = copy_in::write(&plan, &library, &home).expect("written");
    let to = home.join("menu.pdf");
    let links = vec![Link {
        path: to.clone(),
        title: "Menu".to_string(),
        sections: Vec::new(),
        line: library::link_line("Menu", &to),
    }];
    let before = read(&group);
    std::fs::write(&group, with_links_added(&before, &links)).expect("the add");
    let g = Group {
        name: "Docs".to_string(),
        file: group.clone(),
        created: false,
        home,
    };
    let (path, record) = copies_record(&library, "", g, &links, &before, &written).expect("made");
    let mut gate = TestGate::new();
    let report = undo(&path, &record, &library, &mut gate);
    assert!(!to.exists(), "{report:?}");
    assert_eq!(read(&from), "%PDF-1.4\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn undo_keeps_a_copy_that_was_changed_since() {
    let r = run("copies-changed");
    write_file(&r.copy, "# Trip\n\nI wrote more.\n");
    let mut gate = TestGate::new();
    let report = undo(&r.record_path, &r.record, &r.library, &mut gate);
    assert_eq!(read(&r.copy), "# Trip\n\nI wrote more.\n", "{report:?}");
}

#[test]
fn undo_keeps_a_picture_copy_whose_original_is_gone() {
    let r = run("copies-orphan");
    std::fs::remove_file(&r.original_picture).expect("the owner removed it");
    let plan = prepare(&r.record, &r.library);
    assert!(
        plan.pictures_kept.iter().any(|p| same_path(p, &r.picture)),
        "{plan:?}"
    );
    let mut gate = TestGate::new();
    let report = undo(&r.record_path, &r.record, &r.library, &mut gate);
    assert!(
        r.picture.exists(),
        "the copy may be the only one now: {report:?}"
    );
}

#[test]
fn undo_keeps_a_picture_copy_whose_original_changed() {
    let r = run("copies-orig-changed");
    write_file(&r.original_picture, "PNG beach, edited");
    let mut gate = TestGate::new();
    let report = undo(&r.record_path, &r.record, &r.library, &mut gate);
    assert!(r.picture.exists(), "{report:?}");
}

#[test]
fn undo_keeps_a_copy_another_library_file_lists() {
    let r = run("copies-listed");
    write_file(
        &r.library.join("other.md"),
        &format!("# Other\n\n{}\n", library::link_line("Trip", &r.copy)),
    );
    let mut gate = TestGate::new();
    let report = undo(&r.record_path, &r.record, &r.library, &mut gate);
    assert!(r.copy.exists(), "{report:?}");
}

#[test]
fn a_run_of_copies_whose_copy_is_outside_the_library_is_not_acted_on() {
    let mut r = run("copies-outside");
    let elsewhere = r.dir.join("desk").join("trip.md");
    let note = r
        .record
        .notes
        .iter_mut()
        .find(|n| n.created)
        .expect("the copy");
    note.path = elsewhere.clone();
    note.sha256 = digest_of(&elsewhere).unwrap_or_default();
    let plan = prepare(&r.record, &r.library);
    assert!(plan.refusal.is_some(), "{plan:?}");
    let mut gate = TestGate::new();
    let _ = undo(&r.record_path, &r.record, &r.library, &mut gate);
    assert!(elsewhere.exists(), "the owner's file is never removed");
}

#[test]
fn a_picture_copy_that_names_no_original_is_not_acted_on() {
    let mut r = run("copies-no-original");
    r.record.pictures[0].original = None;
    let plan = prepare(&r.record, &r.library);
    assert!(plan.refusal.is_some(), "{plan:?}");
}

/// Fable's review of C1, 5: a copy of a text file whose original is gone may be the only copy
/// left, so undo keeps it, and its line; the record read back names the original.
#[test]
fn undo_keeps_a_copy_whose_original_is_gone() {
    let r = run("copies-note-orphan");
    let loaded = load(&r.record_path, &r.library).expect("it reads back");
    let original = r.dir.join("desk").join("trip.md");
    let named = loaded
        .copied
        .iter()
        .find(|c| same_path(&c.path, &r.copy))
        .expect("the copy names its original");
    assert!(same_path(
        named.original.as_deref().expect("its path"),
        &original
    ));
    std::fs::remove_file(&original).expect("the owner removed it");
    let plan = prepare(&loaded, &r.library);
    assert!(
        plan.notes_kept.iter().any(|p| same_path(p, &r.copy)),
        "{plan:?}"
    );
    let mut gate = TestGate::new();
    let report = undo(&r.record_path, &loaded, &r.library, &mut gate);
    assert!(
        r.copy.exists(),
        "the copy may be the only one now: {report:?}"
    );
    assert!(
        read(&r.group).contains("trip"),
        "its line stays with it: {report:?}"
    );
}

/// Fable's review of C1, 5: a copy the record does not name an original for is never removed.
#[test]
fn undo_keeps_a_copy_the_record_names_no_original_for() {
    let mut r = run("copies-note-unnamed");
    r.record.copied.clear();
    let mut gate = TestGate::new();
    let report = undo(&r.record_path, &r.record, &r.library, &mut gate);
    assert!(r.copy.exists(), "{report:?}");
}
