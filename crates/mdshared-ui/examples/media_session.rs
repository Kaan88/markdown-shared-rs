//! Puts a made-up track on the system's now-playing surface and prints what the system sends
//! back: `cargo run -p mdshared-ui --features media-session --example media_session -- [seconds]`.
//!
//! On Windows open the media flyout (the volume flyout, or Win+A) or press a media key; on
//! Linux use the desktop's media applet or `playerctl -p mdshared_example status`. Play, pause
//! and the seek bar move the fake player; next and previous change the track. It exits after
//! the given seconds (default 60). Without the feature it prints why there is no session.

use std::sync::Arc;
use std::time::{Duration, Instant};

use mdshared_text::image::Image;
use mdshared_ui::media_session::{Command, Identity, MediaSession, NowPlaying, PlayState};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowId};

/// A cover of two colours, different for each track.
fn cover(track: usize) -> Arc<Image> {
    let (a, b) = [
        ([200, 60, 40], [40, 60, 200]),
        ([30, 160, 90], [240, 200, 40]),
    ][track % 2];
    let mut rgba = Vec::with_capacity(64 * 64 * 4);
    for y in 0..64 {
        for x in 0..64 {
            let c = if (x + y) < 64 { a } else { b };
            rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    Arc::new(Image::new(64, 64, rgba).unwrap())
}

struct App {
    until: Instant,
    window: Option<Window>,
    session: Option<MediaSession>,
    now: NowPlaying,
    track: usize,
    /// When playing started, and where it was then.
    since: Option<(Instant, Duration)>,
    last_sent: Instant,
}

impl App {
    fn position(&self) -> Duration {
        match self.since {
            Some((at, from)) => (from + at.elapsed()).min(self.now.length.unwrap_or_default()),
            None => self.now.position,
        }
    }

    fn load(&mut self, track: usize) {
        self.track = track;
        self.now = NowPlaying {
            title: format!("Example track {}", track + 1),
            artist: "mdshared".into(),
            album: "Media session example".into(),
            art: Some(cover(track)),
            length: Some(Duration::from_secs(180)),
            position: Duration::ZERO,
            state: PlayState::Playing,
            can_previous: track > 0,
            can_next: true,
            can_seek: true,
        };
        self.since = Some((Instant::now(), Duration::ZERO));
    }

    fn send(&mut self) {
        self.now.position = self.position();
        if let Some(Err(e)) = self.session.as_mut().map(|s| s.set(&self.now)) {
            println!("set failed: {e}");
        }
        self.last_sent = Instant::now();
    }

    fn act(&mut self, c: Command) {
        println!("the system sent {c:?}");
        let pos = self.position();
        match c {
            Command::Play => self.since = Some((Instant::now(), pos)),
            Command::Pause | Command::Stop => {
                self.now.position = pos;
                self.since = None;
            }
            Command::PlayPause => {
                self.now.position = pos;
                self.since = match self.since {
                    Some(_) => None,
                    None => Some((Instant::now(), pos)),
                }
            }
            Command::Next => self.load(self.track + 1),
            Command::Previous => self.load(self.track.saturating_sub(1)),
            Command::SeekBy(s) => {
                let to = Duration::from_secs_f64((pos.as_secs_f64() + s).max(0.0));
                self.seek(to);
            }
            Command::SetPosition(to) => self.seek(to),
            _ => {}
        }
        self.now.state = if self.since.is_some() {
            PlayState::Playing
        } else {
            PlayState::Paused
        };
        self.send();
    }

    fn seek(&mut self, to: Duration) {
        self.now.position = to;
        if self.since.is_some() {
            self.since = Some((Instant::now(), to));
        }
        if let Some(s) = &mut self.session {
            _ = s.seeked(to);
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("media session example")
            .with_inner_size(LogicalSize::new(360.0, 120.0));
        let window = event_loop.create_window(attrs).unwrap();
        let handle = match window.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
            _ => 0,
        };
        let id = Identity {
            name: "mdshared example",
            bus_name: "mdshared_example",
            desktop_entry: "mdshared-example",
        };
        match MediaSession::new(&id, handle, || {}) {
            Ok(s) => {
                println!("on the system's now-playing surface");
                self.session = Some(s);
            }
            Err(e) => println!("no media session: {e}"),
        }
        self.window = Some(window);
        self.load(0);
        self.send();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let WindowEvent::CloseRequested = event {
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if Instant::now() >= self.until {
            event_loop.exit();
            return;
        }
        let commands = self
            .session
            .as_ref()
            .map(MediaSession::commands)
            .unwrap_or_default();
        for c in commands {
            self.act(c);
        }
        if self.last_sent.elapsed() >= Duration::from_secs(5) {
            self.send();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(100),
        ));
    }
}

fn main() {
    let secs = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(60);
    let event_loop = EventLoop::new().unwrap();
    let mut app = App {
        until: Instant::now() + Duration::from_secs(secs),
        window: None,
        session: None,
        now: NowPlaying::default(),
        track: 0,
        since: None,
        last_sent: Instant::now(),
    };
    event_loop.run_app(&mut app).unwrap();
}
