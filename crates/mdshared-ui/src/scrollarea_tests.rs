use super::*;
use crate::anim::{Clock, ManualClock};
use crate::gfx::{Gfx, Rgb};
use crate::theme::{DARK, LIGHT, Palette as Colors};
use mdshared_text::Bitmap;

const VIEW: (f64, f64) = (400.0, 200.0);

fn area() -> ScrollArea {
    let mut a = ScrollArea::new();
    a.set_view(VIEW);
    a.set_content((1000.0, 2000.0));
    a
}

#[test]
fn content_shorter_than_the_view_does_not_scroll() {
    let mut a = ScrollArea::new();
    assert!(!a.set_view(VIEW));
    assert!(!a.set_content((80.0, 60.0)));
    assert_eq!(a.max(), (0.0, 0.0));
    assert_eq!(a.scrolls(), (false, false));
    assert_eq!(a.offset(), (0.0, 0.0));
    assert!(!a.scroll_by((30.0, 30.0)));
    assert!(!a.wheel(0.0, 120.0, false));
    assert!(!a.key(KeyName::PageDown, Mods::default()));
    assert_eq!(a.bars(Rect::new(0.0, 0.0, 400.0, 200.0)), (None, None));
    // The whole content is in view, and no more.
    assert_eq!(a.visible(), ((0.0, 0.0), (80.0, 60.0)));
}

#[test]
fn content_exactly_the_size_of_the_view_does_not_scroll_or_show_bars() {
    let mut a = ScrollArea::new();
    a.set_view(VIEW);
    a.set_content(VIEW);
    assert_eq!(a.max(), (0.0, 0.0));
    assert_eq!(a.scrolls(), (false, false));
    assert!(!a.set_offset((10.0, 10.0)));
    assert_eq!(a.offset(), (0.0, 0.0));
    assert_eq!(a.bars(Rect::new(0.0, 0.0, 400.0, 200.0)), (None, None));
    // A pixel over either way is a bar's worth of scroll.
    assert!(!a.set_content((400.0, 201.0)));
    assert_eq!(a.scrolls(), (false, true));
    assert!(!a.set_content((401.0, 201.0)));
    assert_eq!(a.scrolls(), (true, true));
}

#[test]
fn content_that_shrinks_pulls_the_offset_back_in() {
    let mut a = area();
    assert!(a.set_offset((900.0, 1700.0)));
    assert_eq!(a.offset(), (600.0, 1700.0));
    // The content is now 500 by 400: the furthest offsets are 100 and 200.
    assert!(a.set_content((500.0, 400.0)));
    assert_eq!(a.offset(), (100.0, 200.0));
    // Told the same size again, nothing moves.
    assert!(!a.set_content((500.0, 400.0)));
    // A view grown past the content pulls it to the top left.
    assert!(a.set_view((900.0, 900.0)));
    assert_eq!(a.max(), (0.0, 0.0));
    assert_eq!(a.offset(), (0.0, 0.0));
}

#[test]
fn a_wheel_notch_away_from_the_user_scrolls_up_and_shift_turns_it_across() {
    let mut a = area();
    a.set_offset((100.0, 200.0));
    // `host::Event::Wheel` gives a notch away from the user as positive: it scrolls up.
    assert!(a.wheel(0.0, 40.0, false));
    assert_eq!(a.offset(), (100.0, 160.0));
    // Towards the user is negative, and scrolls down.
    assert!(a.wheel(0.0, -120.0, false));
    assert_eq!(a.offset(), (100.0, 280.0));
    // Shift turns a vertical wheel across: away from the user scrolls left.
    assert!(a.wheel(0.0, 30.0, true));
    assert_eq!(a.offset(), (70.0, 280.0));
    // A trackpad's two-way movement is taken as it is, both axes subtracted.
    assert!(a.wheel(10.0, 20.0, true));
    assert_eq!(a.offset(), (60.0, 260.0));
    // At the top it does not move, so the caller may give the wheel to a parent.
    a.set_offset((0.0, 0.0));
    assert!(!a.wheel(0.0, 40.0, false));
    assert!(!a.wheel(40.0, 0.0, false));
    // The far end behaves the same way downwards, and a part that is not a number moves
    // nothing.
    a.set_offset((600.0, 1800.0));
    assert!(!a.wheel(0.0, -40.0, false));
    assert!(!a.wheel(f64::NAN, f64::NAN, false));
    assert_eq!(a.offset(), (600.0, 1800.0));
}

#[test]
fn the_page_keys_move_by_a_page_less_a_line_and_home_and_end_go_to_the_ends() {
    let mut a = area();
    // A page is the view's 200 less one line of 24: 176.
    assert!(a.key(KeyName::PageDown, Mods::default()));
    assert_eq!(a.offset(), (0.0, 176.0));
    assert!(a.key(KeyName::PageUp, Mods::default()));
    assert_eq!(a.offset(), (0.0, 0.0));
    // Space pages down, Shift+Space up, exactly as Page Down and Page Up.
    assert!(a.key(KeyName::Space, Mods::default()));
    assert_eq!(a.offset(), (0.0, 176.0));
    assert!(a.key(
        KeyName::Space,
        Mods {
            shift: true,
            ..Mods::default()
        }
    ));
    assert_eq!(a.offset(), (0.0, 0.0));
    // A line at a time, and a line across.
    assert!(a.key(KeyName::Down, Mods::default()));
    assert!(a.key(KeyName::Down, Mods::default()));
    assert_eq!(a.offset(), (0.0, 48.0));
    assert!(a.key(KeyName::Right, Mods::default()));
    assert_eq!(a.offset(), (24.0, 48.0));
    assert!(a.key(KeyName::Up, Mods::default()));
    assert!(a.key(KeyName::Left, Mods::default()));
    assert_eq!(a.offset(), (0.0, 24.0));
    // End is the bottom, Home the top; with Ctrl, bottom right and top left.
    assert!(a.key(KeyName::End, Mods::default()));
    assert_eq!(a.offset(), (0.0, 1800.0));
    assert!(a.key(
        KeyName::End,
        Mods {
            ctrl: true,
            ..Mods::default()
        }
    ));
    assert_eq!(a.offset(), (600.0, 1800.0));
    assert!(a.key(
        KeyName::Home,
        Mods {
            ctrl: true,
            ..Mods::default()
        }
    ));
    assert_eq!(a.offset(), (0.0, 0.0));
    assert!(a.key(KeyName::End, Mods::default()));
    assert!(a.key(KeyName::Home, Mods::default()));
    assert_eq!(a.offset(), (0.0, 0.0));
    // Keys that are not scroll keys, and an Alt chord, are the caller's.
    assert!(!a.key(KeyName::Enter, Mods::default()));
    assert!(!a.key(
        KeyName::PageDown,
        Mods {
            alt: true,
            ..Mods::default()
        }
    ));
    assert_eq!(a.offset(), (0.0, 0.0));
}

#[test]
fn reveal_moves_a_control_above_below_or_taller_than_the_view_into_view() {
    let mut a = area();
    a.set_offset((100.0, 100.0));
    // Already whole on screen: nothing moves.
    assert!(!a.reveal((150.0, 150.0), (50.0, 20.0)));
    assert_eq!(a.offset(), (100.0, 100.0));
    // Above and to the left: to that edge.
    assert!(a.reveal((40.0, 60.0), (60.0, 20.0)));
    assert_eq!(a.offset(), (40.0, 60.0));
    // Below and to the right: its end at the view's end.
    assert!(a.reveal((420.0, 380.0), (60.0, 30.0)));
    assert_eq!(a.offset(), (80.0, 210.0));
    // Taller than the view: its start.
    assert!(a.reveal((20.0, 900.0), (80.0, 260.0)));
    assert_eq!(a.offset(), (20.0, 900.0));
    // A rectangle past the far end stops at the content's end.
    assert!(a.reveal((940.0, 1900.0), (80.0, 100.0)));
    assert_eq!(a.offset(), (600.0, 1800.0));
    // A place that is not a number leaves that axis where it is.
    assert!(!a.reveal((f64::NAN, f64::NAN), (10.0, 10.0)));
    assert_eq!(a.offset(), (600.0, 1800.0));
}

#[test]
fn reveal_focus_moves_only_for_the_control_that_holds_the_focus() {
    let mut a = area();
    let mut focus: Focus<u32> = Focus::new();
    focus.begin_frame();
    for id in [1, 2, 3] {
        focus.add(id);
    }
    focus.end_frame();
    focus.focus(2);
    assert!(!a.reveal_focus(&focus, 1, (0.0, 900.0), (50.0, 20.0)));
    assert_eq!(a.offset(), (0.0, 0.0));
    assert!(a.reveal_focus(&focus, 2, (0.0, 900.0), (50.0, 20.0)));
    assert_eq!(a.offset(), (0.0, 720.0));
    assert!(!a.reveal_focus(&focus, 2, (0.0, 760.0), (50.0, 20.0)));
}

#[test]
fn reveal_focus_brings_a_control_in_once_and_does_not_pin_it_after_a_scroll() {
    let mut a = area();
    let mut focus: Focus<u32> = Focus::new();
    focus.begin_frame();
    for id in [1, 2] {
        focus.add(id);
    }
    focus.end_frame();
    focus.focus(2);
    // The first reveal moves it in; a control taller than the view shows its start.
    assert!(a.reveal_focus(&focus, 2, (0.0, 900.0), (50.0, 260.0)));
    assert_eq!(a.offset(), (0.0, 900.0));
    // Page Down scrolls inside it; the next frame's reveal for the same focus moves nothing.
    assert!(a.key(KeyName::PageDown, Mods::default()));
    assert_eq!(a.offset(), (0.0, 1076.0));
    assert!(!a.reveal_focus(&focus, 2, (0.0, 900.0), (50.0, 260.0)));
    assert_eq!(a.offset(), (0.0, 1076.0));
    // Even to the end of the content, where the control's start is far above the view.
    assert!(a.wheel(0.0, -1000.0, false));
    assert_eq!(a.offset(), (0.0, 1800.0));
    assert!(!a.reveal_focus(&focus, 2, (0.0, 900.0), (50.0, 260.0)));
    assert_eq!(a.offset(), (0.0, 1800.0));
    // Another control takes the focus: it is revealed in its turn.
    focus.focus(1);
    assert!(a.reveal_focus(&focus, 1, (0.0, 100.0), (50.0, 20.0)));
    assert_eq!(a.offset(), (0.0, 100.0));
}

#[test]
fn a_focus_that_leaves_the_area_and_comes_back_to_a_control_is_revealed_again() {
    fn offer(a: &mut ScrollArea, focus: &Focus<u32>, rows: &[(u32, (f64, f64))]) -> bool {
        let mut moved = false;
        for &(id, at) in rows {
            moved |= a.reveal_focus(focus, id, at, (50.0, 30.0));
        }
        moved
    }

    let mut a = area();
    let mut focus: Focus<u32> = Focus::new();
    focus.begin_frame();
    // 1 and 2 are controls the area lays out; 3 is a button outside it.
    for id in [1, 2, 3] {
        focus.add(id);
    }
    focus.end_frame();
    let rows = [(1u32, (0.0, 900.0)), (2, (0.0, 1500.0))];
    focus.focus(2);
    assert!(offer(&mut a, &focus, &rows));
    assert_eq!(a.offset(), (0.0, 1330.0));
    // The same focus must not reveal again after a scroll, or the view would be pinned:
    // the call for row 1 must not clear what the call for row 2 then restores.
    a.set_offset((0.0, 0.0));
    assert!(!offer(&mut a, &focus, &rows));
    assert_eq!(a.offset(), (0.0, 0.0));
    // Tab goes to the button outside the area: the area sees the focus leave.
    focus.focus(3);
    assert!(!offer(&mut a, &focus, &rows));
    assert_eq!(a.offset(), (0.0, 0.0));
    // Shift+Tab comes back to the last row: it is brought into view again.
    focus.focus(2);
    assert!(offer(&mut a, &focus, &rows));
    assert_eq!(a.offset(), (0.0, 1330.0));
}

#[test]
fn layout_column_makes_the_content_as_long_as_the_items_want_and_keeps_them_still() {
    let mut a = ScrollArea::new();
    let flex = Flex::column().gap(10.0);
    let items = [Item::fixed(30.0), Item::fixed(40.0), Item::fixed(50.0)];
    let rects = a.layout_column(Rect::new(0.0, 0.0, 300.0, 120.0), &flex, &items);
    // 30 + 10 + 40 + 10 + 50.
    assert_eq!(a.content(), (300.0, 140.0));
    assert_eq!(a.view(), (300.0, 120.0));
    assert_eq!(
        rects,
        vec![
            Rect::new(0.0, 0.0, 300.0, 30.0),
            Rect::new(0.0, 40.0, 300.0, 40.0),
            Rect::new(0.0, 90.0, 300.0, 50.0),
        ]
    );
    // It scrolls by the 20 the content is longer, and the rectangles do not move: they are
    // in content pixels, and the painter's offset shows them elsewhere on screen.
    assert!(a.scroll_by((0.0, 50.0)));
    assert_eq!(a.offset(), (0.0, 20.0));
    let again = a.layout_column(Rect::new(0.0, 0.0, 300.0, 120.0), &flex, &items);
    assert_eq!(again, rects);
    assert_eq!(a.offset(), (0.0, 20.0));
    // A flexible item keeps its natural size: the column is laid out at its own length, not
    // squeezed into the 120 px view.
    let flex = Flex::column().gap(0.0);
    let items = [Item::flex(1.0).min(80.0), Item::fixed(60.0)];
    let rects = a.layout_column(Rect::new(0.0, 0.0, 300.0, 120.0), &flex, &items);
    assert_eq!(a.content(), (300.0, 140.0));
    assert_eq!(rects[0].h, 80.0);
    assert_eq!(rects[1].y, 80.0);
}

#[test]
fn layout_column_keeps_the_offset_when_the_view_and_the_content_grow_together() {
    let mut a = ScrollArea::new();
    a.set_view((200.0, 200.0));
    a.set_content((200.0, 600.0));
    a.set_offset((0.0, 400.0));
    // A view of 300 and a content of 900 in one frame: clamping against the old content
    // first would pull 400 down to 300 and lose it.
    let flex = Flex::column();
    let items: Vec<Item> = (0..9).map(|_| Item::fixed(100.0)).collect();
    a.layout_column(Rect::new(0.0, 0.0, 300.0, 300.0), &flex, &items);
    assert_eq!(a.content(), (300.0, 900.0));
    assert_eq!(a.view(), (300.0, 300.0));
    assert_eq!(a.offset(), (0.0, 400.0));
}

#[test]
fn points_go_between_the_content_and_the_area() {
    let mut a = area();
    a.set_offset((30.0, 40.0));
    let view = Rect::new(10.0, 20.0, 400.0, 200.0);
    assert_eq!(a.to_area(view, (130.0, 140.0)), (110.0, 120.0));
    assert_eq!(a.to_content(view, 110.0, 120.0), Some((130.0, 140.0)));
    assert_eq!(a.to_content(view, 5.0, 120.0), None);
    assert_eq!(
        a.content_rect(view),
        Rect::new(-20.0, -20.0, 1000.0, 2000.0)
    );
}

#[test]
fn a_size_an_offset_or_a_point_that_is_not_a_number_does_not_panic() {
    let mut a = ScrollArea::new();
    a.set_view((f64::NAN, f64::INFINITY));
    assert_eq!(a.view(), (0.0, 0.0));
    a.set_content((-5.0, f64::NAN));
    assert_eq!(a.content(), (0.0, 0.0));
    assert!(!a.set_offset((f64::NAN, 3.0)));
    assert_eq!(a.offset(), (0.0, 0.0));
    // A view and a content of real sizes; a movement that is not a number does nothing,
    // and an infinite one goes as far as the content lets it.
    a.set_view((300.0, 200.0));
    a.set_content((600.0, 900.0));
    assert!(!a.scroll_by((f64::NAN, f64::NAN)));
    assert_eq!(a.offset(), (0.0, 0.0));
    assert!(a.scroll_by((f64::INFINITY, 0.0)));
    assert_eq!(a.offset(), (300.0, 0.0));
    a.set_offset((0.0, 0.0));
    assert!(!a.reveal((f64::INFINITY, f64::NEG_INFINITY), (10.0, 10.0)));
    assert_eq!(a.offset(), (0.0, 0.0));
}

#[test]
fn a_component_that_is_not_a_number_keeps_its_axis_where_it_is() {
    let mut a = area();
    a.set_offset((100.0, 200.0));
    // An offset with a NaN part moves the other axis and leaves this one where it was.
    assert!(a.set_offset((f64::NAN, 300.0)));
    assert_eq!(a.offset(), (100.0, 300.0));
    assert!(a.scroll_to((f64::NAN, 250.0)));
    assert_eq!(a.offset(), (100.0, 250.0));
    assert!(a.scroll_by((f64::NAN, -50.0)));
    assert_eq!(a.offset(), (100.0, 200.0));
    // Nothing but NaN parts: nothing moves, and the answer says so.
    assert!(!a.scroll_by((f64::NAN, f64::NAN)));
    assert_eq!(a.offset(), (100.0, 200.0));
    assert!(!a.set_offset((f64::NAN, f64::NAN)));
    assert_eq!(a.offset(), (100.0, 200.0));
}

// ---- drawing ---------------------------------------------------------------------------

const PAGE: Rect = Rect::new(0.0, 0.0, 160.0, 120.0);
const VIEW_RECT: Rect = Rect::new(30.0, 20.0, 100.0, 80.0);

fn shot(pal: &Colors, draw: impl FnOnce(&mut Painter<'_>)) -> Bitmap {
    let mut bmp = Bitmap::new(160, 120);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(PAGE, pal.page);
        draw(&mut p);
    }
    bmp
}

fn at(bmp: &Bitmap, x: f32, y: f32) -> Rgb {
    let o = (y as usize * bmp.width as usize + x as usize) * 4;
    [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
}

#[test]
fn the_body_is_clipped_to_the_area() {
    let mut a = ScrollArea::new();
    a.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
    a.set_content((200.0, 400.0));
    a.set_offset((0.0, 50.0));
    let ink = [200, 30, 40];
    let mark = [20, 220, 90];
    let drawn = shot(&LIGHT, |p| {
        let saved = a.scrolled(p, VIEW_RECT);
        p.fill(Rect::new(0.0, 0.0, 200.0, 400.0), ink);
        // A mark whose place says whether the content was moved as well as clipped.
        p.fill(Rect::new(10.0, 60.0, 10.0, 10.0), mark);
        saved.restore(p);
    });
    // Inside the area the body shows, wherever the offset put it.
    assert_eq!(at(&drawn, 80.0, 60.0), ink);
    assert_eq!(at(&drawn, 31.0, 99.0), ink);
    // The translation: content (10, 60) is at screen (40, 30) at offset (0, 50).
    assert_eq!(at(&drawn, 45.0, 35.0), mark);
    // Outside it nothing was touched: the page keeps its pixels all round the area.
    for (x, y) in [(10.0, 10.0), (29.0, 60.0), (131.0, 60.0), (80.0, 19.0)] {
        assert_eq!(at(&drawn, x, y), LIGHT.page, "({x}, {y})");
    }
    assert_eq!(at(&drawn, 80.0, 101.0), LIGHT.page);
    // Without the clip the same fill would cover the whole bitmap.
    let unclipped = shot(&LIGHT, |p| {
        p.fill(Rect::new(0.0, 0.0, 200.0, 400.0), ink);
    });
    assert_eq!(at(&unclipped, 80.0, 10.0), ink);
}

#[test]
fn after_restore_drawing_outside_the_area_is_unclipped_and_in_place() {
    let mut a = ScrollArea::new();
    a.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
    a.set_content((200.0, 400.0));
    a.set_offset((0.0, 50.0));
    let ink = [10, 200, 30];
    let inside = [200, 30, 40];
    let drawn = shot(&LIGHT, |p| {
        let saved = a.scrolled(p, VIEW_RECT);
        p.fill(Rect::new(0.0, 60.0, 10.0, 10.0), inside);
        saved.restore(p);
        // The page's own top left, outside the area, and a corner beyond its right edge:
        // both land where asked, unclipped and unmoved.
        p.fill(Rect::new(0.0, 0.0, 10.0, 10.0), ink);
        p.fill(Rect::new(150.0, 110.0, 10.0, 10.0), ink);
    });
    assert_eq!(at(&drawn, 5.0, 5.0), ink);
    assert_eq!(at(&drawn, 155.0, 115.0), ink);
    // The body drawn before the restore was clipped and moved as it should be.
    assert_eq!(at(&drawn, 35.0, 35.0), inside);
    assert_eq!(at(&drawn, 29.0, 20.0), LIGHT.page);
}

#[test]
fn a_scrollbar_shows_only_where_the_content_is_longer() {
    let plain = shot(&LIGHT, |_| ());
    let bars = |content: (f64, f64), offset: (f64, f64)| {
        let mut a = ScrollArea::new();
        a.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
        a.set_content(content);
        a.set_offset(offset);
        shot(&LIGHT, |p| draw_bars(p, &LIGHT, VIEW_RECT, &a, false))
    };
    // It fits: no bar, and not one pixel changed.
    assert_eq!(bars((100.0, 60.0), (0.0, 0.0)).pixels, plain.pixels);
    // Down only: a bar at the right edge, where the vertical thumb is.
    let mut long = ScrollArea::new();
    long.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
    long.set_content((100.0, 400.0));
    let (down, across) = long.bars(VIEW_RECT);
    assert!(across.is_none());
    let (_, thumb) = down.unwrap();
    let down_only = bars((100.0, 400.0), (0.0, 0.0));
    assert_ne!(at(&down_only, thumb.cx(), thumb.cy()), LIGHT.page);
    assert_eq!(
        at(&down_only, VIEW_RECT.x + 5.0, VIEW_RECT.cy()),
        LIGHT.page
    );
    // Across only: a bar along the bottom, and none at the right edge where the vertical
    // thumb would be.
    let mut wide = ScrollArea::new();
    wide.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
    wide.set_content((400.0, 60.0));
    let (down, across) = wide.bars(VIEW_RECT);
    assert!(down.is_none());
    let (_, hthumb) = across.unwrap();
    let across_only = bars((400.0, 60.0), (0.0, 0.0));
    assert_ne!(at(&across_only, hthumb.cx(), hthumb.cy()), LIGHT.page);
    assert_eq!(
        at(&across_only, VIEW_RECT.right() - 6.0, VIEW_RECT.y + 5.0),
        LIGHT.page
    );
    // Both, scrolled to the far end with the bars active: each stops short of the corner.
    let mut b = ScrollArea::new();
    b.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
    b.set_content((400.0, 400.0));
    b.set_offset((300.0, 320.0));
    let (down, across) = b.bars(VIEW_RECT);
    let (_, dthumb) = down.unwrap();
    let (_, hthumb) = across.unwrap();
    let both = shot(&LIGHT, |p| draw_bars(p, &LIGHT, VIEW_RECT, &b, true));
    assert_ne!(at(&both, dthumb.cx(), dthumb.cy()), LIGHT.page);
    assert_ne!(at(&both, hthumb.cx(), hthumb.cy()), LIGHT.page);
    // The corner the two bars leave to each other shows the page, not either track.
    assert_eq!(
        at(&both, VIEW_RECT.right() - 6.0, VIEW_RECT.bottom() - 6.0),
        LIGHT.page
    );
}

#[test]
fn a_scrolled_frame_shows_the_rows_at_the_offset() {
    const ROW: f32 = 20.0;
    let rows: Vec<Rgb> = (0..12).map(|i| [30 + i * 15, 60, 200 - i * 10]).collect();
    let mut a = ScrollArea::new();
    a.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
    a.set_content((100.0, 12.0 * f64::from(ROW)));
    a.set_offset((0.0, 2.0 * f64::from(ROW)));
    let drawn = shot(&LIGHT, |p| {
        let saved = a.scrolled(p, VIEW_RECT);
        for (i, ink) in rows.iter().enumerate() {
            p.fill(Rect::new(0.0, i as f32 * ROW, 100.0, ROW), *ink);
        }
        saved.restore(p);
    });
    // At offset 40 the top of the view is row 2; four rows of 20 fit in 80.
    for (i, ink) in rows.iter().enumerate().skip(2).take(4) {
        let y = VIEW_RECT.y + (i as f32 - 2.0) * ROW + ROW / 2.0;
        assert_eq!(at(&drawn, VIEW_RECT.cx(), y), *ink, "row {i}");
    }
    // Row 1 is above the view and row 6 below it: the page shows there.
    assert_eq!(at(&drawn, VIEW_RECT.cx(), VIEW_RECT.y - 1.0), LIGHT.page);
    assert_eq!(
        at(&drawn, VIEW_RECT.cx(), VIEW_RECT.bottom() + 1.0),
        LIGHT.page
    );
}

#[test]
fn the_animated_bars_at_rest_draw_the_plain_ones_in_both_palettes() {
    for pal in [&LIGHT, &DARK] {
        let mut a = ScrollArea::new();
        a.set_view((VIEW_RECT.w as f64, VIEW_RECT.h as f64));
        a.set_content((400.0, 400.0));
        let plain = shot(pal, |p| draw_bars(p, pal, VIEW_RECT, &a, true));
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut bmp = Bitmap::new(160, 120);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(PAGE, pal.page);
            anim.begin_frame(clock.now());
            draw_bars_animated(&mut p, pal, &mut anim, "area", VIEW_RECT, &a, true);
        }
        assert!(anim.end_frame().is_none(), "an idle bar asks for no frame");
        assert!(bmp.pixels == plain.pixels, "dark {}", pal.dark);
    }
}

/// A sheet for a person to judge: `cargo test -p mdshared-ui scrollarea_sheet -- --ignored`
/// writes `target/specimen/scrollarea.png`: a dialog card whose body is taller than the
/// card, at the top and scrolled down, light above and dark below.
#[test]
#[ignore]
fn scrollarea_sheet() {
    use crate::gfx::Face;
    use crate::overlay::{DIALOG_FOOTER, DIALOG_PAD, draw_dialog_card};

    let (w, h) = (420.0f32, 300.0f32);
    let mut bmp = Bitmap::new((2.0 * (w + 20.0)) as u32, (2.0 * (h + 20.0)) as u32);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    for (k, pal) in [LIGHT, DARK].iter().enumerate() {
        let oy = k as f32 * (h + 20.0) + 10.0;
        for (j, offset) in [0.0f64, 220.0].iter().enumerate() {
            let ox = j as f32 * (w + 20.0) + 10.0;
            p.fill(
                Rect::new(ox - 10.0, oy - 10.0, w + 20.0, h + 20.0),
                pal.page,
            );
            let card = Rect::new(ox, oy, w, h);
            draw_dialog_card(&mut p, pal, card);
            p.text(
                Face::Semibold,
                15.0,
                card.x + DIALOG_PAD,
                card.y + DIALOG_PAD,
                "Preferences",
                pal.text,
                card.w - 2.0 * DIALOG_PAD,
            );
            let body = Rect::new(
                card.x + DIALOG_PAD,
                card.y + DIALOG_PAD + 30.0,
                card.w - 2.0 * DIALOG_PAD,
                card.h - 2.0 * DIALOG_PAD - DIALOG_FOOTER - 14.0,
            );
            let mut area = ScrollArea::new();
            let flex = Flex::column().gap(8.0);
            let items: Vec<Item> = (0..12).map(|_| Item::fixed(36.0)).collect();
            let rects = area.layout_column(body, &flex, &items);
            area.set_offset((0.0, *offset));
            let saved = area.scrolled(&mut p, body);
            for (i, r) in rects.iter().enumerate() {
                p.frame(*r, 4.0, pal.field, pal.line);
                let label = format!("Setting {}", i + 1);
                p.text(
                    Face::Ui,
                    13.0,
                    r.x + 10.0,
                    r.cy(),
                    &label,
                    pal.text,
                    r.w - 20.0,
                );
            }
            saved.restore(&mut p);
            draw_bars(&mut p, pal, body, &area, true);
        }
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
    std::fs::create_dir_all(&out).unwrap();
    bmp.write_png(&out.join("scrollarea.png")).unwrap();
}
