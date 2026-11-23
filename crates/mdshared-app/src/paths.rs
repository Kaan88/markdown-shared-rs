//! Path helpers: one identity per file, Windows verbatim prefixes, tab titles.

use std::path::{Path, PathBuf};

/// File names differ only in case on the same file: NTFS on Windows, and APFS as macOS formats
/// it by default (a case-sensitive volume is rare, and then two names that differ only in case
/// are taken for one file).
pub const NAMES_IGNORE_CASE: bool = cfg!(any(windows, target_os = "macos"));

/// Whether two paths name the same file as far as spelling goes: case-insensitive on Windows and
/// macOS ([`NAMES_IGNORE_CASE`]).
///
/// Compared part by part, not as one string, so the separators a path happens to be spelled with
/// do not decide the answer: on Windows a library link resolves to `C:\library\Trip/One.md`,
/// because the link itself is written with forward slashes, and that is the same file as
/// `C:\library\Trip\One.md`. A `.` in the middle is not a part and so folds away; a `..` is a
/// part and stays one, because folding it by text crosses a symlink the disk has not been asked
/// about. It asks nothing of the disk at all: two spellings that reach one file through a link
/// are still two paths here.
pub fn same_path(a: &Path, b: &Path) -> bool {
    let (mut a, mut b) = (a.components(), b.components());
    loop {
        match (a.next(), b.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) if same_part(x.as_os_str(), y.as_os_str()) => {}
            _ => return false,
        }
    }
}

fn same_part(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
    if NAMES_IGNORE_CASE {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// Canonical form used to tell whether a path is already open. A missing file falls back to its
/// absolute path with its nearest existing folder canonicalized, so a restored tab for a deleted
/// file keeps the identity it had while the file existed (symlinks and, on Windows, 8.3 short
/// names such as `C:\Users\RUNNER~1` resolve the same way with or without the file).
pub fn canonical_or_absolute(path: &Path) -> PathBuf {
    let resolved = std::fs::canonicalize(path)
        .or_else(|_| canonical_parent_join(path))
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf());
    if cfg!(windows)
        && let Some(plain) = resolved.to_str().and_then(strip_verbatim)
    {
        return PathBuf::from(plain);
    }
    resolved
}

/// A missing path: its nearest existing ancestor canonicalized, the missing tail appended.
fn canonical_parent_join(path: &Path) -> std::io::Result<PathBuf> {
    let abs = std::path::absolute(path)?;
    let mut tail = Vec::new();
    let mut cur = abs.as_path();
    loop {
        if let Ok(base) = std::fs::canonicalize(cur) {
            let mut out = base;
            out.extend(tail.iter().rev());
            return Ok(out);
        }
        match (cur.parent(), cur.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name);
                cur = parent;
            }
            _ => return Ok(abs),
        }
    }
}

/// On Windows, `canonicalize` returns `\\?\C:\x`, which `absolute` does not, and which reads badly in
/// the UI. This gives the ordinary spelling, or `None` if the path only works in verbatim form or is
/// not verbatim at all, so a file keeps one identity whether or not it exists right now.
pub fn strip_verbatim(path: &str) -> Option<String> {
    // MAX_PATH, including the terminator.
    unverbatim(path).filter(|plain| plain.len() < 260)
}

/// The ordinary spelling of a verbatim path whose names allow one, whatever its length.
fn unverbatim(path: &str) -> Option<String> {
    let (plain, rest) = if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        (format!(r"\\{rest}"), rest)
    } else {
        let rest = path.strip_prefix(r"\\?\")?;
        let b = rest.as_bytes();
        if !(b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\') {
            return None;
        }
        (rest.to_string(), &rest[3..])
    };
    rest.split('\\')
        .all(usable_without_verbatim)
        .then_some(plain)
}

/// [`canonical_or_absolute`] for comparing paths: a verbatim path of any length is given its
/// ordinary spelling when its names allow one, so a long path inside a folder reads as inside
/// it (audit 4, N2). For comparing only: a path of 260 or more may need its verbatim form for a
/// call that is not Rust's own.
pub fn comparable(path: &Path) -> PathBuf {
    let resolved = canonical_or_absolute(path);
    if cfg!(windows)
        && let Some(plain) = resolved.to_str().and_then(unverbatim)
    {
        return PathBuf::from(plain);
    }
    resolved
}

/// Verbatim paths skip the Win32 name rules, so a name can exist there and not be reachable otherwise.
fn usable_without_verbatim(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    // `stem` is a name a person typed, so it is cut by bytes only where the bytes are known to be
    // ASCII: a device name is `COM1`, four ASCII bytes, and `ABé` is four bytes whose last
    // character is two of them. Slicing first and asking afterwards crashed the editor.
    let b = stem.as_bytes();
    let device = b.len() == 4
        && b[3].is_ascii_digit()
        && (b[..3].eq_ignore_ascii_case(b"COM") || b[..3].eq_ignore_ascii_case(b"LPT"));
    let reserved = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
        .iter()
        .any(|r| stem.eq_ignore_ascii_case(r))
        || device;
    !reserved
        && name != "."
        && name != ".."
        && !name.ends_with(['.', ' '])
        && !name.contains(|c: char| c < ' ' || "<>:\"/|?*".contains(c))
}

/// Tab titles. Filenames that repeat (ignoring case) gain as many parent folders as it takes to tell
/// them apart, so `a/docs/README.md` and `b/docs/README.md` never read the same.
pub fn tab_labels(paths: &[PathBuf]) -> Vec<String> {
    let name_of = |p: &Path| {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.display().to_string())
    };
    let names: Vec<String> = paths.iter().map(|p| name_of(p)).collect();
    let folded: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    let sep = std::path::MAIN_SEPARATOR_STR;
    paths
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let others: Vec<&PathBuf> = paths
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i && folded[*j] == folded[i])
                .map(|(_, p)| p)
                .collect();
            if others.is_empty() {
                return names[i].clone();
            }
            let parents = |p: &Path| -> Vec<String> {
                p.parent()
                    .map(|d| {
                        d.components()
                            .rev()
                            .map(|c| c.as_os_str().to_string_lossy().into_owned())
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let own = parents(path);
            let rest: Vec<Vec<String>> = others.iter().map(|p| parents(p)).collect();
            for depth in 1..=own.len() {
                let suffix = |v: &[String]| -> Vec<String> {
                    v.iter().take(depth).map(|s| s.to_lowercase()).collect()
                };
                let mine = suffix(&own);
                if rest.iter().all(|o| suffix(o) != mine) {
                    let mut shown: Vec<&str> = own.iter().take(depth).map(String::as_str).collect();
                    shown.reverse();
                    let joined = shown.join(sep).replacen(&format!("{sep}{sep}"), sep, 1);
                    return format!("{} — {}", names[i], joined);
                }
            }
            let full = path
                .parent()
                .map(|d| d.display().to_string())
                .unwrap_or_default();
            format!("{} — {}", names[i], full)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file whose name is four bytes and does not end in an ASCII one — `ABé.md`, `aé.md` — is
    /// not a device name, and asking whether it is one must not cut it in the middle of a
    /// character. It did, and the editor aborted: `end byte index 3 is not a char boundary`,
    /// recorded in a real crash file.
    #[test]
    fn a_four_byte_name_that_is_not_ascii_is_not_mistaken_for_a_device() {
        for name in [
            "ABé.md",
            "aé.md",
            "éé",
            "COMé",
            "LPTé.txt",
            "日本.md",
            "Ωm1",
        ] {
            assert!(
                usable_without_verbatim(name),
                "{name} is an ordinary name, and asking must not panic"
            );
        }
        // The device names it is really looking for still answer.
        for name in ["COM1", "LPT9.txt", "com4.md", "CON", "nul.txt", "CONOUT$"] {
            assert!(!usable_without_verbatim(name), "{name} is a device name");
        }
        // Four ASCII bytes that are not a device.
        for name in ["COMA", "LPT.1", "COM10", "ABC1"] {
            assert!(usable_without_verbatim(name), "{name}");
        }
    }

    /// A library link is written with forward slashes, so on Windows it resolves to a path with
    /// both kinds of separator in it. That is the same file as the one a file dialog hands back,
    /// and the only thing that ever said otherwise was comparing the two as text.
    #[test]
    fn one_file_spelled_with_either_separator_is_one_path() {
        let joined = Path::new(r"C:\library").join("Trip/One.md");
        assert!(same_path(&joined, Path::new(r"C:\library\Trip\One.md")) == cfg!(windows));
        assert!(same_path(
            Path::new("/home/k/library/Trip/One.md"),
            Path::new("/home/k/library/Trip/One.md")
        ));
        // Still two different files, whatever the separators.
        assert!(!same_path(
            Path::new(r"C:\library\Trip\One.md"),
            Path::new(r"C:\library\Trip\Two.md")
        ));
        assert!(!same_path(
            Path::new(r"C:\library\One.md"),
            Path::new(r"C:\library\Trip\One.md")
        ));
        // Case folds where the file system folds it, and the trailing slash is not a part.
        assert_eq!(
            same_path(Path::new("/x/Notes.md"), Path::new("/x/notes.md")),
            NAMES_IGNORE_CASE
        );
        assert!(same_path(Path::new("/x/notes/"), Path::new("/x/notes")));
        // A `.` in the middle is not a part at all, so it folds; `..` is a part and stays one,
        // because folding it by text would cross a symlink the disk has not been asked about.
        assert!(same_path(Path::new("/x/./a.md"), Path::new("/x/a.md")));
        assert!(!same_path(Path::new("/x/y/../a.md"), Path::new("/x/a.md")));
    }

    #[test]
    fn duplicate_parent_tab_labels_differ() {
        let paths: Vec<PathBuf> = ["/x/a/docs/README.md", "/x/b/docs/README.md", "/x/other.md"]
            .iter()
            .map(PathBuf::from)
            .collect();
        let labels = tab_labels(&paths);
        assert_ne!(
            labels[0], labels[1],
            "the old app gave these the same label"
        );
        assert!(labels[0].starts_with("README.md — "));
        assert!(labels[0].contains('a') && labels[1].contains('b'));
        assert_eq!(labels[2], "other.md");
    }

    #[test]
    fn distinct_parents_use_the_parent_name() {
        let paths: Vec<PathBuf> = ["/p/one/README.md", "/p/two/readme.md"]
            .iter()
            .map(PathBuf::from)
            .collect();
        assert_eq!(tab_labels(&paths), ["README.md — one", "readme.md — two"]);
    }

    #[test]
    fn verbatim_windows_paths_become_ordinary() {
        for (from, to) in [
            (r"\\?\C:\Users\kaan\a.md", Some(r"C:\Users\kaan\a.md")),
            (r"\\?\UNC\server\share\a.md", Some(r"\\server\share\a.md")),
            (r"C:\Users\kaan\a.md", None),
            (r"\\server\share\a.md", None),
            (r"\\?\Volume{1234}\a.md", None),
            (r"\\?\C:\Users\NUL.txt", None),
            (r"\\?\C:\Users\com1", None),
            (r"\\?\C:\Users\trailing.", None),
            (r"\\?\C:\Users\space ", None),
            (r"\\?\C:\Users\a:b", None),
        ] {
            assert_eq!(strip_verbatim(from).as_deref(), to, "{from}");
        }
        let long = format!(r"\\?\C:\{}", "a".repeat(300));
        assert_eq!(strip_verbatim(&long), None);
        // For comparing, a long path has its ordinary spelling too (audit 4, N2); a name that
        // needs the verbatim form still keeps it.
        assert_eq!(unverbatim(&long), Some(format!(r"C:\{}", "a".repeat(300))));
        assert_eq!(unverbatim(r"\\?\C:\Users\NUL.txt"), None);
    }

    #[cfg(windows)]
    #[test]
    fn canonical_paths_have_one_identity_on_windows() {
        let dir = std::env::temp_dir().join(format!("mdshared-verbatim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("real.md");
        std::fs::write(&path, "x").unwrap();
        let present = canonical_or_absolute(&path);
        assert!(
            !present.to_string_lossy().starts_with(r"\\?\"),
            "{present:?}"
        );
        std::fs::remove_file(&path).unwrap();
        assert_eq!(canonical_or_absolute(&path), present);
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_keeps_the_identity_of_its_folder() {
        let dir = std::env::temp_dir().join(format!("mdshared-missing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let there = canonical_or_absolute(&dir.join("sub").join("gone.md"));
        assert_eq!(
            there,
            canonical_or_absolute(&dir).join("sub").join("gone.md")
        );
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_still_has_an_absolute_identity() {
        assert!(canonical_or_absolute(Path::new("nowhere/gone.md")).is_absolute());
    }
}
