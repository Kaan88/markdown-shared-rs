//! The paged form in motion ([`Form::draw_animated`]): `draw` is what it was on main, pixel for
//! pixel (against the frozen copy in `frozen.rs`); at rest and with an animator that does not
//! animate, the animated drawing is the plain one; in motion it is not, and once settled it is
//! again.

use super::*;
use crate::anim::{Animator, Clock, ManualClock, Policy};
use crate::gfx::Gfx;
use crate::theme::{DARK, LIGHT, Palette};
use mdshared_text::Bitmap;

const NONE: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: false,
    control: false,
};
const SHIFT: Mods = Mods {
    shift: true,
    ..NONE
};
const W: f32 = 900.0;
const H: f32 = 480.0;

const TOGGLE: u32 = 1;
const SEGMENTS: u32 = 2;
const FONT: u32 = 3;
const TAB: u32 = 4;
const PALE: u32 = 5;
const NAME: u32 = 6;
const RUN: u32 = 7;
const CHOOSE: u32 = 8;
const CHILD: u32 = 9;
const OTHER: u32 = 20;

fn pages() -> Vec<Page> {
    vec![
        Page::new(
            "General",
            vec![
                Item::new(Row::toggle(TOGGLE, "Wrap lines", false).help("Long lines go on under")),
                Item::new(Row::toggle(CHILD, "At the window", true)).child(),
                Item::new(Row::choice(
                    SEGMENTS,
                    "Theme",
                    &["Dark", "Light", "Auto"],
                    0,
                )),
                Item::new(Row::choice(
                    FONT,
                    "Font",
                    &["One", "Two", "Three", "Four", "Five", "Six"],
                    1,
                ))
                .dropdown(),
                Item::gap(),
                Item::new(Row::number(TAB, "Tab width", 4, 1, 8)),
                Item::new(Row::choice(PALE, "Corners", &["Round", "Square"], 0))
                    .disabled("Needs the switch above."),
            ],
        ),
        Page::new(
            "More",
            vec![
                Item::new(Row::toggle(OTHER, "Line numbers", true)),
                Item::new(Row::text(NAME, "Name", "Ada")).button(CHOOSE, "Choose…", true),
                Item::new(Row::action(RUN, "Check for updates", "Check now")),
            ],
        ),
        Page::new("Empty", vec![]),
    ]
}

fn form() -> Form {
    Form::new("Settings", pages())
}

/// Which drawing [`shot_at`] takes.
enum By<'a> {
    Plain,
    Frozen,
    Animated(&'a mut Animator),
}

/// The form drawn over a window of one grey, at `scale`, in a window of `w` x `h`.
fn shot_at(f: &mut Form, pal: &Palette, by: By<'_>, scale: f32, (w, h): (f32, f32)) -> Vec<u8> {
    let mut bmp = Bitmap::new((w * scale).ceil() as u32, (h * scale).ceil() as u32);
    bmp.clear([128; 3]);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        let window = Rect::new(0.0, 0.0, w, h);
        match by {
            By::Plain => f.draw(&mut p, pal, window, true),
            By::Frozen => f.draw_as_on_main(&mut p, pal, window, true),
            By::Animated(a) => f.draw_animated(&mut p, pal, window, true, a, "settings"),
        }
    }
    bmp.pixels
}

fn shot(f: &mut Form, pal: &Palette, by: By<'_>) -> Vec<u8> {
    shot_at(f, pal, by, 1.0, (W, H))
}

fn plain(f: &mut Form, pal: &Palette) -> Vec<u8> {
    shot(f, pal, By::Plain)
}

/// A window's frames on a clock moved by hand.
struct Frames {
    clock: ManualClock,
    anim: Animator,
}

impl Frames {
    fn new() -> Frames {
        Frames {
            clock: ManualClock::new(),
            anim: Animator::new(),
        }
    }
    /// One frame `ms` after the last: the animated drawing.
    fn frame(&mut self, f: &mut Form, pal: &Palette, ms: u64) -> Vec<u8> {
        self.clock.advance_ms(ms);
        self.anim.begin_frame(self.clock.now());
        let px = shot(f, pal, By::Animated(&mut self.anim));
        self.anim.end_frame();
        px
    }
    /// Frames of 16 ms until nothing moves; the last one's drawing.
    fn settle(&mut self, f: &mut Form, pal: &Palette) -> Vec<u8> {
        let mut px = self.frame(f, pal, 16);
        for _ in 0..60 {
            if !self.anim.animating() {
                break;
            }
            px = self.frame(f, pal, 16);
        }
        assert!(!self.anim.animating(), "everything arrives");
        px
    }
}

fn centre(r: Rect) -> (f32, f32) {
    (r.cx(), r.cy())
}

fn click(f: &mut Form, r: Rect) -> Outcome {
    let (x, y) = centre(r);
    f.pointer_move(x, y);
    let a = f.press(x, y, NONE, 1);
    match f.release(x, y) {
        Outcome::None => a,
        o => o,
    }
}

/// A change made by `change` after a first frame at rest: the frame that asks still draws the
/// old state; the next one is in motion (unlike the plain draw); once settled the animated draw
/// is the plain draw again.
fn moves_then_rests(pal: &Palette, what: &str, change: impl Fn(&mut Form)) {
    let mut f = form();
    let mut fr = Frames::new();
    assert!(
        fr.frame(&mut f, pal, 0) == plain(&mut f, pal),
        "{what}: first seen at rest"
    );
    change(&mut f);
    fr.frame(&mut f, pal, 16);
    let moving = fr.frame(&mut f, pal, 16);
    assert!(moving != plain(&mut f, pal), "{what}: in motion");
    let rest = fr.settle(&mut f, pal);
    assert!(rest == plain(&mut f, pal), "{what}: at rest again");
}

// ---- draw is what it was ---------------------------------------------------------------------

type Setup = fn(&mut Form);

/// The states a person sees: every page, a hover on every kind of target, the keyboard's ring,
/// a press held, an open dropdown with its hover row, a disabled row.
fn states() -> Vec<(&'static str, Setup)> {
    vec![
        ("first page", |_| {}),
        ("second page", |f| {
            f.show_page(1);
        }),
        ("empty page", |f| {
            f.show_page(2);
        }),
        ("list hover", |f| {
            let (x, y) = centre(f.list_item_rect(1));
            f.pointer_move(x, y);
        }),
        ("switch hover and press", |f| {
            let (x, y) = centre(f.control_rect(TOGGLE));
            f.pointer_move(x, y);
            f.press(x, y, NONE, 1);
        }),
        ("switch turned", |f| {
            click(f, f.control_rect(TOGGLE));
        }),
        ("segment hover", |f| {
            let (x, y) = centre(f.segment_rect(SEGMENTS, 2));
            f.pointer_move(x, y);
        }),
        ("segment chosen", |f| {
            click(f, f.segment_rect(SEGMENTS, 1));
        }),
        ("disabled segment", |f| {
            let (x, y) = centre(f.segment_rect(PALE, 1));
            f.pointer_move(x, y);
        }),
        ("dropdown hover", |f| {
            let (x, y) = centre(f.control_rect(FONT));
            f.pointer_move(x, y);
        }),
        ("dropdown open", |f| {
            click(f, f.control_rect(FONT));
            f.key(&KeyName::Down, NONE, None);
        }),
        ("stepper hover", |f| {
            let (_, _, plus) = f.stepper_parts(TAB);
            let (x, y) = centre(plus);
            f.pointer_move(x, y);
        }),
        ("close hover", |f| {
            let (x, y) = centre(f.close_rect());
            f.pointer_move(x, y);
        }),
        ("keyboard on the list", |f| {
            f.key(&KeyName::Down, NONE, None);
            f.key(&KeyName::Up, NONE, None);
        }),
        ("keyboard on a switch", |f| {
            f.key(&KeyName::Tab, NONE, None);
        }),
        ("keyboard on the close", |f| {
            f.key(&KeyName::Tab, SHIFT, None);
        }),
        ("text and button", |f| {
            f.show_page(1);
            assert!(f.focus_row(NAME));
            f.key(&KeyName::Tab, NONE, None);
        }),
        ("button hover", |f| {
            f.show_page(1);
        }),
    ]
}

#[test]
fn the_paged_form_still_draws_what_it_drew() {
    for (scale, size) in [(1.0, (W, H)), (1.5, (W, H)), (1.0, (520.0, 360.0))] {
        for (what, setup) in states() {
            for pal in [&LIGHT, &DARK] {
                let mut f = form();
                // Lay it out, so that the rectangles the steps ask for are there.
                shot_at(&mut f, pal, By::Plain, scale, size);
                setup(&mut f);
                if what == "button hover" {
                    shot_at(&mut f, pal, By::Plain, scale, size);
                    let (x, y) = centre(f.button_rect(CHOOSE));
                    assert!(f.pointer_move(x, y), "the button is there");
                }
                let old = shot_at(&mut f, pal, By::Frozen, scale, size);
                let new = shot_at(&mut f, pal, By::Plain, scale, size);
                assert!(
                    old == new,
                    "{what} at {scale} in {size:?}, dark {}",
                    pal.dark
                );
            }
        }
    }
}

// ---- at rest ---------------------------------------------------------------------------------

#[test]
fn the_animated_form_is_the_plain_form_at_rest_in_both_themes() {
    for pal in [&LIGHT, &DARK] {
        for (what, setup) in states() {
            let mut f = form();
            let mut fr = Frames::new();
            assert!(
                fr.frame(&mut f, pal, 0) == plain(&mut f, pal),
                "{what}: first"
            );
            setup(&mut f);
            let rest = fr.settle(&mut f, pal);
            assert!(
                rest == plain(&mut f, pal),
                "{what}: settled, dark {}",
                pal.dark
            );
        }
    }
}

#[test]
fn an_animator_that_does_not_animate_draws_the_plain_form_on_every_frame() {
    for (what, setup) in states() {
        let mut f = form();
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        anim.set_policy(Policy::Never);
        let mut frame = |f: &mut Form| {
            clock.advance_ms(16);
            anim.begin_frame(clock.now());
            let px = shot(f, &LIGHT, By::Animated(&mut anim));
            anim.end_frame();
            px
        };
        frame(&mut f);
        setup(&mut f);
        assert!(
            frame(&mut f) == plain(&mut f, &LIGHT),
            "{what}: the frame that asks"
        );
        assert!(frame(&mut f) == plain(&mut f, &LIGHT), "{what}: the next");
        // And values set from outside on top of it.
        f.set_value(SEGMENTS, Value::Choice(2));
        f.set_value(TOGGLE, Value::Bool(true));
        f.set_value(PALE, Value::Choice(1));
        assert!(frame(&mut f) == plain(&mut f, &LIGHT), "{what}: values set");
    }
}

// ---- in motion -------------------------------------------------------------------------------

#[test]
fn a_switch_slides_when_it_turns_over() {
    for pal in [&LIGHT, &DARK] {
        moves_then_rests(pal, "switch", |f| {
            assert_eq!(
                click(f, f.control_rect(TOGGLE)),
                Outcome::Changed {
                    id: TOGGLE,
                    value: Value::Bool(true)
                }
            );
        });
    }
}

#[test]
fn the_chosen_segment_slides_to_the_new_choice() {
    for pal in [&LIGHT, &DARK] {
        moves_then_rests(pal, "segment", |f| {
            f.set_value(SEGMENTS, Value::Choice(2));
        });
    }
}

#[test]
fn a_disabled_rows_paler_segment_slides_too() {
    moves_then_rests(&LIGHT, "disabled segment", |f| {
        f.set_value(PALE, Value::Choice(1));
    });
}

#[test]
fn a_pages_hover_fill_in_the_list_fades_in_and_out() {
    moves_then_rests(&LIGHT, "list hover in", |f| {
        let (x, y) = centre(f.list_item_rect(1));
        f.pointer_move(x, y);
    });
    // And out again.
    let mut f = form();
    let mut fr = Frames::new();
    fr.frame(&mut f, &DARK, 0);
    let (x, y) = centre(f.list_item_rect(2));
    f.pointer_move(x, y);
    fr.settle(&mut f, &DARK);
    f.pointer_move(1.0, 1.0);
    fr.frame(&mut f, &DARK, 16);
    assert!(
        fr.frame(&mut f, &DARK, 16) != plain(&mut f, &DARK),
        "fading out"
    );
    assert!(fr.settle(&mut f, &DARK) == plain(&mut f, &DARK), "gone");
}

#[test]
fn a_dropdowns_menu_hover_row_fades() {
    let mut f = form();
    let mut fr = Frames::new();
    fr.frame(&mut f, &LIGHT, 0);
    let r = f.control_rect(FONT);
    assert_eq!(click(&mut f, r), Outcome::Redraw);
    assert!(
        fr.frame(&mut f, &LIGHT, 16) == plain(&mut f, &LIGHT),
        "opened at rest"
    );
    f.key(&KeyName::Down, NONE, None);
    fr.frame(&mut f, &LIGHT, 16);
    assert!(
        fr.frame(&mut f, &LIGHT, 16) != plain(&mut f, &LIGHT),
        "in motion"
    );
    assert!(
        fr.settle(&mut f, &LIGHT) == plain(&mut f, &LIGHT),
        "at rest"
    );
}

#[test]
fn a_page_shown_is_drawn_at_rest_at_once() {
    let mut f = form();
    let mut fr = Frames::new();
    fr.frame(&mut f, &LIGHT, 0);
    f.set_value(TOGGLE, Value::Bool(true));
    fr.frame(&mut f, &LIGHT, 16);
    // Mid-slide on the first page, the keyboard shows the second: its switch in the same place
    // is another row of another page, and nothing there moves.
    assert!(f.focus_in_list());
    f.key(&KeyName::Down, NONE, None);
    assert_eq!(f.page(), 1);
    assert!(
        fr.frame(&mut f, &LIGHT, 16) == plain(&mut f, &LIGHT),
        "second page"
    );
    // Back on the first, its switch goes on to rest.
    f.key(&KeyName::Up, NONE, None);
    assert_eq!(f.page(), 0);
    assert!(
        fr.settle(&mut f, &LIGHT) == plain(&mut f, &LIGHT),
        "first page again"
    );
}

#[test]
fn new_pages_with_another_row_in_a_place_start_it_at_rest() {
    let mut f = form();
    let mut fr = Frames::new();
    fr.frame(&mut f, &LIGHT, 0);
    // The switch at index 0 turns over; then the pages come again with another switch there.
    f.set_value(TOGGLE, Value::Bool(true));
    fr.frame(&mut f, &LIGHT, 16);
    let mut p = pages();
    p[0] = Page::new("General", vec![Item::new(Row::toggle(99, "Another", true))]);
    f.set_pages(p);
    fr.frame(&mut f, &LIGHT, 16);
    assert!(
        fr.frame(&mut f, &LIGHT, 16) == plain(&mut f, &LIGHT),
        "another row"
    );
}

// ---- high contrast ---------------------------------------------------------------------------

#[test]
fn under_high_contrast_highlights_change_at_once_and_a_switch_still_slides() {
    let mut f = form();
    assert!(f.set_high_contrast(true));
    assert!(!f.set_high_contrast(true));
    let mut fr = Frames::new();
    fr.frame(&mut f, &DARK, 0);
    let (x, y) = centre(f.list_item_rect(1));
    f.pointer_move(x, y);
    assert!(
        fr.frame(&mut f, &DARK, 16) == plain(&mut f, &DARK),
        "list hover"
    );
    f.set_value(SEGMENTS, Value::Choice(2));
    assert!(
        fr.frame(&mut f, &DARK, 16) == plain(&mut f, &DARK),
        "segment"
    );
    let r = f.control_rect(FONT);
    click(&mut f, r);
    fr.frame(&mut f, &DARK, 16);
    f.key(&KeyName::Down, NONE, None);
    assert!(
        fr.frame(&mut f, &DARK, 16) == plain(&mut f, &DARK),
        "menu row"
    );
    f.key(&KeyName::Escape, NONE, None);
    fr.frame(&mut f, &DARK, 16);
    // A switch is motion, not a highlight: it still slides.
    f.set_value(TOGGLE, Value::Bool(true));
    fr.frame(&mut f, &DARK, 16);
    assert!(
        fr.frame(&mut f, &DARK, 16) != plain(&mut f, &DARK),
        "switch"
    );
    assert!(
        fr.settle(&mut f, &DARK) == plain(&mut f, &DARK),
        "switch at rest"
    );
    // Turning it off again changes nothing the plain draw draws.
    assert!(f.set_high_contrast(false));
    assert!(fr.frame(&mut f, &DARK, 16) == plain(&mut f, &DARK));
}

// ---- nonsense --------------------------------------------------------------------------------

#[test]
fn odd_windows_and_odd_pages_do_not_panic() {
    let odd = vec![
        Page::new("", vec![Item::new(Row::choice(1, "", &[], 0)), Item::gap()]),
        Page::new(
            "x",
            vec![
                Item::new(Row::number(2, "n", 0, i32::MIN, i32::MAX)),
                Item::new(Row::choice(3, "c", &["a"], 7)),
            ],
        ),
    ];
    for set in [vec![], odd, pages()] {
        let mut f = Form::new("s", set);
        let mut fr = Frames::new();
        for (w, h) in [
            (0.0f32, 0.0f32),
            (1.0, 1.0),
            (10.0, 10.0),
            (120.0, 90.0),
            (520.0, 360.0),
            (f32::NAN, 300.0),
            (300.0, -4.0),
        ] {
            for (step, ms) in [(0u64, 0u64), (1, 16), (2, 50), (3, 500)] {
                fr.clock.advance_ms(ms);
                fr.anim.begin_frame(fr.clock.now());
                let bw = if w.is_finite() {
                    w.clamp(1.0, 600.0)
                } else {
                    1.0
                } as u32;
                let bh = if h.is_finite() {
                    h.clamp(1.0, 600.0)
                } else {
                    1.0
                } as u32;
                let mut bmp = Bitmap::new(bw, bh);
                let mut gfx = Gfx::new();
                {
                    let mut p = Painter::new(&mut bmp, &mut gfx, 1.25);
                    let window = Rect::new(0.0, 0.0, w, h);
                    f.draw_animated(&mut p, &DARK, window, true, &mut fr.anim, 7);
                }
                fr.anim.end_frame();
                f.pointer_move(w / 2.0, h / 3.0);
                f.key(&KeyName::Down, NONE, None);
                if step == 2 {
                    f.key(&KeyName::Tab, NONE, None);
                    f.key(&KeyName::Space, NONE, None);
                }
            }
        }
    }
}
