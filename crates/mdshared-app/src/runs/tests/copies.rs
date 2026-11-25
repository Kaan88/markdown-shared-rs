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
        plan.copies_kept.iter().any(|p| same_path(p, &r.picture)),
        "{plan:?}"
    );
    assert!(
        plan.sentence("Trips")
            .contains("its original is gone or changed"),
        "{}",
        plan.sentence("Trips")
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
        plan.copies_kept.iter().any(|p| same_path(p, &r.copy)),
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

// ---- Move them in (C2, D-210) --------------------------------------------------------------

struct Moved1 {
    dir: PathBuf,
    library: PathBuf,
    record_path: PathBuf,
    record: Record,
    md: PathBuf,
    md_bytes: Vec<u8>,
    picture: PathBuf,
    copy: PathBuf,
    note: PathBuf,
    note_text: String,
}

impl Drop for Moved1 {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A run that moves `trip.md` in with the picture it shows; its link to a logo outside its
/// folder is rewritten in the copy, and a note of the library that links to it is rewritten to
/// lead to the copy.
fn move_run(tag: &str) -> Moved1 {
    let dir = crate::paths::canonical_or_absolute(&scratch(tag));
    let library = dir.join("library");
    let group = library.join("trips.md");
    let desk = dir.join("desk");
    let md = desk.join("trips").join("trip.md");
    let md_text = "# Trip\n\n![Beach](img/beach.png)\n\n![Logo](../logo.png)\n";
    write_file(&md, md_text);
    let picture = desk.join("trips").join("img").join("beach.png");
    write_file(&picture, "PNG beach");
    write_file(&desk.join("logo.png"), "PNG logo");
    let note = library.join("Refs").join("ref.md");
    let note_text = "# Ref\n\n[Trip](../../desk/trips/trip.md)\n".to_string();
    write_file(&note, &note_text);
    write_file(
        &library.join("refs.md"),
        &format!(
            "# Refs\n\n{}\n",
            library::link_line("Ref", Path::new("Refs/ref.md"))
        ),
    );
    write_file(&group, "# Trips\n");
    let home = library.join("Trips");
    let wanted = [Wanted {
        from: md.clone(),
        dir: home.clone(),
    }];
    let plan = copy_in::plan(&wanted, std::slice::from_ref(&library));
    let written = copy_in::write(&plan, &library, &home).expect("written");
    let copy = home.join("trip.md");
    let links = vec![Link {
        path: copy.clone(),
        title: "Trip".to_string(),
        sections: Vec::new(),
        line: library::link_line("Trip", &copy),
    }];
    let before = read(&group);
    std::fs::write(&group, with_links_added(&before, &links)).expect("the add");
    let mp = copy_in::move_plan(&plan, &written, &library, &home, &[]);
    assert_eq!(mp.removes.len(), 2, "{mp:?}");
    assert_eq!(mp.relinks.len(), 1, "{mp:?}");
    let g = Group {
        name: "Trips".to_string(),
        file: group.clone(),
        created: false,
        home,
    };
    let (record_path, record) =
        move_in_record(&library, "", g, &links, &before, &written, &mp).expect("made");
    let out = copy_in::move_out(&mp, &mut Delete);
    assert!(out.failures.is_empty(), "{out:?}");
    assert!(!md.exists() && !picture.exists(), "moved");
    assert_ne!(
        read(&copy),
        md_text,
        "the copy's link to the logo was rewritten"
    );
    Moved1 {
        dir,
        library,
        record_path,
        record,
        md,
        md_bytes: md_text.as_bytes().to_vec(),
        picture,
        copy,
        note,
        note_text,
    }
}

/// Undo of a move puts each original back where it was with its own bytes (the text it had
/// before its copy's links were rewritten), gives the library's note its link back, and then
/// takes the copies away, since their originals are there again.
#[test]
fn undo_puts_the_moved_originals_back_and_takes_the_copies_away() {
    let r = move_run("move-undo");
    let loaded = load(&r.record_path, &r.library).expect("it reads back");
    assert_eq!(loaded.removed.len(), 2);
    let plan = prepare(&loaded, &r.library);
    assert!(plan.refusal.is_none(), "{:?}", plan.refusal);
    assert_eq!(
        plan.put_back.len(),
        3,
        "two originals and the note: {plan:?}"
    );
    assert_eq!(plan.notes.len(), 1, "the copy goes: {plan:?}");
    assert!(
        plan.sentence("Trips").starts_with("Puts back 3 files"),
        "{}",
        plan.sentence("Trips")
    );
    let mut gate = TestGate::new();
    let report = undo(&r.record_path, &loaded, &r.library, &mut gate);
    assert_eq!(
        std::fs::read(&r.md).unwrap_or_default(),
        r.md_bytes,
        "{report:?}"
    );
    assert_eq!(read(&r.picture), "PNG beach");
    assert_eq!(
        read(&r.note),
        r.note_text,
        "the note leads to the original again"
    );
    assert!(!r.copy.exists(), "the copy goes: {report:?}");
}

/// A file put where a moved original was is never replaced: the original stays in the library,
/// as its copy, and is listed as kept.
#[test]
fn undo_never_writes_over_a_file_where_an_original_was() {
    let r = move_run("move-undo-taken");
    write_file(&r.md, "someone else's\n");
    let mut gate = TestGate::new();
    let report = undo(&r.record_path, &r.record, &r.library, &mut gate);
    assert_eq!(read(&r.md), "someone else's\n");
    assert!(r.copy.exists(), "the copy is the only one: {report:?}");
    assert!(
        report.restore_kept.iter().any(|p| same_path(p, &r.md)),
        "{report:?}"
    );
}

/// A note the owner changed since the move keeps their text, and the copy it leads to stays.
#[test]
fn undo_keeps_a_rewritten_note_the_owner_changed_since() {
    let r = move_run("move-undo-note");
    write_file(&r.note, "# Ref\n\nmine now\n");
    let mut gate = TestGate::new();
    let report = undo(&r.record_path, &r.record, &r.library, &mut gate);
    assert_eq!(read(&r.note), "# Ref\n\nmine now\n");
    assert!(
        report.restore_kept.iter().any(|p| same_path(p, &r.note)),
        "{report:?}"
    );
}

/// A record whose taken original has a `..` in its path is one this program did not write:
/// nothing of it is acted on.
#[test]
fn a_move_record_with_a_strange_original_is_not_acted_on() {
    let mut r = move_run("move-strange");
    r.record.removed[0].path = r.dir.join("desk").join("..").join("x.md");
    let plan = prepare(&r.record, &r.library);
    assert!(
        plan.refusal.as_deref().is_some_and(|w| w.contains("x.md")),
        "{:?}",
        plan.refusal
    );
}

/// Fable's review of C2, 4: the question names each file undo puts back where it was.
#[test]
fn undo_s_question_names_each_file_it_puts_back() {
    let r = move_run("move-named");
    let plan = prepare(&r.record, &r.library);
    let s = plan.sentence("Trips");
    for p in [&r.md, &r.picture, &r.note] {
        assert!(s.contains(&p.display().to_string()), "{s}");
    }
}

/// Fable's review of C2, 4: a record that would put back a file of another kind than its copy,
/// or one whose copy is not a file the run says it made, is not acted on.
#[test]
fn a_move_record_that_puts_back_another_kind_of_file_is_not_acted_on() {
    let mut r = move_run("move-kind");
    r.record.removed[0].path = r.dir.join("desk").join("trips").join("trip.bat");
    let plan = prepare(&r.record, &r.library);
    assert!(
        plan.refusal
            .as_deref()
            .is_some_and(|w| w.contains("trip.bat")),
        "{:?}",
        plan.refusal
    );
    let mut r = move_run("move-not-own");
    let stray = r.library.join("Trips").join("stray.md");
    write_file(&stray, "# Trip\n");
    r.record.removed[0].original = Some(stray);
    let plan = prepare(&r.record, &r.library);
    assert!(plan.refusal.is_some(), "{plan:?}");
}

/// Fable's review of C2, nit: a note the move made lead to a copy keeps leading to it while the
/// original is not back.
#[test]
fn undo_keeps_a_relinked_note_while_its_original_is_not_back() {
    let r = move_run("move-note-held");
    write_file(&r.md, "someone else's\n");
    let mut gate = TestGate::new();
    let report = undo(&r.record_path, &r.record, &r.library, &mut gate);
    assert_ne!(read(&r.note), r.note_text, "{report:?}");
    assert!(r.copy.exists(), "{report:?}");
    assert!(
        report.restore_kept.iter().any(|p| same_path(p, &r.note)),
        "{report:?}"
    );
}

/// A bin whose window holds one file open with changes not saved.
struct Unsaved(PathBuf);

impl Bin for Unsaved {
    fn recycle(&mut self, path: &Path) -> Result<(), String> {
        std::fs::remove_file(path).map_err(|e| e.to_string())
    }
    fn may_write(&mut self, path: &Path) -> Result<(), String> {
        if same_path(path, &self.0) {
            Err("open with changes not saved".to_string())
        } else {
            Ok(())
        }
    }
}

/// Fable's review of C2, 3: undo does not write a note a tab holds with changes not saved.
#[test]
fn undo_keeps_a_relinked_note_a_tab_holds_unsaved() {
    let r = move_run("move-note-unsaved");
    let relinked = read(&r.note);
    let mut gate = TestGate::new();
    let report = undo_with(
        &r.record_path,
        &r.record,
        &r.library,
        &mut gate,
        &mut Unsaved(r.note.clone()),
    );
    assert_eq!(read(&r.note), relinked, "{report:?}");
    assert_eq!(
        std::fs::read(&r.md).unwrap_or_default(),
        r.md_bytes,
        "{report:?}"
    );
}

/// Fable's review of C2, 2: a move whose record could not be read back is not written, so the
/// caller moves nothing.
#[test]
fn a_move_whose_record_would_be_too_large_writes_none() {
    let dir = crate::paths::canonical_or_absolute(&scratch("move-full"));
    let library = dir.join("library");
    let group = library.join("trips.md");
    write_file(&group, "# Trips\n");
    let mp = copy_in::MovePlan {
        relinks: vec![copy_in::Relink {
            path: library.join("Refs").join("n.md"),
            before: "\"\n".repeat(1_200_000),
            after: String::new(),
            targets: Vec::new(),
        }],
        ..copy_in::MovePlan::default()
    };
    let g = Group {
        name: "Trips".to_string(),
        file: group,
        created: false,
        home: library.join("Trips"),
    };
    let err = move_in_record(
        &library,
        "",
        g,
        &[],
        "# Trips\n",
        &copy_in::Written::default(),
        &mp,
    )
    .expect_err("too large");
    assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
    assert!(!library.join(RUNS_DIR).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A note undo keeps because a tab holds it unsaved still leads to the copy: the copy stays,
/// though its original is back.
#[test]
fn a_copy_a_kept_note_leads_to_stays() {
    let r = move_run("move-note-copy-kept");
    let mut gate = TestGate::new();
    let report = undo_with(
        &r.record_path,
        &r.record,
        &r.library,
        &mut gate,
        &mut Unsaved(r.note.clone()),
    );
    assert!(r.md.exists(), "{report:?}");
    assert!(r.copy.exists(), "the kept note leads to it: {report:?}");
    assert!(
        report.led_to.iter().any(|p| same_path(p, &r.copy)),
        "{report:?}"
    );
}
