//! winit's input reduced to the toolkit's: device positions to logical px, buttons, the wheel,
//! and the click count. Pure, so `host_tests.rs` tests it without a window.

use std::time::{Duration, Instant};

use winit::event::{MouseButton, MouseScrollDelta};

use super::{Button, Event, WHEEL_LINE};

/// Where the click count stops: a triple click is the most anything reads.
pub(super) const MAX_CLICKS: u32 = 3;

/// A scale that is a positive number, else 1.
pub(super) fn clean_scale(s: f64) -> f32 {
    if s.is_finite() && s > 0.0 {
        s as f32
    } else {
        1.0
    }
}

/// A device position as logical px. A position that is not a number is 0.
pub(super) fn logical(x: f64, y: f64, scale: f32) -> (f32, f32) {
    let s = f64::from(clean_scale(f64::from(scale)));
    let c = |v: f64| if v.is_finite() { (v / s) as f32 } else { 0.0 };
    (c(x), c(y))
}

/// winit's button as the toolkit's, `None` for the others.
pub(super) fn button(b: MouseButton) -> Option<Button> {
    Some(match b {
        MouseButton::Left => Button::Left,
        MouseButton::Right => Button::Right,
        MouseButton::Middle => Button::Middle,
        MouseButton::Back => Button::Back,
        MouseButton::Forward => Button::Forward,
        MouseButton::Other(_) => return None,
    })
}

/// A wheel's movement as [`Event::Wheel`]: notches times [`WHEEL_LINE`], or a touchpad's device
/// px as logical px; Shift turns a vertical wheel sideways. winit's sign is kept: positive moves
/// the content right or down (a wheel turned away from the user).
pub(super) fn wheel(delta: MouseScrollDelta, scale: f32, shift: bool) -> Event {
    let (x, y, precise) = match delta {
        MouseScrollDelta::LineDelta(x, y) => (
            f64::from(x) * f64::from(WHEEL_LINE),
            f64::from(y) * f64::from(WHEEL_LINE),
            false,
        ),
        MouseScrollDelta::PixelDelta(p) => {
            let (x, y) = logical(p.x, p.y, scale);
            (f64::from(x), f64::from(y), true)
        }
    };
    let (dx, dy) = crate::viewport::wheel_delta(x, y, shift);
    Event::Wheel {
        dx: dx as f32,
        dy: dy as f32,
        precise,
    }
}

/// How far, in logical px each way, a press may be from the one before and still count towards
/// a double click: half the system's double-click rectangle (`size`, device px) at `scale`.
pub(super) fn click_slop(size: (u32, u32), scale: f32) -> (f32, f32) {
    let s = clean_scale(f64::from(scale));
    (size.0 as f32 / 2.0 / s, size.1 as f32 / 2.0 / s)
}

/// Counts the presses of one button that make a double or triple click.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Clicks {
    last: Option<(Instant, (f32, f32), Button)>,
    count: u32,
}

impl Clicks {
    /// A press of `button` at `at` (logical px) at `now`: 1 for a first press, one more for each
    /// that follows within `interval` and `slop` px each way of the one before, up to
    /// [`MAX_CLICKS`], where it stays while the run goes on.
    pub(super) fn press(
        &mut self,
        now: Instant,
        at: (f32, f32),
        button: Button,
        interval: Duration,
        slop: (f32, f32),
    ) -> u32 {
        let again = self.last.is_some_and(|(t, p, b)| {
            b == button
                && now.saturating_duration_since(t) <= interval
                && (at.0 - p.0).abs() <= slop.0
                && (at.1 - p.1).abs() <= slop.1
        });
        self.count = if again {
            (self.count + 1).min(MAX_CLICKS)
        } else {
            1
        };
        self.last = Some((now, at, button));
        self.count
    }
}
