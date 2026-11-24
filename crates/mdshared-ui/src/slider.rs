//! A slider: a value between two ends, shown as a thumb on a track and moved by the pointer,
//! the keys and the wheel. Also what shares its look: a seek bar (a slider with the part
//! already loaded marked on its track), a progress bar, and the two marks for work whose end
//! is not known, a sliding bar and a spinner.
//!
//! The model ([`Slider`]) holds the value and answers questions of geometry; the caller owns
//! the events and passes them in ([`Slider::press`], [`Slider::drag`], [`Slider::release`],
//! [`Slider::step_by`], [`Slider::wheel`]), and reads [`Slider::value`] afterwards. Drawing is
//! apart ([`draw`], [`draw_animated`]) and changes nothing. Nothing remembers the rectangle:
//! each call is given it.
//!
//! Deliberately not here: a slider with two thumbs (a range), tick marks and labels (the
//! caller draws them from [`Slider::thumb_center`] and [`Slider::fraction_of`]), and a value
//! shown as text.

use crate::anim::{Animator, AsId, Easing, Id, Motion, fluent};
use crate::gfx::{Painter, Rect, mix};
pub use crate::splitter::Axis;
use crate::theme::Palette;
use std::time::Duration;

/// The thumb's diameter, and so the least height (or width, upright) a slider wants.
pub const THUMB: f32 = 18.0;
/// The thickness of the track.
pub const TRACK: f32 = 4.0;
/// Where the decibel scale puts the low end of the track: 60 dB under the high end.
pub const DB_FLOOR: f32 = -60.0;

/// How the track's length maps to the value.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Scale {
    /// Equal distances are equal differences.
    #[default]
    Linear,
    /// Equal distances are equal ratios: for a frequency, a zoom factor, a speed. Both ends
    /// must be above zero; if they are not, the scale is linear.
    Log,
    /// For a volume kept as a gain (0 silent, 1 full): equal distances are equal steps in
    /// decibels, from [`DB_FLOOR`] under the high end at the low end of the track, where the
    /// value is the slider's minimum itself (so the bottom is silence, not a thousandth). The
    /// high end must be above zero; if it is not, the scale is linear.
    Db,
}

/// A gain as decibels: 1 is 0 dB, a half about -6, nothing and less is negative infinity.
pub fn gain_to_db(gain: f32) -> f32 {
    if gain > 0.0 {
        20.0 * gain.log10()
    } else {
        f32::NEG_INFINITY
    }
}

/// Decibels as a gain: 0 dB is 1, -6 about a half.
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// A step of the keyboard. The caller maps its keys: Left and Down are `Less`, Right and Up
/// `More` (also on an upright slider, where up is more), Page Down and Page Up the page steps,
/// Home and End the ends.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Step {
    Less,
    More,
    PageLess,
    PageMore,
    Min,
    Max,
}

/// A value between two ends and the thumb that shows it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Slider {
    min: f32,
    max: f32,
    value: f32,
    /// What the value snaps to, counted from `min`; 0 for none. Linear scale only.
    step: f32,
    axis: Axis,
    scale: Scale,
    /// With the thumb in hand: how far the pointer is from the thumb's middle, along the axis.
    grab: Option<f32>,
}

impl Slider {
    /// A slider lying flat from `min` on the left to `max` on the right, linear, not snapping,
    /// at `value` (kept between the ends). Ends given the wrong way round are swapped; ends
    /// that are not numbers make a slider from 0 to 1.
    pub fn new(min: f32, max: f32, value: f32) -> Slider {
        let (min, max) = if min.is_finite() && max.is_finite() {
            (min.min(max), min.max(max))
        } else {
            (0.0, 1.0)
        };
        let mut s = Slider {
            min,
            max,
            value: min,
            step: 0.0,
            axis: Axis::Horizontal,
            scale: Scale::Linear,
            grab: None,
        };
        s.set(value);
        s
    }

    /// The same, with the value snapping to multiples of `step` from the low end (on the
    /// linear scale; the others do not snap). A key then moves by one step.
    pub fn step(mut self, step: f32) -> Slider {
        self.step = if step.is_finite() { step.max(0.0) } else { 0.0 };
        self.set(self.value);
        self
    }

    /// The same, standing upright: the low end at the bottom.
    pub fn vertical(mut self) -> Slider {
        self.axis = Axis::Vertical;
        self
    }

    /// The same on another [`Scale`].
    pub fn scale(mut self, scale: Scale) -> Slider {
        self.scale = scale;
        self
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn min(&self) -> f32 {
        self.min
    }

    pub fn max(&self) -> f32 {
        self.max
    }

    pub fn axis(&self) -> Axis {
        self.axis
    }

    /// Whether the thumb is in hand.
    pub fn dragging(&self) -> bool {
        self.grab.is_some()
    }

    /// The scale in force: the one asked for, or linear where its ends do not allow it.
    fn scale_in_force(&self) -> Scale {
        match self.scale {
            Scale::Log if self.min > 0.0 => Scale::Log,
            Scale::Db if self.max > 0.0 => Scale::Db,
            _ => Scale::Linear,
        }
    }

    /// Sets the value: kept between the ends and snapped to the step. A value that is not a
    /// number changes nothing. Returns whether the value changed.
    pub fn set(&mut self, value: f32) -> bool {
        if value.is_nan() {
            return false;
        }
        let mut v = value.clamp(self.min, self.max);
        if self.step > 0.0 && self.scale_in_force() == Scale::Linear {
            v = (self.min + ((v - self.min) / self.step).round() * self.step)
                .clamp(self.min, self.max);
        }
        let changed = v != self.value;
        self.value = v;
        changed
    }

    /// Where along the track a value lies: 0 at the low end, 1 at the high end, by the scale.
    /// A value outside the ends is at the nearer end.
    pub fn fraction_of(&self, value: f32) -> f32 {
        let v = value.clamp(self.min, self.max);
        if self.max <= self.min || v.is_nan() {
            return 0.0;
        }
        let t = match self.scale_in_force() {
            Scale::Linear => (v - self.min) / (self.max - self.min),
            Scale::Log => (v / self.min).ln() / (self.max / self.min).ln(),
            Scale::Db => {
                if v <= self.min || v <= 0.0 {
                    0.0
                } else {
                    1.0 - gain_to_db(v / self.max) / DB_FLOOR
                }
            }
        };
        if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) }
    }

    /// Where along the track the slider's own value lies.
    pub fn fraction(&self) -> f32 {
        self.fraction_of(self.value)
    }

    /// The value at a place along the track, 0 to 1, by the scale; not snapped.
    pub fn value_of(&self, fraction: f32) -> f32 {
        let t = if fraction.is_nan() {
            0.0
        } else {
            fraction.clamp(0.0, 1.0)
        };
        let v = match self.scale_in_force() {
            Scale::Linear => self.min + (self.max - self.min) * t,
            Scale::Log => self.min * (self.max / self.min).powf(t),
            Scale::Db => {
                if t <= 0.0 {
                    self.min
                } else {
                    self.max * db_to_gain((1.0 - t) * DB_FLOOR)
                }
            }
        };
        v.clamp(self.min, self.max)
    }

    /// The two ends of the thumb's travel in `r`, along the axis: where its middle is at the
    /// low end and at the high end. The thumb stays whole inside `r`.
    fn travel(&self, r: Rect) -> (f32, f32) {
        match self.axis {
            Axis::Horizontal => (r.x + THUMB / 2.0, r.right() - THUMB / 2.0),
            Axis::Vertical => (r.bottom() - THUMB / 2.0, r.y + THUMB / 2.0),
        }
    }

    /// The point along the axis for a place on the track.
    fn along(&self, r: Rect, fraction: f32) -> f32 {
        let (low, high) = self.travel(r);
        low + (high - low) * fraction
    }

    /// The middle of the thumb in `r`.
    pub fn thumb_center(&self, r: Rect) -> (f32, f32) {
        let at = self.along(r, self.fraction());
        match self.axis {
            Axis::Horizontal => (at, r.cy()),
            Axis::Vertical => (r.cx(), at),
        }
    }

    /// The thumb's square in `r`, [`THUMB`] a side.
    pub fn thumb_rect(&self, r: Rect) -> Rect {
        let (cx, cy) = self.thumb_center(r);
        Rect::new(cx - THUMB / 2.0, cy - THUMB / 2.0, THUMB, THUMB)
    }

    /// The place on the track, 0 to 1, nearest to a point: its position along the axis is all
    /// that counts, so a pointer that strays off the track during a drag still moves the thumb.
    pub fn fraction_at(&self, r: Rect, x: f32, y: f32) -> f32 {
        let (low, high) = self.travel(r);
        let at = if self.axis == Axis::Horizontal { x } else { y };
        if high == low || at.is_nan() {
            return 0.0;
        }
        ((at - low) / (high - low)).clamp(0.0, 1.0)
    }

    /// The value a press at this point would set, snapped as [`set`](Self::set) snaps.
    pub fn value_at(&self, r: Rect, x: f32, y: f32) -> f32 {
        let mut probe = *self;
        probe.set(self.value_of(self.fraction_at(r, x, y)));
        probe.value
    }

    /// A press of the primary button. Inside `r` it takes hold and returns true: on the thumb
    /// the value stays (the thumb is held where it was taken), anywhere else on the slider the
    /// value jumps to the point and the thumb is held by its middle. Outside `r` it returns
    /// false and nothing changes.
    pub fn press(&mut self, r: Rect, x: f32, y: f32) -> bool {
        if !r.contains(x, y) {
            return false;
        }
        let at = if self.axis == Axis::Horizontal { x } else { y };
        if self.thumb_rect(r).contains(x, y) {
            let (cx, cy) = self.thumb_center(r);
            let middle = if self.axis == Axis::Horizontal {
                cx
            } else {
                cy
            };
            self.grab = Some(at - middle);
        } else {
            self.set(self.value_of(self.fraction_at(r, x, y)));
            self.grab = Some(0.0);
        }
        true
    }

    /// The pointer moved with the thumb in hand: the value follows it. Returns whether the
    /// value changed; false with no thumb in hand.
    pub fn drag(&mut self, r: Rect, x: f32, y: f32) -> bool {
        let Some(grab) = self.grab else {
            return false;
        };
        let (x, y) = match self.axis {
            Axis::Horizontal => (x - grab, y),
            Axis::Vertical => (x, y - grab),
        };
        self.set(self.value_of(self.fraction_at(r, x, y)))
    }

    /// Lets go of the thumb. Returns whether it was in hand: for a seek bar, the moment to
    /// seek.
    pub fn release(&mut self) -> bool {
        self.grab.take().is_some()
    }

    /// How far one key press moves along the track: one step of a snapping slider, else a
    /// fiftieth of the track.
    fn key_fraction(&self) -> f32 {
        let span = self.max - self.min;
        if self.step > 0.0 && span > 0.0 && self.scale_in_force() == Scale::Linear {
            (self.step / span).min(1.0)
        } else {
            0.02
        }
    }

    /// A key: one step less or more, a page (five steps, at least a tenth of the track), or an
    /// end. Returns whether the value changed.
    pub fn step_by(&mut self, step: Step) -> bool {
        let small = self.key_fraction();
        let page = (small * 5.0).max(0.1);
        let to = match step {
            Step::Less => self.fraction() - small,
            Step::More => self.fraction() + small,
            Step::PageLess => self.fraction() - page,
            Step::PageMore => self.fraction() + page,
            Step::Min => 0.0,
            Step::Max => 1.0,
        };
        self.set(self.value_of(to))
    }

    /// The wheel turned over the slider by `notches` (positive away from the user, which is
    /// more; a fraction for a touchpad): a key's step a notch. Returns whether the value
    /// changed.
    pub fn wheel(&mut self, notches: f32) -> bool {
        if !notches.is_finite() || notches == 0.0 {
            return false;
        }
        // A snapping slider moves by whole steps, at least one in the direction turned.
        let steps = if self.step > 0.0 {
            notches.abs().ceil().copysign(notches)
        } else {
            notches
        };
        self.set(self.value_of(self.fraction() + steps * self.key_fraction()))
    }
}

/// How a slider is drawn just now, from the caller's own state.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Look {
    /// The pointer is over it.
    pub hot: bool,
    /// The thumb is in hand.
    pub pressed: bool,
    /// The keyboard is on it.
    pub focused: bool,
    /// It cannot be changed: drawn quiet, with no hover or press look.
    pub disabled: bool,
    /// A part of the track to mark, as two values of the slider: for a seek bar, what is
    /// loaded (from the start of the buffer to its end).
    pub buffered: Option<(f32, f32)>,
}

/// The radius of the thumb's inner dot at rest, under the pointer, and in hand.
const DOT: f32 = 5.0;
const DOT_HOT: f32 = 6.5;
const DOT_PRESSED: f32 = 4.0;

fn dot_for(look: &Look) -> f32 {
    if look.disabled {
        DOT
    } else if look.pressed {
        DOT_PRESSED
    } else if look.hot {
        DOT_HOT
    } else {
        DOT
    }
}

/// A piece of the track from one place to another (0 to 1), in the track's own thickness.
fn track_piece(s: &Slider, r: Rect, from: f32, to: f32) -> Rect {
    let (a, b) = (s.along(r, from), s.along(r, to));
    let (lo, hi) = (a.min(b), a.max(b));
    match s.axis {
        Axis::Horizontal => Rect::new(
            lo - TRACK / 2.0,
            r.cy() - TRACK / 2.0,
            hi - lo + TRACK,
            TRACK,
        ),
        Axis::Vertical => Rect::new(
            r.cx() - TRACK / 2.0,
            lo - TRACK / 2.0,
            TRACK,
            hi - lo + TRACK,
        ),
    }
}

/// The drawing both [`draw`] and [`draw_animated`] share: the thumb at `at` (0 to 1 along the
/// track) with an inner dot of radius `dot`.
fn paint(p: &mut Painter, pal: &Palette, r: Rect, s: &Slider, look: &Look, at: f32, dot: f32) {
    let quiet = mix(pal.line, pal.text, 0.18);
    let ink = if look.disabled { pal.faint } else { pal.accent };
    p.rbox(track_piece(s, r, 0.0, 1.0), TRACK / 2.0, quiet);
    if let Some((from, to)) = look.buffered {
        let (from, to) = (s.fraction_of(from), s.fraction_of(to));
        if to > from {
            let loaded = mix(quiet, pal.text, 0.35);
            p.rbox(track_piece(s, r, from, to), TRACK / 2.0, loaded);
        }
    }
    if at > 0.0 {
        p.rbox(track_piece(s, r, 0.0, at), TRACK / 2.0, ink);
    }
    let along = s.along(r, at);
    let (cx, cy) = match s.axis {
        Axis::Horizontal => (along, r.cy()),
        Axis::Vertical => (r.cx(), along),
    };
    if look.focused && !look.disabled {
        p.ring(cx, cy, THUMB / 2.0 + 2.5, 1.5, pal.accent, 1.0);
    }
    let thumb = Rect::new(cx - THUMB / 2.0, cy - THUMB / 2.0, THUMB, THUMB);
    p.frame(thumb, THUMB / 2.0, pal.popup, mix(pal.line, pal.text, 0.25));
    p.ring(cx, cy, dot / 2.0, dot, ink, 1.0);
}

/// Draws a slider in `r`: the track along the middle of the rectangle, the part up to the
/// value in the accent, the part [`Look::buffered`] marks a little stronger than the track,
/// and the thumb, a disc with a dot in the accent that grows under the pointer and shrinks in
/// hand; with the keyboard on it a ring goes round the thumb. A disabled slider is drawn in a
/// faint colour. `r` should be at least [`THUMB`] across the axis; the focus ring reaches
/// 4 px outside the thumb.
pub fn draw(p: &mut Painter, pal: &Palette, r: Rect, slider: &Slider, look: Look) {
    paint(p, pal, r, slider, &look, slider.fraction(), dot_for(&look));
}

/// [`draw`] with motion: the thumb glides to a value set by a key, the wheel, a click on the
/// track or the program, and its dot grows and shrinks over a few frames. While the thumb is
/// in hand ([`Look::pressed`]) it is under the pointer at once: a drag is never behind. `id`
/// names this slider in `anim`. At rest, and on every frame when the animator does not
/// animate, it draws exactly what [`draw`] draws.
pub fn draw_animated(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    r: Rect,
    slider: &Slider,
    look: Look,
) {
    let id = Id::new(id);
    let place = id.with("at");
    if look.pressed {
        anim.set(place, slider.fraction());
    }
    let at = anim.value(place, slider.fraction(), fluent::MOVE);
    let dot = anim.value(id.with("dot"), dot_for(&look), fluent::FADE);
    paint(p, pal, r, slider, &look, at, dot);
}

/// A progress bar for work whose end is known: `r` filled from the left to `fraction` (from
/// 0 to 1) in the accent, over a quiet track, both with round ends. It is as thick as `r` is
/// high (four to six px is usual). A fraction that is not a number draws the empty track.
pub fn draw_progress(p: &mut Painter, pal: &Palette, r: Rect, fraction: f32) {
    let radius = r.h / 2.0;
    p.rbox(r, radius, mix(pal.line, pal.text, 0.18));
    let t = if fraction.is_nan() {
        0.0
    } else {
        fraction.clamp(0.0, 1.0)
    };
    if t > 0.0 {
        // Never shorter than it is high, so the first sliver is a round dot, not a splinter.
        let w = (r.w * t).max(r.h.min(r.w));
        p.rbox(Rect::new(r.x, r.y, w, r.h), radius, pal.accent);
    }
}

/// Seconds since `id` was first drawn, for a mark that moves for as long as it is shown; `None`
/// when the animator does not animate. Asking keeps the animator asking for frames, and a
/// frame that does not ask lets it sleep again: the value is on its way to a target it never
/// reaches.
fn running(anim: &mut Animator, id: Id) -> Option<f32> {
    if !anim.animates() {
        return None;
    }
    const FOREVER: u64 = 1_000_000;
    let forever = Motion::tween(Duration::from_secs(FOREVER), Easing::Linear);
    Some(anim.value_from(id, 0.0, FOREVER as f32, forever))
}

/// A progress bar for work whose end is not known: a piece of the accent that runs along the
/// track again and again, growing as it leaves and shrinking as it arrives. It moves for as
/// long as it is drawn, so the window keeps drawing frames while it shows and sleeps once it
/// is gone. `id` names it in `anim`. With reduced motion it is a still piece in the middle of
/// the track, which still says "busy".
pub fn draw_progress_indeterminate(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    r: Rect,
) {
    let radius = r.h / 2.0;
    p.rbox(r, radius, mix(pal.line, pal.text, 0.18));
    let (from, to) = match running(anim, Id::new(id)) {
        // One run in 1.6 s: the head sets off first and eases in, the tail follows.
        Some(secs) => {
            let t = (secs / 1.6).fract();
            let head = Easing::OutCubic.at((t / 0.75).min(1.0));
            let tail = Easing::InCubic.at(((t - 0.25) / 0.75).max(0.0));
            (tail, head)
        }
        None => (0.3, 0.7),
    };
    let piece = Rect::new(r.x + r.w * from, r.y, r.w * (to - from), r.h);
    if piece.w >= 1.0 {
        let old = p.clip(r);
        p.rbox(piece, radius.min(piece.w / 2.0), pal.accent);
        p.restore(old);
    }
}

/// A spinner: an arc of the accent that turns round `cx, cy` at radius `r`, for a wait with
/// no room for a bar. It turns for as long as it is drawn, as
/// [`draw_progress_indeterminate`] moves. `id` names it in `anim`. With reduced motion it is
/// a still three-quarter ring.
pub fn draw_spinner(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    cx: f32,
    cy: f32,
    r: f32,
) {
    let width = (r / 4.0).clamp(1.5, 4.0);
    let (from, span) = match running(anim, Id::new(id)) {
        // A turn in 1.1 s, the arc breathing between a sixth and two thirds of the ring.
        Some(secs) => {
            let breath = (1.0 - (secs / 1.6 * std::f32::consts::TAU).cos()) / 2.0;
            (secs / 1.1, 0.16 + 0.5 * breath)
        }
        None => (0.0, 0.75),
    };
    let from = from.fract();
    p.arc(cx, cy, r, width, from, from + span, pal.accent, 1.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{Clock, ManualClock, Policy};
    use crate::gfx::Gfx;
    use crate::theme::{DARK, LIGHT};
    use mdshared_text::Bitmap;

    /// A slider's rectangle: the thumb's middle travels from x 29 to 211.
    const R: Rect = Rect::new(20.0, 40.0, 200.0, 20.0);

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-3 * b.abs().max(1.0)
    }

    #[test]
    fn a_new_slider_keeps_its_value_between_its_ends_and_snaps_it() {
        let s = Slider::new(0.0, 100.0, 250.0);
        assert_eq!((s.min(), s.max(), s.value()), (0.0, 100.0, 100.0));
        assert_eq!(
            Slider::new(10.0, -10.0, 0.0).min(),
            -10.0,
            "ends the wrong way round"
        );
        let odd = Slider::new(f32::NAN, 5.0, 3.0);
        assert_eq!((odd.min(), odd.max(), odd.value()), (0.0, 1.0, 1.0));
        let mut s = Slider::new(0.0, 10.0, 3.3).step(0.5);
        assert_eq!(s.value(), 3.5);
        assert!(s.set(7.2) && s.value() == 7.0);
        assert!(!s.set(7.1), "the same step, no change");
        assert!(!s.set(f32::NAN) && s.value() == 7.0);
        assert!(s.set(f32::INFINITY) && s.value() == 10.0);
        // Steps count from the low end, and the high end is reachable though it is no step.
        let mut odd = Slider::new(1.0, 10.0, 1.0).step(4.0);
        odd.set(6.0);
        assert_eq!(odd.value(), 5.0);
        odd.set(9.9);
        assert_eq!(odd.value(), 9.0);
        assert_eq!(Slider::new(0.0, 1.0, 0.4).step(-3.0).value(), 0.4);
        assert_eq!(
            (s.axis(), s.vertical().axis()),
            (Axis::Horizontal, Axis::Vertical)
        );
    }

    #[test]
    fn the_scales_map_the_track_to_the_value_and_back() {
        let lin = Slider::new(-50.0, 150.0, 0.0);
        assert!(close(lin.fraction(), 0.25));
        assert!(close(lin.value_of(0.75), 100.0));
        // Log: the middle of 20 to 20,000 is their geometric mean.
        let log = Slider::new(20.0, 20_000.0, 20.0).scale(Scale::Log);
        assert!(close(log.value_of(0.5), 632.4555));
        assert!(close(log.fraction_of(200.0), 1.0 / 3.0));
        assert!(close(log.fraction_of(2000.0), 2.0 / 3.0));
        // Decibels: half way down the track is 30 dB down, the bottom is silence itself.
        let vol = Slider::new(0.0, 1.0, 1.0).scale(Scale::Db);
        assert!(close(gain_to_db(vol.value_of(0.5)), -30.0));
        assert!(close(vol.fraction_of(db_to_gain(-6.0)), 0.9));
        assert_eq!((vol.value_of(0.0), vol.value_of(1.0)), (0.0, 1.0));
        assert_eq!((vol.fraction_of(0.0), vol.fraction_of(1.0)), (0.0, 1.0));
        // Above full: +6 dB at the top, and the floor is 60 dB under that.
        let boost = Slider::new(0.0, 2.0, 1.0).scale(Scale::Db);
        assert!(close(boost.fraction(), 1.0 - gain_to_db(0.5) / DB_FLOOR));
        // There and back, all along each track, and never falling as the track rises.
        for s in [lin, log, vol, boost] {
            let mut last = s.min();
            for i in 0..=100 {
                let t = i as f32 / 100.0;
                let v = s.value_of(t);
                assert!(v >= last && v >= s.min() && v <= s.max(), "{s:?} at {t}");
                assert!((s.fraction_of(v) - t).abs() < 1e-3, "{s:?} at {t}");
                last = v;
            }
            assert_eq!(s.value_of(f32::NAN), s.min());
            assert_eq!((s.value_of(-1.0), s.value_of(9.0)), (s.min(), s.max()));
            assert_eq!(s.fraction_of(f32::NAN), 0.0);
        }
        // Ends a scale cannot use: it is linear there.
        let no_log = Slider::new(0.0, 10.0, 5.0).scale(Scale::Log);
        assert!(close(no_log.fraction(), 0.5) && close(no_log.value_of(0.2), 2.0));
        let no_db = Slider::new(-5.0, 0.0, -2.5).scale(Scale::Db);
        assert!(close(no_db.fraction(), 0.5));
        // A slider with one value has one place.
        let point = Slider::new(3.0, 3.0, 3.0);
        assert_eq!((point.fraction(), point.value_of(0.7)), (0.0, 3.0));
        assert_eq!((gain_to_db(1.0), gain_to_db(0.0)), (0.0, f32::NEG_INFINITY));
        assert!(close(db_to_gain(-20.0), 0.1));
    }

    #[test]
    fn the_thumb_stays_inside_the_rectangle_and_a_point_maps_to_a_value() {
        let mut s = Slider::new(0.0, 100.0, 0.0);
        assert_eq!(s.thumb_center(R), (29.0, 50.0));
        assert_eq!(s.thumb_rect(R), Rect::new(20.0, 41.0, 18.0, 18.0));
        s.set(100.0);
        assert_eq!(s.thumb_rect(R).right(), R.right());
        s.set(50.0);
        assert_eq!(s.thumb_center(R), (120.0, 50.0));
        assert_eq!(s.value_at(R, 120.0, 50.0), 50.0);
        assert!(close(s.value_at(R, 74.5, 50.0), 25.0));
        // Past the ends of the travel, and far off the track: the nearer end, by x alone.
        assert_eq!(
            (s.value_at(R, 21.0, 50.0), s.value_at(R, 219.0, 50.0)),
            (0.0, 100.0)
        );
        assert_eq!(
            (s.value_at(R, -900.0, 900.0), s.value_at(R, 900.0, -900.0)),
            (0.0, 100.0)
        );
        assert_eq!(s.fraction_at(R, f32::NAN, 50.0), 0.0);
        // Upright: the low end is at the bottom.
        let tall = Rect::new(40.0, 20.0, 20.0, 200.0);
        let mut v = Slider::new(0.0, 100.0, 0.0).vertical();
        assert_eq!(v.thumb_center(tall), (50.0, 211.0));
        v.set(100.0);
        assert_eq!(v.thumb_center(tall), (50.0, 29.0));
        assert!(close(v.value_at(tall, 0.0, 74.5), 75.0));
        // A rectangle with no travel has one place.
        let dot = Rect::new(0.0, 0.0, 18.0, 18.0);
        assert_eq!(s.fraction_at(dot, 9.0, 9.0), 0.0);
        assert_eq!(s.step(10.0).value_at(R, 78.0, 50.0), 30.0, "snapped");
    }

    #[test]
    fn a_press_on_the_track_jumps_and_one_on_the_thumb_holds_it_where_it_was_taken() {
        let mut s = Slider::new(0.0, 100.0, 50.0);
        assert!(!s.press(R, 100.0, 39.0), "outside the rectangle");
        assert!(!s.drag(R, 60.0, 50.0) && !s.release());
        // On the track: the value jumps there and the thumb is held by its middle.
        assert!(s.press(R, 74.5, 50.0));
        assert!(close(s.value(), 25.0) && s.dragging());
        assert!(s.drag(R, 165.5, 50.0));
        assert!(close(s.value(), 75.0));
        assert!(!s.drag(R, 165.5, 90.0), "across the track nothing changes");
        // Off the end of the slider it stops at the end, and still follows back.
        assert!(s.drag(R, 5000.0, 50.0));
        assert_eq!(s.value(), 100.0);
        assert!(s.drag(R, 120.0, -400.0));
        assert_eq!(s.value(), 50.0);
        assert!(s.release());
        assert!(!s.dragging() && !s.release());
        // On the thumb, 5 px right of its middle: no jump, and the 5 px are kept.
        assert!(s.press(R, 125.0, 50.0));
        assert_eq!(s.value(), 50.0);
        assert!(s.drag(R, 216.0, 50.0));
        assert_eq!(s.value(), 100.0);
        assert!(s.drag(R, 34.0, 50.0));
        assert_eq!(s.value(), 0.0);
        s.release();
        // Upright, with steps.
        let tall = Rect::new(40.0, 20.0, 20.0, 200.0);
        let mut v = Slider::new(0.0, 10.0, 0.0).vertical().step(1.0);
        assert!(v.press(tall, 50.0, 100.0));
        assert_eq!(v.value(), 6.0);
        assert!(v.drag(tall, 50.0, 30.0));
        assert_eq!(v.value(), 10.0);
    }

    #[test]
    fn keys_and_the_wheel_move_by_steps() {
        let mut s = Slider::new(0.0, 100.0, 50.0);
        assert!(s.step_by(Step::More));
        assert!(close(s.value(), 52.0));
        s.step_by(Step::Less);
        s.step_by(Step::Less);
        assert!(close(s.value(), 48.0));
        s.step_by(Step::PageMore);
        assert!(close(s.value(), 58.0));
        s.step_by(Step::PageLess);
        assert!(close(s.value(), 48.0));
        assert!(s.step_by(Step::Max) && s.value() == 100.0);
        assert!(
            !s.step_by(Step::More) && !s.step_by(Step::Max),
            "the end holds"
        );
        assert!(s.step_by(Step::Min) && s.value() == 0.0);
        assert!(!s.step_by(Step::Less) && !s.step_by(Step::PageLess));
        // A snapping slider moves by its step, and a page is five of them.
        let mut n = Slider::new(0.0, 20.0, 10.0).step(1.0);
        n.step_by(Step::More);
        assert_eq!(n.value(), 11.0);
        n.step_by(Step::PageLess);
        assert_eq!(n.value(), 6.0);
        // The wheel: a notch is a key's step; a snapping slider moves a whole step even for
        // the small turn of a touchpad.
        assert!(s.wheel(3.0));
        assert!(close(s.value(), 6.0));
        assert!(s.wheel(-0.5));
        assert!(close(s.value(), 5.0));
        assert!(!s.wheel(0.0) && !s.wheel(f32::NAN));
        assert!(n.wheel(0.2) && n.value() == 7.0);
        assert!(n.wheel(-2.0) && n.value() == 5.0);
        // On the decibel scale a key is a fiftieth of the track: 1.2 dB.
        let mut vol = Slider::new(0.0, 1.0, 1.0).scale(Scale::Db);
        vol.step_by(Step::Less);
        assert!(close(gain_to_db(vol.value()), -1.2));
        for _ in 0..60 {
            vol.step_by(Step::Less);
        }
        assert_eq!(vol.value(), 0.0, "it reaches silence");
    }

    fn at(bmp: &Bitmap, x: f32, y: f32) -> [u8; 3] {
        let o = (y as usize * bmp.width as usize + x as usize) * 4;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    }

    fn shot(pal: &Palette, draw: impl FnOnce(&mut Painter<'_>)) -> Bitmap {
        let mut bmp = Bitmap::new(240, 100);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, 240.0, 100.0), pal.pane);
            draw(&mut p);
        }
        bmp
    }

    #[test]
    fn the_slider_draws_its_track_its_filled_part_and_its_thumb() {
        for pal in [&LIGHT, &DARK] {
            let s = Slider::new(0.0, 100.0, 50.0);
            let quiet = mix(pal.line, pal.text, 0.18);
            let plain = shot(pal, |p| draw(p, pal, R, &s, Look::default()));
            // Left of the thumb the accent, right of it the track, both 4 px thick.
            assert_eq!(at(&plain, 60.0, 50.0), pal.accent);
            assert_eq!(at(&plain, 180.0, 50.0), quiet);
            assert_eq!(
                (at(&plain, 60.0, 47.0), at(&plain, 60.0, 52.0)),
                (pal.pane, pal.pane)
            );
            assert_eq!(
                (at(&plain, 60.0, 48.0), at(&plain, 60.0, 51.0)),
                (pal.accent, pal.accent)
            );
            // The thumb: a disc of the popup colour with the accent in its middle.
            assert_eq!(at(&plain, 120.0, 50.0), pal.accent);
            assert_eq!(at(&plain, 120.0, 43.0), pal.popup);
            assert_eq!(at(&plain, 120.0, 39.0), pal.pane);
            // The dot is wider under the pointer and narrower in hand.
            let dot = |look: Look| {
                let bmp = shot(pal, |p| draw(p, pal, R, &s, look));
                (100..140)
                    .filter(|&x| at(&bmp, x as f32, 50.5) == pal.accent && (111..129).contains(&x))
                    .count()
            };
            let (rest, hot, held) = (
                dot(Look::default()),
                dot(Look {
                    hot: true,
                    ..Look::default()
                }),
                dot(Look {
                    hot: true,
                    pressed: true,
                    ..Look::default()
                }),
            );
            assert!(held < rest && rest < hot, "{held} {rest} {hot}");
            // The keyboard: a ring outside the thumb, where there was nothing.
            let focus = shot(pal, |p| {
                draw(
                    p,
                    pal,
                    R,
                    &s,
                    Look {
                        focused: true,
                        ..Look::default()
                    },
                )
            });
            assert_eq!(at(&plain, 120.0, 38.0), pal.pane);
            assert_ne!(at(&focus, 120.0, 38.0), pal.pane);
            // Loaded: a stronger piece of track from 60 to 90, and none before the thumb's end.
            let look = Look {
                buffered: Some((0.0, 90.0)),
                ..Look::default()
            };
            let seek = shot(pal, |p| draw(p, pal, R, &s, look));
            let loaded = mix(quiet, pal.text, 0.35);
            assert_eq!(at(&seek, 180.0, 50.0), loaded);
            assert_eq!(at(&seek, 205.0, 50.0), quiet);
            assert_eq!(
                at(&seek, 60.0, 50.0),
                pal.accent,
                "what is played covers what is loaded"
            );
            // Disabled: no accent anywhere, and no hover look.
            let off = Look {
                disabled: true,
                hot: true,
                focused: true,
                ..Look::default()
            };
            let quiet_one = shot(pal, |p| draw(p, pal, R, &s, off));
            assert!(
                !(0..240)
                    .any(|x| (0..100).any(|y| at(&quiet_one, x as f32, y as f32) == pal.accent))
            );
            assert_eq!(at(&quiet_one, 60.0, 50.0), pal.faint);
            // At the low end there is no filled part at all.
            let empty = Slider::new(0.0, 100.0, 0.0);
            let bmp = shot(pal, |p| draw(p, pal, R, &empty, Look::default()));
            assert_eq!(at(&bmp, 60.0, 50.0), quiet);
        }
        // Upright: the filled part is below the thumb.
        let tall = Rect::new(110.0, 5.0, 20.0, 90.0);
        let v = Slider::new(0.0, 100.0, 50.0).vertical();
        let bmp = shot(&LIGHT, |p| draw(p, &LIGHT, tall, &v, Look::default()));
        assert_eq!(at(&bmp, 120.0, 75.0), LIGHT.accent);
        assert_eq!(at(&bmp, 120.0, 25.0), mix(LIGHT.line, LIGHT.text, 0.18));
    }

    /// A slider drawn through the animator, frame after frame.
    struct Moving {
        clock: ManualClock,
        anim: Animator,
        gfx: Gfx,
    }

    impl Moving {
        fn new() -> Moving {
            Moving {
                clock: ManualClock::new(),
                anim: Animator::new(),
                gfx: Gfx::new(),
            }
        }

        fn frame(&mut self, ms: u64, s: &Slider, look: Look) -> (Bitmap, bool) {
            self.clock.advance_ms(ms);
            self.anim.begin_frame(self.clock.now());
            let mut bmp = Bitmap::new(240, 100);
            {
                let mut p = Painter::new(&mut bmp, &mut self.gfx, 1.0);
                p.fill(Rect::new(0.0, 0.0, 240.0, 100.0), LIGHT.pane);
                draw_animated(&mut p, &LIGHT, &mut self.anim, "vol", R, s, look);
            }
            (bmp, self.anim.end_frame().is_some())
        }

        fn settle(&mut self, s: &Slider, look: Look) -> Bitmap {
            for _ in 0..100 {
                let (bmp, more) = self.frame(10, s, look);
                if !more {
                    return bmp;
                }
            }
            panic!("the slider never comes to rest");
        }
    }

    /// The x of the thumb's middle: the middle of the popup-coloured run on the row above the
    /// dot.
    fn thumb_x(bmp: &Bitmap) -> f32 {
        let xs: Vec<usize> = (0..240)
            .filter(|&x| at(bmp, x as f32, 43.0) == LIGHT.popup)
            .collect();
        (xs[0] + xs[xs.len() - 1] + 1) as f32 / 2.0
    }

    #[test]
    fn the_animated_slider_at_rest_is_the_plain_one_and_glides_between() {
        let mut m = Moving::new();
        let mut s = Slider::new(0.0, 100.0, 20.0);
        let plain = |s: &Slider, look: Look| shot(&LIGHT, |p| draw(p, &LIGHT, R, s, look));
        // First seen: at rest, no further frame.
        let (first, more) = m.frame(0, &s, Look::default());
        assert!(!more && first.pixels == plain(&s, Look::default()).pixels);
        // A key moved it far: the frame that says so is unchanged, then the thumb is between,
        // moving only towards its place, and then it is the plain drawing again.
        s.set(80.0);
        let (same, more) = m.frame(0, &s, Look::default());
        assert!(more && same.pixels == first.pixels);
        let (from, to) = (thumb_x(&first), thumb_x(&plain(&s, Look::default())));
        let mut last = from;
        let mut between = 0;
        loop {
            let (bmp, more) = m.frame(10, &s, Look::default());
            let x = thumb_x(&bmp);
            assert!(x >= last && x <= to, "{x} after {last}");
            between += usize::from(x > from && x < to);
            last = x;
            if !more {
                assert!(bmp.pixels == plain(&s, Look::default()).pixels);
                break;
            }
        }
        assert!(between >= 5, "{between} frames on the way");
        // In hand, the thumb is under the pointer on the same frame.
        let held = Look {
            hot: true,
            pressed: true,
            ..Look::default()
        };
        m.settle(&s, held);
        s.set(10.0);
        let (bmp, _) = m.frame(10, &s, held);
        assert_eq!(thumb_x(&bmp), thumb_x(&plain(&s, held)));
        // The dot changes size over a few frames and ends as the plain one.
        let hot = Look {
            hot: true,
            ..Look::default()
        };
        let rest = m.settle(&s, Look::default());
        m.frame(0, &s, hot);
        let (mid, more) = m.frame(40, &s, hot);
        assert!(more && mid.pixels != rest.pixels && mid.pixels != plain(&s, hot).pixels);
        assert!(m.settle(&s, hot).pixels == plain(&s, hot).pixels);
        // Reduced motion: the plain drawing on every frame.
        m.anim.set_policy(Policy::Never);
        s.set(90.0);
        let (bmp, more) = m.frame(10, &s, Look::default());
        assert!(!more && bmp.pixels == plain(&s, Look::default()).pixels);
    }

    #[test]
    fn a_progress_bar_fills_by_its_fraction() {
        let bar = Rect::new(20.0, 48.0, 200.0, 4.0);
        let quiet = mix(LIGHT.line, LIGHT.text, 0.18);
        let filled = |fraction: f32| {
            let bmp = shot(&LIGHT, |p| draw_progress(p, &LIGHT, bar, fraction));
            assert_eq!(at(&bmp, 120.0, 46.0), LIGHT.pane);
            assert_eq!(at(&bmp, 120.0, 53.0), LIGHT.pane);
            (20..220)
                .filter(|&x| at(&bmp, x as f32, 50.0) == LIGHT.accent)
                .count()
        };
        // The round ends take a pixel or two of full colour off each length.
        for (fraction, want) in [(0.25, 50), (0.5, 100), (1.0, 200), (7.0, 200)] {
            let got = filled(fraction);
            assert!(
                (want - 3..=want).contains(&got),
                "{fraction}: {got} of {want}"
            );
        }
        assert_eq!((filled(0.0), filled(-2.0), filled(f32::NAN)), (0, 0, 0));
        // The first sliver is a dot as wide as the bar is high.
        assert!((1..=4).contains(&filled(0.001)));
        let bmp = shot(&LIGHT, |p| draw_progress(p, &LIGHT, bar, 0.5));
        assert_eq!(at(&bmp, 200.0, 50.0), quiet);
    }

    #[test]
    fn the_marks_without_an_end_move_while_they_are_drawn_and_stand_still_when_asked() {
        let bar = Rect::new(20.0, 48.0, 200.0, 4.0);
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut gfx = Gfx::new();
        let mut frame = |anim: &mut Animator, ms: u64, drawn: bool| {
            clock.advance_ms(ms);
            anim.begin_frame(clock.now());
            let mut bmp = Bitmap::new(240, 100);
            {
                let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
                p.fill(Rect::new(0.0, 0.0, 240.0, 100.0), LIGHT.pane);
                if drawn {
                    draw_progress_indeterminate(&mut p, &LIGHT, anim, "busy", bar);
                    draw_spinner(&mut p, &LIGHT, anim, "spin", 120.0, 20.0, 10.0);
                }
            }
            (bmp, anim.end_frame().is_some())
        };
        // Drawn: every frame asks for the next, and the picture changes.
        let (a, more) = frame(&mut anim, 0, true);
        assert!(more);
        let mut last = a;
        let (mut changed, mut lengths) = (0, std::collections::BTreeSet::new());
        for _ in 0..40 {
            let (bmp, more) = frame(&mut anim, 40, true);
            assert!(more);
            changed += usize::from(bmp.pixels != last.pixels);
            lengths.insert(
                (0..240)
                    .filter(|&x| at(&bmp, x as f32, 50.0) == LIGHT.accent)
                    .count(),
            );
            // The piece never leaves its track, and the spinner never its circle.
            for x in 0..240 {
                let c = at(&bmp, x as f32, 50.0);
                assert!((20..220).contains(&x) || c == LIGHT.pane);
                assert_eq!(at(&bmp, x as f32, 34.0), LIGHT.pane);
            }
            last = bmp;
        }
        assert!(changed >= 38, "it stood still on {} frames", 40 - changed);
        // The bar's piece itself is there and changes length as it runs.
        assert!(
            lengths.len() >= 15 && *lengths.last().unwrap() > 100,
            "{lengths:?}"
        );
        // No longer drawn: the window sleeps, and nothing is remembered.
        assert!(!frame(&mut anim, 40, false).1);
        assert!(anim.is_empty());
        // Reduced motion: both are there, still, and ask for no frame.
        anim.set_system_reduced(true);
        let (still, more) = frame(&mut anim, 40, true);
        assert!(!more);
        assert_eq!(frame(&mut anim, 400, true).0.pixels, still.pixels);
        let accent = |y: f32| {
            (0..240)
                .filter(|&x| at(&still, x as f32, y) == LIGHT.accent)
                .count()
        };
        assert!(
            (75..=81).contains(&accent(50.0)),
            "a piece of two fifths: {}",
            accent(50.0)
        );
        assert!(accent(10.0) > 0, "the top of the spinner's ring");
    }
    /// The sheet a person looks at: the slider in its states, the seek bar, the progress bars
    /// and the spinner, light and dark, at two scales.
    /// `cargo test -p mdshared-ui slider_sheet -- --ignored`, then open
    /// target/specimen/slider.png and slider-150.png.
    #[test]
    #[ignore]
    fn slider_sheet() {
        for (scale, file) in [(1.0, "slider.png"), (1.5, "slider-150.png")] {
            let mut bmp = Bitmap::new((560.0 * scale) as u32, (360.0 * scale) as u32);
            let mut gfx = Gfx::new();
            let clock = ManualClock::new();
            let mut anim = Animator::new();
            anim.set_max_step(Duration::from_secs(1));
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            // Two frames 0.7 s apart, so the marks that move are caught on their way.
            for (half, pal) in [&LIGHT, &DARK, &LIGHT, &DARK].into_iter().enumerate() {
                if half % 2 == 0 {
                    clock.advance_ms(700);
                    anim.begin_frame(clock.now());
                }
                let half = half % 2;
                let oy = half as f32 * 180.0;
                p.fill(Rect::new(0.0, oy, 560.0, 180.0), pal.pane);
                let o = p.offset(0.0, oy);
                let s = Slider::new(0.0, 100.0, 35.0);
                let row = |k: usize| Rect::new(20.0, 14.0 + 30.0 * k as f32, 220.0, 20.0);
                let hot = Look {
                    hot: true,
                    ..Look::default()
                };
                let held = Look {
                    pressed: true,
                    ..hot
                };
                let focus = Look {
                    focused: true,
                    ..Look::default()
                };
                let off = Look {
                    disabled: true,
                    ..Look::default()
                };
                let seek = Look {
                    buffered: Some((0.0, 70.0)),
                    ..Look::default()
                };
                for (k, look) in [Look::default(), hot, held, focus, off]
                    .into_iter()
                    .enumerate()
                {
                    draw(&mut p, pal, row(k), &s, look);
                }
                draw(&mut p, pal, Rect::new(280.0, 14.0, 260.0, 20.0), &s, seek);
                draw_progress(&mut p, pal, Rect::new(280.0, 52.0, 260.0, 4.0), 0.6);
                draw_progress(&mut p, pal, Rect::new(280.0, 72.0, 260.0, 6.0), 0.02);
                let busy = Rect::new(280.0, 94.0, 260.0, 4.0);
                draw_progress_indeterminate(&mut p, pal, &mut anim, ("busy", half), busy);
                draw_spinner(&mut p, pal, &mut anim, ("spin", half), 300.0, 135.0, 12.0);
                draw_spinner(&mut p, pal, &mut anim, ("small", half), 340.0, 135.0, 6.0);
                let v = Slider::new(0.0, 1.0, 0.5).scale(Scale::Db).vertical();
                for k in 0..4 {
                    let tall = Rect::new(400.0 + 34.0 * k as f32, 112.0, 20.0, 60.0);
                    let mut one = v;
                    one.set(0.02 * 6f32.powi(k));
                    draw(&mut p, pal, tall, &one, Look::default());
                }
                p.restore_offset(o);
            }
            let out =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
            bmp.write_png(&out.join(file)).unwrap();
        }
    }
}
