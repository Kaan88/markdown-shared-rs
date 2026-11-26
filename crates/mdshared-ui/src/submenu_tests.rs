use super::*;
use crate::anim::{Clock, ManualClock};
use crate::gfx::{Face, Gfx};
use crate::menu::PAD;
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

const AREA: Rect = Rect::new(0.0, 0.0, 800.0, 600.0);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// New, Open recent (a.md, b.md, More (c.md)), Export (PDF, HTML), Print, Quit.
fn tree() -> Vec<Node<u8>> {
    vec![
        Node::new(Item::new(1, "&New")),
        Node::submenu(
            Item::new(2, "Open &recent"),
            vec![
                Node::new(Item::new(20, "a.md")),
                Node::new(Item::new(21, "b.md")),
                Node::submenu(Item::new(22, "More"), vec![Item::new(220, "c.md").into()]),
            ],
        ),
        Node::submenu(
            Item::new(3, "&Export"),
            vec![
                Node::new(Item::new(30, "PDF")),
                Node::new(Item::new(31, "HTML")),
            ],
        ),
        Node::new(Item::new(4, "&Print").sep()),
        Node::new(Item::new(5, "&Quit")),
    ]
}

fn place(c: &mut Cascade<u8>, area: Rect) {
    let mut bmp = Bitmap::new(8, 8);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    c.place_in(&mut p, area);
}

/// The cascade at (100, 100), placed: its root is 180 by 179 there.
fn opened() -> Cascade<u8> {
    let mut c = Cascade::at(tree(), 100.0, 100.0);
    place(&mut c, AREA);
    c
}

/// A point 20 px into row `row` of the menu at `level`, at its middle height.
fn on(c: &Cascade<u8>, level: usize, row: usize) -> (f32, f32) {
    let r = c.menu(level).unwrap().row_rect(row);
    (r.x + 20.0, r.cy())
}

fn hover(c: &Cascade<u8>, level: usize) -> Option<usize> {
    c.menu(level).unwrap().hover
}

// ---- geometry ----------------------------------------------------------------------------------

#[test]
fn a_menu_around_a_point_opens_below_right_and_flips_at_each_edge() {
    let point = |x, y| around(200.0, 150.0, Rect::new(x, y, 0.0, 0.0), AREA);
    let p = point(100.0, 100.0);
    assert_eq!(
        (p.rect, p.left, p.above),
        (Rect::new(100.0, 100.0, 200.0, 150.0), false, false)
    );
    // At the right edge it opens to the left of the point, at the bottom above it.
    let p = point(700.0, 100.0);
    assert_eq!((p.rect.x, p.left, p.above), (500.0, true, false));
    let p = point(100.0, 550.0);
    assert_eq!((p.rect.y, p.left, p.above), (400.0, false, true));
    let p = point(700.0, 550.0);
    assert_eq!(
        (p.rect.x, p.rect.y, p.left, p.above),
        (500.0, 400.0, true, true)
    );
    // A rectangle: below it, left edges level; flipped, right edges level.
    let button = Rect::new(300.0, 40.0, 80.0, 24.0);
    assert_eq!(around(200.0, 150.0, button, AREA).rect.x, 300.0);
    assert_eq!(around(200.0, 150.0, button, AREA).rect.y, 64.0);
    let corner = Rect::new(740.0, 40.0, 40.0, 24.0);
    assert_eq!(around(200.0, 150.0, corner, AREA).rect.x, 580.0);
    let low = Rect::new(300.0, 560.0, 80.0, 24.0);
    assert_eq!(around(200.0, 150.0, low, AREA).rect.bottom(), 560.0);
}

#[test]
fn a_menu_that_fits_on_neither_side_goes_where_there_is_more_room_and_stays_inside() {
    // 400 px tall at y 350: 246 px below, 346 above. Above, pushed down into the area, over
    // the point.
    let p = around(200.0, 400.0, Rect::new(100.0, 350.0, 0.0, 0.0), AREA);
    assert_eq!((p.rect.y, p.rect.h, p.above), (4.0, 400.0, true));
    // Taller than the area: as tall as the area, from its top.
    let p = around(200.0, 1000.0, Rect::new(100.0, 300.0, 0.0, 0.0), AREA);
    assert_eq!((p.rect.y, p.rect.h), (4.0, 592.0));
    // Wider than the area: as wide as it.
    let p = around(1000.0, 100.0, Rect::new(100.0, 300.0, 0.0, 0.0), AREA);
    assert_eq!((p.rect.x, p.rect.w), (4.0, 792.0));
    // An anchor outside the area to the left: the menu is pulled in.
    let p = around(200.0, 100.0, Rect::new(-300.0, 300.0, 0.0, 0.0), AREA);
    assert_eq!(p.rect.x, 4.0);
}

/// A small LCG with a fixed seed.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    /// A whole number of px in `0..n`.
    fn below(&mut self, n: u64) -> f32 {
        (self.next() % n) as f32
    }
}

fn inside(r: Rect, room: Rect) -> bool {
    r.x >= room.x && r.y >= room.y && r.right() <= room.right() && r.bottom() <= room.bottom()
}

#[test]
fn around_is_the_first_corner_that_fits_in_the_order_below_right_below_left_above_right() {
    let mut rng = Lcg(7);
    for _ in 0..20_000 {
        let area = Rect::new(
            rng.below(50),
            rng.below(50),
            100.0 + rng.below(700),
            100.0 + rng.below(500),
        );
        let anchor = Rect::new(
            rng.below(900) - 50.0,
            rng.below(700) - 50.0,
            rng.below(60),
            rng.below(40),
        );
        let (w, h) = (rng.below(300), rng.below(400));
        let room = room(area);
        let got = around(w, h, anchor, area);
        let corners = [
            (anchor.x, anchor.bottom()),
            (anchor.right() - w, anchor.bottom()),
            (anchor.x, anchor.y - h),
            (anchor.right() - w, anchor.y - h),
        ];
        let first = corners
            .iter()
            .map(|&(x, y)| Rect::new(x, y, w, h))
            .find(|&r| inside(r, room));
        match first {
            Some(r) => assert_eq!(got.rect, r, "{anchor:?} {w}x{h} in {area:?}"),
            None => {
                // Inside all the same, as large as the area allows.
                assert!(
                    inside(got.rect, room),
                    "{anchor:?} {w}x{h} in {area:?}: {got:?}"
                );
                assert_eq!(got.rect.w, w.min(room.w));
                assert_eq!(got.rect.h, h.min(room.h));
            }
        }
    }
}

#[test]
fn a_submenu_opens_to_the_right_flips_to_the_left_and_slides_up() {
    let row = Rect::new(100.0, 137.0, 180.0, 32.0);
    let p = beside(180.0, 106.0, row, AREA, false);
    // Two px off its menu, its first row (5 px of padding down) level with the item.
    assert_eq!(
        (p.rect, p.left),
        (Rect::new(282.0, 132.0, 180.0, 106.0), false)
    );
    // No room on the right: on the left.
    let p = beside(
        180.0,
        106.0,
        Rect::new(600.0, 137.0, 180.0, 32.0),
        AREA,
        false,
    );
    assert_eq!((p.rect.x, p.left), (418.0, true));
    // Leftward first, when there is room there; else the right.
    let p = beside(
        180.0,
        106.0,
        Rect::new(300.0, 137.0, 180.0, 32.0),
        AREA,
        true,
    );
    assert_eq!((p.rect.x, p.left), (118.0, true));
    let p = beside(180.0, 106.0, row, AREA, true);
    assert_eq!((p.rect.x, p.left), (282.0, false));
    // Near the bottom: slid up to end inside the area; taller than it: from its top.
    let p = beside(
        180.0,
        106.0,
        Rect::new(100.0, 550.0, 180.0, 32.0),
        AREA,
        false,
    );
    assert_eq!(p.rect.bottom(), 596.0);
    let p = beside(180.0, 1000.0, row, AREA, false);
    assert_eq!((p.rect.y, p.rect.h), (4.0, 592.0));
    // Room on neither side: the side with more, pushed in over the menu.
    let p = beside(
        200.0,
        106.0,
        Rect::new(10.0, 100.0, 700.0, 32.0),
        AREA,
        false,
    );
    assert_eq!((p.rect.x, p.left), (596.0, false));
}

#[test]
fn nonsense_places_somewhere_without_panicking() {
    let nan = f32::NAN;
    let inf = f32::INFINITY;
    let anchors = [
        Rect::new(nan, nan, nan, nan),
        Rect::new(inf, -inf, 0.0, 0.0),
        Rect::new(10.0, 10.0, -50.0, -50.0),
    ];
    let areas = [AREA, Rect::default(), Rect::new(0.0, 0.0, -10.0, 3.0)];
    for anchor in anchors {
        for area in areas {
            for (w, h) in [
                (nan, nan),
                (inf, inf),
                (-1.0, -1.0),
                (0.0, 0.0),
                (200.0, 100.0),
            ] {
                let a = around(w, h, anchor, area);
                let b = beside(w, h, anchor, area, true);
                for r in [a.rect, b.rect] {
                    assert!(r.w >= 0.0 && r.h >= 0.0, "{anchor:?} {w}x{h} in {area:?}");
                }
                if area == AREA {
                    assert!(inside(a.rect, room(area)) && inside(b.rect, room(area)));
                }
            }
        }
    }
}

#[test]
fn the_aim_test_is_the_triangle_to_the_near_edge() {
    let target = Rect::new(200.0, 100.0, 100.0, 100.0);
    let from = (100.0, 150.0);
    assert!(aims_at(from, (150.0, 150.0), target));
    assert!(aims_at(from, (150.0, 170.0), target));
    assert!(aims_at(from, from, target));
    // Too steep, or backwards.
    assert!(!aims_at(from, (150.0, 100.0), target));
    assert!(!aims_at(from, (101.0, 190.0), target));
    assert!(!aims_at(from, (90.0, 150.0), target));
    // A submenu on the left: the triangle goes to its right edge.
    let left = Rect::new(0.0, 100.0, 100.0, 100.0);
    assert!(aims_at((300.0, 150.0), (200.0, 150.0), left));
    assert!(!aims_at((300.0, 150.0), (310.0, 150.0), left));
    // Nothing to aim at, and nonsense.
    assert!(!aims_at(from, (150.0, 150.0), Rect::default()));
    assert!(!aims_at((f32::NAN, 0.0), (150.0, 150.0), target));
    assert!(!aims_at(from, (f32::INFINITY, 150.0), target));
}

#[test]
fn the_aim_test_agrees_with_the_slopes_to_the_ends_of_the_edge() {
    let mut rng = Lcg(11);
    let (mut ins, mut outs) = (0, 0);
    for _ in 0..20_000 {
        let target = Rect::new(
            300.0 + rng.below(100),
            rng.below(300),
            50.0 + rng.below(150),
            20.0 + rng.below(300),
        );
        let from = (rng.below(300), rng.below(600));
        let to = (rng.below(400), rng.below(600));
        let near = target.x;
        let (top, bottom) = (target.y - SLACK, target.bottom() + SLACK);
        // The pointer went forwards, not past the edge, and its slope from `from` lies between
        // the slopes to the edge's two ends; the slopes compared multiplied out by the runs.
        let run = to.0 - from.0;
        let span = near - from.0;
        let rise = (to.1 - from.1) * span;
        let (lo, hi) = ((top - from.1) * run, (bottom - from.1) * run);
        // Clear of every border by a margin, either way, so rounding decides nothing.
        let m = 4.0 * span;
        let clear_in = run > 1.0 && to.0 < near - 1.0 && rise > lo + m && rise < hi - m;
        let clear_out = run < -1.0 || to.0 > near + 1.0 || rise < lo - m || rise > hi + m;
        let got = aims_at(from, to, target);
        if clear_in {
            ins += 1;
            assert!(got, "{from:?} {to:?} {target:?}");
        }
        if clear_out {
            outs += 1;
            assert!(!got, "{from:?} {to:?} {target:?}");
        }
    }
    assert!(ins > 500 && outs > 5000, "{ins} in, {outs} out");
}

// ---- the menu by a rectangle, and its arrows ---------------------------------------------------

fn items() -> Vec<Item<u8>> {
    ["Cut", "Copy", "Paste", "Select all"]
        .iter()
        .enumerate()
        .map(|(i, l)| Item::new(i as u8, *l))
        .collect()
}

fn shot(draw: impl FnOnce(&mut Painter)) -> Bitmap {
    let mut bmp = Bitmap::new(800, 600);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    p.fill(AREA, LIGHT.page);
    draw(&mut p);
    bmp
}

#[test]
fn a_menu_around_a_point_with_room_draws_as_the_menu_hung_from_that_point() {
    let mut old = Menu::new(items(), 60.0, 50.0, false);
    let mut new = Menu::new(items(), 0.0, 0.0, false).around(Rect::new(60.0, 50.0, 0.0, 0.0));
    let a = shot(|p| old.draw_in(p, &LIGHT, AREA));
    let b = shot(|p| new.draw_in(p, &LIGHT, AREA));
    assert!(a.pixels == b.pixels);
    assert_eq!(old.rect(), new.rect());
    assert!(!new.opened_left() && !new.opened_above());
    // At the bottom right corner it flips both ways, and says so.
    let mut corner = Menu::new(items(), 0.0, 0.0, false).around(Rect::new(790.0, 590.0, 0.0, 0.0));
    let _ = shot(|p| corner.draw_in(p, &LIGHT, AREA));
    assert_eq!(
        (corner.rect().right(), corner.rect().bottom()),
        (790.0, 590.0)
    );
    assert!(corner.opened_left() && corner.opened_above());
    // A menu made `above` says it stands above.
    assert!(Menu::new(items(), 9.0, 9.0, false).above().opened_above());
}

#[test]
fn a_menu_flipped_above_its_anchor_opens_upwards_like_one_made_above() {
    let anchor = Rect::new(60.0, 590.0, 0.0, 0.0);
    let mut flipped = Menu::new(items(), 0.0, 0.0, false).around(anchor);
    let mut above = Menu::new(items(), 60.0, 590.0, false).above();
    let a = shot(|p| flipped.draw_in_presence(p, &LIGHT, AREA, 0.5));
    let b = shot(|p| above.draw_in_presence(p, &LIGHT, AREA, 0.5));
    assert!(flipped.opened_above());
    assert_eq!(flipped.rect(), above.rect());
    assert!(a.pixels == b.pixels);
}

#[test]
fn a_row_that_opens_a_submenu_draws_an_arrow_and_nothing_else_changes() {
    let mut plain = Menu::new(items(), 60.0, 50.0, false);
    let mut marked = Menu::new(items(), 60.0, 50.0, false).submenu_rows([1, 99]);
    assert!(marked.opens_submenu(1) && !marked.opens_submenu(0) && !marked.opens_submenu(99));
    let a = shot(|p| plain.draw_in(p, &LIGHT, AREA));
    let b = shot(|p| marked.draw_in(p, &LIGHT, AREA));
    assert_eq!(plain.rect(), marked.rect());
    let r = marked.row_rect(1);
    let arrow = Rect::new(r.right() - 28.0, r.y, 28.0, r.h);
    let mut differs = false;
    for y in 0..600usize {
        for x in 0..800usize {
            let i = (y * 800 + x) * 4;
            if a.pixels[i..i + 4] != b.pixels[i..i + 4] {
                assert!(
                    arrow.contains(x as f32, y as f32),
                    "{x},{y} outside the arrow"
                );
                differs = true;
            }
        }
    }
    assert!(differs, "the arrow is drawn");
    // A menu with long labels keeps room for the arrow: it is wider by it.
    let long = || vec![Item::new(0u8, "x".repeat(40)), Item::new(1, "y")];
    let mut wide = Menu::new(long(), 10.0, 10.0, false);
    let mut wider = Menu::new(long(), 10.0, 10.0, false).submenu_rows([0]);
    let _ = shot(|p| wide.draw_in(p, &LIGHT, AREA));
    let _ = shot(|p| wider.draw_in(p, &LIGHT, AREA));
    assert_eq!(wider.rect().w, wide.rect().w + 28.0);
}

// ---- the cascade: pointer and time -------------------------------------------------------------

#[test]
fn resting_on_a_row_opens_its_submenu_after_the_delay_and_not_before() {
    let clock = ManualClock::new();
    let t0 = clock.now();
    let mut c = opened();
    assert_eq!(
        c.menu(0).unwrap().rect(),
        Rect::new(100.0, 100.0, 180.0, 179.0)
    );
    let (x, y) = on(&c, 0, 1);
    assert!(c.pointer(x, y, t0));
    assert_eq!(hover(&c, 0), Some(1));
    assert_eq!(c.deadline(), Some(t0 + DELAY));
    // Moving within the row does not put it off.
    assert!(!c.pointer(x + 30.0, y + 5.0, t0 + ms(200)));
    assert_eq!(c.deadline(), Some(t0 + DELAY));
    assert!(!c.tick(t0 + ms(399)));
    assert_eq!(c.depth(), 1);
    assert!(c.tick(t0 + DELAY));
    assert_eq!((c.depth(), c.path()), (2, &[1][..]));
    assert_eq!(c.deadline(), None);
    assert!(!c.tick(t0 + ms(5000)));
    // Placed: beside the root, its first row level with the row that opened it.
    place(&mut c, AREA);
    let sub = c.menu(1).unwrap();
    assert_eq!(sub.rect().x, c.menu(0).unwrap().rect().right() + GAP);
    assert_eq!(sub.row_rect(0).y, c.menu(0).unwrap().row_rect(1).y);
    // The keys stay with the root until the pointer goes in.
    assert_eq!(c.focus(), 0);
    // A row with no submenu asks for nothing.
    let mut c = opened();
    let (x, y) = on(&c, 0, 0);
    assert!(c.pointer(x, y, t0));
    assert_eq!(c.deadline(), None);
}

/// The cascade with Open recent open, placed, and the pointer resting on its row near the
/// right edge.
fn recent_open(t0: Instant) -> Cascade<u8> {
    let mut c = opened();
    let (x, y) = on(&c, 0, 1);
    assert_eq!(c.click(x, y), Outcome::Changed);
    place(&mut c, AREA);
    let r = c.menu(0).unwrap().row_rect(1);
    c.pointer(r.right() - 10.0, r.cy(), t0);
    assert_eq!(c.deadline(), None);
    c
}

#[test]
fn the_pointer_on_its_way_to_an_open_submenu_does_not_close_it() {
    let clock = ManualClock::new();
    let t0 = clock.now();
    let mut c = recent_open(t0);
    let r = c.menu(0).unwrap().row_rect(1);
    let (x0, y0) = (r.right() - 10.0, r.cy());
    // Down and to the right, across Export (a row with a submenu of its own), to the submenu.
    for i in 1..=16u64 {
        let (x, y) = (x0 + i as f32, y0 + 2.0 * i as f32);
        let now = t0 + ms(10 * i);
        c.pointer(x, y, now);
        assert_eq!(hover(&c, 0), Some(1), "step {i}");
        assert_eq!(c.path(), &[1][..]);
        assert!(!c.tick(now));
    }
    assert!(c.menu(1).unwrap().contains(x0 + 20.0, y0 + 34.0));
    c.pointer(x0 + 20.0, y0 + 34.0, t0 + ms(170));
    assert_eq!(c.focus(), 1);
    assert_eq!(c.deadline(), None, "in the submenu, nothing waits");
    assert!(!c.tick(t0 + ms(10_000)));
    assert_eq!((c.path(), hover(&c, 0)), (&[1][..], Some(1)));
}

#[test]
fn a_pointer_that_rests_on_its_way_switches_after_the_delay() {
    let clock = ManualClock::new();
    let t0 = clock.now();
    let mut c = recent_open(t0);
    let r = c.menu(0).unwrap().row_rect(1);
    let (x0, y0) = (r.right() - 10.0, r.cy());
    // Onto Export, on the way, and resting there.
    let stop = t0 + ms(50);
    c.pointer(x0 + 4.0, y0 + 20.0, stop);
    assert_eq!(hover(&c, 0), Some(1));
    assert_eq!(c.deadline(), Some(stop + DELAY));
    assert!(!c.tick(stop + ms(399)));
    assert!(c.tick(stop + DELAY));
    assert_eq!((hover(&c, 0), c.path()), (Some(2), &[2][..]));
}

#[test]
fn moving_straight_down_lights_the_next_row_at_once_and_switches_after_the_delay() {
    let clock = ManualClock::new();
    let t0 = clock.now();
    let mut c = recent_open(t0);
    let (x, y) = on(&c, 0, 1);
    c.pointer(x, y, t0);
    let (x2, y2) = on(&c, 0, 2);
    assert!(c.pointer(x2, y2, t0 + ms(10)));
    assert_eq!(hover(&c, 0), Some(2));
    assert_eq!(c.path(), &[1][..], "the open one stays for now");
    assert!(c.tick(t0 + ms(10) + DELAY));
    assert_eq!(c.path(), &[2][..]);
    // Onto Print, which opens nothing: the submenu closes after the delay.
    let (x3, y3) = on(&c, 0, 3);
    c.pointer(x3, y3, t0 + ms(1000));
    assert!(c.tick(t0 + ms(1000) + DELAY));
    assert_eq!((c.depth(), hover(&c, 0)), (1, Some(3)));
}

#[test]
fn back_on_the_row_that_opened_it_the_submenu_stays() {
    let clock = ManualClock::new();
    let t0 = clock.now();
    let mut c = recent_open(t0);
    let (x, y) = on(&c, 0, 3);
    c.pointer(x, y, t0);
    assert!(c.deadline().is_some());
    let (x, y) = on(&c, 0, 1);
    c.pointer(x, y, t0 + ms(100));
    assert_eq!(c.deadline(), None);
    assert!(!c.tick(t0 + ms(10_000)));
    assert_eq!(c.path(), &[1][..]);
}

#[test]
fn leaving_every_menu_keeps_the_submenu_and_drops_what_was_asked() {
    let clock = ManualClock::new();
    let t0 = clock.now();
    let mut c = recent_open(t0);
    let (x, y) = on(&c, 1, 0);
    c.pointer(x, y, t0);
    assert_eq!(hover(&c, 1), Some(0));
    let (x, y) = on(&c, 0, 3);
    c.pointer(x, y, t0 + ms(10));
    assert!(c.deadline().is_some());
    assert!(c.pointer(700.0, 500.0, t0 + ms(20)));
    assert_eq!(c.deadline(), None);
    assert_eq!((c.path(), hover(&c, 0)), (&[1][..], Some(1)));
    // Into the submenu from a row of the root that asked for it to close: it stays.
    let (x, y) = on(&c, 0, 4);
    c.pointer(x, y, t0 + ms(30));
    let (x, y) = on(&c, 1, 1);
    c.pointer(x, y, t0 + ms(40));
    assert_eq!(c.deadline(), None);
    assert_eq!(
        (c.path(), hover(&c, 0), hover(&c, 1)),
        (&[1][..], Some(1), Some(1))
    );
}

#[test]
fn a_submenu_of_a_submenu_opens_and_its_menu_stays_lit() {
    let clock = ManualClock::new();
    let t0 = clock.now();
    let mut c = recent_open(t0);
    let (x, y) = on(&c, 1, 2);
    c.pointer(x, y, t0);
    assert!(c.tick(t0 + DELAY));
    place(&mut c, AREA);
    assert_eq!((c.depth(), c.path()), (3, &[1, 2][..]));
    let (x, y) = on(&c, 2, 0);
    c.pointer(x, y, t0 + ms(500));
    assert_eq!(
        (hover(&c, 0), hover(&c, 1), hover(&c, 2)),
        (Some(1), Some(2), Some(0))
    );
    // Back to a leaf of the root, straight down from the left: everything after the root goes.
    let (x, y) = on(&c, 0, 1);
    c.pointer(x, y, t0 + ms(600));
    let (x, y) = on(&c, 0, 4);
    c.pointer(x, y, t0 + ms(610));
    assert!(c.tick(t0 + ms(610) + DELAY));
    assert_eq!(c.depth(), 1);
}

// ---- clicks and keys ---------------------------------------------------------------------------

#[test]
fn a_click_opens_a_submenu_at_once_chooses_an_item_or_is_outside() {
    let mut c = opened();
    let (x, y) = on(&c, 0, 2);
    assert_eq!(c.click(x, y), Outcome::Changed);
    assert_eq!(c.path(), &[2][..]);
    assert_eq!(c.click(x, y), Outcome::Nothing);
    place(&mut c, AREA);
    let (x, y) = on(&c, 1, 1);
    assert_eq!(c.click(x, y), Outcome::Chosen(31));
    assert_eq!(
        c.path(),
        &[2][..],
        "an item chosen in a submenu leaves it open"
    );
    // An item chosen in the root closes the submenu, for a cascade kept open.
    let (x, y) = on(&c, 0, 4);
    assert_eq!(c.click(x, y), Outcome::Chosen(5));
    assert_eq!((c.depth(), hover(&c, 0)), (1, Some(4)));
    assert_eq!(c.click(790.0, 590.0), Outcome::Outside);
    // A separator's gap and a disabled row choose nothing.
    let root = c.menu(0).unwrap().row_rect(3);
    assert_eq!(c.click(root.x + 20.0, root.y - 4.0), Outcome::Nothing);
    c.menu_mut(0).unwrap().items[4].enabled = false;
    let (x, y) = on(&c, 0, 4);
    assert_eq!(c.click(x, y), Outcome::Nothing);
    // A disabled row with a submenu opens nothing.
    c.menu_mut(0).unwrap().items[1].enabled = false;
    let (x, y) = on(&c, 0, 1);
    assert_eq!(c.click(x, y), Outcome::Nothing);
    assert_eq!(c.depth(), 1);
}

#[test]
fn right_opens_and_moves_in_left_and_escape_close_back_to_the_menu() {
    let mut c = opened();
    assert_eq!(c.key(&KeyName::Down), Outcome::Changed);
    assert_eq!(hover(&c, 0), Some(0));
    // Right on New, which opens nothing: the application's to use (the next menu of a bar).
    assert_eq!(c.key(&KeyName::Right), Outcome::Ignored);
    assert_eq!(c.key(&KeyName::Left), Outcome::Ignored);
    c.key(&KeyName::Down);
    assert_eq!(c.key(&KeyName::Right), Outcome::Changed);
    assert_eq!((c.path(), c.focus(), hover(&c, 1)), (&[1][..], 1, Some(0)));
    c.key(&KeyName::End);
    assert_eq!(hover(&c, 1), Some(2));
    assert_eq!(c.key(&KeyName::Enter), Outcome::Changed);
    assert_eq!(
        (c.path(), c.focus(), hover(&c, 2)),
        (&[1, 2][..], 2, Some(0))
    );
    assert_eq!(c.key(&KeyName::Enter), Outcome::Chosen(220));
    assert_eq!(c.key(&KeyName::Left), Outcome::Changed);
    assert_eq!((c.depth(), c.focus(), hover(&c, 1)), (2, 1, Some(2)));
    assert_eq!(c.key(&KeyName::Escape), Outcome::Changed);
    assert_eq!((c.depth(), c.focus(), hover(&c, 0)), (1, 0, Some(1)));
    assert_eq!(c.key(&KeyName::Escape), Outcome::Close);
    // Home and End skip a disabled row; Tab is not the cascade's.
    c.menu_mut(0).unwrap().items[0].enabled = false;
    c.key(&KeyName::Home);
    assert_eq!(hover(&c, 0), Some(1));
    c.key(&KeyName::End);
    assert_eq!(hover(&c, 0), Some(4));
    assert_eq!(c.key(&KeyName::Space), Outcome::Chosen(5));
    assert_eq!(c.key(&KeyName::Tab), Outcome::Ignored);
}

#[test]
fn moving_in_a_menu_closes_its_submenu_opened_by_the_pointer() {
    let clock = ManualClock::new();
    let t0 = clock.now();
    let mut c = recent_open(t0);
    let (x, y) = on(&c, 0, 2);
    c.pointer(x, y, t0);
    assert!(c.deadline().is_some());
    // The keys are the root's; Down moves there and closes the submenu, and drops the delay.
    assert_eq!(c.key(&KeyName::Down), Outcome::Changed);
    assert_eq!((c.depth(), hover(&c, 0), c.deadline()), (1, Some(3), None));
    // Escape with a submenu open and the keys in the root closes the submenu.
    let mut c = recent_open(t0);
    assert_eq!(c.focus(), 0);
    assert_eq!(c.key(&KeyName::Escape), Outcome::Changed);
    assert_eq!(c.depth(), 1);
}

/// The cascade with Open recent open, the switch on, and the pointer resting on its row near
/// the right edge.
fn recent_open_keys(t0: Instant) -> Cascade<u8> {
    let mut c = Cascade::at(tree(), 100.0, 100.0).keys_cancel_wait(true);
    place(&mut c, AREA);
    let (x, y) = on(&c, 0, 1);
    assert_eq!(c.click(x, y), Outcome::Changed);
    place(&mut c, AREA);
    let r = c.menu(0).unwrap().row_rect(1);
    c.pointer(r.right() - 10.0, r.cy(), t0);
    assert_eq!(c.deadline(), None);
    c
}

#[test]
fn cancel_wait_drops_a_waiting_close_so_escape_closes_only_the_submenu() {
    let clock = ManualClock::new();
    let t0 = clock.now();
    let mut c = recent_open(t0);
    // Onto Print, which opens nothing: a close of the submenu waits.
    let (x, y) = on(&c, 0, 3);
    c.pointer(x, y, t0 + ms(50));
    assert_eq!(c.deadline(), Some(t0 + ms(50) + DELAY));
    c.cancel_wait();
    assert_eq!(c.deadline(), None);
    assert_eq!(
        (c.depth(), hover(&c, 0)),
        (2, Some(3)),
        "nothing was applied: the lit row is the pointer's, the submenu is open"
    );
    assert_eq!(c.key(&KeyName::Escape), Outcome::Changed);
    assert_eq!(
        (c.depth(), c.path(), hover(&c, 0)),
        (1, &[][..], Some(3)),
        "only the submenu closed; the root stays open"
    );
    assert_eq!(c.key(&KeyName::Escape), Outcome::Close);
    // Nothing waiting, nothing to drop.
    c.cancel_wait();
    assert_eq!(c.deadline(), None);
}

#[test]
fn cancel_wait_drops_a_waiting_close_from_the_padding_too() {
    let clock = ManualClock::new();
    let t0 = clock.now();
    let mut c = recent_open(t0);
    // The padding above the root's first row: inside the menu, on none of its rows.
    let (x, y) = {
        let m = c.menu(0).unwrap();
        (m.rect().x + 20.0, m.rect().y + 2.0)
    };
    assert!(!c.menu(0).unwrap().row_rect(0).contains(x, y));
    c.pointer(x, y, t0 + ms(50));
    assert_eq!((c.focus(), c.deadline()), (0, Some(t0 + ms(50) + DELAY)));
    c.cancel_wait();
    assert_eq!(c.deadline(), None);
    assert_eq!(c.depth(), 2, "nothing was applied");
    assert_eq!(c.key(&KeyName::Escape), Outcome::Changed);
    assert_eq!((c.depth(), c.path()), (1, &[][..]));
}

#[test]
fn with_the_switch_left_closes_only_the_submenu_after_a_cancelled_wait() {
    let clock = ManualClock::new();
    let t0 = clock.now();
    let mut c = recent_open_keys(t0);
    let (x, y) = on(&c, 0, 3);
    c.pointer(x, y, t0 + ms(50));
    assert!(c.deadline().is_some());
    c.cancel_wait();
    assert_eq!(c.deadline(), None, "cancel_wait dropped it");
    assert_eq!(c.key(&KeyName::Left), Outcome::Changed);
    assert_eq!(
        (c.depth(), c.focus()),
        (1, 0),
        "the root stays open and keeps the keys"
    );
    // With nothing open, Left is the menu bar's again.
    assert_eq!(c.key(&KeyName::Left), Outcome::Ignored);
}

#[test]
fn with_the_switch_escape_and_left_drop_the_wait_by_themselves() {
    let clock = ManualClock::new();
    let t0 = clock.now();
    let mut c = recent_open_keys(t0);
    let (x, y) = on(&c, 0, 3);
    c.pointer(x, y, t0 + ms(50));
    assert_eq!(c.key(&KeyName::Escape), Outcome::Changed);
    assert_eq!((c.depth(), hover(&c, 0)), (1, Some(3)));
    let mut c = recent_open_keys(t0);
    let (x, y) = on(&c, 0, 3);
    c.pointer(x, y, t0 + ms(50));
    assert_eq!(c.key(&KeyName::Left), Outcome::Changed);
    assert_eq!((c.depth(), c.focus()), (1, 0));
}

#[test]
fn without_the_wait_cancelled_the_pointer_change_runs_first_as_before() {
    let clock = ManualClock::new();
    let t0 = clock.now();
    // Escape: the waiting close runs first, so only the root is open and Escape is Close, which
    // takes the whole menu. This is the reported bug, kept as the old behaviour.
    let mut c = recent_open(t0);
    let (x, y) = on(&c, 0, 3);
    c.pointer(x, y, t0 + ms(50));
    assert_eq!(c.key(&KeyName::Escape), Outcome::Close);
    assert_eq!((c.depth(), hover(&c, 0)), (1, Some(3)));
    // Left: the same, and with the root alone open it is Ignored.
    let mut c = recent_open(t0);
    let (x, y) = on(&c, 0, 3);
    c.pointer(x, y, t0 + ms(50));
    assert_eq!(c.key(&KeyName::Left), Outcome::Ignored);
    assert_eq!(c.depth(), 1);
    // Every other key still makes the waiting change first, with the switch off and on.
    for mut c in [recent_open(t0), recent_open_keys(t0)] {
        let (x, y) = on(&c, 0, 3);
        c.pointer(x, y, t0 + ms(50));
        assert!(c.deadline().is_some());
        assert_eq!(c.key(&KeyName::Down), Outcome::Changed);
        assert_eq!((c.depth(), hover(&c, 0)), (1, Some(4)));
    }
}

#[test]
fn an_access_key_chooses_its_item_or_opens_its_submenu() {
    let mut c = Cascade::at(tree(), 100.0, 100.0).access_keys(true);
    place(&mut c, AREA);
    assert_eq!(c.key(&KeyName::Char('r')), Outcome::Changed);
    assert_eq!((c.path(), c.focus(), hover(&c, 1)), (&[1][..], 1, Some(0)));
    assert_eq!(c.key(&KeyName::Char('z')), Outcome::Ignored);
    c.key(&KeyName::Escape);
    assert_eq!(c.key(&KeyName::Char('Q')), Outcome::Chosen(5));
    // Off, no letter is a key.
    let mut c = opened();
    assert_eq!(c.key(&KeyName::Char('q')), Outcome::Ignored);
    // Every menu reads them, with their underlines hidden too.
    let mut c = Cascade::at(tree(), 100.0, 100.0).access_keys(true);
    c.set_access_visible(false);
    c.key(&KeyName::Char('e'));
    assert_eq!(c.path(), &[2][..]);
}

#[test]
fn the_wheel_scrolls_the_menu_under_the_pointer() {
    let many: Vec<Node<u8>> = (0..40)
        .map(|i| Item::new(i, format!("Item {i}")).into())
        .collect();
    let mut c = Cascade::at(many, 100.0, 100.0);
    place(&mut c, AREA);
    assert!(c.menu(0).unwrap().scrolls());
    assert!(c.wheel(150.0, 200.0, 64.0));
    assert_eq!(c.menu(0).unwrap().scroll(), 64.0);
    assert!(!c.wheel(700.0, 200.0, 64.0));
    assert!(!c.wheel(150.0, 200.0, f32::NAN));
}

// ---- placement of the cascade ------------------------------------------------------------------

#[test]
fn submenus_flip_at_the_right_edge_and_carry_on_to_the_left() {
    // The root fits at x 560 but its submenu does not fit beyond it: it opens to the left,
    // and its own submenu carries on leftwards.
    let mut c = Cascade::at(tree(), 560.0, 100.0);
    place(&mut c, AREA);
    for key in [KeyName::Down, KeyName::Down, KeyName::Right, KeyName::End] {
        c.key(&key);
    }
    place(&mut c, AREA);
    c.key(&KeyName::Right);
    place(&mut c, AREA);
    let (root, sub, subsub) = (
        c.menu(0).unwrap().rect(),
        c.menu(1).unwrap().rect(),
        c.menu(2).unwrap().rect(),
    );
    assert!(!c.menu(0).unwrap().opened_left());
    assert_eq!(sub.right(), root.x - GAP);
    assert_eq!(subsub.right(), sub.x - GAP);
    assert!(c.menu(2).unwrap().opened_left());
    // A root flipped left at the right edge opens its submenus leftwards from the start.
    let mut c = Cascade::at(tree(), 790.0, 100.0);
    place(&mut c, AREA);
    assert!(c.menu(0).unwrap().opened_left());
    for key in [KeyName::Down, KeyName::Down, KeyName::Right] {
        c.key(&key);
    }
    place(&mut c, AREA);
    assert_eq!(
        c.menu(1).unwrap().rect().right(),
        c.menu(0).unwrap().rect().x - GAP
    );
}

#[test]
fn a_submenu_slides_up_at_the_bottom_and_a_context_menu_stays_in_at_every_corner() {
    // In a short area, Export's submenu of six rows has no room below its row: it slides up.
    let area = Rect::new(0.0, 0.0, 800.0, 260.0);
    let mut nodes = tree();
    nodes[2].children = (0..6)
        .map(|i| Item::new(40 + i, format!("Format {i}")).into())
        .collect();
    let mut c = Cascade::at(nodes, 100.0, 8.0);
    place(&mut c, area);
    let (x, y) = on(&c, 0, 2);
    c.click(x, y);
    place(&mut c, area);
    let sub = c.menu(1).unwrap().rect();
    assert_eq!(sub.bottom(), 256.0);
    assert!(sub.y < c.menu(0).unwrap().row_rect(2).y - PAD);
    // A context menu at each corner of the window stays inside it.
    for (x, y) in [(2.0, 2.0), (798.0, 2.0), (2.0, 598.0), (798.0, 598.0)] {
        let mut c = Cascade::at(tree(), x, y);
        place(&mut c, AREA);
        assert!(inside(c.menu(0).unwrap().rect(), room(AREA)), "at {x},{y}");
    }
}

// ---- drawing -----------------------------------------------------------------------------------

fn drawn(c: &mut Cascade<u8>, how: impl FnOnce(&mut Cascade<u8>, &mut Painter)) -> Bitmap {
    shot(|p| how(c, p))
}

#[test]
fn the_cascade_draws_its_menus_one_over_another_and_at_rest_as_draw_in() {
    let clock = ManualClock::new();
    let t0 = clock.now();
    let mut c = recent_open(t0);
    let plain = drawn(&mut c, |c, p| c.draw_in(p, &LIGHT, AREA));
    let there = drawn(&mut c, |c, p| c.draw_in_presence(p, &LIGHT, AREA, 1.0));
    assert!(plain.pixels == there.pixels);
    // By hand: the root, then the submenu over it.
    let mut root =
        Menu::new(c.menu(0).unwrap().items.clone(), 100.0, 100.0, false).submenu_rows([1, 2]);
    root.hover = Some(1);
    let mut sub = Menu::new(c.menu(1).unwrap().items.clone(), 0.0, 0.0, false)
        .submenu_rows([2])
        .beside(Rect::new(100.0, 137.0, 180.0, 32.0), false);
    sub.hover = c.menu(1).unwrap().hover;
    let by_hand = shot(|p| {
        root.draw_in(p, &LIGHT, AREA);
        sub.draw_in(p, &LIGHT, AREA);
    });
    assert!(plain.pixels == by_hand.pixels);
    // Gone, nothing.
    let gone = drawn(&mut c, |c, p| c.draw_in_presence(p, &LIGHT, AREA, 0.0));
    let blank = shot(|_| {});
    assert!(gone.pixels == blank.pixels);
}

#[test]
fn a_submenu_that_has_just_opened_enters_and_then_draws_as_draw_in() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let mut c = recent_open(clock.now());
    // The first frame shows the root alone with the row lit: the submenu is at 0.
    let mut alone = opened();
    alone.menu_mut(0).unwrap().hover = Some(1);
    let root_only = drawn(&mut alone, |c, p| c.draw_in(p, &LIGHT, AREA));
    let plain = drawn(&mut c, |c, p| c.draw_in(p, &LIGHT, AREA));
    let frame = |c: &mut Cascade<u8>, anim: &mut Animator| {
        anim.begin_frame(clock.now());
        let b = drawn(c, |c, p| {
            c.draw_animated(p, &LIGHT, AREA, 1.0, anim, "menu")
        });
        (b, anim.end_frame())
    };
    let (first, next) = frame(&mut c, &mut anim);
    assert!(first.pixels == root_only.pixels);
    assert!(next.is_some(), "the entrance asks for frames");
    clock.advance_ms(40);
    let (mid, _) = frame(&mut c, &mut anim);
    assert!(mid.pixels != plain.pixels && mid.pixels != root_only.pixels);
    // Frames a sixtieth apart until the entrance has played.
    let mut frames = 0;
    let rest = loop {
        clock.advance_ms(17);
        let (b, next) = frame(&mut c, &mut anim);
        frames += 1;
        if next.is_none() || frames > 60 {
            break b;
        }
    };
    assert!(frames < 20, "{frames} frames");
    assert!(rest.pixels == plain.pixels);
}

// ---- the state machine against random input ----------------------------------------------------

#[test]
fn random_pointers_keys_clicks_and_times_keep_the_cascade_whole() {
    let clock = ManualClock::new();
    let mut rng = Lcg(3);
    let keys = [
        KeyName::Up,
        KeyName::Down,
        KeyName::Left,
        KeyName::Right,
        KeyName::Enter,
        KeyName::Escape,
        KeyName::Home,
        KeyName::End,
        KeyName::Char('e'),
        KeyName::Char('r'),
    ];
    let mut c = Cascade::at(tree(), 100.0, 100.0)
        .access_keys(true)
        .with_delay(ms(100));
    place(&mut c, AREA);
    let mut deepest = 0;
    for step in 0..20_000 {
        clock.advance_ms(rng.next() % 60);
        let now = clock.now();
        match rng.next() % 10 {
            0..=4 => {
                c.pointer(rng.below(700), rng.below(500), now);
            }
            5 => {
                c.tick(now);
            }
            6 => {
                if let Outcome::Chosen(id) = c.click(rng.below(700), rng.below(500)) {
                    assert!(
                        ![2, 3, 22].contains(&id),
                        "a row with a submenu is never chosen"
                    );
                }
            }
            7 => {
                c.key(&keys[(rng.next() % keys.len() as u64) as usize]);
            }
            8 => {
                c.wheel(rng.below(700), rng.below(500), rng.below(100) - 50.0);
            }
            _ => {
                let v = [f32::NAN, f32::INFINITY, -1e9][(rng.next() % 3) as usize];
                c.pointer(v, rng.below(500), now);
                c.click(rng.below(700), v);
            }
        }
        place(&mut c, AREA);
        deepest = deepest.max(c.depth());
        assert_eq!(c.depth(), c.path().len() + 1, "step {step}");
        assert!(c.focus() < c.depth());
        for (k, &row) in c.path().iter().enumerate() {
            assert!(c.opens(k, row), "step {step}: level {k} row {row}");
            // Lit, unless the pointer is on another row of that menu and the change waits.
            if c.pending.is_none_or(|p| p.level != k) {
                assert_eq!(
                    hover(&c, k),
                    Some(row),
                    "step {step}: the opening row is lit"
                );
            }
        }
        if let Some(due) = c.deadline() {
            assert!(due <= now + ms(100));
        }
    }
    assert_eq!(deepest, 3, "the walk reached the deepest submenu");
}

// ---- the sheet ---------------------------------------------------------------------------------

/// `cargo test -p mdshared-ui submenu_sheet -- --ignored`, then open target/specimen/submenu.png
/// and submenu-150.png.
#[test]
#[ignore]
fn submenu_sheet() {
    const W: f32 = 820.0;
    const H: f32 = 430.0;
    for (scale, file) in [(1.0, "submenu.png"), (1.5, "submenu-150.png")] {
        let mut bmp = Bitmap::new((W * scale) as u32, (2.0 * H * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (half, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
            let oy = half as f32 * H;
            p.fill(Rect::new(0.0, oy, W, H), pal.page);
            let o = p.offset(0.0, oy);
            let area = Rect::new(0.0, 0.0, W, H);
            let note = |p: &mut Painter, x: f32, text: &str| {
                p.text(Face::Ui, 11.0, x, 14.0, text, pal.muted, 340.0);
            };
            note(&mut p, 16.0, "from a button, three deep, by the keys");
            note(&mut p, 470.0, "a right-click at the bottom right corner");
            // A drop-down under a button, three menus open.
            let button = Rect::new(16.0, 26.0, 70.0, 26.0);
            p.rbox(button, 5.0, pal.hover);
            p.text(Face::Ui, 13.0, 28.0, button.cy(), "File", pal.text, 60.0);
            let mut nodes = tree();
            nodes[0].item = Item::new(1, "&New").hint("Ctrl+N");
            nodes[3].item = Item::new(4, "&Print…").hint("Ctrl+P").sep();
            let mut c = Cascade::new(nodes, button).access_keys(true);
            c.place_in(&mut p, area);
            c.key(&KeyName::Char('r'));
            c.key(&KeyName::End);
            c.key(&KeyName::Right);
            c.draw_in(&mut p, pal, area);
            // A context menu at the corner, flipped above and left, its submenu on the left.
            let point = (W - 30.0, H - 20.0);
            p.fill(
                Rect::new(point.0 - 1.0, point.1 - 1.0, 3.0, 3.0),
                pal.accent,
            );
            let edit = vec![
                Node::new(Item::new(1, "Cut").hint("Ctrl+X")),
                Node::new(Item::new(2, "Copy").hint("Ctrl+C")),
                Node::new(Item::new(3, "Paste").hint("Ctrl+V").enabled(false)),
                Node::submenu(
                    Item::new(4, "Transform").sep(),
                    vec![
                        Node::new(Item::new(40, "UPPER CASE")),
                        Node::new(Item::new(41, "lower case")),
                        Node::new(Item::new(42, "Title Case")),
                    ],
                ),
                Node::new(Item::new(5, "Select all").hint("Ctrl+A")),
            ];
            let mut c = Cascade::at(edit, point.0, point.1);
            c.place_in(&mut p, area);
            let (x, y) = on(&c, 0, 3);
            c.click(x, y);
            c.place_in(&mut p, area);
            let (x, y) = on(&c, 1, 1);
            c.pointer(x, y, Instant::now());
            c.draw_in(&mut p, pal, area);
            p.restore_offset(o);
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join(file)).unwrap();
    }
}

#[test]
fn a_cascade_with_a_look_gives_it_to_every_menu_and_places_submenus_by_it() {
    let mut c = Cascade::at(tree(), 100.0, 100.0).with_look(crate::menu::Look::FLAT);
    place(&mut c, AREA);
    assert_eq!(c.menu(0).unwrap().look(), crate::menu::Look::FLAT);
    let (x, y) = on(&c, 0, 1);
    assert_eq!(c.click(x, y), Outcome::Changed);
    place(&mut c, AREA);
    let (root, sub) = (c.menu(0).unwrap(), c.menu(1).unwrap());
    assert_eq!(sub.look(), crate::menu::Look::FLAT);
    // Against its menu with no gap, its first row level with the row that opened it.
    assert_eq!(sub.rect().x, root.rect().right());
    assert_eq!(sub.row_rect(0).y, root.row_rect(1).y);
    assert_eq!(root.row_rect(1).h, crate::menu::Look::FLAT.row);
    // A cascade given no look is the rounded one, and `beside` is `beside_with` in it.
    assert_eq!(opened().menu(0).unwrap().look(), crate::menu::Look::ROUNDED);
    let item = Rect::new(100.0, 140.0, 180.0, 32.0);
    for leftward in [false, true] {
        assert_eq!(
            beside(150.0, 200.0, item, AREA, leftward),
            beside_with(
                150.0,
                200.0,
                item,
                AREA,
                leftward,
                &crate::menu::Look::ROUNDED
            )
        );
    }
}

// ---- the soft shadow fades with the cascade -----------------------------------------------------

/// The rounded look with the soft shadow at its strongest, so its far reach shows.
fn soft_look() -> Look {
    let mut look = Look::ROUNDED;
    look.soft_shadow = true;
    look.shadow_strength = 4.0;
    look
}

/// [`recent_open`] in `look`.
fn recent_open_in(look: Look, t0: Instant) -> Cascade<u8> {
    let mut c = Cascade::at(tree(), 100.0, 100.0).with_look(look);
    place(&mut c, AREA);
    let (x, y) = on(&c, 0, 1);
    assert_eq!(c.click(x, y), Outcome::Changed);
    place(&mut c, AREA);
    let r = c.menu(0).unwrap().row_rect(1);
    c.pointer(r.right() - 10.0, r.cy(), t0);
    c
}

/// How much darker than the page the pixel at (x, y) is, 0 to 1.
fn dark_at(b: &Bitmap, x: f32, y: f32) -> f32 {
    let i = (y as usize * b.width as usize + x as usize) * 4;
    1.0 - b.pixels[i + 1] as f32 / LIGHT.page[1] as f32
}

#[test]
fn a_soft_cascade_that_fades_fades_its_whole_shadow() {
    // 21 px left of the root menu, at its middle: past the toolkit shadow's 18 px, inside the
    // soft one's reach, and level with a fade's drop, which is vertical.
    let clock = ManualClock::new();
    let mut c = recent_open_in(soft_look(), clock.now());
    let root = c.menu(0).unwrap().rect();
    let (x, y) = (root.x - 21.0, root.cy());
    let full = dark_at(
        &drawn(&mut c, |c, p| c.draw_in_presence(p, &LIGHT, AREA, 1.0)),
        x,
        y,
    );
    assert!(full > 0.02, "the shadow reaches there: {full}");
    let half = dark_at(
        &drawn(&mut c, |c, p| c.draw_in_presence(p, &LIGHT, AREA, 0.5)),
        x,
        y,
    );
    assert!(half < 0.7 * full, "{half} at a half, {full} whole");
    // And the same through draw_animated, once the submenu's entrance has played.
    let mut anim = Animator::new();
    anim.begin_frame(clock.now());
    let _ = drawn(&mut c, |c, p| {
        c.draw_animated(p, &LIGHT, AREA, 1.0, &mut anim, "m")
    });
    anim.end_frame();
    clock.advance_ms(2000);
    anim.begin_frame(clock.now());
    let half = drawn(&mut c, |c, p| {
        c.draw_animated(p, &LIGHT, AREA, 0.5, &mut anim, "m")
    });
    anim.end_frame();
    let half = dark_at(&half, x, y);
    assert!(
        half < 0.7 * full,
        "animated: {half} at a half, {full} whole"
    );
}

#[test]
fn a_soft_submenu_that_enters_fades_its_whole_shadow_in() {
    // 20 px below the submenu's foot, at its middle: past the old 18 px, and out of the way of
    // its sideways slide, so only the fade can lighten it while it enters.
    let clock = ManualClock::new();
    let mut c = recent_open_in(soft_look(), clock.now());
    let rest = drawn(&mut c, |c, p| c.draw_in(p, &LIGHT, AREA));
    let sub = c.menu(1).unwrap().rect();
    let (x, y) = (sub.cx(), sub.bottom() + 20.0);
    assert!(AREA.contains(x, y), "the probe is in the area");
    let full = dark_at(&rest, x, y);
    assert!(full > 0.015, "the shadow reaches there: {full}");
    let mut anim = Animator::new();
    anim.begin_frame(clock.now());
    let _ = drawn(&mut c, |c, p| {
        c.draw_animated(p, &LIGHT, AREA, 1.0, &mut anim, "m")
    });
    anim.end_frame();
    clock.advance_ms(10);
    anim.begin_frame(clock.now());
    let mid = drawn(&mut c, |c, p| {
        c.draw_animated(p, &LIGHT, AREA, 1.0, &mut anim, "m")
    });
    anim.end_frame();
    let mid = dark_at(&mid, x, y);
    assert!(mid < 0.6 * full, "entering {mid}, at rest {full}");
}

#[test]
fn a_cascade_without_the_soft_shadow_fades_18_px_round_its_menus_as_before() {
    // The toolkit's shadow: the fade takes the menus' bounds and 18 px, pixel for pixel.
    let clock = ManualClock::new();
    let mut c = recent_open(clock.now());
    let half = drawn(&mut c, |c, p| c.draw_in_presence(p, &LIGHT, AREA, 0.5));
    let (a, b) = (c.menu(0).unwrap().rect(), c.menu(1).unwrap().rect());
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let bounds = Rect::new(
        x0,
        y0,
        a.right().max(b.right()) - x0,
        a.bottom().max(b.bottom()) - y0,
    )
    .inset(-18.0);
    let drop = -MENU_DROP;
    let by_hand = drawn(&mut c, |c, p| {
        pop(p, bounds, 0.5, (0.0, drop), |p| {
            for k in 0..2 {
                c.menu_mut(k).unwrap().draw_in(p, &LIGHT, AREA);
            }
        })
    });
    assert!(half.pixels == by_hand.pixels);
}
