//! The system's now-playing surface and media keys: what a player is playing shown where the
//! system shows it (the media flyout and lock screen on Windows, the desktop's media applet
//! through MPRIS on Linux), and the play, pause, next, previous and seek the system sends back.
//!
//! The model is plain: the player fills a [`NowPlaying`] and gives it to
//! [`MediaSession::set`] whenever something in it changes (a new track, play or pause, a
//! position now and then); the session sends the system only what changed ([`changes`]). What
//! the system asks for arrives on its own threads, is queued, and the `wake` the player gave is
//! called (send the window an event); the next frame takes the queue with
//! [`MediaSession::commands`] and acts on each [`Command`] as on its own buttons. There are no
//! callbacks into the player.
//!
//! The backends sit behind the `media-session` feature: on Windows the
//! SystemMediaTransportControls of the window (through `ISystemMediaTransportControlsInterop`,
//! the `windows` crate), on Linux an MPRIS service on the session bus (zbus, already there for
//! the settings portal). Without the feature, and on macOS for now, [`MediaSession::new`]
//! answers `Unsupported` and the player goes on without one. The model types are there
//! everywhere, so the player's code does not change with the feature.
//!
//! Status: on Windows the example (`cargo run -p mdshared-ui --features media-session
//! --example media_session`) was run and checked through the system's own session manager
//! (`GlobalSystemMediaTransportControlsSessionManager`): it saw the title, artist, album,
//! state, timeline and cover, and its pause, next and seek arrived as commands; the flyout
//! itself was not looked at by a person. The Linux backend is only type-checked (from
//! Windows, and in CI); it is **unverified** against a real session bus.
//!
//! Deliberately not here: macOS (MPNowPlayingInfoCenter; roadmap M1 keeps it open); a track
//! list, loop and shuffle, the rate and the volume (MPRIS's are answered as fixed); playing
//! anything; and the media keys of a keyboard with no system surface behind them (winit gives
//! those as keys).

use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mdshared_text::image::Image;

#[cfg(all(target_os = "linux", feature = "media-session"))]
mod mpris;
#[cfg(all(windows, feature = "media-session"))]
mod smtc;

/// Whether the player plays.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PlayState {
    /// Nothing loaded, or stopped at the start.
    #[default]
    Stopped,
    Playing,
    Paused,
}

/// What the player plays, as the system shows it.
#[derive(Clone, Debug, Default)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// The cover; compared by pointer, so the same `Arc` given again is not sent again.
    pub art: Option<Arc<Image>>,
    /// The track's length; `None` for a stream.
    pub length: Option<Duration>,
    /// Where playing is, as of this call.
    pub position: Duration,
    pub state: PlayState,
    pub can_previous: bool,
    pub can_next: bool,
    pub can_seek: bool,
}

/// What [`changes`] found different between two [`NowPlaying`]s: each says which part of the
/// system's view to send again.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Changes {
    /// Title, artist, album or length.
    pub track: bool,
    pub art: bool,
    pub state: bool,
    pub position: bool,
    /// Previous, next or seek allowed.
    pub abilities: bool,
}

impl Changes {
    /// Everything: what the first [`MediaSession::set`] sends.
    pub const ALL: Changes = Changes {
        track: true,
        art: true,
        state: true,
        position: true,
        abilities: true,
    };

    /// Whether anything changed.
    pub fn any(self) -> bool {
        self != Changes::default()
    }
}

/// What differs from `old` (none: everything) to `new`. The art counts as changed when it is
/// another `Arc` or one is missing, not when the pixels differ.
pub fn changes(old: Option<&NowPlaying>, new: &NowPlaying) -> Changes {
    let Some(old) = old else {
        return Changes::ALL;
    };
    Changes {
        track: old.title != new.title
            || old.artist != new.artist
            || old.album != new.album
            || old.length != new.length,
        art: match (&old.art, &new.art) {
            (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
            (None, None) => false,
            _ => true,
        },
        state: old.state != new.state,
        position: old.position != new.position,
        abilities: old.can_previous != new.can_previous
            || old.can_next != new.can_next
            || old.can_seek != new.can_seek,
    }
}

/// What the system asks the player to do.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Command {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    /// Move by this many seconds, back when negative.
    SeekBy(f64),
    /// Move to this place in the track.
    SetPosition(Duration),
    /// Bring the window forward (MPRIS's Raise).
    Raise,
    /// Quit the player (MPRIS's Quit).
    Quit,
}

/// How the player is named to the system.
#[derive(Clone, Copy, Debug)]
pub struct Identity<'a> {
    /// The name people read: "mdplay".
    pub name: &'a str,
    /// A name for the bus, letters, digits and `_` only (MPRIS's
    /// `org.mpris.MediaPlayer2.<bus_name>`; a second instance adds `.instance<pid>`).
    pub bus_name: &'a str,
    /// The desktop file's name without `.desktop`, so the applet finds the icon.
    pub desktop_entry: &'a str,
}

/// The queue the system's threads add to. Without a backend nothing adds to it.
pub(crate) struct Inbox {
    commands: Mutex<Vec<Command>>,
    #[cfg_attr(
        not(all(any(windows, target_os = "linux"), feature = "media-session")),
        allow(dead_code)
    )]
    wake: Box<dyn Fn() + Send + Sync>,
}

impl Inbox {
    #[cfg_attr(
        not(all(any(windows, target_os = "linux"), feature = "media-session")),
        allow(dead_code)
    )]
    pub(crate) fn push(&self, command: Command) {
        self.commands
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(command);
        (self.wake)();
    }

    fn take(&self) -> Vec<Command> {
        std::mem::take(&mut *self.commands.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

#[cfg(all(windows, feature = "media-session"))]
type Backend = smtc::Smtc;
#[cfg(all(target_os = "linux", feature = "media-session"))]
type Backend = mpris::Mpris;
#[cfg(not(all(any(windows, target_os = "linux"), feature = "media-session")))]
type Backend = ();

/// The player's place on the system's now-playing surface. Dropping it takes the player off.
pub struct MediaSession {
    inbox: Arc<Inbox>,
    last: Option<NowPlaying>,
    backend: Backend,
}

impl MediaSession {
    /// Puts the player on the system's surface. `window` is the window's handle as an integer
    /// (the HWND on Windows, where the controls belong to a window; not used elsewhere). `wake`
    /// is called from the system's threads when a command waits; it must be cheap and must not
    /// block. `Unsupported` without the `media-session` feature and on macOS; otherwise the
    /// system's error (no session bus, a window that is not one).
    pub fn new(
        identity: &Identity,
        window: isize,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> io::Result<MediaSession> {
        let inbox = Arc::new(Inbox {
            commands: Mutex::new(Vec::new()),
            wake: Box::new(wake),
        });
        Ok(MediaSession {
            backend: open(identity, window, inbox.clone())?,
            inbox,
            last: None,
        })
    }

    /// Shows `now`: sends the system what changed since the last call (everything the first
    /// time). Call it on a new track, on play and pause, after a seek, and every few seconds
    /// while playing so the system's position stays near. An error leaves the system showing
    /// what it had; the next call sends everything again.
    pub fn set(&mut self, now: &NowPlaying) -> io::Result<()> {
        let what = changes(self.last.as_ref(), now);
        if !what.any() {
            return Ok(());
        }
        match send(&mut self.backend, now, what) {
            Ok(()) => {
                self.last = Some(now.clone());
                Ok(())
            }
            Err(e) => {
                self.last = None;
                Err(e)
            }
        }
    }

    /// Says the position jumped (a seek, a skip back) rather than moved on with playing, so
    /// listeners that follow the position move too (MPRIS's Seeked signal). Also sends the
    /// position as [`set`](Self::set) does.
    pub fn seeked(&mut self, position: Duration) -> io::Result<()> {
        let Some(mut now) = self.last.clone() else {
            return Ok(());
        };
        now.position = position;
        self.set(&now)?;
        jumped(&mut self.backend, position)
    }

    /// The commands the system sent since the last call, oldest first.
    pub fn commands(&self) -> Vec<Command> {
        self.inbox.take()
    }
}

#[cfg(all(windows, feature = "media-session"))]
fn open(identity: &Identity, window: isize, inbox: Arc<Inbox>) -> io::Result<Backend> {
    smtc::Smtc::open(identity, window, inbox)
}
#[cfg(all(target_os = "linux", feature = "media-session"))]
fn open(identity: &Identity, window: isize, inbox: Arc<Inbox>) -> io::Result<Backend> {
    _ = window;
    mpris::Mpris::open(identity, inbox)
}
#[cfg(not(all(any(windows, target_os = "linux"), feature = "media-session")))]
fn open(identity: &Identity, window: isize, inbox: Arc<Inbox>) -> io::Result<Backend> {
    _ = (identity, window, inbox);
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no media session on this system, or the media-session feature is off",
    ))
}

#[cfg(all(any(windows, target_os = "linux"), feature = "media-session"))]
fn send(backend: &mut Backend, now: &NowPlaying, what: Changes) -> io::Result<()> {
    backend.send(now, what)
}
#[cfg(not(all(any(windows, target_os = "linux"), feature = "media-session")))]
fn send(backend: &mut Backend, now: &NowPlaying, what: Changes) -> io::Result<()> {
    _ = (backend, now, what);
    Ok(())
}

#[cfg(all(any(windows, target_os = "linux"), feature = "media-session"))]
fn jumped(backend: &mut Backend, position: Duration) -> io::Result<()> {
    backend.seeked(position)
}
#[cfg(not(all(any(windows, target_os = "linux"), feature = "media-session")))]
fn jumped(backend: &mut Backend, position: Duration) -> io::Result<()> {
    _ = (backend, position);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> NowPlaying {
        NowPlaying {
            title: "Song".into(),
            artist: "Band".into(),
            album: "Record".into(),
            length: Some(Duration::from_secs(200)),
            state: PlayState::Playing,
            can_next: true,
            ..NowPlaying::default()
        }
    }

    #[test]
    fn the_first_time_everything_counts_as_changed() {
        assert_eq!(changes(None, &track()), Changes::ALL);
        assert!(Changes::ALL.any());
        assert!(!changes(Some(&track()), &track()).any());
    }

    #[test]
    fn each_part_is_found_changed_on_its_own() {
        let a = track();
        let mut b = a.clone();
        b.position = Duration::from_secs(5);
        assert_eq!(
            changes(Some(&a), &b),
            Changes {
                position: true,
                ..Changes::default()
            }
        );
        let mut b = a.clone();
        b.state = PlayState::Paused;
        b.can_seek = true;
        assert_eq!(
            changes(Some(&a), &b),
            Changes {
                state: true,
                abilities: true,
                ..Changes::default()
            }
        );
        let mut b = a.clone();
        b.length = None;
        assert!(changes(Some(&a), &b).track);
    }

    #[test]
    fn the_art_is_compared_by_pointer() {
        let pic = Arc::new(Image::new(1, 1, vec![1, 2, 3, 255]).unwrap());
        let mut a = track();
        a.art = Some(pic.clone());
        let mut b = track();
        b.art = Some(pic.clone());
        assert!(!changes(Some(&a), &b).art);
        b.art = Some(Arc::new((*pic).clone()));
        assert!(changes(Some(&a), &b).art);
        b.art = None;
        assert!(changes(Some(&a), &b).art);
    }

    #[test]
    fn commands_queue_in_order_and_each_wakes_the_player() {
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let w = woken.clone();
        let inbox = Inbox {
            commands: Mutex::new(Vec::new()),
            wake: Box::new(move || {
                w.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }),
        };
        inbox.push(Command::Pause);
        inbox.push(Command::SeekBy(-5.0));
        assert_eq!(woken.load(std::sync::atomic::Ordering::Relaxed), 2);
        assert_eq!(inbox.take(), vec![Command::Pause, Command::SeekBy(-5.0)]);
        assert!(inbox.take().is_empty());
    }

    #[cfg(not(all(any(windows, target_os = "linux"), feature = "media-session")))]
    #[test]
    fn without_a_backend_there_is_no_session() {
        let id = Identity {
            name: "test",
            bus_name: "test",
            desktop_entry: "test",
        };
        let e = MediaSession::new(&id, 0, || {}).err().unwrap();
        assert_eq!(e.kind(), io::ErrorKind::Unsupported);
    }
}
