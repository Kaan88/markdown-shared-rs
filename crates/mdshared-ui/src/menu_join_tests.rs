use super::*;
use crate::gfx::{Gfx, Rgb};
use crate::menu::{Item, Menu};
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

/// The bar the tab stands in: a colour no palette has, so every pixel the join paints shows.
const BAR: Rgb = [90, 130, 170];

/// The rounded look without its shadow, so the pixels round the tab are the bar's until the
/// join paints them.
fn unshadowed() -> Look {
    let mut look = Look::ROUNDED;
    look.shadow = false;
    look
}

fn pixel(b: &Bitmap, x: i32, y: i32) -> Rgb {
    let i = (y as usize * b.width as usize + x as usize) * 4;
    [b.pixels[i + 2], b.pixels[i + 1], b.pixels[i]]
}

/// A menu frame drawn at `menu` as the menu draws its own, then the join: the bitmap before
/// and after `join_over`, and what it answered. Logical px are device px at scale 1.
fn joined(pal: &Palette, look: Look, tab: Rect, menu: Rect, scale: f32) -> (Bitmap, Bitmap, bool) {
    let mut bmp = Bitmap::new((320.0 * scale) as u32, (240.0 * scale) as u32);
    bmp.clear(BAR);
    let mut gfx = Gfx::new();
    let before;
    let answer;
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        p.frame(menu, look.radius, pal.popup, pal.popup_line);
        before = p.bmp.clone();
        answer = join_over(&mut p, pal, &look, tab, menu);
    }
    (before, bmp, answer)
}

const TAB: Rect = Rect::new(40.0, 20.0, 60.0, 28.0);
const MENU: Rect = Rect::new(40.0, 48.0, 180.0, 120.0);

#[test]
fn a_menu_joins_its_tab_only_flush_under_it_and_across_it() {
    assert!(joins(TAB, MENU));
    // Within half a px is flush.
    assert!(joins(TAB, Rect::new(40.4, 48.4, 180.0, 120.0)));
    assert!(joins(TAB, Rect::new(10.0, 47.6, 90.4, 120.0)));
    // Apart, flipped above, pushed sideways off it, narrower than it.
    for menu in [
        Rect::new(40.0, 52.0, 180.0, 120.0),
        Rect::new(40.0, 20.0 - 120.0, 180.0, 120.0),
        Rect::new(41.0, 48.0, 180.0, 120.0),
        Rect::new(-100.0, 48.0, 180.0, 120.0),
        Rect::new(50.0, 48.0, 40.0, 120.0),
    ] {
        assert!(!joins(TAB, menu), "{menu:?}");
    }
    // Nonsense joins nothing.
    for bad in [
        Rect::new(f32::NAN, 20.0, 60.0, 28.0),
        Rect::new(40.0, f32::NAN, 60.0, 28.0),
        Rect::new(40.0, 20.0, f32::INFINITY, 28.0),
        Rect::new(40.0, 20.0, 0.0, 28.0),
        Rect::new(40.0, 20.0, 60.0, -1.0),
    ] {
        assert!(!joins(bad, MENU), "{bad:?}");
    }
    for bad in [
        Rect {
            x: f32::NAN,
            ..MENU
        },
        Rect {
            y: f32::NAN,
            ..MENU
        },
        Rect {
            w: f32::INFINITY,
            ..MENU
        },
        Rect { h: 0.0, ..MENU },
        Rect { w: -5.0, ..MENU },
    ] {
        assert!(!joins(TAB, bad), "{bad:?}");
    }
}

#[test]
fn a_menu_that_does_not_join_draws_nothing() {
    for menu in [
        Rect::new(40.0, 52.0, 180.0, 120.0),
        Rect::new(45.0, 48.0, 180.0, 120.0),
        Rect::new(40.0, 48.0, 50.0, 120.0),
    ] {
        let (before, after, answer) = joined(&LIGHT, unshadowed(), TAB, menu, 1.0);
        assert!(!answer, "{menu:?}");
        assert!(before.pixels == after.pixels, "{menu:?}");
    }
    let nan = Rect::new(f32::NAN, 20.0, 60.0, 28.0);
    let (before, after, answer) = joined(&DARK, unshadowed(), nan, MENU, 1.0);
    assert!(!answer && before.pixels == after.pixels);
}

#[test]
fn the_tab_is_the_menus_colour_with_its_line_round_the_top_and_sides_and_the_seam_open() {
    for pal in [&LIGHT, &DARK] {
        let (before, after, answer) = joined(pal, unshadowed(), TAB, MENU, 1.0);
        assert!(answer);
        // Inside the tab, down to its foot.
        assert_eq!(pixel(&after, 70, 34), pal.popup);
        assert_eq!(pixel(&after, 70, 47), pal.popup);
        // Its line: the top, and each side.
        assert_eq!(pixel(&after, 70, 20), pal.popup_line);
        assert_eq!(pixel(&after, 40, 34), pal.popup_line);
        assert_eq!(pixel(&after, 99, 34), pal.popup_line);
        // Its top corners are rounded: the corner pixel is still mostly the bar.
        assert_ne!(pixel(&after, 40, 20), pal.popup_line);
        assert_ne!(pixel(&after, 40, 20), pal.popup);
        // The menu's line under it was there and is opened, between the tab's side lines;
        // past the tab the menu's top line stays.
        assert_eq!(pixel(&before, 70, 48), pal.popup_line);
        assert_eq!(pixel(&after, 70, 48), pal.popup);
        assert_eq!(pixel(&after, 41, 48), pal.popup);
        assert_eq!(pixel(&after, 98, 48), pal.popup);
        assert_eq!(pixel(&after, 150, 48), pal.popup_line);
        // Nothing is drawn above or beside the tab, away from its foot.
        assert_eq!(pixel(&after, 70, 10), BAR);
        assert_eq!(pixel(&after, 30, 34), BAR);
        assert_eq!(pixel(&after, 115, 30), BAR);
    }
}

#[test]
fn the_menus_corner_under_a_tab_at_its_edge_is_square_and_the_other_is_kept() {
    for scale in [1.0, 1.5] {
        let (before, after, _) = joined(&LIGHT, unshadowed(), TAB, MENU, scale);
        let s = |v: f32| (v * scale) as i32;
        let (x0, y0) = (s(MENU.x), s(MENU.y));
        // Rounded before: the very corner is the bar, the inside of the curve the fill.
        assert_eq!(pixel(&before, x0, y0), BAR);
        // Square after: the menu's side line goes straight up to its top, and the corner
        // inside it is the fill.
        for y in y0..y0 + s(8.0) {
            assert_eq!(
                pixel(&after, x0, y),
                LIGHT.popup_line,
                "scale {scale} at y {y}"
            );
        }
        assert_eq!(pixel(&after, x0 + 1, y0 + 1), LIGHT.popup);
        assert_eq!(pixel(&after, x0 + 2, y0 + 2), LIGHT.popup);
        // The top-right corner, which the tab does not reach, is the menu's own.
        let x1 = s(MENU.right());
        for y in y0..y0 + s(8.0) {
            for x in x1 - s(8.0)..x1 {
                assert_eq!(pixel(&after, x, y), pixel(&before, x, y), "at {x},{y}");
            }
        }
        // And the menu below its top corners, and its other side, are untouched.
        for (x, y) in [(x0, s(100.0)), (s(150.0), s(150.0)), (x1 - 1, s(100.0))] {
            assert_eq!(pixel(&after, x, y), pixel(&before, x, y), "at {x},{y}");
        }
    }
}

#[test]
fn squaring_the_corner_keeps_a_lit_first_row() {
    let mut bmp = Bitmap::new(320, 240);
    bmp.clear(BAR);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let items = vec![Item::new(0u8, "Open"), Item::new(1, "Save")];
    let mut menu = Menu::new(items, 0.0, 0.0, false)
        .around(TAB)
        .with_look(unshadowed());
    menu.hover = Some(0);
    let area = Rect::new(0.0, 0.0, 320.0, 240.0);
    menu.draw_in(&mut p, &LIGHT, area);
    assert!(joins(TAB, menu.rect()), "{:?}", menu.rect());
    let before = p.bmp.clone();
    assert!(join_over(&mut p, &LIGHT, &unshadowed(), TAB, menu.rect()));
    // Every pixel of the lit row is as the menu drew it, its rounded corners included.
    let row = menu.row_rect(0);
    for y in row.y as i32..row.bottom() as i32 {
        for x in row.x as i32..row.right() as i32 {
            assert_eq!(pixel(&bmp, x, y), pixel(&before, x, y), "at {x},{y}");
        }
    }
    assert_eq!(pixel(&bmp, row.cx() as i32, row.cy() as i32), LIGHT.hover);
}

#[test]
fn a_tab_inside_the_menus_width_curves_into_its_top() {
    let (before, after, _) = joined(&LIGHT, unshadowed(), TAB, MENU, 1.0);
    // The tab's right line is column 99; the menu's top line row 48. The curve's centre is
    // 8 px right of the tab and 8 px above the menu: (108, 40).
    // At the tab's foot the side line has curved away into the fill.
    assert_eq!(pixel(&before, 100, 47), BAR);
    assert_eq!(pixel(&after, 99, 47), LIGHT.popup);
    assert_eq!(pixel(&after, 100, 47), LIGHT.popup);
    // Above the curve the tab's line is straight.
    assert_eq!(pixel(&after, 99, 38), LIGHT.popup_line);
    // Inside the circle the bar is left.
    assert_eq!(pixel(&after, 107, 41), BAR);
    assert_eq!(pixel(&after, 105, 44), BAR);
    // Past the curve, the menu's top line is its own.
    assert_eq!(pixel(&after, 109, 48), LIGHT.popup_line);
    assert_eq!(pixel(&after, 120, 48), LIGHT.popup_line);
    // On the ring, about 8.5 px from the centre, the line.
    let ring = pixel(&after, 101, 45);
    let near = |a: Rgb, b: Rgb| (0..3).all(|i| (a[i] as i32 - b[i] as i32).abs() <= 8);
    assert!(near(ring, LIGHT.popup_line), "{ring:?}");
    // Nothing is drawn beyond the curve's reach.
    for y in 20..48 {
        for x in 110..320 {
            assert_eq!(pixel(&after, x, y), BAR, "at {x},{y}");
        }
    }
    // Mirrored: a tab flush with the menu's right, inside on its left.
    let tab = Rect::new(160.0, 20.0, 60.0, 28.0);
    let (_, after, answer) = joined(&LIGHT, unshadowed(), tab, MENU, 1.0);
    assert!(answer);
    assert_eq!(pixel(&after, 160, 47), LIGHT.popup);
    assert_eq!(pixel(&after, 159, 47), LIGHT.popup);
    assert_eq!(pixel(&after, 160, 38), LIGHT.popup_line);
    assert_eq!(pixel(&after, 152, 41), BAR);
    assert_eq!(pixel(&after, 219, 30), LIGHT.popup_line);
    assert_eq!(pixel(&after, 219, 49), LIGHT.popup_line);
}

#[test]
fn the_curve_is_no_larger_than_the_room_past_the_tab_and_none_in_a_square_look() {
    // The menu reaches 3 px past the tab: the curve stays over the menu.
    let menu = Rect::new(40.0, 48.0, 63.0, 120.0);
    let (_, after, answer) = joined(&LIGHT, unshadowed(), TAB, menu, 1.0);
    assert!(answer);
    for y in 0..48 {
        for x in 103..320 {
            assert_eq!(pixel(&after, x, y), BAR, "at {x},{y}");
        }
    }
    // A square look: square tab, square corners and no curve.
    let mut square = Look::FLAT;
    square.shadow = false;
    let (before, after, answer) = joined(&LIGHT, square, TAB, MENU, 1.0);
    assert!(answer);
    assert_eq!(pixel(&after, 40, 20), LIGHT.popup_line);
    assert_eq!(pixel(&after, 99, 20), LIGHT.popup_line);
    assert_eq!(pixel(&after, 99, 47), LIGHT.popup_line);
    assert_eq!(pixel(&after, 100, 47), BAR);
    assert_eq!(pixel(&after, 99, 48), LIGHT.popup_line);
    assert_eq!(pixel(&after, 40, 48), LIGHT.popup_line);
    assert_eq!(pixel(&after, 70, 48), LIGHT.popup);
    // A large radius curves no more than FILLET px.
    let mut round = unshadowed();
    round.radius = 20.0;
    let (_, after, _) = joined(&LIGHT, round, TAB, MENU, 1.0);
    for y in 0..48 {
        for x in 100 + FILLET as i32 + 1..320 {
            assert_eq!(pixel(&after, x, y), BAR, "at {x},{y}");
        }
    }
    assert_eq!(before.width, after.width);
}

#[test]
fn the_tabs_shadow_is_the_menus_shadow_at_the_looks_strength() {
    let draw = |look: Look, tab: Rect, how: &dyn Fn(&mut Painter, &Look, Rect)| {
        let mut bmp = Bitmap::new(200, 120);
        bmp.clear([220, 220, 220]);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        how(&mut p, &look, tab);
        bmp
    };
    let under = |p: &mut Painter, look: &Look, tab: Rect| join_under(p, &DARK, look, tab);
    let blank = draw(Look::ROUNDED, TAB, &|_, _, _| {});
    for k in [1.0, 0.5, 2.0] {
        let mut look = Look::ROUNDED;
        look.shadow_strength = k;
        let drawn = draw(look, TAB, &under);
        let by_hand = draw(look, TAB, &|p, l, t| {
            p.shadow_with(t, l.radius, true, l.shadow_strength)
        });
        assert!(drawn.pixels == by_hand.pixels, "k {k}");
        assert!(drawn.pixels != blank.pixels, "k {k}");
    }
    // None: a look without a shadow, at strength 0, or a tab that is not a rectangle.
    let mut zero = Look::ROUNDED;
    zero.shadow_strength = 0.0;
    for (look, tab) in [
        (Look::FLAT, TAB),
        (zero, TAB),
        (Look::ROUNDED, Rect::new(f32::NAN, 0.0, 10.0, 10.0)),
        (Look::ROUNDED, Rect::new(10.0, 10.0, 0.0, 10.0)),
    ] {
        assert!(draw(look, tab, &under).pixels == blank.pixels, "{tab:?}");
    }
}

#[test]
fn in_a_soft_look_the_tab_casts_its_own_soft_shadow() {
    // The prototype's joined control, `0 2px 10px 45%`, at the look's strength: below the
    // middle of the tab's foot, on a light ground, and nothing inside the tab itself.
    let mut look = Look::ROUNDED;
    look.soft_shadow = true;
    look.shadow_strength = 0.6;
    look.radius = 6.0;
    let mut bmp = Bitmap::new(200, 120);
    bmp.clear([255, 255, 255]);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        join_under(&mut p, &DARK, &look, TAB);
    }
    let alpha = |x: i32, y: i32| 1.0 - pixel(&bmp, x, y)[1] as f32 / 255.0;
    let x = TAB.cx() as i32;
    let foot = TAB.bottom() as i32;
    // The tab's Gaussian across its 60 px is whole in its middle; down, it is the foot's.
    let cdf = |t: f32| 0.5 * (1.0 + erf(t / std::f32::consts::SQRT_2));
    for d in [0, 2, 4, 8] {
        let want = 0.45 * 0.6 * cdf((2.0 - (d as f32 + 0.5)) / 5.0);
        let got = alpha(x, foot + d);
        assert!(
            (got - want).abs() <= 1.5 / 255.0,
            "{d} below: {got}, {want}"
        );
    }
    assert_eq!(alpha(x, TAB.cy() as i32), 0.0);
    // And on a light palette, .45 of it.
    let mut light = Bitmap::new(200, 120);
    light.clear([255, 255, 255]);
    {
        let mut p = Painter::new(&mut light, &mut gfx, 1.0);
        join_under(&mut p, &LIGHT, &look, TAB);
    }
    let l = 1.0 - pixel(&light, x, foot)[1] as f32 / 255.0;
    assert!((l - 0.45 * alpha(x, foot)).abs() <= 1.5 / 255.0, "{l}");
}

/// The error function by plain summing of its integrand, for the expected shadow.
fn erf(x: f32) -> f32 {
    let n = 4000;
    let dx = x as f64 / n as f64;
    let f = |t: f64| (-t * t).exp();
    let inner: f64 = (1..n).map(|i| f(i as f64 * dx)).sum();
    (((f(0.0) + f(x as f64)) / 2.0 + inner) * dx * 2.0 / std::f64::consts::PI.sqrt()) as f32
}
