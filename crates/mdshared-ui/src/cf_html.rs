//! The Windows clipboard's "HTML Format" (CF_HTML): HTML wrapped in a header that gives byte
//! offsets. Pure, so the arithmetic is tested on every system; only Windows puts it on the
//! clipboard (other systems carry plain `text/html`).

const PREFIX: &str = "<html><body>\r\n<!--StartFragment-->";
const SUFFIX: &str = "<!--EndFragment-->\r\n</body></html>";

/// Builds the CF_HTML text for an HTML fragment. All offsets are in bytes of the UTF-8 result and
/// count from its first byte; each is written as ten digits so the header has a fixed length.
pub fn cf_html(fragment: &str) -> String {
    let header_len = header(0, 0, 0, 0).len();
    let start_html = header_len;
    let start_fragment = start_html + PREFIX.len();
    let end_fragment = start_fragment + fragment.len();
    let end_html = end_fragment + SUFFIX.len();
    let mut out = header(start_html, end_html, start_fragment, end_fragment);
    out.reserve(end_html - header_len);
    out.push_str(PREFIX);
    out.push_str(fragment);
    out.push_str(SUFFIX);
    out
}

fn header(
    start_html: usize,
    end_html: usize,
    start_fragment: usize,
    end_fragment: usize,
) -> String {
    format!(
        "Version:0.9\r\nStartHTML:{start_html:010}\r\nEndHTML:{end_html:010}\r\nStartFragment:{start_fragment:010}\r\nEndFragment:{end_fragment:010}\r\n"
    )
}

/// The most HTML a paste reads from the clipboard, in bytes.
pub const MAX_BYTES: usize = 16 << 20;

/// The fragment a CF_HTML text holds: the bytes its `StartFragment` and `EndFragment` offsets
/// name, else what lies between the `<!--StartFragment-->` and `<!--EndFragment-->` comments,
/// else everything from the first tag on. Offsets that are missing, out of range or reversed
/// (some programs write `-1`) fall through to the next way. Bytes that are not UTF-8 are
/// replaced. `None` for an empty fragment.
pub fn fragment(cf: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(cf);
    let offset = |name: &str| -> Option<usize> {
        let at = text.find(name)? + name.len();
        let digits: String = text
            .get(at..)?
            .chars()
            .take_while(char::is_ascii_digit)
            .take(12)
            .collect();
        digits.parse().ok()
    };
    // The offsets count bytes of what was on the clipboard, so they are applied to `cf`.
    let by_offsets = offset("StartFragment:")
        .zip(offset("EndFragment:"))
        .filter(|(a, b)| a < b)
        .and_then(|(a, b)| cf.get(a..b))
        .map(|b| String::from_utf8_lossy(b).into_owned());
    let by_comments = || {
        const START: &str = "<!--StartFragment-->";
        let a = text.find(START)? + START.len();
        let b = text.get(a..)?.find("<!--EndFragment-->")? + a;
        text.get(a..b).map(str::to_string)
    };
    let from_first_tag = || text.get(text.find('<')?..).map(str::to_string);
    by_offsets
        .or_else(by_comments)
        .or_else(from_first_tag)
        .filter(|f| !f.trim().is_empty())
}

/// The whole document for `text/html` on systems that take it as is: the fragment with a charset
/// declaration, since a reader may not assume UTF-8.
pub fn html_document(fragment: &str) -> String {
    format!("<html><head><meta charset=\"utf-8\"></head><body>{fragment}</body></html>")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(s: &str, name: &str) -> usize {
        let at = s.find(name).unwrap() + name.len();
        s[at..at + 10].parse().unwrap()
    }

    #[test]
    fn offsets_point_at_the_fragment_in_bytes() {
        let fragment = "<p>caf\u{e9} \u{1f600} <b>x</b></p>";
        let s = cf_html(fragment);
        let (sh, eh) = (field(&s, "StartHTML:"), field(&s, "EndHTML:"));
        let (sf, ef) = (field(&s, "StartFragment:"), field(&s, "EndFragment:"));
        assert_eq!(&s.as_bytes()[sf..ef], fragment.as_bytes());
        assert_eq!(eh, s.len());
        assert!(s[sh..].starts_with("<html>"));
        assert!(s[..sf].ends_with("<!--StartFragment-->"));
        assert!(s[ef..].starts_with("<!--EndFragment-->"));
        assert!(s[sh..eh].ends_with("</html>"));
    }

    #[test]
    fn the_header_is_fixed_length_and_ordered() {
        let s = cf_html("");
        assert!(s.starts_with("Version:0.9\r\nStartHTML:0000000"));
        let (sh, eh) = (field(&s, "StartHTML:"), field(&s, "EndHTML:"));
        let (sf, ef) = (field(&s, "StartFragment:"), field(&s, "EndFragment:"));
        assert_eq!(sh, s.find("<html>").unwrap());
        assert!(sh < sf && sf == ef && ef < eh);
        assert_eq!(cf_html("a").len() - cf_html("").len(), 1);
    }

    #[test]
    fn the_fragment_comes_back_out() {
        let of = |s: &str| fragment(s.as_bytes());
        let piece = "<p>caf\u{e9} <b>x</b></p>";
        assert_eq!(of(&cf_html(piece)).as_deref(), Some(piece));
        // Offsets that do not hold: the comments, then everything from the first tag.
        let bad = "Version:0.9\r\nStartFragment:-1\r\nEndFragment:-1\r\n<html><!--StartFragment--><i>y</i><!--EndFragment--></html>";
        assert_eq!(of(bad).as_deref(), Some("<i>y</i>"));
        let bare = "Version:0.9\r\nStartFragment:999999\r\nEndFragment:5\r\n<b>z</b>";
        assert_eq!(of(bare).as_deref(), Some("<b>z</b>"));
        assert_eq!(of("Version:0.9\r\n"), None);
        assert_eq!(
            fragment(&[0xff, b'<', b'p', b'>', b'a']).as_deref(),
            Some("<p>a")
        );
        // Any cut of it: no panic.
        let whole = cf_html("<p>\u{1F600}</p>").into_bytes();
        for i in 0..whole.len() {
            let _ = fragment(&whole[..i]);
            let _ = fragment(&whole[i..]);
        }
    }

    #[test]
    fn a_document_declares_its_charset() {
        let d = html_document("<p>\u{e9}</p>");
        assert!(d.contains("<meta charset=\"utf-8\">"));
        assert!(d.contains("<body><p>\u{e9}</p></body>"));
    }
}
