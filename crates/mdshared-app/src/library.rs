//! The notes library: a `library` folder beside `settings.ini` whose `.md` files each hold one
//! group of notes. The notes themselves stay where they are and nothing is written to them; a
//! library file is a list of links, read again whenever it is saved, as the editor's saved
//! prompts file is.
//!
//! One file, one group. Its first level-1 heading names the group (its file name otherwise), a
//! level-2 heading or deeper starts a section, and every list item under them is a note:
//!
//! ```text
//! # Personal
//!
//! ## Money
//!
//! - [Taxes 2026](D:/misc/taxes.md)
//! - D:/misc/receipts.md
//! ```
//!
//! A line with no link and no path is left out, as is anything inside a code fence, so prose
//! around the lists is free. A link's text is the title; a line that is only a path takes its
//! title from the note's first heading, and its file name where there is none (such a line
//! needs `<` and `>` around a path with spaces in it, or it reads as prose). A relative path is
//! taken from the library folder. A note that is not on disk is listed as missing rather than
//! dropped, so a moved file is visible instead of silently gone.
//!
//! [`read`] does the whole of it: the folder, each file, the titles and what is missing. It
//! touches the notes only for the titles it has to find, and never more than [`TITLE_BYTES`] of
//! one, so it stays off the launch path by being called when the library is first shown.

use crate::files::{self, FileStamp};
use std::path::{Path, PathBuf};

/// The folder of library files, beside `settings.ini`.
pub const DIR: &str = "library";

/// The file "add this note to the library" writes to when the folder has none.
pub const DEFAULT_FILE: &str = "notes.md";

/// A library file is read at most this far; a larger one is listed empty.
pub const MAX_FILE_BYTES: usize = 1024 * 1024;

/// Library files read at most, in name order.
pub const MAX_FILES: usize = 64;

/// Notes kept per group.
pub const MAX_ENTRIES: usize = 2_000;

/// Bytes of a note read to find the heading its title comes from.
pub const TITLE_BYTES: usize = 8 * 1024;

/// What the first library file is written with.
pub const TEMPLATE: &str = "# Notes\n\
\n\
Each list item below is a note: a link, or a path on its own. A heading of level 2 is a\n\
section. Save this file and the library is read again. Lines that are not notes, like these,\n\
are left out.\n";

/// One note in the library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// What the list shows: the link's text, the note's first heading, or its file name.
    pub title: String,
    /// The note. Absolute, unless the library file gave a path with no root and the library
    /// folder is not known.
    pub path: PathBuf,
    /// The section heading it is under; empty for the notes before the first one.
    pub section: String,
    /// Its line in the library file, counted from 1, so a command can lead to it.
    pub line: usize,
    /// There is no file at `path`. Set by [`read`]; [`parse`] leaves it false.
    pub missing: bool,
}

/// One library file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// The file's first level-1 heading, or its file name without the extension.
    pub name: String,
    pub file: PathBuf,
    /// What the file looked like when it was read, for "has it been saved since".
    pub stamp: FileStamp,
    /// Its headings below the first level, in the order they are written, each once, including
    /// the ones with no note under them yet: where a note can be put.
    pub sections: Vec<String>,
    pub entries: Vec<Entry>,
}

/// Every group, in the order the folder gives (by file name).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Library {
    pub dir: PathBuf,
    pub groups: Vec<Group>,
}

impl Library {
    /// The notes of every group, each with the group it is in.
    pub fn entries(&self) -> impl Iterator<Item = (&Group, &Entry)> {
        self.groups
            .iter()
            .flat_map(|g| g.entries.iter().map(move |e| (g, e)))
    }

    /// How many notes it holds.
    pub fn len(&self) -> usize {
        self.groups.iter().map(|g| g.entries.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether any library file has been saved, added or removed since it was read.
    pub fn changed(&self) -> bool {
        let files = library_files(&self.dir);
        files.len() != self.groups.len()
            || files
                .iter()
                .zip(&self.groups)
                .any(|(f, g)| *f != g.file || files::stamp(f) != g.stamp)
    }
}

/// The `.md` files of a library folder, in name order, at most [`MAX_FILES`].
fn library_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
                && p.is_file()
        })
        .collect();
    files.sort();
    files.truncate(MAX_FILES);
    files
}

/// Reads the whole library under `dir` (the `library` folder itself, not the settings folder):
/// every file, the title of every note that has none written for it, and which notes are not on
/// disk. A folder that is not there is an empty library, not an error.
pub fn read(dir: &Path) -> Library {
    let mut groups = Vec::new();
    for file in library_files(dir) {
        let stamp = files::stamp(&file);
        let text = files::read_stable(&file, MAX_FILE_BYTES)
            .map(|l| files::decode(&l.bytes).0.into_owned())
            .unwrap_or_default();
        let Parsed {
            name,
            sections,
            mut entries,
        } = parse(&text, dir);
        let name = name.unwrap_or_else(|| {
            file.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string()
        });
        for entry in &mut entries {
            entry.missing = !entry.path.exists();
            if entry.title.is_empty() {
                entry.title = title_of(&entry.path);
            }
        }
        groups.push(Group {
            name,
            file,
            stamp,
            sections,
            entries,
        });
    }
    Library {
        dir: dir.to_path_buf(),
        groups,
    }
}

/// The title a note with none written for it goes by: its first heading, else its file name.
pub fn title_of(path: &Path) -> String {
    first_heading(path).unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string()
    })
}

/// The first `#` heading of a note, reading no more than [`TITLE_BYTES`] of it.
fn first_heading(path: &Path) -> Option<String> {
    let loaded = files::read_stable(path, TITLE_BYTES).ok().or_else(|| {
        // Too large to read whole: its start is enough for a heading.
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .ok()?
            .take(TITLE_BYTES as u64)
            .read_to_end(&mut bytes)
            .ok()?;
        Some(files::Loaded {
            bytes,
            stamp: files::stamp(path),
        })
    })?;
    let text = files::decode(&loaded.bytes).0.into_owned();
    let mut fence: Option<String> = None;
    for line in text.lines() {
        if let Some(marker) = fence_marker(line) {
            fence = match &fence {
                Some(open) if closes(&marker, open) => None,
                None => Some(marker),
                _ => fence,
            };
            continue;
        }
        if fence.is_none()
            && let Some(rest) = line.trim_start().strip_prefix('#')
        {
            let title = rest
                .trim_start_matches('#')
                .trim()
                .trim_end_matches('#')
                .trim();
            if !title.is_empty() {
                return Some(title.to_string());
            }
        }
    }
    None
}

/// The fence a line opens or closes, if it is one.
fn fence_marker(line: &str) -> Option<String> {
    let t = line.trim_start();
    let first = t.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let marker: String = t.chars().take_while(|&c| c == first).collect();
    (marker.len() >= 3).then_some(marker)
}

/// Whether `marker` closes the fence `open` opened: the same character, and no shorter.
fn closes(marker: &str, open: &str) -> bool {
    marker.starts_with(open) && marker.len() >= open.len()
}

/// What one library file holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    /// The file's first level-1 heading, if it has one.
    pub name: Option<String>,
    /// Its other headings, in order, each once.
    pub sections: Vec<String>,
    pub entries: Vec<Entry>,
}

/// Reads one library file: its group name (its first level-1 heading), its headings and its
/// notes, in the order they are written. `base` is the folder a path with no root is taken
/// from. Nothing is read from disk here, so [`Entry::missing`] is false and a note with no
/// title written keeps an empty one; [`read`] fills both in.
pub fn parse(text: &str, base: &Path) -> Parsed {
    let mut name: Option<String> = None;
    let mut sections: Vec<String> = Vec::new();
    let mut section = String::new();
    let mut entries: Vec<Entry> = Vec::new();
    let mut fence: Option<String> = None;
    for (n, line) in text.lines().enumerate() {
        if let Some(marker) = fence_marker(line) {
            fence = match &fence {
                Some(open) if closes(&marker, open) => None,
                None => Some(marker),
                _ => fence,
            };
            continue;
        }
        if fence.is_some() {
            continue;
        }
        if let Some(heading) = heading(line) {
            match heading {
                (1, text) if name.is_none() && !text.is_empty() => name = Some(text.to_string()),
                (1, _) => {}
                (_, text) => {
                    section = text.to_string();
                    if !section.is_empty() && !sections.contains(&section) {
                        sections.push(section.clone());
                    }
                }
            }
            continue;
        }
        if entries.len() >= MAX_ENTRIES {
            continue;
        }
        if let Some((title, path)) = note_line(line) {
            let path = resolve(&path, base);
            entries.push(Entry {
                title,
                path,
                section: section.clone(),
                line: n + 1,
                missing: false,
            });
        }
    }
    Parsed {
        name,
        sections,
        entries,
    }
}

/// A heading line as its level and its text.
fn heading(line: &str) -> Option<(usize, &str)> {
    let rest = line.strip_prefix('#')?;
    let more = rest.len() - rest.trim_start_matches('#').len();
    let level = 1 + more;
    let text = rest[more..].trim().trim_end_matches('#').trim();
    (level == 1 || rest[more..].starts_with(' ') || text.is_empty()).then_some((level, text))
}

/// A note line as its title (empty when it is only a path) and the path written on it: a list
/// item or a line of its own, holding `[Title](path)` or a path. Anything else is not a note.
fn note_line(line: &str) -> Option<(String, String)> {
    let t = line.trim();
    let t = t
        .strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))
        .or_else(|| t.strip_prefix("+ "))
        .unwrap_or(t)
        .trim();
    if t.is_empty() {
        return None;
    }
    if let Some(rest) = t.strip_prefix('[')
        && let Some((title, after)) = rest.split_once("](")
        && let Some(path) = after.strip_suffix(')')
    {
        let (path, _) = unwrap_path(path);
        return (!path.is_empty()).then(|| (title.trim().to_string(), path));
    }
    let (path, bracketed) = unwrap_path(t);
    looks_like_path(&path, bracketed).then_some((String::new(), path))
}

/// A path as the file holds it, and whether it was written in `<>`: the brackets are taken off,
/// and `%20` reads as a space, as a Markdown link's target does.
fn unwrap_path(text: &str) -> (String, bool) {
    let t = text.trim();
    match t.strip_prefix('<').and_then(|r| r.strip_suffix('>')) {
        Some(inner) => (inner.trim().replace("%20", " "), true),
        None => (t.replace("%20", " "), false),
    }
}

/// Whether a line with no link on it names a file, so that a sentence in a list is not taken
/// for one: it holds no character a path cannot, it has a folder separator or an extension of a
/// few letters, and it has no space unless it was written in `<>`.
fn looks_like_path(text: &str, bracketed: bool) -> bool {
    let extension = Path::new(text)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            (1..=8).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric())
        });
    !text.is_empty()
        && !text.contains(['*', '?', '"', '|', '\t'])
        && (bracketed || !text.contains(' '))
        && (text.contains('/') || text.contains('\\') || extension)
}

/// A path with no root, taken from the library folder.
fn resolve(path: &str, base: &Path) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_absolute() || base.as_os_str().is_empty() {
        p
    } else {
        base.join(p)
    }
}

/// The line a note is written on: a link, with the path in `<>` when it holds a space.
pub fn link_line(title: &str, path: &Path) -> String {
    let written = path.to_string_lossy().replace('\\', "/");
    let target = if written.contains(' ') {
        format!("<{written}>")
    } else {
        written
    };
    format!("- [{}]({})", title.trim().replace(['[', ']'], ""), target)
}

/// Writes a note into a library file's text under `section` (the end of the file when it is
/// `None` or not there yet) and gives the new text. The file's line ending is kept.
pub fn add_note(text: &str, section: Option<&str>, title: &str, path: &Path) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let line = link_line(title, path);
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let at = section.and_then(|want| {
        let start = lines.iter().position(|l| {
            heading(l).is_some_and(|(level, t)| level > 1 && t.eq_ignore_ascii_case(want.trim()))
        })?;
        // After the section's last note, or after its heading when it has none yet.
        let end = lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, l)| heading(l).is_some())
            .map_or(lines.len(), |(i, _)| i);
        let last = (start + 1..end)
            .rev()
            .find(|&i| note_line(&lines[i]).is_some());
        Some(last.map_or(start + 1, |i| i + 1))
    });
    match at {
        Some(i) => lines.insert(i, line),
        None => {
            if let Some(want) = section.map(str::trim).filter(|s| !s.is_empty()) {
                while lines.last().is_some_and(|l| l.trim().is_empty()) {
                    lines.pop();
                }
                lines.push(String::new());
                lines.push(format!("## {want}"));
            }
            if lines.last().is_some_and(|l| !l.trim().is_empty())
                && lines.last().is_some_and(|l| note_line(l).is_none())
            {
                lines.push(String::new());
            }
            lines.push(line);
        }
    }
    let mut out = lines.join(eol);
    out.push_str(eol);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mdshared-library-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_library_file_reads_as_a_group_of_notes_and_leaves_out_what_is_not_one() {
        let base = Path::new("/lib");
        let text = "# Personal\n\
            \n\
            Anything may be written here.\n\
            \n\
            ## Money\n\
            \n\
            - [Taxes 2026](D:/misc/taxes.md)\n\
            - D:/misc/receipts.md\n\
            - just a sentence about money\n\
            \n\
            ## Trips\n\
            \n\
            * [Japan](<D:/my notes/japan.md>)\n\
            + nearby.md\n\
            \n\
            ```\n\
            - [Not a note](D:/no.md)\n\
            ```\n";
        let Parsed {
            name,
            sections,
            entries,
        } = parse(text, base);
        assert_eq!(name.as_deref(), Some("Personal"));
        let seen: Vec<(&str, String, &str)> = entries
            .iter()
            .map(|e| {
                (
                    e.title.as_str(),
                    e.path.to_string_lossy().replace('\\', "/"),
                    e.section.as_str(),
                )
            })
            .collect();
        assert_eq!(
            seen,
            vec![
                ("Taxes 2026", "D:/misc/taxes.md".to_string(), "Money"),
                ("", "D:/misc/receipts.md".to_string(), "Money"),
                ("Japan", "D:/my notes/japan.md".to_string(), "Trips"),
                ("", "/lib/nearby.md".to_string(), "Trips"),
            ]
        );
        assert_eq!(entries[0].line, 7);
        assert!(entries.iter().all(|e| !e.missing));
        assert_eq!(sections, vec!["Money".to_string(), "Trips".to_string()]);
        // A heading with no note under it yet is still a place a note can go.
        let empty = parse("# Notes\n\n## Later\n", base);
        assert_eq!(empty.sections, vec!["Later".to_string()]);
        assert!(empty.entries.is_empty());
    }

    #[test]
    fn the_library_folder_reads_as_groups_with_titles_and_what_is_missing() {
        let dir = scratch("read");
        let lib = dir.join("library");
        std::fs::create_dir_all(&lib).unwrap();
        let note = dir.join("kept.md");
        std::fs::write(&note, "\n# The kept note\n\ntext\n").unwrap();
        std::fs::write(dir.join("plain.md"), "no heading here\n").unwrap();
        std::fs::write(
            lib.join("a-work.md"),
            format!(
                "# Work\n\n## Now\n\n- {}\n- ../plain.md\n- {}\n",
                note.to_string_lossy().replace('\\', "/"),
                dir.join("gone.md").to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();
        std::fs::write(lib.join("b-rest.md"), "- nothing here\n").unwrap();
        std::fs::write(lib.join("notes.txt"), "- not read\n").unwrap();

        let library = read(&lib);
        assert_eq!(library.groups.len(), 2, "only the .md files");
        assert_eq!(library.groups[0].name, "Work");
        assert_eq!(
            library.groups[1].name, "b-rest",
            "no heading: the file name"
        );
        assert!(library.groups[1].entries.is_empty());
        let work = &library.groups[0].entries;
        assert_eq!(work.len(), 3);
        assert_eq!(work[0].title, "The kept note", "from its first heading");
        assert!(!work[0].missing);
        assert_eq!(work[1].title, "plain.md", "no heading: the file name");
        assert!(!work[1].missing, "a relative path is the library folder's");
        assert_eq!(work[2].title, "gone.md");
        assert!(work[2].missing);
        assert_eq!(library.len(), 3);
        assert!(!library.changed());

        // Saved again: the library knows it must be read again.
        std::fs::write(lib.join("a-work.md"), "# Work\n").unwrap();
        assert!(library.changed());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_note_is_added_under_its_section_and_reads_back() {
        let base = Path::new("/lib");
        let text = "# Personal\n\n## Money\n\n- [Taxes](D:/misc/taxes.md)\n\n## Trips\n\n- [Japan](D:/japan.md)\n";
        let out = add_note(
            text,
            Some("Money"),
            "Receipts",
            Path::new("D:/misc/receipts.md"),
        );
        let entries = parse(&out, base).entries;
        let titles: Vec<&str> = entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, vec!["Taxes", "Receipts", "Japan"]);
        assert_eq!(entries[1].section, "Money");

        // A section that is not there yet is made at the end; a file with none takes it plainly.
        let out = add_note(&out, Some("Reading"), "A book", Path::new("D:/book.md"));
        let entries = parse(&out, base).entries;
        assert_eq!(entries.last().unwrap().section, "Reading");
        let out = add_note("# Notes\n\nprose\n", None, "One", Path::new("D:/one.md"));
        assert_eq!(out, "# Notes\n\nprose\n\n- [One](D:/one.md)\n");
        // A path with a space is written so that it reads back whole.
        let out = add_note(&out, None, "Two", Path::new("D:/my notes/two.md"));
        let entries = parse(&out, base).entries;
        assert_eq!(
            entries
                .last()
                .unwrap()
                .path
                .to_string_lossy()
                .replace('\\', "/"),
            "D:/my notes/two.md"
        );
        // CRLF is kept.
        let out = add_note(
            "# Notes\r\n\r\n- [One](D:/one.md)\r\n",
            None,
            "Two",
            Path::new("D:/two.md"),
        );
        assert!(out.ends_with("- [Two](D:/two.md)\r\n"), "{out:?}");
        assert!(!out.contains("\n\n"), "no bare line feed: {out:?}");
    }
}
