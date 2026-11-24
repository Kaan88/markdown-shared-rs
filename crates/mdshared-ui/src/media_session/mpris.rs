//! The Linux backend: an MPRIS service (`org.mpris.MediaPlayer2` and its `Player`) on the
//! session bus, served by zbus on its own thread.
//!
//! The desktop's applet reads the properties when it likes and is told of changes by
//! `PropertiesChanged`, which this sends itself through the blocking connection. The cover is
//! a PNG file in the user's runtime folder (`$XDG_RUNTIME_DIR`, else the temporary folder),
//! named anew for each picture so that applets that keep pictures by URL load the new one,
//! and removed when it is replaced and when the session ends.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use zbus::blocking::{Connection, connection};
use zbus::zvariant::{ObjectPath, OwnedValue, Value};
use zbus::{fdo, interface};

use super::{Changes, Command, Identity, Inbox, NowPlaying, PlayState};

const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

fn err(e: zbus::Error) -> io::Error {
    io::Error::other(e)
}

/// What the service answers from, shared with zbus's thread.
#[derive(Default)]
struct Shown {
    now: NowPlaying,
    /// Counts tracks, for the track id.
    track: u64,
    art_url: String,
}

impl Shown {
    fn track_id(&self) -> String {
        format!("/org/mdshared/MediaSession/Track{}", self.track)
    }
}

fn status(state: PlayState) -> &'static str {
    match state {
        PlayState::Playing => "Playing",
        PlayState::Paused => "Paused",
        PlayState::Stopped => "Stopped",
    }
}

fn micros(d: Duration) -> i64 {
    i64::try_from(d.as_micros()).unwrap_or(i64::MAX)
}

fn owned(v: Value<'_>) -> Option<OwnedValue> {
    OwnedValue::try_from(v).ok()
}

/// The `Metadata` property: the track id, the length in µs, the title, the artists, the
/// album and the cover's URL, each only when there is one.
fn metadata(shown: &Shown) -> HashMap<String, OwnedValue> {
    let now = &shown.now;
    let mut m = HashMap::new();
    let id = shown.track_id();
    if let Some(v) = ObjectPath::try_from(id.as_str())
        .ok()
        .and_then(|p| owned(Value::from(p)))
    {
        m.insert("mpris:trackid".to_string(), v);
    }
    let mut put = |key: &str, v: Option<Value<'_>>| {
        if let Some(v) = v.and_then(owned) {
            m.insert(key.to_string(), v);
        }
    };
    put("mpris:length", now.length.map(|l| Value::from(micros(l))));
    put(
        "xesam:title",
        (!now.title.is_empty()).then(|| Value::from(now.title.as_str())),
    );
    put(
        "xesam:artist",
        (!now.artist.is_empty()).then(|| Value::from(vec![now.artist.as_str()])),
    );
    put(
        "xesam:album",
        (!now.album.is_empty()).then(|| Value::from(now.album.as_str())),
    );
    put(
        "mpris:artUrl",
        (!shown.art_url.is_empty()).then(|| Value::from(shown.art_url.as_str())),
    );
    m
}

/// A `file://` URL for an absolute path: bytes other than letters, digits and `-._~/`
/// written as `%XX`.
fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for &b in path.as_os_str().as_encoded_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

struct Root {
    identity: String,
    desktop_entry: String,
    inbox: Arc<Inbox>,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        self.inbox.push(Command::Raise);
    }

    fn quit(&self) {
        self.inbox.push(Command::Quit);
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> String {
        self.identity.clone()
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        self.desktop_entry.clone()
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

struct Player {
    shown: Arc<Mutex<Shown>>,
    inbox: Arc<Inbox>,
}

impl Player {
    fn with<T>(&self, f: impl FnOnce(&Shown) -> T) -> T {
        f(&self.shown.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        self.inbox.push(Command::Next);
    }

    fn previous(&self) {
        self.inbox.push(Command::Previous);
    }

    fn pause(&self) {
        self.inbox.push(Command::Pause);
    }

    fn play_pause(&self) {
        self.inbox.push(Command::PlayPause);
    }

    fn stop(&self) {
        self.inbox.push(Command::Stop);
    }

    fn play(&self) {
        self.inbox.push(Command::Play);
    }

    fn seek(&self, offset: i64) {
        if self.with(|s| s.now.can_seek) {
            self.inbox.push(Command::SeekBy(offset as f64 / 1e6));
        }
    }

    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
        // The spec: a stale track id, or a place outside the track, is ignored.
        let wanted = self.with(|s| {
            let length = s.now.length.map_or(i64::MAX, micros);
            s.now.can_seek && track_id.as_str() == s.track_id() && (0..=length).contains(&position)
        });
        if wanted {
            self.inbox.push(Command::SetPosition(Duration::from_micros(
                position.unsigned_abs(),
            )));
        }
    }

    fn open_uri(&self, uri: &str) -> fdo::Result<()> {
        _ = uri;
        Err(fdo::Error::NotSupported("this player opens no URIs".into()))
    }

    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.with(|s| status(s.now.state).to_string())
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        self.with(metadata)
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        1.0
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        self.with(|s| micros(s.now.position))
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.with(|s| s.now.can_next)
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.with(|s| s.now.can_previous)
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.with(|s| s.now.can_seek)
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

pub(super) struct Mpris {
    connection: Connection,
    shown: Arc<Mutex<Shown>>,
    art_dir: PathBuf,
    art_file: Option<PathBuf>,
    pictures: u64,
}

impl Mpris {
    pub(super) fn open(identity: &Identity, inbox: Arc<Inbox>) -> io::Result<Mpris> {
        let shown = Arc::new(Mutex::new(Shown::default()));
        let build = |name: String| -> zbus::Result<Connection> {
            connection::Builder::session()?
                .name(name)?
                .serve_at(
                    PATH,
                    Root {
                        identity: identity.name.to_string(),
                        desktop_entry: identity.desktop_entry.to_string(),
                        inbox: inbox.clone(),
                    },
                )?
                .serve_at(
                    PATH,
                    Player {
                        shown: shown.clone(),
                        inbox: inbox.clone(),
                    },
                )?
                .build()
        };
        let name = format!("org.mpris.MediaPlayer2.{}", identity.bus_name);
        // A second instance of the player takes a name of its own, as the spec says.
        let connection = build(name.clone())
            .or_else(|_| build(format!("{name}.instance{}", std::process::id())))
            .map_err(err)?;
        let art_dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|d| d.is_absolute())
            .unwrap_or_else(std::env::temp_dir);
        Ok(Mpris {
            connection,
            shown,
            art_dir,
            art_file: None,
            pictures: 0,
        })
    }

    /// Writes the cover as a new file and removes the one before; "" when there is none or it
    /// could not be written.
    fn store_art(&mut self, now: &NowPlaying) -> String {
        if let Some(old) = self.art_file.take() {
            _ = std::fs::remove_file(old);
        }
        let Some(image) = &now.art else {
            return String::new();
        };
        self.pictures += 1;
        let path = self.art_dir.join(format!(
            "mdshared-cover-{}-{}.png",
            std::process::id(),
            self.pictures
        ));
        match std::fs::write(&path, mdshared_text::image::encode_png(image)) {
            Ok(()) => {
                let url = file_url(&path);
                self.art_file = Some(path);
                url
            }
            Err(_) => String::new(),
        }
    }

    pub(super) fn send(&mut self, now: &NowPlaying, what: Changes) -> io::Result<()> {
        let art_url = what.art.then(|| self.store_art(now));
        let mut changed: HashMap<&str, OwnedValue> = HashMap::new();
        {
            let mut s = self.shown.lock().unwrap_or_else(PoisonError::into_inner);
            if what.track {
                s.track += 1;
            }
            if let Some(url) = art_url {
                s.art_url = url;
            }
            s.now = now.clone();
            let mut put = |key, v: Value<'_>| {
                if let Some(v) = owned(v) {
                    changed.insert(key, v);
                }
            };
            if what.track || what.art {
                put("Metadata", Value::from(metadata(&s)));
            }
            if what.state {
                put("PlaybackStatus", Value::from(status(now.state)));
            }
            if what.abilities {
                put("CanGoNext", Value::from(now.can_next));
                put("CanGoPrevious", Value::from(now.can_previous));
                put("CanSeek", Value::from(now.can_seek));
            }
        }
        if changed.is_empty() {
            return Ok(());
        }
        self.connection
            .emit_signal(
                None::<&str>,
                PATH,
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                &(PLAYER, changed, Vec::<&str>::new()),
            )
            .map_err(err)
    }

    pub(super) fn seeked(&mut self, position: Duration) -> io::Result<()> {
        self.connection
            .emit_signal(None::<&str>, PATH, PLAYER, "Seeked", &micros(position))
            .map_err(err)
    }
}

impl Drop for Mpris {
    fn drop(&mut self) {
        if let Some(old) = self.art_file.take() {
            _ = std::fs::remove_file(old);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_metadata_holds_only_what_there_is() {
        let mut shown = Shown {
            track: 3,
            ..Shown::default()
        };
        shown.now.title = "Song".into();
        shown.now.length = Some(Duration::from_secs(2));
        let m = metadata(&shown);
        let mut keys: Vec<&str> = m.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["mpris:length", "mpris:trackid", "xesam:title"]);
        assert_eq!(i64::try_from(&m["mpris:length"]).unwrap(), 2_000_000);
    }

    #[test]
    fn a_file_url_escapes_what_a_url_cannot_hold() {
        assert_eq!(
            file_url(Path::new("/run/user/1000/a b%.png")),
            "file:///run/user/1000/a%20b%25.png"
        );
    }

    #[test]
    fn the_status_is_named_as_the_spec_names_it() {
        assert_eq!(status(PlayState::Playing), "Playing");
        assert_eq!(status(PlayState::Stopped), "Stopped");
    }
}
