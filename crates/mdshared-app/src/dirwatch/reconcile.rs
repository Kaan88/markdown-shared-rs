//! Periodic recovery for native backends that can miss events. The worker owns the
//! snapshot and clock deadline; no separate timer can start or queue another scan.

use super::queues::LIMIT;
use super::{Change, Entry, Signal};
use crate::dirlist::read_entry_checked;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(super) const DEFAULT_INTERVAL: Option<Duration> = if cfg!(windows) {
    Some(Duration::from_secs(30))
} else {
    None
};
pub(super) const MIN_INTERVAL: Duration = Duration::from_secs(1);
const RESTART_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) type Snapshot = BTreeMap<PathBuf, Entry>;
pub(super) type Scan = io::Result<Option<Snapshot>>;

#[derive(PartialEq, Eq)]
enum Stage {
    Start,
    Waiting,
    Read,
    ScanOnly,
}

pub(super) struct Periodic {
    interval: Duration,
    deadline: Option<Instant>,
    /// What the caller should have after applying the reports, not just the last scan.
    known: Option<Snapshot>,
    epoch: u64,
    stage: Stage,
    reset_after_native: bool,
}

impl Periodic {
    pub(super) fn new(
        interval: Duration,
        now: Instant,
        initial: Option<Vec<Entry>>,
    ) -> io::Result<Self> {
        let interval = interval.max(MIN_INTERVAL);
        let deadline = now.checked_add(interval).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "directory reconciliation interval is too long",
            )
        })?;
        Ok(Self {
            interval,
            deadline: Some(deadline),
            known: initial.map(snapshot),
            epoch: 0,
            stage: Stage::Start,
            reset_after_native: false,
        })
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    #[cfg(test)]
    pub(super) fn make_due(&mut self, now: Instant) {
        self.deadline = Some(now);
    }

    /// Keep the snapshot aligned with incremental reports. If the caller needs to
    /// replace its own list, the next full read must establish a fresh baseline.
    pub(super) fn observe(&mut self, signals: &[Signal]) {
        for signal in signals {
            let Some(known) = &mut self.known else { return };
            match signal {
                Signal::Changed(Change::Created(entry) | Change::Modified(entry)) => {
                    known.insert(entry.path.clone(), entry.clone());
                }
                Signal::Changed(Change::Removed(path)) => {
                    known.remove(path);
                }
                Signal::Changed(Change::Renamed { from, to }) => {
                    known.remove(from);
                    known.insert(to.path.clone(), to.clone());
                }
                Signal::Rescan | Signal::Lost => self.known = None,
            }
        }
    }

    pub(super) fn restart_finished(&mut self, succeeded: bool, at: Instant) {
        if self.stage == Stage::Waiting {
            if self.deadline.is_some_and(|deadline| at >= deadline) {
                self.stage = Stage::ScanOnly;
            } else {
                self.deadline = Some(at);
                self.stage = if succeeded {
                    Stage::Read
                } else {
                    Stage::ScanOnly
                };
            }
        }
    }

    /// Returns whether a read is due. On the first call for a cycle, request a restart
    /// and wait for its acknowledgement, but not beyond the restart deadline.
    pub(super) fn ready(&mut self, now: Instant, restart: impl FnOnce()) -> bool {
        if self.deadline().is_none_or(|deadline| now < deadline) {
            return false;
        }
        match self.stage {
            Stage::Start => {
                if let Some(deadline) = now.checked_add(RESTART_TIMEOUT) {
                    self.deadline = Some(deadline);
                    self.stage = Stage::Waiting;
                    restart();
                    return false;
                }
                self.stage = Stage::ScanOnly;
            }
            Stage::Waiting => self.stage = Stage::ScanOnly,
            Stage::Read | Stage::ScanOnly => {}
        }
        true
    }

    pub(super) fn sync_epoch(&mut self, epoch: u64) {
        if self.epoch != epoch {
            self.epoch = epoch;
            self.known = None;
        }
    }

    pub(super) fn defer_reset(&mut self) {
        self.reset_after_native = true;
    }

    /// After the native burst, request a plain rescan without another attempt at an exact snapshot.
    pub(super) fn finish_deferred_reset(&mut self, now: Instant) -> bool {
        if !std::mem::take(&mut self.reset_after_native) {
            return false;
        }
        self.known = None;
        self.schedule_next(now);
        true
    }

    /// Prepare a report without mutating the baseline: events or queue resets may
    /// invalidate it between reading the directory and publishing the result.
    pub(super) fn report(&self, result: &Scan) -> (Vec<Signal>, Option<Vec<Entry>>) {
        match result {
            Ok(Some(current)) => {
                let signals = self
                    .known
                    .as_ref()
                    .map_or_else(|| vec![Signal::Rescan], |known| differences(known, current));
                let reset = (signals.first() == Some(&Signal::Rescan))
                    .then(|| current.values().cloned().collect());
                (signals, reset)
            }
            Ok(None) => (vec![Signal::Lost], None),
            Err(_) => (Vec::new(), None),
        }
    }

    /// Only what was published can advance the baseline. The excluded paths remain
    /// as last reported until native changes settle, or a later scan can correct them.
    pub(super) fn committed(
        &mut self,
        result: Scan,
        signals: &[Signal],
        exact: bool,
        end: Instant,
    ) {
        if exact {
            self.known = result.ok().flatten();
        } else {
            self.observe(signals);
        }
        self.schedule_next(end);
    }

    fn schedule_next(&mut self, end: Instant) {
        self.deadline = end.checked_add(self.interval);
        if self.stage == Stage::Read {
            self.stage = Stage::Start;
        }
    }
}

fn snapshot(entries: impl IntoIterator<Item = Entry>) -> Snapshot {
    entries
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect()
}

fn differences(old: &Snapshot, new: &Snapshot) -> Vec<Signal> {
    let mut signals = Vec::new();
    for path in old.keys().filter(|path| !new.contains_key(*path)) {
        if signals.len() == LIMIT {
            return vec![Signal::Rescan];
        }
        signals.push(Signal::Changed(Change::Removed(path.clone())));
    }
    for (path, entry) in new {
        let change = match old.get(path) {
            Some(before) if before == entry => continue,
            Some(_) => Change::Modified(entry.clone()),
            None => Change::Created(entry.clone()),
        };
        if signals.len() == LIMIT {
            return vec![Signal::Rescan];
        }
        signals.push(Signal::Changed(change));
    }
    signals
}

/// A full read used for diffing must be complete. Skip only children that vanished
/// after enumeration reached them. An error opening a missing root is distinct from
/// an enumeration error, which could otherwise falsely delete a large part of the list.
pub(super) fn scan(
    root: &Path,
    folder: &Path,
    stopped: impl Fn() -> bool,
) -> io::Result<Option<Snapshot>> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    let mut current = Snapshot::new();
    for item in entries {
        if stopped() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let item = item?;
        match read_entry_checked(&item) {
            Ok(mut entry) => {
                entry.path = folder.join(item.file_name());
                current.insert(entry.path.clone(), entry);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(Some(current))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dirlist::Kind;
    use std::cell::Cell;

    fn entry(path: &str, size: u64) -> Entry {
        Entry {
            path: path.into(),
            name: path.to_string(),
            kind: Kind::File,
            size,
            modified: None,
            created: None,
            hidden: false,
        }
    }

    fn scan_cycle(
        periodic: &mut Periodic,
        now: Instant,
        restart: impl Fn(),
        scan: impl FnOnce() -> Scan,
        end: Instant,
    ) -> Vec<Signal> {
        if !periodic.ready(now, &restart) {
            if periodic.stage != Stage::Waiting {
                return Vec::new();
            }
            periodic.restart_finished(true, now);
            assert!(periodic.ready(now, || panic!("started a second restart")));
        }
        let result = scan();
        let (signals, reset) = periodic.report(&result);
        periodic.committed(result, &signals, reset.is_some(), end);
        signals
    }

    #[test]
    fn a_slow_recovery_scan_does_not_queue_missed_cycles() {
        let start = Instant::now();
        let interval = Duration::from_secs(30);
        let mut periodic = Periodic::new(interval, start, Some(vec![])).unwrap();
        let restarts = Cell::new(0);
        let restart = || restarts.set(restarts.get() + 1);
        assert!(
            scan_cycle(
                &mut periodic,
                start,
                restart,
                || panic!("early scan"),
                start
            )
            .is_empty()
        );
        assert_eq!(restarts.get(), 0);
        let due = start + interval;
        let finish = due + interval * 10;
        assert!(
            scan_cycle(
                &mut periodic,
                due,
                restart,
                || {
                    assert_eq!(restarts.get(), 1, "restart requested before scanning");
                    Err(io::ErrorKind::PermissionDenied.into())
                },
                finish
            )
            .is_empty()
        );
        assert_eq!(periodic.deadline(), Some(finish + interval));
        assert!(
            scan_cycle(
                &mut periodic,
                finish,
                restart,
                || panic!("catch-up scan"),
                finish
            )
            .is_empty()
        );
        assert_eq!(restarts.get(), 1);
    }

    #[test]
    fn recovery_diffs_against_native_changes_already_reported() {
        let start = Instant::now();
        let interval = Duration::from_secs(1);
        let mut periodic = Periodic::new(
            interval,
            start,
            Some(vec![
                entry("gone", 1),
                entry("changed", 2),
                entry("old-name", 3),
            ]),
        )
        .unwrap();
        periodic.observe(&[Signal::Changed(Change::Renamed {
            from: "old-name".into(),
            to: entry("new-name", 3),
        })]);
        let disk = || snapshot([entry("changed", 4), entry("new-name", 3), entry("new", 5)]);
        let due = start + interval;
        assert_eq!(
            scan_cycle(&mut periodic, due, || {}, || Ok(Some(disk())), due),
            [
                Signal::Changed(Change::Removed("gone".into())),
                Signal::Changed(Change::Modified(entry("changed", 4))),
                Signal::Changed(Change::Created(entry("new", 5))),
            ]
        );
        let next = due + interval;
        assert!(scan_cycle(&mut periodic, next, || {}, || Ok(Some(disk())), next).is_empty());
    }

    #[test]
    fn recovery_bootstraps_without_guessing_the_callers_entries() {
        let start = Instant::now();
        let mut periodic = Periodic::new(Duration::from_secs(1), start, None).unwrap();
        let due = periodic.deadline().unwrap();
        let disk = || Ok(Some(snapshot([entry("a", 1)])));
        assert_eq!(
            scan_cycle(&mut periodic, due, || {}, disk, due),
            [Signal::Rescan]
        );
        let next = periodic.deadline().unwrap();
        assert!(scan_cycle(&mut periodic, next, || {}, disk, next).is_empty());
        let missing = periodic.deadline().unwrap();
        assert_eq!(
            scan_cycle(&mut periodic, missing, || {}, || Ok(None), missing),
            [Signal::Lost]
        );
    }
}
