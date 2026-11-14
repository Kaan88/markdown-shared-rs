//! One running window per user. The first instance listens on a loopback port recorded beside the
//! settings file; a later launch sends its file paths there and exits. Plain `std`: the listener
//! thread only forwards paths into a channel that the GUI thread drains on its poll tick.
//!
//! `app` is the application's name. It opens each hand-over message, so a launch of one application
//! never feeds paths to another application's instance that happens to share a settings folder.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use crate::sister::{MAX_TEXT, Request};

/// Where the running instance records its port.
pub fn port_file(settings: &Path) -> PathBuf {
    settings.with_file_name("instance.port")
}

/// The file a running instance keeps open for as long as it lives.
fn lock_file(settings: &Path) -> PathBuf {
    settings.with_file_name("instance.lock")
}

/// Whether an instance is running, known at once. The port file outlives the instance that wrote
/// it, and on Windows a connection to a loopback port nobody listens on is not refused: it waits
/// out the whole timeout, which was 300 ms added to every ordinary launch. The running instance
/// holds the lock file open with no sharing, so opening it fails while that instance lives and
/// succeeds the moment it is gone, however it ended.
#[cfg(windows)]
fn instance_alive(settings: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    match std::fs::OpenOptions::new()
        .write(true)
        .share_mode(0)
        .open(lock_file(settings))
    {
        Ok(_) => false,
        Err(e) => e.raw_os_error() == Some(ERROR_SHARING_VIOLATION),
    }
}

/// Elsewhere a refused connection returns at once, so the port is asked directly.
#[cfg(not(windows))]
fn instance_alive(_settings: &Path) -> bool {
    true
}

/// Takes the lock [`instance_alive`] looks for. `None` if it cannot be taken, which only costs a
/// later launch its hand-over.
fn hold_lock(settings: &Path) -> Option<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    options.open(lock_file(settings)).ok()
}

/// Hands `paths` to an already running instance. True when one answered, so the caller can exit.
pub fn forward(app: &str, settings: &Path, paths: &[PathBuf]) -> bool {
    let mut message = format!("{app}\n");
    for path in paths {
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.clone());
        message.push_str(&absolute.display().to_string());
        message.push('\n');
    }
    message.push('\n');
    send(settings, message.as_bytes())
}

/// [`forward`] for a file with a place to show: see [`Request::encode`] for what travels. A
/// running instance that predates these options opens the file and ignores the rest; with
/// `follow` it does nothing at all.
pub fn forward_request(app: &str, settings: &Path, request: &Request) -> bool {
    let mut request = request.clone();
    request.path = std::path::absolute(&request.path).unwrap_or(request.path);
    send(settings, &request.to_wire(app))
}

/// The loopback port of the instance recorded beside `settings`, if one is known to run.
fn instance_port(settings: &Path) -> Option<u16> {
    let port = std::fs::read_to_string(port_file(settings))
        .ok()
        .and_then(|t| t.trim().parse::<u16>().ok())?;
    instance_alive(settings).then_some(port)
}

/// Whether an instance answers on its port: a connection that is closed again at once, with
/// nothing sent. The listener reads that as an empty message and does nothing, so the window is
/// not opened, raised or changed. Blocks for at most 300 ms, so call it off the UI thread.
pub fn alive(settings: &Path) -> bool {
    let Some(port) = instance_port(settings) else {
        return false;
    };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// Writes `message` to the instance recorded beside `settings` and waits for its `ok`.
fn send(settings: &Path, message: &[u8]) -> bool {
    let Some(port) = instance_port(settings) else {
        return false;
    };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(300)) else {
        return false;
    };
    _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
    // A message with text can be a megabyte or two, which the socket buffer takes in one go on
    // loopback; the write timeout bounds a receiver that does not read.
    _ = stream.set_write_timeout(Some(Duration::from_millis(1500)));
    if stream.write_all(message).is_err() {
        return false;
    }
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).is_ok() && reply.trim() == "ok"
}

/// Starts listening and records the port. `None` when the port cannot be bound or written.
pub fn listen(app: &str, settings: &Path) -> Option<Receiver<PathBuf>> {
    listen_waking(app, settings, || {})
}

/// `listen`, calling `wake` from the listener thread after each launch's paths are queued, so the
/// shell can drain them at once instead of on a timer. What a launch asked beyond opening its
/// files is dropped: see [`listen_requests_waking`].
pub fn listen_waking(
    app: &str,
    settings: &Path,
    wake: impl Fn() + Send + 'static,
) -> Option<Receiver<PathBuf>> {
    let (sender, receiver) = channel();
    serve(app, settings, wake, move |request| {
        _ = sender.send(request.path);
    })?;
    Some(receiver)
}

/// [`listen_waking`] handing over whole requests: the file and what the launch asked for it. A
/// window that wants to know about them calls this instead; it must not also call `listen_waking`.
pub fn listen_requests_waking(
    app: &str,
    settings: &Path,
    wake: impl Fn() + Send + 'static,
) -> Option<Receiver<Request>> {
    let (sender, receiver) = channel();
    serve(app, settings, wake, move |request| {
        _ = sender.send(request);
    })?;
    Some(receiver)
}

/// How long a message's text may take to arrive, in all.
const READ_DEADLINE: Duration = Duration::from_secs(2);

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

/// Binds the port, records it, and starts the thread that calls `deliver` with the request of each
/// existing file a message names, then `wake`.
///
/// A message may be followed by the text its `?text=<bytes>` line announces: it is read here, up to
/// [`MAX_TEXT`] bytes within [`READ_DEADLINE`], and attached to each request. A message that
/// announces more, or whose text is late, short or not UTF-8, gets no `ok` and delivers nothing.
fn serve(
    app: &str,
    settings: &Path,
    wake: impl Fn() + Send + 'static,
    deliver: impl Fn(Request) + Send + 'static,
) -> Option<()> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    if let Some(dir) = settings.parent() {
        _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(port_file(settings), port.to_string()).ok()?;
    let lock = hold_lock(settings);
    let app = app.to_string();
    std::thread::spawn(move || {
        // Held until the process ends: this thread never returns.
        let _lock = lock;
        for stream in listener.incoming().flatten() {
            _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut reader = BufReader::new(&stream);
            let mut lines = (&mut reader).lines().map_while(Result::ok);
            if lines.next().as_deref() != Some(app.as_str()) {
                continue;
            }
            let body: Vec<String> = lines.take_while(|l| !l.is_empty()).collect();
            let text = match Request::text_length(body.iter().map(String::as_str)) {
                None => None,
                Some(n) if n > MAX_TEXT => continue,
                Some(n) => match read_text(&mut reader, n) {
                    Some(text) => Some(text),
                    None => continue,
                },
            };
            for mut request in Request::parse(body.iter().map(String::as_str)) {
                if request.path.is_file() {
                    request.text = text.clone();
                    deliver(request);
                }
            }
            _ = (&stream).write_all(b"ok\n");
            wake();
        }
    });
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str = "test-app";

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

    /// The listener of the release before text existed, as it was: the same lines, the same reads.
    fn old_serve(app: &'static str, delivered: std::sync::mpsc::Sender<Request>) -> u16 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut lines = BufReader::new(&stream).lines().map_while(Result::ok);
                if lines.next().as_deref() != Some(app) {
                    continue;
                }
                let body: Vec<String> = lines.take_while(|l| !l.is_empty()).collect();
                for request in Request::parse(body.iter().map(String::as_str)) {
                    if request.path.is_file() {
                        _ = delivered.send(request);
                    }
                }
                _ = (&stream).write_all(b"ok\n");
            }
        });
        port
    }

    fn ask(port: u16, message: &[u8]) -> bool {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(300)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(1500)))
            .unwrap();
        // A receiver that closes with bytes unread resets the connection: the write or the read
        // then fails, which is an answer of "no" like any other.
        if stream.write_all(message).is_err() {
            return false;
        }
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply).is_ok() && reply.trim() == "ok"
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
}
