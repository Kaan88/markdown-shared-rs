//! A few worker threads and a queue in front of them, for many small pieces of work of one kind
//! whose results the caller collects: the thumbnails of a folder, the tags of the files of a
//! playlist, the sizes of directories. Where a [`Job`](crate::job::Job) is one long work on its
//! own thread, a pool is many short ones on a fixed number of threads.
//!
//! The caller submits work under a key, a priority and a generation, and is woken when results
//! wait; on its own thread it takes them with [`Pool::drain`]. Three things keep a pool from
//! doing work nobody wants any more:
//!
//! - **The key.** Work queued under a key that is queued already takes that entry's place, so
//!   asking twice for the thumbnail of one file queues it once.
//! - **The priority.** The highest runs first, and equal priorities run in the order they were
//!   submitted: the rows on screen before the rows below them.
//! - **The generation.** A number the caller raises when everything asked for so far is of no
//!   use (another folder was opened). Raising it empties the queue of older work, tells older
//!   work that is running to stop, and throws its result away if it finishes all the same.
//!
//! The wake is a plain closure, as a job's is, so nothing here knows of winit, of a window or of
//! an event loop.
//!
//! Deliberately not here:
//!
//! - no progress: a piece of work here is short, and the caller counts the results it has;
//! - no result for work that was dropped, replaced, made stale or lost to a panic: the caller is
//!   not told key by key, it asks [`Pool::pending`], [`Pool::running`] and [`Pool::lost`];
//! - no order of results: they come in the order the work finished;
//! - no reprioritising of queued work, other than by submitting it again under its key;
//! - no waiting: nothing here blocks the caller, not [`Pool::drain`] and not dropping the pool.

use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

/// What every piece of work can read without the pool's lock: the generation that counts, and
/// whether the pool is gone. Not generic, so a [`Token`] does not name the pool's types.
struct Flags {
    generation: AtomicU64,
    closed: AtomicBool,
}

/// What a piece of work asks whether its result is still wanted. It is a flag, not a signal:
/// nothing happens until the work looks at it, and short work need not look at all.
pub struct Token {
    flags: Arc<Flags>,
    generation: u64,
}

impl Token {
    /// True once the pool's generation has passed this work's, or the pool was dropped: the
    /// result will be thrown away, so the work should return soon. What it returns then does
    /// not matter.
    pub fn cancelled(&self) -> bool {
        self.flags.closed.load(Ordering::Relaxed)
            || self.flags.generation.load(Ordering::Relaxed) > self.generation
    }

    /// The generation the work was submitted under.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

type Work<T> = Box<dyn FnOnce(&Token) -> T + Send>;

struct Queued<K, T> {
    key: K,
    priority: i32,
    generation: u64,
    /// The order of submission, to break ties of priority.
    seq: u64,
    work: Work<T>,
}

struct State<K, T> {
    queue: Vec<Queued<K, T>>,
    done: Vec<(K, T)>,
    seq: u64,
    running: usize,
    /// Worker threads that exist, and those of them waiting for work.
    threads: usize,
    idle: usize,
    lost: usize,
    /// A wake was sent and the caller has not drained since: it needs no second one.
    woken: bool,
}

struct Shared<K, T> {
    state: Mutex<State<K, T>>,
    more: Condvar,
    flags: Arc<Flags>,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl<K, T> Shared<K, T> {
    fn lock(&self) -> MutexGuard<'_, State<K, T>> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A queue of work and the threads that run it. See the [module](self).
///
/// `K` names a piece of work (a path, an index, a pair of them) and comes back with its result;
/// `T` is the result. All work of one pool gives the same type; a caller with two kinds of work
/// keeps two pools or an enum.
pub struct Pool<K, T> {
    shared: Arc<Shared<K, T>>,
    workers: usize,
}

impl<K, T> Pool<K, T>
where
    K: PartialEq + Send + 'static,
    T: Send + 'static,
{
    /// A pool that runs at most `workers` pieces of work at once (at least one). No thread is
    /// made here: each is made when work is submitted and every existing one is busy, so a pool
    /// that is never used costs nothing.
    ///
    /// `wake` is called from a worker thread when a result waits that the caller has not been
    /// told of, and when the last work has ended (so a caller that shows "working" can stop,
    /// also when that last work gave no result). Between a wake and the next [`drain`]
    /// (Self::drain) further results send no further wake: a hundred thumbnails that finish
    /// before the caller looks cost one. It must be cheap, must not block on anything the
    /// caller holds, and must not panic.
    pub fn new(workers: usize, wake: impl Fn() + Send + Sync + 'static) -> Pool<K, T> {
        Pool {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    queue: Vec::new(),
                    done: Vec::new(),
                    seq: 0,
                    running: 0,
                    threads: 0,
                    idle: 0,
                    lost: 0,
                    woken: false,
                }),
                more: Condvar::new(),
                flags: Arc::new(Flags {
                    generation: AtomicU64::new(0),
                    closed: AtomicBool::new(false),
                }),
                wake: Box::new(wake),
            }),
            workers: workers.max(1),
        }
    }

    /// Queues `work` under `key`. The highest `priority` runs first; equal ones in the order
    /// they were submitted. Returns whether it was queued: work of a `generation` older than
    /// the pool's ([`set_generation`](Self::set_generation)) is of no use already and is
    /// dropped here, unrun, with `Ok(false)`.
    ///
    /// Work already queued under an equal key is dropped unrun and this one takes its place,
    /// with its own priority and at the back of that priority. Work under that key that is
    /// *running* is left alone: it finishes, and the caller gets both results, the older first.
    ///
    /// The error is a thread's, when the pool has none yet and cannot make one: nothing was
    /// queued then. With at least one thread alive, a second that cannot be made is not an
    /// error; the work waits for the ones there are.
    pub fn submit<F>(&self, key: K, priority: i32, generation: u64, work: F) -> io::Result<bool>
    where
        F: FnOnce(&Token) -> T + Send + 'static,
    {
        // Whatever is dropped is dropped after the lock is let go: a closure's captures may do
        // anything in their own drop.
        let replaced;
        {
            let mut state = self.shared.lock();
            if generation < self.shared.flags.generation.load(Ordering::Relaxed) {
                return Ok(false);
            }
            if state.idle == 0 && state.threads < self.workers {
                let shared = self.shared.clone();
                let made = std::thread::Builder::new()
                    .name("mdshared-pool".into())
                    .spawn(move || worker(shared));
                match made {
                    Ok(_) => state.threads += 1,
                    Err(e) if state.threads == 0 => return Err(e),
                    Err(_) => {}
                }
            }
            replaced = state
                .queue
                .iter()
                .position(|q| q.key == key)
                .map(|i| state.queue.remove(i));
            state.seq += 1;
            let seq = state.seq;
            state.queue.push(Queued {
                key,
                priority,
                generation,
                seq,
                work: Box::new(work),
            });
        }
        self.shared.more.notify_one();
        drop(replaced);
        Ok(true)
    }

    /// Says that work of a generation below `generation` is of no use any more: what is queued
    /// of it is dropped unrun, what is running of it sees [`Token::cancelled`] and has its
    /// result thrown away when it ends, and results of it that wait to be drained are thrown
    /// away too. Returns how many queued pieces were dropped. A `generation` not above the
    /// pool's changes nothing: it only goes up. A pool starts at zero.
    pub fn set_generation(&self, generation: u64) -> usize {
        let dropped: Vec<Queued<K, T>>;
        let stale: Vec<(K, T)>;
        {
            let mut state = self.shared.lock();
            let flags = &self.shared.flags;
            if generation <= flags.generation.load(Ordering::Relaxed) {
                return 0;
            }
            flags.generation.store(generation, Ordering::Relaxed);
            let (keep, gone) = std::mem::take(&mut state.queue)
                .into_iter()
                .partition(|q| q.generation >= generation);
            state.queue = keep;
            dropped = gone;
            // Results carry no generation once they are done, and they are all older than the
            // one that counts now only if they were finished before this call: all of them.
            stale = std::mem::take(&mut state.done);
            // Nothing waits now, so the next result has to wake the caller afresh.
            state.woken = false;
        }
        drop(stale);
        dropped.len()
    }

    /// The generation that counts: the highest [`set_generation`](Self::set_generation) so far.
    pub fn generation(&self) -> u64 {
        self.shared.flags.generation.load(Ordering::Relaxed)
    }

    /// Takes the results that wait, each with the key it was submitted under, in the order the
    /// work finished. Empty when none wait. It never blocks on the work. After it, the next
    /// result wakes the caller again.
    pub fn drain(&self) -> Vec<(K, T)> {
        let mut state = self.shared.lock();
        state.woken = false;
        std::mem::take(&mut state.done)
    }

    /// How many pieces of work wait in the queue.
    pub fn pending(&self) -> usize {
        self.shared.lock().queue.len()
    }

    /// How many pieces of work run now: never more than the pool's workers.
    pub fn running(&self) -> usize {
        self.shared.lock().running
    }

    /// Whether nothing is queued and nothing runs. Results may still wait to be drained.
    pub fn is_idle(&self) -> bool {
        let state = self.shared.lock();
        state.queue.is_empty() && state.running == 0
    }

    /// How many pieces of work panicked since the pool was made. Each lost its own result and
    /// nothing else: the thread it ran on went on to the next piece.
    pub fn lost(&self) -> usize {
        self.shared.lock().lost
    }

    /// Whether `key` waits in the queue (not whether it runs or is done).
    pub fn queued(&self, key: &K) -> bool {
        self.shared.lock().queue.iter().any(|q| &q.key == key)
    }
}

/// Dropping the pool does not wait. What is queued is dropped unrun; what is running sees
/// [`Token::cancelled`], runs to its own end on its thread, and its result is thrown away; the
/// wake is not called again; the threads end as they come free.
impl<K, T> Drop for Pool<K, T> {
    fn drop(&mut self) {
        self.shared.flags.closed.store(true, Ordering::Relaxed);
        let (queue, done) = {
            let mut state = self.shared.lock();
            (
                std::mem::take(&mut state.queue),
                std::mem::take(&mut state.done),
            )
        };
        self.shared.more.notify_all();
        drop(queue);
        drop(done);
    }
}

/// One worker thread: takes the best piece of the queue, runs it with no lock held, stores what
/// it gave, and waits when there is none.
fn worker<K, T>(shared: Arc<Shared<K, T>>) {
    let mut state = shared.lock();
    loop {
        if shared.flags.closed.load(Ordering::Relaxed) {
            state.threads -= 1;
            return;
        }
        // The highest priority, and of those the first submitted.
        let best = state
            .queue
            .iter()
            .enumerate()
            .max_by_key(|(_, q)| (q.priority, std::cmp::Reverse(q.seq)))
            .map(|(i, _)| i);
        let Some(best) = best else {
            state.idle += 1;
            state = shared.more.wait(state).unwrap_or_else(|e| e.into_inner());
            state.idle -= 1;
            continue;
        };
        let Queued {
            key,
            generation,
            work,
            ..
        } = state.queue.remove(best);
        state.running += 1;
        drop(state);

        let token = Token {
            flags: shared.flags.clone(),
            generation,
        };
        // A panic in the work is the work's own: it loses its result and the thread goes on.
        let out = catch_unwind(AssertUnwindSafe(|| work(&token)));

        state = shared.lock();
        state.running -= 1;
        let closed = shared.flags.closed.load(Ordering::Relaxed);
        let wanted = !closed && generation >= shared.flags.generation.load(Ordering::Relaxed);
        let mut unwanted = None;
        let mut has_result = false;
        match out {
            Ok(value) if wanted => {
                state.done.push((key, value));
                has_result = true;
            }
            Ok(value) => unwanted = Some((key, value)),
            Err(_) => state.lost += 1,
        }
        let idle = state.queue.is_empty() && state.running == 0;
        let wake = !closed && ((has_result && !state.woken) || idle);
        if wake && !state.done.is_empty() {
            state.woken = true;
        }
        if wake || unwanted.is_some() {
            // Woken, and a result dropped, with no lock held: either may do anything, including
            // look at this pool.
            drop(state);
            drop(unwanted);
            if wake {
                (shared.wake)();
            }
            state = shared.lock();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::time::{Duration, Instant};

    const LONG: Duration = Duration::from_secs(5);

    /// Waits for `done`, up to five seconds, so a broken pool fails a test instead of hanging it.
    fn wait_for(mut done: impl FnMut() -> bool) {
        let end = Instant::now() + LONG;
        while !done() {
            assert!(Instant::now() < end, "the pool did not get there in time");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// A pool that counts its wakes.
    fn pool<K, T>(workers: usize) -> (Pool<K, T>, Arc<AtomicUsize>)
    where
        K: PartialEq + Send + 'static,
        T: Send + 'static,
    {
        let wakes = Arc::new(AtomicUsize::new(0));
        let woken = wakes.clone();
        let pool = Pool::new(workers, move || {
            woken.fetch_add(1, Ordering::SeqCst);
        });
        (pool, wakes)
    }

    /// A gate a piece of work waits at: the work holds the receiver, the test the sender.
    fn gate() -> (Sender<()>, Receiver<()>) {
        channel()
    }

    /// Work that says it started, waits at its gate, then gives `value`.
    fn held(
        started: Sender<u32>,
        gate: Receiver<()>,
        value: u32,
    ) -> impl FnOnce(&Token) -> u32 + Send + 'static {
        move |_| {
            started.send(value).unwrap();
            gate.recv_timeout(LONG).expect("the gate was never opened");
            value
        }
    }

    #[test]
    fn work_gives_its_result_under_its_key_and_the_pool_wakes_the_caller() {
        let (pool, wakes) = pool::<&str, u32>(2);
        assert!(pool.is_idle());
        assert_eq!(pool.drain(), vec![]);
        assert!(pool.submit("a", 0, 0, |_| 7).unwrap());
        wait_for(|| wakes.load(Ordering::SeqCst) >= 1);
        wait_for(|| pool.is_idle());
        assert_eq!(pool.drain(), vec![("a", 7)]);
        assert_eq!(pool.drain(), vec![], "a result is handed over once");
        assert_eq!((pool.pending(), pool.running(), pool.lost()), (0, 0, 0));
    }

    #[test]
    fn the_highest_priority_runs_first_and_equal_ones_in_the_order_submitted() {
        let (pool, _) = pool::<&str, u32>(1);
        let (started, starts) = channel();
        let (open, wait) = gate();
        pool.submit("first", 0, 0, held(started, wait, 0)).unwrap();
        // The one worker is inside the first piece; everything below queues behind it.
        assert_eq!(starts.recv_timeout(LONG), Ok(0));
        let order = Arc::new(Mutex::new(Vec::new()));
        for (key, priority) in [
            ("low", 0),
            ("mid-1", 5),
            ("mid-2", 5),
            ("high", 9),
            ("neg", -3),
        ] {
            let order = order.clone();
            let queued = pool.submit(key, priority, 0, move |_| {
                order.lock().unwrap().push(key);
                1
            });
            assert!(queued.unwrap());
        }
        assert_eq!((pool.pending(), pool.running()), (5, 1));
        assert!(pool.queued(&"mid-2") && !pool.queued(&"first"));
        open.send(()).unwrap();
        wait_for(|| pool.is_idle());
        assert_eq!(
            *order.lock().unwrap(),
            ["high", "mid-1", "mid-2", "low", "neg"]
        );
        // The results come in the order the work finished.
        let keys: Vec<&str> = pool.drain().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["first", "high", "mid-1", "mid-2", "low", "neg"]);
    }

    #[test]
    fn work_queued_under_a_queued_key_takes_its_place_and_running_work_is_left_alone() {
        let (pool, _) = pool::<&str, u32>(1);
        let (started, starts) = channel();
        let (open, wait) = gate();
        pool.submit("a", 0, 0, held(started, wait, 1)).unwrap();
        assert_eq!(starts.recv_timeout(LONG), Ok(1));
        // "b" twice while it waits: one entry, the second one's work. Its first closure is
        // dropped unrun, which its guard shows.
        let dropped = Arc::new(AtomicUsize::new(0));
        struct Guard(Arc<AtomicUsize>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let guard = Guard(dropped.clone());
        pool.submit("b", 0, 0, move |_| {
            let _held = &guard;
            10
        })
        .unwrap();
        pool.submit("c", 0, 0, |_| 30).unwrap();
        pool.submit("b", 0, 0, |_| 20).unwrap();
        assert_eq!(pool.pending(), 2);
        assert_eq!(
            dropped.load(Ordering::SeqCst),
            1,
            "the replaced work was dropped"
        );
        // "a" again while "a" runs: queued beside it, and both results arrive.
        pool.submit("a", 0, 0, |_| 2).unwrap();
        assert_eq!(pool.pending(), 3);
        open.send(()).unwrap();
        wait_for(|| pool.is_idle());
        // "b" went to the back of its priority when it was submitted again.
        assert_eq!(pool.drain(), vec![("a", 1), ("c", 30), ("b", 20), ("a", 2)]);
    }

    #[test]
    fn a_new_generation_drops_queued_work_and_discards_what_finishes_late() {
        let (pool, wakes) = pool::<&str, u32>(1);
        let (started, starts) = channel();
        let (open, wait) = gate();
        let saw_cancel = Arc::new(AtomicBool::new(false));
        let saw = saw_cancel.clone();
        pool.submit("running", 0, 1, move |token: &Token| {
            assert!(!token.cancelled());
            assert_eq!(token.generation(), 1);
            started.send(0).unwrap();
            wait.recv_timeout(LONG).expect("the gate was never opened");
            saw.store(token.cancelled(), Ordering::SeqCst);
            1
        })
        .unwrap();
        assert_eq!(starts.recv_timeout(LONG), Ok(0));
        let ran = Arc::new(AtomicUsize::new(0));
        for key in ["old-1", "old-2"] {
            let ran = ran.clone();
            pool.submit(key, 0, 1, move |_| {
                ran.fetch_add(1, Ordering::SeqCst);
                2
            })
            .unwrap();
        }
        pool.submit("new", 0, 2, |_| 3).unwrap();
        assert_eq!(pool.pending(), 3);

        // Another folder was opened: only the work of generation 2 is still wanted.
        assert_eq!(pool.set_generation(2), 2);
        assert_eq!((pool.generation(), pool.pending()), (2, 1));
        // Going back, or staying, changes nothing.
        assert_eq!(pool.set_generation(1), 0);
        assert_eq!(pool.set_generation(2), 0);
        assert_eq!(pool.generation(), 2);
        // Work of the old generation is refused from now on.
        assert!(!pool.submit("late", 0, 1, |_| 4).unwrap());
        assert_eq!(pool.pending(), 1);

        open.send(()).unwrap();
        wait_for(|| pool.is_idle());
        assert!(
            saw_cancel.load(Ordering::SeqCst),
            "the running work was told"
        );
        assert_eq!(ran.load(Ordering::SeqCst), 0, "dropped work never ran");
        assert_eq!(
            pool.drain(),
            vec![("new", 3)],
            "the late result was thrown away"
        );
        assert!(wakes.load(Ordering::SeqCst) >= 1);

        // Results that waited undrained when the generation moved are thrown away as well.
        pool.submit("waits", 0, 2, |_| 5).unwrap();
        wait_for(|| pool.is_idle());
        pool.set_generation(3);
        assert_eq!(pool.drain(), vec![]);
    }

    #[test]
    fn no_more_work_runs_at_once_than_the_pool_has_workers() {
        let (pool, _) = pool::<u32, u32>(3);
        let (started, starts) = channel();
        let mut gates = Vec::new();
        for i in 0..8 {
            let (open, wait) = gate();
            gates.push(open);
            pool.submit(i, 0, 0, held(started.clone(), wait, i))
                .unwrap();
        }
        // Three start, in the order submitted, and the other five wait.
        let mut first: Vec<u32> = (0..3).map(|_| starts.recv_timeout(LONG).unwrap()).collect();
        first.sort();
        assert_eq!(first, [0, 1, 2]);
        assert_eq!((pool.running(), pool.pending()), (3, 5));
        assert!(
            starts.recv_timeout(Duration::from_millis(50)).is_err(),
            "a fourth piece started beside three"
        );
        // One ends: exactly one more starts.
        gates[1].send(()).unwrap();
        assert_eq!(starts.recv_timeout(LONG), Ok(3));
        wait_for(|| pool.running() == 3 && pool.pending() == 4);
        for open in &gates {
            let _ = open.send(());
        }
        wait_for(|| pool.is_idle());
        let mut done = pool.drain();
        done.sort();
        assert_eq!(done, (0..8).map(|i| (i, i)).collect::<Vec<_>>());
        // A pool asked for no workers still has one.
        let (one, _) = super::tests::pool::<u32, u32>(0);
        one.submit(1, 0, 0, |_| 1).unwrap();
        wait_for(|| one.is_idle());
        assert_eq!(one.drain(), vec![(1, 1)]);
    }

    #[test]
    fn a_panic_loses_its_own_result_only_and_the_thread_goes_on() {
        let (pool, wakes) = pool::<&str, u32>(1);
        pool.submit("before", 0, 0, |_| 1).unwrap();
        pool.submit("breaks", 0, 0, |_| {
            panic!("a work of the pool's test panics on purpose")
        })
        .unwrap();
        pool.submit("after", 0, 0, |_| 3).unwrap();
        wait_for(|| pool.is_idle());
        assert_eq!(pool.lost(), 1);
        assert_eq!(pool.drain(), vec![("before", 1), ("after", 3)]);
        // The pool is as usable as before, on the same single thread.
        pool.submit("again", 0, 0, |_| 4).unwrap();
        wait_for(|| pool.is_idle());
        assert_eq!(pool.drain(), vec![("again", 4)]);
        // A panic as the last work still wakes: the caller learns the pool is idle.
        let before = wakes.load(Ordering::SeqCst);
        pool.submit("last", 0, 0, |_| panic!("the last work panics on purpose"))
            .unwrap();
        wait_for(|| pool.lost() == 2 && pool.is_idle());
        wait_for(|| wakes.load(Ordering::SeqCst) > before);
        assert_eq!(pool.drain(), vec![]);
    }

    #[test]
    fn results_the_caller_has_not_drained_wake_it_once() {
        let (pool, wakes) = pool::<u32, u32>(1);
        let (started, starts) = channel();
        let (open, wait) = gate();
        // A piece that holds the worker while twenty quick ones queue, and one at the end that
        // holds it again, so the pool is not idle while the count is read.
        pool.submit(100, 9, 0, held(started.clone(), wait, 100))
            .unwrap();
        assert_eq!(starts.recv_timeout(LONG), Ok(100));
        for i in 0..20 {
            pool.submit(i, 0, 0, move |_| i).unwrap();
        }
        let (open_last, wait_last) = gate();
        pool.submit(200, -9, 0, held(started, wait_last, 200))
            .unwrap();
        open.send(()).unwrap();
        assert_eq!(starts.recv_timeout(LONG), Ok(200));
        // Twenty-one results wait, and the caller was woken for the first of them only.
        assert_eq!(wakes.load(Ordering::SeqCst), 1);
        assert_eq!(pool.drain().len(), 21);
        // After a drain the next result wakes again, and so does the pool going idle.
        open_last.send(()).unwrap();
        wait_for(|| pool.is_idle());
        wait_for(|| wakes.load(Ordering::SeqCst) == 2);
        assert_eq!(pool.drain(), vec![(200, 200)]);
    }

    #[test]
    fn dropping_the_pool_drops_queued_work_and_does_not_wait_for_running_work() {
        let (pool, wakes) = pool::<&str, u32>(1);
        let (started, starts) = channel();
        let (open, wait) = gate();
        let (ended, end) = channel();
        pool.submit("running", 0, 0, move |token: &Token| {
            started.send(0).unwrap();
            wait.recv_timeout(LONG).expect("the gate was never opened");
            ended.send(token.cancelled()).unwrap();
            1
        })
        .unwrap();
        assert_eq!(starts.recv_timeout(LONG), Ok(0));
        let ran = Arc::new(AtomicUsize::new(0));
        let counted = ran.clone();
        pool.submit("queued", 0, 0, move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            2
        })
        .unwrap();
        // The drop returns while the first piece is still at its gate.
        drop(pool);
        open.send(()).unwrap();
        assert_eq!(
            end.recv_timeout(LONG),
            Ok(true),
            "the running work was told"
        );
        // The thread ends without running what was queued and without a wake.
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(ran.load(Ordering::SeqCst), 0);
        assert_eq!(wakes.load(Ordering::SeqCst), 0);
    }
}
