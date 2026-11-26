//! Owns and restarts the native watcher off the application and metadata threads.
//! A stuck native destructor retains one process-wide slot until it returns.

use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Weak};

const MAX_OWNERS: usize = 64;
static OWNERS: Slots = Slots::new(MAX_OWNERS);

struct Slots {
    limit: usize,
    used: AtomicUsize,
}

impl Slots {
    const fn new(limit: usize) -> Self {
        Self {
            limit,
            used: AtomicUsize::new(0),
        }
    }

    fn claim(&self) -> io::Result<Slot<'_>> {
        self.used
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                (used < self.limit).then_some(used + 1)
            })
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    format!(
                        "directory watcher backend-owner limit ({}) reached",
                        self.limit
                    ),
                )
            })?;
        Ok(Slot(self))
    }
}

struct Slot<'a>(&'a Slots);

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.0.used.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Only the application handle owns a sender. Dropping it disconnects the channel even
/// if a slow metadata scan still holds a restart handle.
pub(super) struct Owner {
    requests: Arc<SyncSender<()>>,
    #[cfg(test)]
    pub(super) thread: Option<std::thread::JoinHandle<()>>,
}

pub(super) struct Restart(Weak<SyncSender<()>>);

impl Restart {
    /// At most one request may wait behind a busy owner.
    pub(super) fn request(&self) {
        if let Some(sender) = self.0.upgrade() {
            _ = sender.try_send(());
        }
    }
}

impl Owner {
    pub(super) fn restart_handle(&self) -> Restart {
        Restart(Arc::downgrade(&self.requests))
    }
}

/// Starts the owner and waits for registration. Reuse the native backend for restarts:
/// some backends do not wait for their own thread to exit when dropped.
pub(super) fn start<T, R: Send + 'static>(
    register: impl FnOnce() -> io::Result<(T, R)> + Send + 'static,
    restart: impl FnMut(&mut T) -> io::Result<()> + Send + 'static,
) -> io::Result<(Owner, R)> {
    start_with_slots(&OWNERS, register, restart)
}

fn start_with_slots<T, R: Send + 'static>(
    slots: &'static Slots,
    register: impl FnOnce() -> io::Result<(T, R)> + Send + 'static,
    mut restart: impl FnMut(&mut T) -> io::Result<()> + Send + 'static,
) -> io::Result<(Owner, R)> {
    let slot = slots.claim()?;
    let (requests, receive) = sync_channel(1);
    let requests = Arc::new(requests);
    let alive = Arc::downgrade(&requests);
    let (ready, registered) = sync_channel(1);
    let thread = std::thread::Builder::new()
        .name("mdshared-dirwatch-backend".to_string())
        .spawn(move || {
            let _slot = slot;
            let mut watcher = match register() {
                Ok((watcher, result)) => {
                    if ready.send(Ok(result)).is_err() {
                        return;
                    }
                    watcher
                }
                Err(error) => {
                    _ = ready.send(Err(error));
                    return;
                }
            };
            while receive.recv().is_ok() && alive.strong_count() != 0 {
                _ = restart(&mut watcher);
            }
            drop(watcher);
        })?;
    let owner = Owner {
        requests,
        #[cfg(test)]
        thread: Some(thread),
    };
    #[cfg(not(test))]
    drop(thread);
    let result = registered
        .recv()
        .map_err(|_| io::Error::other("directory watcher backend exited during registration"))??;
    Ok((owner, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_blocked_backend_keeps_its_slot_until_the_owner_exits() {
        static SLOTS: Slots = Slots::new(1);
        let (entered, entering) = sync_channel(1);
        let (release, gate) = sync_channel::<()>(0);
        struct Gated(SyncSender<()>, std::sync::mpsc::Receiver<()>);
        impl Drop for Gated {
            fn drop(&mut self) {
                _ = self.0.send(());
                _ = self.1.recv();
            }
        }

        let (mut owner, ()) =
            start_with_slots(&SLOTS, move || Ok((Gated(entered, gate), ())), |_| Ok(())).unwrap();
        let thread = owner.thread.take().unwrap();
        drop(owner);
        entering.recv_timeout(Duration::from_secs(5)).unwrap();
        let error = start_with_slots(&SLOTS, || Ok(((), ())), |_| Ok(()))
            .err()
            .expect("a stuck owner must still count");
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(error.to_string().contains("backend-owner limit"));
        drop(release);
        thread.join().unwrap();
        assert!(start_with_slots(&SLOTS, || Ok(((), ())), |_| Ok(())).is_ok());
    }

    #[test]
    fn restart_reuses_the_backend_and_retries_after_a_failure() {
        let (send, receive) = sync_channel(4);
        let (mut owner, ()) = start(
            || Ok((0, ())),
            move |calls| {
                *calls += 1;
                send.send(*calls).unwrap();
                if *calls == 1 {
                    return Err(io::Error::other("simulated registration failure"));
                }
                Ok(())
            },
        )
        .unwrap();
        let restart = owner.restart_handle();
        for expected in [1, 2] {
            restart.request();
            assert_eq!(
                receive.recv_timeout(Duration::from_secs(5)).unwrap(),
                expected
            );
        }
        let thread = owner.thread.take().unwrap();
        drop(owner);
        thread.join().unwrap();
    }
}
