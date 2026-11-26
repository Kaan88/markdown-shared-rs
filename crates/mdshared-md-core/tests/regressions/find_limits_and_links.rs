use super::*;

// Find

#[test]
fn find_turkish_i_forms() {
    assert_eq!(count("İstanbul", "istanbul"), 1, "i finds İ");
    assert_eq!(count("istanbul", "İSTANBUL"), 1, "İ finds i");
    assert_eq!(count("ISPARTA", "ısparta"), 1, "ı finds I");
    assert_eq!(count("ısparta", "ISPARTA"), 1, "I finds ı");
    assert_eq!(count("Install", "install"), 1, "English I/i keeps working");
    assert_eq!(count("install", "INSTALL"), 1);
    assert_eq!(count("sınır", "sinir"), 0, "ı and i are different letters");
    assert_eq!(count("sinir", "sınır"), 0);
    assert_eq!(count("İ", "I"), 0, "İ and I are different letters");
    assert_eq!(count("I", "İ"), 0);
    assert_eq!(count("ŞİŞLİ ve şişli", "şişli"), 2);
    assert_eq!(count("IŞIK ışık", "ışık"), 2);
}

#[test]
fn find_ranges_are_valid_after_length_changing_case() {
    let text = "aİb KELVIN\u{212A} ſtraße ΣΑΣ σας";
    let r = find_in_text(text, "i", 100);
    assert!(
        r.hits
            .iter()
            .all(|h| text.is_char_boundary(h.start as usize)
                && text.is_char_boundary(h.end as usize))
    );
    assert_eq!(&text[r.hits[0].usize()], "İ");
    assert_eq!(count(text, "k"), 2);
    assert_eq!(count(text, "s"), 1, "the long s; Greek sigma is not an s");
    assert_eq!(
        count(text, "σας"),
        2,
        "final sigma folds with both other forms"
    );
}

#[test]
fn find_cap_is_ten_thousand() {
    let text = "ab ".repeat(FIND_CAP + 5);
    let r = find_in_text(&text, "AB", FIND_CAP);
    assert_eq!(r.hits.len(), 10_000);
    assert!(r.capped);
    let exact = "ab ".repeat(FIND_CAP);
    assert!(!find_in_text(&exact, "ab", FIND_CAP).capped);
}

#[test]
fn find_stays_inside_blocks_and_skips_drawn_source() {
    let d = parse(
        "foo\n\nbar\n\nwrapped\nline\n\n$$\nneedle\n$$\n\ninline $needle$ math\n\n```mermaid\nneedle\n```\n\n```rust\nneedle\n```\n",
    );
    assert_eq!(find(&d, "foobar").hits.len(), 0);
    assert_eq!(find(&d, "foo bar").hits.len(), 0);
    assert_eq!(
        find(&d, "wrapped line").hits.len(),
        1,
        "a soft break reads as a space"
    );
    assert_eq!(
        find(&d, "needle").hits.len(),
        1,
        "only the code block's copy is visible"
    );
    let over = parse(&format!(
        "```mermaid\nneedle {}\n```\n",
        "x".repeat(MAX_DIAGRAM_BYTES)
    ));
    assert_eq!(
        find(&over, "needle").hits.len(),
        1,
        "oversize source is on screen, so it is searchable"
    );
}

#[test]
fn find_empty_query_finds_nothing() {
    assert_eq!(find_in_text("abc", "", 10).hits.len(), 0);
    assert_eq!(find_in_text("", "abc", 10).hits.len(), 0);
}

// Limits, engine jobs

#[test]
fn oversize_blocks_keep_source_and_reason() {
    let eq = "x".repeat(MAX_EQUATION_BYTES + 1);
    let d = parse(&format!("$$\n{eq}\n$$\n\n${eq}$\n\n```math\n{eq}\n```\n"));
    let reasons = d
        .warnings
        .iter()
        .filter(|w| **w == Warning::Oversize(Oversize::Equation))
        .count();
    assert_eq!(reasons, 3);
    assert_eq!(
        d.warnings[0].to_string(),
        "Equation exceeds the 4 KiB limit"
    );
    assert_eq!(d.text.matches(eq.as_str()).count(), 3);
    assert!(d.engine_jobs().is_empty());
    let small = parse(&format!(
        "```mermaid\n{}\n```\n",
        "x".repeat(MAX_DIAGRAM_BYTES - 1)
    ));
    assert!(matches!(
        small.blocks[0].kind,
        BlockKind::Diagram { oversize: None, .. }
    ));
}

#[test]
fn invalid_utf8_file_loads_with_a_warning() {
    let d = mdshared_md_core::parse_bytes(b"# ok\n\nbad \xFF byte\n");
    assert!(d.text.contains('\u{FFFD}'));
    assert_eq!(d.warnings, [Warning::InvalidUtf8]);
}

#[test]
fn file_limit_is_enforced() {
    let d = mdshared_md_core::parse_bytes(&vec![b'a'; MAX_FILE_BYTES + 1]);
    assert_eq!(d.warnings, [Warning::FileTooLarge]);
    let d = parse(&"a".repeat(MAX_DECODED_BYTES + 1));
    assert_eq!(d.warnings, [Warning::FileTooLarge]);
    assert!(d.blocks.is_empty());
}

#[test]
fn engine_jobs_record_each_kind() {
    let d = parse(
        "```rust\nfn a() {}\n```\n\n```mermaid\ngraph TD\n```\n\n```math\nx\n```\n\n```\nplain\n```\n\n$a$\n\n$$b$$\n",
    );
    let jobs = d.engine_jobs();
    let names: Vec<&str> = jobs
        .iter()
        .map(|j| match j {
            EngineJob::Highlight { .. } => "hl",
            EngineJob::Diagram { .. } => "diagram",
            EngineJob::MathBlock { .. } => "mathblock",
            EngineJob::MathInline { .. } => "mathinline",
        })
        .collect();
    assert_eq!(
        names,
        ["hl", "diagram", "mathblock", "mathinline", "mathblock"]
    );
    if let EngineJob::Highlight { lang, source, .. } = jobs[0] {
        assert_eq!(d.str(lang), "rust");
        assert_eq!(d.text_of(source), "fn a() {}\n");
    }
}

// Encoding

#[test]
fn bom_and_invalid_utf8_do_not_panic() {
    let d = mdshared_md_core::parse_bytes(b"\xEF\xBB\xBF# Title\n");
    assert_eq!(d.str(d.headings[0].title), "Title");
    assert!(d.warnings.is_empty());
    let d = mdshared_md_core::parse_bytes(b"# Ti\xFFtle \xC3\n\nok\xF0\x9F\n");
    assert_eq!(d.str(d.headings[0].title), "Ti\u{FFFD}tle \u{FFFD}");
    assert_eq!(d.warnings, [Warning::InvalidUtf8]);
    let d = parse("\u{FEFF}# Title\n");
    assert_eq!(d.headings.len(), 1);
    assert!(parse("").blocks.is_empty());
    assert!(mdshared_md_core::parse_bytes(b"").blocks.is_empty());
    let d = parse("a\0b\n");
    assert!(d.text.contains('\u{FFFD}'));
}

#[test]
fn utf16_files_decode() {
    let mut le = vec![0xFF, 0xFE];
    le.extend("# Başlık\n".encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(mdshared_md_core::parse_bytes(&le).headings.len(), 1);
    let mut be = vec![0xFE, 0xFF];
    be.extend("# Başlık\n".encode_utf16().flat_map(u16::to_be_bytes));
    let d = mdshared_md_core::parse_bytes(&be);
    assert_eq!(d.str(d.headings[0].title), "Başlık");
    assert_eq!(
        mdshared_md_core::parse_bytes(&[0xFF, 0xFE, 0x41]).warnings,
        [Warning::InvalidUtf8]
    );
}

#[test]
fn link_destinations_with_delimiters_still_parse() {
    let d = parse(
        "[x](https://a.b/c_d_e) [y](<https://a.b/f*g>) [z][r] ![i](https://a.b/h_i_j.png \"t_u_v\") (https://k.l/m_n_o)\n\n[r]: https://a.b/p_q_r \"T_i_t\"\n",
    );
    let dests: Vec<&str> = d.links.iter().map(|l| d.str(l.dest)).collect();
    assert_eq!(
        dests,
        [
            "https://a.b/c_d_e",
            "https://a.b/f*g",
            "https://a.b/p_q_r",
            "https://k.l/m_n_o"
        ]
    );
    assert_eq!(d.str(d.links[2].title), "T_i_t");
    assert_eq!(d.str(d.images[0].src), "https://a.b/h_i_j.png");
    assert_eq!(d.str(d.images[0].title), "t_u_v");
    assert_eq!(d.text.trim(), "x y z  (https://k.l/m_n_o)");
}

#[test]
fn private_use_characters_in_a_document_are_preserved() {
    let d = parse("a\u{E000}b http://x.y/c_d\n");
    assert!(d.text.contains('\u{E000}'));
    assert_eq!(links(&d)[0].0, "http://x.y/c_d");
    let d = parse("a\u{E001} http://x.y/_c_d_ tail\n");
    assert!(d.text.contains('\u{E001}'));
}

// Model details

#[test]
fn alerts_and_footnotes() {
    let d = parse(
        "> [!WARNING]\n> Careful\n\n> [!FOO]\n> plain\n\nClaim[^b] and[^a] again[^b].\n\n[^a]: First.\n[^b]: Second.\n",
    );
    assert_eq!(d.blocks[0].kind, BlockKind::Alert(AlertKind::Warning));
    assert!(!d.block_text(0).contains("[!WARNING]"));
    assert!(d.blocks.iter().any(|b| b.kind == BlockKind::BlockQuote));
    let numbers: Vec<(&str, u32)> = d
        .footnotes
        .iter()
        .map(|f| (d.str(f.label), f.number))
        .collect();
    assert!(numbers.contains(&("b", 1)) && numbers.contains(&("a", 2)));
    assert_eq!(
        d.spans
            .iter()
            .filter(|s| matches!(s.kind, SpanKind::FootnoteRef(_)))
            .count(),
        3
    );
    let a = d.footnotes.iter().find(|f| d.str(f.label) == "a").unwrap();
    assert!(d.block_text(a.block).contains("First."));
}

#[test]
fn task_items_are_marked_tight_and_loose() {
    let d = parse("- [x] tight\n- [ ] also\n\n- plain\n\n1. [X] loose\n\n   more\n");
    let tasks = d
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::ListItem { task: true })
        .count();
    assert_eq!(tasks, 3);
    assert_eq!(
        d.spans
            .iter()
            .filter(|s| matches!(s.kind, SpanKind::Checkbox { .. }))
            .count(),
        3
    );
}

#[test]
fn block_tree_invariants_on_the_showcase() {
    let src = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/md/showcase.md"),
    )
    .unwrap();
    let d = parse(&src);
    common_invariants(&d);
    assert_eq!(d.str(d.headings[0].title), "Native Markdown Reader");
    let dup: Vec<&str> = d
        .headings
        .iter()
        .map(|h| d.str(h.slug))
        .filter(|s| s.starts_with("repeated-heading"))
        .collect();
    assert_eq!(dup, ["repeated-heading", "repeated-heading-1"]);
    assert!(
        d.engine_jobs()
            .iter()
            .filter(|j| matches!(j, EngineJob::Diagram { .. }))
            .count()
            >= 8
    );
}

fn common_invariants(d: &Document) {
    for (i, b) in d.blocks.iter().enumerate() {
        assert!(b.end as usize > i && b.end as usize <= d.blocks.len());
        assert!(
            d.text.is_char_boundary(b.text.start as usize)
                && d.text.is_char_boundary(b.text.end as usize)
        );
    }
}

#[test]
fn details_keys_survive_edits_elsewhere() {
    let a = parse(
        "<details><summary>One</summary>x</details>\n\n<details><summary>Two</summary>y</details>\n",
    );
    let b = parse(
        "<details><summary>New</summary>z</details>\n\n<details><summary>One</summary>x</details>\n\n<details><summary>Two</summary>y</details>\n",
    );
    let keys = |d: &Document| d.details_keys().iter().map(|&(_, k)| k).collect::<Vec<_>>();
    assert_eq!(keys(&a)[0], keys(&b)[1]);
    assert_eq!(keys(&a)[1], keys(&b)[2]);
    let same = parse(
        "<details><summary>S</summary>1</details>\n\n<details><summary>S</summary>2</details>\n",
    );
    let k = keys(&same);
    assert_ne!(k[0], k[1]);
}

// Link classification

#[test]
fn link_rules_match_the_old_app() {
    let dir = Path::new("/docs/sub");
    assert_eq!(
        classify_link("#second-page", dir),
        LinkAction::Fragment("second-page".into())
    );
    assert_eq!(classify_link("#", dir), LinkAction::Fragment(String::new()));
    assert_eq!(
        classify_link("linked.md#second-page", dir),
        LinkAction::OpenMarkdown {
            path: "/docs/sub/linked.md".into(),
            fragment: Some("second-page".into())
        }
    );
    for ext in ["a.MD", "a.markdown", "a.mdown", "a.MKD"] {
        assert!(
            matches!(classify_link(ext, dir), LinkAction::OpenMarkdown { .. }),
            "{ext}"
        );
    }
    assert_eq!(
        classify_link("../x.md", dir),
        LinkAction::OpenMarkdown {
            path: "/docs/x.md".into(),
            fragment: None
        }
    );
    assert_eq!(
        classify_link("my%20file.md?raw=1", dir),
        LinkAction::OpenMarkdown {
            path: "/docs/sub/my file.md".into(),
            fragment: None
        }
    );
    assert_eq!(
        classify_link("a.pdf", dir),
        LinkAction::OtherLocalFile("/docs/sub/a.pdf".into())
    );
    assert_eq!(
        classify_link("/abs/b.md", dir),
        LinkAction::OpenMarkdown {
            path: "/abs/b.md".into(),
            fragment: None
        }
    );
    assert_eq!(
        classify_link("file:///abs/b%20c.md#h", dir),
        LinkAction::OpenMarkdown {
            path: "/abs/b c.md".into(),
            fragment: Some("h".into())
        }
    );
    assert_eq!(
        classify_link("https://x.y/a.md", dir),
        LinkAction::External("https://x.y/a.md".into())
    );
    assert_eq!(
        classify_link("HTTP://x.y", dir),
        LinkAction::External("HTTP://x.y".into())
    );
    assert_eq!(
        classify_link("mailto:a@b.c", dir),
        LinkAction::External("mailto:a@b.c".into())
    );
    assert_eq!(classify_link("ftp://x.y", dir), LinkAction::Ignored);
    assert_eq!(classify_link("details:0", dir), LinkAction::Ignored);
    assert_eq!(
        classify_link("file://remote/share/a.md", dir),
        LinkAction::Ignored
    );
}

#[test]
fn image_sources_classify() {
    let dir = Path::new("/docs");
    assert_eq!(
        classify_image("assets/badge.svg", dir),
        ImageSource::Local("/docs/assets/badge.svg".into())
    );
    assert_eq!(
        classify_image("https://x.y/a.png", dir),
        ImageSource::Remote("https://x.y/a.png".into())
    );
    assert_eq!(
        classify_image("data:image/png;base64,AAAA", dir),
        ImageSource::InlineData
    );
    assert_eq!(
        classify_image("data:image/svg+xml,<svg>", dir),
        ImageSource::Unsupported
    );
    assert_eq!(
        classify_image("ftp://x/y.png", dir),
        ImageSource::Unsupported
    );
}

// Review findings: math

#[test]
fn math_needs_clear_neighbours_as_on_github() {
    let d = parse(
        "cp $SRC/$FILE $DEST and $HOME/$USER then ${A}$B; price $5/$10, $1M-$2M, 5$-6$ and $d$e",
    );
    assert!(math_spans(&d).is_empty(), "{:?}", math_spans(&d));
    assert_eq!(
        d.block_text(0).trim_end(),
        "cp $SRC/$FILE $DEST and $HOME/$USER then ${A}$B; price $5/$10, $1M-$2M, 5$-6$ and $d$e"
    );

    let d = parse("echo $FOO$BAR x. $a$b. a$b$.");
    assert!(math_spans(&d).is_empty(), "{:?}", math_spans(&d));
    assert_eq!(d.block_text(0).trim_end(), "echo $FOO$BAR x. $a$b. a$b$.");

    // Every expected value here is the output of `gh api markdown -f mode=gfm -f context=cli/cli`.
    for (src, math) in [
        ("A1 $SRC/$FILE and $x$ end", vec!["x"]),
        ("A2 $x$y$ z", vec![]),
        ("A3 a$b$ c$d$ e", vec![]),
        ("A4 $a$b$c$", vec![]),
        ("A5 x$ $y$ w", vec!["y"]),
        ("A6 $a$_b$ and $c$é d", vec!["c"]),
        ("A7 é$a$ and _$b$_ and $c$_ x", vec![]),
        ("A8 $5 and $6$ w", vec!["6"]),
        (
            "A9 $p$-$q$ and $p$/$q$, ($r$)/($s$)",
            vec!["p", "p", "r", "s"],
        ),
        ("A10 a$$x$$b and $$y$$c", vec![]),
        ("A11 price$5 and $6 and $7", vec![]),
        ("A13 $foo$bar baz$", vec![]),
        ("A14 a$b $c$", vec!["c"]),
        ("S2 **$x$**", vec!["x"]),
        ("S4 ~~$x$~~", vec!["x"]),
        ("S5 [$x$](u)", vec![]),
        ("S6 ($x$) and [$y$] and {$z$}", vec!["x"]),
        ("S8 $p$ - $q$", vec!["p", "q"]),
        ("S11 $a$$b$", vec!["a"]),
        ("S12 `c`$x$", vec!["x"]),
        ("S13 $x$`c`", vec!["x"]),
        ("S14 <b>$x$</b>", vec![]),
        ("T5 **a**$b$", vec!["b"]),
        ("T7 \\($x$\\)", vec!["x"]),
        ("T8 \"$x$\"", vec![]),
        ("T9 ***$x$***", vec![]),
        ("T10 _a_$x$", vec!["x"]),
        ("T11 <b>a</b> $x$ <br>$y$", vec!["x", "y"]),
        ("T12 （$x$） 　$y$", vec![]),
        ("T14 a\u{a0}$x$", vec![]),
        ("T16 $a$\u{a0}b", vec!["a"]),
        ("T17 $$x$$ y", vec!["x"]),
        ("T19 ($$x$$) and **$$y$$**", vec!["x", "y"]),
        ("|h|\n|-|\n|$a$|", vec!["a"]),
        (">$z$", vec!["z"]),
        ("- x\n  $y$", vec!["y"]),
    ] {
        assert_eq!(math_spans(&parse(src)), math, "{src}");
    }
}

#[test]
fn rejected_math_leaves_markdown_between_its_delimiters_alone() {
    let d = parse("$x$ y $y$. Also \\$a$ b$. **$a$** ($b$) a $c$-d $d$e");
    assert_eq!(math_spans(&d), ["x", "y", "a", "b", "c"]);
    let strong = d.spans.iter().find(|s| s.kind == SpanKind::Strong).unwrap();
    assert_eq!(span_text(&d, strong), "a");
    assert!(d.block_text(0).contains("$d$e"), "{:?}", d.block_text(0));

    let d = parse("cost $5 *and* a$b **bold** c$ more");
    assert!(math_spans(&d).is_empty());
    assert!(d.spans.iter().any(|s| s.kind == SpanKind::Emphasis));
    assert!(d.spans.iter().any(|s| s.kind == SpanKind::Strong));
    assert_eq!(d.block_text(0).trim_end(), "cost $5 and a$b bold c$ more");
}

#[test]
fn hidden_dollars_come_back_everywhere() {
    let d = parse(
        "a$b$c [l$m$n](u$v$w \"t$i$e\") ![p$q$r](s$t$u) `x$y$z` <kbd>f$g$h</kbd>\n\n# k$l$m\n",
    );
    let text = &d.text;
    for part in ["a$b$c", "l$m$n", "x$y$z", "f$g$h", "k$l$m"] {
        assert!(text.contains(part), "{text:?}");
    }
    assert!(d.strings.contains("u$v$w") && d.strings.contains("t$i$e"));
    assert!(d.strings.contains("p$q$r") && d.strings.contains("s$t$u"));
    assert!(!d.text.contains('\u{E003}') && !d.strings.contains('\u{E003}'));

    // A file already using the stand-in cannot hide anything, and shows bad math literally instead.
    let d = parse("\u{E003} and $a$b here");
    assert!(math_spans(&d).is_empty());
    assert_eq!(d.block_text(0).trim_end(), "\u{E003} and $a$b here");
}

#[test]
fn a_long_chain_of_rejected_math_settles_quickly() {
    let src = "$a/$b ".repeat(40_000);
    let t = Instant::now();
    let d = parse(&src);
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    assert!(math_spans(&d).is_empty());
    assert!(d.text.contains("$a/$b"));
}
