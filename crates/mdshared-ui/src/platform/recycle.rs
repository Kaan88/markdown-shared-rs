//! The Recycle Bin (D-046): one file put in the system's bin through the `trash` crate, and the
//! question whether a file's drive has one. Built only with the feature `recycle`, so an
//! application that leaves it off (mdview) never builds the crate.
//!
//! Nothing here runs at start: the question is asked when a plan offers the box, and the bin is
//! used by a write the owner asked for.

use std::path::Path;

/// Why a file cannot be put in the Recycle Bin, as the plan card says it (mdedit's plan 5.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BinRefusal {
    /// The file is not on a local fixed drive: on a network share or a removable drive a
    /// "recycle" may become a delete or an error, so the box is not offered there.
    NoBin,
    /// The system has no bin to offer here, and why.
    Unavailable(String),
}

impl BinRefusal {
    /// The reason in the card's words.
    pub fn text(&self) -> String {
        match self {
            BinRefusal::NoBin => "This drive has no Recycle Bin".to_string(),
            BinRefusal::Unavailable(why) => {
                format!("The Recycle Bin is not available here: {why}")
            }
        }
    }
}

/// Why `path` cannot go to the Recycle Bin; `None` when it can. It reads the disk and the
/// system's settings and changes nothing.
pub fn recycle_refusal(path: &Path) -> Option<BinRefusal> {
    if !path.is_absolute() {
        return Some(BinRefusal::Unavailable(
            "the file's place on disk is not known".to_string(),
        ));
    }
    imp::refusal(path)
}

/// Puts the file at `path` in the Recycle Bin. `Err` says why it did not go; the file is then
/// where it was. Only a regular file is taken, and only where [`recycle_refusal`] has nothing
/// against it. It returns once the file is no longer at `path`.
pub fn recycle(path: &Path) -> Result<(), String> {
    if let Some(no) = recycle_refusal(path) {
        return Err(match no {
            BinRefusal::NoBin => "this drive has no Recycle Bin".to_string(),
            BinRefusal::Unavailable(why) => why,
        });
    }
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("it is not a file".to_string());
    }
    let owned = path.to_path_buf();
    // A thread of its own, every time: on Windows the crate starts COM on the thread that calls
    // it and panics when that thread has already started it another way, and a panic ends the
    // program (the release build aborts). A new thread has not started it at all.
    let worker = std::thread::Builder::new()
        .name("recycle".to_string())
        .spawn(move || trash::delete(&owned).map_err(|e| e.to_string()))
        .map_err(|e| format!("the system could not be asked: {e}"))?;
    worker
        .join()
        .map_err(|_| "the system's Recycle Bin call failed".to_string())??;
    if path.symlink_metadata().is_ok() {
        return Err("it is still where it was".to_string());
    }
    Ok(())
}

/// Opens the Recycle Bin in the file manager. Windows only; elsewhere it does nothing and says
/// so with false.
pub fn open_recycle_bin() -> bool {
    imp::open_bin()
}

/// Whether every mount point, device and type in a `/proc/mounts` text is UTF-8 once its octal
/// escapes are read: the `trash` crate's Linux back end reads them as UTF-8 and would panic on
/// one that is not, so such a system is refused before the crate is called.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
pub(crate) fn mounts_are_text(bytes: &[u8]) -> bool {
    bytes.split(|&b| b == b'\n').all(|line| {
        line.split(|&b| b == b' ' || b == b'\t')
            .filter(|f| !f.is_empty())
            .take(3)
            .all(|field| std::str::from_utf8(&unescape(field)).is_ok())
    })
}

/// `\040` and the other three-digit octal escapes of a mounts file, read; anything else as it is.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn unescape(field: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(field.len());
    let mut i = 0;
    while i < field.len() {
        let octal = field
            .get(i + 1..i + 4)
            .filter(|d| field[i] == b'\\' && d.iter().all(|c| (b'0'..=b'7').contains(c)));
        match octal {
            Some(d) => {
                let v = d.iter().fold(0u32, |acc, c| acc * 8 + u32::from(c - b'0'));
                out.push((v & 0xff) as u8);
                i += 4;
            }
            None => {
                out.push(field[i]);
                i += 1;
            }
        }
    }
    out
}

#[cfg(windows)]
mod imp {
    use super::BinRefusal;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Path, Prefix};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
    };
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW,
    };
    use windows_sys::Win32::UI::Shell::{SHQUERYRBINFO, SHQueryRecycleBinW};

    /// `GetDriveTypeW`'s answer for a hard disk or a flash drive the system counts as fixed.
    const DRIVE_FIXED: u32 = 3;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The text up to the first NUL.
    fn text_of(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }

    fn reg_dword(root: HKEY, subkey: &str, name: &str) -> Option<u32> {
        let sub = wide(subkey);
        let nm = wide(name);
        let mut val: u32 = 0;
        let mut size: u32 = 4;
        // SAFETY: the strings are NUL-terminated; `val` and `size` are valid for 4 bytes.
        let rc = unsafe {
            RegGetValueW(
                root,
                sub.as_ptr(),
                nm.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                &mut val as *mut u32 as *mut _,
                &mut size,
            )
        };
        (rc == 0).then_some(val)
    }

    pub(super) fn refusal(path: &Path) -> Option<BinRefusal> {
        // A share is never a local drive, and the disk is not asked about it.
        if let Some(Component::Prefix(p)) = path.components().next()
            && matches!(p.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..))
        {
            return Some(BinRefusal::NoBin);
        }
        let target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut root = [0u16; 1024];
        // SAFETY: `target` is NUL-terminated; `root` holds as many characters as it is told.
        let ok = unsafe { GetVolumePathNameW(target.as_ptr(), root.as_mut_ptr(), 1024) };
        if ok == 0 {
            // SAFETY: no preconditions.
            let code = unsafe { GetLastError() };
            return Some(BinRefusal::Unavailable(format!(
                "the file's drive could not be found ({})",
                std::io::Error::from_raw_os_error(code as i32)
            )));
        }
        // SAFETY: `root` is NUL-terminated by the call above.
        if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_FIXED {
            return Some(BinRefusal::NoBin);
        }
        let policy = "Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer";
        if reg_dword(HKEY_CURRENT_USER, policy, "NoRecycleFiles") == Some(1)
            || reg_dword(HKEY_LOCAL_MACHINE, policy, "NoRecycleFiles") == Some(1)
        {
            return Some(BinRefusal::Unavailable(
                "a policy on this PC deletes files instead".to_string(),
            ));
        }
        // The drive's own setting, "Don't move files to the Recycle Bin".
        let mut volume = [0u16; 64];
        // SAFETY: `root` is NUL-terminated; `volume` holds as many characters as it is told.
        let named =
            unsafe { GetVolumeNameForVolumeMountPointW(root.as_ptr(), volume.as_mut_ptr(), 64) };
        if named != 0 {
            let name = text_of(&volume);
            if let (Some(a), Some(b)) = (name.find('{'), name.find('}'))
                && a < b
            {
                let key = format!(
                    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\BitBucket\\Volume\\{}",
                    &name[a..=b]
                );
                if reg_dword(HKEY_CURRENT_USER, &key, "NukeOnDelete") == Some(1) {
                    return Some(BinRefusal::Unavailable(
                        "this drive is set to delete files at once".to_string(),
                    ));
                }
            }
        }
        let mut info = SHQUERYRBINFO {
            cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
            i64Size: 0,
            i64NumItems: 0,
        };
        // SAFETY: `root` is NUL-terminated; `info` has its size set.
        let hr = unsafe { SHQueryRecycleBinW(root.as_ptr(), &mut info) };
        if hr < 0 {
            return Some(BinRefusal::Unavailable(format!(
                "the system answered {:#010x} for {}",
                hr as u32,
                text_of(&root)
            )));
        }
        None
    }

    pub(super) fn open_bin() -> bool {
        std::process::Command::new("explorer.exe")
            .arg("shell:RecycleBinFolder")
            .spawn()
            .is_ok()
    }
}

#[cfg(unix)]
mod imp {
    use super::BinRefusal;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    /// Where the bin of the owner's home is: `$XDG_DATA_HOME/Trash` on Linux (`~/.local/share`
    /// when that is not set), `~/.Trash` on macOS.
    fn home_bin() -> Option<PathBuf> {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        if cfg!(target_os = "macos") {
            return Some(home.join(".Trash"));
        }
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".local").join("share"));
        Some(data.join("Trash"))
    }

    /// The device of `path`, or of the nearest folder above it that is there.
    fn device(path: &Path) -> Option<u64> {
        path.ancestors()
            .find_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.dev())
    }

    pub(super) fn refusal(path: &Path) -> Option<BinRefusal> {
        let Some(bin) = home_bin() else {
            return Some(BinRefusal::Unavailable(
                "the home folder is not known".to_string(),
            ));
        };
        let file = match std::fs::metadata(path) {
            Ok(m) => m.dev(),
            Err(e) => return Some(BinRefusal::Unavailable(e.to_string())),
        };
        // Only a file on the drive of the home's own bin: elsewhere the bin is a folder at the top
        // of that drive, which a network or removable drive may not have.
        if device(&bin) != Some(file) {
            return Some(BinRefusal::NoBin);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let mounts = std::fs::read("/proc/mounts").or_else(|_| std::fs::read("/etc/mtab"));
            match mounts {
                Ok(bytes) if super::mounts_are_text(&bytes) => {}
                Ok(_) => {
                    return Some(BinRefusal::Unavailable(
                        "a drive's name in the list of mounts is not UTF-8".to_string(),
                    ));
                }
                Err(e) => {
                    return Some(BinRefusal::Unavailable(format!(
                        "the list of mounts could not be read: {e}"
                    )));
                }
            }
        }
        None
    }

    pub(super) fn open_bin() -> bool {
        false
    }
}

#[cfg(not(any(windows, unix)))]
mod imp {
    use super::BinRefusal;
    use std::path::Path;

    pub(super) fn refusal(_path: &Path) -> Option<BinRefusal> {
        Some(BinRefusal::Unavailable(
            "this system has none the program knows".to_string(),
        ))
    }

    pub(super) fn open_bin() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recycle_refuses_a_relative_path_and_touches_nothing() {
        let r = recycle_refusal(Path::new("notes/a.md"));
        assert!(matches!(r, Some(BinRefusal::Unavailable(_))), "{r:?}");
        assert!(recycle(Path::new("notes/a.md")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn recycle_refusal_says_a_share_has_no_bin_without_asking_the_network() {
        assert_eq!(
            recycle_refusal(Path::new(r"\\server\share\notes\a.md")),
            Some(BinRefusal::NoBin)
        );
        assert_eq!(
            recycle_refusal(Path::new(r"\\?\UNC\server\share\a.md")),
            Some(BinRefusal::NoBin)
        );
    }

    #[test]
    fn recycle_refusal_reads_in_the_cards_words() {
        assert_eq!(BinRefusal::NoBin.text(), "This drive has no Recycle Bin");
        assert_eq!(
            BinRefusal::Unavailable("off".into()).text(),
            "The Recycle Bin is not available here: off"
        );
    }

    #[test]
    fn recycle_mounts_that_are_not_utf8_are_found_before_the_crate_reads_them() {
        let good = b"/dev/sda1 / ext4 rw 0 0\n/dev/sdb1 /media/My\\040Disk vfat rw 0 0\n";
        assert!(mounts_are_text(good));
        // An escaped byte that is not UTF-8 on its own, and a raw one.
        assert!(!mounts_are_text(b"/dev/sdb1 /media/\\377 vfat rw 0 0\n"));
        assert!(!mounts_are_text(b"/dev/sdb1 /media/\xff vfat rw 0 0\n"));
        // The options and the numbers are not read as names.
        assert!(mounts_are_text(b"/dev/sda1 / ext4 \xff 0 0\n"));
        assert!(mounts_are_text(b""));
    }

    /// The real Recycle Bin, by hand: `cargo test -p mdshared-ui --features recycle -- --ignored
    /// recycle_puts_a_temporary_file`. It makes a file of its own in the temporary folder and puts
    /// it in the bin; nothing of the owner's is touched. The file stays in the bin.
    #[test]
    #[ignore]
    fn recycle_puts_a_temporary_file_of_its_own_in_the_bin() {
        let dir = std::env::temp_dir().join(format!("mdshared-recycle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the folder");
        let file = dir.join("mdshared recycle test.txt");
        std::fs::write(&file, b"made by mdshared-ui's recycle test").expect("the file");
        if let Some(no) = recycle_refusal(&file) {
            let _ = std::fs::remove_dir_all(&dir);
            panic!("this temporary folder has no bin: {no:?}");
        }
        recycle(&file).expect("it goes to the bin");
        assert!(!file.exists(), "it is no longer where it was");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
