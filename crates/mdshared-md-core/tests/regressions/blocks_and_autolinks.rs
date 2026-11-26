use super::*;

// Not-ported bugs

#[test]
fn reload_keeps_exact_position_instead_of_snapping_to_heading() {
    let old = parse("# Top\n\nfirst paragraph\n\nsecond paragraph\n\nthird paragraph\n");
    let at_third = old
        .blocks
        .iter()
        .position(|b| old.text_of(b.text).starts_with("third"))
        .unwrap() as u32;
    let pos = ScrollPos {
        block: at_third,
        offset: 37.5,
    };
    let same = parse("# Top\n\nfirst paragraph\n\nsecond paragraph\n\nthird paragraph\n");
    assert_eq!(same.remap_scroll(&old, pos), pos);

    let edited =
        parse("# Top\n\nnew intro\n\nfirst paragraph\n\nsecond paragraph\n\nthird paragraph\n");
    let moved = edited.remap_scroll(&old, pos);
    assert!(edited.block_text(moved.block).starts_with("third"));
    assert_eq!(moved.offset, 37.5);
    assert!(!matches!(
        edited.blocks[moved.block as usize].kind,
        BlockKind::Heading { .. }
    ));
}

// Autolink

#[test]
fn urls_do_not_truncate_at_delimiters() {
    for (src, text) in [
        ("http://a.b/_c_", "http://a.b/_c"),
        ("https://example.com/*x*", "https://example.com/*x"),
        ("https://x.y/~a~ z", "https://x.y/~a"),
        (
            "see https://x.y/a_b_c/d*e*f and more",
            "https://x.y/a_b_c/d*e*f",
        ),
        ("www.x.y/_a_b_", "www.x.y/_a_b"),
    ] {
        let d = parse(src);
        let got = links(&d);
        assert_eq!(got.len(), 1, "{src}");
        assert_eq!(got[0].0, text, "{src}");
        assert!(
            !d.spans.iter().any(|s| matches!(
                s.kind,
                SpanKind::Emphasis | SpanKind::Strike | SpanKind::Strong
            )),
            "{src}"
        );
        assert!(
            d.text
                .contains(src.split_whitespace().next().unwrap_or(src))
                || src.starts_with("see"),
            "{src}"
        );
    }
}

#[test]
fn real_emphasis_next_to_urls_survives() {
    let d = parse("*http://a.b/c* and [x](http://d.e/_f_)*g* **www.h.i**");
    let kinds: Vec<_> = d.spans.iter().map(|s| s.kind).collect();
    assert!(kinds.contains(&SpanKind::Emphasis) && kinds.contains(&SpanKind::Strong));
    let l = links(&d);
    assert_eq!(
        l[0],
        ("http://a.b/c".to_string(), "http://a.b/c".to_string())
    );
    assert_eq!(l[1].1, "http://d.e/_f_", "destinations come back unchanged");
    assert_eq!(l[2].1, "http://www.h.i");
}

#[test]
fn delimiters_inside_code_and_raw_html_are_untouched() {
    let d =
        parse("`http://a.b/_c_` <a href=\"http://d.e/*f*\">x</a>\n\n```\nhttp://g.h/~i~j\n```\n");
    assert!(d.text.contains("http://a.b/_c_"));
    assert!(d.text.contains("http://g.h/~i~j"));
    assert_eq!(links(&d), [("x".to_string(), "http://d.e/*f*".to_string())]);
}

#[test]
fn escaped_character_blocks_a_link() {
    assert!(links(&parse("<foo\\+@bar.example.com>")).is_empty());
    assert_eq!(
        links(&parse("http://a.b/\\_c")),
        [("http://a.b/".to_string(), "http://a.b/".to_string())]
    );
    // An escaped backslash is not an escape of what follows.
    assert_eq!(links(&parse("x\\\\foo@bar.example.com")).len(), 1);
}

#[test]
fn bare_links_are_kept_everywhere_github_keeps_them() {
    let l = links(&parse(
        "Go to https://example.com, www.example.org. Mail a.b+c@example.co.uk now.",
    ));
    assert_eq!(l[0].1, "https://example.com");
    assert_eq!(l[1].1, "http://www.example.org");
    assert_eq!(l[2].1, "mailto:a.b+c@example.co.uk");
    assert_eq!(
        links(&parse("mailto:foo@bar.baz")),
        [(
            "mailto:foo@bar.baz".to_string(),
            "mailto:foo@bar.baz".to_string()
        )]
    );
    assert_eq!(
        links(&parse("xmpp:foo@bar.baz/txt.")),
        [(
            "xmpp:foo@bar.baz/txt".to_string(),
            "xmpp:foo@bar.baz/txt".to_string()
        )]
    );
    assert!(
        links(&parse("[http://c.d](x)"))
            .iter()
            .all(|(_, dest)| dest == "x")
    );
    assert!(links(&parse("`http://a.b`")).is_empty());
    assert!(
        links(&parse("<div>http://a.b</div>")).is_empty(),
        "GitHub does not autolink inside HTML blocks"
    );
}

#[test]
fn turkish_paths_and_unicode_domains_link() {
    let l = links(&parse("https://örnek.com.tr/yol/ığüşöç_İ?a=ş"));
    assert_eq!(l[0].0, "https://örnek.com.tr/yol/ığüşöç_İ?a=ş");
}

// Nested strong

#[test]
fn nested_strong_collapses_in_the_model() {
    for src in [
        "****foo****",
        "**foo **bar** baz**",
        "__foo __bar__ baz__",
        "<b>a <strong>b</strong></b>",
        "**a <b>c</b>**",
    ] {
        let d = parse(src);
        assert_eq!(
            d.spans
                .iter()
                .filter(|s| s.kind == SpanKind::Strong)
                .count(),
            1,
            "{src}"
        );
    }
}

// HTML allow-list

#[test]
fn nothing_executable_survives() {
    let d = parse(concat!(
        "<a href=\"javascript:alert(1)\">x</a> <a href=\" java\nscript:alert(1)\">y</a> <a href=\"JaVaScRiPt:alert(2)\">z</a> ",
        "<a href=\"java&#10;script:alert(3)\">w</a>\n\n<img src=\"javascript:alert(4)\">\n\n<img src=x onerror=alert(5)>\n\n",
        "<script>alert(6)</script>\n\n<iframe src=\"https://evil\"></iframe>\n\n<div onclick=\"alert(7)\">t</div>\n\n",
        "foo <script>alert(8)</script> bar\n\n<style>p{color:red}</style>\n\n<object data=\"x\">alert(9)</object>\n"
    ));
    // The tag filter turns script, style and iframe into text, and object keeps its fallback
    // content, so their words reach the model now. What must not survive is anything executable.
    assert!(d.text.contains("<script>alert(6)</script>"), "{}", d.text);
    assert!(d.text.contains("<iframe"), "{}", d.text);
    assert!(d.text.contains("alert(9)"), "{}", d.text);
    assert!(
        !d.strings.to_lowercase().contains("javascript"),
        "{}",
        d.strings
    );
    assert!(!d.strings.contains("onerror") && !d.strings.contains("onclick"));
    assert!(d.text.contains("foo") && d.text.contains("bar"));
}

#[test]
fn markdown_script_links_never_act() {
    let dir = Path::new("/doc");
    assert_eq!(
        classify_link("javascript:alert(1)", dir),
        LinkAction::Ignored
    );
    assert_eq!(classify_link("data:text/html,x", dir), LinkAction::Ignored);
    assert_eq!(classify_link("vbscript:x", dir), LinkAction::Ignored);
}

#[test]
fn unknown_tags_drop_and_keep_text() {
    let d = parse("<foo-bar baz=1>kept <blink>too</blink></foo-bar>\n");
    assert_eq!(d.text.trim(), "kept too");
    assert!(d.spans.is_empty());
}

#[test]
fn tagfilter_shows_unclosed_filtered_tags_as_text() {
    let d = parse(
        "<strong> <title> <style> <em>\n\n<blockquote>\n  <xmp> is disallowed.  <XMP> is also disallowed.\n</blockquote>\n",
    );
    assert!(d.text.contains("<title>") && d.text.contains("<style>"));
    assert!(
        d.text.contains("<xmp> is disallowed.") && d.text.contains("<XMP> is also disallowed.")
    );
    assert!(d.blocks.iter().any(|b| b.kind == BlockKind::BlockQuote));
}

#[test]
fn allow_listed_inline_tags_map_to_spans() {
    let d = parse(
        "<kbd>K</kbd> <sub>s</sub> <sup>p</sup> <u>u</u> <del>d</del> <s>s2</s> <mark>m</mark> <code>c</code> a<br>b <i>i</i> <b>b</b>\n",
    );
    let has = |k: SpanKind, t: &str| d.spans.iter().any(|s| s.kind == k && span_text(&d, s) == t);
    assert!(has(SpanKind::Kbd, "K") && has(SpanKind::Sub, "s") && has(SpanKind::Sup, "p"));
    assert!(
        has(SpanKind::Underline, "u") && has(SpanKind::Strike, "d") && has(SpanKind::Strike, "s2")
    );
    assert!(
        has(SpanKind::Mark, "m")
            && has(SpanKind::Code, "c")
            && has(SpanKind::Emphasis, "i")
            && has(SpanKind::Strong, "b")
    );
    assert!(d.text.contains("a\nb"));
}

#[test]
fn picture_keeps_theme_variants() {
    let d = parse(
        "<picture>\n <source media=\"(prefers-color-scheme: dark)\" srcset=\"dark.png 2x\">\n <source media=\"(prefers-color-scheme: light)\" srcset=\"light.png\">\n <img alt=\"Logo\" src=\"fallback.png\" width=\"200\" height=\"50%\">\n</picture>\n",
    );
    assert_eq!(d.images.len(), 1);
    let i = d.images[0];
    assert_eq!(
        (
            d.str(i.src),
            d.str(i.dark_src),
            d.str(i.light_src),
            d.str(i.alt)
        ),
        ("fallback.png", "dark.png", "light.png", "Logo")
    );
    assert_eq!(
        (i.width, i.height),
        (Some(Length::Px(200)), Some(Length::Percent(50)))
    );
}

#[test]
fn link_wrapped_in_html_around_markdown_image() {
    let d = parse("<a href=\"https://x.y\">\n\n![badge](b.svg)\n\n</a>\n\nafter\n");
    let para = d
        .blocks
        .iter()
        .position(|b| matches!(b.kind, BlockKind::Paragraph { .. }))
        .unwrap();
    let spans = d.blocks[para].spans;
    assert!(matches!(
        d.spans[spans.start as usize].kind,
        SpanKind::Link(_)
    ));
    assert_eq!(spans.len(), 2);
    assert_eq!(
        d.spans[spans.start as usize + 1].parent,
        spans.start,
        "the image sits inside the link"
    );
    let last = d
        .blocks
        .iter()
        .rposition(|b| matches!(b.kind, BlockKind::Paragraph { .. }))
        .unwrap();
    assert_eq!(
        d.blocks[last].spans.len(),
        0,
        "the closer ended the wrapper"
    );
    assert_eq!(d.blocks.len(), 2, "no empty block is left behind");
}

#[test]
fn centered_div_contains_following_markdown() {
    let d = parse("<div align=\"center\">\n\n# Title\n\n</div>\n\nafter\n");
    assert_eq!(
        d.blocks[0].kind,
        BlockKind::Div {
            align: Align::Center
        }
    );
    assert_eq!(d.blocks[1].parent, 0);
    assert_eq!(d.headings.len(), 1);
    assert_eq!(d.blocks[0].end, 2);
    assert_eq!(d.blocks[2].parent, NONE);
}

#[test]
fn html_tables_and_lists_build_structure() {
    let d = parse(
        "<table><tr><th colspan=2 align=right>h</th></tr><tr><td>a<td>b</table>\n\n<ul><li>x<li>y</ul>\n\n<ol start=3><li>z</ol>\n",
    );
    let cells = d
        .blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::TableCell { .. }))
        .count();
    assert_eq!(cells, 3);
    assert!(d.blocks.iter().any(|b| b.kind
        == BlockKind::TableCell {
            align: Align::Right,
            header: true,
            colspan: 2,
            rowspan: 1
        }));
    assert_eq!(
        d.blocks
            .iter()
            .filter(|b| matches!(b.kind, BlockKind::TableRow { .. }))
            .count(),
        2
    );
    assert_eq!(
        d.blocks
            .iter()
            .filter(|b| matches!(b.kind, BlockKind::ListItem { .. }))
            .count(),
        3
    );
    assert!(
        d.blocks
            .iter()
            .any(|b| b.kind == BlockKind::List { start: Some(3) })
    );
}

#[test]
fn html_headings_join_the_outline() {
    let d =
        parse("<h2 align=\"center\" id=\"custom\">Hello <em>there</em></h2>\n\n## Hello there\n");
    assert_eq!(d.headings.len(), 2);
    assert_eq!(d.str(d.headings[0].slug), "hello-there");
    assert_eq!(d.str(d.headings[1].slug), "hello-there-1");
    assert_eq!(d.resolve_fragment("custom"), Some(0));
    assert_eq!(d.resolve_fragment("user-content-hello-there"), Some(0));
    assert_eq!(
        d.resolve_fragment("HELLO-THERE-1"),
        Some(d.headings[1].block)
    );
}

#[test]
fn anchor_alone_on_a_line_leaves_no_gap_and_targets_the_next_block() {
    let d = parse("intro\n\n<a id=\"x\"></a>\n\n## H\n\n<a name=\"y\">here</a>\n");
    assert_eq!(d.blocks.len(), 3);
    assert!(matches!(
        d.blocks[d.resolve_fragment("x").unwrap() as usize].kind,
        BlockKind::Heading { .. }
    ));
    assert_eq!(
        d.block_text(d.resolve_fragment("y").unwrap()).trim(),
        "here"
    );
    assert_eq!(
        parse("end\n\n<a id=\"z\"></a>\n").resolve_fragment("z"),
        Some(0)
    );
    assert!(parse("<a id=\"only\"></a>\n").anchors.is_empty());
}

#[test]
fn whitespace_collapses_once_outside_code() {
    let d = parse("a  b\tc <!-- x --> d&#10;&#10;e&nbsp; f\n\n`  x  y  `\n\n```\na  b\n```\n");
    assert!(d.text.starts_with("a b c d e\u{a0} f\n"), "{:?}", d.text);
    assert!(
        d.text.contains(" x  y "),
        "code spans keep inner spacing: {:?}",
        d.text
    );
    assert!(d.text.contains("a  b\n"), "code blocks keep their spacing");
}

#[test]
fn spaces_around_images_survive_collapsing() {
    assert_eq!(parse("a ![i](u) b\n").text, "a  b\n");
    assert_eq!(parse("<div><img src=\"u\"> b</div>\n").text, " b\n");
    assert_eq!(parse("- [x] task\n").text, "task\n");
}

#[test]
fn empty_html_containers_leave_nothing_behind() {
    assert!(parse("<p>\n\n<p></p>\n").blocks.is_empty());
    assert!(parse("<p><b>\n").blocks.is_empty());
    let d = parse("<p><b>\n\nbold\n\n</b>\n\nplain\n");
    assert_eq!(d.blocks.len(), 2);
    assert_eq!(
        d.spans
            .iter()
            .filter(|s| s.kind == SpanKind::Strong)
            .count(),
        1
    );
}

// Slugs and outline

#[test]
fn slugs_match_github() {
    let d = parse(
        "# Hello, World!\n\n## `code` and *em*\n\n## C++ & Rust\n\n# Hello, World!\n\n# foo\n\n# foo\n\n# foo-1\n\n## 🚀 Launch\n\n## Türkçe Başlık İ\n\n## snake_case-kept\n",
    );
    let got: Vec<&str> = d.headings.iter().map(|h| d.str(h.slug)).collect();
    assert_eq!(
        &got[..7],
        [
            "hello-world",
            "code-and-em",
            "c--rust",
            "hello-world-1",
            "foo",
            "foo-1",
            "foo-1-1"
        ]
    );
    assert_eq!(got[7], "-launch");
    assert_eq!(got[8], "türkçe-başlık-i\u{307}");
    assert_eq!(got[9], "snake_case-kept");
}

#[test]
fn outline_nesting_follows_levels() {
    let d = parse("# A\n\n### B\n\n## C\n\n# D\n");
    let o = d.outline();
    assert_eq!(
        o.iter().map(|i| i.parent).collect::<Vec<_>>(),
        [NONE, 0, 0, NONE]
    );
    assert_eq!(d.str(d.headings[1].title), "B");
}
