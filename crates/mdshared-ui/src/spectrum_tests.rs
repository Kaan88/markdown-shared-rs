use std::path::Path;
use std::time::{Duration, Instant};

use super::*;
use crate::gfx::{Face, Gfx, Rgb};
use crate::slider::db_to_gain;
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

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

/// The three bands this file's pixel tests use: 1 kHz to 4 kHz at a bin every 1 kHz, so bin 1
/// is band 0, bin 2 band 1 and bin 3 band 2.
fn three_bands() -> Bands {
    Bands::new(3, 8000.0, 8).range(1000.0, 4000.0)
}

/// A spectrum of `three_bands` reading a full bar, a bar at `mid` and silence.
fn three_bars(mid: f32) -> Spectrum {
    let mut spectrum = Spectrum::new(three_bands());
    let bins = [0.0, 1.0, mid, 0.0, 0.0];
    spectrum.update(Instant::now(), &bins);
    spectrum
}

#[test]
fn the_band_edges_run_geometrically_from_the_range_to_nyquist() {
    let bands = Bands::new(10, 48000.0, 2048);
    let edges = bands.edges();
    assert_eq!(edges.len(), 11);
    assert_eq!(bands.count(), 10);
    assert_eq!(edges[0], 20.0);
    assert_eq!(edges[10], 20000.0);
    // Every step is the same ratio, and every edge is the low edge times a power of it: the
    // band widths grow with frequency.
    let ratio = edges[1] / edges[0];
    for (k, pair) in edges.windows(2).enumerate() {
        assert!(close(pair[1] / pair[0], ratio), "step {k}");
        let want = 20.0 * 1000.0f32.powf((k + 1) as f32 / 10.0);
        assert!(close(pair[1], want), "edge {} is {}", k + 1, pair[1]);
    }
    // The higher the band, the wider it is.
    assert!(edges[2] - edges[1] > edges[1] - edges[0]);
}

#[test]
fn a_range_past_nyquist_is_cut_to_it_and_a_broken_rate_or_size_keeps_the_defaults() {
    let cut = Bands::new(4, 32000.0, 1024);
    assert_eq!(cut.high_hz(), 16000.0);
    assert_eq!(cut.sample_rate(), 32000.0);
    assert_eq!(cut.fft_size(), 1024);
    for (rate, fft) in [
        (0.0, 0usize),
        (f32::NAN, 2048),
        (-8000.0, 2048),
        (f32::INFINITY, 2048),
    ] {
        let bands = Bands::new(4, rate, fft);
        assert_eq!(bands.sample_rate(), 48000.0, "rate {rate}");
        assert_eq!(bands.fft_size(), 2048, "rate {rate}");
        assert_eq!(bands.low_hz(), 20.0);
        assert_eq!(bands.high_hz(), 20000.0);
    }
    // A sample rate whose Nyquist frequency is below the default low edge still makes a usable
    // range, closed below its top.
    let narrow = Bands::new(4, 20.0, 1024);
    assert!(narrow.low_hz() > 0.0 && narrow.low_hz() < narrow.high_hz());
    assert_eq!(narrow.high_hz(), 10.0);
    // An FFT size too large to be one is replaced, not multiplied out.
    assert_eq!(Bands::new(4, 44100.0, usize::MAX).fft_size(), 2048);
    assert_eq!(Bands::new(usize::MAX, 44100.0, 2048).count(), 4096);
}

#[test]
fn the_range_builder_keeps_the_old_range_when_given_nonsense() {
    let bands = Bands::new(4, 44100.0, 2048);
    for (low, high) in [
        (0.0, 100.0),
        (1000.0, 1000.0),
        (5000.0, 100.0),
        (f32::NAN, 20000.0),
        (20.0, f32::INFINITY),
    ] {
        assert_eq!(bands.clone().range(low, high), bands, "{low} to {high}");
    }
    let moved = bands.range(100.0, 5000.0);
    assert_eq!(moved.low_hz(), 100.0);
    assert_eq!(moved.high_hz(), 5000.0);
    assert_eq!(moved.edges().len(), 5);
}

#[test]
fn a_count_of_zero_is_no_bars_at_all() {
    let bands = Bands::new(0, 44100.0, 2048);
    assert_eq!(bands.count(), 0);
    assert_eq!(bands.edges(), &[20.0]);
    assert!(bands.reduce(&[0.5; 100]).is_empty());

    let t0 = Instant::now();
    let mut spectrum = Spectrum::new(bands);
    assert_eq!(spectrum.len(), 0);
    assert!(spectrum.is_empty());
    spectrum.update(t0, &[1.0; 64]);
    assert!(spectrum.bars(t0).is_empty() && spectrum.peaks(t0).is_empty());
    assert!(spectrum.at_rest(t0));
    assert_eq!(spectrum.next_wake(t0, Duration::from_millis(16)), None);
    spectrum.reset();
}

#[test]
fn every_bin_lands_in_one_band_and_none_outside_the_range() {
    let bands = Bands::new(64, 44100.0, 2048);
    let bin_hz = (bands.sample_rate() / bands.fft_size() as f32) as f64;
    let edges = bands.edges();
    let mut seen = 0;
    for (k, band) in bands.plan.iter().enumerate() {
        // No gaps and no overlap: each band begins where the one before it ends.
        if k > 0 {
            assert_eq!(bands.plan[k - 1].end, band.first, "band {k}");
        }
        assert!(band.end >= band.first);
        for i in band.first..band.end {
            let hz = i as f64 * bin_hz;
            assert!(
                hz >= edges[k] as f64 - bin_hz * 1e-3,
                "bin {i} at {hz} is below band {k}'s {}",
                edges[k]
            );
            assert!(
                hz < edges[k + 1] as f64 + bin_hz * 1e-3,
                "bin {i} at {hz} is above band {k}'s {}",
                edges[k + 1]
            );
            seen += 1;
        }
    }
    // Outside the range: below the first edge and at or above the last.
    let first = bands.plan[0].first;
    for i in 0..first {
        assert!((i as f64 * bin_hz) < edges[0] as f64 + bin_hz * 1e-3);
    }
    let end = bands.plan[bands.plan.len() - 1].end;
    for i in end..1025 {
        assert!((i as f64 * bin_hz) >= edges[edges.len() - 1] as f64 - bin_hz * 1e-3);
    }
    assert_eq!(seen, end - first);
}

#[test]
fn a_band_takes_the_loudest_bin_in_it() {
    let bands = Bands::new(1, 8000.0, 8).range(1000.0, 4000.0);
    assert_eq!(bands.edges(), &[1000.0, 4000.0]);
    // Bins at 0, 1, 2, 3 and 4 kHz: the band is 1 kHz up to (not including) 4 kHz.
    let bins = [0.0, 0.2, 0.7, 0.1, 0.9];
    assert_eq!(bands.reduce(&bins), vec![0.7]);
    // A bin at the high edge exactly belongs to no band.
    let bins = [0.0, 0.2, 0.3, 0.1, 0.9];
    assert_eq!(bands.reduce(&bins), vec![0.3]);
}

#[test]
fn a_band_with_no_bin_of_its_own_reads_between_the_two_nearest() {
    // 512 bands over 20 Hz to 20 kHz: the first bands are narrower than the 21.5 Hz between
    // the bins, so they hold none and must interpolate.
    let bands = Bands::new(512, 44100.0, 2048);
    let bin_hz = 44100.0 / 2048.0f64;
    let bins: Vec<f32> = (0..1025).map(|i| 0.1 + 0.8 * i as f32 / 1024.0).collect();
    let values = bands.reduce(&bins);
    assert_eq!(values.len(), 512);
    assert!(
        bands.plan[0].first == bands.plan[0].end,
        "the first band was expected to hold no bin"
    );
    assert!(
        bands.plan.iter().filter(|b| b.first == b.end).count() > 0,
        "no band was narrow enough to interpolate"
    );
    // The same answer, worked out plainly from the band's centre frequency.
    let ratio = (bands.high_hz() as f64 / bands.low_hz() as f64).powf(1.0 / 512.0);
    let centre = (bands.low_hz() as f64 * bands.low_hz() as f64 * ratio).sqrt();
    let at = centre / bin_hz;
    let a = at.floor() as usize;
    let t = at - a as f64;
    let v = |i: usize| 0.1 + 0.8 * i as f64 / 1024.0;
    let want = v(a) + (v(a + 1) - v(a)) * t;
    assert!(
        (values[0] as f64 - want).abs() < 1e-5,
        "{} of {want}",
        values[0]
    );
    // Nothing along a smooth ramp is empty.
    assert!(values.iter().all(|v| *v > 0.0));
}

#[test]
fn broken_and_short_bins_read_as_silence_without_a_panic() {
    let bands = Bands::new(8, 44100.0, 2048);
    assert_eq!(bands.reduce(&[]), vec![0.0; 8]);
    assert_eq!(bands.reduce(&vec![f32::NAN; 100]), vec![0.0; 8]);
    let bad = [f32::NEG_INFINITY, f32::INFINITY, f32::NAN, -2.0];
    let values = bands.reduce(&bad);
    assert_eq!(values.len(), 8);
    assert!(values.iter().all(|v| v.is_finite() && *v >= 0.0));
    // A single bin is read for every band that has no bin of its own.
    assert_eq!(bands.reduce(&[0.5]), vec![0.5; 8]);
    // A list cut short is used as far as it goes and still gives one value a bar.
    let short = bands.reduce(&[0.1, 0.2, 0.3]);
    assert_eq!(short.len(), 8);
    assert!(short.iter().all(|v| v.is_finite() && *v >= 0.0));
    // Bins above full scale are kept for the meter to cut to 0 dB.
    let loud = Bands::new(1, 8000.0, 8).range(1000.0, 4000.0);
    assert_eq!(loud.reduce(&[0.0, 3.0, 0.0, 0.0, 0.0]), vec![3.0]);
}

#[test]
fn a_full_scale_bin_puts_its_bar_at_the_top_and_the_rest_at_the_floor() {
    let t0 = Instant::now();
    let mut spectrum = Spectrum::new(Bands::new(8, 8000.0, 16));
    assert_eq!(spectrum.len(), 8);
    let mut bins = vec![0.0f32; 9];
    bins[2] = 1.0;
    spectrum.update(t0, &bins);
    let bars = spectrum.bars(t0);
    assert_eq!(bars.iter().filter(|b| **b == 1.0).count(), 1);
    assert_eq!(bars.iter().filter(|b| **b == 0.0).count(), 7);
    assert_eq!(spectrum.peaks(t0), bars);
    // By the time the peak has fallen through the floor, every bar is empty again.
    assert!(
        spectrum
            .bars(t0 + Duration::from_millis(5000))
            .iter()
            .all(|b| *b == 0.0)
    );
}

#[test]
fn the_bars_fall_by_the_release_and_the_spectrum_comes_to_rest() {
    let t0 = Instant::now();
    let mut spectrum = Spectrum::new(Bands::new(4, 8000.0, 8));
    assert!(spectrum.at_rest(t0));
    assert_eq!(
        spectrum.next_wake(t0, Duration::from_millis(16)),
        None,
        "a spectrum before its first reading is at rest"
    );
    spectrum.update(t0, &[1.0; 5]);
    assert!(spectrum.bars(t0).iter().all(|b| *b == 1.0));
    // 30 dB a second from 0 dB, over a 72 dB scale.
    let now = t0 + Duration::from_millis(500);
    assert!(spectrum.bars(now).iter().all(|b| close(*b, 57.0 / 72.0)));
    assert_eq!(
        spectrum.next_wake(now, Duration::from_millis(16)),
        Some(now + Duration::from_millis(16))
    );
    // The level reaches the floor at 2.4 s, but the held peak is still on its way down.
    let floor_only = t0 + Duration::from_millis(2400);
    assert!(spectrum.bars(floor_only).iter().all(|b| *b == 0.0));
    assert!(
        !spectrum.at_rest(floor_only),
        "the held peak is above the floor"
    );
    assert!(
        spectrum
            .next_wake(floor_only, Duration::from_millis(16))
            .is_some()
    );
    // The peak reaches it at 0.6 s of hold plus 3.6 s of fall.
    let rest = t0 + Duration::from_millis(4300);
    assert!(spectrum.at_rest(rest));
    assert_eq!(spectrum.next_wake(rest, Duration::from_millis(16)), None);
}

#[test]
fn the_held_peaks_stay_then_fall_after_the_hold() {
    let t0 = Instant::now();
    let mut spectrum = Spectrum::new(Bands::new(4, 8000.0, 8));
    spectrum.update(t0, &[1.0; 5]);
    assert!(spectrum.peaks(t0).iter().all(|p| close(*p, 1.0)));
    // Held at 0 dB for the 0.6 s, then 20 dB a second.
    assert!(
        spectrum
            .peaks(t0 + Duration::from_millis(600))
            .iter()
            .all(|p| close(*p, 1.0))
    );
    assert!(
        spectrum
            .peaks(t0 + Duration::from_millis(1600))
            .iter()
            .all(|p| close(*p, 52.0 / 72.0))
    );
    assert!(
        spectrum
            .peaks(t0 + Duration::from_millis(2400))
            .iter()
            .all(|p| close(*p, 0.5))
    );
    // A bar never rises above its peak.
    for now in [
        t0 + Duration::from_millis(700),
        t0 + Duration::from_millis(1600),
    ] {
        for (bar, peak) in spectrum.bars(now).iter().zip(spectrum.peaks(now)) {
            assert!(*bar <= peak + 1e-6);
        }
    }
}

#[test]
fn the_builders_keep_their_defaults_when_given_nonsense() {
    let bands = Bands::new(4, 8000.0, 8);
    let base = Spectrum::new(bands.clone());
    assert_eq!(
        Spectrum::new(bands.clone())
            .floor(f32::NAN)
            .release(0.0)
            .fall(-3.0),
        base
    );
    for bad in [0.0, 12.0, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(Spectrum::new(bands.clone()).floor(bad), base, "floor {bad}");
    }
    for bad in [0.0, -20.0, f32::INFINITY] {
        assert_eq!(
            Spectrum::new(bands.clone()).release(bad),
            base,
            "release {bad}"
        );
        assert_eq!(Spectrum::new(bands.clone()).fall(bad), base, "fall {bad}");
    }
    // Usable values are taken, and start every bar again.
    let t0 = Instant::now();
    let mut custom = Spectrum::new(bands)
        .floor(-20.0)
        .release(60.0)
        .hold(Duration::ZERO)
        .fall(60.0);
    custom.update(t0, &[1.0; 5]);
    assert!(close(custom.bars(t0)[0], 1.0));
    assert!(close(custom.bars(t0 + Duration::from_millis(100))[0], 0.7));
    assert!(custom.at_rest(t0 + Duration::from_millis(400)));
}

#[test]
fn a_reset_empties_every_bar() {
    let t0 = Instant::now();
    let mut spectrum = Spectrum::new(Bands::new(4, 8000.0, 8));
    spectrum.update(t0, &[1.0; 5]);
    assert!(!spectrum.at_rest(t0));
    spectrum.reset();
    assert!(spectrum.bars(t0).iter().all(|b| *b == 0.0));
    assert!(spectrum.peaks(t0).iter().all(|p| *p == 0.0));
    assert!(spectrum.at_rest(t0));
    // A reading after the reset starts from the floor, not from where the bars were.
    spectrum.update(t0, &[0.0; 5]);
    assert!(spectrum.at_rest(t0));
}

#[test]
fn a_bar_is_the_accent_at_its_height_and_the_page_above() {
    let t0 = Instant::now();
    let spectrum = three_bars(db_to_gain(-12.0));
    let r = Rect::new(10.0, 10.0, 300.0, 200.0);
    for pal in [&LIGHT, &DARK] {
        let bmp = shot(pal, 320, 220, |p| {
            draw(
                p,
                pal,
                r,
                &spectrum,
                t0,
                Look {
                    peaks: false,
                    ..Look::default()
                },
            )
        });
        // The first bar is at full scale: accent from the bottom to the top.
        assert_eq!(at(&bmp, 60, 60), pal.accent);
        assert_eq!(at(&bmp, 60, 11), pal.accent);
        // The second, at -12 dB, stands 83% of the way up: accent inside it, the page above.
        assert_eq!(at(&bmp, 150, 110), pal.accent, "half way up the bar");
        assert_eq!(at(&bmp, 150, 50), pal.accent, "inside its top");
        assert_eq!(at(&bmp, 150, 20), pal.pane, "above it");
        // The third is silence: nothing.
        assert_eq!(at(&bmp, 260, 110), pal.pane);
        assert_eq!(at(&bmp, 260, 205), pal.pane);
        // The 2 px gap at the right of the first bar is page.
        assert_eq!(at(&bmp, 109, 60), pal.pane);
    }
}

#[test]
fn the_held_peak_cap_is_drawn_above_the_falling_bar() {
    let t0 = Instant::now();
    let bands = Bands::new(1, 8000.0, 8).range(1000.0, 4000.0);
    let mut spectrum = Spectrum::new(bands);
    spectrum.update(t0, &[0.0, 0.0, 1.0, 0.0, 0.0]);
    let now = t0 + Duration::from_millis(1000);
    // The bar has fallen to -30 dB (0.583 up) and the peak to -8 dB (0.889 up).
    assert!(close(spectrum.bars(now)[0], 42.0 / 72.0));
    assert!(close(spectrum.peaks(now)[0], 64.0 / 72.0));
    let r = Rect::new(10.0, 10.0, 100.0, 200.0);
    for pal in [&LIGHT, &DARK] {
        let bmp = shot(pal, 120, 220, |p| {
            draw(p, pal, r, &spectrum, now, Look::default())
        });
        assert_eq!(at(&bmp, 60, 33), mix(pal.accent, pal.text, 0.35), "the cap");
        assert_eq!(at(&bmp, 60, 20), pal.pane, "above the cap");
        assert_eq!(at(&bmp, 60, 60), pal.pane, "between the cap and the bar");
        assert_eq!(at(&bmp, 60, 120), pal.accent, "the bar below");
        assert_eq!(at(&bmp, 60, 205), pal.accent, "the bar at its foot");
        // Without the cap there is nothing at the peak's height.
        let bare = shot(pal, 120, 220, |p| {
            draw(
                p,
                pal,
                r,
                &spectrum,
                now,
                Look {
                    peaks: false,
                    ..Look::default()
                },
            )
        });
        assert_eq!(at(&bare, 60, 33), pal.pane);
        assert_eq!(at(&bare, 60, 120), pal.accent);
    }
}

#[test]
fn zones_colour_a_bar_by_its_height() {
    let t0 = Instant::now();
    let spectrum = three_bars(db_to_gain(-12.0));
    let r = Rect::new(10.0, 10.0, 300.0, 200.0);
    for pal in [&LIGHT, &DARK] {
        let zoned = shot(pal, 320, 220, |p| {
            draw(
                p,
                pal,
                r,
                &spectrum,
                t0,
                Look {
                    zones: true,
                    ..Look::default()
                },
            )
        });
        // The full bar crosses all three zones: accent below -18 dB, warning to -6 dB,
        // danger above it.
        assert_eq!(at(&zoned, 60, 150), pal.accent, "below the warning");
        assert_eq!(at(&zoned, 60, 50), pal.warn, "between the two edges");
        assert_eq!(at(&zoned, 60, 20), pal.danger, "above the danger edge");
        // The -12 dB bar stays below the warning edge.
        assert_eq!(at(&zoned, 150, 110), pal.accent);
        // Without zones the whole bar is the accent.
        let plain = shot(pal, 320, 220, |p| {
            draw(p, pal, r, &spectrum, t0, Look::default())
        });
        assert_eq!(at(&plain, 60, 50), pal.accent);
        assert_eq!(at(&plain, 60, 20), pal.accent);
    }
}

#[test]
fn frequency_labels_read_in_kilohertz() {
    assert_eq!(frequency_label(20.0), "20");
    assert_eq!(frequency_label(500.0), "500");
    assert_eq!(frequency_label(999.0), "999");
    assert_eq!(frequency_label(1000.0), "1k");
    assert_eq!(frequency_label(1500.0), "1.5k");
    assert_eq!(frequency_label(2500.0), "2.5k");
    assert_eq!(frequency_label(12345.0), "12.3k");
    assert_eq!(frequency_label(20000.0), "20k");
    assert_eq!(frequency_label(999.6), "1k", "a rounding up to a thousand");
    for bad in [0.0, -20.0, f32::NAN, f32::INFINITY] {
        assert_eq!(frequency_label(bad), "0", "{bad}");
    }
}

#[test]
fn x_of_spaces_the_range_logarithmically_across_the_rectangle() {
    let bands = Bands::new(4, 44100.0, 2048);
    let r = Rect::new(10.0, 10.0, 200.0, 50.0);
    assert_eq!(bands.x_of(bands.low_hz(), r), 10.0);
    assert_eq!(bands.x_of(bands.high_hz(), r), 210.0);
    // The geometric middle of the range is the middle of the rectangle.
    let middle = (bands.low_hz() * bands.high_hz()).sqrt();
    assert!(close(bands.x_of(middle, r), 110.0));
    // Each edge of the bands lands on its own share of the width.
    for (k, edge) in bands.edges().iter().enumerate() {
        assert!(close(bands.x_of(*edge, r), 10.0 + 200.0 * k as f32 / 4.0));
    }
    // Outside the range is the nearer end; nonsense is the left edge.
    assert_eq!(bands.x_of(5.0, r), 10.0);
    assert_eq!(bands.x_of(30000.0, r), 210.0);
    assert_eq!(bands.x_of(f32::NAN, r), 10.0);
    assert_eq!(bands.x_of(1000.0, Rect::new(10.0, 10.0, 0.0, 50.0)), 10.0);
}

#[test]
fn an_empty_or_endless_rectangle_draws_nothing() {
    let t0 = Instant::now();
    let spectrum = three_bars(0.5);
    let empty = Spectrum::new(Bands::new(0, 8000.0, 8));
    for pal in [&LIGHT, &DARK] {
        let blank = shot(pal, 60, 60, |_| {});
        for r in [
            Rect::new(0.0, 0.0, 0.0, 30.0),
            Rect::new(0.0, 0.0, 30.0, -4.0),
            Rect::new(f32::NAN, 0.0, 30.0, 30.0),
            Rect::new(0.0, 0.0, f32::INFINITY, 30.0),
        ] {
            let drawn = shot(pal, 60, 60, |p| {
                draw(p, pal, r, &spectrum, t0, Look::default())
            });
            assert!(drawn.pixels == blank.pixels, "{r:?} was drawn");
        }
        let none = shot(pal, 60, 60, |p| {
            draw(
                p,
                pal,
                Rect::new(0.0, 0.0, 30.0, 30.0),
                &empty,
                t0,
                Look {
                    zones: true,
                    ..Look::default()
                },
            )
        });
        assert!(none.pixels == blank.pixels, "no bars drew something");
    }
}

/// The bins of a pink-noise slope with a few peaks in it, the same on every run: a spectrum a
/// person can judge at a glance.
fn pink_bins(fft_size: usize, sample_rate: f32) -> Vec<f32> {
    let peaks = [
        (90.0, 0.7),
        (440.0, 0.6),
        (1250.0, 0.45),
        (3500.0, 0.4),
        (9000.0, 0.35),
    ];
    (0..fft_size / 2 + 1)
        .map(|i| {
            let f = (i as f32 * sample_rate / fft_size as f32).max(30.0);
            let mut mag = 0.4 * (120.0 / f).powf(0.45);
            mag *= 1.0 + 0.2 * (i as f32 * 1.7).sin();
            for &(centre, amp) in &peaks {
                let x = (f / centre).ln() / 0.09;
                mag += amp * (-x * x).exp();
            }
            mag.min(1.0)
        })
        .collect()
}

fn written(scale: f32, file: &str) {
    let t0 = Instant::now();
    let now = t0 + Duration::from_millis(150);
    let bins = pink_bins(2048, 44100.0);
    let mut bmp = Bitmap::new((760.0 * scale) as u32, (560.0 * scale) as u32);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (row, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
            let top = row as f32 * 280.0;
            p.fill(Rect::new(0.0, top, 760.0, 280.0), pal.pane);
            p.text(
                Face::Ui,
                12.0,
                16.0,
                top + 14.0,
                if row == 0 { "light" } else { "dark" },
                pal.text,
                200.0,
            );
            let panels = [
                (32, false, "32 bars"),
                (32, true, "32 bars, zones"),
                (64, false, "64 bars"),
                (64, true, "64 bars, zones"),
            ];
            for (i, (count, zones, caption)) in panels.into_iter().enumerate() {
                let x = 16.0 + (i % 2) as f32 * 372.0;
                let y = top + 38.0 + (i / 2) as f32 * 118.0;
                let r = Rect::new(x, y, 336.0, 92.0);
                let mut spectrum = Spectrum::new(Bands::new(count, 44100.0, 2048));
                spectrum.update(t0, &bins);
                p.text(Face::Ui, 10.0, x, y - 7.0, caption, pal.muted, 200.0);
                draw(
                    &mut p,
                    pal,
                    r,
                    &spectrum,
                    now,
                    Look {
                        zones,
                        ..Look::default()
                    },
                );
                if i / 2 == 1 {
                    for hz in [20.0, 100.0, 1000.0, 10000.0, 20000.0] {
                        let lx = spectrum.bands().x_of(hz, r);
                        p.text(
                            Face::Ui,
                            9.0,
                            lx,
                            y + 102.0,
                            &frequency_label(hz),
                            pal.muted,
                            40.0,
                        );
                    }
                }
            }
        }
    }
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
    std::fs::create_dir_all(&out).expect("the specimen folder");
    bmp.write_png(&out.join(file)).expect("the sheet");
}

/// The sheet a person looks at: 32 and 64 bars of a pink-noise slope with a few peaks, with and
/// without zones, the bars falling under peak caps, light above and dark below.
/// `cargo test --release -p mdshared-ui spectrum_sheet -- --ignored`, then open
/// target/specimen/spectrum.png and spectrum-150.png.
#[test]
#[ignore]
fn spectrum_sheet() {
    written(1.0, "spectrum.png");
    written(1.5, "spectrum-150.png");
}
