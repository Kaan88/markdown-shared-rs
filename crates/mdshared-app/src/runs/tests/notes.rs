//! The record of Write notes ([`Kind::Notes`]) and its undo: no source is checked; the notes it
//! wrote go while they are what it wrote; a copy goes only while its original is where it was and
//! no note undo keeps shows it; an original is never touched.

use super::*;

struct Run1 {
    dir: PathBuf,
    library: PathBuf,
    group: PathBuf,
    /// The group's file before the run's links.
    before: String,
    record_path: PathBuf,
    record: Record,
    /// `one.md` shows `a.png` and `c.png`; `two.md` shows `b.png` and `c.png`.
    one: PathBuf,
    two: PathBuf,
    /// The copies, by name, and the originals they were made from.
    copies: Vec<PathBuf>,
    originals: Vec<PathBuf>,
    folders: Vec<PathBuf>,
}

impl Run1 {
    fn copy(&self, name: &str) -> &PathBuf {
        self.copies
            .iter()
            .find(|p| p.ends_with(name))
            .expect("that copy")
    }
    fn original(&self, name: &str) -> &PathBuf {
        self.originals
            .iter()
            .find(|p| p.ends_with(name))
            .expect("that original")
    }
}

impl Drop for Run1 {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const ONE: &str = "# One\n\n![The error](pictures/a.png)\n\n![Both](pictures/c.png)\n";
const TWO: &str = "# Two\n\n![The code](pictures/b.png)\n\n![Both](pictures/c.png)\n";

/// A run of Write notes as mdedit writes it: the record first, then its folders, the copies of
/// three screenshots on the desk, two notes that show them, and their links in `Shots.md`.
fn run(tag: &str) -> Run1 {
    let dir = scratch(tag);
    let dir = crate::paths::canonical_or_absolute(&dir);
    let library = dir.join("library");
    let group = library.join("Shots.md");
    let before = "# Shots\nhome: Shots\n".to_string();
    write_file(&group, &before);
    let home = library.join("Shots");
    let section = home.join("Sign-in");
    let pictures_dir = section.join("pictures");
    let desk = dir.join("desk");
    let originals: Vec<PathBuf> = ["a.png", "b.png", "c.png"]
        .iter()
        .map(|n| {
            let p = desk.join(n);
            write_file(&p, &format!("PNG {n}"));
            p
        })
        .collect();
    let sha = |b: &[u8]| crate::update::sha256_hex(b);
    let notes: Vec<NewNote> = [("One", ONE), ("Two", TWO)]
        .iter()
        .map(|(title, text)| {
            let path = section.join(format!("{}.md", title.to_lowercase()));
            NewNote {
                link_path: path.strip_prefix(&library).expect("inside").to_path_buf(),
                path,
                text: text.to_string(),
                from_source: text.to_string(),
                title: title.to_string(),
                sections: vec!["Sign-in".to_string()],
                ranges: Vec::new(),
                heading_shift: 0,
                title_added: false,
                already: false,
            }
        })
        .collect();
    let (secs, started) = now_utc();
    let mut record = Record {
        version: RECORD_VERSION,
        kind: Kind::Notes,
        started,
        started_secs: secs,
        state: State::Writing,
        instruction: "One note per screen".to_string(),
        sent: None,
        source: Source {
            path: PathBuf::new(),
            len: 0,
            sha256: String::new(),
            recycled: false,
        },
        group: Group {
            name: "Shots".to_string(),
            file: group.clone(),
            created: false,
            home: home.clone(),
        },
        folders: Vec::new(),
        notes: notes
            .iter()
            .map(|n| NoteRec {
                path: n.path.clone(),
                created: true,
                sha256: sha(n.text.as_bytes()),
                ranges: Vec::new(),
                heading_shift: 0,
                title_added: false,
                sections: n.sections.clone(),
                line: library::link_line(&n.title, &n.link_path),
            })
            .collect(),
        pictures: Vec::new(),
        sections_added: Vec::new(),
        moved: None,
        originals: Vec::new(),
        added: Vec::new(),
        copied: Vec::new(),
        removed: Vec::new(),
    };
    let record_path = record.create(&library).expect("the record");
    std::fs::create_dir_all(&pictures_dir).expect("the folders");
    let folders = vec![home.clone(), section.clone(), pictures_dir.clone()];
    record.folders = folders.clone();
    let mut copies = Vec::new();
    for original in &originals {
        let bytes = std::fs::read(original).expect("the original");
        let to = pictures_dir.join(original.file_name().expect("a name"));
        create_new(&to, &bytes).expect("the copy");
        record.pictures.push(PictureRec {
            path: to.clone(),
            created: true,
            sha256: sha(&bytes),
            original: Some(original.clone()),
            original_sha256: sha(&bytes),
        });
        copies.push(to);
    }
    for n in &notes {
        create_new(&n.path, n.text.as_bytes()).expect("the note");
    }
    let links: Vec<Link> = notes.iter().map(Link::new).collect();
    let mut gate = TestGate::new();
    gate.add_links(&group, &links).expect("the links");
    record.sections_added = vec![vec!["Sign-in".to_string()]];
    record.keep_added(&group, Some(before.clone()), &links, &library);
    record.state = State::Done;
    record.close(&record_path, &library).expect("closed");
    Run1 {
        one: notes[0].path.clone(),
        two: notes[1].path.clone(),
        dir,
        library,
        group,
        before,
        record_path,
        record,
        copies,
        originals,
        folders,
    }
}

fn loaded(r: &Run1) -> Record {
    load(&r.record_path, &r.library).expect("it reads back")
}

#[test]
fn a_notes_record_reads_back_as_its_own_kind_with_its_copies_and_their_originals() {
    let r = run("notes-record");
    let name = r
        .record_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    assert!(name.ends_with("-notes.json"), "{name}");
    assert!(read(&r.record_path).contains("\"kind\": \"notes\""));
    let mut back = loaded(&r);
    assert_eq!(back.kind, Kind::Notes);
    // No source: an empty one, read back (as a run of links' is) as the library folder.
    assert_eq!((back.source.len, back.source.sha256.as_str()), (0, ""));
    back.source = r.record.source.clone();
    assert_eq!(back, r.record);
    assert_eq!(back.pictures.len(), 3);
    assert!(back.pictures.iter().all(|p| p.original.is_some()));
    // It is listed as a run, not as a record of a later version.
    let (runs, unread) = list_all(&r.library);
    assert_eq!(runs.len(), 1);
    assert!(unread.is_empty(), "{unread:?}");
}

/// The first defect of the workaround: undo checked a source, faked as the first copy, and
/// refused once the owner deleted it. A run of Write notes has no source: the rest goes.
#[test]
fn undo_of_a_notes_run_checks_no_source_and_takes_out_what_it_wrote() {
    let r = run("notes-undo");
    // The owner deleted one copy since; nothing names a source.
    std::fs::remove_file(r.copy("a.png")).expect("deleted");
    let record = loaded(&r);
    let plan = prepare(&record, &r.library);
    assert!(plan.refusal.is_none(), "{:?}", plan.refusal);
    assert_eq!(plan.notes.len(), 2);
    assert_eq!(plan.pictures.len(), 2);
    assert!(plan.shown.is_empty());
    let sentence = plan.sentence("Shots");
    assert!(
        sentence
            .starts_with("Removes 2 notes and 2 pictures this run wrote, with 2 lines in Shots"),
        "{sentence}"
    );
    let mut gate = TestGate::new();
    let report = undo(&r.record_path, &record, &r.library, &mut gate);
    assert_eq!(report.refusal, None);
    assert!(report.marked);
    assert_eq!(report.removed_notes.len(), 2);
    assert_eq!(report.removed_pictures.len(), 2);
    assert!(!r.one.exists() && !r.two.exists());
    assert!(r.copies.iter().all(|c| !c.exists()));
    // The originals are never touched; the group's file is what it was; the folders are gone.
    for o in &r.originals {
        let name = o.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(read(o), format!("PNG {name}"));
    }
    assert_eq!(read(&r.group), r.before);
    assert!(r.folders.iter().all(|f| !f.exists()), "{report:?}");
    assert_eq!(loaded(&r).state, State::Undone);
}

/// The second defect: a note changed since is kept, and the copies it shows were removed with
/// the others, breaking its pictures. They stay with it; a copy only the other note showed goes.
#[test]
fn undo_of_a_notes_run_keeps_the_copies_a_kept_note_shows() {
    let r = run("notes-shown");
    std::fs::write(&r.one, format!("{ONE}\nMy own words.\n")).expect("edited");
    let record = loaded(&r);
    let plan = prepare(&record, &r.library);
    assert!(plan.refusal.is_none(), "{:?}", plan.refusal);
    assert_eq!(plan.notes, vec![r.two.clone()]);
    assert_eq!(plan.notes_kept, vec![r.one.clone()]);
    assert_eq!(plan.pictures, vec![r.copy("b.png").clone()]);
    assert_eq!(
        plan.shown,
        vec![r.copy("a.png").clone(), r.copy("c.png").clone()]
    );
    assert!(plan.kept.contains(r.copy("a.png")));
    let sentence = plan.sentence("Shots");
    assert!(
        sentence.contains("2 pictures stay with the notes that show them."),
        "{sentence}"
    );
    let mut gate = TestGate::new();
    let report = undo(&r.record_path, &record, &r.library, &mut gate);
    assert_eq!(report.refusal, None);
    assert!(r.one.exists());
    assert!(!r.two.exists());
    assert!(r.copy("a.png").exists() && r.copy("c.png").exists());
    assert!(!r.copy("b.png").exists());
    assert_eq!(
        report.shown,
        vec![r.copy("a.png").clone(), r.copy("c.png").clone()]
    );
    assert!(report.kept.contains(r.copy("c.png")));
    // The kept note's folder and its pictures' folder stay; nothing of the desk changed.
    assert!(r.copy("a.png").parent().is_some_and(Path::is_dir));
    assert!(r.originals.iter().all(|o| o.is_file()));
}

/// A note the bin would not take (open with changes not saved, say) stays, and so do the copies
/// it shows: what stays is worked out again after the notes went, not only when undo was asked.
#[test]
fn a_note_the_bin_keeps_keeps_the_copies_it_shows() {
    let r = run("notes-bin");
    struct Refusing<'a> {
        keep: &'a Path,
        bin: TestBin,
    }
    impl Bin for Refusing<'_> {
        fn recycle(&mut self, path: &Path) -> Result<(), String> {
            if same_path(path, self.keep) {
                return Err("it is open with changes not saved".to_string());
            }
            self.bin.recycle(path)
        }
    }
    let record = loaded(&r);
    let plan = prepare(&record, &r.library);
    assert!(plan.shown.is_empty(), "{plan:?}");
    let mut bin = Refusing {
        keep: &r.two,
        bin: TestBin::new(&r.dir),
    };
    let mut gate = TestGate::new();
    let report = undo_with(&r.record_path, &record, &r.library, &mut gate, &mut bin);
    assert!(r.two.exists() && !r.one.exists());
    assert!(r.copy("b.png").exists() && r.copy("c.png").exists());
    assert!(!r.copy("a.png").exists());
    assert_eq!(
        report.shown,
        vec![r.copy("b.png").clone(), r.copy("c.png").clone()]
    );
    assert_eq!(loaded(&r).state, State::UndoPending);
}

/// Copy on: a copy whose original was deleted or changed since may be the only one, so it stays,
/// while its note goes; an original is never removed.
#[test]
fn undo_of_a_notes_run_keeps_a_copy_whose_original_is_gone_or_changed() {
    let r = run("notes-originals");
    std::fs::remove_file(r.original("a.png")).expect("deleted");
    std::fs::write(r.original("b.png"), "another picture").expect("changed");
    let record = loaded(&r);
    let plan = prepare(&record, &r.library);
    assert!(plan.refusal.is_none(), "{:?}", plan.refusal);
    assert_eq!(
        plan.copies_kept,
        vec![r.copy("a.png").clone(), r.copy("b.png").clone()]
    );
    assert_eq!(plan.pictures, vec![r.copy("c.png").clone()]);
    let mut gate = TestGate::new();
    let report = undo(&r.record_path, &record, &r.library, &mut gate);
    assert_eq!(report.refusal, None);
    assert!(r.copy("a.png").exists() && r.copy("b.png").exists());
    assert!(!r.copy("c.png").exists());
    assert!(!r.one.exists() && !r.two.exists());
    assert_eq!(read(r.original("b.png")), "another picture");
    assert!(r.original("c.png").is_file());
}

/// A copy of a picture pasted from the clipboard names no original: its temp file goes with the
/// window, so the copy would never go if undo waited for it. It goes while it is what was
/// written, and stays once it changed.
#[test]
fn a_copy_that_names_no_original_goes_while_it_is_what_was_written() {
    let r = run("notes-pasted");
    let mut record = loaded(&r);
    record.pictures[0].original = None;
    record.pictures[0].original_sha256 = String::new();
    record.pictures[1].original = None;
    record.pictures[1].original_sha256 = String::new();
    record.close(&r.record_path, &r.library).expect("rewritten");
    // The temp files are gone, as they are once the window closed; the second copy was edited.
    std::fs::remove_file(r.original("a.png")).expect("gone");
    std::fs::remove_file(r.original("b.png")).expect("gone");
    std::fs::write(r.copy("b.png"), "drawn on").expect("edited");
    let record = loaded(&r);
    let plan = prepare(&record, &r.library);
    assert!(plan.refusal.is_none(), "{:?}", plan.refusal);
    assert_eq!(
        plan.pictures,
        vec![r.copy("a.png").clone(), r.copy("c.png").clone()]
    );
    assert_eq!(plan.pictures_kept, vec![r.copy("b.png").clone()]);
    let mut gate = TestGate::new();
    let report = undo(&r.record_path, &record, &r.library, &mut gate);
    assert_eq!(report.refusal, None);
    assert!(!r.copy("a.png").exists() && !r.copy("c.png").exists());
    assert_eq!(read(r.copy("b.png")), "drawn on");
}

/// A note that links a copy by a name with a space, `%20` and all, in a way the reader does not
/// take as a link keeps it too.
#[test]
fn a_kept_note_that_names_a_copy_with_its_space_encoded_keeps_it() {
    let r = run("notes-encoded");
    let spaced = r.copy("c.png").with_file_name("the code.png");
    std::fs::rename(r.copy("c.png"), &spaced).expect("renamed");
    let mut record = loaded(&r);
    record.pictures[2].path = spaced.clone();
    record.close(&r.record_path, &r.library).expect("rewritten");
    std::fs::write(&r.one, "# One\n\n<img src=\"pictures/the%20code.png\">\n").expect("edited");
    let record = loaded(&r);
    let plan = prepare(&record, &r.library);
    assert_eq!(plan.shown, vec![spaced]);
}

/// A note that cannot be read as text may show any copy: every copy stays with it.
#[test]
fn a_kept_note_that_cannot_be_read_keeps_every_copy() {
    let r = run("notes-unreadable");
    std::fs::write(&r.one, [0xFF, 0xFE, 0x00, 0x41]).expect("not UTF-8");
    let record = loaded(&r);
    let plan = prepare(&record, &r.library);
    assert_eq!(plan.notes, vec![r.two.clone()]);
    assert!(plan.pictures.is_empty(), "{:?}", plan.pictures);
    assert_eq!(plan.shown.len(), 3);
}

/// A picture the kept note shows in a way the reader does not take as a link (HTML) is found by
/// its name, so its picture never breaks.
#[test]
fn a_kept_note_that_shows_a_copy_in_html_keeps_it() {
    let r = run("notes-html");
    std::fs::write(&r.one, "# One\n\n<img src=\"pictures/A.png\">\n").expect("edited");
    let record = loaded(&r);
    let plan = prepare(&record, &r.library);
    assert_eq!(plan.shown, vec![r.copy("a.png").clone()]);
    assert_eq!(
        plan.pictures,
        vec![r.copy("b.png").clone(), r.copy("c.png").clone()]
    );
}
