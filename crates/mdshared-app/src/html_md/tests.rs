use super::*;

fn md(html: &str) -> String {
    to_markdown(html).unwrap_or_else(|| panic!("no Markdown for {html:?}"))
}

#[test]
fn headings_paragraphs_and_emphasis() {
    assert_eq!(
        md("<h1>Title</h1><p>One <b>bold</b> and <em>soft</em> word.</p><h3>Three</h3>"),
        "# Title\n\nOne **bold** and *soft* word.\n\n### Three"
    );
    assert_eq!(
        md("<p><strong> spaced </strong>x</p>"),
        " **spaced** x".trim()
    );
    assert_eq!(md("<p><s>gone</s> <del>too</del></p>"), "~~gone~~ ~~too~~");
}

#[test]
fn backslashes_in_destinations_and_markup_in_alt_stay_literal() {
    assert_eq!(
        md(r#"<a href="\\server\share\plan.md">Plan</a>"#),
        "[Plan](%5C%5Cserver%5Cshare%5Cplan.md)"
    );
    assert_eq!(
        md(r#"<img src="img\" alt="&lt;em&gt;*A*&lt;/em&gt;">"#),
        r"![\<em>\*A\*\</em>](img%5C)"
    );
}

#[test]
fn image_alt_inside_code_keeps_its_previous_escapes() {
    assert_eq!(
        md(r#"<code><img src="img" alt="&lt;em&gt;*A*&lt;/em&gt; \ &amp;amp;"></code>"#),
        r"`![<em>*A*</em> \\ \&amp;](img)`"
    );
}

#[test]
fn image_source_inside_code_keeps_its_backslashes() {
    assert_eq!(
        md(r#"<code><img src="dir\pic name.png" alt="Pic"></code>"#),
        r"`![Pic](dir\pic%20name.png)`"
    );
}

#[test]
fn links_and_images() {
    assert_eq!(
        md(r#"<p>See <a href="https://example.com/a b">the page</a>.</p>"#),
        "See [the page](https://example.com/a%20b)."
    );
    // A link to nowhere, or to a script, keeps its text only.
    assert_eq!(
        md(r##"<p><a href="#top">up</a> <a href="javascript:x()">run</a> <b>x</b></p>"##),
        "up run **x**"
    );
    assert_eq!(
        md(r#"<p><img src="a.png" alt="A [cat]"></p>"#),
        "![A  cat](a.png)"
    );
    assert_eq!(
        md(r#"<a href="x"><img src="y.png" alt="z"></a>"#),
        "[![z](y.png)](x)"
    );
}

#[test]
fn lists_nested_numbered_and_tight() {
    let html =
        "<ul><li>one</li><li>two<ul><li>two a</li><li>two b</li></ul></li><li>three</li></ul>";
    assert_eq!(md(html), "- one\n- two\n  - two a\n  - two b\n- three");
    let html = r#"<ol start="9"><li>nine</li><li>ten<ol><li>inner</li></ol></li></ol><p>after</p>"#;
    assert_eq!(md(html), "9. nine\n10. ten\n    1. inner\n\nafter");
    // Google Docs puts a paragraph in each item.
    assert_eq!(
        md("<ul><li><p>a</p></li><li><p>b</p></li></ul>"),
        "- a\n- b"
    );
    // An item copied without its list.
    assert_eq!(md("<li>alone</li>"), "- alone");
}

#[test]
fn code_inline_and_in_blocks() {
    assert_eq!(md("<p>Run <code>a*b_c</code> now</p>"), "Run `a*b_c` now");
    assert_eq!(md("<p><code>x`y</code></p>"), "``x`y``");
    assert_eq!(
        md(
            "<pre><code class=\"language-rust\">fn main() {\n    let a = 1 &lt; 2;\n}\n</code></pre>"
        ),
        "```rust\nfn main() {\n    let a = 1 < 2;\n}\n```"
    );
    assert_eq!(md("<pre>a\n```\nb</pre>"), "````\na\n```\nb\n````");
    // Tags inside a code block keep their text only; a block in it starts a line of its own.
    assert_eq!(
        md("<pre><b>x</b><br>y<div>z</div></pre>"),
        "```\nx\ny\nz\n```"
    );
}

#[test]
fn quotes_rules_and_breaks() {
    assert_eq!(
        md("<blockquote><p>one</p><p>two</p></blockquote><hr><p>a<br>b</p>"),
        "> one\n>\n> two\n\n---\n\na\\\nb"
    );
    assert_eq!(
        md("<blockquote>out<blockquote>in</blockquote></blockquote>"),
        "> out\n>\n> > in"
    );
}

#[test]
fn unknown_tags_keep_their_text_and_scripts_go() {
    assert_eq!(
        md("<custom-el>kept <b>bold</b></custom-el><script>alert(1)</script><style>p{}</style>"),
        "kept **bold**"
    );
    assert_eq!(
        md("<table><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>2</td></tr></table><h2>t</h2>"),
        "a | b\n\n1 | 2\n\n## t"
    );
}

#[test]
fn text_that_would_read_as_markdown_is_escaped() {
    assert_eq!(
        md("<p># not a heading</p><p>2. not a list</p><p>a * b [c] \\ _d_ snake_case</p><b>x</b>"),
        "\\# not a heading\n\n2\\. not a list\n\na \\* b \\[c\\] \\\\ \\_d\\_ snake_case\n\n**x**"
    );
    assert_eq!(
        md("<p>&lt;div&gt; &amp; &copy; &#x41;&#66; &bogus;</p><i>i</i>"),
        "\\<div> & \u{a9} AB \\&bogus;\n\n*i*"
    );
    // An unknown reference is not decoded: it stays as it was written (its `&` escaped above, as
    // any that reads as a reference is).
    assert_eq!(decode("&bogus; &amp"), "&bogus; &amp");
}

#[test]
fn word_processor_styles() {
    // Google Docs: the whole copy in a <b> that is not bold, bold and italic as styles.
    let html = r#"<meta charset="utf-8"><b style="font-weight:normal;" id="docs-internal-guid-1"><p dir="ltr"><span style="font-weight:700;white-space:pre-wrap;">Bold</span><span style="white-space:pre-wrap;"> and </span><span style="font-style:italic;white-space:pre-wrap;">italic</span></p></b>"#;
    assert_eq!(md(html), "**Bold** and *italic*");
}

#[test]
fn nothing_to_mark_is_none() {
    assert_eq!(to_markdown("<div><span>plain</span> text</div>"), None);
    assert_eq!(to_markdown(""), None);
    assert_eq!(to_markdown("<b>   </b>"), None);
    // A code editor's coloured copy: its plain text is the better paste.
    let vscode = r#"<div style="color: #d4d4d4;font-weight: normal;white-space: pre;"><div><span style="color: #569cd6;font-weight: bold;">fn</span> main</div></div>"#;
    assert_eq!(to_markdown(vscode), None);
}

#[test]
fn a_whole_document_from_the_clipboard() {
    let html = "<html><head><title>T</title><style>b{}</style></head><body>\r\n<!--StartFragment--><h2>Hi</h2><p>there</p><!--EndFragment-->\r\n</body></html>";
    assert_eq!(md(html), "## Hi\n\nthere");
}

// ---- fuzz-style: never a panic, whatever comes in -----------------------------------------------

/// A small deterministic generator (xorshift), so a failure repeats.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[(self.next() % items.len() as u64) as usize]
    }
}

const PIECES: &[&str] = &[
    "<",
    ">",
    "</",
    "/>",
    "<!--",
    "-->",
    "<!",
    "<?",
    "&",
    ";",
    "&#",
    "&#x",
    "&amp;",
    "&#0;",
    "&#xD800;",
    "&#99999999999;",
    "&nbsp;",
    "\"",
    "'",
    "=",
    " ",
    "\n",
    "\t",
    "\r\n",
    "<p>",
    "</p>",
    "<b>",
    "</b>",
    "<i>",
    "</i>",
    "<a href=\"x y\">",
    "<a href=",
    "</a>",
    "<ul>",
    "</ul>",
    "<ol start=\"18446744073709551615\">",
    "<ol start=-3>",
    "</ol>",
    "<li>",
    "</li>",
    "<pre>",
    "</pre>",
    "<code class=\"language-",
    "<code>",
    "</code>",
    "<blockquote>",
    "</blockquote>",
    "<h1>",
    "</h7>",
    "<h0>",
    "<br>",
    "<br/>",
    "<hr>",
    "<img src=",
    "alt=",
    "<script>",
    "</script",
    "<style",
    "<span style=\"font-weight:700\">",
    "</span>",
    "<table>",
    "<tr>",
    "<td>",
    "<th>",
    "`",
    "```",
    "*",
    "_",
    "\\",
    "[",
    "]",
    "#",
    "1. ",
    "- ",
    "é",
    "\u{1F600}",
    "\u{0}",
    "\u{a0}",
    "\u{3000}",
    "ğ",
    "text",
    "<div style=\"white-space:pre",
    "<b style=\"font-weight:normal\">",
    "<p",
    "<P>",
    "</LI>",
    "<UL>",
    "<x=y>",
    "<a b c d>",
];

#[test]
fn arbitrary_html_never_panics() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..20_000 {
        let n = (rng.next() % 40) as usize;
        let mut s = String::new();
        for _ in 0..n {
            s.push_str(rng.pick(PIECES));
        }
        let out = to_markdown(&s);
        if let Some(out) = out {
            assert!(out.len() <= s.len() * 8 + (1 << 16) + 4096, "{s:?}");
        }
    }
}

#[test]
fn arbitrary_characters_never_panic() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    for _ in 0..5_000 {
        let n = (rng.next() % 200) as usize;
        let s: String = (0..n)
            .map(|_| {
                let r = rng.next();
                match r % 4 {
                    // Mostly the characters HTML is made of.
                    0 | 1 => b"<>/=\"'&;#!-? abp\n"[(r >> 8) as usize % 17] as char,
                    2 => char::from_u32((r >> 8) as u32 % 0x11_0000).unwrap_or('x'),
                    _ => char::from_u32(0x80 + (r >> 8) as u32 % 0x800).unwrap_or('y'),
                }
            })
            .collect();
        let _ = to_markdown(&s);
    }
}

#[test]
fn every_cut_of_a_real_copy_reads() {
    let html = r#"<h1 id="x">Café ☕</h1><p>A <b>bold <i>and <a href="https://ex.com/(1)">linked</a></i></b> line&nbsp;here.</p><ul><li>one<ol><li><code>`x`</code></li></ol></li></ul><pre class="lang-py">print("ğ")</pre><blockquote><p>q</p></blockquote>"#;
    for (i, _) in html.char_indices() {
        let _ = to_markdown(&html[..i]);
        let _ = to_markdown(&html[i..]);
    }
    assert_eq!(
        md(html),
        "# Café ☕\n\nA **bold *and [linked](https://ex.com/%281%29)*** line here.\n\n- one\n  1. `` `x` ``\n\n```py\nprint(\"ğ\")\n```\n\n> q"
    );
}

#[test]
fn deep_nesting_stays_bounded_and_quick() {
    let start = std::time::Instant::now();
    let deep = "<ul><li>x".repeat(20_000) + &"<blockquote>q".repeat(20_000);
    let out = to_markdown(&deep).unwrap_or_default();
    assert!(out.len() <= deep.len() * 8 + (1 << 16), "{}", out.len());
    let wide = "<b><i><s><code><a href=x>".repeat(20_000) + "t";
    let _ = to_markdown(&wide);
    let raw = "<script>".repeat(50_000);
    assert_eq!(to_markdown(&raw), None);
    assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
}

// ---- the review of the first version: one test per finding --------------------------------------

#[test]
fn output_never_passes_the_cap() {
    let nest = "<blockquote>".repeat(16) + &r#"<ol start="999999999"><li>"#.repeat(16);
    // A code block left open, written whole by finish().
    let html = nest.clone() + "<pre>" + &"x\n".repeat(50_000);
    assert_eq!(to_markdown(&html), None);
    let html = nest.clone() + "<pre>a" + &"\n".repeat(50_000) + "b";
    assert_eq!(to_markdown(&html), None);
    // A paragraph of hard breaks, written whole at its end tag.
    let html = nest.clone() + "<p>" + &"x<br>".repeat(20_000) + "</p>";
    assert_eq!(to_markdown(&html), None);
    // Shallow, the same paragraph is no trouble.
    let html = "<p>".to_string() + &"x<br>".repeat(20_000) + "</p><b>b</b>";
    let out = md(&html);
    assert!(out.len() <= cap(html.len()));
    assert!(out.ends_with("x\n\n**b**"));
}

#[test]
fn decode_is_quick_on_text_full_of_ampersands() {
    let start = std::time::Instant::now();
    let html = "<b>x</b>".to_string() + &"&".repeat(1_000_000);
    let out = md(&html);
    assert_eq!(out.len(), 5 + 1_000_000);
    let query = "<b>x</b>".to_string() + &"see https://e.com/?a=1&b=2 ".repeat(40_000);
    let _ = md(&query);
    assert!(start.elapsed().as_secs() < 3, "{:?}", start.elapsed());
}

#[test]
fn a_table_row_stays_on_one_line_whatever_its_cells_hold() {
    assert_eq!(
        md(
            "<h2>T</h2><table><tr><td><p>a</p></td><td><p>b</p></td></tr><tr><td><p>1</p></td><td><div>2</div><p>3</p></td></tr></table><p>after</p>"
        ),
        "## T\n\na | b\n\n1 | 2 3\n\nafter"
    );
    // Rows left open, a cell's hard break, and a table in a cell.
    assert_eq!(
        md(
            "<h2>T</h2><table><tr><td>a<br>b<td>c<tr><td>d<table><tr><td>e</td><td>f</td></tr></table></td><td>g</td></table>"
        ),
        "## T\n\na b | c\n\nd e f | g"
    );
}

#[test]
fn a_cell_copied_without_its_table_ends_its_row_at_the_next_block() {
    // A row in no table, left open: the blocks after its cells are blocks, not more of the row.
    assert_eq!(
        md("<td>a</td><h1>T</h1><ul><li>x</li></ul><p>y</p>"),
        "a

# T

- x

y"
    );
    assert_eq!(
        md("<td>a</td><td>b</td><p>y</p><h2>T</h2>"),
        "a | b

y

## T"
    );
    // Blocks inside its cells stay on its line, and a table after it is a table.
    assert_eq!(
        md(
            "<td><p>a</p><p>b</p></td><td>c</td><h2>T</h2><table><tr><td>1</td><td>2</td></tr></table>"
        ),
        "a b | c

## T

1 | 2"
    );
}

#[test]
fn word_styles_are_read_by_their_whole_name() {
    // Word's mso-bidi-font-weight is about complex scripts, not this text.
    assert_eq!(
        md(
            "<h1>T</h1><p class=MsoNormal><b style='mso-bidi-font-weight:normal'><span lang=EN-US>Bold</span></b> rest</p>"
        ),
        "# T\n\n**Bold** rest"
    );
    assert_eq!(
        md("<h1>T</h1><p><span style='mso-bidi-font-weight:700'>plain</span></p>"),
        "# T\n\nplain"
    );
    // The last weight written is the one that counts.
    assert_eq!(
        md(
            "<p><span style='font-weight:700;font-weight:400'>x</span> <span style='font-weight: 400; FONT-WEIGHT: Bold'>y</span></p>"
        ),
        "x **y**"
    );
}

#[test]
fn bold_and_italic_in_one_span_keep_both() {
    assert_eq!(
        md(r#"<p><span style="font-weight:700;font-style:italic;">both</span></p>"#),
        "***both***"
    );
    // Inside bold only the italic is new.
    assert_eq!(
        md(r#"<p><b>x <span style="font-weight:700;font-style:italic">y</span></b></p>"#),
        "**x *y***"
    );
    assert_eq!(
        md(r#"<p><b style="font-style:italic">z</b></p>"#),
        "***z***"
    );
}

#[test]
fn a_quote_inside_a_list_item_stays_in_the_item() {
    assert_eq!(
        md("<ul><li>a<blockquote>q</blockquote></li><li>b</li></ul>"),
        "- a\n\n  > q\n- b"
    );
    // A list inside a quote inside a list (a quote may follow an item's line at once).
    assert_eq!(
        md("<ul><li>a<blockquote><ul><li>in</li></ul></blockquote></li></ul>"),
        "- a\n  > - in"
    );
    // A quote's end closes the list left open in it.
    assert_eq!(
        md("<blockquote><ul><li>x</blockquote><p>after</p>"),
        "> - x\n\nafter"
    );
}

#[test]
fn an_ampersand_that_reads_as_a_reference_is_escaped() {
    assert_eq!(
        md(
            "<p>Use &amp;copy; and &amp;lt;div&amp;gt; or &amp;#169; &amp;#xA9; at AT&amp;T &amp; co</p><b>x</b>"
        ),
        "Use \\&copy; and \\&lt;div\\&gt; or \\&#169; \\&#xA9; at AT&T & co\n\n**x**"
    );
    assert_eq!(
        md(r#"<a href="https://e.com/?a=1&amp;copy;=2&amp;b=3">l</a>"#),
        "[l](https://e.com/?a=1\\&copy;=2&b=3)"
    );
    assert_eq!(
        md(r#"<img src="a.png" alt="&amp;amp; x">"#),
        "![\\&amp; x](a.png)"
    );
    // In code it is code.
    assert_eq!(md("<p><code>&amp;copy;</code></p>"), "`&copy;`");
}

#[test]
fn tildes_that_would_mark_or_fence_are_escaped() {
    assert_eq!(
        md("<p>~~~</p><p>after</p><b>x</b>"),
        "\\~\\~\\~\n\nafter\n\n**x**"
    );
    assert_eq!(
        md("<p>a ~~b~~ c, x~y~z, ~5 min</p><b>x</b>"),
        "a \\~\\~b\\~\\~ c, x\\~y\\~z, ~5 min\n\n**x**"
    );
}

#[test]
fn line_starts_that_would_read_as_markdown_are_escaped() {
    assert_eq!(
        md("<p>a<br>--</p><p>b<br>=</p><p>c<br>-- -</p><p>d<br>==</p><b>x</b>"),
        "a\\\n\\--\n\nb\\\n\\=\n\nc\\\n\\-- -\n\nd\\\n\\==\n\n**x**"
    );
    assert_eq!(
        md(
            "<p>&gt; not a quote</p><p>- not an item</p><p>+ nor this</p><p>---</p><p>===</p><p>1) no</p><p>-</p><p>+</p><b>x</b>"
        ),
        "\\> not a quote\n\n\\- not an item\n\n\\+ nor this\n\n\\---\n\n\\===\n\n1\\) no\n\n\\-\n\n\\+\n\n**x**"
    );
    // Words that only start with these stay as they are.
    assert_eq!(
        md("<p>-dash +plus =eq a-b 2.5 1.x</p><b>x</b>"),
        "-dash +plus =eq a-b 2.5 1.x\n\n**x**"
    );
}

#[test]
fn a_heading_that_ends_in_a_hash_keeps_it() {
    assert_eq!(
        md("<h2>Issue #</h2><h1>#</h1><h3>C#</h3><h4>a ##</h4>"),
        "## Issue \\#\n\n# \\#\n\n### C#\n\n#### a \\##"
    );
}

#[test]
fn white_space_pre_on_a_code_block_is_not_a_code_editors_copy() {
    assert_eq!(
        md("<h2>T</h2><pre><code style=\"white-space: pre;\">fn x()</code></pre>"),
        "## T\n\n```\nfn x()\n```"
    );
    assert_eq!(
        md(
            "<h2>T</h2><pre style=\"white-space:pre\"><span style=\"white-space:pre\">a</span></pre>"
        ),
        "## T\n\n```\na\n```"
    );
    assert_eq!(
        to_markdown("<h2>T</h2><div style=\"white-space:pre\">a</div>"),
        None
    );
}

#[test]
fn an_inline_element_around_a_block_marks_it() {
    assert_eq!(
        md(r#"<h1>T</h1><a href="https://x"><div>card</div></a>"#),
        "# T\n\n[card](https://x)"
    );
    assert_eq!(md("<b><p>a</p><p>b</p></b>"), "**a**\n\n**b**");
    assert_eq!(md("<code><p>a*b</p></code>"), "`a*b`");
    assert_eq!(
        md("<p>x <b>y<div>z</div></b> w</p>"),
        "x **y**\n\n**z**\n\nw"
    );
}

#[test]
fn a_script_link_is_dropped_with_tabs_or_new_lines_in_its_scheme() {
    assert_eq!(
        md(
            "<p><a href=\"java&#9;script:alert(1)\">x</a> <a href=\"java\nscript:y\">y</a> <a href=\" &#10;JavaScript:z\">z</a> <b>b</b></p>"
        ),
        "x y z **b**"
    );
    // Other links lose their control characters too.
    assert_eq!(
        md("<a href=\"https://e.com/a&#9;b\">l</a>"),
        "[l](https://e.com/ab)"
    );
}

#[test]
fn text_before_a_block_inside_pre_is_its_own_line() {
    assert_eq!(md("<pre>y<div>z</div></pre>"), "```\ny\nz\n```");
    assert_eq!(
        md("<pre><div>a</div><div>b</div>c<p>d</p></pre>"),
        "```\na\nb\nc\nd\n```"
    );
}

#[test]
fn more_of_what_the_converter_claims() {
    // Google Docs writes a nested list beside its item.
    assert_eq!(
        md("<ul><li><p>a</p></li><ul><li><p>b</p></li></ul><li><p>c</p></li></ul>"),
        "- a\n  - b\n- c"
    );
    assert_eq!(
        md("<p><kbd>Ctrl</kbd> <samp>out</samp> <tt>tt</tt> <cite>c</cite> <dfn>d</dfn></p>"),
        "`Ctrl` `out` `tt` *c* *d*"
    );
    // A data: picture is dropped, its alt text kept.
    assert_eq!(
        md(r#"<p><img src="data:image/png;base64,AAAA" alt="pic"> <b>x</b></p>"#),
        "pic **x**"
    );
    // A rule and an empty item inside a list.
    assert_eq!(
        md("<ul><li>a<hr></li><li></li><li>b</li></ul>"),
        "- a\n\n  ---\n-\n- b"
    );
}

// ---- fuzz-style: the cap holds, whatever comes in -----------------------------------------------

/// Pieces that make the output grow: deep nesting with wide markers first, then many lines.
const HOSTILE: &[&str] = &[
    "<blockquote>",
    "<ol start=\"999999999\"><li>",
    "<ul><li>",
    "<li>",
    "<pre>",
    "</pre>",
    "<p>",
    "</p>",
    "<br>",
    "x<br>",
    "\n",
    "x\n",
    "<hr>",
    "<h1>",
    "</h1>",
    "<a href=\"https://example.com/a/long/address/of/a/page\">",
    "<div>",
    "</div>",
    "<b>",
    "</b>",
    "<tr><td>",
    "<td>",
    "&",
    "&amp;copy;",
    "~",
    "#",
    "-",
    "=",
    "*",
    "</li>",
    "</ol>",
    "</blockquote>",
];

#[test]
fn hostile_html_never_panics_nor_passes_the_cap() {
    let mut rng = Rng(0x5eed_cafe_f00d_0001);
    for round in 0..300 {
        let depth = (rng.next() % 40) as usize;
        let mut s = String::new();
        for _ in 0..depth {
            s.push_str(rng.pick(&HOSTILE[..4]));
        }
        let n = 1 + (rng.next() % if round % 10 == 0 { 20_000 } else { 400 }) as usize;
        // A run of one piece, then of another: the shape that grows the output most.
        let mut left = n;
        while left > 0 {
            let piece = rng.pick(HOSTILE);
            let run = 1 + (rng.next() % 500) as usize;
            for _ in 0..run.min(left) {
                s.push_str(piece);
            }
            left = left.saturating_sub(run);
        }
        // Every other round ends in one long block left open, written whole at the end.
        if round % 2 == 0 {
            s.push_str(rng.pick(&["<pre>", "<p>", "<h2>", "<li>"]));
            let line = rng.pick(&["x\n", "\n", "x<br>", "<br>", "a&amp;copy;\n"]);
            s.push_str(&line.repeat(1 + (rng.next() % 30_000) as usize));
        }
        if let Some(out) = to_markdown(&s) {
            assert!(
                out.len() <= cap(s.len()),
                "{} > {}",
                out.len(),
                cap(s.len())
            );
        }
    }
    // The same for the inputs the first fuzz test makes.
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..5_000 {
        let n = (rng.next() % 40) as usize;
        let s: String = (0..n).map(|_| rng.pick(PIECES)).collect();
        if let Some(out) = to_markdown(&s) {
            assert!(out.len() <= cap(s.len()), "{s:?}");
        }
    }
}

/// `md` of `html` with a bold word after it, so that a paste with only escapes in it still
/// converts; the word is taken off again.
fn md_marked(html: &str) -> String {
    let out = md(&format!("{html}<p><b>b</b></p>"));
    out.strip_suffix("\n\n**b**")
        .unwrap_or_else(|| panic!("{out:?}"))
        .to_string()
}

/// Audit 4, W7 and T2: a `<` or `&` that ends one text token is escaped by what the next token
/// begins with, as it would be in one token. Syntax-coloured markup has this shape.
#[test]
fn escaping_looks_past_the_end_of_a_text_token() {
    assert_eq!(
        md_marked("<p>&lt;<span>script&gt;alert(1)</span></p>"),
        "\\<script>alert(1)"
    );
    assert_eq!(md_marked("<p>&amp;<span>copy;</span></p>"), "\\&copy;");
    assert_eq!(md_marked("<p>&amp;co<span>py;</span> x</p>"), "\\&copy; x");
    assert_eq!(md_marked("<p>&lt;<span>/b&gt;</span></p>"), "\\</b>");
    // Neither a tag nor a reference: nothing to escape.
    assert_eq!(
        md_marked("<p>1 &lt;<span> 2</span> &amp;<span> 3</span></p>"),
        "1 < 2 & 3"
    );
    // A mark between them: the backslash goes before the `<`, the mark around its word.
    assert_eq!(md("<p>&lt;<b>script</b></p>"), "\\<**script**");
    // Inside code nothing is escaped, and a held `<` does not reach into it.
    assert_eq!(md("<p>&lt;<code>b</code></p>"), "<`b`");
}

/// Audit 4, W8: a backslash in an image's alt is the alt's own: one at its end cannot escape
/// the `]`.
#[test]
fn an_image_alt_that_ends_in_a_backslash_keeps_its_image() {
    assert_eq!(
        md(r#"<p><img src="a.png" alt="back\"></p>"#),
        "![back\\\\](a.png)"
    );
    assert_eq!(
        md(r#"<p><img src="a.png" alt="a\b"></p>"#),
        "![a\\\\b](a.png)"
    );
}

/// Audit 4, N3: a `vbscript:` or `data:` link keeps its text only, as a `javascript:` one does.
#[test]
fn script_and_data_links_keep_their_text_only() {
    assert_eq!(
        md(
            r#"<p><a href="vbscript:msgbox(1)">a</a> <a href=" DATA:text/html,x">b</a> <a href="VBScript:x">c</a> <b>x</b></p>"#
        ),
        "a b c **x**"
    );
}

/// Audit 4, N4: `<?` would start a processing instruction, so it is escaped; and marks inside
/// code would be its text, so none is made there.
#[test]
fn a_processing_instruction_is_escaped_and_code_holds_no_marks() {
    assert_eq!(
        md_marked("<p>&lt;?php echo 1; ?&gt;</p>"),
        "\\<?php echo 1; ?>"
    );
    assert_eq!(
        md(r#"<p><code>a <b>bold</b> <i>it</i> <a href="u">link</a></code></p>"#),
        "`a bold it link`"
    );
}
