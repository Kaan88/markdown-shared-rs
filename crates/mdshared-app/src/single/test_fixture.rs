//! Keep the main-branch hand-over tests' decimal-port fixtures separate from the real
//! authenticated endpoints. The fixture adapts their setup and raw socket probes, not
//! their assertions or the transport used by the public APIs.

use super::{Delivery, File, SocketAddr, wire};
use super::{Duration, Ipv4Addr, Path, PathBuf, Receiver, Request, TcpListener, TcpStream};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::Sender;

struct OldPayload {
    port: u16,
    app: &'static str,
    delivered: Sender<Request>,
}

thread_local! {
    static AUTHENTICATED: RefCell<HashMap<u16, PathBuf>> = RefCell::default();
    static OLD_PAYLOAD: RefCell<Option<OldPayload>> = const { RefCell::new(None) };
}

fn endpoint(settings: &Path) -> PathBuf {
    // Tests sometimes give two independent listeners settings files in the same folder.
    settings
        .with_file_name("fixture-endpoints")
        .join(settings.file_name().unwrap())
        .join("settings.ini")
}

pub(super) fn listen(app: &str, settings: &Path) -> Option<Receiver<PathBuf>> {
    super::listen(app, &endpoint(settings))
}

pub(super) fn listen_waking(
    app: &str,
    settings: &Path,
    wake: impl Fn() + Send + 'static,
) -> Option<Receiver<PathBuf>> {
    super::listen_waking(app, &endpoint(settings), wake)
}

pub(super) fn listen_requests_waking(
    app: &str,
    settings: &Path,
    wake: impl Fn() + Send + 'static,
) -> Option<Receiver<Request>> {
    let endpoint = endpoint(settings);
    let receiver = super::listen_requests_waking(app, &endpoint, wake)?;
    let port = super::instance_record(&endpoint).unwrap().port;
    std::fs::write(port_file(settings), port.to_string()).unwrap();
    AUTHENTICATED.with(|peers| peers.borrow_mut().insert(port, endpoint));
    Some(receiver)
}

pub(super) fn forward(app: &str, settings: &Path, paths: &[PathBuf]) -> bool {
    super::forward(app, &endpoint(settings), paths)
}

pub(super) fn forward_request(app: &str, settings: &Path, request: &Request) -> bool {
    super::forward_request(app, &endpoint(settings), request)
}

pub(super) fn deliver_request(app: &str, settings: &Path, request: &Request) -> Delivery {
    super::deliver_request(app, &endpoint(settings), request)
}

pub(super) fn alive(settings: &Path) -> bool {
    super::alive(&endpoint(settings))
}

pub(super) fn wait_gone(settings: &Path, timeout: Duration) -> bool {
    super::wait_gone(&endpoint(settings), timeout)
}

pub(super) fn port_file(settings: &Path) -> PathBuf {
    settings.with_file_name("instance.port")
}

pub(super) fn lock_file(settings: &Path) -> PathBuf {
    // The stale-port fixture has already written its decimal port. Publish the equivalent
    // authenticated record, then drop the lock as a departed process would.
    drop(hold_lock(settings));
    settings.with_file_name("instance.lock")
}

pub(super) fn hold_lock(settings: &Path) -> File {
    let port: u16 = std::fs::read_to_string(port_file(settings))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let old = OLD_PAYLOAD.with(|peer| peer.borrow_mut().take());
    let dir = super::directory(&endpoint(settings), true).unwrap();
    let registration = super::hold_lock(&dir, std::thread::sleep).unwrap();
    if let Some(old) = old {
        assert_eq!(old.port, port);
        // Test the old payload reader over the authenticated transport as well. The
        // original port remains plaintext and can only be reached by the raw test probe.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let (lock, record) = registration
            .publish(&dir, listener.local_addr().unwrap().port())
            .unwrap();
        old_payload_peer(listener, old.app, old.delivered, Some(record.key));
        lock
    } else {
        registration.publish(&dir, port).unwrap().0
    }
}

pub(super) fn old_serve(app: &'static str, delivered: Sender<Request>) -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    OLD_PAYLOAD.with(|peer| {
        *peer.borrow_mut() = Some(OldPayload {
            port,
            app,
            delivered: delivered.clone(),
        });
    });
    old_payload_peer(listener, app, delivered, None);
    port
}

fn old_payload_peer(
    listener: TcpListener,
    app: &'static str,
    delivered: Sender<Request>,
    key: Option<[u8; 32]>,
) {
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut reader = BufReader::new(&stream);
            if let Some(key) = &key
                && wire::authenticate(&mut reader, &mut &stream, key).is_none()
            {
                continue;
            }
            let mut lines = reader.lines().map_while(Result::ok);
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
}

pub(super) fn ask(port: u16, message: &[u8]) -> bool {
    if let Some(settings) = AUTHENTICATED.with(|peers| peers.borrow().get(&port).cloned()) {
        return super::send(&settings, message);
    }
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(300)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(1500)))
        .unwrap();
    // A historical reader may reset the connection with unread text after its header.
    if stream.write_all(message).is_err() {
        return false;
    }
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).is_ok() && reply.trim() == "ok"
}
