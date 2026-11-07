//! Things a scene may need on its first frame, started on a worker thread as soon as the
//! renderer exists, so opening a scene (or entering Play) does not wait for them: the tree and
//! grass visibility pipelines (tens of milliseconds of shader compiles each) and the grass
//! atlas mips.
//!
//! Only while streaming (`render::grass_streaming`: the editor and the player); with it off
//! (tests, screenshots) everything is built in place on first use, as before.

use std::sync::Mutex;
use std::sync::mpsc::Receiver;

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
        let spawned = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let _ = tx.send(build());
            });
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
