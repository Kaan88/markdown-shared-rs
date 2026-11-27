//! The AppImage this process runs from, when it does. Inside an AppImage `current_exe()` is on a
//! read-only mount; the real file is named by `$APPIMAGE`. Not built on Windows.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
pub(crate) mod runtime;

/// The canonical path `value` (the content of `$APPIMAGE`) names, when it is set, names an
/// existing regular file. This is a location hint, not proof of the running image's identity.
pub fn path_from(value: Option<&OsStr>) -> Option<PathBuf> {
    let value = value.filter(|v| !v.is_empty())?;
    let canonical = std::fs::canonicalize(value).ok()?;
    std::fs::metadata(&canonical)
        .ok()?
        .is_file()
        .then_some(canonical)
}

/// [`path_from`] for this process's `$APPIMAGE`.
#[cfg(not(windows))]
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

#[cfg(target_os = "linux")]
fn owned_single_file(info: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    // SAFETY: geteuid takes no arguments and returns the effective uid of this process.
    let uid = unsafe { libc::geteuid() };
    info.is_file() && info.uid() == uid && info.nlink() == 1
}

/// Lock a verified installation beside its stage, on an owner-only read/write handle.
/// NFS can require write access for an exclusive lock. Keep the lock file so that
/// successive installers always lock the same inode.
#[cfg(target_os = "linux")]
pub(crate) fn lock_update(image: &Path) -> std::io::Result<std::fs::File> {
    use std::fs::OpenOptions;
    use std::io::{Error, ErrorKind};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let mut name = image.as_os_str().to_os_string();
    name.push(".lck");
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let lock = match options.create_new(true).open(&name) {
        Err(e) if e.kind() == ErrorKind::AlreadyExists => options.create_new(false).open(&name),
        result => result,
    }?;
    let info = lock.metadata()?;
    if !owned_single_file(&info) || info.mode() & 0o077 != 0 {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            "the AppImage update lock is not an owner-private regular file",
        ));
    }
    lock.try_lock().map_err(Error::from)?;
    Ok(lock)
}

/// Remove only an abandoned regular stage owned by this account, without following links.
/// The caller has verified the installation and holds its update lock.
#[cfg(target_os = "linux")]
pub(crate) fn remove_stale_stage(path: &Path) -> std::io::Result<()> {
    use std::fs::{remove_file, symlink_metadata};
    use std::io::{Error, ErrorKind};

    let info = match symlink_metadata(path) {
        Ok(info) => info,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if !owned_single_file(&info) {
        return Err(Error::new(
            ErrorKind::AlreadyExists,
            "the existing AppImage stage is not a singly linked regular file owned by this account",
        ));
    }
    remove_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn the_update_lock_fits_when_the_stage_uses_a_full_filename() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let dir = std::env::temp_dir().join(format!("mdshared-long-stage-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = CString::new(dir.as_os_str().as_bytes()).unwrap();
        // SAFETY: path is a live NUL-terminated directory pathname.
        let limit = unsafe { libc::pathconf(path.as_ptr(), libc::_PC_NAME_MAX) };
        let max = usize::try_from(limit).expect("scratch directory has a finite name limit");
        let stem = "a".repeat(max.checked_sub(".new".len()).unwrap());
        let image = dir.join(&stem);
        std::fs::File::create_new(dir.join(format!("{stem}.new"))).unwrap();
        let lock = lock_update(&image).unwrap();
        assert!(dir.join(format!("{stem}.lck")).is_file());
        drop(lock);
        std::fs::remove_dir_all(dir).unwrap();
    }

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
