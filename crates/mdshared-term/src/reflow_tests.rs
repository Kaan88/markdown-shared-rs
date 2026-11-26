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
    assert_eq!(grid(&s), "at goes\n past t\nhe edge\nshort");
    assert_eq!(s.scrollback_len(), 2);
    assert_eq!(at(&s), (3, 5));
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
    assert_eq!((c.row, c.col), (1, 3), "three columns into the row below");
    assert_eq!(grid(&s), "abcdefg\nhijklmn\nopqrst\nxyz\n\n");

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
    assert_eq!(grid(&s), "abc\ndef\ngh");
    assert_eq!(s.scrollback_len(), 0);
    assert_eq!(logical(&s), ["abcdef", "gh"]);
    check(&s);
}

#[test]
fn a_wide_character_at_the_edge_is_padded_off_and_never_cut() {
    let mut s = fed(6, 3, "abcd漢\r\nxyz");
    assert_eq!(grid(&s), "abcd漢\nxyz\n");
    s.resize_reflow(5, 3);
    // It does not fit in the last column: that one stays empty and it goes in the row below.
    assert_eq!(grid(&s), "abcd\n漢\nxyz");
    assert_eq!(logical(&s), ["abcd漢", "xyz"]);
    assert_eq!(
        s.scrollback_len(),
        0,
        "the whole text still fits the screen"
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
        grid(&s),
        "main lin\ne that i\ns long\nsecond",
        "the line is joined onto the rows it grew, not cut"
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
fn the_text_below_the_cursor_is_kept_and_the_overflow_goes_into_the_scrollback() {
    // Six full lines on six rows, the cursor at the top: narrowing makes every line four rows
    // long, and not one of them may be lost below the screen.
    let mut s = Screen::new(20, 6, 1000);
    let lines = [
        "aaaaaaaaaaaaaaaaaaaa",
        "bbbbbbbbbbbbbbbbbbbb",
        "cccccccccccccccccccc",
        "dddddddddddddddddddd",
        "eeeeeeeeeeeeeeeeeeee",
        "ffffffffffffffffffff",
    ];
    for line in lines {
        s.feed(line.as_bytes());
        s.feed(b"\r\n");
    }
    s.feed(b"\x1b[1;1H");
    let before = logical(&s);
    assert_eq!(before, lines);
    s.resize_reflow(5, 6);
    assert_eq!(logical(&s), before, "every line of text is still there");
    assert_eq!(
        s.line(s.rows() - 1).map(Line::text).as_deref(),
        Some("fffff"),
        "the bottom of the text is on the screen"
    );
    assert!(s.scrollback_len() > 0, "what did not fit is history");
    check(&s);
}

#[test]
fn an_anchor_line_is_reported_where_its_text_went() {
    let mut s = Screen::new(20, 3, 100);
    s.feed(b"one two\r\nsix ten\r\nnine\r\nfour");
    s.feed(b"\x1b[2;1H");
    let anchor = s.screen_start();
    let was = s.line_at(anchor).expect("the screen's top line").text();
    let now = s
        .resize_reflow_anchored(6, 3, Some(anchor))
        .expect("the line is still kept");
    let there = s.line_at(now).expect("the line the anchor named");
    assert!(
        was.starts_with(&there.text()),
        "the anchor's text: {was:?} at {now} is {:?}",
        there.text()
    );
}

#[test]
fn an_anchor_line_the_ring_dropped_is_not_reported() {
    let mut s = Screen::new(20, 3, 2);
    s.feed(b"aaaa bbbb cccc\r\ndddd\r\neeee\r\nffff\r\ngggg\r\nhhhh");
    assert!(s.first_line() > 0, "the ring has dropped something");
    let gone = s.first_line() - 1;
    assert_eq!(
        s.resize_reflow_anchored(6, 3, Some(gone)),
        None,
        "a line the scrollback dropped has no place to point at"
    );
}

/// Every line of text the screen keeps, exactly as it holds it: the cells a line says are text,
/// with the spacers of wide characters left out, the soft-wrapped rows joined and the empty
/// rows after the last line dropped. Read from the screen alone, so that a test can compare it
/// with the text it wrote itself.
fn exact(s: &Screen) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut joined = false;
    for n in s.first_line()..s.end_line() {
        let line = s.line_at(n).expect("every kept line is there");
        let text = text_of(&line.cells()[..line.text_len()]);
        if joined {
            out.last_mut().expect("a line to join").push_str(&text);
        } else {
            out.push(text);
        }
        joined = line.wrapped();
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

/// Asserts that the lines the screen keeps are the end of the lines the test wrote: the ring may
/// have dropped whole rows from the front, which cuts the first one, and nothing else may have
/// changed, character for character and space for space.
fn assert_kept(model: &[String], screen: &Screen, round: usize) {
    let kept = exact(screen);
    if kept.is_empty() {
        return;
    }
    let mut tail: Vec<&String> = model.iter().collect();
    while tail.last().is_some_and(|line| line.is_empty()) {
        tail.pop();
    }
    assert!(
        kept.len() <= tail.len(),
        "round {round}: {} lines kept of {} written: {kept:?}",
        kept.len(),
        tail.len()
    );
    let start = tail.len() - kept.len();
    for (i, (line, want)) in kept.iter().zip(tail[start..].iter()).enumerate() {
        if i == 0 {
            assert!(
                want.ends_with(line),
                "round {round}: the first kept line {line:?} is not the end of {want:?}"
            );
        } else {
            assert_eq!(line, *want, "round {round}: line {}", start + i);
        }
    }
}

#[test]
fn the_text_the_test_wrote_survives_a_random_sequence_of_resizes() {
    // The model is the lines the test writes; the screen's reflow must give them back, whatever
    // it does with the rows in between. Nothing here reads the screen to decide what to expect.
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for round in 0..600 {
        let cols = 3 + (next() % 18) as usize;
        let rows = 2 + (next() % 4) as usize;
        let mut s = Screen::new(cols, rows, 20);
        let mut model: Vec<String> = Vec::new();
        let count = 1 + (next() % 5) as usize;
        for i in 0..count {
            let mut line = String::new();
            for _ in 0..(next() % 24) {
                line.push(match next() % 10 {
                    0 => '漢',
                    1 | 2 => ' ',
                    _ => (b'a' + (next() % 26) as u8) as char,
                });
            }
            let red = next() % 4 == 0;
            if red {
                s.feed(b"\x1b[41m");
            }
            s.feed(line.as_bytes());
            if red {
                s.feed(b"\x1b[0m");
            }
            if i + 1 < count {
                s.feed(b"\r\n");
            }
            model.push(line);
        }
        // Put the cursor in the middle of the content now and then, and enter the alternate
        // screen: neither writes a character.
        if next() % 2 == 0 {
            let row = 1 + (next() % rows as u64) as usize;
            let col = 1 + (next() % cols as u64) as usize;
            s.feed(format!("\x1b[{row};{col}H").as_bytes());
        }
        let alt = next() % 3 == 0;
        if alt {
            s.feed(b"\x1b[?1049h");
        }
        // The width changes and the height does not: a change of height alone is `resize`'s,
        // which drops the rows below the cursor, and has its own test.
        let mut width = cols;
        for _ in 0..(1 + next() % 3) {
            width = 2 + (next() % 20) as usize;
            s.resize_reflow(width, rows);
            check(&s);
        }
        if alt {
            s.feed(b"\x1b[?1049l");
        }
        check(&s);
        assert_kept(&model, &s, round);
        // And back to where it started: every line the ring still holds reads as it was
        // written, spaces and wide characters and all.
        if s.first_line() == 0 {
            s.resize_reflow(cols, rows);
            check(&s);
            if s.first_line() == 0 {
                let mut want = model.clone();
                while want.last().is_some_and(|line| line.is_empty()) {
                    want.pop();
                }
                assert_eq!(
                    exact(&s),
                    want,
                    "round {round}: after {width} columns, back at {cols}"
                );
            }
        }
    }
}

#[test]
fn ich_irm_ech_and_cursor_moves_keep_the_text_through_resizes() {
    // The operations the review named, one after another with a resize after each, asserting
    // the exact text every time.
    let mut s = Screen::new(8, 4, 100);
    s.feed(b"abcdefgh");
    assert_eq!(s.row_text(0), "abcdefgh");
    // ICH in the middle of a full row: two cells open, and the last two fall off it.
    s.feed(b"\x1b[1;3H\x1b[2@");
    assert_eq!(s.row_text(0), "ab  cdef");
    s.resize_reflow(5, 4);
    assert_eq!(logical(&s), ["ab  cdef"]);
    // DCH at the start of the row, which is the first row of a wrapped line: the tail of the
    // row is blanked, so what was one line of text is two now, and stays two.
    s.feed(b"\x1b[1;1H\x1b[2P");
    assert_eq!(logical(&s), ["  c", "def"]);
    s.resize_reflow(8, 4);
    assert_eq!(logical(&s), ["  c", "def"]);
    // IRM: a character written at the start of the row pushes the rest of the row right, and
    // the cell that falls off its end is gone from the text.
    s.feed(b"\x1b[4h\x1b[1;1HXY\x1b[4l");
    assert_eq!(s.row_text(0), "XY  c");
    s.resize_reflow(3, 4);
    assert_eq!(logical(&s), ["XY  c", "def"]);
    // ECH erases to the end of the first row: the row's text ends there, and the rows below it
    // are lines of text of their own now.
    s.feed(b"\x1b[1;2H\x1b[2X");
    assert_eq!(logical(&s), ["X", " c", "def"]);
    s.resize_reflow(6, 4);
    assert_eq!(logical(&s), ["X", " c", "def"]);
    // A cursor move and EL: the rest of the row goes with the text that was on it.
    s.feed(b"\x1b[1;2H\x1b[K");
    assert_eq!(logical(&s), ["X", " c", "def"]);
    s.resize_reflow(10, 4);
    assert_eq!(logical(&s), ["X", " c", "def"]);
    check(&s);
}
