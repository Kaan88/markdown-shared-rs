use super::*;
use crate::cover::contrast;
use crate::gfx::{Gfx, Painter, Rect, mix};
use crate::menu::{self, Item, Menu};
use crate::tokens::{ACCENT_CONTRAST, legible};
use mdshared_text::Bitmap;

fn custom() -> Appearance {
    let mut a = Appearance::DEFAULT;
    a.scheme = Scheme::Warm;
    a.accent = AccentFrom::Custom([0x12, 0xab, 0xef]);
    a.radius = 6.0;
    a.shadow = 0.6;
    a.menu_row = 26.0;
    a.joined = true;
    a.motion = Speed::Snappy;
    a.submenu_delay_ms = 200;
    a
}

#[test]
fn the_default_is_the_toolkits_own_look() {
    let d = Appearance::DEFAULT;
    assert_eq!(Appearance::default(), d);
    assert_eq!(d.scheme.palette(), DARK);
    assert_eq!(d.palette(&DARK, Some([255, 0, 0]), Some([0, 255, 0])), DARK);
    assert_eq!(d.menu_look(), Look::ROUNDED);
    assert_eq!(d.submenu_delay(), crate::submenu::DELAY);
    assert_eq!(d.motion, Speed::Fluent);
    assert!(!d.joined);
    assert_eq!(d.sane(), d);
}

#[test]
fn the_themes_accent_and_a_source_with_no_colour_give_the_palette_unchanged() {
    for scheme in Scheme::ALL {
        let base = scheme.palette();
        assert_eq!(
            AccentFrom::Theme.apply(&base, Some([200, 30, 30]), Some([1, 2, 3])),
            base
        );
        // The system gives none; no picture is showing.
        assert_eq!(AccentFrom::System.apply(&base, None, Some([1, 2, 3])), base);
        assert_eq!(AccentFrom::Cover.apply(&base, Some([1, 2, 3]), None), base);
        // And so the same pixels: a menu drawn with each is the same menu.
        let drawn = |pal: &Palette| {
            let mut bmp = Bitmap::new(260, 200);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, 260.0, 200.0), pal.page);
            let items = vec![
                Item::new(0u8, "Shuffle").checked(true),
                Item::new(1, "Repeat"),
            ];
            let mut menu = Menu::new(items, 10.0, 10.0, false);
            menu.hover = Some(1);
            menu.draw(&mut p, pal, 260.0, 200.0);
            bmp.pixels
        };
        let mut a = Appearance::DEFAULT;
        a.accent = AccentFrom::Cover;
        assert!(drawn(&a.palette(&base, Some([9, 9, 9]), None)) == drawn(&base));
    }
}

#[test]
fn an_accent_that_has_a_colour_is_made_legible_on_the_page() {
    let navy = [0x10, 0x20, 0x40];
    for (source, system, cover) in [
        (AccentFrom::System, Some(navy), None),
        (AccentFrom::Cover, None, Some(navy)),
        (AccentFrom::Custom(navy), None, None),
    ] {
        let pal = source.apply(&DARK, system, cover);
        assert_eq!(pal, accented(&DARK, navy), "{source:?}");
        assert_ne!(
            pal.accent, navy,
            "a navy accent is lightened on a dark page"
        );
        assert!(contrast(pal.accent, DARK.page) >= ACCENT_CONTRAST);
        // Everything but the accent's own colours is the base's.
        assert_eq!(
            Palette {
                accent: DARK.accent,
                on_accent: DARK.on_accent,
                on: DARK.on,
                selection: DARK.selection,
                ..pal
            },
            DARK
        );
    }
    // A pale accent on the warm page is darkened.
    let pale = [0xff, 0xe0, 0xa0];
    let pal = AccentFrom::Custom(pale).apply(&WARM, None, None);
    assert!(contrast(pal.accent, WARM.page) >= ACCENT_CONTRAST);
    let a = Appearance {
        accent: AccentFrom::Custom(pale),
        ..Appearance::DEFAULT
    };
    assert_eq!(a.palette(&WARM, None, None), pal);
}

#[test]
fn the_warm_and_cool_palettes_take_the_prototypes_colours_and_read_on_their_pages() {
    for (pal, ink, hover, press) in [
        (WARM, [60, 40, 20], 0.06, 0.1),
        (COOL, [20, 40, 70], 0.055, 0.1),
    ] {
        assert!(!pal.dark);
        assert_eq!((pal.title, pal.pane), (pal.bar, pal.bar));
        // The accent and the colours not named are the light palette's.
        assert_eq!(pal.accent, LIGHT.accent);
        assert_eq!(pal.on_accent, LIGHT.on_accent);
        assert_eq!(
            (pal.danger, pal.warn, pal.folder),
            (LIGHT.danger, LIGHT.warn, LIGHT.folder)
        );
        // The selected row and the selection are made from the accent as `accented` makes them.
        assert_eq!(pal.on, mix(pal.pane, pal.accent, 0.16));
        assert_eq!(
            pal.selection,
            legible(mix(pal.page, pal.accent, 0.75), pal.text, 3.0)
        );
        // Hover and press are the prototype's translucent ink over the bar, within a level.
        let near = |a: Rgb, b: Rgb| (0..3).all(|i| (a[i] as i32 - b[i] as i32).abs() <= 1);
        assert!(near(pal.hover, mix(pal.bar, ink, hover)), "{:?}", pal.hover);
        assert!(near(pal.press, mix(pal.bar, ink, press)), "{:?}", pal.press);
        // Text, the accent and quiet text read.
        assert!(contrast(pal.text, pal.page) >= 7.0);
        assert!(contrast(pal.accent, pal.page) >= ACCENT_CONTRAST);
        assert!(contrast(pal.muted, pal.page) >= 4.5);
        assert!(contrast(pal.text, pal.selection) >= 3.0);
    }
    assert_eq!(Scheme::Warm.palette(), WARM);
    assert_eq!(Scheme::Cool.palette(), COOL);
    assert_eq!(Scheme::Light.palette(), LIGHT);
}

#[test]
fn names_round_trip_and_nothing_else_is_a_name() {
    for s in Scheme::ALL {
        assert_eq!(Scheme::from_name(s.name()), Some(s));
        assert_eq!(
            Scheme::from_name(&format!(" {} ", s.label().to_uppercase())),
            Some(s)
        );
        assert_eq!(s.is_dark(), s == Scheme::Dark);
    }
    for s in Speed::ALL {
        assert_eq!(Speed::from_name(s.name()), Some(s));
        assert_eq!(Speed::from_name(s.label()), Some(s));
    }
    let colours = [[0, 0, 0], [255, 255, 255], [0x12, 0xab, 0xef]];
    for a in AccentFrom::SOURCES
        .into_iter()
        .chain(colours.map(AccentFrom::Custom))
    {
        assert_eq!(AccentFrom::from_name(&a.name()), Some(a));
    }
    assert_eq!(AccentFrom::Custom([0x12, 0xab, 0xef]).name(), "#12abef");
    assert_eq!(
        AccentFrom::from_name(" #12ABef "),
        Some(AccentFrom::Custom([0x12, 0xab, 0xef]))
    );
    assert_eq!(AccentFrom::from_name("SYSTEM"), Some(AccentFrom::System));
    for bad in [
        "",
        "#",
        "#abc",
        "#12345",
        "#1234567",
        "123456",
        "#12345g",
        "#+12345",
        "#-12345",
        "teal",
        "#１２３４５６",
        "theme2",
    ] {
        assert_eq!(AccentFrom::from_name(bad), None, "{bad:?}");
    }
    for bad in ["", "darkish", "dar k", "nord"] {
        assert_eq!(Scheme::from_name(bad), None, "{bad:?}");
        assert_eq!(Speed::from_name(bad), None, "{bad:?}");
    }
    let labels: Vec<_> = Scheme::ALL.iter().map(|s| s.label()).collect();
    assert_eq!(labels, ["Dark", "Light", "Warm", "Cool"]);
    let names: Vec<_> = Speed::ALL.iter().map(|s| s.name()).collect();
    assert_eq!(names, ["off", "snappy", "fluent", "soft"]);
}

#[test]
fn speed_scales_the_fluent_timings_exactly_and_never_overflows() {
    use crate::anim::fluent;
    let factors: Vec<f32> = Speed::ALL.iter().map(|s| s.factor()).collect();
    assert_eq!(factors, [0.0, 0.66, 1.0, 1.5]);
    for d in [fluent::FASTER, fluent::FAST, fluent::NORMAL, fluent::SLOW] {
        assert_eq!(Speed::Fluent.scale(d), d);
        assert_eq!(Speed::Off.scale(d), Duration::ZERO);
    }
    assert_eq!(
        Speed::Snappy.scale(fluent::FAST),
        Duration::from_micros(110_220)
    );
    assert_eq!(
        Speed::Soft.scale(fluent::FAST),
        Duration::from_micros(250_500)
    );
    assert_eq!(Speed::Soft.scale(Duration::MAX), Duration::MAX);
    assert!(Speed::Snappy.scale(Duration::MAX) < Duration::MAX);
    assert_eq!(Speed::Soft.scale_ms(200), 300);
    assert_eq!(Speed::Snappy.scale_ms(250), 165);
    assert_eq!(Speed::Off.scale_ms(250), 0);
    assert_eq!(Speed::Soft.scale_ms(u64::MAX), u64::MAX);
    assert_eq!(Speed::Fluent.scale_ms(u64::MAX), u64::MAX);
    assert_eq!(Speed::Off.policy(), Policy::Never);
    for s in [Speed::Snappy, Speed::Fluent, Speed::Soft] {
        assert_eq!(s.policy(), Policy::System);
    }
}

#[test]
fn sane_holds_every_number_to_its_range_and_puts_back_what_is_not_a_number() {
    let mut odd = Appearance::DEFAULT;
    odd.radius = 99.0;
    odd.shadow = -1.0;
    odd.menu_row = 4.0;
    odd.submenu_delay_ms = u32::MAX;
    let s = odd.sane();
    assert_eq!(
        (s.radius, s.shadow, s.menu_row),
        (MAX_RADIUS, 0.0, MIN_MENU_ROW)
    );
    assert_eq!(s.submenu_delay_ms, MAX_SUBMENU_DELAY_MS);
    odd.radius = f32::NAN;
    odd.shadow = f32::INFINITY;
    odd.menu_row = f32::NEG_INFINITY;
    odd.shadow = f32::NAN;
    let s = odd.sane();
    let d = Appearance::DEFAULT;
    assert_eq!(
        (s.radius, s.shadow, s.menu_row),
        (d.radius, d.shadow, d.menu_row)
    );
    // What is in range, and the choices, are kept.
    assert_eq!(custom().sane(), custom());
    // The menu's look and the delay are made from the sane numbers.
    odd.menu_row = 1000.0;
    assert_eq!(odd.menu_look().row, MAX_MENU_ROW);
    assert_eq!(odd.menu_look().radius, d.radius);
    assert_eq!(odd.submenu_delay(), Duration::from_millis(2000));
}

#[test]
fn the_menus_look_takes_the_radius_row_and_shadow() {
    let a = custom();
    let look = a.menu_look();
    assert_eq!(
        (look.radius, look.row, look.shadow_strength),
        (6.0, 26.0, 0.6)
    );
    assert!(look.shadow);
    // The highlight's corners follow the menu's, two less and at most five.
    assert_eq!(look.hover_radius, 4.0);
    let at = |r: f32| {
        let mut a = Appearance::DEFAULT;
        a.radius = r;
        a.menu_look()
    };
    assert_eq!(at(2.0).hover_radius, 0.0);
    assert_eq!(at(16.0).hover_radius, Look::ROUNDED.hover_radius);
    assert_eq!(at(16.0).inset, Look::ROUNDED.inset);
    // Square: the flat look's highlight, across the row inside the line.
    let square = at(0.0);
    assert_eq!(
        (square.radius, square.hover_radius, square.inset),
        (0.0, 0.0, Look::FLAT.inset)
    );
    // No shadow at 0.
    let mut flat = Appearance::DEFAULT;
    flat.shadow = 0.0;
    assert!(!flat.menu_look().shadow);
    // The rest is the rounded look's.
    assert_eq!(
        (look.pad, look.separator, look.gap),
        (
            Look::ROUNDED.pad,
            Look::ROUNDED.separator,
            Look::ROUNDED.gap
        )
    );
    assert_eq!(look, look.sane());
}

#[test]
fn an_appearance_written_reads_back_the_same() {
    let a = custom();
    let pairs = a.write();
    let keys: Vec<_> = pairs.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, KEYS);
    assert_eq!(pairs[1].1, "#12abef");
    assert_eq!(pairs[3].1, "0.6");
    assert_eq!(pairs[5].1, "on");
    let mut back = Appearance::DEFAULT;
    for (k, v) in &pairs {
        assert!(back.read(k, v), "{k}");
    }
    assert_eq!(back, a);
    // Through the text, with the application's own keys beside them.
    let mut all = pairs.clone();
    all.push(("columns".into(), "title artist".into()));
    let text = to_text("mdplay-look", 3, &all);
    let (version, read) = parse(&text, "mdplay-look").expect("its own text");
    assert_eq!(version, 3);
    let mut back = Appearance::DEFAULT;
    let rest = back.read_all(read);
    assert_eq!(back, a);
    assert_eq!(rest, [("columns".to_string(), "title artist".to_string())]);
    // The default, written and read, is the default.
    let mut d = custom();
    let _ = d.read_all(Appearance::DEFAULT.write());
    assert_eq!(d, Appearance::DEFAULT);
}

#[test]
fn reading_a_bad_value_leaves_the_field_and_an_unknown_key_is_not_read() {
    let mut a = custom();
    assert!(!a.read("colour", "dark"));
    assert!(!a.read("Scheme", "dark"), "keys are exact");
    assert_eq!(a, custom());
    for (k, v) in [
        ("scheme", "nord"),
        ("accent", "#12"),
        ("radius", "round"),
        ("radius", "NaN"),
        ("radius", "inf"),
        ("shadow", ""),
        ("menu_row", "-"),
        ("joined", "maybe"),
        ("motion", "warp"),
        ("submenu_delay", "-5"),
        ("submenu_delay", "1.5"),
    ] {
        assert!(a.read(k, v), "{k} is its own key");
        assert_eq!(a, custom(), "{k}={v}");
    }
    // Out of range is held to it; white space round a value is ignored.
    assert!(a.read("radius", " 40 "));
    assert_eq!(a.radius, MAX_RADIUS);
    assert!(a.read("menu_row", "1"));
    assert_eq!(a.menu_row, MIN_MENU_ROW);
    assert!(a.read("shadow", "7"));
    assert_eq!(a.shadow, MAX_SHADOW);
    assert!(a.read("submenu_delay", "99999999999999"));
    assert_eq!(a.submenu_delay_ms, MAX_SUBMENU_DELAY_MS);
    for (v, on) in [
        ("off", false),
        ("TRUE", true),
        ("0", false),
        ("1", true),
        ("On", true),
    ] {
        assert!(a.read("joined", v));
        assert_eq!(a.joined, on, "{v}");
    }
}

#[test]
fn the_text_round_trips_any_value_and_keeps_unknown_keys() {
    let pairs: Vec<(String, String)> = [
        ("scheme", "cool"),
        ("note", "two\nlines, 100% and \r a return"),
        ("spaces", "  leading and trailing  "),
        ("empty", ""),
        ("unicode", "Çıkış ✓ 🎵"),
        ("later.key", "from a later version"),
        ("scheme", "warm"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let text = to_text("look", 1, &pairs);
    assert!(text.starts_with("look 1\n"));
    assert_eq!(text.lines().count(), 1 + pairs.len());
    let (version, back) = parse(&text, "look").unwrap();
    assert_eq!((version, &back), (1, &pairs));
    // Line ends of another system, a byte-order mark and blank lines.
    let crlf = format!("\u{feff}\r\n{}", text.replace('\n', "\r\n\r\n"));
    assert_eq!(parse(&crlf, "look"), Some((1, pairs.clone())));
    // A later pair wins when read.
    let mut a = Appearance::DEFAULT;
    let rest = a.read_all(back);
    assert_eq!(a.scheme, Scheme::Warm);
    assert_eq!(rest.len(), 5);
    // Keys that could not be read back are not written.
    let long_key = "k".repeat(MAX_KEY + 1);
    let odd: Vec<(String, String)> = [
        "",
        "two words",
        "tab\there",
        "line\nbreak",
        long_key.as_str(),
    ]
    .iter()
    .map(|k| (k.to_string(), "v".to_string()))
    .chain([("big".to_string(), "x".repeat(MAX_VALUE + 1))])
    .chain([("ok".to_string(), "v".to_string())])
    .collect();
    let text = to_text("look", 2, &odd);
    assert_eq!(
        parse(&text, "look"),
        Some((2, vec![("ok".into(), "v".into())]))
    );
    // At most MAX_PAIRS, written or read.
    let many: Vec<(String, String)> = (0..MAX_PAIRS + 10)
        .map(|i| (format!("k{i}"), i.to_string()))
        .collect();
    let (_, back) = parse(&to_text("look", 1, &many), "look").unwrap();
    assert_eq!(back.len(), MAX_PAIRS);
    let mut long = String::from("look 1\n");
    for i in 0..MAX_PAIRS + 10 {
        long.push_str(&format!("k{i} {i}\n"));
    }
    assert_eq!(parse(&long, "look").unwrap().1.len(), MAX_PAIRS);
}

#[test]
fn a_text_that_is_not_its_own_is_none() {
    let text = to_text("look", 1, &Appearance::DEFAULT.write());
    assert!(parse(&text, "look").is_some());
    for (text, name) in [
        (text.as_str(), "other"),
        (text.as_str(), ""),
        ("", "look"),
        ("\n\n  \n", "look"),
        ("look\nscheme dark\n", "look"),
        ("look one\nscheme dark\n", "look"),
        ("look -1\n", "look"),
        ("look 1 2\n", "look"),
        ("look 99999999999\n", "look"),
        ("scheme dark\nlook 1\n", "look"),
        (" 1\n", ""),
    ] {
        assert_eq!(parse(text, name), None, "{text:?} as {name:?}");
    }
    // A header alone is a text with no pairs.
    assert_eq!(parse("look 7", "look"), Some((7, vec![])));
    // Too long a text is refused whole.
    let mut huge = String::from("look 1\n");
    while huge.len() <= MAX_TEXT {
        huge.push_str("pad xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n");
    }
    assert_eq!(parse(&huge, "look"), None);
}

/// A small LCG with a fixed seed, as elsewhere in the tests.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

#[test]
fn any_text_cut_or_garbled_parses_without_panicking_and_reads_into_a_sane_look() {
    let mut pairs = custom().write();
    pairs.push(("own".into(), "keep me %, please\n".into()));
    let text = to_text("look", 1, &pairs);
    // Cut at every character.
    for (cut, _) in text.char_indices() {
        if let Some((_, back)) = parse(&text[..cut], "look") {
            let mut a = Appearance::DEFAULT;
            let _ = a.read_all(back);
            assert_eq!(a.sane(), a, "cut at {cut}");
        }
    }
    // Garbage: random bytes, random lines of words, the header and noise.
    let mut rng = Lcg(0x5eed);
    let words = [
        "look", "1", "scheme", "accent", "#", "%", "%0", "%ZZ", "radius", "-", "\n", "\r\n", " ",
        "\t", "NaN", "1e39", "joined", "🎵", "\u{feff}",
    ];
    for round in 0..2000 {
        let text: String = if round % 2 == 0 {
            let bytes: Vec<u8> = (0..rng.next() % 300).map(|_| rng.next() as u8).collect();
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            let mut t = String::from("look 1\n");
            for _ in 0..rng.next() % 60 {
                t.push_str(words[(rng.next() % words.len() as u64) as usize]);
            }
            t
        };
        if let Some((_, back)) = parse(&text, "look") {
            assert!(back.len() <= MAX_PAIRS);
            let mut a = Appearance::DEFAULT;
            let _ = a.read_all(back);
            assert_eq!(a.sane(), a);
            let _ = a.menu_look();
        }
    }
}

/// The four schemes, each with a menu joined to the title that opens it, in the toolkit's own
/// look (above) and in mdplay's prototype's (below: 6 px corners, shadows at 0.6, 26 px rows, a
/// custom accent): run with `cargo test -p mdshared-ui appearance_sheet -- --ignored`, then
/// open target/specimen/appearance.png and appearance-150.png.
#[test]
#[ignore]
fn appearance_sheet() {
    let looks = [Appearance::DEFAULT, {
        let mut a = custom();
        a.accent = AccentFrom::Custom([0x00, 0x99, 0x99]);
        a
    }];
    for (scale, file) in [(1.0, "appearance.png"), (1.5, "appearance-150.png")] {
        let (cw, ch) = (270.0, 250.0);
        let (w, h) = (cw * 4.0, ch * looks.len() as f32);
        let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (row, base) in looks.iter().enumerate() {
            for (col, scheme) in Scheme::ALL.into_iter().enumerate() {
                let mut a = *base;
                a.scheme = scheme;
                let pal = a.palette(&scheme.palette(), None, None);
                let look = a.menu_look();
                let (ox, oy) = (col as f32 * cw, row as f32 * ch);
                let cell = Rect::new(ox, oy, cw, ch);
                p.fill(cell, pal.page);
                let bar = Rect::new(ox, oy, cw, 30.0);
                p.fill(bar, pal.bar);
                p.fill(Rect::new(ox, bar.bottom(), cw, 1.0), pal.line);
                let titles = ["File", "Edit", "View", "Playback"];
                let mut x = ox + 6.0;
                let mut tab = Rect::default();
                for (i, t) in titles.iter().enumerate() {
                    let tw = p.measure(crate::gfx::Face::Ui, 12.5, t) + 20.0;
                    let r = Rect::new(x.round(), oy + 3.0, tw.round(), 27.0);
                    if i == 1 {
                        tab = r;
                    } else {
                        p.text(
                            crate::gfx::Face::Ui,
                            12.5,
                            r.x + 10.0,
                            r.cy(),
                            t,
                            pal.text,
                            r.w,
                        );
                    }
                    x += tw;
                }
                let items = vec![
                    Item::new(0u8, "Undo").hint("Ctrl+Z"),
                    Item::new(1, "Cut").hint("Ctrl+X").sep(),
                    Item::new(2, "Copy").hint("Ctrl+C"),
                    Item::new(3, "Follow cursor").checked(true).sep(),
                    Item::new(4, "Selection follows playback").checked(false),
                ];
                let mut menu = Menu::new(items, 0.0, 0.0, false)
                    .around(tab)
                    .with_look(look);
                menu.hover = Some(2);
                menu.place_in(&mut p, cell);
                let joins = menu::joins(tab, menu.rect());
                if joins {
                    menu::join_under(&mut p, &pal, &look, tab);
                }
                menu.draw_in(&mut p, &pal, cell);
                if joins {
                    menu::join_over(&mut p, &pal, &look, tab, menu.rect());
                }
                p.text(
                    crate::gfx::Face::Ui,
                    12.5,
                    tab.x + 10.0,
                    tab.cy(),
                    "Edit",
                    pal.text,
                    tab.w,
                );
                p.text(
                    crate::gfx::Face::Ui,
                    11.0,
                    ox + 8.0,
                    oy + ch - 12.0,
                    &format!(
                        "{} · r {} · shadow {} · row {}",
                        scheme.label(),
                        a.radius,
                        a.shadow,
                        a.menu_row
                    ),
                    pal.muted,
                    cw - 16.0,
                );
            }
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).expect("the specimen folder");
        bmp.write_png(&out.join(file)).expect("the sheet");
    }
}
