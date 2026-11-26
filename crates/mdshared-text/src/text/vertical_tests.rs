use crate::bitmap::Bitmap;
use crate::text::{Entry, Key, Subpixel, TextEngine};
use parley::{FontData, LayoutContext, StyleProperty};
use swash::FontRef;
use swash::scale::image::Image;
use swash::scale::{Render, ScaleContext, Scaler, Source};
use swash::zeno::{Format, Vector};

fn windows_font(name: &str) -> Option<FontData> {
    let path = std::path::PathBuf::from(std::env::var_os("WINDIR")?)
        .join("Fonts")
        .join(name);
    Some(FontData::new(std::fs::read(path).ok()?.into(), 0))
}

fn phase(sub: u8) -> Vector {
    Vector::new(sub as f32 / 4.0, 0.0)
}

fn legacy_mask(scaler: &mut Scaler<'_>, glyph: u16, sub: u8) -> Image {
    Render::new(&[Source::Outline])
        .format(Format::Alpha)
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

#[test]
fn a_word_at_16_and_a_quarter_pixels_keeps_its_unhinted_masks() {
    a_word_keeps_its_unhinted_masks(16.25);
}

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
