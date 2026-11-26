//! Where the rows of a [`Listing`](super::Listing) went after it changed, and the row edits one
//! change made.
//!
//! A remap is rows and nothing else: it knows no entry, no path and no disk, and the list owns
//! what a row holds. A change describes itself with a handful of edits and a few rows it names
//! by hand, followed when a row is asked for, so a change on a list of a million entries builds
//! no per-row table; a new sort keeps one answer per old row instead.
//!
//! Deliberately not here: anything that reads or sorts. [`Remap::exact`] and
//! [`Remap::edited`] are the list's two ways of saying what moved.

/// An answer per old row: where it is now, or `None` when the entry is gone or was filtered out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remap {
    /// The rows the list showed before it changed.
    rows: usize,
    way: Way,
}

/// How a remap answers for an old row.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Way {
    /// One answer for every old row, made when the view was rebuilt.
    Exact(Vec<Option<usize>>),
    /// The row edits one change made, applied in order, and the rows it names by hand.
    Edited {
        edits: Vec<Edit>,
        /// Old rows whose answer is not what the edits give: a rename's old row, which follows
        /// the entry to its new name, though the old path is gone.
        named: Vec<(usize, Option<usize>)>,
    },
}

/// One row edit, in the row numbering of the moment it was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Edit {
    /// The row at `at` is gone, and the rows after it moved up one.
    Removed(usize),
    /// A row was inserted at `at`, and the rows from it moved down one.
    Inserted(usize),
}

impl Default for Remap {
    fn default() -> Remap {
        Remap::exact(Vec::new())
    }
}

impl Remap {
    /// A remap with one answer per old row.
    pub(super) fn exact(new_of: Vec<Option<usize>>) -> Remap {
        Remap {
            rows: new_of.len(),
            way: Way::Exact(new_of),
        }
    }

    /// A remap that follows `edits` for every old row, except for the rows in `named`, which
    /// have their answer given outright.
    pub(super) fn edited(
        rows: usize,
        edits: Vec<Edit>,
        named: Vec<(usize, Option<usize>)>,
    ) -> Remap {
        Remap {
            rows,
            way: Way::Edited { edits, named },
        }
    }

    /// The new row of the old row `old`, or `None` when that entry is not shown any more.
    ///
    /// A row the list never had the answer for is `None`, whatever the old row count says: a
    /// caller that kept an index from another list gets nothing, not a row of this one.
    pub fn get(&self, old: usize) -> Option<usize> {
        if old >= self.rows {
            return None;
        }
        match &self.way {
            Way::Exact(new_of) => new_of.get(old).copied().flatten(),
            Way::Edited { edits, named } => {
                if let Some((_, to)) = named.iter().find(|(row, _)| *row == old) {
                    return *to;
                }
                let mut row = old;
                for edit in edits {
                    match *edit {
                        Edit::Removed(at) => {
                            if row == at {
                                return None;
                            }
                            if row > at {
                                row -= 1;
                            }
                        }
                        Edit::Inserted(at) => {
                            if row >= at {
                                row += 1;
                            }
                        }
                    }
                }
                Some(row)
            }
        }
    }

    /// How many old rows this remap speaks for: the length of the list before it changed.
    pub fn len(&self) -> usize {
        self.rows
    }

    /// Whether there were no rows before the change.
    pub fn is_empty(&self) -> bool {
        self.rows == 0
    }

    /// Whether every old row is where it was, and none was lost.
    pub fn unchanged(&self) -> bool {
        (0..self.rows).all(|old| self.get(old) == Some(old))
    }

    /// The remap as the function `Selection::remap` takes, for `selection.remap(remap.mapping())`.
    pub fn mapping(&self) -> impl Fn(usize) -> Option<usize> + '_ {
        |old| self.get(old)
    }
}
