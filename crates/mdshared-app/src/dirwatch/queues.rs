//! Bounded queues on either side of the metadata worker. A backlog that can no longer
//! be represented as changes becomes a rescan. Loss of the watch takes precedence.

use super::{Change, Entry, RawChange, Signal};
use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Instant;

pub(super) const LIMIT: usize = 4_096;
pub(super) type Batch = VecDeque<(RawChange, Instant, Option<usize>)>;
pub(super) type RestartAck = (bool, Instant);

#[derive(Default)]
pub(super) struct Inbox {
    state: Mutex<InboxState>,
    ready: Condvar,
}

#[derive(Default)]
struct InboxState {
    events: Batch,
    stop: bool,
    restarted: Option<RestartAck>,
}

impl Inbox {
    pub(super) fn push(&self, mut change: RawChange, mut at: Instant, cookie: Option<usize>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stop || matches!(state.events.front(), Some((RawChange::Lost, ..))) {
            return;
        }
        if change != RawChange::Lost
            && matches!(state.events.front(), Some((RawChange::Rescan, ..)))
        {
            return;
        }
        if state.events.len() >= LIMIT && change != RawChange::Lost {
            change = RawChange::Rescan;
        }
        let mut discarded = Batch::new();
        if matches!(change, RawChange::Lost | RawChange::Rescan) {
            if let Some((_, first, _)) = state.events.front() {
                at = at.min(*first);
            }
            discarded = std::mem::take(&mut state.events);
        }
        state.events.push_back((change, at, cookie));
        self.ready.notify_one();
        drop(state);
        drop(discarded);
    }

    /// Detaches events before the worker processes them. An expired deadline may return an
    /// empty batch; shutdown returns `None`.
    pub(super) fn wait_for_work(
        &self,
        deadline: Option<Instant>,
    ) -> Option<(Batch, Option<RestartAck>)> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if state.stop {
                return None;
            }
            if !state.events.is_empty() || state.restarted.is_some() {
                return Some((std::mem::take(&mut state.events), state.restarted.take()));
            }
            state = if let Some(deadline) = deadline {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Some((Batch::new(), None));
                }
                self.ready
                    .wait_timeout(state, remaining)
                    .unwrap_or_else(|e| e.into_inner())
                    .0
            } else {
                self.ready.wait(state).unwrap_or_else(|e| e.into_inner())
            };
        }
    }

    #[cfg(test)]
    pub(super) fn wait_for_batch(&self, deadline: Option<Instant>) -> Option<Batch> {
        self.wait_for_work(deadline).map(|(batch, _)| batch)
    }

    pub(super) fn restart_finished(&self, succeeded: bool) {
        self.record_restart(succeeded, Instant::now());
    }

    pub(super) fn record_restart(&self, succeeded: bool, at: Instant) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.stop {
            state.restarted = Some((succeeded, at));
            self.ready.notify_one();
        }
    }

    /// The mailbox is not drained during a scan. Filter against it under the callback lock,
    /// including both endpoints of queued renames. If the paths are unknown or an exact
    /// snapshot raced callbacks, fall back to a plain reset instead of installing stale data.
    /// Returns whether an exact snapshot was published, or `None` if publication is unsafe.
    pub(super) fn publish_scan(
        &self,
        shared: &Shared,
        epoch: u64,
        signals: &mut Vec<Signal>,
        mut snapshot: Option<Vec<Entry>>,
        wake: &dyn Fn(),
    ) -> Option<bool> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stop || matches!(state.events.front(), Some((RawChange::Lost, ..))) {
            return None;
        }
        let overflow = matches!(state.events.front(), Some((RawChange::Rescan, ..)));
        if snapshot.is_some()
            && state.events.iter().any(|(change, _, _)| {
                matches!(
                    change,
                    RawChange::Renamed { .. } | RawChange::RenamedFrom(_) | RawChange::RenamedTo(_)
                )
            })
        {
            // A full reset cannot omit individual paths. Don't put it ahead of a
            // valid rename; retry after the native changes have settled.
            return None;
        }
        if overflow || (snapshot.is_some() && !state.events.is_empty()) {
            *signals = vec![Signal::Rescan];
            snapshot = None;
        } else if !state.events.is_empty() {
            let mut touched = HashSet::<&Path>::new();
            for (change, _, _) in &state.events {
                match change {
                    RawChange::Created(path)
                    | RawChange::Removed(path)
                    | RawChange::Modified(path)
                    | RawChange::RenamedFrom(path)
                    | RawChange::RenamedTo(path) => {
                        touched.insert(path);
                    }
                    RawChange::Renamed { from, to } => {
                        touched.insert(from);
                        touched.insert(to);
                    }
                    RawChange::Lost | RawChange::Rescan => unreachable!("standalone marker"),
                }
            }
            signals.retain(|signal| match signal {
                Signal::Changed(Change::Created(entry) | Change::Modified(entry)) => {
                    !touched.contains(entry.path.as_path())
                }
                Signal::Changed(Change::Removed(path)) => !touched.contains(path.as_path()),
                Signal::Changed(Change::Renamed { from, to }) => {
                    !touched.contains(from.as_path()) && !touched.contains(to.path.as_path())
                }
                Signal::Lost | Signal::Rescan => true,
            });
        }
        let exact = snapshot.is_some();
        if !shared.deliver_checked(Some(epoch), signals.clone(), snapshot, wake) {
            return None;
        }
        if overflow {
            // The reset just published is also the notification for the raw overflow.
            state.events.clear();
        }
        Some(exact)
    }

    pub(super) fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.stop = true;
        let discarded = std::mem::take(&mut state.events);
        self.ready.notify_all();
        drop(state);
        drop(discarded);
    }

    pub(super) fn stopped(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).stop
    }

    /// Checks stop and publishes atomically with respect to dropping the watcher.
    pub(super) fn publish(&self, shared: &Shared, signals: Vec<Signal>, wake: &dyn Fn()) {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.stop {
            shared.deliver(signals, wake);
        }
    }
}

/// Settled reports waiting for the application, and whether it has already been woken.
#[derive(Default)]
pub(super) struct Shared {
    report: Mutex<Report>,
    pending: AtomicBool,
}

#[derive(Default)]
struct Report {
    signals: Vec<Signal>,
    snapshot: Option<Vec<Entry>>,
    /// When a queue reset invalidates what the worker thinks the application has.
    epoch: u64,
}

impl Shared {
    pub(super) fn epoch(&self) -> u64 {
        self.report.lock().unwrap_or_else(|e| e.into_inner()).epoch
    }

    pub(super) fn deliver(&self, signals: Vec<Signal>, wake: &dyn Fn()) {
        self.deliver_checked(None, signals, None, wake);
    }

    fn deliver_checked(
        &self,
        epoch: Option<u64>,
        mut signals: Vec<Signal>,
        mut snapshot: Option<Vec<Entry>>,
        wake: &dyn Fn(),
    ) -> bool {
        {
            let mut report = self.report.lock().unwrap_or_else(|e| e.into_inner());
            if epoch.is_some_and(|epoch| epoch != report.epoch) {
                return false;
            }
            if signals.is_empty() || report.signals.first() == Some(&Signal::Lost) {
                return true;
            }
            if signals.contains(&Signal::Lost) {
                signals = std::mem::replace(&mut report.signals, vec![Signal::Lost]);
                snapshot = report.snapshot.take();
            } else if snapshot.is_some() {
                // This is the exact reset for all earlier changes in the queue.
                std::mem::swap(&mut report.signals, &mut signals);
                std::mem::swap(&mut report.snapshot, &mut snapshot);
            } else if signals.contains(&Signal::Rescan)
                || signals.len() > LIMIT - report.signals.len()
            {
                signals = std::mem::replace(&mut report.signals, vec![Signal::Rescan]);
                snapshot = report.snapshot.take();
                report.epoch = report.epoch.wrapping_add(1);
            } else if report.snapshot.is_none() && report.signals.first() == Some(&Signal::Rescan) {
                return true;
            } else {
                report.signals.append(&mut signals);
            }
        }
        drop((signals, snapshot));
        if !self.pending.swap(true, Ordering::AcqRel) {
            wake();
        }
        true
    }

    pub(super) fn woken(&self) {
        self.pending.store(false, Ordering::Release);
    }

    pub(super) fn take(&self) -> Vec<Signal> {
        let (mut signals, snapshot) = {
            let mut report = self.report.lock().unwrap_or_else(|e| e.into_inner());
            let snapshot = report.snapshot.take();
            if snapshot.is_some() {
                report.epoch = report.epoch.wrapping_add(1);
            }
            (std::mem::take(&mut report.signals), snapshot)
        };
        if snapshot.is_some() {
            signals.truncate(1);
        }
        signals
    }

    pub(super) fn take_with_snapshot(&self) -> (Vec<Signal>, Option<Vec<Entry>>) {
        let mut report = self.report.lock().unwrap_or_else(|e| e.into_inner());
        (std::mem::take(&mut report.signals), report.snapshot.take())
    }
}
