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
//!
//! [`pin`] adds paths that stay watched while nobody asks (the Project window's folders), and
//! [`set_waker`] is called when a pass sees any watched stamp change, so an idle editor (which
//! draws no frames, so polls nothing) wakes up for files added or changed by other programs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// Pause between two background passes over the watched files.
pub const PASS: Duration = Duration::from_millis(200);
/// A file nobody asked about for this long, while other files were asked about, stops being
/// watched. Long, because asking again costs a `stat` on the asker's thread: an object selected
/// again after half a minute froze the editor for seconds while the project's disk was busy. An idle editor draws no frames and asks about nothing: its files stay watched, or
/// the first frame after a quiet spell would `stat` every one of them on the main thread
/// (hundreds of files: seconds while the project's disk is busy).
const FORGET: Duration = Duration::from_secs(600);

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
    /// Watched until unpinned, asked for or not (see [`pin`]).
    pinned: bool,
}

/// Called from the watch thread when a stamp changed.
type Waker = Box<dyn Fn() + Send + Sync>;

/// Watched paths and their last stamps (one process-wide, [`WATCH`]; tests make their own).
#[derive(Default)]
struct Watch {
    background: AtomicBool,
    entries: Mutex<HashMap<PathBuf, Entry>>,
    /// Last time a poller asked about any path.
    asked: Mutex<Option<Instant>>,
    /// Tests: forget after this long instead of [`FORGET`].
    forget: Option<Duration>,
    waker: Mutex<Option<Waker>>,
}

static WATCH: LazyLock<Watch> = LazyLock::new(Watch::default);

impl Watch {
    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// One pass: stat every watched path (without holding the lock) and store the results.
    /// Returns true when a stamp changed (the waker was called).
    fn pass(&self) -> bool {
        let paths: Vec<PathBuf> = {
            let asked = *self.asked.lock().unwrap_or_else(|e| e.into_inner());
            let forget = self.forget.unwrap_or(FORGET);
            let mut map = self.entries();
            map.retain(|_, e| {
                e.pinned || asked.is_none_or(|a| a.saturating_duration_since(e.used) < forget)
            });
            map.keys().cloned().collect()
        };
        let fresh: Vec<(PathBuf, Option<Meta>)> = paths
            .into_iter()
            .map(|p| {
                let m = metadata(&p);
                (p, m)
            })
            .collect();
        let mut changed = false;
        {
            let mut map = self.entries();
            for (p, m) in fresh {
                if let Some(e) = map.get_mut(&p) {
                    changed |= e.meta != m;
                    e.meta = m;
                }
            }
        }
        if changed
            && let Some(wake) = self
                .waker
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
        {
            wake();
        }
        changed
    }

    fn pin(&self, paths: Vec<PathBuf>) {
        let mut map = self.entries();
        for e in map.values_mut() {
            e.pinned = false;
        }
        for p in paths {
            match map.get_mut(&p) {
                Some(e) => e.pinned = true,
                None => {
                    let meta = metadata(&p);
                    map.insert(
                        p,
                        Entry {
                            meta,
                            used: Instant::now(),
                            pinned: true,
                        },
                    );
                }
            }
        }
    }

    fn cached(&self, path: &Path) -> Option<Meta> {
        if !self.background.load(Ordering::Relaxed) {
            return metadata(path);
        }
        let now = Instant::now();
        *self.asked.lock().unwrap_or_else(|e| e.into_inner()) = Some(now);
        if let Some(e) = self.entries().get_mut(path) {
            e.used = now;
            return e.meta;
        }
        // The one `stat` on the caller's thread: a path nobody asked about before.
        let meta = {
            crate::profile_scope!("FileWatch.FirstStat");
            metadata(path)
        };
        self.entries().insert(
            path.to_path_buf(),
            Entry {
                meta,
                used: Instant::now(),
                pinned: false,
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

/// Keep watching exactly these paths (replacing the previous pinned set) even when no poller
/// asks for them. Only with the background thread on.
pub fn pin(paths: Vec<PathBuf>) {
    if is_background() {
        WATCH.pin(paths);
    }
}

/// Call `wake` from the watch thread whenever a pass sees a watched stamp change (the editor
/// asks for a repaint).
pub fn set_waker(wake: impl Fn() + Send + Sync + 'static) {
    *WATCH.waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(wake));
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

    /// Short, so `Instant::now() - TEST_FORGET * 2` exists on a machine that just started.
    const TEST_FORGET: Duration = Duration::from_secs(30);

    /// A watch with the background mode on (passes are made by the test).
    fn watching() -> Watch {
        let w = Watch {
            forget: Some(TEST_FORGET),
            ..Default::default()
        };
        w.background.store(true, Ordering::Relaxed);
        w
    }

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
        let w = watching();
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
    fn pinned_paths_stay_watched_and_changes_wake() {
        let w = watching();
        let dir = temp_file("pinned_dir");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let woken = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = woken.clone();
        *w.waker.lock().unwrap() = Some(Box::new(move || {
            count.fetch_add(1, Ordering::Relaxed);
        }));
        w.pin(vec![dir.clone()]);
        // Nobody asks for it for a long time: still watched.
        w.entries().get_mut(&dir).unwrap().used = Instant::now() - TEST_FORGET * 2;
        assert!(!w.pass(), "nothing changed yet");
        assert_eq!(w.entries().len(), 1);
        // A file added in the folder changes the folder's stamp: the pass wakes the editor.
        let before = w.cached(&dir).and_then(|m| m.modified);
        std::thread::sleep(Duration::from_millis(30));
        std::fs::write(dir.join("new.txt"), "x").unwrap();
        let after = metadata(&dir).and_then(|m| m.modified);
        if after != before {
            assert!(w.pass());
            assert_eq!(woken.load(Ordering::Relaxed), 1);
        }
        // Unpinned and unused: forgotten.
        w.pin(Vec::new());
        w.entries().get_mut(&dir).unwrap().used = Instant::now() - TEST_FORGET * 2;
        w.pass();
        assert!(w.entries().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unused_paths_stop_being_watched() {
        let w = watching();
        let path = temp_file("forgotten.txt");
        w.cached(&path);
        assert_eq!(w.entries().len(), 1);
        w.entries().get_mut(&path).unwrap().used = Instant::now() - TEST_FORGET * 2;
        w.pass();
        assert!(w.entries().is_empty());
    }

    #[test]
    fn an_idle_editor_keeps_its_paths_watched() {
        let w = watching();
        let (a, b) = (temp_file("idle_a.txt"), temp_file("idle_b.txt"));
        w.cached(&a);
        w.cached(&b);
        // A quiet spell: nobody asked about anything for a long time.
        let long_ago = Instant::now() - TEST_FORGET * 2;
        for e in w.entries().values_mut() {
            e.used = long_ago;
        }
        *w.asked.lock().unwrap() = Some(long_ago);
        w.pass();
        assert_eq!(
            w.entries().len(),
            2,
            "nothing is forgotten while nobody asks"
        );
        // Frames again, asking only about one of them: the other was last used long before.
        w.cached(&a);
        w.pass();
        assert_eq!(w.entries().len(), 1);
        assert!(w.entries().contains_key(&a));
    }
}
