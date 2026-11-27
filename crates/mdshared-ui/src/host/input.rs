//! winit's input reduced to the toolkit's: device positions to logical px, buttons, the wheel,
//! and the click count. Pure, so `host_tests.rs` tests it without a window.

use winit::event::{MouseButton, MouseScrollDelta};

use super::{Button, Event, WHEEL_LINE};
use crate::clicks::ClickCounter;

#[cfg(test)]
pub(super) use crate::clicks::MAX_CLICKS;
pub(super) use crate::clicks::click_slop;
pub(super) type Clicks = ClickCounter<Button>;

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

/// The buttons the application was told went down ([`Event::Press`]) and not yet up: what the
/// host tells it was lost ([`Event::PressLost`]) when the window loses the keyboard with a
/// button still held. winit 0.30 on Windows sends no release then (`WM_CAPTURECHANGED` only
/// resets a count), so without it a drag would stay held until the next release, wherever and
/// whenever that comes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Held(u8);

/// The order the buttons are told in, and their bits.
const BUTTONS: [Button; 5] = [
    Button::Left,
    Button::Right,
    Button::Middle,
    Button::Back,
    Button::Forward,
];

fn bit(b: Button) -> u8 {
    1 << BUTTONS.iter().position(|&k| k == b).unwrap_or(0)
}

impl Held {
    /// `b` went down (and the application was told).
    pub(super) fn press(&mut self, b: Button) {
        self.0 |= bit(b);
    }

    /// `b` went up. A release of a button not held (one that came after it was lost, or a
    /// press the application was not told of) changes nothing.
    pub(super) fn release(&mut self, b: Button) {
        self.0 &= !bit(b);
    }

    /// Takes the buttons held, left, right, middle, back, forward in that order, and holds
    /// none.
    pub(super) fn take(&mut self) -> Vec<Button> {
        let held = std::mem::take(&mut self.0);
        BUTTONS
            .into_iter()
            .filter(|&b| held & bit(b) != 0)
            .collect()
    }
}

/// A wheel's movement as [`Event::Wheel`]: notches times [`WHEEL_LINE`], or a touchpad's device
/// px as logical px; Shift turns a vertical wheel sideways. winit's sign is kept: positive moves
/// the content right or down (a wheel turned away from the user).
pub(super) fn wheel(delta: MouseScrollDelta, scale: f32, shift: bool) -> Event {
    let (x, y, precise) = match delta {
        // A precision touchpad on Windows reports fractions of a line; a wheel, whole notches.
        MouseScrollDelta::LineDelta(x, y) => (
            f64::from(x) * f64::from(WHEEL_LINE),
            f64::from(y) * f64::from(WHEEL_LINE),
            [x, y].iter().any(|v| v.is_finite() && v.fract() != 0.0),
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
