//! The layout's arithmetic: fixed and flexible sizes, least and most sizes and their conflicts,
//! shrinking, justification and alignment, wrapping, snapping with no pixel lost, nonsense
//! input, a pseudo-random check of the flexible lengths against the rules they must keep, and
//! the boxes drawn and read back as pixels.
//!
//! The sheet for a person to look at: `cargo test -p mdshared-ui layout_sheet -- --ignored`
//! writes `target/specimen/layout.png` and `layout-150.png`.

use super::*;
use crate::gfx::{Gfx, Painter, Rgb};
use crate::theme::{DARK, LIGHT, Palette};
use mdshared_text::bitmap::Bitmap;

fn area(w: f32, h: f32) -> Rect {
    Rect::new(0.0, 0.0, w, h)
}

fn widths(r: &[Rect]) -> Vec<f32> {
    r.iter().map(|r| r.w).collect()
}

/// A small LCG with a fixed seed, so the pseudo-random tests are the same on every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
    fn unit(&mut self) -> f32 {
        self.next() as f32 / (1u64 << 31) as f32
    }
}

#[test]
fn no_items_give_no_rectangles() {
    assert!(Flex::row().layout(area(100.0, 20.0), &[]).is_empty());
    assert_eq!(Flex::row().natural(&[]), 0.0);
    let pad = Flex::column().padding(Insets::all(5.0));
    assert_eq!(pad.cross_extent(100.0, &[]), 10.0);
    assert_eq!(pad.natural(&[]), 10.0);
}

#[test]
fn fixed_items_stand_in_order_after_the_padding_with_the_gap_between_them() {
    let f = Flex::row()
        .padding(Insets::new(10.0, 4.0, 2.0, 6.0))
        .gap(5.0);
    let r = f.layout(area(200.0, 40.0), &[Item::fixed(30.0), Item::fixed(50.0)]);
    assert_eq!(r[0], Rect::new(10.0, 4.0, 30.0, 30.0));
    assert_eq!(r[1], Rect::new(45.0, 4.0, 50.0, 30.0));
    // The same in a column: the axes swap.
    let f = Flex::column()
        .padding(Insets::new(4.0, 10.0, 6.0, 2.0))
        .gap(5.0);
    let r = f.layout(area(40.0, 200.0), &[Item::fixed(30.0), Item::fixed(50.0)]);
    assert_eq!(r[0], Rect::new(4.0, 10.0, 30.0, 30.0));
    assert_eq!(r[1], Rect::new(4.0, 45.0, 30.0, 50.0));
}

#[test]
fn the_area_s_own_place_moves_every_rectangle_with_it() {
    let items = [Item::fixed(30.0), Item::flex(1.0)];
    let at0 = Flex::row().layout(area(100.0, 10.0), &items);
    let moved = Flex::row().layout(Rect::new(40.0, 7.0, 100.0, 10.0), &items);
    for (a, b) in at0.iter().zip(&moved) {
        assert_eq!(*b, Rect::new(a.x + 40.0, a.y + 7.0, a.w, a.h));
    }
}

#[test]
fn flexible_items_share_what_the_fixed_ones_leave_by_their_weights() {
    let r = Flex::row().gap(10.0).layout(
        area(390.0, 20.0),
        &[Item::fixed(70.0), Item::flex(1.0), Item::flex(2.0)],
    );
    assert_eq!(widths(&r), vec![70.0, 100.0, 200.0]);
    assert_eq!(r[2].right(), 390.0);
}

#[test]
fn a_fixed_item_that_also_grows_grows_from_its_basis() {
    let r = Flex::row().layout(
        area(300.0, 20.0),
        &[Item::fixed(100.0).grow(1.0), Item::flex(1.0)],
    );
    assert_eq!(widths(&r), vec![200.0, 100.0]);
}

#[test]
fn three_equal_shares_of_301_pixels_lose_no_pixel_and_share_their_edges() {
    let r = Flex::row().layout(area(301.0, 10.0), &[Item::flex(1.0); 3]);
    assert_eq!(r[0].x, 0.0);
    assert_eq!(r[0].right(), r[1].x);
    assert_eq!(r[1].right(), r[2].x);
    assert_eq!(r[2].right(), 301.0);
    assert_eq!(widths(&r).iter().sum::<f32>(), 301.0);
    for w in widths(&r) {
        assert!(w == 100.0 || w == 101.0, "{w}");
    }
}

#[test]
fn snapped_edges_cover_the_snapped_length_exactly_for_any_split() {
    // Checked against a second way: the length of the area once its own edges are snapped.
    let mut rng = Lcg(7);
    for _ in 0..500 {
        let n = 1 + rng.below(9) as usize;
        let x = rng.unit() * 50.0;
        let w = rng.unit() * 900.0;
        let items: Vec<Item> = (0..n).map(|_| Item::flex(0.1 + rng.unit() * 3.0)).collect();
        let r = Flex::row().layout(Rect::new(x, 0.0, w, 10.0), &items);
        assert_eq!(r[0].x, x.round());
        assert_eq!(r[n - 1].right(), (x + w).round());
        for k in 1..n {
            assert_eq!(r[k - 1].right(), r[k].x);
        }
        for b in &r {
            assert_eq!(b.x, b.x.round());
            assert_eq!(b.w, b.w.round());
        }
        let total: f32 = widths(&r).iter().sum();
        assert_eq!(total, (x + w).round() - x.round());
    }
}

#[test]
fn a_step_of_device_pixels_puts_every_edge_on_the_device_grid() {
    let step = 1.0 / 1.5;
    let r = Flex::row()
        .snap(step)
        .layout(area(100.0, 10.0), &[Item::flex(1.0); 7]);
    for b in &r {
        let dev = b.x * 1.5;
        assert!((dev - dev.round()).abs() < 1e-3, "{}", b.x);
    }
    assert!((r[6].right() - 100.0).abs() < 1e-3);
}

#[test]
fn a_step_of_0_keeps_the_exact_edges() {
    let r = Flex::row()
        .snap(0.0)
        .layout(area(100.0, 10.0), &[Item::flex(1.0); 3]);
    assert!((r[1].x - 100.0 / 3.0).abs() < 1e-4);
    assert!((r[2].right() - 100.0).abs() < 1e-4);
    // A step that makes no sense is the same as 0.
    let nan = Flex::row().snap(f32::NAN);
    assert_eq!(nan.layout(area(100.0, 10.0), &[Item::flex(1.0); 3]), r);
}

#[test]
fn an_item_held_at_its_least_size_leaves_the_rest_to_the_others() {
    let r = Flex::row().layout(
        area(300.0, 10.0),
        &[Item::flex(1.0).min(200.0), Item::flex(1.0), Item::flex(1.0)],
    );
    assert_eq!(widths(&r), vec![200.0, 50.0, 50.0]);
}

#[test]
fn an_item_held_at_its_most_size_leaves_the_rest_to_the_others() {
    let r = Flex::row().layout(
        area(300.0, 10.0),
        &[Item::flex(1.0).max(40.0), Item::flex(1.0), Item::flex(2.0)],
    );
    assert_eq!(widths(&r), vec![40.0, 87.0, 173.0]);
    assert_eq!(r[2].right(), 300.0);
}

#[test]
fn when_every_grower_is_at_its_most_size_the_space_left_is_justified() {
    let items = [Item::flex(1.0).max(50.0), Item::flex(1.0).max(50.0)];
    let at = |j| Flex::row().justify(j).layout(area(300.0, 10.0), &items);
    assert_eq!(at(Justify::Start)[0].x, 0.0);
    assert_eq!(at(Justify::End)[1].right(), 300.0);
    assert_eq!(at(Justify::Center)[0].x, 100.0);
}

#[test]
fn a_least_size_above_the_most_size_wins() {
    let it = Item::fixed(10.0).min(80.0).max(30.0);
    assert_eq!(it.natural(), 80.0);
    let r = Flex::row().layout(area(300.0, 10.0), &[it.grow(1.0), Item::flex(1.0)]);
    assert_eq!(r[0].w, 80.0);
    assert_eq!(r[1].w, 220.0);
    // Shrinking stops at it too.
    let r = Flex::row().layout(area(20.0, 10.0), &[it.shrink(1.0)]);
    assert_eq!(r[0].w, 80.0);
}

#[test]
fn items_that_shrink_give_up_room_by_weight_times_basis_down_to_their_least_size() {
    let r = Flex::row().layout(
        area(240.0, 10.0),
        &[
            Item::fixed(100.0).shrink(1.0),
            Item::fixed(200.0).shrink(1.0),
        ],
    );
    assert_eq!(widths(&r), vec![80.0, 160.0]);
    // The first stops at 95; the second gives up the rest.
    let r = Flex::row().layout(
        area(240.0, 10.0),
        &[
            Item::fixed(100.0).shrink(1.0).min(95.0),
            Item::fixed(200.0).shrink(1.0),
        ],
    );
    assert_eq!(widths(&r), vec![95.0, 145.0]);
}

#[test]
fn items_that_do_not_fit_run_past_the_end_from_the_start_whatever_the_justification() {
    for j in [
        Justify::Start,
        Justify::Center,
        Justify::End,
        Justify::SpaceEvenly,
    ] {
        let r = Flex::row()
            .justify(j)
            .gap(4.0)
            .layout(area(100.0, 10.0), &[Item::fixed(80.0), Item::fixed(80.0)]);
        assert_eq!(r[0].x, 0.0);
        assert_eq!(r[1], Rect::new(84.0, 0.0, 80.0, 10.0));
    }
}

#[test]
fn zero_space_gives_flexible_items_their_least_size_and_no_negative_size() {
    for a in [area(0.0, 0.0), area(-50.0, -10.0)] {
        let r = Flex::row().padding(Insets::all(8.0)).gap(4.0).layout(
            a,
            &[Item::flex(1.0), Item::flex(1.0).min(10.0), Item::fixed(5.0)],
        );
        assert_eq!(r.len(), 3);
        assert_eq!(widths(&r), vec![0.0, 10.0, 5.0]);
        for b in &r {
            assert!(b.w >= 0.0 && b.h >= 0.0);
            assert_eq!(b.h, 0.0);
        }
    }
}

#[test]
fn nonsense_in_gives_finite_rectangles_and_no_panic() {
    let bad = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -5.0];
    for &v in &bad {
        let f = Flex::row()
            .gap(v)
            .line_gap(v)
            .snap(v)
            .padding(Insets::all(v))
            .wrap(true);
        let items = [
            Item::fixed(v),
            Item::flex(v),
            Item::fixed(10.0).min(v).max(v).grow(v).shrink(v).cross(v),
        ];
        for a in [Rect::new(v, v, v, v), area(v, 30.0), area(100.0, 30.0)] {
            for b in f.layout(a, &items) {
                for c in [b.x, b.y, b.w, b.h] {
                    assert!(c.is_finite(), "{a:?} {b:?}");
                }
                assert!(b.w >= 0.0 && b.h >= 0.0);
            }
        }
        assert!(f.natural(&items).is_finite());
        assert!(f.cross_extent(v, &items).is_finite());
    }
}

#[test]
fn an_infinite_most_size_is_no_limit_and_a_negative_one_is_0() {
    let r = Flex::row().layout(area(100.0, 10.0), &[Item::flex(1.0).max(f32::INFINITY)]);
    assert_eq!(r[0].w, 100.0);
    let r = Flex::row().layout(area(100.0, 10.0), &[Item::flex(1.0).max(-3.0)]);
    assert_eq!(r[0].w, 0.0);
}

#[test]
fn justification_places_what_is_left() {
    let items = [Item::fixed(20.0), Item::fixed(20.0), Item::fixed(20.0)];
    let xs = |j| -> Vec<f32> {
        (Flex::row().justify(j).layout(area(120.0, 10.0), &items))
            .iter()
            .map(|r| r.x)
            .collect()
    };
    assert_eq!(xs(Justify::Start), vec![0.0, 20.0, 40.0]);
    assert_eq!(xs(Justify::Center), vec![30.0, 50.0, 70.0]);
    assert_eq!(xs(Justify::End), vec![60.0, 80.0, 100.0]);
    assert_eq!(xs(Justify::SpaceBetween), vec![0.0, 50.0, 100.0]);
    assert_eq!(xs(Justify::SpaceAround), vec![10.0, 50.0, 90.0]);
    assert_eq!(xs(Justify::SpaceEvenly), vec![15.0, 50.0, 85.0]);
    // One item between: at the start.
    let one = Flex::row().justify(Justify::SpaceBetween);
    assert_eq!(one.layout(area(120.0, 10.0), &items[..1])[0].x, 0.0);
}

#[test]
fn items_with_a_cross_size_are_aligned_and_those_without_fill_the_line() {
    let f = Flex::row().align(Align::Center);
    let r = f.layout(
        area(100.0, 40.0),
        &[
            Item::fixed(10.0),
            Item::fixed(10.0).cross(20.0),
            Item::fixed(10.0).cross(20.0).align(Align::End),
            Item::fixed(10.0).cross(20.0).align(Align::Start),
        ],
    );
    assert_eq!((r[0].y, r[0].h), (0.0, 40.0));
    assert_eq!((r[1].y, r[1].h), (10.0, 20.0));
    assert_eq!((r[2].y, r[2].h), (20.0, 20.0));
    assert_eq!((r[3].y, r[3].h), (0.0, 20.0));
}

#[test]
fn a_cross_size_larger_than_a_line_that_does_not_wrap_is_held_to_it() {
    let r = Flex::column()
        .align(Align::Center)
        .layout(area(30.0, 100.0), &[Item::fixed(10.0).cross(80.0)]);
    assert_eq!(r[0], Rect::new(0.0, 0.0, 30.0, 10.0));
}

#[test]
fn wrapped_items_go_on_to_new_lines_each_as_tall_as_its_tallest() {
    let f = Flex::row().wrap(true).gap(10.0).line_gap(4.0);
    let chip = |w: f32, h: f32| Item::fixed(w).cross(h);
    let r = f.layout(
        area(100.0, 200.0),
        &[
            chip(40.0, 20.0),
            chip(40.0, 24.0),
            chip(40.0, 20.0),
            chip(90.0, 20.0),
        ],
    );
    assert_eq!(r[0], Rect::new(0.0, 0.0, 40.0, 20.0));
    assert_eq!(r[1], Rect::new(50.0, 0.0, 40.0, 24.0));
    assert_eq!(r[2], Rect::new(0.0, 28.0, 40.0, 20.0));
    assert_eq!(r[3], Rect::new(0.0, 52.0, 90.0, 20.0));
    assert_eq!(
        f.cross_extent(100.0, &[chip(40.0, 20.0), chip(40.0, 24.0)]),
        24.0
    );
    assert_eq!(
        f.cross_extent(
            100.0,
            &[
                chip(40.0, 20.0),
                chip(40.0, 24.0),
                chip(40.0, 20.0),
                chip(90.0, 20.0)
            ]
        ),
        72.0
    );
}

#[test]
fn an_item_longer_than_a_line_gets_a_line_of_its_own() {
    let f = Flex::row().wrap(true);
    let r = f.layout(
        area(50.0, 100.0),
        &[
            Item::fixed(20.0).cross(10.0),
            Item::fixed(80.0).cross(10.0),
            Item::fixed(20.0).cross(10.0),
        ],
    );
    assert_eq!((r[0].x, r[0].y), (0.0, 0.0));
    assert_eq!((r[1].x, r[1].y, r[1].w), (0.0, 10.0, 80.0));
    assert_eq!((r[2].x, r[2].y), (0.0, 20.0));
}

#[test]
fn each_wrapped_line_shares_its_own_space_among_its_own_items() {
    let tile = Item::flex(1.0).min(60.0).cross(30.0);
    let r = Flex::row()
        .wrap(true)
        .layout(area(200.0, 100.0), &[tile; 4]);
    // Three tiles of 60 fit in 200; the fourth goes on, alone, and takes the whole line.
    assert_eq!(widths(&r[..3]).iter().sum::<f32>(), 200.0);
    assert_eq!(r[3], Rect::new(0.0, 30.0, 200.0, 30.0));
}

#[test]
fn wrapped_lines_where_no_item_has_a_cross_size_share_what_is_left() {
    let r = Flex::row()
        .wrap(true)
        .line_gap(10.0)
        .layout(area(100.0, 100.0), &[Item::fixed(60.0); 3]);
    // One item a line; three lines share 100 less two gaps of 10: 26.67 each, snapped.
    assert_eq!(r[0], Rect::new(0.0, 0.0, 60.0, 27.0));
    assert_eq!((r[1].y, r[1].bottom()), (37.0, 63.0));
    assert_eq!((r[2].y, r[2].bottom()), (73.0, 100.0));
}

#[test]
fn the_natural_length_is_the_natural_sizes_the_gaps_and_the_main_padding() {
    let f = Flex::column()
        .gap(8.0)
        .padding(Insets::new(1.0, 10.0, 2.0, 20.0));
    let items = [
        Item::fixed(30.0),
        Item::flex(1.0).min(12.0),
        Item::fixed(5.0).min(9.0),
    ];
    assert_eq!(f.natural(&items), 30.0 + 12.0 + 9.0 + 16.0 + 30.0);
    // Laid out in exactly that length, nothing grows past its natural size.
    let r = f.layout(area(50.0, f.natural(&items)), &items);
    assert_eq!(
        r.iter().map(|r| r.h).collect::<Vec<_>>(),
        vec![30.0, 12.0, 9.0]
    );
}

#[test]
fn the_cross_extent_without_wrapping_is_the_largest_cross_size_and_the_padding() {
    let f = Flex::row().padding(Insets::xy(3.0, 5.0));
    let items = [
        Item::fixed(10.0).cross(14.0),
        Item::fixed(10.0),
        Item::fixed(10.0).cross(22.0),
    ];
    assert_eq!(f.cross_extent(1000.0, &items), 32.0);
}

#[test]
fn layout_into_clears_the_vector_it_is_given() {
    let mut out = vec![Rect::new(1.0, 2.0, 3.0, 4.0); 9];
    Flex::row().layout_into(area(100.0, 10.0), &[Item::flex(1.0)], &mut out);
    assert_eq!(out, vec![area(100.0, 10.0)]);
    Flex::row().layout_into(area(100.0, 10.0), &[], &mut out);
    assert!(out.is_empty());
}

#[test]
fn insets_too_large_for_the_rectangle_give_an_empty_one() {
    let r = Insets::new(30.0, 30.0, 30.0, 30.0).shrink(Rect::new(10.0, 10.0, 40.0, 40.0));
    assert_eq!(r, Rect::new(40.0, 40.0, 0.0, 0.0));
    assert_eq!(Insets::xy(2.0, 3.0), Insets::new(2.0, 3.0, 2.0, 3.0));
    assert_eq!(
        Insets::all(2.0).shrink(area(10.0, 10.0)),
        Rect::new(2.0, 2.0, 6.0, 6.0)
    );
    assert_eq!(
        Insets::ZERO.shrink(Rect::new(f32::NAN, 0.0, 5.0, 5.0)),
        area(0.0, 0.0)
    );
}

#[test]
fn resolved_lengths_keep_their_bounds_fill_the_line_and_share_by_weight() {
    // A second way of saying what the lengths must be, checked on pseudo-random lines: every
    // length within its bounds; when something could still grow (or shrink), the line is
    // exactly filled; the growers off their bounds have grown by the same amount per weight.
    let mut rng = Lcg(42);
    let mut sizes = Vec::new();
    for _ in 0..3000 {
        let n = 1 + rng.below(6) as usize;
        let items: Vec<Item> = (0..n)
            .map(|_| {
                let mut it = Item::fixed((rng.below(4) * 40) as f32);
                if rng.below(3) > 0 {
                    it = it.grow(rng.below(4) as f32);
                }
                if rng.below(2) == 0 {
                    it = it.shrink(rng.below(3) as f32);
                }
                if rng.below(3) == 0 {
                    it = it.min(rng.below(100) as f32);
                }
                if rng.below(3) == 0 {
                    it = it.max(rng.below(200) as f32);
                }
                it
            })
            .collect();
        let len = rng.below(500) as f32;
        let gap = rng.below(3) as f32 * 4.0;
        resolve(len, gap, &items, &mut sizes);
        let avail = len - gap * (n - 1) as f32;
        let natural: f32 = items.iter().map(Item::natural).sum();
        let sum: f32 = sizes.iter().sum();
        let growing = avail > natural;
        for (it, &s) in items.iter().zip(&sizes) {
            let (lo, hi) = it.bounds();
            assert!(s >= lo - 1e-3 && s <= hi + 1e-3, "{items:?} {sizes:?}");
        }
        let can_move = items.iter().zip(&sizes).any(|(it, &s)| {
            let (lo, hi) = it.bounds();
            if growing {
                it.grow > 0.0 && s < hi - 1e-3
            } else {
                it.shrink * it.basis > 0.0 && s > lo + 1e-3
            }
        });
        if can_move {
            assert!((sum - avail).abs() < 1e-2, "{len} {items:?} {sizes:?}");
        }
        if growing {
            let per: Vec<f32> = (items.iter().zip(&sizes))
                .filter(|(it, s)| {
                    let (lo, hi) = it.bounds();
                    it.grow > 0.0 && **s > lo + 1e-3 && **s < hi - 1e-3 && it.basis <= it.natural()
                })
                .map(|(it, s)| (s - it.basis) / it.grow)
                .collect();
            for w in per.windows(2) {
                assert!((w[0] - w[1]).abs() < 1e-2, "{items:?} {sizes:?}");
            }
        } else {
            // The shrinkers off their bounds have given up the same amount per weight times
            // basis.
            let per: Vec<f32> = (items.iter().zip(&sizes))
                .filter(|(it, s)| {
                    let (lo, hi) = it.bounds();
                    it.shrink * it.basis > 0.0
                        && **s > lo + 1e-3
                        && **s < hi - 1e-3
                        && it.basis >= it.natural()
                })
                .map(|(it, s)| (it.basis - s) / (it.shrink * it.basis))
                .collect();
            for w in per.windows(2) {
                assert!((w[0] - w[1]).abs() < 1e-3, "{len} {items:?} {sizes:?}");
            }
        }
    }
}

#[test]
fn growing_counts_from_the_basis_not_from_the_least_size() {
    let r = Flex::row().layout(
        area(200.0, 10.0),
        &[Item::fixed(10.0).min(50.0).grow(1.0), Item::flex(1.0)],
    );
    assert_eq!(widths(&r), vec![105.0, 95.0]);
}

#[test]
fn a_wrapping_layout_as_long_as_its_natural_length_keeps_one_line() {
    // The padding is added in `natural` and taken off again in `layout`; the float rounding of
    // that must not push the last item onto a second line.
    let f = Flex::row()
        .wrap(true)
        .padding(Insets::xy(8.0, 0.0))
        .gap(3.0);
    let mut rng = Lcg(3);
    for _ in 0..2000 {
        let n = 1 + rng.below(6) as usize;
        let items: Vec<Item> = (0..n)
            .map(|_| Item::fixed(rng.unit() * 90.0).cross(10.0))
            .collect();
        let len = f.natural(&items);
        let r = f.layout(Rect::new(0.0, 0.0, len, 100.0), &items);
        assert!(r.iter().all(|b| b.y == 0.0), "{items:?}");
        assert_eq!(f.cross_extent(len, &items), 10.0);
    }
}

#[test]
fn the_cross_extent_of_a_wrapping_layout_takes_the_main_padding_off_first() {
    let f = Flex::row()
        .wrap(true)
        .padding(Insets::new(10.0, 2.0, 10.0, 3.0));
    let items = [Item::fixed(50.0).cross(20.0); 2];
    assert_eq!(f.cross_extent(120.0, &items), 25.0);
    assert_eq!(f.cross_extent(119.0, &items), 45.0);
    // No limit: one line. NaN: no room, a line each.
    assert_eq!(f.cross_extent(f32::INFINITY, &items), 25.0);
    assert_eq!(f.cross_extent(f32::NAN, &items), 45.0);
}

#[test]
fn huge_lengths_and_a_tiny_step_still_give_finite_rectangles() {
    let r = Flex::row().layout(area(100.0, 10.0), &[Item::fixed(3e38).shrink(10.0)]);
    assert!(r[0].w.is_finite());
    let r = Flex::row()
        .gap(1e38)
        .layout(area(100.0, 10.0), &[Item::fixed(5.0); 10]);
    assert!(r.iter().all(|b| b.x.is_finite() && b.right().is_finite()));
    let r = Flex::row()
        .snap(1e-40)
        .layout(area(10.0, 10.0), &[Item::flex(1.0); 3]);
    assert!(r.iter().all(|b| b.x.is_finite() && b.w.is_finite()));
    assert!((r[2].right() - 10.0).abs() < 1e-4);
}

#[test]
fn a_negative_infinite_most_size_is_no_limit() {
    let r = Flex::row().layout(area(100.0, 10.0), &[Item::flex(1.0).max(f32::NEG_INFINITY)]);
    assert_eq!(r[0].w, 100.0);
}

#[test]
fn new_with_a_direction_is_row_and_column() {
    assert_eq!(Flex::new(Direction::Row), Flex::row());
    assert_eq!(Flex::new(Direction::Column), Flex::column());
}

fn at(bmp: &Bitmap, scale: f32, x: f32, y: f32) -> Rgb {
    let (x, y) = ((x * scale) as usize, (y * scale) as usize);
    let o = (y * bmp.width as usize + x) * 4;
    let p = &bmp.pixels[o..o + 3];
    [p[2], p[1], p[0]]
}

fn colours(pal: &Palette) -> [Rgb; 2] {
    [pal.accent, pal.text]
}

/// Draws a row of flexible boxes in alternating colours on the pane colour.
fn draw_row(p: &mut Painter, pal: &Palette, scale: f32, row: Rect, n: usize) -> Vec<Rect> {
    let r = Flex::row()
        .snap(1.0 / scale)
        .layout(row, &vec![Item::flex(1.0); n]);
    p.fill(row, pal.pane);
    for (k, b) in r.iter().enumerate() {
        p.fill(*b, colours(pal)[k % 2]);
    }
    r
}

#[test]
fn boxes_drawn_from_the_layout_cover_every_device_pixel_of_the_row_once() {
    for pal in [&LIGHT, &DARK] {
        for scale in [1.0f32, 1.5] {
            let (w, h) = (301.0f32, 10.0f32);
            let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            let r = draw_row(&mut p, pal, scale, area(w, h), 7);
            // Every device column belongs to exactly the box whose edges hold it, in that
            // box's colour: no column left in the pane colour, none in the wrong box.
            for dx in 0..bmp.width {
                let x = (dx as f32 + 0.5) / scale;
                let k = r.iter().position(|b| x >= b.x && x < b.right()).unwrap();
                assert_eq!(
                    at(&bmp, 1.0, dx as f32, 4.0),
                    colours(pal)[k % 2],
                    "{scale} {dx}"
                );
            }
        }
    }
}

/// Writes target/specimen/layout.png and layout-150.png: a toolbar, a form row, wrapped chips
/// and justified boxes, in light and dark, for a person to look at.
#[test]
#[ignore]
fn layout_sheet() {
    for (scale, file) in [(1.0f32, "layout.png"), (1.5, "layout-150.png")] {
        let (w, h) = (480.0f32, 440.0f32);
        let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (half, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
            let ox = half as f32 * 240.0;
            p.fill(Rect::new(ox, 0.0, 240.0, h), pal.pane);
            let [a, b] = colours(pal);
            let page = Flex::column()
                .padding(Insets::all(10.0))
                .gap(12.0)
                .snap(1.0 / scale);
            let bands = page.layout(
                Rect::new(ox, 0.0, 240.0, h),
                &[
                    Item::fixed(32.0),
                    Item::fixed(28.0),
                    Item::fixed(100.0),
                    Item::fixed(150.0),
                    Item::flex(1.0),
                ],
            );
            // A toolbar: icon, title, two buttons.
            let bar = Flex::row().gap(6.0).align(Align::Center).snap(1.0 / scale);
            let tb = bar.layout(
                bands[0],
                &[
                    Item::fixed(20.0).cross(20.0),
                    Item::flex(1.0).min(40.0),
                    Item::fixed(44.0).cross(24.0),
                    Item::fixed(44.0).cross(24.0),
                ],
            );
            for (k, r) in tb.iter().enumerate() {
                p.rbox(*r, 4.0, if k == 1 { b } else { a });
            }
            // A label and a field.
            let form = Flex::row().gap(8.0).snap(1.0 / scale);
            for (k, r) in (form.layout(bands[1], &[Item::fixed(70.0), Item::flex(1.0)]))
                .iter()
                .enumerate()
            {
                p.rbox(*r, 4.0, [a, b][k]);
            }
            // Chips that wrap.
            let chips = Flex::row()
                .wrap(true)
                .gap(6.0)
                .line_gap(6.0)
                .snap(1.0 / scale);
            let widths = [40.0, 64.0, 30.0, 52.0, 70.0, 44.0, 36.0, 58.0];
            let items: Vec<Item> = widths.iter().map(|&w| Item::fixed(w).cross(22.0)).collect();
            for (k, r) in chips.layout(bands[2], &items).iter().enumerate() {
                p.rbox(*r, 11.0, [a, b][k % 2]);
            }
            // Justified rows of three boxes.
            let js = [
                Justify::Start,
                Justify::Center,
                Justify::End,
                Justify::SpaceBetween,
                Justify::SpaceEvenly,
            ];
            let rows = Flex::column().gap(6.0).snap(1.0 / scale);
            let lanes = rows.layout(bands[3], &[Item::flex(1.0); 5]);
            for (lane, j) in lanes.iter().zip(js) {
                let f = Flex::row().justify(j).gap(4.0).snap(1.0 / scale);
                for r in f.layout(*lane, &[Item::fixed(36.0); 3]) {
                    p.fill(r, a);
                }
            }
            // Seven equal shares: no seam, no gap.
            let last = bands[4];
            let row = Rect::new(last.x, last.y, last.w, last.h.min(30.0));
            draw_row(&mut p, pal, scale, row, 7);
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join(file)).unwrap();
    }
}
