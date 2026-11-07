//! Things a scene may need on its first frame, started on a worker thread as soon as the
//! renderer exists, so opening a scene (or entering Play) does not wait for them: the tree,
//! grass visibility, grass cull, volumetrics and cloud pipelines (tens of milliseconds of
//! shader compiles each, hundreds on a first launch), the cloud noise volumes, and (process-
//! wide, in their own modules) the grass atlas mips and the blue noise.
//!
//! Only while streaming (`render::grass_streaming`: the editor and the player); with it off
//! (tests, screenshots) everything is built in place on first use, as before.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;

/// Workers still running (their captured device and queue are dropped when they finish).
static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// Wait (up to `timeout`) until no worker is running: a renderer dropped before taking a value
/// leaves its worker building on the old device. The GPU tests wait before making the next
/// device, so no two tests' devices are busy at once. False on timeout.
#[cfg(test)]
pub(crate) fn wait_idle(timeout: std::time::Duration) -> bool {
    let started = std::time::Instant::now();
    while RUNNING.load(Ordering::Acquire) > 0 {
        if started.elapsed() > timeout {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    true
}

/// A value being built on a worker thread, taken (waiting if needed) on first use.
pub(super) struct Prebuilt<T> {
    /// (Behind a lock only so the renderer stays `Sync`.)
    rx: Mutex<Option<Receiver<T>>>,
}

impl<T> Default for Prebuilt<T> {
    fn default() -> Self {
        Self {
            rx: Mutex::new(None),
        }
    }
}

impl<T: Send + 'static> Prebuilt<T> {
    /// Start `build` on a worker thread named `name` (nothing when streaming is off).
    pub fn spawn(name: &str, build: impl FnOnce() -> T + Send + 'static) -> Self {
        if !super::grass_streaming() {
            return Self::default();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        RUNNING.fetch_add(1, Ordering::AcqRel);
        let spawned = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                // (A value nobody takes any more is dropped by the failed send.)
                let _ = tx.send(build());
                RUNNING.fetch_sub(1, Ordering::AcqRel);
            });
        if spawned.is_err() {
            RUNNING.fetch_sub(1, Ordering::AcqRel);
        }
        Self {
            rx: Mutex::new(spawned.ok().map(|_| rx)),
        }
    }

    /// The value from the worker, waiting for it when it is not done yet; None when nothing
    /// was started (or it already was taken, or the worker failed): build in place then.
    pub fn take(&mut self) -> Option<T> {
        let rx = self.rx.get_mut().ok()?.take()?;
        crate::profile_scope!("Prebuilt.Wait");
        rx.recv().ok()
    }
}
