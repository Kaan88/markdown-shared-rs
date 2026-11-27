//! Finish authenticated hand-overs: a single worker keeps every request in arrival order,
//! whether it needs a filesystem check or not. A full queue refuses one message; a disconnected
//! receiver releases registration before the client sees the failure.

use super::{CONNECT, Timed, connect};
use crate::sister::Request;
use std::fs::File;
use std::io::{self, Write};
use std::net::TcpStream;
use std::sync::mpsc::{Sender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

const QUEUE: usize = 16;
type Lifetime = Mutex<Option<File>>;

#[derive(Clone)]
pub(super) struct Stop {
    lifetime: Weak<Lifetime>,
    port: u16,
}

impl Stop {
    pub(super) fn new(port: u16, lock: File) -> (Self, Arc<Lifetime>) {
        let lifetime = Arc::new(Mutex::new(Some(lock)));
        (
            Self {
                lifetime: Arc::downgrade(&lifetime),
                port,
            },
            lifetime,
        )
    }

    pub(super) fn requested(&self) -> bool {
        self.lifetime
            .upgrade()
            .is_none_or(|lock| lock.lock().unwrap().is_none())
    }

    fn request(&self) {
        let Some(lifetime) = self.lifetime.upgrade() else {
            return;
        };
        {
            let mut lifetime = lifetime.lock().unwrap();
            let Some(lock) = lifetime.take() else { return };
            // Release before we close the request socket: the sender may immediately
            // try to register. Keep the mutex held until concurrent callers can see this.
            drop(lock);
        }
        // Wake a blocking accept. If the backlog is full it already has a connection.
        _ = connect(self.port, CONNECT);
    }
}

/// A complete authenticated message. An empty or entirely filtered launch remains a no-op.
pub(super) struct Pending {
    stream: TcpStream,
    requests: Vec<Request>,
    deadline: Instant,
}

impl Pending {
    pub(super) fn new(stream: TcpStream, requests: Vec<Request>, deadline: Instant) -> Self {
        Self {
            stream,
            requests,
            deadline,
        }
    }

    /// False when a final channel send discovers that the receiver has gone away.
    fn deliver<T>(
        self,
        stop: &Stop,
        sender: &Sender<T>,
        wake: impl Fn(),
        filter: impl Fn(Request) -> Option<T>,
    ) -> bool {
        let mut writer = Timed(&self.stream, self.deadline);
        let answer = if self.requests.iter().any(|request| request.doc.is_some()) {
            b"ok doc\n".as_slice()
        } else {
            b"ok\n"
        };
        let mut accepted = Vec::new();
        for request in self.requests {
            if writer.left().is_err() {
                return true;
            }
            accepted.extend(filter(request));
        }
        // Do not deliver a late filesystem result after the sender may have retried.
        if writer.left().is_err() {
            return true;
        }
        if !accepted
            .into_iter()
            .all(|request| sender.send(request).is_ok())
        {
            stop.request();
            return false;
        }
        _ = writer.write_all(answer);
        // Once queued, still wake the window if the sender disconnects during the reply.
        wake();
        true
    }
}

pub(super) fn filtered<T: Send + 'static>(
    sender: Sender<T>,
    wake: impl Fn() + Send + 'static,
    filter: impl Fn(Request) -> Option<T> + Send + 'static,
) -> io::Result<impl Fn(Pending, &Stop) -> bool + Send + 'static> {
    let (queue, batches) = sync_channel::<(Pending, Stop)>(QUEUE);
    std::thread::Builder::new().spawn(move || {
        for (pending, stop) in batches {
            if stop.requested() || !pending.deliver(&stop, &sender, &wake, &filter) {
                break;
            }
        }
    })?;
    Ok(move |pending: Pending, stop: &Stop| {
        // Full drops only this pending socket without an acknowledgement.
        !matches!(
            queue.try_send((pending, stop.clone())),
            Err(TrySendError::Disconnected(_))
        )
    })
}

#[cfg(test)]
#[test]
fn a_full_path_queue_refuses_one_message_without_stopping_the_listener() {
    use std::io::Read;
    use std::net::{Ipv4Addr, TcpListener};
    use std::sync::mpsc::channel;
    use std::time::Duration;

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let dir = std::env::temp_dir().join(format!("mdshared-path-queue-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let file = File::create_new(dir.join("lifetime")).unwrap();
    let (stop, lifetime) = Stop::new(port, file);
    let (tx, rx) = channel();
    let (entered, checking) = channel();
    let (release, gate) = channel::<()>();
    let gate = Mutex::new(gate);
    let test_deadline = Duration::from_secs(5);
    let dispatch = filtered(
        tx,
        || {},
        move |request| {
            if request.path == std::path::Path::new("blocked") {
                entered.send(()).unwrap();
                gate.lock().unwrap().recv_timeout(test_deadline).unwrap();
            }
            Some(request)
        },
    )
    .unwrap();
    let submit = |name| {
        let client = connect(port, CONNECT).unwrap();
        let (stream, _) = listener.accept().unwrap();
        let pending = Pending::new(
            stream,
            vec![Request::new(name)],
            Instant::now() + test_deadline,
        );
        assert!(dispatch(pending, &stop));
        client
    };
    let mut clients = Vec::new();
    clients.push(submit("blocked"));
    checking.recv_timeout(test_deadline).unwrap();
    for _ in 0..QUEUE {
        clients.push(submit("queued"));
    }
    let mut refused = submit("overloaded");
    refused.set_read_timeout(Some(test_deadline)).unwrap();
    assert_eq!(refused.read(&mut [0; 1]).unwrap(), 0, "no acknowledgement");
    assert!(!stop.requested());
    release.send(()).unwrap();
    for _ in 0..clients.len() {
        rx.recv_timeout(test_deadline).unwrap();
    }
    clients.push(submit("next"));
    assert_eq!(
        rx.recv_timeout(test_deadline).unwrap().path,
        std::path::Path::new("next")
    );
    drop(lifetime);
    std::fs::remove_dir_all(dir).unwrap();
}
