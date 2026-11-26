//! Owns the native watcher off the application thread. Registration still answers
//! synchronously, but dropping the handle only disconnects the owner's stop channel.

use std::sync::mpsc::{Sender, channel, sync_channel};

/// Starts the owner and waits for registration to finish. The native watcher is created and
/// destroyed on that thread, independently of a metadata worker that may be stuck in a read.
pub(super) fn start<T, R: Send + 'static>(
    register: impl FnOnce() -> Option<(T, R)> + Send + 'static,
) -> Option<(Sender<()>, R)> {
    let (stop, stopped) = channel();
    let (ready, registered) = sync_channel(0);
    std::thread::Builder::new()
        .name("mdshared-dirwatch-backend".to_string())
        .spawn(move || {
            let Some((watcher, result)) = register() else {
                return;
            };
            if ready.send(result).is_ok() {
                _ = stopped.recv();
            }
            drop(watcher);
        })
        .ok()?;
    Some((stop, registered.recv().ok()?))
}
