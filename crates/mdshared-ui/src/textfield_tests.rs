//! The caret's blink: its timing on a clock moved by hand, its restart on input, when it asks
//! for no frame, and what it draws.

use super::*;
use crate::anim::{Clock, ManualClock};
use crate::gfx::{Gfx, rgb};
use mdshared_text::Bitmap;
use std::time::Instant;

const HALF: Duration = Duration::from_millis(500);
const ID: &str = "field";

/// Something done to a field.
type Edit = fn(&mut TextField);

/// A field, an animator and a clock moved by hand.
struct Rig {
    clock: ManualClock,
    anim: Animator,
    field: TextField,
}

impl Rig {
    fn new(text: &str) -> Rig {
        let mut field = TextField::new();
        field.set_text(text);
        Rig {
            clock: ManualClock::new(),
            anim: Animator::new(),
            field,
        }
    }

    /// One frame `ms` later that asks whether the caret is on: what it said, and the deadline.
    fn frame(
        &mut self,
        ms: u64,
        blink: Option<Duration>,
        focused: bool,
    ) -> (bool, Option<Instant>) {
        self.clock.advance_ms(ms);
        self.anim.begin_frame(self.clock.now());
        let on = self.field.blink_caret(&mut self.anim, ID, blink, focused);
        (on, self.anim.end_frame())
    }

    /// The same, focused, at the usual half-period.
    fn tick(&mut self, ms: u64) -> (bool, Option<Instant>) {
        self.frame(ms, Some(HALF), true)
    }

    fn at(&self, ms: u64) -> Option<Instant> {
        Some(self.clock.now() + Duration::from_millis(ms))
    }
}

#[test]
fn a_focused_caret_is_on_for_one_half_period_and_off_for_the_next_and_wakes_only_then() {
    let mut r = Rig::new("hello");
    // First asked: on, and the window wakes when it goes off.
    let (on, deadline) = r.tick(0);
    assert!(on);
    assert_eq!(deadline, r.at(500));
    // Woken at each deadline, it alternates, and each deadline is the next change.
    let mut lit = true;
    for step in 0..40 {
        let (on, deadline) = r.tick(500);
        lit = !lit;
        assert_eq!(on, lit, "step {step}");
        assert_eq!(deadline, r.at(500), "step {step}");
    }
    // A frame drawn early for another reason waits for what is left of the half-period.
    let (on, deadline) = r.tick(120);
    assert_eq!(on, lit);
    assert_eq!(deadline, r.at(380));
}

#[test]
fn the_blink_agrees_with_the_time_gone_by_on_uneven_frames() {
    // Against a plain count: the caret is on in even half-periods since the blink began.
    let half = Duration::from_millis(530);
    let mut r = Rig::new("abc");
    r.frame(0, Some(half), true);
    let mut elapsed = 0u64;
    let mut seed = 0x9E37_79B9_u64;
    for _ in 0..3000 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // Steps under the animator's stall guard (50 ms), so its time is the clock's.
        let ms = (seed >> 33) % 50;
        elapsed += ms;
        let (on, deadline) = r.frame(ms, Some(half), true);
        assert_eq!(on, (elapsed / 530).is_multiple_of(2), "at {elapsed} ms");
        let left = 530 - elapsed % 530;
        assert_eq!(deadline, r.at(left), "at {elapsed} ms");
    }
}

#[test]
fn typing_moving_the_caret_and_selecting_start_the_blink_again_with_the_caret_shown() {
    let stops: Vec<f32> = (0..=5).map(|i| i as f32 * 7.0).collect();
    let edits: [(&str, Edit); 9] = [
        ("insert", |f| {
            f.insert("x");
        }),
        ("left", |f| {
            f.key(EditKey::Left, false, false);
        }),
        ("left at the start", |f| {
            f.key(EditKey::Home, false, false);
            f.key(EditKey::Left, false, false);
        }),
        ("backspace", |f| {
            f.key(EditKey::Backspace, false, false);
        }),
        ("press", |f| f.press(10.0, &[0.0, 7.0, 14.0], false)),
        ("set_text", |f| f.set_text("new")),
        ("clear", |f| f.clear()),
        ("preedit", |f| {
            f.set_preedit(Preedit::new("ka".into(), None));
        }),
        ("handle_key", |f| {
            f.handle_key(&KeyName::Char('q'), Mods::default(), Some("q"), false);
        }),
    ];
    for (name, edit) in edits {
        let mut r = Rig::new("hello");
        r.tick(0);
        // Off, part way through.
        let (on, _) = r.tick(500);
        assert!(!on, "{name}");
        r.tick(100);
        edit(&mut r.field);
        let (on, deadline) = r.tick(10);
        assert!(on, "{name}: shown after the edit");
        assert_eq!(deadline, r.at(500), "{name}: a whole half-period on");
        assert!(!r.tick(500).0, "{name}: and then it blinks again");
    }
    // A drag that ends with no selection (back where it began) starts it again too.
    let mut r = Rig::new("hello");
    r.field.press(0.0, &stops, false);
    r.tick(0);
    r.tick(500);
    r.field.drag(0.0, &stops);
    assert_eq!(r.tick(10), (true, r.at(500)));
    // Nothing that edits or moves: it keeps its time.
    let mut r = Rig::new("hello");
    r.tick(0);
    r.tick(200);
    let (on, deadline) = r.tick(100);
    assert!(on);
    assert_eq!(deadline, r.at(200));
}

#[test]
fn focusing_starts_the_blink_with_the_caret_shown() {
    let mut r = Rig::new("hello");
    r.tick(0);
    assert!(!r.tick(500).0);
    // Focus goes elsewhere for a frame, and comes back.
    assert_eq!(r.frame(100, Some(HALF), false), (false, None));
    let (on, deadline) = r.tick(10);
    assert!(on);
    assert_eq!(deadline, r.at(500));
    // A frame in which the field is not drawn at all (another page is shown) does the same.
    r.tick(0);
    assert!(!r.tick(500).0);
    r.clock.advance_ms(100);
    r.anim.begin_frame(r.clock.now());
    assert_eq!(r.anim.end_frame(), None);
    assert_eq!(r.tick(10), (true, r.at(500)));
}

#[test]
fn an_unfocused_field_has_no_caret_and_asks_for_no_frame() {
    let mut r = Rig::new("hello");
    for ms in [0, 250, 500, 1000] {
        assert_eq!(r.frame(ms, Some(HALF), false), (false, None));
    }
    assert!(r.anim.is_empty(), "nothing is kept for it");
    // A blink that was running is dropped at the end of the first unfocused frame.
    r.tick(0);
    assert!(!r.anim.is_empty());
    assert_eq!(r.frame(10, Some(HALF), false), (false, None));
    assert!(r.anim.is_empty());
}

#[test]
fn with_blinking_switched_off_the_caret_is_steady_and_asks_for_no_frame() {
    let mut r = Rig::new("hello");
    for ms in [0, 300, 500, 700, 5000] {
        assert_eq!(r.frame(ms, None, true), (true, None));
        assert_eq!(r.frame(ms, Some(Duration::ZERO), true), (true, None));
    }
    assert!(r.anim.is_empty());
    // A blink that was running stops the frame the time goes away.
    r.tick(0);
    assert!(!r.tick(500).0);
    assert_eq!(r.frame(10, None, true), (true, None));
    // With reduced motion the animator holds it steadily on, too.
    r.anim.set_system_reduced(true);
    for ms in [0, 500, 500] {
        assert_eq!(r.tick(ms), (true, None));
    }
}

#[test]
fn a_shown_selection_hides_the_caret_and_asks_for_no_frame() {
    let mut r = Rig::new("hello");
    r.tick(0);
    r.field.select_all();
    for ms in [0, 250, 500, 750] {
        assert_eq!(r.tick(ms), (false, None));
    }
    assert!(r.anim.is_empty());
    // The selection collapses: the caret is back, on, for a whole half-period.
    r.field.key(EditKey::Right, false, false);
    assert_eq!(r.tick(10), (true, r.at(500)));
    // An input method composing over a selection hides it, so the caret shows and blinks.
    r.field.select_all();
    r.field.set_preedit(Preedit::new("ka".into(), None));
    assert_eq!(r.tick(10), (true, r.at(500)));
    // A secret draws no composition, so its selection stays shown.
    r.field.set_secret(true);
    assert_eq!(r.tick(10), (false, None));
}

// ---- drawing -----------------------------------------------------------------------------------

const COLORS: FieldColors = FieldColors {
    text: rgb(0x202020),
    placeholder: rgb(0x808080),
    selection: rgb(0x3060c0),
    caret: rgb(0xd02020),
};
const BACK: Rgb = rgb(0xffffff);
const AREA: Rect = Rect {
    x: 10.0,
    y: 10.0,
    w: 200.0,
    h: 30.0,
};

fn shot(draw: impl FnOnce(&mut Painter<'_>)) -> Bitmap {
    let mut bmp = Bitmap::new(220, 50);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, 220.0, 50.0), BACK);
        draw(&mut p);
    }
    bmp
}

fn pixel(bmp: &Bitmap, x: u32, y: u32) -> Rgb {
    let o = (y as usize * bmp.width as usize + x as usize) * 4;
    [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
}

/// Draws `field` with the blink of `anim` one frame at `now`.
fn blinking(
    field: &mut TextField,
    anim: &mut Animator,
    now: Instant,
    blink: Option<Duration>,
    focused: bool,
) -> Bitmap {
    anim.begin_frame(now);
    let bmp = shot(|p| {
        field.draw_blinking(
            p,
            anim,
            ID,
            blink,
            AREA,
            Face::Ui,
            14.0,
            &COLORS,
            "Search",
            focused,
        )
    });
    anim.end_frame();
    bmp
}

fn plain(field: &TextField, focused: bool, caret_on: bool) -> Bitmap {
    let mut field = field.clone();
    shot(|p| {
        field.draw(
            p,
            AREA,
            Face::Ui,
            14.0,
            &COLORS,
            "Search",
            focused,
            caret_on,
        )
    })
}

#[test]
fn the_caret_is_drawn_in_the_on_phase_and_absent_in_the_off_phase() {
    for text in ["", "hello"] {
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut field = TextField::new();
        field.set_text(text);
        // The caret's column: at the left edge when empty, after the text otherwise.
        let x = if text.is_empty() {
            AREA.x
        } else {
            let mut gfx = Gfx::new();
            let mut bmp = Bitmap::new(1, 1);
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            AREA.x
                + p.caret_stops(Face::Ui, 14.0, text)
                    .last()
                    .copied()
                    .unwrap_or(0.0)
        };
        let (cx, cy) = (x as u32, AREA.cy() as u32);

        let on = blinking(&mut field, &mut anim, clock.now(), Some(HALF), true);
        assert_eq!(pixel(&on, cx, cy), COLORS.caret, "{text:?}: on");
        assert_eq!(on.pixels, plain(&field, true, true).pixels, "{text:?}");

        clock.advance_ms(500);
        let off = blinking(&mut field, &mut anim, clock.now(), Some(HALF), true);
        assert_eq!(pixel(&off, cx, cy), BACK, "{text:?}: off");
        assert_eq!(off.pixels, plain(&field, true, false).pixels, "{text:?}");

        clock.advance_ms(500);
        let again = blinking(&mut field, &mut anim, clock.now(), Some(HALF), true);
        assert_eq!(again.pixels, on.pixels, "{text:?}: on again");

        // Blinking switched off: steadily the "on" drawing.
        clock.advance_ms(500);
        let steady = blinking(&mut field, &mut anim, clock.now(), None, true);
        assert_eq!(steady.pixels, on.pixels, "{text:?}: steady");

        // Not focused: the plain unfocused drawing, which has no caret.
        let away = blinking(&mut field, &mut anim, clock.now(), Some(HALF), false);
        assert_eq!(away.pixels, plain(&field, false, true).pixels, "{text:?}");
        assert_eq!(pixel(&away, cx, cy), BACK, "{text:?}: unfocused");
    }
}

#[test]
fn a_selection_is_drawn_without_a_caret() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let mut field = TextField::new();
    field.set_text("hello");
    field.select_all();
    let bmp = blinking(&mut field, &mut anim, clock.now(), Some(HALF), true);
    assert_eq!(bmp.pixels, plain(&field, true, false).pixels);
    assert_ne!(
        bmp.pixels,
        plain(&field, true, true).pixels,
        "draw shows one"
    );
}
