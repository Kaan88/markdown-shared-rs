use super::*;

// Review findings: block HTML inside inline content

/// The structure of one table cell in the body row.
fn cell_tree(row: &str) -> String {
    let whole = tree(&parse(&format!("| h |\n|---|\n| {row} |\n")));
    let body = whole.strip_prefix("Tb[Tr[Td(h)] Tr[").unwrap();
    body.strip_suffix("]]").unwrap().to_string()
}

#[test]
fn block_html_in_a_table_cell_keeps_its_structure() {
    assert_eq!(
        cell_tree("<ul><li>one</li><li>two</li></ul>"),
        "Td[L[I[T(one)] I[T(two)]]]"
    );
    assert_eq!(cell_tree("<ol><li>x</li></ol>"), "Td[L[I[T(x)]]]");
    assert_eq!(cell_tree("<p>p1</p><p>p2</p>"), "Td[P(p1) P(p2)]");
    assert_eq!(
        cell_tree("<details><summary>S</summary>body</details>"),
        "Td[D[S[T(S)] T(body)]]"
    );
    assert_eq!(
        cell_tree("<blockquote>q</blockquote><pre>c  d</pre>"),
        "Td[Q[T(q)] C(c  d)]"
    );
    assert_eq!(
        cell_tree("<table><tr><td>in</td></tr></table>"),
        "Td[Tb[Tr[Td[T(in)]]]]"
    );
    assert_eq!(
        cell_tree("x<hr>y<div>z</div>w"),
        "Td[T(x) R T(y) V[T(z)] T(w)]"
    );
    assert_eq!(
        cell_tree("**a** b<ul><li>c</li></ul>"),
        "Td[T(a b) L[I[T(c)]]]"
    );

    let d = parse("| h |\n|---|\n| <h3>Title</h3> |\n");
    assert_eq!(d.headings.len(), 1);
    assert_eq!(d.str(d.headings[0].slug), "title");
    assert_eq!(d.outline().len(), 1);

    // What came before the tag keeps its formatting, in the child it moved to.
    let d = parse("| h |\n|---|\n| **a** b<ul><li>c</li></ul> |\n");
    let plain = (0..d.blocks.len())
        .find(|&i| d.blocks[i].kind == BlockKind::Plain && d.block_text(i as u32).starts_with('a'))
        .unwrap();
    let spans = &d.spans[d.blocks[plain].spans.usize()];
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].kind, SpanKind::Strong);
    assert_eq!(
        common::writer::html(&d)
            .matches("<strong>a</strong>")
            .count(),
        1
    );

    // A plain cell is untouched.
    assert_eq!(cell_tree("plain *x*"), "Td(plain x)");
}

#[test]
fn block_html_in_a_paragraph_splits_it_as_a_browser_does() {
    let d = parse("Hello <p>inner</p> world and <ul><li>a</li><li>b</li></ul> done");
    assert_eq!(
        tree(&d),
        "P(Hello) P(inner) T(world and) L[I[T(a)] I[T(b)]] T(done)"
    );

    let d = parse("<a href=\"x\"><h2>Link heading</h2></a>");
    assert_eq!(tree(&d), "H(Link heading)");
    assert_eq!(d.str(d.headings[0].slug), "link-heading");
    assert_eq!(links(&d), [("Link heading".to_string(), "x".to_string())]);

    // Formatting opened before the tag continues inside it, and ends with its own end event.
    let d = parse("**a <p>b</p> c** d");
    let strong: Vec<String> = d
        .spans
        .iter()
        .filter(|s| s.kind == SpanKind::Strong)
        .map(|s| span_text(&d, s))
        .collect();
    assert_eq!(strong, ["a ", "b", "c"]);
    assert_eq!(d.block_text(d.blocks.len() as u32 - 1).trim_end(), "d");

    // An HTML block continues what the paragraph left open, with no empty blocks from the newlines.
    // Pulldown ends the paragraph at `<li>`, and a browser only ever sees one stream of tags.
    let d = parse("x <ul>\n<li>a</li>\n</ul>\ny\n\nnext");
    assert_eq!(tree(&d), "P(x) L[I[T(a)]] T(y) P(next)");
    let d = parse("x <div> open\n\nnext");
    assert_eq!(tree(&d), "P(x) V[T(open) P(next)]");

    // Blocks in a tight list item, and a stray closer that closes nothing.
    assert_eq!(tree(&parse("- a <p>b</p>")), "L[I[T(a) P(b)]]");
    assert_eq!(tree(&parse("a </div> b")), "P(a b)");

    // A Markdown heading stays one heading.
    let d = parse("# a <div>b</div> c");
    assert_eq!(tree(&d), "H(a b c)");
    assert_eq!(d.headings.len(), 1);

    // `<pre>` in a paragraph takes Markdown events as plain text.
    let d = parse("a <pre>x  **y**\nz `w`</pre> b");
    assert_eq!(tree(&d), "P(a) C(x  y\nz w) T(b)");
}

// Review findings: footnotes

#[test]
fn footnote_labels_match_ignoring_case() {
    let d = parse("Ref[^Note] and[^note] again[^Note] other[^b].\n\n[^note]: Def\n\n[^B]: Other\n");
    assert_eq!(d.footnotes.len(), 2);
    assert_eq!(d.str(d.footnotes[0].label), "Note");
    assert_eq!(d.footnotes[0].number, 1);
    assert_eq!(d.footnotes[1].number, 2);
    for f in &d.footnotes {
        assert!(matches!(
            d.blocks[f.block as usize].kind,
            BlockKind::FootnoteDefinition { .. }
        ));
    }
    let refs = d
        .spans
        .iter()
        .filter(|s| matches!(s.kind, SpanKind::FootnoteRef(0)))
        .count();
    assert_eq!(refs, 3);
    // Non-ASCII case folds too.
    let d = parse("x[^Ä] y[^ä]\n\n[^ä]: d\n");
    assert_eq!(d.footnotes.len(), 1);
    assert_ne!(d.footnotes[0].block, NONE);
}

// Review findings: slugs

#[test]
fn duplicate_headings_stay_linear() {
    let src = "### Bug Fixes\n\n- x\n\n".repeat(20_000);
    let t = Instant::now();
    let d = parse(&src);
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    assert_eq!(d.str(d.headings[19_999].slug), "bug-fixes-19999");

    let d = parse(&"#\n".repeat(32_768));
    assert_eq!(d.str(d.headings[2].slug), "-2");

    let mut s = mdshared_md_core::slug::Slugger::default();
    let got: Vec<String> = ["a", "a", "a-1", "a", "a-1", "a-2", "a"]
        .iter()
        .map(|t| s.slug(t))
        .collect();
    assert_eq!(got, ["a", "a-1", "a-1-1", "a-2", "a-1-2", "a-2-1", "a-3"]);
}

#[test]
fn slugs_match_github_anchors() {
    // From `gh api markdown -f mode=markdown`: mdview's old output differed on every one of these.
    for (src, slug) in [
        ("# می‌خواهم", "می\u{200c}خواهم"),
        ("# 👨‍💻 Dev", "\u{200d}-dev"),
        ("# x² + y² ½ ① Ⅷ", "x--y---ⅷ"),
        ("# ΣΑΣ Σ", "σασ-σ"),
        ("# a  b", "a--b"),
        ("# a<br>b", "ab"),
        ("# a <br> b", "a--b"),
        ("<h3><img src=x.png><br>Logo Title</h3>", "logo-title"),
        ("<h3>\n  Multi line\n  title\n</h3>", "--multi-line--title"),
        ("Setext\nline two\n===", "setextline-two"),
        ("# tab\there", "tabhere"),
        ("# a[^1] b\n\n[^1]: f", "a1-b"),
        ("# ‿under‿", "‿under‿"),
        ("# မြန်မာ", "မြန်မာ"),
        ("# ខ្មែរ", "ខ្មែរ"),
        // These already agreed and must stay that way.
        ("# foo&nbsp;bar", "foobar"),
        ("# Straße ẞ", "straße-ß"),
        ("# ![alt text](x.png) Title", "-title"),
        ("# $x^2$ math", "x2-math"),
        ("# Q&amp;A", "qa"),
        ("# ⚙️ Config", "\u{fe0f}-config"),
    ] {
        let d = parse(src);
        assert_eq!(d.str(d.headings[0].slug), slug, "{src}");
    }
}

#[test]
fn heading_titles_are_one_line() {
    let d =
        parse("Setext  \nline two\n===\n\n<h2>a<br>b</h2>\n\n<h3><img src=x.png><br>Logo</h3>\n");
    let titles: Vec<&str> = d.headings.iter().map(|h| d.str(h.title)).collect();
    assert_eq!(titles, ["Setext line two", "a b", "Logo"]);
}

// Review findings: limits on hostile input

fn assert_quick(what: &str, limit: Duration, src: &str) -> Document {
    let t = Instant::now();
    let d = parse(src);
    assert!(t.elapsed() < limit, "{what} took {:?}", t.elapsed());
    d
}

#[test]
fn ambient_tags_cannot_multiply_memory() {
    for (name, src) in [
        (
            "repeated",
            format!("{}\n{}", "<i>\n".repeat(30_000), "a\n\n".repeat(30_000)),
        ),
        (
            "div then many",
            format!(
                "<div></div>{}\n\n{}",
                "<i>".repeat(5000),
                "p\n\n".repeat(50_000)
            ),
        ),
        (
            "links",
            format!(
                "{}\n{}",
                "<a href=x>\n".repeat(30_000),
                "a\n\n".repeat(30_000)
            ),
        ),
        (
            "every tag",
            format!(
                "<div>{}\n\n{}",
                "<i><b><u><s><code><kbd><sub><sup><mark><a href=x>".repeat(100),
                "a\n\n".repeat(100_000)
            ),
        ),
    ] {
        let d = assert_quick(name, Duration::from_secs(2), &src);
        assert!(
            d.spans.len() <= src.len() / 2,
            "{name}: {} spans from {} bytes",
            d.spans.len(),
            src.len()
        );
    }
    // The budget ran out on this one, and the reader is told.
    let src = format!("<div><i>\n\n{}", "a\n\n".repeat(100_000));
    let d = parse(&src);
    assert!(d.warnings.contains(&Warning::UnclosedHtmlDropped));
    // An ordinary document with a forgotten closer keeps its formatting throughout.
    let src = format!("<div><b>\n\n{}", "some paragraph text\n\n".repeat(1000));
    let d = parse(&src);
    assert!(d.warnings.is_empty());
    assert_eq!(
        d.spans
            .iter()
            .filter(|s| s.kind == SpanKind::Strong)
            .count(),
        1000
    );
    // Closing works on the single entry that stands for repeats.
    let d = parse("<div><i><i>\n\nslanted\n\n</i>\n\nplain\n");
    let em: Vec<String> = d
        .spans
        .iter()
        .filter(|s| s.kind == SpanKind::Emphasis)
        .map(|s| span_text(&d, s))
        .collect();
    assert_eq!(
        em.iter().filter(|s| !s.is_empty()).collect::<Vec<_>>(),
        ["slanted"]
    );
}

#[test]
fn unmatched_closers_and_deep_html_stay_linear() {
    let n = 300_000;
    for (name, src) in [
        (
            "div closers",
            format!("{}\n{}\n", "<div>".repeat(n), "</p>".repeat(n)),
        ),
        (
            "span closers",
            format!("{}{}", "<i>".repeat(n), "</b>".repeat(n)),
        ),
        (
            "p span closers",
            format!("<p>{}{}", "<i>".repeat(n), "</u>".repeat(n)),
        ),
        (
            "x span closers",
            format!("x{}{}", "<i>".repeat(n), "</b>".repeat(n)),
        ),
        (
            "li",
            format!("{}{}", "<div>".repeat(n), "<li></li>".repeat(n)),
        ),
        (
            "li under list",
            format!("<ul><li>{}{}", "<div>".repeat(n), "<li></li>".repeat(n)),
        ),
        (
            "closer beyond markdown",
            format!("<p>\n\n> {}{}", "<div>".repeat(n), "</p>".repeat(n)),
        ),
        (
            "closed under a block",
            format!("<div><u><p>{}{}", "<i>".repeat(n), "</u>".repeat(n)),
        ),
        (
            "cells",
            format!("<table><tr>{}{}", "<div>".repeat(n), "<td>".repeat(n)),
        ),
    ] {
        let d = assert_quick(name, Duration::from_secs(5), &src);
        let depth = d.blocks.iter().rev().take(1).count();
        assert!(depth <= 1);
    }

    let d = parse(&"<div>".repeat(5000));
    assert!(d.blocks.len() <= mdshared_md_core::limits::MAX_HTML_DEPTH);
    // Text past the limit is still there.
    let d = parse(&format!("{}deep text", "<div>".repeat(5000)));
    assert!(d.text.contains("deep text"));
}

#[test]
fn backslash_runs_stay_linear_and_escape_correctly() {
    let src = "\\".repeat(1024 * 1024);
    let d = assert_quick("backslashes", Duration::from_secs(1), &src);
    assert_eq!(d.text.trim_end().len(), 512 * 1024);

    // Whether the `_` is escaped decides if the email link can include it, for odd and even runs that
    // span several text events.
    for (n, link) in [
        (0, "_a@b.c"),
        (1, "a@b.c"),
        (2, "_a@b.c"),
        (3, "a@b.c"),
        (4, "_a@b.c"),
        (5, "a@b.c"),
    ] {
        let src = format!("{}_a@b.c", "\\".repeat(n));
        assert_eq!(links(&parse(&src))[0].0, link, "{src}");
    }
    assert_eq!(parse("\\\\*a*").spans[0].kind, SpanKind::Emphasis);
    assert_eq!(parse("\\\\\\*a*").spans.len(), 0);
    assert_eq!(parse("\\\\\\*a*").text.trim_end(), "\\*a*");
}

// Review findings: dropped inline elements

#[test]
fn literal_tags_never_swallow_what_follows() {
    let d = parse("- <object> `</object>`\n\nparagraph one\n\n# Heading two\n\nparagraph three\n");
    assert!(d.text.contains("</object>"), "{:?}", d.text);
    assert!(d.text.contains("paragraph one") && d.text.contains("paragraph three"));
    assert_eq!(d.headings.len(), 1);

    let d = parse("- <embed> `</embed>`\n- second item\n\nall the rest\n");
    assert!(d.text.contains("second item") && d.text.contains("all the rest"));

    // Tag-filtered tags show literally, and their content is treated as ordinary HTML text.
    let d = parse("a <script> `</script>` and more text");
    assert!(d.text.contains("<script>") && d.text.contains("</script>"));
    assert!(d.block_text(0).contains("and more text"));
    assert_eq!(d.spans[0].kind, SpanKind::Code);

    let d = parse("a <embed> \\</embed> visible?");
    assert_eq!(d.block_text(0).trim_end(), "a </embed> visible?");

    assert_eq!(
        parse("a <script>x</script> b").block_text(0).trim_end(),
        "a <script>x</script> b"
    );
    let d = parse("a <script>*x* `y`</script> b");
    assert!(d.text.contains("<script>") && d.text.contains("</script>"));
    assert!(d.spans.iter().any(|s| s.kind == SpanKind::Code));

    // Object keeps its fallback text now, as GitHub does.
    let d = parse("- <object>gone</object> after\n\nnext\n");
    assert!(d.text.contains("after") && d.text.contains("next") && d.text.contains("gone"));

    // Markdown delimiters around a literal tag stay balanced.
    let d = parse("a <script> *b </script> c* d");
    assert!(d.text.contains("b") && d.text.contains("c") && d.text.contains("d"));
    // A closer in the next block does not count, and both tags still show.
    let d = parse("a <script> b\n\nc </script> d");
    assert!(
        d.text.contains("<script> b") && d.text.contains("c </script> d"),
        "{:?}",
        d.text
    );

    // Many tags in one block stay linear and all show.
    let src = format!("x {}`</script>`", "<script> a ".repeat(50_000));
    let d = assert_quick("waiting", Duration::from_secs(2), &src);
    assert_eq!(d.text.matches("<script>").count(), 50_000);
}

// Review findings: presentation attributes

#[test]
fn html_presentation_attributes_reach_the_model() {
    let d = parse("<div dir=\"rtl\">\n\nمرحبا\n\n</div>\n");
    assert_eq!(d.attrs_of(0).unwrap().dir, Some(Dir::Rtl));
    assert!(d.attrs_of(1).is_none());

    let d = parse("<p dir=\"rtl\" align=\"justify\">نص Latin first</p>\n");
    assert_eq!(
        d.blocks[0].kind,
        BlockKind::Paragraph {
            align: Align::Justify
        }
    );
    assert_eq!(d.attrs_of(0).unwrap().dir, Some(Dir::Rtl));

    let d = parse(
        "<table align=\"center\" width=\"100%\"><tr><td width=\"50%\" height=\"20\" valign=\"top\" dir=\"auto\">x</td></tr></table>\n",
    );
    let table = d.attrs_of(0).unwrap();
    assert_eq!(
        (table.align, table.width),
        (Align::Center, Some(Length::Percent(100)))
    );
    let cell = d.attrs_of(2).unwrap();
    assert!(matches!(d.blocks[2].kind, BlockKind::TableCell { .. }));
    assert_eq!(cell.width, Some(Length::Percent(50)));
    assert_eq!(cell.height, Some(Length::Px(20)));
    assert_eq!(cell.valign, Some(VAlign::Top));
    assert_eq!(cell.dir, Some(Dir::Auto));

    let d = parse("<hr width=\"50%\" align=\"left\">\n");
    assert_eq!(d.blocks[0].kind, BlockKind::ThematicBreak);
    assert_eq!(d.attrs_of(0).unwrap().width, Some(Length::Percent(50)));
    assert_eq!(d.attrs_of(0).unwrap().align, Align::Left);

    let d = parse(
        "<ol type=\"a\"><li>x</li></ol>\n\n<ol type=\"A\"><li>y</li></ol>\n\n<ul type=\"SQUARE\"><li>z</li></ul>\n",
    );
    let markers: Vec<_> = d.block_attrs.iter().map(|a| a.marker).collect();
    assert_eq!(
        markers,
        [
            Some(ListMarker::LowerAlpha),
            Some(ListMarker::UpperAlpha),
            Some(ListMarker::Square)
        ]
    );

    let d = parse("<img src=\"logo.png\" align=\"right\">\n");
    assert_eq!(d.images[0].align, ImageAlign::Right);
    let d = parse("![x](y.png)\n");
    assert_eq!(d.images[0].align, ImageAlign::None);

    // A dropped empty paragraph takes its attributes with it, so the table stays sorted and true.
    let d = parse("<p dir=\"rtl\"></p>\n\n<div dir=\"ltr\">x</div>\n");
    assert_eq!(d.block_attrs.len(), 1);
    assert_eq!(d.attrs_of(0).unwrap().dir, Some(Dir::Ltr));

    // Attributes inline in a paragraph, and on tags with nothing to record.
    let d = parse("a <div dir=\"rtl\">b</div>\n");
    assert_eq!(d.block_attrs.len(), 1);
    let d = parse("<div class=\"x\">b</div>\n");
    assert!(d.block_attrs.is_empty());
}

#[test]
fn image_without_a_usable_source_keeps_its_alt() {
    let d = parse("<img alt=\"no src\">\n");
    let img = d.images[0];
    assert_eq!(d.str(img.alt), "no src");
    assert_eq!(img.src.len, 0);
    assert!(d.spans.iter().any(|s| matches!(s.kind, SpanKind::Image(0))));

    let d = parse("<img src=\"javascript:alert(1)\" alt=\"bad\">\n");
    assert_eq!(d.str(d.images[0].alt), "bad");
    assert_eq!(d.images[0].src.len, 0);
    assert!(!d.strings.contains("javascript"));

    // With nothing to show there is still nothing to add.
    assert!(parse("<img>\n").images.is_empty());
}

// Review findings: links

#[test]
fn urls_naming_a_host_never_reach_the_filesystem() {
    let dir = Path::new("/docs");
    for src in [
        "//evil.example/share/x.md",
        "\\\\evil.example\\share\\x.md",
        "/\\evil.example\\share\\x.md",
        "\\/evil.example/share/x.md",
        "%2F%2Fevil.example/share/x.md",
        "%5C%5Cevil.example%5Cshare%5Cx.md",
        "/%5Cevil.example/x.md",
        "\\\\.\\pipe\\x.md",
        "\\\\?\\UNC\\evil\\share\\x.md",
        "//evil.example:445/share/x.md",
        "file:////evil.example/share/x.md",
        "file:\\\\evil.example\\share\\x.md",
        "file:%5C%5Cevil.example%5Cx.md",
        "file://evil.example/share/x.md",
    ] {
        assert_eq!(classify_link(src, dir), LinkAction::Ignored, "{src}");
        assert_eq!(classify_image(src, dir), ImageSource::Unsupported, "{src}");
    }
    // Ordinary local forms are unaffected.
    assert_eq!(
        classify_image("/abs/a.png", dir),
        ImageSource::Local(PathBuf::from("/abs/a.png"))
    );
    assert_eq!(
        classify_image("sub//a.png", dir),
        ImageSource::Local(PathBuf::from("/docs/sub/a.png"))
    );
    assert_eq!(
        classify_link("file:///abs/a.md", dir),
        LinkAction::OpenMarkdown {
            path: PathBuf::from("/abs/a.md"),
            fragment: None
        }
    );
}

// Review findings: reload position

#[test]
fn reload_does_not_jump_to_a_repeated_block_elsewhere() {
    let build = |first: &str| {
        (0..50)
            .map(|i| match i {
                11 => first.to_string(),
                42 => "Fixed".to_string(),
                _ => format!("para {i}"),
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let old = parse(&build("Fixed"));
    let new = parse(&build("Changed"));
    let pos = ScrollPos {
        block: 11,
        offset: 5.0,
    };
    assert_eq!(new.remap_scroll(&old, pos), pos);

    // Before the edit stays, after it shifts by the change in length.
    let old = parse("a\n\nb\n\nc\n\nd\n\ne");
    let added = parse("a\n\nb\n\nnew\n\nnew\n\nc\n\nd\n\ne");
    let at = |block| ScrollPos { block, offset: 1.0 };
    assert_eq!(added.remap_scroll(&old, at(1)).block, 1);
    assert_eq!(added.remap_scroll(&old, at(3)).block, 5);
    let removed = parse("a\n\nd\n\ne");
    assert_eq!(removed.remap_scroll(&old, at(4)).block, 2);
    assert_eq!(removed.remap_scroll(&old, at(0)).block, 0);
    // Inside the removed part, and past the end, it stays in range.
    assert_eq!(removed.remap_scroll(&old, at(2)).block, 2);
    assert_eq!(removed.remap_scroll(&old, at(99)).block, 2);
    // Repeated blocks at the edit point cannot make the two ends overlap.
    let old = parse("x\n\nx\n\nx");
    let new = parse("x\n\nx");
    assert!(new.remap_scroll(&old, at(2)).block <= 1);
    assert_eq!(parse("").remap_scroll(&old, at(2)), ScrollPos::default());
}

// Review findings: find and closed details

#[test]
fn find_skips_closed_details_bodies() {
    let d =
        parse("<details><summary>More needle</summary>\n\nSecret needle\n\n</details>\n\nneedle");
    let details = d
        .blocks
        .iter()
        .position(|b| matches!(b.kind, BlockKind::Details { .. }))
        .unwrap() as u32;
    assert_eq!(find(&d, "needle").hits.len(), 2);
    assert_eq!(find(&d, "Secret").hits.len(), 0);
    assert_eq!(find_visible(&d, "needle", |_| true).hits.len(), 3);
    assert_eq!(find_visible(&d, "Secret", |b| b == details).hits.len(), 1);
    assert_eq!(find_visible(&d, "needle", |_| false).hits.len(), 2);

    let d = parse("<details open>\n\nSecret\n\n</details>\n");
    assert_eq!(find(&d, "Secret").hits.len(), 1);
    assert_eq!(find_visible(&d, "Secret", |_| false).hits.len(), 0);

    // No summary hides everything, nesting stays ordered, and math in a summary is still hidden.
    let d = parse(
        "<details>\n<summary>\n\n$x$ s1\n\n</summary>\n\nb1\n\n<details>\n<summary>s2</summary>\n\nb2\n\n</details>\n\nb3\n\n</details>\n\nb4\n\n$$q$$\n",
    );
    for (query, hits) in [
        ("s1", 1),
        ("b1", 0),
        ("s2", 0),
        ("b2", 0),
        ("b3", 0),
        ("b4", 1),
        ("x", 0),
        ("q", 0),
    ] {
        assert_eq!(find(&d, query).hits.len(), hits, "{query}");
    }
    let hidden = d.hidden_ranges(&|_| false);
    assert!(
        hidden.windows(2).all(|w| w[0].end <= w[1].start),
        "{hidden:?}"
    );
    // Open outer with closed inner.
    let outer = 0;
    assert_eq!(find_visible(&d, "b1", |b| b == outer).hits.len(), 1);
    assert_eq!(find_visible(&d, "s2", |b| b == outer).hits.len(), 1);
    assert_eq!(find_visible(&d, "b2", |b| b == outer).hits.len(), 0);
}
