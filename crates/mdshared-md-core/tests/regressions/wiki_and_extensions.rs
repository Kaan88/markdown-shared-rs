use super::*;

// Wiki-links

/// Parses with every `[[note]]` answered the same way.
fn parse_notes(src: &str, all: WikiAs) -> Document {
    mdshared_md_core::parse_wiki(src, &mut |_| all)
}

fn wiki_links(d: &Document) -> Vec<(String, String)> {
    d.spans
        .iter()
        .filter_map(|s| match s.kind {
            SpanKind::Link(l) if d.links[l as usize].wiki => {
                Some((span_text(d, s), d.str(d.links[l as usize].dest).to_string()))
            }
            _ => None,
        })
        .collect()
}

fn pair(a: &str, b: &str) -> (String, String) {
    (a.to_string(), b.to_string())
}

#[test]
fn wiki_links_parse_in_every_form() {
    let d = parse_notes(
        "[[Note]] [[Note|shown text]] [[Note#Heading]] [[Note#Heading|text]] [[#Heading]] [[folder/Note]] [[ Spaced |x]]",
        WikiAs::Link,
    );
    assert_eq!(
        wiki_links(&d),
        [
            pair("Note", "Note"),
            pair("shown text", "Note"),
            pair("Note > Heading", "Note#Heading"),
            pair("text", "Note#Heading"),
            pair("Heading", "#Heading"),
            pair("folder/Note", "folder/Note"),
            pair("x", "Spaced"),
        ]
    );
    assert_eq!(
        d.text,
        "Note shown text Note > Heading text Heading folder/Note x\n"
    );
    assert!(d.links.iter().all(|l| l.wiki && !l.missing));
    assert!(d.wiki_text.is_empty());
    // An empty label falls back to the note's name.
    let d = parse_notes("[[Other|]]", WikiAs::Link);
    assert_eq!(wiki_links(&d), [pair("Other", "Other")]);
}

/// The kinds of the spans in a document with the text each covers, in order.
fn span_kinds(d: &Document) -> Vec<(String, String)> {
    d.spans
        .iter()
        .map(|s| (format!("{:?}", s.kind), span_text(d, s)))
        .collect()
}

#[test]
fn a_wiki_label_may_hold_inline_markup() {
    let d = parse_notes("x [[Note|a *b* c]] y", WikiAs::Link);
    assert_eq!(d.text.trim_end(), "x a b c y");
    assert_eq!(wiki_links(&d), [pair("a b c", "Note")]);
    let kinds = span_kinds(&d);
    assert_eq!(kinds.len(), 2, "{kinds:?}");
    assert_eq!(kinds[1], ("Emphasis".to_string(), "b".to_string()));
    // The emphasis is the link's child, and the link ends before ` y`.
    assert_eq!(d.spans[1].parent, 0);
    assert!(d.links[0].wiki && !d.links[0].missing);

    // Strong, code, a heading, a missing note, the markup first or last, two in a line.
    let d = parse_notes(
        "[[N#H|**x** and `y`]] then [[M|*z*]] [[K|q *r*]]",
        WikiAs::Missing,
    );
    assert_eq!(d.text.trim_end(), "x and y then z q r");
    assert_eq!(
        wiki_links(&d),
        [pair("x and y", "N#H"), pair("z", "M"), pair("q r", "K")]
    );
    assert!(d.links.iter().all(|l| l.missing));

    // The text that stays text is the same as for a label with no markup.
    let d = parse_notes("[[Gone|a *b*]]", WikiAs::Text);
    assert_eq!(d.text.trim_end(), "[[Gone|a b]]");
    assert!(d.links.is_empty());
    let d = parse("[[Note|a *b* c]]");
    assert!(d.links.is_empty());

    // A heading's anchor comes from the label shown.
    let d = parse_notes("# [[Note|The *big* one]]", WikiAs::Link);
    assert_eq!(d.text.trim_end(), "The big one");
}

#[test]
fn a_wiki_label_with_markup_that_never_closes_stays_text() {
    for src in [
        "[[Note|a *b* c",
        "[[Note|a *b* [c]]",
        r"[[Note|a *b*\]]",
        "[[Note|a `b]]` c",
        "[[Note|a *b* <i>c</i>]]",
        "![[Note|a *b* c]]",
    ] {
        let d = parse_notes(src, WikiAs::Link);
        assert_eq!(d.links.iter().filter(|l| l.wiki).count(), 0, "{src:?}");
    }
    // Markup that crosses the brackets still leaves a sound document: the link ends where the
    // outer markup does.
    let d = parse_notes("*x [[Note|a *b* c]] d*", WikiAs::Link);
    assert_eq!(wiki_links(&d), [pair("a b c", "Note")]);
    assert_eq!(d.text.trim_end(), "x a b c d");
}

/// Parses with embeds asked about: `pictures` are the files that are there, `notes` the notes.
fn parse_embeds(src: &str, pictures: &[&str], notes: &[&str]) -> Document {
    mdshared_md_core::parse_wiki_embeds(src, &mut |name, embed| {
        if embed && mdshared_md_core::links::is_image_name(name) {
            if pictures.contains(&name) {
                WikiAs::Link
            } else {
                WikiAs::Text
            }
        } else if notes.contains(&name) {
            WikiAs::Link
        } else if embed {
            WikiAs::Missing
        } else {
            WikiAs::Text
        }
    })
}

#[test]
fn an_embed_of_a_picture_that_is_there_shows_the_picture() {
    let d = parse_embeds(
        "a ![[pic.png]] b ![[sub/My Pic.SVG|alt words]] c",
        &["pic.png", "sub/My Pic.SVG"],
        &[],
    );
    assert_eq!(d.text.trim_end(), "a  b  c");
    assert_eq!(d.images.len(), 2);
    assert_eq!(d.str(d.images[0].src), "pic.png");
    assert_eq!(d.str(d.images[0].alt), "");
    assert_eq!(d.str(d.images[1].src), "sub/My Pic.SVG");
    assert_eq!(d.str(d.images[1].alt), "alt words");
    // It is the image `![](pic.png)` makes: the same atom in the same place.
    let plain = parse("a ![](pic.png) b");
    assert_eq!(plain.images.len(), 1);
    assert_eq!(plain.text.trim_end(), "a  b");
    let kinds = |d: &Document| {
        d.spans
            .iter()
            .map(|s| (s.kind, s.start, s.end))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        kinds(&parse_embeds("a ![[pic.png]] b", &["pic.png"], &[])),
        kinds(&plain)
    );
    // A size is not a label, and the picture keeps its own size.
    let d = parse_embeds("![[pic.png|200]] ![[pic.png|200x100]]", &["pic.png"], &[]);
    assert!(
        d.images
            .iter()
            .all(|i| d.str(i.alt).is_empty() && i.width.is_none())
    );
}

#[test]
fn an_embed_of_a_note_is_a_link_to_it() {
    let d = parse_embeds("see ![[Note]] and ![[Note#Part|the part]]", &[], &["Note"]);
    assert_eq!(d.text.trim_end(), "see Note and the part");
    assert_eq!(
        wiki_links(&d),
        [pair("Note", "Note"), pair("the part", "Note#Part")]
    );
    assert!(d.links.iter().all(|l| l.wiki && !l.missing));
    assert!(d.images.is_empty());
    // The same links as without the `!`.
    let plain = parse_embeds("see [[Note]]", &[], &["Note"]);
    assert_eq!(wiki_links(&plain)[0], pair("Note", "Note"));
}

#[test]
fn an_embed_that_is_not_found_stays_text() {
    for src in [
        "![[gone.png]]",
        "![[Gone]]",
        "![[Gone|x]]",
        "![[pic.png#h]]",
        r"\![[pic.png]]",
    ] {
        let d = parse_embeds(src, &["pic.png"], &["Note"]);
        assert!(d.links.is_empty() && d.images.is_empty(), "{src:?}");
        assert_eq!(d.text.trim_end(), src.replace(r"\!", "!"), "{src:?}");
    }
    // What stayed text because the application said so is remembered as an embed.
    let d = mdshared_md_core::parse_wiki_embeds("![[a.png]] [[b]]", &mut |_, _| WikiAs::Text);
    let kept: Vec<&str> = d.wiki_text.iter().map(|&s| d.str(s)).collect();
    assert_eq!(kept, ["!a.png", "b"]);
    // `parse_wiki`, which knows no embeds, leaves them as written.
    let d = parse_notes("![[Note]] ![[pic.png]]", WikiAs::Link);
    assert!(d.links.is_empty() && d.images.is_empty());
    assert_eq!(d.text.trim_end(), "![[Note]] ![[pic.png]]");
    // Nothing is made in code, inside a link or from a `!` that belongs to another run.
    let d = parse_embeds(
        "`![[pic.png]]` [x ![[pic.png]]](u) **a!**[[Note]]",
        &["pic.png"],
        &["Note"],
    );
    assert!(d.images.is_empty());
}

#[test]
fn the_application_decides_what_each_note_name_becomes() {
    let src = "[[Here]] [[Gone|label]] [[Never#h]] [[#Top]] [[Never]]";
    let d = mdshared_md_core::parse_wiki(src, &mut |note| match note {
        "Here" => WikiAs::Link,
        "Gone" => WikiAs::Missing,
        _ => WikiAs::Text,
    });
    assert_eq!(
        d.text.trim_end(),
        "Here label [[Never#h]] [[#Top]] [[Never]]"
    );
    let flags: Vec<(bool, bool)> = d.links.iter().map(|l| (l.wiki, l.missing)).collect();
    assert_eq!(flags, [(true, false), (true, true)]);
    // What stayed text is remembered by note name, so the application can look again.
    let kept: Vec<&str> = d.wiki_text.iter().map(|&s| d.str(s)).collect();
    assert_eq!(kept, ["Never", "", "Never"]);
    // Plain `parse` knows no notes: everything stays as written.
    let d = parse(src);
    assert_eq!(d.text.trim_end(), src);
    assert!(d.links.is_empty());
    assert_eq!(d.wiki_text.len(), 5);
}

#[test]
fn wiki_links_are_not_made_in_code_or_from_broken_brackets() {
    for src in [
        "`[[Note]]`",
        "```\n[[Note]]\n```",
        "    [[Note]]",
        "\\[[Note]]",
        "[\\[Note]]",
        "[[Note\\]]",
        "[[Note]\\]",
        "[[]]",
        "[[|label]]",
        "[[#]]",
        "[[a[b]]",
        "[[Note]",
        "[Note]]",
        "[[No\\\nte]]",
        "![[Note]]",
        "![[pic.png]]",
        "[[https://example.com/a]]",
        "<a href=x>[[Note]]</a>",
        "[see [[Note]]](x.md)",
        "$[[Note]]$",
    ] {
        let d = parse_notes(src, WikiAs::Link);
        assert_eq!(d.links.iter().filter(|l| l.wiki).count(), 0, "{src:?}");
        // Nothing but wiki-links is different from a parse that knows no notes.
        let plain = parse(src);
        assert_eq!((&d.text, &d.spans), (&plain.text, &plain.spans), "{src:?}");
    }
    // A third bracket outside is just a bracket.
    let d = parse_notes("[[[Note]]] and [[a]]b]]", WikiAs::Link);
    assert_eq!(d.text.trim_end(), "[Note] and ab]]");
    assert_eq!(wiki_links(&d), [pair("Note", "Note"), pair("a", "a")]);
    // A soft line break inside the brackets is a space in the name.
    assert_eq!(
        wiki_links(&parse_notes("[[Long\nname]]", WikiAs::Link)),
        [pair("Long name", "Long name")]
    );
}

#[test]
fn wiki_links_in_a_table_need_the_bar_escaped() {
    let d = parse_notes(
        "| a | b |\n|---|---|\n| [[Note\\|shown]] | [[Other]] |\n| [[Note|split]] | x |\n",
        WikiAs::Link,
    );
    assert_eq!(
        wiki_links(&d),
        [pair("shown", "Note"), pair("Other", "Other")]
    );
    // An unescaped bar ends the cell, as it does on GitHub: both halves stay text.
    assert!(d.text.contains("[[Note") && d.text.contains("split]]"));
    // Outside a table the escaped bar means the same.
    assert_eq!(
        wiki_links(&parse_notes("[[Note\\|shown]]", WikiAs::Link)),
        [pair("shown", "Note")]
    );
}

/// The document a review of the first wiki-link pass was rendered from: ordinary Markdown that
/// happens to hold double brackets. Whatever the application answers, CommonMark's own meaning
/// of a line is kept.
const ORDINARY: &str = "\
Citation style: see [[1]](https://example.com/one) and [[2]][two].

Array type `int[[3]]` in code, and plain [[not a note]] text. C++ attr: [[nodiscard]] int f();

A ref link [x] and [[x]]. Collapsed [[2]][] and [[x]][].

[two]: https://example.com/two
[x]: https://example.com/x
[[2]]: https://example.com/bracketed
";

#[test]
fn double_brackets_with_a_commonmark_meaning_keep_it() {
    let plain = parse(ORDINARY);
    assert_eq!(
        links(&plain),
        [
            pair("[1]", "https://example.com/one"),
            pair("[2]", "https://example.com/two"),
            pair("x", "https://example.com/x"),
            pair("x", "https://example.com/x"),
            pair("x", "https://example.com/x"),
            // Not a definition (a label holds no bracket), so its address is a bare URL.
            pair(
                "https://example.com/bracketed",
                "https://example.com/bracketed"
            ),
        ]
    );
    assert_eq!(
        plain.text,
        "Citation style: see [1] and [2].\n\
         Array type int[[3]] in code, and plain [[not a note]] text. C++ attr: [[nodiscard]] int f();\n\
         A ref link x and [x]. Collapsed [[2]][] and [x][].\n\
         [[2]]: https://example.com/bracketed\n"
    );
    // With notes, as with a folder open: the two that mean nothing to CommonMark become
    // links to missing notes, and every other line is exactly as it was.
    for all in [WikiAs::Missing, WikiAs::Link] {
        let d = parse_notes(ORDINARY, all);
        assert_eq!(
            wiki_links(&d),
            [
                pair("not a note", "not a note"),
                pair("nodiscard", "nodiscard"),
                pair("2", "2"),
                pair("2", "2"),
            ],
            "{all:?}"
        );
        let other: Vec<(String, String)> = links(&d)
            .into_iter()
            .filter(|l| !l.1.starts_with("not") && l.1 != "nodiscard" && l.1 != "2")
            .collect();
        assert_eq!(other, links(&plain), "{all:?}");
        assert!(d.text.contains("int[[3]] in code"));
        assert!(d.text.starts_with("Citation style: see [1] and [2].\n"));
        assert!(d.text.contains("A ref link x and [x]."));
        assert_eq!(
            d.links.iter().filter(|l| l.missing).count(),
            if all == WikiAs::Missing { 4 } else { 0 }
        );
    }
    // Without notes, as with no folder open, the parse is the plain one.
    let d = parse_notes(ORDINARY, WikiAs::Text);
    assert_eq!((&d.text, &d.spans), (&plain.text, &plain.spans));
}

#[test]
fn wiki_links_keep_other_syntax_working() {
    // A heading's anchor comes from the text shown.
    let d = parse_notes("# See [[Note|the note]]\n", WikiAs::Link);
    assert_eq!(d.str(d.headings[0].slug), "see-the-note");
    assert_eq!(d.str(d.headings[0].title), "See the note");
    // Left as text, the heading is GitHub's.
    let d = parse("# See [[Note|the note]]\n");
    assert_eq!(d.str(d.headings[0].slug), "see-notethe-note");
    // Bare URLs around a wiki-link are still linked, at their new places.
    let d = parse_notes(
        "www.example.com then [[Note|n]] then https://example.org/x and `code`",
        WikiAs::Link,
    );
    assert_eq!(
        links(&d),
        [
            pair("www.example.com", "http://www.example.com"),
            pair("n", "Note"),
            pair("https://example.org/x", "https://example.org/x"),
        ]
    );
    assert_eq!(
        d.text.trim_end(),
        "www.example.com then n then https://example.org/x and code"
    );
    // Inside emphasis the link nests in it.
    let d = parse_notes("*see [[Note]] here*", WikiAs::Missing);
    assert_eq!(spans_of(&d, SpanKind::Emphasis), ["see Note here"]);
    assert_eq!(wiki_links(&d), [pair("Note", "Note")]);
}

#[test]
fn a_hundred_thousand_wiki_openers_parse_quickly() {
    for unit in [
        "[[", "[[a]]", "[[a|", "![[", "[[a|b]] ", "[[#", "\\[[[a]]", "[[a]](", "[[a]][",
    ] {
        let src = unit.repeat(100_000);
        let started = Instant::now();
        let d = parse_notes(&src, WikiAs::Missing);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{unit:?}: {:?}",
            started.elapsed()
        );
        assert!(d.links.len() <= 100_000);
        check_text_ranges(&d);
    }
}

/// Every span lies inside the text, on character boundaries, in order of its start.
fn check_text_ranges(d: &Document) {
    let mut last = 0;
    for s in &d.spans {
        assert!(s.start <= s.end && s.end as usize <= d.text.len());
        assert!(
            d.text.is_char_boundary(s.start as usize) && d.text.is_char_boundary(s.end as usize)
        );
        assert!(s.start >= last);
        last = s.start;
    }
}

/// The text of each top-level block and which clear it carries, as `text:Clear`.
fn clears(d: &Document) -> Vec<String> {
    d.children(mdshared_md_core::model::NONE)
        .map(|b| {
            let text = d.text[d.blocks[b as usize].text.usize()].trim().to_string();
            match d.clear_of(b) {
                Some(c) => format!("{text}:{c:?}"),
                None => text,
            }
        })
        .collect()
}

#[test]
fn a_br_with_clear_starts_the_next_block_below_the_floats() {
    for (tag, want) in [
        (r#"<br clear="all">"#, "Both"),
        (r#"<br clear=all>"#, "Both"),
        (r#"<br clear='both' />"#, "Both"),
        (r#"<BR CLEAR="Left"/>"#, "Left"),
        (r#"<br clear="right">"#, "Right"),
    ] {
        // Alone, and ending a paragraph: the next paragraph carries it.
        let d = parse(&format!("a\n\n{tag}\n\nb\n"));
        assert_eq!(clears(&d), ["a".to_string(), format!("b:{want}")], "{tag}");
        let d = parse(&format!("a {tag}\n\nb\n"));
        assert_eq!(clears(&d), ["a".to_string(), format!("b:{want}")], "{tag}");
    }
    // In the middle of a paragraph the rest of it becomes the block that clears.
    let d = parse("one <br clear=\"all\"> two\n\nthree\n");
    assert_eq!(clears(&d), ["one", "two:Both", "three"]);
    // After an image in the same paragraph, as READMEs write it.
    let d =
        parse("<img src=\"a.png\" align=\"left\" width=\"40\"> text\n<br clear=\"left\">\nafter\n");
    assert_eq!(clears(&d), ["text", "after:Left"]);
    assert_eq!(d.images.len(), 1);
}

#[test]
fn a_br_without_a_usable_clear_is_the_break_it_was() {
    for tag in [
        "<br>",
        "<br clear>",
        "<br clear=\"\">",
        "<br clear=\"none\">",
        "<br clear=\"sideways\">",
    ] {
        let d = parse(&format!("a {tag} b\n"));
        assert!(d.clears.is_empty(), "{tag}");
        assert_eq!(d.text.trim_end(), "a \n b", "{tag}");
    }
    // Where the block cannot be ended the break stays a break, with nothing cleared.
    let d = parse("# head <br clear=\"all\"> more\n\nnext\n");
    assert!(d.clears.is_empty());
    assert!(d.text.contains('\n'));
    // A clear with nothing after it, or in code, asks for nothing.
    assert!(parse("a\n\n<br clear=\"all\">\n").clears.is_empty());
    assert!(parse("`<br clear=\"all\">`\n\nb\n").clears.is_empty());
}

#[test]
fn clears_stay_in_order_and_inside_the_document() {
    let d = parse(
        "a <br clear=\"left\"> b\n\n- c <br clear=\"all\"> d\n\n> q\n> <br clear=\"right\">\n> r\n\n| t |\n|---|\n| <br clear=\"all\"> x |\n\n<br clear=\"all\">\n\n```\ncode\n```\n",
    );
    for pair in d.clears.windows(2) {
        assert!(pair[0].block < pair[1].block);
    }
    assert!(d.clears.iter().all(|c| (c.block as usize) < d.blocks.len()));
}

#[test]
fn abbr_with_a_title_is_a_span_and_one_without_is_plain_text() {
    let d =
        parse("The <abbr title=\"World Health &amp; Co\">WHO</abbr> and <abbr>NASA</abbr> met.");
    let abbrs: Vec<_> = d
        .spans
        .iter()
        .filter_map(|s| match s.kind {
            SpanKind::Abbr(title) => Some((span_text(&d, s), d.str(title).to_string())),
            _ => None,
        })
        .collect();
    assert_eq!(
        abbrs,
        vec![("WHO".to_string(), "World Health & Co".to_string())]
    );
    assert_eq!(d.block_text(0), "The WHO and NASA met.");
    // An empty title is no title.
    let d = parse("a <abbr title=\"\">X</abbr> b");
    assert!(!d.spans.iter().any(|s| matches!(s.kind, SpanKind::Abbr(_))));
}

#[test]
fn time_and_bdo_keep_their_text() {
    let d = parse("on <time datetime=\"2026-11-15\">15 Nov</time> by <bdo dir=\"rtl\">hand</bdo>.");
    assert_eq!(d.block_text(0), "on 15 Nov by hand.");
}

#[test]
fn a_block_id_names_the_paragraph_item_or_block_before_it() {
    let d = parse(
        "First paragraph. ^first\n\nSecond one\nspans two lines ^two-lines\n\n- an item ^item\n- plain\n\n\
         > a quote\n\n^quote\n\nNot an id: a^b and ^trailing-not-at-end here\n\n# Head ^head\n",
    );
    let text_of = |id: &str| {
        d.resolve_block_id(id)
            .map(|b| d.block_text(b).trim().to_string())
    };
    assert_eq!(text_of("first").as_deref(), Some("First paragraph. ^first"));
    assert_eq!(
        text_of("two-lines").as_deref(),
        Some("Second one spans two lines ^two-lines")
    );
    assert_eq!(text_of("item").as_deref(), Some("an item ^item"));
    // An id on a line of its own names the quote before it.
    assert_eq!(text_of("quote").as_deref(), Some("a quote"));
    assert_eq!(text_of("head").as_deref(), Some("Head ^head"));
    assert_eq!(text_of("trailing-not-at-end"), None);
    assert_eq!(text_of("b"), None);
    assert_eq!(text_of("missing"), None);
    assert_eq!(text_of(""), None);
    // A fragment `^id` resolves the same way, after the headings.
    assert_eq!(d.resolve_fragment("^first"), d.resolve_block_id("first"));
    assert_eq!(d.resolve_fragment("^nope"), None);
}
