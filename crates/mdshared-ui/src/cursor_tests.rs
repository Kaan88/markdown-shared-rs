use super::*;
use std::collections::HashSet;

use winit::window::{CursorIcon, ResizeDirection};

/// Every cursor the enum has, so the tests that claim "every" can say so in one place.
const ALL: [Cursor; 14] = [
    Cursor::Default,
    Cursor::Text,
    Cursor::Hand,
    Cursor::Grab,
    Cursor::Grabbing,
    Cursor::NotAllowed,
    Cursor::ResizeEw,
    Cursor::ResizeNs,
    Cursor::ResizeNwse,
    Cursor::ResizeNesw,
    Cursor::Move,
    Cursor::Wait,
    Cursor::Progress,
    Cursor::Crosshair,
];

#[test]
fn every_cursor_maps_to_a_different_icon() {
    // `Hand` is winit's `Pointer`; there is no icon without a cursor and no two cursors sharing
    // one, so the window always shows a different picture for a different answer.
    let icons: HashSet<CursorIcon> = ALL.iter().map(|c| c.icon()).collect();
    assert_eq!(icons.len(), ALL.len());
    assert_eq!(Cursor::Hand.icon(), CursorIcon::Pointer);
}

#[test]
fn the_default_cursor_is_the_arrow_and_maps_to_winit_s_default() {
    assert_eq!(Cursor::default(), Cursor::Default);
    assert_eq!(Cursor::Default.icon(), CursorIcon::Default);
}

#[test]
fn every_resize_direction_gets_the_cursor_for_its_axis_or_diagonal() {
    let cases = [
        (
            ResizeDirection::East,
            Cursor::ResizeEw,
            CursorIcon::EwResize,
        ),
        (
            ResizeDirection::West,
            Cursor::ResizeEw,
            CursorIcon::EwResize,
        ),
        (
            ResizeDirection::North,
            Cursor::ResizeNs,
            CursorIcon::NsResize,
        ),
        (
            ResizeDirection::South,
            Cursor::ResizeNs,
            CursorIcon::NsResize,
        ),
        (
            ResizeDirection::NorthWest,
            Cursor::ResizeNwse,
            CursorIcon::NwseResize,
        ),
        (
            ResizeDirection::SouthEast,
            Cursor::ResizeNwse,
            CursorIcon::NwseResize,
        ),
        (
            ResizeDirection::NorthEast,
            Cursor::ResizeNesw,
            CursorIcon::NeswResize,
        ),
        (
            ResizeDirection::SouthWest,
            Cursor::ResizeNesw,
            CursorIcon::NeswResize,
        ),
    ];
    for (dir, want, icon) in cases {
        assert_eq!(Cursor::of_resize(dir), want, "{dir:?}");
        assert_eq!(Cursor::of_resize(dir).icon(), icon, "{dir:?}");
    }
}

#[test]
fn every_resize_direction_has_an_answer_and_opposites_share_one() {
    // The list above is all eight; this checks that a direction and its opposite, which resize
    // along the same axis, are the same double-headed cursor.
    for (a, b) in [
        (ResizeDirection::East, ResizeDirection::West),
        (ResizeDirection::North, ResizeDirection::South),
        (ResizeDirection::NorthWest, ResizeDirection::SouthEast),
        (ResizeDirection::NorthEast, ResizeDirection::SouthWest),
    ] {
        assert_eq!(Cursor::of_resize(a), Cursor::of_resize(b));
    }
}

#[test]
fn a_header_edge_is_east_west_and_a_title_is_a_hand() {
    assert_eq!(of_header(Some(HeaderHit::Edge(0))), Some(Cursor::ResizeEw));
    assert_eq!(of_header(Some(HeaderHit::Edge(7))), Some(Cursor::ResizeEw));
    assert_eq!(of_header(Some(HeaderHit::Title(0))), Some(Cursor::Hand));
    assert_eq!(of_header(None), None);
}

#[test]
fn a_side_by_side_splitter_is_east_west_and_a_stacked_one_north_south() {
    // Axis names the way the panes lie: Horizontal is side by side with a vertical handle.
    assert_eq!(of_splitter(Axis::Horizontal, true), Some(Cursor::ResizeEw));
    assert_eq!(of_splitter(Axis::Vertical, true), Some(Cursor::ResizeNs));
    assert_eq!(of_splitter(Axis::Horizontal, false), None);
    assert_eq!(of_splitter(Axis::Vertical, false), None);
}

#[test]
fn a_window_edge_answers_only_on_the_border_band() {
    assert_eq!(of_window_edge(None), None);
    assert_eq!(
        of_window_edge(Some(ResizeDirection::West)),
        Some(Cursor::ResizeEw)
    );
    assert_eq!(
        of_window_edge(Some(ResizeDirection::SouthEast)),
        Some(Cursor::ResizeNwse)
    );
}

#[test]
fn the_first_candidate_with_an_answer_wins() {
    assert_eq!(
        pick([None, Some(Cursor::Text), Some(Cursor::Hand)]),
        Cursor::Text
    );
    assert_eq!(pick([Some(Cursor::Grab), Some(Cursor::Hand)]), Cursor::Grab);
    assert_eq!(pick(vec![Some(Cursor::Crosshair)]), Cursor::Crosshair);
}

#[test]
fn pick_with_nothing_to_choose_is_the_default_arrow() {
    assert_eq!(pick(Vec::new()), Cursor::Default);
    assert_eq!(pick([None, None, None]), Cursor::Default);
}

#[test]
fn a_held_drag_cursor_beats_the_hover_cursor_under_the_pointer() {
    // The shape the application passes during a drag: the held cursor first.
    let held = Some(Cursor::ResizeEw);
    let hover = [Some(Cursor::Hand), Some(Cursor::Text)];
    assert_eq!(pick([held].into_iter().chain(hover)), Cursor::ResizeEw);
    // And once the release clears it, the hover answer is back.
    assert_eq!(pick([None].into_iter().chain(hover)), Cursor::Hand);
}

#[test]
fn the_state_answers_only_when_the_cursor_changes() {
    let mut state = CursorState::new();
    assert_eq!(state.current(), None);
    assert_eq!(state.set(Cursor::Text), Some(CursorIcon::Text));
    assert_eq!(state.current(), Some(Cursor::Text));
    // The same cursor again: the loop must not call set_cursor.
    assert_eq!(state.set(Cursor::Text), None);
    assert_eq!(state.set(Cursor::Hand), Some(CursorIcon::Pointer));
    assert_eq!(state.set(Cursor::Hand), None);
    assert_eq!(state.set(Cursor::Text), Some(CursorIcon::Text));
}

#[test]
fn the_state_answers_again_after_forget() {
    let mut state = CursorState::new();
    assert_eq!(state.set(Cursor::Hand), Some(CursorIcon::Pointer));
    assert_eq!(state.set(Cursor::Hand), None);
    state.forget();
    assert_eq!(state.current(), None);
    // The window may no longer show it, so the same cursor is an answer again.
    assert_eq!(state.set(Cursor::Hand), Some(CursorIcon::Pointer));
    assert_eq!(state.set(Cursor::Default), Some(CursorIcon::Default));
}

#[test]
fn an_explicit_default_is_told_once_and_then_silent() {
    let mut state = CursorState::new();
    assert_eq!(state.set(Cursor::Default), Some(CursorIcon::Default));
    assert_eq!(state.set(Cursor::Default), None);
    assert_eq!(state.current(), Some(Cursor::Default));
}

#[test]
fn a_state_made_by_default_is_ready_for_its_first_answer() {
    let mut state = CursorState::default();
    assert_eq!(state.current(), None);
    assert_eq!(state.set(Cursor::Wait), Some(CursorIcon::Wait));
}
