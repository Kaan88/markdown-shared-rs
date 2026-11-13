//! Where the user was in each file: one position per path, so a file opened again comes back at
//! the place it was left.
//!
//! The store is bounded and ordered by use; the least recently used path is forgotten first. What
//! a position *is* belongs to the application: it is kept here as one opaque string (a viewer
//! writes a block and an offset, an editor a line and a column). The store goes into the settings
//! file as one line per path through [`Places::write`] and comes back through [`Places::read`],
//! so it is saved whenever the application saves its settings and shares their atomic write.
//!
//! Paths are compared as [`same_path`] does, so pass the canonical form
//! ([`canonical_or_absolute`](crate::paths::canonical_or_absolute)), as for the recent files.

use crate::paths::same_path;
use crate::settings::{Writer, decode_path, encode_path, escape, unescape};
use std::path::{Path, PathBuf};

/// How many files a store remembers unless told otherwise.
pub const MAX_PLACES: usize = 300;

/// Between the position and the path on a settings line. A position is escaped so it never
/// holds one; the path is everything after the first.
const SEPARATOR: char = '|';

/// Most recently used first, one entry per path, at most `limit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    entries: Vec<(PathBuf, String)>,
    limit: usize,
}

impl Default for Places {
    fn default() -> Self {
        Places::new(MAX_PLACES)
    }
}

impl Places {
    /// An empty store that keeps at most `limit` paths (at least one).
    pub fn new(limit: usize) -> Places {
        Places {
            entries: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Records `position` for `path` and makes it the most recently used.
    pub fn set(&mut self, path: impl Into<PathBuf>, position: impl Into<String>) {
        let path = path.into();
        self.entries.retain(|(p, _)| !same_path(p, &path));
        self.entries.insert(0, (path, position.into()));
        self.entries.truncate(self.limit);
    }

    /// The position recorded for `path`. Looking does not count as use.
    pub fn get(&self, path: &Path) -> Option<&str> {
        self.entries
            .iter()
            .find(|(p, _)| same_path(p, path))
            .map(|(_, position)| position.as_str())
    }

    /// Forgets `path`. True if it was known.
    pub fn remove(&mut self, path: &Path) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(p, _)| !same_path(p, path));
        self.entries.len() != before
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every path and its position, most recently used first.
    pub fn iter(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.entries
            .iter()
            .map(|(path, position)| (path.as_path(), position.as_str()))
    }

    /// Adds one `key=value` line per path to a settings file, most recently used first.
    pub fn write(&self, out: &mut Writer, key: &str) {
        for (path, position) in &self.entries {
            out.line(key, &encode(path, position));
        }
    }

    /// Takes the value of one line [`write`](Self::write) wrote. Lines are read in the file's
    /// order, so each one goes after those already read; a path seen before, a line past the
    /// limit and a line that is not a place are skipped. True if the line was taken.
    pub fn read(&mut self, value: &str) -> bool {
        let Some((path, position)) = decode(value) else {
            return false;
        };
        if self.entries.len() >= self.limit || self.entries.iter().any(|(p, _)| same_path(p, &path))
        {
            return false;
        }
        self.entries.push((path, position));
        true
    }
}

fn encode(path: &Path, position: &str) -> String {
    let position = escape(position).replace(SEPARATOR, "%7C");
    format!("{position}{SEPARATOR}{}", encode_path(path))
}

fn decode(value: &str) -> Option<(PathBuf, String)> {
    let (position, path) = value.split_once(SEPARATOR)?;
    if path.is_empty() {
        return None;
    }
    Some((decode_path(path), unescape(position)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::pairs;

    fn file(places: &Places) -> String {
        let mut out = Writer::new("app");
        out.line("zoom", "120");
        places.write(&mut out, "place");
        out.finish()
    }

    fn parse(text: &str, limit: usize) -> Places {
        let mut places = Places::new(limit);
        for (key, value) in pairs(text) {
            if key == "place" {
                places.read(value);
            }
        }
        places
    }

    #[test]
    fn the_last_position_set_for_a_path_is_the_one_kept() {
        let mut places = Places::default();
        assert!(places.is_empty());
        places.set("/a.md", "10");
        places.set("/b.md", "20");
        places.set("/a.md", "30");
        assert_eq!(places.len(), 2);
        assert_eq!(places.get(Path::new("/a.md")), Some("30"));
        assert_eq!(places.get(Path::new("/b.md")), Some("20"));
        assert_eq!(places.get(Path::new("/c.md")), None);
        let order: Vec<_> = places.iter().map(|(p, _)| p.to_path_buf()).collect();
        assert_eq!(order, [PathBuf::from("/a.md"), PathBuf::from("/b.md")]);
        assert!(places.remove(Path::new("/a.md")));
        assert!(!places.remove(Path::new("/a.md")));
        assert_eq!(places.len(), 1);
        places.clear();
        assert!(places.is_empty());
    }

    #[test]
    fn the_least_recently_used_path_is_forgotten_first() {
        let mut places = Places::new(3);
        for name in ["/1.md", "/2.md", "/3.md"] {
            places.set(name, name);
        }
        // Setting a known path again makes it the newest; looking at one does not.
        places.set("/1.md", "again");
        assert_eq!(places.get(Path::new("/2.md")), Some("/2.md"));
        places.set("/4.md", "four");
        assert_eq!(places.len(), 3);
        assert_eq!(places.get(Path::new("/2.md")), None, "the oldest went");
        assert_eq!(places.get(Path::new("/1.md")), Some("again"));
        assert_eq!(places.get(Path::new("/3.md")), Some("/3.md"));
        assert_eq!(Places::new(0).limit, 1);
        assert_eq!(Places::default().limit, MAX_PLACES);
    }

    #[test]
    fn paths_differing_in_case_are_one_file_on_windows_only() {
        let mut places = Places::default();
        places.set("/Docs/Read.md", "1");
        places.set("/docs/read.md", "2");
        assert_eq!(places.len(), if cfg!(windows) { 1 } else { 2 });
        assert_eq!(places.get(Path::new("/docs/read.md")), Some("2"));
    }

    #[test]
    fn the_settings_file_keeps_order_positions_and_awkward_names() {
        let mut places = Places::default();
        let odd = PathBuf::from("dir | with = 50% signs").join("a|b.md");
        places.set("/plain.md", "12:4");
        places.set(&odd, "block=7|offset=3.5\nheading=a%20b");
        places.set("/empty.md", "");
        let text = file(&places);
        assert_eq!(text.lines().count(), 5, "one line per path: {text}");
        let again = parse(&text, MAX_PLACES);
        assert_eq!(again, places);
        assert_eq!(
            again.get(&odd),
            Some("block=7|offset=3.5\nheading=a%20b"),
            "the position comes back byte for byte"
        );
        assert_eq!(again.get(Path::new("/empty.md")), Some(""));
        let order: Vec<_> = again.iter().map(|(p, _)| p.to_path_buf()).collect();
        assert_eq!(order[0], PathBuf::from("/empty.md"), "newest first");
    }

    #[test]
    fn reading_skips_damage_repeats_and_what_is_over_the_limit() {
        let mut places = Places::new(2);
        assert!(!places.read("no separator"));
        assert!(!places.read("position|"), "a line without a path");
        assert!(places.read("1|/a.md"));
        assert!(!places.read("2|/a.md"), "the first line for a path wins");
        assert!(places.read("|/b.md"));
        assert!(!places.read("3|/c.md"), "past the limit");
        assert_eq!(places.len(), 2);
        assert_eq!(places.get(Path::new("/a.md")), Some("1"));
        assert_eq!(places.get(Path::new("/b.md")), Some(""));
        // A file written with a larger limit is cut to this one, newest kept.
        let mut big = Places::new(10);
        for i in 0..10 {
            big.set(format!("/{i}.md"), i.to_string());
        }
        let cut = parse(&file(&big), 4);
        assert_eq!(cut.len(), 4);
        assert_eq!(cut.get(Path::new("/9.md")), Some("9"));
        assert_eq!(cut.get(Path::new("/5.md")), None);
    }
}
