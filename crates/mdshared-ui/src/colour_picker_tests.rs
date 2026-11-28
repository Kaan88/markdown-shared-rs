//! The colour picker's model and drawing: the conversions against a second way of computing
//! them and on every grey, the hex in and out, press, drag and keys on the square and the
//! strip, the hue kept through grey and black, cancel, and the drawing's pixels.

use super::*;
use crate::gfx::Gfx;
use crate::theme::{DARK, LIGHT};

const NONE: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: false,
    control: false,
};
const SHIFT: Mods = Mods {
    ctrl: false,
    shift: true,
    alt: false,
    control: false,
};

/// A small LCG with a fixed seed, for samples.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
}

/// HSV to RGB the textbook way, by sector, in f64: the second way the tests check against.
fn by_sector(c: Hsv) -> Rgb {
    let (h, s, v) = (f64::from(c.h), f64::from(c.s), f64::from(c.v));
    let h = if h >= 360.0 { 0.0 } else { h } / 60.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i as i32 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [r, g, b].map(|x| (x * 255.0).round() as u8)
}

fn layout() -> Layout {
    Layout::new(Rect::new(100.0, 50.0, CONTENT_W, CONTENT_H))
}

#[test]
fn every_hex_of_a_dense_sample_and_every_grey_comes_back_as_itself() {
    let mut n = 0;
    for r in (0..=255u8).step_by(3) {
        for g in (0..=255u8).step_by(3) {
            for b in (0..=255u8).step_by(3) {
                let c = [r, g, b];
                assert_eq!(hsv_to_rgb(rgb_to_hsv(c)), c, "{c:?}");
                n += 1;
            }
        }
    }
    // Every value of each channel against random others, and the greys.
    let mut lcg = Lcg(7);
    for k in 0..=255u8 {
        for _ in 0..200 {
            let (a, b) = (lcg.next() as u8, lcg.next() as u8);
            for c in [[k, a, b], [a, k, b], [a, b, k]] {
                assert_eq!(hsv_to_rgb(rgb_to_hsv(c)), c, "{c:?}");
            }
        }
        assert_eq!(hsv_to_rgb(rgb_to_hsv([k; 3])), [k; 3]);
        n += 601;
    }
    assert!(n > 700_000);
}

#[test]
fn hex_to_hsv_to_hex_is_the_same_text_in_its_own_form() {
    let mut lcg = Lcg(11);
    for _ in 0..20_000 {
        let c = [lcg.next() as u8, lcg.next() as u8, lcg.next() as u8];
        for form in [
            HexForm::DEFAULT,
            HexForm {
                hash: false,
                upper: true,
            },
            HexForm {
                hash: true,
                upper: true,
            },
            HexForm {
                hash: false,
                upper: false,
            },
        ] {
            let text = form.format(c);
            let back = form.format(hsv_to_rgb(rgb_to_hsv(parse_hex(&text).unwrap())));
            assert_eq!(back, text);
            if text.bytes().any(|b| b.is_ascii_alphabetic()) {
                assert_eq!(HexForm::of(&text), form, "{text}");
            }
        }
    }
}

#[test]
fn hsv_to_rgb_agrees_with_the_textbook_sectors() {
    let mut lcg = Lcg(3);
    for _ in 0..100_000 {
        let c = Hsv::new(
            (lcg.next() % 36_000) as f32 / 100.0,
            (lcg.next() % 1001) as f32 / 1000.0,
            (lcg.next() % 1001) as f32 / 1000.0,
        );
        let (a, b) = (hsv_to_rgb(c), by_sector(c));
        for k in 0..3 {
            assert!(a[k].abs_diff(b[k]) <= 1, "{c:?}: {a:?} against {b:?}");
        }
    }
    assert_eq!(hsv_to_rgb(Hsv::new(0.0, 1.0, 1.0)), [255, 0, 0]);
    assert_eq!(hsv_to_rgb(Hsv::new(120.0, 1.0, 1.0)), [0, 255, 0]);
    assert_eq!(hsv_to_rgb(Hsv::new(240.0, 1.0, 1.0)), [0, 0, 255]);
    assert_eq!(hsv_to_rgb(Hsv::new(360.0, 1.0, 1.0)), [255, 0, 0]);
    assert_eq!(hsv_to_rgb(Hsv::new(200.0, 0.0, 1.0)), [255; 3]);
    assert_eq!(hsv_to_rgb(Hsv::new(200.0, 1.0, 0.0)), [0; 3]);
}

#[test]
fn nonsense_numbers_make_a_sane_colour() {
    assert_eq!(
        Hsv::new(f32::NAN, f32::NAN, f32::NAN),
        Hsv::new(0.0, 0.0, 0.0)
    );
    assert_eq!(Hsv::new(-10.0, 2.0, -1.0), Hsv::new(350.0, 1.0, 0.0));
    assert_eq!(Hsv::new(370.0, 0.5, 0.5).h, 10.0);
    assert_eq!(Hsv::new(f32::INFINITY, 0.5, 0.5).h, 0.0);
    assert_eq!(Hsv::new(360.0, 0.5, 0.5).h, 360.0);
    let _ = hsv_to_rgb(Hsv {
        h: f32::NAN,
        s: f32::INFINITY,
        v: -3.0,
    });
}

#[test]
fn hexes_are_six_digits_with_or_without_a_hash() {
    assert_eq!(parse_hex("#1a2B3c"), Some([0x1a, 0x2b, 0x3c]));
    assert_eq!(parse_hex("1A2B3C"), Some([0x1a, 0x2b, 0x3c]));
    assert_eq!(parse_hex("  #ffffff "), Some([255; 3]));
    for bad in [
        "", "#", "#fff", "fff", "#1234567", "##123456", "12345g", "#12 456", "ééé",
    ] {
        assert_eq!(parse_hex(bad), None, "{bad:?}");
    }
    assert_eq!(HexForm::of(""), HexForm::DEFAULT);
    assert_eq!(
        HexForm::of("12AB34"),
        HexForm {
            hash: false,
            upper: true
        }
    );
    assert_eq!(
        HexForm::of("#123456"),
        HexForm {
            hash: true,
            upper: false
        }
    );
    assert_eq!(HexForm::DEFAULT.format([1, 2, 255]), "#0102ff");
}

#[test]
fn a_press_on_the_square_takes_saturation_across_and_brightness_up() {
    let l = layout();
    let mut p = Picker::new([255, 0, 0], HexForm::DEFAULT);
    let sq = l.square;
    // The top right is the pure hue, the top left white, the bottom black.
    assert_eq!(p.press(&l, sq.x, sq.y), Outcome::Colour);
    assert_eq!(p.rgb(), [255; 3]);
    assert_eq!(p.held(), Some(Part::Square));
    assert_eq!(p.focus(), Part::Square);
    assert_eq!(p.drag(&l, sq.right(), sq.y), Outcome::Colour);
    assert_eq!(p.rgb(), [255, 0, 0]);
    assert_eq!(p.drag(&l, sq.cx(), sq.bottom()), Outcome::Colour);
    assert_eq!(p.rgb(), [0; 3]);
    // Out of the square the marker stays on its edges.
    p.drag(&l, sq.right() + 500.0, sq.y - 500.0);
    assert_eq!(p.hsv(), Hsv::new(0.0, 1.0, 1.0));
    p.drag(&l, sq.x + sq.w * 0.5, sq.y + sq.h * 0.25);
    let c = p.hsv();
    assert!(
        (c.s - 0.5).abs() < 1e-5 && (c.v - 0.75).abs() < 1e-5,
        "{c:?}"
    );
    assert_eq!(p.hex(), "#bf6060");
    assert_eq!(p.field().text(), "#bf6060");
    assert!(p.release());
    assert!(!p.release());
    assert_eq!(p.drag(&l, sq.x, sq.y), Outcome::None, "nothing held");
}

#[test]
fn a_press_on_the_strip_sets_the_hue_and_keeps_the_rest() {
    let l = layout();
    let mut p = Picker::new([0x80, 0x40, 0x40], HexForm::DEFAULT);
    let before = p.hsv();
    let hue = l.hue;
    assert_eq!(p.press(&l, hue.x + hue.w / 3.0, hue.cy()), Outcome::Colour);
    assert_eq!(p.focus(), Part::Hue);
    let c = p.hsv();
    assert!((c.h - 120.0).abs() < 1e-3, "{c:?}");
    assert_eq!((c.s, c.v), (before.s, before.v));
    assert_eq!(p.rgb(), [0x40, 0x80, 0x40]);
    p.drag(&l, hue.right() + 40.0, hue.cy());
    assert_eq!(p.hsv().h, 360.0);
    p.drag(&l, hue.x - 40.0, hue.y - 100.0);
    assert_eq!(p.hsv().h, 0.0);
    // A press a little outside the strip still takes it.
    p.release();
    assert_eq!(p.press(&l, hue.cx(), hue.y - 3.0), Outcome::Colour);
    assert_eq!(p.held(), Some(Part::Hue));
}

#[test]
fn a_drag_through_grey_and_black_keeps_the_hue_and_comes_back_to_the_colour() {
    let l = layout();
    let mut p = Picker::new([0x20, 0x60, 0xc0], HexForm::DEFAULT);
    let hue = p.hsv().h;
    let sq = l.square;
    p.press(&l, sq.right(), sq.y);
    // To the grey edge, to black, and back.
    p.drag(&l, sq.x, sq.cy());
    assert_eq!(p.rgb()[0], p.rgb()[1]);
    assert_eq!(p.hsv().h, hue);
    p.drag(&l, sq.right(), sq.bottom());
    assert_eq!(p.rgb(), [0; 3]);
    assert_eq!(p.hsv().h, hue);
    assert_eq!(p.hsv().s, 1.0);
    p.drag(&l, sq.right(), sq.y);
    assert_eq!(p.hsv(), Hsv::new(hue, 1.0, 1.0));
    // A grey or black typed in keeps the hue too (and black the saturation).
    p.set_rgb([0x77; 3]);
    assert_eq!(p.hsv().h, hue);
    p.set_rgb([0, 0, 0]);
    assert_eq!((p.hsv().h, p.hsv().s), (hue, 0.0));
}

#[test]
fn the_arrows_move_saturation_brightness_and_hue_shift_by_more() {
    let mut p = Picker::new(hsv_to_rgb(Hsv::new(100.0, 0.5, 0.5)), HexForm::DEFAULT);
    let c = p.hsv();
    p.key(&KeyName::Right, NONE, None);
    assert!((p.hsv().s - (c.s + 0.01)).abs() < 1e-4);
    p.key(&KeyName::Left, SHIFT, None);
    assert!((p.hsv().s - (c.s + 0.01 - 0.1)).abs() < 1e-4);
    p.key(&KeyName::Up, NONE, None);
    assert!((p.hsv().v - (c.v + 0.01)).abs() < 1e-4);
    p.key(&KeyName::Down, SHIFT, None);
    assert!((p.hsv().v - (c.v + 0.01 - 0.1)).abs() < 1e-4);
    assert!((p.hsv().h - c.h).abs() < 1e-4, "the square leaves the hue");
    // On the edge a key changes nothing.
    for _ in 0..20 {
        p.key(&KeyName::Right, SHIFT, None);
    }
    assert_eq!(p.hsv().s, 1.0);
    assert_eq!(p.key(&KeyName::Right, NONE, None), Outcome::None);
    // The strip: round the circle.
    p.tab(false);
    assert_eq!(p.focus(), Part::Hue);
    let h = p.hsv().h;
    p.key(&KeyName::Right, NONE, None);
    assert!((p.hsv().h - (h + 1.0)).abs() < 1e-3);
    p.key(&KeyName::Down, SHIFT, None);
    assert!((p.hsv().h - (h - 9.0)).abs() < 1e-3);
    p.set_hsv(Hsv::new(2.0, 1.0, 1.0));
    p.key(&KeyName::Left, SHIFT, None);
    assert!((p.hsv().h - 352.0).abs() < 1e-3);
    p.set_hsv(Hsv::new(360.0, 1.0, 1.0));
    p.key(&KeyName::Up, NONE, None);
    assert!((p.hsv().h - 1.0).abs() < 1e-3);
    assert_eq!(p.key(&KeyName::Home, NONE, None), Outcome::Unused);
    // Every change of colour by a key is reported as one, and the hex follows.
    p.tab(true);
    let before = p.hex();
    assert_eq!(p.key(&KeyName::Down, SHIFT, None), Outcome::Colour);
    assert_ne!(p.hex(), before);
    assert_eq!(p.field().text(), p.hex());
}

#[test]
fn tab_goes_round_the_three_parts_and_enter_and_escape_close() {
    let mut p = Picker::new([1, 2, 3], HexForm::DEFAULT);
    let mut seen = vec![p.focus()];
    for _ in 0..3 {
        assert_eq!(p.key(&KeyName::Tab, NONE, None), Outcome::Redraw);
        seen.push(p.focus());
    }
    assert_eq!(seen, [Part::Square, Part::Hue, Part::Hex, Part::Square]);
    p.key(&KeyName::Tab, SHIFT, None);
    assert_eq!(p.focus(), Part::Hex);
    assert_eq!(p.key(&KeyName::Enter, NONE, None), Outcome::Accept);
    assert_eq!(p.key(&KeyName::Escape, NONE, None), Outcome::Cancel);
}

#[test]
fn typing_a_hex_moves_the_markers_and_a_part_waits() {
    let mut p = Picker::new([0x12, 0x34, 0x56], HexForm::DEFAULT);
    p.set_focus(Part::Hex);
    p.field_mut().select_all();
    for ch in "#ff8000".chars() {
        let s = ch.to_string();
        let o = p.key(&KeyName::Char(ch), NONE, Some(&s));
        if ch == '0' && p.field().text() == "#ff8000" {
            assert_eq!(o, Outcome::Colour);
        } else {
            assert_eq!(o, Outcome::Redraw, "{:?}", p.field().text());
            assert_eq!(p.rgb(), [0x12, 0x34, 0x56], "a part of a hex waits");
        }
    }
    assert_eq!(p.rgb(), [0xff, 0x80, 0x00]);
    assert!((p.hsv().h - 30.0).abs() < 0.2);
    // Typed in another form, the colour is still written in the picker's.
    p.field_mut().select_all();
    for ch in "00FF00".chars() {
        let s = ch.to_string();
        p.key(&KeyName::Char(ch), NONE, Some(&s));
    }
    assert_eq!(p.hex(), "#00ff00");
    assert_eq!(p.field().text(), "00FF00", "the field keeps what was typed");
    // Left with half a hex, the field shows the colour again.
    p.key(&KeyName::Backspace, NONE, None);
    assert_eq!(p.rgb(), [0, 0xff, 0], "half a hex changes nothing");
    p.tab(false);
    assert_eq!(p.field().text(), "#00ff00");
}

#[test]
fn cancel_goes_back_to_the_colour_and_the_markers_it_opened_with() {
    let l = layout();
    let mut p = Picker::new([0x33, 0x66, 0x99], HexForm::of("336699"));
    let at = p.hsv();
    p.press(&l, l.square.x, l.square.bottom());
    p.release();
    assert!(p.cancel());
    assert_eq!(p.hsv(), at);
    assert_eq!(p.hex(), "336699");
    assert!(!p.cancel(), "already there");
    // The old swatch is a cancel too.
    p.press(&l, l.hue.cx(), l.hue.cy());
    p.release();
    assert_eq!(p.press(&l, l.old.cx(), l.old.cy()), Outcome::Colour);
    assert_eq!(p.rgb(), [0x33, 0x66, 0x99]);
    assert_eq!(p.press(&l, l.old.cx(), l.old.cy()), Outcome::Redraw);
    assert_eq!(p.press(&l, l.hex.cx(), l.hex.cy()), Outcome::Redraw);
    assert_eq!(p.focus(), Part::Hex);
    assert_eq!(p.press(&l, l.square.x - 50.0, l.square.y), Outcome::None);
}

#[test]
fn from_text_opens_on_the_text_or_the_fallback() {
    let p = Picker::from_text("#ABCDEF", [1, 1, 1]);
    assert_eq!(p.rgb(), [0xab, 0xcd, 0xef]);
    assert_eq!(p.hex(), "#ABCDEF");
    let p = Picker::from_text("red", [9, 9, 9]);
    assert_eq!(p.rgb(), [9, 9, 9]);
    assert_eq!(p.opened(), [9, 9, 9]);
}

#[test]
fn nonsense_points_and_layouts_do_not_panic() {
    let mut p = Picker::new([10, 20, 30], HexForm::DEFAULT);
    for l in [
        layout(),
        Layout::new(Rect::new(f32::NAN, 0.0, 10.0, 10.0)),
        Layout::new(Rect::new(0.0, 0.0, 0.0, 0.0)),
        Layout::new(Rect::new(0.0, 0.0, -5.0, f32::INFINITY)),
        Layout::new(Rect::new(0.0, 0.0, 3.0, 3.0)),
    ] {
        for (x, y) in [
            (f32::NAN, 0.0),
            (0.0, f32::INFINITY),
            (0.0, 0.0),
            (1.0, 1.0),
            (-1e30, 1e30),
        ] {
            p.press(&l, x, y);
            p.drag(&l, x, y);
            p.release();
        }
    }
    let c = p.hsv();
    assert!(c.h.is_finite() && c.s.is_finite() && c.v.is_finite());
}

fn drawn(pal: &Palette, scale: f32, picker: &mut Picker, cache: &mut Cache, look: Look) -> Bitmap {
    let mut bmp = Bitmap::new((300.0 * scale) as u32, (300.0 * scale) as u32);
    bmp.clear([40, 90, 140]);
    let mut gfx = Gfx::new();
    let card = Rect::new(
        20.0,
        20.0,
        CONTENT_W + 2.0 * PADDING,
        CONTENT_H + 2.0 * PADDING,
    );
    let l = Layout::new(card.inset(PADDING));
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        draw_picker(&mut p, pal, card, &l, picker, cache, look);
    }
    bmp
}

fn pixel(bmp: &Bitmap, scale: f32, x: f32, y: f32) -> Rgb {
    let (x, y) = ((x * scale) as usize, (y * scale) as usize);
    let o = (y * bmp.width as usize + x) * 4;
    let p = &bmp.pixels[o..o + 3];
    [p[2], p[1], p[0]]
}

#[test]
fn the_square_shows_white_the_hue_and_black_at_its_corners_and_the_strip_every_hue() {
    for pal in [&LIGHT, &DARK] {
        for scale in [1.0, 1.5] {
            let mut picker = Picker::new([0, 0, 255], HexForm::DEFAULT);
            picker.set_hsv(Hsv::new(240.0, 0.0, 0.0));
            let mut cache = Cache::new();
            let bmp = drawn(pal, scale, &mut picker, &mut cache, Look::default());
            let l = Layout::new(Rect::new(32.0, 32.0, CONTENT_W, CONTENT_H));
            let sq = l.square;
            let near = |a: Rgb, b: Rgb| (0..3).all(|k| a[k].abs_diff(b[k]) <= 6);
            let at = |x: f32, y: f32| pixel(&bmp, scale, x, y);
            assert!(
                near(at(sq.x + 1.0, sq.y + 1.0), [255; 3]),
                "{:?}",
                at(sq.x + 1.0, sq.y + 1.0)
            );
            assert!(near(at(sq.right() - 1.0, sq.y + 1.0), [0, 0, 255]));
            assert!(near(at(sq.right() - 1.0, sq.bottom() - 1.0), [0; 3]));
            let hue = l.hue;
            for (t, c) in [
                (0.004, [255, 0, 0]),
                (1.0 / 3.0, [0, 255, 0]),
                (2.0 / 3.0, [0, 0, 255]),
            ] {
                let got = at(hue.x + hue.w * t, hue.cy());
                assert!((0..3).all(|k| got[k].abs_diff(c[k]) <= 24), "{t}: {got:?}");
            }
            // The card is the popup's colour.
            assert_eq!(at(26.0, 200.0), pal.popup);
        }
    }
}

#[test]
fn the_markers_move_with_the_colour_and_the_cache_serves_the_same_pixels() {
    for pal in [&LIGHT, &DARK] {
        let mut cache = Cache::new();
        let mut a = Picker::new([200, 40, 40], HexForm::DEFAULT);
        let one = drawn(pal, 1.0, &mut a, &mut cache, Look::default());
        let again = drawn(pal, 1.0, &mut a, &mut cache, Look::default());
        assert!(one.pixels == again.pixels, "the cache draws the same");
        let fresh = drawn(pal, 1.0, &mut a, &mut Cache::new(), Look::default());
        assert!(one.pixels == fresh.pixels);
        let mut b = a.clone();
        b.set_hsv(Hsv::new(a.hsv().h, 0.2, 0.3));
        let moved = drawn(pal, 1.0, &mut b, &mut cache, Look::default());
        assert!(one.pixels != moved.pixels);
        // The marker is a white ring: white on the ring's radius, round the new place.
        let l = Layout::new(Rect::new(32.0, 32.0, CONTENT_W, CONTENT_H));
        let (mx, my) = l.square_marker(b.hsv());
        assert_eq!(pixel(&moved, 1.0, mx + 6.0, my), [255; 3]);
        // A ring shows round the part with the keyboard.
        let ringed = drawn(
            pal,
            1.0,
            &mut b,
            &mut cache,
            Look {
                ring: true,
                caret_on: false,
            },
        );
        assert_eq!(
            pixel(&ringed, 1.0, l.square.x - 2.5, l.square.cy()),
            pal.accent
        );
        assert_ne!(
            pixel(&moved, 1.0, l.square.x - 2.5, l.square.cy()),
            pal.accent
        );
    }
}

#[test]
fn odd_cards_draw_nothing_or_stay_inside() {
    let mut picker = Picker::new([1, 2, 3], HexForm::DEFAULT);
    let mut cache = Cache::new();
    let mut gfx = Gfx::new();
    let mut bmp = Bitmap::new(50, 40);
    bmp.clear([7, 7, 7]);
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    for card in [
        Rect::new(f32::NAN, 0.0, 100.0, 100.0),
        Rect::new(0.0, 0.0, 0.0, 10.0),
        Rect::new(0.0, 0.0, 10.0, -1.0),
    ] {
        draw_picker(
            &mut p,
            &DARK,
            card,
            &Layout::new(card),
            &mut picker,
            &mut cache,
            Look::default(),
        );
    }
    assert!(p.bmp.pixels.chunks(4).all(|c| c[..3] == [7, 7, 7]));
    // Larger than the bitmap, partly off it.
    for card in [
        Rect::new(-100.0, -100.0, 400.0, 400.0),
        Rect::new(30.0, 30.0, 400.0, 3.0),
    ] {
        draw_picker(
            &mut p,
            &LIGHT,
            card,
            &Layout::new(card.inset(12.0)),
            &mut picker,
            &mut cache,
            Look::default(),
        );
    }
}

#[test]
fn the_flyout_holds_the_content_and_its_padding() {
    let mut f = flyout(Rect::new(10.0, 10.0, 100.0, 28.0));
    f.place_in(Rect::new(0.0, 0.0, 800.0, 600.0));
    let c = f.content_rect();
    assert_eq!((c.w, c.h), (CONTENT_W, CONTENT_H));
    assert!(f.rect().y >= 38.0, "below the anchor");
}

#[test]
fn the_hex_field_area_is_where_the_field_is_drawn_and_a_field_input_edits_it() {
    let l = layout();
    let a = hex_field_area(&l, 1.5);
    assert_eq!(a.frame(), l.hex);
    let t = a.text();
    assert_eq!(
        t.area,
        Rect::new(l.hex.x + 10.0, l.hex.y, l.hex.w - 20.0, l.hex.h)
    );
    assert_eq!((t.face, t.size, t.scale), (Face::Mono, 13.0, 1.5));
    // A layout of no size: an empty text rectangle, no panic.
    let none = hex_field_area(&Layout::new(Rect::default()), f32::NAN);
    assert!(none.text().area.w <= 0.0 || none.frame().w <= 0.0);

    // A FieldInput given the picker's field and this area: a triple click selects the hex.
    let mut p = Picker::new([0x33, 0x66, 0xcc], HexForm::DEFAULT);
    let mut gfx = Gfx::new();
    let a = hex_field_area(&l, 1.0);
    let at = (a.text().area.x + 4.0, a.text().area.cy());
    assert_eq!(p.press(&l, at.0, at.1), Outcome::Redraw);
    assert_eq!(p.focus(), Part::Hex);
    let mut input = crate::textfield::FieldInput::new()
        .timing(std::time::Duration::from_millis(500), (2.0, 2.0));
    let t0 = std::time::Instant::now();
    for k in 0..3 {
        let now = t0 + std::time::Duration::from_millis(10 * k);
        input.press(p.field_mut(), &mut gfx, a, at, Mods::default(), now);
        input.release(p.field_mut(), &mut gfx, a, at, now);
    }
    assert_eq!(p.field().selected_text(), p.hex());
}
