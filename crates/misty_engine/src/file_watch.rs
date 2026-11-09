//! File stamps for change polling, kept fresh by a background thread.
//!
//! The editor checks hundreds of files twice a second for live reload (textures, models,
//! shaders, effects, terrains, scripts, scenes, materials, prefabs). Each check is a `stat`,
//! which on Windows costs 20–100 µs, more while the disk is busy: done on the main thread they
//! made a 10–25 ms frame every half second. With [`set_background`] on (the editor), a thread
//! stats every watched file in turn and the pollers read its last results with
//! [`cached_metadata`] / [`cached_modified`], a map lookup.
//!
//! The cache only answers "nothing changed". A poller whose cached stamp differs from the one
//! it recorded checks the file again directly (`stat` now) before acting, so every decision is
//! made on a fresh stamp, exactly as before; the editor's own saves (recorded right after
//! writing) can never be taken for outside changes because of a stale cached stamp. A change on
//! disk is noticed at most one background pass ([`PASS`]) later than before.
//!
//! Off (the default: tests, the player, tools), both functions `stat` directly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// Pause between two background passes over the watched files.
pub const PASS: Duration = Duration::from_millis(200);
/// A file nobody asked about for this long stops being watched.
const FORGET: Duration = Duration::from_secs(30);

/// What a `stat` says about a path (`None` from the functions below: it doesn't exist).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meta {
    pub modified: Option<SystemTime>,
    pub len: u64,
    pub is_file: bool,
}

/// `std::fs::metadata`, reduced to [`Meta`].
pub fn metadata(path: &Path) -> Option<Meta> {
    let m = std::fs::metadata(path).ok()?;
    Some(Meta {
        modified: m.modified().ok(),
        len: m.len(),
        is_file: m.is_file(),
    })
}

struct Entry {
    meta: Option<Meta>,
    /// Last time a poller asked.
    used: Instant,
}

/// Watched paths and their last stamps (one process-wide, [`WATCH`]; tests make their own).
#[derive(Default)]
struct Watch {
    background: AtomicBool,
    entries: Mutex<HashMap<PathBuf, Entry>>,
}

static WATCH: LazyLock<Watch> = LazyLock::new(Watch::default);

impl Watch {
    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// One pass: stat every watched path (without holding the lock) and store the results.
    fn pass(&self) {
        let paths: Vec<PathBuf> = {
            let mut map = self.entries();
            map.retain(|_, e| e.used.elapsed() < FORGET);
            map.keys().cloned().collect()
        };
        let fresh: Vec<(PathBuf, Option<Meta>)> = paths
            .into_iter()
            .map(|p| {
                let m = metadata(&p);
                (p, m)
            })
            .collect();
        let mut map = self.entries();
        for (p, m) in fresh {
            if let Some(e) = map.get_mut(&p) {
                e.meta = m;
            }
        }
    }

    fn cached(&self, path: &Path) -> Option<Meta> {
        if !self.background.load(Ordering::Relaxed) {
            return metadata(path);
        }
        if let Some(e) = self.entries().get_mut(path) {
            e.used = Instant::now();
            return e.meta;
        }
        let meta = metadata(path);
        self.entries().insert(
            path.to_path_buf(),
            Entry {
                meta,
                used: Instant::now(),
            },
        );
        meta
    }
}

/// Keep the stamps fresh on a background thread (the editor turns this on at start).
pub fn set_background(on: bool) {
    let w = &*WATCH;
    if on && !w.background.swap(true, Ordering::Relaxed) {
        let spawned = std::thread::Builder::new()
            .name("File Watch".into())
            .spawn(|| {
                let w = &*WATCH;
                while w.background.load(Ordering::Relaxed) {
                    w.pass();
                    std::thread::sleep(PASS);
                }
            });
        if spawned.is_err() {
            w.background.store(false, Ordering::Relaxed);
        }
    } else if !on {
        w.background.store(false, Ordering::Relaxed);
    }
}

pub fn is_background() -> bool {
    WATCH.background.load(Ordering::Relaxed)
}

/// The path's stamp as last seen by the background thread (first call per path: `stat` now,
/// and watch it from then on). Without the background thread: `stat` now.
pub fn cached_metadata(path: &Path) -> Option<Meta> {
    WATCH.cached(path)
}

/// [`cached_metadata`]'s modification time (`None` when the file is missing), as
/// `std::fs::metadata(path).and_then(|m| m.modified()).ok()` would give.
pub fn cached_modified(path: &Path) -> Option<SystemTime> {
    cached_metadata(path).and_then(|m| m.modified)
}

/// Paths watched now (tests, Profiler).
pub fn watched() -> usize {
    WATCH.entries().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("misty-file-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn without_the_thread_every_call_stats_the_file() {
        let w = Watch::default();
        let path = temp_file("direct.txt");
        std::fs::write(&path, "a").unwrap();
        assert_eq!(w.cached(&path).map(|m| m.len), Some(1));
        std::fs::write(&path, "abc").unwrap();
        assert_eq!(w.cached(&path).map(|m| m.len), Some(3));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(w.cached(&path), None);
        assert!(w.entries().is_empty(), "nothing is watched while off");
    }

    #[test]
    fn cached_stamps_change_on_the_next_pass() {
        let w = Watch::default();
        w.background.store(true, Ordering::Relaxed);
        let path = temp_file("watched.txt");
        std::fs::write(&path, "a").unwrap();
        // First ask: stat now.
        assert_eq!(w.cached(&path).map(|m| m.len), Some(1));
        std::fs::write(&path, "abcd").unwrap();
        // Until the next pass the cache keeps the stamp it has; the direct stat is fresh.
        assert_eq!(w.cached(&path).map(|m| m.len), Some(1));
        assert_eq!(metadata(&path).map(|m| m.len), Some(4));
        w.pass();
        assert_eq!(w.cached(&path).map(|m| m.len), Some(4));
        std::fs::remove_file(&path).unwrap();
        w.pass();
        assert_eq!(w.cached(&path), None, "a deleted file reads as missing");
        std::fs::write(&path, "xy").unwrap();
        w.pass();
        assert_eq!(w.cached(&path).map(|m| m.len), Some(2), "and comes back");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn unused_paths_stop_being_watched() {
        let w = Watch::default();
        w.background.store(true, Ordering::Relaxed);
        let path = temp_file("forgotten.txt");
        w.cached(&path);
        assert_eq!(w.entries().len(), 1);
        w.entries().get_mut(&path).unwrap().used = Instant::now() - FORGET * 2;
        w.pass();
        assert!(w.entries().is_empty());
    }
}
