//! One running window per user. The first instance listens on a loopback port recorded beside the
//! settings file; a later launch sends its file paths there and exits. Both processes prove
//! they hold the fresh key from the owner's private port file before any paths travel. The listener
//! thread only forwards paths into a channel that the GUI thread drains on its poll tick.
//!
//! `app` is the application's name. It opens each hand-over message, so a launch of one application
//! never feeds paths to another application's instance that happens to share a settings folder.

use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

#[cfg(test)]
use crate::sister::MAX_TEXT;
use crate::sister::{Delivery, Request};
mod delivery;
mod record;
mod registration;
mod wire;
use delivery::{Pending, Stop};
use record::{Directory, Record};
use registration::{Discovery, hold_lock, retry};

const CONNECT: Duration = Duration::from_millis(300);
const STARTUP: Duration = Duration::from_secs(2);
const LEGACY_DEFER: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(20);

fn settings_dir(settings: &Path) -> &Path {
    settings
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn private_dir(settings: &Path) -> PathBuf {
    record::endpoint_path(settings_dir(settings))
}

fn directory(settings: &Path, create: bool) -> Option<Directory> {
    if create {
        fs::create_dir_all(settings_dir(settings)).ok()?;
    }
    record::open_endpoint(settings_dir(settings), create)
}

/// Where the authenticated listener records its port, in a directory separate from the
/// legacy decimal port file. Linux and macOS prefer private, host-local storage.
pub fn port_file(settings: &Path) -> PathBuf {
    private_dir(settings).join(record::PORT)
}

fn connect(port: u16, timeout: Duration) -> io::Result<TcpStream> {
    TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), timeout)
}

fn legacy_port(settings: &Path, open: impl FnOnce(&Path) -> Option<File>) -> Option<u16> {
    let file = open(&settings_dir(settings).join("instance.port"))?;
    let mut text = String::new();
    if file.take(16).read_to_string(&mut text).ok()? == 16 {
        return None;
    }
    text.trim().parse().ok().filter(|&port| port != 0)
}

/// A bare connection is a liveness hint, never authority to send or to postpone registration.
fn legacy_alive(settings: &Path, timeout: Duration) -> bool {
    #[cfg(windows)]
    {
        let _ = timeout;
        legacy_predecessor(settings)
    }
    #[cfg(not(windows))]
    {
        legacy_port(settings, record::legacy_probe_file)
            .is_some_and(|port| connect(port, timeout).is_ok())
    }
}

fn legacy_predecessor(settings: &Path) -> bool {
    legacy_port(settings, record::legacy_file).is_some()
        && record::legacy_locked(&settings_dir(settings).join("instance.lock"))
}

fn wait_for_legacy(settings: &Path, end: Instant) {
    while legacy_predecessor(settings) && retry(end, &mut std::thread::sleep) {}
}

/// Hands `paths` to a running instance in bounded batches. True when all batches were
/// acknowledged. If a later batch fails, earlier acknowledged batches are not sent again.
pub fn forward(app: &str, settings: &Path, paths: &[PathBuf]) -> bool {
    wire::send_paths(app, paths, |message| send(settings, message))
}

/// [`forward`] for a file with a place to show: see [`Request::encode`] for the payload.
/// A port recorded by a build without authentication is not used.
pub fn forward_request(app: &str, settings: &Path, request: &Request) -> bool {
    deliver_request(app, settings, request) == Delivery::Taken
}

/// What a receiver answers to a message it has taken, and to one that names a document without
/// a file (`?doc=`). A receiver that predates such documents answers [`OK`] to those too, having
/// found no file in them: that is how the sender knows it was not understood.
const OK: &str = "ok";
const OK_DOC: &str = "ok doc";

/// [`forward_request`], saying which of three things happened: see [`Delivery`]. A request for a
/// document without a file is `Taken` only by a receiver that knows such documents. A live
/// listener that predates authentication is `Older`, and receives no request at all.
pub fn deliver_request(app: &str, settings: &Path, request: &Request) -> Delivery {
    let mut request = request.clone();
    if !request.path.as_os_str().is_empty() {
        request.path = std::path::absolute(&request.path).unwrap_or(request.path);
    }
    let wanted = match request.doc {
        Some(_) => OK_DOC,
        None => OK,
    };
    match exchange(settings, &request.to_wire(app)) {
        Some(reply) if reply == wanted => Delivery::Taken,
        Some(reply) if reply == OK => Delivery::Older,
        _ if !matches!(discovery(settings), Discovery::Ready(_))
            && legacy_alive(settings, CONNECT) =>
        {
            Delivery::Older
        }
        _ => Delivery::Nobody,
    }
}

fn discovery(settings: &Path) -> Discovery {
    directory(settings, false).map_or(Discovery::Absent, |dir| registration::discover(&dir))
}

fn wait_for_record(
    settings: &Path,
    end: Instant,
    mut pause: impl FnMut(Duration),
) -> Option<Record> {
    loop {
        match discovery(settings) {
            Discovery::Ready(record) => return Some(record),
            Discovery::Starting if retry(end, &mut pause) => {}
            _ => return None,
        }
    }
}

#[cfg(test)]
fn instance_record(settings: &Path) -> Option<Record> {
    match discovery(settings) {
        Discovery::Ready(record) => Some(record),
        _ => None,
    }
}

/// Whether a listener is present, including a legacy one. This is not an authenticated
/// request: at most it opens a connection and closes it without sending anything.
/// A connection can take 300 ms, so call this off the UI thread.
pub fn alive(settings: &Path) -> bool {
    let end = Instant::now() + CONNECT;
    let record = wait_for_record(settings, end, std::thread::sleep);
    let left = end
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(1));
    match record {
        Some(record) => connect(record.port, left).is_ok(),
        None => legacy_alive(settings, left),
    }
}

/// Waits until no instance recorded beside `settings` is running, for at most `timeout`. True
/// when none is. A restarted process calls this before [`forward`]: the old process, still closing,
/// would otherwise answer, take the hand-over, and leave no window behind. A listener that
/// has not published yet also counts as running, as does a predecessor that holds its old lock.
pub fn wait_gone(settings: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    loop {
        let left = timeout.saturating_sub(start.elapsed());
        if matches!(discovery(settings), Discovery::Absent) && !legacy_predecessor(settings) {
            return true;
        }
        if left.is_zero() || start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(POLL.min(timeout.saturating_sub(start.elapsed())));
    }
}

/// Writes `message` to the instance recorded beside `settings` and waits for its `ok`.
fn send(settings: &Path, message: &[u8]) -> bool {
    exchange(settings, message).is_some_and(|reply| reply == OK)
}

/// Writes `message` to the instance recorded beside `settings` and returns the line it answers,
/// trimmed; `None` when nothing listens or no answer comes.
fn exchange(settings: &Path, message: &[u8]) -> Option<String> {
    let record = wait_for_record(settings, Instant::now() + STARTUP, std::thread::sleep)?;
    let stream = connect(record.port, CONNECT).ok()?;
    stream.set_nodelay(true).ok()?;
    let end = Instant::now() + READ_DEADLINE;
    let mut reader = BufReader::new(Timed(&stream, end));
    let mut writer = Timed(&stream, end);
    let proof = wire::challenge(&mut reader, &mut writer, &record.key)?;
    writer.1 = Instant::now() + READ_DEADLINE * 2;
    writer.write_all(&proof).ok()?;
    writer.write_all(message).ok()?;
    // The challenge already arrived, so the listener's remaining header, text and reply
    // time fit inside this fresh budget, even if Windows buffered the entire upload.
    reader.get_mut().1 = Instant::now() + READ_DEADLINE * 2 + ACK_DEADLINE + CONNECT;
    wire::reply(&mut reader)
}

/// Recompute the remaining time before every blocking read, even inside a single line.
struct Timed<'a>(&'a TcpStream, Instant);
impl Read for Timed<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.0.set_read_timeout(Some(self.left()?))?;
        (&*self.0).read(out)
    }
}

impl Write for Timed<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.set_write_timeout(Some(self.left()?))?;
        (&*self.0).write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Timed<'_> {
    fn left(&self) -> io::Result<Duration> {
        let left = self.1.saturating_duration_since(Instant::now());
        (!left.is_zero())
            .then_some(left)
            .ok_or_else(|| io::ErrorKind::TimedOut.into())
    }
}

/// Starts listening and records the port. `None` when immediate setup fails. When a legacy
/// predecessor holds its lock, registration waits in the background until the lock clears or
/// until the independent endpoint can be published at the deferral deadline.
pub fn listen(app: &str, settings: &Path) -> Option<Receiver<PathBuf>> {
    listen_waking(app, settings, || {})
}

/// `listen`, calling `wake` from the delivery worker after each launch's paths are queued, so the
/// shell can drain them at once instead of on a timer. What a launch asked beyond opening its
/// files is dropped: see [`listen_requests_waking`].
pub fn listen_waking(
    app: &str,
    settings: &Path,
    wake: impl Fn() + Send + 'static,
) -> Option<Receiver<PathBuf>> {
    listen_files(app, settings, wake, Path::is_file)
}

fn listen_files(
    app: &str,
    settings: &Path,
    wake: impl Fn() + Send + 'static,
    is_file: impl Fn(&Path) -> bool + Send + 'static,
) -> Option<Receiver<PathBuf>> {
    listen_filtered(app, settings, wake, move |request| {
        (!request.closed && is_file(&request.path)).then_some(request.path)
    })
}

fn listen_filtered<T: Send + 'static>(
    app: &str,
    settings: &Path,
    wake: impl Fn() + Send + 'static,
    filter: impl Fn(Request) -> Option<T> + Send + 'static,
) -> Option<Receiver<T>> {
    let (sender, receiver) = channel();
    serve(
        app,
        settings,
        delivery::filtered(sender, wake, filter).ok()?,
    )?;
    Some(receiver)
}

/// [`listen_waking`] handing over whole requests: the file and what the launch asked for it. A
/// window that wants to know about them calls this instead; it must not also call `listen_waking`.
/// One worker keeps every request in arrival order and checks ordinary paths for an existing
/// file or folder.
/// Documents, closed views and unplace requests do not need an existing path.
pub fn listen_requests_waking(
    app: &str,
    settings: &Path,
    wake: impl Fn() + Send + 'static,
) -> Option<Receiver<Request>> {
    listen_checked_requests(app, settings, wake, |path| {
        path.metadata().is_ok_and(|m| m.is_file() || m.is_dir())
    })
}

fn listen_checked_requests(
    app: &str,
    settings: &Path,
    wake: impl Fn() + Send + 'static,
    exists: impl Fn(&Path) -> bool + Send + 'static,
) -> Option<Receiver<Request>> {
    listen_filtered(app, settings, wake, move |request| {
        (request.doc.is_some() || request.closed || request.unplace || exists(&request.path))
            .then_some(request)
    })
}

/// [`listen_requests_waking`] without a filesystem check. Do not register both on the same
/// settings. The caller decides what to do with a missing or otherwise unusable path.
pub fn listen_unresolved_requests_waking(
    app: &str,
    settings: &Path,
    wake: impl Fn() + Send + 'static,
) -> Option<Receiver<Request>> {
    listen_filtered(app, settings, wake, Some)
}

/// How long a message's text may take to arrive, in all.
const READ_DEADLINE: Duration = Duration::from_secs(2);
const ACK_DEADLINE: Duration = Duration::from_secs(2);

/// The `n` bytes of text after a message, as UTF-8, or `None` when they do not arrive within
/// [`READ_DEADLINE`] or are not valid text. Runs on the listener thread, never the window's.
fn read_text(reader: &mut impl std::io::Read, n: usize) -> Option<String> {
    let end = std::time::Instant::now() + READ_DEADLINE;
    let mut bytes = vec![0; n];
    let mut filled = 0;
    while filled < n {
        if std::time::Instant::now() >= end {
            return None;
        }
        match reader.read(&mut bytes[filled..]) {
            Ok(0) | Err(_) => return None,
            Ok(got) => filled += got,
        }
    }
    String::from_utf8(bytes).ok()
}

/// Binds the port and owns the registration until the listener exits. If a predecessor
/// holds the legacy lock, wait in the background, then publish even if the lock remains.
fn serve(
    app: &str,
    settings: &Path,
    dispatch: impl Fn(Pending, &Stop) -> bool + Send + 'static,
) -> Option<()> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    let dir = directory(settings, true)?;
    let registration = hold_lock(&dir, std::thread::sleep)?;
    let app = app.to_string();
    let run = move |record, (lock, _dir): (File, Directory)| {
        let (stop, _lifetime) = Stop::new(port, lock);
        receive(listener, &app, record, &stop, dispatch);
    };
    let start = if legacy_predecessor(settings) {
        let settings = settings.to_owned();
        std::thread::Builder::new().spawn(move || {
            wait_for_legacy(&settings, Instant::now() + LEGACY_DEFER);
            if let Some((lock, record)) = registration.publish(&dir, port) {
                run(record, (lock, dir));
            }
        })
    } else {
        let (lock, record) = registration.publish(&dir, port)?;
        std::thread::Builder::new().spawn(move || run(record, (lock, dir)))
    };
    start.ok().map(|_| ())
}

fn accept_next<T>(
    mut accept: impl FnMut() -> io::Result<T>,
    stopped: impl Fn() -> bool,
    mut pause: impl FnMut(Duration),
) -> Option<T> {
    while !stopped() {
        let connection = accept();
        if stopped() {
            return None;
        }
        match connection {
            Ok(stream) => return Some(stream),
            Err(e) if invalid_listener(&e) => return None,
            // Firewall, protocol and resource errors can affect just one connection.
            Err(_) => pause(POLL),
        }
    }
    None
}

fn invalid_listener(error: &io::Error) -> bool {
    #[cfg(unix)]
    return matches!(
        error.raw_os_error(),
        Some(libc::EBADF | libc::ENOTSOCK | libc::EINVAL)
    );
    #[cfg(windows)]
    {
        const WSAEBADF: i32 = 10009;
        const WSAEINVAL: i32 = 10022;
        const WSAENOTSOCK: i32 = 10038;
        matches!(
            error.raw_os_error(),
            Some(WSAEBADF | WSAEINVAL | WSAENOTSOCK)
        )
    }
}

/// Authenticate, parse, and queue the message without resolving paths. A valid empty
/// launch is still acknowledged as a no-op. Exit when the receiver can no longer deliver.
fn receive(
    listener: TcpListener,
    app: &str,
    record: Record,
    stop: &Stop,
    dispatch: impl Fn(Pending, &Stop) -> bool,
) {
    while let Some(stream) = accept_next(
        || listener.accept().map(|(stream, _)| stream),
        || stop.requested(),
        std::thread::sleep,
    ) {
        let end = Instant::now() + READ_DEADLINE;
        let mut reader = BufReader::new(Timed(&stream, end));
        let mut writer = Timed(&stream, end);
        if wire::authenticate(&mut reader, &mut writer, &record.key).is_none() {
            continue;
        }
        let Some(body) = wire::header(&mut reader, app) else {
            continue;
        };
        let Some(destinations) = wire::bounded_destinations(&body) else {
            continue;
        };
        let Some(text_len) = wire::checked_text_length(&body) else {
            continue;
        };
        if text_len.is_some() && destinations != 1 {
            continue;
        }
        let mut requests = Request::parse(body.iter().map(String::as_str));
        if let Some(n) = text_len {
            let [request] = requests.as_mut_slice() else {
                continue;
            };
            reader.get_mut().1 = Instant::now() + READ_DEADLINE;
            let Some(text) = read_text(&mut reader, n) else {
                continue;
            };
            request.text = Some(text);
        }
        if stop.requested()
            || !dispatch(
                Pending::new(stream, requests, Instant::now() + ACK_DEADLINE),
                stop,
            )
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_fixture::{
        alive, ask, deliver_request, forward, forward_request, hold_lock, listen,
        listen_requests_waking, listen_waking, lock_file, old_serve, port_file, wait_gone,
    };
    use super::*;

    const APP: &str = "test-app";

    #[test]
    fn a_restart_waits_for_the_old_instance_to_end() {
        let dir = std::env::temp_dir().join(format!("mdshared-wait-{}", std::process::id()));
        let settings = dir.join("settings.ini");
        _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(wait_gone(&settings, Duration::from_secs(1)), "none runs");
        // An instance by hand: its port, its port file and its lock, which it lets go of later.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        std::fs::write(
            port_file(&settings),
            listener.local_addr().unwrap().port().to_string(),
        )
        .unwrap();
        let lock = hold_lock(&settings);
        assert!(
            !wait_gone(&settings, Duration::from_millis(150)),
            "a running instance is waited for, not skipped"
        );
        let ends = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(listener);
            drop(lock);
        });
        let started = std::time::Instant::now();
        assert!(wait_gone(&settings, Duration::from_secs(10)));
        assert!(started.elapsed() >= Duration::from_millis(100));
        ends.join().unwrap();
        // The ordinary launch that follows finds nothing to hand its files to.
        assert!(!forward(APP, &settings, &[]));
    }

    #[test]
    fn a_second_launch_reaches_the_first() {
        let dir = std::env::temp_dir().join(format!("mdshared-single-{}", std::process::id()));
        let settings = dir.join("settings.ini");
        assert!(!forward(APP, &settings, &[]), "nothing listens yet");
        let receiver = listen(APP, &settings).expect("listen");
        let file = dir.join("a.md");
        std::fs::write(&file, "# a\n").unwrap();
        assert!(forward(
            APP,
            &settings,
            &[file.clone(), dir.join("missing.md")]
        ));
        assert_eq!(receiver.recv_timeout(Duration::from_secs(2)).unwrap(), file);
        assert!(
            !forward("other-app", &settings, std::slice::from_ref(&file)),
            "another application's name is not accepted"
        );
        assert!(
            receiver.try_recv().is_err(),
            "a missing file is not forwarded"
        );
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_reaches_a_request_receiver_and_not_a_paths_one() {
        let dir = std::env::temp_dir().join(format!("mdshared-single-dir-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let requests = dir.join("requests.ini");
        let receiver = listen_requests_waking(APP, &requests, || {}).expect("listen");
        assert!(forward(APP, &requests, &[dir.join("sub")]));
        let got = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(got.path, dir.join("sub"));
        let paths = dir.join("paths.ini");
        let only = listen(APP, &paths).expect("listen");
        assert!(forward(APP, &paths, &[dir.join("sub")]));
        assert!(only.recv_timeout(Duration::from_millis(300)).is_err());
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_closed_reaches_a_request_receiver_though_its_file_is_gone_and_opens_nothing() {
        let dir =
            std::env::temp_dir().join(format!("mdshared-single-closed-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.ini");
        let receiver = listen_requests_waking(APP, &settings, || {}).expect("listen");
        let request = Request {
            closed: true,
            ..Request::new(dir.join("deleted.md"))
        };
        assert_eq!(deliver_request(APP, &settings, &request), Delivery::Taken);
        let got = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(got.closed && got.follow && got.path == dir.join("deleted.md"));
        // A receiver that takes files only is not asked to open one that exists.
        let file = dir.join("a.md");
        std::fs::write(&file, "# a\n").unwrap();
        let paths = dir.join("paths.ini");
        let only = listen(APP, &paths).expect("listen");
        let request = Request {
            closed: true,
            ..Request::new(&file)
        };
        assert_eq!(deliver_request(APP, &paths, &request), Delivery::Taken);
        assert!(only.recv_timeout(Duration::from_millis(300)).is_err());
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unplace_reaches_a_request_receiver_though_it_names_no_file() {
        let dir =
            std::env::temp_dir().join(format!("mdshared-single-unplace-{}", std::process::id()));
        let settings = dir.join("settings.ini");
        let receiver = listen_requests_waking(APP, &settings, || {}).expect("listen");
        let request = Request {
            unplace: true,
            ..Request::default()
        };
        assert_eq!(
            deliver_request(APP, &settings, &request),
            Delivery::Taken,
            "answered ok"
        );
        let got = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(got.unplace && got.follow && got.path.as_os_str().is_empty());
        // A receiver that takes files only gets nothing.
        let paths = dir.join("paths.ini");
        let only = listen(APP, &paths).expect("listen");
        assert_eq!(deliver_request(APP, &paths, &request), Delivery::Taken);
        assert!(only.recv_timeout(Duration::from_millis(300)).is_err());
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn alive_is_a_bare_connection_that_changes_nothing() {
        let dir = std::env::temp_dir().join(format!("mdshared-alive-{}", std::process::id()));
        let settings = dir.join("settings.ini");
        assert!(!alive(&settings), "nothing listens yet");
        let receiver = listen(APP, &settings).expect("listen");
        assert!(alive(&settings));
        assert!(alive(&settings));
        // The listener took the connections for empty messages: nothing was delivered, and it
        // still serves the next real one.
        let file = dir.join("a.md");
        std::fs::write(&file, "# a\n").unwrap();
        assert!(forward(APP, &settings, std::slice::from_ref(&file)));
        assert_eq!(receiver.recv_timeout(Duration::from_secs(2)).unwrap(), file);
        assert!(receiver.try_recv().is_err());
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_old_receiver_ignores_text_and_does_nothing_with_it() {
        let dir = std::env::temp_dir().join(format!("mdshared-oldtext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.md");
        std::fs::write(&file, "# a\n").unwrap();
        let (tx, rx) = channel();
        let port = old_serve("test-app", tx);
        // Small, larger than the old reader's buffer, and the cap. The bytes after the empty line
        // are never read by it. It must not deliver them as paths, and must not be broken by
        // them. What the sender sees is another matter: closing with bytes unread resets the
        // connection, so a large text may get "no" instead of "ok" (a small one, already in the
        // reader's buffer, gets "ok"). Either is an answer the editor treats as no live link.
        let mut answers = Vec::new();
        for size in [10, 4_000, 20_000, MAX_TEXT] {
            let mut request = Request::new(&file);
            request.text = Some("x".repeat(size));
            request.line = Some(3);
            answers.push(ask(port, &request.to_wire("test-app")));
        }
        eprintln!("old receiver answers for 10 B, 4 KB, 20 KB, 2 MiB: {answers:?}");
        assert!(answers[0], "a small text is answered");
        let mut off = Request::new(&file);
        off.live_off = true;
        assert!(ask(port, &off.to_wire("test-app")));
        // A text that looks like a message of its own is not read as one either.
        let mut tricky = Request::new(&file);
        tricky.text = Some(format!("test-app\n{}\n\n", file.display()));
        assert!(ask(port, &tricky.to_wire("test-app")));
        // Exactly one request per message, each the follow request itself, which the viewer of
        // that release treats as a follow: no raise, no open, and no text.
        let got: Vec<Request> = (0..6)
            .map(|_| rx.recv_timeout(Duration::from_secs(2)).unwrap())
            .collect();
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
        assert!(got.iter().all(|r| r.follow && r.path == file));
        assert!(got.iter().all(|r| r.text.is_none()));
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_arrives_with_its_request_and_only_within_the_cap() {
        let dir = std::env::temp_dir().join(format!("mdshared-text-{}", std::process::id()));
        let settings = dir.join("settings.ini");
        let receiver = listen_requests_waking(APP, &settings, || {}).expect("listen");
        let file = dir.join("a.md");
        std::fs::write(&file, "# a\n").unwrap();
        let mut request = Request::new(&file);
        request.text = Some("# live \u{e9}\u{1f600}\n".repeat(1000));
        request.line = Some(7);
        assert!(forward_request(APP, &settings, &request));
        let got = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(got.text, request.text);
        assert!(got.follow && got.line == Some(7));

        let mut off = Request::new(&file);
        off.live_off = true;
        assert!(forward_request(APP, &settings, &off));
        let got = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(got.live_off && got.follow && got.text.is_none());

        // The cap is exact: the largest passes, one byte more is refused unanswered.
        let mut big = Request::new(&file);
        big.text = Some("a".repeat(MAX_TEXT));
        assert!(forward_request(APP, &settings, &big));
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(2)).unwrap().text,
            big.text
        );
        big.text = Some("a".repeat(MAX_TEXT + 1));
        assert!(!forward_request(APP, &settings, &big));
        assert!(receiver.try_recv().is_err());

        // Text that is not UTF-8, or that is shorter than announced, delivers nothing.
        let port: u16 = std::fs::read_to_string(port_file(&settings))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let mut head = Request::new(&file);
        head.text = Some("abcd".into());
        let mut message = head.encode(APP).into_bytes();
        message.extend_from_slice(&[0xff, 0xfe, b'a', b'b']);
        assert!(!ask(port, &message));
        let mut message = head.encode(APP).into_bytes();
        message.extend_from_slice(b"ab");
        assert!(!ask(port, &message), "two bytes short, never completes");
        assert!(receiver.try_recv().is_err());
        // The listener is still serving.
        assert!(forward_request(APP, &settings, &Request::new(&file)));
        assert!(receiver.recv_timeout(Duration::from_secs(2)).is_ok());
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_round_trip_of_200_kb_is_quick() {
        let dir = std::env::temp_dir().join(format!("mdshared-rtt-{}", std::process::id()));
        let settings = dir.join("settings.ini");
        let receiver = listen_requests_waking(APP, &settings, || {}).expect("listen");
        let file = dir.join("a.md");
        std::fs::write(&file, "# a\n").unwrap();
        let mut request = Request::new(&file);
        request.text = Some("line of text\n".repeat(200 * 1024 / 13));
        let mut worst = Duration::ZERO;
        for _ in 0..20 {
            let t = std::time::Instant::now();
            assert!(forward_request(APP, &settings, &request));
            receiver.recv_timeout(Duration::from_secs(2)).unwrap();
            worst = worst.max(t.elapsed());
        }
        eprintln!("200 KB send + receive, worst of 20: {worst:?}");
        assert!(worst < Duration::from_millis(250));
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stale_port_file_does_not_cost_the_connection_timeout() {
        let dir = std::env::temp_dir().join(format!("mdshared-stale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.ini");
        // What an instance that has exited leaves: its port, and a lock nobody holds.
        let free = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = free.local_addr().unwrap().port();
        drop(free);
        std::fs::write(port_file(&settings), port.to_string()).unwrap();
        std::fs::write(lock_file(&settings), "").unwrap();
        let started = std::time::Instant::now();
        assert!(!forward(APP, &settings, &[]));
        if cfg!(windows) {
            assert!(
                started.elapsed() < Duration::from_millis(100),
                "{:?}",
                started.elapsed()
            );
        }
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_document_without_a_file_arrives_and_is_answered_as_understood() {
        let dir = std::env::temp_dir().join(format!("mdshared-doc-{}", std::process::id()));
        let settings = dir.join("settings.ini");
        let receiver = listen_requests_waking(APP, &settings, || {}).expect("listen");
        let show = Request {
            open: true,
            text: Some("# never saved \u{e9}\n".into()),
            line: Some(1),
            ..Request::document("mdedit-1-2", "Untitled 2")
        };
        assert_eq!(deliver_request(APP, &settings, &show), Delivery::Taken);
        let got = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            got, show,
            "nothing on disk is needed, and the path stays empty"
        );
        // An empty document is a text of no bytes.
        let empty = Request {
            text: Some(String::new()),
            ..Request::document("mdedit-1-2", "Untitled 2")
        };
        assert!(forward_request(APP, &settings, &empty));
        let got = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(got.follow && got.text.as_deref() == Some(""));
        // Saved: the file it became comes with the name it had.
        let file = dir.join("new.md");
        std::fs::write(&file, "# saved\n").unwrap();
        let saved = Request {
            follow: true,
            doc: Some("mdedit-1-2".into()),
            ..Request::new(&file)
        };
        assert_eq!(deliver_request(APP, &settings, &saved), Delivery::Taken);
        let got = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!((got.path, got.doc), (file.clone(), saved.doc.clone()));
        // A paths-only receiver never sees a document: there is no file in it.
        let dir2 = dir.join("paths");
        let settings2 = dir2.join("settings.ini");
        let paths = listen_waking(APP, &settings2, || {}).expect("listen");
        assert_eq!(deliver_request(APP, &settings2, &show), Delivery::Taken);
        assert!(paths.recv_timeout(Duration::from_millis(200)).is_err());
        // Nobody there.
        let nobody = dir.join("none").join("settings.ini");
        assert_eq!(deliver_request(APP, &nobody, &show), Delivery::Nobody);
        assert_eq!(
            deliver_request(APP, &nobody, &Request::new(&file)),
            Delivery::Nobody
        );
        _ = std::fs::remove_dir_all(&dir);
    }

    // The raw legacy probe and the authenticated copy of the same payload reader
    // check the old acknowledgement semantics, not interoperability with legacy transport.
    #[test]
    fn an_old_receiver_does_nothing_with_a_document_and_the_sender_can_tell() {
        let dir = std::env::temp_dir().join(format!("mdshared-olddoc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.ini");
        let (tx, rx) = channel();
        let port = old_serve("test-app", tx);
        std::fs::write(port_file(&settings), port.to_string()).unwrap();
        let _lock = hold_lock(&settings);
        let show = Request {
            open: true,
            text: Some("# never saved\n".into()),
            ..Request::document("mdedit-1-2", "Untitled 2")
        };
        // It answers `ok` as it does to anything, having found no file to open.
        assert!(ask(port, &show.to_wire("test-app")));
        assert_eq!(
            deliver_request("test-app", &settings, &show),
            Delivery::Older
        );
        assert!(!forward_request("test-app", &settings, &show));
        let off = Request {
            live_off: true,
            ..Request::document("mdedit-1-2", "Untitled 2")
        };
        assert_eq!(
            deliver_request("test-app", &settings, &off),
            Delivery::Older
        );
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        // A request for a file is taken by it as before.
        let file = dir.join("a.md");
        std::fs::write(&file, "# a\n").unwrap();
        assert_eq!(
            deliver_request("test-app", &settings, &Request::new(&file)),
            Delivery::Taken
        );
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap().path, file);
        _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
#[path = "single/overlap_tests.rs"]
mod overlap_tests;

#[cfg(test)]
mod test_fixture;
