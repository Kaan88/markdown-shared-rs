//! The screen's tests: byte streams go in, and the grid is compared as text.

// `take_dirty` gives ranges of rows, and "one range" is what many of these tests expect.
#![allow(clippy::single_range_in_vec_init)]

use crate::screen::*;

/// The screen's rows as text, one a line, trailing spaces gone.
fn grid(s: &Screen) -> String {
    (0..s.rows())
        .map(|r| s.row_text(r))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A screen of this size with a scrollback of 100, fed `bytes`.
fn fed(cols: usize, rows: usize, bytes: &str) -> Screen {
    let mut s = Screen::new(cols, rows, 100);
    s.feed(bytes.as_bytes());
    s
}

fn at(s: &Screen) -> (usize, usize) {
    (s.cursor().row, s.cursor().col)
}

/// What every grid must hold whatever was fed: the cursor inside it, every line as wide as the
/// screen, and every wide character whole.
fn check(s: &Screen) {
    let (row, col) = at(s);
    assert!(row < s.rows() && col < s.cols(), "cursor at {row},{col}");
    for r in 0..s.rows() {
        let line = s.line(r).unwrap();
        assert_eq!(line.len(), s.cols(), "row {r}");
        for c in 0..s.cols() {
            let cell = line.cell(c);
            match cell.width {
                0 => assert!(c > 0 && line.cell(c - 1).width == 2, "half at {r},{c}"),
                2 => assert!(
                    c + 1 < s.cols() && line.cell(c + 1).width == 0,
                    "half at {r},{c}"
                ),
                1 => {}
                w => panic!("width {w} at {r},{c}"),
            }
        }
    }
}

#[test]
fn text_and_the_plain_controls() {
    let s = fed(10, 3, "ab\r\ncd\tx\x08y");
    assert_eq!(grid(&s), "ab\ncd      y\n");
    assert_eq!(at(&s), (1, 9));
    // Line feed alone keeps the column; vertical tab and form feed are line feeds.
    assert_eq!(grid(&fed(10, 3, "ab\ncd")), "ab\n  cd\n");
    assert_eq!(grid(&fed(10, 3, "a\x0bb\x0cc")), "a\n b\n  c");
    // New-line mode: a line feed returns the carriage too.
    assert_eq!(grid(&fed(10, 3, "\x1b[20hab\ncd")), "ab\ncd\n");
    // Backspace stops at the left edge.
    assert_eq!(grid(&fed(10, 3, "a\x08\x08\x08b")), "b\n\n");
    // A new screen is blank cells.
    let s = Screen::new(4, 2, 0);
    assert_eq!(s.cell(0, 0), Cell::BLANK);
    assert_eq!(s.cell(99, 99), Cell::BLANK);
    assert!(s.line(2).is_none() && s.row_text(2).is_empty());
    assert_eq!((s.cols(), s.rows()), (4, 2));
}

#[test]
fn a_character_in_the_last_column_wraps_only_when_the_next_one_comes() {
    // Five in five columns: the cursor stays on the last one, and nothing has scrolled.
    let mut s = fed(5, 3, "abcde");
    assert_eq!(grid(&s), "abcde\n\n");
    assert_eq!(at(&s), (0, 4));
    assert!(!s.line(0).unwrap().wrapped());
    s.feed(b"\x1b[6n");
    assert_eq!(s.take_reply(), b"\x1b[1;5R");
    // The sixth wraps, and the line is marked as going on.
    s.feed(b"f");
    assert_eq!(grid(&s), "abcde\nf\n");
    assert_eq!(at(&s), (1, 1));
    assert!(s.line(0).unwrap().wrapped() && !s.line(1).unwrap().wrapped());
    // A line end after a full line leaves no empty line, and no mark.
    let s = fed(5, 3, "abcde\r\nx");
    assert_eq!(grid(&s), "abcde\nx\n");
    assert!(!s.line(0).unwrap().wrapped());
    // Moving the cursor forgets the wait.
    assert_eq!(grid(&fed(5, 3, "abcde\x08X")), "abcXe\n\n");
    assert_eq!(grid(&fed(5, 3, "abcde\x1b[1Cf")), "abcdf\n\n");
    assert_eq!(grid(&fed(5, 3, "abcde\x1b[1;5Hf")), "abcdf\n\n");
    assert_eq!(grid(&fed(5, 3, "abcde\rf")), "fbcde\n\n");
    // On the bottom row the wrap scrolls.
    let s = fed(5, 2, "11111\r\n22222x");
    assert_eq!(grid(&s), "22222\nx");
    assert_eq!(s.scrollback_len(), 1);
    // Without autowrap the last column is written over and over.
    let s = fed(5, 3, "\x1b[?7labcdefg");
    assert_eq!(grid(&s), "abcdg\n\n");
    assert_eq!(at(&s), (0, 4));
    // Switched back on, it wraps again.
    assert_eq!(grid(&fed(5, 3, "\x1b[?7labcdefg\x1b[?7hhi")), "abcdh\ni\n");
}

#[test]
fn lines_that_scroll_off_the_top_join_a_bounded_scrollback_and_keep_their_numbers() {
    let mut s = Screen::new(5, 3, 2);
    s.feed(b"1\r\n2\r\n3");
    assert_eq!((s.first_line(), s.screen_start(), s.end_line()), (0, 0, 3));
    s.feed(b"\r\n4\r\n5\r\n6");
    assert_eq!(grid(&s), "4\n5\n6");
    // Three scrolled off and two are kept.
    assert_eq!(s.scrollback_len(), 2);
    assert_eq!(s.scrollback_line(0).unwrap().text(), "2");
    assert_eq!(s.scrollback_line(1).unwrap().text(), "3");
    assert!(s.scrollback_line(2).is_none());
    assert_eq!((s.first_line(), s.screen_start(), s.end_line()), (1, 3, 6));
    assert!(s.line_at(0).is_none());
    assert_eq!(s.line_at(1).unwrap().text(), "2");
    assert_eq!(s.line_at(3).unwrap().text(), "4");
    assert_eq!(s.line_at(5).unwrap().text(), "6");
    assert!(s.line_at(6).is_none() && s.line_at(u64::MAX).is_none());
    // A scrollback line has lost its trailing blanks and still reads blank past its end.
    assert_eq!(s.scrollback_line(0).unwrap().len(), 1);
    assert_eq!(s.scrollback_line(0).unwrap().cell(4), Cell::BLANK);
    assert!(!s.scrollback_line(0).unwrap().is_empty());
    // Cleared by hand or by ED 3: the rows on the screen keep their numbers.
    s.feed(b"\x1b[3J");
    assert_eq!(s.scrollback_len(), 0);
    assert_eq!((s.first_line(), s.screen_start()), (3, 3));
    assert_eq!(s.line_at(3).unwrap().text(), "4");
    s.feed(b"\r\n7");
    s.clear_scrollback();
    assert_eq!((s.scrollback_len(), s.screen_start()), (0, 4));

    // No scrollback at all: nothing is kept and the numbers still count.
    let mut none = Screen::new(5, 2, 0);
    none.feed(b"1\r\n2\r\n3\r\n4");
    assert_eq!(
        (
            none.scrollback_len(),
            none.first_line(),
            none.screen_start()
        ),
        (0, 2, 2)
    );
    assert_eq!(grid(&none), "3\n4");
}

#[test]
fn the_cursor_goes_where_it_is_sent_and_stops_at_the_edges() {
    let s = fed(10, 4, "\x1b[2;3Hx\x1b[Ay\x1b[2Bz\x1b[4D<\x1b[Gw");
    assert_eq!(grid(&s), "   y\n  x\nw<  z\n");
    // Past the edges it stops.
    assert_eq!(at(&fed(10, 4, "\x1b[99;99H")), (3, 9));
    assert_eq!(at(&fed(10, 4, "\x1b[99;99H\x1b[999A\x1b[999D")), (0, 0));
    assert_eq!(at(&fed(10, 4, "\x1b[999B\x1b[999C")), (3, 9));
    assert_eq!(at(&fed(10, 4, "\x1b[65535;65535H\x1b[65535C")), (3, 9));
    // No parameter and 0 are 1.
    assert_eq!(at(&fed(10, 4, "\x1b[3;3H\x1b[0A\x1b[D")), (1, 1));
    assert_eq!(at(&fed(10, 4, "\x1b[3;3H\x1b[H")), (0, 0));
    assert_eq!(at(&fed(10, 4, "\x1b[;5H")), (0, 4));
    assert_eq!(at(&fed(10, 4, "\x1b[3f")), (2, 0));
    // The other spellings: column and row alone, next and previous line, relative moves.
    assert_eq!(at(&fed(10, 4, "\x1b[2;2H\x1b[7G")), (1, 6));
    assert_eq!(at(&fed(10, 4, "\x1b[2;2H\x1b[7`")), (1, 6));
    assert_eq!(at(&fed(10, 4, "\x1b[2;2H\x1b[4d")), (3, 1));
    assert_eq!(at(&fed(10, 4, "\x1b[2;5H\x1b[2E")), (3, 0));
    assert_eq!(at(&fed(10, 4, "\x1b[4;5H\x1b[2F")), (1, 0));
    assert_eq!(at(&fed(10, 4, "\x1b[2;2H\x1b[3a\x1b[1e")), (2, 4));
    // Saved and restored, both ways of saying it; restored with nothing saved is home.
    assert_eq!(at(&fed(10, 4, "\x1b[2;5H\x1b7\x1b[H\x1b8")), (1, 4));
    assert_eq!(at(&fed(10, 4, "\x1b[2;5H\x1b[s\x1b[H\x1b[u")), (1, 4));
    assert_eq!(at(&fed(10, 4, "\x1b[2;5H\x1b8")), (0, 0));
    // The save keeps the colours too.
    let s = fed(10, 4, "\x1b[31m\x1b7\x1b[0m\x1b8x");
    assert_eq!(s.cell(0, 0).fg, Color::Indexed(1));
    // Index, reverse index and next line.
    assert_eq!(grid(&fed(4, 3, "a\x1bDb\x1bEc\x1bM\x1b[Cd")), "a\n bd\nc");
}

#[test]
fn erasing_and_editing_inside_a_line() {
    let aligned = |more: &str| grid(&fed(4, 3, &format!("\x1b#8{more}")));
    assert_eq!(aligned(""), "EEEE\nEEEE\nEEEE");
    assert_eq!(aligned("\x1b[2;2H\x1b[K"), "EEEE\nE\nEEEE");
    assert_eq!(aligned("\x1b[2;2H\x1b[0K"), "EEEE\nE\nEEEE");
    assert_eq!(aligned("\x1b[2;2H\x1b[1K"), "EEEE\n  EE\nEEEE");
    assert_eq!(aligned("\x1b[2;2H\x1b[2K"), "EEEE\n\nEEEE");
    assert_eq!(aligned("\x1b[2;3H\x1b[J"), "EEEE\nEE\n");
    assert_eq!(aligned("\x1b[2;3H\x1b[1J"), "\n   E\nEEEE");
    assert_eq!(aligned("\x1b[2;3H\x1b[2J"), "\n\n");
    // Erasing does not move the cursor.
    assert_eq!(at(&fed(4, 3, "\x1b#8\x1b[2;3H\x1b[2J")), (1, 2));

    let edit = |seq: &str| grid(&fed(6, 2, &format!("abcdef\x1b[1;2H{seq}")));
    assert_eq!(edit("\x1b[3X"), "a   ef\n");
    assert_eq!(edit("\x1b[99X"), "a\n");
    assert_eq!(edit("\x1b[2@"), "a  bcd\n");
    assert_eq!(edit("\x1b[99@"), "a\n");
    assert_eq!(edit("\x1b[2P"), "adef\n");
    assert_eq!(edit("\x1b[99P"), "a\n");
    // Insert mode: what is written pushes the rest right.
    assert_eq!(edit("\x1b[4hXY"), "aXYbcd\n");
    assert_eq!(edit("\x1b[4hXY\x1b[4lZ"), "aXYZcd\n");
    // Repeat the last character.
    assert_eq!(grid(&fed(6, 2, "ab\x1b[3b")), "abbbb\n");
    assert_eq!(grid(&fed(6, 2, "\x1b[3b")), "\n");
    // Erased cells take the background that is set; written ones their own.
    let s = fed(6, 2, "\x1b[44mab\x1b[K\x1b[2;1H\x1b[41m\x1b[2X");
    assert_eq!(s.cell(0, 0).bg, Color::Indexed(4));
    assert_eq!(s.cell(0, 5).bg, Color::Indexed(4));
    assert_eq!(s.cell(1, 1).bg, Color::Indexed(1));
    assert_eq!(s.cell(1, 2).bg, Color::Default);
}

#[test]
fn lines_are_inserted_deleted_and_scrolled_inside_the_region() {
    let five = |more: &str| fed(4, 5, &format!("1\r\n2\r\n3\r\n4\r\n5{more}"));
    assert_eq!(grid(&five("\x1b[2;1H\x1b[2L")), "1\n\n\n2\n3");
    assert_eq!(grid(&five("\x1b[2;1H\x1b[2M")), "1\n4\n5\n\n");
    assert_eq!(grid(&five("\x1b[2;1H\x1b[99L")), "1\n\n\n\n");
    assert_eq!(grid(&five("\x1b[2;1H\x1b[99M")), "1\n\n\n\n");
    // Deleted lines are not history; lines scrolled off the top are.
    assert_eq!(five("\x1b[1;1H\x1b[2M").scrollback_len(), 0);
    let up = five("\x1b[2S");
    assert_eq!(grid(&up), "3\n4\n5\n\n");
    assert_eq!(up.scrollback_len(), 2);
    assert_eq!(grid(&five("\x1b[1T")), "\n1\n2\n3\n4");
    assert_eq!(grid(&five("\x1b[99T")), "\n\n\n\n");

    // A region of rows 2 to 4: a line feed on its last row scrolls it alone, and nothing
    // joins the scrollback, since the screen's top row stayed.
    let s = five("\x1b[2;4r\x1b[4;1H\n");
    assert_eq!(grid(&s), "1\n3\n4\n\n5");
    assert_eq!(s.scrollback_len(), 0);
    assert_eq!(grid(&five("\x1b[2;4r\x1b[2;1H\x1bM")), "1\n\n2\n3\n5");
    assert_eq!(grid(&five("\x1b[2;4r\x1b[2S")), "1\n4\n\n\n5");
    assert_eq!(grid(&five("\x1b[2;4r\x1b[3;1H\x1b[L")), "1\n2\n\n3\n5");
    // Outside the region, inserting and deleting lines do nothing, and a line feed on the
    // screen's last row does not scroll.
    assert_eq!(
        grid(&five("\x1b[2;4r\x1b[5;1H\x1b[L\x1b[M\n\n")),
        "1\n2\n3\n4\n5"
    );
    // Setting the region homes the cursor; one row or an upside-down one is refused.
    assert_eq!(at(&five("\x1b[2;4r")), (0, 0));
    assert_eq!(grid(&five("\x1b[3;3r\x1b[5;1H\nx")), "2\n3\n4\n5\nx");
    assert_eq!(grid(&five("\x1b[4;2r\x1b[5;1H\nx")), "2\n3\n4\n5\nx");
    // Up and down stop at the region's edges from inside it.
    assert_eq!(at(&five("\x1b[2;4r\x1b[3;1H\x1b[9A")), (1, 0));
    assert_eq!(at(&five("\x1b[2;4r\x1b[3;1H\x1b[9B")), (3, 0));
    // A region whose top is the screen's top feeds the scrollback.
    let s = five("\x1b[1;3r\x1b[3;1H\n");
    assert_eq!(grid(&s), "2\n3\n\n4\n5");
    assert_eq!(s.scrollback_len(), 1);
}

#[test]
fn origin_mode_counts_rows_from_the_region() {
    let mut s = fed(4, 5, "\x1b[2;4r\x1b[?6h");
    assert_eq!(at(&s), (1, 0));
    s.feed(b"\x1b[1;1HX\x1b[99;1HY\x1b[6n");
    assert_eq!(grid(&s), "\nX\n\nY\n");
    // The report counts from the region too.
    assert_eq!(s.take_reply(), b"\x1b[3;2R");
    s.feed(b"\x1b[2dZ");
    assert_eq!(s.row_text(2), " Z");
    s.feed(b"\x1b[?6l\x1b[1;1HT");
    assert_eq!(s.row_text(0), "T");
}

#[test]
fn colours_and_attributes() {
    let s = fed(
        20,
        2,
        "\x1b[1;4;31;42mA\x1b[0mB\x1b[38;5;200mC\x1b[48;2;1;2;3mD\x1b[38:2::9:8:7mE\x1b[38:5:17mF\
         \x1b[91;104mG\x1b[39;49mH\x1b[7;9;3;2mI\x1b[22;23;27;29mJ\x1b[4mK\x1b[4:0mL\x1b[5;8mM\x1b[mN",
    );
    assert_eq!(s.row_text(0), "ABCDEFGHIJKLMN");
    let cell = |c: usize| s.cell(0, c);
    assert_eq!(cell(0).attrs, Attrs::BOLD.with(Attrs::UNDERLINE));
    assert_eq!(
        (cell(0).fg, cell(0).bg),
        (Color::Indexed(1), Color::Indexed(2))
    );
    assert_eq!(
        cell(1),
        Cell {
            ch: 'B',
            ..Cell::BLANK
        }
    );
    assert_eq!(cell(2).fg, Color::Indexed(200));
    assert_eq!(
        (cell(3).fg, cell(3).bg),
        (Color::Indexed(200), Color::Rgb(1, 2, 3))
    );
    assert_eq!(cell(4).fg, Color::Rgb(9, 8, 7));
    assert_eq!(cell(5).fg, Color::Indexed(17));
    assert_eq!(
        (cell(6).fg, cell(6).bg),
        (Color::Indexed(9), Color::Indexed(12))
    );
    assert_eq!((cell(7).fg, cell(7).bg), (Color::Default, Color::Default));
    let all = Attrs::INVERSE
        .with(Attrs::STRIKE)
        .with(Attrs::ITALIC)
        .with(Attrs::DIM);
    assert_eq!(cell(8).attrs, all);
    assert!(all.contains(Attrs::DIM) && !all.contains(Attrs::BOLD) && !all.is_empty());
    assert_eq!(all.bits(), 2 + 4 + 32 + 128);
    assert!(cell(9).attrs.is_empty());
    assert_eq!(cell(10).attrs, Attrs::UNDERLINE);
    assert_eq!(cell(11).attrs, Attrs::NONE);
    assert_eq!(cell(12).attrs, Attrs::BLINK.with(Attrs::HIDDEN));
    assert_eq!(
        cell(13),
        Cell {
            ch: 'N',
            ..Cell::BLANK
        }
    );

    // The colon form without the colour-space slot, and numbers past 255.
    let s = fed(
        9,
        1,
        "\x1b[38:2:9:8:7mA\x1b[48;2;999;0;300mB\x1b[38;5;999mC",
    );
    assert_eq!(s.cell(0, 0).fg, Color::Rgb(9, 8, 7));
    assert_eq!(s.cell(0, 1).bg, Color::Rgb(255, 0, 255));
    assert_eq!(s.cell(0, 2).fg, Color::Indexed(255));
    // A colour of a kind nobody knows is skipped, and what follows still counts.
    let s = fed(9, 1, "\x1b[38;9;1mA\x1b[0;38:7mB\x1b[38mC");
    assert_eq!(
        (s.cell(0, 0).fg, s.cell(0, 0).attrs),
        (Color::Default, Attrs::BOLD)
    );
    assert_eq!(
        s.cell(0, 1),
        Cell {
            ch: 'B',
            ..Cell::BLANK
        }
    );
    assert_eq!(s.cell(0, 2).fg, Color::Default);
}

#[test]
fn wide_characters_take_two_cells_and_are_never_left_as_halves() {
    let s = fed(6, 2, "a漢b");
    assert_eq!(s.row_text(0), "a漢b");
    assert_eq!((s.cell(0, 1).ch, s.cell(0, 1).width), ('漢', 2));
    assert!(s.cell(0, 2).is_spacer() && !s.cell(0, 1).is_spacer());
    assert_eq!(at(&s), (0, 4));
    check(&s);
    // In the last column it does not fit: the column stays empty and it goes below.
    let s = fed(4, 2, "abc漢");
    assert_eq!(grid(&s), "abc\n漢");
    assert!(s.line(0).unwrap().wrapped());
    check(&s);
    // Ending exactly at the edge it waits there like any character.
    let s = fed(4, 2, "ab漢");
    assert_eq!((grid(&s), at(&s)), ("ab漢\n".to_string(), (0, 3)));
    let s = fed(4, 2, "ab漢c");
    assert_eq!(grid(&s), "ab漢\nc");
    // Without autowrap it is written in the last two columns.
    let s = fed(4, 2, "\x1b[?7labc漢");
    assert_eq!(grid(&s), "ab漢\n");
    check(&s);
    // Writing over either half blanks the other.
    for (seq, want) in [
        ("漢\rx", "x"),
        ("漢\x1b[1;2Hx", " x"),
        ("a漢\x1b[1;2H\x1b[X", "a"),
        ("a漢\x1b[1;3H\x1b[X", "a"),
        ("a漢b\x1b[1;3H\x1b[P", "a b"),
        ("a漢b\x1b[1;3H\x1b[@", "a   b"),
        ("a漢b\x1b[1;2H\x1b[1K", "   b"),
        ("a漢b\x1b[1;3H\x1b[K", "a"),
        ("漢漢漢\x1b[1;1H\x1b[@", " 漢漢"),
        ("漢漢漢\x1b[1;2H\x1b[4hx", " x 漢"),
    ] {
        let s = fed(6, 2, seq);
        assert_eq!(s.row_text(0), want, "{seq:?}");
        check(&s);
    }
    // A mark that combines is dropped, and so is a variation selector.
    assert_eq!(grid(&fed(6, 1, "e\u{301}x\u{fe0f}y\u{200d}")), "exy");
    // One column: a wide character is kept in the one cell there is.
    let s = fed(1, 2, "漢");
    assert_eq!((s.cell(0, 0).ch, s.cell(0, 0).width), ('漢', 1));
    check(&s);
}

#[test]
fn the_alternate_screen_comes_empty_and_gives_the_main_one_back() {
    let mut s = fed(6, 3, "main\r\ntwo");
    assert!(!s.modes().alt_screen);
    s.feed(b"\x1b[?1049h");
    assert!(s.modes().alt_screen);
    assert_eq!(grid(&s), "\n\n");
    s.feed(b"\x1b[HALT\r\n1\r\n2\r\n3\r\n4");
    assert_eq!(grid(&s), "2\n3\n4");
    // What scrolls off the alternate screen is not history.
    assert_eq!(s.scrollback_len(), 0);
    s.feed(b"\x1b[?1049l");
    assert!(!s.modes().alt_screen);
    assert_eq!(grid(&s), "main\ntwo\n");
    // The cursor is back where it was when the program left.
    assert_eq!(at(&s), (1, 3));
    // The older switches do the same without the cursor; twice on is once.
    for n in [47, 1047] {
        let mut s = fed(6, 3, "main");
        s.feed(format!("\x1b[?{n}h\x1b[?{n}hx\x1b[3;3H").as_bytes());
        assert_eq!(grid(&s), "    x\n\n");
        s.feed(format!("\x1b[?{n}l").as_bytes());
        assert_eq!(grid(&s), "main\n\n");
        assert_eq!(at(&s), (2, 2));
        s.feed(format!("\x1b[?{n}l").as_bytes());
        assert_eq!(grid(&s), "main\n\n");
    }
    // Each screen has its own saved cursor.
    let s = fed(
        6,
        3,
        "\x1b[2;2H\x1b7\x1b[?47h\x1b[3;3H\x1b7\x1b[?47l\x1b[H\x1b8",
    );
    assert_eq!(at(&s), (1, 1));
}

#[test]
fn tab_stops() {
    let mut s = fed(20, 1, "\ta\tb");
    assert_eq!(s.row_text(0), "        a       b");
    // At the last stop a tab goes to the last column.
    s.feed(b"\t\tc");
    assert_eq!(s.row_text(0), "        a       b  c");
    // All cleared, one set at column 5; back tab finds it from the right.
    s.feed(b"\x1b[3g\r\x1b[5G\x1bH\r\td\x1b[20G\x1b[Ze");
    assert_eq!(s.row_text(0), "    e   a       b  c");
    assert_eq!(at(&s), (0, 5));
    // One stop cleared; two tabs forward and two back, each in one sequence.
    let s = fed(30, 1, "\x1b[9G\x1b[g\r\tx\r\x1b[2Iy\x1b[2Zz");
    assert_eq!(s.row_text(0), "                z       y");
    // Back from the first stop is the left edge.
    assert_eq!(at(&fed(20, 1, "\x1b[4G\x1b[9Z")), (0, 0));
}

#[test]
fn the_line_drawing_set_frames_a_box() {
    assert_eq!(grid(&fed(6, 1, "\x1b(0lqk\x1b(Bq")), "┌─┐q");
    assert_eq!(grid(&fed(6, 1, "\x1b)0\x0eqx\x0fqx")), "─│qx");
    assert_eq!(grid(&fed(6, 1, "\x1b(0`a~Q\x1b(A`")), "◆▒·Q`");
    // It is saved and restored with the cursor.
    assert_eq!(grid(&fed(6, 1, "\x1b(0\x1b7\x1b(B\x1b8q")), "─");
}

#[test]
fn queries_are_answered_in_order_and_the_answers_are_bounded() {
    let mut s = Screen::new(10, 5, 0);
    assert!(s.take_reply().is_empty());
    s.feed(b"\x1b[c\x1b[0c\x1b[>c\x1b[5n\x1b[3;7H\x1b[6n");
    assert_eq!(
        s.take_reply(),
        b"\x1b[?1;2c\x1b[?1;2c\x1b[>0;0;0c\x1b[0n\x1b[3;7R"
    );
    assert!(s.take_reply().is_empty());
    s.feed(b"\x1b[?25$p\x1b[?25l\x1b[?25$p\x1b[?2004$p\x1b[?9999$p");
    assert_eq!(
        s.take_reply(),
        b"\x1b[?25;1$y\x1b[?25;2$y\x1b[?2004;2$y\x1b[?9999;0$y"
    );
    // Queries nobody collects do not pile up without end.
    for _ in 0..10_000 {
        s.feed(b"\x1b[c");
    }
    let n = s.take_reply().len();
    assert!(n > 0 && n <= 4096, "{n} bytes of replies");
    // Queries this terminal does not know get no answer.
    s.feed(b"\x1b[1c\x1b[9n\x1b[>1c");
    assert!(s.take_reply().is_empty());
}

#[test]
fn an_explicit_cursor_style_is_distinct_from_the_default_and_dirties_its_row() {
    let mut s = Screen::new(10, 3, 0);
    s.feed(b"\x1b[2;1H");
    let default = s.cursor();
    assert!(!s.cursor_style_is_explicit());
    s.take_dirty();

    s.feed(b"\x1b[1 q");
    assert!(s.cursor_style_is_explicit());
    assert_eq!(s.cursor(), default);
    let dirty = 1..2;
    assert_eq!(s.take_dirty(), std::slice::from_ref(&dirty));

    for reset in [b"\x1b[0 q".as_slice(), b"\x1b[ q"] {
        s.feed(reset);
        assert!(!s.cursor_style_is_explicit());
        assert_eq!(s.cursor(), default);
        assert_eq!(s.take_dirty(), std::slice::from_ref(&dirty));
        s.feed(b"\x1b[6 q");
        assert!(s.cursor_style_is_explicit());
        assert_eq!(s.cursor().shape, CursorShape::Bar);
    }
    s.feed(b"\x1bc");
    assert!(!s.cursor_style_is_explicit());
    assert_eq!(s.cursor(), Screen::new(10, 3, 0).cursor());
}

#[test]
fn out_of_range_cursor_styles_leave_the_cursor_and_dirty_rows_unchanged() {
    let mut s = Screen::new(10, 3, 0);
    for setup in [b"".as_slice(), b"\x1b[4 q", b"\x1b[5 q"] {
        s.feed(setup);
        let before = (s.cursor(), s.cursor_style_is_explicit());
        s.take_dirty();
        for invalid in [b"\x1b[7 q".as_slice(), b"\x1b[12 q", b"\x1b[65535 q"] {
            s.feed(invalid);
            assert_eq!((s.cursor(), s.cursor_style_is_explicit()), before);
            assert!(s.take_dirty().is_empty());
        }
    }
}

#[test]
fn modes_title_bell_and_cursor_shape() {
    let mut s = Screen::new(10, 3, 10);
    assert_eq!(s.modes(), Modes::default());
    s.feed(b"\x1b[?1;6;25;66;1004;1006;2004h\x1b[?7;25l\x1b[4;20h\x1b[?1002h");
    assert_eq!(
        s.modes(),
        Modes {
            app_cursor: true,
            app_keypad: true,
            autowrap: false,
            cursor_visible: false,
            origin: true,
            insert: true,
            newline: true,
            alt_screen: false,
            bracketed_paste: true,
            mouse: MouseMode::Drag,
            mouse_sgr: true,
            focus: true,
        }
    );
    assert!(!s.cursor().visible);
    s.feed(b"\x1b[?1000h");
    assert_eq!(s.modes().mouse, MouseMode::Press);
    s.feed(b"\x1b[?1003h");
    assert_eq!(s.modes().mouse, MouseMode::Motion);
    s.feed(b"\x1b[?1003l\x1b>");
    assert_eq!(s.modes().mouse, MouseMode::Off);
    assert!(!s.modes().app_keypad);
    s.feed(b"\x1b=");
    assert!(s.modes().app_keypad);
    // A soft reset puts the writing modes back and leaves the screen and the mouse alone.
    s.feed(b"text\x1b[2;4r\x1b[1m\x1b[!pX");
    let m = s.modes();
    assert!(m.autowrap && m.cursor_visible && !m.origin && !m.insert && !m.app_cursor);
    assert!(m.bracketed_paste && m.focus);
    assert!(s.row_text(0).starts_with("text") || s.row_text(1).starts_with("text"));
    assert!(s.cell(at(&s).0, at(&s).1 - 1).attrs.is_empty());

    // Title: OSC 0 and 2, either terminator; other commands are not titles.
    assert_eq!(s.title(), "");
    assert_eq!(s.take_title(), None);
    s.feed(b"\x1b]0;hello\x07");
    assert_eq!(s.title(), "hello");
    assert_eq!(s.take_title().as_deref(), Some("hello"));
    assert_eq!(s.take_title(), None);
    s.feed("\x1b]2;zwölf ✓\x1b\\\x1b]1;icon\x07\x1b]52;c;AAAA\x07\x1b]8;;http://x\x07".as_bytes());
    assert_eq!(s.take_title().as_deref(), Some("zwölf ✓"));
    s.feed(b"\x1b]2;\x07");
    assert_eq!(s.take_title().as_deref(), Some(""));
    let long = format!("\x1b]0;{}\x07", "t".repeat(3000));
    s.feed(long.as_bytes());
    assert_eq!(s.title().len(), 1024);

    assert!(!s.take_bell());
    s.feed(b"\x07");
    assert!(s.take_bell() && !s.take_bell());

    assert_eq!(
        (s.cursor().shape, s.cursor().blinking),
        (CursorShape::Block, true)
    );
    for (n, shape, blinking) in [
        (1, CursorShape::Block, true),
        (2, CursorShape::Block, false),
        (3, CursorShape::Underline, true),
        (4, CursorShape::Underline, false),
        (5, CursorShape::Bar, true),
        (6, CursorShape::Bar, false),
        (0, CursorShape::Block, true),
    ] {
        s.feed(format!("\x1b[{n} q").as_bytes());
        assert_eq!(
            (s.cursor().shape, s.cursor().blinking),
            (shape, blinking),
            "{n}"
        );
    }
}

#[test]
fn a_full_reset_is_a_new_terminal_whose_lines_go_on_counting() {
    let mut s = Screen::new(5, 2, 10);
    s.feed(b"1\r\n2\r\n3\r\n4\x1b]0;t\x07\x1b[?1049h\x1b[?2004h\x1b[31m\x1b[5n");
    assert_eq!(s.screen_start(), 2);
    s.take_title();
    s.feed(b"\x1bc");
    assert_eq!(grid(&s), "\n");
    assert_eq!(s.modes(), Modes::default());
    assert_eq!(
        (s.scrollback_len(), s.first_line(), s.screen_start()),
        (0, 2, 2)
    );
    assert_eq!(at(&s), (0, 0));
    // The title is gone, and that is a change; the reply that was owed is still owed.
    assert_eq!(s.take_title().as_deref(), Some(""));
    assert_eq!(s.take_reply(), b"\x1b[0n");
    // It keeps working, with its scrollback, and what followed the reset in the same read.
    s.feed(b"a\r\nb\r\nc\x1bcafter");
    assert_eq!(grid(&s), "after\n");
    assert_eq!(s.cell(0, 0).fg, Color::Default);
}

#[test]
fn the_rows_that_changed_are_reported_once() {
    let mut s = Screen::new(5, 4, 10);
    assert_eq!(s.take_dirty(), [0..4]);
    assert!(s.take_dirty().is_empty());
    s.feed(b"x");
    assert_eq!(s.take_dirty(), [0..1]);
    // Written elsewhere: that row, and the row the cursor left.
    s.feed(b"\x1b[3;1Hy");
    assert_eq!(s.take_dirty(), [0..1, 2..3]);
    // The cursor alone moved: the row it left and the one it reached.
    s.feed(b"\x1b[1;1H");
    assert_eq!(s.take_dirty(), [0..1, 2..3]);
    // Moved within its row, hidden, reshaped: its row.
    s.feed(b"\x1b[C");
    assert_eq!(s.take_dirty(), [0..1]);
    s.feed(b"\x1b[?25l");
    assert_eq!(s.take_dirty(), [0..1]);
    s.feed(b"\x1b[4 q");
    assert_eq!(s.take_dirty(), [0..1]);
    // Nothing happened: nothing.
    s.feed(b"\x1b[m\x1b[1;2H\x1b]0;t\x07");
    assert!(s.take_dirty().is_empty());
    // An erase of two rows, and a scroll of a region.
    s.feed(b"\x1b[3;1H\x1b[J");
    assert_eq!(s.take_dirty(), [0..1, 2..4]);
    s.feed(b"\x1b[2;3r\x1b[2;1H\x1bM");
    assert_eq!(s.take_dirty(), [1..3]);
    // A scroll of the screen, a switch of screens and a resize change every row.
    s.feed(b"\x1b[r\x1b[4;1H\n");
    assert_eq!(s.take_dirty(), [0..4]);
    s.feed(b"\x1b[?1049h");
    assert_eq!(s.take_dirty(), [0..4]);
    s.resize(5, 3);
    assert_eq!(s.take_dirty(), [0..3]);
    s.mark_all_dirty();
    assert_eq!(s.take_dirty(), [0..3]);
}

/// A stream with a bit of everything, for the tests that feed it in different ways.
const MIXED: &str = "plain text that wraps over the edge of a narrow screen\r\n\
    \x1b[1;31mred\x1b[0m \x1b[38;2;10;20;30mrgb\x1b[m \x1b[48:5:99midx\x1b[m\r\n\
    wide 漢字 and é and 😀 \u{fffd}\r\n\
    \x1b]0;a title\x1b\\\x1b[?1049h\x1b[2J\x1b[Halt screen\x1b[?1049l\
    \x1b(0lqqk\x1b(B\r\n\x1b[5;3r\x1b[2;4r\x1b[4;1H\n\n\x1b[r\
    \x1b[6n\x1b[c\x1b[3;3H\x1b[2@\x1b[P\x1b[4h!\x1b[4l\x1b7\x1b[H\x1b8\ttab\x1bPignored\x1b\\ end";

#[test]
fn bytes_fed_in_any_pieces_give_the_same_screen() {
    let whole = fed(20, 6, MIXED);
    check(&whole);
    for piece in [1, 2, 3, 5, 7, 11] {
        let mut s = Screen::new(20, 6, 100);
        for chunk in MIXED.as_bytes().chunks(piece) {
            s.feed(chunk);
        }
        assert_eq!(grid(&s), grid(&whole), "pieces of {piece}");
        for r in 0..6 {
            assert_eq!(s.line(r), whole.line(r), "pieces of {piece}, row {r}");
        }
        assert_eq!(s.scrollback_len(), whole.scrollback_len());
        for i in 0..s.scrollback_len() {
            assert_eq!(s.scrollback_line(i), whole.scrollback_line(i));
        }
        assert_eq!(s.cursor(), whole.cursor());
        assert_eq!(s.modes(), whole.modes());
        assert_eq!(s.title(), whole.title());
        assert_eq!(s.take_reply(), whole.clone().take_reply());
    }
    assert_eq!(whole.title(), "a title");
    assert!(!whole.clone().take_reply().is_empty());
}

#[test]
fn a_resize_cuts_and_pads_and_keeps_the_cursors_line() {
    // Narrower cuts; wider again does not bring the cells back.
    let mut s = fed(6, 2, "abcdef\r\nxy");
    s.resize(3, 2);
    assert_eq!(grid(&s), "abc\nxy");
    assert_eq!(at(&s), (1, 2));
    s.resize(6, 2);
    assert_eq!(grid(&s), "abc\nxy");
    check(&s);
    // A wide character cut in two goes altogether.
    let mut s = fed(6, 2, "ab漢");
    s.resize(3, 2);
    assert_eq!(grid(&s), "ab\n");
    check(&s);

    // Fewer rows with the cursor at the bottom: the top rows become history.
    let mut s = fed(5, 4, "1\r\n2\r\n3\r\n4");
    s.resize(5, 2);
    assert_eq!(grid(&s), "3\n4");
    assert_eq!((s.scrollback_len(), at(&s)), (2, (1, 1)));
    assert_eq!(s.line_at(0).unwrap().text(), "1");
    assert_eq!(s.line_at(3).unwrap().text(), "4");
    // More rows come empty at the bottom; history stays history.
    s.resize(5, 4);
    assert_eq!(grid(&s), "3\n4\n\n");
    assert_eq!(s.scrollback_len(), 2);
    // Fewer rows with the cursor near the top: the empty rows below it go first.
    let mut s = fed(5, 4, "1\r\n2");
    s.resize(5, 2);
    assert_eq!(grid(&s), "1\n2");
    assert_eq!((s.scrollback_len(), at(&s)), (0, (1, 1)));
    // With the cursor in the middle: below first, then the top.
    let mut s = fed(5, 4, "1\r\n2\r\n3\r\n4\x1b[2;1H");
    s.resize(5, 1);
    assert_eq!(grid(&s), "2");
    assert_eq!((s.scrollback_len(), at(&s)), (1, (0, 0)));

    // The main screen is resized behind the alternate one, keeping its latest lines.
    let mut s = fed(5, 4, "1\r\n2\r\n3\r\n4\x1b[?1049h");
    s.resize(7, 2);
    assert_eq!(s.scrollback_len(), 2);
    s.feed(b"alt\x1b[?1049l");
    assert_eq!(grid(&s), "3\n4");
    assert_eq!(s.line(0).unwrap().len(), 7);
    check(&s);
    // The alternate screen's own lost rows are not history.
    let mut s = fed(5, 4, "\x1b[?1049h1\r\n2\r\n3\r\n4");
    s.resize(5, 2);
    assert_eq!((grid(&s), s.scrollback_len()), ("3\n4".to_string(), 0));

    // The region is the whole screen again, tabs reach the new columns, sizes are bounded.
    let mut s = fed(10, 4, "\x1b[2;3r");
    s.resize(20, 3);
    s.feed(b"\x1b[3;1H\n\ta\tb");
    assert_eq!(s.row_text(2), "        a       b");
    assert_eq!(s.scrollback_len(), 1);
    s.resize(0, 0);
    assert_eq!((s.cols(), s.rows()), (1, 1));
    s.feed("x漢\r\n\ty\x1b[5;5H\x1b[2@\x1b[P\x1b[L".as_bytes());
    check(&s);
    s.resize(1, 1);
    let s = Screen::new(0, 0, 0);
    assert_eq!((s.cols(), s.rows()), (1, 1));
    let s = Screen::new(usize::MAX, 1, 0);
    assert_eq!(s.cols(), MAX_SIDE);
}

#[test]
fn garbage_never_panics_and_never_leaves_the_grid_broken() {
    // A small generator with a fixed seed: the same garbage every run.
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let finals = b"@ABCDEFGHIJKLMPSTXZ`abcdefghlmnpqrsu";
    let texts = ["a", "漢", "é", "😀", "\u{301}", " ", "~", "q"];
    let mut s = Screen::new(13, 7, 5);
    let mut bytes = Vec::new();
    for round in 0..60_000 {
        bytes.clear();
        let r = next();
        match r % 10 {
            // A control sequence with plausible parameters.
            0..=3 => {
                bytes.extend_from_slice(b"\x1b[");
                if r & 0x100 != 0 {
                    bytes.push(b'?');
                }
                for i in 0..(r >> 9) % 4 {
                    if i > 0 {
                        bytes.push(if r & 0x200 != 0 { b';' } else { b':' });
                    }
                    let v = [0, 1, 2, 3, 5, 6, 7, 13, 14, 25, 38, 47, 48, 1049, 65535]
                        [(next() % 15) as usize];
                    bytes.extend_from_slice(v.to_string().as_bytes());
                }
                bytes.push(finals[(next() % finals.len() as u64) as usize]);
            }
            // An escape sequence, a control, or a string.
            4 => bytes.extend_from_slice(&[0x1b, b"78DEHMc=>#()]P"[(next() % 14) as usize]]),
            5 => bytes.push(b"\x07\x08\t\n\r\x0e\x0f\x18\x1b"[(next() % 9) as usize]),
            6 => bytes.extend_from_slice(texts[(next() % 8) as usize].as_bytes()),
            7 => bytes.extend_from_slice(b"some words that wrap"),
            // Raw bytes.
            _ => bytes.extend((0..r % 5).map(|_| (next() >> 11) as u8)),
        }
        s.feed(&bytes);
        if round % 997 == 0 {
            s.resize(1 + (next() % 30) as usize, 1 + (next() % 12) as usize);
        }
        check(&s);
        assert!(s.scrollback_len() <= 5);
        assert!(s.first_line() <= s.screen_start());
    }
    // Whatever state it ended in, the readers still work.
    let _ = (s.take_dirty(), s.take_reply(), s.take_title(), grid(&s));
}

/// A session as a person has one, written apart from the tests above as a check on them: a
/// shell prompt and a listing in colour, a full-screen program on the alternate screen, the
/// shell's screen given back as it was, a progress line rewritten in place, and a tab.
#[test]
fn a_shell_session_a_full_screen_program_and_back() {
    let mut s = fed(20, 5, "$ ls\r\n\x1b[1;34mdir\x1b[0m  file.txt\r\n$ ");
    assert_eq!(grid(&s), "$ ls\ndir  file.txt\n$\n\n");
    assert_eq!(at(&s), (2, 2));
    let d = s.cell(1, 0);
    assert_eq!((d.ch, d.fg), ('d', Color::Indexed(4)));
    assert!(d.attrs.contains(Attrs::BOLD));
    let f = s.cell(1, 5);
    assert_eq!((f.ch, f.fg), ('f', Color::Default));
    assert!(f.attrs.is_empty());

    s.feed(b"\x1b[?1049h\x1b[2J\x1b[Hhello\x1b[5;1H-- INSERT --");
    assert!(s.modes().alt_screen);
    assert_eq!(grid(&s), "hello\n\n\n\n-- INSERT --");
    assert_eq!(
        s.scrollback_len(),
        0,
        "the alternate screen feeds no scrollback"
    );

    s.feed(b"\x1b[?1049l");
    assert!(!s.modes().alt_screen);
    assert_eq!(grid(&s), "$ ls\ndir  file.txt\n$\n\n");
    assert_eq!(at(&s), (2, 2));

    s.feed(b"10%\r100%\x1b[K");
    assert_eq!(s.row_text(2), "100%");
    s.feed(b"\r\na\tb");
    assert_eq!(s.row_text(3), "a       b");
    assert_eq!(at(&s), (3, 9));
    // Three more lines: the first two rows go to the scrollback, in order.
    s.feed(b"\r\none\r\ntwo\r\nthree");
    assert_eq!(grid(&s), "100%\na       b\none\ntwo\nthree");
    assert_eq!(s.scrollback_len(), 2);
    assert_eq!(s.scrollback_line(0).unwrap().text(), "$ ls");
    assert_eq!(s.scrollback_line(1).unwrap().text(), "dir  file.txt");
    check(&s);
}
