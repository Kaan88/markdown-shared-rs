//! Coordinate publishing and discovery without reading another generation's record.
//! A listener holds its lifetime lock only after taking the exclusive publication gate.
//! Readers hold the shared gate while they check the lifetime lock and read the record.
//! If startup fails, the lifetime lock drops before the gate reopens.

use super::{POLL, STARTUP, record};
use record::{Directory, Record};
use std::fs::{File, TryLockError};
use std::time::{Duration, Instant};

pub(super) struct Registration {
    _lifetime: File,
    gate: Option<File>,
}

impl Registration {
    pub(super) fn publish_held(&mut self, dir: &Directory, port: u16) -> Option<Record> {
        self.gate.as_ref()?;
        let record = Record::publish(dir, port)?;
        drop(self.gate.take());
        Some(record)
    }

    #[cfg(test)]
    pub(super) fn publish(mut self, dir: &Directory, port: u16) -> Option<(File, Record)> {
        let record = self.publish_held(dir, port)?;
        Some((self._lifetime, record))
    }
}

fn listener_holds(file: &File) -> bool {
    matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock))
}

pub(super) fn instance_alive(dir: &Directory) -> bool {
    record::lock(dir, false).is_some_and(|file| listener_holds(&file))
}

pub(super) fn hold_lock(dir: &Directory, mut pause: impl FnMut(Duration)) -> Option<Registration> {
    let gate = record::gate(dir, true)?;
    let end = Instant::now() + STARTUP;
    loop {
        match gate.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) if retry(end, &mut pause) => {}
            _ => return None,
        }
    }
    let lifetime = record::lock(dir, true)?;
    loop {
        match lifetime.try_lock() {
            Ok(()) => {
                return Some(Registration {
                    _lifetime: lifetime,
                    gate: Some(gate),
                });
            }
            Err(TryLockError::WouldBlock) => {
                // We hold the gate, so another exclusive lifetime lock belongs to a
                // published listener, not a startup with an old record still on disk.
                if instance_alive(dir) || !retry(end, &mut pause) {
                    return None;
                }
            }
            Err(TryLockError::Error(_)) => return None,
        }
    }
}

pub(super) enum Discovery {
    Absent,
    Starting,
    Ready(Record),
}

pub(super) fn discover(dir: &Directory) -> Discovery {
    let Some(gate) = record::gate(dir, false) else {
        return Discovery::Absent;
    };
    match gate.try_lock_shared() {
        Err(TryLockError::WouldBlock) => Discovery::Starting,
        Err(TryLockError::Error(_)) => Discovery::Absent,
        Ok(()) if instance_alive(dir) => {
            Record::load(dir).map_or(Discovery::Absent, Discovery::Ready)
        }
        Ok(()) => Discovery::Absent,
    }
}

pub(super) fn retry(end: Instant, pause: &mut impl FnMut(Duration)) -> bool {
    let left = end.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return false;
    }
    pause(left.min(POLL));
    true
}
