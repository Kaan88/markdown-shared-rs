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

struct Descriptor {
    storage: Vec<u32>,
    len: usize,
}

impl Descriptor {
    fn read(file: &File) -> Option<Self> {
        let information = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
        let mut needed = 0;
        // SAFETY: the handle is live; a null, zero-length buffer queries the size.
        unsafe {
            GetKernelObjectSecurity(
                file.as_raw_handle(),
                information,
                null_mut(),
                0,
                &mut needed,
            );
        }
        let capacity = needed;
        if !(20..=isize::MAX as usize).contains(&(capacity as usize)) {
            return None;
        }
        let mut storage = vec![0u32; (capacity as usize).div_ceil(size_of::<u32>())];
        // SAFETY: the aligned buffer is initialized and holds at least capacity bytes.
        let result = unsafe {
            GetKernelObjectSecurity(
                file.as_raw_handle(),
                information,
                storage.as_mut_ptr().cast(),
                capacity,
                &mut needed,
            )
        };
        (result != 0 && needed <= capacity).then_some(Self {
            storage,
            len: needed as usize,
        })
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: len is bounded by the initialized storage; bytes borrow that storage.
        unsafe { std::slice::from_raw_parts(self.storage.as_ptr().cast(), self.len) }
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
    inherit: u32,
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
            || AddAccessAllowedAceEx(acl, ACL_REVISION, inherit, FILE_ALL_ACCESS, user) == 0
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
            let result = with_private_attributes(0, |sa| {
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
        with_private_attributes(0, |sa| {
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

#[cfg(test)]
pub(in crate::single) fn create_legacy_fixture_dir(path: &Path) -> io::Result<()> {
    // Its ordinary child files inherit a controlled DACL, but Windows still chooses
    // each child's default owner from the token, just as main's File::create did.
    with_private_attributes(OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE, |sa| {
        with_name(path, |name| {
            // SAFETY: the name and the inherited ACL are live through the call.
            if unsafe { CreateDirectoryW(name, sa) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        })
    })
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
    (plain(&file, false) && legacy_acl(&file)).then_some(file)
}

pub(in crate::single) fn legacy_locked(path: &Path) -> bool {
    let check = file_options()
        .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES)
        .open(path);
    if !check.is_ok_and(|file| plain(&file, false) && legacy_acl(&file)) {
        return false;
    }
    file_options()
        .read(true)
        .open(path)
        .is_err_and(|e| e.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32))
}

fn process_token() -> Option<OwnedHandle> {
    let mut token = null_mut();
    // SAFETY: OpenProcessToken returns a new, owned handle on success.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        Some(OwnedHandle::from_raw_handle(token))
    }
}

fn with_sid<T>(
    token: &OwnedHandle,
    kind: TOKEN_INFORMATION_CLASS,
    use_sid: impl FnOnce(PSID, &[u8]) -> T,
) -> Option<T> {
    let min = if kind == TokenUser {
        size_of::<TOKEN_USER>()
    } else if kind == TokenOwner {
        size_of::<TOKEN_OWNER>()
    } else {
        return None;
    };
    let mut len = 0;
    // SAFETY: a null buffer and zero length ask Windows for the necessary size.
    unsafe {
        GetTokenInformation(token.as_raw_handle(), kind, null_mut(), 0, &mut len);
    }
    let capacity = len as usize;
    if !(min..=isize::MAX as usize).contains(&capacity) {
        return None;
    }
    let mut buf = vec![0usize; capacity.div_ceil(size_of::<usize>())];
    let p = buf.as_mut_ptr().cast();
    // SAFETY: Windows fills a live, aligned buffer large enough for the requested result.
    if unsafe { GetTokenInformation(token.as_raw_handle(), kind, p, len, &mut len) } == 0
        || !(min..=capacity).contains(&(len as usize))
    {
        return None;
    }
    // SAFETY: The successful query wrote the fixed header for this information class.
    let sid = unsafe {
        if kind == TokenUser {
            (*p.cast::<TOKEN_USER>()).User.Sid
        } else {
            (*p.cast::<TOKEN_OWNER>()).Owner
        }
    };
    let offset = sid.addr().checked_sub(p.addr())?;
    // SAFETY: len was checked against the size of the owned buffer.
    let bytes = unsafe { std::slice::from_raw_parts(p.cast::<u8>(), len as usize) };
    let sid = sid_bytes(bytes, offset)?;
    // Win32 takes a PSID even for read-only parameters. The callback borrows this aligned SID.
    Some(use_sid(sid.as_ptr().cast_mut().cast(), sid))
}

fn with_user<T>(use_sid: impl FnOnce(PSID) -> T) -> Option<T> {
    with_sid(&process_token()?, TokenUser, |sid, _| use_sid(sid))
}

pub(super) fn private(file: &File) -> bool {
    plain(file, false) && private_acl(file)
}

fn private_acl(file: &File) -> bool {
    check_security(file, |security, user, _| security.private(user))
}

fn legacy_acl(file: &File) -> bool {
    check_security(file, |security, user, token| {
        security.legacy(user, || {
            with_sid(token, TokenOwner, |_, owner| owner == ADMINISTRATORS) == Some(true)
        })
    })
}

fn check_security(
    file: &File,
    check: impl FnOnce(Security<'_>, &[u8], &OwnedHandle) -> bool,
) -> bool {
    let (Some(desc), Some(token)) = (Descriptor::read(file), process_token()) else {
        return false;
    };
    let Some(security) = Security::parse(desc.bytes()) else {
        return false;
    };
    with_sid(&token, TokenUser, |_, user| check(security, user, &token)) == Some(true)
}

// Well-known SIDs S-1-5-18 and S-1-5-32-544, in the self-relative descriptor's byte layout.
const SYSTEM: &[u8] = &[1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];
const ADMINISTRATORS: &[u8] = &[1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 32, 2, 0, 0];

fn sid_bytes(bytes: &[u8], offset: usize) -> Option<&[u8]> {
    let sid = bytes.get(offset..)?;
    let [1, count @ 0..=15, ..] = *sid else {
        return None;
    };
    (offset.is_multiple_of(4)).then_some(())?;
    sid.get(..8 + 4 * usize::from(count))
}

fn word(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..)?.get(..2)?.try_into().ok()?,
    ))
}

fn dword(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..)?.get(..4)?.try_into().ok()?,
    ))
}

struct Security<'a> {
    owner: &'a [u8],
    acl: &'a [u8],
    protected: bool,
}

impl<'a> Security<'a> {
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        // SECURITY_DESCRIPTOR_RELATIVE: header, then four DWORD offsets. This code
        // needs only the owner and DACL. A missing or null DACL grants everyone access.
        const HEADER: usize = 20;
        let control = word(bytes, 2)?;
        let required = SE_SELF_RELATIVE | SE_DACL_PRESENT;
        if bytes[0] != 1 || control & required != required {
            return None;
        }
        let owner = dword(bytes, 4)? as usize;
        let acl = dword(bytes, 16)? as usize;
        if owner < HEADER || acl < HEADER || !acl.is_multiple_of(4) {
            return None;
        }
        let acl = bytes.get(acl..)?;
        let size = usize::from(word(acl, 2)?);
        if size < 8 || !size.is_multiple_of(4) {
            return None;
        }
        Some(Self {
            owner: sid_bytes(bytes, owner)?,
            acl: acl.get(..size)?,
            protected: control & SE_DACL_PROTECTED != 0,
        })
    }

    fn all_aces(&self, mut check: impl FnMut(&[u8]) -> bool) -> Option<bool> {
        let mut entries = self.acl.get(8..)?;
        for _ in 0..word(self.acl, 4)? {
            let size = usize::from(word(entries, 2)?);
            if size < 4 || !size.is_multiple_of(4) {
                return None;
            }
            let (ace, rest) = entries.split_at_checked(size)?;
            if !check(ace) {
                return Some(false);
            }
            entries = rest;
        }
        Some(true)
    }

    fn private(&self, user: &[u8]) -> bool {
        self.owner == user
            && self.protected
            && word(self.acl, 4) == Some(1)
            && self.all_aces(|ace| ace[0] == 0 && sid_bytes(ace, 8) == Some(user)) == Some(true)
    }

    fn legacy(&self, user: &[u8], admin_default_owner: impl FnOnce() -> bool) -> bool {
        if self.owner != user && (self.owner != ADMINISTRATORS || !admin_default_owner()) {
            return false;
        }
        // SYSTEM and local administrators can already take ownership and write any
        // file. Inherited read grants are harmless; reject other write/control grants.
        const READ_ONLY: u32 =
            FILE_GENERIC_READ | FILE_GENERIC_EXECUTE | GENERIC_READ | GENERIC_EXECUTE;
        self.all_aces(|ace| {
            if ace[1] & INHERIT_ONLY_ACE as u8 != 0 || ace[0] == 1 {
                return true; // Inherit-only entry, or ACCESS_DENIED_ACE_TYPE.
            }
            if ace[0] != 0 {
                return false; // Do not guess the meaning of other allow ACE layouts.
            }
            let (Some(mask), Some(sid)) = (dword(ace, 4), sid_bytes(ace, 8)) else {
                return false;
            };
            sid == user || sid == SYSTEM || sid == ADMINISTRATORS || mask & !READ_ONLY == 0
        }) == Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &[u8] = &[1, 1, 0, 0, 0, 0, 0, 5, 19, 0, 0, 0];
    const USERS: &[u8] = &[1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 33, 2, 0, 0];

    fn descriptor(owner: &[u8], protected: bool, grants: &[(&[u8], u32)]) -> Vec<u8> {
        let mut bytes = vec![1, 0];
        let control =
            SE_SELF_RELATIVE | SE_DACL_PRESENT | if protected { SE_DACL_PROTECTED } else { 0 };
        bytes.extend(control.to_le_bytes());
        bytes.extend(20u32.to_le_bytes()); // Owner offset; no group or SACL.
        bytes.extend([0; 8]);
        bytes.extend(((20 + owner.len()) as u32).to_le_bytes());
        bytes.extend(owner);
        let size = 8 + grants.iter().map(|(sid, _)| 8 + sid.len()).sum::<usize>();
        bytes.extend([2, 0]);
        bytes.extend((size as u16).to_le_bytes());
        bytes.extend((grants.len() as u16).to_le_bytes());
        bytes.extend([0; 2]);
        for (sid, mask) in grants {
            bytes.extend([0, if protected { 0 } else { INHERITED_ACE as u8 }]);
            bytes.extend(((8 + sid.len()) as u16).to_le_bytes());
            bytes.extend(mask.to_le_bytes());
            bytes.extend_from_slice(sid);
        }
        bytes
    }

    #[test]
    fn an_administrators_owned_legacy_record_needs_the_matching_token_and_safe_grants() {
        let inherited = descriptor(
            ADMINISTRATORS,
            false,
            &[
                (USER, FILE_ALL_ACCESS),
                (SYSTEM, FILE_ALL_ACCESS),
                (ADMINISTRATORS, FILE_ALL_ACCESS),
                (USERS, FILE_GENERIC_READ),
            ],
        );
        let inherited = Security::parse(&inherited).unwrap();
        assert!(inherited.legacy(USER, || true));
        assert!(!inherited.legacy(USER, || false));
        assert!(!inherited.private(USER), "never a v2 record");

        for (owner, mask) in [
            (USERS, FILE_GENERIC_READ),
            (USER, FILE_GENERIC_WRITE),
            (ADMINISTRATORS, WRITE_DAC),
        ] {
            let desc = descriptor(owner, false, &[(USER, FILE_ALL_ACCESS), (USERS, mask)]);
            assert!(!Security::parse(&desc).unwrap().legacy(USER, || true));
        }
        let mut null_dacl = descriptor(USER, false, &[]);
        null_dacl[16..20].fill(0);
        assert!(Security::parse(&null_dacl).is_none());
    }

    #[test]
    fn descriptor_offsets_and_ace_lengths_are_checked_before_reading_sids() {
        let desc = descriptor(USER, true, &[(USER, FILE_ALL_ACCESS)]);
        let bytes = desc.as_slice();
        let before = Security::parse(bytes).unwrap();
        assert!(before.private(USER));

        let mut corrupt = bytes.to_vec();
        corrupt[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Security::parse(&corrupt).is_none());
        let acl = dword(bytes, 16).unwrap() as usize;
        for size in [0u16, 4, u16::MAX] {
            let mut corrupt = bytes.to_vec();
            corrupt[acl + 10..acl + 12].copy_from_slice(&size.to_le_bytes());
            let parsed = Security::parse(&corrupt).unwrap();
            assert!(!parsed.private(USER));
            assert!(!parsed.legacy(USER, || false));
        }
    }
}
