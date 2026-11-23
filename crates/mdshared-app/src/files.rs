//! Reading files from disk: change stamps, a read that waits out a half-finished save, text
//! decoding, writing a new file that never replaces one, and replacing a file of the owner's
//! through a temporary name that cannot be one of the owner's.

use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// What the change detector compares: a missing file is a state of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStamp {
    Missing,
    Present {
        modified: Option<SystemTime>,
        size: u64,
    },
}

pub fn stamp(path: &Path) -> FileStamp {
    match std::fs::metadata(path) {
        Ok(m) => FileStamp::Present {
            modified: m.modified().ok(),
            size: m.len(),
        },
        Err(_) => FileStamp::Missing,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    TooLarge,
    StillChanging,
    Io(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::TooLarge => f.write_str("File exceeds the size limit"),
            LoadError::StillChanging => {
                f.write_str("File is still changing; wait for the save to finish and retry")
            }
            LoadError::Io(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for LoadError {}

const READ_ATTEMPTS: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_millis(40);

pub struct Loaded {
    pub bytes: Vec<u8>,
    pub stamp: FileStamp,
}

/// Reads a file of at most `max_bytes`, retrying while it changes underneath the read so a
/// half-finished save is not shown.
pub fn read_stable(path: &Path, max_bytes: usize) -> Result<Loaded, LoadError> {
    let mut last_valid = None;
    for attempt in 0..READ_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(RETRY_DELAY);
        }
        let before = stamp(path);
        let bytes = read_limited(path, max_bytes)?;
        let after = stamp(path);
        let unchanged = before == after
            && matches!(after, FileStamp::Present { size, .. } if size == bytes.len() as u64);
        if !unchanged {
            continue;
        }
        // Invalid UTF-8 can also mean a save caught mid-write, so it gets the same retries.
        let valid = has_valid_encoding(&bytes);
        last_valid = Some(Loaded {
            bytes,
            stamp: after,
        });
        if valid {
            break;
        }
    }
    last_valid.ok_or(LoadError::StillChanging)
}

fn read_limited(path: &Path, max_bytes: usize) -> Result<Vec<u8>, LoadError> {
    let io_error = |e: io::Error| LoadError::Io(e.to_string());
    let file = std::fs::File::open(path).map_err(io_error)?;
    let len = file.metadata().map_err(io_error)?.len();
    if len > max_bytes as u64 {
        return Err(LoadError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(len as usize);
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > max_bytes {
        return Err(LoadError::TooLarge);
    }
    Ok(bytes)
}

fn has_valid_encoding(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xFE])
        || bytes.starts_with(&[0xFE, 0xFF])
        || std::str::from_utf8(bytes).is_ok()
}

/// Decodes file bytes to text with any BOM removed. The flag reports replaced invalid input.
pub fn decode(bytes: &[u8]) -> (std::borrow::Cow<'_, str>, bool) {
    use std::borrow::Cow;
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16(rest, u16::from_be_bytes);
    }
    match String::from_utf8_lossy(bytes) {
        Cow::Borrowed(s) => (Cow::Borrowed(s), false),
        Cow::Owned(s) => (Cow::Owned(s), true),
    }
}

fn decode_utf16(bytes: &[u8], read: fn([u8; 2]) -> u16) -> (std::borrow::Cow<'_, str>, bool) {
    let units = bytes.as_chunks::<2>().0.iter().map(|c| read(*c));
    let mut invalid = !bytes.len().is_multiple_of(2);
    let text: String = char::decode_utf16(units)
        .map(|r| {
            r.unwrap_or_else(|_| {
                invalid = true;
                char::REPLACEMENT_CHARACTER
            })
        })
        .collect();
    (text.into(), invalid)
}

/// How many temporary names are tried before giving up when each is already taken.
const TEMP_TRIES: u32 = 8;

/// Creates `path` holding `bytes`, and refuses if the name is taken, so no file is ever replaced.
///
/// The bytes go to a temporary name beside the target first — `name.part`, or a name with the
/// process id in it when that one is already taken — and are flushed and synced to the disk
/// there. Only then is the temporary renamed onto the target, so a crash or a power cut leaves
/// either no file at the target or the whole of it, never half of it. The temporary is removed
/// on every way out of this function, a refusal included; a process killed while it writes
/// leaves its `.part` behind, as nothing of ours runs then, and the next attempt takes the next
/// name rather than that one.
///
/// When the name is taken it returns [`io::ErrorKind::AlreadyExists`], and the file that is
/// there is not opened, changed or read.
///
/// The last step is a check that the name is still free and then a rename, and the pair is not
/// atomic against another process creating the same name at that instant: a file that appears
/// between the check and the rename is replaced by this one, on Windows and on Unix alike. No
/// standard-library call closes that window on every platform — a hard link does, but only on
/// file systems that have them — so two programs that choose the same new name at the same
/// moment may lose one of the two files. Two calls of this function in one process cannot
/// collide with each other: each writes a temporary of its own.
///
/// The folder is not made: a missing parent is [`io::ErrorKind::NotFound`]. The bytes are
/// written as given, with no BOM added and no line ending changed.
pub fn create_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    // Refuse before writing anything, so a taken name costs no temporary file at all.
    match std::fs::symlink_metadata(path) {
        Ok(_) => return Err(already_exists(path)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let (mut file, temp) = create_temporary(path)?;
    let written = file
        .write_all(bytes)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all());
    drop(file);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    // The real refusal: the check above is only an early one, and the name may have been taken
    // while the bytes were being written.
    match std::fs::symlink_metadata(path) {
        Ok(_) => {
            let _ = std::fs::remove_file(&temp);
            return Err(already_exists(path));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            let _ = std::fs::remove_file(&temp);
            return Err(e);
        }
    }
    if let Err(e) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    // The rename itself, on systems where a folder can be synced.
    #[cfg(unix)]
    if let Some(dir) = path.parent()
        && let Ok(d) = std::fs::File::open(dir)
    {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Writes `bytes` over the file at `path`, or makes it when it is not there: the write for a
/// file of the owner's (a rewrite, its undo, a library file, a moved note), where
/// [`crate::settings::write_atomic`] is the write for the app's own settings files.
///
/// The bytes go to a temporary name of this function's own beside the file (`name.part`, or a
/// name with the process id in it), made with `create_new`, so a file of the owner's called
/// `name.tmp` or `name.part` is never opened, truncated or renamed away. They are flushed and
/// synced, the file's permissions are put on the temporary (so a file only its owner may read
/// stays so, and a script keeps its executable bit), and the temporary is renamed over the
/// file. The temporary is removed on every way out but the last, and a failed rename leaves the
/// file as it was.
///
/// A read-only file is refused ([`io::ErrorKind::PermissionDenied`]), on every system alike: on
/// Windows the rename would fail, and elsewhere it would replace the file anyway. A link is
/// written through: the file it leads to is the one replaced, and the link stays. Anything that
/// is not a file is refused. The folder is not made. A hard link to the file is split by the
/// rename (the other name keeps the old bytes); so are what a rename never carries, a Windows
/// file's own access list and its alternate streams.
pub fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let target = match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path)?,
        Ok(m) if !m.is_file() => return Err(not_a_file(path)),
        Ok(_) => path.to_path_buf(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => path.to_path_buf(),
        Err(e) => return Err(e),
    };
    let old = match std::fs::metadata(&target) {
        Ok(m) if !m.is_file() => return Err(not_a_file(path)),
        Ok(m) => Some(m.permissions()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    if old.as_ref().is_some_and(std::fs::Permissions::readonly) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("the file is read-only: {}", path.display()),
        ));
    }
    let (mut file, temp) = create_temporary(&target)?;
    let written = file
        .write_all(bytes)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all());
    drop(file);
    let done = written
        .and_then(|()| match old {
            Some(permissions) => std::fs::set_permissions(&temp, permissions),
            None => Ok(()),
        })
        .and_then(|()| std::fs::rename(&temp, &target));
    if let Err(e) = done {
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    // The rename itself, on systems where a folder can be synced.
    #[cfg(unix)]
    if let Some(dir) = target.parent()
        && let Ok(d) = std::fs::File::open(dir)
    {
        let _ = d.sync_all();
    }
    Ok(())
}

fn not_a_file(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("it is not a file: {}", path.display()),
    )
}

fn already_exists(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("the file already exists: {}", path.display()),
    )
}

/// Opens a temporary file beside `path` that did not exist, or says why none could be made.
fn create_temporary(path: &Path) -> io::Result<(std::fs::File, PathBuf)> {
    let mut last = None;
    for attempt in 0..TEMP_TRIES {
        let Some(temp) = temporary_name(path, attempt) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the path has no file name to write the temporary beside",
            ));
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => return Ok((file, temp)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "no temporary name beside the file was free",
        )
    }))
}

/// `name.part`, then a name carrying the process id, so two writers do not take each other's.
fn temporary_name(path: &Path, attempt: u32) -> Option<PathBuf> {
    let mut name = path.file_name()?.to_os_string();
    if attempt == 0 {
        name.push(".part");
    } else {
        name.push(format!(".{}.{attempt}.part", std::process::id()));
    }
    Some(path.with_file_name(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_strips_a_bom_and_reads_utf16() {
        assert_eq!(decode(b"\xEF\xBB\xBFhi"), ("hi".into(), false));
        assert_eq!(decode(b"\xFF\xFEh\0i\0"), ("hi".into(), false));
        assert_eq!(decode(b"\xFE\xFF\0h\0i"), ("hi".into(), false));
        assert!(decode(b"a\xFFb").1);
    }

    #[test]
    fn read_stable_enforces_the_limit() {
        let dir = std::env::temp_dir().join(format!("mdshared-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.txt");
        std::fs::write(&path, "12345").unwrap();
        assert_eq!(read_stable(&path, 5).unwrap().bytes, b"12345");
        assert_eq!(read_stable(&path, 4).err(), Some(LoadError::TooLarge));
        assert!(matches!(
            read_stable(&dir.join("gone.txt"), 5),
            Err(LoadError::Io(_))
        ));
        _ = std::fs::remove_dir_all(&dir);
    }

    /// A folder of this test's own, so tests running at the same time do not share files.
    fn scratch(label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "mdshared-create-new-{}-{n}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The names directly in `dir`, sorted, to show what was left behind.
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn create_new_writes_the_file() {
        let dir = scratch("writes");
        let path = dir.join("note.md");
        create_new(&path, b"one\ntwo\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"one\ntwo\n");
        assert_eq!(names(&dir), vec!["note.md"]);
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_new_refuses_a_taken_name_without_touching_the_file_there() {
        let dir = scratch("taken");
        let path = dir.join("note.md");
        std::fs::write(&path, "the one that is there").unwrap();
        let err = create_new(&path, b"the new one").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&path).unwrap(), b"the one that is there");
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_new_leaves_no_temporary_file_behind_on_either_path() {
        let dir = scratch("temp");
        let free = dir.join("free.md");
        create_new(&free, b"x").unwrap();
        let taken = dir.join("taken.md");
        std::fs::write(&taken, "kept").unwrap();
        assert_eq!(
            create_new(&taken, b"y").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(names(&dir), vec!["free.md", "taken.md"]);
        _ = std::fs::remove_dir_all(&dir);
    }

    /// Audit 4, D1 (audit 3, C9): a file of the owner's called `<name>.tmp`, or `<name>.part`,
    /// beside the one replaced is never touched, and nothing is left behind.
    #[test]
    fn replace_leaves_the_owners_tmp_and_part_files_alone() {
        let dir = scratch("replace-tmp");
        let path = dir.join("notes.txt");
        std::fs::write(&path, "old\n").unwrap();
        std::fs::write(dir.join("notes.txt.tmp"), "the owner's tmp\n").unwrap();
        std::fs::write(dir.join("notes.txt.part"), "the owner's part\n").unwrap();
        replace(&path, b"new\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new\n");
        assert_eq!(
            std::fs::read(dir.join("notes.txt.tmp")).unwrap(),
            b"the owner's tmp\n"
        );
        assert_eq!(
            std::fs::read(dir.join("notes.txt.part")).unwrap(),
            b"the owner's part\n"
        );
        assert_eq!(
            names(&dir),
            vec!["notes.txt", "notes.txt.part", "notes.txt.tmp"]
        );
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_makes_a_file_that_is_not_there() {
        let dir = scratch("replace-new");
        let path = dir.join("new.md");
        replace(&path, b"x").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"x");
        assert_eq!(names(&dir), vec!["new.md"]);
        _ = std::fs::remove_dir_all(&dir);
    }

    /// A read-only file is refused and left as it was, with no temporary beside it: on Windows
    /// the rename fails, and the old write left its `.tmp` there (audit 4, B6).
    #[test]
    fn replace_refuses_a_read_only_file_and_leaves_nothing_behind() {
        let dir = scratch("replace-ro");
        let path = dir.join("locked.md");
        std::fs::write(&path, "kept\n").unwrap();
        let mut p = std::fs::metadata(&path).unwrap().permissions();
        p.set_readonly(true);
        std::fs::set_permissions(&path, p).unwrap();
        let err = replace(&path, b"lost\n").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        // It says so in its own words, whatever the system's refusal would have said.
        assert!(err.to_string().contains("read-only"), "{err}");
        assert_eq!(std::fs::read(&path).unwrap(), b"kept\n");
        assert_eq!(names(&dir), vec!["locked.md"]);
        let mut p = std::fs::metadata(&path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        p.set_readonly(false);
        std::fs::set_permissions(&path, p).unwrap();
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_refuses_a_folder() {
        let dir = scratch("replace-dir");
        let sub = dir.join("sub.md");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(
            replace(&sub, b"x").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(sub.is_dir());
        _ = std::fs::remove_dir_all(&dir);
    }

    /// Audit 4, W3: the file keeps its mode, so a note only its owner may read stays so and a
    /// script keeps its executable bit.
    #[cfg(unix)]
    #[test]
    fn replace_keeps_the_files_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("replace-mode");
        for mode in [0o600, 0o755] {
            let path = dir.join(format!("f{mode:o}"));
            std::fs::write(&path, "old").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            replace(&path, b"new").unwrap();
            let now = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(now, mode);
        }
        _ = std::fs::remove_dir_all(&dir);
    }

    /// A link is written through: the file it leads to gets the bytes and the link stays.
    #[cfg(unix)]
    #[test]
    fn replace_writes_through_a_link() {
        let dir = scratch("replace-link");
        let real = dir.join("real.md");
        std::fs::write(&real, "old").unwrap();
        let link = dir.join("link.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        replace(&link, b"new").unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&real).unwrap(), b"new");
        _ = std::fs::remove_dir_all(&dir);
    }
}
