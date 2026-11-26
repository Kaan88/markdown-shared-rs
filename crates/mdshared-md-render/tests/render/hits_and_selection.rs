use super::*;

#[test]
fn a_click_maps_back_to_the_word_under_it() {
    let doc = mdshared_md_core::parse("# Title\n\nHello **world** after.\n");
    let options = Options {
        width: 500.0,
        scale: 1.0,
        theme: LIGHT,
        max_height: None,
        base_dir: None,
    };
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&doc, &options);
    renderer.view_frame(&doc, &options, 0.0, 400.0);
    let world = doc.text.find("world").expect("the word is in the document") as u32;
    let rects = renderer.selection_rects(&doc, &options, world, world + 5);
    assert_eq!(rects.len(), 1, "one line, one rect");
    let rect = rects[0];
    assert!(rect.w > 10.0, "{rect:?}");
    let hit = renderer
        .hit_test(&doc, &options, rect.x + 1.0, rect.y + rect.h / 2.0)
        .expect("a click in the text");
    assert_eq!(hit.offset, world, "the click is on the first letter");
    assert_eq!(
        &doc.text[hit.offset as usize..hit.offset as usize + 5],
        "world"
    );
    assert_eq!(hit.link, None);
    // A click past the end of the line still lands on it, at the last caret stop.
    let hit = renderer
        .hit_test(&doc, &options, rect.x + 400.0, rect.y + rect.h / 2.0)
        .expect("a click to the right");
    assert_eq!(hit.block, renderer.page_layout().unwrap().blocks()[1].block);
    let after = doc.text.find("after").unwrap() as u32;
    assert!(hit.offset >= after, "offset {}", hit.offset);
}

#[test]
fn a_click_finds_a_link_and_a_footnote_reference() {
    let doc = mdshared_md_core::parse(
        "A [link](https://example.com) and a reference[^n].\n\n[^n]: The note.\n",
    );
    let options = Options {
        width: 640.0,
        scale: 1.0,
        theme: LIGHT,
        max_height: None,
        base_dir: None,
    };
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&doc, &options);
    renderer.view_frame(&doc, &options, 0.0, 400.0);
    let link = doc.text.find("link").expect("link text") as u32;
    let rect = renderer.selection_rects(&doc, &options, link, link + 4)[0];
    let hit = renderer
        .hit_test(&doc, &options, rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
        .expect("a click on the link");
    let index = hit.link.expect("a link under the pointer");
    assert_eq!(
        doc.str(doc.links[index as usize].dest),
        "https://example.com"
    );
    // The footnote mark has no text of its own in the document, so sweep the line for it.
    let text = &doc.blocks[renderer.page_layout().unwrap().blocks()[0].block as usize].text;
    let row = renderer.selection_rects(&doc, &options, text.start, text.end)[0];
    let mut found = None;
    let mut x = row.x;
    while x < row.x + row.w {
        if let Some(hit) = renderer.hit_test(&doc, &options, x, row.y + row.h / 2.0)
            && let Some(note) = hit.footnote
        {
            found = Some((note, hit.offset));
            break;
        }
        x += 1.0;
    }
    let (note, offset) = found.expect("the footnote mark is hittable");
    assert_eq!(doc.footnotes[note as usize].number, 1);
    assert_eq!(
        offset,
        doc.spans
            .iter()
            .find(|s| matches!(s.kind, mdshared_md_core::model::SpanKind::FootnoteRef(_)))
            .unwrap()
            .start
    );
    assert!(
        renderer.page_layout().unwrap().footnote_y(1).is_some(),
        "the footnote list has a position"
    );
}

#[test]
fn blocks_are_found_by_page_row() {
    let doc = mdshared_md_core::parse("first paragraph\n\nsecond paragraph\n");
    let page = mdshared_md_render::layout(&doc, &Options::light());
    let blocks = page.blocks();
    assert_eq!(blocks.len(), 2);
    assert_eq!(
        page.block_at(blocks[0].y + 1.0).unwrap().block,
        blocks[0].block
    );
    assert_eq!(
        page.block_at(blocks[1].y + 1.0).unwrap().block,
        blocks[1].block
    );
    // A row above the content and one in the bottom padding both clamp to a block.
    assert_eq!(page.block_at(0.0).unwrap().block, blocks[0].block);
    assert_eq!(
        page.block_at(page.total_height() - 1.0).unwrap().block,
        blocks[1].block
    );
    // A nested block reports the row of the top-level block that holds it.
    let list = mdshared_md_core::parse("- item\n");
    let list_page = mdshared_md_render::layout(&list, &Options::light());
    let top = list_page.blocks()[0].block;
    let child = list.children(top).next().expect("a list item");
    assert_eq!(list_page.y_of(&list, child), Some(list_page.blocks()[0].y));
    assert_eq!(list_page.y_of(&list, 0), Some(list_page.blocks()[0].y));
}

#[test]
fn hit_testing_and_selection_do_not_change_the_frame() {
    let doc = fixture("showcase.md");
    let options = Options {
        width: 520.0,
        scale: 1.0,
        theme: LIGHT,
        max_height: None,
        base_dir: None,
    };
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&doc, &options);
    renderer.view_frame(&doc, &options, 0.0, 400.0);
    // Ask for the same view once more so the delta is explicitly empty, then use hit testing.
    let before = renderer
        .view_frame(&doc, &options, 0.0, 400.0)
        .bitmap
        .pixels
        .clone();
    let text = &doc.blocks[renderer.page_layout().unwrap().blocks()[0].block as usize].text;
    let _ = renderer.selection_rects(&doc, &options, text.start, text.end);
    let heading = doc.headings.last().expect("a heading").block;
    let last = *renderer
        .page_layout()
        .unwrap()
        .blocks()
        .iter()
        .find(|block| block.block == heading)
        .expect("the heading is a top-level block");
    let hit = renderer
        .hit_test(&doc, &options, 30.0, last.y + 5.0)
        .expect("a click on a block far below the viewport");
    assert_eq!(hit.block, last.block, "the block was laid out on demand");
    let frame = renderer.view_frame(&doc, &options, 0.0, 400.0);
    assert_eq!(
        frame.band,
        Some((0, 0)),
        "the frame delta is about the document only"
    );
    assert_eq!(frame.bitmap.pixels, before, "the frame is byte-identical");
}

#[test]
fn a_selection_that_wraps_gives_one_rect_per_line() {
    let doc = mdshared_md_core::parse(
        "This paragraph is long enough that a narrow page breaks it over several lines, which is \
         what the selection needs in order to produce more than one rectangle on the page.\n",
    );
    let options = Options {
        width: 300.0,
        scale: 1.0,
        theme: LIGHT,
        max_height: None,
        base_dir: None,
    };
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&doc, &options);
    let text = &doc.blocks[renderer.page_layout().unwrap().blocks()[0].block as usize].text;
    let rects = renderer.selection_rects(&doc, &options, text.start, text.end);
    assert!(rects.len() > 1, "{rects:?}");
    // Every rect is on its own row band and inside the page.
    for rect in &rects {
        assert!(rect.x >= 0.0 && rect.w > 0.0 && rect.h > 0.0);
        assert!(rect.y >= 0.0 && rect.y + rect.h <= renderer.page_layout().unwrap().total_height());
    }
}

#[test]
fn a_details_summary_toggles_and_the_page_remeasures() {
    let doc = mdshared_md_core::parse(
        "# Title\n\n<details>\n<summary>More</summary>\n\nHidden body text.\n\n</details>\n",
    );
    let details = doc
        .blocks
        .iter()
        .position(|block| {
            matches!(
                block.kind,
                mdshared_md_core::model::BlockKind::Details { .. }
            )
        })
        .expect("a details block") as u32;
    let summary = doc
        .children(details)
        .find(|child| {
            matches!(
                doc.blocks[*child as usize].kind,
                mdshared_md_core::model::BlockKind::Summary
            )
        })
        .expect("a summary");
    let options = Options {
        width: 500.0,
        scale: 1.0,
        theme: LIGHT,
        max_height: None,
        base_dir: None,
    };
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&doc, &options);
    renderer.view_frame(&doc, &options, 0.0, 400.0);
    let closed = renderer.page_layout().unwrap().clone();
    let text = doc.blocks[summary as usize].text;
    let rect = renderer.selection_rects(&doc, &options, text.start, text.end)[0];
    let hit = renderer
        .hit_test(&doc, &options, rect.x + 2.0, rect.y + rect.h / 2.0)
        .expect("a click on the summary");
    assert_eq!(hit.details, Some(details), "the summary names its details");
    assert!(!renderer.details_open(&doc, details), "closed as written");
    renderer.set_details(&doc, details, true);
    assert!(renderer.details_open(&doc, details));
    // The toggled block is measured again by the next frame; nothing else is.
    renderer.view_frame(&doc, &options, 0.0, 400.0);
    let open = renderer.page_layout().unwrap().clone();
    assert!(
        open.total_height() > closed.total_height() + 20.0,
        "opening shows the body: {} vs {}",
        open.total_height(),
        closed.total_height()
    );
    renderer.set_details(&doc, details, false);
    renderer.view_frame(&doc, &options, 0.0, 400.0);
    let again = renderer.page_layout().unwrap().clone();
    assert!(
        (again.total_height() - closed.total_height()).abs() < 0.5,
        "closing restores the height: {} vs {}",
        again.total_height(),
        closed.total_height()
    );
}

#[test]
fn a_missing_image_falls_back_to_the_placeholder() {
    let dir = temp_dir("image-missing");
    let doc = mdshared_md_core::parse("![alt text](nope.png)\n");
    let rendered = render(
        &doc,
        &Options {
            width: 400.0,
            scale: 1.0,
            theme: LIGHT,
            max_height: None,
            base_dir: Some(dir),
        },
    );
    assert!(rendered.content_height > 0.0);
    // The placeholder is muted text, so the page is not simply empty.
    let ink = rendered
        .bitmap
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] != LIGHT.bg[2] || p[1] != LIGHT.bg[1] || p[2] != LIGHT.bg[0])
        .count();
    assert!(ink > 100, "placeholder pixels: {ink}");
}

#[test]
fn subpixel_text_uses_rgb_stripe_order() {
    // Phase 0 found zeno's `Format::Subpixel` swapping red and blue, which put coloured fringes on
    // the wrong side of every stem. On an RGB-stripe panel red is the left third of a pixel and
    // blue the right, so dark ink starting partway into a pixel covers blue more than red: the left
    // edge of a dark stem on a light page is red-tinted (R > B) and its right edge blue-tinted. A
    // swap in the mask format or in the blit flips both.
    let doc = mdshared_md_core::parse("# llllllll IIIIIIII\n");
    let rendered = render(
        &doc,
        &Options {
            width: 780.0,
            scale: 1.0,
            theme: LIGHT,
            max_height: None,
            base_dir: None,
        },
    );
    let bitmap = &rendered.bitmap;
    let bg = &bitmap.pixels[0..3];
    let (mut left, mut right) = ((0, 0), (0, 0)); // (red-tinted, blue-tinted)
    for y in 0..bitmap.height {
        let row = &bitmap.pixels[(y * bitmap.width * 4) as usize..][..(bitmap.width * 4) as usize];
        let inked = |x: u32| row[(x * 4) as usize..][..3] != *bg;
        for x in 1..bitmap.width - 1 {
            let p = &row[(x * 4) as usize..][..3];
            let (b, r) = (p[0] as i32, p[2] as i32);
            if (b - r).abs() < 8 {
                continue;
            }
            let tally = if inked(x) && !inked(x - 1) {
                &mut left
            } else if inked(x) && !inked(x + 1) {
                &mut right
            } else {
                continue;
            };
            if r > b { tally.0 += 1 } else { tally.1 += 1 }
        }
    }
    assert!(
        left.0 + left.1 > 20 && right.0 + right.1 > 20,
        "too few edges: {left:?} {right:?}"
    );
    assert!(
        left.0 > left.1 * 3,
        "left edges should be red-tinted: {left:?}"
    );
    assert!(
        right.1 > right.0 * 3,
        "right edges should be blue-tinted: {right:?}"
    );
}
