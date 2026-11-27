//! A level meter: the ballistics of a peak meter with a held peak, and the drawing of one.
//!
//! The model ([`LevelMeter`]) holds two numbers: the level, which jumps up to a reading at once
//! and falls back at a steady rate, and the held peak, which stays where it was for a while and
//! then falls to meet the level. The caller owns the measuring: it finds the largest absolute
//! sample of each buffer of audio, calls [`LevelMeter::update`] with it and the clock's
//! [`Instant`] each time audio arrives, and asks [`LevelMeter::next_wake`] for the moment its
//! event loop should wake to draw the fall. No clock is read here: every function that needs
//! time is given it.
//!
//! Drawing is apart ([`draw_meter`], [`draw_stereo`]) and changes nothing. Colours come from the
//! palette only: a quiet track, three zones of the level, and the held peak as a line.
//!
//! Deliberately not here: RMS or loudness (LUFS) measurement, capturing audio, and a decibel
//! scale with labels or tick marks (the caller draws those from [`LevelMeter::fraction`] and
//! [`LevelMeter::peak_fraction`]).

use crate::gfx::{Painter, Rect, mix};
use crate::theme::Palette;
use std::time::{Duration, Instant};

/// The level at which the accent gives way to the warning colour.
pub const ZONE_WARN_DB: f32 = -18.0;
/// The level at which the warning colour gives way to the danger colour.
pub const ZONE_DANGER_DB: f32 = -6.0;

/// The greatest level shown, in decibels over full scale: a louder reading is cut to this.
const CEILING_DB: f32 = 6.0;
/// The rounding of the track and of the level on it, in logical px.
const ROUND: f32 = 2.0;
/// The gap between the two meters of a stereo pair, in logical px.
const STEREO_GAP: f32 = 2.0;
/// The least time [`LevelMeter::next_wake`] will ask to wait.
const LEAST_WAKE: Duration = Duration::from_millis(1);

/// A peak meter: a level that falls back after each reading, and the loudest recent reading
/// held above it.
///
/// The defaults are a floor of -60 dB, a release of 20 dB a second, a hold of 1.5 s and a fall of
/// the held peak of 20 dB a second. Every builder method below keeps the default when its
/// argument is not a usable number.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelMeter {
    /// Where the level and the peak stop falling, in decibels; always below zero.
    floor: f32,
    /// How fast the level falls, in decibels a second.
    release: f32,
    /// How long the held peak stays where it is before it starts to fall.
    hold: Duration,
    /// How fast the held peak falls after the hold, in decibels a second.
    fall: f32,
    /// The level in decibels as it stood at `at`.
    level: f32,
    /// When `level` was last set; `None` before the first reading and after a reset.
    at: Option<Instant>,
    /// The held peak in decibels as it stood at `peak_at`.
    peak: f32,
    /// When the hold of `peak` began; `None` before the first reading and after a reset.
    peak_at: Option<Instant>,
    /// A reading at or above full scale has arrived since the clip was last reset.
    clipped: bool,
}

impl LevelMeter {
    /// A meter at its floor, with the default floor, release, hold and fall.
    pub fn new() -> LevelMeter {
        LevelMeter {
            floor: -60.0,
            release: 20.0,
            hold: Duration::from_millis(1500),
            fall: 20.0,
            level: -60.0,
            at: None,
            peak: -60.0,
            peak_at: None,
            clipped: false,
        }
    }

    /// The same with a different floor: the decibel level at which the meter is empty and
    /// stops falling. A floor of zero or above, or one that is not a number, is ignored and
    /// the default -60 kept.
    pub fn floor(mut self, db: f32) -> LevelMeter {
        if db.is_finite() && db < 0.0 {
            self.floor = db;
            self.level = self.level.max(db);
            self.peak = self.peak.max(db);
        }
        self
    }

    /// The same with a different release: how many decibels the level falls each second. Zero,
    /// a negative rate or one that is not a number is ignored and 20 kept.
    pub fn release(mut self, db_per_sec: f32) -> LevelMeter {
        if db_per_sec.is_finite() && db_per_sec > 0.0 {
            self.release = db_per_sec;
        }
        self
    }

    /// The same with a different hold: how long the held peak stays before it starts to fall.
    /// A hold of no time is allowed.
    pub fn hold(mut self, hold: Duration) -> LevelMeter {
        self.hold = hold;
        self
    }

    /// The same with a different fall of the held peak, in decibels a second after the hold.
    /// Zero, a negative rate or one that is not a number is ignored and 20 kept.
    pub fn fall(mut self, db_per_sec: f32) -> LevelMeter {
        if db_per_sec.is_finite() && db_per_sec > 0.0 {
            self.fall = db_per_sec;
        }
        self
    }

    /// Takes a new reading at `now`: `peak` is the largest absolute sample since the last
    /// reading, where 1 is full scale (0 dB) and 0 is silence. The level jumps up to the
    /// reading at once when the reading is above the level as it stands at `now`; otherwise
    /// the level keeps falling. A reading at or above the held peak as it stands at `now`
    /// becomes the held peak, held from `now`. A reading at or above full scale sets the clip
    /// mark.
    ///
    /// A negative `peak` counts by its magnitude. A `peak` that is not a number changes
    /// nothing at all, not even the clip mark; a level below the floor, or a reading of zero,
    /// is the floor; a reading above +6 dB is cut to +6.
    pub fn update(&mut self, now: Instant, peak: f32) {
        if peak.is_nan() {
            return;
        }
        let reading = (20.0 * peak.abs().log10()).clamp(self.floor, CEILING_DB);
        let level = self.level_db(now).max(reading);
        self.level = level;
        self.at = Some(now);

        if reading >= self.peak_db(now) {
            self.peak = reading;
            self.peak_at = Some(now);
        }
        if peak.abs() >= 1.0 {
            self.clipped = true;
        }
    }

    /// The level at `now`, in decibels: the last level less the release for the time since it
    /// was set, never below the floor. A `now` earlier than the last update counts as no time
    /// passed. A meter that has never been updated is at its floor.
    pub fn level_db(&self, now: Instant) -> f32 {
        match self.at {
            Some(at) => {
                let secs = now.saturating_duration_since(at).as_secs_f32();
                (self.level - self.release * secs).max(self.floor)
            }
            None => self.floor,
        }
    }

    /// The held peak at `now`, in decibels: where it was set until the hold is over, then
    /// falling at the fall rate, and never below [`LevelMeter::level_db`] at the same moment.
    pub fn peak_db(&self, now: Instant) -> f32 {
        let level = self.level_db(now);
        match self.peak_at {
            Some(at) => {
                let since = now.saturating_duration_since(at);
                let fallen = if since > self.hold {
                    (since - self.hold).as_secs_f32() * self.fall
                } else {
                    0.0
                };
                (self.peak - fallen).max(level)
            }
            None => level,
        }
    }

    /// The level at `now` as a fraction of the way up the meter: 0 at the floor, 1 at 0 dB,
    /// and 1 for anything louder.
    pub fn fraction(&self, now: Instant) -> f32 {
        self.db_fraction(self.level_db(now))
    }

    /// The held peak at `now` as a fraction of the way up the meter, by the same mapping as
    /// [`LevelMeter::fraction`].
    pub fn peak_fraction(&self, now: Instant) -> f32 {
        self.db_fraction(self.peak_db(now))
    }

    /// Where a level lies between the floor and 0 dB, kept between 0 and 1.
    fn db_fraction(&self, db: f32) -> f32 {
        ((db - self.floor) / -self.floor).clamp(0.0, 1.0)
    }

    /// Whether a reading at or above full scale has arrived since the clip was last reset.
    pub fn clipped(&self) -> bool {
        self.clipped
    }

    /// Clears the clip mark, and nothing else.
    pub fn reset_clip(&mut self) {
        self.clipped = false;
    }

    /// Puts the meter back to silence: the level and the peak at the floor, no clip mark, and
    /// no time carried over.
    pub fn reset(&mut self) {
        self.level = self.floor;
        self.at = None;
        self.peak = self.floor;
        self.peak_at = None;
        self.clipped = false;
    }

    /// Whether both the level and the held peak are at the floor at `now`: the meter is at
    /// rest and its window may sleep.
    pub fn at_rest(&self, now: Instant) -> bool {
        self.level_db(now) <= self.floor && self.peak_db(now) <= self.floor
    }

    /// When the caller should next wake to draw this meter at `now`, or `None` when it is at
    /// rest and nothing will change until the next reading. The wait is `interval`, but never
    /// less than a millisecond.
    pub fn next_wake(&self, now: Instant, interval: Duration) -> Option<Instant> {
        if self.at_rest(now) {
            None
        } else {
            Some(now + interval.max(LEAST_WAKE))
        }
    }
}

impl Default for LevelMeter {
    fn default() -> LevelMeter {
        LevelMeter::new()
    }
}

/// The part of `r` from the fraction `from` to the fraction `to` of the way along its axis,
/// counted from the left or from the bottom.
fn span(r: Rect, from: f32, to: f32, vertical: bool) -> Rect {
    if vertical {
        Rect::new(r.x, r.bottom() - r.h * to, r.w, r.h * (to - from))
    } else {
        Rect::new(r.x + r.w * from, r.y, r.w * (to - from), r.h)
    }
}

/// The colour of the zone a level falls in: the accent below -18 dB, the warning colour below
/// -6 dB, and the danger colour above that.
fn zone_colour(pal: &Palette, db: f32) -> crate::gfx::Rgb {
    if db > ZONE_DANGER_DB {
        pal.danger
    } else if db > ZONE_WARN_DB {
        pal.warn
    } else {
        pal.accent
    }
}

/// Draws a level meter filling `r`: a quiet track, the level from the left (or from the bottom
/// when `vertical`) in the accent up to [`ZONE_WARN_DB`], the warning colour up to
/// [`ZONE_DANGER_DB`], and the danger colour above it, the held peak as a 2 px line across the
/// meter in the colour of its own zone, and the last 3 px at the right (or top) end in the
/// danger colour when the clip mark is set.
///
/// The level and the peak are taken at `now`, so the caller draws with the same clock it
/// updates with. An empty rectangle, or one with a side that is not a number, draws nothing.
pub fn draw_meter(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    meter: &LevelMeter,
    now: Instant,
    vertical: bool,
) {
    if !(r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite())
        || r.w <= 0.0
        || r.h <= 0.0
    {
        return;
    }
    p.rbox(r, ROUND, mix(pal.line, pal.text, 0.18));

    let level = meter.fraction(now);
    let zones = [
        (0.0, meter.db_fraction(ZONE_WARN_DB), pal.accent),
        (
            meter.db_fraction(ZONE_WARN_DB),
            meter.db_fraction(ZONE_DANGER_DB),
            pal.warn,
        ),
        (meter.db_fraction(ZONE_DANGER_DB), 1.0, pal.danger),
    ];
    // One rounded bar for the whole level, drawn once per zone under that zone's clip: the
    // bar's ends are round and the joins between zones straight.
    let bar = span(r, 0.0, level, vertical);
    for (from, to, colour) in zones {
        let end = level.min(to);
        if end > from {
            let old = p.clip(span(r, from, end, vertical));
            p.rbox(bar, ROUND, colour);
            p.restore(old);
        }
    }

    let peak = meter.peak_fraction(now);
    if peak > 0.0 {
        let line = if vertical {
            let h = r.h.min(2.0);
            let y = (r.bottom() - r.h * peak - h / 2.0)
                .min(r.bottom() - h)
                .max(r.y);
            Rect::new(r.x, y, r.w, h)
        } else {
            let w = r.w.min(2.0);
            let x = (r.x + r.w * peak - w / 2.0).min(r.right() - w).max(r.x);
            Rect::new(x, r.y, w, r.h)
        };
        p.fill(line, zone_colour(pal, meter.peak_db(now)));
    }

    if meter.clipped() {
        let mark = if vertical {
            Rect::new(r.x, r.y, r.w, r.h.min(3.0))
        } else {
            let w = r.w.min(3.0);
            Rect::new(r.right() - w, r.y, w, r.h)
        };
        p.rbox(mark, ROUND, pal.danger);
    }
}

/// Draws two meters in `r`, splitting it across its axis with a 2 px gap: the two halves of an
/// upright meter side by side, or of a flat meter one above the other. `left` is the first half
/// (the left or the top) and `right` the second, and each is drawn as [`draw_meter`] draws one.
/// An empty rectangle, or one with a side that is not a number, draws nothing.
pub fn draw_stereo(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    left: &LevelMeter,
    right: &LevelMeter,
    now: Instant,
    vertical: bool,
) {
    if !(r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite())
        || r.w <= 0.0
        || r.h <= 0.0
    {
        return;
    }
    let (a, b) = if vertical {
        let w = ((r.w - STEREO_GAP) / 2.0).max(0.0);
        (
            Rect::new(r.x, r.y, w, r.h),
            Rect::new(r.x + w + STEREO_GAP, r.y, w, r.h),
        )
    } else {
        let h = ((r.h - STEREO_GAP) / 2.0).max(0.0);
        (
            Rect::new(r.x, r.y, r.w, h),
            Rect::new(r.x, r.y + h + STEREO_GAP, r.w, h),
        )
    };
    draw_meter(p, pal, a, left, now, vertical);
    draw_meter(p, pal, b, right, now, vertical);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Gfx;
    use crate::gfx::Rgb;
    use crate::slider::db_to_gain;
    use crate::theme::{DARK, LIGHT};
    use mdshared_text::Bitmap;

    /// The meter's rectangle lying flat, and the same standing up.
    const R: Rect = Rect::new(20.0, 40.0, 200.0, 12.0);
    const V: Rect = Rect::new(40.0, 20.0, 12.0, 200.0);

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-3 * b.abs().max(1.0)
    }

    fn at(bmp: &Bitmap, x: i32, y: i32) -> Rgb {
        let o = (y as usize * bmp.width as usize + x as usize) * 4;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    }

    fn shot(pal: &Palette, w: u32, h: u32, draw: impl FnOnce(&mut Painter<'_>)) -> Bitmap {
        let mut bmp = Bitmap::new(w, h);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, w as f32, h as f32), pal.pane);
            draw(&mut p);
        }
        bmp
    }

    fn track(pal: &Palette) -> Rgb {
        mix(pal.line, pal.text, 0.18)
    }

    #[test]
    fn a_subpixel_meter_draws_its_peak_across_the_whole_short_axis() {
        let now = Instant::now();
        let mut m = LevelMeter::new();
        m.update(now, 0.1);
        for (r, vertical) in [
            (Rect::new(0.1, 0.0, 0.3, 1.0), false),
            (Rect::new(0.0, 0.1, 1.0, 0.3), true),
        ] {
            let mut bmp = Bitmap::new(10, 10);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, 10.0);
            draw_meter(&mut p, &LIGHT, r, &m, now, vertical);
            let at_axis = |n| {
                if vertical {
                    at(&bmp, 5, n)
                } else {
                    at(&bmp, n, 5)
                }
            };
            assert_eq!(at_axis(1), LIGHT.accent);
            assert_eq!(at_axis(3), LIGHT.accent);
            assert_ne!(at_axis(4), LIGHT.accent);
        }
    }

    #[test]
    fn a_new_meter_is_at_the_floor_and_at_rest() {
        let t0 = Instant::now();
        let m = LevelMeter::new();
        assert_eq!(m.level_db(t0), -60.0);
        assert_eq!(m.peak_db(t0), -60.0);
        assert_eq!((m.fraction(t0), m.peak_fraction(t0)), (0.0, 0.0));
        assert!(m.at_rest(t0));
        assert_eq!(m.next_wake(t0, Duration::from_millis(16)), None);
        assert_eq!(m, LevelMeter::default());
    }

    #[test]
    fn a_reading_of_one_is_zero_db_and_a_half_is_about_minus_six() {
        let t0 = Instant::now();
        let mut full = LevelMeter::new();
        full.update(t0, 1.0);
        assert_eq!((full.level_db(t0), full.fraction(t0)), (0.0, 1.0));
        assert_eq!((full.peak_db(t0), full.peak_fraction(t0)), (0.0, 1.0));

        let mut half = LevelMeter::new();
        half.update(t0, 0.5);
        assert!(close(half.level_db(t0), -6.0206), "{}", half.level_db(t0));

        let mut silent = LevelMeter::new();
        silent.update(t0, 0.0);
        assert_eq!(silent.level_db(t0), -60.0);
        assert_eq!(silent.fraction(t0), 0.0);
        assert!(silent.at_rest(t0));
    }

    #[test]
    fn the_level_jumps_up_at_once_and_only_falls_by_its_release() {
        let t0 = Instant::now();
        let mut m = LevelMeter::new();
        m.update(t0, 0.25);
        assert!(close(m.level_db(t0), -12.0412), "{}", m.level_db(t0));
        // A quieter reading leaves the level on its way down, and does not re-arm it.
        m.update(t0 + Duration::from_millis(100), 0.1);
        assert!(close(m.level_db(t0 + Duration::from_millis(100)), -14.0412));
        assert!(close(m.level_db(t0 + Duration::from_millis(200)), -16.0412));
        // A louder one is there at once.
        m.update(t0 + Duration::from_millis(200), 0.8);
        assert!(close(m.level_db(t0 + Duration::from_millis(200)), -1.9382));
    }

    #[test]
    fn the_level_falls_twenty_db_a_second() {
        let t0 = Instant::now();
        let mut m = LevelMeter::new();
        m.update(t0, 1.0);
        assert_eq!(m.level_db(t0 + Duration::from_millis(500)), -10.0);
        assert_eq!(m.level_db(t0 + Duration::from_millis(1000)), -20.0);
        assert_eq!(m.level_db(t0 + Duration::from_millis(3000)), -60.0);
        assert_eq!(m.level_db(t0 + Duration::from_millis(9000)), -60.0);
    }

    #[test]
    fn the_peak_holds_for_a_second_and_a_half_then_falls_to_the_level() {
        let t0 = Instant::now();
        let mut m = LevelMeter::new();
        m.update(t0, 1.0);
        assert_eq!(m.peak_db(t0 + Duration::from_millis(1500)), 0.0);
        assert!(close(m.peak_db(t0 + Duration::from_millis(2000)), -10.0));
        assert!(close(m.peak_db(t0 + Duration::from_millis(4000)), -50.0));
        // It stops at the level rather than falling through the floor.
        let late = t0 + Duration::from_millis(6000);
        assert_eq!(m.peak_db(late), m.level_db(late));
        assert_eq!(m.peak_db(late), -60.0);
    }

    #[test]
    fn a_louder_reading_during_the_hold_replaces_the_peak() {
        let t0 = Instant::now();
        let mut m = LevelMeter::new();
        m.update(t0, 0.5);
        assert!(close(m.peak_db(t0 + Duration::from_millis(400)), -6.0206));
        m.update(t0 + Duration::from_millis(1000), 1.0);
        assert_eq!(m.peak_db(t0 + Duration::from_millis(2000)), 0.0);
        assert!(close(m.peak_db(t0 + Duration::from_millis(3000)), -10.0));
    }

    #[test]
    fn the_meter_comes_to_rest_and_asks_for_no_more_frames() {
        let t0 = Instant::now();
        let mut m = LevelMeter::new();
        m.update(t0, 1.0);
        let wait = Duration::from_millis(16);
        assert_eq!(m.next_wake(t0, wait), Some(t0 + wait));
        assert_eq!(m.next_wake(t0, Duration::ZERO), Some(t0 + LEAST_WAKE));
        assert!(!m.at_rest(t0 + Duration::from_millis(4000)));
        let late = t0 + Duration::from_millis(4600);
        assert!(m.at_rest(late));
        assert_eq!(m.next_wake(late, wait), None);
    }

    #[test]
    fn a_reading_at_full_scale_clips_until_the_clip_is_reset() {
        let t0 = Instant::now();
        let mut m = LevelMeter::new();
        assert!(!m.clipped());
        m.update(t0, 0.99);
        assert!(!m.clipped());
        m.update(t0, 1.0);
        assert!(m.clipped());
        m.reset_clip();
        assert!(!m.clipped());
        m.update(t0, 2.0);
        assert_eq!(m.level_db(t0), CEILING_DB, "a louder reading is cut to +6");
        assert!(m.clipped());
        m.reset();
        assert!(!m.clipped() && m.at_rest(t0) && m.level_db(t0) == -60.0);
    }

    #[test]
    fn a_reading_that_is_not_a_number_changes_nothing() {
        let t0 = Instant::now();
        let mut m = LevelMeter::new();
        m.update(t0, f32::NAN);
        assert!(m.at_rest(t0) && !m.clipped());
        m.update(t0, 1.0);
        m.update(t0 + Duration::from_millis(500), f32::NAN);
        assert_eq!(m.level_db(t0 + Duration::from_millis(500)), -10.0);
        // The hold was not begun again by the reading that never arrived.
        assert_eq!(m.peak_db(t0 + Duration::from_millis(1000)), 0.0);
        assert!(close(m.peak_db(t0 + Duration::from_millis(2000)), -10.0));
    }

    #[test]
    fn a_negative_reading_counts_by_its_magnitude() {
        let t0 = Instant::now();
        let mut down = LevelMeter::new();
        down.update(t0, -0.5);
        let mut up = LevelMeter::new();
        up.update(t0, 0.5);
        assert_eq!(down.level_db(t0), up.level_db(t0));
        assert_eq!(down.peak_db(t0), up.peak_db(t0));
        assert!(!down.clipped());
        down.update(t0, -1.0);
        assert!(down.clipped() && down.level_db(t0) == 0.0);
    }

    #[test]
    fn a_time_before_the_last_update_counts_as_no_time_passed() {
        let t0 = Instant::now();
        let mut m = LevelMeter::new();
        m.update(t0 + Duration::from_millis(1000), 1.0);
        assert_eq!(m.level_db(t0), 0.0);
        assert_eq!(m.peak_db(t0), 0.0);
        assert_eq!(m.level_db(t0 + Duration::from_millis(500)), 0.0);
        assert_eq!(m.level_db(t0 + Duration::from_millis(1500)), -10.0);
    }

    #[test]
    fn the_builders_keep_their_defaults_when_given_nonsense() {
        let t0 = Instant::now();
        let mut m = LevelMeter::new()
            .floor(f32::NAN)
            .release(f32::NAN)
            .fall(f32::NAN);
        m.update(t0, 1.0);
        assert_eq!(m.level_db(t0 + Duration::from_millis(4000)), -60.0);
        assert!(close(m.peak_db(t0 + Duration::from_millis(2000)), -10.0));
        for bad in [f32::INFINITY, f32::NEG_INFINITY, 0.0, 12.0] {
            let mut m = LevelMeter::new().floor(bad);
            m.update(t0, 1.0);
            assert_eq!(m.level_db(t0 + Duration::from_millis(4000)), -60.0);
        }
        for bad in [f32::INFINITY, 0.0, -20.0] {
            let mut m = LevelMeter::new().release(bad);
            m.update(t0, 1.0);
            assert_eq!(m.level_db(t0 + Duration::from_millis(500)), -10.0);
        }
        // Usable values are taken.
        let mut m = LevelMeter::new()
            .floor(-30.0)
            .release(60.0)
            .hold(Duration::ZERO)
            .fall(60.0);
        m.update(t0, 1.0);
        assert_eq!(m.level_db(t0 + Duration::from_millis(500)), -30.0);
        assert!(close(m.peak_db(t0 + Duration::from_millis(500)), -30.0));
    }

    #[test]
    fn the_fractions_stay_between_zero_and_one() {
        let t0 = Instant::now();
        let mut last = -1.0;
        for gain in [0.0, 0.0001, 0.01, 0.5, 1.0, 4.0, f32::INFINITY] {
            let mut m = LevelMeter::new();
            m.update(t0, gain);
            let (level, peak) = (m.fraction(t0), m.peak_fraction(t0));
            assert!((0.0..=1.0).contains(&level), "{gain}: {level}");
            assert!((0.0..=1.0).contains(&peak), "{gain}: {peak}");
            assert!(level >= last && peak >= last, "{gain} went down");
            assert!(m.fraction(t0 + Duration::from_millis(9000)) >= 0.0);
            last = level;
        }
        let mut m = LevelMeter::new().floor(-20.0);
        m.update(t0, 1.0);
        assert_eq!(m.fraction(t0), 1.0);
        assert!(close(m.fraction(t0 + Duration::from_millis(500)), 0.5));
    }

    #[test]
    fn the_decay_matches_a_plain_step_by_step_computation() {
        let t0 = Instant::now();
        let mut m = LevelMeter::new();
        m.update(t0, 1.0);
        let mut db = 0.0f32;
        let mut now = t0;
        for _ in 0..700 {
            now += Duration::from_millis(1);
            db = (db - 20.0 * 0.001).max(-60.0);
        }
        assert!(
            (m.level_db(now) - db).abs() < 0.05,
            "{} of {db}",
            m.level_db(now)
        );
        assert!(db < -13.9 && db > -14.1, "{db}");
    }

    #[test]
    fn a_meter_at_the_floor_draws_only_its_track() {
        let t0 = Instant::now();
        let m = LevelMeter::new();
        for pal in [&LIGHT, &DARK] {
            let bmp = shot(pal, 240, 100, |p| draw_meter(p, pal, R, &m, t0, false));
            let bare = shot(pal, 240, 100, |p| p.rbox(R, ROUND, track(pal)));
            assert!(bmp.pixels == bare.pixels, "more than the track was drawn");
            let tall = shot(pal, 100, 240, |p| draw_meter(p, pal, V, &m, t0, true));
            let bare = shot(pal, 100, 240, |p| p.rbox(V, ROUND, track(pal)));
            assert!(tall.pixels == bare.pixels, "more than the track was drawn");
        }
    }

    #[test]
    fn a_meter_at_full_scale_shows_its_three_zones_in_both_directions() {
        let t0 = Instant::now();
        let mut m = LevelMeter::new();
        m.update(t0, 1.0);
        for pal in [&LIGHT, &DARK] {
            let flat = shot(pal, 240, 100, |p| draw_meter(p, pal, R, &m, t0, false));
            assert_eq!(at(&flat, 60, 46), pal.accent, "20% up");
            assert_eq!(at(&flat, 180, 46), pal.warn, "80% up");
            assert_eq!(at(&flat, 210, 46), pal.danger, "95% up");
            let tall = shot(pal, 100, 240, |p| draw_meter(p, pal, V, &m, t0, true));
            assert_eq!(at(&tall, 46, 180), pal.accent, "20% up from the bottom");
            assert_eq!(at(&tall, 46, 60), pal.warn, "80% up from the bottom");
            assert_eq!(at(&tall, 46, 30), pal.danger, "95% up from the bottom");
            // A quiet meter shows the accent only at its foot.
            let mut low = LevelMeter::new();
            low.update(t0, db_to_gain(-50.0));
            let bmp = shot(pal, 240, 100, |p| draw_meter(p, pal, R, &low, t0, false));
            assert_eq!(at(&bmp, 22, 46), pal.accent);
            assert_eq!(at(&bmp, 120, 46), track(pal));
        }
    }

    #[test]
    fn the_held_peak_is_a_line_across_the_meter() {
        let t0 = Instant::now();
        let mut m = LevelMeter::new();
        m.update(t0, 1.0);
        let now = t0 + Duration::from_millis(3000);
        assert_eq!(m.level_db(now), -60.0);
        assert!(close(m.peak_db(now), -30.0));
        for pal in [&LIGHT, &DARK] {
            assert!(close(m.peak_fraction(now), 0.5));
            let flat = shot(pal, 240, 100, |p| draw_meter(p, pal, R, &m, now, false));
            assert_eq!(at(&flat, 119, 46), pal.accent, "the line's left pixel");
            assert_eq!(at(&flat, 120, 46), pal.accent);
            assert_eq!(at(&flat, 121, 46), track(pal), "two pixels only");
            assert_eq!(at(&flat, 119, 40), pal.accent, "across the whole track");
            assert_eq!(at(&flat, 119, 51), pal.accent);
            assert_eq!(at(&flat, 100, 46), track(pal), "the level is at the floor");
            let tall = shot(pal, 100, 240, |p| draw_meter(p, pal, V, &m, now, true));
            assert_eq!(at(&tall, 46, 120), pal.accent);
            assert_eq!(at(&tall, 46, 118), track(pal));
        }
        // A peak at the floor is no line at all.
        let m = LevelMeter::new();
        let bmp = shot(&LIGHT, 240, 100, |p| {
            draw_meter(p, &LIGHT, R, &m, t0, false)
        });
        assert_eq!(at(&bmp, 20, 46), track(&LIGHT));
    }

    #[test]
    fn a_clipped_meter_marks_its_top_end() {
        let t0 = Instant::now();
        let mut m = LevelMeter::new();
        m.update(t0, 1.0);
        let now = t0 + Duration::from_millis(2500);
        for pal in [&LIGHT, &DARK] {
            let flat = shot(pal, 240, 100, |p| draw_meter(p, pal, R, &m, now, false));
            assert_eq!(at(&flat, 217, 46), pal.danger);
            assert_eq!(at(&flat, 219, 46), pal.danger);
            assert_eq!(at(&flat, 214, 46), track(pal), "no wider than 3 px");
            let tall = shot(pal, 100, 240, |p| draw_meter(p, pal, V, &m, now, true));
            assert_eq!(at(&tall, 46, 21), pal.danger);
            assert_eq!(at(&tall, 46, 26), track(pal), "no taller than 3 px");
        }
        // Without the clip mark the end is the track again.
        let quiet = LevelMeter::new();
        let bmp = shot(&LIGHT, 240, 100, |p| {
            draw_meter(p, &LIGHT, R, &quiet, now, false)
        });
        assert_eq!(at(&bmp, 218, 46), track(&LIGHT));
    }

    #[test]
    fn a_stereo_pair_splits_the_rectangle_with_a_gap() {
        let t0 = Instant::now();
        let loud = LevelMeter::new();
        let mut loud = loud;
        loud.update(t0, 1.0);
        let quiet = LevelMeter::new();
        let tall = Rect::new(40.0, 20.0, 42.0, 200.0);
        let bmp = shot(&LIGHT, 120, 240, |p| {
            draw_stereo(p, &LIGHT, tall, &loud, &quiet, t0, true)
        });
        // Two 20 px meters, a 2 px gap, the first loud and the second empty.
        assert_eq!(at(&bmp, 45, 180), LIGHT.accent);
        assert_eq!(at(&bmp, 61, 180), LIGHT.pane, "the gap");
        assert_eq!(at(&bmp, 70, 180), track(&LIGHT), "the quiet one");
        assert_eq!(at(&bmp, 70, 215), track(&LIGHT));
    }

    #[test]
    fn an_empty_or_endless_rectangle_draws_nothing() {
        let t0 = Instant::now();
        let mut m = LevelMeter::new();
        m.update(t0, 1.0);
        for pal in [&LIGHT, &DARK] {
            let blank = shot(pal, 60, 60, |_| {});
            for r in [
                Rect::new(0.0, 0.0, 0.0, 30.0),
                Rect::new(0.0, 0.0, 30.0, -4.0),
                Rect::new(f32::NAN, 0.0, 30.0, 30.0),
                Rect::new(0.0, 0.0, f32::INFINITY, 30.0),
            ] {
                let drawn = shot(pal, 60, 60, |p| draw_meter(p, pal, r, &m, t0, false));
                assert!(drawn.pixels == blank.pixels, "{r:?} was drawn");
                let pair = shot(pal, 60, 60, |p| draw_stereo(p, pal, r, &m, &m, t0, true));
                assert!(pair.pixels == blank.pixels, "stereo {r:?} was drawn");
            }
        }
    }

    /// The sheet a person looks at: flat and upright meters at several levels, a stereo pair
    /// and a clipped one, light above and dark below.
    /// `cargo test -p mdshared-ui meter_sheet -- --ignored`, then open
    /// target/specimen/meter.png.
    #[test]
    #[ignore]
    fn meter_sheet() {
        let t0 = Instant::now();
        let now = t0 + Duration::from_millis(250);
        let mut bmp = Bitmap::new(640, 460);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            for (row, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
                let top = row as f32 * 230.0;
                p.fill(Rect::new(0.0, top, 640.0, 230.0), pal.pane);
                let levels = [-60.0, -40.0, ZONE_WARN_DB, ZONE_DANGER_DB, 0.0];
                for (i, db) in levels.into_iter().enumerate() {
                    let mut m = LevelMeter::new();
                    m.update(t0, db_to_gain(db));
                    draw_meter(
                        &mut p,
                        pal,
                        Rect::new(20.0, top + 20.0 + i as f32 * 34.0, 220.0, 14.0),
                        &m,
                        now,
                        false,
                    );
                    let mut tall = LevelMeter::new();
                    tall.update(t0, db_to_gain(db));
                    draw_meter(
                        &mut p,
                        pal,
                        Rect::new(280.0 + i as f32 * 34.0, top + 20.0, 14.0, 160.0),
                        &tall,
                        now,
                        true,
                    );
                }
                let mut clipped = LevelMeter::new();
                clipped.update(t0, 1.2);
                draw_meter(
                    &mut p,
                    pal,
                    Rect::new(20.0, top + 196.0, 220.0, 14.0),
                    &clipped,
                    now,
                    false,
                );
                let mut left = LevelMeter::new();
                left.update(t0, db_to_gain(-8.0));
                let mut right = LevelMeter::new();
                right.update(t0, db_to_gain(-26.0));
                draw_stereo(
                    &mut p,
                    pal,
                    Rect::new(470.0, top + 20.0, 60.0, 180.0),
                    &left,
                    &right,
                    now,
                    true,
                );
            }
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).expect("the specimen folder");
        bmp.write_png(&out.join("meter.png")).expect("the sheet");
    }
}
