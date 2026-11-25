use super::*;
use crate::gfx::{Gfx, Rgb};
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

/// A pseudo-random signal between -1 and 1, the same on every run.
fn noise(n: usize, seed: u32) -> Vec<f32> {
    let mut x = seed.max(1);
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x as f32 / u32::MAX as f32) * 2.0 - 1.0
        })
        .collect()
}

/// The least and the most of `samples[range]`.
fn truth(samples: &[f32], range: Range<usize>) -> (f32, f32) {
    samples[range]
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &s| {
            (lo.min(s), hi.max(s))
        })
}

fn at(bmp: &Bitmap, x: i32, y: i32) -> Rgb {
    let o = (y as usize * bmp.width as usize + x as usize) * 4;
    [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
}

/// Draws into a fresh bitmap of the page colour at scale 1.
fn drawn(w: u32, h: u32, f: impl FnOnce(&mut Painter)) -> Bitmap {
    let mut bmp = Bitmap::new(w, h);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    p.fill(Rect::new(0.0, 0.0, w as f32, h as f32), LIGHT.page);
    f(&mut p);
    bmp
}

#[test]
fn each_bucket_holds_the_least_and_the_most_of_its_samples() {
    let mut b = PeakBuilder::new(4);
    b.push(&[0.1, -0.5, 0.3, 0.0, 0.9, 0.2]);
    assert_eq!(b.samples(), 6);
    let peaks = b.peaks();
    assert_eq!(
        peaks.columns(2, 0..8),
        vec![Some((-0.5, 0.3)), Some((0.2, 0.9))]
    );
    // The bucket being filled goes on filling.
    b.push(&[-0.7, 0.0]);
    let done = b.finish();
    assert_eq!(done.samples(), 8);
    assert_eq!(
        done.columns(2, 0..8),
        vec![Some((-0.5, 0.3)), Some((-0.7, 0.9))]
    );
}

#[test]
fn interleaved_channels_count_as_one_frame_with_the_outline_of_both() {
    let mut b = PeakBuilder::new(2);
    b.push_interleaved(&[0.5, -0.25, 0.1, 0.2, -0.9, 0.0], 2);
    let p = b.finish();
    assert_eq!(p.samples(), 3);
    assert_eq!(
        p.columns(2, 0..4),
        vec![Some((-0.25, 0.5)), Some((-0.9, 0.0))]
    );
}

#[test]
fn loud_and_broken_samples_are_cut_to_full_scale_and_silence() {
    let mut b = PeakBuilder::new(8);
    b.push(&[3.0, f32::NAN, -f32::INFINITY]);
    assert_eq!(b.finish().columns(1, 0..3), vec![Some((-1.0, 1.0))]);
    let mut b = PeakBuilder::new(8);
    b.push(&[f32::NAN, f32::NAN]);
    assert_eq!(b.finish().columns(1, 0..2), vec![Some((0.0, 0.0))]);
}

#[test]
fn every_level_halves_the_one_below_until_one_is_left() {
    let mut b = PeakBuilder::new(1);
    b.push(&noise(1000, 7));
    let p = b.finish();
    // 1000, 500, 250, 125, 63, 32, 16, 8, 4, 2, 1.
    assert_eq!(p.levels(), 11);
    assert_eq!(Peaks::empty().levels(), 1);
    assert_eq!(Peaks::empty().columns(3, 0..10), vec![None, None, None]);
}

#[test]
fn a_column_holds_its_samples_and_at_most_a_bucket_more_on_each_side() {
    let samples = noise(100_000, 3);
    let mut b = PeakBuilder::new(BUCKET);
    for chunk in samples.chunks(4096) {
        b.push(chunk);
    }
    let p = b.finish();
    for (width, view) in [
        (800, 0..100_000u64),
        (37, 5_000..60_000),
        (3000, 10..90_000),
    ] {
        let cols = p.columns(width, view.clone());
        assert_eq!(cols.len(), width);
        let span = (view.end - view.start) as f64;
        let per = span / width as f64;
        // The level the columns were read from has buckets no wider than a column.
        let size = (1..p.levels())
            .map(|k| (BUCKET as u64) << k)
            .filter(|&s| s as f64 <= per)
            .max()
            .unwrap_or(BUCKET as u64) as usize;
        for (i, c) in cols.iter().enumerate() {
            let (lo, hi) = c.unwrap();
            let a = (view.start as f64 + span * i as f64 / width as f64) as usize;
            let b =
                ((view.start as f64 + span * (i + 1) as f64 / width as f64) as usize).max(a + 1);
            let inner = truth(&samples, a..b);
            assert!(
                lo <= inner.0 && hi >= inner.1,
                "column {i} misses its samples"
            );
            let outer = truth(
                &samples,
                (a / size * size)..(b.div_ceil(size) * size).min(samples.len()),
            );
            assert!(lo >= outer.0 && hi <= outer.1, "column {i} reaches too far");
        }
    }
}

#[test]
fn columns_past_the_decoded_samples_are_none() {
    let mut b = PeakBuilder::new(10);
    b.push(&[0.5; 25]);
    let p = b.peaks();
    let cols = p.columns(10, 0..100);
    assert!(cols[..3].iter().all(Option::is_some));
    assert!(cols[3..].iter().all(Option::is_none));
    assert!(p.columns(0, 0..100).is_empty());
    assert_eq!(p.columns(2, 50..50), vec![None, None]);
}

#[test]
fn zoomed_closer_than_a_bucket_the_columns_repeat_it() {
    let mut b = PeakBuilder::new(100);
    b.push(&[0.25; 100]);
    b.push(&[-0.75; 100]);
    let p = b.finish();
    let cols = p.columns(20, 90..110);
    assert_eq!(cols[0], Some((0.25, 0.25)));
    assert_eq!(cols[19], Some((-0.75, -0.75)));
}

#[test]
fn peaks_written_as_bytes_read_back_within_a_step() {
    let mut b = PeakBuilder::new(64);
    b.push(&noise(10_000, 11));
    let p = b.finish();
    let bytes = p.to_bytes();
    assert_eq!(bytes.len(), 28 + 157 * 4);
    let back = Peaks::from_bytes(&bytes).unwrap();
    assert_eq!(
        (back.samples(), back.bucket(), back.levels()),
        (10_000, 64, p.levels())
    );
    for (a, b) in p
        .columns(500, 0..10_000)
        .iter()
        .zip(back.columns(500, 0..10_000))
    {
        let ((a0, a1), (b0, b1)) = (a.unwrap(), b.unwrap());
        assert!((a0 - b0).abs() <= 1.0 / 32767.0 && (a1 - b1).abs() <= 1.0 / 32767.0);
    }
    assert_eq!(
        Peaks::from_bytes(&Peaks::empty().to_bytes()),
        Some(Peaks::empty())
    );
}

#[test]
fn bytes_that_are_not_peaks_read_as_none() {
    let mut b = PeakBuilder::new(4);
    b.push(&[0.5; 10]);
    let good = b.finish().to_bytes();
    assert!(Peaks::from_bytes(&good).is_some());
    assert!(Peaks::from_bytes(&good[..good.len() - 1]).is_none());
    assert!(Peaks::from_bytes(&[good.as_slice(), &[0]].concat()).is_none());
    assert!(Peaks::from_bytes(b"MDPEAKS2").is_none());
    assert!(Peaks::from_bytes(&[]).is_none());
    let mut wrong_count = good.clone();
    wrong_count[24] = 9;
    assert!(Peaks::from_bytes(&wrong_count).is_none());
    let mut no_bucket = good.clone();
    no_bucket[8..16].fill(0);
    assert!(Peaks::from_bytes(&no_bucket).is_none());
}

#[test]
fn the_part_before_the_playhead_is_in_the_accent_and_the_rest_faint() {
    // Two samples a column: from the middle up to 0.8, then from the middle down.
    let mut b = PeakBuilder::new(1);
    b.push(&[0.0, 0.8].repeat(100));
    b.push(&[0.0, -0.8].repeat(100));
    let p = b.finish();
    let r = Rect::new(0.0, 0.0, 200.0, 40.0);
    let bmp = drawn(200, 40, |pa| draw(pa, &LIGHT, r, &p, 0..400, Some(100)));
    // Above the middle in the first half, below it in the second.
    assert_eq!(at(&bmp, 10, 8), LIGHT.accent);
    assert_eq!(at(&bmp, 49, 8), LIGHT.accent);
    assert_eq!(at(&bmp, 50, 8), LIGHT.faint);
    assert_eq!(at(&bmp, 150, 32), LIGHT.faint);
    assert_eq!(at(&bmp, 150, 8), LIGHT.page);
    assert_eq!(at(&bmp, 10, 32), LIGHT.page);
    let all = drawn(200, 40, |pa| draw(pa, &LIGHT, r, &p, 0..400, None));
    assert_eq!(at(&all, 150, 32), LIGHT.accent);
}

#[test]
fn silence_is_a_hairline_and_what_is_not_decoded_is_nothing() {
    let mut b = PeakBuilder::new(1);
    b.push(&[0.0; 100]);
    let p = b.peaks();
    let r = Rect::new(0.0, 0.0, 200.0, 40.0);
    let bmp = drawn(200, 40, |pa| draw(pa, &LIGHT, r, &p, 0..200, None));
    let column: Vec<Rgb> = (0..40).map(|y| at(&bmp, 20, y)).collect();
    assert_eq!(column.iter().filter(|&&c| c != LIGHT.page).count(), 1);
    assert_eq!(column[20], LIGHT.accent);
    assert!((0..40).all(|y| at(&bmp, 150, y) == LIGHT.page));
}

#[test]
fn an_empty_or_broken_rectangle_or_view_draws_nothing() {
    let mut b = PeakBuilder::new(1);
    b.push(&noise(100, 5));
    let p = b.finish();
    let blank = drawn(50, 50, |_| {});
    for (r, view) in [
        (Rect::new(0.0, 0.0, 0.0, 40.0), 0..100),
        (Rect::new(0.0, 0.0, 40.0, f32::NAN), 0..100),
        (Rect::new(f32::INFINITY, 0.0, 40.0, 40.0), 0..100),
        (Rect::new(0.0, 0.0, 40.0, 40.0), 60..60),
    ] {
        let bmp = drawn(50, 50, |pa| draw(pa, &DARK, r, &p, view, Some(30)));
        assert_eq!(bmp.pixels, blank.pixels);
    }
}

/// A sheet for a person to judge: `cargo test -p mdshared-ui waveform_sheet -- --ignored`
/// writes `target/specimen/waveform.png` and `waveform-150.png` (light above, dark below): a
/// whole track played to a third, the same zoomed in, a track still decoding, and silence.
#[test]
#[ignore]
fn waveform_sheet() {
    // A tone that swells and fades, with a beat, over noise.
    let rate = 8000.0;
    let samples: Vec<f32> = noise(240_000, 9)
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let t = i as f32 / rate;
            let swell = (t / 30.0 * std::f32::consts::PI).sin().abs();
            let beat = if (t * 2.0).fract() < 0.1 { 1.0 } else { 0.4 };
            (swell * beat * ((t * 220.0 * std::f32::consts::TAU).sin() * 0.7 + n * 0.3))
                .clamp(-1.0, 1.0)
        })
        .collect();
    let mut b = PeakBuilder::new(BUCKET);
    b.push(&samples[..90_000]);
    let partial = b.peaks();
    b.push(&samples[90_000..]);
    let whole = b.finish();
    let silent = {
        let mut s = PeakBuilder::new(BUCKET);
        s.push(&[0.0; 50_000]);
        s.finish()
    };
    for (scale, file) in [(1.0, "waveform.png"), (1.5, "waveform-150.png")] {
        let mut bmp = Bitmap::new((520.0 * scale) as u32, (360.0 * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (half, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
            let oy = half as f32 * 180.0;
            p.fill(Rect::new(0.0, oy, 520.0, 180.0), pal.pane);
            let o = p.offset(0.0, oy);
            let row = |k: usize, h: f32| Rect::new(20.0, 12.0 + 44.0 * k as f32, 480.0, h);
            draw(&mut p, pal, row(0, 36.0), &whole, 0..240_000, Some(80_000));
            draw(
                &mut p,
                pal,
                row(1, 36.0),
                &whole,
                70_000..90_000,
                Some(80_000),
            );
            draw(&mut p, pal, row(2, 36.0), &partial, 0..240_000, None);
            draw(&mut p, pal, row(3, 20.0), &silent, 0..50_000, Some(10_000));
            p.restore_offset(o);
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join(file)).unwrap();
    }
}
