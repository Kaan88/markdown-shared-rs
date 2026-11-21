//! A worker thread and the handle the caller keeps: the work is a closure on its own thread, it
//! is given a cancel flag and a progress counter, and it wakes the caller, which reads the
//! progress and takes the result. The wake is a plain closure, so nothing here knows of winit,
//! of a window or of an event loop; the caller decides what a wake means.
//!
//! Deliberately not here:
//!
//! - no thread pool: a job is one thread, made when it starts and gone when the work returns;
//! - no queue: the work is the closure handed to [`Job::start`], and nothing is kept between
//!   jobs;
//! - no restart: a job runs once, and a second run is a second job;
//! - no ordering or limit between jobs: two jobs run at the same time and neither knows of the
//!   other, so the caller is the one that decides how many may run;
//! - no debounce and no coalescing of the work's own calls: if the work wants to be started only
//!   after an edit has rested, that decision is the caller's.
//!
//! What it does keep is small and fixed: one cancel flag, one pair of progress numbers, one slot
//! for the result, and the time of the last wake. All of it is shared by [`Arc`], so dropping the
//! handle neither stops the work nor frees anything it still writes into.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often progress may wake the caller at most. The end of the work is not held to it: it
/// always wakes once, so a caller waiting for an event is told the job is done at once.
pub const WAKE_EVERY: Duration = Duration::from_millis(40);

/// The state every part of a job shares. Not generic, so [`Cancel`] and [`Progress`] handed to
/// the work do not name the result's type.
struct Core {
    cancel: AtomicBool,
    /// True from the moment the thread starts until it has returned (or panicked).
    running: AtomicBool,
    counts: Mutex<Counts>,
}

struct Counts {
    at: usize,
    of: usize,
    /// When the last progress wake was sent, so the next one can be kept 40 ms away.
    last: Instant,
}

/// What the work asks whether it should stop. It is a flag, not a signal: it is set by
/// [`Job::cancel`], it stays set, and nothing happens until the work looks at it.
pub struct Cancel {
    core: Arc<Core>,
}

impl Cancel {
    /// True once the caller has cancelled. The work should return soon after it sees this; it is
    /// not interrupted.
    pub fn cancelled(&self) -> bool {
        self.core.cancel.load(Ordering::Relaxed)
    }
}

/// What the work reports its progress with.
pub struct Progress {
    core: Arc<Core>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Progress {
    /// Records `at` of `of` and wakes the caller if 40 ms have passed since the last wake.
    ///
    /// A report lower than the one before it in either number is ignored: the counters never go
    /// backwards, so a work that reports per phase keeps the largest count it reached. The
    /// caller's reading is not slowed by a wake it does not need.
    pub fn report(&self, at: usize, of: usize) {
        let wake_now = {
            let mut counts = self.core.counts.lock().unwrap_or_else(|e| e.into_inner());
            if at < counts.at || of < counts.of {
                return;
            }
            counts.at = at;
            counts.of = of;
            let now = Instant::now();
            if now.saturating_duration_since(counts.last) < WAKE_EVERY {
                return;
            }
            counts.last = now;
            true
        };
        // Woken with no lock held: the wake may do anything, including look at this job.
        if wake_now {
            (self.wake)();
        }
    }
}

/// A work on its own thread. The caller reads [`Job::progress`], asks [`Job::running`], takes
/// the result once with [`Job::take`], and may [`Job::cancel`] it.
pub struct Job<T> {
    core: Arc<Core>,
    done: Arc<Mutex<Option<T>>>,
}

impl<T: Send + 'static> Job<T> {
    /// Starts `work` on a thread, which calls `wake` as it reports progress and once when it
    /// ends. The wake may be called from that thread at any moment until the work returns, so it
    /// must be cheap, must not block on anything the caller holds, and must not panic.
    ///
    /// The error is the thread's, when one cannot be made: no work has run then.
    pub fn start<W, F>(wake: W, work: F) -> io::Result<Job<T>>
    where
        W: Fn() + Send + Sync + 'static,
        F: FnOnce(&Cancel, &Progress) -> T + Send + 'static,
    {
        let core = Arc::new(Core {
            cancel: AtomicBool::new(false),
            running: AtomicBool::new(true),
            counts: Mutex::new(Counts {
                at: 0,
                of: 0,
                last: Instant::now(),
            }),
        });
        let done = Arc::new(Mutex::new(None));
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let (their_core, their_done, their_wake) = (core.clone(), done.clone(), wake.clone());
        std::thread::Builder::new()
            .name("mdshared-job".into())
            .spawn(move || {
                let cancel = Cancel {
                    core: their_core.clone(),
                };
                let progress = Progress {
                    core: their_core.clone(),
                    wake: their_wake.clone(),
                };
                let mut ending = Ending {
                    core: their_core,
                    done: their_done,
                    out: None,
                    wake: their_wake,
                };
                ending.out = Some(work(&cancel, &progress));
            })?;
        Ok(Job { core, done })
    }

    /// The last progress the work reported, as `(at, of)`. `(0, 0)` until it reports.
    pub fn progress(&self) -> (usize, usize) {
        let counts = self.core.counts.lock().unwrap_or_else(|e| e.into_inner());
        (counts.at, counts.of)
    }

    /// True while the work runs; false once it has returned (or panicked) and its last wake has
    /// been sent, so a job that says false here will call the wake no more. The result is in
    /// place before that, if the work produced one.
    pub fn running(&self) -> bool {
        self.core.running.load(Ordering::Relaxed)
    }

    /// Whether [`Job::cancel`] was called. It does not say the work noticed: a work that never
    /// looks at its flag runs to its end.
    pub fn cancelled(&self) -> bool {
        self.core.cancel.load(Ordering::Relaxed)
    }

    /// Asks the work to stop. The flag is set and this returns; it does not wait, and it does
    /// not wake the caller, since there is nothing new to read until the work returns.
    pub fn cancel(&self) {
        self.core.cancel.store(true, Ordering::Relaxed);
    }

    /// The result, once the work has returned; `None` before then, `None` again after it was
    /// taken, and `None` when the work panicked. A cancelled job still hands back what its work
    /// returned, so a half-finished run can report what it did; the flag says it was cut short.
    pub fn take(&self) -> Option<T> {
        let mut done = self.done.lock().unwrap_or_else(|e| e.into_inner());
        done.take()
    }
}

/// Stores the result and wakes the caller when the thread ends, and does both when the work
/// panics, so a caller is never left with a job that runs for ever. Kept as a guard rather than
/// written at the end of the closure for that second case.
struct Ending<T> {
    core: Arc<Core>,
    done: Arc<Mutex<Option<T>>>,
    out: Option<T>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl<T> Drop for Ending<T> {
    fn drop(&mut self) {
        if let Some(out) = self.out.take() {
            let mut done = self.done.lock().unwrap_or_else(|e| e.into_inner());
            *done = Some(out);
        }
        // The wake goes before the flag: a caller that sees `running` false has had its last
        // wake already, and no wake can follow that it has not seen.
        (self.wake)();
        self.core.running.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// Waits for `done`, up to two seconds, so a broken job fails a test instead of hanging it.
    fn wait_for(mut done: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(2);
        while !done() {
            assert!(Instant::now() < end, "the worker did not finish in time");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_job_that_finishes_gives_its_result() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let woken = wakes.clone();
        let job = Job::start(
            move || {
                woken.fetch_add(1, Ordering::Relaxed);
            },
            |_, _| 7u32,
        )
        .unwrap();
        wait_for(|| !job.running());
        assert_eq!(job.take(), Some(7));
        assert_eq!(job.take(), None, "a result is handed over once");
        assert!(wakes.load(Ordering::Relaxed) >= 1, "the end wakes once");
        assert!(!job.cancelled());
    }

    #[test]
    fn a_job_cancelled_part_way_stops_and_says_it_was_cancelled() {
        let job = Job::start(
            || {},
            |cancel, _| {
                let mut turns = 0u64;
                while !cancel.cancelled() {
                    turns += 1;
                    if turns == 200_000 {
                        break;
                    }
                    std::thread::yield_now();
                }
                turns
            },
        )
        .unwrap();
        job.cancel();
        wait_for(|| !job.running());
        assert!(job.cancelled());
        let turns = job.take().expect("a cancelled job still says what it did");
        assert!(
            turns < 200_000,
            "the work ran to its own limit, so it never saw the flag"
        );
    }

    #[test]
    fn progress_never_goes_backwards_and_the_wake_comes_at_most_once_an_interval() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let woken = wakes.clone();
        let started = Instant::now();
        let job = Job::start(
            move || {
                woken.fetch_add(1, Ordering::Relaxed);
            },
            |_, progress| {
                for at in 0..200 {
                    progress.report(at, 400);
                }
                progress.report(150, 400);
            },
        )
        .unwrap();
        wait_for(|| !job.running());
        job.take();
        let span = started.elapsed();
        assert_eq!(job.progress(), (199, 400), "a lower report is ignored");
        let burst = wakes.load(Ordering::Relaxed);
        // One progress wake per 40 ms at most, however many reports came in, plus the one at the
        // end. Measured over the run, so a slow machine is allowed the extra wake it may need.
        let allowed = (span.as_millis() / WAKE_EVERY.as_millis()) as usize + 2;
        assert!(
            burst <= allowed,
            "200 reports in {span:?} woke the caller {burst} times, more than {allowed}"
        );

        // A report after the interval does wake, so a slow job is seen while it runs.
        let before = wakes.load(Ordering::Relaxed);
        let woken = wakes.clone();
        let job = Job::start(
            move || {
                woken.fetch_add(1, Ordering::Relaxed);
            },
            |_, progress| {
                progress.report(1, 2);
                std::thread::sleep(Duration::from_millis(60));
                progress.report(2, 2);
            },
        )
        .unwrap();
        wait_for(|| !job.running());
        job.take();
        assert_eq!(job.progress(), (2, 2));
        let after = wakes.load(Ordering::Relaxed);
        assert!(
            after - before >= 2,
            "a report 60 ms later did not wake the caller"
        );
    }

    #[test]
    fn dropping_the_handle_does_not_leave_the_thread_writing_into_freed_state() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let woken = wakes.clone();
        // The work waits here until the handle is gone, and only then touches its buffer.
        let gate = Arc::new(AtomicBool::new(false));
        let opened = gate.clone();
        let job = Job::start(
            move || {
                woken.fetch_add(1, Ordering::Relaxed);
            },
            move |_, progress| {
                progress.report(1, 2);
                while !opened.load(Ordering::Relaxed) {
                    std::thread::yield_now();
                }
                let mut held = vec![0u8; 1 << 16];
                for (i, byte) in held.iter_mut().enumerate() {
                    *byte = i as u8;
                }
                progress.report(2, 2);
                held
            },
        )
        .unwrap();
        drop(job);
        gate.store(true, Ordering::Relaxed);
        // The wake closure and the buffer belong to the thread now; the end of the work still
        // calls the one and fills the other, with nothing of the handle left behind.
        wait_for(|| wakes.load(Ordering::Relaxed) >= 1);
        assert!(wakes.load(Ordering::Relaxed) >= 1);
    }
}
