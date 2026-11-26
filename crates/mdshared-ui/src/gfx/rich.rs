//! Rich text through the painter: a parley layout the caller shaped, each run in the brush the
//! caller gave that run, and the wavy underline a spelling checker draws under a word.
//!
//! The caller owns the layout and the fonts in it; this only draws. Glyphs go through the
//! painter's own `TextEngine`, so they share the glyph cache, the subpixel masks and the
//! antialiasing of every other text draw, and a run of another family, size, weight or colour is
//! drawn exactly as parley shaped it. Coordinates are logical px and honour the clip, the offset
//! and the scale, as the boxes and the text in `gfx` do.
//!
//! Deliberately not here: shaping, wrapping and alignment (parley does those; the `parley` this
//! crate draws with is re-exported by [`richtext`](crate::richtext)), hit testing a point to a
//! byte, the selection, and the colour of a squiggle (the caller asks for one).

use super::{Brush, Painter, Path, Rect, Rgb};
use parley::Layout;

/// The path of a spelling squiggle: one wave across the whole of `band`, `thickness` thick, that
/// begins and ends on the band's middle line.
///
/// There is a whole number of half waves, so the wave meets the middle line at both ends however
/// wide the band is; their lengths are equal, and one is `wavelength / 2` where that is a length.
/// The wave's peaks are half the band's height less half the stroke from the middle line, so the
/// stroke fills the band from edge to edge. A wavelength that is not a number, is zero or is
/// negative gives one wave of the band's own width; a band with no width or height gives an
/// empty path.
fn squiggle(band: Rect, thickness: f32, wavelength: f32) -> Path {
    let amp = ((band.h - thickness) / 2.0).max(0.0);
    let mid = band.y + band.h / 2.0;
    let half = if wavelength.is_finite() && wavelength > 0.0 {
        wavelength / 2.0
    } else {
        band.w
    };
    // At most a thousand half waves: a wavelength far below a pixel would put millions of them
    // in a wide band, and nothing on a screen would show the difference.
    let turns = (band.w / half).round().clamp(1.0, 1000.0);
    let step = band.w / turns;
    let mut path = Path::new().move_to(band.x, mid);
    for i in 0..turns as usize {
        let x = band.x + step * i as f32;
        // A quadratic from the middle line up (or down) and back: its top is exactly `amp`
        // above (or below) the middle line, since the control point is twice as far out.
        let peak = if i % 2 == 0 {
            mid - 2.0 * amp
        } else {
            mid + 2.0 * amp
        };
        path = path.quad_to(x + step / 2.0, peak, x + step, mid);
    }
    path
}

impl Painter<'_> {
    /// Draws a caller's parley layout with the layout's own brushes: red, green, blue and a
    /// baseline shift per run, one font, size and weight per run, as the caller shaped it.
    ///
    /// The layout is drawn in its own units, which are the device pixels it was shaped at: a
    /// caller that shaped at `size * scale` passes the logical px it wants, as every draw here
    /// takes them, and the painter moves the layout to whole device pixels at its scale, adds its
    /// offset and honours its clip. Returns the rectangle the layout's lines cover, with its top
    /// left corner at `(x, y)` and its size the layout's own divided by the scale, so a caller
    /// can draw a background, a caret or a squiggle against it without measuring again. A place
    /// that is not a number draws nothing; an empty layout draws nothing and returns no size.
    pub fn rich_text(&mut self, layout: &Layout<Brush>, x: f32, y: f32) -> Rect {
        let (w, h) = (layout.width(), layout.height());
        let size = Rect::new(x, y, w / self.scale, h / self.scale);
        if !x.is_finite() || !y.is_finite() {
            return size;
        }
        let dx = (x * self.scale).round() + self.origin.0 as f32;
        let dy = (y * self.scale).round() + self.origin.1 as f32;
        let (xi, yi) = (dx.floor() as i32, dy.floor() as i32);
        let (bw, bh) = (w.ceil() as i32 + 2, h.ceil() as i32 + 2);
        let c = self.clip;
        if xi >= c.2 || yi >= c.3 || xi + bw <= c.0 || yi + bh <= c.1 {
            return size;
        }
        self.gfx
            .engine
            .draw_layout_clipped(self.bmp, layout, dx, dy, c);
        size
    }

    /// A spelling squiggle through the whole of `band`: one path, `thickness` logical px thick,
    /// `wavelength` logical px from one crest to the next, in a colour at an alpha.
    ///
    /// The wave starts and ends on the band's middle line and fills the band's height, so the
    /// stroke never leaves the band: the band is also the clip while it is drawn, which cuts the
    /// round ends of the stroke where it begins and ends. A squiggle drawn under a run is one
    /// path of a few quadratics, not many separate marks, so it is continuous along its whole
    /// width.
    ///
    /// A band with no width or height, a thickness or an alpha that is zero, negative or not a
    /// number, or a place that is not a number, draws nothing. A wavelength that is not a
    /// positive length draws one wave across the band.
    pub fn wavy_underline(
        &mut self,
        band: Rect,
        thickness: f32,
        wavelength: f32,
        c: Rgb,
        alpha: f32,
    ) {
        let numbers = [band.x, band.y, band.w, band.h, thickness, alpha];
        if numbers.iter().any(|v| !v.is_finite())
            || band.w <= 0.0
            || band.h <= 0.0
            || thickness <= 0.0
            || alpha <= 0.0
        {
            return;
        }
        let path = squiggle(band, thickness, wavelength);
        if path.is_empty() {
            return;
        }
        // The band is the clip as well: nothing of the stroke, its round ends included, may land
        // outside it, whatever the scale rounds the band's edges to.
        let old = self.clip;
        let (x0, y0, x1, y1) = self.dev_rect(band);
        self.clip = (old.0.max(x0), old.1.max(y0), old.2.min(x1), old.3.min(y1));
        self.stroke_path(&path, thickness, c, alpha);
        self.clip = old;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::{Gfx, MONO_FAMILY, UI_FAMILY};
    use mdshared_text::{Bitmap, TextEngine};
    use parley::{Alignment, AlignmentOptions, FontFamily, LayoutContext, StyleProperty};

    const WHITE: Rgb = [255, 255, 255];
    /// A text colour as a brush: red, green, blue, and 255 for "on the baseline".
    const RED_BRUSH: Brush = [200, 30, 30, 255];
    const BLUE_BRUSH: Brush = [20, 60, 220, 255];
    const BLACK_BRUSH: Brush = [20, 20, 20, 255];
    const RED: Rgb = [200, 30, 30];

    /// A page of `w` by `h` device px, filled with `bg`, with `draw` on it at `scale`.
    fn page(w: u32, h: u32, scale: f32, bg: Rgb, draw: impl FnOnce(&mut Painter<'_>)) -> Bitmap {
        let mut bmp = Bitmap::new(w, h);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            let (lw, lh) = (w as f32 / scale, h as f32 / scale);
            p.fill(Rect::new(0.0, 0.0, lw, lh), bg);
            draw(&mut p);
        }
        bmp
    }

    fn at(bmp: &Bitmap, x: i32, y: i32) -> Rgb {
        let o = (y as usize * bmp.width as usize + x as usize) * 4;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    }

    /// The pixels that are not `bg`: how many, and the box they lie in (x0, y0, x1, y1, the far
    /// edges outside).
    fn ink(bmp: &Bitmap, bg: Rgb) -> (usize, (i32, i32, i32, i32)) {
        let (mut n, mut b) = (0, (i32::MAX, i32::MAX, i32::MIN, i32::MIN));
        for y in 0..bmp.height as i32 {
            for x in 0..bmp.width as i32 {
                if at(bmp, x, y) != bg {
                    n += 1;
                    b = (b.0.min(x), b.1.min(y), b.2.max(x + 1), b.3.max(y + 1));
                }
            }
        }
        (n, b)
    }

    /// The pixel of a box that is furthest from `bg`, so the colour of the ink there without
    /// hunting for a glyph's middle.
    fn strongest(bmp: &Bitmap, r: Rect, bg: Rgb) -> Rgb {
        let mut best = bg;
        let mut best_d = -1i32;
        for y in r.y as i32..r.bottom() as i32 {
            for x in r.x as i32..r.right() as i32 {
                let p = at(bmp, x, y);
                let d = (0..3)
                    .map(|i| (p[i] as i32 - bg[i] as i32).abs())
                    .sum::<i32>();
                if d > best_d {
                    best_d = d;
                    best = p;
                }
            }
        }
        best
    }

    fn distance(a: Rgb, b: Rgb) -> i32 {
        (0..3).map(|i| (a[i] as i32 - b[i] as i32).abs()).sum()
    }

    /// A one-line layout of `text` in the interface family at `size` device px, every run in
    /// `base` except what `add` says otherwise.
    fn layout(
        text: &str,
        size: f32,
        base: Brush,
        add: impl FnOnce(&mut parley::RangedBuilder<'_, Brush>),
    ) -> Layout<Brush> {
        let mut fonts = mdshared_text::shared_fonts();
        let mut layouts = LayoutContext::<Brush>::new();
        let mut b = layouts.ranged_builder(&mut fonts, text, 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::from(UI_FAMILY)));
        b.push_default(StyleProperty::FontSize(size));
        b.push_default(StyleProperty::Brush(base));
        add(&mut b);
        let mut layout = b.build(text);
        layout.break_all_lines(None);
        layout.align(Alignment::Start, AlignmentOptions::default());
        layout
    }

    /// Two runs of `text`, the second one from byte `split` on in `second`.
    fn two_runs(text: &str, size: f32, split: usize, second: Brush) -> Layout<Brush> {
        layout(text, size, RED_BRUSH, |b| {
            b.push(StyleProperty::Brush(second), split..)
        })
    }

    #[test]
    fn two_runs_are_drawn_in_their_own_colours() {
        let text = "wrong word";
        let layout = two_runs(text, 26.0, 6, BLUE_BRUSH);
        let boxes = crate::richtext::runs(&layout);
        assert_eq!(boxes.len(), 2, "{boxes:?}");
        assert_eq!(boxes[0].range, 0..6);
        assert_eq!(boxes[1].range, 6..10);

        let mut bmp = Bitmap::new(260, 60);
        let mut gfx = Gfx::new();
        let rect = {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, 260.0, 60.0), WHITE);
            p.rich_text(&layout, 8.0, 8.0)
        };
        assert!((rect.w - layout.width()).abs() < 0.01, "{rect:?}");
        assert!((rect.h - layout.height()).abs() < 0.01, "{rect:?}");

        // The first run's ink is red and the second's is blue, not one colour over both.
        let red = [RED_BRUSH[0], RED_BRUSH[1], RED_BRUSH[2]];
        let blue = [BLUE_BRUSH[0], BLUE_BRUSH[1], BLUE_BRUSH[2]];
        for (k, want) in [(0, red), (1, blue)] {
            let box_ = boxes[k].rect;
            let box_ = Rect::new(box_.x + 8.0, box_.y + 8.0, box_.w, box_.h);
            let got = strongest(&bmp, box_, WHITE);
            let other = if k == 0 { blue } else { red };
            assert!(
                distance(got, want) < distance(got, other),
                "run {k} is {got:?}, not near {want:?}"
            );
            // And it is the colour the caller asked for, not merely the nearer of the two.
            assert!(distance(got, want) < 60, "run {k} is {got:?}");
        }

        // The drawing is the text engine's own, at the same place: the painter only moves the
        // caller's layout to whole device pixels and hands it over.
        let mut want = Bitmap::new(260, 60);
        want.clear(WHITE);
        let mut engine = TextEngine::new(4 << 20);
        engine.set_subpixel(crate::platform::subpixel_order());
        engine.draw_layout(&mut want, &layout, 8.0, 8.0);
        assert!(bmp.pixels == want.pixels, "not the engine's drawing of it");
    }

    #[test]
    fn a_run_of_another_family_is_drawn_as_that_family_shaped_it() {
        let text = "abcdef";
        let mixed = layout(text, 24.0, BLACK_BRUSH, |b| {
            b.push(
                StyleProperty::FontFamily(FontFamily::from(MONO_FAMILY)),
                3..,
            )
        });
        let same = layout(text, 24.0, BLACK_BRUSH, |_| ());
        let m = crate::richtext::runs(&mixed);
        assert_eq!(m.len(), 2, "{m:?}");
        assert_eq!(m[1].range, 3..6);
        assert_eq!(crate::richtext::runs(&same).len(), 1);

        // The two runs really were shaped in two different fonts ...
        let fonts: Vec<u64> = mixed
            .lines()
            .flat_map(|line| line.runs())
            .map(|run| run.font().data.id())
            .collect();
        assert_eq!(fonts.len(), 2);
        assert_ne!(fonts[0], fonts[1], "the mono family was not found");
        // ... and the painter draws what parley shaped, pixel for pixel.
        let mut got = Bitmap::new(240, 50);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut got, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, 240.0, 50.0), WHITE);
            p.rich_text(&mixed, 5.0, 5.0);
        }
        let mut want = Bitmap::new(240, 50);
        want.clear(WHITE);
        let mut engine = TextEngine::new(4 << 20);
        engine.set_subpixel(crate::platform::subpixel_order());
        engine.draw_layout(&mut want, &mixed, 5.0, 5.0);
        assert!(got.pixels == want.pixels);
        assert!(ink(&got, WHITE).0 > 100);
    }

    #[test]
    fn a_wavy_underline_is_continuous_across_its_band_and_stays_inside_it() {
        let band = Rect::new(10.0, 20.0, 120.0, 6.0);
        let bmp = page(150, 50, 1.0, WHITE, |p| {
            p.wavy_underline(band, 1.5, 8.0, RED, 1.0)
        });
        let (n, b) = ink(&bmp, WHITE);
        assert!(n > 0);
        let (x0, y0, x1, y1) = (10, 20, 130, 26);
        assert!(
            b.0 >= x0 && b.1 >= y0 && b.2 <= x1 && b.3 <= y1,
            "ink at {b:?} leaves the band"
        );
        // No gap along the band: every device column of it has ink somewhere.
        for x in x0..x1 {
            assert!(
                (y0..y1).any(|y| at(&bmp, x, y) != WHITE),
                "no ink in column {x}"
            );
        }
        // The stroke fills the band's height: its crests touch the top row and its troughs the
        // bottom one.
        assert!((x0..x1).any(|x| at(&bmp, x, y0) != WHITE), "no crest");
        assert!((x0..x1).any(|x| at(&bmp, x, y1 - 1) != WHITE), "no trough");
        // The squiggle is one path with a crest above the middle line and a trough below it,
        // alternating: two neighbouring halves of a wavelength are on opposite sides.
        let mid = 23;
        let side = |x: i32| -> i32 {
            (y0..y1)
                .filter(|&y| at(&bmp, x, y) != WHITE)
                .map(|y| y - mid)
                .sum::<i32>()
                .signum()
        };
        assert_eq!(
            (side(x0 + 2), side(x0 + 6)),
            (-1, 1),
            "the wave does not turn"
        );

        // A thin band is a straight line where the stroke fills it, still continuous.
        let flat = page(150, 50, 1.0, WHITE, |p| {
            p.wavy_underline(Rect::new(10.0, 22.0, 60.0, 1.0), 1.0, 8.0, RED, 1.0)
        });
        assert_eq!(ink(&flat, WHITE).1, (10, 22, 70, 23));

        // Nothing for nothing.
        let none = page(150, 50, 1.0, WHITE, |p| {
            p.wavy_underline(Rect::new(10.0, 20.0, 0.0, 6.0), 1.5, 8.0, RED, 1.0);
            p.wavy_underline(Rect::new(10.0, 20.0, 120.0, 0.0), 1.5, 8.0, RED, 1.0);
            p.wavy_underline(band, 0.0, 8.0, RED, 1.0);
            p.wavy_underline(band, 1.5, 8.0, RED, 0.0);
            p.wavy_underline(band, 1.5, 8.0, RED, f32::NAN);
            p.wavy_underline(band, f32::NAN, 8.0, RED, 1.0);
            p.wavy_underline(Rect::new(f32::NAN, 20.0, 120.0, 6.0), 1.5, 8.0, RED, 1.0);
        });
        assert_eq!(ink(&none, WHITE).0, 0);
    }

    #[test]
    fn the_painter_clip_cuts_the_layout_and_the_wave() {
        let layout = two_runs("wrong word", 20.0, 6, BLUE_BRUSH);
        let wave = Rect::new(8.0, 26.0, 200.0, 5.0);
        let whole = page(260, 60, 1.0, WHITE, |p| {
            p.rich_text(&layout, 8.0, 6.0);
            p.wavy_underline(wave, 1.5, 8.0, RED, 1.0);
        });
        let cut = page(260, 60, 1.0, WHITE, |p| {
            let old = p.clip(Rect::new(0.0, 0.0, 70.0, 30.0));
            p.rich_text(&layout, 8.0, 6.0);
            p.wavy_underline(wave, 1.5, 8.0, RED, 1.0);
            p.restore(old);
        });
        let mut inside = 0;
        for y in 0..60 {
            for x in 0..260 {
                let in_clip = x < 70 && y < 30;
                let got = at(&cut, x, y);
                if in_clip {
                    assert_eq!(got, at(&whole, x, y), "{x},{y}");
                    inside += (got != WHITE) as usize;
                } else {
                    assert_eq!(got, WHITE, "{x},{y} is outside the clip");
                }
            }
        }
        assert!(inside > 50, "the clip kept nothing");
        // The layout is cut as well: the whole drawing has text ink above the wave, and some of
        // it is on each side of the clip's right edge.
        let (mut text, mut text_inside) = (0, 0);
        for y in 0..26 {
            for x in 0..260 {
                if at(&whole, x, y) != WHITE {
                    text += 1;
                    text_inside += usize::from(x < 70);
                }
            }
        }
        assert!(
            text > 100 && text_inside > 20,
            "the layout's own ink: {text} of it, {text_inside} inside the clip"
        );
    }

    #[test]
    fn the_scale_places_the_layout_and_keeps_the_wave_in_its_band() {
        let band = Rect::new(6.0, 25.0, 60.0, 5.0);
        for scale in [1.0f32, 1.5, 2.0] {
            let layout = layout("speling", 14.0 * scale, BLACK_BRUSH, |_| ());
            let (w, h) = ((80.0 * scale) as u32, (40.0 * scale) as u32);
            let (got, shown) = scale_page(w, h, scale, |p| p.rich_text(&layout, 4.0, 4.0));
            // The size the caller gets is in logical px, whatever the scale.
            assert!(
                (shown.w - layout.width() / scale).abs() < 0.01,
                "{scale}: {} of {}",
                shown.w,
                layout.width()
            );
            assert!(
                (shown.h - layout.height() / scale).abs() < 0.01,
                "{scale}: {} of {}",
                shown.h,
                layout.height()
            );
            // The painting is the engine's, at the device place the scale names.
            let mut want = Bitmap::new(w, h);
            want.clear(WHITE);
            let mut engine = TextEngine::new(4 << 20);
            engine.set_subpixel(crate::platform::subpixel_order());
            engine.draw_layout(
                &mut want,
                &layout,
                (4.0 * scale).round(),
                (4.0 * scale).round(),
            );
            assert!(
                got.pixels == want.pixels,
                "{scale}: not the engine's drawing"
            );

            // The wave fills its band in device px and nothing outside it, at every scale.
            let wave = page(w, h, scale, WHITE, |p| {
                p.wavy_underline(band, 1.0, 6.0, RED, 1.0)
            });
            let dev = (
                (band.x * scale).round() as i32,
                (band.y * scale).round() as i32,
                (band.right() * scale).round() as i32,
                (band.bottom() * scale).round() as i32,
            );
            assert_eq!(ink(&wave, WHITE).1, dev, "{scale}: the band in device px");
        }
    }

    /// A page of `w` by `h` device px at `scale` with `draw` on it, and its bitmap beside what
    /// `draw` answered.
    fn scale_page(
        w: u32,
        h: u32,
        scale: f32,
        draw: impl FnOnce(&mut Painter<'_>) -> Rect,
    ) -> (Bitmap, Rect) {
        let mut bmp = Bitmap::new(w, h);
        let mut gfx = Gfx::new();
        let answer = {
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            p.fill(
                Rect::new(0.0, 0.0, w as f32 / scale, h as f32 / scale),
                WHITE,
            );
            draw(&mut p)
        };
        (bmp, answer)
    }

    /// The sheet a person looks at: mixed runs and squiggles, light and dark, at two scales.
    /// `cargo test -p mdshared-ui rich_text_sheet -- --ignored`, then open
    /// `target/specimen/richtext.png` and `richtext-150.png`.
    #[test]
    #[ignore]
    fn rich_text_sheet() {
        for (scale, file) in [(1.0f32, "richtext.png"), (1.5, "richtext-150.png")] {
            let mut bmp = Bitmap::new((560.0 * scale) as u32, (300.0 * scale) as u32);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            for (half, (bg, fg, accent)) in [
                ([251, 251, 251], [31, 31, 31], [0, 95, 184]),
                ([32, 32, 32], [230, 230, 230], [96, 205, 255]),
            ]
            .into_iter()
            .enumerate()
            {
                let oy = half as f32 * 150.0;
                p.fill(Rect::new(0.0, oy, 560.0, 150.0), bg);
                let o = p.offset(0.0, oy);
                let ink = [fg[0], fg[1], fg[2], 255];
                let bold = [accent[0], accent[1], accent[2], 255];
                let title = layout("A rich line of text", 20.0 * scale, ink, |b| {
                    b.push(
                        StyleProperty::FontFamily(FontFamily::from(MONO_FAMILY)),
                        2..6,
                    );
                    b.push(StyleProperty::Brush(bold), 7..11);
                });
                p.rich_text(&title, 20.0, 14.0);
                let body_text = "The quick brown fox jumps over the lazy dog, with a wrod in it.";
                let wrong = body_text.find("wrod").expect("the misspelling");
                // The marked word is a run of its own, which is how a caller finds where to put
                // the squiggle: mark it in the layout, read its box, draw under it.
                let body = layout(body_text, 13.0 * scale, ink, |b| {
                    b.push(StyleProperty::Brush(bold), 10..15);
                    b.push(
                        StyleProperty::FontFamily(FontFamily::from(MONO_FAMILY)),
                        16..21,
                    );
                    b.push(
                        StyleProperty::Brush([ink[0], ink[1], ink[2], mdshared_text::text::MARKED]),
                        wrong..wrong + 4,
                    );
                });
                p.rich_text(&body, 20.0, 48.0);
                if let Some(word) = crate::richtext::runs(&body)
                    .into_iter()
                    .find(|b| b.range.start == wrong)
                {
                    let band = Rect::new(
                        20.0 + word.rect.x / scale,
                        48.0 + word.rect.bottom() / scale + 1.0,
                        word.rect.w / scale,
                        3.0,
                    );
                    p.wavy_underline(band, 1.0, 4.0, accent, 1.0);
                }
                // A raised run, as the fourth byte of a brush asks for.
                let raised = layout("E = mc2  and  H2O", 15.0 * scale, ink, |b| {
                    b.push(
                        StyleProperty::Brush([ink[0], ink[1], ink[2], mdshared_text::text::RAISED]),
                        6..7,
                    );
                    b.push(
                        StyleProperty::Brush([
                            ink[0],
                            ink[1],
                            ink[2],
                            mdshared_text::text::LOWERED,
                        ]),
                        15..16,
                    );
                });
                p.rich_text(&raised, 20.0, 86.0);
                p.text(
                    crate::gfx::Face::Ui,
                    11.0,
                    20.0,
                    128.0,
                    "rich text: per-run brushes, families, sizes and baseline shifts",
                    fg,
                    500.0,
                );
                p.restore_offset(o);
            }
            let out =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
            bmp.write_png(&out.join(file)).unwrap();
        }
    }
}
