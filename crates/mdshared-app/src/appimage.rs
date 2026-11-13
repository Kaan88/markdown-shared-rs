//! The AppImage this process runs from, when it does. Inside an AppImage `current_exe()` is on a
//! read-only mount; the real file is named by `$APPIMAGE`. Not built on Windows.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The canonical path `value` (the content of `$APPIMAGE`) names, when it is set, names an
/// existing regular file. This is the one rule for "this is an AppImage".
pub fn path_from(value: Option<&OsStr>) -> Option<PathBuf> {
    let value = value.filter(|v| !v.is_empty())?;
    let canonical = std::fs::canonicalize(value).ok()?;
    std::fs::metadata(&canonical)
        .ok()?
        .is_file()
        .then_some(canonical)
}

/// [`path_from`] for this process's `$APPIMAGE`.
pub fn path() -> Option<PathBuf> {
    path_from(std::env::var_os("APPIMAGE").as_deref())
}

/// Whether files can be created in `dir`, tried by making and removing one: the mode bits do not
/// say (read-only mounts, ACLs).
pub fn folder_is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".mdshared-write-test-{}", std::process::id()));
    let ok = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .is_ok();
    if ok {
        _ = std::fs::remove_file(&probe);
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_existing_file_is_an_appimage() {
        let d = std::env::temp_dir().join(format!("mdshared-appimage-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("app.AppImage");
        std::fs::write(&f, "x").unwrap();
        assert_eq!(
            path_from(Some(f.as_os_str())),
            Some(std::fs::canonicalize(&f).unwrap())
        );
        assert_eq!(path_from(None), None);
        assert_eq!(path_from(Some(OsStr::new(""))), None);
        assert_eq!(path_from(Some(d.as_os_str())), None, "a folder");
        assert_eq!(path_from(Some(d.join("gone").as_os_str())), None);
        assert!(folder_is_writable(&d));
        _ = std::fs::remove_dir_all(&d);
    }
}
