use super::*;

// Audit fixes: each behaviour below changed in `phase1-audit-fixes`, and these are the tests that
// fail if it reverts. Expected values are what `gh api markdown -f mode=gfm` showed where the audit
// compared against GitHub.

#[test]
fn object_head_template_and_embed_keep_their_children() {
    // GitHub strips these tags and keeps what they hold; a document must not come out blank.
    let d = parse("<object data=\"x.svg\"><img src=\"f.png\" alt=\"fb\"></object>");
    assert_eq!(d.images.len(), 1);
    assert_eq!(d.str(d.images[0].alt), "fb");
    let d = parse("<object><p>para</p></object>\n\nz");
    assert!(
        d.text.contains("para") && d.text.contains('z'),
        "{:?}",
        d.text
    );
    for (src, want) in [
        ("<object>x</object>y", "xy"),
        ("<embed>x</embed>y", "xy"),
        ("<template>x</template>y", "xy"),
        ("<head>x</head>y", "xy"),
    ] {
        assert_eq!(parse(src).text.trim(), want, "{src:?}");
    }
    // The usual `<object>` fallback keeps its text and its link.
    let d = parse(
        "<object data=\"x.pdf\">cannot show this <a href=\"x.pdf\">download</a></object>\n\nnext",
    );
    assert!(
        d.text.contains("cannot show this") && d.text.contains("next"),
        "{:?}",
        d.text
    );
    assert_eq!(links(&d)[0].1, "x.pdf");
}

#[test]
fn script_style_and_iframe_show_as_literal_text() {
    // The GFM tag filter escapes these, so they read as text; nothing here executes either way.
    for src in [
        "<script>alert(1)</script>",
        "<style>p{color:red}</style>",
        "<iframe src=x></iframe>",
    ] {
        let d = parse(src);
        assert_eq!(d.text.trim_end(), src, "{src:?}");
        assert!(!d.blocks.is_empty(), "{src:?}");
    }
}

#[test]
fn a_blank_quote_line_makes_the_next_block_an_ordinary_alert_child() {
    // The marker line only stops being an alert when the next line is a setext underline for it.
    for src in [
        "> [!NOTE]\n>\n> Title\n> ===",
        "> [!NOTE]\n>\n> Title\n> ---",
        "> [!NOTE]\n>\n> Title\n> ***",
    ] {
        assert!(has_alert(&parse(src)), "{src:?}");
    }
    let d = parse("> [!NOTE]\n>\n> Title\n> ===");
    assert_eq!(d.str(d.headings[0].title), "Title");
    let d = parse("> [!NOTE]\n>\n> Title\n> ---");
    assert_eq!(d.str(d.headings[0].title), "Title");
    assert_eq!(d.headings[0].level, 2);
    let d = parse("> [!NOTE]\n>\n> ---");
    assert!(
        has_alert(&d) && d.blocks.iter().any(|b| b.kind == BlockKind::ThematicBreak),
        "{:?}",
        kinds(&d)
    );
    let d = parse("> [!NOTE]\n>\n> # Title");
    assert!(has_alert(&d));
    assert_eq!(d.str(d.headings[0].title), "Title");
    // With no blank line the underline rewrites the marker line, so no alert.
    assert!(!has_alert(&parse("> [!NOTE]\n> Title\n> ===")));
    assert!(!has_alert(&parse("> [!NOTE]\n> Title\n> ---")));
}

#[test]
fn empty_inline_html_is_content_for_an_alert() {
    // GitHub keeps an alert whose only body is empty inline HTML.
    for src in [
        "> [!NOTE]\n> <i></i>",
        "> [!NOTE]\n> <b></b>",
        "> [!NOTE]\n> <span></span>",
        "> [!NOTE]\n> <a href=\"x\"></a>",
        "> [!NOTE]\n> <img>",
        "> [!NOTE]\n> <p></p>",
        "> [!NOTE]\n> <source>",
        "> [!NOTE]\n> <br>",
        "> [!NOTE]\n> <hr>",
    ] {
        assert!(has_alert(&parse(src)), "{src:?}");
    }
    // Nothing the sanitizer keeps, so the quote stays plain with its marker.
    for src in [
        "> [!NOTE]",
        "> [!NOTE]\n>",
        "> [!NOTE]\n> <!-- c -->",
        "> [!NOTE]\n> <foo></foo>",
        "> [!NOTE]\n> <a></a>",
        "> [!NOTE]\n> <input type=checkbox>",
    ] {
        assert!(!has_alert(&parse(src)), "{src:?}");
        assert!(parse(src).text.contains("[!NOTE]"), "{src:?}");
    }
}

#[test]
fn dashes_under_a_marker_line_are_a_setext_heading() {
    // pulldown reads `-` and `--` as paragraph text; GitHub makes the marker line an H2.
    for src in ["> [!NOTE]\n> -", "> [!NOTE]\n> --"] {
        let d = parse(src);
        assert!(!has_alert(&d), "{src:?}");
        assert_eq!(d.headings[0].level, 2, "{src:?}");
        assert_eq!(d.str(d.headings[0].title), "[!NOTE]", "{src:?}");
    }
    // `#x` is not an ATX heading, so the dashes underline the whole first line.
    let d = parse("> [!NOTE]\n> #x\n> ---");
    assert!(!has_alert(&d));
    assert_eq!(d.str(d.headings[0].title), "[!NOTE] #x");
}

#[test]
fn a_table_cell_resets_the_tags_that_reject_an_alert() {
    // A <td> resets <div>, <li> and quote ancestors on GitHub; only <details> still rejects.
    for src in [
        "<div align=\"center\">\n<table><tr><td>\n\n> [!TIP]\n> x\n\n</td></tr></table>\n</div>",
        "<li><table><tr><td>\n\n> [!TIP]\n> x\n\n</td></tr></table></li>",
        "<blockquote><table><tr><td>\n\n> [!TIP]\n> x\n\n</td></tr></table></blockquote>",
        "<table><tr><th>\n\n> [!TIP]\n> x\n\n</th></tr></table>",
    ] {
        assert!(has_alert(&parse(src)), "{src:?}");
    }
    for src in [
        "<details><table><tr><td>\n\n> [!TIP]\n> x\n\n</td></tr></table></details>",
        "<table><tr><td><div>\n\n> [!TIP]\n> x\n\n</div></td></tr></table>",
    ] {
        assert!(!has_alert(&parse(src)), "{src:?}");
    }
}

#[test]
fn a_footnote_definition_under_a_marker_rejects_the_alert() {
    // GitHub draws a footnote definition as a list item, so the marker line stays plain text.
    let d = parse("> [!NOTE]\n> [^1]: f");
    assert!(!has_alert(&d));
    assert!(d.text.contains("[!NOTE]"), "{:?}", d.text);
}

#[test]
fn a_reference_in_a_skipped_footnote_definition_is_still_numbered() {
    // GitHub numbers b from inside the dropped duplicate, so x points at note 2.
    let d = parse("[^a]: 1\n\n[^a]: 2[^b]\n\n[^b]: b\n\nx[^a]");
    assert_eq!(d.footnotes.len(), 2);
    let number = |label: &str| {
        d.footnotes
            .iter()
            .find(|f| d.str(f.label) == label)
            .expect("a footnote")
            .number
    };
    assert_eq!(number("b"), 1);
    assert_eq!(number("a"), 2);
    assert!(!d.text.contains("2"), "the duplicate's text is dropped");
    assert!(d.text.contains('b'), "{:?}", d.text);
}

#[test]
fn a_footnote_definition_past_the_depth_limit_owns_no_block() {
    let src = format!(
        "{}\n\n[^a]: first\n\n[^a]: second\n\nr[^a]",
        "<div>".repeat(300)
    );
    let d = parse(&src);
    assert_eq!(d.footnotes.len(), 1);
    assert!(d.footnotes[0].defined);
    assert_eq!(d.footnotes[0].number, 1);
    assert_eq!(
        d.footnotes[0].block, NONE,
        "a flattened definition names no block"
    );
    // Shallower nesting gives the definition a real block, and the duplicate still loses.
    let src = format!(
        "{}\n\n[^a]: first\n\n[^a]: second\n\nr[^a]",
        "<div>".repeat(10)
    );
    let d = parse(&src);
    assert!(matches!(
        d.blocks[d.footnotes[0].block as usize].kind,
        BlockKind::FootnoteDefinition { .. }
    ));
    assert_eq!(d.block_text(d.footnotes[0].block).trim_end(), "first");
}

#[test]
fn empty_implied_rows_cells_and_items_are_dropped() {
    // A link wrapped around a cell no longer adds an empty leading column; the cell count is right.
    let d = parse("<table><tr><a href=\"u\"><td>a</td></a><td>b</td></tr></table>");
    let row = d.children(0).next().unwrap();
    assert_eq!(d.children(row).count(), 2, "{:?}", kinds(&d));
    // Empty tags and empty block elements add no phantom row or item.
    for src in [
        "<table><p></p><tr><td>a</td></tr></table>",
        "<table><b></b><tr><td>a</td></tr></table>",
    ] {
        let d = parse(src);
        assert_eq!(d.children(0).count(), 1, "{src}: {:?}", kinds(&d));
    }
    for src in [
        "<ul><p></p><li>a</li></ul>",
        "<ul><li>a</li><b></b></ul>",
        "<ul><a href=\"u\"><li>a</li></a><li>b</li></ul>",
    ] {
        let want = if src.contains("b</li>") { 2 } else { 1 };
        assert_eq!(parse(src).children(0).count(), want, "{src}");
    }
}

#[test]
fn raw_input_is_ignored_but_task_boxes_still_draw() {
    // GitHub's sanitizer removes a raw input outside a task list.
    let d = parse("<input type=\"checkbox\" checked>y");
    assert!(
        !d.spans
            .iter()
            .any(|s| matches!(s.kind, SpanKind::Checkbox { .. })),
        "{:?}",
        d.spans
    );
    assert_eq!(d.text, "y\n");
    let d = parse("<input type=\"checkbox\">");
    assert!(d.blocks.is_empty(), "{:?}", kinds(&d));
    let d = parse("<ul><li><input type=\"checkbox\" checked disabled> done</li></ul>");
    assert!(d.text.contains("done"), "{:?}", d.text);
    // A Markdown task item still draws its box.
    let d = parse("- [x] task");
    assert!(
        d.spans
            .iter()
            .any(|s| matches!(s.kind, SpanKind::Checkbox { checked: true }))
    );
    // And a raw input is not content that keeps an alert.
    assert!(!has_alert(&parse("> [!NOTE]\n> <input type=checkbox>")));
}

#[test]
fn find_matches_a_crlf_line_break() {
    // A CRLF pair in a pasted query is one line break, not two spaces.
    let d = parse("para one\nsame para\n");
    for q in ["one\nsame", "one same", "one\r\nsame"] {
        assert_eq!(find(&d, q).hits.len(), 1, "{q:?}");
    }
    // A selection from inside a code block still does not match; that is COVERAGE's find gap.
    let d = parse("```\nfoo\nbar\n```\n");
    assert_eq!(find(&d, "foo").hits.len(), 1);
    assert_eq!(find(&d, "foo\nbar").hits.len(), 0);
}

#[test]
fn github_backtick_math_is_inline_math() {
    let d = parse("Euler: $`e^{i\\pi} + 1 = 0`$ and ($`a_b`$) done");
    assert_eq!(
        math_spans(&d),
        vec!["e^{i\\pi} + 1 = 0", "a_b"],
        "{:?}",
        math_spans(&d)
    );
    // Inside a fence it is code, untouched.
    let d = parse("```\n$`x`$\n```\n");
    assert!(math_spans(&d).is_empty());
    assert!(d.block_text(0).contains("$`x`$"), "{:?}", d.block_text(0));
    // An unpaired opener or a formula with a `$` inside stays as written.
    let d = parse("cost $`5 and $`a$b`$ here");
    assert!(math_spans(&d).is_empty(), "{:?}", math_spans(&d));
}

#[test]
fn small_is_a_span() {
    let d = parse("a <small>fine print</small> b");
    assert!(
        d.spans
            .iter()
            .any(|s| s.kind == SpanKind::Small && span_text(&d, s) == "fine print")
    );
}

#[test]
fn q_is_drawn_between_curly_quotes() {
    let d = parse("He said <q>stop</q> twice, and a stray </q> is ignored.");
    let text = d.block_text(0);
    assert!(text.contains("\u{201c}stop\u{201d} twice"), "{text:?}");
    assert_eq!(text.matches('\u{201d}').count(), 1, "{text:?}");
}

/// The text of each span of one kind, in order.

#[test]
fn ruby_is_a_span_holding_base_and_notes() {
    let d = parse(
        "<ruby>漢<rt>kan</rt>字<rt>ji</rt></ruby> and <ruby>東<rp>(</rp><rt>tou</rt><rp>)</rp></ruby>",
    );
    // One text with the annotations in it, so find and copy read them; no parentheses, since the
    // `<rp>` text is a fallback for a browser with no ruby and is not shown.
    assert_eq!(d.block_text(0), "漢kan字ji and 東tou");
    assert_eq!(
        spans_of(&d, SpanKind::Ruby),
        ["漢kan字ji", "東tou"],
        "{:?}",
        d.spans
    );
    assert_eq!(spans_of(&d, SpanKind::RubyNote), ["kan", "ji", "tou"]);
}

#[test]
fn rp_hides_its_text_only_until_it_ends() {
    // An `<rp>` never closed does not hide the rest of the document.
    let d = parse("a <rp>hidden\n\nshown <b>bold</b>");
    assert!(!d.text.contains("hidden"), "{:?}", d.text);
    assert!(d.text.contains("shown bold"), "{:?}", d.text);
    // Inside a Markdown paragraph, where the text arrives as Markdown events.
    let d = parse("*x* <ruby>a<rp>(</rp><rt>b</rt><rp>)</rp></ruby> **y**");
    assert_eq!(d.block_text(0), "x ab y");
}

#[test]
fn description_lists_get_term_and_definition_blocks() {
    let d = parse(
        "<dl>\n<dt><b>Term</b> one</dt>\n<dd>Def with <code>code</code></dd>\n<dt>Two\n<dd>Implied end tags\n</dl>\n",
    );
    assert_eq!(
        tree(&d),
        "V[Dt[T(Term one)] Dd[T(Def with code)] Dt[T(Two)] Dd[T(Implied end tags)]]"
    );
    assert_eq!(spans_of(&d, SpanKind::Strong), ["Term"]);
    assert_eq!(spans_of(&d, SpanKind::Code), ["code"]);
}

#[test]
fn markdown_inside_description_list_items_still_parses() {
    let d = parse("<dl>\n<dt>\n\n**Term** and *more*\n\n</dt>\n<dd>\n\n- a\n- b\n\n</dd>\n</dl>\n");
    assert_eq!(tree(&d), "V[Dt[P(Term and more)] Dd[L[I[T(a)] I[T(b)]]]]");
}

#[test]
fn figure_holds_its_content_and_a_caption() {
    let d = parse(
        "<figure>\n<img src=\"a.png\" alt=\"A\">\n<figcaption>The <i>caption</i></figcaption>\n</figure>\n",
    );
    assert_eq!(tree(&d), "Fg[T Fc[T(The caption)]]");
    assert_eq!(d.images.len(), 1);
    assert_eq!(d.blocks[0].kind, BlockKind::Figure { align: Align::None });
    assert_eq!(spans_of(&d, SpanKind::Emphasis), ["caption"]);
    let d = parse("<figure align=\"center\"><img src=\"a.png\"></figure>");
    assert_eq!(
        d.blocks[0].kind,
        BlockKind::Figure {
            align: Align::Center
        }
    );
}

#[test]
fn find_sees_what_the_page_shows_for_undrawn_blocks() {
    // A Mermaid family the engine is built without keeps its source on the page, so find must
    // see it; a family it draws is hidden behind the picture.
    let d = parse("```mermaid\narchitecture-beta\n    group needle(cloud)[API]\n```\n");
    assert!(matches!(
        d.blocks[0].kind,
        BlockKind::Diagram {
            oversize: Some(mdshared_md_core::limits::Oversize::UnsupportedDiagram),
            ..
        }
    ));
    assert_eq!(mdshared_md_core::find::find(&d, "needle").hits.len(), 1);
    assert!(d.engine_jobs().is_empty());
    let d = parse("```mermaid\n%% a comment\nflowchart LR\n    needle --> b\n```\n");
    assert_eq!(mdshared_md_core::find::find(&d, "needle").hits.len(), 0);

    // A footnote definition nobody references is not listed, so it is not found either.
    let d = parse("Text[^a].\n\n[^a]: Listed needle.\n[^b]: Unlisted needle.\n");
    assert_eq!(mdshared_md_core::find::find(&d, "needle").hits.len(), 1);
}

#[test]
fn find_searches_a_diagram_the_renderer_could_not_draw() {
    let d = parse("```mermaid\nflowchart LR\n    needle --> \n```\n\nText.\n");
    let block = d.blocks[0].text;
    assert_eq!(mdshared_md_core::find::find(&d, "needle").hits.len(), 0);
    let shown = [(block.start, block.end)];
    let found = mdshared_md_core::find::find_shown(&d, "needle", |_| true, &shown);
    assert_eq!(found.hits.len(), 1);
}
