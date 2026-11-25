//! A spectrum analyser: the caller's frequency bins gathered into bars on a logarithmic
//! frequency axis, each bar a [`LevelMeter`] that jumps to its value and falls back with the
//! ballistics of a peak meter, and the drawing of those bars.
//!
//! The caller owns the FFT. It gives [`Bands::reduce`] or [`Spectrum::update`] the magnitudes of
//! linear frequency bins: bin `i` is at `i * sample_rate / fft_size`, a magnitude of 1 is full
//! scale (0 dB) and 0 is silence. [`Bands`] maps them to `count` bars from `low_hz` to `high_hz`,
//! geometric in frequency: each bar is the loudest bin whose centre falls in its band, or, for a
//! band too narrow to hold a bin at all (the bottom of the axis, usually), the value read by
//! linear interpolation between the two nearest bins.
//!
//! [`Spectrum`] holds one [`LevelMeter`] a bar, so bars rise on the frame a reading arrives, fall
//! at a steady rate afterwards, and hold a peak cap for a while. The caller updates it with the
//! clock's [`Instant`] each time audio arrives, draws it with the same `now`, and asks
//! [`Spectrum::next_wake`] when to wake for the fall: once every bar is at rest the window may
//! sleep. No clock is read here: every function that needs time is given it.
//!
//! Drawing ([`draw`]) is separate and changes nothing: bars from the bottom in the accent (or in
//! the meter's three zone colours), and the held peaks as thin caps. An axis is the caller's:
//! [`Bands::x_of`] places a frequency and [`frequency_label`] writes it.
//!
//! Deliberately not here: the FFT, windowing, averaging across frames (a [`LevelMeter`] a bar
//! already does the fall), more than one channel, and a drawn frequency axis with its ticks and
//! labels (the caller draws those from [`Bands::edges`], [`Bands::x_of`] and
//! [`frequency_label`]).

use std::time::{Duration, Instant};

use crate::gfx::{Painter, Rect, mix};
use crate::meter::{LevelMeter, ZONE_DANGER_DB, ZONE_WARN_DB};
use crate::theme::Palette;

/// The bottom of the axis when no other range is given, in hertz.
pub const DEFAULT_LOW_HZ: f32 = 20.0;
/// The top of the axis when no other range is given, in hertz. It is cut to the Nyquist
/// frequency, which is usually below it.
pub const DEFAULT_HIGH_HZ: f32 = 20000.0;

/// The sample rate used when the one given is zero or not a number.
const DEFAULT_RATE: f32 = 48000.0;
/// The smallest sample rate taken seriously: below it the axis and its bins are nonsense.
const MIN_RATE: f32 = 1.0;
/// The FFT size used when the one given is zero or absurd.
const DEFAULT_FFT: usize = 2048;
/// The largest FFT size taken seriously.
const MAX_FFT: usize = 1 << 24;
/// The most bars there can be. A window cannot show more, and a caller that asks for more has
/// made a mistake that would otherwise cost gigabytes.
const MAX_BANDS: usize = 4096;

/// The floor of every bar's [`LevelMeter`]: the level at which a bar is empty.
const DEFAULT_FLOOR_DB: f32 = -72.0;
/// How fast a bar falls after its reading, in decibels a second.
const DEFAULT_RELEASE: f32 = 30.0;
/// How long a bar's held peak stays before it starts to fall.
const DEFAULT_HOLD: Duration = Duration::from_millis(600);
/// How fast a bar's held peak falls after the hold, in decibels a second.
const DEFAULT_FALL: f32 = 20.0;

/// One band of a [`Bands`]: the bins that fall in it, and where to read when it holds none.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Band {
    /// The first bin index whose centre frequency is in the band.
    first: usize,
    /// One past the last such bin.
    end: usize,
    /// The band's centre frequency, in bins: the index to read between when the band is empty.
    centre_bin: f32,
}

/// The mapping of linear frequency bins to `count` bars on a logarithmic axis.
///
/// The bars are geometric in frequency between the low and the high edge: band `k` runs from
/// `low * (high / low)^(k / count)` to the next edge. The high edge is cut to the Nyquist
/// frequency of the sample rate, and a range that is not a usable pair of numbers is replaced by
/// the default 20 Hz to 20 kHz. A band is the loudest bin whose centre frequency falls in it;
/// a band narrower than the spacing of the bins reads the value between its two nearest bins.
#[derive(Clone, Debug, PartialEq)]
pub struct Bands {
    /// `count + 1` edge frequencies, in hertz, low to high.
    edges: Vec<f32>,
    /// One entry a bar, in the same order.
    plan: Vec<Band>,
    /// The frequency spacing of the bins, in hertz.
    bin_hz: f32,
    sample_rate: f32,
    fft_size: usize,
}

impl Bands {
    /// Bands of `count` bars over the default 20 Hz to 20 kHz for an FFT of `fft_size` bins at
    /// `sample_rate` hertz, cut to the Nyquist frequency.
    ///
    /// A sample rate that is zero, negative, endless or not a number counts as 48 kHz; an FFT
    /// size of zero or more than 16 777 216 counts as 2048; a count of more than 4096 counts as
    /// 4096. A count of zero is no bars: [`reduce`](Self::reduce) gives an empty list and
    /// [`edges`](Self::edges) the low edge alone.
    pub fn new(count: usize, sample_rate: f32, fft_size: usize) -> Bands {
        Bands::build(
            count,
            sample_rate,
            fft_size,
            DEFAULT_LOW_HZ,
            DEFAULT_HIGH_HZ,
        )
    }

    /// The same bands with another range, in hertz, low and high. A pair that is not a usable
    /// range (either not a number, the low not above zero, the low not below the high) is
    /// ignored and the range the bands already have kept.
    pub fn range(mut self, low_hz: f32, high_hz: f32) -> Bands {
        if low_hz.is_finite() && high_hz.is_finite() && low_hz > 0.0 && low_hz < high_hz {
            self = Bands::build(
                self.count(),
                self.sample_rate,
                self.fft_size,
                low_hz,
                high_hz,
            );
        }
        self
    }

    fn build(count: usize, sample_rate: f32, fft_size: usize, low_hz: f32, high_hz: f32) -> Bands {
        let count = count.min(MAX_BANDS);
        let sample_rate = if sample_rate.is_finite() && sample_rate > MIN_RATE {
            sample_rate
        } else {
            DEFAULT_RATE
        };
        let fft_size = if fft_size == 0 || fft_size > MAX_FFT {
            DEFAULT_FFT
        } else {
            fft_size
        };
        let nyquist = sample_rate / 2.0;
        let high = high_hz.min(nyquist);
        // The octave below the top when the range begins above the top; a range is always
        // usable from here on, so the logarithm below has something to divide by.
        let low = if low_hz.is_finite() && low_hz > 0.0 && low_hz < high {
            low_hz
        } else {
            high / 2.0
        };

        let mut edges = Vec::with_capacity(count + 1);
        edges.push(low);
        if count > 0 {
            let ratio = (high as f64 / low as f64).powf(1.0 / count as f64);
            for k in 1..count {
                edges.push((low as f64 * ratio.powi(k as i32)) as f32);
            }
            edges.push(high);
        }

        let bin_hz = sample_rate / fft_size as f32;
        let plan = edges
            .windows(2)
            .map(|pair| {
                let (a, b) = (pair[0] as f64, pair[1] as f64);
                Band {
                    first: (a / bin_hz as f64).ceil() as usize,
                    end: (b / bin_hz as f64).ceil() as usize,
                    centre_bin: ((a * b).sqrt() / bin_hz as f64) as f32,
                }
            })
            .collect();

        Bands {
            edges,
            plan,
            bin_hz,
            sample_rate,
            fft_size,
        }
    }

    /// How many bars the bands make.
    pub fn count(&self) -> usize {
        self.plan.len()
    }

    /// The `count + 1` band edges, in hertz, low to high: band `k` runs from `edges()[k]` up to
    /// `edges()[k + 1]`. For no bars it is the low edge alone.
    pub fn edges(&self) -> &[f32] {
        &self.edges
    }

    /// The bottom of the axis, in hertz.
    pub fn low_hz(&self) -> f32 {
        self.edges[0]
    }

    /// The top of the axis, in hertz: the high edge given, cut to the Nyquist frequency.
    pub fn high_hz(&self) -> f32 {
        self.edges[self.edges.len() - 1]
    }

    /// The sample rate the bands were built for, in hertz.
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// The FFT size the bands were built for, in bins.
    pub fn fft_size(&self) -> usize {
        self.fft_size
    }

    /// One value a bar from the magnitudes of the linear frequency bins, where bin `i` is at
    /// `i * sample_rate / fft_size` and 1 is full scale.
    ///
    /// A bar is the loudest bin in its band. A band that holds no bin takes the value at its
    /// centre frequency, read by linear interpolation between the two nearest bins, so the
    /// narrow low bars are not empty. A magnitude that is not a finite number, or is negative,
    /// counts as zero; a list shorter than the FFT name suggests is used as far as it goes, and
    /// bands past its end interpolate from its last bin. An empty list is silence everywhere.
    pub fn reduce(&self, bins: &[f32]) -> Vec<f32> {
        self.plan
            .iter()
            .map(|band| {
                let first = band.first.min(bins.len());
                let end = band.end.min(bins.len());
                if first < end {
                    bins[first..end]
                        .iter()
                        .fold(0.0f32, |loudest, &v| loudest.max(clean(v)))
                } else {
                    self.read_between(bins, band)
                }
            })
            .collect()
    }

    /// The value at a band's centre frequency, read between the two bins around it; the last
    /// bin repeats past the end, and no bins at all is silence.
    fn read_between(&self, bins: &[f32], band: &Band) -> f32 {
        if bins.is_empty() {
            return 0.0;
        }
        let at = band.centre_bin.max(0.0);
        let a = (at.floor() as usize).min(bins.len() - 1);
        let b = (a + 1).min(bins.len() - 1);
        let t = (at - a as f32).clamp(0.0, 1.0);
        clean(bins[a]) + (clean(bins[b]) - clean(bins[a])) * t
    }

    /// Where `hz` lies across `r` on the logarithmic axis: `r.x` at the low edge, `r.right()` at
    /// the high edge. A frequency outside the range is cut to the nearer edge. A frequency that
    /// is not a number, or a rectangle that is not a usable one, gives `r.x`.
    pub fn x_of(&self, hz: f32, r: Rect) -> f32 {
        if !(hz.is_finite() && r.x.is_finite() && r.w.is_finite() && r.w > 0.0) {
            return r.x;
        }
        let (low, high) = (self.low_hz(), self.high_hz());
        if !(low > 0.0 && high > low) {
            return r.x;
        }
        let t = ((hz.clamp(low, high) / low).ln() / (high / low).ln()).clamp(0.0, 1.0);
        r.x + r.w * t
    }
}

/// A magnitude as a bar's value: zero for silence, a negative number or anything that is not a
/// finite number.
fn clean(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// A spectrum analyser: one [`LevelMeter`] a bar, fed from frequency bins.
///
/// The defaults are a floor of -72 dB, a release of 30 dB a second and a hold of 0.6 s, with the
/// held peak falling at 20 dB a second after the hold (the [`LevelMeter`] default). Every builder
/// method keeps what it has when its argument is not a usable number.
#[derive(Clone, Debug, PartialEq)]
pub struct Spectrum {
    bands: Bands,
    /// One meter a bar, in the same order as [`Bands::edges`].
    meters: Vec<LevelMeter>,
    floor: f32,
    release: f32,
    hold: Duration,
    fall: f32,
}

impl Spectrum {
    /// A spectrum of `bands`, every bar at its floor.
    pub fn new(bands: Bands) -> Spectrum {
        let mut spectrum = Spectrum {
            bands,
            meters: Vec::new(),
            floor: DEFAULT_FLOOR_DB,
            release: DEFAULT_RELEASE,
            hold: DEFAULT_HOLD,
            fall: DEFAULT_FALL,
        };
        spectrum.rebuild();
        spectrum
    }

    /// The bands the bars are made from, for a caller that draws the axis.
    pub fn bands(&self) -> &Bands {
        &self.bands
    }

    /// How many bars there are.
    pub fn len(&self) -> usize {
        self.meters.len()
    }

    /// Whether there are no bars at all.
    pub fn is_empty(&self) -> bool {
        self.meters.is_empty()
    }

    /// The same with another floor: the level at which a bar is empty. Zero or above, or a
    /// number that is not one, is ignored and -72 dB kept.
    pub fn floor(mut self, db: f32) -> Spectrum {
        if db.is_finite() && db < 0.0 {
            self.floor = db;
            self.rebuild();
        }
        self
    }

    /// The same with another release: how many decibels a bar falls each second. Zero, a
    /// negative rate or one that is not a number is ignored and 30 kept.
    pub fn release(mut self, db_per_sec: f32) -> Spectrum {
        if db_per_sec.is_finite() && db_per_sec > 0.0 {
            self.release = db_per_sec;
            self.rebuild();
        }
        self
    }

    /// The same with another hold: how long a bar's peak stays before it starts to fall. A hold
    /// of no time is allowed.
    pub fn hold(mut self, hold: Duration) -> Spectrum {
        self.hold = hold;
        self.rebuild();
        self
    }

    /// The same with another fall of the held peaks, in decibels a second after the hold. Zero,
    /// a negative rate or one that is not a number is ignored and 20 kept.
    pub fn fall(mut self, db_per_sec: f32) -> Spectrum {
        if db_per_sec.is_finite() && db_per_sec > 0.0 {
            self.fall = db_per_sec;
            self.rebuild();
        }
        self
    }

    /// A meter with the settings this spectrum holds.
    fn meter(&self) -> LevelMeter {
        LevelMeter::new()
            .floor(self.floor)
            .release(self.release)
            .hold(self.hold)
            .fall(self.fall)
    }

    /// Every bar's meter made again, empty: the settings changed.
    fn rebuild(&mut self) {
        self.meters = (0..self.bands.count()).map(|_| self.meter()).collect();
    }

    /// Takes a reading at `now`: `bins` are the magnitudes of the linear frequency bins, as
    /// [`Bands::reduce`] takes them. Each bar jumps to its value at once or keeps falling, and
    /// holds its peak as its [`LevelMeter`] does.
    pub fn update(&mut self, now: Instant, bins: &[f32]) {
        let values = self.bands.reduce(bins);
        for (meter, value) in self.meters.iter_mut().zip(values) {
            meter.update(now, value);
        }
    }

    /// The bars at `now` as fractions from 0 (the floor) to 1 (0 dB and louder), one a bar, in
    /// the order of [`Bands::edges`].
    pub fn bars(&self, now: Instant) -> Vec<f32> {
        self.meters.iter().map(|m| m.fraction(now)).collect()
    }

    /// The held peaks at `now`, by the same mapping as [`bars`](Self::bars): each is at or above
    /// its bar.
    pub fn peaks(&self, now: Instant) -> Vec<f32> {
        self.meters.iter().map(|m| m.peak_fraction(now)).collect()
    }

    /// Puts every bar back to silence: no level, no peak, no clip mark.
    pub fn reset(&mut self) {
        for meter in &mut self.meters {
            meter.reset();
        }
    }

    /// Whether every bar is at its floor at `now`: nothing is moving and the window may sleep.
    /// A spectrum of no bars is at rest.
    pub fn at_rest(&self, now: Instant) -> bool {
        self.meters.iter().all(|m| m.at_rest(now))
    }

    /// When the caller should next wake to draw the fall at `now`, or `None` when every bar is
    /// at rest. The wait is `interval`, but never less than a millisecond (the least a
    /// [`LevelMeter`] waits).
    pub fn next_wake(&self, now: Instant, interval: Duration) -> Option<Instant> {
        self.meters
            .iter()
            .filter_map(|m| m.next_wake(now, interval))
            .min()
    }

    /// Where a level in decibels lies between the floor and 0 dB, kept between 0 and 1: the
    /// fractions of the zone edges, for the drawing.
    fn db_fraction(&self, db: f32) -> f32 {
        ((db - self.floor) / -self.floor).clamp(0.0, 1.0)
    }
}

/// How [`draw`] shows a spectrum: the space between bars, whether the held peaks are capped,
/// and whether the bars are coloured by height in the meter's three zones.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// The logical px left free at the right of each bar.
    pub gap: f32,
    /// Whether the held peaks are drawn as a thin cap above each bar.
    pub peaks: bool,
    /// Whether the bars take the accent, warning and danger colours by height rather than the
    /// accent throughout.
    pub zones: bool,
}

impl Default for Look {
    fn default() -> Look {
        Look {
            gap: 2.0,
            peaks: true,
            zones: false,
        }
    }
}

/// Draws a spectrum filling `r`, its bars standing from the bottom.
///
/// The bars are [`Painter::columns`] in the accent, or, with `look.zones`, in the accent below
/// [`ZONE_WARN_DB`], the warning colour up to [`ZONE_DANGER_DB`] and the danger colour above,
/// each zone drawn under its own clip so the joins between them are straight. With
/// `look.peaks`, each held peak is a 2 device px cap at its height in a lighter colour. Levels
/// are taken at `now`, the same clock the caller updates with. An empty rectangle, one with a
/// side that is not a number, or a spectrum of no bars draws nothing.
pub fn draw(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    spectrum: &Spectrum,
    now: Instant,
    look: Look,
) {
    if !(r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite())
        || r.w <= 0.0
        || r.h <= 0.0
    {
        return;
    }
    let bars = spectrum.bars(now);
    let columns: Vec<(f32, f32)> = bars.iter().map(|&bar| (0.0, bar)).collect();
    if look.zones {
        let warn = spectrum.db_fraction(ZONE_WARN_DB);
        let danger = spectrum.db_fraction(ZONE_DANGER_DB);
        for (from, to, colour) in [
            (0.0, warn, pal.accent),
            (warn, danger, pal.warn),
            (danger, 1.0, pal.danger),
        ] {
            if to > from {
                let band = Rect::new(r.x, r.bottom() - r.h * to, r.w, r.h * (to - from));
                let old = p.clip(band);
                p.columns(r, &columns, look.gap, colour, 1.0);
                p.restore(old);
            }
        }
    } else {
        p.columns(r, &columns, look.gap, pal.accent, 1.0);
    }

    if look.peaks {
        // A cap two device pixels tall, as a fraction of the height, its top at the peak.
        let cap = (2.0 * p.px() / r.h).min(1.0);
        let caps: Vec<(f32, f32)> = spectrum
            .peaks(now)
            .into_iter()
            .map(|peak| {
                if peak > 0.0 {
                    ((peak - cap).max(0.0), peak)
                } else {
                    (0.0, 0.0)
                }
            })
            .collect();
        p.columns(r, &caps, look.gap, mix(pal.accent, pal.text, 0.35), 1.0);
    }
}

/// A frequency as an axis label: whole hertz below a kilohertz ("20", "500"), and one decimal
/// of a kilohertz above with a trailing zero left off ("1k", "2.5k", "20k"). Zero, a negative
/// frequency, or one that is not a number reads "0".
pub fn frequency_label(hz: f32) -> String {
    if !hz.is_finite() || hz <= 0.0 {
        return "0".to_string();
    }
    let hz = hz.round();
    if hz < 1000.0 {
        return format!("{hz:.0}");
    }
    let text = format!("{:.1}", hz / 1000.0);
    let text = text.strip_suffix(".0").unwrap_or(&text);
    format!("{text}k")
}

#[cfg(test)]
#[path = "spectrum_tests.rs"]
mod tests;
