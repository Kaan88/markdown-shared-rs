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
        if hit.line(&field.display(), &mut gfx).width <= 0.0 {
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
        let shown = self.field.display();
        let byte = shown
            .char_indices()
            .nth(index)
            .map_or(shown.len(), |(b, _)| b);
        let line = self.hit.line(&shown, &mut self.gfx);
        let x = line.x(super::super::line::Stop {
            byte,
            affinity: parley::Affinity::Downstream,
        });
        (
            snapped_origin(self.hit, self.field.scroll) + x,
            self.hit.area.cy(),
        )
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
    if r.hit.line(&r.field.display(), &mut r.gfx).width <= 0.0 {
        eprintln!("skipped: no advancing bullet glyph in the system fonts");
        return;
    }
    let at = r.at(1);
    r.click(at, false);
    assert_eq!(r.field.caret(), (2, 2));
    r.click(at, false);
    assert_eq!(r.field.caret(), (r.field.text().len(), 0));
    assert_eq!(r.field.selection_for_export(), None);
}

#[test]
fn rtl_left_edge_hits_the_logical_end_and_decomposed_word_keeps_its_accent() {
    let Some(mut r) = Rig::new("אבג") else {
        return;
    };
    r.click((r.hit.area.x + 0.5, r.hit.area.cy()), false);
    assert_eq!(
        r.field.caret(),
        (r.field.text().len(), r.field.text().len())
    );

    let Some(mut r) = Rig::new("e\u{301} z") else {
        return;
    };
    let start = r.hit.area.x;
    let end_of_cluster = r.at(2).0;
    let y = r.hit.area.cy();
    assert!(end_of_cluster > start);
    r.click((start + (end_of_cluster - start) * 0.8, y), false);
    assert_eq!(
        r.field.caret(),
        (3, 3),
        "the accent has no caret stop of its own"
    );
    r.click((start + (end_of_cluster - start) * 0.8, y), false);
    assert_eq!(r.field.selection_for_export(), Some("e\u{301}"));
}

#[test]
fn a_mixed_direction_selection_paints_two_bands_with_an_unselected_gap() {
    let Some(mut r) = Rig::new("a אבג b") else {
        return;
    };
    r.field.key(EditKey::Home, false, false);
    for _ in 0..3 {
        r.field.key(EditKey::Right, false, true);
    }
    assert_eq!(r.field.selection_for_export(), Some("a א"));
    let mut bitmap = Bitmap::new(420, 75);
    let mut p = Painter::new(&mut bitmap, &mut r.gfx, r.hit.scale);
    p.fill(Rect::new(0.0, 0.0, 280.0, 50.0), [255; 3]);
    r.field.draw(
        &mut p,
        r.hit.area,
        r.hit.face,
        r.hit.size,
        &FieldColors {
            text: [255; 3],
            placeholder: [100; 3],
            selection: [0, 100, 255],
            caret: [0; 3],
        },
        "",
        true,
        false,
    );
    // Above the glyphs, but within the selection highlight. Count contiguous coloured runs.
    let y = ((r.hit.area.cy() - r.hit.size * 0.625) * r.hit.scale).ceil() as usize;
    let row = &bitmap.pixels[y * 420 * 4..(y + 1) * 420 * 4];
    let mut last = false;
    let mut bands = 0;
    for px in row.as_chunks::<4>().0 {
        let colored = px[2] < 254;
        bands += usize::from(colored && !last);
        last = colored;
    }
    assert_eq!(bands, 2);
}

#[test]
fn drawing_a_long_field_does_not_shape_its_prefixes() {
    let Some(mut r) = Rig::new(&"abcd ".repeat(128)) else {
        return;
    };
    let mut gfx = Gfx::new();
    let mut bitmap = Bitmap::new(420, 75);
    let mut p = Painter::new(&mut bitmap, &mut gfx, r.hit.scale);
    r.field.draw(
        &mut p,
        r.hit.area,
        r.hit.face,
        r.hit.size,
        &FieldColors {
            text: [0; 3],
            placeholder: [100; 3],
            selection: [0, 100, 255],
            caret: [0; 3],
        },
        "",
        true,
        true,
    );
    assert_eq!(gfx.cached_layouts(), 1);
}

#[test]
fn a_public_preedit_cursor_inside_a_character_is_discarded_before_drawing() {
    let Some(mut r) = Rig::new("base") else {
        return;
    };
    assert!(r.field.set_preedit(Some(super::super::Preedit {
        text: "é".into(),
        cursor: Some((1, 1)),
    })));
    assert_eq!(r.field.preedit().unwrap().cursor, None);
    let mut bitmap = Bitmap::new(420, 75);
    let mut p = Painter::new(&mut bitmap, &mut r.gfx, r.hit.scale);
    r.field.draw(
        &mut p,
        r.hit.area,
        r.hit.face,
        r.hit.size,
        &FieldColors {
            text: [0; 3],
            placeholder: [100; 3],
            selection: [0, 100, 255],
            caret: [0; 3],
        },
        "",
        true,
        true,
    );
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
