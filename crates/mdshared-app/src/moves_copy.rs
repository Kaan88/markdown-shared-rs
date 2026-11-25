//! "Copy them in" (`docs/ai-pictures-plan.md` 4.3, phase C1; D-209): files filed into a group
//! are copied into the folder their group and section give them, and the library's line points
//! at the copy. A Markdown file takes the pictures it shows from its own folder or below to the
//! same place beside the copy, so the words of those links do not change (the mover's rule,
//! D-091); its other relative links are rewritten to lead where they did (D-093). Nothing of the
//! owner's is changed, moved or removed: a copy is made with `create_new` and never replaces a
//! file, and a write that fails takes away what it made before it says why.
//!
//! Nothing read here can panic: every file is read through [`read_regular`] with a limit, and
//! every cut of a text is the link reader's, which is checked.

use super::links;
use super::{PATH_MAX_CHARS, below, chars, in_the_way};
use crate::paths::same_path;
use crate::runs::{DIGEST_MAX_BYTES, PictureRec, digest_of, read_regular};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The most names tried in a folder: `name.md`, `name-2.md` … `name-99.md`.
const NAMES_MAX: usize = 99;

/// One file to copy in: where it is, and the folder it goes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub from: PathBuf,
    pub dir: PathBuf,
}

/// What happens to one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Copy {
    /// Copied to `to`. `text` is what is written when the file's links had to change; `None`
    /// copies its bytes as they are.
    New { to: PathBuf, text: Option<String> },
    /// A file with these very bytes is at `to` already: linked there, not written again.
    There { to: PathBuf },
    /// Linked where it is, for the reason given: it is inside the library already, or it
    /// cannot be copied.
    Stays { why: String },
}

/// A picture a Markdown file shows, copied beside its copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureCopy {
    pub from: PathBuf,
    pub to: PathBuf,
    /// A file with these very bytes is at `to` already (another file of the run brought it, or
    /// it was there before): nothing is written for it.
    pub there: bool,
}

/// The plan for one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    pub from: PathBuf,
    pub copy: Copy,
    pub pictures: Vec<PictureCopy>,
    /// How many of its links are rewritten.
    pub rewrites: usize,
    /// What its row says besides where it goes: "not UTF-8: its links are as they were".
    pub note: Option<String>,
}

impl Planned {
    /// What the library's line leads to: the copy, or the file itself when it stays.
    pub fn target(&self) -> &Path {
        match &self.copy {
            Copy::New { to, .. } | Copy::There { to } => to,
            Copy::Stays { .. } => &self.from,
        }
    }
}

/// Why a file stays where it is.
pub const INSIDE: &str = "already inside the library's folder: linked where it is";

/// What is copied, and where. `inside` are the folders a file already in stays in (the library
/// folder and the group's home): those are linked where they are, as "Move the notes…" is the way
/// to rearrange them.
pub fn plan(wanted: &[Wanted], inside: &[PathBuf]) -> Vec<Planned> {
    let mut taken: Vec<PathBuf> = Vec::new();
    // First the names, from the files' own bytes.
    let mut out: Vec<Planned> = wanted
        .iter()
        .map(|w| {
            let copy = first_copy(w, inside, &mut taken);
            Planned {
                from: w.from.clone(),
                copy,
                pictures: Vec::new(),
                rewrites: 0,
                note: None,
            }
        })
        .collect();
    // Then the pictures each Markdown file shows, once (Fable's review of C1: planned again,
    // a picture already taken by this very file would be dropped, and a picture two files share
    // would be marked as there for both, so neither wrote it).
    let mut texts: Vec<Option<String>> = vec![None; out.len()];
    for i in 0..out.len() {
        let Some(w) = wanted.get(i) else { break };
        if matches!(out[i].copy, Copy::Stays { .. }) || !is_markdown(&w.from) {
            continue;
        }
        let Some(text) = links::note_text(&w.from) else {
            out[i].note = Some("not UTF-8: copied with its links as they were".to_string());
            continue;
        };
        let Some(from_dir) = w.from.parent() else {
            continue;
        };
        out[i].pictures = pictures_of(&text, from_dir, &w.dir, inside, &out, i, &mut taken);
        texts[i] = Some(text);
    }
    // Then each Markdown file's links. A file whose text changes is no longer the bytes it was
    // named by, so its name is chosen again from the text it will hold: a file there may hold
    // exactly that, or the name may be taken by other bytes. That can change where another
    // file's link must lead, so this is done again until nothing moves.
    for _ in 0..passes_max(out.len()) {
        let mut moved = false;
        for i in 0..out.len() {
            let (Some(w), Some(text)) = (wanted.get(i), texts.get(i).and_then(Option::as_ref))
            else {
                continue;
            };
            if matches!(out[i].copy, Copy::Stays { .. }) {
                continue;
            }
            let Some(from_dir) = w.from.parent() else {
                continue;
            };
            let places: Vec<(PathBuf, PathBuf)> = out
                .iter()
                .enumerate()
                .filter(|(k, _)| *k != i)
                .map(|(_, p)| (p.from.clone(), p.target().to_path_buf()))
                .collect();
            let pictures = &out[i].pictures;
            let place = |p: &Path| -> PathBuf {
                if let Some(c) = pictures.iter().find(|c| same_path(&c.from, p)) {
                    return c.to.clone();
                }
                match places.iter().find(|(f, _)| links::same_file(f, p)) {
                    Some((_, t)) => t.clone(),
                    None => p.to_path_buf(),
                }
            };
            let (new, n) = links::rewrite(text, from_dir, &w.dir, &place, &[]);
            out[i].rewrites = n;
            let wants = (n > 0).then_some(new);
            let same = match (&out[i].copy, &wants) {
                (Copy::New { text: had, .. }, _) => had == &wants,
                (Copy::There { to }, Some(t)) => holds(to, t.as_bytes()),
                (Copy::There { .. }, None) => true,
                (Copy::Stays { .. }, _) => true,
            };
            if same {
                continue;
            }
            let before = out[i].target().to_path_buf();
            taken.retain(|t| !same_path(t, &before));
            let bytes = wants
                .as_deref()
                .map(str::as_bytes)
                .unwrap_or(text.as_bytes());
            out[i].copy = match free_name(w, bytes, &mut taken) {
                Ok((to, true)) => Copy::There { to },
                Ok((to, false)) => Copy::New { to, text: wants },
                Err(why) => Copy::Stays { why },
            };
            if matches!(out[i].copy, Copy::Stays { .. }) {
                give_up_pictures(&mut out, i, &mut taken);
            }
            if !same_path(out[i].target(), &before) {
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    out
}

/// The most link passes: each pass settles at least one more file of a chain of links, so a
/// run of `n` files needs at most `n + 1` (Fable's review of C1); a run is a few dozen files.
fn passes_max(files: usize) -> usize {
    (files + 1).clamp(2, 200)
}

/// A file that will not be copied after all takes no picture with it: one it was to write is
/// written by another file of the run that shows it, if any, or its place is free again.
fn give_up_pictures(out: &mut [Planned], me: usize, taken: &mut Vec<PathBuf>) {
    let Some(mine) = out.get_mut(me).map(|p| std::mem::take(&mut p.pictures)) else {
        return;
    };
    for c in mine.iter().filter(|c| !c.there) {
        let heir = out
            .iter_mut()
            .enumerate()
            .filter(|(k, _)| *k != me)
            .flat_map(|(_, p)| p.pictures.iter_mut())
            .find(|o| o.there && same_path(&o.to, &c.to) && !exists(&o.to));
        match heir {
            Some(o) => o.there = false,
            None => taken.retain(|t| !same_path(t, &c.to)),
        }
    }
}

/// What is written, for the record.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Written {
    /// Each file's copy: its path, whether this run made it, the digest of its bytes, and the
    /// file it is a copy of with that file's digest, so undo removes the copy only while the
    /// original is where it was (Fable's review of C1).
    pub files: Vec<PictureRec>,
    pub pictures: Vec<PictureRec>,
    /// The folders this run made inside the library's reach (the library folder and the group's
    /// home), parents first: the record's, which undo removes when they are empty.
    pub folders: Vec<PathBuf>,
    /// The folders it made above the group's home, which undo may not touch: only
    /// [`take_back`] removes them, when a write fails.
    pub made_outside: Vec<PathBuf>,
}

/// Writes the copies `plan` gives. A file is read again here, and one that changed since its
/// name was chosen is still copied as it is now. On the first failure everything this call made
/// is taken away again (only while it is the bytes it wrote) and the reason is given. `library`
/// and `home` are the run's reach: a folder made outside them is not the record's.
pub fn write(plan: &[Planned], library: &Path, home: &Path) -> Result<Written, String> {
    let mut done = Written::default();
    let reach = |p: &Path| crate::runs::inside(library, home, p);
    let result = write_all(plan, &reach, &mut done);
    if let Err(why) = result {
        take_back(&done);
        return Err(why);
    }
    Ok(done)
}

fn write_all(
    plan: &[Planned],
    reach: &dyn Fn(&Path) -> bool,
    done: &mut Written,
) -> Result<(), String> {
    for p in plan {
        match &p.copy {
            Copy::Stays { .. } => continue,
            Copy::There { to } => {
                done.files.push(PictureRec {
                    path: to.clone(),
                    created: false,
                    sha256: digest_of(to).unwrap_or_default(),
                    original: Some(p.from.clone()),
                    original_sha256: digest_of(&p.from).unwrap_or_default(),
                });
            }
            Copy::New { to, text } => {
                let original = read_regular(&p.from, DIGEST_MAX_BYTES, false)
                    .ok_or_else(|| format!("{} could not be read", p.from.display()))?;
                let bytes = match text {
                    Some(t) => t.as_bytes().to_vec(),
                    None => original.clone(),
                };
                make_dirs(to, reach, done)?;
                create_new(to, &bytes)
                    .map_err(|e| format!("{} could not be written: {e}", to.display()))?;
                done.files.push(PictureRec {
                    path: to.clone(),
                    created: true,
                    sha256: sha_of(&bytes),
                    original: Some(p.from.clone()),
                    original_sha256: sha_of(&original),
                });
            }
        }
        for c in &p.pictures {
            if done.pictures.iter().any(|r| same_path(&r.path, &c.to)) {
                continue;
            }
            let original_sha256 = digest_of(&c.from).unwrap_or_default();
            if c.there {
                done.pictures.push(PictureRec {
                    path: c.to.clone(),
                    created: false,
                    sha256: digest_of(&c.to).unwrap_or_default(),
                    original: Some(c.from.clone()),
                    original_sha256,
                });
                continue;
            }
            let bytes = read_regular(&c.from, DIGEST_MAX_BYTES, false)
                .ok_or_else(|| format!("{} could not be read", c.from.display()))?;
            make_dirs(&c.to, reach, done)?;
            create_new(&c.to, &bytes)
                .map_err(|e| format!("{} could not be written: {e}", c.to.display()))?;
            done.pictures.push(PictureRec {
                path: c.to.clone(),
                created: true,
                sha256: sha_of(&bytes),
                original: Some(c.from.clone()),
                original_sha256,
            });
        }
    }
    Ok(())
}

/// Takes away what a write made: its files while they are the bytes it wrote, then its
/// folders while they are empty, deepest first. [`write`] calls it when it fails; a caller calls
/// it when what comes after the copies (the library's lines) cannot be written.
pub fn take_back(done: &Written) {
    for p in done.files.iter().chain(&done.pictures) {
        if p.created && digest_of(&p.path).is_some_and(|d| d.eq_ignore_ascii_case(&p.sha256)) {
            let _ = std::fs::remove_file(&p.path);
        }
    }
    let mut folders: Vec<&PathBuf> = done.folders.iter().chain(&done.made_outside).collect();
    folders.sort_by_key(|f| std::cmp::Reverse(f.components().count()));
    for folder in folders {
        let _ = std::fs::remove_dir(folder);
    }
}

// ---- Move them in (phase C2, D-210) --------------------------------------------------------------

/// Why an original stays where it is although the switch says "Move them in": its row says it.
pub const OTHER_DRIVE: &str = "on another drive: copied";
pub const CHANGED: &str = "changed since it was copied: copied";
pub const LINKED_OUTSIDE: &str = "a note of the library outside its folder links to it: copied";
pub const LISTED: &str = "a group of the library lists it: copied";
pub const OTHERS_USE: &str = "another file links to it: copied";
pub const NOT_READ: &str = "not every file that may link to it could be read: copied";
pub const TOO_LARGE: &str = "too large for undo to keep: copied";
pub const UNSAVED: &str = "open with changes not saved: copied";
pub const UNSAVED_NOTE: &str = "a note open with changes not saved links to it: copied";
pub const SHOWN_BY_STAYING: &str = "a file that stays shows it: copied";
/// A move whose record would be too large to read back is a copy (Fable's review of C2, 2).
pub const RECORD_FULL: &str = "the run's record has no room to keep it for undo: copied";

/// The most text a move keeps for undo (the originals of rewritten files and of the library's
/// notes whose links it changes), well inside what a record may hold.
const KEPT_MAX: usize = 3 * 1024 * 1024;

/// An original "Move them in" takes away once its copy is listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remove {
    pub path: PathBuf,
    /// Its digest when the plan was made: it goes only while it still has it.
    pub sha256: String,
    /// Where its bytes are now: the copy, or the picture's copy.
    pub copy: PathBuf,
    /// The copy's digest when the plan was made: the original goes only while the copy is these
    /// bytes (Fable's review of C2, 5).
    pub copy_sha256: String,
    /// Its text, when the copy's text differs (its links were rewritten): what undo writes back.
    pub text: Option<String>,
    /// For a picture, the files of the run that show it: it goes only once every one of them has
    /// gone (Fable's review of C2, 1). Empty for a file.
    pub shown_by: Vec<PathBuf>,
}

/// A note of the library whose links lead to an original that goes: they are made to lead to
/// the copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relink {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
    /// The originals its links lead to: none of them goes if it cannot be rewritten.
    pub targets: Vec<PathBuf>,
}

/// What "Move them in" does after the copies are made and listed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MovePlan {
    /// The files first, then the pictures: [`move_out`] takes them in this order.
    pub removes: Vec<Remove>,
    pub relinks: Vec<Relink>,
    /// The originals that stay, and why.
    pub stays: Vec<(PathBuf, String)>,
}

impl MovePlan {
    /// Why `path` stays, when it does.
    pub fn why_stays(&self, path: &Path) -> Option<&str> {
        self.stays
            .iter()
            .find(|(p, _)| same_path(p, path))
            .map(|(_, w)| w.as_str())
    }

    /// Everything stays, for `why`: a move whose record cannot hold it is a copy.
    pub fn none(&self, why: &str) -> MovePlan {
        let mut out = MovePlan {
            stays: self.stays.clone(),
            ..MovePlan::default()
        };
        for r in &self.removes {
            out.stays.push((r.path.clone(), why.to_string()));
        }
        out
    }
}

/// What a move did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MoveOut {
    pub removed: Vec<PathBuf>,
    pub relinked: Vec<PathBuf>,
    /// What could not be done, and why: the original is then where it was.
    pub failures: Vec<(PathBuf, String)>,
}

/// What of a written copy-in goes from where it was. An original goes only when it is on the
/// copy's drive, still the bytes that were copied, held by no tab with changes not saved
/// (`unsaved`), listed by no group, and linked to by no note of the library outside the
/// library's reach (`library` and `home`: undo may rewrite the notes inside it only). A picture
/// goes only when every file of the run that shows it goes, and no other file that may link to it
/// does (the files the library lists and the Markdown files beside the originals), as the mover's
/// carry rule says (D-091). The library's notes inside the reach that link to an original that
/// goes are rewritten to lead to its copy; one open with changes not saved keeps what it links to
/// where it is. Any other file on the disk is not looked for.
pub fn move_plan(
    plan: &[Planned],
    written: &Written,
    library: &Path,
    home: &Path,
    unsaved: &[PathBuf],
) -> MovePlan {
    let mut out = MovePlan::default();
    let stay = |out: &mut MovePlan, path: &Path, why: &str| {
        if out.why_stays(path).is_none() {
            out.stays.push((path.to_path_buf(), why.to_string()));
        }
    };
    let open = |path: &Path| unsaved.iter().any(|u| links::same_file(u, path));
    let froms: Vec<PathBuf> = plan
        .iter()
        .filter(|p| !matches!(p.copy, Copy::Stays { .. }))
        .map(|p| p.from.clone())
        .collect();
    // The files themselves.
    let mut files: Vec<Remove> = Vec::new();
    let mut kept_files = 0usize;
    for p in plan {
        if matches!(p.copy, Copy::Stays { .. }) {
            continue;
        }
        let Some(rec) = written
            .files
            .iter()
            .find(|f| same_path(&f.path, p.target()))
        else {
            continue;
        };
        if open(&p.from) {
            stay(&mut out, &p.from, UNSAVED);
            continue;
        }
        if !same_volume(&p.from, &rec.path) {
            stay(&mut out, &p.from, OTHER_DRIVE);
            continue;
        }
        if !digest_of(&p.from).is_some_and(|d| d.eq_ignore_ascii_case(&rec.original_sha256)) {
            stay(&mut out, &p.from, CHANGED);
            continue;
        }
        let text = if rec.sha256.eq_ignore_ascii_case(&rec.original_sha256) {
            None
        } else {
            match links::note_text(&p.from) {
                Some(t) if kept_files + t.len() <= KEPT_MAX => {
                    kept_files += t.len();
                    Some(t)
                }
                _ => {
                    stay(&mut out, &p.from, TOO_LARGE);
                    continue;
                }
            }
        };
        files.push(Remove {
            path: p.from.clone(),
            sha256: rec.original_sha256.clone(),
            copy: rec.path.clone(),
            copy_sha256: rec.sha256.clone(),
            text,
            shown_by: Vec::new(),
        });
    }
    // Who else may link to the pictures: the files the library lists, every file of the run,
    // and the Markdown files beside them.
    let lib = crate::library::read(library);
    let listed: Vec<PathBuf> = lib
        .groups
        .iter()
        .flat_map(|g| g.entries.iter().map(|e| e.path.clone()))
        .collect();
    let mut users: Vec<PathBuf> = listed.clone();
    users.extend(plan.iter().map(|p| p.from.clone()));
    let mut dirs: Vec<PathBuf> = plan
        .iter()
        .filter_map(|p| p.from.parent().map(Path::to_path_buf))
        .collect();
    dirs.sort();
    dirs.dedup_by(|a, b| same_path(a, b));
    for d in &dirs {
        users.extend(super::md_files_in(d, links::USERS_MAX));
    }
    let index = links::Users::read(&users);
    // The library's notes, each read once: those that may link to what goes.
    let notes: Vec<(PathBuf, String)> = listed
        .iter()
        .filter(|l| is_markdown(l) && !froms.iter().any(|f| links::same_file(f, l)))
        .filter(|l| !written.files.iter().any(|w| same_path(&w.path, l)))
        .filter_map(|l| links::note_text(l).map(|t| (l.clone(), t)))
        .collect();
    let leads_to = |text: &str, note: &Path, path: &Path| {
        note.parent().is_some_and(|base| {
            links::links(text, base)
                .iter()
                .any(|l| links::same_file(&l.target, path))
        })
    };
    // What no rewrite can follow stays: a file a group lists (its line would lead to nothing),
    // one a note outside the reach links to, one a note open with changes not saved links to.
    let held_by_notes = |out: &mut MovePlan, list: &mut Vec<Remove>| {
        list.retain(|c| {
            let why = if listed.iter().any(|l| links::same_file(l, &c.path)) {
                Some(LISTED)
            } else {
                notes.iter().find_map(|(note, text)| {
                    if !leads_to(text, note, &c.path) {
                        None
                    } else if !crate::runs::inside(library, home, note) {
                        Some(LINKED_OUTSIDE)
                    } else if open(note) {
                        Some(UNSAVED_NOTE)
                    } else {
                        None
                    }
                })
            };
            if let Some(w) = why {
                stay(out, &c.path, w);
            }
            why.is_none()
        });
    };
    held_by_notes(&mut out, &mut files);
    // The files settle first, the pictures after them: a picture is decided again whenever a
    // file it is shown by stays. Each round leaves at least one more original where it is.
    for _ in 0..=plan.len() + written.pictures.len() + 1 {
        let going: Vec<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
        let mut pictures: Vec<Remove> = Vec::new();
        for pic in &written.pictures {
            let Some(o) = &pic.original else { continue };
            if files.iter().any(|c| same_path(&c.path, o))
                || pictures.iter().any(|c| same_path(&c.path, o))
                || out.why_stays(o).is_some()
            {
                continue;
            }
            let shown_by: Vec<PathBuf> = plan
                .iter()
                .filter(|p| p.pictures.iter().any(|c| same_path(&c.from, o)))
                .map(|p| p.from.clone())
                .collect();
            let why = if open(o) {
                Some(UNSAVED)
            } else if !same_volume(o, &pic.path) {
                Some(OTHER_DRIVE)
            } else if !digest_of(o).is_some_and(|d| d.eq_ignore_ascii_case(&pic.original_sha256)) {
                Some(CHANGED)
            } else if !index.complete {
                Some(NOT_READ)
            } else if !shown_by
                .iter()
                .all(|s| going.iter().any(|g| same_path(g, s)))
            {
                Some(SHOWN_BY_STAYING)
            } else if index.others_than(o, &going) {
                Some(OTHERS_USE)
            } else {
                None
            };
            match why {
                Some(w) => stay(&mut out, o, w),
                None => pictures.push(Remove {
                    path: o.clone(),
                    sha256: pic.original_sha256.clone(),
                    copy: pic.path.clone(),
                    copy_sha256: pic.sha256.clone(),
                    text: None,
                    shown_by,
                }),
            }
        }
        held_by_notes(&mut out, &mut pictures);
        // The library's notes inside the reach that lead to what goes are rewritten to lead to
        // its copy; one that cannot be keeps what it leads to where it is, and the round is made
        // again without them.
        let all: Vec<Remove> = files.iter().chain(pictures.iter()).cloned().collect();
        let mut relinks = Vec::new();
        let mut kept = kept_files;
        let mut dropped: Vec<PathBuf> = Vec::new();
        for (note, text) in &notes {
            if !crate::runs::inside(library, home, note) {
                continue;
            }
            let targets: Vec<PathBuf> = all
                .iter()
                .filter(|c| leads_to(text, note, &c.path))
                .map(|c| c.path.clone())
                .collect();
            if targets.is_empty() {
                continue;
            }
            let Some(base) = note.parent() else { continue };
            let place = |p: &Path| -> PathBuf {
                match all.iter().find(|c| links::same_file(&c.path, p)) {
                    Some(c) => c.copy.clone(),
                    None => p.to_path_buf(),
                }
            };
            let (after, n) = links::rewrite(text, base, base, &place, &[]);
            if n == 0 || kept + text.len() > KEPT_MAX {
                for t in &targets {
                    stay(&mut out, t, TOO_LARGE);
                }
                dropped.extend(targets);
                continue;
            }
            kept += text.len();
            relinks.push(Relink {
                path: note.clone(),
                before: text.clone(),
                after,
                targets,
            });
        }
        if dropped.is_empty() {
            out.relinks = relinks;
            out.removes = all;
            return out;
        }
        files.retain(|c| !dropped.iter().any(|d| same_path(d, &c.path)));
    }
    // Never reached: every round that does not return leaves an original where it is. Should it
    // be, nothing goes.
    let stays: Vec<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
    for s in stays {
        stay(&mut out, &s, TOO_LARGE);
    }
    out
}

/// Does what `mp` says, after its record is written: each note rewritten (only while it still
/// says what was read), then each original to `bin`, only while it is the bytes that were copied
/// and its copy is there with the bytes it was written with, and only when every note that links
/// to it was rewritten and, for a picture, every file of the run that shows it has gone.
pub fn move_out(mp: &MovePlan, bin: &mut dyn crate::runs::Bin) -> MoveOut {
    let mut done = MoveOut::default();
    let mut held: Vec<PathBuf> = Vec::new();
    for r in &mp.relinks {
        match super::rewrite_note(&r.path, &r.before, &r.after) {
            Ok(()) => done.relinked.push(r.path.clone()),
            Err(why) => {
                held.extend(r.targets.iter().cloned());
                done.failures.push((r.path.clone(), why));
            }
        }
    }
    for r in &mp.removes {
        if held.iter().any(|h| same_path(h, &r.path)) {
            done.failures.push((
                r.path.clone(),
                "a note that links to it could not be rewritten: copied".to_string(),
            ));
            continue;
        }
        if !r
            .shown_by
            .iter()
            .all(|s| done.removed.iter().any(|d| same_path(d, s)))
        {
            done.failures
                .push((r.path.clone(), SHOWN_BY_STAYING.to_string()));
            continue;
        }
        let same = digest_of(&r.path).is_some_and(|d| d.eq_ignore_ascii_case(&r.sha256));
        let copy = plain(&r.copy)
            && digest_of(&r.copy).is_some_and(|d| d.eq_ignore_ascii_case(&r.copy_sha256))
            && !one_file(&r.path, &r.copy);
        if !same || !copy {
            done.failures.push((r.path.clone(), CHANGED.to_string()));
            continue;
        }
        match bin.recycle(&r.path) {
            Ok(()) => done.removed.push(r.path.clone()),
            Err(why) => done.failures.push((r.path.clone(), why)),
        }
    }
    done
}

/// Whether two paths are one file on the disk: the same device and file number on Unix (a bind
/// mount or a hard link), the same path once resolved on Windows.
fn one_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let id = |p: &Path| std::fs::metadata(p).ok().map(|m| (m.dev(), m.ino()));
        let (ia, ib) = (id(a), id(b));
        ia.is_some() && ia == ib
    }
    #[cfg(not(unix))]
    {
        links::same_file(a, b)
    }
}

/// Whether two paths are on one volume: a rename between them is a move, not a copy.
fn same_volume(a: &Path, b: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::path::Component;
        let root = |p: &Path| match crate::paths::canonical_or_absolute(p).components().next() {
            Some(Component::Prefix(x)) => Some(x.as_os_str().to_string_lossy().to_lowercase()),
            _ => None,
        };
        let (ra, rb) = (root(a), root(b));
        ra.is_some() && ra == rb
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let dev = |p: &Path| std::fs::metadata(p).ok().map(|m| m.dev());
        let (da, db) = (dev(a), dev(b));
        da.is_some() && da == db
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (a, b);
        false
    }
}

// ---- the parts -----------------------------------------------------------------------------------

fn is_markdown(path: &Path) -> bool {
    crate::runs::has_extension(path, &["md", "markdown"])
}

/// A regular file, not a link to one, that a copy can be made of.
fn plain(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink() && m.len() <= DIGEST_MAX_BYTES)
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Whether the file at `path` holds exactly `bytes`.
fn holds(path: &Path, bytes: &[u8]) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() == bytes.len() as u64)
        && digest_of(path).is_some_and(|d| d.eq_ignore_ascii_case(&sha_of(bytes)))
}

fn sha_of(bytes: &[u8]) -> String {
    crate::update::sha256_hex(bytes)
}

/// The copy of one file by its own bytes: where it goes, or why it stays.
fn first_copy(w: &Wanted, inside: &[PathBuf], taken: &mut Vec<PathBuf>) -> Copy {
    if inside.iter().any(|d| below(d, &w.from).is_some()) {
        return Copy::Stays {
            why: INSIDE.to_string(),
        };
    }
    if !plain(&w.from) {
        return Copy::Stays {
            why: "not a file that can be copied: linked where it is".to_string(),
        };
    }
    let Some(bytes) = read_regular(&w.from, DIGEST_MAX_BYTES, false) else {
        return Copy::Stays {
            why: "could not be read: linked where it is".to_string(),
        };
    };
    match free_name(w, &bytes, taken) {
        Ok((to, true)) => Copy::There { to },
        Ok((to, false)) => Copy::New { to, text: None },
        Err(why) => Copy::Stays { why },
    }
}

/// The name in `w.dir` for a file of `bytes`: its own name when free, the one there when it
/// holds the same bytes (`true`), else `name-2` and on. Never a name the run already took.
fn free_name(
    w: &Wanted,
    bytes: &[u8],
    taken: &mut Vec<PathBuf>,
) -> Result<(PathBuf, bool), String> {
    let Some(name) = w.from.file_name() else {
        return Err("has no name: linked where it is".to_string());
    };
    let name = Path::new(name);
    let stem = name
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = name.extension().map(|e| e.to_string_lossy().into_owned());
    if let Some(blocker) = in_the_way(&w.dir) {
        return Err(format!(
            "{blocker} is a file where a folder must be: linked where it is"
        ));
    }
    for n in 1..=NAMES_MAX {
        let file = match (n, &ext) {
            (1, _) => name.to_path_buf(),
            (_, Some(e)) => PathBuf::from(format!("{stem}-{n}.{e}")),
            (_, None) => PathBuf::from(format!("{stem}-{n}")),
        };
        let to = w.dir.join(file);
        if taken.iter().any(|t| same_path(t, &to)) {
            continue;
        }
        if chars(&to) > PATH_MAX_CHARS {
            return Err(format!(
                "its copy's path would be {} characters: linked where it is",
                chars(&to)
            ));
        }
        if exists(&to) {
            if holds(&to, bytes) {
                taken.push(to.clone());
                return Ok((to, true));
            }
            continue;
        }
        taken.push(to.clone());
        return Ok((to, false));
    }
    Err(format!(
        "every name up to -{NAMES_MAX} is taken: linked where it is"
    ))
}

/// The pictures `text` (a file in `from_dir`) shows from its own folder or below, each to the
/// same place under `to_dir`. One that cannot go there (a name taken by other bytes, another
/// picture of the run already going there) is not carried: the file's link to it is rewritten
/// to lead where it is.
fn pictures_of(
    text: &str,
    from_dir: &Path,
    to_dir: &Path,
    inside: &[PathBuf],
    out: &[Planned],
    me: usize,
    taken: &mut Vec<PathBuf>,
) -> Vec<PictureCopy> {
    let mut found: Vec<PictureCopy> = Vec::new();
    for link in links::links(text, from_dir) {
        let t = &link.target;
        if found.iter().any(|c| same_path(&c.from, t))
            || !links::is_picture(t)
            || !plain(t)
            || inside.iter().any(|d| below(d, t).is_some())
        {
            continue;
        }
        let Some(rel) = below(from_dir, t) else {
            continue;
        };
        let to = to_dir.join(&rel);
        // The same picture to the same place for another file of the run: once.
        let earlier = out
            .iter()
            .enumerate()
            .filter(|(k, _)| *k != me)
            .flat_map(|(_, p)| p.pictures.iter())
            .find(|c| same_path(&c.to, &to));
        if let Some(e) = earlier {
            if links::same_file(&e.from, t) {
                found.push(PictureCopy {
                    from: t.clone(),
                    to,
                    there: true,
                });
            }
            continue;
        }
        let mine = found.iter().any(|c| same_path(&c.to, &to));
        let other_file = out.iter().any(|p| same_path(p.target(), &to));
        if mine || other_file || chars(&to) > PATH_MAX_CHARS {
            continue;
        }
        let there = if exists(&to) {
            match read_regular(t, DIGEST_MAX_BYTES, false) {
                Some(bytes) if holds(&to, &bytes) => true,
                _ => continue,
            }
        } else {
            if taken.iter().any(|p| same_path(p, &to)) {
                continue;
            }
            false
        };
        taken.push(to.clone());
        found.push(PictureCopy {
            from: t.clone(),
            to,
            there,
        });
    }
    found
}

/// Makes the folders `file` needs, recording those that were not there, parents first: in
/// `folders` when `reach` allows them, else in `made_outside`.
fn make_dirs(file: &Path, reach: &dyn Fn(&Path) -> bool, done: &mut Written) -> Result<(), String> {
    let Some(dir) = file.parent() else {
        return Ok(());
    };
    let mut missing: Vec<PathBuf> = dir
        .ancestors()
        .take_while(|a| !a.as_os_str().is_empty() && !exists(a))
        .map(Path::to_path_buf)
        .collect();
    missing.reverse();
    for m in missing {
        std::fs::create_dir(&m).map_err(|e| format!("{} could not be made: {e}", m.display()))?;
        if reach(&m) {
            done.folders.push(m);
        } else {
            done.made_outside.push(m);
        }
    }
    Ok(())
}

/// Creates `to` holding `bytes`, flushed; refuses a name that is taken.
fn create_new(to: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    f.write_all(bytes)?;
    f.sync_all()
}

#[cfg(test)]
#[path = "moves_copy_tests.rs"]
mod tests;
