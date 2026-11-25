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
    let done = write(&plan).expect("written");
    assert_eq!(bytes(&to), b"%PDF-1.4 the owner's\n");
    assert_eq!(
        bytes(&from),
        b"%PDF-1.4 the owner's\n",
        "the original is not touched"
    );
    assert_eq!(done.files.len(), 1);
    assert!(done.files[0].1, "this run made it");
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
    write(&plan).expect("written");
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
    let done = write(&plan).expect("written");
    assert!(!done.files[0].1, "not this run's file");
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
    let done = write(&plan).expect("nothing to write");
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
    write(&plan).expect("written");
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
    let done = write(&plan).expect("written");
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
    write(&plan).expect("written");
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
    write(&plan).expect("written");
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
    write(&plan).expect("written");
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
    let done = write(&plan).expect("written");
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
    write(&plan).expect("written");
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
    let err = write(&plan).expect_err("the second copy cannot be made");
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
        files: vec![(to.clone(), true, sha_of(b"a\n"))],
        pictures: Vec::new(),
        folders: vec![w.group.clone()],
    };
    take_back(&done);
    assert_eq!(text(&to), "changed by someone\n");
}
