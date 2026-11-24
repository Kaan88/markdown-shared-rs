use super::*;

/// Accents across the wheel and the greys, light and dark, the system ones included.
fn accents() -> Vec<Rgb> {
    let mut out = vec![
        [0, 120, 212],   // Windows's default blue
        [255, 185, 0],   // gold
        [16, 124, 16],   // green
        [232, 17, 35],   // red
        [136, 23, 152],  // purple
        [255, 255, 255], // white
        [0, 0, 0],       // black
        [128, 128, 128], // grey
        [255, 255, 0],   // yellow
        [0, 255, 255],   // cyan
    ];
    for h in 0..12u32 {
        let x = (h * 255 / 12) as u8;
        out.push([x, 255 - x, x / 2]);
        out.push([255 - x / 3, x, 200]);
    }
    out
}

#[test]
fn without_tokens_the_look_is_the_toolkits_own() {
    assert_eq!(Tokens::new(false).palette, LIGHT);
    assert_eq!(Tokens::new(true).palette, DARK);
    assert_eq!(Tokens::new(true).sizes, Sizes::DEFAULT);
    assert_eq!(Sizes::default(), Sizes::DEFAULT);
    assert_eq!(Sizes::DEFAULT.row, crate::pane::ROW_H);
    assert_eq!(Sizes::DEFAULT.thumb, crate::slider::THUMB);
    // The palette's own accent changes nothing.
    assert_eq!(accented(&LIGHT, LIGHT.accent), LIGHT);
    assert_eq!(
        Tokens::new(true).with_accent(DARK.accent),
        Tokens::new(true)
    );
}

#[test]
fn every_accent_is_made_legible_on_its_page() {
    for base in [LIGHT, DARK, HIGH_CONTRAST_LIGHT, HIGH_CONTRAST_DARK] {
        for a in accents() {
            let p = accented(&base, a);
            assert!(
                contrast(p.accent, p.page) >= ACCENT_CONTRAST - 0.01,
                "{a:?} on {:?}: {}",
                base.page,
                contrast(p.accent, p.page)
            );
            assert!(
                contrast(p.on_accent, p.accent) >= 4.0,
                "text on {:?}: {}",
                p.accent,
                contrast(p.on_accent, p.accent)
            );
            assert!(
                contrast(p.text, p.selection) >= 3.0 - 0.01,
                "text on the selection of {a:?}: {}",
                contrast(p.text, p.selection)
            );
            assert_eq!(p.text, base.text);
            assert_eq!(p.danger, base.danger);
        }
    }
}

#[test]
fn a_legible_accent_is_left_as_it_is() {
    let blue = [0, 80, 160];
    assert!(contrast(blue, LIGHT.page) >= ACCENT_CONTRAST);
    assert_eq!(accented(&LIGHT, blue).accent, blue);
    assert_eq!(legible(blue, LIGHT.page, 4.5), blue);
}

#[test]
fn legible_moves_by_the_least_that_reaches() {
    let gold = [255, 185, 0];
    let moved = legible(gold, LIGHT.page, 4.5);
    assert!(contrast(moved, LIGHT.page) >= 4.5);
    // Not much more than asked: the search stops at the edge.
    assert!(contrast(moved, LIGHT.page) < 4.7);
    // On a dark page the same gold is legible already.
    assert_eq!(legible(gold, DARK.page, 4.5), gold);
    // A ratio nothing reaches gives the end of the road.
    assert_eq!(legible([128, 128, 128], [255, 255, 255], 30.0), [0, 0, 0]);
}

#[test]
fn the_high_contrast_palettes_reach_seven_to_one() {
    for p in [HIGH_CONTRAST_LIGHT, HIGH_CONTRAST_DARK] {
        for (name, c) in [("text", p.text), ("muted", p.muted), ("accent", p.accent)] {
            assert!(
                contrast(c, p.page) >= 7.0,
                "{name}: {}",
                contrast(c, p.page)
            );
        }
        assert!(contrast(p.faint, p.page) >= 4.5);
        assert!(contrast(p.line, p.page) >= 3.0);
        assert!(contrast(p.on_accent, p.accent) >= 7.0);
        assert!(contrast(p.text, p.hover) >= 7.0);
        assert!(contrast(p.text, p.on) >= 7.0);
        assert!(contrast(p.text, p.selection) >= 7.0);
        assert!(contrast(p.warn, p.page) >= 4.5);
        assert!(contrast(p.danger, p.page) >= 4.5);
    }
    assert!(!Tokens::high_contrast(false).palette.dark);
    assert_eq!(Tokens::high_contrast(true).palette, HIGH_CONTRAST_DARK);
}

#[test]
fn sizes_scale_together_and_refuse_nonsense() {
    let big = Sizes::DEFAULT.scaled(1.25);
    assert_eq!(big.text, 13.0 * 1.25);
    assert_eq!(big.row, crate::pane::ROW_H * 1.25);
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(Sizes::DEFAULT.scaled(bad), Sizes::DEFAULT);
    }
    assert_eq!(Tokens::new(false).scaled(2.0).sizes.radius, 10.0);
}

/// The system's answers cannot be checked against a known value (they are the user's
/// settings), but asking must not fall over, and the look must be one of the known ones.
#[test]
fn the_system_look_is_one_of_the_known_ones() {
    let t = Tokens::from_system(true);
    eprintln!(
        "system: dark {}, high contrast {}, accent {:?}",
        crate::platform::system_is_dark(),
        crate::platform::system_high_contrast(),
        crate::platform::system_accent()
    );
    assert!(contrast(t.palette.accent, t.palette.page) >= ACCENT_CONTRAST - 0.01);
    assert_eq!(t.sizes, Sizes::DEFAULT);
    let plain = Tokens::from_system(false);
    assert!(
        [LIGHT, DARK, HIGH_CONTRAST_LIGHT, HIGH_CONTRAST_DARK].contains(&plain.palette),
        "an unaccented look is a known palette"
    );
}

/// A sheet for a person to judge: `cargo test -p mdshared-ui tokens_sheet -- --ignored` writes
/// `target/specimen/tokens.png`: for the toolkit's palettes, the high-contrast ones and four
/// accents in light and dark, a link, a button in the accent, a selected row and selected text.
#[test]
#[ignore]
fn tokens_sheet() {
    use crate::gfx::{Face, Gfx, Painter, Rect};
    use mdshared_text::Bitmap;
    let looks: Vec<(String, Palette)> = {
        let mut v = vec![
            ("light".to_string(), LIGHT),
            ("dark".to_string(), DARK),
            ("high contrast light".to_string(), HIGH_CONTRAST_LIGHT),
            ("high contrast dark".to_string(), HIGH_CONTRAST_DARK),
        ];
        for (name, a) in [
            ("gold", [255, 185, 0]),
            ("green", [16, 124, 16]),
            ("purple", [136, 23, 152]),
            ("teal", [68, 122, 137]),
        ] {
            v.push((format!("{name} light"), accented(&LIGHT, a)));
            v.push((format!("{name} dark"), accented(&DARK, a)));
        }
        v
    };
    let (w, row) = (620.0, 40.0);
    let mut bmp = Bitmap::new(w as u32, (row * looks.len() as f32) as u32);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    for (k, (name, pal)) in looks.iter().enumerate() {
        let y = k as f32 * row;
        p.fill(Rect::new(0.0, y, w, row), pal.page);
        let cy = y + row / 2.0;
        p.text(Face::Ui, 12.0, 10.0, cy, name, pal.muted, 150.0);
        p.text(Face::Ui, 13.0, 165.0, cy, "A link", pal.accent, 80.0);
        let button = Rect::new(240.0, y + 7.0, 90.0, 26.0);
        p.rbox(button, 5.0, pal.accent);
        p.text_center(Face::Ui, 13.0, button, "Accent", pal.on_accent);
        let selected = Rect::new(345.0, y + 6.0, 130.0, 28.0);
        p.rbox(selected, 5.0, pal.on);
        p.text(Face::Ui, 13.0, 355.0, cy, "Selected row", pal.text, 110.0);
        let marked = Rect::new(490.0, y + 10.0, 60.0, 20.0);
        p.fill(marked, pal.selection);
        p.text(Face::Ui, 13.0, 492.0, cy, "selected", pal.text, 120.0);
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
    std::fs::create_dir_all(&out).unwrap();
    bmp.write_png(&out.join("tokens.png")).unwrap();
}
