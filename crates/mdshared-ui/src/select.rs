//! Which rows of a list are selected, and how clicks and keys change that: one row, several
//! with Ctrl, a run with Shift; and finding a row by typing the start of its name. State and
//! arithmetic only. It draws nothing, holds no rows and no labels (rows are indices, as in
//! [`rows`](crate::rows) and [`grid`](crate::grid)), and reads no clock: the caller passes the
//! time of a key.
//!
//! The rules are the ones of a Windows file list, which the other systems share in all that
//! matters here:
//!
//! - a click selects the row alone; with Ctrl it adds or removes the row and leaves the rest;
//!   with Shift it selects from the anchor to the row, in place of what was selected; with both
//!   it adds that run to what was selected;
//! - an arrow key moves the cursor and selects the row alone; with Shift it selects from the
//!   anchor to the new row;
//! - the anchor is where the last plain or Ctrl click, or the last unshifted key, landed.
//!
//! Deliberately not here: drag and drop, the rubber band's rectangle (that is geometry:
//! [`Rows::band`](crate::rows::Rows::band) and [`Grid::band`](crate::grid::Grid::band) give the
//! rows under one, and [`Selection::set_band`] takes them), and any notion of what a row is.

use std::ops::Range;
use std::time::{Duration, Instant};

use crate::keys::Mods;
use crate::menu::same_letter;
use crate::rows::{Nav, Rows};

/// The selected rows of one list, the cursor (the row the keyboard is on, drawn with the focus
/// mark) and the anchor (the fixed end of a Shift run).
///
/// The rows are kept as sorted runs, so "select all" of a hundred thousand rows is one run and
/// asking whether a row is selected is a binary search.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    anchor: Option<usize>,
    cursor: Option<usize>,
    /// Sorted, not empty, not touching: two runs that meet are one.
    runs: Vec<Range<usize>>,
}

impl Selection {
    /// Nothing selected, no cursor.
    pub const fn new() -> Selection {
        Selection {
            anchor: None,
            cursor: None,
            runs: Vec::new(),
        }
    }

    /// The row the keyboard is on.
    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    /// The fixed end of a Shift run.
    pub fn anchor(&self) -> Option<usize> {
        self.anchor
    }

    /// Whether row `i` is selected.
    pub fn contains(&self, i: usize) -> bool {
        self.runs
            .binary_search_by(|r| {
                if r.end <= i {
                    std::cmp::Ordering::Less
                } else if r.start > i {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
    }

    /// The selected rows as runs, in order: each from its first row to one past its last, none
    /// empty, no two touching. For drawing (a run of rows can share one rounded background)
    /// and for acting on the selection without walking every row.
    pub fn ranges(&self) -> &[Range<usize>] {
        &self.runs
    }

    /// The selected rows one by one, in order.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.runs.iter().flat_map(|r| r.clone())
    }

    /// How many rows are selected.
    pub fn len(&self) -> usize {
        self.runs.iter().map(|r| r.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// The first selected row.
    pub fn first(&self) -> Option<usize> {
        self.runs.first().map(|r| r.start)
    }

    /// The one selected row, when exactly one is: what "rename" and "open" act on.
    pub fn single(&self) -> Option<usize> {
        match self.runs.as_slice() {
            [r] if r.len() == 1 => Some(r.start),
            _ => None,
        }
    }

    /// Selects nothing. The cursor and the anchor stay: the keyboard goes on from where it was.
    pub fn clear(&mut self) {
        self.runs.clear();
    }

    /// Selects nothing and forgets the cursor and the anchor: for a list that shows other rows
    /// now (another folder).
    pub fn reset(&mut self) {
        *self = Selection::new();
    }

    /// Every row of a list of `count`. The cursor stays (or goes to the first row if there was
    /// none), and so does the anchor.
    pub fn select_all(&mut self, count: usize) {
        self.runs.clear();
        if count > 0 {
            self.runs.push(0..count);
            self.cursor = Some(self.cursor.map_or(0, |c| c.min(count - 1)));
            self.anchor = self.anchor.or(self.cursor);
        }
    }

    /// A click on row `i` with these modifiers held (`ctrl` is the primary modifier, Command
    /// on a Mac, as [`Mods`] has it): the rules at the top of the module. The cursor goes to
    /// `i`. A press on empty space below the rows is not a click on a row: the caller calls
    /// [`clear`](Self::clear) for that, unless Ctrl or Shift is held.
    pub fn click(&mut self, i: usize, mods: Mods) {
        match (mods.ctrl, mods.shift) {
            (false, false) => self.select_only(i),
            (true, false) => {
                self.toggle(i);
                self.anchor = Some(i);
            }
            (ctrl, true) => {
                let anchor = self.anchor.or(self.cursor).unwrap_or(i);
                if !ctrl {
                    self.runs.clear();
                }
                self.insert(anchor.min(i)..anchor.max(i) + 1);
                self.anchor = Some(anchor);
            }
        }
        self.cursor = Some(i);
    }

    /// A press of the secondary button on row `i`, before its menu opens: a row that is already
    /// selected keeps the whole selection (the menu acts on all of it), any other row is
    /// selected alone.
    pub fn context(&mut self, i: usize) {
        if self.contains(i) {
            self.cursor = Some(i);
        } else {
            self.select_only(i);
        }
    }

    /// Row `i` alone, with the cursor and the anchor on it.
    pub fn select_only(&mut self, i: usize) {
        self.runs.clear();
        self.runs.push(i..i + 1);
        self.cursor = Some(i);
        self.anchor = Some(i);
    }

    /// Adds row `i` if it is not selected and removes it if it is; the cursor goes to it. What
    /// Ctrl+Space does to the row under the cursor.
    pub fn toggle(&mut self, i: usize) {
        if self.contains(i) {
            self.remove(i);
        } else {
            self.insert(i..i + 1);
        }
        self.cursor = Some(i);
    }

    /// Moves the cursor to row `i` and selects nothing new: what Ctrl with an arrow key does,
    /// so that rows far apart can be collected with Ctrl+Space.
    pub fn set_cursor(&mut self, i: usize) {
        self.cursor = Some(i);
    }

    /// The cursor moved to row `i` by a key: selected alone, or, when `extend` (Shift is
    /// held), the run from the anchor to it in place of what was selected. A grid or any list
    /// that finds the new row itself calls this; a plain list calls [`nav`](Self::nav).
    pub fn move_to(&mut self, i: usize, extend: bool) {
        if extend {
            let anchor = self.anchor.or(self.cursor).unwrap_or(i);
            self.runs.clear();
            self.runs.push(anchor.min(i)..anchor.max(i) + 1);
            self.anchor = Some(anchor);
            self.cursor = Some(i);
        } else {
            self.select_only(i);
        }
    }

    /// An arrow, Home, End or page key on the list `rows`: the cursor moves as
    /// [`Rows::nav`] says and the selection follows as [`move_to`](Self::move_to) says.
    /// Returns the new cursor, for the caller to bring into view
    /// ([`Rows::reveal`]), or `None` for an empty list, where nothing changes.
    pub fn nav(&mut self, rows: &Rows, nav: Nav, extend: bool) -> Option<usize> {
        let to = rows.nav(self.cursor, nav)?;
        self.move_to(to, extend);
        Some(to)
    }

    /// The rows under a rubber band: `rows` (any order, repeats allowed) are selected in place
    /// of what was, or, when `add` (Ctrl is held), on top of `base`, the selection as it was
    /// when the band began, which the caller kept. The cursor goes to the last row given.
    pub fn set_band(
        &mut self,
        rows: impl IntoIterator<Item = usize>,
        base: Option<&Selection>,
        add: bool,
    ) {
        self.runs.clear();
        if let Some(base) = base.filter(|_| add) {
            self.runs.clone_from(&base.runs);
        }
        let mut last = None;
        for i in rows {
            self.insert(i..i + 1);
            last = Some(i);
        }
        if last.is_some() {
            self.cursor = last;
            self.anchor = self.anchor.or(last);
        }
    }

    /// Rewrites every row through `new_of` after the list changed under the selection (it was
    /// sorted another way, rows were deleted or inserted): `Some(new)` is where an old row is
    /// now, `None` says it is gone. Selected rows that are gone are dropped; a cursor or anchor
    /// that is gone is forgotten. The cost is one call per selected row, not per row of the
    /// list.
    pub fn remap(&mut self, new_of: impl Fn(usize) -> Option<usize>) {
        let mut rows: Vec<usize> = self.iter().filter_map(&new_of).collect();
        rows.sort_unstable();
        rows.dedup();
        self.runs.clear();
        for i in rows {
            match self.runs.last_mut() {
                Some(last) if last.end == i => last.end = i + 1,
                _ => self.runs.push(i..i + 1),
            }
        }
        self.cursor = self.cursor.and_then(&new_of);
        self.anchor = self.anchor.and_then(&new_of);
    }

    /// Drops what lies past a list of `count` rows: for a list that only got shorter at its
    /// end. A cursor past the end goes to the last row.
    pub fn truncate(&mut self, count: usize) {
        self.runs.retain_mut(|r| {
            r.end = r.end.min(count);
            r.start < r.end
        });
        let keep = |at: Option<usize>| match (at, count) {
            (_, 0) => None,
            (Some(i), _) => Some(i.min(count - 1)),
            (None, _) => None,
        };
        self.cursor = keep(self.cursor);
        self.anchor = keep(self.anchor);
    }

    /// Adds a run, joining it with the runs it touches.
    fn insert(&mut self, run: Range<usize>) {
        if run.is_empty() {
            return;
        }
        // The runs that end before this one starts stay in front; those that start after it
        // ends stay behind; everything between is swallowed.
        let from = self.runs.partition_point(|r| r.end < run.start);
        let to = self.runs.partition_point(|r| r.start <= run.end);
        let start = self
            .runs
            .get(from)
            .map_or(run.start, |r| r.start.min(run.start));
        let end = if to > from {
            self.runs[to - 1].end.max(run.end)
        } else {
            run.end
        };
        self.runs.splice(from..to, std::iter::once(start..end));
    }

    /// Removes one row, splitting its run if it was inside one.
    fn remove(&mut self, i: usize) {
        let Some(k) = self.runs.iter().position(|r| r.contains(&i)) else {
            return;
        };
        let r = self.runs[k].clone();
        let mut parts = Vec::with_capacity(2);
        if r.start < i {
            parts.push(r.start..i);
        }
        if i + 1 < r.end {
            parts.push(i + 1..r.end);
        }
        self.runs.splice(k..k + 1, parts);
    }
}

/// Finding a row by typing: the letters typed in quick succession are the start of a name, and
/// the same letter typed again and again steps through the names that start with it.
///
/// It keeps the letters and the time of the last one; the caller passes each key with its time
/// and a way to read the label of a row, and moves its cursor to the row returned (usually
/// with [`Selection::select_only`]).
#[derive(Clone, Debug)]
pub struct TypeAhead {
    typed: String,
    last: Option<Instant>,
    pause: Duration,
}

impl Default for TypeAhead {
    fn default() -> TypeAhead {
        TypeAhead::new()
    }
}

impl TypeAhead {
    /// The pause after which the next letter starts a new word: one second, as in Windows.
    pub const PAUSE: Duration = Duration::from_secs(1);

    pub const fn new() -> TypeAhead {
        TypeAhead {
            typed: String::new(),
            last: None,
            pause: TypeAhead::PAUSE,
        }
    }

    /// The same with another pause.
    pub const fn with_pause(mut self, pause: Duration) -> TypeAhead {
        self.pause = pause;
        self
    }

    /// What has been typed and still counts, for a caller that shows it.
    pub fn typed(&self) -> &str {
        &self.typed
    }

    /// Forgets what was typed: when the list changes or loses the keyboard.
    pub fn reset(&mut self) {
        self.typed.clear();
        self.last = None;
    }

    /// The key `ch` typed at `now` in a list of `count` rows whose cursor is on `cursor`;
    /// `label(i)` is the text of row `i`. Returns the row to go to, or `None` when no label
    /// matches (the cursor stays) or the key is not one that types (a control character; a
    /// space that starts a word, which belongs to the list's own keys).
    ///
    /// - After a pause longer than [`PAUSE`](Self::PAUSE) the letters start anew.
    /// - One letter, or the same letter several times: the next row after the cursor whose
    ///   label starts with it, round the end of the list to its start. So `s`, `s`, `s` walks
    ///   the names in S.
    /// - Otherwise the letters are the start of a label: the first match from the cursor on,
    ///   the cursor's own row included, so typing on does not leave a row that still matches.
    ///
    /// Letters compare as the access keys of menus do ([`same_letter`]): equal, or equal in
    /// Unicode lower case. Each `label` is read at most once per key.
    pub fn key<S: AsRef<str>>(
        &mut self,
        ch: char,
        now: Instant,
        count: usize,
        cursor: Option<usize>,
        label: impl Fn(usize) -> S,
    ) -> Option<usize> {
        let stale = self
            .last
            .is_none_or(|last| now.saturating_duration_since(last) > self.pause);
        if stale {
            self.typed.clear();
        }
        if ch.is_control() || (ch == ' ' && self.typed.is_empty()) {
            return None;
        }
        self.last = Some(now);
        self.typed.push(ch);
        if count == 0 {
            return None;
        }
        let first = self.typed.chars().next()?;
        let repeated = self.typed.chars().all(|c| same_letter(first, c));
        // One letter over and over steps on from the cursor; a word starts at the cursor.
        let (word, skip) = if repeated {
            (first.to_string(), 1)
        } else {
            (self.typed.clone(), 0)
        };
        let starts = |text: &str| {
            let mut text = text.chars();
            word.chars()
                .all(|w| text.next().is_some_and(|t| same_letter(t, w)))
        };
        let from = match cursor {
            Some(c) => c.min(count - 1) + skip,
            None => 0,
        };
        (0..count)
            .map(|k| (from + k) % count)
            .find(|&i| starts(label(i).as_ref()))
    }
}

#[cfg(test)]
// The runs of a selection are ranges, and many of the expected values below are one run.
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;

    const PLAIN: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
        control: false,
    };
    const CTRL: Mods = Mods {
        ctrl: true,
        ..PLAIN
    };
    const SHIFT: Mods = Mods {
        shift: true,
        ..PLAIN
    };
    const BOTH: Mods = Mods {
        ctrl: true,
        ..SHIFT
    };

    fn rows_of(s: &Selection) -> Vec<usize> {
        s.iter().collect()
    }

    #[test]
    fn clicks_follow_the_rules_of_a_file_list() {
        let mut s = Selection::new();
        assert!(s.is_empty() && s.cursor().is_none() && s.single().is_none());
        s.click(3, PLAIN);
        assert_eq!(
            (rows_of(&s), s.cursor(), s.anchor()),
            (vec![3], Some(3), Some(3))
        );
        assert_eq!(s.single(), Some(3));
        // Shift: from the anchor to the row, either way, in place of what was.
        s.click(6, SHIFT);
        assert_eq!(rows_of(&s), vec![3, 4, 5, 6]);
        assert_eq!((s.cursor(), s.anchor()), (Some(6), Some(3)));
        s.click(1, SHIFT);
        assert_eq!(rows_of(&s), vec![1, 2, 3]);
        assert_eq!(s.ranges(), &[1..4]);
        // Ctrl: one row more, or one less, and the anchor moves there.
        s.click(8, CTRL);
        assert_eq!(rows_of(&s), vec![1, 2, 3, 8]);
        assert_eq!(s.anchor(), Some(8));
        s.click(2, CTRL);
        assert_eq!(rows_of(&s), vec![1, 3, 8]);
        assert_eq!(s.ranges(), &[1..2, 3..4, 8..9]);
        assert_eq!((s.len(), s.first(), s.single()), (3, Some(1), None));
        // Ctrl+Shift: the run from the anchor is added to what is there.
        s.click(5, BOTH);
        assert_eq!(rows_of(&s), vec![1, 2, 3, 4, 5, 8]);
        assert_eq!((s.cursor(), s.anchor()), (Some(5), Some(2)));
        // A plain click is the row alone again.
        s.click(0, PLAIN);
        assert_eq!(s.ranges(), &[0..1]);
        // Shift with nothing to start from starts at the row.
        let mut fresh = Selection::new();
        fresh.click(4, SHIFT);
        assert_eq!((rows_of(&fresh), fresh.anchor()), (vec![4], Some(4)));
    }

    #[test]
    fn the_secondary_button_keeps_a_selection_it_lands_in() {
        let mut s = Selection::new();
        s.click(2, PLAIN);
        s.click(5, SHIFT);
        s.context(4);
        assert_eq!((rows_of(&s), s.cursor()), (vec![2, 3, 4, 5], Some(4)));
        s.context(9);
        assert_eq!(
            (rows_of(&s), s.cursor(), s.anchor()),
            (vec![9], Some(9), Some(9))
        );
    }

    #[test]
    fn keys_move_the_cursor_and_shift_extends_from_the_anchor() {
        let rows = Rows::new(20.0, 100.0, 30);
        let mut s = Selection::new();
        // No cursor: Down starts at the top.
        assert_eq!(s.nav(&rows, Nav::Down, false), Some(0));
        assert_eq!(s.nav(&rows, Nav::Down, false), Some(1));
        assert_eq!(rows_of(&s), vec![1]);
        assert_eq!(s.nav(&rows, Nav::Down, true), Some(2));
        assert_eq!(s.nav(&rows, Nav::PageDown, true), Some(7));
        assert_eq!(s.ranges(), &[1..8]);
        // Back past the anchor: the run turns round it.
        assert_eq!(s.nav(&rows, Nav::Home, true), Some(0));
        assert_eq!((s.ranges(), s.anchor()), (&[0..2][..], Some(1)));
        // Without Shift the row is alone and the anchor is there.
        assert_eq!(s.nav(&rows, Nav::End, false), Some(29));
        assert_eq!((s.ranges(), s.anchor()), (&[29..30][..], Some(29)));
        // Ctrl with an arrow moves the cursor only; Ctrl+Space collects.
        s.set_cursor(10);
        assert_eq!((rows_of(&s), s.cursor()), (vec![29], Some(10)));
        s.toggle(10);
        s.toggle(12);
        s.toggle(29);
        assert_eq!(rows_of(&s), vec![10, 12]);
        // An empty list answers nothing and changes nothing.
        let before = s.clone();
        assert_eq!(s.nav(&Rows::new(20.0, 100.0, 0), Nav::Down, false), None);
        assert_eq!(s, before);
    }

    #[test]
    fn select_all_clear_and_truncate() {
        let mut s = Selection::new();
        s.select_all(0);
        assert!(s.is_empty() && s.cursor().is_none());
        s.select_all(100_000);
        assert_eq!(
            (s.len(), s.ranges(), s.cursor()),
            (100_000, &[0..100_000][..], Some(0))
        );
        assert!(s.contains(0) && s.contains(99_999) && !s.contains(100_000));
        s.click(50, CTRL);
        assert_eq!(s.ranges(), &[0..50, 51..100_000]);
        assert!(!s.contains(50) && s.contains(49) && s.contains(51));
        s.truncate(60);
        assert_eq!((s.ranges(), s.cursor()), (&[0..50, 51..60][..], Some(50)));
        s.set_cursor(59);
        s.truncate(40);
        assert_eq!(
            (s.ranges(), s.cursor(), s.anchor()),
            (&[0..40][..], Some(39), Some(39))
        );
        s.clear();
        assert!(s.is_empty());
        assert_eq!(
            s.cursor(),
            Some(39),
            "the keyboard goes on from where it was"
        );
        s.select_all(40);
        s.truncate(0);
        assert_eq!(s, Selection::new());
        s.select_only(3);
        s.reset();
        assert_eq!(s, Selection::default());
    }

    #[test]
    fn runs_join_and_split_whatever_the_order() {
        let mut s = Selection::new();
        for i in [5, 1, 3, 9, 2, 4, 7, 8, 0, 6] {
            s.click(i, CTRL);
        }
        assert_eq!(s.ranges(), &[0..10]);
        for i in [0, 9, 4, 5] {
            s.click(i, CTRL);
        }
        assert_eq!(s.ranges(), &[1..4, 6..9]);
        // A run laid over several others swallows them and what touches it.
        s.click(20, CTRL);
        s.click(3, CTRL);
        s.click(19, BOTH);
        assert_eq!(s.ranges(), &[1..21]);
        // Against a plain set of rows, over many pseudo-random clicks.
        let mut s = Selection::new();
        let mut set = std::collections::BTreeSet::new();
        let mut seed: u32 = 12345;
        for _ in 0..3000 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let i = (seed >> 16) as usize % 40;
            s.click(i, CTRL);
            if !set.insert(i) {
                set.remove(&i);
            }
            assert!(s.iter().eq(set.iter().copied()));
            assert!(s.ranges().windows(2).all(|w| w[0].end < w[1].start));
            assert!(s.ranges().iter().all(|r| r.start < r.end));
            assert_eq!(s.contains(i), set.contains(&i));
        }
    }

    #[test]
    fn a_rubber_band_replaces_or_adds_to_what_was_selected() {
        let mut s = Selection::new();
        s.click(1, PLAIN);
        s.click(9, CTRL);
        let base = s.clone();
        // The band grows and shrinks; each time the rows under it are given whole.
        s.set_band([3, 4, 5], Some(&base), false);
        assert_eq!((rows_of(&s), s.cursor()), (vec![3, 4, 5], Some(5)));
        s.set_band([4, 3], Some(&base), false);
        assert_eq!(rows_of(&s), vec![3, 4]);
        // With Ctrl: on top of what there was when it began.
        s.set_band([8, 3, 3], Some(&base), true);
        assert_eq!(rows_of(&s), vec![1, 3, 8, 9]);
        s.set_band([], Some(&base), true);
        assert_eq!(rows_of(&s), vec![1, 9]);
        s.set_band([], None, false);
        assert!(s.is_empty());
    }

    #[test]
    fn a_remap_follows_rows_through_a_sort_and_a_deletion() {
        let mut s = Selection::new();
        s.click(2, PLAIN);
        s.click(4, SHIFT);
        s.click(8, CTRL);
        // Sorted the other way round in a list of ten: row i is now row 9 - i.
        s.remap(|i| Some(9 - i));
        assert_eq!(rows_of(&s), vec![1, 5, 6, 7]);
        assert_eq!((s.cursor(), s.anchor()), (Some(1), Some(1)));
        // Rows 5 and 6 deleted: what was after them moves up two.
        s.set_cursor(6);
        s.remap(|i| match i {
            5 | 6 => None,
            i if i > 6 => Some(i - 2),
            i => Some(i),
        });
        assert_eq!(rows_of(&s), vec![1, 5]);
        assert_eq!((s.cursor(), s.anchor()), (None, Some(1)));
        // Two old rows that land on one new row are one row.
        s.remap(|_| Some(0));
        assert_eq!(s.ranges(), &[0..1]);
        s.remap(|_| None);
        assert!(s.is_empty() && s.anchor().is_none());
    }

    #[test]
    fn a_file_listings_remap_keeps_the_selection_on_its_entries() {
        use mdshared_app::dirlist::{Change, Column, Entry, Kind, Listing, Sort};
        use std::path::PathBuf;

        let entry = |name: &str, size: u64| Entry {
            path: PathBuf::from("/listing").join(name),
            name: name.to_string(),
            kind: Kind::File,
            size,
            modified: None,
            created: None,
            hidden: false,
        };
        let mut listing = Listing::new();
        listing.set_entries(vec![
            entry("a.txt", 3),
            entry("b.txt", 1),
            entry("c.txt", 2),
        ]);
        let mut s = Selection::new();
        s.click(1, PLAIN);
        assert_eq!(listing.entry(1).unwrap().name, "b.txt");

        // By size the entry is the first row, and the selection follows it there.
        let remap = listing.set_sort(Sort {
            column: Column::Size,
            descending: false,
        });
        s.remap(remap.mapping());
        assert_eq!(rows_of(&s), vec![0]);
        assert_eq!(listing.entry(s.cursor().unwrap()).unwrap().name, "b.txt");

        // Back to the names, where it is the second row again.
        let remap = listing.set_sort(Sort {
            column: Column::Name,
            descending: false,
        });
        s.remap(remap.mapping());
        assert_eq!(s.cursor(), Some(1));

        // A rename keeps the selection on the entry under its new name, last by name.
        let remap = listing.apply(Change::Renamed {
            from: PathBuf::from("/listing/b.txt"),
            to: entry("z.txt", 1),
        });
        s.remap(remap.mapping());
        let cursor = s.cursor().expect("the renamed entry is still selected");
        assert_eq!(listing.entry(cursor).unwrap().name, "z.txt");
        assert_eq!(cursor, 2);
        assert_eq!(rows_of(&s), vec![2]);

        // Removing the selected entry drops the selection.
        let remap = listing.apply(Change::Removed(PathBuf::from("/listing/z.txt")));
        s.remap(remap.mapping());
        assert!(s.is_empty() && s.cursor().is_none());
    }

    const NAMES: [&str; 8] = [
        "Alpha", "apple", "Beta", "sand", "Sea", "second", "Şeker", "zeta",
    ];

    fn typed(ta: &mut TypeAhead, at: Instant, ch: char, cursor: Option<usize>) -> Option<usize> {
        ta.key(ch, at, NAMES.len(), cursor, |i| NAMES[i])
    }

    #[test]
    fn the_same_letter_steps_through_the_names_that_start_with_it() {
        let t = Instant::now();
        let mut ta = TypeAhead::new();
        let mut cursor = None;
        let mut seen = Vec::new();
        for k in 0..5 {
            cursor = typed(&mut ta, t + Duration::from_millis(100 * k), 's', cursor);
            seen.push(cursor.unwrap());
        }
        // sand, Sea, second, and round again; "Şeker" is another letter.
        assert_eq!(seen, vec![3, 4, 5, 3, 4]);
        assert_eq!(ta.typed(), "sssss");
        // From a cursor on a match, one letter moves on; capitals match small letters.
        let mut ta = TypeAhead::new();
        assert_eq!(typed(&mut ta, t, 'A', Some(0)), Some(1));
        let mut ta = TypeAhead::new();
        assert_eq!(typed(&mut ta, t, 'a', Some(1)), Some(0));
        let mut ta = TypeAhead::new();
        assert_eq!(typed(&mut ta, t, 'ş', Some(0)), Some(6));
        // No name starts with it: nothing, and the cursor is the caller's to keep.
        let mut ta = TypeAhead::new();
        assert_eq!(typed(&mut ta, t, 'q', Some(2)), None);
    }

    #[test]
    fn letters_typed_quickly_are_the_start_of_a_name_and_a_pause_starts_anew() {
        let t = Instant::now();
        let ms = Duration::from_millis;
        let mut ta = TypeAhead::new();
        let a = typed(&mut ta, t, 's', None);
        assert_eq!(a, Some(3));
        // "se": the cursor's row no longer matches, the first that does is Sea.
        let b = typed(&mut ta, t + ms(200), 'e', a);
        assert_eq!(b, Some(4));
        // "sec": on to second; "seco" stays on it, the row that still matches.
        let c = typed(&mut ta, t + ms(400), 'c', b);
        assert_eq!(c, Some(5));
        assert_eq!(typed(&mut ta, t + ms(600), 'O', c), Some(5));
        assert_eq!(ta.typed(), "secO");
        // No match: nothing, and the letters stay until the pause.
        assert_eq!(typed(&mut ta, t + ms(800), 'x', c), None);
        // Exactly the pause is not yet a pause; a little more is, and "z" is a new word.
        assert_eq!(typed(&mut ta, t + ms(1800), 'z', c), None);
        assert_eq!(typed(&mut ta, t + ms(2801), 'z', c), Some(7));
        assert_eq!(ta.typed(), "z");
        // A shorter pause can be asked for, and a reset forgets at once.
        let mut quick = TypeAhead::default().with_pause(ms(100));
        typed(&mut quick, t, 's', None);
        assert_eq!(typed(&mut quick, t + ms(150), 'a', Some(3)), Some(0));
        quick.reset();
        assert_eq!(quick.typed(), "");
    }

    #[test]
    fn keys_that_do_not_type_and_lists_without_rows_answer_nothing() {
        let t = Instant::now();
        let mut ta = TypeAhead::new();
        for ch in ['\r', '\u{8}', '\u{1b}', '\t', ' '] {
            assert_eq!(typed(&mut ta, t, ch, Some(0)), None, "{ch:?}");
            assert_eq!(ta.typed(), "");
        }
        // A space inside a word is a letter of it.
        let names = ["new file", "newer"];
        let find = |ta: &mut TypeAhead, ch| ta.key(ch, t, 2, Some(1), |i| names[i]);
        for ch in ['n', 'e', 'w'] {
            assert_eq!(find(&mut ta, ch), Some(if ch == 'n' { 0 } else { 1 }));
        }
        assert_eq!(find(&mut ta, ' '), Some(0));
        // No rows; a cursor past the end; a clock that goes back.
        let mut ta = TypeAhead::new();
        assert_eq!(ta.key('a', t, 0, None, |_| "a"), None);
        let mut ta = TypeAhead::new();
        assert_eq!(ta.key('b', t, 2, Some(99), |i| ["ba", "bb"][i]), Some(0));
        let mut ta = TypeAhead::new();
        ta.key('n', t + Duration::from_secs(5), 2, None, |i| names[i]);
        assert_eq!(ta.key('e', t, 2, Some(0), |i| names[i]), Some(0));
        assert_eq!(ta.typed(), "ne");
    }
}
