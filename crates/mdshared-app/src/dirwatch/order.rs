//! The order a report is applied in: removals before puts, with a cycle broken by reading the
//! disk instead of ordering it.
//!
//! This is the half of the coalescer that turns a set of settled verdicts into the order a
//! [`Listing`](crate::dirlist::Listing) needs. It reads nothing and changes no state.

use super::Verdict;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::path::Path;

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

    /// The path this verdict takes away, if it takes one.
    fn removes(&self) -> Option<&Path> {
        match &self.verdict {
            Verdict::Removed(path) => Some(path.as_path()),
            Verdict::Renamed { from, .. } => Some(from.as_path()),
            Verdict::Created(_) | Verdict::Modified(_) => None,
            Verdict::Rescan | Verdict::Lost => None,
        }
    }

    /// The path this verdict puts an entry under, if it puts one.
    fn puts(&self) -> Option<&Path> {
        match &self.verdict {
            Verdict::Created(path) | Verdict::Modified(path) => Some(path.as_path()),
            Verdict::Renamed { to, .. } => Some(to.as_path()),
            Verdict::Removed(_) => None,
            Verdict::Rescan | Verdict::Lost => None,
        }
    }
}

/// Puts the verdicts in an order that can be applied top to bottom, and breaks a cycle.
///
/// A rename both takes its old name away and puts its new one; a creation or a modification only
/// puts, and a removal only takes away. Whenever one verdict takes `P` away and another puts an
/// entry at `P`, the removal must come first. A cycle can only be made of renames, because only
/// a rename has both an incoming and an outgoing edge; the renames in it are broken into a
/// removal and a modification, so every verdict in the cycle has one job and the order exists,
/// and the disk then decides whether each name is there. The order is the same every time for
/// the same events: among the verdicts that could go next, the first one seen goes first.
pub(super) fn settled(items: Vec<Item>) -> Vec<Verdict> {
    let items = renumber(dedupe_removed(items));
    let order = order_of(&items);
    if order.len() == items.len() {
        return order
            .into_iter()
            .map(|i| items[i].verdict.clone())
            .collect();
    }
    // Only the verdicts that could not be placed are in a cycle; everything else keeps its
    // rename, so an unrelated rename is still followed by the selection.
    let placed: HashSet<usize> = order.iter().copied().collect();
    let mut split = Vec::with_capacity(items.len() + 4);
    for (i, item) in items.into_iter().enumerate() {
        if placed.contains(&i) {
            split.push(item);
            continue;
        }
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
    let order = order_of(&split);
    if order.len() == split.len() {
        return order
            .into_iter()
            .map(|i| split[i].verdict.clone())
            .collect();
    }
    // One job per verdict cannot form a cycle, so this cannot happen; if it ever does, the list
    // is still applied rather than lost.
    split.into_iter().map(|item| item.verdict).collect()
}

/// Drops a removal of a name another removal already reports.
fn dedupe_removed(items: Vec<Item>) -> Vec<Item> {
    let mut seen: HashSet<std::path::PathBuf> = HashSet::new();
    items
        .into_iter()
        .filter(|item| match &item.verdict {
            Verdict::Removed(path) => seen.insert(path.clone()),
            _ => true,
        })
        .collect()
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

/// The order of as many item indices as can be placed: a removal before a put at the same name.
///
/// The puts are indexed by path once, so a report of twenty thousand verdicts is ordered in
/// about `n log n` and not in `n` squared. The queue of verdicts that could go next is a heap
/// keyed by the verdict's place in the events, so the order does not depend on the platform's
/// hash order.
fn order_of(items: &[Item]) -> Vec<usize> {
    let n = items.len();
    let mut puts: HashMap<&Path, Vec<usize>> = HashMap::with_capacity(n);
    for (i, item) in items.iter().enumerate() {
        if let Some(put) = item.puts() {
            puts.entry(put).or_default().push(i);
        }
    }
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut before_count = vec![0usize; n];
    let mut linked: Vec<usize> = Vec::new();
    for (from, item) in items.iter().enumerate() {
        let Some(gone) = item.removes() else {
            continue;
        };
        linked.clear();
        for &to in puts.get(gone).map(Vec::as_slice).unwrap_or(&[]) {
            if to != from && !linked.contains(&to) {
                linked.push(to);
            }
        }
        for to in linked.iter().copied() {
            after[from].push(to);
            before_count[to] += 1;
        }
    }
    let mut ready: BinaryHeap<Reverse<(usize, usize)>> = (0..n)
        .filter(|&i| before_count[i] == 0)
        .map(|i| Reverse((items[i].seq, i)))
        .collect();
    let mut order = Vec::with_capacity(n);
    while let Some(Reverse((_, next))) = ready.pop() {
        order.push(next);
        for &later in &after[next] {
            before_count[later] -= 1;
            if before_count[later] == 0 {
                ready.push(Reverse((items[later].seq, later)));
            }
        }
    }
    order
}
