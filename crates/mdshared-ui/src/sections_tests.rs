use super::*;

const AREA: Rect = Rect::new(0.0, 100.0, 300.0, 200.0);

/// Three groups with 24 px headings: 5 rows, 0 rows, 30 rows, rows 20 px.
fn library() -> Sections {
    let mut s = Sections::new(200.0, 20.0);
    s.add_group(5, Some(24.0));
    s.add_group(0, Some(24.0));
    s.add_group(30, Some(24.0));
    s
}

#[test]
fn the_fenwick_tree_agrees_with_plain_sums() {
    let values: Vec<f64> = (0..37)
        .map(|i| f64::from((i * 7 % 5) as u8) * 3.0)
        .collect();
    let mut f = Fenwick::default();
    for &v in &values {
        f.push(v);
    }
    for i in 0..=values.len() {
        assert_eq!(f.prefix(i), values[..i].iter().sum::<f64>(), "prefix {i}");
    }
    f.add(10, 4.5);
    assert_eq!(f.prefix(11), values[..11].iter().sum::<f64>() + 4.5);
    // Every offset finds the entry whose span holds it, passing over empty ones.
    let mut values = values;
    values[10] += 4.5;
    let total: f64 = values.iter().sum();
    let mut y = 0.0;
    while y < total {
        let i = f.search(y);
        let top: f64 = values[..i].iter().sum();
        assert!(top <= y && y < top + values[i], "offset {y} found {i}");
        y += 0.5;
    }
    assert_eq!(f.search(total), values.len());
}

#[test]
fn headings_and_rows_share_one_index_space() {
    let s = library();
    assert_eq!(s.len(), 1 + 5 + 1 + 1 + 30);
    assert_eq!(s.groups(), 3);
    assert_eq!(s.entry(0), Some(Entry::Heading(0)));
    assert_eq!(s.entry(1), Some(Entry::Row { group: 0, row: 0 }));
    assert_eq!(s.entry(6), Some(Entry::Heading(1)));
    assert_eq!(s.entry(7), Some(Entry::Heading(2)));
    assert_eq!(s.entry(8), Some(Entry::Row { group: 2, row: 0 }));
    assert_eq!(s.entry(38), None);
    for i in 0..s.len() {
        assert_eq!(s.index(s.entry(i).unwrap()), Some(i));
    }
    assert_eq!(s.index(Entry::Row { group: 1, row: 0 }), None);
    assert_eq!(s.rows_of(2), 8..38);
    let mut plain = Sections::new(100.0, 10.0);
    plain.add_group(3, None);
    assert_eq!(plain.entry(0), Some(Entry::Row { group: 0, row: 0 }));
    assert_eq!(plain.index(Entry::Heading(0)), None);
}

#[test]
fn rows_are_laid_out_at_the_estimate_until_measured() {
    let mut s = library();
    assert_eq!(s.top(1), 24.0);
    assert_eq!(s.top(7), 24.0 + 100.0 + 24.0);
    assert_eq!(s.content(), 3.0 * 24.0 + 35.0 * 20.0);
    assert!(!s.is_measured(1) && s.is_measured(0));
    assert!(s.set_height(1, 50.0));
    assert!(s.is_measured(1));
    assert!(!s.set_height(1, 50.0));
    assert_eq!(s.top(2), 74.0);
    assert_eq!(s.content(), 3.0 * 24.0 + 34.0 * 20.0 + 50.0);
    assert!(!s.set_height(999, 5.0));
    // Nonsense is no height.
    s.set_height(2, f32::NAN);
    assert_eq!(s.height(2), 0.0);
}

#[test]
fn a_row_measured_above_the_view_keeps_the_view_still() {
    let mut s = library();
    s.scroll = 300.0;
    let before = s.visible();
    let first = before[0];
    let at = s.rect(AREA, first).y;
    // Row 1 lies wholly above: the offset moves with it.
    s.set_height(1, 60.0);
    assert_eq!(s.scroll, 340.0);
    assert_eq!(s.visible(), before);
    assert_eq!(s.rect(AREA, first).y, at);
    // A row on screen does not move the offset.
    let on_screen = before[2];
    s.set_height(on_screen, 30.0);
    assert_eq!(s.scroll, 340.0);
}

#[test]
fn the_visible_entries_skip_a_collapsed_groups_rows() {
    let mut s = library();
    assert_eq!(s.visible(), (0..10).collect::<Vec<_>>());
    s.set_collapsed(0, true);
    assert!(s.is_collapsed(0));
    // Heading 0, heading 1, heading 2 and the rows that fit after them.
    let v = s.visible();
    assert_eq!(&v[..4], &[0, 6, 7, 8]);
    assert!(v.iter().all(|&i| s.is_shown(i)));
    assert_eq!(s.height(3), 0.0);
    assert!(!s.set_collapsed(0, true));
    assert!(s.toggle(0));
    assert_eq!(s.visible(), (0..10).collect::<Vec<_>>());
}

#[test]
fn a_collapsed_row_keeps_its_measured_height_for_when_it_opens() {
    let mut s = library();
    s.set_collapsed(2, true);
    assert!(!s.set_height(9, 44.0));
    assert_eq!(s.height(9), 0.0);
    s.set_collapsed(2, false);
    assert_eq!(s.height(9), 44.0);
}

#[test]
fn collapsing_the_sticky_group_scrolls_back_to_its_heading() {
    let mut s = library();
    s.scroll = 300.0;
    let (h, _) = s.sticky(AREA).unwrap();
    assert_eq!(s.entry(h), Some(Entry::Heading(2)));
    s.set_collapsed(2, true);
    assert_eq!(s.scroll, s.clamp_scroll(s.top(7)));
}

#[test]
fn the_heading_sticks_at_the_top_and_the_next_one_pushes_it_up() {
    let mut s = Sections::new(100.0, 20.0);
    s.add_group(10, Some(24.0));
    s.add_group(10, Some(24.0));
    // In place at the top: nothing sticks.
    assert_eq!(s.sticky(AREA), None);
    s.scroll = 50.0;
    assert_eq!(
        s.sticky(AREA),
        Some((0, Rect::new(0.0, 100.0, 300.0, 24.0)))
    );
    // The second heading is at 224; at 210 it is 14 px below the top and pushes the first up.
    s.scroll = 210.0;
    assert_eq!(s.sticky(AREA), Some((0, Rect::new(0.0, 90.0, 300.0, 24.0))));
    s.scroll = 230.0;
    assert_eq!(s.sticky(AREA).map(|t| t.0), Some(11));
    s.sticky_headings = false;
    assert_eq!(s.sticky(AREA), None);
    let mut plain = Sections::new(100.0, 20.0);
    plain.add_group(50, None);
    plain.scroll = 300.0;
    assert_eq!(plain.sticky(AREA), None);
}

#[test]
fn a_point_finds_the_entry_drawn_there_and_the_sticky_heading_first() {
    let mut s = library();
    assert_eq!(s.entry_at(AREA, 10.0, 105.0), Some(0));
    assert_eq!(s.entry_at(AREA, 10.0, 125.0), Some(1));
    assert_eq!(s.entry_at(AREA, 10.0, 90.0), None);
    assert_eq!(s.entry_at(AREA, 400.0, 125.0), None);
    s.scroll = 300.0;
    let (h, r) = s.sticky(AREA).unwrap();
    assert_eq!(s.entry_at(AREA, 10.0, r.y + 2.0), Some(h));
    let under = s.entry_at(AREA, 10.0, r.bottom() + 1.0).unwrap();
    assert!(matches!(s.entry(under), Some(Entry::Row { group: 2, .. })));
    let mut short = Sections::new(500.0, 20.0);
    short.add_group(2, None);
    assert_eq!(
        short.entry_at(Rect::new(0.0, 0.0, 100.0, 500.0), 5.0, 45.0),
        None
    );
}

#[test]
fn rects_follow_the_offset() {
    let mut s = library();
    assert_eq!(s.rect(AREA, 1), Rect::new(0.0, 124.0, 300.0, 20.0));
    s.scroll = 30.0;
    assert_eq!(s.rect(AREA, 1).y, 94.0);
    s.scroll = 1e9;
    assert_eq!(s.clamp_scroll(s.scroll), s.max_scroll());
    s.scroll = f64::NAN;
    assert_eq!(s.clamp_scroll(s.scroll), 0.0);
    assert!(s.thumb(AREA).is_some());
    let mut fits = Sections::new(1000.0, 20.0);
    fits.add_group(3, Some(24.0));
    assert!(fits.thumb(AREA).is_none());
    assert_eq!(fits.max_scroll(), 0.0);
}

#[test]
fn the_arrows_go_from_row_to_row_over_headings_and_closed_groups() {
    let mut s = library();
    assert_eq!(s.nav(None, Nav::Down), Some(1));
    assert_eq!(s.nav(None, Nav::Up), Some(37));
    assert_eq!(s.nav(Some(5), Nav::Down), Some(8));
    assert_eq!(s.nav(Some(8), Nav::Up), Some(5));
    assert_eq!(s.nav(Some(1), Nav::Up), Some(1));
    assert_eq!(s.nav(Some(37), Nav::Down), Some(37));
    assert_eq!(s.nav(Some(3), Nav::Home), Some(1));
    assert_eq!(s.nav(Some(3), Nav::End), Some(37));
    s.set_collapsed(2, true);
    assert_eq!(s.nav(Some(5), Nav::Down), Some(5));
    assert_eq!(s.nav(None, Nav::End), Some(5));
    s.headings_take_cursor = true;
    assert_eq!(s.nav(Some(5), Nav::Down), Some(6));
    assert_eq!(s.nav(Some(6), Nav::Down), Some(7));
    assert_eq!(s.nav(Some(7), Nav::Down), Some(7));
    assert_eq!(s.nav(None, Nav::Home), Some(0));
    assert_eq!(s.nav(Some(8), Nav::Up), Some(7));
    assert_eq!(Sections::new(100.0, 10.0).nav(None, Nav::Down), None);
}

#[test]
fn a_page_moves_by_the_viewports_height() {
    let mut s = Sections::new(100.0, 20.0);
    s.add_group(50, Some(24.0));
    // 100 px is five rows of 20.
    assert_eq!(s.nav(Some(1), Nav::PageDown), Some(6));
    assert_eq!(s.nav(Some(6), Nav::PageUp), Some(1));
    assert_eq!(s.nav(Some(48), Nav::PageDown), Some(50));
    assert_eq!(s.nav(Some(3), Nav::PageUp), Some(1));
    // A row taller than the page still moves on by one.
    s.set_height(10, 500.0);
    assert_eq!(s.nav(Some(10), Nav::PageDown), Some(11));
}

#[test]
fn reveal_keeps_a_row_out_from_under_its_sticky_heading() {
    let mut s = library();
    s.scroll = 400.0;
    let row = 15;
    let top = s.top(row);
    assert!(top < 400.0);
    assert_eq!(s.reveal(row), top - 24.0);
    s.sticky_headings = false;
    assert_eq!(s.reveal(row), top);
    // One below goes to the bottom; one on screen leaves the offset.
    s.scroll = 0.0;
    assert_eq!(s.reveal(12), s.top(12) + 20.0 - 200.0);
    assert_eq!(s.reveal(2), 0.0);
    assert_eq!(s.reveal(999), 0.0);
}

#[test]
fn an_anchor_brings_the_same_entry_back_to_the_top() {
    let mut s = library();
    s.scroll = 333.0;
    let a = s.anchor().unwrap();
    for i in 1..6 {
        s.heights[i] = 80.0;
    }
    s.laid = Fenwick::default();
    for i in 0..s.len() {
        s.laid.push(f64::from(s.height(i)));
    }
    let to = s.restore(a);
    assert_eq!(to, s.top(a.entry) + a.offset);
    assert_eq!(s.anchor(), Some(a));
}

#[test]
fn a_list_of_a_million_rows_is_exact_at_its_end() {
    let mut s = Sections::new(600.0, 17.0);
    s.add_group(1_000_000, None);
    assert_eq!(s.content(), 17_000_000.0);
    s.scroll = s.max_scroll();
    let v = s.visible();
    assert_eq!(*v.last().unwrap(), 999_999);
    assert_eq!(s.rect(AREA, 999_999).bottom(), AREA.y + 600.0);
    s.set_height(500_000, 1.0);
    assert_eq!(s.top(999_999), 17.0 * 999_999.0 - 16.0);
}

#[test]
fn clearing_leaves_an_empty_list() {
    let mut s = library();
    s.scroll = 50.0;
    s.clear();
    assert!(s.is_empty());
    assert_eq!(s.groups(), 0);
    assert!(s.visible().is_empty());
    assert_eq!(s.content(), 0.0);
    assert_eq!(s.anchor(), None);
    s.add_group(2, Some(10.0));
    assert_eq!(s.content(), 50.0);
}
