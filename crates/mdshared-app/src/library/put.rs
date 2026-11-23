//! Where the library is, and putting a note into it: what every app of the family that reads or
//! adds to the notes library needs and must do the same way. The editor owns the setting
//! (`library_dir` in its `settings.ini`); a viewer reads the editor's file to find the same
//! folder. Nothing here is called by an existing caller.
//!
//! - [`dir_for`]: the folder a settings file and its `library_dir` preference name.
//! - [`pref_in`], [`dir_from_settings`]: the preference read out of a settings file's text or
//!   of the file.
//! - [`put_note`]: one note written into a library file, checked to read back, written whole.

use super::{Library, TEMPLATE, add_note_in_group, parse, title_of, tree};
use crate::{files, paths, settings};
use std::path::{Path, PathBuf};

/// The most a `library_dir` value may hold, as the editor reads it; a longer one, or one with a
/// line break, is no folder.
const PREF_MAX: usize = 2048;

/// The section of a settings file the preference is in.
const SECTION: &str = "preferences";

/// The library folder for the settings file `settings_file` and the preference `pref`: empty is
/// `library` beside the settings file, a path with no root is taken from the settings file's
/// folder, a rooted one is used as it is. `None` when the settings file has no folder to be
/// beside, so no build reads or writes a library by accident.
pub fn dir_for(settings_file: &Path, pref: &str) -> Option<PathBuf> {
    let beside = settings_file
        .parent()
        .filter(|d| !d.as_os_str().is_empty())?;
    let set = pref.trim();
    if set.is_empty() {
        return Some(beside.join(super::DIR));
    }
    let set = Path::new(set);
    Some(if set.is_absolute() {
        set.to_path_buf()
    } else {
        beside.join(set)
    })
}

/// The `library_dir` preference in the text of a settings file: the `[preferences]` section's
/// key (text with no section header counts as that section), unescaped. Empty when it is not
/// there, is too long or has a line break in it.
pub fn pref_in(text: &str) -> String {
    let mut in_section = true;
    let mut found = String::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            in_section = &t[1..t.len() - 1] == SECTION;
            continue;
        }
        if !in_section {
            continue;
        }
        for (key, value) in settings::pairs(line) {
            if key == "library_dir" {
                let v = settings::unescape(value);
                found = if v.len() <= PREF_MAX && !v.contains(['\n', '\r']) {
                    v
                } else {
                    String::new()
                };
            }
        }
    }
    found
}

/// The library folder another app's settings file names: its preference ([`pref_in`]) put through
/// [`dir_for`]. A file that is not there names the default folder beside it; one that cannot be
/// read is `None` rather than a guess.
pub fn dir_from_settings(settings_file: &Path) -> Option<PathBuf> {
    let text = settings::read(settings_file).ok()?.unwrap_or_default();
    dir_for(settings_file, &pref_in(&text))
}

/// Why [`put_note`] wrote nothing.
#[derive(Debug, PartialEq, Eq)]
pub enum PutError {
    /// The library file's folder could not be made, or the file read or written.
    Io(String),
    /// The new text would not read back as the old notes plus this one.
    WouldNotReadBack,
    /// The library file is not UTF-8 text (another code page, or UTF-16): writing it back as
    /// UTF-8 would change every letter outside ASCII, so it is not written (audit 4, D3).
    NotUtf8,
    /// The library file changed on disk between its read and the write: another program wrote
    /// it, and its text is kept (audit 4, E6).
    Changed,
}

impl std::fmt::Display for PutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PutError::Io(why) => f.write_str(why),
            PutError::WouldNotReadBack => f.write_str("the file would not read back as its notes"),
            PutError::NotUtf8 => f.write_str(
                "the file is not UTF-8 text, so it is left as it is; save it as UTF-8 first",
            ),
            PutError::Changed => {
                f.write_str("the file changed while the note was being added; try again")
            }
        }
    }
}

/// The title the library lists the file `note` under, in any group.
pub fn listed_as(library: &Library, note: &Path) -> Option<String> {
    let same = paths::canonical_or_absolute(note);
    library
        .entries()
        .find(|(_, e)| paths::canonical_or_absolute(&e.path) == same)
        .map(|(_, e)| e.title.clone())
}

/// Writes `note` into the library file `file` under the heading path `section` (the group's own
/// notes when it is empty), making the file from [`TEMPLATE`] when it is not there. A note
/// inside the library folder `dir` is linked relative to it. The new text is written only when
/// it reads back as the notes there were plus this one at its place, and in one atomic write
/// ([`files::replace`]). A file that is not UTF-8 text is refused rather than written back
/// changed ([`PutError::NotUtf8`]); one that begins with a UTF-8 byte order mark keeps it; one
/// that changed on disk since it was read is not written over ([`PutError::Changed`]). Whether
/// the file is open with changes not saved is the caller's to ask first. Gives the title the
/// note was listed under.
pub fn put_note(
    file: &Path,
    section: &[String],
    note: &Path,
    dir: &Path,
) -> Result<String, PutError> {
    let io = |e: std::io::Error| PutError::Io(e.to_string());
    let title = title_of(note);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    let stamp = files::stamp(file);
    let (text, bom) = match std::fs::read(file) {
        Ok(bytes) => {
            let utf16 = bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]);
            let (text, invalid) = files::decode(&bytes);
            if utf16 || invalid {
                return Err(PutError::NotUtf8);
            }
            (text.into_owned(), bytes.starts_with(&[0xEF, 0xBB, 0xBF]))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (TEMPLATE.to_string(), false),
        Err(e) => return Err(io(e)),
    };
    let base = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let new = add_note_in_group(&text, section, &title, &tree::link_path(&base, note));
    let before = parse(&text, dir).entries;
    let after = parse(&new, dir).entries;
    let same = paths::canonical_or_absolute(note);
    let added = after.len() == before.len() + 1
        && after
            .iter()
            .any(|e| paths::canonical_or_absolute(&e.path) == same);
    if !added {
        return Err(PutError::WouldNotReadBack);
    }
    if files::stamp(file) != stamp {
        return Err(PutError::Changed);
    }
    let mut bytes = Vec::with_capacity(new.len() + 3);
    if bom {
        bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    bytes.extend_from_slice(new.as_bytes());
    files::replace(file, &bytes).map_err(io)?;
    Ok(title)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mdshared-put-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_folder_is_beside_the_settings_file_unless_the_preference_says() {
        let s = scratch("dir_for");
        let file = s.join("settings.ini");
        assert_eq!(dir_for(&file, ""), Some(s.join("library")));
        assert_eq!(dir_for(&file, "  "), Some(s.join("library")));
        assert_eq!(dir_for(&file, "notes/lib"), Some(s.join("notes/lib")));
        let abs = s.join("elsewhere");
        assert_eq!(dir_for(&file, &abs.to_string_lossy()), Some(abs));
        assert_eq!(
            dir_for(Path::new("settings.ini"), ""),
            None,
            "no folder, no library"
        );
    }

    #[test]
    fn the_preference_is_read_from_its_section_only_and_unescaped() {
        let t = "[window]\nlibrary_dir=wrong\n[preferences]\nfont_size=12\nlibrary_dir=D:\\One%20Drive\\library\n[other]\nlibrary_dir=nope\n";
        assert_eq!(pref_in(t), "D:\\One Drive\\library");
        assert_eq!(
            pref_in("library_dir=bare\n"),
            "bare",
            "no header counts as the section"
        );
        assert_eq!(pref_in("[preferences]\nfont_size=1\n"), "");
        let long = format!("[preferences]\nlibrary_dir={}\n", "a".repeat(PREF_MAX + 1));
        assert_eq!(pref_in(&long), "");
    }

    #[test]
    fn a_settings_file_names_its_library_and_a_missing_one_the_default() {
        let s = scratch("from");
        let file = s.join("settings.ini");
        assert_eq!(dir_from_settings(&file), Some(s.join("library")));
        std::fs::write(&file, "[preferences]\nlibrary_dir=mine\n").unwrap();
        assert_eq!(dir_from_settings(&file), Some(s.join("mine")));
    }

    #[test]
    fn a_note_is_written_into_a_new_library_file_relative_and_reads_back() {
        let s = scratch("put");
        let lib = s.join("library");
        let note = lib.join("Money").join("Budget.md");
        std::fs::create_dir_all(note.parent().unwrap()).unwrap();
        std::fs::write(&note, "# Budget\n").unwrap();
        let file = lib.join("notes.md");
        let title = put_note(&file, &[], &note, &lib).unwrap();
        assert_eq!(title, "Budget");
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("Money/Budget.md"), "{text}");
        assert!(
            !text.contains("mdshared-put-put"),
            "linked relative, not absolute: {text}"
        );
        let read = super::super::read(&lib);
        assert_eq!(read.len(), 1);
        assert_eq!(listed_as(&read, &note).as_deref(), Some("Budget"));
        let two = s.join("Two.md");
        std::fs::write(&two, "# Two\n").unwrap();
        put_note(&file, &[], &two, &lib).unwrap();
        assert_eq!(super::super::read(&lib).len(), 2);
    }

    #[test]
    fn a_file_that_cannot_be_read_is_an_error_and_is_left_alone() {
        let s = scratch("err");
        let lib = s.join("library");
        let note = s.join("N.md");
        std::fs::write(&note, "# N\n").unwrap();
        let file = lib.join("notes.md");
        std::fs::create_dir_all(&file).unwrap();
        assert!(matches!(
            put_note(&file, &[], &note, &lib),
            Err(PutError::Io(_))
        ));
        assert!(file.is_dir());
    }

    #[test]
    fn a_note_that_would_not_read_back_is_refused_and_nothing_is_written() {
        let s = scratch("fence");
        let lib = s.join("library");
        std::fs::create_dir_all(&lib).unwrap();
        let note = s.join("N.md");
        std::fs::write(
            &note, "# N
",
        )
        .unwrap();
        // An unclosed code fence: a line added at the end is inside it, so it is not a note.
        let file = lib.join("notes.md");
        let before = "# G

```
code
";
        std::fs::write(&file, before).unwrap();
        assert_eq!(
            put_note(&file, &["Z".to_string()], &note, &lib),
            Err(PutError::WouldNotReadBack)
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
    }

    /// Audit 4, D3: a library file in another code page is refused, not written back with every
    /// letter outside ASCII replaced.
    #[test]
    fn a_library_file_that_is_not_utf8_is_refused_and_left_byte_for_byte() {
        let s = scratch("cp1254");
        let lib = s.join("library");
        std::fs::create_dir_all(&lib).unwrap();
        let note = s.join("N.md");
        std::fs::write(&note, "# N\n").unwrap();
        let file = lib.join("Personal.md");
        // `- [Çağrı](<cagri.md>)` in Windows-1254: Ç is C7, ğ is F0, ı is FD.
        let before: &[u8] = b"# Personal\n\n- [\xC7a\xF0r\xFD](<cagri.md>)\n";
        std::fs::write(&file, before).unwrap();
        assert_eq!(put_note(&file, &[], &note, &lib), Err(PutError::NotUtf8));
        assert_eq!(std::fs::read(&file).unwrap(), before);
    }

    /// A UTF-16 file reads without a replaced character, but would be written back as UTF-8: it
    /// is refused too.
    #[test]
    fn a_utf16_library_file_is_refused() {
        let s = scratch("utf16");
        let lib = s.join("library");
        std::fs::create_dir_all(&lib).unwrap();
        let note = s.join("N.md");
        std::fs::write(&note, "# N\n").unwrap();
        let file = lib.join("Personal.md");
        let mut before = vec![0xFF, 0xFE];
        for u in "# Personal\n".encode_utf16() {
            before.extend_from_slice(&u.to_le_bytes());
        }
        std::fs::write(&file, &before).unwrap();
        assert_eq!(put_note(&file, &[], &note, &lib), Err(PutError::NotUtf8));
        assert_eq!(std::fs::read(&file).unwrap(), before);
    }

    /// A UTF-8 file with a byte order mark keeps it, and the owner's `<name>.tmp` beside it is
    /// not touched (audit 4, D1).
    #[test]
    fn a_bom_is_kept_and_the_owners_tmp_file_is_left_alone() {
        let s = scratch("bom");
        let lib = s.join("library");
        std::fs::create_dir_all(&lib).unwrap();
        let note = s.join("N.md");
        std::fs::write(&note, "# N\n").unwrap();
        let file = lib.join("Personal.md");
        std::fs::write(&file, "\u{FEFF}# Personal\n").unwrap();
        let tmp = lib.join("Personal.md.tmp");
        std::fs::write(&tmp, "the owner's\n").unwrap();
        put_note(&file, &[], &note, &lib).unwrap();
        let bytes = std::fs::read(&file).unwrap();
        assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "{bytes:?}");
        assert!(String::from_utf8_lossy(&bytes).contains("[N]"));
        assert_eq!(std::fs::read_to_string(&tmp).unwrap(), "the owner's\n");
    }
}
