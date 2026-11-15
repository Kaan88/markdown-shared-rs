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
    fn a_document_declares_its_charset() {
        let d = html_document("<p>\u{e9}</p>");
        assert!(d.contains("<meta charset=\"utf-8\">"));
        assert!(d.contains("<body><p>\u{e9}</p></body>"));
    }
}
