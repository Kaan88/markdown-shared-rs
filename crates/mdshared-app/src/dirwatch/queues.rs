//! Bounded queues on either side of the metadata worker. A backlog that can no longer
//! be represented as changes becomes a rescan. Loss of the watch takes precedence.

use super::{RawChange, Signal};
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
    pub(super) fn wait_for_batch(&self, deadline: Option<Instant>) -> Option<Batch> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if state.stop {
                return None;
            }
            if !state.events.is_empty() {
                return Some(std::mem::take(&mut state.events));
            }
            state = if let Some(deadline) = deadline {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Some(Batch::new());
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
    signals: Mutex<Vec<Signal>>,
    pending: AtomicBool,
}

impl Shared {
    pub(super) fn deliver(&self, mut signals: Vec<Signal>, wake: &dyn Fn()) {
        if signals.is_empty() {
            return;
        }
        {
            let mut queued = self.signals.lock().unwrap_or_else(|e| e.into_inner());
            if queued.first() == Some(&Signal::Lost) {
                return;
            }
            if signals.contains(&Signal::Lost) {
                signals = std::mem::replace(&mut *queued, vec![Signal::Lost]);
            } else if queued.first() == Some(&Signal::Rescan) {
                return;
            } else if signals.len() > LIMIT - queued.len() || signals.contains(&Signal::Rescan) {
                signals = std::mem::replace(&mut *queued, vec![Signal::Rescan]);
            } else {
                queued.append(&mut signals);
            }
        }
        drop(signals);
        if !self.pending.swap(true, Ordering::AcqRel) {
            wake();
        }
    }

    pub(super) fn woken(&self) {
        self.pending.store(false, Ordering::Release);
    }

    pub(super) fn take(&self) -> Vec<Signal> {
        std::mem::take(&mut *self.signals.lock().unwrap_or_else(|e| e.into_inner()))
    }
}
