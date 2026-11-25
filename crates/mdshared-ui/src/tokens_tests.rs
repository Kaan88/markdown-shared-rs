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
    // Under a Windows contrast theme the look is made from that theme's colours.
    let scheme = crate::platform::system_contrast_colors().map(|c| Tokens::from_system_colors(&c));
    eprintln!("contrast theme: {scheme:?}");
    assert!(
        [LIGHT, DARK, HIGH_CONTRAST_LIGHT, HIGH_CONTRAST_DARK].contains(&plain.palette)
            || scheme == Some(plain),
        "an unaccented look is a known palette or the system's contrast theme"
    );
}

/// A contrast theme from its eight colours, in the order of [`SystemColors`]'s fields.
const fn scheme(c: [u32; 8]) -> SystemColors {
    SystemColors {
        window: rgb(c[0]),
        window_text: rgb(c[1]),
        highlight: rgb(c[2]),
        highlight_text: rgb(c[3]),
        hot_light: rgb(c[4]),
        gray_text: rgb(c[5]),
        button_face: rgb(c[6]),
        button_text: rgb(c[7]),
    }
}

/// Windows 11's Aquatic, from `hcblack.theme` (Resources, Ease of Access Themes).
const AQUATIC: SystemColors = scheme([
    0x202020, 0xffffff, 0x8ee3f0, 0x263b50, 0x75e9fc, 0xa6a6a6, 0x202020, 0xffffff,
]);
/// Windows 11's Desert, a light one, from `hcwhite.theme`.
const DESERT: SystemColors = scheme([
    0xfffaef, 0x3d3d3d, 0x903909, 0xfff5e3, 0x1c5e75, 0x676767, 0xfffaef, 0x202020,
]);
/// Windows 11's Dusk, from `hc1.theme`: its button text is not its window text.
const DUSK: SystemColors = scheme([
    0x2d3236, 0xffffff, 0xa1bfde, 0x212d3b, 0x70ebde, 0xa6a6a6, 0x2d3236, 0xb6f6f0,
]);
/// Windows 11's Night sky, from `hc2.theme`.
const NIGHT_SKY: SystemColors = scheme([
    0x000000, 0xffffff, 0xd6b4fd, 0x2b2b2b, 0x8080ff, 0xa6a6a6, 0x000000, 0xffee32,
]);
/// Windows 10's High Contrast Black.
const HIGH_CONTRAST_BLACK: SystemColors = scheme([
    0x000000, 0xffffff, 0x1aebff, 0x000000, 0xffff00, 0x3ff23f, 0x000000, 0xffffff,
]);

const SCHEMES: [(&str, SystemColors); 5] = [
    ("Aquatic", AQUATIC),
    ("Desert", DESERT),
    ("Dusk", DUSK),
    ("Night sky", NIGHT_SKY),
    ("High Contrast Black", HIGH_CONTRAST_BLACK),
];

#[test]
fn a_contrast_theme_gives_its_own_colours_to_the_palette() {
    let p = Tokens::from_system_colors(&AQUATIC).palette;
    let c = AQUATIC;
    assert!(p.dark);
    for (name, field) in [
        ("page", p.page),
        ("title", p.title),
        ("bar", p.bar),
        ("pane", p.pane),
        ("field", p.field),
        ("popup", p.popup),
    ] {
        assert_eq!(field, c.window, "{name}");
    }
    for (name, field) in [
        ("text", p.text),
        ("muted", p.muted),
        ("folder", p.folder),
        ("popup_line", p.popup_line),
    ] {
        assert_eq!(field, c.window_text, "{name}");
    }
    assert_eq!(p.faint, c.gray_text);
    assert_eq!(p.line, c.gray_text);
    assert_eq!(p.thumb, c.gray_text);
    assert_eq!(p.accent, c.highlight);
    assert_eq!(p.on_accent, c.highlight_text);
    // What no system colour names is the toolkit's dark high-contrast palette's.
    assert_eq!(p.danger, HIGH_CONTRAST_DARK.danger);
    assert_eq!(p.warn, HIGH_CONTRAST_DARK.warn);
    assert_eq!(p.warn_bg, HIGH_CONTRAST_DARK.warn_bg);
    assert_eq!(p.find_mark, HIGH_CONTRAST_DARK.find_mark);
    assert_eq!(p.find_current, HIGH_CONTRAST_DARK.find_current);
    assert_eq!(Tokens::from_system_colors(&AQUATIC).sizes, Sizes::DEFAULT);

    // Windows 10's High Contrast Black, field by field where the theme names it.
    let b = Tokens::from_system_colors(&HIGH_CONTRAST_BLACK).palette;
    assert_eq!(b.page, [0, 0, 0]);
    assert_eq!(b.bar, [0, 0, 0]);
    assert_eq!(b.text, [255, 255, 255]);
    assert_eq!(b.faint, [0x3f, 0xf2, 0x3f]);
    assert_eq!(b.line, [0x3f, 0xf2, 0x3f]);
    assert_eq!(b.accent, [0x1a, 0xeb, 0xff]);
    assert_eq!(b.on_accent, [0, 0, 0]);

    // Desert is light, whatever the desktop's choice, and takes the light palette's marks.
    let d = Tokens::from_system_colors(&DESERT).palette;
    assert!(!d.dark);
    assert_eq!(d.page, DESERT.window);
    assert_eq!(d.text, DESERT.window_text);
    assert_eq!(d.accent, DESERT.highlight);
    assert_eq!(d.danger, HIGH_CONTRAST_LIGHT.danger);
}

#[test]
fn every_contrast_theme_keeps_its_contrast_and_the_made_fills_read() {
    for (name, c) in SCHEMES {
        let p = Tokens::from_system_colors(&c).palette;
        // The theme's own pairs are kept exactly, so their contrast is the theme's.
        assert_eq!(
            contrast(p.text, p.page),
            contrast(c.window_text, c.window),
            "{name}"
        );
        assert!(contrast(p.text, p.page) >= 7.0, "{name}: text on page");
        assert_eq!(
            contrast(p.on_accent, p.accent),
            contrast(c.highlight_text, c.highlight)
        );
        assert!(
            contrast(p.text, p.bar) >= SYSTEM_FILL_CONTRAST,
            "{name}: text on bar"
        );
        for (fill, colour) in [
            ("hover", p.hover),
            ("press", p.press),
            ("on", p.on),
            ("selection", p.selection),
        ] {
            assert!(
                contrast(p.text, colour) >= SYSTEM_FILL_CONTRAST - 0.01,
                "{name}: text on {fill}: {}",
                contrast(p.text, colour)
            );
            assert_ne!(colour, p.page, "{name}: {fill} is seen on the page");
        }
        // The toolkit draws more than `text` on a made fill: a selected row's hint and the
        // letters a filter matched, in `faint` and `accent` (palette.rs), and a hovered row's
        // hint in `faint`. They read at the same target as `text`.
        for (ground, fill, colour_name, colour) in [
            (p.on, "on", "faint", p.faint),
            (p.on, "on", "accent", p.accent),
            (p.hover, "hover", "faint", p.faint),
            (p.hover, "hover", "accent", p.accent),
        ] {
            assert!(
                contrast(colour, ground) >= SYSTEM_FILL_CONTRAST - 0.01,
                "{name}: {colour_name} on {fill}: {}",
                contrast(colour, ground)
            );
        }
        // `danger` and `warn` are icons and lines as well as text, so 3:1 against the window
        // is enough for them.
        for (colour_name, colour) in [("danger", p.danger), ("warn", p.warn)] {
            assert!(
                contrast(colour, p.page) >= SYSTEM_LINE_CONTRAST - 0.01,
                "{name}: {colour_name} on the window: {}",
                contrast(colour, p.page)
            );
        }
        assert!(
            contrast(p.line, p.page) >= SYSTEM_LINE_CONTRAST,
            "{name}: line"
        );
        assert!(contrast(p.faint, p.page) >= 4.5, "{name}: faint");
        assert!(contrast(p.accent, p.page) >= 4.5, "{name}: accent");
        // A press shows more than a hover.
        assert!(
            contrast(p.press, p.page) > contrast(p.hover, p.page),
            "{name}"
        );
    }
    // The toolkit's own high-contrast palettes make no fill; theirs are given, and must read
    // for the same colours.
    for (name, p) in [
        ("high contrast light", HIGH_CONTRAST_LIGHT),
        ("high contrast dark", HIGH_CONTRAST_DARK),
    ] {
        for (ground, fill, colour_name, colour) in [
            (p.on, "on", "faint", p.faint),
            (p.on, "on", "accent", p.accent),
            (p.hover, "hover", "faint", p.faint),
            (p.hover, "hover", "accent", p.accent),
        ] {
            assert!(
                contrast(colour, ground) >= SYSTEM_FILL_CONTRAST,
                "{name}: {colour_name} on {fill}: {}",
                contrast(colour, ground)
            );
        }
    }
}

#[test]
fn a_button_face_or_grey_text_that_does_not_read_is_not_used_for_the_chrome_or_lines() {
    let poor = SystemColors {
        // White text on a light grey button face: 1.6:1.
        button_face: rgb(0xcccccc),
        // A grey line on black at 1.3:1.
        gray_text: rgb(0x202020),
        ..NIGHT_SKY
    };
    let p = Tokens::from_system_colors(&poor).palette;
    assert_eq!(p.title, poor.window);
    assert_eq!(p.bar, poor.window);
    assert_eq!(p.line, poor.window_text);
    assert_eq!(p.thumb, poor.window_text);
    // The disabled text stays the theme's.
    assert_eq!(p.faint, poor.gray_text);
    // A button face the text reads on is the chrome's.
    let face = SystemColors {
        button_face: rgb(0x1c1c40),
        ..NIGHT_SKY
    };
    assert_eq!(Tokens::from_system_colors(&face).palette.bar, rgb(0x1c1c40));
    assert_eq!(
        Tokens::from_system_colors(&face).palette.page,
        NIGHT_SKY.window
    );
}

#[test]
fn nonsense_colours_give_a_palette_and_no_panic() {
    for c in [scheme([0; 8]), scheme([0xffffff; 8]), scheme([0x808080; 8])] {
        let p = Tokens::from_system_colors(&c).palette;
        assert_eq!(p.page, c.window);
        assert_eq!(p.text, c.window_text);
    }
}

#[test]
fn a_fill_that_no_colour_reads_on_is_the_best_the_way_gives() {
    // White, black and a mid grey laid on one fill: no ground reads at 4.5 for all three
    // (white and black alone leave only a narrow band of luminance, and the grey sits outside
    // it), so the answer is the place where the worst of them reads best.
    let read = [[255, 255, 255], [0, 0, 0], [128, 128, 128]];
    let start = [128, 128, 128];
    let worst = |c: Rgb| {
        read.iter()
            .map(|&r| contrast(r, c))
            .fold(f32::INFINITY, f32::min)
    };
    let fill = legible_fill(start, &read, SYSTEM_FILL_CONTRAST);
    assert!(worst(fill) < SYSTEM_FILL_CONTRAST);
    // Nothing towards either end of the way reads better than the answer does.
    for toward in [[0, 0, 0], [255, 255, 255]] {
        for step in 0..=64 {
            let t = step as f32 / 64.0;
            let w = worst(mix(fill, toward, t));
            assert!(
                w <= worst(fill) + 0.001,
                "{toward:?} at {t}: {w} beats {}",
                worst(fill)
            );
        }
    }
    // No colours to read for leaves the fill as it is.
    assert_eq!(legible_fill(start, &[], SYSTEM_FILL_CONTRAST), start);
}

#[test]
fn without_the_systems_colours_high_contrast_is_the_toolkits_own() {
    assert_eq!(
        Tokens::high_contrast_with(None, true),
        Tokens::high_contrast(true)
    );
    assert_eq!(
        Tokens::high_contrast_with(None, false),
        Tokens::high_contrast(false)
    );
    // With them, dark or light is the theme's, not the caller's.
    assert_eq!(
        Tokens::high_contrast_with(Some(&DESERT), true),
        Tokens::from_system_colors(&DESERT)
    );
    assert_eq!(
        Tokens::high_contrast_with(Some(&AQUATIC), false),
        Tokens::from_system_colors(&AQUATIC)
    );
}

#[test]
fn under_high_contrast_no_backdrop_is_asked_for() {
    for want in Backdrop::ALL {
        assert_eq!(allowed_backdrop(want, true), Backdrop::Off);
        assert_eq!(allowed_backdrop(want, false), want);
    }
}

/// The backdrop rule given to a [`crate::frost::Frost`]: a switch to high contrast while the
/// window has the keyboard takes the backdrop away at the next frame, and back again after.
#[test]
fn a_switch_to_high_contrast_turns_the_frost_off_at_once() {
    use crate::frost::{Frost, System};
    struct Sys(Vec<Backdrop>);
    impl System for Sys {
        fn available(&mut self) -> bool {
            true
        }
        fn set(&mut self, b: Backdrop) -> bool {
            self.0.push(b);
            true
        }
    }
    let mut sys = Sys(Vec::new());
    let mut frost = Frost::default();
    let pal = DARK;
    frost.sync(
        allowed_backdrop(Backdrop::Mica, false),
        true,
        &pal,
        &mut sys,
    );
    assert!(frost.key().is_some());
    let hc = Tokens::from_system_colors(&AQUATIC).palette;
    assert!(frost.sync(allowed_backdrop(Backdrop::Mica, true), true, &hc, &mut sys));
    assert!(frost.key().is_none());
    frost.sync(
        allowed_backdrop(Backdrop::Mica, false),
        true,
        &pal,
        &mut sys,
    );
    assert!(frost.key().is_some());
    assert_eq!(sys.0, [Backdrop::Mica, Backdrop::Off, Backdrop::Mica]);
}

/// A sheet for a person to judge: `cargo test -p mdshared-ui tokens_sheet -- --ignored` writes
/// `target/specimen/tokens.png`: for the toolkit's palettes, the high-contrast ones, the five
/// system contrast themes of the tests, and four accents in light and dark, a link, a button in the accent, a selected row and selected text.
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
        for (name, c) in SCHEMES {
            v.push((name.to_lowercase(), Tokens::from_system_colors(&c).palette));
        }
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

#[test]
fn a_selected_row_stands_out_and_reads_under_every_contrast_theme() {
    for (name, colors) in SCHEMES {
        let pal = Tokens::from_system_colors(&colors).palette;
        let (fill, text) = selected(&pal, true);
        assert_eq!(
            (fill, text),
            (colors.highlight, colors.highlight_text),
            "{name}"
        );
        assert!(contrast(text, fill) >= 4.5, "{name}: text on the selection");
        assert!(
            contrast(fill, pal.page) > contrast(pal.on, pal.page),
            "{name}: the selection stands further from the window than `on`"
        );
    }
    assert_eq!(selected(&LIGHT, false), (LIGHT.on, LIGHT.text));
    assert_eq!(
        selected(&HIGH_CONTRAST_DARK, false),
        (HIGH_CONTRAST_DARK.accent, HIGH_CONTRAST_DARK.on_accent)
    );
}
