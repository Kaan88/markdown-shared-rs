use super::*;
use crate::check::{Tick, draw_tick};
use crate::focus::draw_ring;
use crate::gfx::{Face, Gfx, Rgb};
use crate::menu::Menu;
use crate::overlay::draw_dialog_button;
use crate::submenu::MARGIN;
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

const AREA: Rect = Rect::new(0.0, 0.0, 800.0, 600.0);

/// Content 200 by 100: a card 232 by 132.
const CONTENT: (f32, f32) = (200.0, 100.0);

fn placed(anchor: Rect) -> Flyout {
    let mut f = Flyout::new(anchor, CONTENT);
    f.place_in(AREA);
    f
}

fn inside(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

/// The area less the margin a placed card keeps from its edges.
fn room(area: Rect) -> Rect {
    area.inset(MARGIN)
}

// ---- placement --------------------------------------------------------------------------------

#[test]
fn a_flyout_opens_below_its_anchor_a_gap_away_with_its_left_edge_at_the_anchors() {
    let anchor = Rect::new(100.0, 100.0, 80.0, 30.0);
    let f = placed(anchor);
    assert_eq!(f.card_size(), (232.0, 132.0));
    assert_eq!(f.rect(), Rect::new(100.0, 130.0 + GAP, 232.0, 132.0));
    assert!(!f.opened_above() && !f.opened_left());
}

#[test]
fn place_in_returns_the_rectangle_it_keeps() {
    let mut f = Flyout::new(Rect::new(100.0, 100.0, 80.0, 30.0), CONTENT);
    assert_eq!(f.rect(), Rect::default());
    let r = f.place_in(AREA);
    assert_eq!(r, f.rect());
}

#[test]
fn near_the_bottom_edge_the_card_flips_above_the_anchor() {
    let anchor = Rect::new(100.0, 500.0, 80.0, 30.0);
    let f = placed(anchor);
    assert!(f.opened_above() && !f.opened_left());
    assert_eq!(f.rect().bottom(), anchor.y - GAP);
    assert_eq!(f.rect().x, anchor.x);
    assert!(inside(f.rect(), room(AREA)));
}

#[test]
fn near_the_right_edge_the_card_goes_left_its_right_edge_at_the_anchors() {
    let anchor = Rect::new(700.0, 100.0, 80.0, 30.0);
    let f = placed(anchor);
    assert!(f.opened_left() && !f.opened_above());
    assert_eq!(f.rect().right(), anchor.right());
    assert_eq!(f.rect().y, anchor.bottom() + GAP);
    assert!(inside(f.rect(), room(AREA)));
}

#[test]
fn a_card_that_fits_on_neither_side_slides_inside_the_area() {
    // Too wide to go right of the anchor or left of it in a 400 px area: pushed in.
    let area = Rect::new(0.0, 0.0, 400.0, 600.0);
    let mut f = Flyout::new(Rect::new(200.0, 100.0, 20.0, 30.0), CONTENT);
    let r = f.place_in(area);
    assert_eq!(r.x, MARGIN);
    assert!(inside(r, room(area)));
    assert_eq!(r.y, 130.0 + GAP);
}

#[test]
fn at_each_corner_the_card_stays_on_screen_and_off_its_anchor() {
    // (anchor, above, left)
    let corners = [
        (Rect::new(0.0, 0.0, 20.0, 20.0), false, false),
        (Rect::new(780.0, 0.0, 20.0, 20.0), false, true),
        (Rect::new(0.0, 580.0, 20.0, 20.0), true, false),
        (Rect::new(780.0, 580.0, 20.0, 20.0), true, true),
    ];
    for (anchor, above, left) in corners {
        let f = placed(anchor);
        assert!(inside(f.rect(), room(AREA)), "{anchor:?}: {:?}", f.rect());
        assert!(!overlaps(f.rect(), anchor), "{anchor:?}: {:?}", f.rect());
        assert_eq!(f.opened_above(), above, "{anchor:?}");
        assert_eq!(f.opened_left(), left, "{anchor:?}");
        assert_eq!((f.rect().w, f.rect().h), f.card_size());
    }
}

#[test]
fn a_flyout_that_prefers_above_opens_above_and_flips_below_near_the_top() {
    let mid = Rect::new(100.0, 300.0, 80.0, 30.0);
    let mut f = Flyout::new(mid, CONTENT).side(Side::Above);
    let r = f.place_in(AREA);
    assert!(f.opened_above());
    assert_eq!(r, Rect::new(100.0, 300.0 - GAP - 132.0, 232.0, 132.0));

    let top = Rect::new(100.0, 40.0, 80.0, 30.0);
    let mut f = Flyout::new(top, CONTENT).side(Side::Above);
    let r = f.place_in(AREA);
    assert!(!f.opened_above());
    assert_eq!(r.y, top.bottom() + GAP);
    assert_eq!(Side::default(), Side::Below);
}

#[test]
fn preferring_above_a_card_that_fits_on_neither_side_goes_where_there_is_more_room() {
    // A 200 px high area, a card 132 high: neither above nor below the anchor has room.
    let area = Rect::new(0.0, 0.0, 800.0, 200.0);
    // (anchor, above): 82 px below the grown anchor and 72 above it; then 62 below and 92 above.
    for (anchor, above) in [
        (Rect::new(100.0, 80.0, 80.0, 30.0), false),
        (Rect::new(100.0, 100.0, 80.0, 30.0), true),
    ] {
        let mut up = Flyout::new(anchor, CONTENT).side(Side::Above);
        let r = up.place_in(area);
        assert_eq!(up.opened_above(), above, "{anchor:?}");
        assert!(inside(r, room(area)), "{anchor:?}: {r:?}");
        assert_eq!(r.h, 132.0);
        assert!(overlaps(r, anchor), "pushed in over the anchor");
        // Pushed against the edge of the side it went to.
        if above {
            assert_eq!(r.y, MARGIN);
        } else {
            assert_eq!(r.bottom(), area.bottom() - MARGIN);
        }
        // Where nothing fits, the preferred side does not matter.
        let mut down = Flyout::new(anchor, CONTENT);
        assert_eq!(down.place_in(area), r, "{anchor:?}");
        assert_eq!(down.opened_above(), above);
    }
}

#[test]
fn the_gap_and_the_padding_can_be_set() {
    let anchor = Rect::new(100.0, 100.0, 80.0, 30.0);
    let mut f = Flyout::new(anchor, CONTENT).gap(10.0).padding(8.0);
    assert_eq!(f.card_size(), (216.0, 116.0));
    let r = f.place_in(AREA);
    assert_eq!(r, Rect::new(100.0, 140.0, 216.0, 116.0));
    assert_eq!(f.content_rect(), Rect::new(108.0, 148.0, 200.0, 100.0));
}

#[test]
fn a_moved_anchor_and_a_new_size_take_effect_at_the_next_placement() {
    let mut f = placed(Rect::new(100.0, 100.0, 80.0, 30.0));
    f.set_anchor(Rect::new(300.0, 200.0, 40.0, 20.0));
    f.set_content_size((100.0, 50.0));
    assert_eq!(f.anchor(), Rect::new(300.0, 200.0, 40.0, 20.0));
    assert_eq!(f.rect().x, 100.0);
    let r = f.place_in(AREA);
    assert_eq!(r, Rect::new(300.0, 220.0 + GAP, 132.0, 82.0));
}

/// A small LCG, for anchors all over the area.
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: u32) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) % n as u64) as f32
    }

    /// A number in `0..n` with a fraction, in steps of 1/16 (halves among them).
    fn fraction(&mut self, n: u32) -> f32 {
        self.below(n * 16) / 16.0
    }
}

#[test]
fn anywhere_the_card_is_on_its_side_when_it_fits_there_else_the_other_else_inside() {
    // The rule worked out again plainly, for anchors and sizes at random in the area, whole and
    // fractional. The card's edges are rounded, so each lies within half a px of where the rule
    // puts it, and never outside the margin.
    let mut rng = Lcg(7);
    let room = room(AREA);
    let near = |a: f32, b: f32| (a - b).abs() <= 0.5;
    for side in [Side::Below, Side::Above] {
        for i in 0..4000 {
            let whole = i % 2 == 0;
            let mut n = |max: u32| {
                if whole {
                    rng.below(max)
                } else {
                    rng.fraction(max)
                }
            };
            let anchor = Rect::new(n(790), n(590), n(120), n(60));
            let content = (n(400), n(300));
            let mut f = Flyout::new(anchor, content).side(side);
            let r = f.place_in(AREA);
            let (w, h) = f.card_size();
            assert!(inside(r, room), "{anchor:?} {content:?}: {r:?}");
            assert!(r.x.fract() == 0.0 && r.right().fract() == 0.0, "{r:?}");
            assert!(r.y.fract() == 0.0 && r.bottom().fract() == 0.0, "{r:?}");
            assert!((r.w - w).abs() <= 1.0 && (r.h - h).abs() <= 1.0, "{r:?}");
            let fits = |start: f32, len: f32, lo: f32, hi: f32| start >= lo && start + len <= hi;
            let below = fits(anchor.bottom() + GAP, h, room.y, room.bottom());
            let above = fits(anchor.y - GAP - h, h, room.y, room.bottom());
            let (first, second) = match side {
                Side::Below => (below, above),
                Side::Above => (above, below),
            };
            let wants_above = side == Side::Above;
            if first || second {
                let up = if first { wants_above } else { !wants_above };
                assert_eq!(f.opened_above(), up, "{anchor:?} {content:?}");
                if up {
                    assert!(near(r.bottom(), anchor.y - GAP), "{anchor:?}: {r:?}");
                } else {
                    assert!(near(r.y, anchor.bottom() + GAP), "{anchor:?}: {r:?}");
                }
            }
            if fits(anchor.x, w, room.x, room.right()) {
                assert!(near(r.x, anchor.x), "{anchor:?}: {r:?}");
            } else if fits(anchor.right() - w, w, room.x, room.right()) {
                assert!(near(r.right(), anchor.right()), "{anchor:?}: {r:?}");
            }
        }
    }
}

#[test]
fn the_cards_edges_are_rounded_so_one_on_the_margin_stays_on_it() {
    // The card is 196.5 wide and fits from 599.5 to 796, the margin, exactly. Rounding the
    // place and the size apart would give 600 + 197 = 797, past the margin.
    let mut f = Flyout::new(Rect::new(599.5, 100.0, 0.0, 30.0), (164.5, 50.0));
    let r = f.place_in(AREA);
    assert!(!f.opened_left());
    assert_eq!(r, Rect::new(600.0, 134.0, 196.0, 82.0));
    assert_eq!(r.right(), AREA.right() - MARGIN);
    assert!(inside(r, room(AREA)));
}

#[test]
fn nonsense_sizes_are_none_and_a_nonsense_anchor_is_the_areas_corner() {
    let f = Flyout::new(Rect::new(10.0, 10.0, 10.0, 10.0), (f32::NAN, -5.0));
    assert_eq!(f.card_size(), (2.0 * PADDING, 2.0 * PADDING));
    let f = Flyout::new(Rect::default(), (f32::INFINITY, 10.0))
        .gap(f32::NAN)
        .padding(-3.0);
    assert_eq!(f.card_size(), (0.0, 10.0));

    let mut f = Flyout::new(Rect::new(f32::NAN, 5.0, 10.0, 10.0), CONTENT).gap(0.0);
    let r = f.place_in(Rect::new(50.0, 60.0, 500.0, 400.0));
    assert_eq!((r.x, r.y), (54.0, 64.0));

    // An area of no size or not a number: no panic, and the content is never of negative size.
    for area in [
        Rect::default(),
        Rect::new(0.0, 0.0, f32::NAN, f32::NAN),
        Rect::new(f32::INFINITY, 0.0, 10.0, 10.0),
    ] {
        let mut f = Flyout::new(Rect::new(10.0, 10.0, 10.0, 10.0), CONTENT);
        f.place_in(area);
        let c = f.content_rect();
        assert!(c.w >= 0.0 || c.w.is_nan());
        assert!(c.h >= 0.0 || c.h.is_nan());
    }
}

// ---- content -----------------------------------------------------------------------------------

#[test]
fn the_content_rect_is_the_card_less_its_padding() {
    let f = placed(Rect::new(100.0, 100.0, 80.0, 30.0));
    let c = f.content_rect();
    assert_eq!(c, Rect::new(116.0, 134.0 + PADDING, 200.0, 100.0));
    assert!(inside(c, f.rect()));
    for anchor in [
        Rect::new(780.0, 580.0, 20.0, 20.0),
        Rect::new(0.0, 580.0, 20.0, 20.0),
    ] {
        let f = placed(anchor);
        assert!(inside(f.content_rect(), f.rect()));
        assert_eq!((f.content_rect().w, f.content_rect().h), CONTENT);
    }
}

#[test]
fn a_card_cut_by_a_small_area_cuts_its_content_too() {
    let area = Rect::new(0.0, 0.0, 120.0, 90.0);
    let mut f = Flyout::new(Rect::new(10.0, 10.0, 20.0, 20.0), CONTENT);
    let r = f.place_in(area);
    assert!(inside(r, room(area)));
    let c = f.content_rect();
    assert!(inside(c, r));
    assert!(c.w < CONTENT.0 && c.h < CONTENT.1);
}

// ---- pointer and keys --------------------------------------------------------------------------

#[test]
fn a_press_on_the_card_is_inside_and_one_away_from_it_is_outside() {
    let f = placed(Rect::new(100.0, 100.0, 80.0, 30.0));
    let r = f.rect();
    assert_eq!(f.pointer_down(r.cx(), r.cy()), Outcome::Inside);
    assert_eq!(f.pointer_down(r.x, r.y), Outcome::Inside);
    assert!(f.contains(r.x, r.y));
    assert_eq!(f.pointer_down(r.right(), r.cy()), Outcome::Outside);
    assert_eq!(f.pointer_down(600.0, 500.0), Outcome::Outside);
    assert_eq!(f.pointer_down(-10.0, -10.0), Outcome::Outside);
    // The gap between the anchor and the card is neither.
    assert_eq!(f.pointer_down(120.0, 131.0), Outcome::Outside);
}

#[test]
fn a_press_on_the_anchor_is_its_own_outcome_so_it_closes_and_does_not_reopen() {
    let anchor = Rect::new(100.0, 100.0, 80.0, 30.0);
    let f = placed(anchor);
    assert_eq!(f.pointer_down(anchor.cx(), anchor.cy()), Outcome::Anchor);
    assert_eq!(f.pointer_down(anchor.x, anchor.y), Outcome::Anchor);
    // Before the first placement the card is nowhere; the anchor is still the anchor.
    let unplaced = Flyout::new(anchor, CONTENT);
    assert_eq!(
        unplaced.pointer_down(anchor.cx(), anchor.cy()),
        Outcome::Anchor
    );
    assert_eq!(unplaced.pointer_down(300.0, 300.0), Outcome::Outside);
}

#[test]
fn where_the_card_was_pushed_over_its_anchor_a_press_there_is_the_cards() {
    // A card that fits neither above nor below is pushed over the anchor.
    let area = Rect::new(0.0, 0.0, 800.0, 200.0);
    let anchor = Rect::new(100.0, 80.0, 80.0, 30.0);
    let mut f = Flyout::new(anchor, CONTENT);
    let r = f.place_in(area);
    assert!(overlaps(r, anchor));
    let (x, y) = (anchor.cx(), r.y.max(anchor.y) + 1.0);
    assert!(anchor.contains(x, y) && r.contains(x, y));
    assert_eq!(f.pointer_down(x, y), Outcome::Inside);
}

#[test]
fn a_press_that_is_not_a_number_is_unused() {
    let f = placed(Rect::new(100.0, 100.0, 80.0, 30.0));
    for (x, y) in [
        (f32::NAN, 150.0),
        (150.0, f32::NAN),
        (f32::INFINITY, 150.0),
        (150.0, f32::NEG_INFINITY),
    ] {
        assert_eq!(f.pointer_down(x, y), Outcome::Unused);
    }
}

#[test]
fn escape_closes_and_every_other_key_is_the_contents() {
    let f = placed(Rect::new(100.0, 100.0, 80.0, 30.0));
    assert_eq!(f.key(&KeyName::Escape), Outcome::Close);
    for key in [
        KeyName::Tab,
        KeyName::Enter,
        KeyName::Space,
        KeyName::Down,
        KeyName::Char('a'),
        KeyName::F(4),
    ] {
        assert_eq!(f.key(&key), Outcome::Unused, "{key:?}");
    }
}

// ---- focus -------------------------------------------------------------------------------------

const BUTTON: u32 = 1;
const EDITOR: u32 = 2;
const SCOPE: u32 = 10;
const CHECK: u32 = 11;
const OK: u32 = 12;

fn frame(focus: &mut Focus<u32>, open: bool) {
    focus.begin_frame();
    focus.add(BUTTON);
    focus.add(EDITOR);
    if open {
        focus.add_in(SCOPE, CHECK);
        focus.add_in(SCOPE, OK);
    }
    focus.end_frame();
}

#[test]
fn the_flyout_is_a_focus_scope_tab_stays_in_it_and_the_anchor_that_had_the_focus_gets_it_back() {
    let mut focus = Focus::new();
    frame(&mut focus, false);
    assert!(focus.tab(false));
    assert_eq!(focus.focused(), Some(BUTTON));
    assert!(focus.ring_visible(BUTTON));

    // Enter on the button opens the flyout: the content's first widget takes the focus.
    let fly = placed(Rect::new(100.0, 100.0, 80.0, 30.0));
    open_scope(&mut focus, SCOPE, None);
    frame(&mut focus, true);
    assert_eq!(focus.scope(), Some(SCOPE));
    assert_eq!(focus.focused(), Some(CHECK));
    assert!(focus.ring_visible(CHECK), "opened from the keyboard");

    // Tab and Shift+Tab go round the content only.
    let mut seen = Vec::new();
    for shift in [false, false, false, true, true] {
        assert_eq!(fly.key(&KeyName::Tab), Outcome::Unused);
        assert!(focus.tab(shift));
        seen.push(focus.focused().unwrap());
    }
    assert_eq!(seen, [OK, CHECK, OK, CHECK, OK]);

    // Escape closes it; the focus is back on the button, rings and all.
    assert_eq!(fly.key(&KeyName::Escape), Outcome::Close);
    assert!(close_scope(&mut focus, SCOPE));
    assert_eq!(focus.scope(), None);
    assert_eq!(focus.focused(), Some(BUTTON));
    frame(&mut focus, false);
    assert_eq!(focus.focused(), Some(BUTTON));
    assert!(focus.ring_visible(BUTTON));
    assert!(!close_scope(&mut focus, SCOPE), "closed already");
}

#[test]
fn opened_by_a_shortcut_elsewhere_closing_gives_the_focus_back_there_not_to_the_anchor() {
    // The editor has the focus; Ctrl+K opens a flyout under a toolbar button.
    let mut focus = Focus::new();
    frame(&mut focus, false);
    focus.focus(EDITOR);
    focus.key_used();
    let fly = placed(Rect::new(100.0, 100.0, 80.0, 30.0));
    open_scope(&mut focus, SCOPE, Some(OK));
    assert_eq!(
        focus.focused(),
        Some(EDITOR),
        "nothing moves before the frame"
    );
    frame(&mut focus, true);
    assert_eq!(focus.focused(), Some(OK), "the named widget");
    assert!(focus.ring_visible(OK));

    // Escape: back to the editor, and the button never had the focus.
    assert_eq!(fly.key(&KeyName::Escape), Outcome::Close);
    assert!(close_scope(&mut focus, SCOPE));
    assert_eq!(focus.focused(), Some(EDITOR));
    frame(&mut focus, false);
    assert_eq!(focus.focused(), Some(EDITOR));
    assert!(!focus.is_focused(BUTTON));

    // The same with a press outside, or on the anchor, to close it.
    for (x, y, outcome) in [
        (700.0, 500.0, Outcome::Outside),
        (110.0, 110.0, Outcome::Anchor),
    ] {
        open_scope(&mut focus, SCOPE, None);
        frame(&mut focus, true);
        assert_eq!(focus.focused(), Some(CHECK));
        assert_eq!(fly.pointer_down(x, y), outcome);
        assert!(close_scope(&mut focus, SCOPE));
        frame(&mut focus, false);
        assert_eq!(focus.focused(), Some(EDITOR), "{outcome:?}");
    }
}

#[test]
fn an_anchor_that_is_no_widget_works_and_closing_gives_back_what_was_there() {
    // A flyout on a word: no focus id at all for its anchor.
    let word = Rect::new(300.0, 200.0, 46.0, 18.0);
    let fly = placed(word);

    // Nothing had the focus: Tab works inside, and closing leaves nothing focused.
    let mut focus = Focus::new();
    frame(&mut focus, false);
    assert_eq!(focus.focused(), None);
    open_scope(&mut focus, SCOPE, None);
    frame(&mut focus, true);
    assert_eq!(focus.focused(), Some(CHECK));
    assert!(focus.tab(false));
    assert_eq!(focus.focused(), Some(OK));
    assert!(focus.tab(false));
    assert_eq!(focus.focused(), Some(CHECK));
    assert_eq!(fly.pointer_down(word.cx(), word.cy()), Outcome::Anchor);
    assert!(close_scope(&mut focus, SCOPE));
    frame(&mut focus, false);
    assert_eq!(focus.focused(), None);

    // The editor the word is in had it: it gets it back.
    focus.focus(EDITOR);
    open_scope(&mut focus, SCOPE, None);
    frame(&mut focus, true);
    assert_eq!(focus.focused(), Some(CHECK));
    assert_eq!(fly.key(&KeyName::Escape), Outcome::Close);
    assert!(close_scope(&mut focus, SCOPE));
    assert_eq!(focus.focused(), Some(EDITOR));
}

#[test]
fn close_scope_leaves_a_scope_that_is_not_the_innermost_alone() {
    let mut focus = Focus::new();
    frame(&mut focus, false);
    focus.focus(EDITOR);
    open_scope(&mut focus, SCOPE, None);
    frame(&mut focus, true);
    // The content opened a scope of its own (a menu).
    focus.push_scope(99);
    assert!(!close_scope(&mut focus, SCOPE));
    assert_eq!(focus.scope(), Some(99));
    assert!(focus.pop_scope());
    assert!(close_scope(&mut focus, SCOPE));
    assert_eq!(focus.focused(), Some(EDITOR));
}

// ---- drawing -----------------------------------------------------------------------------------

const W: u32 = 320;
const H: u32 = 220;
const CARD: Rect = Rect::new(40.0, 40.0, 200.0, 100.0);

fn at(bmp: &Bitmap, x: u32, y: u32) -> Rgb {
    let o = ((y * bmp.width + x) * 4) as usize;
    [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
}

fn canvas(pal: &Palette, draw: impl FnOnce(&mut Painter)) -> Bitmap {
    let mut bmp = Bitmap::new(W, H);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, W as f32, H as f32), pal.page);
        draw(&mut p);
    }
    bmp
}

fn luma(c: Rgb) -> u32 {
    c.iter().map(|&v| v as u32).sum()
}

#[test]
fn the_card_draws_its_body_its_border_and_a_shadow_below() {
    for pal in [&LIGHT, &DARK] {
        let bmp = canvas(pal, |p| draw_card(p, pal, CARD));
        assert_eq!(at(&bmp, 140, 90), pal.popup, "the body");
        assert_eq!(at(&bmp, 40, 90), pal.popup_line, "the left border");
        assert_eq!(at(&bmp, 239, 90), pal.popup_line, "the right border");
        assert_eq!(at(&bmp, 140, 40), pal.popup_line, "the top border");
        assert_eq!(at(&bmp, 140, 139), pal.popup_line, "the bottom border");
        assert_eq!(at(&bmp, 41, 90), pal.popup);
        assert!(luma(at(&bmp, 140, 143)) < luma(pal.page), "the shadow");
        assert_eq!(at(&bmp, 300, 200), pal.page, "nothing far away");
    }
}

#[test]
fn the_card_is_drawn_as_a_menus_frame_and_shadow() {
    for pal in [&LIGHT, &DARK] {
        let mut menu: Menu<u8> = Menu::new(Vec::new(), 40.0, 40.0, false);
        let area = Rect::new(0.0, 0.0, W as f32, H as f32);
        let menu_bmp = canvas(pal, |p| menu.draw_in(p, pal, area));
        let card_bmp = canvas(pal, |p| draw_card(p, pal, menu.rect()));
        assert!(menu.rect().w > 0.0 && menu.rect().h > 0.0);
        assert!(menu_bmp.pixels == card_bmp.pixels);
    }
}

#[test]
fn a_card_with_no_size_or_not_a_number_draws_nothing() {
    let blank = canvas(&LIGHT, |_| {});
    for r in [
        Rect::default(),
        Rect::new(40.0, 40.0, 0.0, 100.0),
        Rect::new(40.0, 40.0, -10.0, 100.0),
        Rect::new(f32::NAN, 40.0, 100.0, 100.0),
        Rect::new(40.0, 40.0, f32::INFINITY, 100.0),
    ] {
        let bmp = canvas(&LIGHT, |p| draw_card(p, &LIGHT, r));
        assert!(bmp.pixels == blank.pixels, "{r:?}");
    }
}

/// A flyout below an anchor at (40, 10), its card at (40, 44), its content a filled box.
fn animated(pal: &Palette, t: f32, called: &mut bool) -> Bitmap {
    let mut f = Flyout::new(Rect::new(40.0, 10.0, 60.0, 30.0), (160.0, 60.0));
    f.place_in(Rect::new(0.0, 0.0, W as f32, H as f32));
    canvas(pal, |p| {
        draw_card_animated(p, pal, &f, t, |p, r| {
            *called = true;
            p.fill(r, pal.accent);
        })
    })
}

#[test]
fn at_rest_the_animated_card_is_the_card_and_its_content_pixel_for_pixel() {
    for pal in [&LIGHT, &DARK] {
        let mut f = Flyout::new(Rect::new(40.0, 10.0, 60.0, 30.0), (160.0, 60.0));
        f.place_in(Rect::new(0.0, 0.0, W as f32, H as f32));
        let plain = canvas(pal, |p| {
            draw_card(p, pal, f.rect());
            p.fill(f.content_rect(), pal.accent);
        });
        for t in [1.0, 2.0, f32::NAN] {
            let mut called = false;
            let bmp = animated(pal, t, &mut called);
            assert!(called);
            assert!(bmp.pixels == plain.pixels, "t = {t}");
        }
    }
}

#[test]
fn gone_the_animated_card_draws_nothing_and_half_way_it_is_faded() {
    for pal in [&LIGHT, &DARK] {
        let blank = canvas(pal, |_| {});
        for t in [0.0, -1.0] {
            let mut called = false;
            let bmp = animated(pal, t, &mut called);
            assert!(!called);
            assert!(bmp.pixels == blank.pixels);
        }
        let mut called = false;
        let half = animated(pal, 0.5, &mut called);
        assert!(called);
        // The middle of the content: between the page and the accent.
        let c = at(&half, 120, 90);
        assert!(c != pal.page && c != pal.accent, "{c:?}");
    }
}

// ---- sheet -------------------------------------------------------------------------------------

/// A flyout's content as an application might draw it: a line of text, a check box with its
/// label and a button, the button with the focus ring.
fn content(p: &mut Painter, pal: &Palette, r: Rect) {
    p.text(
        Face::Ui,
        13.5,
        r.x,
        r.y + 9.0,
        "Show hidden files",
        pal.text,
        r.w,
    );
    let tick = Rect::new(r.x, r.y + 30.0, 18.0, 18.0);
    draw_tick(p, pal, tick, Tick::new(true));
    p.text(
        Face::Ui,
        13.0,
        tick.right() + 8.0,
        tick.cy(),
        "Also in subfolders",
        pal.text,
        r.w - 26.0,
    );
    let button = Rect::new(r.right() - 90.0, r.bottom() - 30.0, 90.0, 30.0);
    draw_dialog_button(p, pal, button, "Apply", true, false, false);
    draw_ring(p, pal, button, 4.0);
}

/// `cargo test -p mdshared-ui flyout_sheet -- --ignored`, then open target/specimen/flyout.png
/// and flyout-150.png: a flyout below a button, one flipped above near the bottom, one gone
/// left at the right edge and one at the bottom right corner, light and dark.
#[test]
#[ignore]
fn flyout_sheet() {
    const SW: f32 = 760.0;
    const SH: f32 = 420.0;
    for (scale, file) in [(1.0, "flyout.png"), (1.5, "flyout-150.png")] {
        let mut bmp = Bitmap::new((SW * scale) as u32, (2.0 * SH * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (half, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
            let oy = half as f32 * SH;
            p.fill(Rect::new(0.0, oy, SW, SH), pal.page);
            let o = p.offset(0.0, oy);
            let area = Rect::new(0.0, 0.0, SW, SH);
            let anchors = [
                (Rect::new(16.0, 16.0, 90.0, 28.0), "Options"),
                (Rect::new(16.0, SH - 44.0, 90.0, 28.0), "Above"),
                (Rect::new(SW - 106.0, 16.0, 90.0, 28.0), "Left"),
                (Rect::new(SW - 36.0, SH - 36.0, 28.0, 28.0), "…"),
            ];
            for (anchor, label) in anchors {
                p.rbox(anchor, 5.0, pal.hover);
                p.text_center(Face::Ui, 13.0, anchor, label, pal.text);
                let mut f = Flyout::new(anchor, (220.0, 96.0));
                let r = f.place_in(area);
                draw_card(&mut p, pal, r);
                content(&mut p, pal, f.content_rect());
            }
            p.restore_offset(o);
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join(file)).unwrap();
    }
}
