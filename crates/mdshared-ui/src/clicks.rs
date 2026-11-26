//! Count nearby presses for text fields, editors and other pointer targets. Callers supply the
//! clock, button and coordinates and decide when a drag or change of target breaks the sequence.
//! Deliberately not here: event delivery or selection.

use std::time::{Duration, Instant};

/// The largest count returned by [`ClickCounter::press`].
pub const MAX_CLICKS: u32 = 3;

/// Half the system's double-click rectangle on each axis, in logical pixels. `size` is in device
/// pixels; an invalid scale is treated as 1.
pub fn click_slop(size: (u32, u32), scale: f32) -> (f32, f32) {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    (size.0 as f32 / 2.0 / scale, size.1 as f32 / 2.0 / scale)
}

pub(crate) fn within(at: (f32, f32), before: (f32, f32), slop: (f32, f32)) -> bool {
    (at.0 - before.0).abs() <= slop.0 && (at.1 - before.1).abs() <= slop.1
}

/// Counts presses of one button, starting over when the button, time or position changes too much.
/// Use `()` when the caller routes just one button to the counter.
#[derive(Clone, Copy, Debug)]
pub struct ClickCounter<B = ()> {
    last: Option<(Instant, (f32, f32), B)>,
    count: u32,
}

impl<B> Default for ClickCounter<B> {
    fn default() -> Self {
        Self {
            last: None,
            count: 0,
        }
    }
}

impl<B: Eq> ClickCounter<B> {
    /// Counts a press at `at` (logical pixels). A press of the same button within `interval` and
    /// `slop` pixels each way of the preceding one increments the count, up to 3. The limits are
    /// inclusive. A non-finite position starts a new sequence; a clock that went back counts as
    /// no elapsed time. Use [`crate::platform::double_click_time`] and [`click_slop`] for the system
    /// settings.
    pub fn press(
        &mut self,
        now: Instant,
        at: (f32, f32),
        button: B,
        interval: Duration,
        slop: (f32, f32),
    ) -> u32 {
        let again = self.last.as_ref().is_some_and(|(t, p, b)| {
            *b == button && now.saturating_duration_since(*t) <= interval && within(at, *p, slop)
        });
        self.count = if again {
            (self.count + 1).min(MAX_CLICKS)
        } else {
            1
        };
        self.last = Some((now, at, button));
        self.count
    }

    /// Forgets the previous press, so the next press starts at 1.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
