//! A moved note's own links (D-091): what its relative Markdown links point at, who else points
//! there, and the note's text once it and the files it carries are where they go.
//!
//! Read: inline links and pictures (`[](…)`, `![](…)`) and reference definitions, outside fenced
//! code and code spans, as [`wikilink::scan`] finds them. Left alone: absolute paths, addresses
//! with a scheme, `#anchors`, wiki-links, HTML, and any destination that names nothing on disk or
//! a folder. Nothing read from a note can panic here: every cut is checked with `get`.

use super::ChangeKind;
use super::Edit;
use crate::paths::{canonical_or_absolute, same_path};
use crate::runs::{has_extension, read_regular};
use crate::wikilink::{self, LinkSyntax};
use std::path::{Component, Path, PathBuf};

/// The most bytes of a note, or of a file that may link to a picture, read for its links.
pub(super) const TEXT_MAX: u64 = 8 * 1024 * 1024;

/// The most files read to learn who else links to what. Past it, nothing is carried.
pub(super) const USERS_MAX: usize = 10_000;

/// Extensions said as "picture" on the card.
const PICTURES: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "avif", "tif", "tiff",
];

/// Whether `path` is a picture, for what a row says.
pub(super) fn is_picture(path: &Path) -> bool {
    has_extension(path, PICTURES)
}

/// One relative link of a note.
#[derive(Clone, Debug)]
pub(super) struct Link {
    /// The byte range of the destination's path (before any `?` or `#`), as written.
    pub start: usize,
    pub end: usize,
    /// Written between `<` and `>`, where a space needs no escape.
    pub angle: bool,
    /// What it names, resolved against the note's folder (not asked of the disk).
    pub target: PathBuf,
}

/// The note's text, when a move can rewrite it: a regular file, UTF-8, not past [`TEXT_MAX`].
pub(super) fn note_text(path: &Path) -> Option<String> {
    String::from_utf8(read_regular(path, TEXT_MAX, false)?).ok()
}

/// `rel` (decoded, `/`-separated) from `base`, part by part. `None` for a `..` past the root or
/// a part that would be a root or a drive of its own.
fn resolve(base: &Path, rel: &str) -> Option<PathBuf> {
    let mut out = base.to_path_buf();
    let seps: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };
    for part in rel.split(seps) {
        match part {
            "" | "." => {}
            ".." => {
                if !out.pop() {
                    return None;
                }
            }
            p => {
                let mut c = Path::new(p).components();
                match (c.next(), c.next()) {
                    (Some(Component::Normal(_)), None) => out.push(p),
                    _ => return None,
                }
            }
        }
    }
    Some(out)
}

/// The relative links of `text`, a note in the folder `base`, in the order they are written.
pub(super) fn links(text: &str, base: &Path) -> Vec<Link> {
    let mut out = Vec::new();
    wikilink::scan(text, |f| {
        if !matches!(f.syntax, LinkSyntax::Inline | LinkSyntax::Definition) {
            return true;
        }
        let start = f.offset;
        let Some(end) = start.checked_add(f.target.len()) else {
            return true;
        };
        if text.get(start..end) != Some(f.target) {
            return true;
        }
        let written = f.target;
        let path_len = written.find(['#', '?']).unwrap_or(written.len());
        let Some(path_w) = written.get(..path_len) else {
            return true;
        };
        if let Some((decoded, _)) = wikilink::relative_destination(path_w)
            && let Some(target) = resolve(base, &decoded)
        {
            let angle = start
                .checked_sub(1)
                .and_then(|b| text.as_bytes().get(b))
                .is_some_and(|&c| c == b'<');
            out.push(Link {
                start,
                end: start + path_len,
                angle,
                target,
            });
        }
        true
    });
    out
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| !m.is_dir())
}

/// Whether two paths are one file, asking the disk where their links lead.
pub(super) fn same_file(a: &Path, b: &Path) -> bool {
    same_path(a, b) || same_path(&canonical_or_absolute(a), &canonical_or_absolute(b))
}

/// `target` as a link from the folder `base` writes it: `../x/y.png`, with forward slashes. On
/// another drive, where no relative path leads, the absolute path. `None` for a name that is not
/// Unicode, which a link could not spell.
fn relative(base: &Path, target: &Path) -> Option<String> {
    let (b, t) = (canonical_or_absolute(base), canonical_or_absolute(target));
    let bc: Vec<Component> = b.components().collect();
    let tc: Vec<Component> = t.components().collect();
    let common = bc
        .iter()
        .zip(&tc)
        .take_while(|(x, y)| same_path(Path::new(x.as_os_str()), Path::new(y.as_os_str())))
        .count();
    let rooted = |c: &[Component]| {
        c.iter()
            .take_while(|p| matches!(p, Component::Prefix(_) | Component::RootDir))
            .count()
    };
    if common < rooted(&bc).max(1) || common < rooted(&tc).max(1) {
        // Nothing in common past the root: the absolute path.
        let s = t.to_str()?.replace('\\', "/");
        return Some(s);
    }
    let mut parts: Vec<&str> = vec![".."; bc.len() - common];
    for c in &tc[common..] {
        parts.push(c.as_os_str().to_str()?);
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// `path` spelled as a destination the way `written` was: the characters the source escaped
/// with `%` are escaped again (in its case of hex), a space and `<`/`>` are escaped outside angle
/// brackets, and what would end the path or the line always is.
fn spelled(path: &str, written: &str, angle: bool) -> String {
    let wb = written.as_bytes();
    let hex = |c: u8| c.is_ascii_hexdigit();
    let mut escaped_ascii: Vec<u8> = Vec::new();
    let mut escaped_other = false;
    let mut lower = false;
    let mut i = 0;
    while i < wb.len() {
        if wb[i] == b'%'
            && wb.get(i + 1).is_some_and(|&c| hex(c))
            && wb.get(i + 2).is_some_and(|&c| hex(c))
        {
            let pair = &written[i + 1..i + 3];
            lower |= pair.bytes().any(|c| c.is_ascii_lowercase());
            match u8::from_str_radix(pair, 16) {
                Ok(v) if v < 0x80 => escaped_ascii.push(v),
                Ok(_) => escaped_other = true,
                Err(_) => {}
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    let had_pct = !escaped_ascii.is_empty() || escaped_other;
    let pb = path.as_bytes();
    let balanced = {
        let mut depth = 0i64;
        let mut ok = true;
        for &c in pb {
            match c {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    ok &= depth >= 0;
                }
                _ => {}
            }
        }
        ok && depth == 0
    };
    let mut out = String::with_capacity(path.len());
    for (at, ch) in path.char_indices() {
        let pct_next = ch == '%'
            && pb.get(at + 1).is_some_and(|&c| hex(c))
            && pb.get(at + 2).is_some_and(|&c| hex(c));
        let must = ch.is_control()
            || matches!(ch, '#' | '?' | '<' | '>')
            || (!angle && ch == ' ')
            || (!angle && !balanced && matches!(ch, '(' | ')'))
            || (ch == '%' && (had_pct || pct_next))
            || (ch.is_ascii() && escaped_ascii.contains(&(ch as u8)))
            || (!ch.is_ascii() && escaped_other);
        if must {
            let mut buf = [0u8; 4];
            for b in ch.encode_utf8(&mut buf).bytes() {
                if lower {
                    out.push_str(&format!("%{b:02x}"));
                } else {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// The text with every relative link of a note that moved from the folder `before` to the
/// folder `after` leading where its file is now: `place` says where a file that was at a path
/// is now (itself when it did not move). A link whose file is not there, before or after, is
/// left as it is, and so is one that still leads to the right file. A line a record says this
/// move wrote (`prefer`, [`ChangeKind::Link`]) goes back to the exact words it had, when those
/// lead where the rewritten line does. Returns the text and how many links changed.
pub(super) fn rewrite(
    text: &str,
    before: &Path,
    after: &Path,
    place: &dyn Fn(&Path) -> PathBuf,
    prefer: &[Edit],
) -> (String, usize) {
    let mut found = links(text, before);
    found.sort_by_key(|l| l.start);
    let mut out = text.to_string();
    let mut n = 0usize;
    let mut limit = usize::MAX;
    for link in found.iter().rev() {
        if link.end > limit {
            continue;
        }
        let now = place(&link.target);
        if !exists(&link.target) && !exists(&now) {
            continue;
        }
        let Some(written) = text.get(link.start..link.end) else {
            continue;
        };
        let Some((decoded, _)) = wikilink::relative_destination(written) else {
            continue;
        };
        if resolve(after, &decoded).is_some_and(|p| same_file(&p, &now)) {
            continue;
        }
        let Some(rel) = relative(after, &now) else {
            continue;
        };
        let new = spelled(&rel, written, link.angle);
        if out.get(link.start..link.end).is_none() {
            continue;
        }
        out.replace_range(link.start..link.end, &new);
        limit = link.start;
        n += 1;
    }
    if !prefer.iter().any(|p| p.kind == ChangeKind::Link) {
        return (out, n);
    }
    // The words a line had, when it is one this move wrote and they lead where the new one does.
    // Also a line that needs no change to lead there: between two sibling folders
    // `../Inbox/a.png` reads the same from both, and undo still owes the words it had.
    let old_lines: Vec<&str> = text.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = out.split_inclusive('\n').collect();
    if old_lines.len() != new_lines.len() {
        return (out, n);
    }
    let targets = |line: &str| -> Vec<PathBuf> {
        links(line, after)
            .into_iter()
            .map(|l| canonical_or_absolute(&l.target))
            .collect()
    };
    let mut fixed = String::with_capacity(out.len());
    for (o, nl) in old_lines.iter().zip(&new_lines) {
        let ob = body(o);
        let nb = body(nl);
        if let Some(p) = prefer
            .iter()
            .find(|p| p.kind == ChangeKind::Link && p.after.trim_end() == ob.trim_end())
            && p.before != nb
        {
            let (a, b) = (targets(&p.before), targets(nb));
            if !b.is_empty() && a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| same_path(x, y))
            {
                if ob == nb {
                    n += 1;
                }
                fixed.push_str(&p.before);
                fixed.push_str(&nl[nb.len()..]);
                continue;
            }
        }
        fixed.push_str(nl);
    }
    (fixed, n)
}

fn body(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

/// The lines that differ between two texts of the same number of lines, as a record keeps them.
pub(super) fn edits(old: &str, new: &str) -> Vec<Edit> {
    old.split_inclusive('\n')
        .zip(new.split_inclusive('\n'))
        .filter(|(o, n)| body(o) != body(n))
        .map(|(o, n)| Edit {
            kind: ChangeKind::Link,
            sections: Vec::new(),
            before: body(o).to_string(),
            after: body(n).to_string(),
        })
        .collect()
}

/// Who links to what: every file read, its relative links by file name, and its wiki-links'
/// names. A wiki-link resolves by name, so a picture named by one is taken to be that file's.
pub(super) struct Users {
    files: Vec<PathBuf>,
    /// (lower-case file name, the path the link resolves to, the file it is in).
    links: Vec<(String, PathBuf, usize)>,
    /// (lower-case name a wiki-link gives, the file it is in).
    names: Vec<(String, usize)>,
    /// Every file could be read: without it, nothing is said to have one user.
    pub complete: bool,
}

fn lower_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

impl Users {
    /// Reads `files` (each once, a missing one passed over) for their links.
    pub fn read(files: &[PathBuf]) -> Users {
        let mut users = Users {
            files: Vec::new(),
            links: Vec::new(),
            names: Vec::new(),
            complete: true,
        };
        for file in files {
            if users.files.iter().any(|f| same_path(f, file)) {
                continue;
            }
            if users.files.len() >= USERS_MAX {
                users.complete = false;
                break;
            }
            let Ok(meta) = std::fs::metadata(file) else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            let Some(bytes) = read_regular(file, TEXT_MAX, true) else {
                users.complete = false;
                continue;
            };
            let text = String::from_utf8_lossy(&bytes);
            let at = users.files.len();
            users.files.push(file.clone());
            let base = file.parent().unwrap_or(Path::new(""));
            for l in links(&text, base) {
                users.links.push((lower_name(&l.target), l.target, at));
            }
            wikilink::scan(&text, |f| {
                if let LinkSyntax::Wiki { .. } = f.syntax
                    && let Some(t) = wikilink::parse_target(f.target)
                {
                    let name = t
                        .note
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_lowercase();
                    if !name.is_empty() {
                        users.names.push((name, at));
                    }
                }
                true
            });
        }
        users
    }

    /// Whether a file other than `note` links to `target`.
    pub fn others(&self, target: &Path, note: &Path) -> bool {
        let name = lower_name(target);
        let stem = target
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let not_note = |i: usize| self.files.get(i).is_some_and(|f| !same_file(f, note));
        self.names
            .iter()
            .any(|(n, i)| (*n == name || *n == stem) && not_note(*i))
            || self
                .links
                .iter()
                .any(|(n, p, i)| *n == name && not_note(*i) && same_file(p, target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_spelled_as_the_source_spelled_its_own() {
        assert_eq!(
            spelled("../pics/a b.png", "pics/a%20b.png", false),
            "../pics/a%20b.png"
        );
        assert_eq!(
            spelled("../pics/a b.png", "pics/a b.png", true),
            "../pics/a b.png"
        );
        assert_eq!(
            spelled("../pics/a b.png", "x.png", false),
            "../pics/a%20b.png"
        );
        assert_eq!(spelled("../é.png", "%c3%a9.png", false), "../%c3%a9.png");
        assert_eq!(spelled("../é.png", "é.png", false), "../é.png");
        assert_eq!(spelled("a#b?.png", "x", false), "a%23b%3F.png");
        assert_eq!(spelled("a(1).png", "x", false), "a(1).png");
        assert_eq!(spelled("a(.png", "x", false), "a%28.png");
    }

    #[test]
    fn hostile_destinations_do_not_panic() {
        let base = std::env::temp_dir();
        for text in [
            "[a](é)",
            "[a](<é>",
            "[a](%)",
            "[a](%zz%)",
            "[a](../../../../../../../../../../../../x.png)",
            "[a](C:x.png) [b](\u{0}) [c](a//b) [d](./)",
            "[x]: <é",
            "[x]: a](b.png)",
            "![](\u{202e}.png)",
        ] {
            let _ = links(text, &base);
            let _ = rewrite(text, &base, &base.join("x"), &|p| p.to_path_buf(), &[]);
        }
    }
}
