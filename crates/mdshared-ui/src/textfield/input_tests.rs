//! One field's input driven as an application drives it: presses, moves and releases at the
//! places the measured text puts its characters, the menu placed by drawing it, a clock moved
//! by hand. The clipboard is the test fixture's; nothing touches the desktop's.

use super::*;
use crate::anim::{Clock, ManualClock};
use crate::gfx::Face;
use crate::textfield::paste_fixture;
use crate::theme::LIGHT;
use mdshared_text::Bitmap;

const WINDOW: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 420.0,
    h: 320.0,
};

struct Rig {
    gfx: Gfx,
    /// A second text context, only for measuring where characters are, so that `at` can be
    /// asked while the rig is borrowed for an event.
    measure: std::cell::RefCell<Gfx>,
    field: TextField,
    input: FieldInput,
    area: FieldArea,
    clock: ManualClock,
}

fn mods(shift: bool) -> Mods {
    Mods {
        shift,
        ..Mods::default()
    }
}

impl Rig {
    /// A field whose frame has 30 px of padding left of the text (where an icon would be) and
    /// 30 px right of it.
    fn new(text: &str) -> Option<Rig> {
        let mut field = TextField::new();
        field.set_text(text);
        let mut gfx = Gfx::new();
        let hit = FieldHit {
            area: Rect::new(40.0, 10.0, 200.0, 24.0),
            face: Face::Mono,
            size: 16.0,
            scale: 1.0,
        };
        if hit.line(&field.display(), &mut gfx).width <= 0.0 {
            eprintln!("skipped: no advancing glyph for the test text in the system fonts");
            return None;
        }
        Some(Rig {
            gfx,
            measure: std::cell::RefCell::new(Gfx::new()),
            field,
            input: FieldInput::new().timing(Duration::from_millis(500), (2.0, 3.0)),
            area: FieldArea::new(Rect::new(10.0, 6.0, 260.0, 32.0), hit),
            clock: ManualClock::new(),
        })
    }

    /// The point before char `index` as the text is drawn now.
    fn at(&self, index: usize) -> (f32, f32) {
        let hit = self.area.text();
        let shown = self.field.display();
        let byte = shown
            .char_indices()
            .nth(index)
            .map_or(shown.len(), |(b, _)| b);
        let line = hit.line(&shown, &mut self.measure.borrow_mut());
        let x = line.x(super::super::line::Stop {
            byte,
            affinity: parley::Affinity::Downstream,
        });
        (
            pointer::snapped_origin(hit, self.field.scroll) + x,
            hit.area.cy(),
        )
    }

    fn press_with(&mut self, at: (f32, f32), shift: bool) -> FieldOutcome {
        let now = self.clock.now();
        self.input.press(
            &mut self.field,
            &mut self.gfx,
            self.area,
            at,
            mods(shift),
            now,
        )
    }

    fn press(&mut self, at: (f32, f32)) -> FieldOutcome {
        self.press_with(at, false)
    }

    fn move_to(&mut self, at: (f32, f32)) -> FieldOutcome {
        let now = self.clock.now();
        self.input
            .pointer_move(&mut self.field, &mut self.gfx, self.area, at, now)
    }

    fn release(&mut self, at: (f32, f32)) -> FieldOutcome {
        let now = self.clock.now();
        self.input
            .release(&mut self.field, &mut self.gfx, self.area, at, now)
    }

    fn click(&mut self, at: (f32, f32)) {
        assert_eq!(self.press(at), FieldOutcome::Focus);
        self.release(at);
    }

    fn right(&mut self, at: (f32, f32)) -> FieldOutcome {
        let now = self.clock.now();
        self.input
            .secondary_press(&mut self.field, &mut self.gfx, self.area, at, now)
    }

    fn key(&mut self, key: KeyName, m: Mods, typed: Option<&str>) -> FieldOutcome {
        self.input
            .key(&mut self.field, &mut self.gfx, self.area, &key, m, typed)
    }

    fn tick(&mut self) -> FieldOutcome {
        let now = self.clock.now();
        self.input
            .tick(&mut self.field, &mut self.gfx, self.area, now)
    }

    /// Draws the open menu, which places it, as a frame would.
    fn place_menu(&mut self) {
        let mut bitmap = Bitmap::new(WINDOW.w as u32, WINDOW.h as u32);
        let mut p = Painter::new(&mut bitmap, &mut self.gfx, 1.0);
        self.input.draw_menu(&mut p, &LIGHT, WINDOW);
    }

    /// The middle of row `i` of the placed menu.
    fn row(&self, i: usize) -> (f32, f32) {
        let menu = self.input.menu().and_then(|m| m.menu(0)).expect("open");
        let r = menu.row_rect(i);
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    fn labels(&self) -> Vec<String> {
        let menu = self.input.menu().and_then(|m| m.menu(0)).expect("open");
        menu.items.iter().map(|i| i.label.clone()).collect()
    }
}

#[test]
fn a_click_places_the_caret_a_drag_selects_and_a_move_after_the_release_does_nothing() {
    let Some(mut r) = Rig::new("alpha beta") else {
        return;
    };
    let (a1, a5) = (r.at(1), r.at(5));
    assert_eq!(r.press(a1), FieldOutcome::Focus);
    assert_eq!(r.field.caret(), (1, 1));
    assert!(r.input.is_dragging());
    assert_eq!(r.move_to(a5), FieldOutcome::Redraw);
    assert_eq!(r.field.selected_text(), "lpha");
    // Held, a move that changes nothing is still the field's.
    assert_eq!(r.move_to(a5), FieldOutcome::Taken);
    assert_eq!(r.release(a5), FieldOutcome::Taken);
    assert!(!r.input.is_dragging());
    assert_eq!(r.move_to(r.at(0)), FieldOutcome::Unused);
    assert_eq!(r.field.selected_text(), "lpha");
    assert_eq!(r.release(a5), FieldOutcome::Unused);
}

#[test]
fn shift_click_extends_and_double_then_triple_click_select_a_word_then_everything() {
    let Some(mut r) = Rig::new("alpha beta gamma") else {
        return;
    };
    let b = r.at(7);
    r.click(r.at(2));
    r.clock.advance(Duration::from_secs(1));
    assert_eq!(r.press_with(r.at(4), true), FieldOutcome::Focus);
    r.release(r.at(4));
    assert_eq!(r.field.selected_text(), "ph");
    r.clock.advance(Duration::from_secs(1));
    r.click(b);
    r.clock.advance(Duration::from_millis(100));
    r.click(b);
    assert_eq!(r.field.selected_text(), "beta");
    r.clock.advance(Duration::from_millis(100));
    r.click(b);
    assert_eq!(r.field.selected_text(), "alpha beta gamma");
}

#[test]
fn a_press_in_the_padding_places_the_caret_at_the_nearest_end_of_the_text() {
    let Some(mut r) = Rig::new("alpha") else {
        return;
    };
    let frame = r.area.frame();
    let y = frame.y + 2.0; // above the text rectangle, inside the frame
    assert_eq!(r.press((frame.x + 2.0, y)), FieldOutcome::Focus);
    r.release((frame.x + 2.0, y));
    assert_eq!(r.field.caret(), (0, 0));
    r.clock.advance(Duration::from_secs(1));
    assert_eq!(r.press((frame.right() - 2.0, y)), FieldOutcome::Focus);
    r.release((frame.right() - 2.0, y));
    assert_eq!(r.field.caret(), (5, 5));
    // Outside the frame the press is not the field's and moves nothing.
    assert_eq!(r.press((frame.right() + 5.0, y)), FieldOutcome::Unused);
    assert_eq!(
        r.press((frame.x + 2.0, frame.bottom() + 5.0)),
        FieldOutcome::Unused
    );
    assert_eq!(r.field.caret(), (5, 5));
}

#[test]
fn a_press_elsewhere_ends_the_drag_and_breaks_the_run_of_clicks() {
    let Some(mut r) = Rig::new("alpha beta") else {
        return;
    };
    let b = r.at(7);
    r.press(r.at(1));
    r.move_to(r.at(3));
    assert_eq!(r.press((400.0, 300.0)), FieldOutcome::Unused);
    assert!(!r.input.is_dragging());
    assert_eq!(r.move_to(r.at(5)), FieldOutcome::Unused);
    assert_eq!(r.field.selected_text(), "lp");
    // Two quick clicks with a press elsewhere between them are two first clicks.
    r.click(b);
    r.press((400.0, 300.0));
    r.click(b);
    assert!(!r.field.has_selection());
    // `other_press` (the middle button) does the same.
    r.click(b);
    assert_eq!(r.input.other_press(b), FieldOutcome::Unused);
    r.click(b);
    assert!(!r.field.has_selection());
}

#[test]
fn the_release_extends_to_where_the_button_went_up_without_a_move_there() {
    let Some(mut r) = Rig::new("alpha beta") else {
        return;
    };
    r.press(r.at(0));
    assert_eq!(r.release(r.at(4)), FieldOutcome::Redraw);
    assert_eq!(r.field.selected_text(), "alph");
}

#[test]
fn dragging_past_the_end_scrolls_on_ticks_and_the_release_stops_it() {
    let text = "a long line that is much wider than the two hundred pixels of the field";
    let Some(mut r) = Rig::new(text) else {
        return;
    };
    assert_eq!(r.input.deadline(), None);
    r.press(r.at(2));
    let beyond = (r.area.text().area.right() + 30.0, r.area.text().area.cy());
    assert_eq!(r.move_to(beyond), FieldOutcome::Redraw);
    let first = r
        .input
        .deadline()
        .expect("edge scrolling waits for its next step");
    assert_eq!(r.tick(), FieldOutcome::Unused, "not yet due");
    let caret = r.field.caret().0;
    r.clock.advance(first - r.clock.now());
    assert_eq!(r.tick(), FieldOutcome::Redraw);
    assert!(r.field.caret().0 > caret);
    assert_eq!(r.release(beyond), FieldOutcome::Taken);
    assert_eq!(r.input.deadline(), None);
    r.clock.advance(Duration::from_secs(1));
    assert_eq!(r.tick(), FieldOutcome::Unused);
}

#[test]
fn cancel_ends_a_drag_and_closes_the_menu_and_keeps_the_text() {
    let Some(mut r) = Rig::new("alpha beta") else {
        return;
    };
    r.press(r.at(1));
    r.move_to(r.at(4));
    assert!(!r.input.cancel(), "no menu was open");
    assert!(!r.input.is_dragging());
    assert_eq!(r.input.deadline(), None);
    assert_eq!(r.move_to(r.at(8)), FieldOutcome::Unused);
    assert_eq!(r.field.selected_text(), "lph");
    r.right(r.at(2));
    assert!(r.input.menu_open());
    assert!(r.input.cancel());
    assert!(!r.input.menu_open());
    assert!(!r.input.close_menu());
    assert_eq!(r.field.text(), "alpha beta");
}

#[test]
fn a_right_press_moves_a_lone_caret_but_keeps_a_selection_and_opens_the_menu() {
    let Some(mut r) = Rig::new("alpha beta") else {
        return;
    };
    assert_eq!(r.right(r.at(3)), FieldOutcome::Focus);
    assert_eq!(r.field.caret(), (3, 3));
    assert!(r.input.menu_open());
    assert_eq!(
        r.key(KeyName::Escape, mods(false), None),
        FieldOutcome::Redraw
    );
    assert!(!r.input.menu_open());
    r.field.select_all();
    assert_eq!(r.right(r.at(1)), FieldOutcome::Focus);
    assert_eq!(r.field.selected_text(), "alpha beta");
    // A right press elsewhere closes it and goes no further; with nothing open it is unused.
    assert_eq!(r.right((400.0, 300.0)), FieldOutcome::Redraw);
    assert!(!r.input.menu_open());
    assert_eq!(r.right((400.0, 300.0)), FieldOutcome::Unused);
}

#[test]
fn the_open_menu_takes_every_event_and_a_press_outside_it_reaches_nothing_else() {
    let Some(mut r) = Rig::new("alpha beta") else {
        return;
    };
    r.right(r.at(6));
    r.place_menu();
    let menu = r.input.menu_bounds().expect("open");
    assert!(r.move_to((menu.x + 5.0, menu.y + 5.0)).taken());
    assert!(r.move_to((400.0, 300.0)).taken());
    assert_eq!(
        r.key(KeyName::Char('a'), mods(false), Some("a")),
        FieldOutcome::Taken
    );
    assert_eq!(r.input.text(&mut r.field, "x"), FieldOutcome::Taken);
    assert_eq!(r.release(r.at(1)), FieldOutcome::Taken);
    assert!(r.input.wheel((400.0, 300.0), 40.0).taken());
    assert_eq!(r.field.text(), "alpha beta");
    // A press on the field outside the menu closes it and places no caret.
    let caret = r.field.caret();
    let left = r.at(0);
    assert!(!menu.contains(left.0, left.1));
    assert_eq!(r.press(left), FieldOutcome::Redraw);
    assert!(!r.input.menu_open());
    assert_eq!(r.field.caret(), caret);
    assert!(!r.input.is_dragging());
}

#[test]
fn a_chosen_item_is_applied_and_paste_comes_back_as_an_edit() {
    let Some(mut r) = Rig::new("alpha beta") else {
        return;
    };
    let _clipboard = paste_fixture("new");
    r.right(r.at(2));
    r.place_menu();
    assert_eq!(
        r.press(r.row(3)),
        FieldOutcome::Redraw,
        "Select all edits nothing"
    );
    assert!(!r.input.menu_open());
    assert_eq!(r.field.selected_text(), "alpha beta");
    r.right(r.at(2));
    r.place_menu();
    assert_eq!(r.press(r.row(2)), FieldOutcome::Edited);
    assert_eq!(r.field.text(), "new");
    // The keyboard chooses too: Up from nothing lit is the last row, Select all.
    r.right(r.at(1));
    assert_eq!(r.key(KeyName::Up, mods(false), None), FieldOutcome::Redraw);
    assert_eq!(
        r.key(KeyName::Enter, mods(false), None),
        FieldOutcome::Redraw
    );
    assert_eq!(r.field.selected_text(), "new");
}

#[test]
fn labels_word_the_menu_from_its_next_opening() {
    let Some(mut r) = Rig::new("kelime") else {
        return;
    };
    r.input = FieldInput::new().labels(EditLabels::new("Kes", "Kopyala", "Yapıştır", "Tümü"));
    r.right(r.at(1));
    assert_eq!(r.labels(), ["Kes", "Kopyala", "Yapıştır", "Tümü"]);
    r.input.set_labels(EditLabels::ENGLISH);
    assert_eq!(r.labels()[0], "Kes", "an open menu keeps its words");
    r.input.cancel();
    r.right(r.at(1));
    assert_eq!(r.labels(), ["Cut", "Copy", "Paste", "Select all"]);
}

#[test]
fn shift_f10_opens_the_menu_under_the_caret() {
    let Some(mut r) = Rig::new("alpha beta") else {
        return;
    };
    r.click(r.at(6));
    let caret = r.at(6);
    let shift_f10 = |r: &mut Rig| r.key(KeyName::F(10), mods(true), None);
    assert_eq!(shift_f10(&mut r), FieldOutcome::Redraw);
    assert!(r.input.menu_open());
    r.place_menu();
    let menu = r.input.menu().and_then(|m| m.menu(0)).unwrap().rect();
    assert!((menu.x - caret.0).abs() <= 1.0, "{} vs {}", menu.x, caret.0);
    assert!(menu.y >= r.area.text().area.bottom() - 1.0);
    r.input.cancel();
    // F10 alone, or with Ctrl, is the caller's.
    assert_eq!(
        r.key(KeyName::F(10), mods(false), None),
        FieldOutcome::Unused
    );
    let ctrl_shift = Mods {
        ctrl: true,
        shift: true,
        ..Mods::default()
    };
    assert_eq!(
        r.key(KeyName::F(10), ctrl_shift, None),
        FieldOutcome::Unused
    );
    assert!(!r.input.menu_open());
}

#[test]
fn editing_keys_edit_or_move_and_the_others_are_the_callers() {
    let Some(mut r) = Rig::new("alpha") else {
        return;
    };
    assert_eq!(
        r.key(KeyName::Backspace, mods(false), None),
        FieldOutcome::Edited
    );
    assert_eq!(r.field.text(), "alph");
    assert_eq!(
        r.key(KeyName::Left, mods(false), None),
        FieldOutcome::Redraw
    );
    assert_eq!(
        r.key(KeyName::Char('x'), mods(false), Some("x")),
        FieldOutcome::Edited
    );
    assert_eq!(r.field.text(), "alpxh");
    for key in [
        KeyName::Enter,
        KeyName::Escape,
        KeyName::Tab,
        KeyName::Up,
        KeyName::Down,
    ] {
        assert_eq!(r.key(key, mods(false), None), FieldOutcome::Unused);
    }
    // An editing key ends a drag.
    r.press(r.at(1));
    assert_eq!(r.key(KeyName::End, mods(false), None), FieldOutcome::Redraw);
    assert!(!r.input.is_dragging());
}

#[test]
fn committed_text_and_preedits_reach_the_field() {
    let Some(mut r) = Rig::new("ab") else {
        return;
    };
    assert_eq!(r.input.text(&mut r.field, "ç"), FieldOutcome::Edited);
    assert_eq!(r.field.text(), "abç");
    assert_eq!(r.input.text(&mut r.field, ""), FieldOutcome::Taken);
    let pre = Preedit::new("ka".into(), None);
    assert_eq!(
        r.input.preedit(&mut r.field, pre.clone()),
        FieldOutcome::Redraw
    );
    assert_eq!(r.input.preedit(&mut r.field, pre), FieldOutcome::Taken);
    assert_eq!(r.input.preedit(&mut r.field, None), FieldOutcome::Redraw);
}

#[test]
fn the_wheel_is_only_the_fields_while_its_menu_is_open() {
    let Some(mut r) = Rig::new("alpha") else {
        return;
    };
    assert_eq!(r.input.wheel((50.0, 20.0), 40.0), FieldOutcome::Unused);
    r.right(r.at(1));
    r.place_menu();
    assert!(r.input.wheel((50.0, 20.0), 40.0).taken());
    assert!(r.input.wheel((50.0, 20.0), f32::NAN).taken());
}

#[test]
fn the_cursor_is_the_arrow_on_a_menu_the_i_beam_on_the_field_and_through_a_drag() {
    let Some(mut r) = Rig::new("alpha") else {
        return;
    };
    let area = r.area;
    let far = (400.0, 300.0);
    assert_eq!(
        r.input.cursor_at(area, (area.frame().x + 1.0, 20.0)),
        Some(Cursor::Text)
    );
    assert_eq!(r.input.cursor_at(area, far), None);
    r.press(r.at(1));
    assert_eq!(r.input.cursor_at(area, far), Some(Cursor::Text));
    r.release(r.at(1));
    r.right(r.at(1));
    assert_eq!(r.input.cursor_at(area, r.at(1)), Some(Cursor::Default));
    assert_eq!(r.input.cursor_at(area, far), Some(Cursor::Default));
}

#[test]
fn the_menu_bounds_cover_the_placed_menu_and_nothing_is_drawn_when_closed() {
    let Some(mut r) = Rig::new("alpha") else {
        return;
    };
    assert_eq!(r.input.menu_bounds(), None);
    // Drawing a closed menu draws nothing.
    let mut bitmap = Bitmap::new(WINDOW.w as u32, WINDOW.h as u32);
    bitmap.clear(LIGHT.page);
    let before = bitmap.clone();
    {
        let mut p = Painter::new(&mut bitmap, &mut r.gfx, 1.0);
        r.input.draw_menu(&mut p, &LIGHT, WINDOW);
    }
    assert!(bitmap.pixels == before.pixels);
    r.right(r.at(1));
    r.place_menu();
    let menu = r.input.menu().and_then(|m| m.menu(0)).unwrap().rect();
    let bounds = r.input.menu_bounds().unwrap();
    assert!(bounds.x <= menu.x && bounds.right() >= menu.right());
    assert!(bounds.y <= menu.y && bounds.bottom() >= menu.bottom());
    assert!(r.input.menu_mut().is_some());
}

#[test]
fn a_new_scale_makes_a_new_pointer_so_a_run_of_clicks_starts_again() {
    let Some(mut r) = Rig::new("alpha beta") else {
        return;
    };
    let b = r.at(7);
    r.click(b);
    let mut hit = r.area.text();
    hit.scale = 1.5;
    r.area = FieldArea::new(r.area.frame(), hit);
    r.click(b);
    assert!(
        !r.field.has_selection(),
        "a first click, not a double click"
    );
}

#[test]
fn nonsense_positions_and_areas_do_not_panic_or_take_presses() {
    let Some(mut r) = Rig::new("alpha") else {
        return;
    };
    for at in [
        (f32::NAN, 20.0),
        (50.0, f32::INFINITY),
        (f32::NEG_INFINITY, f32::NAN),
    ] {
        assert_eq!(r.press(at), FieldOutcome::Unused);
        assert_eq!(r.right(at), FieldOutcome::Unused);
        assert_eq!(r.move_to(at), FieldOutcome::Unused);
        assert!(!r.area.contains(at));
    }
    let good = r.area.text();
    // A frame not made of numbers counts as the text rectangle alone.
    let nan = Rect::new(f32::NAN, 0.0, f32::INFINITY, 10.0);
    r.area = FieldArea::new(nan, good);
    assert!(r.area.contains(r.at(1)));
    assert!(!r.area.contains((15.0, 20.0)));
    // A text rectangle with no size: a press in the frame still focuses, places nothing, and
    // the menu opens at the frame's corner for Shift+F10.
    let mut empty = good;
    empty.area = Rect::new(40.0, 10.0, 0.0, 0.0);
    r.area = FieldArea::new(Rect::new(10.0, 6.0, 260.0, 32.0), empty);
    let caret = r.field.caret();
    assert_eq!(r.press((100.0, 20.0)), FieldOutcome::Focus);
    assert_eq!(r.field.caret(), caret);
    assert_eq!(r.move_to((120.0, 20.0)), FieldOutcome::Unused);
    assert_eq!(
        r.key(KeyName::F(10), mods(true), None),
        FieldOutcome::Redraw
    );
    r.place_menu();
    let menu = r.input.menu().and_then(|m| m.menu(0)).unwrap().rect();
    assert!((menu.x - 10.0).abs() <= 1.0);
    // A text rectangle not made of numbers inside a usable frame: focused, nothing placed,
    // no panic in the padding's nearest-end arithmetic.
    r.input.cancel();
    let mut nan_text = good;
    nan_text.area = Rect::new(f32::NAN, f32::NAN, f32::NAN, f32::NAN);
    r.area = FieldArea::new(Rect::new(10.0, 6.0, 260.0, 32.0), nan_text);
    assert_eq!(r.press((12.0, 8.0)), FieldOutcome::Focus);
    assert_eq!(r.right((12.0, 8.0)), FieldOutcome::Focus);
    assert_eq!(r.release((12.0, 8.0)), FieldOutcome::Taken);
    r.input.cancel();
    assert_eq!(
        r.key(KeyName::F(10), mods(true), None),
        FieldOutcome::Redraw
    );
    r.input.cancel();
    assert_eq!(r.field.caret(), caret);
    // Neither rectangle usable: nothing is the field's.
    r.input.cancel();
    r.area = FieldArea::text_only(empty);
    assert_eq!(r.press((40.0, 10.0)), FieldOutcome::Unused);
    assert_eq!(r.tick(), FieldOutcome::Unused);
}

#[test]
fn outcomes_say_whether_they_were_taken_and_need_a_frame() {
    use FieldOutcome::*;
    let table = [
        (Unused, false, false),
        (Taken, true, false),
        (Redraw, true, true),
        (Focus, true, true),
        (Edited, true, true),
    ];
    for (outcome, taken, redraw) in table {
        assert_eq!(outcome.taken(), taken, "{outcome:?}");
        assert_eq!(outcome.redraw(), redraw, "{outcome:?}");
    }
}

#[test]
fn builders_and_defaults() {
    let input = FieldInput::default();
    assert_eq!(input.timer(), INPUT_TIMER);
    assert!(!input.menu_open() && !input.is_dragging());
    assert_eq!(input.deadline(), None);
    let named = FieldInput::new().timer_name("search").menu_look(Look::FLAT);
    assert_eq!(named.timer(), "search");
    let hit = FieldHit {
        area: Rect::new(1.0, 2.0, 3.0, 4.0),
        face: Face::Ui,
        size: 13.0,
        scale: 1.0,
    };
    let area = FieldArea::text_only(hit);
    assert_eq!(area.frame(), hit.area);
    assert_eq!(area.text().area, hit.area);
}

/// The window host's routing, without a window: `route` and the timer decision are what
/// `event` and `timer_fired` do around a `Cx`. Named `host` so CI's `host::` filter runs it.
#[cfg(feature = "host")]
mod host {
    use super::*;
    use crate::host::{Button, Event, KeyPress};
    use crate::textfield::input::hosted::Timer;

    fn route(r: &mut Rig, focused: bool, event: Event) -> (FieldOutcome, bool) {
        let now = r.clock.now();
        r.input.route(
            &mut r.field,
            &mut r.gfx,
            r.area,
            focused,
            Mods::default(),
            now,
            &event,
        )
    }

    fn press(x: f32, y: f32, button: Button) -> Event {
        Event::Press {
            x,
            y,
            button,
            clicks: 1,
        }
    }

    fn key(name: KeyName, text: Option<&str>) -> Event {
        Event::Key(KeyPress {
            name: Some(name),
            mods: Mods::default(),
            text: text.map(str::to_owned),
            repeat: false,
        })
    }

    #[test]
    fn host_events_reach_the_same_methods_and_keys_need_the_keyboard() {
        let Some(mut r) = Rig::new("alpha beta") else {
            return;
        };
        let (a1, a4) = (r.at(1), r.at(4));
        assert_eq!(
            route(&mut r, false, press(a1.0, a1.1, Button::Left)).0,
            FieldOutcome::Focus
        );
        let moved = Event::PointerMove { x: a4.0, y: a4.1 };
        assert_eq!(route(&mut r, false, moved).0, FieldOutcome::Redraw);
        let up = Event::Release {
            x: a4.0,
            y: a4.1,
            button: Button::Left,
        };
        assert_eq!(route(&mut r, false, up).0, FieldOutcome::Taken);
        assert_eq!(r.field.selected_text(), "lph");
        // Not focused: keys and text are the caller's.
        assert_eq!(
            route(&mut r, false, key(KeyName::Backspace, None)).0,
            FieldOutcome::Unused
        );
        assert_eq!(
            route(&mut r, false, Event::Text("z".into())).0,
            FieldOutcome::Unused
        );
        assert_eq!(
            route(&mut r, true, key(KeyName::Backspace, None)).0,
            FieldOutcome::Edited
        );
        assert_eq!(r.field.text(), "aa beta");
        assert_eq!(
            route(&mut r, true, Event::Text("z".into())).0,
            FieldOutcome::Edited
        );
        // A key with no name (a modifier) is unused.
        let bare = Event::Key(KeyPress {
            name: None,
            mods: Mods::default(),
            text: None,
            repeat: false,
        });
        assert_eq!(route(&mut r, true, bare).0, FieldOutcome::Unused);
        // The middle button is the caller's.
        assert_eq!(
            route(&mut r, false, press(a1.0, a1.1, Button::Middle)).0,
            FieldOutcome::Unused
        );
    }

    #[test]
    fn host_an_open_menu_gets_keys_without_the_keyboard_and_focus_loss_closes_it() {
        let Some(mut r) = Rig::new("alpha") else {
            return;
        };
        let a = r.at(2);
        assert_eq!(
            route(&mut r, false, press(a.0, a.1, Button::Right)).0,
            FieldOutcome::Focus
        );
        assert!(r.input.menu_open());
        assert_eq!(
            route(&mut r, false, key(KeyName::Char('q'), Some("q"))).0,
            FieldOutcome::Taken
        );
        assert_eq!(
            route(&mut r, false, Event::Text("q".into())).0,
            FieldOutcome::Taken
        );
        r.place_menu();
        let wheel = Event::Wheel {
            dx: 0.0,
            dy: 40.0,
            precise: false,
        };
        assert!(route(&mut r, false, wheel).0.taken());
        // Focus loss closes the menu, asks for a frame, and is still the application's.
        assert_eq!(
            route(&mut r, false, Event::Focus(false)),
            (FieldOutcome::Unused, true)
        );
        assert!(!r.input.menu_open());
        assert_eq!(
            route(&mut r, false, Event::Focus(false)),
            (FieldOutcome::Unused, false)
        );
        // A drag held across a focus loss is over when the focus comes back.
        r.clock.advance(Duration::from_secs(1));
        route(&mut r, true, press(a.0, a.1, Button::Left));
        assert!(r.input.is_dragging());
        route(&mut r, true, Event::Focus(false));
        assert!(!r.input.is_dragging());
        let back = r.at(4);
        assert_eq!(
            route(
                &mut r,
                true,
                Event::PointerMove {
                    x: back.0,
                    y: back.1
                }
            )
            .0,
            FieldOutcome::Unused
        );
        // So is one held across a change of scale.
        route(&mut r, true, press(a.0, a.1, Button::Left));
        route(&mut r, true, Event::Scale(1.5));
        assert!(!r.input.is_dragging());
        // With no move seen yet, the wheel has no place and is unused.
        let mut fresh = FieldInput::new();
        let w = Event::Wheel {
            dx: 0.0,
            dy: 1.0,
            precise: true,
        };
        let now = r.clock.now();
        let out = fresh.route(
            &mut r.field,
            &mut r.gfx,
            r.area,
            true,
            Mods::default(),
            now,
            &w,
        );
        assert_eq!(out, (FieldOutcome::Unused, false));
    }

    #[test]
    fn host_the_timer_is_set_while_due_and_cancelled_only_by_the_input_that_set_it() {
        let text = "a long line that is much wider than the two hundred pixels of the field";
        let Some(mut r) = Rig::new(text) else {
            return;
        };
        assert_eq!(
            r.input.next_timer(),
            Timer::Leave,
            "idle: nothing set, nothing cancelled"
        );
        r.press(r.at(2));
        let beyond = (r.area.text().area.right() + 30.0, r.area.text().area.cy());
        r.move_to(beyond);
        let due = r.input.deadline().unwrap();
        assert_eq!(r.input.next_timer(), Timer::Set(due));
        r.clock.advance(due - r.clock.now());
        let now = r.clock.now();
        assert_eq!(
            r.input.fired(&mut r.field, &mut r.gfx, r.area, now),
            FieldOutcome::Redraw
        );
        // Fired, so no longer set: the next decision sets it again for the next step.
        assert!(matches!(r.input.next_timer(), Timer::Set(_)));
        r.release(beyond);
        assert_eq!(r.input.next_timer(), Timer::Cancel);
        assert_eq!(r.input.next_timer(), Timer::Leave);
        // A timer that fires with nothing due is still this input's: taken, nothing moved.
        let now = r.clock.now();
        assert_eq!(
            r.input.fired(&mut r.field, &mut r.gfx, r.area, now),
            FieldOutcome::Taken
        );
        // Another input under the same name never cancels a timer it did not set.
        let mut other = FieldInput::new();
        assert_eq!(other.next_timer(), Timer::Leave);
    }
}
