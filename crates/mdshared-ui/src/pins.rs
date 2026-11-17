//! Pinned tabs, as both applications keep them: the pinned tabs come first, a pin stands where
//! their close button would (drawn by [`crate::chrome`]), closing several tabs passes them by, and
//! the session brings them back pinned. These are the rules on a list of tabs; each application
//! keeps its own tab type and its own active index, which every call here keeps on the same tab.

/// A tab that can be pinned.
pub trait Pinnable {
    fn pinned(&self) -> bool;
    fn set_pinned(&mut self, on: bool);
}

/// How many tabs are pinned: they are the first ones.
pub fn count<T: Pinnable>(tabs: &[T]) -> usize {
    tabs.iter().take_while(|t| t.pinned()).count()
}

/// Pins or unpins tab `i`. A pinned tab goes to the end of the pinned ones, an unpinned one to
/// the start of the others; `active` stays on its tab. False when nothing changed.
pub fn set<T: Pinnable>(tabs: &mut Vec<T>, i: usize, on: bool, active: &mut Option<usize>) -> bool {
    if tabs.get(i).is_none_or(|t| t.pinned() == on) {
        return false;
    }
    let pinned = count(tabs);
    let to = if on { pinned } else { pinned.saturating_sub(1) };
    let mut tab = tabs.remove(i);
    tab.set_pinned(on);
    tabs.insert(to, tab);
    *active = active.map(|a| moved(a, i, to));
    true
}

/// Where tab `from` may go when dragged to `to`: a pinned tab stays among the pinned ones, any
/// other after them.
pub fn clamp_move<T: Pinnable>(tabs: &[T], from: usize, to: usize) -> usize {
    let pinned = count(tabs);
    if tabs.get(from).is_some_and(|t| t.pinned()) {
        to.min(pinned.saturating_sub(1))
    } else {
        to.max(pinned)
    }
}

/// Moves tab `from` to `to` within the rules of [`clamp_move`]; `active` stays on its tab. False
/// when nothing moved.
pub fn move_tab<T: Pinnable>(
    tabs: &mut Vec<T>,
    from: usize,
    to: usize,
    active: &mut Option<usize>,
) -> bool {
    if from >= tabs.len() || to >= tabs.len() {
        return false;
    }
    let to = clamp_move(tabs, from, to);
    if from == to {
        return false;
    }
    let tab = tabs.remove(from);
    tabs.insert(to, tab);
    *active = active.map(|a| moved(a, from, to));
    true
}

/// The index tab `a` has after the tab at `from` moved to `to`.
pub fn moved(a: usize, from: usize, to: usize) -> usize {
    if a == from {
        to
    } else if from < a && a <= to {
        a - 1
    } else if to <= a && a < from {
        a + 1
    } else {
        a
    }
}

/// After a session is read: marks as pinned the tabs `place` finds a place for (their index in
/// the session's list of pins) and puts them first in that order; the others keep theirs.
/// `active` stays on its tab.
pub fn restore<T: Pinnable>(
    tabs: &mut Vec<T>,
    place: impl Fn(&T) -> Option<usize>,
    active: &mut Option<usize>,
) {
    let mut order: Vec<(usize, usize)> = tabs
        .iter_mut()
        .enumerate()
        .map(|(i, t)| {
            let p = place(t);
            t.set_pinned(p.is_some());
            (p.unwrap_or(usize::MAX), i)
        })
        .collect();
    // Stable: equal places (the unpinned tabs) keep their order.
    order.sort_by_key(|&(p, _)| p);
    let old_active = *active;
    let mut taken: Vec<Option<T>> = std::mem::take(tabs).into_iter().map(Some).collect();
    for (new, &(_, old)) in order.iter().enumerate() {
        tabs.push(taken[old].take().expect("each tab once"));
        if old_active == Some(old) {
            *active = Some(new);
        }
    }
}

/// The next tab a "close others" or "close all" closes: the last one that is neither pinned nor
/// `keep`.
pub fn next_to_close<T: Pinnable>(tabs: &[T], keep: Option<usize>) -> Option<usize> {
    tabs.iter()
        .enumerate()
        .rposition(|(i, t)| !t.pinned() && Some(i) != keep)
}

/// Which tab to show at start: with `prefer_pinned` (the setting to start on the last pinned tab
/// used) that one when it is open, else the tab that was showing when the window closed.
pub fn start_tab(
    prefer_pinned: bool,
    last_pinned: Option<usize>,
    was_active: Option<usize>,
) -> Option<usize> {
    match last_pinned {
        Some(p) if prefer_pinned => Some(p),
        _ => was_active.or(last_pinned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Tab(&'static str, bool);

    impl Pinnable for Tab {
        fn pinned(&self) -> bool {
            self.1
        }
        fn set_pinned(&mut self, on: bool) {
            self.1 = on;
        }
    }

    fn names(tabs: &[Tab]) -> String {
        tabs.iter()
            .map(|t| {
                if t.1 {
                    format!("[{}]", t.0)
                } else {
                    t.0.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn tabs(spec: &str) -> Vec<Tab> {
        spec.split(' ')
            .map(|s| match s.strip_prefix('[') {
                Some(n) => Tab(
                    Box::leak(n.trim_end_matches(']').to_string().into_boxed_str()),
                    true,
                ),
                None => Tab(Box::leak(s.to_string().into_boxed_str()), false),
            })
            .collect()
    }

    #[test]
    fn pinning_moves_a_tab_to_the_pins_end_and_unpinning_out_of_them() {
        let mut t = tabs("[a] b c d");
        let mut active = Some(3);
        assert!(set(&mut t, 2, true, &mut active));
        assert_eq!(names(&t), "[a] [c] b d");
        assert_eq!(active, Some(3), "d stays active");
        assert!(set(&mut t, 0, false, &mut active));
        assert_eq!(names(&t), "[c] a b d");
        assert!(!set(&mut t, 0, true, &mut active), "already pinned");
        assert!(!set(&mut t, 9, true, &mut active));
        let mut active = Some(1);
        assert!(set(&mut t, 1, true, &mut active));
        assert_eq!((names(&t).as_str(), active), ("[c] [a] b d", Some(1)));
    }

    #[test]
    fn a_drag_keeps_pinned_tabs_first() {
        let mut t = tabs("[a] [b] c d");
        let mut active = Some(0);
        assert!(move_tab(&mut t, 0, 3, &mut active));
        assert_eq!((names(&t).as_str(), active), ("[b] [a] c d", Some(1)));
        assert!(move_tab(&mut t, 3, 0, &mut active));
        assert_eq!((names(&t).as_str(), active), ("[b] [a] d c", Some(1)));
        assert!(
            !move_tab(&mut t, 2, 1, &mut active),
            "d cannot go among the pins"
        );
    }

    #[test]
    fn a_session_brings_its_pins_back_first_in_their_order() {
        let mut t = tabs("a b c d e");
        let mut active = Some(4);
        let pins = ["d", "b"];
        restore(&mut t, |t| pins.iter().position(|p| *p == t.0), &mut active);
        assert_eq!(names(&t), "[d] [b] a c e");
        assert_eq!(active, Some(4));
        let mut active = Some(3);
        let mut t = tabs("a b c d e");
        restore(&mut t, |t| pins.iter().position(|p| *p == t.0), &mut active);
        assert_eq!(active, Some(0), "d is first now");
    }

    #[test]
    fn closing_many_passes_the_pins_and_the_kept_tab() {
        let t = tabs("[a] b [c] d");
        assert_eq!(next_to_close(&t, None), Some(3));
        assert_eq!(next_to_close(&t, Some(3)), Some(1));
        assert_eq!(next_to_close(&tabs("[a] b"), Some(1)), None);
    }

    #[test]
    fn the_start_tab_is_the_last_pinned_one_only_when_asked() {
        assert_eq!(start_tab(true, Some(1), Some(3)), Some(1));
        assert_eq!(start_tab(false, Some(1), Some(3)), Some(3));
        assert_eq!(start_tab(true, None, Some(3)), Some(3));
        assert_eq!(start_tab(false, Some(1), None), Some(1));
        assert_eq!(start_tab(true, None, None), None);
    }
}
