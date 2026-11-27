//! Create a file with a protected, owner-only ACL from the start. Check the opened
//! handle on both publication and lookup: a shared folder or a filesystem that does
//! not preserve ACLs must not turn another account into a trusted peer.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::*;
use windows_sys::Win32::Security::Cryptography::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

pub(super) fn random() -> Option<[u8; 32]> {
    let mut buf = [0; 32];
    // SAFETY: the system RNG needs no handle; buf has exactly the requested size.
    (unsafe {
        BCryptGenRandom(
            null_mut(),
            buf.as_mut_ptr(),
            32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    } >= 0)
        .then_some(buf)
}

struct Descriptor(PSECURITY_DESCRIPTOR);
impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: GetSecurityInfo returns a LocalAlloc-owned descriptor.
        unsafe {
            LocalFree(self.0);
        }
    }
}

fn with_name<T>(path: &Path, f: impl FnOnce(*const u16) -> io::Result<T>) -> io::Result<T> {
    let name: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
    if name[..name.len() - 1].contains(&0) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    f(name.as_ptr())
}

fn with_private_attributes<T>(
    use_attributes: impl FnOnce(&SECURITY_ATTRIBUTES) -> io::Result<T>,
) -> io::Result<T> {
    // SAFETY: the descriptor, aligned ACL buffer and token SID all live through the call.
    // Windows bounds-checks the ACL as it inserts the user's single allow entry.
    with_user(|user| unsafe {
        let mut descriptor = std::mem::MaybeUninit::<SECURITY_DESCRIPTOR>::uninit();
        let desc = descriptor.as_mut_ptr().cast();
        let mut storage = [0u32; 32]; // ACL header, one ACE, at most SECURITY_MAX_SID_SIZE.
        let acl = storage.as_mut_ptr().cast();
        if InitializeSecurityDescriptor(desc, 1) == 0
            || InitializeAcl(acl, size_of_val(&storage) as u32, ACL_REVISION) == 0
            || AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, user) == 0
            || SetSecurityDescriptorDacl(desc, 1, acl, 0) == 0
            || SetSecurityDescriptorOwner(desc, user, 0) == 0
            || SetSecurityDescriptorControl(desc, SE_DACL_PROTECTED, SE_DACL_PROTECTED) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let sa = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: desc,
            bInheritHandle: 0,
        };
        use_attributes(&sa)
    })
    .ok_or_else(|| io::Error::from(io::ErrorKind::PermissionDenied))?
}

pub(in crate::single) struct Directory {
    path: PathBuf,
    // Without FILE_SHARE_DELETE this directory cannot be replaced while we use its name.
    _handle: File,
}

impl Directory {
    pub(in crate::single) fn open(path: &Path, create: bool) -> Option<Self> {
        if create {
            let parent = path.parent()?.canonicalize().ok()?;
            let result = with_private_attributes(|sa| {
                with_name(&parent.join(path.file_name().unwrap_or_default()), |name| {
                    // SAFETY: both arguments live through the call; the new ACL is protected.
                    (unsafe { CreateDirectoryW(name, sa) } != 0)
                        .then_some(())
                        .ok_or_else(io::Error::last_os_error)
                })
            });
            if let Err(e) = result
                && e.kind() != io::ErrorKind::AlreadyExists
            {
                return None;
            }
        }
        let handle = OpenOptions::new()
            // Metadata-only access does not participate in Windows sharing checks.
            .access_mode(FILE_LIST_DIRECTORY | READ_CONTROL | FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .ok()?;
        if !plain(&handle, true) || !private_acl(&handle) {
            return None;
        }
        let mut name = vec![0; 512];
        loop {
            // SAFETY: the directory is live, and the buffer has the requested number of u16s.
            let n = unsafe {
                GetFinalPathNameByHandleW(
                    handle.as_raw_handle(),
                    name.as_mut_ptr(),
                    name.len() as u32,
                    0,
                )
            } as usize;
            if n == 0 {
                return None;
            }
            if n < name.len() {
                return Some(Self {
                    path: OsString::from_wide(&name[..n]).into(),
                    _handle: handle,
                });
            }
            name.resize(n + 1, 0);
        }
    }

    pub(super) fn create_file(&self, name: &OsStr) -> io::Result<File> {
        with_private_attributes(|sa| {
            with_name(&self.path.join(name), |path| {
                // SAFETY: the path and ACL live through the call; CREATE_NEW never opens an
                // existing entry. On success ownership of the handle transfers to File.
                unsafe {
                    let h = CreateFileW(
                        path,
                        GENERIC_READ | GENERIC_WRITE,
                        FILE_SHARE_READ | FILE_SHARE_WRITE,
                        sa,
                        CREATE_NEW,
                        FILE_FLAG_OPEN_REPARSE_POINT,
                        null_mut(),
                    );
                    if h == INVALID_HANDLE_VALUE {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(File::from_raw_handle(h))
                    }
                }
            })
        })
    }

    pub(super) fn read_file(&self, name: &OsStr) -> io::Result<File> {
        file_options().read(true).open(self.path.join(name))
    }

    pub(super) fn open_lock(&self, name: &OsStr) -> io::Result<File> {
        file_options()
            .read(true)
            .write(true)
            .open(self.path.join(name))
    }

    pub(super) fn rename(&self, from: &OsStr, to: &OsStr) -> bool {
        fs::rename(self.path.join(from), self.path.join(to)).is_ok()
    }

    pub(super) fn remove(&self, name: &OsStr) {
        _ = fs::remove_file(self.path.join(name));
    }
}

fn file_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    options
}

fn plain(file: &File, dir: bool) -> bool {
    // SAFETY: the handle is live; only ordinary disk files or directories are allowed.
    (unsafe { GetFileType(file.as_raw_handle()) }) == FILE_TYPE_DISK
        && file.metadata().is_ok_and(|m| {
            (if dir { m.is_dir() } else { m.is_file() })
                && m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
        })
}

pub(in crate::single) fn legacy_file(path: &Path) -> Option<File> {
    let file = file_options().read(true).open(path).ok()?;
    (plain(&file, false) && owner_acl(&file, false)).then_some(file)
}

pub(in crate::single) fn legacy_locked(path: &Path) -> bool {
    let check = file_options()
        .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES)
        .open(path);
    if !check.is_ok_and(|file| plain(&file, false) && owner_acl(&file, false)) {
        return false;
    }
    file_options()
        .read(true)
        .open(path)
        .is_err_and(|e| e.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32))
}

fn with_user<T>(use_sid: impl FnOnce(PSID) -> T) -> Option<T> {
    let mut token = null_mut();
    // SAFETY: the token handle is owned, the buffer is sized by Windows and aligned for
    // TOKEN_USER, and the buffer containing the SID outlives use_sid.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut len = 0;
        GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut len);
        let mut buf = vec![0usize; (len as usize).div_ceil(size_of::<usize>())];
        let ptr = buf.as_mut_ptr().cast();
        (GetTokenInformation(token.as_raw_handle(), TokenUser, ptr, len, &mut len) != 0)
            .then(|| use_sid((*ptr.cast::<TOKEN_USER>()).User.Sid))
    }
}

pub(super) fn private(file: &File) -> bool {
    plain(file, false) && private_acl(file)
}

fn private_acl(file: &File) -> bool {
    owner_acl(file, true)
}

fn owner_acl(file: &File, require_private: bool) -> bool {
    let (mut owner, mut acl, mut desc) = (null_mut(), null_mut(), null_mut());
    // SAFETY: Windows returns the owner and ACL inside desc, held until all checks finish.
    // Inspect the ACE type before using the allow-ACE layout and its inline SID.
    unsafe {
        if GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION
                | if require_private {
                    DACL_SECURITY_INFORMATION
                } else {
                    0
                },
            &mut owner,
            null_mut(),
            &mut acl,
            null_mut(),
            &mut desc,
        ) != ERROR_SUCCESS
        {
            return false;
        }
        let _desc = Descriptor(desc);
        if owner.is_null() || with_user(|sid| EqualSid(owner, sid) != 0) != Some(true) {
            return false;
        }
        if !require_private {
            return true;
        }
        let (mut control, mut revision) = (0, 0);
        if acl.is_null()
            || (*acl).AceCount != 1
            || GetSecurityDescriptorControl(desc, &mut control, &mut revision) == 0
            || control & SE_DACL_PROTECTED == 0
        {
            return false;
        }
        let mut ace = null_mut();
        GetAce(acl, 0, &mut ace) != 0
            && (*ace.cast::<ACE_HEADER>()).AceType == 0
            && EqualSid(
                (&raw mut (*ace.cast::<ACCESS_ALLOWED_ACE>()).SidStart).cast(),
                owner,
            ) != 0
    }
}
