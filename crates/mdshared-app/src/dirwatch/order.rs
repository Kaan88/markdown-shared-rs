//! The order a report is applied in: removals before puts, with cycles broken by reading the
//! disk instead of ordering them.
//!
//! This is the half of the coalescer that turns a set of settled verdicts into the order a
//! [`Listing`](crate::dirlist::Listing) needs. It reads nothing and changes no state.

use super::Verdict;
use std::path::{Path, PathBuf};

/// One verdict and the place it has in the events, which is the order equal verdicts keep.
#[derive(Clone, Debug)]
pub(super) struct Item {
    verdict: Verdict,
    seq: usize,
}

impl Item {
    /// An item whose place is given by `settled` when it builds the list.
    pub(super) fn new(verdict: Verdict) -> Item {
        Item { verdict, seq: 0 }
    }

    /// The paths this verdict takes away.
    fn removes(&self) -> Vec<&Path> {
        match &self.verdict {
            Verdict::Removed(path) => vec![path.as_path()],
            Verdict::Renamed { from, .. } => vec![from.as_path()],
            Verdict::Created(_) | Verdict::Modified(_) => Vec::new(),
            Verdict::Rescan | Verdict::Lost => Vec::new(),
        }
    }

    /// The paths this verdict puts an entry under.
    fn puts(&self) -> Vec<&Path> {
        match &self.verdict {
            Verdict::Created(path) | Verdict::Modified(path) => vec![path.as_path()],
            Verdict::Renamed { to, .. } => vec![to.as_path()],
            Verdict::Removed(_) => Vec::new(),
            Verdict::Rescan | Verdict::Lost => Vec::new(),
        }
    }
}

/// Puts the verdicts in an order that can be applied top to bottom, and breaks a cycle.
///
/// A rename both takes its old name away and puts its new one; a creation or a modification only
/// puts, and a removal only takes away. Whenever one verdict takes `P` away and another puts an
/// entry at `P`, the removal must come first. When no such order exists, each rename in the
/// report is broken into a removal and a modification, so every verdict has one job and the
/// order exists; the disk then decides whether each name is there. The order is the same every
/// time for the same events: among the verdicts that could go next, the first one seen goes
/// first.
pub(super) fn settled(items: Vec<Item>) -> Vec<Verdict> {
    let mut items = renumber(dedupe_removed(items));
    if let Some(order) = order_of(&items) {
        return order
            .into_iter()
            .map(|i| items[i].verdict.clone())
            .collect();
    }
    // A cycle: read both sides instead of ordering them. A removal of an unlisted name does
    // nothing, and the modification is what the disk says the name is now.
    let mut split = Vec::with_capacity(items.len() * 2);
    for item in items.drain(..) {
        match item.verdict {
            Verdict::Renamed { from, to } => {
                split.push(Item {
                    verdict: Verdict::Removed(from),
                    seq: 0,
                });
                split.push(Item {
                    verdict: Verdict::Modified(to),
                    seq: 0,
                });
            }
            other => split.push(Item {
                verdict: other,
                seq: 0,
            }),
        }
    }
    let split = renumber(dedupe_removed(split));
    match order_of(&split) {
        Some(order) => order
            .into_iter()
            .map(|i| split[i].verdict.clone())
            .collect(),
        // One job per verdict cannot form a cycle, so this cannot happen; if it ever does, the
        // list is still applied rather than lost.
        None => split.into_iter().map(|item| item.verdict).collect(),
    }
}

/// Drops a removal of a name another removal already reports.
fn dedupe_removed(items: Vec<Item>) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::with_capacity(items.len());
    let mut removed: Vec<PathBuf> = Vec::new();
    for item in items {
        if let Verdict::Removed(path) = &item.verdict {
            if removed.contains(path) {
                continue;
            }
            removed.push(path.clone());
        }
        out.push(item);
    }
    out
}

/// Gives each item its place in the report, so equal choices are broken the same way every run.
fn renumber(items: Vec<Item>) -> Vec<Item> {
    items
        .into_iter()
        .enumerate()
        .map(|(seq, mut item)| {
            item.seq = seq;
            item
        })
        .collect()
}

/// An order of the item indices that removes before it puts at the same name, or `None` when
/// the verdicts form a cycle and no such order exists.
fn order_of(items: &[Item]) -> Option<Vec<usize>> {
    let n = items.len();
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut before_count = vec![0usize; n];
    for (from, item) in items.iter().enumerate() {
        let removes = item.removes();
        if removes.is_empty() {
            continue;
        }
        for (to, other) in items.iter().enumerate() {
            if from == to {
                continue;
            }
            let puts = other.puts();
            if removes
                .iter()
                .any(|gone| puts.iter().any(|put| gone == put))
            {
                after[from].push(to);
                before_count[to] += 1;
            }
        }
    }
    let mut ready: Vec<usize> = (0..n).filter(|&i| before_count[i] == 0).collect();
    ready.sort_by_key(|&i| items[i].seq);
    let mut order = Vec::with_capacity(n);
    while !ready.is_empty() {
        let next = ready.remove(0);
        order.push(next);
        for &later in &after[next] {
            before_count[later] -= 1;
            if before_count[later] == 0 {
                ready.push(later);
            }
        }
        ready.sort_by_key(|&i| items[i].seq);
    }
    (order.len() == n).then_some(order)
}
