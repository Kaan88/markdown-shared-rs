//! Searching the text files of a folder: the walk, the lines that match, a cap on what is kept, and
//! a worker thread that streams the results. No window system and no Markdown. What counts as a
//! match is the application's: it passes a matcher that finds the matches in one line.
//!
//! The walk follows the Files tree of `mdshared-ui`: the same names are left out (a name starting
//! with a dot, the hidden attribute on Windows, `target` and `node_modules`), the files are the
//! ones the application's `shows_file` lists, and the order is the tree's (the folders of a folder
//! first, each sorted by name ignoring case, then its files). A link to a folder is not followed.
//!
//! [`search_tree`] runs on the calling thread and calls a sink for every event; [`Search`] runs it
//! on a thread of its own and keeps the events until the window takes them.

use crate::files::decode;
use crate::notes::skips_entry;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The most chars of a matching line that are kept.
pub const SNIPPET_MAX: usize = 200;
/// How many chars of context a cut line keeps before its first match.
const LEAD: usize = 40;
/// The deepest folder that is searched.
pub const MAX_DEPTH: usize = 32;
/// The most matches marked in one line.
pub const MARKS_PER_LINE: usize = 16;
/// The worker wakes the window at most this often while results come in.
const WAKE_EVERY: Duration = Duration::from_millis(40);
/// A `Progress` event is sent after this many files were read.
const PROGRESS_EVERY: usize = 16;
/// How often the lines of a very long file look at the cancel flag.
const CANCEL_EVERY: usize = 2048;

/// Finds the matches in one line of text: byte ranges into it, in order, at most as many as the
/// second argument says. The application's own find, so a search of a folder finds what a find in
/// the page would.
pub type Matcher<'a> = &'a dyn Fn(&str, usize) -> Vec<Range<usize>>;

/// What a search leaves out and where it stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// A file larger than this is skipped.
    pub max_file_bytes: u64,
    /// The search stops after this many matching lines in all.
    pub max_hits: usize,
    /// At most this many matching lines are kept of one file; the search goes on with the next.
    pub max_hits_per_file: usize,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            max_file_bytes: 16 << 20,
            max_hits: 2_000,
            max_hits_per_file: 200,
        }
    }
}

/// A matching line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// The line of the file, counting from 1.
    pub line: u32,
    /// The line, trimmed, and cut around its first match when it is long (a cut end is an ellipsis).
    pub text: String,
    /// Where the matches are in `text`: byte ranges on char boundaries.
    pub marks: Vec<Range<usize>>,
}

/// What a search reports as it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A file with at least one matching line. `rel` is its path below the folder searched, with
    /// `/` between the parts. `more` is true when the file had more matching lines than were kept.
    File {
        path: PathBuf,
        rel: String,
        hits: Vec<Hit>,
        more: bool,
    },
    /// Sent every so often: the files read so far.
    Progress { files: usize },
    /// The last event: files read and searched, files skipped (too large, binary, unreadable),
    /// whether it stopped at `max_hits` with more to show, and whether it was cancelled.
    Done {
        files: usize,
        skipped: usize,
        truncated: bool,
        cancelled: bool,
    },
}

fn byte_of_char(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(i, _)| i)
}

/// `line` trimmed, cut to [`SNIPPET_MAX`] chars around its first match, and the marks moved with it.
fn snippet(line: &str, marks: &[Range<usize>]) -> (String, Vec<Range<usize>>) {
    let lead = line.len() - line.trim_start().len();
    let text = line.trim();
    let marks: Vec<Range<usize>> = marks
        .iter()
        .filter_map(|m| {
            let (a, b) = (
                m.start.saturating_sub(lead).min(text.len()),
                m.end.saturating_sub(lead).min(text.len()),
            );
            (a < b && text.is_char_boundary(a) && text.is_char_boundary(b)).then_some(a..b)
        })
        .collect();
    if text.chars().count() <= SNIPPET_MAX {
        return (text.to_string(), marks);
    }
    let first = marks.first().map_or(0, |m| m.start);
    let first_char = text[..first].chars().count();
    let from = byte_of_char(text, first_char.saturating_sub(LEAD));
    let to = from + byte_of_char(&text[from..], SNIPPET_MAX);
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    let shift = out.len();
    out.push_str(&text[from..to]);
    if to < text.len() {
        out.push('…');
    }
    let marks = marks
        .into_iter()
        .filter(|m| m.start >= from && m.end <= to)
        .map(|m| m.start - from + shift..m.end - from + shift)
        .collect();
    (out, marks)
}

/// The matching lines of `text`, at most `cap`, and whether there was another beyond them.
fn scan(text: &str, matcher: Matcher, cap: usize, cancel: &AtomicBool) -> (Vec<Hit>, bool) {
    let mut hits = Vec::new();
    for (i, raw) in text.split('\n').enumerate() {
        if i % CANCEL_EVERY == CANCEL_EVERY - 1 && cancel.load(Ordering::Relaxed) {
            break;
        }
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim().is_empty() {
            continue;
        }
        let marks = matcher(line, MARKS_PER_LINE);
        if marks.is_empty() {
            continue;
        }
        if hits.len() >= cap {
            return (hits, true);
        }
        let (text, marks) = snippet(line, &marks);
        hits.push(Hit {
            line: i as u32 + 1,
            text,
            marks,
        });
    }
    (hits, false)
}

struct Walk<'a> {
    root: &'a Path,
    shows_file: &'a dyn Fn(&Path) -> bool,
    matcher: Matcher<'a>,
    opts: &'a Options,
    cancel: &'a AtomicBool,
    sink: &'a mut dyn FnMut(Event),
    files: usize,
    skipped: usize,
    hits: usize,
    truncated: bool,
    cancelled: bool,
}

impl Walk<'_> {
    fn stopped(&self) -> bool {
        self.truncated || self.cancelled
    }

    fn dir(&mut self, dir: &Path, depth: usize) {
        if self.cancel.load(Ordering::Relaxed) {
            self.cancelled = true;
            return;
        }
        let Ok(read) = std::fs::read_dir(dir) else {
            return;
        };
        let mut dirs: Vec<(String, PathBuf)> = Vec::new();
        let mut files: Vec<(String, PathBuf, u64)> = Vec::new();
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            let meta = if kind.is_symlink() {
                std::fs::metadata(&path)
            } else {
                entry.metadata()
            };
            let Ok(meta) = meta else {
                continue;
            };
            // A link to a folder is not followed, so the walk cannot loop; one to a file is read.
            if (kind.is_symlink() && meta.is_dir()) || skips_entry(&name, meta.is_dir(), &meta) {
                continue;
            }
            if meta.is_dir() {
                dirs.push((name, path));
            } else if meta.is_file() && (self.shows_file)(&path) {
                files.push((name, path, meta.len()));
            }
        }
        let key = |name: &str| (name.to_lowercase(), name.to_string());
        dirs.sort_by_key(|d| key(&d.0));
        files.sort_by_key(|f| key(&f.0));
        if depth < MAX_DEPTH {
            for (_, path) in dirs {
                self.dir(&path, depth + 1);
                if self.stopped() {
                    return;
                }
            }
        }
        for (_, path, size) in files {
            self.file(&path, size);
            if self.stopped() {
                return;
            }
        }
    }

    fn file(&mut self, path: &Path, size: u64) {
        if self.cancel.load(Ordering::Relaxed) {
            self.cancelled = true;
            return;
        }
        if size > self.opts.max_file_bytes {
            self.skipped += 1;
            return;
        }
        let Ok(bytes) = std::fs::read(path) else {
            self.skipped += 1;
            return;
        };
        let (text, _) = decode(&bytes);
        if text.contains('\0') {
            self.skipped += 1;
            return;
        }
        self.files += 1;
        // Once the total is reached the walk goes on only to learn whether there is more.
        let room = self.opts.max_hits.saturating_sub(self.hits);
        let cap = self.opts.max_hits_per_file.min(room);
        let (hits, more) = scan(&text, self.matcher, cap, self.cancel);
        if self.cancel.load(Ordering::Relaxed) {
            self.cancelled = true;
            return;
        }
        self.hits += hits.len();
        if more && cap == room {
            self.truncated = true;
        }
        if !hits.is_empty() {
            let rel = path
                .strip_prefix(self.root)
                .unwrap_or(path)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            (self.sink)(Event::File {
                path: path.to_path_buf(),
                rel,
                hits,
                more,
            });
        }
        if self.files.is_multiple_of(PROGRESS_EVERY) {
            (self.sink)(Event::Progress { files: self.files });
        }
    }
}

/// Searches the files under `root` and calls `sink` for every event on the calling thread; the
/// last event is always `Event::Done`. `cancel` is looked at between files and now and then in a
/// long one; a cancelled search ends with `cancelled` set. A file is skipped when it is larger
/// than `opts.max_file_bytes`, cannot be read, or holds a NUL (a binary file).
pub fn search_tree(
    root: &Path,
    shows_file: &dyn Fn(&Path) -> bool,
    matcher: Matcher,
    opts: &Options,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Event),
) {
    let mut walk = Walk {
        root,
        shows_file,
        matcher,
        opts,
        cancel,
        sink,
        files: 0,
        skipped: 0,
        hits: 0,
        truncated: false,
        cancelled: false,
    };
    walk.dir(root, 0);
    let done = Event::Done {
        files: walk.files,
        skipped: walk.skipped,
        truncated: walk.truncated,
        cancelled: walk.cancelled,
    };
    (walk.sink)(done);
}

/// A search running on a thread of its own. The events wait here until [`take`](Self::take); the
/// thread calls `wake` (at most every 40 ms, and at the end) so the window knows to take them.
/// Dropping the handle cancels the search.
pub struct Search {
    events: Arc<Mutex<Vec<Event>>>,
    cancel: Arc<AtomicBool>,
}

impl Search {
    /// Starts searching `root`. `matcher` takes the place of [`Matcher`] on the thread.
    pub fn start(
        root: PathBuf,
        shows_file: Box<dyn Fn(&Path) -> bool + Send>,
        matcher: Box<dyn Fn(&str, usize) -> Vec<Range<usize>> + Send>,
        opts: Options,
        wake: impl Fn() + Send + 'static,
    ) -> Search {
        let events = Arc::new(Mutex::new(Vec::new()));
        let cancel = Arc::new(AtomicBool::new(false));
        let (shared, flag) = (events.clone(), cancel.clone());
        let failed = events.clone();
        let spawned = std::thread::Builder::new()
            .name("folder-search".into())
            .spawn(move || {
                let mut last: Option<Instant> = None;
                let mut sink = |event: Event| {
                    let end = matches!(event, Event::Done { .. });
                    shared.lock().unwrap_or_else(|e| e.into_inner()).push(event);
                    if end || last.is_none_or(|t| t.elapsed() >= WAKE_EVERY) {
                        last = Some(Instant::now());
                        wake();
                    }
                };
                search_tree(&root, &*shows_file, &*matcher, &opts, &flag, &mut sink);
            });
        if spawned.is_err() {
            failed
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(Event::Done {
                    files: 0,
                    skipped: 0,
                    truncated: false,
                    cancelled: false,
                });
        }
        Search { events, cancel }
    }

    /// What has come in since the last call, in order.
    pub fn take(&self) -> Vec<Event> {
        std::mem::take(&mut *self.events.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Asks the thread to stop; its last event says it was cancelled.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A folder of its own under the temporary folder, removed when dropped.
    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Dir {
            let dir = std::env::temp_dir().join(format!(
                "mdshared-foldersearch-{name}-{}",
                std::process::id()
            ));
            _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Dir(dir)
        }

        fn write(&self, rel: &str, text: &str) -> PathBuf {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
            path
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Case-insensitive ASCII find of `needle`.
    fn find(needle: &'static str) -> impl Fn(&str, usize) -> Vec<Range<usize>> {
        move |line: &str, cap: usize| {
            let (hay, n) = (line.to_ascii_lowercase(), needle.to_ascii_lowercase());
            hay.match_indices(&n)
                .take(cap)
                .map(|(i, m)| i..i + m.len())
                .collect()
        }
    }

    fn md(path: &Path) -> bool {
        path.extension().is_some_and(|e| e == "md")
    }

    fn run(root: &Path, needle: &'static str, opts: &Options) -> Vec<Event> {
        let mut events = Vec::new();
        let matcher = find(needle);
        search_tree(
            root,
            &md,
            &matcher,
            opts,
            &AtomicBool::new(false),
            &mut |e| events.push(e),
        );
        events
    }

    fn files(events: &[Event]) -> Vec<(String, Vec<u32>)> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::File { rel, hits, .. } => {
                    Some((rel.clone(), hits.iter().map(|h| h.line).collect()))
                }
                _ => None,
            })
            .collect()
    }

    fn done(events: &[Event]) -> (usize, usize, bool, bool) {
        match events.last() {
            Some(Event::Done {
                files,
                skipped,
                truncated,
                cancelled,
            }) => (*files, *skipped, *truncated, *cancelled),
            other => panic!("no Done at the end: {other:?}"),
        }
    }

    #[test]
    fn finds_lines_with_numbers_and_marks_in_the_trees_order() {
        let d = Dir::new("order");
        d.write("b.md", "one\nA Needle here\n\nneedle needle\n");
        d.write("a.md", "none\n");
        d.write("Zed/z.md", "the needle\r\nnext\r\n");
        d.write("alpha/n.md", "needle");
        d.write("c.txt", "needle\n");
        let events = run(&d.0, "needle", &Options::default());
        // Folders first (by name, ignoring case), then the files; only the .md files.
        assert_eq!(
            files(&events),
            vec![
                ("alpha/n.md".to_string(), vec![1]),
                ("Zed/z.md".to_string(), vec![1]),
                ("b.md".to_string(), vec![2, 4]),
            ]
        );
        assert_eq!(done(&events), (4, 0, false, false));
        let Event::File { hits, .. } = &events[1] else {
            panic!("second event is a file");
        };
        assert_eq!(hits[0].text, "the needle");
        assert_eq!(hits[0].marks, vec![4..10]);
        let Event::File { hits, path, .. } = &events[2] else {
            panic!("third event is a file");
        };
        assert_eq!(path, &d.0.join("b.md"));
        assert_eq!(hits[1].marks, vec![0..6, 7..13]);
    }

    #[test]
    fn leaves_out_what_the_tree_leaves_out() {
        let d = Dir::new("ignore");
        d.write(".hidden/x.md", "needle");
        d.write("node_modules/p/readme.md", "needle");
        d.write("target/t.md", "needle");
        d.write(".dot.md", "needle");
        d.write("ok/ok.md", "needle");
        let events = run(&d.0, "needle", &Options::default());
        assert_eq!(files(&events), vec![("ok/ok.md".to_string(), vec![1])]);
    }

    #[test]
    fn skips_big_and_binary_files_and_counts_them() {
        let d = Dir::new("skip");
        d.write("big.md", &"needle\n".repeat(100));
        d.write("bin.md", "needle\0\n");
        d.write("ok.md", "needle\n");
        let opts = Options {
            max_file_bytes: 100,
            ..Options::default()
        };
        let events = run(&d.0, "needle", &opts);
        assert_eq!(files(&events), vec![("ok.md".to_string(), vec![1])]);
        assert_eq!(done(&events), (1, 2, false, false));
    }

    #[test]
    fn reads_utf16_files_by_their_mark() {
        let d = Dir::new("utf16");
        let mut bytes = vec![0xFF, 0xFE];
        for u in "hello needle\n".encode_utf16() {
            bytes.extend(u.to_le_bytes());
        }
        let path = d.0.join("w.md");
        fs::write(&path, bytes).unwrap();
        let events = run(&d.0, "needle", &Options::default());
        assert_eq!(files(&events), vec![("w.md".to_string(), vec![1])]);
    }

    #[test]
    fn a_file_keeps_at_most_its_share_and_says_there_was_more() {
        let d = Dir::new("per-file");
        d.write("a.md", &"needle\n".repeat(10));
        d.write("b.md", "needle\n");
        let opts = Options {
            max_hits_per_file: 3,
            ..Options::default()
        };
        let events = run(&d.0, "needle", &opts);
        assert_eq!(
            files(&events),
            vec![
                ("a.md".to_string(), vec![1, 2, 3]),
                ("b.md".to_string(), vec![1])
            ]
        );
        let Event::File { more, .. } = &events[0] else {
            panic!("a file first");
        };
        assert!(*more);
        // The total was not reached.
        assert!(!done(&events).2);
    }

    #[test]
    fn stops_at_the_total_only_when_there_is_more_to_show() {
        let d = Dir::new("total");
        d.write("a.md", "needle\nneedle\n");
        d.write("b.md", "nothing\n");
        let opts = Options {
            max_hits: 2,
            ..Options::default()
        };
        // Exactly 2 matches: nothing is cut.
        let events = run(&d.0, "needle", &opts);
        assert_eq!(files(&events), vec![("a.md".to_string(), vec![1, 2])]);
        assert_eq!(done(&events), (2, 0, false, false));
        d.write("c.md", "a needle\n");
        let events = run(&d.0, "needle", &opts);
        assert_eq!(files(&events), vec![("a.md".to_string(), vec![1, 2])]);
        assert!(done(&events).2, "c.md has one more");
    }

    #[test]
    fn a_cancelled_search_says_so() {
        let d = Dir::new("cancel");
        d.write("a.md", "needle\n");
        let cancel = AtomicBool::new(true);
        let mut events = Vec::new();
        let matcher = find("needle");
        search_tree(
            &d.0,
            &md,
            &matcher,
            &Options::default(),
            &cancel,
            &mut |e| events.push(e),
        );
        assert_eq!(events.len(), 1);
        assert_eq!(done(&events), (0, 0, false, true));
    }

    #[test]
    fn long_lines_are_cut_around_the_first_match() {
        let line = format!("{}needle{}", "é".repeat(150), "x".repeat(300));
        let (text, marks) = snippet(&line, &[300..306]);
        assert!(text.starts_with('…') && text.ends_with('…'));
        assert_eq!(&text[marks[0].clone()], "needle");
        assert!(text.chars().count() <= SNIPPET_MAX + 2);
        // A match at the start is not cut at its front.
        let line = format!("needle{}", "y".repeat(400));
        let (text, marks) = snippet(&line, &[0..6]);
        assert!(!text.starts_with('…') && text.ends_with('…'));
        assert_eq!(marks, vec![0..6]);
    }

    #[test]
    fn leading_whitespace_is_trimmed_and_the_marks_move() {
        let (text, marks) = snippet("    - a needle ", &[8..14]);
        assert_eq!(text, "- a needle");
        assert_eq!(&text[marks[0].clone()], "needle");
    }

    #[test]
    fn the_thread_hands_over_events_and_wakes() {
        let d = Dir::new("thread");
        d.write("a.md", "needle\n");
        let woken = Arc::new(AtomicBool::new(false));
        let w = woken.clone();
        let search = Search::start(
            d.0.clone(),
            Box::new(md),
            Box::new(find("needle")),
            Options::default(),
            move || w.store(true, Ordering::Relaxed),
        );
        let mut events = Vec::new();
        let start = Instant::now();
        while !events.iter().any(|e| matches!(e, Event::Done { .. })) {
            assert!(start.elapsed() < Duration::from_secs(20), "no end");
            events.extend(search.take());
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(woken.load(Ordering::Relaxed));
        assert_eq!(files(&events), vec![("a.md".to_string(), vec![1])]);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_folder_is_not_followed() {
        let d = Dir::new("link");
        d.write("real/r.md", "needle");
        std::os::unix::fs::symlink(d.0.join("real"), d.0.join("link")).unwrap();
        let events = run(&d.0, "needle", &Options::default());
        assert_eq!(files(&events), vec![("real/r.md".to_string(), vec![1])]);
    }
}
