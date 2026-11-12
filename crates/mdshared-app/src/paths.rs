//! Path helpers: one identity per file, Windows verbatim prefixes, tab titles.

use std::path::{Path, PathBuf};

/// Whether two paths name the same file as far as spelling goes: case-insensitive on Windows.
pub fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// Canonical form used to tell whether a path is already open. A missing file falls back to its
/// absolute path, so a restored tab for a deleted file keeps a stable identity.
pub fn canonical_or_absolute(path: &Path) -> PathBuf {
    let resolved = std::fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf());
    if cfg!(windows)
        && let Some(plain) = resolved.to_str().and_then(strip_verbatim)
    {
        return PathBuf::from(plain);
    }
    resolved
}

/// On Windows, `canonicalize` returns `\\?\C:\x`, which `absolute` does not, and which reads badly in
/// the UI. This gives the ordinary spelling, or `None` if the path only works in verbatim form or is
/// not verbatim at all, so a file keeps one identity whether or not it exists right now.
pub fn strip_verbatim(path: &str) -> Option<String> {
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
    // MAX_PATH, including the terminator.
    (plain.len() < 260 && rest.split('\\').all(usable_without_verbatim)).then_some(plain)
}

/// Verbatim paths skip the Win32 name rules, so a name can exist there and not be reachable otherwise.
fn usable_without_verbatim(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    let reserved = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
        .iter()
        .any(|r| stem.eq_ignore_ascii_case(r))
        || (stem.len() == 4
            && (stem[..3].eq_ignore_ascii_case("COM") || stem[..3].eq_ignore_ascii_case("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
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
    fn a_missing_file_still_has_an_absolute_identity() {
        assert!(canonical_or_absolute(Path::new("nowhere/gone.md")).is_absolute());
    }
}
