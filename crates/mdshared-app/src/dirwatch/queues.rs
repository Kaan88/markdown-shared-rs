//! Bounded queues on either side of the metadata worker. A backlog that can no longer
//! be represented as changes becomes a rescan. Loss of the watch takes precedence.

use super::{Entry, RawChange, Signal};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Instant;

pub(super) const LIMIT: usize = 4_096;
pub(super) type Batch = VecDeque<(RawChange, Instant, Option<usize>)>;

#[derive(Default)]
pub(super) struct Inbox {
    state: Mutex<InboxState>,
    ready: Condvar,
}

#[derive(Default)]
struct InboxState {
    events: Batch,
    stop: bool,
    restarted: Option<bool>,
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
    pub(super) fn wait_for_work(&self, deadline: Option<Instant>) -> Option<(Batch, Option<bool>)> {
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
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.stop {
            state.restarted = Some(succeeded);
            self.ready.notify_one();
        }
    }

    pub(super) fn scan_is_clear(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        !state.stop && state.events.is_empty()
    }

    /// The mailbox is not drained during a scan. Check it again under the same lock
    /// used by callbacks, so even an event arriving during diff preparation wins.
    pub(super) fn publish_scan(
        &self,
        shared: &Shared,
        epoch: u64,
        signals: Vec<Signal>,
        snapshot: Option<Vec<Entry>>,
        wake: &dyn Fn(),
    ) -> bool {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stop || !state.events.is_empty() {
            return false;
        }
        shared.deliver_checked(Some(epoch), signals, snapshot, wake)
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
