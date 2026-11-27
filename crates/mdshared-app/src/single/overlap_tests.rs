use super::*;

#[path = "overlap_tests/fixture.rs"]
mod fixture;
use fixture::{remove, retire, scratch};

const APP: &str = "test-app";

// The old version wrote its decimal port BEFORE opening the lock and ignored a failed lock.
// On Unix it opened the file but did not acquire an advisory lock.
fn legacy_start(path: &Path) -> (TcpListener, File) {
    let (listener, lock) = legacy_start_with_umask(path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            path.join("instance.port"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        lock.set_permissions(fs::Permissions::from_mode(0o600))
            .unwrap();
    }
    (listener, lock)
}

fn legacy_start_with_umask(path: &Path) -> (TcpListener, File) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    fs::write(
        path.join("instance.port"),
        listener.local_addr().unwrap().port().to_string(),
    )
    .unwrap();
    let mut options = File::options();
    options.write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let lock = options.open(path.join("instance.lock")).unwrap();
    (listener, lock)
}

#[test]
fn settings_files_in_the_same_directory_contend_for_one_public_listener() {
    let path = scratch("same-settings-dir");
    let (first, second) = (path.join("first.ini"), path.join("second.ini"));
    let rx = listen_requests_waking(APP, &first, || {}).unwrap();
    assert!(listen(APP, &second).is_none());
    let request = Request::document("one", "One");
    assert_eq!(deliver_request(APP, &second, &request), Delivery::Taken);
    assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap().doc, request.doc);
    retire(path, rx);
}

#[test]
fn a_stale_legacy_record_beside_the_real_settings_does_not_delay_startup() {
    let path = scratch("stale-legacy-beside-settings");
    let settings = path.join("settings.ini");
    drop(legacy_start(&path));
    let started = Instant::now();
    let request = Request::new(path.join("note.md"));
    assert!(!forward(
        APP,
        &settings,
        std::slice::from_ref(&request.path)
    ));
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Nobody);
    if cfg!(windows) {
        assert!(started.elapsed() < CONNECT / 2, "{:?}", started.elapsed());
    }
    let rx = listen(APP, &settings).unwrap();
    fs::write(&request.path, b"note").unwrap();
    assert!(forward(APP, &settings, std::slice::from_ref(&request.path)));
    assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap(), request.path);
    retire(path, rx);
}

#[test]
fn a_legacy_only_endpoint_reports_older_for_an_existing_file_without_receiving_it() {
    let path = scratch("legacy-file");
    let settings = path.join("settings.ini");
    let (listener, lock) = legacy_start(&path);
    let file = path.join("note.md");
    fs::write(&file, b"note").unwrap();
    assert_eq!(
        deliver_request(APP, &settings, &Request::new(&file)),
        Delivery::Older
    );
    assert!(!forward(APP, &settings, &[file]));
    listener.set_nonblocking(true).unwrap();
    let mut probes = 0;
    while let Ok((mut stream, _)) = listener.accept() {
        stream.set_nonblocking(false).unwrap();
        stream.set_read_timeout(Some(CONNECT)).unwrap();
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0, "no payload");
        probes += 1;
    }
    if cfg!(unix) {
        assert!(probes != 0, "the actual legacy socket was probed");
    }
    drop((listener, lock));
    remove(path);
}

#[test]
fn legacy_first_is_detected_without_payload_and_registration_waits_for_exit() {
    let path = scratch("legacy-first");
    let settings = path.join("settings.ini");
    let (old_listener, old_lock) = legacy_start(&path);
    // A positively identifiable Unix predecessor must hold an advisory lock, unlike
    // the original lockless legacy build which can only give a liveness hint.
    #[cfg(unix)]
    old_lock.try_lock().unwrap();
    assert!(alive(&settings));
    assert!(!wait_gone(&settings, Duration::from_millis(60)));
    let request = Request {
        text: Some("never send this to an old listener".into()),
        ..Request::new(path.join("private.md"))
    };
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Older);
    assert!(!forward(
        APP,
        &settings,
        std::slice::from_ref(&request.path)
    ));
    let rx = listen_requests_waking(APP, &settings, || {}).expect("deferred listener");
    // Keep the predecessor past the old startup deadline. This same receiver must
    // still become reachable after the predecessor goes away.
    delay(STARTUP + POLL);
    assert!(!port_file(&settings).exists());
    old_listener.set_nonblocking(true).unwrap();
    let mut probes = 0;
    while let Ok((mut stream, _)) = old_listener.accept() {
        stream.set_nonblocking(false).unwrap();
        stream.set_read_timeout(Some(CONNECT)).unwrap();
        assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0, "zero payload bytes");
        probes += 1;
    }
    if cfg!(unix) {
        assert!(
            probes >= 2,
            "alive and delivery checked the legacy endpoint"
        );
    }
    drop((old_listener, old_lock));
    assert!(!legacy_predecessor(&settings));
    fs::write(&request.path, b"note").unwrap();
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Taken);
    assert_eq!(rx.recv_timeout(CONNECT).unwrap().text, request.text);
    retire(path, rx);
}

#[test]
fn the_v2_listener_publishes_at_the_deadline_even_if_the_predecessor_releases_later() {
    let path = scratch("legacy-past-deadline");
    let settings = path.join("settings.ini");
    let legacy = legacy_start(&path);
    #[cfg(unix)]
    legacy.1.try_lock().unwrap();
    assert!(legacy_predecessor(&settings));
    let rx = listen_requests_waking(APP, &settings, || {}).unwrap();
    assert!(matches!(discovery(&settings), Discovery::Starting));
    assert!(
        wait_for_record(
            &settings,
            Instant::now() + LEGACY_DEFER + READ_DEADLINE,
            std::thread::sleep
        )
        .is_some(),
        "v2 publishes even though the predecessor still holds its lock"
    );
    assert!(legacy_predecessor(&settings));
    let first = Request::document("first", "");
    assert_eq!(deliver_request(APP, &settings, &first), Delivery::Taken);
    assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap().doc, first.doc);
    drop(legacy);
    let second = Request::document("second", "");
    assert_eq!(deliver_request(APP, &settings, &second), Delivery::Taken);
    assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap().doc, second.doc);
    retire(path, rx);
}

#[cfg(unix)]
#[test]
fn a_reused_legacy_port_does_not_delay_restart_or_registration() {
    let path = scratch("legacy-reused");
    let settings = path.join("settings.ini");
    let (unrelated, unlocked) = legacy_start(&path);
    drop(unlocked);
    assert!(
        alive(&settings),
        "a bare legacy connection is still a liveness hint"
    );
    assert!(wait_gone(&settings, Duration::from_millis(60)));
    let rx = listen_requests_waking(APP, &settings, || {}).unwrap();
    assert!(
        port_file(&settings).exists(),
        "registration is not deferred"
    );
    let request = Request::document("one", "One");
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Taken);
    assert_eq!(rx.recv_timeout(CONNECT).unwrap().doc, request.doc);
    let mut probe = accept_legacy_probe(&unrelated).expect("a legacy liveness probe was queued");
    probe.set_read_timeout(Some(CONNECT)).unwrap();
    assert_eq!(probe.read(&mut [0; 1]).unwrap(), 0, "no payload");
    drop(unrelated);
    retire(path, rx);
}

#[cfg(unix)]
fn accept_legacy_probe(listener: &TcpListener) -> io::Result<TcpStream> {
    listener.set_nonblocking(true)?;
    let (stream, _) = listener.accept()?;
    stream.set_nonblocking(false)?;
    Ok(stream)
}

#[cfg(unix)]
#[test]
fn a_missing_legacy_probe_fails_without_waiting_for_a_connection() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let (tx, rx) = channel();
    let probe = std::thread::spawn(move || {
        _ = tx.send(accept_legacy_probe(&listener).unwrap_err().kind());
    });
    assert_eq!(
        rx.recv_timeout(READ_DEADLINE).expect("accept cannot block"),
        io::ErrorKind::WouldBlock
    );
    probe.join().unwrap();
}

#[track_caller]
fn assert_same_record(before: &[u8], after: &[u8]) {
    assert!(before == after, "the authenticated record changed");
}

#[test]
fn a_failed_record_comparison_reports_no_record_bytes() {
    let failure = std::panic::catch_unwind(|| assert_same_record(&[254], &[255])).unwrap_err();
    let message = failure
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| failure.downcast_ref::<String>().map(String::as_str));
    assert_eq!(message, Some("the authenticated record changed"));
}

#[test]
fn legacy_second_cannot_overwrite_the_versioned_listener_or_take_its_lock() {
    let path = scratch("legacy-second");
    let settings = path.join("settings.ini");
    let rx = listen_requests_waking(APP, &settings, || {}).unwrap();
    let secure_record = fs::read(port_file(&settings)).unwrap();
    let legacy = legacy_start(&path);
    assert_same_record(&secure_record, &fs::read(port_file(&settings)).unwrap());
    assert!(alive(&settings));
    let request = Request {
        text: Some("only the new listener sees this".into()),
        ..Request::document("one", "One")
    };
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Taken);
    assert_eq!(rx.recv_timeout(CONNECT).unwrap().text, request.text);
    drop(legacy);
    retire(path, rx);
}

#[test]
fn a_passing_probe_does_not_keep_a_new_listener_from_registering() {
    let path = scratch("passing-probe");
    let settings = path.join("settings.ini");
    let dir = directory(&settings, true).unwrap();
    let mut probe = Some(record::lock(&dir, true).unwrap());
    probe.as_ref().unwrap().try_lock_shared().unwrap();
    // Leave a stale record to ensure a published record alone is not mistaken for a listener.
    Record::publish(&dir, 12345).unwrap();
    assert!(instance_record(&settings).is_none());
    let listener_lock = hold_lock(&dir, |_| {
        drop(probe.take().expect("retry while the probe holds the lock"));
    })
    .unwrap();
    assert!(probe.is_none());
    assert!(matches!(discovery(&settings), Discovery::Starting));
    let (listener_lock, _) = listener_lock.publish(&dir, 12345).unwrap();
    assert!(instance_record(&settings).is_some());
    assert!(hold_lock(&dir, |_| panic!("a published listener is not a probe")).is_none());
    drop((listener_lock, dir));
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn discovery_waits_for_this_generation_instead_of_returning_the_stale_port() {
    let path = scratch("publication-generation");
    let settings = path.join("settings.ini");
    let dir = directory(&settings, true).unwrap();
    let old = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let new = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    Record::publish(&dir, old.local_addr().unwrap().port()).unwrap();
    let mut registration = Some(hold_lock(&dir, |_| panic!("no contender")).unwrap());
    assert!(
        instance_record(&settings).is_none(),
        "the old bytes are not current"
    );
    let port = new.local_addr().unwrap().port();
    let mut lock = None;
    let record = wait_for_record(&settings, Instant::now() + STARTUP, |_| {
        let (held, _) = registration
            .take()
            .expect("discovery retries")
            .publish(&dir, port)
            .unwrap();
        lock = Some(held);
    })
    .unwrap();
    assert!(lock.is_some());
    assert_eq!(record.port, port);
    drop((lock, dir));
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn record_and_lock_refuse_a_non_file() {
    let path = scratch("not-a-file");
    let settings = path.join("settings.ini");
    let dir = directory(&settings, true).unwrap();
    fs::create_dir(port_file(&settings)).unwrap();
    fs::create_dir(private_dir(&settings).join(record::local_name(record::LOCK).as_ref())).unwrap();
    assert!(Record::load(&dir).is_none());
    assert!(record::lock(&dir, true).is_none());
    drop(dir);
    fs::remove_dir_all(path).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_refuses_a_junction_to_an_otherwise_private_directory() {
    let path = scratch("single-junction");
    let settings = path.join("settings.ini");
    let private = path.join("elsewhere");
    drop(Directory::open(&private, true).unwrap());
    let junction = private_dir(&settings);
    assert!(
        std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&private)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(directory(&settings, false).is_none());
    assert!(directory(&settings, true).is_none());
    fs::remove_dir_all(path).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_keeps_the_checked_directory_from_being_renamed_while_open() {
    let path = scratch("single-pinned-directory");
    let settings = path.join("settings.ini");
    let dir = directory(&settings, true).unwrap();
    let name = private_dir(&settings);
    let moved = path.join("moved");
    assert!(fs::rename(&name, &moved).is_err());
    drop(dir);
    fs::rename(name, moved).unwrap();
    fs::remove_dir_all(path).unwrap();
}

#[cfg(unix)]
#[test]
fn private_directory_record_and_lock_do_not_follow_links_or_open_fifos() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{PermissionsExt, symlink};
    let path = scratch("nofollow");
    let settings = path.join("settings.ini");
    let dir = directory(&settings, true).unwrap();
    let port = port_file(&settings);
    let lock = private_dir(&settings).join(record::local_name(record::LOCK).as_ref());
    Record::publish(&dir, 12345).unwrap();
    let alternate = path.join("alternate");
    fs::rename(&port, &alternate).unwrap();
    symlink(&alternate, &port).unwrap();
    symlink(&alternate, &lock).unwrap();
    assert!(Record::load(&dir).is_none());
    assert!(record::lock(&dir, true).is_none());
    for name in [&port, &lock] {
        fs::remove_file(name).unwrap();
        let name = CString::new(name.as_os_str().as_bytes()).unwrap();
        // SAFETY: name is a live NUL-terminated path in the test's scratch directory.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    }
    assert!(Record::load(&dir).is_none());
    assert!(record::lock(&dir, true).is_none());
    let private = private_dir(&settings);
    fs::set_permissions(&private, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(directory(&settings, false).is_none());
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
    let moved = path.join("moved");
    fs::rename(&private, &moved).unwrap();
    symlink(&moved, &private).unwrap();
    assert!(directory(&settings, false).is_none());
    fs::remove_dir_all(path).unwrap();
}

fn delay(by: Duration) {
    // No sleeps as synchronization: this is an intentional protocol delay, with the only
    // sender kept alive so timeout, not disconnection, is what releases the gate.
    let (_gate, waiting) = channel::<()>();
    assert_eq!(
        waiting.recv_timeout(by),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    );
}

fn gated_check() -> (
    impl Fn(&Path) -> bool + Send + Sync,
    Receiver<()>,
    std::sync::mpsc::Sender<()>,
) {
    let (checking, entered) = channel();
    let (release, gate) = channel();
    let gate = std::sync::Mutex::new(Some(gate));
    let check = move |_: &Path| {
        if let Some(gate) = gate.lock().unwrap().take() {
            checking.send(()).unwrap();
            gate.recv_timeout(READ_DEADLINE * 2).unwrap();
        }
        true
    };
    (check, entered, release)
}

fn challenge_socket(settings: &Path) -> (TcpStream, [u8; 32]) {
    let record = instance_record(settings).unwrap();
    let stream = connect(record.port, CONNECT).unwrap();
    stream.set_read_timeout(Some(READ_DEADLINE)).unwrap();
    let proof = wire::challenge(&mut BufReader::new(&stream), &mut &stream, &record.key).unwrap();
    (stream, proof)
}

#[test]
fn a_failed_send_releases_registration_before_the_client_can_relisten() {
    let path = scratch("immediate-replacement");
    let settings = path.join("settings.ini");
    let file = path.join("note.md");
    fs::write(&file, b"note").unwrap();
    let (check, entered, release) = gated_check();
    let rx = listen_files(APP, &settings, || {}, check).unwrap();
    let (copy, first) = (settings.clone(), file.clone());
    let failing = std::thread::spawn(move || forward(APP, &copy, &[first]));
    entered.recv_timeout(READ_DEADLINE).unwrap();
    // The old accept thread is waiting for this proof. It cannot drop the lock on its
    // normal exit before the first client sees EOF. The failed send must release it.
    let (unfinished, _) = challenge_socket(&settings);
    drop(rx);
    release.send(()).unwrap();
    assert!(!failing.join().unwrap());
    let replacement = listen(APP, &settings).expect("register immediately after refusal");
    assert!(forward(APP, &settings, std::slice::from_ref(&file)));
    assert_eq!(replacement.recv_timeout(READ_DEADLINE).unwrap(), file);
    drop(unfinished);
    retire(path, replacement);
}

#[test]
fn a_closed_view_for_the_same_path_follows_its_gated_open() {
    let path = scratch("ordered-path-requests");
    let settings = path.join("settings.ini");
    let file = path.join("note.md");
    let (check, entered, release) = gated_check();
    let rx = listen_checked_requests(APP, &settings, || {}, check).unwrap();
    let (copy, first) = (settings.clone(), file.clone());
    let opening = std::thread::spawn(move || forward(APP, &copy, &[first]));
    entered.recv_timeout(READ_DEADLINE).unwrap();
    let (stream, proof) = challenge_socket(&settings);
    let closed = Request {
        closed: true,
        ..Request::new(&file)
    };
    let mut writer = &stream;
    writer.write_all(&proof).unwrap();
    writer.write_all(&closed.to_wire(APP)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(150)))
        .unwrap();
    let mut reader = BufReader::new(&stream);
    assert!(
        wire::reply(&mut reader).is_none(),
        "closed must not overtake open"
    );
    assert!(rx.try_recv().is_err());
    release.send(()).unwrap();
    assert!(opening.join().unwrap());
    stream.set_read_timeout(Some(READ_DEADLINE)).unwrap();
    assert_eq!(wire::reply(&mut reader).as_deref(), Some(OK));
    for expected_closed in [false, true] {
        let got = rx.recv_timeout(READ_DEADLINE).unwrap();
        assert_eq!(
            (got.path.as_path(), got.closed),
            (file.as_path(), expected_closed)
        );
    }
    retire(path, rx);
}

#[test]
fn an_unplace_follows_a_gated_file_request_that_places_the_window() {
    let path = scratch("ordered-window-placement");
    let settings = path.join("settings.ini");
    let (check, entered, release) = gated_check();
    let rx = listen_checked_requests(APP, &settings, || {}, check).unwrap();
    let placed = Request {
        place: crate::sister::Rect::parse("10,20,300,400"),
        ..Request::new(path.join("note.md"))
    };
    let copy = settings.clone();
    let sent = placed.clone();
    let opening = std::thread::spawn(move || deliver_request(APP, &copy, &sent));
    entered.recv_timeout(READ_DEADLINE).unwrap();

    let (stream, proof) = challenge_socket(&settings);
    let unplace = Request {
        unplace: true,
        ..Request::default()
    };
    let mut writer = &stream;
    writer.write_all(&proof).unwrap();
    writer.write_all(&unplace.to_wire(APP)).unwrap();
    // A third challenge proves the listener has dispatched the unplace while the
    // first request is still gated. No timing guess is needed to check the queue.
    drop(challenge_socket(&settings));
    let early = rx.try_recv();
    release.send(()).unwrap();
    assert!(early.is_err(), "unplace overtook the file request");
    assert_eq!(opening.join().unwrap(), Delivery::Taken);
    assert_eq!(
        wire::reply(&mut BufReader::new(stream)).as_deref(),
        Some(OK)
    );
    assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap(), placed);
    let got = rx.recv_timeout(READ_DEADLINE).unwrap();
    assert!(got.unplace && got.path.as_os_str().is_empty());
    retire(path, rx);
}

#[test]
fn accept_backs_off_after_connection_errors_and_only_stops_for_an_invalid_listener() {
    use std::io::ErrorKind::*;
    #[cfg(windows)]
    let retry_codes = [10024, 4, 10055, 8, 14, 10013]; // Includes firewall denial.
    #[cfg(unix)]
    let retry_codes = [
        libc::EMFILE,
        libc::ENFILE,
        libc::ENOBUFS,
        libc::ENOMEM,
        libc::EPROTO,
        libc::EPERM,
    ];
    #[cfg(windows)]
    let invalid_codes = [10009, 10038, 10022];
    #[cfg(unix)]
    let invalid_codes = [libc::EBADF, libc::ENOTSOCK, libc::EINVAL];
    let kinds = [
        ConnectionAborted,
        Interrupted,
        WouldBlock,
        ConnectionReset,
        PermissionDenied,
    ];
    let mut inputs = kinds
        .map(|kind| Err(io::Error::from(kind)))
        .into_iter()
        .chain(retry_codes.map(|code| Err(io::Error::from_raw_os_error(code))))
        .chain([Ok("connection")]);
    let mut waits = Vec::new();
    assert_eq!(
        accept_next(
            || inputs.next().unwrap(),
            || false,
            |delay| waits.push(delay)
        ),
        Some("connection")
    );
    assert_eq!(waits, vec![POLL; retry_codes.len() + kinds.len()]);
    for code in invalid_codes {
        assert_eq!(
            accept_next::<()>(
                || Err(io::Error::from_raw_os_error(code)),
                || false,
                |_| panic!("invalid listener must not retry")
            ),
            None
        );
    }
    assert_eq!(
        accept_next::<()>(|| panic!("accept after stop"), || true, |_| unreachable!()),
        None
    );
}

#[test]
fn a_pending_path_check_defers_its_ack_without_blocking_the_next_connection() {
    let path = scratch("blocked-file-check");
    let settings = path.join("settings.ini");
    let file = path.join("note.md");
    let (check, entered, release) = gated_check();
    let rx = listen_files(APP, &settings, || {}, check).unwrap();
    let settings2 = settings.clone();
    let file2 = file.clone();
    let (reply, acknowledged) = channel();
    let send = std::thread::spawn(move || {
        reply.send(forward(APP, &settings2, &[file2])).unwrap();
    });
    entered.recv_timeout(READ_DEADLINE).unwrap();
    assert_eq!(
        acknowledged.recv_timeout(Duration::from_millis(150)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout),
        "the final receiver has not accepted a path yet"
    );
    assert!(
        rx.try_recv().is_err(),
        "the worker is still checking the file"
    );
    let record = instance_record(&settings).unwrap();
    let stream = connect(record.port, CONNECT).unwrap();
    stream.set_read_timeout(Some(READ_DEADLINE)).unwrap();
    let mut reader = BufReader::new(&stream);
    let mut writer = &stream;
    let proof = wire::challenge(&mut reader, &mut writer, &record.key)
        .expect("the listener can authenticate a second request during the file check");
    writer.write_all(&proof).unwrap();
    writer.write_all(b"test-app\n\n").unwrap();
    release.send(()).unwrap();
    assert!(acknowledged.recv_timeout(READ_DEADLINE).unwrap());
    assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap(), file);
    assert_eq!(wire::reply(&mut reader).as_deref(), Some(OK));
    send.join().unwrap();
    retire(path, rx);
}

#[test]
fn an_unsaved_text_update_cannot_overtake_the_open_for_its_document() {
    let path = scratch("ordered-document");
    let settings = path.join("settings.ini");
    let (check, entered, release) = gated_check();
    let rx = listen_checked_requests(APP, &settings, || {}, check).unwrap();
    let (copy, file) = (settings.clone(), path.join("slow.md"));
    let first = std::thread::spawn(move || forward(APP, &copy, &[file]));
    entered.recv_timeout(READ_DEADLINE).unwrap();

    let open = Request {
        path: path.join("draft.md"),
        open: true,
        text: Some("first".into()),
        ..Request::document("doc", "Document")
    };
    let update = Request {
        text: Some("edited".into()),
        ..Request::document("doc", "Document")
    };
    let streams = [&open, &update].map(|request| {
        let (stream, proof) = challenge_socket(&settings);
        (&stream).write_all(&proof).unwrap();
        (&stream).write_all(&request.to_wire(APP)).unwrap();
        stream
    });
    drop(challenge_socket(&settings));
    let early = rx.try_recv();
    release.send(()).unwrap();
    assert!(early.is_err(), "the update overtook the queued open");
    assert!(first.join().unwrap());
    assert_eq!(
        rx.recv_timeout(READ_DEADLINE).unwrap().path,
        path.join("slow.md")
    );
    for (stream, request) in streams.into_iter().zip([open, update]) {
        assert_eq!(
            wire::reply(&mut BufReader::new(stream)).as_deref(),
            Some(OK_DOC)
        );
        let got = rx.recv_timeout(READ_DEADLINE).unwrap();
        assert_eq!(
            (got.doc, got.open, got.text),
            (request.doc, request.open, request.text)
        );
    }
    retire(path, rx);
}

#[test]
fn the_existing_request_listener_skips_missing_paths_and_a_closed_queue_gets_no_ack() {
    let path = scratch("closed-request-queue");
    let settings = path.join("settings.ini");
    let (wakes, woken) = channel();
    let rx = listen_requests_waking(APP, &settings, move || _ = wakes.send(())).unwrap();
    let request = Request::new(path.join("still-missing.md"));
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Taken);
    woken.recv_timeout(READ_DEADLINE).unwrap();
    assert!(
        rx.try_recv().is_err(),
        "an ordinary missing path was not queued"
    );
    fs::write(&request.path, b"now present").unwrap();
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Taken);
    assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap(), request);
    woken.recv_timeout(READ_DEADLINE).unwrap();
    drop(rx);
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Nobody);
    assert!(
        woken.try_recv().is_err(),
        "the rejected request did not wake the window"
    );
    assert!(wait_gone(&settings, READ_DEADLINE * 2));
    assert!(!alive(&settings));
    remove(path);
}

#[test]
fn only_the_opt_in_request_listener_delivers_unresolved_paths() {
    let path = scratch("raw-request-queue");
    let settings = path.join("settings.ini");
    let rx = listen_unresolved_requests_waking(APP, &settings, || {}).unwrap();
    let request = Request::new(path.join("still-missing.md"));
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Taken);
    assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap(), request);
    drop(rx);
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Nobody);
    assert!(wait_gone(&settings, READ_DEADLINE * 2));
    assert!(!alive(&settings));
    let replacement = listen_unresolved_requests_waking(APP, &settings, || {}).unwrap();
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Taken);
    assert_eq!(replacement.recv_timeout(READ_DEADLINE).unwrap(), request);
    retire(path, replacement);
}

#[test]
fn the_paths_only_listener_does_not_ack_a_closed_receiver_and_releases_its_lock() {
    let path = scratch("closed-path-queue");
    let settings = path.join("settings.ini");
    let file = path.join("note.md");
    fs::write(&file, b"note").unwrap();
    let (wakes, woken) = channel();
    let rx = listen_waking(APP, &settings, move || _ = wakes.send(())).unwrap();
    assert!(forward(APP, &settings, std::slice::from_ref(&file)));
    assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap(), file);
    woken.recv_timeout(READ_DEADLINE).unwrap();
    drop(rx);
    assert!(!forward(APP, &settings, std::slice::from_ref(&file)));
    assert!(
        woken.try_recv().is_err(),
        "no wake after a failed final send"
    );
    assert!(wait_gone(&settings, READ_DEADLINE * 2));
    assert!(!alive(&settings));
    let replacement = listen_waking(APP, &settings, || {}).unwrap();
    assert!(forward(APP, &settings, std::slice::from_ref(&file)));
    assert_eq!(replacement.recv_timeout(READ_DEADLINE).unwrap(), file);
    retire(path, replacement);
}

#[test]
fn a_malformed_or_overflowing_text_length_is_not_an_absent_length() {
    let path = scratch("invalid-text-length");
    let settings = path.join("settings.ini");
    let rx = listen_requests_waking(APP, &settings, || {}).unwrap();
    let overflow = format!("?text={}0", usize::MAX);
    for field in [
        &overflow,
        "?text=oops",
        "?text=",
        "?text",
        &format!("?text=0\n{overflow}"),
    ] {
        let wire = format!("{APP}\n?doc=bad\n{field}\n\n");
        assert!(exchange(&settings, wire.as_bytes()).is_none());
        assert!(rx.try_recv().is_err());
    }
    let valid = Request {
        text: Some(String::new()),
        ..Request::document("valid", "Document")
    };
    assert_eq!(deliver_request(APP, &settings, &valid), Delivery::Taken);
    assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap().text, valid.text);
    retire(path, rx);
}

#[test]
fn all_text_declarations_must_agree_and_fit_the_cap() {
    let path = scratch("competing-text-lengths");
    let settings = path.join("settings.ini");
    let rx = listen_requests_waking(APP, &settings, || {}).unwrap();
    for second in [1, MAX_TEXT + 1] {
        let mismatched = format!("{APP}\n?doc=bad\n?text=0\n?text={second}\n\n");
        assert!(exchange(&settings, mismatched.as_bytes()).is_none());
        assert!(rx.try_recv().is_err());
    }
    let matching = format!("{APP}\n?doc=valid\n?text=4\n?text=4\n\nnote");
    assert_eq!(
        exchange(&settings, matching.as_bytes()).as_deref(),
        Some(OK_DOC)
    );
    assert_eq!(
        rx.recv_timeout(READ_DEADLINE).unwrap().text.as_deref(),
        Some("note")
    );
    retire(path, rx);
}

#[test]
fn long_shared_fields_cannot_expand_past_the_header_budget() {
    let path = scratch("expanded-fields");
    let settings = path.join("settings.ini");
    let rx = listen_requests_waking(APP, &settings, || {}).unwrap();
    let title = "t".repeat(60 * 1024);
    let from = "f".repeat(60 * 1024);
    let ordinary = format!("{APP}\n?path=one\n?doc=one\n?title={title}\n?from={from}\n\n");
    let oversized = format!(
        "{APP}\n{}?doc=one\n?title={title}\n?from={from}\n\n",
        "?path=one\n".repeat(128),
    );
    assert!(
        wire::header(&mut oversized.as_bytes(), APP).is_some(),
        "fits on the wire"
    );
    assert!(exchange(&settings, oversized.as_bytes()).is_none());
    assert!(rx.try_recv().is_err());
    assert_eq!(
        exchange(&settings, ordinary.as_bytes()).as_deref(),
        Some(OK_DOC)
    );
    let received = rx.recv_timeout(READ_DEADLINE).unwrap();
    assert_eq!(received.title.as_deref(), Some(title.as_str()));
    assert_eq!(received.from.as_deref(), Some(from.as_str()));
    retire(path, rx);
}

#[test]
fn text_with_two_destinations_is_rejected_before_any_request_is_queued() {
    let path = scratch("text-fanout");
    let settings = path.join("settings.ini");
    let rx = listen_requests_waking(APP, &settings, || {}).unwrap();
    let (one, two) = (path.join("one.md"), path.join("two.md"));
    fs::write(&one, b"one").unwrap();
    fs::write(&two, b"two").unwrap();
    let message = format!(
        "{APP}\n{}\n{}\n?text=4\n\nnote",
        one.display(),
        two.display()
    );
    assert!(!send(&settings, message.as_bytes()));
    assert!(rx.try_recv().is_err());
    let request = Request {
        text: Some("note".into()),
        ..Request::new(one)
    };
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Taken);
    assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap().text, request.text);
    retire(path, rx);
}

#[test]
fn a_slow_challenge_then_a_slow_acknowledgement_does_not_lose_delivery() {
    let path = scratch("slow-reply");
    let settings = path.join("settings.ini");
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let dir = directory(&settings, true).unwrap();
    let (lock, record) = hold_lock(&dir, std::thread::sleep)
        .unwrap()
        .publish(&dir, listener.local_addr().unwrap().port())
        .unwrap();
    let peer = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&stream);
        let mut writer = &stream;
        stream.set_read_timeout(Some(READ_DEADLINE)).unwrap();
        delay(Duration::from_millis(850));
        wire::authenticate(&mut reader, &mut writer, &record.key).unwrap();
        assert_eq!(
            wire::header(&mut reader, APP).unwrap(),
            ["?follow", "?doc=one", "?title=", "?text=4"]
        );
        let mut text = [0; 4];
        reader.read_exact(&mut text).unwrap();
        assert_eq!(&text, b"note");
        delay(Duration::from_millis(850));
        writer.write_all(b"ok doc\n").unwrap();
    });
    let request = Request {
        text: Some("note".into()),
        ..Request::document("one", "")
    };
    assert_eq!(deliver_request(APP, &settings, &request), Delivery::Taken);
    peer.join().unwrap();
    drop((lock, dir));
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn the_receiver_allows_slow_progress_in_both_the_header_and_text_phases() {
    let path = scratch("slow-upload");
    let settings = path.join("settings.ini");
    let rx = listen_requests_waking(APP, &settings, || {}).unwrap();
    let record = instance_record(&settings).unwrap();
    let stream = connect(record.port, CONNECT).unwrap();
    let mut reader = BufReader::new(&stream);
    stream.set_read_timeout(Some(READ_DEADLINE)).unwrap();
    let mut writer = &stream;
    let proof = wire::challenge(&mut reader, &mut writer, &record.key).unwrap();
    writer.write_all(&proof).unwrap();
    writer.write_all(b"test-app\n?doc=one\n?text=4\n").unwrap();
    delay(Duration::from_millis(1100));
    writer.write_all(b"\nno").unwrap();
    delay(Duration::from_millis(1100));
    writer.write_all(b"te").unwrap();
    assert_eq!(wire::reply(&mut reader).as_deref(), Some(OK_DOC));
    assert_eq!(
        rx.recv_timeout(CONNECT).unwrap().text.as_deref(),
        Some("note")
    );
    retire(path, rx);
}

#[test]
fn header_progress_does_not_extend_the_overall_read_deadline() {
    let path = scratch("header-overall-deadline");
    let settings = path.join("settings.ini");
    let rx = listen_requests_waking(APP, &settings, || {}).unwrap();
    let record = instance_record(&settings).unwrap();
    let stream = connect(record.port, CONNECT).unwrap();
    stream.set_nodelay(true).unwrap();
    stream.set_read_timeout(Some(READ_DEADLINE)).unwrap();
    let mut reader = BufReader::new(&stream);
    let mut writer = &stream;
    let proof = wire::challenge(&mut reader, &mut writer, &record.key).unwrap();
    writer.write_all(&proof).unwrap();
    writer.write_all(b"test-app\n?doc=late\n").unwrap();
    // Every individual gap is well within the read timeout; the terminating blank line
    // comes after the total budget, not just after one long pause.
    let step = Duration::from_millis(750);
    delay(step);
    writer.write_all(b"?title=still\n").unwrap();
    delay(step);
    writer.write_all(b"?from=progress\n").unwrap();
    delay(step);
    _ = writer.write_all(b"\n");
    assert!(wire::reply(&mut reader).is_none());
    assert!(rx.try_recv().is_err());
    let valid = Request::document("next", "");
    assert_eq!(deliver_request(APP, &settings, &valid), Delivery::Taken);
    assert_eq!(rx.recv_timeout(READ_DEADLINE).unwrap().doc, valid.doc);
    retire(path, rx);
}
