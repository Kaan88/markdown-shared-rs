//! Pixel tests of the grid's drawing, the sheet and the speed measurement.

#![allow(clippy::single_range_in_vec_init)]

use super::*;
use crate::anim::{Animator, Clock, ManualClock, Policy};
use crate::gfx::{Gfx, Painter, Rect, Rgb};
use crate::theme::{DARK, LIGHT, Palette};
use mdshared_term::{Point, Screen, Selection, SelectionKind};
use mdshared_text::Bitmap;
use std::time::Duration;

const CW: f32 = 9.0;
const CH: f32 = 18.0;

fn metrics(scale: f32) -> Metrics {
    Metrics::fixed(14.0, scale, CW, CH)
}

fn px(b: &Bitmap, x: i32, y: i32) -> Rgb {
    let o = (y as usize * b.width as usize + x as usize) * 4;
    [b.pixels[o + 2], b.pixels[o + 1], b.pixels[o]]
}

/// A screen of `cols` by `rows` with `bytes` fed in.
fn screen(cols: usize, rows: usize, bytes: &[u8]) -> Screen {
    let mut s = Screen::new(cols, rows, 100);
    s.feed(bytes);
    s
}

struct Frame {
    bmp: Bitmap,
    gfx: Gfx,
    font: CellFont,
}

impl Frame {
    fn new(w: u32, h: u32) -> Frame {
        Frame {
            bmp: Bitmap::new(w, h),
            gfx: Gfx::new(),
            font: CellFont::new(),
        }
    }

    /// Draws all of `s` at `area` with the scheme of `pal`, the bitmap cleared to a colour no
    /// scheme has first, so anything left undrawn shows.
    #[allow(clippy::too_many_arguments)]
    fn full(
        &mut self,
        pal: &Palette,
        scale: f32,
        area: Rect,
        s: &Screen,
        sel: Option<&Selection>,
        view: View,
    ) {
        self.bmp.clear([1, 2, 3]);
        let mut p = Painter::new(&mut self.bmp, &mut self.gfx, scale);
        draw_screen(
            &mut p,
            &mut self.font,
            &Scheme::from_palette(pal),
            &metrics(scale),
            area,
            s,
            sel,
            view,
        );
    }
}

fn area(cols: usize, rows: usize) -> Rect {
    Rect::new(0.0, 0.0, cols as f32 * CW, rows as f32 * CH)
}

/// The middle pixel of cell `row, col` at scale 1 from the top left corner.
fn mid(row: usize, col: usize) -> (i32, i32) {
    (
        (col as f32 * CW + CW / 2.0) as i32,
        (row as f32 * CH + 3.0) as i32,
    )
}

#[test]
fn backgrounds_take_the_cells_colours_and_the_padding_the_schemes_in_both_palettes() {
    for pal in [DARK, LIGHT] {
        let scheme = Scheme::from_palette(&pal);
        let s = screen(
            10,
            3,
            b"\x1b[41m  \x1b[0m  \x1b[48;5;21m \x1b[48;2;10;200;30m \x1b[0m\r\n\x1b[7m \x1b[0m",
        );
        let mut f = Frame::new(120, 80);
        f.full(
            &pal,
            1.0,
            Rect::new(0.0, 0.0, 110.0, 70.0),
            &s,
            None,
            View::new().focused(false),
        );
        let at = |row, col| {
            let (x, y) = mid(row, col);
            px(&f.bmp, x, y)
        };
        assert_eq!(at(0, 0), scheme.ansi[1]);
        assert_eq!(at(0, 1), scheme.ansi[1]);
        assert_eq!(at(0, 2), scheme.background);
        assert_eq!(at(0, 4), [0, 0, 255]);
        assert_eq!(at(0, 5), [10, 200, 30]);
        // Inverse: the default foreground as the background.
        assert_eq!(at(1, 0), scheme.foreground);
        // The padding at the right and below the grid, inside the area; nothing outside it.
        assert_eq!(px(&f.bmp, 105, 5), scheme.background);
        assert_eq!(px(&f.bmp, 5, 65), scheme.background);
        assert_eq!(px(&f.bmp, 115, 5), [1, 2, 3]);
        assert_eq!(px(&f.bmp, 5, 75), [1, 2, 3]);
    }
}

#[test]
fn text_is_drawn_in_its_colour_and_a_hidden_cell_shows_only_its_background() {
    for pal in [DARK, LIGHT] {
        let scheme = Scheme::from_palette(&pal);
        let s = screen(6, 2, b"\x1b[31mM\x1b[0mM\x1b[8mM\x1b[0m");
        let mut f = Frame::new(54, 36);
        f.full(&pal, 1.0, area(6, 2), &s, None, View::new().focused(false));
        let cell = |col: i32| {
            let mut px_in = Vec::new();
            for y in 0..18 {
                for x in col * 9..col * 9 + 9 {
                    px_in.push(px(&f.bmp, x, y));
                }
            }
            px_in
        };
        // The red M has pixels near the scheme's red; the plain one near the foreground.
        let near = |a: Rgb, b: Rgb| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) < 60);
        assert!(cell(0).iter().any(|&c| near(c, scheme.ansi[1])), "red M");
        assert!(
            cell(1).iter().any(|&c| near(c, scheme.foreground)),
            "plain M"
        );
        assert!(cell(0).iter().filter(|&&c| c != scheme.background).count() > 10);
        assert!(cell(2).iter().all(|&c| c == scheme.background), "hidden M");
        // Column 3 holds the hollow cursor; the blank after it is background.
        assert!(cell(4).iter().all(|&c| c == scheme.background));
    }
}

#[test]
fn bold_and_italic_change_the_glyph_and_underline_and_strike_draw_lines() {
    let pal = DARK;
    let scheme = Scheme::from_palette(&pal);
    let s = screen(
        8,
        1,
        b"a\x1b[1ma\x1b[0m\x1b[3ma\x1b[0m\x1b[4m \x1b[0m\x1b[9m \x1b[0m",
    );
    let mut f = Frame::new(72, 18);
    f.full(&pal, 1.0, area(8, 1), &s, None, View::new().focused(false));
    let cell = |col: i32| {
        (0..18)
            .flat_map(|y| (col * 9..col * 9 + 9).map(move |x| (x, y)))
            .map(|(x, y)| px(&f.bmp, x, y))
            .collect::<Vec<_>>()
    };
    assert_ne!(cell(0), cell(1), "bold");
    assert_ne!(cell(0), cell(2), "italic");
    let m = metrics(1.0);
    let row_of =
        |col: i32, y: i32| (col * 9..col * 9 + 9).all(|x| px(&f.bmp, x, y) == scheme.foreground);
    assert!(row_of(3, m.underline_y()), "underline");
    assert!(row_of(4, m.strike_y()), "strike");
    assert!(!row_of(3, m.strike_y()));
}

#[test]
fn a_wide_character_takes_two_cells_and_its_selection_both() {
    let pal = LIGHT;
    let scheme = Scheme::from_palette(&pal);
    let s = screen(6, 1, "日x".as_bytes());
    assert_eq!(s.cell(0, 0).width, 2);
    let mut f = Frame::new(54, 18);
    f.full(&pal, 1.0, area(6, 1), &s, None, View::new().focused(false));
    let inked =
        |x0: i32| (0..18).any(|y| (x0..x0 + 9).any(|x| px(&f.bmp, x, y) != scheme.background));
    assert!(inked(0) && inked(9), "the glyph spans both cells");
    // A selection dragged from the x back onto the second half covers both halves.
    let line = s.screen_start();
    let mut sel = Selection::start(&s, Point::new(line, 2), SelectionKind::Stream, 1);
    sel.extend(&s, Point::new(line, 1));
    f.full(
        &pal,
        1.0,
        area(6, 1),
        &s,
        Some(&sel),
        View::new().focused(false),
    );
    assert_eq!(px(&f.bmp, 1, 1), scheme.selection);
    assert_eq!(px(&f.bmp, 16, 1), scheme.selection);
}

#[test]
fn box_lines_run_unbroken_across_cells() {
    for scale in [1.0, 1.5] {
        let pal = DARK;
        let scheme = Scheme::from_palette(&pal);
        let s = screen(8, 3, "┌──────┐\r\n│      │\r\n╞══════╡".as_bytes());
        let m = metrics(scale);
        let (cw, ch) = m.cell_device();
        let mut f = Frame::new((cw * 8) as u32, (ch * 3) as u32);
        let a = Rect::new(0.0, 0.0, 8.0 * CW, 3.0 * CH);
        f.full(&pal, scale, a, &s, None, View::new().focused(false));
        // Some pixel row of the first text row is the foreground from the first cell's middle
        // to the last's: the top of the frame.
        let full_row = (0..ch)
            .any(|y| (cw / 2..cw * 8 - cw / 2).all(|x| px(&f.bmp, x, y) == scheme.foreground));
        assert!(full_row, "the top edge at {scale}");
        // And some pixel column runs from the top row's middle to the bottom row's: the side.
        let full_col = (0..cw)
            .any(|x| (ch / 2..ch * 3 - ch / 2).all(|y| px(&f.bmp, x, y) == scheme.foreground));
        assert!(full_col, "the left edge at {scale}");
        // The double line has two such rows in the bottom text row.
        let rows = (ch * 2..ch * 3)
            .filter(|&y| (cw..cw * 7).all(|x| px(&f.bmp, x, y) == scheme.foreground))
            .count();
        assert!(rows >= 2, "{rows} rows of the double line at {scale}");
    }
}

#[test]
fn block_elements_fill_their_part_of_the_cell() {
    let pal = DARK;
    let scheme = Scheme::from_palette(&pal);
    let s = screen(4, 1, "█▀▐▒".as_bytes());
    let mut f = Frame::new(36, 18);
    f.full(&pal, 1.0, area(4, 1), &s, None, View::new().focused(false));
    assert!((0..18).all(|y| (0..9).all(|x| px(&f.bmp, x, y) == scheme.foreground)));
    assert_eq!(px(&f.bmp, 13, 2), scheme.foreground);
    assert_eq!(px(&f.bmp, 13, 16), scheme.background);
    assert_eq!(px(&f.bmp, 19, 9), scheme.background);
    assert_eq!(px(&f.bmp, 25, 9), scheme.foreground);
    let shade = px(&f.bmp, 31, 9);
    assert!(shade != scheme.foreground && shade != scheme.background);
}

#[test]
fn the_cursor_takes_the_shape_the_program_asked_for_and_is_hollow_without_the_focus() {
    for pal in [DARK, LIGHT] {
        let scheme = Scheme::from_palette(&pal);
        let m = metrics(1.0);
        let t = m.cursor_thickness();
        let mut f = Frame::new(36, 18);
        // Block (the default), focused: the cell is filled.
        let s = screen(4, 1, b"");
        f.full(&pal, 1.0, area(4, 1), &s, None, View::new());
        assert_eq!(px(&f.bmp, 4, 9), scheme.cursor);
        assert_eq!(px(&f.bmp, 13, 9), scheme.background);
        // Without the focus: the outline only.
        f.full(&pal, 1.0, area(4, 1), &s, None, View::new().focused(false));
        assert_eq!(px(&f.bmp, 0, 0), scheme.cursor);
        assert_eq!(px(&f.bmp, 8, 17), scheme.cursor);
        assert_eq!(px(&f.bmp, 4, 9), scheme.background);
        // A bar (DECSCUSR 6): the left edge of the cell.
        let s = screen(4, 1, b"\x1b[6 q");
        f.full(&pal, 1.0, area(4, 1), &s, None, View::new());
        assert_eq!(px(&f.bmp, 0, 9), scheme.cursor);
        assert_eq!(px(&f.bmp, t - 1, 9), scheme.cursor);
        assert_eq!(px(&f.bmp, t + 1, 9), scheme.background);
        // An underline (DECSCUSR 4): the bottom of the cell.
        let s = screen(4, 1, b"\x1b[4 q");
        f.full(&pal, 1.0, area(4, 1), &s, None, View::new());
        assert_eq!(px(&f.bmp, 4, 17), scheme.cursor);
        assert_eq!(px(&f.bmp, 4, 17 - t), scheme.background);
        // Hidden by the program (DECTCEM off), or by the view: nothing.
        let s = screen(4, 1, b"\x1b[?25l");
        f.full(&pal, 1.0, area(4, 1), &s, None, View::new());
        assert_eq!(px(&f.bmp, 4, 9), scheme.background);
        let s = screen(4, 1, b"");
        f.full(
            &pal,
            1.0,
            area(4, 1),
            &s,
            None,
            View::new().cursor_shown(false),
        );
        assert_eq!(px(&f.bmp, 4, 9), scheme.background);
    }
}

#[test]
fn a_block_cursor_over_a_character_draws_it_in_the_cursor_text_colour() {
    let pal = DARK;
    let scheme = Scheme::from_palette(&pal);
    let s = screen(4, 1, b"M\x1b[D");
    assert_eq!(s.cursor().col, 0);
    let mut f = Frame::new(36, 18);
    f.full(&pal, 1.0, area(4, 1), &s, None, View::new());
    let cell: Vec<Rgb> = (0..18)
        .flat_map(|y| (0..9).map(move |x| (x, y)))
        .map(|(x, y)| px(&f.bmp, x, y))
        .collect();
    assert!(cell.contains(&scheme.cursor));
    let near = |a: Rgb, b: Rgb| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) < 60);
    assert!(cell.iter().any(|&c| near(c, scheme.cursor_text)));
}

#[test]
fn a_selection_paints_its_cells_in_the_selection_colour() {
    for pal in [DARK, LIGHT] {
        let scheme = Scheme::from_palette(&pal);
        let s = screen(10, 2, b"hello\r\nsecond");
        let line = s.screen_start();
        let mut sel = Selection::start(&s, Point::new(line, 2), SelectionKind::Stream, 1);
        sel.extend(&s, Point::new(line, 4));
        let mut f = Frame::new(90, 36);
        f.full(
            &pal,
            1.0,
            area(10, 2),
            &s,
            Some(&sel),
            View::new().focused(false),
        );
        assert_eq!(s.row_text(0), "hello");
        assert_eq!(px(&f.bmp, mid(0, 3).0, 1), scheme.selection);
        assert_eq!(px(&f.bmp, mid(0, 5).0, 1), scheme.background);
        assert_eq!(px(&f.bmp, mid(0, 1).0, 1), scheme.background);
        // Selected text in a colour of its own, when the scheme has one.
        let own = Scheme {
            selection_text: Some([255, 0, 255]),
            ..scheme
        };
        f.bmp.clear([0, 0, 0]);
        let mut p = Painter::new(&mut f.bmp, &mut f.gfx, 1.0);
        let v = View::new().focused(false);
        draw_screen(
            &mut p,
            &mut f.font,
            &own,
            &metrics(1.0),
            area(10, 2),
            &s,
            Some(&sel),
            v,
        );
        let near = |a: Rgb| a[0] as i32 > a[1] as i32 + 70 && a[2] as i32 > a[1] as i32 + 50;
        let inked = (0..18).any(|y| (18..27).any(|x| near(px(&f.bmp, x, y))));
        assert!(inked, "the selected l in the selection's text colour");
    }
}

#[test]
fn rows_drawn_alone_over_the_last_frame_are_the_whole_new_frame() {
    for pal in [DARK, LIGHT] {
        for scale in [1.0, 1.5] {
            let mut s = screen(
                20,
                6,
                b"\x1b[1;33mbold yellow\x1b[0m\r\n\x1b[44mblue\x1b[0m ok\r\n$ ",
            );
            s.take_dirty();
            let m = metrics(scale);
            let (cw, ch) = m.cell_device();
            let a = Rect::new(0.0, 0.0, 20.0 * CW + 3.0, 6.0 * CH + 5.0);
            let (w, h) = ((cw * 20 + 10) as u32, (ch * 6 + 10) as u32);
            let mut old = Frame::new(w, h);
            old.full(&pal, scale, a, &s, None, View::new());
            s.feed("ls ─┼─ 日本\r\n\x1b[31mred\x1b[0m".as_bytes());
            let dirty = s.take_dirty();
            let rows = View::new().damage(&s, &dirty);
            assert!(!rows.is_empty());
            {
                let mut p = Painter::new(&mut old.bmp, &mut old.gfx, scale);
                let scheme = Scheme::from_palette(&pal);
                draw_rows(
                    &mut p,
                    &mut old.font,
                    &scheme,
                    &m,
                    a,
                    &s,
                    None,
                    View::new(),
                    &rows,
                );
            }
            let mut new = Frame::new(w, h);
            new.full(&pal, scale, a, &s, None, View::new());
            assert!(
                old.bmp.pixels == new.bmp.pixels,
                "scale {scale}, dark {}",
                pal.dark
            );
        }
    }
}

#[test]
fn a_row_drawn_alone_touches_nothing_outside_its_band() {
    let pal = DARK;
    let s = screen(10, 4, b"one\r\ntwo\r\nthree");
    let m = metrics(1.0);
    let mut f = Frame::new(100, 80);
    f.bmp.clear([1, 2, 3]);
    let a = Rect::new(0.0, 0.0, 95.0, 72.0);
    {
        let mut p = Painter::new(&mut f.bmp, &mut f.gfx, 1.0);
        draw_rows(
            &mut p,
            &mut f.font,
            &Scheme::from_palette(&pal),
            &m,
            a,
            &s,
            None,
            View::new(),
            &[1..2],
        );
    }
    let band = m.row_rect(a, 1);
    for y in 0..80 {
        for x in 0..100 {
            let inside = band.contains(x as f32 + 0.5, y as f32 + 0.5);
            assert_eq!(px(&f.bmp, x, y) != [1, 2, 3], inside, "{x},{y}");
        }
    }
}

#[test]
fn the_view_scrolled_back_shows_the_scrollback_as_the_screen_showed_it() {
    let pal = LIGHT;
    let mut s = Screen::new(12, 3, 100);
    s.feed(b"\x1b[32mfirst\x1b[0m\r\nsecond\r\nthird\r\nfourth\r\nfifth");
    assert_eq!(s.scrollback_len(), 2);
    // Two lines back the view shows first, second and third.
    let mut back = Frame::new(108, 54);
    back.full(
        &pal,
        1.0,
        area(12, 3),
        &s,
        None,
        View::new().scrolled(2).focused(false),
    );
    let fresh = screen(12, 3, b"\x1b[32mfirst\x1b[0m\r\nsecond\r\nthird");
    let mut want = Frame::new(108, 54);
    want.full(
        &pal,
        1.0,
        area(12, 3),
        &fresh,
        None,
        View::new().cursor_shown(false),
    );
    assert!(back.bmp.pixels == want.bmp.pixels);
}

#[test]
fn the_blinking_cursor_at_rest_is_the_plain_one_and_off_is_no_cursor() {
    let half = Duration::from_millis(500);
    let pal = DARK;
    let s = screen(6, 2, b"ab");
    let a = area(6, 2);
    let mut plain = Frame::new(54, 36);
    plain.full(&pal, 1.0, a, &s, None, View::new());
    let mut hidden = Frame::new(54, 36);
    hidden.full(&pal, 1.0, a, &s, None, View::new().cursor_shown(false));
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let mut f = Frame::new(54, 36);
    let frame = |anim: &mut Animator, f: &mut Frame, view: View, blink: Option<Duration>| {
        anim.begin_frame(clock.now());
        f.bmp.clear([1, 2, 3]);
        let mut p = Painter::new(&mut f.bmp, &mut f.gfx, 1.0);
        let scheme = Scheme::from_palette(&pal);
        draw_screen_animated(
            &mut p,
            &mut f.font,
            &scheme,
            &metrics(1.0),
            a,
            &s,
            None,
            view,
            anim,
            "cursor",
            blink,
        );
        anim.end_frame()
    };
    // On at first, with a deadline at the change.
    let deadline = frame(&mut anim, &mut f, View::new(), Some(half));
    assert!(f.bmp.pixels == plain.bmp.pixels);
    assert_eq!(deadline, Some(clock.now() + half));
    clock.advance(half + Duration::from_millis(1));
    frame(&mut anim, &mut f, View::new(), Some(half));
    assert!(f.bmp.pixels == hidden.bmp.pixels, "off half");
    clock.advance(half);
    frame(&mut anim, &mut f, View::new(), Some(half));
    assert!(f.bmp.pixels == plain.bmp.pixels, "on again");
    // Blinking off, a steady cursor asked by the program, and reduced motion: the plain
    // drawing, and no frame asked for.
    assert_eq!(frame(&mut anim, &mut f, View::new(), None), None);
    assert!(f.bmp.pixels == plain.bmp.pixels);
    let steady = screen(6, 2, b"ab\x1b[2 q");
    assert!(!steady.cursor().blinking);
    anim.begin_frame(clock.now());
    assert!(cursor_blinks_on(
        &mut anim,
        "c2",
        Some(half),
        &steady,
        View::new()
    ));
    assert_eq!(anim.end_frame(), None);
    anim.set_policy(Policy::Never);
    assert_eq!(frame(&mut anim, &mut f, View::new(), Some(half)), None);
    assert!(f.bmp.pixels == plain.bmp.pixels);
    anim.set_policy(Policy::Always);
    // Without the focus: the hollow cursor, steady, and the window idle.
    let mut unfocused = Frame::new(54, 36);
    unfocused.full(&pal, 1.0, a, &s, None, View::new().focused(false));
    assert_eq!(
        frame(&mut anim, &mut f, View::new().focused(false), Some(half)),
        None
    );
    assert!(f.bmp.pixels == unfocused.bmp.pixels);
}

#[test]
fn the_animated_rows_are_the_plain_rows_at_rest() {
    let pal = LIGHT;
    let s = screen(6, 3, b"ab\r\ncd");
    let a = area(6, 3);
    let rows = [1..2];
    let mut plain = Frame::new(54, 54);
    plain.bmp.clear([1, 2, 3]);
    {
        let mut p = Painter::new(&mut plain.bmp, &mut plain.gfx, 1.0);
        let scheme = Scheme::from_palette(&pal);
        draw_rows(
            &mut p,
            &mut plain.font,
            &scheme,
            &metrics(1.0),
            a,
            &s,
            None,
            View::new(),
            &rows,
        );
    }
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    anim.begin_frame(clock.now());
    let mut f = Frame::new(54, 54);
    f.bmp.clear([1, 2, 3]);
    {
        let mut p = Painter::new(&mut f.bmp, &mut f.gfx, 1.0);
        let scheme = Scheme::from_palette(&pal);
        let blink = Some(Duration::from_millis(500));
        draw_rows_animated(
            &mut p,
            &mut f.font,
            &scheme,
            &metrics(1.0),
            a,
            &s,
            None,
            View::new(),
            &rows,
            &mut anim,
            "c",
            blink,
        );
    }
    assert!(
        anim.end_frame().is_some(),
        "the blink goes on while other rows are drawn"
    );
    assert!(f.bmp.pixels == plain.bmp.pixels);
}

#[test]
fn a_painter_offset_moves_the_grid_by_whole_pixels() {
    let pal = DARK;
    let s = screen(6, 2, "ab─┼".as_bytes());
    let mut at = Frame::new(80, 60);
    at.full(
        &pal,
        1.0,
        Rect::new(10.0, 7.0, 54.0, 36.0),
        &s,
        None,
        View::new(),
    );
    let mut moved = Frame::new(80, 60);
    moved.bmp.clear([1, 2, 3]);
    {
        let mut p = Painter::new(&mut moved.bmp, &mut moved.gfx, 1.0);
        p.offset(10.0, 7.0);
        let scheme = Scheme::from_palette(&pal);
        draw_screen(
            &mut p,
            &mut moved.font,
            &scheme,
            &metrics(1.0),
            area(6, 2),
            &s,
            None,
            View::new(),
        );
    }
    assert!(at.bmp.pixels == moved.bmp.pixels);
}

#[test]
fn nonsense_rectangles_and_rows_draw_nothing_and_do_not_panic() {
    let pal = DARK;
    let s = screen(5, 2, b"hi");
    let mut f = Frame::new(40, 40);
    for a in [
        Rect::new(0.0, 0.0, 0.0, 0.0),
        Rect::new(f32::NAN, f32::NAN, f32::NAN, f32::NAN),
        Rect::new(-1e9, -1e9, 1e9, 1e9),
        Rect::new(f32::INFINITY, 0.0, 10.0, 10.0),
        Rect::new(30.0, 30.0, 500.0, 500.0),
    ] {
        f.full(&pal, 1.0, a, &s, None, View::new().scrolled(usize::MAX));
        let mut p = Painter::new(&mut f.bmp, &mut f.gfx, 1.0);
        let scheme = Scheme::from_palette(&pal);
        let m = metrics(1.0);
        draw_rows(
            &mut p,
            &mut f.font,
            &scheme,
            &m,
            a,
            &s,
            None,
            View::new(),
            &[],
        );
        draw_rows(
            &mut p,
            &mut f.font,
            &scheme,
            &m,
            a,
            &s,
            None,
            View::new(),
            &[std::ops::Range { start: 5, end: 1 }, 1..usize::MAX],
        );
    }
    // An empty area draws nothing at all.
    f.bmp.clear([1, 2, 3]);
    let mut p = Painter::new(&mut f.bmp, &mut f.gfx, 1.0);
    let scheme = Scheme::from_palette(&pal);
    draw_screen(
        &mut p,
        &mut f.font,
        &scheme,
        &metrics(1.0),
        Rect::new(5.0, 5.0, 0.0, 0.0),
        &s,
        None,
        View::new(),
    );
    assert!(f.bmp.pixels.chunks(4).all(|c| c[..3] == [3, 2, 1]));
}

/// The sheet: a screen with colours, attributes, box drawing, wide characters, the cursor's
/// shapes and a selection, light and dark, at 100%, 125%, and 150%. `cargo test -p mdshared-ui
/// --features term termview_sheet -- --ignored`, then open the PNGs under `target/specimen`
/// (or set `TEXT_SHARPNESS_SHOTS` to write them elsewhere).
#[test]
#[ignore]
fn termview_sheet() {
    let text = concat!(
        "\x1b[1mColours\x1b[0m  ",
        "\x1b[30m\x1b[47m 0 \x1b[0m\x1b[31m 1 \x1b[32m 2 \x1b[33m 3 \x1b[34m 4 \x1b[35m 5 \x1b[36m 6 \x1b[37m 7 \x1b[0m",
        "\x1b[90m 8 \x1b[91m 9 \x1b[92m10 \x1b[93m11 \x1b[94m12 \x1b[95m13 \x1b[96m14 \x1b[97m15\x1b[0m\r\n",
        "\x1b[41m red \x1b[42m green \x1b[43m yellow \x1b[44m blue \x1b[45m magenta \x1b[46m cyan \x1b[0m ",
        "\x1b[48;5;208m 208 \x1b[48;5;244m 244 \x1b[48;2;120;40;200m rgb \x1b[0m\r\n",
        "plain \x1b[1mbold\x1b[0m \x1b[3mitalic\x1b[0m \x1b[1;3mbold italic\x1b[0m \x1b[4munderline\x1b[0m ",
        "\x1b[9mstrike\x1b[0m \x1b[2mdim\x1b[0m \x1b[7minverse\x1b[0m hid[\x1b[8mden\x1b[0m]\r\n",
        "Wide: 日本語のテキスト 한국어 中文 and emoji 😀 at the end\r\n",
        "┌──────┬──────┐ ╔══════╦══════╗ ┏━━━━━━┳━━━━━━┓ ╭──────╮ ┌┄┄┄┄┄┄┐\r\n",
        "│ cell │ cell │ ║ cell ║ cell ║ ┃ cell ┃ cell ┃ │ arcs │ ┆ dash ┆\r\n",
        "├──────┼──────┤ ╠══════╬══════╣ ┣━━━━━━╋━━━━━━┫ ╰──────╯ └┄┄┄┄┄┄┘\r\n",
        "│ cell │ cell │ ║ cell ║ cell ║ ┃ cell ┃ cell ┃ ╒══╤══╕ ╓──╥──╖ ╱╲╳\r\n",
        "└──────┴──────┘ ╚══════╩══════╝ ┗━━━━━━┻━━━━━━┛ ╘══╧══╛ ╙──╨──╜\r\n",
        "Blocks: █▉▊▋▌▍▎▏ ▁▂▃▄▅▆▇█ ▀▔▕▐ ░▒▓ ▖▗▘▙▚▛▜▝▞▟ ",
        "\x1b[31m████\x1b[32m▄▄▄▄\x1b[34m▀▀▀▀\x1b[0m\r\n",
        "Selection: the words here are selected across  \r\n",
        "two lines, then the cursor shapes below.\r\n",
        "block [ ]  bar [ ]  underline [ ]  hollow [ ]",
    );
    for (scale, file) in [
        (1.0, "termview.png"),
        (1.25, "termview-125.png"),
        (1.5, "termview-150.png"),
    ] {
        let mut font = CellFont::new();
        let m = font.metrics(14.0, scale);
        let (cols, rows) = (100usize, 13usize);
        let grid = Rect::new(0.0, 0.0, cols as f32 * m.cell_w(), rows as f32 * m.cell_h());
        let gap = 12.0;
        let w = grid.w + 2.0 * gap;
        let h = 2.0 * (grid.h + gap) + gap;
        let mut bmp = Bitmap::new((w * scale).ceil() as u32, (h * scale).ceil() as u32);
        bmp.clear([128, 128, 128]);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (i, pal) in [LIGHT, DARK].iter().enumerate() {
            let scheme = Scheme::from_palette(pal);
            let top = gap + i as f32 * (grid.h + gap);
            let area = Rect::new(gap, top, grid.w, grid.h);
            let mut s = Screen::new(cols, rows, 0);
            s.feed(text.as_bytes());
            let start = s.screen_start();
            let mut sel =
                Selection::start(&s, Point::new(start + 10, 11), SelectionKind::Stream, 1);
            sel.extend(&s, Point::new(start + 11, 8));
            draw_screen(
                &mut p,
                &mut font,
                &scheme,
                &m,
                area,
                &s,
                Some(&sel),
                View::new().cursor_shown(false),
            );
            // Each cursor shape in a little grid of its own laid over a "[ ]" of the last row:
            // block, bar and underline focused, then the hollow block of a view without focus.
            let last = text.rsplit("\r\n").next().unwrap_or("");
            let shapes = ["\x1b[2 q", "\x1b[6 q", "\x1b[4 q", "\x1b[2 q"];
            for (k, (col, _)) in last.match_indices("[ ]").enumerate().take(4) {
                let mut c = Screen::new(3, 1, 0);
                let under = if k == 0 { "x" } else { " " };
                c.feed(format!("[{under}]\x1b[1;2H{}", shapes[k]).as_bytes());
                let cell = m.cell_rect(area, rows - 1, col);
                let at = Rect::new(cell.x, cell.y, 3.0 * m.cell_w(), m.cell_h());
                draw_screen(
                    &mut p,
                    &mut font,
                    &scheme,
                    &m,
                    at,
                    &c,
                    None,
                    View::new().focused(k < 3),
                );
            }
        }
        let out = std::env::var_os("TEXT_SHARPNESS_SHOTS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen")
            });
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join(file)).unwrap();
    }
}

/// Not a correctness test: `cargo test --release -p mdshared-ui --features term termview_speed
/// -- --ignored --nocapture` prints the time of a whole 200 by 60 frame of text, warm.
#[test]
#[ignore]
fn termview_speed() {
    let (cols, rows) = (200usize, 60usize);
    let mut s = Screen::new(cols, rows, 0);
    let mut seed: u32 = 1;
    let mut bytes = Vec::new();
    for r in 0..rows {
        for c in 0..cols - 1 {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            if c % 16 == 0 {
                bytes.extend_from_slice(format!("\x1b[{}m", 31 + (seed >> 16) % 7).as_bytes());
            }
            bytes.push(b'!' + ((seed >> 8) % 90) as u8);
        }
        if r + 1 < rows {
            bytes.extend_from_slice(b"\r\n");
        }
    }
    s.feed(&bytes);
    let mut font = CellFont::new();
    let m = font.metrics(14.0, 1.0);
    let a = Rect::new(0.0, 0.0, cols as f32 * m.cell_w(), rows as f32 * m.cell_h());
    let mut bmp = Bitmap::new(a.w.ceil() as u32, a.h.ceil() as u32);
    let mut gfx = Gfx::new();
    let scheme = Scheme::from_palette(&DARK);
    let mut draw = |n: usize| {
        let t = std::time::Instant::now();
        for _ in 0..n {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            draw_screen(&mut p, &mut font, &scheme, &m, a, &s, None, View::new());
        }
        t.elapsed().as_secs_f64() / n as f64
    };
    draw(2);
    for _ in 0..3 {
        let t = draw(20);
        println!(
            "{cols}x{rows} grid: {:.2} ms a frame, {:.0} ns a cell",
            t * 1e3,
            t * 1e9 / (cols * rows) as f64
        );
    }
}
