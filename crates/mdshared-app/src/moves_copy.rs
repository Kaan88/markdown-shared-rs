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
    // Then each Markdown file's pictures and links. A file whose text changes is no longer the
    // bytes a same-named file there holds, so it gets a name of its own; that can change where
    // another file's link must lead, so this is done again until nothing moves (a few times at
    // most: a file goes from There to New once).
    for _ in 0..4 {
        let mut moved = false;
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
            let pictures = pictures_of(&text, from_dir, &w.dir, inside, &out, i, &mut taken);
            let places: Vec<(PathBuf, PathBuf)> = out
                .iter()
                .enumerate()
                .filter(|(k, _)| *k != i)
                .map(|(_, p)| (p.from.clone(), p.target().to_path_buf()))
                .collect();
            let place = |p: &Path| -> PathBuf {
                if let Some(c) = pictures.iter().find(|c| same_path(&c.from, p)) {
                    return c.to.clone();
                }
                match places.iter().find(|(f, _)| links::same_file(f, p)) {
                    Some((_, t)) => t.clone(),
                    None => p.to_path_buf(),
                }
            };
            let (new, n) = links::rewrite(&text, from_dir, &w.dir, &place, &[]);
            out[i].pictures = pictures;
            out[i].rewrites = n;
            let text = (n > 0).then_some(new);
            out[i].copy = match (&out[i].copy, text) {
                (Copy::There { to }, Some(t)) if !holds(to, t.as_bytes()) => {
                    moved = true;
                    match free_name(w, t.as_bytes(), &mut taken) {
                        Ok((to, there)) if there => Copy::There { to },
                        Ok((to, _)) => Copy::New { to, text: Some(t) },
                        Err(why) => Copy::Stays { why },
                    }
                }
                (Copy::There { to }, _) => Copy::There { to: to.clone() },
                (Copy::New { to, .. }, t) => Copy::New {
                    to: to.clone(),
                    text: t,
                },
                (s, _) => s.clone(),
            };
        }
        if !moved {
            break;
        }
    }
    out
}

/// What is written, for the record.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Written {
    /// Each file's copy: its path, whether this run made it, and the digest of its bytes.
    pub files: Vec<(PathBuf, bool, String)>,
    pub pictures: Vec<PictureRec>,
    /// The folders this run made, parents first.
    pub folders: Vec<PathBuf>,
}

/// Writes the copies `plan` gives. A file is read again here, and one that changed since its
/// name was chosen is still copied as it is now. On the first failure everything this call made
/// is taken away again (only while it is the bytes it wrote) and the reason is given.
pub fn write(plan: &[Planned]) -> Result<Written, String> {
    let mut done = Written::default();
    let result = write_all(plan, &mut done);
    if let Err(why) = result {
        take_back(&done);
        return Err(why);
    }
    Ok(done)
}

fn write_all(plan: &[Planned], done: &mut Written) -> Result<(), String> {
    for p in plan {
        match &p.copy {
            Copy::Stays { .. } => continue,
            Copy::There { to } => {
                let sha = digest_of(to).unwrap_or_default();
                done.files.push((to.clone(), false, sha));
            }
            Copy::New { to, text } => {
                let bytes = match text {
                    Some(t) => t.as_bytes().to_vec(),
                    None => read_regular(&p.from, DIGEST_MAX_BYTES, false)
                        .ok_or_else(|| format!("{} could not be read", p.from.display()))?,
                };
                make_dirs(to, done)?;
                create_new(to, &bytes)
                    .map_err(|e| format!("{} could not be written: {e}", to.display()))?;
                done.files.push((to.clone(), true, sha_of(&bytes)));
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
            make_dirs(&c.to, done)?;
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
    for (path, created, sha) in &done.files {
        if *created && digest_of(path).is_some_and(|d| d.eq_ignore_ascii_case(sha)) {
            let _ = std::fs::remove_file(path);
        }
    }
    for p in &done.pictures {
        if p.created && digest_of(&p.path).is_some_and(|d| d.eq_ignore_ascii_case(&p.sha256)) {
            let _ = std::fs::remove_file(&p.path);
        }
    }
    for folder in done.folders.iter().rev() {
        let _ = std::fs::remove_dir(folder);
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

/// Makes the folders `file` needs, recording those that were not there, parents first.
fn make_dirs(file: &Path, done: &mut Written) -> Result<(), String> {
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
        done.folders.push(m);
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
