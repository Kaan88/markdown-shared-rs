//! One metadata worker serializes incremental reads and periodic scans. Its inbox
//! wait ends at either deadline, so a silent backend does not prevent recovery.

use super::queues::{Batch, Inbox, LIMIT, Shared};
use super::reconcile::{self, Periodic};
use super::resolve::{Identity, lost_signal, signal_of};
use super::{Coalescer, RawChange, Read, Signal, Verdict};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub(super) struct Worker {
    pub(super) inbox: Arc<Inbox>,
    pub(super) shared: Arc<Shared>,
    pub(super) coalescer: Coalescer,
    pub(super) read: Arc<Read>,
    pub(super) root: PathBuf,
    pub(super) started: Option<Identity>,
    pub(super) periodic: Option<Periodic>,
}

impl Worker {
    pub(super) fn run(mut self, wake: &dyn Fn(), restart: impl Fn()) {
        let mut seen = 0;
        loop {
            let deadline = self
                .coalescer
                .deadline()
                .into_iter()
                .chain(self.periodic.as_ref().and_then(Periodic::deadline))
                .min();
            let Some(batch) = self.inbox.wait_for_batch(deadline) else {
                return;
            };
            feed_batch(&mut self.coalescer, batch, &mut seen);
            let verdicts = self.coalescer.flush(Instant::now());
            if !verdicts.is_empty() {
                seen = 0;
                let Some(signals) = self.resolve(verdicts) else {
                    return;
                };
                if let Some(periodic) = &mut self.periodic {
                    periodic.observe(&signals);
                }
                if self.publish(signals, wake) {
                    return;
                }
            }
            if self.inbox.stopped() {
                return;
            }
            if let Some(periodic) = &mut self.periodic {
                let signals = periodic.run_due(
                    Instant::now(),
                    &restart,
                    || {
                        #[cfg(unix)]
                        if let (Some(started), Some(current)) =
                            (self.started, super::resolve::identity(&self.root))
                            && started != current
                        {
                            return Ok(None);
                        }
                        reconcile::scan(&self.root, self.coalescer.folder(), || {
                            self.inbox.stopped()
                        })
                    },
                    Instant::now,
                );
                if self.publish(signals, wake) {
                    return;
                }
            }
        }
    }

    fn resolve(&self, verdicts: Vec<Verdict>) -> Option<Vec<Signal>> {
        let mut signals = Vec::with_capacity(verdicts.len());
        for verdict in verdicts {
            if self.inbox.stopped() {
                return None;
            }
            let signal = match verdict {
                Verdict::Lost => Some(lost_signal(&self.root, self.started)),
                other => signal_of(other, &*self.read),
            };
            if let Some(signal) = signal {
                if signal == Signal::Rescan {
                    return Some(vec![signal]);
                }
                signals.push(signal);
            }
        }
        Some(signals)
    }

    /// Returns whether the worker should stop after publishing.
    fn publish(&self, signals: Vec<Signal>, wake: &dyn Fn()) -> bool {
        let lost = signals.first() == Some(&Signal::Lost);
        self.inbox.publish(&self.shared, signals, wake);
        if lost {
            self.inbox.stop();
        }
        lost
    }
}

/// The worker can receive many small batches before the next flush. Bound the work it keeps
/// across batches as well as the two queues. After budget is exhausted, only loss can supersede
/// the rescan.
pub(super) fn feed_batch(coalescer: &mut Coalescer, batch: Batch, seen: &mut usize) {
    for (change, at, cookie) in batch {
        if change == RawChange::Lost || *seen < LIMIT {
            coalescer.push_tracked(change, at, cookie);
            *seen = (*seen + 1).min(LIMIT);
        } else if !coalescer.rescan && !coalescer.lost {
            coalescer.push(RawChange::Rescan, at);
        }
        if coalescer.is_idle() {
            *seen = 0;
        }
    }
}
