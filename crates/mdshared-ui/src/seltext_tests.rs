//! Tests for [`seltext`](super): hit testing, the clicks, the keys, the actions and the pixels.
//!
//! The sheet a person looks at is `seltext_sheet`:
//! `cargo test -p mdshared-ui seltext_sheet -- --ignored`, then open
//! `target/specimen/seltext.png` and `seltext-150.png`.

use super::*;
use crate::gfx::{Gfx, Painter, Rect, Rgb, Wrap};
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

fn layout(face: Face, size: f32, width: f32, text: &str, scale: f32) -> Vec<Line> {
    let mut bmp = Bitmap::new(16, 16);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, scale);
    p.paragraph_lines(face, size, width, text, [0, 0, 0])
}

/// The lines of `text` in the interface face, wide enough for one line unless it breaks.
fn lines_of(text: &str) -> Vec<Line> {
    layout(Face::Ui, 13.0, 300.0, text, 1.0)
}

fn model(text: &str) -> SelectableText {
    let mut m = SelectableText::new();
    m.set_text(text);
    m
}

fn ctrl() -> Mods {
    Mods {
        ctrl: true,
        ..Mods::default()
    }
}

fn shift() -> Mods {
    Mods {
        shift: true,
        ..Mods::default()
    }
}

/// The line that holds byte `at`.
fn line_at(lines: &[Line], at: usize) -> &Line {
    lines
        .iter()
        .find(|l| at >= l.range.start && at < l.range.end)
        .unwrap_or_else(|| lines.last().unwrap())
}

/// The index of byte `at` among `line`'s stops.
fn stop_index(line: &Line, text: &str, at: usize) -> usize {
    text[line.range.start..at].chars().count()
}

/// A point inside the character that starts at byte `at`.
fn x_in_char(line: &Line, text: &str, at: usize) -> f32 {
    let i = stop_index(line, text, at);
    (line.stops[i] + line.stops[i + 1]) / 2.0
}

fn middle(line: &Line) -> f32 {
    line.y + line.height / 2.0
}

#[test]
fn a_press_lands_on_the_nearest_boundary_at_line_ends_and_inside_a_word() {
    let text = "alpha beta gamma delta epsilon";
    for scale in [1.0, 1.5] {
        let lines = layout(Face::Ui, 13.0, 70.0, text, scale);
        assert!(lines.len() >= 3, "scale {scale}: {} lines", lines.len());
        let mut m = model(text);
        let none = Mods::default();

        // Far right of the first wrapped line is its end, far left of the last is its start.
        let first = &lines[0];
        m.press(&lines, 1000.0, middle(first), none, 1);
        assert_eq!(m.caret().0, first.range.end, "scale {scale}");
        assert_eq!(
            m.caret_line(),
            Some(0),
            "the caret stays on the line clicked"
        );
        assert!(!m.has_selection(), "a single click selects nothing");
        let last = lines.last().unwrap();
        m.press(&lines, -1000.0, middle(last), none, 1);
        assert_eq!(m.caret().0, last.range.start, "scale {scale}");

        // In the middle of "gamma" the caret is on one of that word's boundaries.
        let at = text.find("gamma").unwrap();
        let line = line_at(&lines, at);
        let i = stop_index(line, text, at) + 2;
        let x = (line.stops[i] + line.stops[i + 1]) / 2.0;
        m.press(&lines, x, middle(line), none, 1);
        assert!(
            m.caret().0 == at + 2 || m.caret().0 == at + 3,
            "scale {scale}: caret {} for gamma at {at}",
            m.caret().0
        );
        assert!(text.is_char_boundary(m.caret().0));

        // A point below the paragraph lands on its last line.
        m.press(&lines, x, 10_000.0, none, 1);
        assert_eq!(m.caret().0, offset_in_line(last, text, x), "scale {scale}");
    }
}

#[test]
fn a_shift_click_extends_the_selection_from_the_first_press() {
    let text = "alpha beta gamma delta epsilon";
    let lines = layout(Face::Ui, 13.0, 70.0, text, 1.0);
    let mut m = model(text);
    m.press(&lines, 0.0, middle(&lines[0]), Mods::default(), 1);
    let at = text.find("beta").unwrap();
    let line = line_at(&lines, at);
    let i = stop_index(line, text, at);
    let x = line.stops[i] + 0.75 * (line.stops[i + 1] - line.stops[i]);
    m.press(&lines, x, middle(line), shift(), 1);
    assert_eq!(m.selection(), 0..at + 1);
    assert_eq!(m.selected_text(), &text[..at + 1]);
}

#[test]
fn a_double_click_selects_a_word_and_a_triple_click_its_line() {
    let text = "one two three\nfour five";
    let lines = layout(Face::Ui, 13.0, 300.0, text, 1.0);
    assert_eq!(lines.len(), 2);
    let at = text.find("two").unwrap();
    let line = line_at(&lines, at);
    let x = x_in_char(line, text, at);
    let mut m = model(text);
    m.press(&lines, x, middle(line), Mods::default(), 2);
    assert_eq!(m.selected_text(), "two");
    m.press(&lines, x, middle(line), Mods::default(), 3);
    assert_eq!(m.selected_text(), "one two three");
    assert!(!m.selected_text().ends_with('\n'), "the newline stays out");
    // Three clicks on the second line, and on a space inside a line, select that line too.
    let four = text.find("four").unwrap();
    m.press(
        &lines,
        x_in_char(line_at(&lines, four), text, four),
        middle(line_at(&lines, four)),
        Mods::default(),
        3,
    );
    assert_eq!(m.selected_text(), "four five");
    // End stops before the newline, so Shift+End does not take the next line with it.
    m.press(&lines, 0.0, middle(&lines[0]), Mods::default(), 1);
    assert_eq!(m.key(&KeyName::End, shift()), None);
    assert_eq!(m.selected_text(), "one two three");
    assert_eq!(m.key(&KeyName::Home, shift()), None);
    assert!(!m.has_selection());
}

#[test]
fn a_triple_click_selects_the_wrapped_line_as_one_log_line() {
    let text = "one two three\nfour";
    let lines = layout(Face::Ui, 13.0, 55.0, text, 1.0);
    assert!(lines.len() >= 3, "the first log line wraps: {lines:?}");
    let second = &lines[1];
    let mut m = model(text);
    m.press(&lines, 1.0, middle(second), Mods::default(), 3);
    assert_eq!(m.selected_text(), "one two three");
}

#[test]
fn shift_down_extends_to_the_next_wrapped_line_at_the_same_x() {
    let text = "alpha beta gamma delta epsilon";
    let lines = layout(Face::Ui, 13.0, 70.0, text, 1.0);
    assert!(lines.len() >= 2);
    let mut m = model(text);
    m.press(&lines, 0.0, middle(&lines[0]), Mods::default(), 1);
    assert_eq!(m.caret().0, 0);
    assert_eq!(m.key(&KeyName::Down, shift()), None);
    assert_eq!(m.caret().0, lines[1].range.start);
    assert_eq!(m.selection(), 0..lines[1].range.start);
    assert_eq!(m.selected_text(), &text[..lines[1].range.start]);
    // Down again, now without Shift, moves and drops the selection.
    assert_eq!(m.key(&KeyName::Down, Mods::default()), None);
    assert_eq!(m.caret().0, lines[2].range.start);
    assert!(!m.has_selection());
    // Up keeps the x as well; at the first line it stays put.
    assert_eq!(m.key(&KeyName::Up, Mods::default()), None);
    assert_eq!(m.caret().0, lines[1].range.start);
    assert_eq!(m.key(&KeyName::Up, Mods::default()), None);
    assert_eq!(m.caret().0, 0);
    assert_eq!(m.key(&KeyName::Up, shift()), None);
    assert_eq!(m.caret().0, 0);
    assert!(!m.has_selection());
}

#[test]
fn ctrl_c_copies_the_selection_and_ctrl_a_selects_everything() {
    let text = "hello world";
    let lines = layout(Face::Ui, 13.0, 300.0, text, 1.0);
    let hello = text.find("hello").unwrap();
    let line = &lines[0];
    let mut m = model(text);
    assert_eq!(m.key(&KeyName::Char('c'), ctrl()), None, "nothing selected");
    m.press(
        &lines,
        x_in_char(line, text, hello),
        middle(line),
        Mods::default(),
        2,
    );
    assert_eq!(m.selected_text(), "hello");
    assert_eq!(
        m.key(&KeyName::Char('c'), ctrl()),
        Some(Action::Copy("hello".into()))
    );
    assert_eq!(m.key(&KeyName::Char('a'), ctrl()), None);
    assert_eq!(m.selection(), 0..text.len());
    assert_eq!(
        m.key(&KeyName::Char('c'), ctrl()),
        Some(Action::Copy(text.into()))
    );
    m.clear_selection();
    assert!(!m.has_selection());
    assert_eq!(m.selected_text(), "");
}

#[test]
fn alt_c_is_refused_and_so_are_alt_arrows() {
    let text = "hello world";
    let lines = layout(Face::Ui, 13.0, 300.0, text, 1.0);
    let mut m = model(text);
    m.select_all();
    let before = m.caret();
    let alt = Mods {
        alt: true,
        ..Mods::default()
    };
    let alt_ctrl = Mods {
        alt: true,
        ctrl: true,
        ..Mods::default()
    };
    assert_eq!(m.key(&KeyName::Char('c'), alt), None);
    assert_eq!(m.key(&KeyName::Char('c'), alt_ctrl), None);
    assert_eq!(m.caret(), before);
    assert_eq!(m.selected_text(), text);
    assert_eq!(m.key(&KeyName::Down, alt), None);
    assert_eq!(m.caret(), before);
    // Other keys are the caller's and leave the text alone.
    for key in [
        KeyName::Escape,
        KeyName::Enter,
        KeyName::Tab,
        KeyName::Space,
        KeyName::PageDown,
        KeyName::F(5),
        KeyName::Char('z'),
    ] {
        assert_eq!(m.key(&key, Mods::default()), None);
        assert_eq!(m.caret(), before);
    }
    // Home and End do move, and this paragraph has its lines.
    m.set_lines(&lines);
    assert_eq!(m.key(&KeyName::Home, Mods::default()), None);
    assert_eq!(m.caret().0, 0);
    assert_eq!(m.key(&KeyName::End, Mods::default()), None);
    assert_eq!(m.caret().0, text.len());
}

#[test]
fn the_context_menu_offers_copy_only_with_a_selection_and_select_all() {
    let mut m = model("hi");
    let items = m.context_menu_items();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].label, "Copy");
    assert!(!items[0].enabled, "nothing to copy yet");
    assert_eq!(items[1].label, "Select all");
    assert_eq!(items[1].id, Action::SelectAll);
    m.select_all();
    let items = m.context_menu_items();
    assert!(items[0].enabled);
    assert_eq!(items[0].id, Action::Copy("hi".into()));
}

#[test]
fn paragraph_lines_joins_back_into_the_text_and_matches_the_height_paragraph_draws() {
    let text = "café 日本語 😀\nsecond line that wraps once or twice";
    for (scale, width) in [(1.0, 90.0), (1.5, 90.0)] {
        let mut bmp = Bitmap::new(8, 8);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        let lines = p.paragraph_lines(Face::Ui, 13.0, width, text, [0, 0, 0]);
        assert!(lines.len() > 2);
        let mut at = 0;
        for l in &lines {
            assert_eq!(l.range.start, at);
            assert_eq!(l.stops.len(), text[l.range.clone()].chars().count() + 1);
            assert_eq!(l.edges.len(), l.stops.len());
            at = l.range.end;
        }
        assert_eq!(at, text.len());
        let height = p.paragraph(Face::Ui, 13.0, 0.0, 0.0, width, text, [0, 0, 0]);
        let count = (height / lines[0].height).round() as usize;
        assert_eq!(count, lines.len(), "scale {scale}: {height} px");
    }
}

#[test]
fn a_press_drag_and_the_keys_stay_on_char_boundaries_of_non_ascii_text() {
    let text = "café 日本語 😀 fin";
    for scale in [1.0, 1.5] {
        let lines = layout(Face::Ui, 13.0, 70.0, text, scale);
        let mut m = model(text);
        let mut x = -30.0;
        while x < 140.0 {
            for line in &lines {
                m.press(&lines, x, middle(line), Mods::default(), 1);
                assert!(text.is_char_boundary(m.caret().0), "press at {x}");
                m.drag(&lines, x + 3.0, middle(line));
                assert!(text.is_char_boundary(m.caret().0), "drag at {x}");
                let _ = m.selected_text();
                m.press(&lines, x, middle(line), Mods::default(), 2);
                assert!(text.is_char_boundary(m.caret().0), "double at {x}");
                assert!(text.is_char_boundary(m.anchor), "double at {x}");
                let _ = m.selected_text();
                m.press(&lines, x, middle(line), Mods::default(), 3);
                assert!(text.is_char_boundary(m.caret().0), "triple at {x}");
                assert!(text.is_char_boundary(m.anchor), "triple at {x}");
            }
            x += 2.7;
        }
        m.select_all();
        for _ in 0..60 {
            m.key(&KeyName::Left, shift());
            assert!(text.is_char_boundary(m.caret().0));
            assert!(text.is_char_boundary(m.anchor));
            m.key(&KeyName::Right, shift());
            assert!(text.is_char_boundary(m.caret().0));
        }
    }
}

#[test]
fn nothing_goes_wrong_with_empty_text_or_nonsense_coordinates() {
    let mut m = SelectableText::new();
    m.press(&[], f32::NAN, f32::NAN, Mods::default(), 1);
    m.press(&[], f32::INFINITY, f32::NEG_INFINITY, Mods::default(), 2);
    m.drag(&[], 4.0, 4.0);
    m.key(&KeyName::Down, shift());
    assert_eq!(m.caret(), (0, 0));
    assert!(!m.has_selection());
    assert_eq!(m.selected_text(), "");
    // Mismatched lines do not panic either.
    let lines = layout(Face::Ui, 13.0, 40.0, "some words here", 1.0);
    let mut m = model("some words here");
    m.press(&lines[1..], 10.0, 100.0, Mods::default(), 1);
    m.drag(&[], 10.0, 10.0);
    assert!(m.text().is_char_boundary(m.caret().0));
    // Setting the text drops the selection and the old lines.
    m.select_all();
    m.set_text("new");
    assert_eq!(m.selection(), 0..0);
    assert_eq!(m.text(), "new");
    // A drag without a press does nothing.
    m.press(&[], 0.0, 0.0, Mods::default(), 1);
    m.release();
    m.drag(&[], 100.0, 0.0);
    assert_eq!(m.caret().0, 0);
}

#[test]
fn monospace_text_wraps_and_selects_the_same_way() {
    let text = "/home/me/Pictures/a path with spaces in it/file.png done";
    let lines = layout(Face::Mono, 12.0, 120.0, text, 1.0);
    assert!(lines.len() >= 3, "mono wraps: {}", lines.len());
    let mut m = model(text);
    let first = &lines[0];
    m.press(&lines, 1000.0, middle(first), Mods::default(), 1);
    assert_eq!(m.caret().0, first.range.end);
    let at = text.find("spaces").unwrap();
    let line = line_at(&lines, at);
    m.press(
        &lines,
        x_in_char(line, text, at),
        middle(line),
        Mods::default(),
        2,
    );
    assert_eq!(m.selected_text(), "spaces");
}

/// The pixel the blend of `fg` at 0.35 over `bg` gives, as `Painter::fill_a` computes it.
fn blended(bg: Rgb, fg: Rgb) -> Rgb {
    let a = (0.35 * 256.0) as u32;
    let ia = 256 - a;
    [
        ((fg[0] as u32 * a + bg[0] as u32 * ia) >> 8) as u8,
        ((fg[1] as u32 * a + bg[1] as u32 * ia) >> 8) as u8,
        ((fg[2] as u32 * a + bg[2] as u32 * ia) >> 8) as u8,
    ]
}

fn pixel(bmp: &Bitmap, x: f32, y: f32, scale: f32) -> Rgb {
    let (x, y) = ((x * scale).round() as usize, (y * scale).round() as usize);
    let at = (y * bmp.width as usize + x) * 4;
    [bmp.pixels[at + 2], bmp.pixels[at + 1], bmp.pixels[at]]
}

fn close(a: Rgb, b: Rgb) -> bool {
    (0..3).all(|i| a[i].abs_diff(b[i]) <= 1)
}

#[test]
fn the_selected_chars_have_the_highlight_behind_them_and_the_others_do_not() {
    let text = "a b";
    for scale in [1.0, 1.5] {
        for pal in [&LIGHT, &DARK] {
            let (w, h) = (240.0, 60.0);
            let area = Rect::new(10.0, 10.0, w, h);
            let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            p.fill(Rect::new(0.0, 0.0, w, h), pal.page);
            let lines = p.paragraph_lines(Face::Ui, 13.0, w, text, pal.text);
            assert_eq!(lines.len(), 1);
            let line = &lines[0];
            // Select "a " by a press at its start and a drag past the space.
            let mut m = model(text);
            m.press(&lines, 0.0, middle(line), Mods::default(), 1);
            m.drag(&lines, x_in_char(line, text, 1) + 1.0, middle(line));
            assert_eq!(m.selection(), 0..2);
            draw(&mut p, pal, area, Face::Ui, 13.0, &m, true);

            let expected = blended(pal.page, pal.selection);
            let space_x = area.x + (line.stops[1] + line.stops[2]) / 2.0;
            let y = area.y + line.y + line.height / 2.0;
            assert!(
                close(pixel(&bmp, space_x, y, scale), expected),
                "scale {scale}, dark {}: the selected space is not highlighted",
                pal.dark
            );
            // The whole line box of the space is covered, above the glyphs and below them.
            for f in [0.1, 0.9] {
                let y = area.y + line.y + line.height * f;
                assert!(
                    close(pixel(&bmp, space_x, y, scale), expected),
                    "scale {scale}: the highlight is not {f} of the way down"
                );
            }
            // "b" is not selected: above its glyph the page shows through.
            let b_x = area.x + x_in_char(line, text, 2);
            let top = area.y + line.y + 1.0 / scale;
            assert!(
                close(pixel(&bmp, b_x, top, scale), pal.page),
                "scale {scale}: an unselected char has a highlight"
            );

            // With no focus the selection is not drawn at all.
            let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            p.fill(Rect::new(0.0, 0.0, w, h), pal.page);
            draw(&mut p, pal, area, Face::Ui, 13.0, &m, false);
            assert!(
                close(pixel(&bmp, space_x, y, scale), pal.page),
                "scale {scale}: an unfocused pane drew a selection"
            );
        }
    }
}

#[test]
fn the_end_of_a_wrapped_line_stays_on_that_line_for_end_home_and_down() {
    let text = "alpha beta gamma delta epsilon";
    let none = Mods::default();
    for scale in [1.0, 1.5] {
        let lines = layout(Face::Ui, 13.0, 70.0, text, scale);
        assert!(lines.len() >= 3, "scale {scale}: {} lines", lines.len());
        let mut m = model(text);
        m.press(&lines, 0.0, middle(&lines[0]), none, 1);
        m.key(&KeyName::End, none);
        assert_eq!(m.caret().0, lines[0].range.end, "scale {scale}");
        assert_eq!(
            m.caret_line(),
            Some(0),
            "scale {scale}: End stays on line 0"
        );
        m.key(&KeyName::End, none);
        assert_eq!(m.caret().0, lines[0].range.end, "scale {scale}: End again");
        assert_eq!(m.caret_line(), Some(0), "scale {scale}: End again");
        m.key(&KeyName::Home, none);
        assert_eq!(m.caret().0, 0, "scale {scale}: Home after End");

        // A click past the end of line 0, then Down, is on line 1, at its end.
        m.press(&lines, 1000.0, middle(&lines[0]), none, 1);
        assert_eq!(m.caret_line(), Some(0), "scale {scale}");
        m.key(&KeyName::Down, none);
        assert_eq!(m.caret_line(), Some(1), "scale {scale}: Down from line 0");
        assert_eq!(m.caret().0, lines[1].range.end, "scale {scale}");
        m.key(&KeyName::Home, none);
        assert_eq!(m.caret().0, lines[1].range.start, "scale {scale}");
        m.key(&KeyName::Up, none);
        assert_eq!(m.caret_line(), Some(0), "scale {scale}: Up from line 1");

        // Shift+End from the start selects line 0 and Shift+Down keeps the line end's x.
        m.press(&lines, 0.0, middle(&lines[0]), none, 1);
        m.key(&KeyName::End, shift());
        assert_eq!(m.selection(), 0..lines[0].range.end, "scale {scale}");
        m.key(&KeyName::Down, shift());
        assert_eq!(m.caret_line(), Some(1), "scale {scale}");

        // Right puts the caret at the start of the next line, as Windows does.
        let end0 = lines[0].range.end;
        let x = x_in_char(&lines[0], text, end0 - 1);
        m.press(&lines, x, middle(&lines[0]), none, 1);
        while m.caret().0 < end0 {
            m.key(&KeyName::Right, none);
        }
        assert_eq!(m.caret().0, end0);
        assert_eq!(m.caret_line(), Some(1), "scale {scale}: Right goes down");
    }
}

#[test]
fn a_double_click_selects_the_word_of_the_char_under_the_pointer_anywhere_in_it() {
    let text = "foo bar";
    let none = Mods::default();
    for scale in [1.0, 1.5] {
        let lines = layout(Face::Ui, 13.0, 300.0, text, scale);
        let line = &lines[0];
        for (i, c) in text.char_indices() {
            let want = match (c, i) {
                (' ', _) => " ",
                (_, 0..3) => "foo",
                _ => "bar",
            };
            for f in [0.02, 0.3, 0.5, 0.7, 0.98] {
                let x = line.stops[i] + f * (line.stops[i + 1] - line.stops[i]);
                let mut m = model(text);
                m.press(&lines, x, middle(line), none, 2);
                assert_eq!(m.selected_text(), want, "scale {scale}: char {i} at {f}");
            }
        }
        let mut m = model(text);
        m.press(&lines, 1000.0, middle(line), none, 2);
        assert_eq!(m.selected_text(), "bar", "past the end, the last char");
        m.press(&lines, -1000.0, middle(line), none, 2);
        assert_eq!(m.selected_text(), "foo", "before the start, the first char");
        m.press(&lines, f32::NAN, f32::NAN, none, 2);
        assert!(text.is_char_boundary(m.caret().0));
    }
}

/// The clusters of the grapheme test, and the text they make.
const GRAPHEMES: [&str; 9] = [
    "x",
    "e\u{301}",
    "y",
    " ",
    "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
    " ",
    "\u{1F1F9}\u{1F1F7}",
    " ",
    "z",
];

fn grapheme_edges() -> Vec<usize> {
    let mut at = 0;
    let mut edges = vec![0];
    for g in GRAPHEMES {
        at += g.len();
        edges.push(at);
    }
    edges
}

#[test]
fn a_press_and_the_arrows_land_only_on_the_edges_of_clusters() {
    let text: String = GRAPHEMES.concat();
    let text = text.as_str();
    let edges = grapheme_edges();
    let none = Mods::default();
    for scale in [1.0, 1.5] {
        let lines = layout(Face::Ui, 13.0, 400.0, text, scale);
        assert_eq!(lines.len(), 1, "scale {scale}");
        let line = &lines[0];
        // The chars inside a cluster have its start's x and are no edge.
        let mut start_x = 0.0;
        for (i, (b, _)) in text.char_indices().enumerate() {
            if edges.contains(&b) {
                assert!(line.edges[i], "scale {scale}: byte {b} is an edge");
                start_x = line.stops[i];
            } else {
                assert!(
                    !line.edges[i],
                    "scale {scale}: byte {b} is inside a cluster"
                );
                assert_eq!(line.stops[i], start_x, "scale {scale}: byte {b}");
            }
        }
        assert!(*line.edges.last().unwrap());

        let mut m = model(text);
        let right = line.stops.last().unwrap() + 10.0;
        let mut x = -10.0;
        while x < right {
            m.press(&lines, x, middle(line), none, 1);
            assert!(edges.contains(&m.caret().0), "scale {scale}: press at {x}");
            m.drag(&lines, x + 1.3, middle(line));
            assert!(edges.contains(&m.caret().0), "scale {scale}: drag at {x}");
            m.press(&lines, x, middle(line), none, 2);
            let r = m.selection();
            assert!(
                edges.contains(&r.start) && edges.contains(&r.end),
                "{x}: {r:?}"
            );
            x += 0.37;
        }

        // Right walks the edges one by one, Shift+Left walks them back.
        m.press(&lines, -10.0, middle(line), none, 1);
        let mut seen = vec![m.caret().0];
        for _ in 0..edges.len() {
            m.key(&KeyName::Right, none);
            seen.push(m.caret().0);
        }
        seen.dedup();
        assert_eq!(seen, edges, "scale {scale}: Right");
        let mut back = vec![m.caret().0];
        for _ in 0..edges.len() {
            m.key(&KeyName::Left, shift());
            back.push(m.caret().0);
        }
        back.dedup();
        back.reverse();
        assert_eq!(back, edges, "scale {scale}: Shift+Left");
        assert_eq!(m.selection(), 0..text.len());
    }
}

#[test]
fn a_double_click_on_an_accented_letter_takes_the_whole_word() {
    let text = "cafe\u{301} noir";
    let lines = lines_of(text);
    let line = &lines[0];
    let mut m = model(text);
    let x = x_in_char(line, text, 1);
    m.press(&lines, x, middle(line), Mods::default(), 2);
    assert_eq!(m.selected_text(), "cafe\u{301}");
}

#[test]
fn lines_of_another_text_are_ignored_and_nothing_panics() {
    let none = Mods::default();
    let other = lines_of("hello world");
    let mut m = model("hi");
    m.press(&other, 1000.0, 5.0, none, 1);
    assert_eq!(m.caret(), (0, 0));
    assert_eq!(m.caret_line(), None, "the lines were dropped");
    for clicks in [1, 2, 3] {
        m.press(&other, 1000.0, 5.0, shift(), clicks);
        m.drag(&other, 3.0, 3.0);
        m.release();
    }
    m.set_lines(&other);
    for key in [
        KeyName::Left,
        KeyName::Right,
        KeyName::Home,
        KeyName::End,
        KeyName::Up,
        KeyName::Down,
    ] {
        m.key(&key.clone(), none);
        m.key(&key, shift());
        assert!(m.caret().0 <= 2);
    }
    let _ = m.selected_text();

    // A range off a char boundary, and stops or edges of the wrong count.
    let text = "\u{e9}a";
    let mut off = lines_of(text);
    off[0].range = 1..3;
    let mut short = lines_of(text);
    short[0].stops.pop();
    let mut edges = lines_of(text);
    edges[0].edges.push(true);
    for bad in [off, short, edges] {
        let mut m = model(text);
        m.press(&bad, 1000.0, 5.0, none, 1);
        m.press(&bad, 1000.0, 5.0, none, 2);
        m.key(&KeyName::End, none);
        assert_eq!(m.caret_line(), None);
        assert!(text.is_char_boundary(m.caret().0));
    }
    // The good lines of a mixed set are kept.
    let lines = layout(Face::Ui, 13.0, 40.0, "some words here", 1.0);
    assert!(lines.len() >= 2);
    let mut mixed = lines.clone();
    mixed.push(other[0].clone());
    let mut m = model("some words here");
    m.set_lines(&mixed);
    m.key(&KeyName::End, none);
    assert_eq!(m.caret().0, lines[0].range.end);
}

/// The least of five timings of `f`, which is steadier than one.
fn least_of_five(mut f: impl FnMut()) -> std::time::Duration {
    (0..5)
        .map(|_| {
            let started = std::time::Instant::now();
            f();
            started.elapsed()
        })
        .min()
        .unwrap()
}

#[test]
fn paragraph_lines_is_linear_on_a_long_line_of_one_token() {
    // 20,000 chars is the size asked for. Five times as many chars take about five times as
    // long when the work is linear and about 25 times when it is quadratic (a quadratic pass took
    // 300 ms at 100,000 chars in release, a linear one 2 ms), so the ratio of the two is the
    // check, which a slow machine does not change. The absolute bound is only for a hang.
    let mut lines_took = Vec::new();
    let mut model_took = Vec::new();
    for len in [20_000, 100_000] {
        let text = "a".repeat(len);
        let mut bmp = Bitmap::new(8, 8);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        // The first call shapes the text; the timed ones only read the cached layout.
        let lines = p.paragraph_lines(Face::Ui, 13.0, 300.0, &text, [0, 0, 0]);
        let took = least_of_five(|| {
            let again = p.paragraph_lines(Face::Ui, 13.0, 300.0, &text, [0, 0, 0]);
            assert_eq!(lines, again);
        });
        let stops: usize = lines.iter().map(|l| l.stops.len()).sum();
        assert_eq!(stops, text.len() + lines.len());
        // The model's double click and keys on it are linear too.
        let took_model = least_of_five(|| {
            let mut m = model(&text);
            m.press(&lines, 5000.0, middle(&lines[0]), Mods::default(), 2);
            m.key(&KeyName::End, shift());
            m.key(&KeyName::Left, shift());
            assert_eq!(m.selection().start, 0);
        });
        println!("{len} chars: paragraph_lines {took:?}, model {took_model:?}");
        lines_took.push(took);
        model_took.push(took_model);
    }
    if !cfg!(debug_assertions) {
        let slack = std::time::Duration::from_millis(5);
        for (what, took) in [("paragraph_lines", &lines_took), ("the model", &model_took)] {
            assert!(
                took[1] <= took[0] * 12 + slack,
                "{what} is not linear: {:?} for 20,000 chars, {:?} for 100,000",
                took[0],
                took[1]
            );
            assert!(
                took[1].as_secs() < 2,
                "{what} took {:?} for 100,000 chars",
                took[1]
            );
        }
    }
}

#[test]
fn draw_reads_the_layout_paragraph_draws_so_the_text_is_shaped_once() {
    let text = "hello world";
    let mut bmp = Bitmap::new(200, 40);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let mut m = model(text);
        m.select_all();
        let area = Rect::new(0.0, 0.0, 200.0, 40.0);
        draw(&mut p, &LIGHT, area, Face::Ui, 13.0, &m, true);
    }
    assert_eq!(gfx.cached_layouts(), 1);
}

#[test]
fn a_double_click_on_spaces_stops_at_a_newline() {
    let text = "ab  \n  cd\n\nef";
    let lines = lines_of(text);
    assert_eq!(lines.len(), 4);
    let none = Mods::default();
    let mut m = model(text);
    m.press(
        &lines,
        x_in_char(&lines[0], text, 3),
        middle(&lines[0]),
        none,
        2,
    );
    assert_eq!(m.selection(), 2..4, "the spaces before the newline");
    m.press(&lines, 1000.0, middle(&lines[0]), none, 2);
    assert_eq!(m.selection(), 2..4, "past the end, still not the newline");
    m.press(
        &lines,
        x_in_char(&lines[1], text, 5),
        middle(&lines[1]),
        none,
        2,
    );
    assert_eq!(m.selection(), 5..7, "the spaces after it");
    // An empty line selects nothing, and leaves the caret on it.
    m.press(&lines, 3.0, middle(&lines[2]), none, 2);
    assert!(!m.has_selection());
    assert_eq!(m.caret().0, 10);
}

#[test]
fn a_lone_carriage_return_and_the_unicode_separators_end_a_line() {
    let text = "one\rtwo\u{2028}three\u{2029}four";
    let none = Mods::default();
    let lines = lines_of(text);
    assert_eq!(lines.len(), 4, "parley breaks at each: {lines:?}");
    let mut m = model(text);
    for (i, want) in ["one", "two", "three", "four"].into_iter().enumerate() {
        let line = &lines[i];
        m.press(&lines, 1.0, middle(line), none, 3);
        assert_eq!(m.selected_text(), want, "triple click on line {i}");
        m.press(&lines, 0.0, middle(line), none, 1);
        m.key(&KeyName::End, shift());
        assert_eq!(m.selected_text(), want, "Shift+End on line {i}");
        m.press(&lines, 1000.0, middle(line), none, 1);
        let end = line.range.start + want.len();
        assert_eq!(m.caret().0, end, "click past line {i}");
    }
    // Without lines, End and Home use the same line breaks.
    let mut m = model(text);
    m.key(&KeyName::End, none);
    assert_eq!(m.caret().0, 3);
    for _ in 0..2 {
        m.key(&KeyName::Right, none);
    }
    m.key(&KeyName::End, shift());
    assert_eq!(m.caret().0, text.find('\u{2028}').unwrap());
    m.key(&KeyName::Home, none);
    assert_eq!(m.caret().0, 4);
}

#[test]
fn right_end_and_a_triple_click_treat_a_crlf_as_one_line_break() {
    let text = "one\r\ntwo\r\n\r\nthree";
    let none = Mods::default();
    let lines = lines_of(text);
    assert_eq!(lines.len(), 4, "parley breaks at each CRLF: {lines:?}");
    // The caret never stands between the \r and the \n, with lines and without.
    for with_lines in [true, false] {
        let mut m = model(text);
        if with_lines {
            m.set_lines(&lines);
        }
        let mut stood = vec![0];
        for _ in 0..text.chars().count() {
            m.key(&KeyName::Right, none);
            stood.push(m.caret().0);
        }
        assert!(
            !stood.contains(&4) && !stood.contains(&9) && !stood.contains(&11),
            "with lines {with_lines}: Right stood inside a CRLF: {stood:?}"
        );
        assert_eq!(*stood.last().unwrap(), text.len(), "Right reaches the end");
        // Left comes back the same way.
        let mut back = vec![m.caret().0];
        for _ in 0..text.chars().count() {
            m.key(&KeyName::Left, none);
            back.push(m.caret().0);
        }
        assert!(
            !back.contains(&4) && !back.contains(&9) && !back.contains(&11),
            "with lines {with_lines}: Left stood inside a CRLF: {back:?}"
        );
        assert_eq!(*back.last().unwrap(), 0);
        // Shift+Right at the end of "one" selects the whole line break.
        m.key(&KeyName::End, none);
        assert_eq!(
            m.caret().0,
            3,
            "with lines {with_lines}: End stops before the CRLF"
        );
        m.key(&KeyName::Right, shift());
        assert_eq!(m.selected_text(), "\r\n", "with lines {with_lines}");
        m.key(&KeyName::Right, none);
        m.key(&KeyName::End, shift());
        assert_eq!(
            m.selected_text(),
            "two",
            "with lines {with_lines}: End on the second line"
        );
        // The empty line between two CRLFs: End stays at its start.
        m.key(&KeyName::Right, none);
        m.key(&KeyName::Right, none);
        assert_eq!(
            m.caret().0,
            10,
            "with lines {with_lines}: on the empty line"
        );
        m.key(&KeyName::End, none);
        assert_eq!(
            m.caret().0,
            10,
            "with lines {with_lines}: End on the empty line"
        );
    }
    let mut m = model(text);
    for (i, want) in ["one", "two", "", "three"].into_iter().enumerate() {
        let line = &lines[i];
        m.press(&lines, 1.0, middle(line), none, 3);
        assert_eq!(m.selected_text(), want, "triple click on line {i}");
        m.press(&lines, 1000.0, middle(line), none, 1);
        assert_eq!(
            m.caret().0,
            line.range.start + want.len(),
            "a click past line {i} lands before its CRLF"
        );
    }
}

/// A registry key with a stretch of 90 chars and no space: wider than the area in any face.
const KEY: &str = "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\\
                   {ec8030f7-c20a-464f-9b0e-13a3a9e97384}";

/// Whether the device px column `x` of `bmp` holds anything but `page` in rows `y0..y1`.
fn inked(bmp: &Bitmap, x: usize, y0: usize, y1: usize, page: Rgb) -> bool {
    (y0..y1.min(bmp.height as usize)).any(|y| {
        let at = (y * bmp.width as usize + x) * 4;
        [bmp.pixels[at + 2], bmp.pixels[at + 1], bmp.pixels[at]] != page
    })
}

#[test]
fn a_long_path_wraps_anywhere_inside_a_narrow_area_and_copies_as_it_was() {
    for scale in [1.0, 1.5] {
        for pal in [&LIGHT, &DARK] {
            let (w, h) = (320.0, 260.0);
            let area = Rect::new(10.0, 10.0, 130.0, 240.0);
            let fresh = || {
                let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
                let mut gfx = Gfx::new();
                Painter::new(&mut bmp, &mut gfx, scale).fill(Rect::new(0.0, 0.0, w, h), pal.page);
                (bmp, gfx)
            };
            let mut m = model(KEY).with_wrap(Wrap::Anywhere);
            assert_eq!(m.wrap(), Wrap::Anywhere);

            // The lines break inside the words and every char boundary is inside the area.
            let (mut bmp, mut gfx) = fresh();
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            let lines = lines(&mut p, pal, area.w, Face::Mono, 12.0, &m);
            assert!(lines.len() >= 4, "scale {scale}: {} lines", lines.len());
            let mut at = 0;
            for l in &lines {
                assert_eq!(l.range.start, at, "the lines are contiguous");
                at = l.range.end;
                let right = l.stops.iter().copied().fold(0.0, f32::max);
                assert!(
                    right <= area.w + 0.01,
                    "scale {scale}: line {:?} ends at {right}, past {}",
                    l.range,
                    area.w
                );
            }
            assert_eq!(at, KEY.len(), "every byte is on a line, none added");

            // Drawn, the clip cuts nothing: the paragraph unclipped is the same pixels, and it
            // has no ink right of the area. Every line has ink.
            draw(&mut p, pal, area, Face::Mono, 12.0, &m, false);
            let (mut free, mut gfx) = fresh();
            let mut q = Painter::new(&mut free, &mut gfx, scale);
            q.paragraph_wrapped(
                Face::Mono,
                12.0,
                area.x,
                area.y,
                area.w,
                KEY,
                pal.text,
                Wrap::Anywhere,
            );
            assert!(
                bmp.pixels == free.pixels,
                "scale {scale}, dark {}: the clip cut a char off",
                pal.dark
            );
            let edge = (area.right() * scale).ceil() as usize;
            let rows = (0, (h * scale) as usize);
            assert!(
                (edge..free.width as usize).all(|x| !inked(&free, x, rows.0, rows.1, pal.page)),
                "scale {scale}: ink right of the area"
            );
            for l in &lines {
                let y0 = ((area.y + l.y) * scale) as usize;
                let y1 = ((area.y + l.y + l.height) * scale) as usize;
                let x0 = (area.x * scale) as usize;
                assert!(
                    (x0..edge).any(|x| inked(&bmp, x, y0, y1, pal.page)),
                    "scale {scale}: line {:?} has no ink",
                    l.range
                );
            }
            // Words alone would have run past the area: the test asks something.
            let (mut words, mut gfx) = fresh();
            Painter::new(&mut words, &mut gfx, scale).paragraph(
                Face::Mono,
                12.0,
                area.x,
                area.y,
                area.w,
                KEY,
                pal.text,
            );
            assert!(
                (edge..words.width as usize).any(|x| inked(&words, x, rows.0, rows.1, pal.page)),
                "scale {scale}: breaking between words kept the key inside the area"
            );

            // A selection across the first break copies the text as it was, with no line break.
            let (l0, l1) = (&lines[0], &lines[1]);
            m.press(&lines, middle_x(l0), middle(l0), Mods::default(), 1);
            m.drag(&lines, middle_x(l1), middle(l1));
            let selection = m.selection();
            assert!(selection.start < l0.range.end && l0.range.end < selection.end);
            let copied = m.key(&KeyName::Char('c'), ctrl());
            assert_eq!(copied, Some(Action::Copy(KEY[selection].to_string())));
            m.select_all();
            assert_eq!(
                m.key(&KeyName::Char('c'), ctrl()),
                Some(Action::Copy(KEY.to_string()))
            );
            // And the highlight is on both sides of the break.
            m.press(&lines, middle_x(l0), middle(l0), Mods::default(), 1);
            m.drag(&lines, middle_x(l1), middle(l1));
            let (mut bmp, mut gfx) = fresh();
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            draw(&mut p, pal, area, Face::Mono, 12.0, &m, true);
            let lit = blended(pal.page, pal.selection);
            for (l, x) in [
                (l0, last_char_x(l0)),
                (l1, (l1.stops[0] + l1.stops[1]) / 2.0),
            ] {
                let y = area.y + l.y + l.height * 0.08;
                assert!(
                    close(pixel(&bmp, area.x + x, y, scale), lit),
                    "scale {scale}, dark {}: no highlight on line {:?}",
                    pal.dark,
                    l.range
                );
            }
        }
    }
}

/// The x of the middle of the last char of `line`, in its own coordinates.
fn last_char_x(line: &Line) -> f32 {
    let n = line.stops.len();
    (line.stops[n - 2] + line.stops[n - 1]) / 2.0
}

/// The x of the middle of `line`, in its own coordinates.
fn middle_x(line: &Line) -> f32 {
    (line.stops[0] + line.stops[line.stops.len() - 1]) / 2.0
}

#[test]
fn by_default_the_lines_are_those_paragraph_draws() {
    let text = "a few words that wrap, and C:\\Program Files\\Mozilla Firefox\\uninstall";
    let mut bmp = Bitmap::new(8, 8);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.5);
    let m = model(text);
    assert_eq!(m.wrap(), Wrap::Words);
    assert_eq!(
        lines(&mut p, &LIGHT, 90.0, Face::Ui, 13.0, &m),
        p.paragraph_lines(Face::Ui, 13.0, 90.0, text, LIGHT.text)
    );
    let mut m = m.with_wrap(Wrap::Anywhere);
    m.set_wrap(Wrap::Words);
    assert_eq!(m.wrap(), Wrap::Words);
    assert!(lines(&mut p, &LIGHT, 90.0, Face::Ui, 13.0, &model("")).is_empty());
}

#[test]
fn left_and_right_drop_a_selection_at_its_ends_and_ctrl_home_and_end_go_to_the_text_ends() {
    let text = "hello world";
    let lines = lines_of(text);
    let line = &lines[0];
    let none = Mods::default();
    let x = x_in_char(line, text, 7);
    let mut m = model(text);
    m.press(&lines, x, middle(line), none, 2);
    assert_eq!(m.selection(), 6..11);
    m.key(&KeyName::Left, none);
    assert_eq!(m.caret(), (6, 6));
    m.press(&lines, x, middle(line), none, 2);
    m.key(&KeyName::Right, none);
    assert_eq!(m.caret(), (11, 11));
    m.press(&lines, x, middle(line), none, 2);
    m.key(&KeyName::Left, shift());
    assert_eq!(m.selection(), 6..10, "Shift+Left moves the caret");

    let ctrl_shift = Mods {
        ctrl: true,
        shift: true,
        ..Mods::default()
    };
    m.key(&KeyName::End, ctrl());
    assert_eq!(m.caret(), (11, 11));
    m.key(&KeyName::Home, ctrl_shift);
    assert_eq!(m.selection(), 0..11);
    m.key(&KeyName::Home, ctrl());
    assert_eq!(m.caret(), (0, 0));
    m.key(&KeyName::End, ctrl_shift);
    assert_eq!(m.selection(), 0..11);
}

#[test]
fn shift_up_on_the_first_line_and_shift_down_on_the_last_go_to_the_text_ends() {
    let text = "alpha beta gamma delta epsilon";
    let lines = layout(Face::Ui, 13.0, 70.0, text, 1.0);
    let none = Mods::default();
    let mut m = model(text);
    m.press(
        &lines,
        x_in_char(&lines[0], text, 2),
        middle(&lines[0]),
        none,
        1,
    );
    let at = m.caret().0;
    assert!(at > 0);
    m.key(&KeyName::Up, none);
    assert_eq!(m.caret(), (at, at), "Up without Shift stays");
    m.key(&KeyName::Up, shift());
    assert_eq!(m.selection(), 0..at);
    let last = lines.last().unwrap();
    let x = x_in_char(last, text, last.range.start + 1);
    m.press(&lines, x, middle(last), none, 1);
    let at = m.caret().0;
    m.key(&KeyName::Down, none);
    assert_eq!(m.caret(), (at, at), "Down without Shift stays");
    m.key(&KeyName::Down, shift());
    assert_eq!(m.selection(), at..text.len());
}

#[test]
fn a_shift_click_after_a_double_click_keeps_the_word() {
    let text = "one two three";
    let lines = lines_of(text);
    let line = &lines[0];
    let none = Mods::default();
    let two = text.find("two").unwrap();
    let mut m = model(text);
    m.press(&lines, x_in_char(line, text, two), middle(line), none, 2);
    assert_eq!(m.selected_text(), "two");
    m.press(&lines, 0.0, middle(line), shift(), 1);
    assert_eq!(
        m.selection(),
        0..two + 3,
        "back to the start, the word kept"
    );
    m.press(&lines, 1000.0, middle(line), shift(), 1);
    assert_eq!(
        m.selection(),
        two..text.len(),
        "and forward, from its start"
    );
    m.drag(&lines, 0.0, middle(line));
    assert_eq!(
        m.selection(),
        0..two + 3,
        "a drag after it keeps the word too"
    );
    m.release();
    let x = x_in_char(line, text, two + 1);
    m.press(&lines, x, middle(line), shift(), 1);
    assert_eq!(m.selected_text(), "two", "inside the word, the word");
    // After a plain click the anchor is the click again.
    m.press(&lines, 0.0, middle(line), none, 1);
    m.press(&lines, 1000.0, middle(line), shift(), 1);
    assert_eq!(m.selection(), 0..text.len());
    // A triple click keeps its line the same way.
    let text = "one\ntwo three";
    let lines = lines_of(text);
    let mut m = model(text);
    m.press(&lines, 1.0, middle(&lines[1]), none, 3);
    assert_eq!(m.selected_text(), "two three");
    m.press(&lines, 0.0, middle(&lines[0]), shift(), 1);
    assert_eq!(m.selection(), 0..text.len());
}

#[test]
fn select_all_is_disabled_on_empty_text() {
    let m = model("");
    assert!(!m.context_menu_items()[1].enabled);
    let m = model("x");
    assert!(m.context_menu_items()[1].enabled);
}

#[test]
fn a_selected_empty_line_has_a_highlight_as_wide_as_a_space() {
    let text = "a\n\nb";
    for scale in [1.0, 1.5] {
        let (w, h) = (200.0, 80.0);
        let area = Rect::new(10.0, 10.0, 150.0, 60.0);
        let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        p.fill(Rect::new(0.0, 0.0, w, h), LIGHT.page);
        let lines = p.paragraph_lines(Face::Ui, 13.0, area.w, text, LIGHT.text);
        assert_eq!(lines.len(), 3);
        let space = p.caret_stops(Face::Ui, 13.0, " ")[1];
        assert!(space >= 2.0, "a space is {space} px");
        let mut m = model(text);
        m.select_all();
        draw(&mut p, &LIGHT, area, Face::Ui, 13.0, &m, true);
        let y = area.y + middle(&lines[1]);
        let expected = blended(LIGHT.page, LIGHT.selection);
        assert!(
            close(pixel(&bmp, area.x + space / 2.0, y, scale), expected),
            "scale {scale}: the empty line has no highlight"
        );
        assert!(
            close(pixel(&bmp, area.x + space + 3.0, y, scale), LIGHT.page),
            "scale {scale}: the highlight is wider than a space"
        );
    }
}

#[test]
fn the_highlight_sits_where_the_glyphs_do_at_a_fractional_x() {
    let text = "the quick brown fox jumps over the lazy dog";
    let scale = 1.5;
    let area = Rect::new(10.5, 10.0, 380.0, 40.0);
    let (w, h) = (400.0, 60.0);
    let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, scale);
    p.fill(Rect::new(0.0, 0.0, w, h), LIGHT.page);
    let lines = p.paragraph_lines(Face::Ui, 13.0, area.w, text, LIGHT.text);
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    let ox = (area.x * scale).round();
    // A stop where the glyphs' device px and the rounding of the plain sum differ.
    let (i, dev) = (1..line.stops.len() - 1)
        .map(|i| (i, (ox + line.stops[i] * scale).round()))
        .find(|&(i, dev)| ((area.x + line.stops[i]) * scale).round() != dev)
        .expect("a stop where the two roundings differ");
    let mut m = model(text);
    m.press(&lines, line.stops[i], middle(line), Mods::default(), 1);
    assert_eq!(m.caret().0, i, "ASCII: the stop index is the byte offset");
    m.key(&KeyName::End, shift());
    draw(&mut p, &LIGHT, area, Face::Ui, 13.0, &m, true);
    let row = ((area.y * scale).round() + line.y * scale).round() as usize + 1;
    let at = |x: usize| {
        let o = (row * bmp.width as usize + x) * 4;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    };
    let expected = blended(LIGHT.page, LIGHT.selection);
    assert!(
        close(at(dev as usize), expected),
        "the highlight starts at {dev}"
    );
    assert!(close(at(dev as usize - 1), LIGHT.page), "and not before it");
}

/// The sheet a person looks at: a log line and a path, with and without a selection, light above
/// and dark below. `cargo test -p mdshared-ui seltext_sheet -- --ignored`, then open
/// `target/specimen/seltext.png` and `seltext-150.png`.
#[test]
#[ignore]
fn seltext_sheet() {
    let text = "Opening /home/me/Pictures/a rather long file name.png\n\
                Done in 12 ms. café 日本語 😀";
    let (w, h) = (420.0, 220.0);
    for (scale, file) in [(1.0, "seltext.png"), (1.5, "seltext-150.png")] {
        let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (half, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
            let oy = half as f32 * (h / 2.0);
            p.fill(Rect::new(0.0, oy, w, h / 2.0), pal.page);
            let area = Rect::new(14.0, oy + 14.0, w - 28.0, 70.0);
            let lines = p.paragraph_lines(Face::Mono, 12.0, area.w, text, pal.text);
            let mut m = model(text);
            m.press(&lines, 0.0, lines[0].y + 2.0, Mods::default(), 1);
            m.drag(&lines, 150.0, lines[0].y + 2.0);
            draw(&mut p, pal, area, Face::Mono, 12.0, &m, true);
            let below = Rect::new(area.x, area.y + 84.0, area.w, 70.0);
            let mut all = model(text);
            all.select_all();
            draw(&mut p, pal, below, Face::Mono, 12.0, &all, true);
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join(file)).unwrap();
    }
}
