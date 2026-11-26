//! Opens a local asset without waiting for a Unix FIFO writer. The caller checks the opened
//! handle, not a name that could have been replaced between a check and the open.

use std::fs::File;
use std::path::Path;

pub(super) fn open(path: &Path) -> Option<(File, u64)> {
    let mut options = File::options();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nonblock()?);
    }
    let file = options.open(path).ok()?;
    let meta = file.metadata().ok()?;
    meta.is_file().then_some((file, meta.len()))
}

/// The platform ABI's `O_NONBLOCK`. Rust's `OpenOptionsExt` takes the raw flag.
/// On an unknown Unix ABI, do not fall back to a potentially blocking open.
#[cfg(unix)]
fn nonblock() -> Option<i32> {
    if cfg!(any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd"
    )) {
        Some(4)
    } else if cfg!(any(target_os = "linux", target_os = "android")) {
        if cfg!(any(
            target_arch = "mips",
            target_arch = "mips64",
            target_arch = "mips32r6",
            target_arch = "mips64r6"
        )) {
            Some(128)
        } else if cfg!(any(target_arch = "sparc", target_arch = "sparc64")) {
            Some(0x4000)
        } else {
            Some(0x800)
        }
    } else {
        None
    }
}

#[cfg(all(test, unix))]
#[test]
fn a_fifo_without_a_writer_is_rejected_without_waiting() {
    use std::process::Command;
    use std::sync::mpsc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let path = std::env::temp_dir().join(format!(
        "mdshared-image-fifo-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    assert!(
        Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let (send, receive) = mpsc::channel();
    let test_path = path.clone();
    let worker = std::thread::spawn(move || send.send(super::read(&test_path).is_none()));
    let result = receive.recv_timeout(Duration::from_secs(3));
    if result.is_err() {
        // Release a worker stuck in the old blocking open before failing the test.
        use std::os::unix::fs::OpenOptionsExt;
        let release = File::options()
            .read(true)
            .write(true)
            .custom_flags(nonblock().unwrap())
            .open(&path)
            .unwrap();
        drop(release);
    }
    std::fs::remove_file(path).unwrap();
    assert_eq!(result, Ok(true));
    worker.join().unwrap().unwrap();
}
