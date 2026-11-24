//! A rotary knob and a fader for an audio mixer or a music player: the two shapes a hand
//! reaches for that a flat slider does not give. Both hold a [`slider::Slider`] and leave the
//! value, its ends, its snapping and its scale to it, so a knob and a fader follow the same
//! rules as a slider and can be read with the same code.
//!
//! The models ([`Knob`], [`Fader`]) hold the value and answer questions of geometry; the caller
//! owns the events and passes them in ([`Knob::press`], [`Knob::drag`], [`Knob::release`],
//! [`Knob::reset`], [`Knob::wheel`], and the same on [`Fader`]), and reads [`Knob::value`]
//! afterwards. Double clicks are recognised by the caller, which then calls
//! [`reset`](Knob::reset); Shift is read by the caller, which passes it to
//! [`drag`](Knob::drag) as `fine`. A knob turning is vertical drag only: up is more, and the
//! pointer's x is not used. Drawing is apart ([`draw_knob`], [`draw_fader`]) and changes
//! nothing, and nothing remembers the rectangle: each call is given it.
//!
//! Deliberately not here: motion (the pointer is at its value on the frame it is drawn, and no
//! frame is asked for), the value as text (a dB readout is the caller's, from
//! [`Knob::value`]), keyboard focus handling (the caller passes [`slider::Look::focused`] and
//! maps the keys to [`step_by`](Knob::step_by)), and the scale of ticks beyond the decibel
//! marks of [`DEFAULT_TICKS`], which the caller may replace.

use crate::gfx::{Face, Painter, Path, Rect, mix};
use crate::slider::{self, Slider, Step};
use crate::theme::Palette;

/// How far the pointer travels, in logical px, for a knob to go from one end to the other: 200
/// px of drag for the whole range, a comfortable flick of the wrist rather than a scroll across
/// the screen.
pub const KNOB_TRAVEL: f32 = 200.0;

/// The part of a turn a knob sweeps: 270 degrees, from [`KNOB_START`] round to the other lower
/// corner.
pub const KNOB_SWEEP: f32 = 0.75;

/// Where a knob's sweep begins, in the unit [`Painter::arc`] takes: turns clockwise from twelve
/// o'clock. So the start is the lower left at half past seven, which `Painter::arc` counts as
/// -0.375 (225 degrees the other way), and the sweep ends at 0.375, the lower right. The middle
/// of the sweep is 0, straight up.
pub const KNOB_START: f32 = -0.375;

/// The thickness of the ring a knob's value is drawn on.
pub const KNOB_RING: f32 = 4.0;

/// The gap between the inside of the ring and the body disc.
pub const KNOB_GAP: f32 = 4.0;

/// The width of a fader's cap. Its height is [`slider::THUMB`], the height of a slider's thumb,
/// so a tick's y is the y the slider's own thumb would have at that value.
pub const FADER_CAP: f32 = 26.0;

/// The decibel marks a fader shows unless the caller gives others: a little above full scale,
/// then the usual mixing marks down to the floor of the decibel scale. Marks outside the
/// slider's own range are left out by [`Fader::ticks`].
pub const DEFAULT_TICKS: &[f32] = &[6.0, 0.0, -6.0, -12.0, -18.0, -24.0, -36.0, -48.0, -60.0];

/// Where a drag was taken hold of: the pointer's y, the place along the track at that moment,
/// and whether the drag is fine, which is not known until the first move. Kept so that turning
/// Shift on or off mid-drag re-bases the anchor to the pointer and the value that is there, and
/// the value does not jump; the first move only says how the drag is to be read.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Anchor {
    y: f32,
    fraction: f32,
    fine: Option<bool>,
}

/// A knob: a value on a [`Slider`], turned by dragging up and down, and the angle its sweep
/// begins at. It is a `Slider` held round, so its ends, its step and its scale ([`slider::Scale`])
/// are the slider's.
///
/// A press inside the knob's circle takes hold of it and leaves the value where it is, unlike a
/// press on a slider's track, which jumps there; a knob is turned, not pointed at. A drag of
/// [`KNOB_TRAVEL`] px from one end reaches the other, and Shift (the `fine` flag) makes the same
/// motion ten times finer. The value a finished drag leaves stays until the caller resets it,
/// which a double click usually is.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Knob {
    slider: Slider,
    /// Where [`reset`](Self::reset) goes, kept as given: a value outside the slider's ends is
    /// kept to them when it is taken, and one that is not a number changes nothing.
    default: f32,
    /// Draw the value's arc from the middle of the track outwards, for a control that is round
    /// about its centre: pan, balance, a cut or a boost.
    bipolar: bool,
    anchor: Option<Anchor>,
}

impl Knob {
    /// A knob holding `slider`, which resets to `default`. The slider is taken as it is: its
    /// ends, its step, its scale and its axis (a knob has no axis, so the axis changes nothing).
    pub fn new(slider: Slider, default: f32) -> Knob {
        Knob {
            slider,
            default,
            bipolar: false,
            anchor: None,
        }
    }

    /// The same knob drawn from the middle value outwards, as a pan or a tone control is: the
    /// value's arc grows both ways from the centre, so a value of no change is drawn with no
    /// arc at all. The centre is the slider's zero where its range holds zero, and the middle
    /// of the track otherwise.
    pub fn bipolar(mut self) -> Knob {
        self.bipolar = true;
        self
    }

    /// Whether the value's arc is drawn from the middle outwards.
    pub fn is_bipolar(&self) -> bool {
        self.bipolar
    }

    /// The value, as the slider keeps it: between its ends and snapped to its step.
    pub fn value(&self) -> f32 {
        self.slider.value()
    }

    /// Where the value lies along the track: 0 at the low end, 1 at the high end, by the
    /// slider's scale.
    pub fn fraction(&self) -> f32 {
        self.slider.fraction()
    }

    /// The value [`reset`](Self::reset) goes to, as it was given.
    pub fn default_value(&self) -> f32 {
        self.default
    }

    /// The slider inside, to read its ends, its step or its scale from.
    pub fn slider(&self) -> &Slider {
        &self.slider
    }

    /// The slider inside, to set or step directly. A caller that sets it this way skips the
    /// drag bookkeeping, which is what a program doing the work itself wants.
    pub fn slider_mut(&mut self) -> &mut Slider {
        &mut self.slider
    }

    /// Whether the knob is in hand.
    pub fn dragging(&self) -> bool {
        self.anchor.is_some()
    }

    /// Where the middle of the value's arc is, 0 to 1 along the track: the slider's zero when
    /// its range holds zero (a pan from -1 to 1 is at a half), and the middle of the track when
    /// it does not.
    pub fn centre(&self) -> f32 {
        if self.slider.min() <= 0.0 && self.slider.max() >= 0.0 {
            self.slider.fraction_of(0.0)
        } else {
            0.5
        }
    }

    /// The angle at `fraction` (0 to 1 along the track), in the unit [`Painter::arc`] takes:
    /// turns clockwise from twelve o'clock. Fraction 0 is [`KNOB_START`] (the lower left, half
    /// past seven), fraction 1 is [`KNOB_START`] plus [`KNOB_SWEEP`] (the lower right, half past
    /// four), and a half is straight up. A fraction outside 0 to 1, or one that is not a
    /// number, is the nearer end.
    pub fn angle_of(&self, fraction: f32) -> f32 {
        let t = if fraction.is_nan() {
            0.0
        } else {
            fraction.clamp(0.0, 1.0)
        };
        KNOB_START + KNOB_SWEEP * t
    }

    /// Whether a point is on the knob in `r`: inside the circle inscribed in it, which is the
    /// ring and the body together. A point that is not a number, or a rectangle with no size,
    /// is not on it.
    pub fn hit(&self, r: Rect, x: f32, y: f32) -> bool {
        if !r.w.is_finite() || !r.h.is_finite() || !x.is_finite() || !y.is_finite() {
            return false;
        }
        let radius = r.w.min(r.h) / 2.0;
        radius > 0.0 && (x - r.cx()).hypot(y - r.cy()) <= radius
    }

    /// A press of the primary button: on the knob it takes hold and returns true, with the value
    /// left where it is (a knob does not jump to where it was clicked). Off the knob, or with a
    /// point that is not a number, it returns false and nothing changes.
    pub fn press(&mut self, r: Rect, x: f32, y: f32) -> bool {
        if !self.hit(r, x, y) {
            return false;
        }
        self.anchor = Some(Anchor {
            y,
            fraction: self.slider.fraction(),
            fine: None,
        });
        true
    }

    /// The pointer moved with the knob in hand by dragging up (`y` smaller) or down: up is
    /// more, and [`KNOB_TRAVEL`] px from the press reaches the other end. With `fine` the same
    /// motion is a tenth as much, and turning `fine` on or off mid-drag re-bases the anchor at
    /// the pointer, so the value does not jump. Returns whether the value changed; false with
    /// no knob in hand or a `y` that is not a number. The pointer's x is not used.
    pub fn drag(&mut self, _x: f32, y: f32, fine: bool) -> bool {
        let Some(mut anchor) = self.anchor else {
            return false;
        };
        if !y.is_finite() {
            return false;
        }
        match anchor.fine {
            Some(was) if was != fine => {
                anchor = Anchor {
                    y,
                    fraction: self.slider.fraction(),
                    fine: Some(fine),
                }
            }
            None => anchor.fine = Some(fine),
            Some(_) => {}
        }
        self.anchor = Some(anchor);
        let travel = if fine {
            KNOB_TRAVEL * 10.0
        } else {
            KNOB_TRAVEL
        };
        let fraction = anchor.fraction + (anchor.y - y) / travel;
        self.slider.set(self.slider.value_of(fraction))
    }

    /// Lets go of the knob. Returns whether it was in hand, which is the moment a double click
    /// has finished or a drag is reported.
    pub fn release(&mut self) -> bool {
        self.anchor.take().is_some()
    }

    /// Puts the value back to [`default_value`](Self::default_value), kept between the slider's
    /// ends and snapped to its step. Returns whether the value changed; a default that is not a
    /// number changes nothing.
    pub fn reset(&mut self) -> bool {
        self.slider.set(self.default)
    }

    /// The wheel turned over the knob by `notches` (positive away from the user, which is more):
    /// the slider's own wheel step a notch. Returns whether the value changed.
    pub fn wheel(&mut self, notches: f32) -> bool {
        self.slider.wheel(notches)
    }

    /// A key press, as [`Slider::step_by`] takes it: a step, a page or an end. Returns whether
    /// the value changed.
    pub fn step_by(&mut self, step: Step) -> bool {
        self.slider.step_by(step)
    }
}

/// A point at `r` px from `cx, cy` in the direction `turns` (clockwise from twelve o'clock),
/// the direction [`Painter::arc`] takes.
fn point_at(cx: f32, cy: f32, r: f32, turns: f32) -> (f32, f32) {
    let a = turns * std::f32::consts::TAU;
    (cx + r * a.sin(), cy - r * a.cos())
}

/// Draws a knob in `r`: the ring a full sweep in the quiet track colour, the value's part of it
/// in the accent (from the low end, or from the centre outwards on a [`bipolar`](Knob::bipolar)
/// knob), a body disc with an outline, and a pointer from near the middle to the value's angle.
/// Under the pointer the body's outline is stronger, in hand the disc is a touch darker, and
/// with the keyboard on it a ring goes just inside `r`'s edge. A disabled knob has no accent.
///
/// The knob is the circle inscribed in `r`; it wants a square rectangle of about 80 px and up. A
/// rectangle with no size, or with a side that is not a number, draws nothing. The rest of
/// [`slider::Look`] ([`buffered`](slider::Look::buffered)) is not drawn: a knob has no loaded
/// part.
pub fn draw_knob(p: &mut Painter, pal: &Palette, r: Rect, knob: &Knob, look: slider::Look) {
    if !r.x.is_finite()
        || !r.y.is_finite()
        || !r.w.is_finite()
        || !r.h.is_finite()
        || r.w <= 0.0
        || r.h <= 0.0
    {
        return;
    }
    let radius = r.w.min(r.h) / 2.0;
    let (cx, cy) = (r.cx(), r.cy());
    let arc_r = radius - 1.0 - KNOB_RING / 2.0;
    let body = arc_r - KNOB_RING / 2.0 - KNOB_GAP;
    if body <= 0.0 {
        return;
    }
    let quiet = mix(pal.line, pal.text, 0.18);
    let ink = if look.disabled { pal.faint } else { pal.accent };
    p.arc(
        cx,
        cy,
        arc_r,
        KNOB_RING,
        knob.angle_of(0.0),
        knob.angle_of(1.0),
        quiet,
        1.0,
    );
    let (from, to) = if knob.bipolar {
        let centre = knob.centre();
        let at = knob.fraction();
        (centre.min(at), centre.max(at))
    } else {
        (0.0, knob.fraction())
    };
    if to > from {
        p.arc(
            cx,
            cy,
            arc_r,
            KNOB_RING,
            knob.angle_of(from),
            knob.angle_of(to),
            ink,
            1.0,
        );
    }
    let body_fill = if look.disabled {
        mix(pal.pane, pal.line, 0.5)
    } else if look.pressed {
        mix(pal.popup, pal.press, 0.55)
    } else {
        pal.popup
    };
    let edge = if look.disabled {
        pal.line
    } else if look.hot || look.pressed {
        mix(pal.line, pal.text, 0.5)
    } else {
        mix(pal.line, pal.text, 0.25)
    };
    p.ring(cx, cy, body / 2.0, body, body_fill, 1.0);
    p.ring(cx, cy, body - 0.5, 1.0, edge, 1.0);
    let at = knob.angle_of(knob.fraction());
    let (ix, iy) = point_at(cx, cy, body * 0.35, at);
    let (ox, oy) = point_at(cx, cy, body - 2.0, at);
    p.stroke_path(&Path::new().move_to(ix, iy).line_to(ox, oy), 2.5, ink, 1.0);
    if look.focused && !look.disabled {
        p.ring(cx, cy, radius - 0.75, 1.5, pal.accent, 1.0);
    }
}

/// A fader: a value on an upright [`Slider`], drawn as a cap on a thin track with the decibel
/// marks beside it. It is the fader of a mixing desk; the slider inside is made upright
/// ([`Slider::vertical`]) so the low end is always the bottom, and its scale is untouched.
///
/// A fader's value is in its slider's own unit. A mixing desk's runs from the decibel floor to
/// a little above full scale ([`slider::DB_FLOOR`] to +6), equal steps of which are equal
/// decibels on a linear slider, and [`DEFAULT_TICKS`] and the labels are decibels there. A
/// [`Scale::Db`](slider::Scale::Db) slider keeps a gain from 0 to 1 instead, and its marks are
/// then gains; [`slider::gain_to_db`] turns them into numbers a label can show.
///
/// A press on the track jumps the value there, as on a slider; a press on the cap takes hold of
/// it without a jump. A drag moves the value by the slice of the track passed over, and with
/// `fine` (Shift) a tenth of it. Double clicks are the caller's, which calls
/// [`reset`](Self::reset).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Fader {
    slider: Slider,
    default: f32,
    anchor: Option<Anchor>,
}

impl Fader {
    /// A fader holding `slider`, made upright, which resets to `default`. Its ends, step and
    /// scale are kept, but not its axis.
    pub fn new(slider: Slider, default: f32) -> Fader {
        Fader {
            slider: slider.vertical(),
            default,
            anchor: None,
        }
    }

    /// The value, as the slider keeps it.
    pub fn value(&self) -> f32 {
        self.slider.value()
    }

    /// Where the value lies along the track: 0 at the bottom, 1 at the top, by the slider's
    /// scale.
    pub fn fraction(&self) -> f32 {
        self.slider.fraction()
    }

    /// The value [`reset`](Self::reset) goes to, as it was given.
    pub fn default_value(&self) -> f32 {
        self.default
    }

    /// The slider inside, to read its ends, its step or its scale from.
    pub fn slider(&self) -> &Slider {
        &self.slider
    }

    /// The slider inside, to set or step directly.
    pub fn slider_mut(&mut self) -> &mut Slider {
        &mut self.slider
    }

    /// Whether the cap is in hand.
    pub fn dragging(&self) -> bool {
        self.anchor.is_some()
    }

    /// A press of the primary button: inside `r` it takes hold and returns true, jumping the
    /// value if the press was off the cap (as a slider does) and leaving it if the cap was hit.
    /// Outside `r` it returns false and nothing changes.
    pub fn press(&mut self, r: Rect, x: f32, y: f32) -> bool {
        if !self.slider.press(r, x, y) {
            return false;
        }
        self.anchor = Some(Anchor {
            y,
            fraction: self.slider.fraction(),
            fine: None,
        });
        true
    }

    /// The pointer moved with the cap in hand: the value follows by the slice of the track
    /// between the press and the pointer, and with `fine` by a tenth of it. Turning `fine` on or
    /// off mid-drag re-bases the anchor at the pointer, so the value does not jump. Returns
    /// whether the value changed; false with no cap in hand, a `y` that is not a number, or a
    /// rectangle too short to have a track. The pointer's x is not used.
    pub fn drag(&mut self, r: Rect, _x: f32, y: f32, fine: bool) -> bool {
        let Some(mut anchor) = self.anchor else {
            return false;
        };
        if !y.is_finite() {
            return false;
        }
        let travel = r.h - slider::THUMB;
        if travel <= 0.0 || !travel.is_finite() {
            return false;
        }
        match anchor.fine {
            Some(was) if was != fine => {
                anchor = Anchor {
                    y,
                    fraction: self.slider.fraction(),
                    fine: Some(fine),
                }
            }
            None => anchor.fine = Some(fine),
            Some(_) => {}
        }
        self.anchor = Some(anchor);
        let travel = if fine { travel * 10.0 } else { travel };
        let fraction = anchor.fraction + (anchor.y - y) / travel;
        self.slider.set(self.slider.value_of(fraction))
    }

    /// Lets go of the cap. Returns whether it was in hand.
    pub fn release(&mut self) -> bool {
        self.anchor = None;
        self.slider.release()
    }

    /// Puts the value back to [`default_value`](Self::default_value), kept between the slider's
    /// ends and snapped to its step. Returns whether the value changed; a default that is not a
    /// number changes nothing.
    pub fn reset(&mut self) -> bool {
        self.slider.set(self.default)
    }

    /// The wheel turned over the fader by `notches`: the slider's own wheel step a notch.
    /// Returns whether the value changed.
    pub fn wheel(&mut self, notches: f32) -> bool {
        self.slider.wheel(notches)
    }

    /// A key press, as [`Slider::step_by`] takes it. Returns whether the value changed.
    pub fn step_by(&mut self, step: Step) -> bool {
        self.slider.step_by(step)
    }

    /// The y of each mark of `marks` in `r`, with the mark itself, from the top of the fader
    /// down. A mark outside the slider's range or one that is not a number is left out, so a
    /// decibel list serves sliders of any range. The y is the middle of a slider's thumb at that
    /// value, which is where [`draw_fader`] puts the tick; a rectangle with no size gives none.
    pub fn ticks(&self, r: Rect, marks: &[f32]) -> Vec<(f32, f32)> {
        if !r.w.is_finite() || !r.h.is_finite() || r.w <= 0.0 || r.h <= 0.0 {
            return Vec::new();
        }
        let (min, max) = (self.slider.min(), self.slider.max());
        let mut ticks: Vec<(f32, f32)> = marks
            .iter()
            .copied()
            .filter(|v| v.is_finite() && (min..=max).contains(v))
            .map(|v| {
                let mut probe = self.slider;
                probe.set(v);
                (probe.thumb_center(r).1, v)
            })
            .collect();
        ticks.sort_by(|a, b| a.0.total_cmp(&b.0));
        ticks
    }
}

/// What a fader's tick reads: the number with no decimals when it is whole. A mark at the
/// bottom of a fader whose low end reaches the decibel floor ([`slider::DB_FLOOR`]) is silence,
/// and reads "-inf" rather than "-60": a scale from silence to a little above full is what
/// [`DEFAULT_TICKS`] is for.
fn tick_label(fader: &Fader, mark: f32) -> String {
    let slider = fader.slider();
    if slider.fraction_of(mark) <= 1e-4 && slider.min() <= slider::DB_FLOOR {
        return "-inf".to_string();
    }
    if mark == 0.0 {
        return "0".to_string();
    }
    if (mark - mark.round()).abs() < 0.05 {
        format!("{mark:.0}")
    } else {
        format!("{mark:.1}")
    }
}

/// Draws a fader in `r` with the decibel marks of [`DEFAULT_TICKS`] that fall inside its
/// slider's range: the track up the middle, the part from the bottom to the value in the accent,
/// a tick beside the track for each mark (the zero mark longer and in the text colour), a label
/// for each when `labels` is set, and the cap, a rounded rectangle wider than the track with a
/// line across it in the accent. Under the pointer the cap's edge is stronger, in hand it is
/// stronger still, with the keyboard on it a ring goes round the cap, and a disabled fader has
/// no accent.
///
/// `r` is the whole column: the track is its middle, the marks its right, the labels its left
/// (a column 80 px wide). The nine labels of [`DEFAULT_TICKS`] stand apart down about 180 px of
/// track; a shorter fader wants fewer marks, which [`draw_fader_with`] takes. A rectangle with
/// no size, a side that is not a number, or one too short to hold the cap draws nothing.
pub fn draw_fader(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    fader: &Fader,
    look: slider::Look,
    labels: bool,
) {
    draw_fader_with(p, pal, r, fader, look, DEFAULT_TICKS, labels);
}

/// [`draw_fader`] with the caller's own marks, in whatever unit the slider's values are (a
/// frequency's hertz, a gain's decibels); [`Fader::ticks`] says which are inside the range and
/// where they go. The label of a whole number has no decimals; on a fader whose low end reaches
/// the decibel floor, the mark at the bottom reads "-inf", since that is what a decibel scale
/// calls silence.
pub fn draw_fader_with(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    fader: &Fader,
    look: slider::Look,
    marks: &[f32],
    labels: bool,
) {
    if !r.x.is_finite()
        || !r.y.is_finite()
        || !r.w.is_finite()
        || !r.h.is_finite()
        || r.w <= 0.0
        || r.h <= 0.0
    {
        return;
    }
    let (cx, low, high) = (
        r.cx(),
        r.bottom() - slider::THUMB / 2.0,
        r.y + slider::THUMB / 2.0,
    );
    let travel = low - high;
    if travel <= 0.0 {
        return;
    }
    let quiet = mix(pal.line, pal.text, 0.18);
    let ink = if look.disabled { pal.faint } else { pal.accent };
    let track = Rect::new(
        cx - slider::TRACK / 2.0,
        high - slider::TRACK / 2.0,
        slider::TRACK,
        travel + slider::TRACK,
    );
    p.rbox(track, slider::TRACK / 2.0, quiet);
    let at = low - travel * fader.fraction();
    if at < low {
        p.rbox(
            Rect::new(
                cx - slider::TRACK / 2.0,
                at - slider::TRACK / 2.0,
                slider::TRACK,
                low - at + slider::TRACK,
            ),
            slider::TRACK / 2.0,
            ink,
        );
    }
    let label_right = cx - FADER_CAP / 2.0 - 6.0;
    let mark_left = cx + FADER_CAP / 2.0 + 8.0;
    for (y, mark) in fader.ticks(r, marks) {
        let strong = mark == 0.0;
        let length = if strong { 8.0 } else { 5.0 };
        let colour = if look.disabled {
            pal.faint
        } else if strong {
            pal.text
        } else {
            pal.faint
        };
        p.fill(Rect::new(mark_left, y - 0.5, length, 1.0), colour);
        if labels {
            let text = tick_label(fader, mark);
            let width = p.measure(Face::Ui, 9.5, &text);
            let x = (label_right - width).max(r.x);
            p.text(Face::Ui, 9.5, x, y, &text, colour, label_right - x);
        }
    }
    let cap = Rect::new(
        cx - FADER_CAP / 2.0,
        at - slider::THUMB / 2.0,
        FADER_CAP,
        slider::THUMB,
    );
    let fill = if look.disabled { pal.pane } else { pal.popup };
    let edge = if look.disabled {
        pal.line
    } else if look.hot || look.pressed {
        mix(pal.line, pal.text, 0.5)
    } else {
        mix(pal.line, pal.text, 0.25)
    };
    p.frame(cap, 3.0, fill, edge);
    p.fill(Rect::new(cap.x + 4.0, at - 1.0, cap.w - 8.0, 2.0), ink);
    if look.focused && !look.disabled {
        p.ring(cx, at, FADER_CAP / 2.0 + 3.0, 1.5, pal.accent, 1.0);
    }
}

#[cfg(test)]
#[path = "knob_tests.rs"]
mod tests;
