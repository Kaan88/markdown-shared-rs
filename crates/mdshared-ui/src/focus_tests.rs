use super::*;
use crate::gfx::Gfx;
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

/// Declares `ids` in the base scope and ends the frame.
fn frame(f: &mut Focus<u32>, ids: &[u32]) {
    f.begin_frame();
    for &id in ids {
        f.add(id);
    }
    f.end_frame();
}

/// Declares the base's `ids` and the widgets of one open scope, and ends the frame.
fn scoped_frame(f: &mut Focus<u32>, base: &[u32], scope: u32, inside: &[u32]) {
    f.begin_frame();
    for &id in base {
        f.add(id);
    }
    for &id in inside {
        f.add_in(scope, id);
    }
    f.end_frame();
}

#[test]
fn tab_visits_the_widgets_in_the_order_they_were_declared_and_wraps_round() {
    let mut f = Focus::<u32>::new();
    frame(&mut f, &[1, 2, 3]);
    assert_eq!(f.focused(), None);
    assert!(f.tab(false) && f.focused() == Some(1));
    assert!(f.tab(false) && f.focused() == Some(2));
    assert!(f.tab(false) && f.focused() == Some(3));
    assert!(
        f.tab(false) && f.focused() == Some(1),
        "Tab wraps to the first"
    );
    assert!(f.ring_visible(1), "a Tab shows the ring");
}

#[test]
fn shift_tab_walks_backwards_and_wraps_from_the_first_to_the_last() {
    let mut f = Focus::<u32>::new();
    frame(&mut f, &[1, 2, 3]);
    assert!(
        f.tab(true) && f.focused() == Some(3),
        "Shift+Tab starts at the end"
    );
    assert!(f.tab(true) && f.focused() == Some(2));
    assert!(f.tab(true) && f.focused() == Some(1));
    assert!(
        f.tab(true) && f.focused() == Some(3),
        "Shift+Tab wraps to the last"
    );
}

#[test]
fn a_focused_widget_that_disappears_hands_the_focus_to_the_one_that_took_its_place() {
    let mut f = Focus::<u32>::new();
    frame(&mut f, &[1, 2, 3]);
    f.tab(false);
    f.tab(false);
    assert_eq!(f.focused(), Some(2));
    frame(&mut f, &[1, 3]);
    assert_eq!(f.focused(), Some(3), "the next widget in the order");
    frame(&mut f, &[1, 2]);
    assert_eq!(f.focused(), Some(2), "the last one gone: the one before it");
    frame(&mut f, &[]);
    assert_eq!(f.focused(), None, "nothing left to focus");
}

#[test]
fn an_empty_frame_takes_the_focus_off_the_only_widget() {
    let mut f = Focus::<u32>::new();
    frame(&mut f, &[1]);
    f.tab(false);
    assert!(f.is_focused(1));
    frame(&mut f, &[]);
    assert_eq!(f.focused(), None);
    assert!(!f.is_focused(1));
    assert!(!f.tab(false), "there is nothing to tab to");
}

#[test]
fn tab_skips_a_widget_that_is_not_a_tab_stop_but_a_press_still_focuses_it() {
    let mut f = Focus::<u32>::new();
    f.begin_frame();
    f.add(1);
    f.add_with(2, Options::no_tab_stop());
    f.add(3);
    f.end_frame();
    f.tab(false);
    assert_eq!(f.focused(), Some(1));
    f.tab(false);
    assert_eq!(
        f.focused(),
        Some(3),
        "Tab passes over the widget without a stop"
    );
    f.tab(false);
    assert_eq!(f.focused(), Some(1));
    assert!(f.focus(2), "a press can put the focus on it");
    assert!(f.is_focused(2));
    assert!(!f.ring_visible(2), "the press hides the ring");
    f.tab(false);
    assert_eq!(
        f.focused(),
        Some(3),
        "Tab continues after the widget pressed"
    );
}

#[test]
fn tab_with_no_tab_stop_in_the_scope_stays_put_and_answers_false() {
    let mut f = Focus::<u32>::new();
    f.begin_frame();
    f.add_with(7, Options::no_tab_stop());
    f.end_frame();
    assert!(!f.tab(false));
    assert_eq!(f.focused(), None);
    f.focus(7);
    assert!(!f.tab(false));
    assert_eq!(f.focused(), Some(7));
}

#[test]
fn a_scope_skips_its_own_widgets_without_a_tab_stop_but_a_press_still_focuses_them() {
    let mut f = Focus::<u32>::new();
    frame(&mut f, &[1]);
    f.push_scope(10);
    f.begin_frame();
    f.add(1);
    f.add_in(10, 20);
    f.add_in_with(10, 21, Options::no_tab_stop());
    f.add_in(10, 22);
    f.end_frame();
    assert_eq!(f.focused(), Some(20));
    f.tab(false);
    assert_eq!(
        f.focused(),
        Some(22),
        "Tab passes over the scope's own skip"
    );
    f.tab(false);
    assert_eq!(f.focused(), Some(20));
    assert!(f.focus(21));
    assert!(f.is_focused(21), "a press reaches it inside the scope");
    f.tab(false);
    assert_eq!(f.focused(), Some(22), "and Tab carries on after it");
}

#[test]
fn a_widget_declared_twice_in_one_frame_is_declared_once_with_its_first_options() {
    let mut f = Focus::<u32>::new();
    f.begin_frame();
    f.add(1);
    f.add_with(1, Options::no_tab_stop());
    f.add_in(9, 1);
    f.add(2);
    f.end_frame();
    f.tab(false);
    assert_eq!(
        f.focused(),
        Some(1),
        "the first declaration's Tab stop stands"
    );
    f.tab(false);
    assert_eq!(f.focused(), Some(2), "and the id is in the order only once");
}

#[test]
fn the_ring_shows_for_the_keyboard_and_hides_for_the_pointer_until_the_next_key() {
    let mut f = Focus::<u32>::new();
    frame(&mut f, &[1, 2]);
    assert!(f.focus(1));
    assert!(!f.ring_visible(1), "a click focuses without a ring");
    f.key_used();
    assert!(f.ring_visible(1), "a key shows it");
    f.pointer_used();
    assert!(!f.ring_visible(1), "the pointer hides it again");
    f.tab(false);
    assert!(f.ring_visible(2));
    assert!(!f.ring_visible(1), "no ring where the focus is not");
    assert!(!f.ring_visible(9));
    assert!(!f.focus(2), "a press on the focused widget changes nothing");
    assert!(!f.ring_visible(2), "and still hides the ring");
}

#[test]
fn blur_takes_the_focus_and_the_ring_off_every_widget() {
    let mut f = Focus::<u32>::new();
    frame(&mut f, &[1]);
    f.focus(1);
    f.key_used();
    assert!(f.ring_visible(1));
    f.blur();
    assert_eq!(f.focused(), None);
    assert!(!f.ring_visible(1));
}

#[test]
fn a_scope_takes_the_focus_keeps_tab_inside_and_gives_the_focus_back_when_it_closes() {
    let mut f = Focus::<u32>::new();
    frame(&mut f, &[1, 2, 3]);
    f.tab(false);
    f.tab(false);
    assert_eq!(f.focused(), Some(2));
    f.push_scope(10);
    assert_eq!(f.scope(), Some(10));
    scoped_frame(&mut f, &[1, 2, 3], 10, &[20, 21]);
    assert_eq!(f.focused(), Some(20), "the scope's first widget");
    f.tab(false);
    assert_eq!(f.focused(), Some(21));
    f.tab(false);
    assert_eq!(f.focused(), Some(20), "Tab stays inside the scope");
    f.tab(true);
    assert_eq!(f.focused(), Some(21), "Shift+Tab too");
    assert!(f.pop_scope());
    assert_eq!(f.focused(), Some(2), "the widget that had the focus before");
    assert_eq!(f.scope(), None);
    f.tab(false);
    assert_eq!(f.focused(), Some(3), "Tab is back on the base widgets");
    assert!(!f.pop_scope(), "nothing left to close");
}

#[test]
fn a_scope_can_open_on_the_widget_the_caller_names() {
    let mut f = Focus::<u32>::new();
    frame(&mut f, &[1, 2]);
    f.push_scope_on(10, 22);
    scoped_frame(&mut f, &[1, 2], 10, &[20, 21, 22]);
    assert_eq!(f.focused(), Some(22), "the named widget, not the first");
    f.tab(false);
    assert_eq!(f.focused(), Some(20), "the scope wraps as usual");

    let mut g = Focus::<u32>::new();
    frame(&mut g, &[1]);
    g.push_scope_on(10, 99);
    scoped_frame(&mut g, &[1], 10, &[20, 21]);
    assert_eq!(
        g.focused(),
        Some(20),
        "a name it does not declare is the first"
    );
}

#[test]
fn nested_scopes_stack_and_each_pop_returns_to_its_own_widget() {
    let mut f = Focus::<u32>::new();
    frame(&mut f, &[1, 2]);
    f.tab(false);
    assert_eq!(f.focused(), Some(1));
    f.push_scope(10);
    scoped_frame(&mut f, &[1, 2], 10, &[20, 21]);
    assert_eq!(f.focused(), Some(20));
    f.tab(false);
    assert_eq!(f.focused(), Some(21));
    f.push_scope(11);
    assert_eq!(f.scope(), Some(11));
    f.begin_frame();
    f.add(1);
    f.add(2);
    f.add_in(10, 20);
    f.add_in(10, 21);
    f.add_in(11, 30);
    f.add_in(11, 31);
    f.end_frame();
    assert_eq!(f.focused(), Some(30), "the inner scope's first widget");
    f.tab(false);
    assert_eq!(f.focused(), Some(31));
    f.tab(false);
    assert_eq!(f.focused(), Some(30), "the inner scope wraps inside itself");
    assert!(f.pop_scope());
    assert_eq!(f.focused(), Some(21), "back into the dialog");
    assert_eq!(f.scope(), Some(10));
    f.tab(false);
    assert_eq!(f.focused(), Some(20), "the dialog's own order");
    assert!(f.pop_scope());
    assert_eq!(f.focused(), Some(1), "and back to the base");
    assert_eq!(f.scope(), None);
}

#[test]
fn a_scope_that_closes_after_its_remembered_widget_left_gives_the_focus_to_its_neighbour() {
    let mut f = Focus::<u32>::new();
    frame(&mut f, &[1, 2, 3]);
    f.tab(false);
    f.tab(false);
    assert_eq!(f.focused(), Some(2));
    f.push_scope(10);
    scoped_frame(&mut f, &[1, 3], 10, &[20]);
    assert_eq!(f.focused(), Some(20));
    assert!(f.pop_scope());
    assert_eq!(
        f.focused(),
        Some(3),
        "the stop that followed the one remembered"
    );

    let mut g = Focus::<u32>::new();
    frame(&mut g, &[1, 2, 3]);
    g.tab(false);
    g.tab(false);
    g.push_scope(10);
    scoped_frame(&mut g, &[1], 10, &[20]);
    assert!(g.pop_scope());
    assert_eq!(
        g.focused(),
        Some(1),
        "the neighbour left too: the first stop left"
    );

    let mut h = Focus::<u32>::new();
    frame(&mut h, &[1, 2]);
    h.tab(false);
    h.tab(false);
    h.push_scope(10);
    scoped_frame(&mut h, &[], 10, &[20]);
    assert!(h.pop_scope());
    assert_eq!(h.focused(), None, "nothing is left to return to");
}

#[test]
fn a_scope_that_declares_no_widget_has_no_focus_and_takes_the_first_one_that_appears() {
    let mut f = Focus::<u32>::new();
    frame(&mut f, &[1, 2]);
    f.tab(false);
    assert_eq!(f.focused(), Some(1));
    f.push_scope(10);
    frame(&mut f, &[1, 2]);
    assert_eq!(f.scope(), Some(10));
    assert_eq!(f.focused(), None, "the open scope has no widgets to focus");
    assert!(!f.tab(false), "and Tab has nowhere to go inside it");
    assert_eq!(f.focused(), None);
    scoped_frame(&mut f, &[1, 2], 10, &[20, 21]);
    assert_eq!(
        f.focused(),
        Some(20),
        "the first widget that appears takes it"
    );
}

#[test]
fn a_new_model_is_empty_and_the_options_default_to_a_tab_stop() {
    assert_eq!(Options::default(), Options::new());
    assert!(Options::new().tab_stop);
    assert!(!Options::no_tab_stop().tab_stop);
    let a = Focus::<u32>::new();
    let b = Focus::<u32>::default();
    assert_eq!(a.focused(), None);
    assert_eq!(a.focused(), b.focused());
    assert_eq!(a.scope(), None);
}

fn at(bmp: &Bitmap, x: usize, y: usize) -> [u8; 3] {
    let o = (y * 60 + x) * 4;
    [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
}

/// A 60 x 60 popup with a 20 x 20 widget on it and, when asked, its focus ring.
fn ringed(pal: &Palette, ring: bool, radius: f32) -> Bitmap {
    let mut bmp = Bitmap::new(60, 60);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, 60.0, 60.0), pal.popup);
        if ring {
            draw_ring(&mut p, pal, Rect::new(20.0, 20.0, 20.0, 20.0), radius);
        }
    }
    bmp
}

#[test]
fn the_focus_ring_is_two_px_of_accent_just_outside_the_widget_and_leaves_it_untouched() {
    for pal in [&LIGHT, &DARK] {
        let plain = ringed(pal, false, 4.0);
        let ring = ringed(pal, true, 4.0);
        assert_ne!(plain.pixels, ring.pixels, "the ring drew nothing");
        for (x, y) in [(18, 30), (41, 30), (30, 18), (30, 41)] {
            assert_eq!(at(&ring, x, y), pal.accent, "the ring at {x},{y}");
        }
        // The widget's own square is what it was, pixel for pixel, along both middles; the
        // four square corners are outside its rounded corners and outside the ring's.
        for x in 20..40 {
            for y in [24, 27, 30, 33, 36] {
                assert_eq!(at(&ring, x, y), at(&plain, x, y), "changed {x},{y}");
            }
        }
        for y in 20..40 {
            for x in [24, 27, 30, 33, 36] {
                assert_eq!(at(&ring, x, y), at(&plain, x, y), "changed {x},{y}");
            }
        }
        // And nothing past the ring, which reaches three px outside.
        for (x, y) in [(15, 30), (44, 30), (30, 15), (30, 44)] {
            assert_eq!(at(&ring, x, y), at(&plain, x, y), "reached {x},{y}");
        }
    }
}

#[test]
fn a_ring_around_nothing_draws_nothing_and_a_bad_radius_is_a_square_ring() {
    for r in [
        Rect::new(20.0, 20.0, 0.0, 20.0),
        Rect::new(20.0, 20.0, 20.0, -3.0),
        Rect::new(f32::NAN, 20.0, 20.0, 20.0),
        Rect::new(20.0, f32::INFINITY, 20.0, 20.0),
    ] {
        let plain = ringed(&LIGHT, false, 4.0);
        let mut bmp = plain.clone();
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            draw_ring(&mut p, &LIGHT, r, 4.0);
        }
        assert_eq!(bmp.pixels, plain.pixels, "drew {r:?}");
    }
    let square = ringed(&LIGHT, true, f32::NAN);
    assert_eq!(
        at(&square, 18, 30),
        LIGHT.accent,
        "a square ring, still outside"
    );
}

/// The sheet a person looks at: the ring round a button, a tick box and a field, light and
/// dark at two scales. `cargo test -p mdshared-ui focus_sheet -- --ignored`, then open
/// target/specimen/focus-ring.png and focus-ring-150.png.
#[test]
#[ignore]
fn focus_sheet() {
    let (w, h) = (240.0, 300.0);
    for (scale, file) in [(1.0, "focus-ring.png"), (1.5, "focus-ring-150.png")] {
        let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (half, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
            let oy = half as f32 * (h / 2.0);
            p.fill(Rect::new(0.0, oy, w, h / 2.0), pal.page);
            let o = p.offset(0.0, oy);
            let button = Rect::new(60.0, 24.0, 120.0, 30.0);
            draw_ring(&mut p, pal, button, 6.0);
            p.rbox(button, 6.0, pal.bar);
            let tick = Rect::new(112.0, 76.0, 16.0, 16.0);
            draw_ring(&mut p, pal, tick, 4.0);
            p.frame(tick, 4.0, pal.field, pal.accent);
            let field = Rect::new(40.0, 116.0, 160.0, 26.0);
            draw_ring(&mut p, pal, field, 5.0);
            p.frame(field, 5.0, pal.field, pal.line);
            p.restore_offset(o);
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join(file)).unwrap();
    }
}
