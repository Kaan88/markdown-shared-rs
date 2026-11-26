//! Encodes images, text and styles for the standalone HTML page.

use super::*;

pub(super) fn picture_of(d: &DecodedImage) -> Option<Picture> {
    let png = png_bytes(d)?;
    Some(Picture {
        uri: format!("data:image/png;base64,{}", base64(&png)),
        width: d.width as f32 / 2.0,
        height: d.height as f32 / 2.0,
        baseline: d.baseline as f32 / 2.0,
        dark: None,
    })
}

/// `picture` with the drawing for the dark page, if there is one and it is not the same pixels.
pub(super) fn with_dark(picture: Option<Picture>, dark: Option<DecodedImage>) -> Option<Picture> {
    let mut picture = picture?;
    if let Some(dark) = dark.and_then(|d| picture_of(&d))
        && dark.uri != picture.uri
    {
        picture.dark = Some(dark.uri);
    }
    Some(picture)
}

fn png_bytes(d: &DecodedImage) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, d.width, d.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&d.rgba).ok()?;
    }
    Some(out)
}

/// Rounded to a quarter px, without a trailing `.0`.
pub(super) fn px(v: f32) -> String {
    let q = (v * 4.0).round() / 4.0;
    if q.fract() == 0.0 {
        format!("{}", q as i64)
    } else {
        format!("{q}")
    }
}

pub(super) fn align_css(a: Align) -> Option<&'static str> {
    match a {
        Align::None => None,
        Align::Left => Some("text-align:left;"),
        Align::Center => Some("text-align:center;"),
        Align::Right => Some("text-align:right;"),
        Align::Justify => Some("text-align:justify;"),
    }
}

pub(super) fn marker_style(m: Option<ListMarker>) -> String {
    let t = match m {
        None => return String::new(),
        Some(ListMarker::Decimal) => "decimal",
        Some(ListMarker::LowerAlpha) => "lower-alpha",
        Some(ListMarker::UpperAlpha) => "upper-alpha",
        Some(ListMarker::LowerRoman) => "lower-roman",
        Some(ListMarker::UpperRoman) => "upper-roman",
        Some(ListMarker::Disc) => "disc",
        Some(ListMarker::Circle) => "circle",
        Some(ListMarker::Square) => "square",
        Some(ListMarker::None) => "none",
    };
    format!(" style=\"list-style-type:{t}\"")
}

/// Whether a link may be written: a web or mail address, or one with no scheme (a relative file
/// or a `#heading`). Anything else (`javascript:`, `data:`, `file:`) is its text only.
pub(super) fn safe_href(href: &str) -> bool {
    let href = href.trim();
    if href.is_empty() {
        return false;
    }
    let head: String = href
        .chars()
        .take_while(|c| !matches!(c, ':' | '/' | '?' | '#'))
        .collect();
    if head.len() == href.len() || !href[head.len()..].starts_with(':') {
        // No scheme: a relative reference.
        return true;
    }
    matches!(
        head.to_ascii_lowercase().as_str(),
        "http" | "https" | "mailto"
    )
}

/// Appends `text` escaped for HTML; `breaks` turns a newline (a hard break) into `<br>`.
pub(super) fn escape_into(out: &mut String, text: &str, breaks: bool) {
    // The clipboard writer's escaping (`mdshared_md_core::rich`), so both agree on it.
    mdshared_md_core::rich::escape_html(out, text, if breaks { Some("<br>\n") } else { None });
}

/// The source with each run of one colour in a `<span>`. `dark` is the same source coloured for
/// the dark page: when its tokens cover the same ranges, each run also carries its dark colour as
/// `--d`, which the page's dark style puts to use.
pub(super) fn highlighted(
    out: &mut String,
    source: &str,
    tokens: &[highlight::Token],
    dark: Option<&[highlight::Token]>,
) {
    let dark = dark.filter(|d| {
        d.len() == tokens.len() && d.iter().zip(tokens).all(|(a, b)| a.range == b.range)
    });
    type Run = ((Color, Option<Color>), usize);
    let mut at = 0;
    let mut run: Option<Run> = None;
    let flush = |out: &mut String, run: &mut Option<Run>, end: usize| {
        if let Some(((c, d), start)) = run.take() {
            let _ = write!(
                out,
                "<span style=\"color:#{:02x}{:02x}{:02x}",
                c[0], c[1], c[2]
            );
            if let Some(d) = d {
                let _ = write!(out, ";--d:#{:02x}{:02x}{:02x}", d[0], d[1], d[2]);
            }
            out.push_str("\">");
            escape_into(out, &source[start..end], false);
            out.push_str("</span>");
        }
    };
    for (i, t) in tokens.iter().enumerate() {
        let (s, e) = (t.range.start, t.range.end);
        if s < at
            || e > source.len()
            || s >= e
            || !source.is_char_boundary(s)
            || !source.is_char_boundary(e)
        {
            continue;
        }
        if s > at {
            flush(out, &mut run, at);
            escape_into(out, &source[at..s], false);
        }
        let colors = (t.color, dark.map(|d| d[i].color));
        match run {
            Some((c, _)) if c == colors => {}
            _ => {
                flush(out, &mut run, s);
                run = Some((colors, s));
            }
        }
        at = e;
    }
    flush(out, &mut run, at);
    if at < source.len() {
        escape_into(out, &source[at..], false);
    }
}

/// The type of a picture by its first bytes: the formats a browser shows that the page shows.
pub(super) fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if image::looks_like_svg(bytes) {
        Some("image/svg+xml")
    } else {
        None
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(super) fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// A `file://` address for a path: it works on this machine only, which is what an image too
/// large to embed gets.
pub(super) fn file_url(path: &Path) -> String {
    let path_text = path.to_string_lossy();
    // File URLs take a drive letter, not the Windows namespace used to open a canonical path.
    #[cfg(windows)]
    let path_text = if crate::image::verbatim_disk(path) {
        &path_text[4..]
    } else {
        &path_text
    };
    let s = path_text.replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/');
    }
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(b as char);
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// A font list as CSS: each family quoted, the generic ones left as keywords.
fn css_families(list: &str) -> String {
    const GENERIC: &[&str] = &[
        "serif",
        "sans-serif",
        "monospace",
        "cursive",
        "fantasy",
        "system-ui",
    ];
    list.split(',')
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(|f| {
            if GENERIC.contains(&f) {
                f.to_string()
            } else {
                format!("\"{}\"", f.replace(['"', '\\'], ""))
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The colour variables of one palette: the page's colours and the five alerts'.
fn palette(theme: &Theme) -> String {
    use mdshared_md_core::model::AlertKind::*;
    let mut css = String::new();
    let vars = [
        ("bg", hex(theme.bg)),
        ("text", hex(theme.text)),
        ("heading", hex(theme.heading)),
        ("muted", hex(theme.muted)),
        ("accent", hex(theme.accent)),
        ("border", hex(theme.border)),
        ("code-bg", hex(theme.code_bg)),
        ("head-bg", hex(theme.head_bg)),
        ("tint", hex(theme.tint)),
        ("mark", hex(theme.mark())),
    ];
    for (k, v) in &vars {
        let _ = write!(css, "--{k}:{v};");
    }
    for (name, kind) in [
        ("note", Note),
        ("tip", Tip),
        ("important", Important),
        ("warning", Warning),
        ("caution", Caution),
    ] {
        let (c, bg) = theme.alert(kind);
        let _ = write!(css, "--{name}:{};--{name}-bg:{};", hex(c), hex(bg));
    }
    css
}

/// The page's style sheet. The colours are the viewer's light palette, and under
/// `prefers-color-scheme: dark` its dark palette, whichever of the two the viewer showed when
/// the file was made: the page follows the reader's browser. Fonts and widths are the viewer's.
/// Printing keeps the light palette.
pub(super) fn stylesheet(theme: &Theme) -> String {
    let mut css = String::new();
    let width = if theme.reading_width >= 100_000.0 {
        "none".to_string()
    } else {
        format!("{}px", (theme.reading_width + 2.0 * theme.margin) as i64)
    };
    let _ = writeln!(
        css,
        ":root{{color-scheme:light;{}--sans:{};--mono:{};--width:{width};--margin:{}px}}\n",
        palette(&crate::theme::LIGHT),
        css_families(theme.sans),
        css_families(theme.mono),
        theme.margin as i64
    );
    css.push_str(STYLE);
    let _ = write!(
        css,
        "@media screen and (prefers-color-scheme:dark){{\n:root{{color-scheme:dark;{}}}\n\
         span[style*=\"--d:\"]{{color:var(--d)!important}}\n\
         .lt{{display:none}}.dk{{display:inline}}\n}}\n",
        palette(&crate::theme::DARK)
    );
    css
}

const STYLE: &str = r##"html{background:var(--bg);-webkit-text-size-adjust:100%}
body{margin:0;color:var(--text);font:16px/1.6 var(--sans);overflow-wrap:break-word}
main{box-sizing:border-box;max-width:var(--width);margin:0 auto;padding:32px var(--margin) 48px}
h1,h2,h3,h4,h5,h6{color:var(--heading);font-weight:600;line-height:1.25;margin:0}
h1{font-size:34px;margin-bottom:14px}
h2{font-size:24px;margin:22px 0 12px}
h3{font-size:18px;margin:12px 0 8px}
h4{font-size:17px;margin:10px 0 8px}
h5{font-size:16px;margin:8px 0 6px}
h6{font-size:15px;margin:8px 0 6px}
p,ul,ol,pre,table,blockquote,figure,details,.alert,.dt,.dd{margin:0 0 12px}
p.math,p.diagram{text-align:center;margin:10px 0 18px}
img{max-width:100%;height:auto}
.dk{display:none}
a{color:var(--accent)}
hr{border:0;border-top:1px solid var(--border);margin:16px 0}
ul,ol{padding-left:2em}
li>ul,li>ol{margin:0}
li.task{list-style:none}
li.task>input{margin:0 .4em 0 -1.4em;vertical-align:middle}
code,kbd{font-family:var(--mono);font-size:.9em}
:not(pre)>code{background:var(--code-bg);border-radius:4px;padding:.1em .35em}
kbd{border:1px solid var(--border);border-bottom-width:2px;border-radius:4px;padding:.05em .4em;background:var(--code-bg)}
mark{background:var(--mark);color:inherit}
pre{position:relative;background:var(--code-bg);border:1px solid var(--border);border-radius:6px;padding:13px 16px;overflow:auto;font:14px/1.5 var(--mono);white-space:pre}
pre code{font:inherit;background:none;padding:0}
pre[data-lang]::after{content:attr(data-lang);position:absolute;top:6px;right:12px;font:12px var(--sans);color:var(--muted)}
blockquote{border-left:3px solid var(--border);margin-left:0;padding:2px 0 2px 16px;color:var(--muted)}
.alert{border-left:3px solid var(--note);background:var(--note-bg);padding:12px 16px;border-radius:0 4px 4px 0}
.alert>:last-child{margin-bottom:0}
.alert-title{font-weight:600;color:var(--note)}
.alert-tip{border-color:var(--tip);background:var(--tip-bg)}.alert-tip .alert-title{color:var(--tip)}
.alert-important{border-color:var(--important);background:var(--important-bg)}.alert-important .alert-title{color:var(--important)}
.alert-warning{border-color:var(--warning);background:var(--warning-bg)}.alert-warning .alert-title{color:var(--warning)}
.alert-caution{border-color:var(--caution);background:var(--caution-bg)}.alert-caution .alert-title{color:var(--caution)}
table{border-collapse:collapse;max-width:100%}
th,td{border:1px solid var(--border);padding:6px 13px;text-align:left;vertical-align:top}
th{background:var(--head-bg);font-weight:600}
summary{cursor:pointer;font-weight:600}
summary>div,summary>p{display:inline}
figure{margin-left:40px;margin-right:40px}
abbr[title]{text-decoration:underline dotted}
figcaption{font-size:.875em;color:var(--muted)}
.dt{font-weight:600;margin-bottom:2px}
.dd{margin-left:2em}
sup.fnref{font-size:.75em;line-height:0}
.footnotes{font-size:.9em;color:var(--muted)}
.footnotes ol{margin:0}
.footnotes p{margin:0;display:inline}
.footnotes .back{text-decoration:none}
@media print{
@page{margin:16mm}
html{background:#fff}
body{color:#000;font-size:11pt}
main{max-width:none;padding:0}
*{-webkit-print-color-adjust:exact;print-color-adjust:exact}
pre{white-space:pre-wrap;overflow:visible;break-inside:avoid}
pre[data-lang]::after{position:static;display:block;text-align:right;margin-bottom:4px}
table,figure,blockquote,.alert,img{break-inside:avoid}
tr{break-inside:avoid}
h1,h2,h3,h4,h5,h6{break-after:avoid}
a{color:inherit}
a[href^="http"]:not([href*="#"])::after{content:" (" attr(href) ")";font-size:.85em;color:var(--muted);word-break:break-all}
}
"##;
