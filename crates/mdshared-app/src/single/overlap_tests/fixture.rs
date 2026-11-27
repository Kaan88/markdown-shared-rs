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
    #[cfg(unix)]
    create_private(&path);
    path
}

#[cfg(unix)]
fn create_private(path: &Path) {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(0o700).create(path).unwrap();
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

#[test]
fn dropping_a_guarded_receiver_allows_immediate_relisten_without_sending() {
    let path = scratch("guarded-relisten");
    let settings = path.join("settings.ini");
    let first = listen_waking_guarded(APP, &settings, || {}).unwrap();
    assert!(listen_guarded(APP, &settings).is_none());
    drop(first);
    let second = listen_guarded(APP, &settings).expect("drop released the lock");
    let file = path.join("note.md");
    fs::write(&file, b"note").unwrap();
    assert!(forward(APP, &settings, std::slice::from_ref(&file)));
    assert_eq!(second.recv_timeout(READ_DEADLINE).unwrap(), file);
    drop(second);
    remove(path);
}

#[test]
fn dropping_a_guarded_receiver_releases_the_gate_before_deferred_publication() {
    let path = scratch("guarded-deferred-relisten");
    let settings = path.join("settings.ini");
    let legacy = legacy_start(&path);
    #[cfg(unix)]
    legacy.1.try_lock().unwrap();
    assert!(legacy_predecessor(&settings));
    let first = listen_requests_waking_guarded(APP, &settings, || {}).unwrap();
    assert!(!port_file(&settings).exists());
    drop(first);
    let second = listen_unresolved_requests_waking_guarded(APP, &settings, || {})
        .expect("drop released the gate");
    drop(legacy);
    let request = Request::document("replacement", "");
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Taken);
    assert_eq!(second.recv_timeout(READ_DEADLINE).unwrap().doc, request.doc);
    drop(second);
    remove(path);
}

#[test]
fn discovery_opens_the_gate_and_lifetime_lock_read_only() {
    let path = scratch("discovery-read-only");
    let settings = path.join("settings.ini");
    let dir = directory(&settings, true).unwrap();
    let (held, _) = hold_lock(&dir, |_| unreachable!())
        .unwrap()
        .publish(&dir, 12345)
        .unwrap();
    for mut lock in [
        record::gate(&dir, false).unwrap(),
        record::lock(&dir, false).unwrap(),
    ] {
        assert!(lock.write_all(b"cannot write a discovery handle").is_err());
    }
    assert!(matches!(discovery(&settings), Discovery::Ready(record) if record.port == 12345));
    drop((held, dir));
    remove(path);
}

#[test]
fn a_refused_second_batch_does_not_deliver_the_first_batch() {
    let path = scratch("refused-selection");
    let settings = path.join("settings.ini");
    let rx = listen_unresolved_requests_waking(APP, &settings, || {}).unwrap();
    let mut base = path.clone();
    for _ in 0..15 {
        base.push("x".repeat(200));
    }
    // The first 256 paths fit under 1 MiB. Reading the second batch crosses the
    // total selection cap, even though both batches would fit separately.
    let paths: Vec<_> = (0..400).map(|i| base.join(format!("{i}.md"))).collect();
    assert!(!forward(APP, &settings, &paths));
    let next = Request::new(path.join("next.md"));
    assert_eq!(deliver_request(APP, &settings, &next), Delivery::Taken);
    assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap(), next);
    assert!(rx.try_recv().is_err());
    retire(path, rx);
}

#[test]
fn forwarding_five_thousand_short_paths_fits_a_single_selection() {
    let path = scratch("short-selection");
    let settings = path.join("settings.ini");
    let rx = listen_unresolved_requests_waking(APP, &settings, || {}).unwrap();
    let paths: Vec<_> = (0..5_000).map(|i| path.join(i.to_string())).collect();
    assert!(forward(APP, &settings, &paths));
    for expected in paths {
        assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap().path, expected);
    }
    assert!(rx.try_recv().is_err());
    retire(path, rx);
}

#[cfg(unix)]
#[test]
fn forward_preserves_a_carriage_return_inside_a_unix_filename() {
    let path = scratch("path-with-carriage-return");
    let settings = path.join("settings.ini");
    let file = path.join("still\rhere.md");
    fs::write(&file, b"note").unwrap();
    let rx = listen(APP, &settings).unwrap();
    assert!(forward(APP, &settings, std::slice::from_ref(&file)));
    assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap(), file);
    retire(path, rx);
}

#[cfg(target_os = "linux")]
const REGISTRATION_CHILD: &str = "MDSHARED_TEST_REGISTRATION_CHILD";

#[cfg(target_os = "linux")]
#[test]
fn discovery_and_contention_ignore_xdg_runtime_dir() {
    let path = scratch("registration-environment");
    let settings = path.join("new-directory/settings.ini");
    let runtime = path.join("runtime");
    drop(Directory::open(&runtime, true).unwrap());
    let recorded = port_file(&settings);
    assert!(!recorded.exists());
    let rx = listen_requests_waking(APP, &settings, || {}).unwrap();
    assert_eq!(port_file(&settings), recorded);
    assert!(recorded.is_file());
    assert!(recorded.starts_with(settings.parent().unwrap()));
    let child = || {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "single::overlap_tests::fixture::probe_registration_from_another_environment",
            ])
            .env_remove(REGISTRATION_CHILD);
        command
    };
    assert!(
        child().output().unwrap().status.success(),
        "unflagged helper is inert"
    );
    for with_runtime in [true, false] {
        let mut command = child();
        command
            .env(REGISTRATION_CHILD, "1")
            .env("MDSHARED_TEST_SETTINGS", &settings)
            .env("MDSHARED_TEST_PORT", &recorded);
        if with_runtime {
            command.env("XDG_RUNTIME_DIR", &runtime);
        } else {
            command.env_remove("XDG_RUNTIME_DIR");
        }
        let output = command.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "{stdout}"
        );
        assert_eq!(
            rx.recv_timeout(READ_DEADLINE).unwrap().doc.as_deref(),
            Some("env-probe")
        );
    }
    assert!(fs::read_dir(runtime).unwrap().next().is_none());
    retire(settings.parent().unwrap().to_path_buf(), rx);
    remove(path);
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "run in child processes with different environments by the registration test"]
fn probe_registration_from_another_environment() {
    if std::env::var_os(REGISTRATION_CHILD).as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let settings = PathBuf::from(std::env::var_os("MDSHARED_TEST_SETTINGS").unwrap());
    let recorded = PathBuf::from(std::env::var_os("MDSHARED_TEST_PORT").unwrap());
    assert_eq!(port_file(&settings), recorded);
    assert!(alive(&settings));
    assert!(listen(APP, &settings).is_none());
    assert_eq!(
        deliver_request(APP, &settings, &Request::document("env-probe", "")),
        Delivery::Taken
    );
}

#[cfg(unix)]
#[test]
fn a_readable_legacy_record_or_lock_is_not_authority_to_delay_publication() {
    use std::os::unix::fs::PermissionsExt;
    let path = scratch("legacy-readable-not-private");
    let settings = path.join("settings.ini");
    let port = path.join("instance.port");
    let (listener, lock) = legacy_start(&path);
    lock.try_lock().unwrap();
    let mode = fs::Permissions::from_mode;
    assert!(legacy_predecessor(&settings));
    fs::set_permissions(&port, mode(0o644)).unwrap();
    assert!(alive(&settings));
    assert!(
        !legacy_predecessor(&settings),
        "a readable port is only a hint"
    );
    fs::set_permissions(&port, mode(0o600)).unwrap();
    lock.set_permissions(mode(0o644)).unwrap();
    assert!(alive(&settings));
    assert!(
        !legacy_predecessor(&settings),
        "a readable lock is not authority"
    );
    let rx = listen_requests_waking(APP, &settings, || {}).unwrap();
    assert!(
        port_file(&settings).is_file(),
        "publication was not deferred"
    );
    drop((listener, lock));
    retire(path, rx);
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

#[cfg(windows)]
#[test]
fn a_readable_windows_legacy_lock_needs_a_live_port_and_never_defers() {
    use std::os::windows::fs::OpenOptionsExt;

    let path = scratch("legacy-readable-lock");
    let settings = path.join("settings.ini");
    let lock_path = path.join("instance.lock");
    let (listener, lock) = legacy_start(&path);
    drop(lock);
    let output = std::process::Command::new("icacls.exe")
        .arg(&lock_path)
        .args(["/grant", "*S-1-5-32-545:(R)"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lock = File::options()
        .read(true)
        .share_mode(0)
        .open(&lock_path)
        .unwrap();

    let request = Request::document("legacy", "");
    assert!(!legacy_predecessor(&settings));
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Older);
    listener.set_nonblocking(true).unwrap();
    let (mut probe, _) = listener.accept().unwrap();
    probe.set_nonblocking(false).unwrap();
    probe.set_read_timeout(Some(CONNECT)).unwrap();
    assert_eq!(probe.read(&mut [0; 1]).unwrap(), 0, "no payload");
    drop((probe, listener));
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Nobody);
    let rx = listen_requests_waking(APP, &settings, || {}).unwrap();
    assert!(
        port_file(&settings).is_file(),
        "publication was not deferred"
    );
    drop(lock);
    retire(path, rx);
}

#[cfg(unix)]
const LEGACY_UMASK_CHILD: &str = "MDSHARED_TEST_LEGACY_UMASK_CHILD";

#[cfg(unix)]
#[test]
fn a_group_writable_legacy_record_is_probed_only_in_owner_only_settings() {
    let root = scratch("legacy-umask");
    create_private(&root.join("ordinary"));
    create_private(&root.join("group-writable"));
    fs::write(root.join("group-writable/note.md"), b"note").unwrap();
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", "umask 0002; exec \"$@\"", "sh"])
        .arg(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "single::overlap_tests::fixture::probe_a_legacy_record_created_under_umask_0002",
        ])
        .env(LEGACY_UMASK_CHILD, "1")
        .env("MDSHARED_TEST_LEGACY_UMASK_ROOT", &root)
        .output()
        .unwrap();
    remove(root);
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
    if std::env::var_os(LEGACY_UMASK_CHILD).as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let root = PathBuf::from(std::env::var_os("MDSHARED_TEST_LEGACY_UMASK_ROOT").unwrap());
    let ordinary = root.join("ordinary");
    let path = root.join("group-writable");
    for dir in [&ordinary, &path] {
        assert_eq!(
            fs::metadata(dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
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
    let settings = path.join("settings.ini");
    let (listener, lock) = legacy_start_with_umask(&path);
    let mode = fs::metadata(path.join("instance.port"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o664);
    let file = path.join("note.md");
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
}
