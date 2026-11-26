//! The application's named timers: a small map from a name to the instant it is due, folded
//! into the loop's one wake-up with the animator's deadline.
//!
//! A timer is set, moved or cancelled by its name: setting a name that is already there moves
//! it, so one name is one timer. When a turn begins its due names are taken as a snapshot, in
//! firing order (earliest first, set order at one instant); each then fires only if it is
//! still waiting and still due when its turn comes, so a callback may cancel or move a
//! sibling. A timer set during the turn waits for the next one, even at `now`. Nothing here
//! reads a clock: the caller passes `now`, so the arithmetic is tested on fixed instants.
//!
//! Deliberately not here: repeating timers (an application asks again from
//! [`App::timer_fired`](super::App::timer_fired)), a timer's value or payload, and a clock.

use std::time::Instant;

/// The named timers waiting: one instant per name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct NamedTimers {
    /// In the order the names were first set; moving a name keeps its place.
    timers: Vec<(String, Instant)>,
}

impl NamedTimers {
    /// Sets the timer called `name` to `at`, moving it when it is already waiting. An instant
    /// in the past is accepted: it is due at once.
    pub(super) fn set(&mut self, name: &str, at: Instant) {
        match self.timers.iter_mut().find(|(n, _)| n == name) {
            Some((_, when)) => *when = at,
            None => self.timers.push((name.to_string(), at)),
        }
    }

    /// Removes the timer called `name`; true when one was waiting. A timer that has already
    /// fired is gone already, so cancelling it answers false.
    pub(super) fn cancel(&mut self, name: &str) -> bool {
        let before = self.timers.len();
        self.timers.retain(|(n, _)| n != name);
        self.timers.len() != before
    }

    /// When `name` is due, if it is waiting. Only the tests ask by name: the loop asks for the
    /// earliest of all.
    #[cfg(test)]
    pub(super) fn at(&self, name: &str) -> Option<Instant> {
        self.timers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, at)| *at)
    }

    /// When the earliest timer is due: what the loop waits for. `None` when none waits.
    pub(super) fn next(&self) -> Option<Instant> {
        self.timers.iter().map(|(_, at)| *at).min()
    }

    /// How many timers wait. Only the tests ask.
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.timers.len()
    }

    /// The names due at or before `now`, in firing order: earliest first, and at one instant
    /// the order they were set. Taken when a turn begins: a timer set during the turn is not
    /// here and waits for the next one.
    pub(super) fn due_at(&self, now: Instant) -> Vec<String> {
        let mut due: Vec<(Instant, usize, &String)> = self
            .timers
            .iter()
            .enumerate()
            .filter(|(_, (_, at))| *at <= now)
            .map(|(i, (name, at))| (*at, i, name))
            .collect();
        due.sort_by_key(|(at, i, _)| (*at, *i));
        due.into_iter().map(|(_, _, name)| name.clone()).collect()
    }

    /// Takes `name` only if it is still waiting and still due at or before `now`: a callback
    /// may have cancelled it or moved it later since the turn began. True when it was taken.
    pub(super) fn take_if_due(&mut self, name: &str, now: Instant) -> bool {
        let Some(i) = self.timers.iter().position(|(n, _)| n == name) else {
            return false;
        };
        if self.timers[i].1 > now {
            return false;
        }
        self.timers.remove(i);
        true
    }
}

#[cfg(test)]
#[path = "timers_tests.rs"]
mod tests;
