//! Open the private endpoint directory once and work relative to that handle, so replacing
//! a name in its parent cannot redirect either the lock or the key. Never follow a link
//! or wait for a FIFO in place of either file.

use std::ffi::{CString, OsStr, c_int};
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

const NOFOLLOW_NONBLOCK: c_int = libc::O_NOFOLLOW | libc::O_NONBLOCK;

/// The Linux kernel hostname, as bytes rather than an environment override.
#[cfg(target_os = "linux")]
pub(super) fn hostname() -> Option<Vec<u8>> {
    let mut name = [0u8; 65]; // Linux reserves up to 64 bytes plus the terminator.
    // SAFETY: the buffer is writable and its exact length is passed to gethostname.
    if unsafe { libc::gethostname(name.as_mut_ptr().cast(), name.len()) } != 0 {
        return None;
    }
    let name = name.split(|&b| b == 0).next()?;
    (!name.is_empty()).then(|| name.to_vec())
}

pub(super) fn random() -> Option<[u8; 32]> {
    let mut bytes = [0; 32];
    File::open("/dev/urandom")
        .ok()?
        .read_exact(&mut bytes)
        .ok()?;
    Some(bytes)
}

fn owned(meta: &std::fs::Metadata) -> bool {
    // SAFETY: geteuid takes no arguments and returns the caller's effective uid.
    meta.uid() == unsafe { libc::geteuid() }
}

fn open_path(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(NOFOLLOW_NONBLOCK)
        .open(path)
}

pub(in crate::single) fn legacy_file(path: &Path) -> Option<File> {
    let file = open_path(path).ok()?;
    let meta = file.metadata().ok()?;
    (meta.is_file() && owned(&meta) && meta.mode() & 0o022 == 0).then_some(file)
}

/// A 0664 decimal port written under umask 0002 is safe to probe inside an owner-only
/// settings directory. It does not establish a predecessor or authorize any payload.
pub(in crate::single) fn legacy_probe_file(path: &Path) -> Option<File> {
    legacy_file(path).or_else(|| {
        let dir = Directory::open(path.parent()?, false)?;
        let file = dir.read_file(path.file_name()?).ok()?;
        let meta = file.metadata().ok()?;
        (meta.is_file()
            && owned(&meta)
            && meta.nlink() == 1
            && meta.mode() & 0o777 == 0o664
            && no_grants(&file))
        .then_some(file)
    })
}

pub(in crate::single) fn legacy_locked(path: &Path) -> bool {
    legacy_file(path).is_some_and(|file| {
        matches!(
            file.try_lock_shared(),
            Err(std::fs::TryLockError::WouldBlock)
        )
    })
}

pub(in crate::single) struct Directory(File);

impl Directory {
    pub(in crate::single) fn open(path: &Path, create: bool) -> Option<Self> {
        if create
            && let Err(e) = DirBuilder::new().mode(0o700).create(path)
            && e.kind() != io::ErrorKind::AlreadyExists
        {
            return None;
        }
        let file = open_path(path).ok()?;
        let meta = file.metadata().ok()?;
        (meta.is_dir() && owned(&meta) && meta.mode() & 0o777 == 0o700 && no_grants(&file))
            .then_some(Self(file))
    }

    fn child(&self, name: &OsStr, flags: c_int) -> io::Result<File> {
        let name = name_to_c(name)?;
        // SAFETY: the directory is held open, name is a NUL-terminated single component,
        // and the variadic mode is a promoted int. A successful call gives us an owned fd.
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                flags | NOFOLLOW_NONBLOCK | libc::O_CLOEXEC,
                0o600 as c_int,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openat just returned this new file descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    pub(super) fn create_file(&self, name: &OsStr) -> io::Result<File> {
        self.child(name, libc::O_RDWR | libc::O_CREAT | libc::O_EXCL)
    }

    pub(super) fn read_file(&self, name: &OsStr) -> io::Result<File> {
        self.child(name, libc::O_RDONLY)
    }

    pub(super) fn open_lock(&self, name: &OsStr) -> io::Result<File> {
        self.child(name, libc::O_RDWR)
    }

    pub(super) fn rename(&self, from: &OsStr, to: &OsStr) -> bool {
        let (Ok(from), Ok(to)) = (name_to_c(from), name_to_c(to)) else {
            return false;
        };
        let fd = self.0.as_raw_fd();
        // SAFETY: fd is an open directory and both names are NUL-terminated.
        unsafe { libc::renameat(fd, from.as_ptr(), fd, to.as_ptr()) == 0 }
    }

    pub(super) fn remove(&self, name: &OsStr) {
        let Ok(name) = name_to_c(name) else { return };
        // SAFETY: fd is an open directory and name is NUL-terminated. Flags 0 remove files only.
        unsafe { libc::unlinkat(self.0.as_raw_fd(), name.as_ptr(), 0) };
    }
}

pub(super) fn private(file: &File) -> bool {
    file.metadata()
        .is_ok_and(|m| m.is_file() && owned(&m) && m.mode() & 0o077 == 0)
        && no_grants(file)
}

#[cfg(not(target_os = "macos"))]
fn no_grants(_file: &File) -> bool {
    // The POSIX ACL mask for named users and groups is already covered by the mode check.
    true
}

#[cfg(target_os = "macos")]
fn no_grants(file: &File) -> bool {
    use std::ffi::c_void;
    use std::ptr::null_mut;
    unsafe extern "C" {
        fn acl_get_fd_np(fd: c_int, kind: c_int) -> *mut c_void;
        fn acl_valid(acl: *mut c_void) -> c_int;
        fn acl_get_entry(acl: *mut c_void, which: c_int, entry: *mut *mut c_void) -> c_int;
        fn acl_get_tag_type(entry: *mut c_void, tag: *mut c_int) -> c_int;
        fn acl_free(acl: *mut c_void) -> c_int;
    }
    // SAFETY: the fd is live; the ACL and its borrowed entries are owned by libSystem until freed.
    unsafe {
        let acl = acl_get_fd_np(file.as_raw_fd(), 0x100); // ACL_TYPE_EXTENDED
        if acl.is_null() {
            return io::Error::last_os_error().kind() == io::ErrorKind::NotFound;
        }
        let mut ok = acl_valid(acl) == 0;
        let mut which = 0; // ACL_FIRST_ENTRY
        while ok {
            let mut entry = null_mut();
            if acl_get_entry(acl, which, &mut entry) != 0 {
                // On a validated Darwin ACL, iteration ends with EINVAL.
                ok = io::Error::last_os_error().kind() == io::ErrorKind::InvalidInput;
                break;
            }
            let mut tag = 0;
            // Deny entries cannot grant access. We deliberately accept no allow entries.
            ok = acl_get_tag_type(entry, &mut tag) == 0 && tag == 2; // ACL_EXTENDED_DENY
            which = -1; // ACL_NEXT_ENTRY
        }
        acl_free(acl);
        ok
    }
}

fn name_to_c(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| io::ErrorKind::InvalidInput.into())
}

#[cfg(test)]
#[test]
fn the_kernel_sees_nonblocking_and_close_on_exec_on_private_files() {
    let path = std::env::temp_dir().join(format!("mdshared-unix-flags-{}", std::process::id()));
    let dir = Directory::open(&path, true).unwrap();
    let name = OsStr::new("lock");
    let created = dir.create_file(name).unwrap();
    assert_eq!(
        dir.create_file(name).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    for file in [
        created,
        dir.open_lock(name).unwrap(),
        dir.read_file(name).unwrap(),
    ] {
        // SAFETY: fcntl only queries flags on this open fd; these commands take no extra arg.
        let (status, fd_flags) = unsafe {
            (
                libc::fcntl(file.as_raw_fd(), libc::F_GETFL),
                libc::fcntl(file.as_raw_fd(), libc::F_GETFD),
            )
        };
        assert!(status >= 0 && status & libc::O_NONBLOCK != 0);
        assert!(fd_flags >= 0 && fd_flags & libc::FD_CLOEXEC != 0);
    }
    std::fs::remove_dir_all(path).unwrap();
}
