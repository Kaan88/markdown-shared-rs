//! Reading files from disk: change stamps, a read that waits out a half-finished save, and text
//! decoding.

use std::fmt;
use std::io::{self, Read};
use std::path::Path;
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
}
