//! One fresh key per listener. Publish a complete, private file by rename; never truncate
//! an existing file (it might be an older build's world-readable port file or a link).

use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Write};
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use std::path::Path;

#[cfg(windows)]
#[path = "private_windows.rs"]
mod os;

#[cfg(unix)]
#[path = "private_unix.rs"]
mod os;

pub(super) use os::Directory;
#[cfg(all(test, windows))]
pub(super) use os::create_legacy_fixture_dir;
pub(super) use os::legacy_file;
pub(super) use os::legacy_locked;
#[cfg(unix)]
pub(super) use os::legacy_probe_file;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use os::{endpoint_path, open_endpoint};

pub(super) const LOCK: &str = "instance.lock";
pub(super) const PORT: &str = "instance.port";
const GATE: &str = "publishing.lock";

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn endpoint_path(settings: &Path) -> std::path::PathBuf {
    settings.join("instance-v2")
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn open_endpoint(settings: &Path, create: bool) -> Option<Directory> {
    Directory::open(&endpoint_path(settings), create)
}

pub(super) fn lock(dir: &Directory, create: bool) -> Option<File> {
    named_lock(dir, LOCK, create)
}

pub(super) fn gate(dir: &Directory, create: bool) -> Option<File> {
    named_lock(dir, GATE, create)
}

fn named_lock(dir: &Directory, base: &str, create: bool) -> Option<File> {
    let name = OsStr::new(base);
    let file = match dir.open_lock(name) {
        Err(e) if create && e.kind() == std::io::ErrorKind::NotFound => {
            dir.create_file(name).or_else(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    dir.open_lock(name)
                } else {
                    Err(e)
                }
            })
        }
        other => other,
    }
    .ok()?;
    os::private(&file).then_some(file)
}

pub(super) fn random() -> Option<[u8; 32]> {
    os::random()
}

pub(super) struct Record {
    pub port: u16,
    pub key: [u8; 32],
}

impl Record {
    pub fn publish(dir: &Directory, port: u16) -> Option<Self> {
        let key = random()?;
        // The temporary name is independent of the key, even in a listable folder.
        let nonce = random()?;
        let tmp = format!(
            ".instance-{:032x}",
            u128::from_ne_bytes(nonce[..16].try_into().ok()?)
        );
        let tmp = OsStr::new(&tmp);
        let mut file = dir.create_file(tmp).ok()?;
        let result = (|| {
            if !os::private(&file) {
                return None;
            }
            file.write_all(&port.to_be_bytes()).ok()?;
            file.write_all(&key).ok()?;
            drop(file);
            dir.rename(tmp, OsStr::new(PORT)).then_some(())?;
            Some(Self { port, key })
        })();
        if result.is_none() {
            dir.remove(tmp);
        }
        result
    }

    pub fn load(dir: &Directory) -> Option<Self> {
        let mut file = dir.read_file(OsStr::new(PORT)).ok()?;
        if !os::private(&file) || file.metadata().ok()?.len() != 34 {
            return None;
        }
        let mut port = [0; 2];
        let mut key = [0; 32];
        file.read_exact(&mut port).ok()?;
        file.read_exact(&mut key).ok()?;
        let port = u16::from_be_bytes(port);
        (port != 0).then_some(Self { port, key })
    }
}

#[cfg(all(test, target_os = "macos"))]
#[test]
fn an_inherited_macos_read_grant_defeats_mode_0600_and_the_record_is_refused() {
    use std::fs::{self, DirBuilder};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    use std::process::Command;

    let path = std::env::temp_dir().join(format!("mdshared-acl-{}", std::process::id()));
    DirBuilder::new().mode(0o700).create(&path).unwrap();
    let dir = Directory::open(&path, false).unwrap();
    // Apply the grant after opening the directory to exercise both file checks even though
    // a subsequent directory lookup also rejects this ACL. Only new files inherit the grant.
    assert!(
        Command::new("/bin/chmod")
            .args(["+a", "group:everyone allow read,file_inherit,only_inherit"])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    assert!(Record::publish(&dir, 12345).is_none());
    assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
    let mut file = File::options()
        .mode(0o600)
        .write(true)
        .create_new(true)
        .open(path.join(PORT))
        .unwrap();
    file.write_all(&[1; 34]).unwrap();
    assert_eq!(file.metadata().unwrap().mode() & 0o777, 0o600);
    assert!(Record::load(&dir).is_none());
    fs::remove_dir_all(path).unwrap();
}
