use super::*;

thread_local! {
    static INLINE_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn count_inline_visit() {
    INLINE_VISITS.with(|n| n.set(n.get() + 1));
}

#[test]
fn export_visits_each_flat_span_once() {
    for count in [256, 512] {
        let doc = mdshared_md_core::parse(&"*é* ".repeat(count));
        INLINE_VISITS.set(0);
        let html = export_html(
            &doc,
            &ExportOptions {
                title: "",
                theme: crate::LIGHT,
                base_dir: None,
                remote_images: false,
                print: false,
            },
        );
        assert_eq!(html.matches("<em>é</em>").count(), count);
        assert_eq!(INLINE_VISITS.get(), count);
    }
}
use crate::theme::LIGHT;
use mdshared_md_core::parse;

fn export(md: &str) -> String {
    export_with(md, None, false)
}

fn export_with(md: &str, base: Option<&Path>, remote: bool) -> String {
    // A formula or a diagram goes to the engine, which the engine's own tests count on
    // having to themselves.
    let _engine = crate::engine::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let doc = parse(md);
    export_html(
        &doc,
        &ExportOptions {
            title: "",
            theme: LIGHT,
            base_dir: base,
            remote_images: remote,
            print: false,
        },
    )
}

fn body(html: &str) -> &str {
    let from = html.find("<main>").unwrap() + 7;
    let to = html.rfind("</main>").unwrap();
    &html[from..to]
}

#[test]
fn base64_matches_the_standard_vectors() {
    assert_eq!(base64(b""), "");
    assert_eq!(base64(b"f"), "Zg==");
    assert_eq!(base64(b"fo"), "Zm8=");
    assert_eq!(base64(b"foo"), "Zm9v");
    assert_eq!(base64(b"foob"), "Zm9vYg==");
    assert_eq!(base64(b"fooba"), "Zm9vYmE=");
    assert_eq!(base64(b"foobar"), "Zm9vYmFy");
}

#[test]
fn the_page_is_complete_and_self_contained() {
    let html = export("# Title\n\nHello *there* **you**.\n");
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("<title>Title</title>"));
    assert!(html.contains("<style>"));
    assert!(html.contains("<h1 id=\"title\">Title</h1>"));
    assert!(html.contains("<p>Hello <em>there</em> <strong>you</strong>.</p>"));
    assert!(!html.contains("<script"), "no script outside print");
    assert!(!html.contains("<link"), "no external style sheet");
    assert!(html.trim_end().ends_with("</html>"));
}

#[test]
fn the_title_option_wins_and_is_escaped() {
    let doc = parse("# A\n");
    let html = export_html(
        &doc,
        &ExportOptions {
            title: "a <b> & c",
            theme: LIGHT,
            base_dir: None,
            remote_images: false,
            print: false,
        },
    );
    assert!(html.contains("<title>a &lt;b&gt; &amp; c</title>"));
}

#[test]
fn print_adds_the_dialog_script_and_opens_details() {
    let doc = parse("<details><summary>S</summary>\n\nbody\n\n</details>\n");
    let make = |print| {
        export_html(
            &doc,
            &ExportOptions {
                title: "t",
                theme: LIGHT,
                base_dir: None,
                remote_images: false,
                print,
            },
        )
    };
    let plain = make(false);
    let printed = make(true);
    assert!(!plain.contains("window.print"));
    assert!(!plain.contains("<details open"));
    assert!(printed.contains("window.print"));
    assert!(printed.contains("<details open"));
}

#[test]
fn text_from_the_document_is_escaped() {
    let html = export("a <b>x</b> & \"q\" `<i>`\n");
    let b = body(&html);
    assert!(!b.contains("<script"));
    assert!(b.contains("&amp;"));
    assert!(b.contains("&lt;i&gt;"));
}

#[test]
fn unsafe_links_lose_their_address() {
    let html = export(
        "[a](javascript:alert(1)) [b](https://example.com/x?y=1&z=2) [c](notes/a.md) [d](#top) [e](mailto:x@y.z) [f](data:text/html,hi)\n",
    );
    let b = body(&html);
    assert!(!b.contains("javascript"));
    assert!(!b.contains("data:text"));
    assert!(b.contains("<a href=\"https://example.com/x?y=1&amp;z=2\">b</a>"));
    assert!(b.contains("<a href=\"notes/a.md\">c</a>"));
    assert!(b.contains("<a href=\"#top\">d</a>"));
    assert!(b.contains("<a href=\"mailto:x@y.z\">e</a>"));
    assert!(safe_href("a/b:c"), "a colon after a slash is not a scheme");
}

#[test]
fn lists_tasks_and_tables() {
    let html =
        export("- [x] done\n- [ ] todo\n\n3. three\n4. four\n\n| a | b |\n|:-:|--:|\n| 1 | 2 |\n");
    let b = body(&html);
    assert!(
        b.contains("<li class=\"task\"><input type=\"checkbox\" checked disabled> done</li>"),
        "{b}"
    );
    assert!(b.contains("<input type=\"checkbox\" disabled> todo"));
    assert!(b.contains("<ol start=\"3\">"));
    assert!(b.contains("<th style=\"text-align:center;\">a</th>"), "{b}");
    assert!(b.contains("<td style=\"text-align:right;\">2</td>"));
}

#[test]
fn code_is_highlighted_with_inline_colours_and_escaped() {
    let html = export("```rust\nfn main() { let a = \"<x>\"; }\n```\n\n```\nplain <b>\n```\n");
    let b = body(&html);
    assert!(b.contains("<pre data-lang=\"rust\"><code>"), "{b}");
    assert!(b.contains("<span style=\"color:#"));
    assert!(b.contains("&lt;x&gt;"));
    assert!(b.contains("<pre><code>plain &lt;b&gt;</code></pre>"), "{b}");
}

#[test]
fn vue_and_svelte_code_blocks_are_highlighted() {
    let vue = "```vue\n<template><p :id=\"id\">{{ msg }}</p></template>\n<script setup lang=\"ts\">\nconst msg = 'hi'\n</script>\n```\n";
    let svelte = "```svelte\n<script>\n  let n = 0;\n</script>\n{#if n > 1}<b on:click={() => n++}>{n}</b>{/if}\n```\n";
    for (md, lang) in [(vue, "vue"), (svelte, "svelte")] {
        let html = export(md);
        let b = body(&html);
        assert!(b.contains(&format!("<pre data-lang=\"{lang}\">")), "{b}");
        // More than one colour: markup, script and expressions are told apart.
        let colours: std::collections::HashSet<&str> = b
            .split("color:#")
            .skip(1)
            .filter_map(|s| s.get(..6))
            .collect();
        assert!(colours.len() >= 3, "{lang}: {colours:?}");
    }
}

#[test]
fn a_formula_or_diagram_without_the_engine_keeps_its_source() {
    // Whether or not the engine is installed where the tests run, the source is not lost.
    let html = export("$$\nx^2\n$$\n\n```mermaid\ngraph TD; A-->B\n```\n");
    let b = body(&html);
    assert!(b.contains("x^2"), "{b}");
    assert!(
        b.contains("A--&gt;B") || b.contains("alt=\"graph TD; A--&gt;B\""),
        "{b}"
    );
}

#[test]
fn footnotes_are_listed_at_the_end() {
    let html = export("Text[^1].\n\n[^1]: The note.\n");
    let b = body(&html);
    assert!(
        b.contains("<sup class=\"fnref\"><a href=\"#fn-1\" id=\"fnref-1\">1</a></sup>"),
        "{b}"
    );
    assert!(b.contains("<li id=\"fn-1\">"));
    assert!(b.contains("The note."));
    assert!(b.find("Text").unwrap() < b.find("The note.").unwrap());
}

#[test]
fn alerts_and_quotes() {
    let html = export("> [!WARNING]\n> careful\n\n> quoted\n");
    let b = body(&html);
    assert!(b.contains("class=\"alert alert-warning\""), "{b}");
    assert!(b.contains("<p class=\"alert-title\">Warning</p>"));
    assert!(b.contains("<blockquote>"));
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdview-export-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 1x1 PNG.
const PIXEL: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0,
    1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0x0d, 0x49, 0x44, 0x41, 0x54,
    0x78, 0x9c, 0x63, 0xf8, 0xff, 0xff, 0x3f, 0, 5, 0xfe, 2, 0xfe, 0xa7, 0x35, 0x81, 0x84, 0, 0, 0,
    0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

#[test]
fn a_local_image_is_embedded_and_a_missing_one_is_its_alt_text() {
    let dir = scratch("img");
    std::fs::write(dir.join("p.png"), PIXEL).unwrap();
    std::fs::write(dir.join("notes.txt"), "not an image").unwrap();
    let md = "![pic](p.png \"T\") ![gone](nope.png) ![txt](notes.txt) ![up](../x.png)\n";
    let html = export_with(md, Some(&dir), false);
    let b = body(&html);
    assert!(b.contains("src=\"data:image/png;base64,iVBOR"), "{b}");
    assert!(b.contains("alt=\"pic\" title=\"T\""));
    assert!(b.contains("gone"));
    assert!(!b.contains("nope.png"));
    assert!(!b.contains("notes.txt"));
    // Without a base folder a relative path has nothing to be relative to.
    let html = export_with("![pic](p.png)\n", None, false);
    assert!(!body(&html).contains("<img"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn remote_images_follow_the_readers_choice() {
    let md = "![r](https://example.com/a.png)\n";
    let off = export_with(md, None, false);
    assert!(!body(&off).contains("example.com"));
    assert!(body(&off).contains('r'));
    let on = export_with(md, None, true);
    assert!(body(&on).contains("<img src=\"https://example.com/a.png\""));
}

#[test]
fn a_data_image_is_kept_and_other_data_is_not() {
    let md = "![a](data:image/png;base64,AAAA) ![b](data:text/html;base64,AAAA)\n";
    let html = export(md);
    let b = body(&html);
    assert!(b.contains("data:image/png;base64,AAAA"));
    assert!(!b.contains("text/html"));
}

#[test]
fn file_urls_are_escaped_and_windows_paths_get_a_slash() {
    assert_eq!(
        file_url(Path::new("/a b/c#d.png")),
        "file:///a%20b/c%23d.png"
    );
    assert_eq!(
        file_url(Path::new("C:\\x\\y z.png")),
        "file:///C:/x/y%20z.png"
    );
    #[cfg(windows)]
    assert_eq!(
        file_url(Path::new(r"\\?\C:\x\y z.png")),
        "file:///C:/x/y%20z.png"
    );
}

#[test]
fn the_style_sheet_follows_the_theme() {
    let mut theme = LIGHT.with_fonts("Noto Serif", "");
    theme.reading_width = 1000.0;
    let css = stylesheet(&theme);
    // The default list follows the chosen family: Segoe UI first, or on a Mac the system's.
    let next = if cfg!(target_os = "macos") {
        "system-ui"
    } else {
        "\"Segoe UI\""
    };
    assert!(
        css.contains(&format!("--sans:\"Noto Serif\", {next}")),
        "{}",
        &css[..300]
    );
    assert!(css.contains("sans-serif"));
    assert!(!css.contains("\"sans-serif\""));
    assert!(css.contains("--width:1080px"));
    theme.reading_width = f32::MAX;
    assert!(stylesheet(&theme).contains("--width:none"));
    assert!(css.contains("@media print"));
}

#[test]
fn highlight_runs_merge_and_cover_the_source() {
    let source = "ab cd";
    let tokens = vec![
        highlight::Token {
            range: 0..1,
            color: [1, 2, 3],
        },
        highlight::Token {
            range: 1..2,
            color: [1, 2, 3],
        },
        highlight::Token {
            range: 3..5,
            color: [9, 9, 9],
        },
    ];
    let mut out = String::new();
    highlighted(&mut out, source, &tokens, None);
    assert_eq!(
        out,
        "<span style=\"color:#010203\">ab</span> <span style=\"color:#090909\">cd</span>"
    );
}

#[test]
fn a_hard_break_is_a_br_and_headings_carry_ids() {
    let html = export("line one  \nline two\n\n## Two words\n");
    let b = body(&html);
    assert!(b.contains("line one<br>"), "{b}");
    assert!(b.contains("<h2 id=\"two-words\">Two words</h2>"));
}

#[test]
fn an_empty_document_still_makes_a_page() {
    let html = export("");
    assert!(html.contains("<title>Document</title>"));
    assert!(body(&html).trim().is_empty());
}

#[test]
fn a_link_around_a_picture_alone_keeps_both() {
    let dir = scratch("linked-image");
    std::fs::write(dir.join("p.png"), PIXEL).unwrap();
    let html = export_with(
        "<p align=\"center\"><b>Head</b><br><a href=\"https://example.com\"><img src=\"p.png\" alt=\"badge\" width=\"18\"></a></p>\n\n[![alt](p.png)](https://example.org)\n",
        Some(&dir),
        false,
    );
    let b = body(&html);
    assert!(
        b.contains("<a href=\"https://example.com\"><img src=\"data:image/png;base64,"),
        "{b}"
    );
    assert!(
        b.contains("<a href=\"https://example.org\"><img src=\"data:image/png;base64,"),
        "{b}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_summary_stays_on_the_line_of_its_marker() {
    let html = export("<details>\n<summary>More</summary>\n\nBody.\n\n</details>\n");
    assert!(html.contains("summary>div,summary>p{display:inline}"));
    assert!(body(&html).contains("<summary>"));
}

#[test]
fn an_abbreviation_keeps_its_title_and_a_bare_one_is_plain_text() {
    let html =
        export("The <abbr title=\"A &amp; B\">AB</abbr> and <abbr>CD</abbr> and <time>now</time>.");
    assert!(
        body(&html).contains("<abbr title=\"A &amp; B\">AB</abbr> and CD and now."),
        "{}",
        body(&html)
    );
    assert!(html.contains("abbr[title]{text-decoration:underline dotted}"));
}

#[test]
fn a_table_is_placed_and_its_columns_sized_by_the_html_attributes() {
    let html = export(
        "<table width=\"50%\" align=\"center\"><tr align=\"center\"><td width=\"30\">a</td>\
             <td align=\"right\" valign=\"bottom\">b</td><td width=\"25%\">c</td></tr>\
             <tr valign=\"middle\"><td>d</td></tr></table>\n",
    );
    let b = body(&html);
    assert!(
        b.contains("<table style=\"margin-left:auto;margin-right:auto;width:50%;\">"),
        "{b}"
    );
    // A cell that states no `align` takes its row's; `width` is the column's.
    assert!(
        b.contains("<td style=\"text-align:center;width:30px;\">a</td>"),
        "{b}"
    );
    assert!(
        b.contains("<td style=\"text-align:right;vertical-align:bottom;\">b</td>"),
        "{b}"
    );
    assert!(
        b.contains("<td style=\"text-align:center;width:25%;\">c</td>"),
        "{b}"
    );
    assert!(
        b.contains("<td style=\"vertical-align:middle;\">d</td>"),
        "{b}"
    );
}

#[test]
fn a_table_in_center_or_an_aligned_div_is_placed_as_the_viewer_does() {
    let b = export("<center>\n\n| a |\n|---|\n| b |\n\n</center>\n");
    assert!(
        body(&b).contains("<table style=\"margin-left:auto;margin-right:auto;\">"),
        "{}",
        body(&b)
    );
    let b = export("<div align=\"right\">\n\n| a |\n|---|\n| b |\n\n</div>\n");
    assert!(
        body(&b).contains("<table style=\"margin-left:auto;margin-right:0;\">"),
        "{}",
        body(&b)
    );
    // Its own `align` wins over the one around it; a plain table has no margins of its own.
    let b =
        export("<div align=\"right\"><table align=\"left\"><tr><td>x</td></tr></table></div>\n");
    assert!(
        body(&b).contains("<table>") && !body(&b).contains("margin"),
        "{}",
        body(&b)
    );
    let b = export("| a |\n|---|\n| b |\n");
    assert!(body(&b).contains("<table>"), "{}", body(&b));
}

#[test]
fn the_page_follows_the_readers_colour_scheme_whichever_theme_made_it() {
    for theme in [LIGHT, crate::theme::DARK] {
        let _engine = crate::engine::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let doc = parse("```rust\nfn main() {}\n```\n");
        let html = export_html(
            &doc,
            &ExportOptions {
                title: "",
                theme,
                base_dir: None,
                remote_images: false,
                print: false,
            },
        );
        let light = html
            .find(":root{color-scheme:light;--bg:#fbfbfb;")
            .expect("light root");
        let dark = html
            .find("@media screen and (prefers-color-scheme:dark){")
            .expect("a dark block");
        assert!(light < dark);
        let block = &html[dark..html[dark..]
            .find("</style>")
            .map_or(html.len(), |e| dark + e)];
        assert!(
            block.contains(":root{color-scheme:dark;--bg:#202020;--text:#e4e4e4;"),
            "{block}"
        );
        assert!(block.contains("--note:#60cdff;") && block.contains("--caution:#f85149;"));
        assert!(block.contains("span[style*=\"--d:\"]{color:var(--d)!important}"));
        // The code carries both colours, the page being one file for both.
        assert!(body(&html).contains(";--d:#"), "{}", body(&html));
    }
}

#[test]
fn printing_keeps_the_light_palette_and_a_themed_picture_is_written_for_each_scheme() {
    let dir = scratch("themed");
    std::fs::write(dir.join("light.png"), PIXEL).unwrap();
    let mut other = PIXEL.to_vec();
    *other.last_mut().unwrap() ^= 1;
    std::fs::write(dir.join("dark.png"), &other).unwrap();
    let md = "<picture><source media=\"(prefers-color-scheme: dark)\" srcset=\"dark.png\">\
                  <source media=\"(prefers-color-scheme: light)\" srcset=\"light.png\">\
                  <img src=\"light.png\" alt=\"themed\"></picture>\n\n![plain](light.png)\n";
    let html = export_with(md, Some(&dir), false);
    let b = body(&html);
    assert!(
        b.contains("<img class=\"lt\" src=\"data:image/png;base64,"),
        "{b}"
    );
    assert!(
        b.contains("<img class=\"dk\" src=\"data:image/png;base64,"),
        "{b}"
    );
    // An image with no twin is written once and carries no scheme class.
    assert!(b.contains("<img src=\"data:image/png;base64,"), "{b}");
    assert_eq!(b.matches("alt=\"themed\"").count(), 2);
    assert!(html.contains(".dk{display:none}"));
    assert!(html.contains(".lt{display:none}.dk{display:inline}"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn highlight_runs_carry_the_dark_colour_when_both_lists_agree() {
    let source = "ab cd";
    let token = |range, color| highlight::Token { range, color };
    let light = vec![token(0..2, [1, 2, 3]), token(3..5, [4, 5, 6])];
    let dark = vec![token(0..2, [7, 8, 9]), token(3..5, [7, 8, 9])];
    let mut out = String::new();
    highlighted(&mut out, source, &light, Some(&dark));
    assert_eq!(
        out,
        "<span style=\"color:#010203;--d:#070809\">ab</span> \
             <span style=\"color:#040506;--d:#070809\">cd</span>"
    );
    // Lists that do not cover the same ranges leave the light colours alone.
    let other = vec![token(0..1, [7, 8, 9])];
    let mut out = String::new();
    highlighted(&mut out, source, &light, Some(&other));
    assert!(!out.contains("--d"), "{out}");
}
