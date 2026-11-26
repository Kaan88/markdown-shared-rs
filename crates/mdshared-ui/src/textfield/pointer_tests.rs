//! Mouse input against measured stops and the pixels produced by the ordinary draw method.

use super::*;
use crate::{
    anim::{Clock, ManualClock},
    gfx::Painter,
    textfield::{EditKey, FieldColors},
};
use mdshared_text::Bitmap;

struct Rig {
    gfx: Gfx,
    field: TextField,
    pointer: Pointer,
    hit: FieldHit,
    clock: ManualClock,
}

impl Rig {
    fn new(text: &str) -> Option<Self> {
        let mut field = TextField::new();
        field.set_text(text);
        let mut gfx = Gfx::new();
        let hit = FieldHit {
            area: Rect::new(12.0, 8.0, 240.0, 28.0),
            face: Face::Mono,
            size: 16.0,
            scale: 1.5,
        };
        if hit.stops(&field, &mut gfx).get(1).copied().unwrap_or(0.0) <= 0.0 {
            eprintln!("skipped: no advancing glyph for the test text in the system fonts");
            return None;
        }
        Some(Self {
            gfx,
            field,
            pointer: Pointer::new(Duration::from_millis(500), (2.0, 3.0)),
            hit,
            clock: ManualClock::new(),
        })
    }

    fn at(&mut self, index: usize) -> (f32, f32) {
        let x = self.hit.stops(&self.field, &mut self.gfx)[index];
        (self.hit.area.x + x - self.field.scroll, self.hit.area.cy())
    }

    fn press(&mut self, at: (f32, f32), shift: bool) -> bool {
        self.pointer.press(
            &mut self.field,
            &mut self.gfx,
            self.hit,
            at,
            Mods {
                shift,
                ..Mods::default()
            },
            self.clock.now(),
        )
    }

    fn click(&mut self, at: (f32, f32), shift: bool) {
        assert!(self.press(at, shift));
        self.pointer.release();
    }

    fn move_to(&mut self, at: (f32, f32)) -> bool {
        self.pointer.pointer_move(
            &mut self.field,
            &mut self.gfx,
            self.hit,
            at,
            self.clock.now(),
        )
    }

    fn tick(&mut self) -> bool {
        self.pointer
            .tick(&mut self.field, &mut self.gfx, self.hit, self.clock.now())
    }
}

#[test]
fn click_places_the_caret_and_shift_and_drag_keep_the_original_anchor() {
    let Some(mut r) = Rig::new("héllo world") else {
        return;
    };
    let start = r.at(1);
    let end = r.at(5);
    let left = r.at(0);
    assert_eq!(r.hit.cursor(start), Some(Cursor::Text));
    assert_eq!(r.hit.cursor((r.hit.area.right(), start.1)), None);
    assert!(!r.press((f32::NAN, start.1), false));
    assert!(r.press(start, false));
    assert_eq!(r.field.caret(), (1, 1));
    assert!(r.move_to(end));
    assert_eq!(r.field.selected_text(), "éllo");
    assert!(r.move_to(left));
    assert_eq!(r.field.caret(), (0, 1));
    r.pointer.release();
    assert!(!r.move_to(end));
    assert_eq!(r.field.selected_text(), "h");
    r.click(end, true);
    assert_eq!(r.field.selected_text(), "éllo");
    r.click(end, false);
    assert_eq!(r.field.caret(), (6, 6));
}

#[test]
fn clicks_at_the_time_and_distance_limits_select_a_word_then_the_whole_line() {
    let Some(mut r) = Rig::new("one two three") else {
        return;
    };
    let (x, y) = r.at(5);
    let at = (x + 1.0, y);
    r.click(at, false);
    assert!(!r.field.has_selection());
    r.clock.advance(Duration::from_millis(500));
    assert!(r.press((at.0 + 2.0, y + 3.0), false));
    assert_eq!(r.field.selected_text(), "two");
    assert!(!r.move_to((at.0 + 2.0, y + 2.0)));
    assert_eq!(r.field.selected_text(), "two");
    r.pointer.release();
    r.clock.advance(Duration::from_millis(500));
    r.click(at, false);
    assert_eq!(r.field.selected_text(), "one two three");

    for (delay, point) in [(501, at), (5, (at.0 + 2.1, y)), (5, (at.0, y + 3.1))] {
        r.pointer.cancel();
        r.click(at, false);
        r.clock.advance(Duration::from_millis(delay));
        r.click(point, false);
        assert!(!r.field.has_selection());
    }
}

#[test]
fn a_drag_breaks_the_click_sequence_even_when_the_next_press_is_nearby() {
    let Some(mut r) = Rig::new("one two") else {
        return;
    };
    let a = r.at(0);
    let b = r.at(2);
    assert!(r.press(a, false));
    assert!(r.move_to(b));
    r.pointer.release();
    r.click(a, false);
    assert_eq!(r.field.caret(), (0, 0));
}

#[test]
fn dragging_outside_scrolls_in_both_directions_and_stops_at_the_ends_or_release() {
    let Some(mut r) = Rig::new("01234567890123456789 01234567890123456789") else {
        return;
    };
    r.hit.area.w = 48.0;
    let a = r.at(2);
    let right = (r.hit.area.right() + 0.01, a.1);
    let left = (r.hit.area.x - 0.01, a.1);
    assert!(r.press(a, false));
    assert!(r.move_to(right));
    let first_scroll = r.field.scroll;
    assert!(first_scroll > 0.0);
    assert_eq!(r.pointer.deadline(), Some(r.clock.now() + SCROLL_INTERVAL));
    assert!(!r.tick());
    assert!(!r.move_to((right.0 + 1.0, right.1)));
    assert_eq!(r.field.scroll, first_scroll);
    for _ in 0..r.field.text().len() {
        r.clock.advance(SCROLL_INTERVAL);
        if !r.tick() {
            break;
        }
    }
    assert_eq!(r.field.caret(), (r.field.text().len(), 2));
    assert!(r.pointer.deadline().is_none());

    let right_scroll = r.field.scroll;
    assert!(r.move_to(left));
    assert!(r.field.scroll < right_scroll);
    assert!(r.pointer.deadline().is_some());
    r.pointer.release();
    r.clock.advance(SCROLL_INTERVAL);
    assert!(!r.tick());
    assert!(r.pointer.deadline().is_none());
    assert!(!r.move_to(left));

    let a = (r.hit.area.cx(), r.hit.area.cy());
    assert!(r.press(a, false));
    assert!(r.move_to(left));
    for _ in 0..r.field.text().len() {
        r.clock.advance(SCROLL_INTERVAL);
        if !r.tick() {
            break;
        }
    }
    assert_eq!(r.field.scroll, 0.0);
    assert_eq!(r.field.caret().0, 0);
    assert!(r.pointer.deadline().is_none());
}

#[test]
fn the_secret_uses_masked_stops_and_double_click_does_not_expose_word_boundaries() {
    let Some(mut r) = Rig::new("é ç") else {
        return;
    };
    r.field.set_secret(true);
    if r.hit.stops(&r.field, &mut r.gfx)[1] <= 0.0 {
        eprintln!("skipped: no advancing bullet glyph in the system fonts");
        return;
    }
    let at = r.at(1);
    r.click(at, false);
    assert_eq!(r.field.caret(), (2, 2));
    r.click(at, false);
    assert_eq!(r.field.selected_text(), "é ç");
}

#[test]
fn a_drag_paints_the_same_selection_pixels_as_the_equivalent_keyboard_selection() {
    let Some(mut r) = Rig::new("one two") else {
        return;
    };
    let start = r.at(1);
    let end = r.at(5);
    let mut reference = r.field.clone();
    let hit = r.hit;
    let paint = |field: &mut TextField, gfx: &mut Gfx| {
        let mut bitmap = Bitmap::new(420, 75);
        let mut p = Painter::new(&mut bitmap, gfx, hit.scale);
        p.fill(Rect::new(0.0, 0.0, 280.0, 50.0), [255; 3]);
        field.draw(
            &mut p,
            hit.area,
            hit.face,
            hit.size,
            &FieldColors {
                text: [0; 3],
                placeholder: [100; 3],
                selection: [0, 100, 255],
                caret: [0; 3],
            },
            "",
            true,
            false,
        );
        bitmap.pixels
    };
    let unselected = paint(&mut reference, &mut r.gfx);
    reference.key(EditKey::Home, false, false);
    reference.key(EditKey::Right, false, false);
    for _ in 1..5 {
        reference.key(EditKey::Right, false, true);
    }
    let expected = paint(&mut reference, &mut r.gfx);
    assert_ne!(expected, unselected);
    assert!(r.press(start, false));
    r.move_to(end);
    r.pointer.release();
    let actual = paint(&mut r.field, &mut r.gfx);
    assert!(
        actual == expected,
        "drag did not draw the keyboard selection's pixels"
    );
}
