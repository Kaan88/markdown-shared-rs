//! Tests of the accessibility tree builder: `cargo test -p mdshared-ui --features a11y a11y`.

use super::*;

#[test]
fn an_author_id_gives_the_same_id_every_time_and_different_ones_differ() {
    assert_eq!(id("UninstallButton"), id("UninstallButton"));
    assert_ne!(id("UninstallButton"), id("MoveButton"));
    assert_ne!(id(""), NodeId(0));
    let list = id("ProgramList");
    assert_ne!(item_id(list, 0), item_id(list, 1));
    assert_ne!(item_id(list, 0), item_id(id("Leftovers"), 0));
}

#[test]
fn two_thousand_rows_have_two_thousand_ids() {
    let list = id("ProgramList");
    let ids: HashSet<_> = (0..2000).map(|i| item_id(list, i)).collect();
    assert_eq!(ids.len(), 2000);
    assert!(!ids.contains(&list));
}

const AREA: Rect = Rect {
    x: 0.0,
    y: 80.0,
    w: 400.0,
    h: 400.0,
};
const ROW_H: f32 = 40.0;
const BUTTON: Rect = Rect {
    x: 8.0,
    y: 8.0,
    w: 100.0,
    h: 32.0,
};

/// A window with a button and a list of the programs `shown` (keys into 2,000 programs),
/// `selected` by key.
fn frame(
    tree: &mut Tree,
    invoked: u32,
    scroll: f32,
    shown: &[usize],
    selected: Option<usize>,
) -> TreeUpdate {
    tree.begin();
    let l = List {
        label: "Programs",
        author: "ProgramList",
        area: AREA,
        row_h: ROW_H,
        count: shown.len(),
        scroll,
        multiselect: true,
    };
    let list_id = list(tree, &l, |i| {
        let k = shown[i];
        Row {
            key: k,
            label: format!("Program {k}"),
            author: format!("Row{k}"),
            selected: Some(k) == selected,
        }
    });
    let b = id("UninstallButton");
    let mut button = button("Uninstall", "UninstallButton", BUTTON, false);
    button.set_description(format!("pressed {invoked}"));
    tree.add(b, button);
    tree.add(tree.root(), window("Purge", 1.5, vec![b, list_id]));
    tree.end(tree.root())
}

fn all() -> Vec<usize> {
    (0..2000).collect()
}

fn shape(tree: &mut Tree, invoked: u32, scroll: f32, selected: Option<usize>) -> TreeUpdate {
    frame(tree, invoked, scroll, &all(), selected)
}

fn sent(u: &TreeUpdate) -> HashSet<NodeId> {
    u.nodes.iter().map(|(i, _)| *i).collect()
}

fn node(u: &TreeUpdate, n: NodeId) -> &Node {
    &u.nodes.iter().find(|(i, _)| *i == n).expect("the node").1
}

fn map(u: &TreeUpdate) -> HashMap<NodeId, Node> {
    u.nodes.iter().cloned().collect()
}

fn row_id(key: usize) -> NodeId {
    item_id(id("ProgramList"), key)
}

#[test]
fn the_first_update_is_the_whole_tree_and_then_only_what_changed() {
    let mut tree = Tree::new(id("Window"));
    let first = shape(&mut tree, 0, 0.0, None);
    assert!(first.tree.is_some());
    assert_eq!(first.nodes.len(), 2 + 1 + 2000);
    assert_eq!(tree.len(), 2003);

    let same = shape(&mut tree, 0, 0.0, None);
    assert!(same.nodes.is_empty(), "nothing changed, nothing sent");
    assert!(same.tree.is_none());

    let pressed = shape(&mut tree, 1, 0.0, None);
    assert_eq!(pressed.nodes.len(), 1);
    assert_eq!(pressed.nodes[0].0, id("UninstallButton"));
}

#[test]
fn a_small_scroll_resends_the_list_and_the_two_rows_its_edges_cut() {
    let mut tree = Tree::new(id("Window"));
    // At 5 and at 10 the viewport shows rows 0 to 10, the first and the last cut.
    shape(&mut tree, 0, 5.0, None);
    let scrolled = shape(&mut tree, 0, 10.0, None);
    let want = HashSet::from([id("ProgramList"), row_id(0), row_id(10)]);
    assert_eq!(sent(&scrolled), want);
    // By a whole row: row 0 leaves, row 10 comes.
    shape(&mut tree, 0, 0.0, None);
    let scrolled = shape(&mut tree, 0, 40.0, None);
    assert_eq!(sent(&scrolled), want);
}

#[test]
fn a_scroll_resends_the_list_and_the_rows_that_gain_or_lose_bounds() {
    let mut tree = Tree::new(id("Window"));
    // At 0, rows 0 to 9 are in view; at 1234, rows 30 to 40.
    shape(&mut tree, 0, 0.0, None);
    let scrolled = shape(&mut tree, 0, 1234.0, None);
    let mut want: HashSet<NodeId> = (0..10).chain(30..41).map(row_id).collect();
    want.insert(id("ProgramList"));
    assert_eq!(sent(&scrolled), want);
}

#[test]
fn selecting_a_row_changes_only_that_row() {
    let mut tree = Tree::new(id("Window"));
    shape(&mut tree, 0, 0.0, None);
    let sel = shape(&mut tree, 0, 0.0, Some(1499));
    assert_eq!(sel.nodes.len(), 1);
    assert_eq!(sel.nodes[0].0, row_id(1499));
    assert_eq!(sel.nodes[0].1.is_selected(), Some(true));
    // and moving the selection changes two
    let moved = shape(&mut tree, 0, 0.0, Some(5));
    assert_eq!(moved.nodes.len(), 2);
}

#[test]
fn the_full_tree_has_every_node_and_the_information() {
    let mut tree = Tree::new(id("Window"));
    shape(&mut tree, 0, 0.0, None);
    shape(&mut tree, 1, 0.0, None);
    let full = tree.full(id("UninstallButton"));
    assert_eq!(full.nodes.len(), 2003);
    assert_eq!(full.tree.as_ref().map(|t| t.root), Some(id("Window")));
    assert_eq!(full.focus, id("UninstallButton"));
    // A focus that is not in the tree gives the root.
    assert_eq!(tree.full(id("Nowhere")).focus, id("Window"));
}

#[test]
fn rows_carry_their_position_and_the_list_its_count_and_scroll() {
    let mut tree = Tree::new(id("Window"));
    let up = shape(&mut tree, 0, 400.0, None);
    let l = node(&up, id("ProgramList"));
    assert_eq!(l.size_of_set(), Some(2000));
    assert!(l.is_multiselectable());
    // A clipping list would make AccessKit hide the rows outside the viewport from UIA.
    assert!(!l.clips_children());
    assert_eq!(l.transform().map(|t| t.as_coeffs()[5]), Some(-400.0));
    // Its bounds, through the transform, are the viewport on screen.
    let b = l.bounds().unwrap();
    assert_eq!((b.y0 - 400.0, b.y1 - 400.0), (80.0, 480.0));
    let row11 = node(&up, row_id(10));
    assert_eq!(row11.position_in_set(), Some(10));
    assert_eq!(row11.author_id(), Some("Row10"));
    let rb = row11.bounds().unwrap();
    assert_eq!((rb.y0, rb.y1), (80.0 + 10.0 * 40.0, 80.0 + 11.0 * 40.0));
}

/// The node a point on screen (logical px) hits, the way AccessKit's hit test goes: children
/// last to first, each through its parent's transform, a node's own bounds only after all its
/// children missed, and no regard for a parent's bounds.
fn hit(u: &HashMap<NodeId, Node>, at: NodeId, x: f64, y: f64) -> Option<NodeId> {
    let n = &u[&at];
    for c in n.children().iter().rev() {
        // The only child with a transform here is the list, a translation.
        let (cx, cy) = match u[c].transform() {
            Some(t) => {
                let k = t.as_coeffs();
                (x - k[4], y - k[5])
            }
            None => (x, y),
        };
        if let Some(h) = hit(u, *c, cx, cy) {
            return Some(h);
        }
    }
    n.bounds()
        .filter(|b| b.x0 <= x && x < b.x1 && b.y0 <= y && y < b.y1)
        .map(|_| at)
}

#[test]
fn a_row_scrolled_over_the_toolbar_has_no_bounds_and_does_not_take_its_hits() {
    let mut tree = Tree::new(id("Window"));
    let up = shape(&mut tree, 0, 400.0, None);
    // Row 8 is two rows above the viewport, at 0 to 40 on screen: over the button. Row 9 is the
    // one just above, and row 10 the first in view.
    assert!(node(&up, row_id(8)).bounds().is_none());
    assert!(node(&up, row_id(9)).bounds().is_none());
    assert!(node(&up, row_id(10)).bounds().is_some());
    // Far rows have none, and still take ScrollIntoView.
    let far = node(&up, row_id(1500));
    assert!(far.bounds().is_none());
    assert!(far.supports_action(Action::ScrollIntoView));
    // Points in logical px, inside the window's scale. The button, not row 8, takes this one.
    assert_eq!(
        hit(&map(&up), id("Window"), 76.0, 28.0),
        Some(id("UninstallButton"))
    );
    assert_eq!(hit(&map(&up), id("Window"), 76.0, 100.0), Some(row_id(10)));
}

#[test]
fn at_any_scroll_the_button_keeps_its_hits_and_a_point_in_view_hits_its_row() {
    let mut seed: u64 = 0x0b7e;
    let mut tree = Tree::new(id("Window"));
    for _ in 0..60 {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let scroll = ((seed >> 33) % 320_000) as f32 / 4.0;
        frame(&mut tree, 0, scroll, &all(), None);
        let full = map(&tree.full(id("Window")));
        for y in [8.5, 20.0, 39.5] {
            for x in [8.5, 60.0, 107.5] {
                assert_eq!(
                    hit(&full, id("Window"), x, y),
                    Some(id("UninstallButton")),
                    "({x}, {y}) at scroll {scroll}"
                );
            }
        }
        // Over the list, the row drawn there: the one at that offset from the first row's top.
        for y in [80.0, 81.5, 250.0, 479.5] {
            let at = ((f64::from(scroll) + y - f64::from(AREA.y)) / f64::from(ROW_H)) as usize;
            let got = hit(&full, id("Window"), 200.0, y);
            if at < 2000 {
                assert_eq!(got, Some(row_id(at)), "y {y} at scroll {scroll}");
            } else {
                assert_ne!(got, Some(id("UninstallButton")));
            }
        }
    }
}

#[test]
fn a_rows_bounds_are_the_part_of_it_in_the_viewport() {
    let mut seed: u64 = 0x5eed;
    let mut tree = Tree::new(id("Window"));
    for _ in 0..40 {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        // Quarter pixels, exact in f32, up to past the end of the list.
        let scroll = ((seed >> 33) % 340_000) as f32 / 4.0;
        frame(&mut tree, 0, scroll, &all(), None);
        let full: HashMap<NodeId, Node> = tree.full(id("Window")).nodes.into_iter().collect();
        let (s, h) = (f64::from(scroll), f64::from(ROW_H));
        // The viewport in content coordinates.
        let (vt, vb) = (f64::from(AREA.y) + s, f64::from(AREA.bottom()) + s);
        for i in 0..2000 {
            let top = f64::from(AREA.y) + i as f64 * h;
            let (y0, y1) = (top.max(vt), (top + h).min(vb));
            let want = (y1 > y0).then_some((y0, y1));
            let got = full[&row_id(i)].bounds().map(|b| (b.y0, b.y1));
            assert_eq!(got, want, "row {i} at scroll {scroll}");
        }
    }
}

#[test]
fn a_filter_resends_the_list_and_only_the_rows_whose_fields_changed() {
    let mut tree = Tree::new(id("Window"));
    shape(&mut tree, 0, 0.0, None);
    // Dropping the tail moves no row that stays: only the list node changes (its children and
    // count), and the rows that went are not sent.
    let head: Vec<usize> = (0..1000).collect();
    let cut = frame(&mut tree, 0, 0.0, &head, None);
    assert_eq!(sent(&cut), HashSet::from([id("ProgramList")]));
    assert_eq!(tree.len(), 2 + 1 + 1000);
    let l = node(&cut, id("ProgramList"));
    assert_eq!(l.children().len(), 1000);
    assert_eq!(l.size_of_set(), Some(1000));

    // Dropping program 2 moves every row after it up by one: those are resent, and nothing else.
    let mut gap = head.clone();
    gap.remove(2);
    let u = frame(&mut tree, 0, 0.0, &gap, None);
    let mut want: HashSet<NodeId> = (3..1000).map(row_id).collect();
    want.insert(id("ProgramList"));
    assert_eq!(sent(&u), want);
    assert_eq!(node(&u, row_id(3)).position_in_set(), Some(2));

    // And bringing it back sends it whole, with those that move down again.
    let back = frame(&mut tree, 0, 0.0, &head, None);
    let mut want: HashSet<NodeId> = (2..1000).map(row_id).collect();
    want.insert(id("ProgramList"));
    assert_eq!(sent(&back), want);
    assert_eq!(node(&back, row_id(2)).label(), Some("Program 2"));
}

#[test]
fn ids_stay_with_their_item_through_a_filter() {
    let mut tree = Tree::new(id("Window"));
    let before = shape(&mut tree, 0, 0.0, None);
    let n1500 = node(&before, row_id(1500)).clone();
    assert_eq!(n1500.label(), Some("Program 1500"));
    // "Program 15": programs 1500 to 1599, and 15, 150 to 159.
    let shown: Vec<usize> = (0..2000)
        .filter(|k| format!("Program {k}").contains("Program 15"))
        .collect();
    frame(&mut tree, 0, 0.0, &shown, Some(1500));
    let full = tree.full(tree.root());
    let l = node(&full, id("ProgramList"));
    let at = shown.iter().position(|k| *k == 1500).unwrap();
    // The row at that position is still program 1500, by the same id.
    assert_eq!(l.children()[at], row_id(1500));
    let now = node(&full, row_id(1500));
    assert_eq!(now.label(), Some("Program 1500"));
    assert_eq!(now.author_id(), Some("Row1500"));
    assert_eq!(now.position_in_set(), Some(at));
    assert_eq!(now.is_selected(), Some(true));
    // A program the filter hides is not in the tree at all.
    assert!(!full.nodes.iter().any(|(i, _)| *i == row_id(1400)));
}

#[test]
fn rows_are_found_back_from_their_ids() {
    let index = RowIndex::new("ProgramList", 0..2000);
    for k in [0, 1, 999, 1999] {
        assert_eq!(index.key(row_id(k)), Some(k));
        assert_eq!(key_of("ProgramList", 2000, row_id(k)), Some(k));
    }
    assert_eq!(index.key(id("UninstallButton")), None);
    assert_eq!(index.key(item_id(id("Other"), 3)), None);
    assert_eq!(key_of("ProgramList", 10, row_id(10)), None);
    // Keys need not be positions or start at 0.
    let sparse = RowIndex::new("ProgramList", [7, 70, 700]);
    assert_eq!(sparse.key(row_id(70)), Some(70));
    assert_eq!(sparse.key(row_id(1)), None);
}

/// A root with the children `kids`, each a button; `extra` are added but not named.
fn small(tree: &mut Tree, kids: &[&str], extra: &[&str], focus: NodeId) -> TreeUpdate {
    tree.begin();
    let r = Rect::new(0.0, 0.0, 10.0, 10.0);
    for k in kids.iter().chain(extra) {
        tree.add(id(k), button(k, k, r, false));
    }
    let children = kids.iter().map(|k| id(k)).collect();
    tree.add(tree.root(), window("W", 1.0, children));
    tree.end(focus)
}

#[test]
fn a_node_left_out_and_back_is_sent_again() {
    let mut tree = Tree::new(id("W"));
    small(&mut tree, &["A", "B"], &[], id("W"));
    let gone = small(&mut tree, &["B"], &[], id("W"));
    assert_eq!(sent(&gone), HashSet::from([id("W")]));
    assert_eq!(tree.len(), 2);
    let back = small(&mut tree, &["A", "B"], &[], id("W"));
    assert_eq!(sent(&back), HashSet::from([id("W"), id("A")]));
}

// A node still added but no longer named is the caller's mistake: a debug build stops on it.
#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "no parent names"))]
fn a_node_no_longer_named_is_dropped_and_sent_again_when_named() {
    let mut tree = Tree::new(id("W"));
    small(&mut tree, &["A", "B"], &[], id("W"));
    // AccessKit removes A when the root stops naming it; so must the tree.
    let gone = small(&mut tree, &["B"], &["A"], id("W"));
    assert_eq!(sent(&gone), HashSet::from([id("W")]));
    assert_eq!(tree.len(), 2);
    let back = small(&mut tree, &["A", "B"], &[], id("W"));
    assert_eq!(sent(&back), HashSet::from([id("W"), id("A")]));
}

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "no parent names"))]
fn an_orphan_is_not_sent() {
    let mut tree = Tree::new(id("W"));
    let u = small(&mut tree, &["A"], &["Orphan"], id("W"));
    // AccessKit panics on a new node that no parent names.
    assert_eq!(sent(&u), HashSet::from([id("W"), id("A")]));
    assert_eq!(tree.len(), 2);
}

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "is not in the tree"))]
fn a_focus_not_in_the_tree_becomes_the_root() {
    let mut tree = Tree::new(id("W"));
    let u = small(&mut tree, &["A"], &[], id("Gone"));
    assert_eq!(u.focus, id("W"));
    let u = small(&mut tree, &["A"], &[], id("A"));
    assert_eq!(u.focus, id("A"));
}

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "names a child"))]
fn a_child_not_added_or_named_twice_is_taken_off_its_parent() {
    let mut tree = Tree::new(id("W"));
    tree.begin();
    let r = Rect::new(0.0, 0.0, 10.0, 10.0);
    tree.add(id("A"), button("A", "A", r, false));
    tree.add(id("G"), group("G", "G", r, vec![id("A"), id("Missing")]));
    tree.add(
        tree.root(),
        window("W", 1.0, vec![id("G"), id("A"), id("W")]),
    );
    let u = tree.end(id("W"));
    let ids: HashSet<NodeId> = sent(&u);
    assert_eq!(ids, HashSet::from([id("W"), id("G"), id("A")]));
    // A is named once only, and the parent names no node that is not sent.
    let named: usize = u.nodes.iter().map(|(_, n)| n.children().len()).sum();
    assert_eq!(named, 2);
}

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "root"))]
fn a_frame_without_its_root_sends_an_empty_window() {
    let mut tree = Tree::new(id("W"));
    tree.begin();
    let u = tree.end(id("W"));
    assert_eq!(sent(&u), HashSet::from([id("W")]));
    assert_eq!(node(&u, id("W")).role(), Role::Window);
}

#[test]
fn a_node_added_this_frame_can_be_changed_and_hidden() {
    let mut tree = Tree::new(id("Window"));
    shape(&mut tree, 0, 0.0, None);
    tree.begin();
    assert!(tree.get_mut(id("ProgramList")).is_none(), "not added yet");
    let l = List {
        label: "Programs",
        author: "ProgramList",
        area: AREA,
        row_h: ROW_H,
        count: 3,
        scroll: 0.0,
        multiselect: true,
    };
    let lid = list(&mut tree, &l, |i| Row {
        key: i,
        label: format!("Program {i}"),
        author: format!("Row{i}"),
        selected: false,
    });
    tree.get_mut(lid).expect("just added").set_hidden();
    tree.add(tree.root(), window("Purge", 1.5, vec![lid]));
    let u = tree.end(tree.root());
    assert!(node(&u, lid).is_hidden());
    // Its rows are unchanged, and hidden with it by AccessKit.
    assert!(!u.nodes.iter().any(|(i, _)| *i == row_id(0)));
}

#[test]
fn a_plain_list_carries_the_size_of_the_set_its_items_need() {
    let mut tree = Tree::new(id("W"));
    tree.begin();
    let items = (0..6).map(|r| Item {
        key: r,
        label: format!("Files {}", r + 1),
        author: format!("Leftover1_{r}"),
        bounds: (r < 3).then(|| Rect::new(0.0, r as f32 * 28.0, 300.0, 28.0)),
    });
    let area = Some(Rect::new(0.0, 0.0, 300.0, 168.0));
    let l = plain_list(&mut tree, "Leftover files", "LeftoverList1", area, items);
    tree.add(tree.root(), window("W", 1.0, vec![l]));
    let u = tree.end(tree.root());
    let ln = node(&u, l);
    assert_eq!(ln.role(), Role::List);
    assert_eq!(ln.size_of_set(), Some(6));
    assert_eq!(ln.label(), Some("Leftover files"));
    assert_eq!(ln.children().len(), 6);
    let first = node(&u, item_id(l, 0));
    assert_eq!(first.role(), Role::ListItem);
    assert_eq!(first.position_in_set(), Some(0));
    assert_eq!(first.author_id(), Some("Leftover1_0"));
    assert!(first.bounds().is_some());
    assert!(node(&u, item_id(l, 5)).bounds().is_none());
    assert_eq!(node(&u, item_id(l, 5)).position_in_set(), Some(5));
}

#[test]
fn nan_and_infinities_give_numbers() {
    let nan = Rect::new(f32::NAN, f32::INFINITY, f32::NAN, f32::NEG_INFINITY);
    let b = bounds(nan);
    assert_eq!((b.x0, b.y0, b.x1, b.y1), (0.0, 0.0, 0.0, 0.0));
    let b = bounds(Rect::new(1.0, f32::NAN, 2.0, 3.0));
    assert_eq!((b.x0, b.y0, b.x1, b.y1), (1.0, 0.0, 3.0, 3.0));
    let r = Rect::new(0.0, 0.0, 10.0, 10.0);
    assert_eq!(
        progress("P", "p", r, Some(f32::NAN)).numeric_value(),
        Some(0.0)
    );
    assert_eq!(
        progress("P", "p", r, Some(f32::INFINITY)).numeric_value(),
        Some(100.0)
    );
    assert_eq!(progress("P", "p", r, Some(-1.0)).numeric_value(), Some(0.0));

    for (scroll, row_h, h) in [
        (f32::NAN, 40.0, 400.0),
        (f32::INFINITY, 40.0, 400.0),
        (0.0, f32::NAN, 400.0),
        (0.0, 0.0, 400.0),
        (0.0, -40.0, 400.0),
        (0.0, 40.0, f32::NAN),
        (-500.0, 40.0, 400.0),
    ] {
        let mut tree = Tree::new(id("W"));
        tree.begin();
        let l = List {
            label: "L",
            author: "L",
            area: Rect::new(0.0, 0.0, 100.0, h),
            row_h,
            count: 50,
            scroll,
            multiselect: false,
        };
        let lid = list(&mut tree, &l, |i| Row {
            key: i,
            label: format!("{i}"),
            author: String::new(),
            selected: false,
        });
        tree.add(tree.root(), window("W", 1.0, vec![lid]));
        let u = tree.end(tree.root());
        assert_eq!(u.nodes.len(), 52, "{scroll} {row_h} {h}");
        for (_, n) in &u.nodes {
            if let Some(b) = n.bounds() {
                assert!(
                    [b.x0, b.y0, b.x1, b.y1].iter().all(|v| v.is_finite()),
                    "{scroll} {row_h} {h}"
                );
            }
            if let Some(t) = n.transform() {
                assert!(t.as_coeffs().iter().all(|v| v.is_finite()));
            }
        }
        let rows_with_bounds = u
            .nodes
            .iter()
            .filter(|(i, n)| *i != lid && n.bounds().is_some())
            .count();
        if !(row_h.is_finite() && row_h > 0.0) || scroll < 0.0 || !h.is_finite() {
            // No pitch, no viewport, or the viewport wholly above the first row.
            assert_eq!(rows_with_bounds, 0, "{scroll} {row_h} {h}");
        } else {
            assert!(rows_with_bounds > 0, "{scroll} {row_h} {h}");
        }
    }
}

#[test]
fn requests_name_their_node() {
    let n = id("X");
    let r = |action, data| ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: n,
        data,
    };
    assert_eq!(request(r(Action::Click, None)), Request::Click(n));
    assert_eq!(request(r(Action::Focus, None)), Request::Focus(n));
    assert_eq!(
        request(r(Action::ScrollIntoView, None)),
        Request::ScrollIntoView(n)
    );
    assert_eq!(request(r(Action::Expand, None)), Request::Expand(n));
    assert_eq!(request(r(Action::Collapse, None)), Request::Collapse(n));
    assert_eq!(
        request(r(Action::SetValue, Some(ActionData::Value("abc".into())))),
        Request::SetValue(n, "abc".into())
    );
    assert_eq!(
        request(r(Action::SetValue, Some(ActionData::NumericValue(3.0)))),
        Request::SetNumber(n, 3.0)
    );
    assert!(matches!(
        request(r(Action::ShowContextMenu, None)),
        Request::Other(_)
    ));
}

#[test]
fn the_widget_nodes_have_their_roles_and_states() {
    let r = Rect::new(0.0, 0.0, 10.0, 10.0);
    assert_eq!(button("B", "b", r, false).role(), Role::Button);
    assert!(button("B", "b", r, true).is_disabled());
    assert_eq!(link("L", "l", r, false).role(), Role::Link);
    let c = check_box("C", "c", r, Some(true), false);
    assert_eq!(
        (c.role(), c.toggled()),
        (Role::CheckBox, Some(Toggled::True))
    );
    assert_eq!(
        check_box("C", "c", r, None, false).toggled(),
        Some(Toggled::Mixed)
    );
    let s = switch("S", "s", r, false, false);
    assert_eq!(
        (s.role(), s.toggled()),
        (Role::Switch, Some(Toggled::False))
    );
    // AccessKit makes a switch a UIA Button; the role description says what it is.
    assert_eq!(s.role_description(), Some(SWITCH_ROLE));
    assert_eq!(s.role_description(), Some("toggle switch"));
    let h = column_header("Size", "SizeColumn", r);
    assert_eq!((h.role(), h.label()), (Role::ColumnHeader, Some("Size")));
    assert!(!h.supports_action(Action::Click));
    let d = combo_box("Theme", "ThemeBox", r, "Dark", true);
    assert_eq!((d.value(), d.is_expanded()), (Some("Dark"), Some(true)));
    let e = expander("Details", "DebugExpander", r, false);
    assert_eq!(e.is_expanded(), Some(false));
    let mb = menu_button("Sort by", "SortMenu", r, true, false);
    assert_eq!(
        (mb.role(), mb.is_expanded(), mb.has_popup()),
        (Role::Button, Some(true), Some(HasPopup::Menu))
    );
    assert!(mb.supports_action(Action::Expand));
    assert!(!mb.is_disabled());
    // A disabled menu button keeps its state and offers no action.
    let quiet = menu_button("Sort by", "SortMenu", r, true, true);
    assert!(quiet.is_disabled());
    assert!(!quiet.supports_action(Action::Click));
    assert!(!quiet.supports_action(Action::Expand));
    let p = progress("Scan", "ScanProgress", r, Some(0.25));
    assert_eq!(p.numeric_value(), Some(25.0));
    assert_eq!(progress("Scan", "x", r, None).numeric_value(), None);
    let t = text("3 selected", "FooterCount", r);
    assert_eq!((t.role(), t.value()), (Role::Label, Some("3 selected")));
    assert_eq!(t.label(), None);
    assert_eq!(
        text_input("Search", "SearchBox", r, "fire", false).value(),
        Some("fire")
    );
    assert_eq!(text_input("Pin", "p", r, "1234", true).value(), None);
    assert_eq!(tab("Apps", "TabApps", r, true, 0).is_selected(), Some(true));
    let dl = dialog("Uninstall?", "ConfirmDialog", r, true, vec![id("Yes")]);
    assert!(dl.is_modal());
    assert_eq!(dl.children(), &[id("Yes")]);
    assert_eq!(group("", "G", r, vec![]).label(), None);
}

#[test]
fn the_window_carries_the_scale() {
    let w = window("Purge", 1.5, vec![]);
    assert_eq!(w.transform().map(|t| t.as_coeffs()[0]), Some(1.5));
}

#[test]
fn tabs_say_where_they_are_and_the_tab_list_how_many_there_are() {
    let r = Rect::new(0.0, 0.0, 80.0, 32.0);
    let names = ["Apps", "Leftovers", "Startup", "Settings"];
    let mut tree = Tree::new(id("W"));
    tree.begin();
    let mut ids = Vec::new();
    for (i, n) in names.iter().enumerate() {
        let tid = id(&format!("Tab{n}"));
        tree.add(tid, tab(n, &format!("Tab{n}"), r, i == 1, i));
        ids.push(tid);
    }
    let strip = id("Pages");
    tree.add(strip, tab_list("Pages", "Pages", r, ids.clone()));
    tree.add(tree.root(), window("W", 1.0, vec![strip]));
    let u = tree.end(tree.root());
    assert_eq!(node(&u, strip).size_of_set(), Some(4));
    for (i, tid) in ids.iter().enumerate() {
        // AccessKit's 0-based position; UIA says one more ("2 of 4").
        assert_eq!(node(&u, *tid).position_in_set(), Some(i));
        assert_eq!(node(&u, *tid).is_selected(), Some(i == 1));
    }
    assert_eq!(tab_list("Empty", "e", r, vec![]).size_of_set(), Some(0));
}

#[test]
fn clip_keeps_the_part_inside_and_nothing_of_what_is_outside() {
    let area = Rect::new(100.0, 50.0, 300.0, 200.0);
    let inside = Rect::new(120.0, 60.0, 40.0, 20.0);
    assert_eq!(clip(inside, area), Some(inside));
    // Cut by the top edge and by the right edge.
    assert_eq!(
        clip(Rect::new(110.0, 30.0, 40.0, 40.0), area),
        Some(Rect::new(110.0, 50.0, 40.0, 20.0))
    );
    assert_eq!(
        clip(Rect::new(380.0, 100.0, 40.0, 10.0), area),
        Some(Rect::new(380.0, 100.0, 20.0, 10.0))
    );
    // Wider than the area both ways: the area itself.
    assert_eq!(clip(Rect::new(0.0, 0.0, 1000.0, 1000.0), area), Some(area));
    // Wholly above, below, beside, or only touching an edge: nothing.
    assert_eq!(clip(Rect::new(120.0, 0.0, 40.0, 50.0), area), None);
    assert_eq!(clip(Rect::new(120.0, 250.0, 40.0, 10.0), area), None);
    assert_eq!(clip(Rect::new(0.0, 60.0, 100.0, 10.0), area), None);
    assert_eq!(clip(Rect::new(500.0, 60.0, 10.0, 10.0), area), None);
    // Empty and negative sizes have nothing inside.
    assert_eq!(clip(Rect::new(120.0, 60.0, 0.0, 10.0), area), None);
    assert_eq!(clip(Rect::new(120.0, 60.0, -10.0, 10.0), area), None);
    assert_eq!(clip(inside, Rect::new(100.0, 50.0, 300.0, -5.0)), None);
    // Nonsense in, nothing out.
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(clip(Rect::new(bad, 60.0, 40.0, 20.0), area), None);
        assert_eq!(clip(Rect::new(120.0, 60.0, bad, 20.0), area), None);
        assert_eq!(clip(inside, Rect::new(100.0, bad, 300.0, 200.0)), None);
        assert_eq!(clip(inside, Rect::new(100.0, 50.0, 300.0, bad)), None);
    }
}

#[test]
fn clip_agrees_with_cutting_each_axis_on_its_own() {
    // Against a second way: the overlap of the two intervals on each axis, kept when both are
    // longer than nothing. Coordinates are whole numbers, so the arithmetic is exact.
    let mut seed = 0x2545_f491_u32;
    let mut next = |m: u32| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) % m
    };
    for _ in 0..2_000 {
        let mut v = [0.0f32; 8];
        for x in v.iter_mut() {
            *x = next(200) as f32 - 50.0;
        }
        let (r, a) = (
            Rect::new(v[0], v[1], v[2], v[3]),
            Rect::new(v[4], v[5], v[6], v[7]),
        );
        let cut = |p0: f32, l0: f32, p1: f32, l1: f32| {
            let (s, e) = (p0.max(p1), (p0 + l0).min(p1 + l1));
            (e > s).then_some((s, e - s))
        };
        let want = match (cut(r.x, r.w, a.x, a.w), cut(r.y, r.h, a.y, a.h)) {
            (Some((x, w)), Some((y, h))) => Some(Rect::new(x, y, w, h)),
            _ => None,
        };
        assert_eq!(clip(r, a), want, "{r:?} in {a:?}");
    }
}
