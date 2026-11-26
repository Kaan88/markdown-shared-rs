use super::*;

// Review findings: Windows paths

// Review findings: minor

/// Each expected text is what `gh api markdown -f mode=gfm -f context=cli/cli` showed, with the line
/// break that mode adds read as the soft break it is in a file.
#[test]
fn misplaced_or_empty_alerts_stay_plain_quotes_with_their_marker() {
    for (src, para) in [
        ("> [!TIP]", "[!TIP]"),
        ("> [!tip]\n>\n", "[!tip]"),
        ("> [!NOTE]\n\nafter", "[!NOTE]"),
        ("> [!NOTE]\n> <!-- c -->", "[!NOTE]"),
        ("- > [!NOTE]\n  > in list", "[!NOTE] in list"),
        ("> > [!NOTE]\n> > nested", "[!NOTE] nested"),
        ("<div>\n\n> [!NOTE]\n> x\n\n</div>", "[!NOTE] x"),
        ("<details>\n\n> [!NOTE]\n> x\n\n</details>", "[!NOTE] x"),
        (
            "<blockquote>\n\n> [!NOTE]\n> x\n\n</blockquote>",
            "[!NOTE] x",
        ),
        ("<ul><li>\n\n> [!NOTE]\n> x\n\n</li></ul>", "[!NOTE] x"),
        (
            "<table><tr><td><div>\n\n> [!NOTE]\n> x\n\n</div></td></tr></table>",
            "[!NOTE] x",
        ),
        ("x[^a]\n\n[^a]: > [!NOTE]\n    > in note", "[!NOTE] in note"),
        ("> [!NOTE]\n> > [!TIP]\n> > inner", "[!TIP] inner"),
    ] {
        let d = parse(src);
        let paragraph = d
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| matches!(b.kind, BlockKind::Paragraph { .. }))
            .map(|(i, _)| d.block_text(i as u32).trim_end())
            .any(|t| t == para);
        assert!(paragraph, "{src:?}: {:?}", d.text);
        assert!(
            d.blocks.iter().any(|b| b.kind == BlockKind::BlockQuote),
            "{src:?}"
        );
        assert!(!d.text.contains('\u{E004}'), "{src:?}");
    }
    // The one nested marker above stays plain inside an alert that is a real one.
    assert!(has_alert(&parse("> [!NOTE]\n> > [!TIP]\n> > inner")));
    for src in [
        "> [!TIP]",
        "- > [!NOTE]\n  > x",
        "> > [!NOTE]\n> > x",
        "<div>\n\n> [!NOTE]\n> x\n\n</div>",
    ] {
        assert!(!has_alert(&parse(src)), "{src:?}");
    }
    // With `$` in the document, the same pass that checks math checks this too.
    let d = parse("- > [!NOTE]\n  > costs $5 or $6 today\n\n$x$\n");
    assert!(!has_alert(&d));
    assert!(d.text.contains("[!NOTE] costs $5"), "{:?}", d.text);
}

#[test]
fn a_setext_underline_takes_the_marker_line_as_on_github() {
    let d = parse("> [!NOTE]\n> ---\n");
    assert!(!has_alert(&d));
    assert_eq!(d.str(d.headings[0].title), "[!NOTE]");
    let d = parse("> [!NOTE]\n> text\n> ---\n");
    assert!(!has_alert(&d));
    assert_eq!(d.str(d.headings[0].title), "[!NOTE] text");
    assert!(has_alert(&parse("> [!NOTE]\n> # real heading\n")));
    assert!(has_alert(&parse("> [!NOTE]\n> * * *\n")));
}

#[test]
fn real_alerts_are_kept_where_github_keeps_them() {
    for src in [
        "> [!NOTE]\n> body",
        "> [!tip]\n> lower",
        "> [!TIP]  \n> trailing spaces",
        "> [!NOTE]\n>\n> after a blank",
        "> [!NOTE]\n> - a",
        "> [!NOTE]\n> ```\n> code\n> ```",
        "<table><tr><td>\n\n> [!NOTE]\n> in td\n\n</td></tr></table>",
        "<table><tr><th>\n\n> [!NOTE]\n> in th\n\n</th></tr></table>",
        "<center>\n\n> [!NOTE]\n> x\n\n</center>",
        "<section>\n\n> [!NOTE]\n> x\n\n</section>",
    ] {
        assert!(has_alert(&parse(src)), "{src:?}");
    }
    assert_eq!(
        parse("> [!NOTE] title\n> body").text,
        "[!NOTE] title body\n"
    );
}

#[test]
fn rejected_alert_is_still_kept_in_a_document_that_cannot_hide_it() {
    let d = parse("\u{E004}\n\n- > [!NOTE]\n  > x\n\n> [!TIP]\n");
    assert!(!has_alert(&d));
    assert!(
        d.text.contains("[!NOTE]") && d.text.contains("[!TIP]"),
        "{:?}",
        d.text
    );
    assert!(
        d.text.contains('\u{E004}'),
        "the document's own character stays"
    );
    let d = parse("> [!NOTE]\n> ---\n\n\u{E004}");
    assert!(!has_alert(&d) && d.text.contains("[!NOTE]"), "{:?}", d.text);
    // Nor does a character from that block change because a URL elsewhere was protected.
    let d = parse("\u{E004} \u{E003} http://a.b/c_d");
    assert!(d.text.contains("\u{E004} \u{E003}"), "{:?}", d.text);
}

#[test]
fn first_footnote_definition_wins_and_the_later_subtree_goes() {
    let d = parse("[^a]: first\n\n[^a]: second *x* http://a.b/ [l](u)\n\n# h\n\nx[^a]\n");
    assert_eq!(d.footnotes.len(), 1);
    assert_eq!(d.block_text(d.footnotes[0].block).trim_end(), "first");
    let defs = d
        .blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::FootnoteDefinition { .. }))
        .count();
    assert_eq!(defs, 1);
    assert!(!d.text.contains("second"), "{:?}", d.text);
    assert!(d.links.is_empty());
    // Any label case is the same footnote, and what follows is untouched.
    let d = parse("x[^a]\n\n[^A]: first\n\n[^a]:\n    > second\n\n    more\n\nafter\n");
    assert_eq!(d.footnotes.len(), 1);
    assert!(d.text.contains("first") && d.text.contains("after"));
    assert!(!d.text.contains("second") && !d.text.contains("more"));
}

fn cell(d: &Document, n: usize) -> (u16, u16) {
    d.blocks
        .iter()
        .filter_map(|b| match b.kind {
            BlockKind::TableCell {
                colspan, rowspan, ..
            } => Some((colspan, rowspan)),
            _ => None,
        })
        .nth(n)
        .expect("a cell")
}

#[test]
fn html_spans_and_lengths_are_clamped() {
    for (attrs, want) in [
        (r#"colspan="0" rowspan="0""#, (1, 1)),
        ("colspan=1000 rowspan=65534", (1000, 65534)),
        ("colspan=1001 rowspan=65535", (1000, 65534)),
        (
            "colspan=99999999999999999999999 rowspan=70000",
            (1000, 65534),
        ),
        ("colspan=x rowspan=-1", (1, 1)),
        ("colspan='2x' rowspan=' 3 '", (2, 3)),
    ] {
        let d = parse(&format!("<table><tr><td {attrs}>a</td></tr></table>"));
        assert_eq!(cell(&d, 0), want, "{attrs}");
    }
    for (attrs, width, height) in [
        ("width=inf height=NaN", None, None),
        ("width=infinity height=-1", None, None),
        ("width=99999999999999", Some(2000), None),
        ("height=1e30", None, Some(1)),
        ("width=3000 height=100", Some(2000), Some(67)),
        ("width=12001px height=11999", Some(2000), Some(2000)),
    ] {
        let d = parse(&format!("<img src=a {attrs}>"));
        let px = |l: Option<Length>| {
            l.map(|l| match l {
                Length::Px(n) => n,
                other => panic!("{other:?}"),
            })
        };
        assert_eq!(px(d.images[0].width), width, "{attrs}");
        assert_eq!(px(d.images[0].height), height, "{attrs}");
    }
    let d = parse("<img src=a width=inf% height=5000%>");
    assert_eq!(d.images[0].width, None);
    assert_eq!(d.images[0].height, Some(Length::Percent(1000)));
    let d = parse("<table width=inf height=20000><tr><td>a</td></tr></table>");
    let attrs = d.attrs_of(0).unwrap();
    assert_eq!((attrs.width, attrs.height), (None, Some(Length::Px(12000))));
}

#[test]
fn html_lengths_follow_the_dimension_rule() {
    // HTML's rule: digits, an optional fraction, then `%` only if it comes immediately. Anything
    // else ends the number, so `10em` is 10 while `+5` and `.5` are not lengths at all.
    for (value, want) in [
        ("10em", Some(Length::Px(10))),
        ("100abc", Some(Length::Px(100))),
        ("1e3", Some(Length::Px(1))),
        ("+5", None),
        (".5", None),
        ("-1", None),
        ("50 %", Some(Length::Px(50))),
        ("300px;", Some(Length::Px(300))),
        ("300;", Some(Length::Px(300))),
        ("100%;", Some(Length::Percent(100))),
        ("2.5", Some(Length::Px(2))),
        (" 3 ", Some(Length::Px(3))),
    ] {
        let d = parse(&format!("<img src=a width=\"{value}\">"));
        assert_eq!(d.images[0].width, want, "{value:?}");
    }
    // A leading `+` is a number for colspan, as HTML reads it.
    let d = parse("<table><tr><td colspan=\"+2\">a</td></tr></table>");
    assert_eq!(cell(&d, 0).0, 2);
    // GitHub clamps image width and height to 2000; table attributes stop at 12000.
    let d = parse("<img src=a width=5000px>");
    assert_eq!(d.images[0].width, Some(Length::Px(2000)));
    let d = parse("<img src=a height=99999>");
    assert_eq!(d.images[0].height, Some(Length::Px(2000)));
    let d = parse("<img src=a width=3000 height=100>");
    assert_eq!(d.images[0].width, Some(Length::Px(2000)));
    assert_eq!(d.images[0].height, Some(Length::Px(67)));
    let d = parse("<table width=99999><tr><td>a</td></tr></table>");
    assert_eq!(d.attrs_of(0).unwrap().width, Some(Length::Px(12000)));
}

#[test]
fn orphan_cells_and_rows_keep_their_text() {
    for src in ["<td>raw</td>", "<tr><td>raw</td></tr>", "<th>raw</th>"] {
        let d = parse(src);
        assert_eq!(kinds(&d), [BlockKind::Plain], "{src}");
        assert_eq!(d.text, "raw\n", "{src}");
    }
    let d = parse("a <td>b</td> c");
    assert_eq!(d.blocks.len(), 1, "a browser does not end the paragraph");
    assert!(d.text.contains('a') && d.text.contains('b') && d.text.contains('c'));
    let d = parse("| h |\n|---|\n| a <tr>b<td>c |\n");
    // This expected value differs from GitHub: inside a GFM cell GitHub moves `b` before the table
    // and splits `a` and `c` into separate rows. The difference is listed in COVERAGE's known gaps.
    assert_eq!(d.block_text(d.blocks.len() as u32 - 1).trim_end(), "a bc");
}

#[test]
fn table_content_always_has_a_row_and_a_cell() {
    let is_row =
        |d: &Document, b: u32| matches!(d.blocks[b as usize].kind, BlockKind::TableRow { .. });
    let d = parse("<table><td>direct</td></table>");
    let row = d.children(0).next().unwrap();
    assert!(is_row(&d, row));
    assert!(matches!(
        d.blocks[d.children(row).next().unwrap() as usize].kind,
        BlockKind::TableCell { .. }
    ));
    // A `<td>` with no row still gets its implied parents, and keeps its own attributes.
    let d = parse("<table><td colspan=2>a</td></table>");
    assert_eq!(cell(&d, 0).0, 2, "{:?}", kinds(&d));

    let d = parse("<table><tr>\n\nStray\n\n</tr><td>no row</td>text in table</table>\n\nout");
    assert_eq!(d.text, "Stray\nno row\ntext in table\nout\n");
    let rows: Vec<u32> = d.children(0).collect();
    assert_eq!(rows.len(), 2, "{:?}", kinds(&d));
    assert!(rows.iter().all(|&r| is_row(&d, r)));
    assert_eq!(d.children(rows[0]).count(), 1);
    assert_eq!(d.children(rows[1]).count(), 2);
    assert!(
        matches!(d.blocks.last().unwrap().kind, BlockKind::Paragraph { .. })
            && d.blocks.last().unwrap().parent == NONE,
        "the table ended"
    );

    let d = parse("<table>x<tr><td>a</td></tr>y</table>z");
    assert_eq!(d.text, "x\na\ny\nz\n");
    assert_eq!(d.children(0).count(), 3);
    assert!(d.children(0).all(|r| is_row(&d, r)));
}

#[test]
fn list_content_always_has_an_item() {
    let d = parse("<ul>x<li>a</li>y</ul>");
    assert_eq!(d.text, "x\na\ny\n");
    let items: Vec<u32> = d.children(0).collect();
    assert_eq!(items.len(), 3);
    assert!(
        items
            .iter()
            .all(|&i| matches!(d.blocks[i as usize].kind, BlockKind::ListItem { .. }))
    );
    let marker = |b: u32| d.attrs_of(b).and_then(|a| a.marker);
    assert_eq!(marker(items[0]), Some(ListMarker::None));
    assert_eq!(marker(items[1]), None, "a real item keeps its marker");

    let d = parse("<ul><ul><li>a</li></ul></ul>");
    let outer_item = d.children(0).next().unwrap();
    assert!(matches!(
        d.blocks[outer_item as usize].kind,
        BlockKind::ListItem { .. }
    ));
    assert_eq!(
        d.attrs_of(outer_item).unwrap().marker,
        Some(ListMarker::None)
    );

    // A lone item is kept, as on GitHub, in a list that lasts only while items keep coming.
    let d = parse("<li>a</li><li>b</li>text\n\npara");
    assert_eq!(
        kinds(&d)[..3],
        [
            BlockKind::List { start: None },
            BlockKind::ListItem { task: false },
            BlockKind::Plain
        ]
    );
    assert_eq!(d.children(NONE).count(), 3, "{:?}", kinds(&d));
    assert_eq!(d.text, "a\nb\ntext \npara\n");
}

#[test]
fn display_math_has_no_carriage_returns() {
    for src in ["$$\r\nm\r\nn\r\n$$\r\n", "$$\rm\rn\r$$\r"] {
        let d = parse(src);
        assert!(matches!(d.blocks[0].kind, BlockKind::Math { .. }));
        assert_eq!(d.block_text(0), "\nm\nn\n", "{src:?}");
        assert!(!d.text.contains('\r'));
    }
    assert!(!parse("a $x\r\ny$ b\r\n").text.contains('\r'));
}

#[test]
fn bare_url_ending_in_an_entity_trims_as_on_github() {
    for (src, link, text) in [
        ("http://a.b/c&amp; x", "http://a.b/c", "http://a.b/c& x\n"),
        ("http://a.b/c&gt; x", "http://a.b/c", "http://a.b/c> x\n"),
        ("http://a.b/c&lt; x", "http://a.b/c", "http://a.b/c< x\n"),
        ("http://a.b/c&quot; x", "http://a.b/c", "http://a.b/c\" x\n"),
        ("http://a.b/c&copy; x", "http://a.b/c", "http://a.b/c© x\n"),
        ("http://a.b/c&x; x", "http://a.b/c", "http://a.b/c&x; x\n"),
        ("http://a.b/c&amp;.", "http://a.b/c", "http://a.b/c&.\n"),
        (
            "http://a.b/c.&amp;, x",
            "http://a.b/c",
            "http://a.b/c.&, x\n",
        ),
        ("http://a.b/c&amp;) x", "http://a.b/c", "http://a.b/c&) x\n"),
        ("(http://a.b/c&amp;)", "http://a.b/c", "(http://a.b/c&)\n"),
        ("www.a.b/c&amp; x", "www.a.b/c", "www.a.b/c& x\n"),
        // No entity: these stay in, and so does a numeric or digit-bearing form's `&`.
        ("http://a.b/c& x", "http://a.b/c&", "http://a.b/c& x\n"),
        (
            "http://a.b/c&x1; x",
            "http://a.b/c&x1",
            "http://a.b/c&x1; x\n",
        ),
        // `frac12` and `sup2` are entities, but their names carry digits, so the URL does not trim
        // on them; the decoded character stays inside the link. GitHub links the raw spelling
        // instead, which is the decoded-entity gap in COVERAGE.
        (
            "http://a.b/c&frac12; x",
            "http://a.b/c½",
            "http://a.b/c½ x\n",
        ),
        ("http://a.b/c&sup2; x", "http://a.b/c²", "http://a.b/c² x\n"),
    ] {
        let d = parse(src);
        assert_eq!(links(&d)[0].0, link, "{src}");
        assert_eq!(d.text, text, "{src}");
    }
    // The trim still works when the run does not start at offset 0.
    assert_eq!(
        links(&parse("para\n\nhttp://a.b/c&amp; x"))[0].0,
        "http://a.b/c"
    );
    let d = parse("y www.a.b/c&amp; x");
    assert_eq!(links(&d)[0].1, "http://www.a.b/c");
    // Only a URL's end is affected.
    assert_eq!(links(&parse("http://a.b/c&amp;d x"))[0].0, "http://a.b/c&d");
    let d = parse("http://a.b/c&amp; http://d.e/f&amp; x");
    let got: Vec<_> = links(&d).into_iter().map(|l| l.0).collect();
    assert_eq!(got, ["http://a.b/c", "http://d.e/f"]);
}

#[test]
fn find_matches_a_typed_space_against_a_non_breaking_one() {
    assert_eq!(find_in_text("a\u{a0}b", "a b", 100).hits.len(), 1);
    assert_eq!(find_in_text("a\u{a0}b", "a\u{a0}b", 100).hits.len(), 1);
    assert_eq!(find_in_text("a b", "a\u{a0}b", 100).hits.len(), 1);
    // The query's first character takes the byte-search shortcut, which must see it too.
    let hits = find_in_text("x\u{a0}y z", " ", 100).hits;
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].usize(), 1..3);
    assert_eq!(find_in_text("©\u{a0}y", " y", 100).hits.len(), 1);
    let d = parse("a&nbsp;b");
    assert_eq!(find(&d, "a b").hits.len(), 1);
}

#[test]
fn files_under_the_limit_never_load_blank() {
    let mut utf16 = vec![0xFF, 0xFE];
    while utf16.len() < MAX_FILE_BYTES {
        utf16.extend_from_slice(&[0xE5, 0x65]); // 日 in UTF-16LE
    }
    utf16.truncate(MAX_FILE_BYTES);
    let d = mdshared_md_core::parse_bytes(&utf16);
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
    assert!(d.text.len() > MAX_FILE_BYTES);

    let d = mdshared_md_core::parse_bytes(&vec![0xFF; 6 * 1024 * 1024]);
    assert_eq!(d.warnings, [Warning::InvalidUtf8]);
    assert!(d.text.len() > 6 * 1024 * 1024);

    let d = mdshared_md_core::parse_bytes(&vec![0xFF; MAX_FILE_BYTES]);
    assert_eq!(d.warnings, [Warning::InvalidUtf8]);
    assert!(!d.text.is_empty());
}

#[test]
fn details_keys_count_repeats_and_stay_fast_on_many_summaries() {
    let d = parse(
        "<details><summary>s</summary></details>\n\n<details><summary>s</summary></details>\n\n<details><summary>t</summary></details>\n",
    );
    let keys = d.details_keys();
    assert_eq!(keys.len(), 3);
    assert_ne!(keys[0].1, keys[1].1);
    assert_ne!(keys[0].1, keys[2].1);
    let many: String = (0..200_000)
        .map(|i| format!("<details><summary>s{i}</summary></details>\n"))
        .collect();
    let d = parse(&many);
    let t = Instant::now();
    assert_eq!(d.details_keys().len(), 200_000);
    // The list this replaced took over 4 s here. Far from the limit, so load can't fail it.
    assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
}

#[test]
fn picture_sources_only_count_inside_a_picture_until_its_image() {
    let dark = r#"media="(prefers-color-scheme: dark)" srcset=d.png"#;
    let d = parse(&format!("<source {dark}>\n\n<p>x</p>\n\n<img src=a>\n"));
    assert_eq!(d.images[0].dark_src, StrRef::default());
    let d = parse(&format!("<picture><source {dark}><img src=a></picture>"));
    assert_eq!(d.str(d.images[0].dark_src), "d.png");
    // Unclosed, it still ends with its image.
    let d = parse(&format!(
        "<picture><source {dark}><img src=a><img src=b>\n\n<img src=c>\n"
    ));
    assert_eq!(d.str(d.images[0].dark_src), "d.png");
    assert_eq!(d.images[1].dark_src, StrRef::default());
    assert_eq!(d.images[2].dark_src, StrRef::default());
    // And a later source without a new picture does nothing.
    let d = parse(&format!(
        "<picture><img src=a></picture><source {dark}><img src=b>"
    ));
    assert_eq!(d.images[1].dark_src, StrRef::default());
    // A picture that closes without an image leaves nothing for a later image to pick up.
    let d = parse(&format!("<picture><source {dark}></picture><img src=b>"));
    assert_eq!(d.images[0].src.len, 1);
    assert_eq!(d.images[0].dark_src, StrRef::default());
    // The picture ends with the block that opened it, unclosed or not.
    for src in [
        format!("<picture><source {dark}>\n\n<img src=b>"),
        format!("<div><picture><source {dark}></div>\n\n<img src=b>"),
        format!("> <picture><source {dark}>\n\n<img src=b>"),
        format!("- <picture><source {dark}>\n- <img src=b>"),
        format!("<p><picture><source {dark}></p>\n<p><img src=b></p>"),
    ] {
        assert_eq!(parse(&src).images[0].dark_src, StrRef::default(), "{src}");
    }
    // A source that is not a direct child of the picture does not count.
    let d = parse(&format!(
        "<picture><div><source {dark}></div><img src=a></picture>"
    ));
    assert_eq!(d.images[0].dark_src, StrRef::default());
    // A browser takes the first matching source, not the last.
    let d = parse(concat!(
        "<picture>",
        "<source media=\"(prefers-color-scheme: dark)\" srcset=d1.png>",
        "<source media=\"(prefers-color-scheme: dark)\" srcset=d2.png>",
        "<img src=a></picture>",
    ));
    assert_eq!(d.str(d.images[0].dark_src), "d1.png");
}

#[test]
fn find_work_is_bounded_on_a_long_query_against_repetitive_text() {
    // Took 3.5 s before.
    let text = "a".repeat(1 << 20);
    let long = format!("{}b", "a".repeat(5000));
    let r = find_in_text(&text, &long, 10_000);
    assert!(r.hits.is_empty() && r.capped);

    // Up to the limit nothing is lost, even at the worst place in the text.
    let query = format!("{}b", "a".repeat(FIND_WORK_PER_BYTE - 1));
    let haystack = format!("{}b", "a".repeat(1 << 16));
    let r = find_in_text(&haystack, &query, 10_000);
    assert_eq!(r.hits.len(), 1);
    assert!(!r.capped);

    // A long query on ordinary text is exact: it fails early almost everywhere.
    let line = "The quick brown fox jumps over the lazy dog. ";
    let page = line.repeat(10_000);
    let query = format!("{} {}", line.repeat(40), "tail");
    let r = find_in_text(&format!("{page}{query}"), &query, 10_000);
    assert_eq!(r.hits.len(), 1);
    assert!(!r.capped);
}

#[test]
fn a_fragment_is_decoded_once() {
    let dir = Path::new("/d");
    let d = parse("<a id=\"a%41\"></a>\n\nx\n\n# Bü\n");
    let fragment = match classify_link("#a%2541", dir) {
        LinkAction::Fragment(f) => f,
        other => panic!("{other:?}"),
    };
    assert_eq!(fragment, "a%2541");
    assert!(d.resolve_fragment(&fragment).is_some());
    assert_eq!(d.resolve_fragment("a%41"), None, "means the anchor aA");
    let fragment = match classify_link("#b%C3%BC", dir) {
        LinkAction::Fragment(f) => f,
        other => panic!("{other:?}"),
    };
    assert_eq!(d.resolve_fragment(&fragment), Some(d.headings[0].block));
    for href in ["o.md#a%2541", "file:///abs/o.md#a%2541"] {
        match classify_link(href, dir) {
            LinkAction::OpenMarkdown { fragment, .. } => {
                assert_eq!(fragment.as_deref(), Some("a%2541"), "{href}")
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn protected_urls_give_the_same_strings_as_unprotected_ones() {
    // Accepting the audit's tests-parity #10: the fewer-allocations change on the bare-URL path
    // produces the same values either way, so no value assertion can tell it from a revert. Only a
    // counting allocator could, and that would be a test of one backend, not of behaviour.
    let plain = parse("![al*t `c` $m$](u.png \"t\") [a](/u_v \"w_x\") [^f]\n\n[^f]: n\n");
    let protected =
        parse("![al*t `c` $m$](u.png \"t\") [a](/u_v \"w_x\") [^f]\n\n[^f]: n\n\nhttp://a.b/c_d\n");
    let image = |d: &Document, r: StrRef| d.str(r).to_string();
    assert_eq!(
        image(&protected, protected.images[0].alt),
        image(&plain, plain.images[0].alt)
    );
    assert_eq!(image(&protected, protected.links[0].dest), "/u_v");
    assert_eq!(image(&protected, protected.links[0].title), "w_x");
    assert_eq!(links(&protected)[1].1, "http://a.b/c_d");
}

fn block_depth(d: &Document) -> usize {
    let mut depth = vec![0usize; d.blocks.len()];
    let mut max = 0;
    for (i, b) in d.blocks.iter().enumerate() {
        depth[i] = if b.parent == NONE {
            1
        } else {
            depth[b.parent as usize] + 1
        };
        max = max.max(depth[i]);
    }
    max
}

#[test]
fn nesting_stops_at_a_fixed_depth_with_the_text_kept() {
    let n = 100_000;
    for (src, text) in [
        (format!("{} a", ">".repeat(n)), "a\n".to_string()),
        (format!("{}x", "<div>".repeat(n)), "x\n".to_string()),
        (
            (0..600)
                .map(|i| format!("{}- l{i}\n", "  ".repeat(i)))
                .collect(),
            (0..600).map(|i| format!("l{i}\n")).collect(),
        ),
        (
            format!(
                "{}<table><tr><td>t</td><td>u</td></tr></table>",
                ">".repeat(n)
            ),
            "t\nu\n".to_string(),
        ),
        (
            format!("{}<ul><li>t</li><li>u</li></ul>", "<div>".repeat(300)),
            "t\nu\n".to_string(),
        ),
        (
            format!(
                "{} | h |\n{} |---|\n{} | c |\n",
                ">".repeat(300),
                ">".repeat(300),
                ">".repeat(300)
            ),
            "h\nc\n".to_string(),
        ),
        // An implied table row, its cell and a leaf inside them reach the real bound, five past the
        // container limit: 254 sections plus a table and its implied row, cell and leaf.
        (
            format!("{}<table><table>x", "<section>".repeat(254)),
            "x\n".to_string(),
        ),
    ] {
        let d = parse(&src);
        assert!(
            block_depth(&d) <= MAX_BLOCK_DEPTH + 5,
            "{}",
            block_depth(&d)
        );
        assert_eq!(d.text, text, "{:.40}", src);
        assert!(d.blocks.len() < 2000);
    }
    // At the limit nothing changes.
    let d = parse(&format!("{} a", ">".repeat(MAX_BLOCK_DEPTH - 1)));
    assert_eq!(block_depth(&d), MAX_BLOCK_DEPTH);
}
