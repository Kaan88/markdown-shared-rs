use super::*;

/// The page rectangles of one word of the document, found by its text.

#[test]
fn a_ruby_annotation_is_drawn_small_above_its_base_and_the_line_grows() {
    let with = mdshared_md_core::parse(
        "Lead line of text that wraps before the word <ruby>base<rt>note</rt></ruby> and more \
         text to fill another full line of the paragraph.",
    );
    let without = mdshared_md_core::parse(
        "Lead line of text that wraps before the word base and more \
         text to fill another full line of the paragraph.",
    );
    let options = narrow(330.0);
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&with, &options);
    // `layout` estimates; the exact pass measures every block.
    let grown = mdshared_md_render::layout(&with, &options).total_height()
        - mdshared_md_render::layout(&without, &options).total_height();
    assert!(grown >= 8.0, "the line makes room for the note: {grown}");

    let base = rects_of(&mut renderer, &with, &options, "base")[0];
    let note = rects_of(&mut renderer, &with, &options, "note")[0];
    // The note is above the base, smaller, and centred on it.
    assert!(
        note.y + note.h <= base.y + base.h / 2.0,
        "{note:?} {base:?}"
    );
    assert!(note.h < base.h, "{note:?} {base:?}");
    let centre = |r: mdshared_md_render::Rect| r.x + r.w / 2.0;
    assert!(
        (centre(note) - centre(base)).abs() < 3.0,
        "{note:?} {base:?}"
    );
    // Nothing on the line above reaches into the band the note sits in.
    let lead = rects_of(&mut renderer, &with, &options, "Lead")[0];
    assert!(lead.y + lead.h <= note.y + 1.0, "{lead:?} {note:?}");
}

#[test]
fn selecting_across_a_ruby_covers_base_annotation_and_the_text_after_it() {
    let doc = mdshared_md_core::parse("before <ruby>漢字<rt>kanji</rt></ruby> after");
    let options = narrow(500.0);
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&doc, &options);
    let from = doc.text.find("before").unwrap() as u32;
    let to = doc.text.len() as u32;
    let rects = renderer.selection_rects(&doc, &options, from, to);
    assert_eq!(rects.len(), 2, "the line and the annotation: {rects:?}");
    // The copied text is the document's: base, annotation and the rest, in order.
    assert_eq!(
        doc.text[from as usize..].trim_end(),
        "before 漢字kanji after"
    );
    // A selection that starts inside the annotation starts at the text after it.
    let inside = doc.text.find("anji").unwrap() as u32;
    let rects = renderer.selection_rects(&doc, &options, inside, to);
    let after = rects_of(&mut renderer, &doc, &options, "after")[0];
    assert!(
        rects.iter().any(|r| r.x >= after.x - 8.0),
        "{rects:?} {after:?}"
    );
    // A click on the base lands in the base, and one on the annotation resolves too.
    let base = rects_of(&mut renderer, &doc, &options, "漢字")[0];
    let hit = renderer
        .hit_test(&doc, &options, base.x + 2.0, base.y + base.h / 2.0)
        .unwrap();
    assert_eq!(hit.offset, doc.text.find("漢字").unwrap() as u32);
    let note = rects_of(&mut renderer, &doc, &options, "kanji")[0];
    assert!(
        renderer
            .hit_test(&doc, &options, note.x + 2.0, note.y + note.h / 2.0)
            .is_some()
    );
}

#[test]
fn a_description_list_bolds_terms_and_indents_definitions() {
    let list = mdshared_md_core::parse("<dl><dt>Wwwwww</dt><dd>Mmmmmm</dd></dl>");
    let div = mdshared_md_core::parse("<div>Wwwwww</div>");
    let options = narrow(500.0);
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&list, &options);
    let term = rects_of(&mut renderer, &list, &options, "Wwwwww")[0];
    let definition = rects_of(&mut renderer, &list, &options, "Mmmmmm")[0];
    let mut other = DocumentRenderer::new();
    other.layout(&div, &options);
    let regular = rects_of(&mut other, &div, &options, "Wwwwww")[0];
    assert!(
        term.w > regular.w * 1.03,
        "bold is wider: {term:?} {regular:?}"
    );
    assert!(
        (definition.x - term.x - 16.0).abs() < 1.5,
        "the definition is 16 px in: {term:?} {definition:?}"
    );
    assert!(definition.y > term.y + term.h - 1.0);
    // The gap after a definition before the next term is wider than the one inside a group.
    let two = mdshared_md_core::parse("<dl><dt>T1</dt><dd>D1</dd><dt>T2</dt><dd>D2</dd></dl>");
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&two, &options);
    let y = |w: &str, r: &mut DocumentRenderer| rects_of(r, &two, &options, w)[0].y;
    let inside = y("D1", &mut renderer) - y("T1", &mut renderer);
    let between = y("T2", &mut renderer) - y("D1", &mut renderer);
    assert!(between > inside + 8.0, "{inside} {between}");
}

#[test]
fn a_figure_caption_is_smaller_and_follows_the_figure_alignment() {
    let doc = mdshared_md_core::parse(
        "<figure><figcaption>Caption words here</figcaption></figure>\n\n\
         <figure align=\"center\"><figcaption>Caption words here</figcaption></figure>\n\n\
         Caption words here\n",
    );
    let options = narrow(500.0);
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&doc, &options);
    let rect = |renderer: &mut DocumentRenderer, nth: usize| {
        let from = doc.text.match_indices("Caption").nth(nth).unwrap().0 as u32;
        renderer.selection_rects(&doc, &options, from, from + 18)[0]
    };
    let (left, centred, body) = (
        rect(&mut renderer, 0),
        rect(&mut renderer, 1),
        rect(&mut renderer, 2),
    );
    assert!(
        left.w < body.w * 0.95,
        "smaller than the body text: {left:?} {body:?}"
    );
    assert!(
        (left.x - body.x - 40.0).abs() < 1.0,
        "left aligned by default, in the 40 px margin: {left:?} {body:?}"
    );
    assert!(
        centred.x > left.x + 40.0,
        "align=center centres it: {centred:?} {left:?}"
    );
}

#[test]
fn a_figure_with_a_local_image_draws_it_above_its_caption() {
    let doc = mdshared_md_core::parse(
        "<figure align=\"center\"><img src=\"badge.png\" alt=\"badge\"><figcaption>Caption</figcaption></figure>",
    );
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/md/assets");
    let options = Options {
        base_dir: Some(dir),
        ..narrow(500.0)
    };
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&doc, &options);
    let caption = rects_of(&mut renderer, &doc, &options, "Caption")[0];
    let frame = renderer.view_frame(&doc, &options, 0.0, 400.0);
    // The badge is blue, centred over the caption, and entirely above it.
    let (mut left, mut right, mut bottom) = (u32::MAX, 0, 0);
    for (i, p) in frame.bitmap.pixels.as_chunks::<4>().0.iter().enumerate() {
        if p[0] > 140 && p[2] < 40 && p[1] > 80 {
            let (x, y) = (i as u32 % frame.bitmap.width, i as u32 / frame.bitmap.width);
            left = left.min(x);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    assert!(right > left, "the badge is drawn");
    assert!(bottom as f32 <= caption.y + 1.0, "{bottom} {caption:?}");
    let image_centre = (left + right) as f32 / 2.0;
    assert!((image_centre - (caption.x + caption.w / 2.0)).abs() < 3.0);
}

#[test]
fn a_point_inside_a_formula_or_diagram_reports_its_source() {
    if !engine_available() {
        eprintln!("skipping: no mdview-engine in this Cargo target profile");
        return;
    }
    use mdshared_md_render::SourceKind;
    let doc = mdshared_md_core::parse(
        "Inline $x^2 + 1$ in a sentence.\n\n$$\nE = mc^2\n$$\n\n```mermaid\ngraph TD\n  A --> B\n```\n",
    );
    let options = narrow(500.0);
    let mut renderer = DocumentRenderer::new();
    renderer.layout(&doc, &options);
    renderer.view_frame(&doc, &options, 0.0, 600.0);
    let text = |s: mdshared_md_render::Source| doc.text[s.start as usize..s.end as usize].trim();
    let mut found: Vec<(SourceKind, String)> = Vec::new();
    let mut beside = 0;
    let total = renderer.page_layout().unwrap().total_height();
    let mut y = 2.0;
    while y < total {
        let mut x = 2.0;
        while x < 500.0 {
            if let Some(hit) = renderer.hit_test(&doc, &options, x, y) {
                match hit.source {
                    Some(s) => {
                        let item = (s.kind, text(s).to_string());
                        if !found.contains(&item) {
                            found.push(item);
                        }
                    }
                    None => beside += 1,
                }
            }
            x += 5.0;
        }
        y += 3.0;
    }
    assert!(
        found.contains(&(SourceKind::Formula, "x^2 + 1".to_string())),
        "{found:?}"
    );
    assert!(
        found.contains(&(SourceKind::Formula, "E = mc^2".to_string())),
        "{found:?}"
    );
    assert!(
        found.contains(&(SourceKind::Diagram, "graph TD\n  A --> B".to_string())),
        "{found:?}"
    );
    assert_eq!(found.len(), 3, "{found:?}");
    // The text of the sentence, outside the formula, is not a source.
    assert!(beside > 0);
    let word = doc.text.find("sentence").unwrap() as u32;
    let rect = renderer.selection_rects(&doc, &options, word, word + 8)[0];
    let hit = renderer
        .hit_test(&doc, &options, rect.x + 2.0, rect.y + rect.h / 2.0)
        .unwrap();
    assert_eq!(hit.source, None);
}

#[test]
fn a_decoration_inside_a_run_is_drawn_where_it_is() {
    // Strikethrough does not change the font, so the struck words share a shaped run with the
    // text before them; the line used to be looked up at the run's start and never drawn.
    let struck = page("plain ~~struck~~ plain\n");
    let plain = page("plain struck plain\n");
    assert_ne!(struck.bitmap.pixels, plain.bitmap.pixels);
    // And a link's underline stops at the link.
    let link = page("[link](#a) and text that is not a link at all\n");
    let accent = count(&link, [LIGHT.accent[2], LIGHT.accent[1], LIGHT.accent[0]]);
    let long = page("[link and text that is not a link at all](#a)\n");
    let all = count(&long, [LIGHT.accent[2], LIGHT.accent[1], LIGHT.accent[0]]);
    assert!(accent > 0 && accent * 3 < all, "{accent} of {all}");
}

#[test]
fn sub_and_superscript_leave_the_baseline() {
    let rows = |markdown: &str| {
        let r = page(markdown);
        let width = r.bitmap.width as usize;
        let ink: Vec<usize> = r
            .bitmap
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, p)| p[1] < 0x80)
            .map(|(i, _)| i / width)
            .collect();
        (ink[0], ink[ink.len() - 1])
    };
    let (top, bottom) = rows("xxx\n");
    assert!(rows("x<sup>x</sup>x\n").0 < top, "superscript rises");
    assert!(rows("x<sub>x</sub>x\n").1 > bottom, "subscript drops");
}

#[test]
fn each_alert_kind_has_its_own_colour() {
    let bar = |markdown: &str| {
        let r = page(markdown);
        // The bar is the first column of the block, a few rows in.
        let at = (60 * r.bitmap.width as usize + LIGHT.margin as usize + 1) * 4;
        [
            r.bitmap.pixels[at],
            r.bitmap.pixels[at + 1],
            r.bitmap.pixels[at + 2],
        ]
    };
    let kinds = ["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION"];
    let colours: Vec<[u8; 3]> = kinds
        .iter()
        .map(|kind| bar(&format!("> [!{kind}]\n> Body text.\n")))
        .collect();
    for (i, a) in colours.iter().enumerate() {
        assert_ne!(*a, [LIGHT.bg[2], LIGHT.bg[1], LIGHT.bg[0]], "{}", kinds[i]);
        for b in &colours[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

#[test]
fn html_presentation_attributes_are_honoured() {
    // `<ol type>` sets the marker for the whole list.
    let lettered = page("<ol type=\"a\"><li>one</li><li>two</li></ol>\n");
    let numbered = page("<ol><li>one</li><li>two</li></ol>\n");
    assert_ne!(lettered.bitmap.pixels, numbered.bitmap.pixels);

    // `<div align>` and `<center>` reach the Markdown inside them.
    let left = ink_columns(&page("Short.\n")).0;
    for centred in [
        "<div align=\"center\">\n\nShort.\n\n</div>\n",
        "<center>Short.</center>\n",
    ] {
        let (first, last) = ink_columns(&page(centred));
        assert!(first > left + 200, "{centred:?} starts at {first}");
        let middle = (first + last) / 2;
        assert!(
            (middle as i32 - 390).abs() < 4,
            "{centred:?} is centred on {middle}"
        );
    }

    // `<hr width>` shortens the rule.
    let border = [LIGHT.border[2], LIGHT.border[1], LIGHT.border[0]];
    let full = count(&page("<hr>\n"), border);
    let half = count(&page("<hr width=\"50%\">\n"), border);
    assert!(full > 0 && half * 2 == full, "{half} of {full}");

    // `align="justify"` stretches every line but the last to the column.
    let text = "word ".repeat(60);
    // The widest ragged line may itself reach the column (it does in Helvetica's metrics, on a
    // Mac), so the check is that the lines moved and that the justified ones reach the column.
    let ragged = page(&format!("<p>{text}</p>\n"));
    let justified = page(&format!("<p align=\"justify\">{text}</p>\n"));
    assert_ne!(ragged.bitmap.pixels, justified.bitmap.pixels);
    let justified = ink_columns(&justified).1;
    assert!(justified >= ink_columns(&ragged).1, "{justified}");
    assert!(justified + 4 >= 780 - LIGHT.margin as usize, "{justified}");
}

#[test]
fn a_code_label_is_not_drawn_through_a_long_first_line() {
    // The label sits at the right of the first line; code that reaches it stops short of it.
    let long = format!("```json\n{}\n```\n", "\"key\": 1, ".repeat(12));
    let wide = page(&long);
    let short = page("```json\n\"key\": 1,\n```\n");
    let width = wide.bitmap.width as usize;
    // The label's own pixels are the same in both: the columns it occupies, on its rows.
    let label = |r: &mdshared_md_render::Rendered| -> Vec<u8> {
        (16..40)
            .flat_map(|row| {
                let at = (row * width + width - LIGHT.margin as usize - 46) * 4;
                r.bitmap.pixels[at..at + 34 * 4].to_vec()
            })
            .collect()
    };
    assert_eq!(label(&wide), label(&short));
}

#[test]
fn mark_is_highlighted() {
    let marked = page("some <mark>marked</mark> text\n");
    let yellow = count(&marked, [0xc5, 0xf8, 0xff]);
    assert!(yellow > 200, "highlight pixels: {yellow}");
    assert_eq!(count(&page("some marked text\n"), [0xc5, 0xf8, 0xff]), 0);
}

#[test]
fn dir_moves_a_paragraph_to_its_side() {
    let (first, last) = ink_columns(&page("<p dir=\"rtl\">Short.</p>\n"));
    assert!(
        first > 600 && last + 4 >= 780 - LIGHT.margin as usize,
        "{first}..{last}"
    );
    // Arabic starts at the right by itself, and at the left when its block says `ltr`.
    let arabic = "\u{645}\u{631}\u{62d}\u{628}\u{627}";
    assert!(ink_columns(&page(&format!("{arabic}\n"))).0 > 600);
    let forced = format!("<div dir=\"ltr\">\n\n{arabic}\n\n</div>\n");
    assert!(ink_columns(&page(&forced)).0 < LIGHT.margin as usize + 4);
}
