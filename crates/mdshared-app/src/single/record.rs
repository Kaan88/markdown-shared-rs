//! One fresh key per listener. Publish a complete, private file by rename; never truncate
//! an existing file (it might be an older build's world-readable port file or a link).
//! On Linux, a shared settings directory contains separate registrations for each hostname.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

#[cfg(windows)]
#[path = "private_windows.rs"]
mod os;

#[cfg(unix)]
#[path = "private_unix.rs"]
mod os;

pub(super) use os::legacy_file;
pub(super) use os::legacy_locked;
#[cfg(unix)]
pub(super) use os::legacy_probe_file;

pub(super) const LOCK: &str = "instance.lock";
pub(super) const PORT: &str = "instance.port";
const GATE: &str = "publishing.lock";

pub(super) struct Directory {
    inner: os::Directory,
    host: &'static str,
}

impl Directory {
    pub(super) fn open(path: &Path, create: bool) -> Option<Self> {
        Self::for_host(path, create, local_host()?)
    }

    fn for_host(path: &Path, create: bool, host: &'static str) -> Option<Self> {
        Some(Self {
            inner: os::Directory::open(path, create)?,
            host,
        })
    }
}

#[cfg(target_os = "linux")]
fn local_host() -> Option<&'static str> {
    use std::sync::LazyLock;
    static HOST: LazyLock<Option<String>> = LazyLock::new(|| os::hostname().map(encode_host));
    HOST.as_deref()
}

#[cfg(not(target_os = "linux"))]
fn local_host() -> Option<&'static str> {
    Some("")
}

#[cfg(target_os = "linux")]
fn encode_host(bytes: Vec<u8>) -> String {
    use std::fmt::Write;
    let mut name = String::new();
    for b in bytes {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
            name.push(char::from(b));
        } else {
            // Escape the escape character too, so two hostnames cannot share a filename.
            _ = write!(name, "%{b:02x}");
        }
    }
    name
}

fn name_for(base: &'static str, host: &str) -> Cow<'static, str> {
    if host.is_empty() {
        Cow::Borrowed(base)
    } else {
        Cow::Owned(format!("{base}.{host}"))
    }
}

pub(super) fn local_name(base: &'static str) -> Cow<'static, str> {
    // Keep the path accessor infallible, but do not open a registration without a hostname.
    name_for(base, local_host().unwrap_or_default())
}

pub(super) fn lock(dir: &Directory, create: bool) -> Option<File> {
    named_lock(dir, LOCK, create)
}

pub(super) fn gate(dir: &Directory, create: bool) -> Option<File> {
    named_lock(dir, GATE, create)
}

fn named_lock(dir: &Directory, base: &'static str, create: bool) -> Option<File> {
    let name = name_for(base, dir.host);
    let name = OsStr::new(name.as_ref());
    let dir = &dir.inner;
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
        let port_name = name_for(PORT, dir.host);
        let dir = &dir.inner;
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
            dir.rename(tmp, OsStr::new(port_name.as_ref()))
                .then_some(())?;
            Some(Self { port, key })
        })();
        if result.is_none() {
            dir.remove(tmp);
        }
        result
    }

    pub fn load(dir: &Directory) -> Option<Self> {
        let port_name = name_for(PORT, dir.host);
        let mut file = dir.inner.read_file(OsStr::new(port_name.as_ref())).ok()?;
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

#[cfg(all(test, target_os = "linux"))]
#[test]
fn hosts_sharing_a_settings_directory_have_independent_registrations() {
    use crate::single::registration::{Discovery, discover, hold_lock};

    let path = std::env::temp_dir().join(format!("mdshared-host-scopes-{}", std::process::id()));
    let first = Directory::for_host(&path, true, "host-one").unwrap();
    let second = Directory::for_host(&path, false, "host-two").unwrap();
    let same = Directory::for_host(&path, false, "host-one").unwrap();
    let no_wait = |_| panic!("the other host cannot block publication");
    let pending = hold_lock(&first, no_wait).unwrap();
    assert!(matches!(discover(&same), Discovery::Starting));
    assert!(matches!(discover(&second), Discovery::Absent));
    let (b, _) = hold_lock(&second, no_wait)
        .unwrap()
        .publish(&second, 12346)
        .unwrap();
    let (a, _) = pending.publish(&first, 12345).unwrap();
    assert!(hold_lock(&same, no_wait).is_none());
    assert!(matches!(discover(&same), Discovery::Ready(record) if record.port == 12345));
    assert!(matches!(discover(&second), Discovery::Ready(record) if record.port == 12346));
    assert_eq!(encode_host(b"a/b%".to_vec()), "a%2fb%25");
    drop((a, b, first, second, same));
    std::fs::remove_dir_all(path).unwrap();
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
        .open(path.join(local_name(PORT).as_ref()))
        .unwrap();
    file.write_all(&[1; 34]).unwrap();
    assert_eq!(file.metadata().unwrap().mode() & 0o777, 0o600);
    assert!(Record::load(&dir).is_none());
    fs::remove_dir_all(path).unwrap();
}
