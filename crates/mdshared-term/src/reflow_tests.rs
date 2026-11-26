//! The tests of reflow on resize: the screen's size changes and the lines are compared as text.
//!
//! The helpers are the ones `screen_tests` uses, so that a line here means what it means there.

use crate::screen::*;
use crate::screen_tests::{at, check, fed, grid};

/// One line's content as text: the cells the line holds text in, with the spacers of wide
/// characters left out.
fn content(line: &Line) -> String {
    text_of(&line.cells()[..line.text_len()])
}

/// Every line the screen keeps, the scrollback first, with each run of soft-wrapped lines joined
/// again: the lines of text the screen holds, and the empty ones after them dropped.
fn logical(s: &Screen) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut joined = false;
    for n in s.first_line()..s.end_line() {
        let line = s.line_at(n).expect("every kept line is there");
        if joined {
            out.last_mut()
                .expect("a line to join")
                .push_str(&content(line));
        } else {
            out.push(content(line));
        }
        joined = line.wrapped();
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    // A line of text ends where its last character is: the blank cells after it on the screen
    // are not something a resize can keep or lose.
    for line in &mut out {
        let kept = line.trim_end_matches(' ').len();
        line.truncate(kept);
    }
    out
}

/// Every line a screen keeps, as text: what the two sides of a comparison must agree on.
fn kept(s: &Screen) -> Vec<String> {
    (s.first_line()..s.end_line())
        .map(|n| s.line_at(n).expect("every kept line is there").text())
        .collect()
}

/// The characters of these cells, with the spacers of wide characters left out.
fn text_of(cells: &[Cell]) -> String {
    cells
        .iter()
        .filter(|cell| !cell.is_spacer())
        .map(|cell| cell.ch)
        .collect()
}

/// The text of the line of text the cursor is on that comes before the cursor, whether the
/// cursor is on a character or past the end of the text: what a resize must leave the cursor
/// standing in. Each line of text before it contributes all it holds, cut where its last
/// character is, as `logical` cuts it.
fn before_cursor(s: &Screen) -> String {
    let cursor = s.screen_start() + s.cursor().row as u64;
    let mut out = String::new();
    let mut line_text = String::new();
    for n in s.first_line()..=cursor {
        let line = s.line_at(n).expect("the cursor's line is there");
        if n == cursor {
            // A cursor that hangs past a full row stands after its last cell.
            let upto = (s.cursor().col + usize::from(s.pending())).min(line.len());
            line_text.push_str(&text_of(&line.cells()[..upto]));
        } else {
            line_text.push_str(&content(line));
        }
        if !line.wrapped() {
            out.push_str(line_text.trim_end_matches(' '));
            line_text.clear();
        }
    }
    out.push_str(line_text.trim_end_matches(' '));
    out
}

/// Asserts that two screens hold the same thing: the same lines with the same wrapped flags,
/// the same history, the same cursor and the same modes.
fn assert_same(a: &Screen, b: &Screen) {
    assert_eq!((a.cols(), a.rows()), (b.cols(), b.rows()), "size");
    assert_eq!(grid(a), grid(b), "the grid");
    for r in 0..a.rows() {
        let (one, two) = (a.line(r).unwrap(), b.line(r).unwrap());
        assert_eq!(one.wrapped(), two.wrapped(), "row {r} wrapped");
    }
    assert_eq!(kept(a), kept(b), "the kept lines");
    assert_eq!(a.scrollback_len(), b.scrollback_len(), "the scrollback");
    assert_eq!(a.first_line(), b.first_line(), "the first line");
    assert_eq!(a.cursor(), b.cursor(), "the cursor");
    assert_eq!(a.modes(), b.modes(), "the modes");
}

#[test]
fn narrowing_and_widening_again_gives_back_the_same_lines() {
    let mut s = fed(20, 4, "a long line that goes past the edge\r\nshort");
    let before = grid(&s);
    let at_before = at(&s);
    assert_eq!(before, "a long line that goe\ns past the edge\nshort\n");

    // The line is wrapped again rather than cut, and what did not fit the screen is history.
    s.resize_reflow(7, 4);
    assert_eq!(grid(&s), " past t\nhe edge\nshort\n");
    assert_eq!(s.scrollback_len(), 3);
    assert_eq!(at(&s), (2, 5));
    check(&s);

    // Wider again: the two rows join back into the one line they were, and the rows the
    // history held come back onto the screen.
    s.resize_reflow(20, 4);
    assert_eq!(grid(&s), before);
    assert_eq!(s.scrollback_len(), 0);
    assert_eq!(at(&s), at_before);
    check(&s);
}

#[test]
fn a_character_inserted_at_the_start_of_a_wrapped_row_survives_a_reflow() {
    // The row that goes on in the next one has an empty cell at its end, for the wide character
    // that did not fit; inserting a cell at its start moves that empty cell off the row, and the
    // row's text grows by one. A reflow that went on believing the row had an empty cell at its
    // end dropped the "c" of "abc漢".
    let mut s = fed(4, 3, "abc漢");
    assert_eq!(grid(&s), "abc\n漢\n");
    s.feed(b"\x1b[1;1H\x1b[@");
    assert_eq!(s.row_text(0), " abc");
    s.resize_reflow(10, 3);
    assert_eq!(s.row_text(0), " abc漢");
    assert_eq!(logical(&s), [" abc漢"]);
    check(&s);
}

#[test]
fn inserting_at_the_start_of_a_row_does_not_split_a_wide_character() {
    // The same insert with a wide character in the row: it moves one cell right, whole, and the
    // reflow joins the line with both its cells intact.
    let mut s = fed(4, 3, "a漢字");
    assert_eq!(grid(&s), "a漢\n字\n");
    s.feed(b"\x1b[1;1H\x1b[@");
    assert_eq!((s.cell(0, 2).ch, s.cell(0, 3).is_spacer()), ('漢', true));
    s.resize_reflow(10, 3);
    assert_eq!(s.row_text(0), " a漢字");
    assert_eq!(
        (s.cell(0, 2).ch, s.cell(0, 3).is_spacer()),
        ('漢', true),
        "the character is still two cells wide"
    );
    assert_eq!(logical(&s), [" a漢字"]);
    check(&s);
}

#[test]
fn a_narrowing_behind_the_alternate_screen_keeps_the_whole_line() {
    // The main screen is cut to three columns while the alternate one is shown; the cells it
    // loses are the empty end of a row, not text, so widening again brings the line back whole.
    let mut s = fed(4, 3, "abc漢");
    s.feed(b"\x1b[?1049h");
    s.resize_reflow(3, 3);
    s.feed(b"\x1b[?1049l");
    s.resize_reflow(10, 3);
    assert_eq!(s.row_text(0), "abc漢");
    assert_eq!(logical(&s), ["abc漢"]);
    check(&s);
}

#[test]
fn a_widening_behind_the_alternate_screen_does_not_grow_spaces() {
    // The other way: a row cut to four columns, padded to eight behind the alternate screen, and
    // joined again at ten. The padding is not text, so no space appears in the line.
    let mut s = fed(4, 3, "abcdefgh");
    s.feed(b"\x1b[?1049h");
    s.resize_reflow(8, 3);
    s.feed(b"\x1b[?1049l");
    s.resize_reflow(10, 3);
    assert_eq!(s.row_text(0), "abcdefgh");
    assert_eq!(logical(&s), ["abcdefgh"]);
    check(&s);
}

#[test]
fn the_cursor_stays_on_the_same_character_of_the_same_line() {
    let mut s = Screen::new(20, 6, 100);
    s.feed(b"abcdefghijklmnopqrst\r\nxyz");
    // Put the cursor on the 'k' of the line that fills the row exactly; the line below it is
    // another line of text, since a hard break ends this one.
    s.feed(b"\x1b[1;11H");
    assert_eq!((at(&s), s.cell(0, 10).ch), ((0, 10), 'k'));

    s.resize_reflow(7, 6);
    let c = s.cursor();
    assert_eq!(s.cell(c.row, c.col).ch, 'k', "on the same character");
    assert_eq!((c.row, c.col), (0, 3), "three columns into the row below");
    assert_eq!(grid(&s), "hijklmn\nopqrst\nxyz\n\n\n");

    // Back to the width it was: the letter is under the cursor again, where it was.
    s.resize_reflow(20, 6);
    assert_eq!(at(&s), (0, 10));
    assert_eq!(s.cell(0, 10).ch, 'k');
    check(&s);
}

#[test]
fn a_hard_break_stays_where_it_is() {
    // A soft wrap joins when the screen widens; the hard break under it does not.
    let mut s = fed(4, 5, "one\r\ntwo\r\nthree");
    assert_eq!(grid(&s), "one\ntwo\nthre\ne\n");
    assert_eq!(logical(&s), ["one", "two", "three"]);
    s.resize_reflow(10, 5);
    assert_eq!(grid(&s), "one\ntwo\nthree\n\n");
    assert_eq!(logical(&s), ["one", "two", "three"]);

    // A line filled to its last column and ended by a hard break does not join the next one
    // either, however the width changes.
    let mut s = fed(6, 3, "abcdef\r\ngh");
    assert_eq!(grid(&s), "abcdef\ngh\n");
    s.resize_reflow(12, 3);
    assert_eq!(grid(&s), "abcdef\ngh\n");
    assert_eq!(logical(&s), ["abcdef", "gh"]);
    s.resize_reflow(3, 3);
    assert_eq!(grid(&s), "def\ngh\n");
    assert_eq!(s.scrollback_line(0).map(Line::text).as_deref(), Some("abc"));
    assert_eq!(logical(&s), ["abcdef", "gh"]);
    check(&s);
}

#[test]
fn a_wide_character_at_the_edge_is_padded_off_and_never_cut() {
    let mut s = fed(6, 3, "abcd漢\r\nxyz");
    assert_eq!(grid(&s), "abcd漢\nxyz\n");
    s.resize_reflow(5, 3);
    // It does not fit in the last column: that one stays empty and it goes in the row below,
    // and the row of text above it is pushed into the scrollback by the new row it takes.
    assert_eq!(grid(&s), "漢\nxyz\n");
    assert_eq!(logical(&s), ["abcd漢", "xyz"]);
    assert_eq!(
        s.scrollback_line(0).map(Line::text).as_deref(),
        Some("abcd")
    );
    check(&s);
    // Wider again: the pad is not a space of the text, so the character comes back up.
    s.resize_reflow(6, 3);
    assert_eq!(grid(&s), "abcd漢\nxyz\n");
    assert_eq!(logical(&s), ["abcd漢", "xyz"]);
    assert!(s.line(0).unwrap().cell(5).is_spacer());
    check(&s);
}

#[test]
fn the_scrollback_is_reflowed_and_the_ring_keeps_its_limit() {
    // The history is joined and wrapped again like the screen is, so no line is wider than
    // the screen it is on now.
    let mut s = Screen::new(20, 2, 8);
    s.feed(b"first line that is long\r\nsecond\r\nthird\r\nfourth");
    assert_eq!(s.scrollback_len(), 3);
    let before = logical(&s);
    s.resize_reflow(10, 2);
    assert_eq!(logical(&s), before);
    for i in 0..s.scrollback_len() {
        let line = s.scrollback_line(i).unwrap();
        assert!(line.len() <= 10, "line {i} is {} wide", line.len());
    }
    assert!(s.scrollback_len() <= 8);
    check(&s);

    // The ring keeps its limit: a history the new wrapping makes longer loses its oldest lines
    // instead of growing, and the lines on the screen do not move.
    let mut s = Screen::new(20, 2, 2);
    s.feed(b"aaaa bbbb cccc dddd\r\neeee\r\nffff");
    assert_eq!((grid(&s), s.scrollback_len()), ("eeee\nffff".into(), 1));
    s.resize_reflow(5, 2);
    assert_eq!((grid(&s), s.scrollback_len()), ("eeee\nffff".into(), 2));
    assert_eq!(s.first_line(), 2);
    assert_eq!(
        s.scrollback_line(1).map(Line::text).as_deref(),
        Some("dddd")
    );
    assert_eq!(
        kept(&s),
        ["cccc", "dddd", "eeee", "ffff"],
        "the two oldest rows of the long line are gone, and no other line is"
    );
    check(&s);
}

#[test]
fn the_alternate_screen_is_cut_and_the_main_screen_behind_it_is_reflowed() {
    let text = "main line that is long\r\nsecond\x1b[?1049h\
        the alternate screen text is long enough to wrap\r\nand more";
    let mut s = fed(20, 4, text);
    let mut plain = fed(20, 4, text);
    assert!(s.modes().alt_screen);
    s.resize_reflow(8, 4);
    plain.resize(8, 4);
    // The alternate screen's own picture is cut, as `resize` cuts it: the same rows.
    assert_eq!(grid(&s), grid(&plain));
    // The main screen behind it is joined and wrapped again, so leaving it gives it back whole.
    s.feed(b"\x1b[?1049l");
    plain.feed(b"\x1b[?1049l");
    assert_eq!(
        grid(&plain),
        "main lin\nng\nsecond\n",
        "the plain call cut it"
    );
    assert_eq!(logical(&s), ["main line that is long", "second"]);
    assert_eq!(
        s.scrollback_line(0).map(Line::text).as_deref(),
        Some("main lin"),
        "and the rest of the line is on the screen below it"
    );
    check(&s);
}

#[test]
fn the_saved_cursor_follows_its_text_through_a_reflow() {
    // ESC 7 keeps a place and ESC 8 goes back to it: after a re-wrap the place is on the same
    // character, not on the same row.
    let mut s = fed(20, 4, "line one\r\nline two\r\nprompt$ \x1b7");
    s.resize_reflow(6, 4);
    s.feed(b"\x1b8");
    let c = s.cursor();
    assert_eq!(s.row_text(c.row), "$");
    assert_eq!(c.col, 2);
}

#[test]
fn the_main_screen_behind_the_alternate_one_is_reflowed_with_its_saved_cursor() {
    let mut s = fed(20, 4, "line one\r\nline two\r\nprompt$ \x1b[?1049h");
    s.resize_reflow(6, 4);
    s.feed(b"\x1b[?1049l");
    let c = s.cursor();
    assert_eq!(s.row_text(c.row), "$");
    assert_eq!(c.col, 2);
    assert_eq!(logical(&s), ["line one", "line two", "prompt$"]);
    check(&s);
}

#[test]
fn a_change_of_height_alone_is_what_it_was() {
    for rows in [1, 3, 6] {
        let mut a = fed(12, 4, "one\r\ntwo that is long\r\nthree");
        let mut b = a.clone();
        a.resize_reflow(12, rows);
        b.resize(12, rows);
        assert_same(&a, &b);
    }
}

#[test]
fn the_logical_content_survives_a_random_resize_round_trip() {
    // A small generator with a fixed seed: the same text every run.
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for round in 0..1500 {
        let cols = 2 + (next() % 24) as usize;
        let rows = 2 + (next() % 5) as usize;
        let mut s = Screen::new(cols, rows, usize::MAX);
        let mut text = String::new();
        for _ in 0..(next() % 6) {
            for _ in 0..(next() % 30) {
                text.push(match next() % 10 {
                    0 => '漢',
                    1 | 2 => ' ',
                    _ => (b'a' + (next() % 26) as u8) as char,
                });
            }
            // Most lines are ended by a hard break; some run on into the next.
            if next() % 4 != 0 {
                text.push_str("\r\n");
            }
        }
        s.feed(text.as_bytes());
        let before = logical(&s);
        let place = before_cursor(&s);
        let narrow = 2 + (next() % 24) as usize;
        let narrow_rows = 2 + (next() % 5) as usize;
        s.resize_reflow(narrow, narrow_rows);
        check(&s);
        assert_eq!(
            logical(&s),
            before,
            "round {round}: {text:?} at {narrow} columns, {narrow_rows} rows"
        );
        assert_eq!(
            before_cursor(&s),
            place,
            "round {round}: the cursor's place in the text, at {narrow} columns"
        );

        // And back to the size it was: the same lines again, and the cursor in its place.
        s.resize_reflow(cols, rows);
        check(&s);
        assert_eq!(
            logical(&s),
            before,
            "round {round}: {text:?} back at {cols}"
        );
        assert_eq!(before_cursor(&s), place, "round {round}: the cursor back");
    }
}
