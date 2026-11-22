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
    // Tags inside a code block keep their text only.
    assert_eq!(
        md("<pre><b>x</b><br>y<div>z</div></pre>"),
        "```\nx\nyz\n```"
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
        "\\<div> & \u{a9} AB &bogus;\n\n*i*"
    );
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
