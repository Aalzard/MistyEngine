//! A thread pool for heavy background asset work that takes seconds (Misty GI mesh distance
//! field bakes, virtual geometry builds), kept apart from rayon's global pool.
//!
//! The render thread's own parallel loops (shadow lists, draw prep) run on the global pool. A
//! job injected there from the render thread waits until a worker has nothing of its own left
//! to do, so a bake that saturated the global pool froze the frame until it finished (23 s for
//! MeadowDemo's rocks). Here the workers also run below normal priority (Windows), one fewer
//! than the cores: the render thread and the global pool always get a core first.

use std::sync::OnceLock;

/// The pool (built on first use).
pub fn pool() -> &'static rayon::ThreadPool {
    static POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
        // Without a lower priority, leave half the cores to the frame.
        let threads = if cfg!(windows) { cores - 1 } else { cores / 2 }.max(1);
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("misty background {i}"))
            .start_handler(|_| lower_priority())
            .build()
            .expect("background thread pool")
    })
}

#[cfg(windows)]
fn lower_priority() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThread() -> isize;
        fn SetThreadPriority(thread: isize, priority: i32) -> i32;
    }
    const THREAD_PRIORITY_BELOW_NORMAL: i32 = -1;
    // SAFETY: the current thread's pseudo handle needs no closing.
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(not(windows))]
fn lower_priority() {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A long job on the background pool leaves the global pool free: a parallel loop started
    /// from another thread finishes while the job still runs.
    #[test]
    fn background_work_never_blocks_the_global_pool() {
        use rayon::prelude::*;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let stop = Arc::new(AtomicBool::new(false));
        let busy = stop.clone();
        pool().spawn(move || {
            // Every background worker spins until the global loop is done.
            (0..1024).into_par_iter().for_each(|_| {
                let t = std::time::Instant::now();
                while !busy.load(Ordering::Relaxed) && t.elapsed().as_secs() < 30 {
                    std::hint::spin_loop();
                }
            });
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        let t = std::time::Instant::now();
        let sum: u64 = (0..10_000u64).into_par_iter().sum();
        let waited = t.elapsed();
        stop.store(true, Ordering::Relaxed);
        assert_eq!(sum, 49_995_000);
        assert!(waited.as_secs_f32() < 5.0, "global pool waited {waited:?}");
    }
}
