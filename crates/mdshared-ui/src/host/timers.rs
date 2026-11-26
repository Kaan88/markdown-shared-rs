//! The application's named timers: a small map from a name to the instant it is due, folded
//! into the loop's one wake-up with the animator's deadline.
//!
//! A timer is set, moved or cancelled by its name: setting a name that is already there moves
//! it, so one name is one timer. The names due by a given instant are taken together and in
//! the order they were set, so an application that asks for two timers at the same instant is
//! told about both in one turn. Nothing here reads a clock: the caller passes `now`, so the
//! arithmetic is tested on fixed instants.
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

    /// Takes the earliest timer due at or before `now`, if any. One at a time, so an
    /// application that cancels or moves a sibling from [`App::timer_fired`](super::App::timer_fired)
    /// is obeyed: the loop looks again after each callback. Timers due at the same instant come
    /// in the order they were set.
    pub(super) fn take_earliest_due(&mut self, now: Instant) -> Option<String> {
        let i = self
            .timers
            .iter()
            .enumerate()
            .filter(|(_, (_, at))| *at <= now)
            .min_by_key(|(_, (_, at))| *at)
            .map(|(i, _)| i)?;
        Some(self.timers.remove(i).0)
    }
}

#[cfg(test)]
#[path = "timers_tests.rs"]
mod tests;
