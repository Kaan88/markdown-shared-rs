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
    let ids: std::collections::HashSet<_> = (0..2000).map(|i| item_id(list, i)).collect();
    assert_eq!(ids.len(), 2000);
    assert!(!ids.contains(&list));
}

fn shape(tree: &mut Tree, invoked: u32, scroll: f32, selected: Option<usize>) -> TreeUpdate {
    tree.begin();
    let area = Rect::new(0.0, 80.0, 400.0, 400.0);
    let l = List {
        label: "Programs",
        author: "ProgramList",
        area,
        row_h: 40.0,
        count: 2000,
        scroll,
        multiselect: true,
    };
    let list_id = list(tree, &l, |i| {
        (
            format!("Program {i}"),
            format!("Row{i}"),
            Some(i) == selected,
        )
    });
    let b = id("UninstallButton");
    let mut button = button(
        "Uninstall",
        "UninstallButton",
        Rect::new(8.0, 8.0, 100.0, 32.0),
        false,
    );
    button.set_description(format!("pressed {invoked}"));
    tree.add(b, button);
    tree.add(tree.root(), window("Purge", 1.5, vec![b, list_id]));
    tree.end(tree.root())
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
fn a_scroll_changes_only_the_list_node() {
    let mut tree = Tree::new(id("Window"));
    shape(&mut tree, 0, 0.0, None);
    let scrolled = shape(&mut tree, 0, 1234.0, None);
    assert_eq!(scrolled.nodes.len(), 1);
    assert_eq!(scrolled.nodes[0].0, id("ProgramList"));
}

#[test]
fn selecting_a_row_changes_only_that_row() {
    let mut tree = Tree::new(id("Window"));
    shape(&mut tree, 0, 0.0, None);
    let sel = shape(&mut tree, 0, 0.0, Some(1499));
    assert_eq!(sel.nodes.len(), 1);
    assert_eq!(sel.nodes[0].0, item_id(id("ProgramList"), 1499));
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
}

#[test]
fn rows_carry_their_position_and_the_list_its_count_and_scroll() {
    let mut tree = Tree::new(id("Window"));
    let up = shape(&mut tree, 0, 400.0, None);
    let list_id = id("ProgramList");
    let (_, l) = up.nodes.iter().find(|(i, _)| *i == list_id).unwrap();
    assert_eq!(l.size_of_set(), Some(2000));
    assert!(l.is_multiselectable());
    // A clipping list would make AccessKit hide the rows outside the viewport from UIA.
    assert!(!l.clips_children());
    assert_eq!(l.transform().map(|t| t.as_coeffs()[5]), Some(-400.0));
    // Its bounds, through the transform, are the viewport on screen.
    let b = l.bounds().unwrap();
    assert_eq!((b.y0 - 400.0, b.y1 - 400.0), (80.0, 480.0));
    let (_, row5) = up
        .nodes
        .iter()
        .find(|(i, _)| *i == item_id(list_id, 4))
        .unwrap();
    assert_eq!(row5.position_in_set(), Some(4));
    assert_eq!(row5.author_id(), Some("Row4"));
    let rb = row5.bounds().unwrap();
    assert_eq!((rb.y0, rb.y1), (80.0 + 4.0 * 40.0, 80.0 + 5.0 * 40.0));
}

#[test]
fn rows_are_found_back_from_their_ids() {
    let index = RowIndex::new("ProgramList", 2000);
    let list_id = id("ProgramList");
    for i in [0, 1, 999, 1999] {
        assert_eq!(index.row(item_id(list_id, i)), Some(i));
        assert_eq!(row_of("ProgramList", 2000, item_id(list_id, i)), Some(i));
    }
    assert_eq!(index.row(id("UninstallButton")), None);
    assert_eq!(index.row(item_id(id("Other"), 3)), None);
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
    let d = combo_box("Theme", "ThemeBox", r, "Dark", true);
    assert_eq!((d.value(), d.is_expanded()), (Some("Dark"), Some(true)));
    let e = expander("Details", "DebugExpander", r, false);
    assert_eq!(e.is_expanded(), Some(false));
    let p = progress("Scan", "ScanProgress", r, Some(0.25));
    assert_eq!(p.numeric_value(), Some(25.0));
    assert_eq!(progress("Scan", "x", r, None).numeric_value(), None);
    assert_eq!(
        text("3 selected", "FooterCount", r).value(),
        Some("3 selected")
    );
    assert_eq!(
        text_input("Search", "SearchBox", r, "fire", false).value(),
        Some("fire")
    );
    assert_eq!(text_input("Pin", "p", r, "1234", true).value(), None);
    assert_eq!(tab("Apps", "TabApps", r, true).is_selected(), Some(true));
    let dl = dialog("Uninstall?", "ConfirmDialog", r, true, vec![id("Yes")]);
    assert!(dl.is_modal());
    assert_eq!(dl.children(), &[id("Yes")]);
}

#[test]
fn the_window_carries_the_scale() {
    let w = window("Purge", 1.5, vec![]);
    assert_eq!(w.transform().map(|t| t.as_coeffs()[0]), Some(1.5));
}
