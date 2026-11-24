//! The Windows backend: the window's SystemMediaTransportControls.
//!
//! A desktop program has no `GetForCurrentView`; `ISystemMediaTransportControlsInterop`
//! gives the controls of a window instead. The flyout names the program by its executable
//! (or its AppUserModelID when it sets one), so [`Identity`] is not used here. The button and
//! seek events come on a thread of the system's pool and only queue a [`Command`].

use std::io;
use std::sync::Arc;
use std::time::Duration;

use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
    SystemMediaTransportControls, SystemMediaTransportControlsButton,
    SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::Storage::Streams::{
    DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
use windows::core::{HSTRING, Ref};

use super::{Changes, Command, Identity, Inbox, NowPlaying, PlayState};

fn err(e: windows::core::Error) -> io::Error {
    io::Error::other(e)
}

/// A time as WinRT counts it, in 100 ns.
fn span(d: Duration) -> TimeSpan {
    TimeSpan {
        Duration: i64::try_from(d.as_nanos() / 100).unwrap_or(i64::MAX),
    }
}

fn duration(t: TimeSpan) -> Duration {
    Duration::from_nanos(u64::try_from(t.Duration).unwrap_or(0).saturating_mul(100))
}

/// The command a button of the flyout or a media key stands for.
fn command(button: SystemMediaTransportControlsButton) -> Option<Command> {
    Some(match button {
        SystemMediaTransportControlsButton::Play => Command::Play,
        SystemMediaTransportControlsButton::Pause => Command::Pause,
        SystemMediaTransportControlsButton::Stop => Command::Stop,
        SystemMediaTransportControlsButton::Next => Command::Next,
        SystemMediaTransportControlsButton::Previous => Command::Previous,
        SystemMediaTransportControlsButton::FastForward => Command::SeekBy(10.0),
        SystemMediaTransportControlsButton::Rewind => Command::SeekBy(-10.0),
        _ => return None,
    })
}

pub(super) struct Smtc {
    controls: SystemMediaTransportControls,
    pressed: i64,
    moved: i64,
}

impl Smtc {
    pub(super) fn open(identity: &Identity, window: isize, inbox: Arc<Inbox>) -> io::Result<Smtc> {
        _ = identity;
        let interop = windows::core::factory::<
            SystemMediaTransportControls,
            ISystemMediaTransportControlsInterop,
        >()
        .map_err(err)?;
        // SAFETY: the interop call only reads the handle; a handle that is not a window is
        // refused with an error, not used.
        let controls: SystemMediaTransportControls =
            unsafe { interop.GetForWindow(HWND(window as *mut core::ffi::c_void)) }.map_err(err)?;
        let to = inbox.clone();
        let pressed = controls
            .ButtonPressed(&TypedEventHandler::new(
                move |_, args: Ref<SystemMediaTransportControlsButtonPressedEventArgs>| {
                    if let Some(c) = command(args.ok()?.Button()?) {
                        to.push(c);
                    }
                    Ok(())
                },
            ))
            .map_err(err)?;
        let moved = controls
            .PlaybackPositionChangeRequested(&TypedEventHandler::new(
                move |_, args: Ref<PlaybackPositionChangeRequestedEventArgs>| {
                    let at = args.ok()?.RequestedPlaybackPosition()?;
                    inbox.push(Command::SetPosition(duration(at)));
                    Ok(())
                },
            ))
            .map_err(err)?;
        let smtc = Smtc {
            controls,
            pressed,
            moved,
        };
        let c = &smtc.controls;
        c.SetIsPlayEnabled(true).map_err(err)?;
        c.SetIsPauseEnabled(true).map_err(err)?;
        c.SetIsStopEnabled(true).map_err(err)?;
        c.DisplayUpdater()
            .and_then(|u| u.SetType(MediaPlaybackType::Music))
            .map_err(err)?;
        c.SetIsEnabled(true).map_err(err)?;
        Ok(smtc)
    }

    pub(super) fn send(&mut self, now: &NowPlaying, what: Changes) -> io::Result<()> {
        let c = &self.controls;
        if what.track || what.art {
            let u = c.DisplayUpdater().map_err(err)?;
            u.SetType(MediaPlaybackType::Music).map_err(err)?;
            let m = u.MusicProperties().map_err(err)?;
            m.SetTitle(&HSTRING::from(now.title.as_str()))
                .map_err(err)?;
            m.SetArtist(&HSTRING::from(now.artist.as_str()))
                .map_err(err)?;
            m.SetAlbumTitle(&HSTRING::from(now.album.as_str()))
                .map_err(err)?;
            if what.art {
                let art = match &now.art {
                    Some(image) => Some(picture(image).map_err(err)?),
                    None => None,
                };
                u.SetThumbnail(art.as_ref()).map_err(err)?;
            }
            u.Update().map_err(err)?;
        }
        if what.state {
            let status = match now.state {
                PlayState::Playing => MediaPlaybackStatus::Playing,
                PlayState::Paused => MediaPlaybackStatus::Paused,
                PlayState::Stopped => MediaPlaybackStatus::Stopped,
            };
            c.SetPlaybackStatus(status).map_err(err)?;
        }
        if what.abilities {
            c.SetIsNextEnabled(now.can_next).map_err(err)?;
            c.SetIsPreviousEnabled(now.can_previous).map_err(err)?;
            c.SetIsFastForwardEnabled(now.can_seek).map_err(err)?;
            c.SetIsRewindEnabled(now.can_seek).map_err(err)?;
        }
        if what.track || what.position || what.abilities {
            let length = now.length.unwrap_or_default();
            let t = SystemMediaTransportControlsTimelineProperties::new().map_err(err)?;
            t.SetStartTime(span(Duration::ZERO)).map_err(err)?;
            t.SetEndTime(span(length)).map_err(err)?;
            t.SetMinSeekTime(span(Duration::ZERO)).map_err(err)?;
            let seekable = if now.can_seek { length } else { Duration::ZERO };
            t.SetMaxSeekTime(span(seekable)).map_err(err)?;
            let at = match now.length {
                Some(l) => now.position.min(l),
                None => now.position,
            };
            t.SetPosition(span(at)).map_err(err)?;
            c.UpdateTimelineProperties(&t).map_err(err)?;
        }
        Ok(())
    }

    /// The timeline was sent by `send` already; the flyout has no separate seek signal.
    pub(super) fn seeked(&mut self, position: Duration) -> io::Result<()> {
        _ = position;
        Ok(())
    }
}

/// The cover as a stream the flyout reads: the picture written as PNG into memory.
fn picture(
    image: &mdshared_text::image::Image,
) -> windows::core::Result<RandomAccessStreamReference> {
    let bytes = mdshared_text::image::encode_png(image);
    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(&bytes)?;
    // A stream in memory: these finish at once, and waiting for them blocks for no time.
    writer.StoreAsync()?.join()?;
    writer.FlushAsync()?.join()?;
    writer.DetachStream()?;
    stream.Seek(0)?;
    RandomAccessStreamReference::CreateFromStream(&stream)
}

impl Drop for Smtc {
    fn drop(&mut self) {
        let c = &self.controls;
        _ = c.RemoveButtonPressed(self.pressed);
        _ = c.RemovePlaybackPositionChangeRequested(self.moved);
        if let Ok(u) = c.DisplayUpdater() {
            _ = u.ClearAll();
            _ = u.Update();
        }
        _ = c.SetIsEnabled(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flyouts_buttons_become_commands() {
        use SystemMediaTransportControlsButton as B;
        assert_eq!(command(B::Play), Some(Command::Play));
        assert_eq!(command(B::Pause), Some(Command::Pause));
        assert_eq!(command(B::Next), Some(Command::Next));
        assert_eq!(command(B::Rewind), Some(Command::SeekBy(-10.0)));
        assert_eq!(command(B::Record), None);
    }

    #[test]
    fn times_go_to_hundreds_of_nanoseconds_and_back() {
        let d = Duration::from_millis(123_456);
        assert_eq!(span(d).Duration, 1_234_560_000);
        assert_eq!(duration(span(d)), d);
        assert_eq!(duration(TimeSpan { Duration: -5 }), Duration::ZERO);
    }

    #[test]
    fn a_cover_becomes_a_stream_the_flyout_can_read() {
        let image = mdshared_text::image::Image::new(2, 2, vec![200; 16]).unwrap();
        assert!(picture(&image).is_ok());
    }
}
