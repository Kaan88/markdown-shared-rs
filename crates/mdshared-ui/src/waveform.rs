//! A track's waveform: the least and the most sample under each pixel, made once from the
//! samples and kept at halving resolutions, so a waveform of any width, zoomed in or out, is
//! read from the level nearest its pixels rather than from the samples.
//!
//! The player owns the audio. On a worker (a [`crate::thumbs`]-like job on
//! [`mdshared_app::pool::Pool`]) it decodes the track and gives the samples to a
//! [`PeakBuilder`] as they come; [`PeakBuilder::peaks`] gives what is there so far, so the
//! waveform can fill in while the track decodes, and [`PeakBuilder::finish`] gives the whole.
//! [`Peaks`] are small (four bytes a [`BUCKET`] of samples, twice that with every level) and
//! [`Peaks::to_bytes`] writes them for the player's own disk cache, keyed as it likes.
//!
//! Drawing ([`draw`]) asks [`Peaks::columns`] for one (least, most) pair a device pixel over the
//! part of the track in view, and draws the part before the playhead in the accent and the
//! rest faint. Silence is a hairline, not nothing.
//!
//! Deliberately not here: decoding audio (the player's, roadmap M6); RMS or loudness in the
//! waveform; one row a channel (a builder a channel, and a [`draw`] a row, does it); and a
//! zoom closer than a bucket a pixel, where the samples themselves are needed: there the
//! columns repeat their bucket.

use std::ops::Range;

use crate::gfx::{Painter, Rect};
use crate::theme::Palette;

/// The samples a bucket of the finest level holds by default: 256 is under 6 ms at 44.1 kHz,
/// finer than a pixel of any track longer than a few seconds drawn across a window.
pub const BUCKET: usize = 256;

/// What [`Peaks::to_bytes`] begins with: the format and its version.
const MAGIC: &[u8; 8] = b"MDPEAKS1";

/// The least and the most of two ranges.
fn join(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    (a.0.min(b.0), a.1.max(b.1))
}

/// Takes samples as they are decoded and makes [`Peaks`] of them. It is `Send`: it belongs on
/// the worker that decodes.
#[derive(Clone, Debug)]
pub struct PeakBuilder {
    bucket: usize,
    base: Vec<(f32, f32)>,
    /// The bucket being filled, and how many samples it has.
    current: (f32, f32),
    filled: usize,
    samples: u64,
}

impl PeakBuilder {
    /// A builder with buckets of `bucket` samples ([`BUCKET`] is the usual; 0 counts as 1).
    pub fn new(bucket: usize) -> PeakBuilder {
        PeakBuilder {
            bucket: bucket.max(1),
            base: Vec::new(),
            current: (f32::INFINITY, f32::NEG_INFINITY),
            filled: 0,
            samples: 0,
        }
    }

    /// A builder that expects about `samples` samples, so its storage is made once.
    pub fn with_capacity(bucket: usize, samples: u64) -> PeakBuilder {
        let mut b = PeakBuilder::new(bucket);
        let buckets = samples.div_ceil(b.bucket as u64);
        b.base
            .reserve(usize::try_from(buckets).unwrap_or(0).min(1 << 24));
        b
    }

    /// Takes mono samples, where ±1 is full scale. A louder sample counts as full scale; a
    /// sample that is not a number counts as silence.
    pub fn push(&mut self, samples: &[f32]) {
        for &s in samples {
            self.frame(s, s);
        }
    }

    /// Takes interleaved samples of `channels` channels (0 counts as 1): each frame counts
    /// once, with the least and the most of its channels, so the waveform is the outline of
    /// them all. A frame cut short at the end counts with what it has.
    pub fn push_interleaved(&mut self, samples: &[f32], channels: usize) {
        for frame in samples.chunks(channels.max(1)) {
            let (lo, hi) = frame
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &s| {
                    (lo.min(s), hi.max(s))
                });
            self.frame(lo, hi);
        }
    }

    fn frame(&mut self, lo: f32, hi: f32) {
        let clean = |s: f32| if s.is_nan() { 0.0 } else { s.clamp(-1.0, 1.0) };
        let (lo, hi) = (clean(lo), clean(hi));
        self.current = join(self.current, (lo.min(hi), hi.max(lo)));
        self.filled += 1;
        self.samples += 1;
        if self.filled == self.bucket {
            self.base.push(self.current);
            self.current = (f32::INFINITY, f32::NEG_INFINITY);
            self.filled = 0;
        }
    }

    /// How many samples (frames) have been taken.
    pub fn samples(&self) -> u64 {
        self.samples
    }

    /// The peaks of what has been taken so far, the bucket being filled included: for a
    /// waveform that fills in while the track decodes. It copies; call it a few times a second,
    /// not for every buffer.
    pub fn peaks(&self) -> Peaks {
        let mut base = self.base.clone();
        if self.filled > 0 {
            base.push(self.current);
        }
        Peaks::from_base(self.bucket, self.samples, base)
    }

    /// The peaks of everything taken.
    pub fn finish(mut self) -> Peaks {
        if self.filled > 0 {
            self.base.push(self.current);
        }
        Peaks::from_base(self.bucket, self.samples, self.base)
    }
}

/// The peaks of a track at every resolution from a bucket of samples up to the whole track.
#[derive(Clone, Debug, PartialEq)]
pub struct Peaks {
    bucket: usize,
    samples: u64,
    /// `levels[k][i]` is the (least, most) of the samples `i * bucket << k` up to the next.
    levels: Vec<Vec<(f32, f32)>>,
}

impl Peaks {
    fn from_base(bucket: usize, samples: u64, base: Vec<(f32, f32)>) -> Peaks {
        let mut levels = vec![base];
        while let Some(last) = levels.last().filter(|l| l.len() > 1) {
            let next = last
                .chunks(2)
                .map(|pair| pair.iter().copied().reduce(join).unwrap_or((0.0, 0.0)))
                .collect();
            levels.push(next);
        }
        Peaks {
            bucket,
            samples,
            levels,
        }
    }

    /// Peaks of no samples: a waveform that draws nothing.
    pub fn empty() -> Peaks {
        Peaks::from_base(BUCKET, 0, Vec::new())
    }

    /// How many samples (frames) the peaks cover.
    pub fn samples(&self) -> u64 {
        self.samples
    }

    /// The samples in a bucket of the finest level.
    pub fn bucket(&self) -> usize {
        self.bucket
    }

    /// How many resolutions are kept: 1 for a track of one bucket, one more each time the
    /// buckets double.
    pub fn levels(&self) -> usize {
        self.levels.len()
    }

    /// The (least, most) sample in each of `width` equal columns over the samples `view`,
    /// each between -1 and 1; `None` for a column past the samples there are (a track still
    /// decoding, or a view past its end). Each pair is read from the coarsest level whose
    /// buckets are no wider than a column, so it is never less than the column's samples and
    /// at most a bucket of that level more on either side. Empty for a width of 0; all `None`
    /// for an empty view.
    pub fn columns(&self, width: usize, view: Range<u64>) -> Vec<Option<(f32, f32)>> {
        if view.end <= view.start || self.samples == 0 {
            return vec![None; width];
        }
        let span = (view.end - view.start) as f64;
        let per_column = span / width.max(1) as f64;
        let mut k = 0;
        while k + 1 < self.levels.len() && ((self.bucket as u64) << (k + 1)) as f64 <= per_column {
            k += 1;
        }
        let size = (self.bucket as u64) << k;
        let level = &self.levels[k];
        (0..width)
            .map(|i| {
                let a = view.start + (span * i as f64 / width as f64) as u64;
                let b = view.start + (span * (i + 1) as f64 / width as f64) as u64;
                let b = b.max(a + 1).min(self.samples);
                if a >= self.samples {
                    return None;
                }
                let first = (a / size) as usize;
                let end = (b.div_ceil(size) as usize).clamp(first + 1, level.len());
                level.get(first..end)?.iter().copied().reduce(join)
            })
            .collect()
    }

    /// The peaks as bytes, for a disk cache: a header, then the finest level at 16 bits a
    /// number (the coarser levels are made again on reading). A track of three minutes at
    /// 44.1 kHz is about 120 KB.
    pub fn to_bytes(&self) -> Vec<u8> {
        let base = &self.levels[0];
        let mut out = Vec::with_capacity(28 + base.len() * 4);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(self.bucket as u64).to_le_bytes());
        out.extend_from_slice(&self.samples.to_le_bytes());
        out.extend_from_slice(&(base.len() as u32).to_le_bytes());
        let q = |v: f32| ((v.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes();
        for &(lo, hi) in base {
            out.extend_from_slice(&q(lo));
            out.extend_from_slice(&q(hi));
        }
        out
    }

    /// Peaks read back from [`to_bytes`](Self::to_bytes), within 1/32767 of what was written;
    /// `None` for bytes that are not that (another format, cut short, a count that does not
    /// fit the samples).
    pub fn from_bytes(bytes: &[u8]) -> Option<Peaks> {
        let rest = bytes.strip_prefix(MAGIC)?;
        let (bucket, rest) = rest.split_first_chunk::<8>()?;
        let (samples, rest) = rest.split_first_chunk::<8>()?;
        let (count, rest) = rest.split_first_chunk::<4>()?;
        let bucket = usize::try_from(u64::from_le_bytes(*bucket)).ok()?;
        let samples = u64::from_le_bytes(*samples);
        let count = u32::from_le_bytes(*count) as usize;
        if bucket == 0 || samples.div_ceil(bucket as u64) != count as u64 || rest.len() != count * 4
        {
            return None;
        }
        let v = |b: [u8; 2]| f32::from(i16::from_le_bytes(b)) / 32767.0;
        let base = rest
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| {
                let (lo, hi) = (v([c[0], c[1]]), v([c[2], c[3]]));
                (lo.min(hi), hi.max(lo))
            })
            .collect();
        Some(Peaks::from_base(bucket, samples, base))
    }
}

/// Draws the waveform of the samples `view` filling `r`, a column a device pixel, centred on
/// the middle of `r` with full scale at its edges. With a `playhead` (a sample), what lies
/// before it is in the accent and the rest faint, split at the playhead's own pixel; without
/// one it is all in the accent. Where the peaks are not yet there nothing is drawn. An empty
/// rectangle, or one with a side that is not a number, draws nothing.
pub fn draw(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    peaks: &Peaks,
    view: Range<u64>,
    playhead: Option<u64>,
) {
    if !(r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite())
        || r.w <= 0.0
        || r.h <= 0.0
        || view.end <= view.start
    {
        return;
    }
    let width = ((r.w * p.scale).round() as usize).max(1);
    // The height in device pixels, and a device pixel of it as a fraction: the least a
    // column is drawn, on a whole pixel row so that silence is a sharp hairline.
    let (tall, bottom) = (r.h * p.scale, r.bottom() * p.scale);
    let least = (1.0 / tall).min(1.0);
    let values: Vec<(f32, f32)> = peaks
        .columns(width, view.clone())
        .into_iter()
        .map(|c| match c {
            None => (0.0, 0.0),
            Some((lo, hi)) => {
                let (lo, hi) = (0.5 + lo * 0.5, 0.5 + hi * 0.5);
                if hi - lo >= least {
                    (lo, hi)
                } else {
                    let row = (bottom - (lo + hi) / 2.0 * tall).floor();
                    let top = ((bottom - row) / tall).clamp(least, 1.0);
                    (top - least, top)
                }
            }
        })
        .collect();
    let Some(head) = playhead else {
        p.columns(r, &values, 0.0, pal.accent, 1.0);
        return;
    };
    let span = (view.end - view.start) as f64;
    let at = (head.saturating_sub(view.start) as f64 / span).min(1.0) as f32;
    let split = r.x + r.w * at;
    let old = p.clip(Rect::new(r.x, r.y, split - r.x, r.h));
    p.columns(r, &values, 0.0, pal.accent, 1.0);
    p.restore(old);
    let old = p.clip(Rect::new(split, r.y, r.right() - split, r.h));
    p.columns(r, &values, 0.0, pal.faint, 1.0);
    p.restore(old);
}

#[cfg(test)]
#[path = "waveform_tests.rs"]
mod tests;
