//! Background queues for what the editor's first open of a project used to do on the main
//! thread (round 44): shader pipeline compiles ([`shaders`]) and asset imports ([`imports`]:
//! models, texture decodes). The window keeps drawing while they run; the status bar shows
//! their [`Progress`] ("Compiling shaders 14/40", "Importing 120/850").
//!
//! A queue only runs jobs; what a job makes it publishes itself, through its [`Ticket`], into
//! whatever cache asked for it (the renderer picks the results up on a later frame and swaps
//! them in for the placeholder it drew meanwhile). Jobs start in the order they were queued.
//! [`Queue::cancel`] (project closed) drops the jobs not started yet and voids the tickets of
//! the running ones, so nothing of the old project lands in the next one's caches. A job that
//! fails or panics counts as done and leaves its error for the Console
//! ([`Queue::take_errors`]): a broken file never leaves the queue waiting.
//!
//! Nothing uses the queues unless asked to: the renderer only defers work while
//! [`crate::render::Renderer::set_background_loads`] is on (the editor's Scene and Game views),
//! so tests, golden shots, thumbnails and the player load everything at once, as before.

use std::collections::VecDeque;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// What a queue has finished of the work given to it since it was last idle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub done: u32,
    pub total: u32,
}

impl Progress {
    /// Jobs queued or running.
    pub fn busy(self) -> bool {
        self.done < self.total
    }

    /// Both queues' work together.
    pub fn plus(self, other: Progress) -> Progress {
        Progress {
            done: self.done + other.done,
            total: self.total + other.total,
        }
    }
}

type Job = Box<dyn FnOnce(&Ticket) -> Result<(), String> + Send>;
/// Run instead of the job's publish when the job panicked (see [`Queue::push_or`]).
type Failed = Box<dyn FnOnce(String) + Send>;

struct Queued {
    label: String,
    job: Job,
    failed: Option<Failed>,
}

#[derive(Default)]
struct State {
    queue: VecDeque<Queued>,
    /// Bumped by [`Queue::cancel`]: tickets of an older generation publish nothing.
    generation: u64,
    running: u32,
    done: u32,
    total: u32,
    errors: Vec<String>,
    workers: u32,
}

struct Shared {
    state: Mutex<State>,
    /// A job was queued.
    wake: Condvar,
    /// A job finished.
    finished: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A job's permission to publish what it made: void once the queue was cancelled after the
/// job was queued.
pub struct Ticket {
    generation: u64,
    shared: Arc<Shared>,
}

impl Ticket {
    /// The queue was cancelled since this job was queued (stop early if convenient).
    pub fn cancelled(&self) -> bool {
        self.shared.lock().generation != self.generation
    }

    /// Run `publish` unless the queue was cancelled since the job was queued. It runs under
    /// the queue's lock, so a cancel cannot slip in between the check and the publish; keep
    /// it short (insert into a map). Returns whether it ran.
    pub fn publish(&self, publish: impl FnOnce()) -> bool {
        let state = self.shared.lock();
        if state.generation != self.generation {
            return false;
        }
        publish();
        drop(state);
        true
    }
}

/// A first-in, first-out job queue with its own worker threads (started on first use, below
/// normal priority on Windows, so the main thread always gets a core first).
pub struct Queue {
    name: &'static str,
    threads: u32,
    shared: Arc<Shared>,
}

impl Queue {
    /// A queue of `threads` workers, named `misty background <name> <n>` (the Profiler's
    /// Timeline shows them as background threads).
    pub fn new(name: &'static str, threads: u32) -> Self {
        Self {
            name,
            threads: threads.max(1),
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                wake: Condvar::new(),
                finished: Condvar::new(),
            }),
        }
    }

    /// Queue `job` (`label` names it in error messages). It runs on a worker after the jobs
    /// queued before it have started.
    pub fn push(
        &self,
        label: impl Into<String>,
        job: impl FnOnce(&Ticket) -> Result<(), String> + Send + 'static,
    ) {
        self.enqueue(label.into(), Box::new(job), None);
    }

    /// [`Queue::push`] with what to publish when the job panics instead of publishing:
    /// `failed(message)` runs under the queue's lock, like a publish (and not after a
    /// cancel). A cache that marks a key pending when it queues the job needs this, or a
    /// panicking import or compile leaves the key pending for good: never loaded, never
    /// retried, and the renderer's `background_pending` never back to 0.
    pub fn push_or(
        &self,
        label: impl Into<String>,
        job: impl FnOnce(&Ticket) -> Result<(), String> + Send + 'static,
        failed: impl FnOnce(String) + Send + 'static,
    ) {
        self.enqueue(label.into(), Box::new(job), Some(Box::new(failed)));
    }

    fn enqueue(&self, label: String, job: Job, failed: Option<Failed>) {
        let mut state = self.shared.lock();
        if state.done >= state.total {
            // Idle: a new batch of work (the status bar counts from zero again).
            state.done = 0;
            state.total = 0;
        }
        state.total += 1;
        state.queue.push_back(Queued { label, job, failed });
        // Another worker while there is more work than workers (up to the queue's threads).
        let wanted = state.running + state.queue.len() as u32;
        if state.workers < self.threads && state.workers < wanted {
            state.workers += 1;
            let index = state.workers - 1;
            let shared = self.shared.clone();
            let spawned = std::thread::Builder::new()
                .name(format!("misty background {} {index}", self.name))
                .spawn(move || work(shared));
            if spawned.is_err() {
                state.workers -= 1;
            }
        }
        drop(state);
        self.shared.wake.notify_one();
    }

    /// Drop the jobs not started yet and void the running ones' tickets (the project was
    /// closed: nothing they make may reach the next project's caches).
    pub fn cancel(&self) {
        let mut state = self.shared.lock();
        state.generation += 1;
        let dropped = state.queue.len() as u32;
        state.queue.clear();
        state.total -= dropped;
        state.errors.clear();
        drop(state);
        self.shared.finished.notify_all();
    }

    /// What was done of the current batch of work.
    pub fn progress(&self) -> Progress {
        let state = self.shared.lock();
        Progress {
            done: state.done,
            total: state.total,
        }
    }

    /// Errors of failed jobs since the last call ("<label>: <error>").
    pub fn take_errors(&self) -> Vec<String> {
        std::mem::take(&mut self.shared.lock().errors)
    }

    /// Wait until every queued job has finished, at most `timeout`; false = still busy.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        let mut state = self.shared.lock();
        while state.done < state.total {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            state = self
                .shared
                .finished
                .wait_timeout(state, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        true
    }

    /// The queue's name (as its threads are named).
    pub fn name(&self) -> &'static str {
        self.name
    }
}

/// A worker: run jobs as they come, forever (the threads live as long as the process).
fn work(shared: Arc<Shared>) {
    crate::background::lower_priority();
    loop {
        let (item, generation) = {
            let mut state = shared.lock();
            loop {
                if let Some(item) = state.queue.pop_front() {
                    state.running += 1;
                    break (item, state.generation);
                }
                state = shared.wake.wait(state).unwrap_or_else(|e| e.into_inner());
            }
        };
        let ticket = Ticket {
            generation,
            shared: shared.clone(),
        };
        let job = item.job;
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| job(&ticket)));
        let panicked = result.is_err();
        let result = result.unwrap_or_else(|panic| Err(panic_message(&panic)));
        let mut state = shared.lock();
        state.running -= 1;
        if state.generation == generation {
            state.done += 1;
            if panicked && let (Some(failed), Err(e)) = (item.failed, &result) {
                failed(e.clone());
            }
            if let Err(e) = result {
                state.errors.push(format!("{}: {e}", item.label));
            }
        } else {
            // Cancelled meanwhile: `cancel` left it counted in `total` while it ran.
            state.done += 1;
        }
        drop(state);
        shared.finished.notify_all();
    }
}

fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "failed".to_owned())
}

/// Shader pipelines compiled the first time a view needs them.
pub fn shaders() -> &'static Queue {
    static QUEUE: OnceLock<Queue> = OnceLock::new();
    QUEUE.get_or_init(|| {
        // The driver compiles each pipeline on the calling thread: a few at once, leaving
        // cores to the main thread and the asset imports.
        let cores = std::thread::available_parallelism().map_or(2, |n| n.get()) as u32;
        Queue::new("shaders", (cores / 3).clamp(1, 4))
    })
}

/// Asset imports: models read and processed, textures decoded (their block compression has
/// its own worker, `sprites::compress`).
pub fn imports() -> &'static Queue {
    static QUEUE: OnceLock<Queue> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(2, |n| n.get()) as u32;
        Queue::new("imports", (cores / 4).clamp(1, 3))
    })
}

/// The project is closing: drop what both queues still have to do.
pub fn cancel_all() {
    shaders().cancel();
    imports().cancel();
}

/// Anything still compiling or importing.
pub fn busy() -> bool {
    shaders().progress().busy() || imports().progress().busy()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    const WAIT: Duration = Duration::from_secs(30);

    /// A job that panics before publishing runs its `failed` instead (the texture and shader
    /// caches clear their pending key with it), with the panic's message; a cancelled one
    /// doesn't.
    #[test]
    fn a_panicking_job_publishes_its_failure() {
        let q = Queue::new("test-panic", 1);
        let (tx, rx) = mpsc::channel();
        let tx2 = tx.clone();
        q.push_or(
            "broken.png",
            |_| panic!("decoder crashed"),
            move |msg| tx2.send(format!("failed: {msg}")).unwrap(),
        );
        assert!(q.wait_idle(WAIT));
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), "failed: decoder crashed");
        assert_eq!(
            q.take_errors(),
            vec!["broken.png: decoder crashed".to_owned()]
        );
        // A job that publishes normally never runs `failed`.
        let tx3 = tx.clone();
        q.push_or(
            "fine.png",
            move |t| {
                t.publish(|| tx.send("published".to_owned()).unwrap());
                Ok(())
            },
            move |_| tx3.send("failed".to_owned()).unwrap(),
        );
        assert!(q.wait_idle(WAIT));
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), "published");
        assert!(rx.try_recv().is_err());
        // Cancelled while running: nothing is published, failure included.
        let (started_tx, started) = mpsc::channel();
        let (go_tx, go) = mpsc::channel::<()>();
        let (ftx, frx) = mpsc::channel::<String>();
        q.push_or(
            "old project.png",
            move |_| {
                started_tx.send(()).unwrap();
                go.recv().unwrap();
                panic!("late crash")
            },
            move |msg| ftx.send(msg).unwrap(),
        );
        started.recv_timeout(WAIT).unwrap();
        q.cancel();
        go_tx.send(()).unwrap();
        assert!(q.wait_idle(WAIT));
        assert!(frx.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn jobs_start_in_the_order_they_were_queued() {
        let q = Queue::new("test order", 1);
        let order = Arc::new(Mutex::new(Vec::new()));
        for i in 0..20 {
            let order = order.clone();
            q.push(format!("job {i}"), move |t| {
                t.publish(|| order.lock().unwrap().push(i));
                Ok(())
            });
        }
        assert!(q.wait_idle(WAIT));
        assert_eq!(*order.lock().unwrap(), (0..20).collect::<Vec<_>>());
        assert_eq!(
            q.progress(),
            Progress {
                done: 20,
                total: 20
            }
        );
        assert!(!q.progress().busy());
    }

    #[test]
    fn progress_counts_the_current_batch() {
        let q = Queue::new("test progress", 1);
        let (go, gate) = mpsc::channel::<()>();
        let gate = Arc::new(Mutex::new(gate));
        for _ in 0..3 {
            let gate = gate.clone();
            q.push("gated", move |_| {
                let _ = gate.lock().unwrap().recv_timeout(WAIT);
                Ok(())
            });
        }
        assert_eq!(q.progress().total, 3);
        assert!(q.progress().busy());
        for _ in 0..3 {
            go.send(()).unwrap();
        }
        assert!(q.wait_idle(WAIT));
        assert_eq!(q.progress(), Progress { done: 3, total: 3 });
        // A new batch starts from zero.
        q.push("next", |_| Ok(()));
        assert!(q.wait_idle(WAIT));
        assert_eq!(q.progress(), Progress { done: 1, total: 1 });
    }

    #[test]
    fn cancel_drops_queued_jobs_and_voids_running_ones() {
        let q = Queue::new("test cancel", 1);
        let published = Arc::new(Mutex::new(Vec::new()));
        let (started_tx, started) = mpsc::channel::<()>();
        let (go, gate) = mpsc::channel::<()>();
        {
            let published = published.clone();
            q.push("running", move |t| {
                started_tx.send(()).unwrap();
                let _ = gate.recv_timeout(WAIT);
                t.publish(|| published.lock().unwrap().push("running"));
                Ok(())
            });
        }
        for i in 0..5 {
            let published = published.clone();
            q.push(format!("queued {i}"), move |t| {
                t.publish(|| published.lock().unwrap().push("queued"));
                Ok(())
            });
        }
        started.recv_timeout(WAIT).unwrap();
        q.cancel();
        // Only the running job is left to finish.
        assert_eq!(q.progress().total, q.progress().done + 1);
        go.send(()).unwrap();
        assert!(q.wait_idle(WAIT));
        assert!(published.lock().unwrap().is_empty(), "nothing published");
        // The queue works again after a cancel.
        let after = published.clone();
        q.push("after", move |t| {
            t.publish(|| after.lock().unwrap().push("after"));
            Ok(())
        });
        assert!(q.wait_idle(WAIT));
        assert_eq!(*published.lock().unwrap(), vec!["after"]);
    }

    #[test]
    fn a_failed_or_panicking_job_reports_an_error_and_never_hangs() {
        let q = Queue::new("test errors", 2);
        q.push("Assets/Broken.glb", |_| Err("not a glTF file".into()));
        q.push("Assets/Panics.png", |_| panic!("decoder blew up"));
        q.push("Assets/Fine.png", |_| Ok(()));
        assert!(q.wait_idle(WAIT), "the queue finished");
        assert_eq!(q.progress(), Progress { done: 3, total: 3 });
        let mut errors = q.take_errors();
        errors.sort();
        assert_eq!(
            errors,
            vec![
                "Assets/Broken.glb: not a glTF file".to_owned(),
                "Assets/Panics.png: decoder blew up".to_owned()
            ]
        );
        assert!(q.take_errors().is_empty());
    }

    #[test]
    fn several_workers_run_jobs_at_once() {
        let q = Queue::new("test parallel", 3);
        let (tx, rx) = mpsc::channel::<()>();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        for _ in 0..3 {
            let barrier = barrier.clone();
            let tx = tx.clone();
            q.push("together", move |_| {
                barrier.wait();
                tx.send(()).unwrap();
                Ok(())
            });
        }
        for _ in 0..3 {
            rx.recv_timeout(WAIT)
                .expect("three jobs ran at the same time");
        }
        assert!(q.wait_idle(WAIT));
    }
}
