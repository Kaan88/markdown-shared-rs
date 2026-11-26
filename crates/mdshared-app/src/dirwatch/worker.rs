//! One metadata worker serializes incremental reads and periodic scans. Its inbox
//! wait ends at either deadline, so a silent backend does not prevent recovery.

use super::queues::{Batch, Inbox, LIMIT, Shared};
use super::reconcile::{self, Periodic, Scan};
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

struct Cycle<'a> {
    wake: &'a dyn Fn(),
    restart: &'a dyn Fn(),
    scan: &'a dyn Fn(&Worker) -> Scan,
    now: &'a dyn Fn() -> Instant,
}

impl Worker {
    pub(super) fn run(mut self, wake: &dyn Fn(), restart: impl Fn()) {
        self.run_with_scan(wake, &restart, &Self::scan);
    }

    pub(super) fn run_with_scan(
        &mut self,
        wake: &dyn Fn(),
        restart: &dyn Fn(),
        scan: &dyn Fn(&Worker) -> Scan,
    ) {
        let mut seen = 0;
        let cycle = Cycle {
            wake,
            restart,
            scan,
            now: &Instant::now,
        };
        loop {
            let deadline = self
                .coalescer
                .deadline()
                .or_else(|| self.periodic.as_ref().and_then(Periodic::deadline));
            let Some(work) = self.inbox.wait_for_work(deadline) else {
                return;
            };
            if !self.cycle(work, &mut seen, &cycle) {
                return;
            }
        }
    }

    fn cycle(
        &mut self,
        (batch, restarted): (Batch, Option<bool>),
        seen: &mut usize,
        io: &Cycle<'_>,
    ) -> bool {
        if let (Some(periodic), Some(succeeded)) = (&mut self.periodic, restarted) {
            periodic.restart_finished(succeeded);
        }
        feed_batch(&mut self.coalescer, batch, seen);
        let verdicts = self.coalescer.flush((io.now)());
        if !verdicts.is_empty() {
            *seen = 0;
            let Some(signals) = self.resolve(verdicts) else {
                return false;
            };
            if let Some(periodic) = &mut self.periodic {
                periodic.observe(&signals);
            }
            if self.publish(signals, io.wake) {
                return false;
            }
        }
        if self.coalescer.is_idle()
            && self.inbox.scan_is_clear()
            && let Some(periodic) = &mut self.periodic
            && periodic.ready((io.now)(), io.restart)
        {
            let epoch = self.shared.epoch();
            periodic.sync_epoch(epoch);
            let result = (io.scan)(self);
            let periodic = self.periodic.as_mut().unwrap();
            let (signals, snapshot) = periodic.report(&result);
            let lost = signals.first() == Some(&Signal::Lost);
            if self
                .inbox
                .publish_scan(&self.shared, epoch, signals, snapshot, io.wake)
            {
                periodic.committed(result, (io.now)());
                if lost {
                    self.inbox.stop();
                    return false;
                }
            }
        }
        !self.inbox.stopped()
    }

    fn scan(&self) -> Scan {
        #[cfg(unix)]
        if let (Some(started), Some(current)) = (self.started, super::resolve::identity(&self.root))
            && started != current
        {
            return Ok(None);
        }
        reconcile::scan(&self.root, self.coalescer.folder(), || self.inbox.stopped())
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

#[cfg(test)]
mod tests {
    use super::super::reconcile::Snapshot;
    use super::*;
    use crate::dirlist::{Entry, Kind, Listing};
    use std::cell::Cell;
    use std::time::Duration;

    fn path(name: &str) -> PathBuf {
        PathBuf::from("folder").join(name)
    }

    fn entry(name: &str) -> Entry {
        Entry {
            path: path(name),
            name: name.into(),
            kind: Kind::File,
            size: 1,
            modified: None,
            created: None,
            hidden: false,
        }
    }

    fn snapshot(name: &str) -> Snapshot {
        let entry = entry(name);
        [(entry.path.clone(), entry)].into()
    }

    fn worker(now: Instant, initial: Option<Vec<Entry>>) -> Worker {
        let mut periodic = Periodic::new(Duration::from_secs(1), now, initial).unwrap();
        periodic.make_due(now);
        Worker {
            inbox: Arc::new(Inbox::default()),
            shared: Arc::new(Shared::default()),
            coalescer: Coalescer::new("folder", Duration::from_millis(100)),
            read: Arc::new(|path| Ok(entry(path.file_name().unwrap().to_str().unwrap()))),
            root: "folder".into(),
            started: None,
            periodic: Some(periodic),
        }
    }

    fn tick(worker: &mut Worker, seen: &mut usize, io: &Cycle<'_>, ack: Option<bool>) {
        if let Some(ack) = ack {
            worker.inbox.restart_finished(ack);
        }
        let work = worker.inbox.wait_for_work(Some(Instant::now())).unwrap();
        assert!(worker.cycle(work, seen, io));
    }

    #[test]
    fn selection_follows_a_rename_that_is_pending_or_arrives_during_a_scan() {
        for while_scanning in [false, true] {
            let start = Instant::now();
            let clock = Cell::new(start);
            let scans = Cell::new(0);
            let mut worker = worker(start, Some(vec![entry("a")]));
            let inbox = Arc::clone(&worker.inbox);
            let rename = || {
                inbox.push(RawChange::RenamedFrom(path("a")), clock.get(), None);
                inbox.push(RawChange::RenamedTo(path("b")), clock.get(), None);
            };
            let io = Cycle {
                wake: &|| {},
                restart: &|| {},
                scan: &|_| {
                    scans.set(scans.get() + 1);
                    if while_scanning && scans.get() == 1 {
                        rename();
                    }
                    Ok(Some(snapshot("b")))
                },
                now: &|| clock.get(),
            };
            let mut seen = 0;
            tick(&mut worker, &mut seen, &io, None); // request restart
            if !while_scanning {
                rename();
            }
            tick(&mut worker, &mut seen, &io, Some(true));
            assert!(
                worker.shared.take().is_empty(),
                "no scan delta may beat the rename"
            );
            assert_eq!(scans.get(), usize::from(while_scanning));
            tick(&mut worker, &mut seen, &io, None); // any events from inside the scan
            clock.set(start + Duration::from_millis(100));
            tick(&mut worker, &mut seen, &io, None);
            assert_eq!(
                scans.get(),
                1 + usize::from(while_scanning),
                "scan retried after settling"
            );

            let mut listing = Listing::new();
            listing.set_folder(Some("folder".into()));
            listing.set_entries(vec![entry("a")]);
            let mut selected = listing.row_of(&path("a"));
            let signals = worker.shared.take();
            assert_eq!(signals.len(), 1);
            for signal in signals {
                let Signal::Changed(change) = signal else {
                    panic!("unexpected {signal:?}")
                };
                let remap = listing.apply(change);
                selected = selected.and_then(|row| remap.get(row));
            }
            assert_eq!(
                selected.and_then(|row| listing.entry(row)).map(|e| &e.path),
                Some(&path("b"))
            );
        }
    }

    #[test]
    fn discarding_a_snapshot_does_not_confirm_it_as_the_callers_baseline() {
        let start = Instant::now();
        let clock = Cell::new(start);
        let disk = Cell::new("a");
        let mut worker = worker(start, None);
        let io = Cycle {
            wake: &|| {},
            restart: &|| {},
            scan: &|_| Ok(Some(snapshot(disk.get()))),
            now: &|| clock.get(),
        };
        let mut seen = 0;
        tick(&mut worker, &mut seen, &io, None);
        tick(&mut worker, &mut seen, &io, Some(true));
        assert_eq!(worker.shared.take(), [Signal::Rescan]); // legacy reread could miss a
        clock.set(start + Duration::from_secs(1));
        tick(&mut worker, &mut seen, &io, None);
        tick(&mut worker, &mut seen, &io, Some(true));
        let (signals, baseline) = worker.shared.take_with_snapshot();
        assert_eq!(signals, [Signal::Rescan]);
        assert_eq!(baseline, Some(vec![entry("a")]));

        let mut listing = Listing::new();
        listing.set_folder(Some("folder".into()));
        listing.set_entries(baseline.unwrap());
        disk.set("b");
        clock.set(start + Duration::from_secs(2));
        tick(&mut worker, &mut seen, &io, None);
        tick(&mut worker, &mut seen, &io, Some(true));
        let (signals, baseline) = worker.shared.take_with_snapshot();
        assert!(baseline.is_none());
        for signal in signals {
            let Signal::Changed(change) = signal else {
                panic!("unexpected {signal:?}")
            };
            listing.apply(change);
        }
        assert_eq!(
            listing
                .visible()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            ["b"]
        );
    }

    #[test]
    fn scans_wait_for_the_restart_result_and_continue_in_scan_only_mode_on_failure() {
        let start = Instant::now();
        let clock = Cell::new(start);
        let restarts = Cell::new(0);
        let scans = Cell::new(0);
        let mut worker = worker(start, Some(vec![entry("a")]));
        let io = Cycle {
            wake: &|| {},
            restart: &|| restarts.set(restarts.get() + 1),
            scan: &|_| {
                scans.set(scans.get() + 1);
                Ok(Some(snapshot("b")))
            },
            now: &|| clock.get(),
        };
        let mut seen = 0;
        tick(&mut worker, &mut seen, &io, None);
        tick(&mut worker, &mut seen, &io, None);
        assert_eq!(restarts.get(), 1);
        assert_eq!(scans.get(), 0);
        tick(&mut worker, &mut seen, &io, Some(false));
        assert_eq!(scans.get(), 1);
        assert_eq!(
            worker.shared.take().len(),
            2,
            "recovery survives the failed restart"
        );
        clock.set(start + Duration::from_secs(1));
        tick(&mut worker, &mut seen, &io, None);
        assert_eq!(scans.get(), 2);
        assert_eq!(restarts.get(), 1);
    }
}
