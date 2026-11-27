//! Scratch directories for the loopback tests. Retire listeners before removing their
//! endpoint directories: Windows pins a directory until the listener actually exits.

use super::*;

pub(super) fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("mdshared-{name}-{}", std::process::id()));
    match fs::remove_dir_all(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => panic!("cannot clear {}: {e}", path.display()),
    }
    #[cfg(windows)]
    record::create_legacy_fixture_dir(&path).unwrap();
    #[cfg(not(windows))]
    fs::create_dir(&path).unwrap();
    path
}

pub(super) fn retire<T>(path: PathBuf, rx: Receiver<T>) {
    drop(rx);
    let settings = path.join("settings.ini");
    let probe = path.join("retire.md");
    fs::write(&probe, b"retire").unwrap();
    assert_eq!(
        deliver_request(APP, &settings, &Request::new(probe)),
        Delivery::Nobody,
        "the disconnected receiver cannot acknowledge"
    );
    assert!(wait_gone(&settings, READ_DEADLINE * 2));
    remove(path);
}

pub(super) fn remove(path: PathBuf) {
    #[cfg(unix)]
    {
        let endpoint = private_dir(&path.join("settings.ini"));
        if !endpoint.starts_with(&path)
            && let Err(e) = fs::remove_dir_all(&endpoint)
        {
            assert_eq!(e.kind(), io::ErrorKind::NotFound, "{}", endpoint.display());
        }
    }
    let end = Instant::now() + READ_DEADLINE;
    loop {
        match fs::remove_dir_all(&path) {
            Ok(()) => return,
            // The registration lock can go away just before the pinned directory closes.
            Err(e)
                if cfg!(windows)
                    && matches!(e.raw_os_error(), Some(5 | 32))
                    && retry(end, &mut std::thread::sleep) => {}
            Err(e) => panic!("cannot remove {}: {e}", path.display()),
        }
    }
}

#[test]
fn a_loopback_fixture_clears_stale_state_and_releases_its_directory() {
    let name = "reusable-loopback-fixture";
    let path = scratch(name);
    let stale = path.join("stale");
    fs::write(&stale, b"old").unwrap();
    assert_eq!(scratch(name), path);
    assert!(!stale.exists());
    let settings = path.join("settings.ini");
    let rx = listen_requests_waking(APP, &settings, || {}).unwrap();
    let request = Request::document("current", "");
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Taken);
    assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap().doc, request.doc);
    retire(path.clone(), rx);
    assert!(!path.exists());
}

#[cfg(windows)]
#[test]
fn a_legacy_record_can_grant_other_users_read_access_but_not_write_access() {
    let path = scratch("legacy-acl-grants");
    let settings = path.join("settings.ini");
    let port = path.join("instance.port");
    let (listener, lock) = legacy_start(&path);
    let grant = |access| {
        let output = std::process::Command::new("icacls.exe")
            .arg(&port)
            .arg("/grant")
            .arg(access)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    let request = Request::document("legacy", "");
    grant("*S-1-5-32-545:(R)"); // Builtin Users.
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Older);
    grant("*S-1-5-32-545:(W)");
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Nobody);
    drop((listener, lock));
    remove(path);
}

#[cfg(unix)]
#[test]
fn a_group_writable_legacy_record_is_probed_only_in_owner_only_settings() {
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", "umask 0002; exec \"$@\"", "sh"])
        .arg(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "single::overlap_tests::fixture::probe_a_legacy_record_created_under_umask_0002",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{stdout}"
    );
}

#[cfg(unix)]
#[test]
#[ignore = "helper run with umask 0002 by the legacy permissions test"]
fn probe_a_legacy_record_created_under_umask_0002() {
    use std::os::unix::fs::PermissionsExt;
    let ordinary = scratch("ordinary-legacy-under-umask");
    let listener = legacy_start(&ordinary);
    for name in ["instance.port", "instance.lock"] {
        assert_eq!(
            fs::metadata(ordinary.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    drop(listener);
    remove(ordinary);

    let path = scratch("legacy-group-writable");
    let settings = path.join("settings.ini");
    let (listener, lock) = legacy_start_with_umask(&path);
    let mode = fs::metadata(path.join("instance.port"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o664);
    let file = path.join("note.md");
    fs::write(&file, b"note").unwrap();
    let request = Request {
        text: Some("not for the legacy socket".into()),
        ..Request::new(file)
    };

    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Nobody);
    assert_eq!(
        accept_legacy_probe(&listener).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Older);
    assert!(!legacy_predecessor(&settings));
    let mut probe = accept_legacy_probe(&listener).unwrap();
    probe.set_read_timeout(Some(CONNECT)).unwrap();
    assert_eq!(probe.read(&mut [0; 1]).unwrap(), 0, "no payload");
    drop((listener, lock));
    remove(path);
}
