//! The library as a pane lists it: one row per group, section and note, in the order the files
//! are written, with what is folded left out; and the questions a pane asks of a library and a
//! path (is this file listed, which note is it, how is a link to it written). Pure: no drawing,
//! no app strings. Moved from mdedit's library pane, so that a second app's Notes face draws
//! the same tree.

use super::{Entry, Group, Library};
use crate::paths::{canonical_or_absolute, same_path};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

/// What a row stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A library file. Its key is the file.
    Group,
    /// A heading inside one.
    Section,
    /// A note: the file it opens, whether that file is gone, and its line in the library file.
    Note {
        path: PathBuf,
        missing: bool,
        line: usize,
    },
    /// The row a name is being typed in for something that is not there yet.
    Editing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    pub label: String,
    pub depth: u16,
    /// What folding remembers this row by; empty for a note.
    pub key: String,
    /// The headings a section is under, itself last; empty for the others.
    pub section: Vec<String>,
    /// Where a group's new notes go, as its `home:` line writes it; `None` when the line is
    /// not there, and for every row that is not a group.
    pub home: Option<String>,
}

/// The key a group or a section is remembered by: the library file, then the headings.
pub fn key_of(file: &Path, section: &[String]) -> String {
    let mut key = file.to_string_lossy().into_owned();
    for s in section {
        key.push('\u{1}');
        key.push_str(s);
    }
    key
}

/// The rows of `library`, in the order the files are written, leaving out what is under a key
/// in `folded` (the row with the key stays; what is inside it goes).
pub fn rows(library: &Library, folded: &HashSet<String>) -> Vec<Row> {
    let mut rows = Vec::new();
    for group in &library.groups {
        let key = key_of(&group.file, &[]);
        let shut = folded.contains(&key);
        rows.push(Row {
            kind: Kind::Group,
            label: group.name.clone(),
            depth: 0,
            key,
            section: Vec::new(),
            home: group.home.as_deref().map(|h| home_of(h, &library.dir)),
        });
        if shut {
            continue;
        }
        let note = |e: &Entry, depth: u16| Row {
            kind: Kind::Note {
                path: e.path.clone(),
                missing: e.missing,
                line: e.line,
            },
            label: e.title.clone(),
            depth,
            key: String::new(),
            section: Vec::new(),
            home: None,
        };
        // The notes written before the first heading belong to the group itself.
        rows.extend(
            group
                .entries
                .iter()
                .filter(|e| e.sections.is_empty())
                .map(|e| note(e, 1)),
        );
        for section in &group.sections {
            // A section inside a folded one is not listed at all.
            if (1..section.len()).any(|n| folded.contains(&key_of(&group.file, &section[..n]))) {
                continue;
            }
            let key = key_of(&group.file, section);
            let depth = section.len() as u16;
            rows.push(Row {
                kind: Kind::Section,
                label: section.last().cloned().unwrap_or_default(),
                depth,
                key: key.clone(),
                section: section.clone(),
                home: None,
            });
            if folded.contains(&key) {
                continue;
            }
            rows.extend(
                group
                    .entries
                    .iter()
                    .filter(|e| e.sections == *section)
                    .map(|e| note(e, depth + 1)),
            );
        }
    }
    rows
}

/// A path with its `.` and `..` folded away, by its text alone: a note linked as
/// `../notes/a.md` from the library folder and the tab that opened `notes/a.md` are one file.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Whether two paths are the same note: the same file once `.` and `..` are folded away, with
/// the platform's idea of case. Text only, so a row can ask it every frame.
pub fn same_note(a: &Path, b: &Path) -> bool {
    same_path(&lexical(a), &lexical(b))
}

/// The note entry of any group that names the file `file`, and its group. The spellings of the
/// path may differ (`./a.md`, `../library/a.md`, links and drive letters in another case), so
/// the text is folded first, and the disk is asked for the files that share a name with it
/// ([`canonical_or_absolute`]: links resolved).
pub fn listing<'a>(library: &'a Library, file: &Path) -> Option<(&'a Group, &'a Entry)> {
    let name = file.file_name()?;
    let real = std::cell::OnceCell::new();
    library.entries().find(|(_, e)| {
        same_note(&e.path, file)
            || (e.path.file_name().is_some_and(|n| {
                n.to_string_lossy().to_lowercase() == name.to_string_lossy().to_lowercase()
            }) && {
                let wanted = real.get_or_init(|| canonical_or_absolute(file));
                same_path(&canonical_or_absolute(&e.path), wanted)
            })
    })
}

/// The library as the app shows it: a library file that only names a note is not a group. A
/// note made in the library folder (by an older version, or by hand) is a `.md` file the reader
/// takes for a library file; when another library file lists it as a note and it lists no note
/// of its own, it is that note and nothing else, and is left out here. Headings in it are not
/// sections of a group (a note is written in with headings at once). A library file that lists
/// notes of its own is kept even when another lists it: a library may link one file from
/// another on purpose.
pub fn without_notes(full: &Library) -> Library {
    let mut groups = Vec::with_capacity(full.groups.len());
    for group in &full.groups {
        let hidden = group.entries.is_empty() && listing(full, &group.file).is_some();
        if !hidden {
            groups.push(group.clone());
        }
    }
    Library {
        dir: full.dir.clone(),
        omitted: full.omitted,
        groups,
    }
}

/// How a note is written into a library file: relative to the library folder `dir` when it is
/// inside it (so the library can be moved or synced as a whole), as it is otherwise.
pub fn link_path(dir: &Path, note: &Path) -> PathBuf {
    // The dots go first: `std::path::absolute` leaves `..` in place on Unix, where it may cross
    // a symlink, and a `..` stops the walk that resolves a folder which is not on disk yet — so
    // a path spelled with one would keep a different prefix from the library's own.
    let (base, real) = (
        canonical_or_absolute(&lexical(dir)),
        canonical_or_absolute(&lexical(note)),
    );
    match real.strip_prefix(&base) {
        Ok(inside) if !inside.as_os_str().is_empty() => inside.to_path_buf(),
        _ => note.to_path_buf(),
    }
}

/// Whether the library lists the file `path` as a note, in any group.
pub fn lists(library: &Library, path: &Path) -> bool {
    library.entries().any(|(_, e)| same_note(&e.path, path))
}

/// The name of the folder a note is in, as the recent files' menu shows it.
pub fn folder_of(path: &Path) -> Option<String> {
    let name = path.parent()?.file_name()?.to_string_lossy().into_owned();
    (!name.is_empty()).then_some(name)
}

/// Where a group's new notes go, as the group's `home:` line writes it: taken from the library
/// folder when it is inside it (the relative form the line is written in), whole when it is not.
pub fn home_of(home: &Path, dir: &Path) -> String {
    let shown = home
        .strip_prefix(dir)
        .ok()
        .filter(|rest| !rest.as_os_str().is_empty())
        .unwrap_or(home);
    shown.to_string_lossy().replace('\\', "/")
}

/// What a pane says when the library has no rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Empty {
    /// The folder is not there (nothing was ever made, or the setting points nowhere).
    NoFolder,
    /// The folder is there and holds no library file.
    NoNotes,
}

pub fn empty_state(dir: &Path) -> Empty {
    if dir.as_os_str().is_empty() || !dir.is_dir() {
        Empty::NoFolder
    } else {
        Empty::NoNotes
    }
}

/// The foot a pane gives a folder the reader could not read whole: how many groups it shows of
/// how many there are, as "512 of 583 groups shown". `None` when every library file was read.
/// `shown` is what the pane draws, `read` the groups the reader kept (a library file that only
/// names a note is not drawn), and `omitted` the files past the reader's limit.
pub fn omitted_label(shown: usize, read: usize, omitted: usize) -> Option<String> {
    (omitted > 0).then(|| format!("{shown} of {} groups shown", read + omitted))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, sections: &[&str]) -> Entry {
        Entry {
            title: title.to_string(),
            path: PathBuf::from(format!("D:/{title}.md")),
            sections: sections.iter().map(|s| (*s).to_string()).collect(),
            line: 1,
            missing: false,
        }
    }

    fn library() -> Library {
        let group = Group {
            name: "Personal".to_string(),
            home: None,
            file: PathBuf::from("D:/lib/personal.md"),
            stamp: crate::files::FileStamp::Missing,
            sections: vec![
                vec!["Money".to_string()],
                vec!["Money".to_string(), "2026".to_string()],
                vec!["Trips".to_string()],
            ],
            entries: vec![
                entry("Inbox", &[]),
                entry("Taxes", &["Money"]),
                entry("This year", &["Money", "2026"]),
                entry("Japan", &["Trips"]),
            ],
        };
        Library {
            dir: PathBuf::from("D:/lib"),
            groups: vec![group],
            omitted: 0,
        }
    }

    fn shown(rows: &[Row]) -> Vec<(u16, &str)> {
        rows.iter().map(|r| (r.depth, r.label.as_str())).collect()
    }

    #[test]
    fn the_rows_are_a_tree_of_groups_sections_and_notes_less_what_is_folded() {
        let lib = library();
        let all = rows(&lib, &HashSet::new());
        assert_eq!(
            shown(&all),
            vec![
                (0, "Personal"),
                (1, "Inbox"),
                (1, "Money"),
                (2, "Taxes"),
                (2, "2026"),
                (3, "This year"),
                (1, "Trips"),
                (2, "Japan"),
            ]
        );
        assert_eq!(all[0].kind, Kind::Group);
        assert_eq!(all[2].kind, Kind::Section);
        assert_eq!(
            all[4].section,
            vec!["Money".to_string(), "2026".to_string()]
        );
        assert!(matches!(
            &all[3].kind,
            Kind::Note {
                line: 1,
                missing: false,
                ..
            }
        ));
        assert!(all[3].key.is_empty(), "a note is not folded");
        // A folded section keeps its row and drops what is under it, sections and notes.
        let money: HashSet<String> = [key_of(&lib.groups[0].file, &["Money".to_string()])].into();
        assert_eq!(
            shown(&rows(&lib, &money)),
            vec![
                (0, "Personal"),
                (1, "Inbox"),
                (1, "Money"),
                (1, "Trips"),
                (2, "Japan"),
            ]
        );
        // A folded group is one row.
        let group: HashSet<String> = [key_of(&lib.groups[0].file, &[])].into();
        assert_eq!(shown(&rows(&lib, &group)), vec![(0, "Personal")]);
    }

    #[test]
    fn a_group_row_names_its_home_from_the_library_folder() {
        let mut lib = library();
        lib.groups[0].home = Some(PathBuf::from("D:/lib/work/notes"));
        assert_eq!(
            rows(&lib, &HashSet::new())[0].home.as_deref(),
            Some("work/notes")
        );
        lib.groups[0].home = Some(PathBuf::from("E:/elsewhere"));
        assert_eq!(
            rows(&lib, &HashSet::new())[0].home.as_deref(),
            Some("E:/elsewhere")
        );
    }

    #[test]
    fn a_folder_is_the_last_part_of_the_path_before_the_file() {
        assert_eq!(
            folder_of(Path::new("D:/notes/taxes/taxes.md")).as_deref(),
            Some("taxes")
        );
        assert_eq!(folder_of(Path::new("taxes.md")), None);
        assert_eq!(folder_of(Path::new("/taxes.md")), None);
        // A note under a group's home and its section folders shows the folder it is in.
        assert_eq!(
            folder_of(Path::new("D:/library/Japan 2025/Tokyo/Food/ramen.md")).as_deref(),
            Some("Food")
        );
    }

    #[test]
    fn a_library_with_no_folder_or_an_empty_one_is_told_apart() {
        assert_eq!(empty_state(Path::new("")), Empty::NoFolder);
        assert_eq!(
            empty_state(&std::env::temp_dir().join("mdedit-no-such-library-folder")),
            Empty::NoFolder
        );
        assert_eq!(empty_state(&std::env::temp_dir()), Empty::NoNotes);
    }

    #[test]
    fn a_link_is_relative_inside_the_library_folder_and_as_it_is_outside_it() {
        let lib = std::env::temp_dir()
            .join("mdedit-link-path")
            .join("library");
        let inside = lib.join("Home").join("Budget.md");
        assert_eq!(
            link_path(&lib, &inside),
            PathBuf::from("Home").join("Budget.md")
        );
        assert_eq!(link_path(&lib, &lib.join("a.md")), PathBuf::from("a.md"));
        // Spelled with a `..` that comes back in.
        let climbing = lib.join("..").join("library").join("a.md");
        assert_eq!(link_path(&lib, &climbing), PathBuf::from("a.md"));
        // Outside, beside it or above it, the path stays whole.
        let beside = lib.with_file_name("library2").join("a.md");
        assert_eq!(link_path(&lib, &beside), beside);
        let above = lib.parent().unwrap().join("a.md");
        assert_eq!(link_path(&lib, &above), above);
        // The folder itself is not a note inside it.
        assert_eq!(link_path(&lib, &lib), lib);
    }

    fn group_of(file: &str, notes: &[(&str, &str)]) -> Group {
        let mut g = library().groups[0].clone();
        g.file = PathBuf::from(file);
        g.sections.clear();
        g.entries = notes
            .iter()
            .enumerate()
            .map(|(line, (title, path))| {
                let mut e = entry(title, &[]);
                e.path = PathBuf::from(path);
                e.line = line + 3;
                e
            })
            .collect();
        g
    }

    #[test]
    fn a_library_file_that_lists_no_note_and_is_listed_by_another_is_not_shown() {
        let full = Library {
            dir: PathBuf::from("/lib"),
            omitted: 0,
            groups: vec![
                group_of(
                    "/lib/a.md",
                    &[("Budget", "/lib/b.md"), ("Plan", "/lib/c.md")],
                ),
                group_of("/lib/b.md", &[]),
                group_of("/lib/c.md", &[("Own", "/other/own.md")]),
                group_of("/lib/d.md", &[]),
            ],
        };
        let shown: Vec<_> = without_notes(&full)
            .groups
            .iter()
            .map(|g| g.file.to_string_lossy().into_owned())
            .collect();
        // b is a note and nothing else; c is listed too but has notes of its own; d is listed
        // by nobody.
        assert_eq!(shown, vec!["/lib/a.md", "/lib/c.md", "/lib/d.md"]);
        assert_eq!(without_notes(&full).dir, full.dir);
        // A file that lists only itself is not hidden by that.
        let own = Library {
            dir: PathBuf::from("/lib"),
            omitted: 0,
            groups: vec![group_of("/lib/a.md", &[("Me", "/lib/a.md")])],
        };
        assert_eq!(without_notes(&own).groups.len(), 1);
        let empty_alone = Library {
            dir: PathBuf::from("/lib"),
            omitted: 0,
            groups: vec![group_of("/lib/z.md", &[])],
        };
        assert_eq!(without_notes(&empty_alone).groups.len(), 1);
    }

    #[test]
    fn a_note_is_found_in_the_library_by_any_spelling_of_its_path() {
        let full = Library {
            dir: PathBuf::from("/lib"),
            omitted: 0,
            groups: vec![group_of(
                "/lib/a.md",
                &[("One", "/lib/../notes/one.md"), ("Two", "/lib/./two.md")],
            )],
        };
        let found = |p: &str| listing(&full, Path::new(p)).map(|(_, e)| e.title.clone());
        assert_eq!(found("/notes/one.md").as_deref(), Some("One"));
        assert_eq!(found("/lib/two.md").as_deref(), Some("Two"));
        assert_eq!(found("/lib/three.md"), None);
        assert_eq!(found("/notes/two.md"), None);
        assert!(lists(&full, Path::new("/notes/one.md")));
        assert!(!lists(&full, Path::new("/lib/three.md")));
    }

    #[test]
    fn a_capped_library_keeps_the_count_of_the_groups_it_left_out() {
        let full = Library {
            dir: PathBuf::from("/lib"),
            omitted: 71,
            groups: vec![
                group_of("/lib/a.md", &[("Two", "/lib/b.md")]),
                group_of("/lib/b.md", &[]),
            ],
        };
        let shown = without_notes(&full);
        assert_eq!(shown.groups.len(), 1, "the note is still not a group");
        assert_eq!(
            shown.omitted, 71,
            "the count is copied, not silently dropped with the hidden group"
        );
    }

    #[test]
    fn the_omitted_count_says_how_many_groups_the_pane_shows() {
        assert_eq!(
            omitted_label(512, 512, 71).as_deref(),
            Some("512 of 583 groups shown")
        );
        // A library file that only names a note is read but not drawn: the total still counts it.
        assert_eq!(
            omitted_label(511, 512, 71).as_deref(),
            Some("511 of 583 groups shown")
        );
        assert_eq!(omitted_label(3, 3, 0), None, "nothing left out, no row");
    }
}
