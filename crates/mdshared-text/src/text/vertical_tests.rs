use crate::bitmap::Bitmap;
use crate::text::{Entry, Key, Subpixel, TextEngine};
use parley::{FontData, LayoutContext, StyleProperty};
use swash::FontRef;
use swash::scale::image::Image;
use swash::scale::{Render, ScaleContext, Scaler, Source};
use swash::zeno::{Format, Vector};

#[cfg(windows)]
fn windows_font(name: &str) -> Option<FontData> {
    let path = std::path::PathBuf::from(std::env::var_os("WINDIR")?)
        .join("Fonts")
        .join(name);
    Some(FontData::new(std::fs::read(path).ok()?.into(), 0))
}

fn phase(sub: u8) -> Vector {
    Vector::new(sub as f32 / 4.0, 0.0)
}

#[cfg(windows)]
fn legacy_mask(scaler: &mut Scaler<'_>, glyph: u16, sub: u8) -> Image {
    legacy_mask_in(scaler, glyph, sub, Format::Alpha)
}

fn legacy_mask_in(scaler: &mut Scaler<'_>, glyph: u16, sub: u8, format: Format) -> Image {
    Render::new(&[Source::Outline])
        .format(format)
        .offset(phase(sub))
        .render(scaler, glyph)
        .unwrap()
}

fn mask(engine: &mut TextEngine, font: &FontData, glyph: u16, size: f32, sub: u8) -> Entry {
    engine.raster(
        Key {
            font: font.data.id(),
            index: 0,
            glyph,
            size: size.to_bits(),
            sub,
            coords: 0,
        },
        font,
        std::iter::empty(),
        size,
    )
}

#[cfg(windows)]
#[test]
fn small_segoe_ui_cap_and_crossbar_snap_without_moving_x() {
    let Some(font) = windows_font("segoeui.ttf") else {
        eprintln!("skipped: Segoe UI is unavailable");
        return;
    };
    let reference = FontRef::from_index(font.data.as_ref(), 0).unwrap();
    let glyph = reference.charmap().map('H');
    assert_ne!(glyph, 0);
    let size = 13.0;
    let mut ctx = ScaleContext::new();
    let mut unhinted = ctx.builder(reference).size(size).build();
    let outline = unhinted.scale_outline(glyph).unwrap();
    let legacy = legacy_mask(&mut unhinted, glyph, 1);
    let mut engine = TextEngine::new(4 << 20);
    engine.set_subpixel(Subpixel::None);
    let got = mask(&mut engine, &font, glyph, size, 1);
    assert_eq!(
        (got.left, got.w),
        (legacy.placement.left, legacy.placement.width)
    );
    assert!(
        !got.data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| p[0])
            .eq(legacy.data),
        "the fitted mask must differ from the original"
    );
    let fitted = &engine.vertical.linear;
    assert_eq!(fitted.verbs(), outline.verbs());
    for (a, b) in fitted.points().iter().zip(outline.points()) {
        assert_eq!(a.x.to_bits(), b.x.to_bits());
        assert_eq!(a.y.fract(), 0.0, "a horizontal edge of H is between rows");
    }
    assert_ne!(fitted.bounds().max.y, outline.bounds().max.y);
}

#[cfg(windows)]
fn a_word_keeps_its_unhinted_masks(size: f32) {
    let Some(font) = windows_font("segoeui.ttf") else {
        eprintln!("skipped: Segoe UI is unavailable");
        return;
    };
    let reference = FontRef::from_index(font.data.as_ref(), 0).unwrap();
    let mut ctx = ScaleContext::new();
    let mut unhinted = ctx.builder(reference).size(size).build();
    let mut engine = TextEngine::new(4 << 20);
    engine.set_subpixel(Subpixel::None);
    // Narrow and wide glyphs in one word, plus a superscript. Exercise every X phase.
    for ch in "Illuminatingˡ".chars() {
        let glyph = reference.charmap().map(ch);
        assert_ne!(glyph, 0);
        for sub in 0..4 {
            let legacy = legacy_mask(&mut unhinted, glyph, sub);
            let got = mask(&mut engine, &font, glyph, size, sub);
            let p = legacy.placement;
            assert_eq!(
                (got.left, got.top, got.w, got.h),
                (p.left, p.top, p.width, p.height),
                "{ch:?} at {size} px, phase {sub}"
            );
            assert!(
                got.data
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .all(|p| p[0] == p[1] && p[1] == p[2])
                    && got
                        .data
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .map(|p| p[0])
                        .eq(legacy.data),
                "{ch:?} at {size} px, phase {sub}: mask differs from unhinted"
            );
        }
    }
}

#[cfg(windows)]
#[test]
fn a_word_at_16_and_a_quarter_pixels_keeps_its_unhinted_masks() {
    a_word_keeps_its_unhinted_masks(16.25);
}

#[cfg(windows)]
#[test]
fn a_word_at_19_and_a_half_pixels_keeps_its_unhinted_masks() {
    a_word_keeps_its_unhinted_masks(19.5);
}

#[test]
fn neighbouring_fractional_sizes_do_not_share_cached_masks() {
    let mut fonts = crate::shared_fonts();
    let mut layouts = LayoutContext::new();
    let mut layout = |size| {
        let mut builder = layouts.ranged_builder(&mut fonts, "H", 1.0, true);
        builder.push_default(StyleProperty::FontSize(size));
        let mut layout = builder.build("H");
        layout.break_all_lines(None);
        layout
    };
    let mut engine = TextEngine::new(4 << 20);
    let mut fb = Bitmap::new(320, 80);
    engine.draw_layout(&mut fb, &layout(16.0), 0.0, 0.0);
    assert_eq!(engine.stats.misses, 1);
    engine.draw_layout(&mut fb, &layout(16.0 + 1.0 / 128.0), 0.0, 0.0);
    assert_eq!(engine.stats.misses, 2);
}

/// One half-em square at glyph 0. Only the tables swash needs to scale this glyph are
/// included. Touch both top points in Y, then move them by a signed 26.6 pixel distance.
fn square_font(shift: i16) -> FontData {
    let mut head = [0; 54];
    head[..4].copy_from_slice(&[0, 1, 0, 0]);
    head[12..16].copy_from_slice(&0x5f0f3cf5_u32.to_be_bytes());
    head[17] = 1 << 3; // the TrueType hinter only fits whole device sizes
    head[18..20].copy_from_slice(&1024_u16.to_be_bytes());
    let mut hhea = [0; 36];
    hhea[..4].copy_from_slice(&[0, 1, 0, 0]);
    hhea[34..36].copy_from_slice(&1_u16.to_be_bytes());
    let mut maxp = [0; 32];
    maxp[..10].copy_from_slice(&[0, 1, 0, 0, 0, 1, 0, 4, 0, 1]);
    maxp[14..16].copy_from_slice(&2_u16.to_be_bytes()); // glyph and twilight zones
    maxp[24..26].copy_from_slice(&16_u16.to_be_bytes()); // stack depth

    let [hi, lo] = shift.to_be_bytes();
    let program = [
        0x00, // SVTCA[y]
        0xb0, 2, 0x2e, // MDAP[no round], point 2
        0xb0, 3, 0x2e, // MDAP[no round], point 3
        0xb0, 2, 0x17, // SLOOP, two points
        0xb1, 2, 3, 0xb8, hi, lo, 0x38, // SHPIX, points 2 and 3
    ];
    maxp[26..28].copy_from_slice(&(program.len() as u16).to_be_bytes());
    let mut glyf = vec![0, 1, 0, 0, 0, 0, 2, 0, 2, 0, 0, 3, 0, program.len() as u8];
    glyf.extend(program);
    glyf.extend([0x31, 0x21, 0x11, 0x21, 2, 0, 0xfe, 0, 2, 0]);
    glyf.resize(glyf.len().next_multiple_of(2), 0);
    let loca = [0, 0, 0, (glyf.len() / 2) as u8];
    let tables: [(&[u8; 4], &[u8]); 6] = [
        (b"glyf", &glyf),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &[2, 0, 0, 0]),
        (b"loca", &loca),
        (b"maxp", &maxp),
    ];
    let mut sfnt = vec![0, 1, 0, 0, 0, tables.len() as u8, 0, 64, 0, 2, 0, 32];
    sfnt.resize(12 + 16 * tables.len(), 0);
    for (i, (tag, contents)) in tables.into_iter().enumerate() {
        let record = 12 + 16 * i;
        let offset = (sfnt.len() as u32).to_be_bytes();
        sfnt[record..record + 4].copy_from_slice(tag);
        sfnt[record + 8..record + 12].copy_from_slice(&offset);
        sfnt[record + 12..record + 16].copy_from_slice(&(contents.len() as u32).to_be_bytes());
        sfnt.extend_from_slice(contents);
        sfnt.resize(sfnt.len().next_multiple_of(4), 0);
    }
    FontData::new(sfnt.into(), 0)
}

fn check_rejected_fit(font: &FontData, size: f32, expected_shift: f32, modes: &[Subpixel]) {
    let reference = FontRef::from_index(font.data.as_ref(), 0).unwrap();
    let mut ctx = ScaleContext::new();
    let hinted = ctx
        .builder(reference)
        .size(size)
        .hint(true)
        .build()
        .scale_outline(0)
        .unwrap();
    let mut linear = ctx.builder(reference).size(size).build();
    let outline = linear.scale_outline(0).unwrap();
    assert_eq!(hinted.verbs(), outline.verbs());
    assert!(
        hinted
            .points()
            .iter()
            .zip(outline.points())
            .all(|(a, b)| a.x == b.x)
    );
    let moved: Vec<_> = hinted
        .points()
        .iter()
        .zip(outline.points())
        .map(|(a, b)| a.y - b.y)
        .filter(|&dy| dy != 0.0)
        .collect();
    assert_eq!(moved, [expected_shift; 2]);

    let mut engine = TextEngine::new(4 << 20);
    for &mode in modes {
        engine.set_subpixel(mode);
        for sub in 0..4 {
            let legacy = legacy_mask_in(&mut linear, 0, sub, mode.format());
            let got = mask(&mut engine, font, 0, size, sub);
            let p = legacy.placement;
            assert_eq!(
                (got.left, got.top, got.w, got.h),
                (p.left, p.top, p.width, p.height)
            );
            assert_eq!(got.data.len(), (p.width * p.height * 3) as usize);
            match mode {
                Subpixel::None => assert!(
                    got.data
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .copied()
                        .eq(legacy.data.iter().map(|&a| [a; 3]))
                ),
                _ => assert!(
                    got.data
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .zip(legacy.data.as_chunks::<4>().0)
                        .all(|(a, b)| a[..] == b[..3])
                ),
            }
            assert_eq!(engine.vertical.linear.points(), outline.points());
        }
    }
}

#[test]
fn an_overstretched_hint_keeps_the_unhinted_pixels_at_every_phase() {
    // The two top points move more than four ems but land inside the four-em box.
    check_rejected_fit(
        &square_font(-72 * 64),
        16.0,
        -72.0,
        &[Subpixel::None, Subpixel::Rgb, Subpixel::Bgr],
    );
}

#[test]
fn a_fit_outside_the_em_box_keeps_the_unhinted_pixels() {
    // Each point moves less than four ems, but the top edge ends up outside the box.
    check_rejected_fit(&square_font(60 * 64), 16.0, 60.0, &[Subpixel::None]);
}

#[test]
fn a_fit_that_exceeds_the_mask_budget_keeps_the_unhinted_pixels() {
    check_rejected_fit(&square_font(128 * 64), 2040.0, 128.0, &[Subpixel::Rgb]);
}

#[test]
fn an_unhinted_glyph_over_the_mask_budget_is_not_rasterized() {
    let font = square_font(0);
    let mut engine = TextEngine::new(4 << 20);
    // The fractional request bypasses the hinter. At 1026 x 1026 x 4 bytes the mask would
    // be just above the 4 MiB budget.
    let got = mask(&mut engine, &font, 0, 2050.25, 0);
    assert_eq!((got.w, got.h, got.data.len()), (0, 0, 0));
}

#[test]
fn the_mask_budget_counts_channels_and_rejects_overflow_and_long_sides() {
    use super::mask_bytes;

    assert_eq!(mask_bytes(Format::Alpha, 2048, 2048), Some(4 << 20));
    assert_eq!(mask_bytes(Format::Subpixel, 1024, 1024), Some(4 << 20));
    assert_eq!(mask_bytes(Format::Subpixel, 1025, 1024), None);
    assert_eq!(mask_bytes(Format::Alpha, 65_536, 65_536), None);
    assert_eq!(mask_bytes(Format::Subpixel, 32_768, 32_768), None);
    assert_eq!(mask_bytes(Format::Alpha, 1, 8193), None);
    assert_eq!(mask_bytes(Format::Subpixel, 16, 0), None);
}
