use super::scroll_sync::{line_of_row, row_of_line};
use super::*;
use mdshared_md_render::BlockBox;

fn text(n: usize) -> String {
    (0..n)
        .map(|i| format!("Paragraph number {i}, with some words to wrap.\n\n"))
        .collect()
}

#[test]
fn a_frame_comes_once_the_size_and_text_are_known_and_only_when_something_changed() {
    let mut s = Server::new();
    s.apply(Request::Text {
        text: text(50),
        base_dir: None,
    });
    assert!(s.frame().is_none(), "no size yet");
    s.apply(Request::View {
        width: 400,
        height: 300,
        scale: 1.0,
        dark: false,
    });
    let first = s.frame().expect("a frame");
    assert_eq!((first.width, first.height), (400, 300));
    assert_eq!(first.band, None, "the first frame is whole");
    assert_eq!(first.pixels.len(), 400 * 300 * 4);
    assert!(first.total > 300.0);
    assert!(s.frame().is_none(), "nothing changed");

    // A scroll sends only the rows that came into view.
    s.apply(Request::Scroll { by: 40.0 });
    let moved = s.frame().expect("a frame");
    assert_eq!(moved.top, 40.0);
    assert_eq!(moved.shift, 40);
    assert_eq!(moved.band, Some((260, 40)));
    let mut copy = first.pixels.clone();
    let mut size = (400, 300);
    assert!(moved.apply(&mut copy, &mut size));
}

#[test]
fn the_caret_line_is_brought_into_view_and_a_move_inside_it_keeps_the_page() {
    let mut s = Server::new();
    s.apply(Request::View {
        width: 400,
        height: 300,
        scale: 1.0,
        dark: true,
    });
    s.apply(Request::Text {
        text: text(200),
        base_dir: None,
    });
    s.frame();
    // Paragraph 100 is on line 201.
    s.apply(Request::Follow {
        line: 201,
        column: 0,
    });
    let far = s.frame().expect("moved").top;
    assert!(far > 1000.0, "{far}");
    // The next paragraph is in view: the page stays.
    s.apply(Request::Follow {
        line: 203,
        column: 0,
    });
    assert!(s.frame().is_none_or(|u| u.top == far));
    // Typing keeps the caret's place in view.
    s.apply(Request::Text {
        text: format!("# Title\n\n{}", text(200)),
        base_dir: None,
    });
    let after = s.frame().expect("a frame").top;
    assert!((after - far).abs() < 200.0, "{far} {after}");
}

#[test]
fn after_unfollow_a_new_text_or_size_leaves_the_page_where_the_reader_put_it() {
    let mut s = Server::new();
    s.apply(Request::View {
        width: 400,
        height: 300,
        scale: 1.0,
        dark: true,
    });
    s.apply(Request::Text {
        text: text(200),
        base_dir: None,
    });
    s.apply(Request::Follow { line: 1, column: 0 });
    s.frame();
    s.apply(Request::Unfollow);
    s.apply(Request::Scroll { by: 3000.0 });
    let read = s.frame().expect("scrolled").top;
    assert!(read > 2000.0, "{read}");
    s.apply(Request::Text {
        text: format!(
            "{}
more
",
            text(200)
        ),
        base_dir: None,
    });
    let after = s.frame().map_or(read, |u| u.top);
    assert!((after - read).abs() < 1.0, "{read} {after}");
    s.apply(Request::View {
        width: 420,
        height: 300,
        scale: 1.0,
        dark: true,
    });
    let resized = s.frame().map_or(read, |u| u.top);
    assert!(resized > 1500.0, "back to line 1: {resized}");
}

#[test]
fn lines_and_rows_map_straight_between_the_blocks_and_back() {
    // Blocks on lines 3 and 10, at rows 24 and 200 (the second 100 high), 20 lines.
    let k = vec![(1.0, 0.0), (3.0, 24.0), (10.0, 200.0), (21.0, 300.0)];
    assert_eq!(row_of_line(&k, 3.0), 24.0);
    assert_eq!(row_of_line(&k, 6.5), 112.0);
    assert_eq!(row_of_line(&k, 1.0), 0.0);
    assert_eq!(row_of_line(&k, 99.0), 300.0, "past the end: the end");
    for line in [1.0, 2.0, 3.0, 4.25, 9.9, 15.0, 21.0] {
        let back = line_of_row(&k, row_of_line(&k, line));
        assert!((back - line).abs() < 1e-3, "{line} came back as {back}");
    }
}

#[test]
fn the_editors_top_line_goes_to_the_panes_top_and_the_update_says_where_it_is() {
    let mut s = Server::new();
    s.apply(Request::View {
        width: 400,
        height: 300,
        scale: 1.0,
        dark: false,
    });
    s.apply(Request::Text {
        text: text(200),
        base_dir: None,
    });
    let first = s.frame().expect("a frame");
    assert!(first.line < 2.0, "{}", first.line);
    // Paragraph 100 is on line 201: aligned, its block is at the top.
    s.apply(Request::Align {
        line: 201.0,
        tail: 0.0,
        end: 0.0,
    });
    let at = s.frame().expect("moved");
    assert!(at.top > 1000.0, "{}", at.top);
    assert!((at.line - 201.0).abs() < 0.5, "{}", at.line);
    // Half a line further: the page moves down a little, not a block.
    s.apply(Request::Align {
        line: 201.5,
        tail: 0.0,
        end: 0.0,
    });
    let half = s.frame().expect("moved");
    assert!(
        half.top > at.top && half.top - at.top < 40.0,
        "{} {}",
        at.top,
        half.top
    );
    // The wheel moves the page; the update gives the line now at the top.
    s.apply(Request::Scroll { by: 400.0 });
    let wheel = s.frame().expect("moved");
    assert!(wheel.line > 205.0, "{}", wheel.line);
}

#[test]
fn over_the_editors_last_screen_the_page_goes_on_to_its_bottom_and_back() {
    // 20 lines, the page 3000 rows, its bottom at 2700; the editor ends at line 15, a
    // screen being 5 lines.
    let sync = Sync {
        knots: vec![(1.0, 0.0), (10.0, 1000.0), (21.0, 3000.0)],
        bottom: 2700.0,
        end: Some((10.0, 15.0)),
    };
    assert_eq!(sync.row(1.0), 0.0);
    assert_eq!(
        sync.row(10.0),
        1000.0,
        "before the last screen: the same line"
    );
    assert_eq!(
        sync.row(15.0),
        2700.0,
        "the editor at its end: the page at its own"
    );
    assert_eq!(sync.row(19.0), 2700.0);
    let mut last = -1.0;
    for i in 0..=140 {
        let line = 1.0 + i as f32 * 0.1;
        let row = sync.row(line);
        assert!(row >= last, "the page goes back up at line {line}");
        last = row;
        let back = sync.line(row);
        assert!((back - line).abs() < 0.01, "{line} came back as {back}");
    }
    // The page shorter than the editor's end: it stops at its bottom, and the page at its
    // bottom is the editor at its end.
    let short = Sync {
        bottom: 900.0,
        ..sync
    };
    assert_eq!(short.row(12.0), 900.0);
    assert_eq!(short.line(900.0), 15.0);
    assert!((short.line(short.row(5.0)) - 5.0).abs() < 0.01);
    // No last screen (the editor does not scroll): the same line, the page at its top.
    let still = Sync {
        end: Some((1.0, 1.0)),
        ..short
    };
    assert_eq!(still.row(1.0), 0.0);
}

#[test]
fn the_editor_at_its_end_puts_the_page_at_its_end() {
    let mut s = Server::new();
    s.apply(Request::View {
        width: 400,
        height: 300,
        scale: 1.0,
        dark: false,
    });
    s.apply(Request::Text {
        text: text(200),
        base_dir: None,
    });
    s.frame().expect("a frame");
    // The editor shows 30 lines of the 400: its end is line 371, its last screen from 341.
    let at = |s: &mut Server, line: f32| {
        s.apply(Request::Align {
            line,
            tail: 341.0,
            end: 371.0,
        });
        s.frame().expect("moved")
    };
    let end = at(&mut s, 371.0);
    assert_eq!(end.top, end.total - 300.0, "the page at its bottom");
    assert_eq!(end.line, 371.0);
    let before = at(&mut s, 341.0);
    assert!(before.top < end.top && (before.line - 341.0).abs() < 0.05);
    // The wheel over the page from there: the editor's line goes down with it, to its end.
    s.apply(Request::Scroll { by: 100.0 });
    let wheel = s.frame().expect("moved");
    assert!(wheel.line > 341.0 && wheel.line < 371.0, "{}", wheel.line);
    s.apply(Request::Scroll { by: 1e6 });
    assert_eq!(s.frame().expect("moved").line, 371.0);
}

/// A server with `text` on a 400 by 300 pane, its first frame taken.
fn shown(text: &str) -> Server {
    let mut s = Server::new();
    s.apply(Request::View {
        width: 400,
        height: 300,
        scale: 1.0,
        dark: false,
    });
    s.apply(Request::Text {
        text: text.into(),
        base_dir: None,
    });
    s.frame().expect("a frame");
    s.selection_answer(true);
    s
}

#[test]
fn a_block_is_found_by_its_place_and_a_gap_keeps_room_where_it_was() {
    let mut s = shown(
        "# Title

First.

- one
- two

Last.
",
    );
    let spans: Vec<BlockBox> = s.renderer.page_layout().unwrap().blocks().to_vec();
    let list = spans[2];
    s.apply(Request::BlockAt {
        x: 200.0,
        y: list.y + 2.0,
    });
    let got = s.take_block().flatten().expect("the list");
    assert_eq!((got.first, got.next), (5, Some(8)));
    let (down, across) = got.at.expect("where in it");
    assert!(
        (0.0..0.5).contains(&down) && (0.0..=1.0).contains(&across),
        "{down} {across}"
    );
    s.apply(Request::BlockOfLine { line: 6 });
    let got = s.take_block().flatten().expect("the list again");
    assert_eq!((got.first, got.next, got.at), (5, Some(8), None));
    s.apply(Request::BlockOfLine { line: 7 });
    assert_eq!(
        s.take_block().flatten().map(|b| b.first),
        Some(5),
        "a blank line after it"
    );
    assert_eq!(s.take_block(), None, "once per question");
    s.apply(Request::BlockAt { x: 200.0, y: 299.0 });
    assert_eq!(s.take_block(), Some(None), "below the last block");

    s.apply(Request::Gap {
        gap: Some((5, 6, 90.0)),
    });
    s.frame().expect("a frame");
    assert_eq!(s.gap_y(), Some(list.y));
    let after = s.renderer.page_layout().unwrap().blocks()[3];
    assert_eq!(after.y, list.y + 90.0);
    // A new text keeps the gap; clearing it brings the list back.
    s.apply(Request::Text {
        text: "# Title

First.

- one
- two
- three

Last.
"
        .into(),
        base_dir: None,
    });
    s.frame().expect("a frame");
    assert_eq!(s.gap_y(), Some(list.y));
    s.apply(Request::Gap { gap: None });
    s.frame().expect("a frame");
    assert_eq!(s.gap_y(), None);
    assert!(s.renderer.page_layout().unwrap().blocks()[2].height > 0.0);
}

/// Applies `reqs` as one batch and returns what the answer carries.
fn answer(s: &mut Server, reqs: Vec<Request>) -> (Option<Vec<[f32; 4]>>, Option<Copied>) {
    for r in reqs {
        s.apply(r);
    }
    let drew = s.frame().is_some();
    s.selection_answer(drew)
}

fn copy(s: &mut Server) -> String {
    answer(s, vec![Request::Copy]).1.expect("an answer").text
}

#[test]
fn a_click_on_a_task_box_gives_its_mark_and_selects_nothing() {
    let text = "Tasks:\n\n- [ ] one\n- [x] two\n\n```\n- [ ] code\n```\n";
    let mut s = shown(text);
    // Every point of the pane a click would tick, and what it gives.
    let mut found = Vec::new();
    for y in (0..300).step_by(2) {
        for x in (0..200).step_by(2) {
            s.apply(Request::Press {
                x: x as f32,
                y: y as f32,
                clicks: 1,
            });
            s.apply(Request::Release);
            if let Some(t) = s.take_task()
                && !found.contains(&t)
            {
                found.push(t);
            }
        }
    }
    // The two boxes, by the bytes between their brackets; the fenced one is not a box.
    assert_eq!(found, [(11, true), (21, false)]);
    assert_eq!(&text[11..12], " ");
    assert_eq!(&text[21..22], "x");
    // A click on a box selects nothing; a double click there selects a word as before.
    let (bx, by) = (0..300)
        .step_by(2)
        .flat_map(|y| (0..200).step_by(2).map(move |x| (x as f32, y as f32)))
        .find(|&(x, y)| s.task_at(x, y).is_some())
        .expect("a box");
    let (rects, _) = answer(&mut s, vec![Request::SelectAll]);
    assert!(rects.is_some_and(|r| !r.is_empty()));
    let (rects, _) = answer(
        &mut s,
        vec![Request::Press {
            x: bx,
            y: by,
            clicks: 1,
        }],
    );
    assert_eq!(rects, Some(vec![]), "the click cleared the selection");
    assert!(s.take_task().is_some());
    answer(
        &mut s,
        vec![Request::Press {
            x: bx,
            y: by,
            clicks: 2,
        }],
    );
    assert!(s.take_task().is_none());
    // Over the box the cursor is a hand; over the words, the text cursor.
    s.apply(Request::Hover { x: bx, y: by });
    assert_eq!(s.take_pointer(), Some(Pointer::Hand));
    assert_eq!(s.take_pointer(), None, "once per hover");
    let on_text = (0..300)
        .step_by(2)
        .flat_map(|y| (0..200).step_by(2).map(move |x| (x as f32, y as f32)))
        .any(|(x, y)| s.pointer_at(x, y) == Pointer::Text);
    assert!(on_text);
    assert_eq!(s.pointer_at(399.0, 299.0), Pointer::Arrow);
    // A text that no longer agrees with the page about its boxes ticks nothing.
    s.source = "- [ ] one\n".into();
    s.apply(Request::Press {
        x: bx,
        y: by,
        clicks: 1,
    });
    assert!(s.take_task().is_none());
}

#[test]
fn the_text_a_tick_brings_leaves_the_page_where_it_is() {
    // A long page with a box far down, the caret (Follow) at the first line.
    let mut text: String = (0..120).map(|i| format!("Line {i}.\n\n")).collect();
    text.push_str("- [ ] far down\n");
    let mut s = shown(&text);
    s.apply(Request::Follow { line: 1, column: 0 });
    s.apply(Request::Scroll { by: 1e6 });
    let bottom = s.frame().expect("scrolled").top;
    assert!(bottom > 0.0);
    let (bx, by) = (0..300)
        .step_by(2)
        .flat_map(|y| (0..200).step_by(2).map(move |x| (x as f32, y as f32)))
        .find(|&(x, y)| s.task_at(x, y).is_some())
        .expect("the box in view");
    s.apply(Request::Press {
        x: bx,
        y: by,
        clicks: 1,
    });
    s.apply(Request::Release);
    let (at, checked) = s.take_task().expect("ticked");
    assert!(checked);
    // The editor's answer: the same text with the mark changed, and no Follow (the caret
    // did not move). The page stays at the box.
    let mut ticked = text.clone();
    ticked.replace_range(at as usize..at as usize + 1, "x");
    s.apply(Request::Text {
        text: ticked,
        base_dir: None,
    });
    let after = s.frame().map_or(s.top, |u| u.top);
    assert_eq!(after, bottom);
    // A text typed after that follows the caret again.
    s.apply(Request::Text {
        text: format!("x{text}"),
        base_dir: None,
    });
    assert!(s.frame().map_or(s.top, |u| u.top) < bottom);
}

/// Audit 4, E10: a view larger than a frame can carry is drawn at the largest that can.
#[test]
fn a_view_larger_than_a_frame_can_carry_is_drawn_at_the_largest_one() {
    let mut s = Server::new();
    s.apply(Request::Text {
        text: "# Wide
"
        .into(),
        base_dir: None,
    });
    s.apply(Request::View {
        width: wire::MAX_SIDE + 808,
        height: 12,
        scale: 1.0,
        dark: false,
    });
    let f = s.frame().expect("drawn");
    assert_eq!((f.width, f.height), (wire::MAX_SIDE, 12));
    assert_eq!(f.pixels.len(), wire::MAX_SIDE as usize * 12 * 4);
}

/// Audit 4, E7: a tick and a new text in the same batch (the reader clicked a box while the
/// editor sent what was typed): the tick's offset was into the old text, so it is not
/// answered, and the page follows the caret for the new text instead of staying still.
#[test]
fn a_tick_followed_by_a_new_text_before_its_answer_is_dropped() {
    let mut text: String = (0..120)
        .map(|i| {
            format!(
                "Line {i}.

"
            )
        })
        .collect();
    text.push_str(
        "- [ ] far down
",
    );
    let mut s = shown(&text);
    s.apply(Request::Follow { line: 1, column: 0 });
    s.apply(Request::Scroll { by: 1e6 });
    let bottom = s.frame().expect("scrolled").top;
    let (bx, by) = (0..300)
        .step_by(2)
        .flat_map(|y| (0..200).step_by(2).map(move |x| (x as f32, y as f32)))
        .find(|&(x, y)| s.task_at(x, y).is_some())
        .expect("the box in view");
    s.apply(Request::Press {
        x: bx,
        y: by,
        clicks: 1,
    });
    s.apply(Request::Release);
    // Three characters typed at the top: every offset below them moved.
    s.apply(Request::Text {
        text: format!("ab {text}"),
        base_dir: None,
    });
    assert_eq!(
        s.take_task(),
        None,
        "no tick against a text it was not found in"
    );
    assert!(
        s.frame().map_or(s.top, |u| u.top) < bottom,
        "the page follows the caret"
    );
}

#[test]
fn a_drag_selects_the_text_under_it_and_copy_gives_it() {
    let mut s = shown("First paragraph here.\n\nSecond one, a little longer.\n");
    // A click that does not move selects nothing and copies nothing.
    let (rects, _) = answer(
        &mut s,
        vec![Request::Press {
            x: 30.0,
            y: 30.0,
            clicks: 1,
        }],
    );
    assert!(rects.is_none(), "a caret is not a selection");
    answer(&mut s, vec![Request::Release]);
    assert_eq!(copy(&mut s), "");
    // From the start of the first line down past the second paragraph: both, as text.
    let (rects, _) = answer(
        &mut s,
        vec![
            Request::Press {
                x: 0.0,
                y: 20.0,
                clicks: 1,
            },
            Request::Drag { x: 399.0, y: 250.0 },
            Request::Release,
        ],
    );
    let rects = rects.expect("the selection is shown");
    assert!(rects.len() >= 2, "a rectangle a line: {rects:?}");
    let text = copy(&mut s);
    assert!(
        text.contains("First paragraph here.") && text.contains("a little longer."),
        "{text:?}"
    );
    // The same again changes nothing to show.
    assert!(answer(&mut s, vec![]).0.is_none());
    // A click elsewhere clears it.
    let (rects, _) = answer(
        &mut s,
        vec![
            Request::Press {
                x: 30.0,
                y: 30.0,
                clicks: 1,
            },
            Request::Release,
        ],
    );
    assert_eq!(rects, Some(vec![]));
    assert_eq!(copy(&mut s), "");
}

#[test]
fn two_clicks_take_a_word_three_the_paragraph_and_all_takes_everything() {
    let mut s = shown("Alpha beta gamma.\n\nNext paragraph.\n");
    let press = |clicks| Request::Press {
        x: 2.0,
        y: 30.0,
        clicks,
    };
    answer(&mut s, vec![press(2), Request::Release]);
    assert_eq!(copy(&mut s), "Alpha", "a double click keeps its word");
    answer(&mut s, vec![press(3), Request::Release]);
    assert_eq!(copy(&mut s), "Alpha beta gamma.");
    answer(&mut s, vec![Request::SelectAll]);
    let all = copy(&mut s);
    assert!(
        all.starts_with("Alpha") && all.contains("Next paragraph."),
        "{all:?}"
    );
    let html = answer(&mut s, vec![Request::Copy]).1.unwrap().html;
    assert!(html.contains("<p>"), "{html}");
    answer(&mut s, vec![Request::Unselect]);
    assert_eq!(copy(&mut s), "");
}

#[test]
fn a_new_text_clears_the_selection_and_the_rectangles_follow_the_page() {
    let mut s = shown(&text(100));
    let (rects, _) = answer(&mut s, vec![Request::SelectAll]);
    let first = rects.expect("shown")[0];
    // The page scrolled: the rectangles are page positions, so they stay as they were.
    let (rects, _) = answer(&mut s, vec![Request::Scroll { by: 500.0 }]);
    assert!(rects.is_none_or(|r| r[0] == first));
    // A new text: the offsets were into the old one.
    let (rects, _) = answer(
        &mut s,
        vec![Request::Text {
            text: text(99),
            base_dir: None,
        }],
    );
    assert_eq!(rects, Some(vec![]));
    assert_eq!(copy(&mut s), "");
}

#[test]
fn the_loop_draws_once_for_everything_that_was_waiting() {
    let (tx, rx) = mpsc::channel();
    tx.send(Request::View {
        width: 200,
        height: 100,
        scale: 1.0,
        dark: false,
    })
    .unwrap();
    tx.send(Request::Text {
        text: "# One\n".into(),
        base_dir: None,
    })
    .unwrap();
    tx.send(Request::Text {
        text: "# Two\n".into(),
        base_dir: None,
    })
    .unwrap();
    tx.send(Request::Quit).unwrap();
    let mut answers = Vec::new();
    serve(rx, |u| {
        answers.push(u.answers);
        true
    });
    // Quit came with the rest, so nothing was drawn; a batch is answered once.
    assert!(answers.len() <= 1, "{answers:?}");
}
