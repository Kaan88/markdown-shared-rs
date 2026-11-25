//! The tests of "Copy them in": each rule of `moves_copy` has one that fails when the code that
//! keeps it is taken out.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn scratch(tag: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("mdshared-copy-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    crate::paths::canonical_or_absolute(&dir)
}

fn put(path: &Path, bytes: &[u8]) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("its folder");
    }
    std::fs::write(path, bytes).expect("a file");
}

fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

fn text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// A library folder, the group's folder in it, and a folder of the owner's outside it.
struct World {
    dir: PathBuf,
    library: PathBuf,
    group: PathBuf,
    outside: PathBuf,
}

impl World {
    fn new(tag: &str) -> World {
        let dir = scratch(tag);
        let library = dir.join("library");
        let group = library.join("Shots");
        let outside = dir.join("desk");
        std::fs::create_dir_all(&library).expect("the library");
        std::fs::create_dir_all(&outside).expect("the desk");
        World {
            dir,
            library,
            group,
            outside,
        }
    }

    fn want(&self, from: &Path) -> Wanted {
        Wanted {
            from: from.to_path_buf(),
            dir: self.group.clone(),
        }
    }

    fn inside(&self) -> Vec<PathBuf> {
        vec![self.library.clone()]
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn a_file_is_copied_into_its_folder_and_the_original_is_left_as_it_was() {
    let w = World::new("plain");
    let from = w.outside.join("report.pdf");
    put(&from, b"%PDF-1.4 the owner's\n");
    let plan = plan(&[w.want(&from)], &w.inside());
    let to = w.group.join("report.pdf");
    assert_eq!(
        plan[0].copy,
        Copy::New {
            to: to.clone(),
            text: None
        }
    );
    let done = write(&plan, &w.library, &w.group).expect("written");
    assert_eq!(bytes(&to), b"%PDF-1.4 the owner's\n");
    assert_eq!(
        bytes(&from),
        b"%PDF-1.4 the owner's\n",
        "the original is not touched"
    );
    assert_eq!(done.files.len(), 1);
    assert!(done.files[0].created, "this run made it");
    assert_eq!(done.folders, vec![w.group.clone()], "the folder it made");
}

#[test]
fn a_name_taken_by_other_bytes_gets_the_next_number() {
    let w = World::new("taken");
    let from = w.outside.join("notes.txt");
    put(&from, b"mine\n");
    put(&w.group.join("notes.txt"), b"someone else's\n");
    put(&w.group.join("notes-2.txt"), b"another\n");
    let plan = plan(&[w.want(&from)], &w.inside());
    let to = w.group.join("notes-3.txt");
    assert_eq!(
        plan[0].copy,
        Copy::New {
            to: to.clone(),
            text: None
        }
    );
    write(&plan, &w.library, &w.group).expect("written");
    assert_eq!(
        text(&w.group.join("notes.txt")),
        "someone else's\n",
        "never replaced"
    );
    assert_eq!(text(&to), "mine\n");
}

#[test]
fn the_same_bytes_already_there_are_linked_and_not_written_again() {
    let w = World::new("there");
    let from = w.outside.join("a.png");
    put(&from, b"\x89PNG same");
    put(&w.group.join("a.png"), b"\x89PNG same");
    let plan = plan(&[w.want(&from)], &w.inside());
    assert_eq!(
        plan[0].copy,
        Copy::There {
            to: w.group.join("a.png")
        }
    );
    let done = write(&plan, &w.library, &w.group).expect("written");
    assert!(!done.files[0].created, "not this run's file");
    assert!(done.folders.is_empty());
}

#[test]
fn a_file_inside_the_library_stays_where_it_is() {
    let w = World::new("inside");
    let from = w.library.join("Old").join("kept.md");
    put(&from, b"# Kept\n");
    let plan = plan(&[w.want(&from)], &w.inside());
    assert_eq!(
        plan[0].copy,
        Copy::Stays {
            why: INSIDE.to_string()
        }
    );
    assert_eq!(plan[0].target(), from.as_path());
    let done = write(&plan, &w.library, &w.group).expect("nothing to write");
    assert!(done.files.is_empty());
    assert!(!w.group.exists(), "no folder made for a file that stays");
}

#[test]
fn two_files_of_one_run_with_one_name_get_two_names() {
    let w = World::new("twins");
    let a = w.outside.join("one").join("readme.md");
    let b = w.outside.join("two").join("readme.md");
    put(&a, b"# One\n");
    put(&b, b"# Two\n");
    let plan = plan(&[w.want(&a), w.want(&b)], &w.inside());
    assert_eq!(plan[0].target(), w.group.join("readme.md").as_path());
    assert_eq!(plan[1].target(), w.group.join("readme-2.md").as_path());
    write(&plan, &w.library, &w.group).expect("written");
    assert_eq!(text(&w.group.join("readme.md")), "# One\n");
    assert_eq!(text(&w.group.join("readme-2.md")), "# Two\n");
}

#[test]
fn a_markdown_file_takes_the_pictures_below_it_and_its_words_do_not_change() {
    let w = World::new("pictures");
    let md = w.outside.join("trip.md");
    let body = "# Trip\n\n![Beach](img/beach.png)\n";
    put(&md, body.as_bytes());
    put(&w.outside.join("img").join("beach.png"), b"\x89PNG beach");
    let plan = plan(&[w.want(&md)], &w.inside());
    let to_pic = w.group.join("img").join("beach.png");
    assert_eq!(plan[0].pictures.len(), 1, "{plan:?}");
    assert_eq!(plan[0].pictures[0].to, to_pic);
    assert_eq!(
        plan[0].rewrites, 0,
        "the picture is at the same place beside the copy"
    );
    let done = write(&plan, &w.library, &w.group).expect("written");
    assert_eq!(text(&w.group.join("trip.md")), body);
    assert_eq!(bytes(&to_pic), b"\x89PNG beach");
    assert_eq!(done.pictures.len(), 1);
    let p = &done.pictures[0];
    assert!(p.created);
    assert_eq!(
        p.original.as_deref(),
        Some(w.outside.join("img").join("beach.png").as_path())
    );
    assert!(
        !p.original_sha256.is_empty(),
        "the original's digest, for undo"
    );
    assert_eq!(
        done.folders,
        vec![w.group.clone(), w.group.join("img")],
        "parents first"
    );
}

#[test]
fn a_link_outside_its_folder_is_rewritten_to_lead_where_it_did() {
    let w = World::new("rewrite");
    let md = w.outside.join("notes").join("trip.md");
    put(
        &md,
        "# Trip\n\n![Map](../maps/map.png) and [plan](../plan.txt)\n".as_bytes(),
    );
    let map = w.outside.join("maps").join("map.png");
    put(&map, b"\x89PNG map");
    put(&w.outside.join("plan.txt"), b"plan\n");
    let plan = plan(&[w.want(&md)], &w.inside());
    assert!(
        plan[0].pictures.is_empty(),
        "a picture above its folder is not carried"
    );
    assert_eq!(plan[0].rewrites, 2);
    write(&plan, &w.library, &w.group).expect("written");
    let copy = text(&w.group.join("trip.md"));
    let links = links::links(&copy, &w.group);
    assert_eq!(links.len(), 2, "{copy}");
    assert!(
        links.iter().any(|l| links::same_file(&l.target, &map)),
        "{copy}"
    );
    assert!(
        links
            .iter()
            .any(|l| links::same_file(&l.target, &w.outside.join("plan.txt"))),
        "{copy}"
    );
    assert!(
        text(&md).contains("](../maps/map.png)"),
        "the original's words are not touched"
    );
}

#[test]
fn a_link_to_another_file_of_the_run_leads_to_its_copy() {
    let w = World::new("between");
    let a = w.outside.join("a").join("one.md");
    let b = w.outside.join("b").join("two.md");
    put(&a, "# One\n\nSee [two](../b/two.md).\n".as_bytes());
    put(&b, b"# Two\n");
    let plan = plan(&[w.want(&a), w.want(&b)], &w.inside());
    write(&plan, &w.library, &w.group).expect("written");
    let copy = text(&w.group.join("one.md"));
    assert!(copy.contains("](two.md)"), "{copy}");
}

#[test]
fn a_markdown_file_that_is_not_utf8_is_copied_byte_for_byte() {
    let w = World::new("latin1");
    let md = w.outside.join("old.md");
    let raw = b"# Caf\xe9\n\n![x](x.png)\n";
    put(&md, raw);
    put(&w.outside.join("x.png"), b"\x89PNG x");
    let plan = plan(&[w.want(&md)], &w.inside());
    assert!(
        plan[0].note.is_some(),
        "its row says why its links are as they were"
    );
    assert!(plan[0].pictures.is_empty());
    write(&plan, &w.library, &w.group).expect("written");
    assert_eq!(bytes(&w.group.join("old.md")), raw.to_vec());
}

#[test]
fn a_picture_of_two_files_is_copied_once() {
    let w = World::new("shared-pic");
    let a = w.outside.join("a.md");
    let b = w.outside.join("b.md");
    put(&a, "![p](p.png)\n".as_bytes());
    put(&b, "![p](p.png)\n".as_bytes());
    put(&w.outside.join("p.png"), b"\x89PNG p");
    let plan = plan(&[w.want(&a), w.want(&b)], &w.inside());
    let done = write(&plan, &w.library, &w.group).expect("written");
    assert_eq!(done.pictures.len(), 1, "{done:?}");
    assert!(done.pictures[0].created);
    assert_eq!(bytes(&w.group.join("p.png")), b"\x89PNG p");
}

#[test]
fn a_picture_whose_place_holds_other_bytes_is_not_carried_and_its_link_leads_to_it() {
    let w = World::new("pic-taken");
    let md = w.outside.join("a.md");
    put(&md, "![p](p.png)\n".as_bytes());
    let pic = w.outside.join("p.png");
    put(&pic, b"\x89PNG mine");
    put(&w.group.join("p.png"), b"\x89PNG theirs");
    let plan = plan(&[w.want(&md)], &w.inside());
    assert!(plan[0].pictures.is_empty());
    write(&plan, &w.library, &w.group).expect("written");
    assert_eq!(
        bytes(&w.group.join("p.png")),
        b"\x89PNG theirs",
        "never replaced"
    );
    let copy = text(&w.group.join("a.md"));
    let links = links::links(&copy, &w.group);
    assert!(links::same_file(&links[0].target, &pic), "{copy}");
}

#[test]
fn a_failed_write_takes_away_what_it_made() {
    let w = World::new("fail");
    let a = w.outside.join("a.txt");
    let b = w.outside.join("b.txt");
    put(&a, b"a\n");
    put(&b, b"b\n");
    let plan = plan(&[w.want(&a), w.want(&b)], &w.inside());
    // Someone takes the second name between the plan and the write.
    put(&w.group.join("b.txt"), b"theirs\n");
    let err = write(&plan, &w.library, &w.group).expect_err("the second copy cannot be made");
    assert!(err.contains("b.txt"), "{err}");
    assert!(
        !w.group.join("a.txt").exists(),
        "the first copy is taken away"
    );
    assert_eq!(text(&w.group.join("b.txt")), "theirs\n", "theirs is left");
}

#[test]
fn a_failed_write_leaves_a_copy_that_was_changed_since() {
    // `take_back` removes a copy only while it is what was written.
    let w = World::new("fail-changed");
    let to = w.group.join("a.txt");
    put(&to, b"changed by someone\n");
    let done = Written {
        files: vec![PictureRec {
            path: to.clone(),
            created: true,
            sha256: sha_of(b"a\n"),
            original: None,
            original_sha256: String::new(),
        }],
        pictures: Vec::new(),
        folders: vec![w.group.clone()],
        made_outside: Vec::new(),
    };
    take_back(&done);
    assert_eq!(text(&to), "changed by someone\n");
}

/// Fable's review of C1, 1: a file whose name is chosen again (its links changed, so it no
/// longer holds the bytes of the same-named file there) keeps the pictures it shows, and a
/// picture two files show is written once, not marked as there for both.
#[test]
fn a_second_pass_keeps_each_picture_once() {
    let w = World::new("second-pass");
    let trip = w.outside.join("trip.md");
    let trip_text = b"# Trip\n\n![a](img/a.png)\n\n[Notes](notes.txt)\n";
    put(&trip, trip_text);
    put(&w.outside.join("img").join("a.png"), b"PNG a");
    let other = w.outside.join("other.md");
    put(&other, b"# Other\n\n![a](img/a.png)\n");
    let notes = w.outside.join("notes.txt");
    put(&notes, b"mine\n");
    // There already: trip.md with its very bytes, and a notes.txt of other bytes, so notes.txt
    // goes to notes-2.txt, trip.md's link changes, and trip.md is named again.
    put(&w.group.join("trip.md"), trip_text);
    put(&w.group.join("notes.txt"), b"theirs\n");
    let plan = plan(
        &[w.want(&trip), w.want(&other), w.want(&notes)],
        &w.inside(),
    );
    assert_eq!(plan[0].target(), w.group.join("trip-2.md"));
    assert_eq!(plan[0].pictures.len(), 1, "{plan:?}");
    write(&plan, &w.library, &w.group).expect("written");
    assert_eq!(bytes(&w.group.join("img").join("a.png")), b"PNG a");
    let copy = text(&w.group.join("trip-2.md"));
    assert!(copy.contains("](img/a.png)"), "{copy}");
    assert!(copy.contains("](notes-2.txt)"), "{copy}");
    assert!(text(&w.group.join("other.md")).contains("](img/a.png)"));
}

/// Fable's review of C1, nit: a copy whose links change is named by the text it will hold, so
/// a file there that holds exactly that text is linked, not copied again under another name.
#[test]
fn a_rewritten_copy_is_named_by_its_new_text() {
    let w = World::new("renamed-by-text");
    let x = w.outside.join("x.md");
    put(&x, b"# X\n\n[Y](y.txt)\n");
    let y = w.outside.join("y.txt");
    put(&y, b"mine\n");
    put(&w.group.join("y.txt"), b"theirs\n");
    put(&w.group.join("x.md"), b"# X\n\n[Y](y-2.txt)\n");
    let plan = plan(&[w.want(&x), w.want(&y)], &w.inside());
    assert_eq!(
        plan[0].copy,
        Copy::There {
            to: w.group.join("x.md")
        },
        "{plan:?}"
    );
}

/// Fable's review of C1, 2: folders made above the group's home are not the record's, since
/// undo may not touch them; a write that fails still takes them away.
#[test]
fn folders_made_outside_the_reach_are_kept_apart() {
    let w = World::new("outside-folders");
    let from = w.outside.join("a.txt");
    put(&from, b"a\n");
    let above = w.dir.join("away");
    let home = above.join("home");
    let plan = plan(
        &[Wanted {
            from: from.clone(),
            dir: home.clone(),
        }],
        &w.inside(),
    );
    let done = write(&plan, &w.library, &home).expect("written");
    assert_eq!(done.folders, vec![home.clone()]);
    assert_eq!(done.made_outside, vec![above.clone()]);
    take_back(&done);
    assert!(!above.exists(), "taken back, both");
}

// ---- Move them in (C2) ---------------------------------------------------------------------

/// A group file in the library that lists `note` (a library note) by `link`.
fn list_note(w: &World, note: &Path) {
    let group = w.library.join("Refs.md");
    put(
        &group,
        format!("# Refs\n\n{}\n", crate::library::link_line("Ref", note)).as_bytes(),
    );
}

fn moved(w: &World, wanted: &[Wanted]) -> (Vec<Planned>, Written, MovePlan) {
    let plan = plan(wanted, &w.inside());
    let done = write(&plan, &w.library, &w.group).expect("written");
    let mp = move_plan(&plan, &done, &w.library, &w.group, &[]);
    (plan, done, mp)
}

/// Move them in takes the file and the picture it alone shows away from where they were, once
/// the copies are made; the copies hold their bytes.
#[test]
fn a_moved_file_goes_with_its_picture_and_the_copies_hold_them() {
    let w = World::new("move-plain");
    let md = w.outside.join("trip.md");
    put(&md, b"# Trip\n\n![a](img/a.png)\n");
    let pic = w.outside.join("img").join("a.png");
    put(&pic, b"PNG a");
    let (_, _, mp) = moved(&w, &[w.want(&md)]);
    assert_eq!(mp.removes.len(), 2, "{mp:?}");
    let out = move_out(&mp, &mut crate::runs::Delete);
    assert!(out.failures.is_empty(), "{out:?}");
    assert!(!md.exists() && !pic.exists(), "the originals are gone");
    assert_eq!(
        text(&w.group.join("trip.md")),
        "# Trip\n\n![a](img/a.png)\n"
    );
    assert_eq!(bytes(&w.group.join("img").join("a.png")), b"PNG a");
}

/// A picture another file beside the original shows stays, so that file still finds it.
#[test]
fn a_picture_another_file_shows_stays() {
    let w = World::new("move-shared-pic");
    let md = w.outside.join("trip.md");
    put(&md, b"# Trip\n\n![a](img/a.png)\n");
    let pic = w.outside.join("img").join("a.png");
    put(&pic, b"PNG a");
    put(&w.outside.join("other.md"), b"# Other\n\n![a](img/a.png)\n");
    let (_, _, mp) = moved(&w, &[w.want(&md)]);
    assert_eq!(mp.why_stays(&pic), Some(OTHERS_USE));
    move_out(&mp, &mut crate::runs::Delete);
    assert!(!md.exists() && pic.exists());
}

/// A note of the library, inside its folder, that links to a file that is moved in is made to
/// lead to the copy.
#[test]
fn a_library_note_that_links_to_a_moved_file_leads_to_its_copy() {
    let w = World::new("move-relink");
    let md = w.outside.join("trip.md");
    put(&md, b"# Trip\n");
    let note = w.library.join("Refs").join("ref.md");
    put(&note, b"# Ref\n\nSee [the trip](../../desk/trip.md).\n");
    list_note(&w, &note);
    let (_, _, mp) = moved(&w, &[w.want(&md)]);
    assert_eq!(mp.relinks.len(), 1, "{mp:?}");
    let out = move_out(&mp, &mut crate::runs::Delete);
    assert!(out.failures.is_empty(), "{out:?}");
    assert!(!md.exists());
    assert_eq!(text(&note), "# Ref\n\nSee [the trip](../Shots/trip.md).\n");
}

/// A note the library lists outside its reach that links to the file keeps the file where it
/// is: undo could not give the note back, so it is not rewritten and the file is copied only.
#[test]
fn a_library_note_outside_the_reach_keeps_the_original() {
    let w = World::new("move-outside-note");
    let md = w.outside.join("trip.md");
    put(&md, b"# Trip\n");
    let note = w.dir.join("elsewhere").join("ref.md");
    put(&note, b"# Ref\n\n[Trip](../desk/trip.md)\n");
    list_note(&w, &note);
    let (_, _, mp) = moved(&w, &[w.want(&md)]);
    assert_eq!(mp.why_stays(&md), Some(LINKED_OUTSIDE), "{mp:?}");
    move_out(&mp, &mut crate::runs::Delete);
    assert!(md.exists());
    assert_eq!(text(&note), "# Ref\n\n[Trip](../desk/trip.md)\n");
}

/// An original changed after it was copied is not taken away: the copy is not what it holds.
#[test]
fn an_original_changed_since_it_was_copied_stays() {
    let w = World::new("move-changed");
    let md = w.outside.join("a.txt");
    put(&md, b"one\n");
    let (_, _, mp) = moved(&w, &[w.want(&md)]);
    assert_eq!(mp.removes.len(), 1);
    put(&md, b"one, and more\n");
    let out = move_out(&mp, &mut crate::runs::Delete);
    assert!(out.removed.is_empty() && md.exists(), "{out:?}");
    put(&md, b"one\n");
    let other = w.outside.join("b.txt");
    put(&other, b"two\n");
    let plan = plan(&[w.want(&other)], &w.inside());
    let done = write(&plan, &w.library, &w.group).expect("written");
    put(&other, b"two, changed\n");
    let mp = move_plan(&plan, &done, &w.library, &w.group, &[]);
    assert_eq!(mp.why_stays(&other), Some(CHANGED));
}

/// Fable's second review of C1, nit: a file that cannot be copied after all (every name taken
/// once its links change) gives up the picture it was to write, and another file of the run
/// that shows the same picture writes it.
#[test]
fn a_file_that_stays_after_all_hands_its_picture_on() {
    let w = World::new("stays-hands-on");
    let x = w.outside.join("x.md");
    let x_text = b"# X\n\n![a](img/a.png)\n\n[N](n.txt)\n";
    put(&x, x_text);
    put(&w.outside.join("img").join("a.png"), b"PNG a");
    let y = w.outside.join("y.md");
    put(&y, b"# Y\n\n![a](img/a.png)\n");
    let n = w.outside.join("n.txt");
    put(&n, b"mine\n");
    put(&w.group.join("x.md"), x_text);
    for k in 2..=NAMES_MAX {
        put(&w.group.join(format!("x-{k}.md")), b"other\n");
    }
    put(&w.group.join("n.txt"), b"theirs\n");
    let plan = plan(&[w.want(&x), w.want(&y), w.want(&n)], &w.inside());
    assert!(
        matches!(plan[0].copy, Copy::Stays { .. }),
        "{:?}",
        plan[0].copy
    );
    write(&plan, &w.library, &w.group).expect("written");
    assert_eq!(bytes(&w.group.join("img").join("a.png")), b"PNG a");
}

/// Fable's second review of C1, nit: a chain of files, each linking to the next, all renamed
/// because the last one's link changes, settles however long it is.
#[test]
fn a_long_chain_of_links_settles() {
    let w = World::new("chain");
    let mut wanted = Vec::new();
    for k in 1..=6 {
        let next = if k == 6 {
            "n.txt".to_string()
        } else {
            format!("f{}.md", k + 1)
        };
        let text = format!("# {k}\n\n[next]({next})\n");
        let f = w.outside.join(format!("f{k}.md"));
        put(&f, text.as_bytes());
        put(&w.group.join(format!("f{k}.md")), text.as_bytes());
        wanted.push(w.want(&f));
    }
    let n = w.outside.join("n.txt");
    put(&n, b"mine\n");
    put(&w.group.join("n.txt"), b"theirs\n");
    wanted.push(w.want(&n));
    let plan = plan(&wanted, &w.inside());
    for (k, p) in plan.iter().take(6).enumerate() {
        assert_eq!(
            p.target(),
            w.group.join(format!("f{}-2.md", k + 1)),
            "{plan:?}"
        );
    }
}

/// Fable's review of C2, 1: a file that stays keeps the picture it shows where it is, though no
/// other file shows it.
#[test]
fn a_picture_shown_by_a_file_that_stays_stays_with_it() {
    let w = World::new("move-pic-of-staying");
    let md = w.outside.join("trip.md");
    put(&md, b"# Trip\n\n![b](img/beach.png)\n");
    let pic = w.outside.join("img").join("beach.png");
    put(&pic, b"PNG b");
    let note = w.dir.join("elsewhere").join("ref.md");
    put(&note, b"# Ref\n\n[Trip](../desk/trip.md)\n");
    list_note(&w, &note);
    let (_, _, mp) = moved(&w, &[w.want(&md)]);
    assert_eq!(mp.why_stays(&md), Some(LINKED_OUTSIDE), "{mp:?}");
    assert_eq!(mp.why_stays(&pic), Some(SHOWN_BY_STAYING), "{mp:?}");
    move_out(&mp, &mut crate::runs::Delete);
    assert!(md.exists() && pic.exists());
}

/// The bin that refuses one file.
struct Refusing(PathBuf);

impl crate::runs::Bin for Refusing {
    fn recycle(&mut self, path: &Path) -> Result<(), String> {
        if path == self.0 {
            return Err("in use".to_string());
        }
        std::fs::remove_file(path).map_err(|e| e.to_string())
    }
}

/// Fable's review of C2, 1: a picture goes only once the file that shows it has gone; a file the
/// bin would not take keeps its picture.
#[test]
fn a_picture_stays_when_the_file_that_shows_it_could_not_go() {
    let w = World::new("move-pic-held");
    let md = w.outside.join("trip.md");
    put(&md, b"# Trip\n\n![b](img/beach.png)\n");
    let pic = w.outside.join("img").join("beach.png");
    put(&pic, b"PNG b");
    let (_, _, mp) = moved(&w, &[w.want(&md)]);
    assert_eq!(mp.removes.len(), 2, "{mp:?}");
    let out = move_out(&mp, &mut Refusing(md.clone()));
    assert!(md.exists() && pic.exists(), "{out:?}");
    assert!(
        out.failures
            .iter()
            .any(|(p, w)| p == &pic && w == SHOWN_BY_STAYING),
        "{out:?}"
    );
}

/// Fable's review of C2, 3: a file a tab holds with changes not saved stays (with the picture it
/// shows), and so does a file a library note open that way links to: neither is changed from
/// under its tab.
#[test]
fn a_file_or_a_note_open_with_changes_not_saved_keeps_its_original() {
    let w = World::new("move-unsaved");
    let md = w.outside.join("trip.md");
    put(&md, b"# Trip\n\n![b](img/beach.png)\n");
    let pic = w.outside.join("img").join("beach.png");
    put(&pic, b"PNG b");
    let other = w.outside.join("b.txt");
    put(&other, b"two\n");
    let note = w.library.join("Refs").join("ref.md");
    put(&note, b"# Ref\n\nSee [b](../../desk/b.txt).\n");
    list_note(&w, &note);
    let plan = plan(&[w.want(&md), w.want(&other)], &w.inside());
    let done = write(&plan, &w.library, &w.group).expect("written");
    let mp = move_plan(
        &plan,
        &done,
        &w.library,
        &w.group,
        &[md.clone(), note.clone()],
    );
    assert_eq!(mp.why_stays(&md), Some(UNSAVED), "{mp:?}");
    assert_eq!(mp.why_stays(&pic), Some(SHOWN_BY_STAYING), "{mp:?}");
    assert_eq!(mp.why_stays(&other), Some(UNSAVED_NOTE), "{mp:?}");
    assert!(mp.relinks.is_empty() && mp.removes.is_empty(), "{mp:?}");
    // Without the tabs, both go and the note is rewritten.
    let mp = move_plan(&plan, &done, &w.library, &w.group, &[]);
    assert_eq!(mp.removes.len(), 3, "{mp:?}");
    assert_eq!(mp.relinks.len(), 1, "{mp:?}");
}

/// Fable's review of C2, 5: an original goes only while its copy is the bytes that were written.
#[test]
fn an_original_goes_only_while_its_copy_holds_what_was_written() {
    let w = World::new("move-copy-changed");
    let md = w.outside.join("a.txt");
    put(&md, b"one\n");
    let (_, _, mp) = moved(&w, &[w.want(&md)]);
    assert_eq!(mp.removes.len(), 1, "{mp:?}");
    put(&w.group.join("a.txt"), b"not the copy\n");
    let out = move_out(&mp, &mut crate::runs::Delete);
    assert!(md.exists() && out.removed.is_empty(), "{out:?}");
}
