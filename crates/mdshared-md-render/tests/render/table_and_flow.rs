use super::*;

#[test]
fn table_attributes_are_honoured() {
    let border = [LIGHT.border[2], LIGHT.border[1], LIGHT.border[0]];
    let columns_of = |r: &mdshared_md_render::Rendered| {
        let width = r.bitmap.width as usize;
        let mut span = (usize::MAX, 0);
        for (i, p) in r.bitmap.pixels.as_chunks::<4>().0.iter().enumerate() {
            if p[..3] == border {
                span = (span.0.min(i % width), span.1.max(i % width));
            }
        }
        span
    };
    // `width` sets the table's width and `align` centres it.
    let full = columns_of(&page(
        "<table width=\"100%\"><tr><td>a</td><td>b</td></tr></table>\n",
    ));
    assert_eq!(full, (40, 739));
    let half = columns_of(&page(
        "<table width=\"50%\" align=\"center\"><tr><td>a</td><td>b</td></tr></table>\n",
    ));
    assert_eq!(half.1 - half.0 + 1, 350);
    assert!((half.0 as i32 - 215).abs() <= 1, "{half:?}");

    // A row's `height` makes it taller, and `valign` moves the text down inside it.
    let plain = page("<table><tr><td>a</td></tr></table>\n");
    let tall = page("<table><tr height=\"120\"><td>a</td></tr></table>\n");
    assert!(tall.bitmap.height >= plain.bitmap.height + 80);
    let ink_rows = |markdown: &str| {
        let r = page(markdown);
        let width = r.bitmap.width as usize;
        r.bitmap
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .position(|p| p[1] < 0x60)
            .map(|i| i / width)
            .unwrap()
    };
    let top = ink_rows("<table><tr height=\"120\"><td>a</td></tr></table>\n");
    let middle = ink_rows("<table><tr height=\"120\"><td valign=\"middle\">a</td></tr></table>\n");
    let bottom = ink_rows("<table><tr height=\"120\" valign=\"bottom\"><td>a</td></tr></table>\n");
    assert!(
        top + 30 < middle && middle + 30 < bottom,
        "{top} {middle} {bottom}"
    );

    // A row's `align` reaches its cells, and a cell's `width` sets its column.
    let left = ink_columns(&page("<table width=\"100%\"><tr><td>a</td></tr></table>\n")).0;
    let right = ink_columns(&page(
        "<table width=\"100%\"><tr align=\"right\"><td>a</td></tr></table>\n",
    ))
    .0;
    assert!(right > left + 500, "{left} {right}");
    let second = |markdown: &str| {
        let r = page(markdown);
        let width = r.bitmap.width as usize;
        // The x of the `b`, the last ink on the row.
        r.bitmap
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, p)| p[1] < 0x60)
            .map(|(i, _)| i % width)
            .max()
            .unwrap()
    };
    let even = second("<table width=\"100%\"><tr><td>a</td><td>b</td></tr></table>\n");
    let wide =
        second("<table width=\"100%\"><tr><td width=\"80%\">a</td><td>b</td></tr></table>\n");
    assert!(wide > even + 150, "{even} {wide}");

    // A caption is centred above the grid, not a row inside it.
    let caption_ink = |markdown: &str| {
        let captioned = page(markdown);
        let width = captioned.bitmap.width as usize;
        let mut span = (usize::MAX, 0);
        // Ink above the table's top border only.
        let top = captioned
            .bitmap
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .position(|p| p[..3] == border)
            .unwrap()
            / width;
        for (i, p) in captioned
            .bitmap
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
        {
            if i / width < top && p[1] < 0x60 {
                span = (span.0.min(i % width), span.1.max(i % width));
            }
        }
        (span, columns_of(&captioned))
    };
    let ((first, last), _) =
        caption_ink("<table width=\"100%\"><caption>Cap</caption><tr><td>a</td></tr></table>\n");
    assert!(
        ((first + last) as i32 / 2 - 390).abs() < 4,
        "{first}..{last}"
    );
    // Over a table narrower than the column it is centred on the table,
    let ((first, last), (left, right)) = caption_ink(
        "<table><caption>Cap</caption><tr><td>a cell wider than its caption</td></tr></table>\n",
    );
    assert!(right < 400, "{right}");
    assert!(
        ((first + last) as i32 - (left + right) as i32).abs() < 8,
        "{first}..{last} over {left}..{right}"
    );
    // and one wider than its table starts at the column's edge, on one line.
    let ((first, last), (_, right)) = caption_ink(
        "<table><caption>A caption wider than its table</caption><tr><td>a</td></tr></table>\n",
    );
    assert!((40..44).contains(&first), "{first}");
    assert!(last > right + 100, "{last} past {right}");
}

#[test]
fn an_aligned_image_stands_beside_its_text() {
    let dir = temp_dir("image-align");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/md/assets/badge.png"),
        dir.join("badge.png"),
    )
    .unwrap();
    let options = Options {
        base_dir: Some(dir.clone()),
        ..Options::light()
    };
    let draw = |markdown: &str| render(&mdshared_md_core::parse(markdown), &options);
    // The image sits in a link, and the renderer reports where a link is under a point (a floated
    // image through its own hit test), so where the image went is read from the layout, not from
    // pixels: no glyph, in any font, is a link or a float.
    let linked_columns = |markdown: &str| {
        let doc = mdshared_md_core::parse(markdown);
        let mut renderer = DocumentRenderer::new();
        renderer.layout(&doc, &options);
        renderer.view_frame(&doc, &options, 0.0, 400.0);
        let mut span: Option<(usize, usize)> = None;
        for x in 0..options.width as usize {
            let on_image = (0..30).map(|k| k as f32 * 8.0).any(|y| {
                renderer
                    .hit_test(&doc, &options, x as f32, y)
                    .is_some_and(|h| h.link.is_some())
            });
            if on_image {
                span = Some(span.map_or((x, x), |(first, _)| (first, x)));
            }
        }
        span.expect("the image is hittable")
    };
    let text = "word ".repeat(40);
    let link = |attrs: &str| {
        format!("<a href=\"https://example.com/\"><img src=\"badge.png\" width=\"100\"{attrs}></a>")
    };
    let right = format!("{} {text}\n", link(" align=\"right\""));
    let (first, last) = linked_columns(&right);
    assert!(first >= 640 && last <= 740, "{first}..{last}");
    assert!(
        last - first >= 95,
        "the image is 100 px wide: {first}..{last}"
    );
    // Inline, the image is a character of the line: the rectangle the layout reports for it starts
    // at the left margin.
    let inline = format!("{} {text}\n", link(""));
    let doc = mdshared_md_core::parse(&inline);
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&doc, &options);
    renderer.view_frame(&doc, &options, 0.0, 400.0);
    let at = renderer.selection_rects(&doc, &options, 0, 1)[0];
    assert!(at.x < 60.0 && at.w >= 100.0, "{at:?}");
    // Beside the image the text is one block, not pushed under it.
    let (right, inline) = (draw(&right), draw(&inline));
    assert!(right.bitmap.height < inline.bitmap.height + 20);
}

#[test]
fn a_block_that_cannot_be_drawn_says_why() {
    let named = page("```mermaid\narchitecture-beta\n    group api(cloud)[API]\n```\n");
    let other = page("```text\narchitecture-beta\n    group api(cloud)[API]\n```\n");
    // The reason is longer than a language name: more muted ink at the top right.
    let ink = |r: &mdshared_md_render::Rendered| {
        let width = r.bitmap.width as usize;
        r.bitmap
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(i, p)| i % width > 400 && p[1] < 0xc0)
            .count()
    };
    assert!(
        ink(&named) > ink(&other) * 3,
        "{} {}",
        ink(&named),
        ink(&other)
    );
}

#[test]
fn text_runs_beside_a_floated_image_and_then_under_it() {
    let dir = temp_dir("image-float");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/md/assets/badge.png"),
        dir.join("badge.png"),
    )
    .unwrap();
    let text = "word ".repeat(120);
    let doc = mdshared_md_core::parse(&format!(
        "<img src=\"badge.png\" width=\"200\" align=\"right\"> {text}\n"
    ));
    let rendered = render(
        &doc,
        &Options {
            base_dir: Some(dir.clone()),
            ..Options::light()
        },
    );
    let width = rendered.bitmap.width as usize;
    // The last dark-text column of each row: short beside the image, the full column under it.
    let reach = |row: usize| {
        (0..width)
            .rev()
            .find(|x| {
                let p = &rendered.bitmap.pixels[(row * width + x) * 4..][..4];
                p[0] < 0x60 && p[1] < 0x60 && p[2] < 0x60
            })
            .unwrap_or(0)
    };
    let first_lines = (44..60).map(reach).max().unwrap();
    let last_lines = (rendered.bitmap.height as usize - 90..rendered.bitmap.height as usize - 50)
        .map(reach)
        .max()
        .unwrap();
    assert!(first_lines < 540, "beside the image: {first_lines}");
    assert!(last_lines > 640, "under the image: {last_lines}");
}

/// The first and last column holding the table border's colour.
fn border_columns(r: &mdshared_md_render::Rendered) -> (usize, usize) {
    let border = [LIGHT.border[2], LIGHT.border[1], LIGHT.border[0]];
    let width = r.bitmap.width as usize;
    let mut span = (usize::MAX, 0);
    for (i, p) in r.bitmap.pixels.as_chunks::<4>().0.iter().enumerate() {
        if p[..3] == border {
            span = (span.0.min(i % width), span.1.max(i % width));
        }
    }
    span
}

#[test]
fn a_table_is_as_wide_as_its_content() {
    let width_of = |markdown: &str| {
        let (first, last) = border_columns(&page(markdown));
        (first, last + 1 - first)
    };
    // A small table stays small, at the left of the column.
    let small = "| Key | Value |\n| --- | ---: |\n| a | 1 |\n| bb | 22 |\n";
    let (left, small_width) = width_of(small);
    assert_eq!(left, 40);
    assert!((80..200).contains(&small_width), "{small_width}");
    // One line longer than the page takes the page's width, no more.
    let long = "word ".repeat(60);
    let wide = format!("| Key | Value |\n| --- | --- |\n| a | {long} |\n");
    assert_eq!(width_of(&wide), (40, 700));
    // `align`, on the table or on a `<div>` around it, places a table narrower than the page.
    let cells = "<tr><td>one</td><td>two</td></tr>";
    let (plain_left, plain_width) = width_of(&format!("<table>{cells}</table>\n"));
    assert_eq!(plain_left, 40);
    for centred in [
        format!("<table align=\"center\">{cells}</table>\n"),
        format!("<div align=\"center\">\n<table>{cells}</table>\n</div>\n"),
    ] {
        let (left, width) = width_of(&centred);
        assert_eq!(width, plain_width, "{centred}");
        assert!(
            (left as i32 * 2 + width as i32 - 780).abs() <= 2,
            "{centred}: {left} {width}"
        );
    }
    let (left, width) = width_of(&format!("<table align=\"right\">{cells}</table>\n"));
    assert_eq!(left + width, 740);
    // A cell across two columns widens them when it needs more than they hold.
    let spanning = format!(
        "<table>{cells}<tr><td colspan=\"2\">a cell that is wider than both columns</td></tr></table>\n"
    );
    let (_, spanning_width) = width_of(&spanning);
    assert!(
        spanning_width > plain_width + 100 && spanning_width < 400,
        "{plain_width} {spanning_width}"
    );
}

#[test]
fn table_columns_follow_their_content_and_stated_widths() {
    let options = Options::light();
    let x_of = |markdown: &str, word: &str| {
        let doc = mdshared_md_core::parse(markdown);
        let mut renderer = DocumentRenderer::new();
        renderer.layout_exact(&doc, &options);
        let rects = rects_of(&mut renderer, &doc, &options, word);
        (rects[0].x, rects.len())
    };
    // The first cell's text starts after the margin, the border and the cell padding.
    let start = 40.0 + 1.0 + 13.0;
    // A column stated in px is that wide, whatever it holds.
    let stated = "<table><tr><td width=\"200\">a</td><td>next</td></tr></table>\n";
    assert_eq!(x_of(stated, "next").0, start + 200.0);
    // A column stated in percent is that share of a table grown to make it so.
    let half = "<table><tr><td width=\"50%\">a</td><td>the rest of it</td></tr></table>\n";
    let rest = x_of(half, "the rest").0;
    let free = x_of(
        "<table><tr><td>a</td><td>the rest of it</td></tr></table>\n",
        "the rest",
    )
    .0;
    assert!(rest > free + 50.0, "{free} {rest}");
    let (first, last) = border_columns(&page(half));
    let inner = (last + 1 - first - 2) as f32;
    assert!(
        (rest - start - inner / 2.0).abs() <= 1.0,
        "{rest} in {inner}"
    );
    // Too wide for the page: the short column keeps its word and the long one wraps.
    let long = "several words that go on ".repeat(8);
    let wrapping = format!("| Name | Text |\n| --- | --- |\n| identifier | {long} |\n");
    assert_eq!(x_of(&wrapping, "identifier").1, 1);
    assert!(x_of(&wrapping, "several words").0 < 200.0);
    // Words too long to stand side by side: the long one breaks, the short one stays whole.
    let hash = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".repeat(2);
    let breaking = format!("| Hash | Note |\n| --- | --- |\n| {hash} | unchanged |\n");
    assert_eq!(x_of(&breaking, "unchanged").1, 1);
    assert!(x_of(&breaking, &hash).1 > 1);
    assert_eq!(border_columns(&page(&breaking)), (40, 739));
}

#[test]
fn a_click_in_a_table_lands_on_the_text_of_its_cell() {
    // Cells of one row share their rows of pixels, so the column decides which one was meant.
    let rows = "<tr><td>first</td><td>second</td></tr><tr><td>third</td><td>fourth</td></tr>";
    for (table, centred) in [
        (format!("<table width=\"100%\">{rows}</table>\n"), false),
        (format!("<table align=\"center\">{rows}</table>\n"), true),
    ] {
        let doc = mdshared_md_core::parse(&table);
        let options = Options::light();
        let mut renderer = DocumentRenderer::new();
        renderer.layout(&doc, &options);
        renderer.view_frame(&doc, &options, 0.0, 400.0);
        for word in ["first", "second", "third", "fourth"] {
            let at = doc.text.find(word).unwrap() as u32;
            let rect = rects_of(&mut renderer, &doc, &options, word)[0];
            // A centred table is in the middle of the page, and the highlight is where it is.
            assert!(
                !centred || (rect.x > 300.0 && rect.x + rect.w < 480.0),
                "{word}: {rect:?}"
            );
            let hit = renderer
                .hit_test(&doc, &options, rect.x + 1.0, rect.y + rect.h / 2.0)
                .expect("a click in the cell");
            assert_eq!(hit.offset, at, "{word} in {table}");
            assert!(hit.on_text, "{word} in {table}");
        }
    }
}

#[test]
fn the_tables_fixture_keeps_its_tables_inside_what_holds_them() {
    // A narrow table, a wrapping one, spans, stated widths, tables in a list and a quote, and
    // a word that must break: every table starts where its container does and none passes the
    // right edge of the column.
    let doc = fixture("tables.md");
    for width in [780.0, 420.0] {
        let rendered = render(&doc, &narrow(width));
        let (first, last) = border_columns(&rendered);
        assert_eq!(first, 40, "{width}");
        assert!(last < width as usize - 40, "{width}: {last}");
        assert!(rendered.content_height > 1000.0, "{width}");
    }
}

#[test]
fn a_point_in_a_code_box_reports_its_box_and_code() {
    let doc = mdshared_md_core::parse(
        "Intro.\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\n- item\n\n  ```\n  nested\n  ```\n\nAfter.\n\n    indented\n",
    );
    let options = narrow(500.0);
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&doc, &options);
    renderer.view_frame(&doc, &options, 0.0, 800.0);
    let total = renderer.page_layout().unwrap().total_height();
    let mut found: Vec<mdshared_md_render::Code> = Vec::new();
    let mut outside = 0;
    let mut y = 2.0;
    while y < total {
        let mut x = 2.0;
        while x < 500.0 {
            if let Some(hit) = renderer.hit_test(&doc, &options, x, y) {
                match hit.code {
                    Some(code) => {
                        // The point is inside the box it reports, in page coordinates.
                        let r = code.rect;
                        assert!(x >= r.x && x <= r.x + r.w, "{x} in {r:?}");
                        assert!(y >= r.y && y <= r.y + r.h, "{y} in {r:?}");
                        if !found.contains(&code) {
                            found.push(code);
                        }
                    }
                    None => outside += 1,
                }
            }
            x += 7.0;
        }
        y += 3.0;
    }
    let texts: Vec<&str> = found
        .iter()
        .map(|c| doc.text[c.start as usize..c.end as usize].trim_end_matches('\n'))
        .collect();
    assert_eq!(
        texts,
        [
            "fn main() {\n    println!(\"hi\");\n}",
            "nested",
            "indented"
        ],
        "{found:?}"
    );
    // The paragraphs around them are not code.
    assert!(outside > 0);
    // The same box is found again from a point just inside its corner.
    let again = renderer.hit_test(&doc, &options, found[0].rect.x + 5.0, found[0].rect.y + 5.0);
    assert_eq!(again.and_then(|h| h.code), Some(found[0]));
}

#[test]
fn a_ruby_base_is_not_broken_across_lines_and_its_note_stays_over_it() {
    let words =
        "A line of plain words before <ruby>wide base<rt>note</rt></ruby> and a tail of words.";
    let plain = "A line of plain words before wide base and a tail of words.";
    let (with, without) = (
        mdshared_md_core::parse(words),
        mdshared_md_core::parse(plain),
    );
    let mut split_without = false;
    for width in (160..420).step_by(7) {
        let options = narrow(width as f32);
        let mut renderer = DocumentRenderer::new();
        let rects = rects_of(&mut renderer, &with, &options, "wide base");
        assert_eq!(
            rects.len(),
            1,
            "the base is on one line at {width}: {rects:?}"
        );
        let note = rects_of(&mut renderer, &with, &options, "note")[0];
        assert!(
            note.y + note.h <= rects[0].y + rects[0].h,
            "{note:?} {rects:?}"
        );
        let mut other = DocumentRenderer::new();
        split_without |= rects_of(&mut other, &without, &options, "wide base").len() > 1;
        // The break after the base is still possible: the line is never wider than the column.
        let tail = rects_of(&mut renderer, &with, &options, "tail")[0];
        assert!(tail.x + tail.w <= width as f32, "{tail:?} at {width}");
    }
    assert!(
        split_without,
        "the same words do break when there is no ruby"
    );
}

#[test]
fn a_figure_keeps_a_40_px_margin_at_each_side_and_a_narrow_column_keeps_its_content() {
    let doc = mdshared_md_core::parse(
        "<figure><figcaption>Caption words here</figcaption></figure>\n\n\
         <div align=\"right\"><figure align=\"right\"><figcaption>Caption words here</figcaption></figure></div>\n\n\
         Caption words here\n",
    );
    let options = narrow(500.0);
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&doc, &options);
    let rect = |renderer: &mut DocumentRenderer, nth: usize| {
        let from = doc.text.match_indices("Caption").nth(nth).unwrap().0 as u32;
        renderer.selection_rects(&doc, &options, from, from + 18)[0]
    };
    let (figure, right, body) = (
        rect(&mut renderer, 0),
        rect(&mut renderer, 1),
        rect(&mut renderer, 2),
    );
    assert!(
        (figure.x - body.x - 40.0).abs() < 1.0,
        "{figure:?} {body:?}"
    );
    // Aligned right, the caption ends 40 px before the column's right edge.
    assert!(right.x + right.w <= 500.0 - 40.0 + 1.0, "{right:?}");

    // A column too narrow for two margins still holds the caption's words.
    let tight = narrow(120.0);
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&doc, &tight);
    let from = doc.text.find("Caption").unwrap() as u32;
    let rects = renderer.selection_rects(&doc, &tight, from, from + 18);
    assert!(rects.iter().all(|r| r.w > 5.0), "{rects:?}");
}
