//! Asset saves off the main thread.
//!
//! [`super::write_atomic`] flushes to the device (`sync_all`), so a power cut can't leave an
//! empty or old file behind a finished save. That flush takes 2.5 ms on an SSD and 0.9 s (up
//! to 27 s) on a busy hard disk, and a material is saved on every frame of a slider drag: done
//! on the main thread, the editor stopped responding.
//!
//! [`save_later`] hands the bytes to a writer thread instead. The thread keeps only the latest
//! bytes per path, and writes a path at most once per [`MIN_INTERVAL`], so a drag of a few
//! seconds becomes a few writes and the last one always has the final value. [`flush`] waits
//! for everything pending (before the files are read back, moved or packed, and on exit).
//!
//! Off (the default: tests, the player, tools), [`save_later`] writes at once.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The shortest time between two writes of the same file while changes keep coming.
pub const MIN_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Default)]
struct State {
    /// Latest bytes waiting per path.
    pending: HashMap<PathBuf, Vec<u8>>,
    /// The path being written now.
    writing: Option<PathBuf>,
    /// When each path was last written.
    written: HashMap<PathBuf, Instant>,
    /// Someone waits in [`Saver::flush`]: write everything now.
    flushing: usize,
    /// Files written so far.
    writes: u64,
    /// Failed writes, for the Console.
    errors: Vec<String>,
    stop: bool,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    min_interval: Duration,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A writer thread and its queue (one process-wide, [`SAVER`]; tests make their own).
pub struct Saver {
    shared: Arc<Shared>,
}

impl Saver {
    pub fn new(min_interval: Duration) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            changed: Condvar::new(),
            min_interval,
        });
        let worker = shared.clone();
        let spawned = std::thread::Builder::new()
            .name("misty asset saver".into())
            .spawn(move || run(&worker));
        if spawned.is_err() {
            shared.lock().stop = true;
        }
        Self { shared }
    }

    /// Write `bytes` to `path` soon (replacing bytes still waiting for it).
    pub fn save(&self, path: &Path, bytes: Vec<u8>) -> std::io::Result<()> {
        let mut s = self.shared.lock();
        if s.stop {
            // No writer thread: write now.
            drop(s);
            return super::write_atomic(path, bytes);
        }
        s.pending.insert(path.to_path_buf(), bytes);
        self.shared.changed.notify_all();
        Ok(())
    }

    /// Bytes for `path` are waiting or being written (the file is older than what was saved).
    pub fn pending(&self, path: &Path) -> bool {
        let s = self.shared.lock();
        s.pending.contains_key(path) || s.writing.as_deref() == Some(path)
    }

    /// Anything waiting or being written.
    pub fn busy(&self) -> bool {
        let s = self.shared.lock();
        !s.pending.is_empty() || s.writing.is_some()
    }

    /// Wait until everything handed over so far is on disk.
    pub fn flush(&self) {
        let mut s = self.shared.lock();
        s.flushing += 1;
        self.shared.changed.notify_all();
        while !s.stop && (!s.pending.is_empty() || s.writing.is_some()) {
            s = self
                .shared
                .changed
                .wait(s)
                .unwrap_or_else(|e| e.into_inner());
        }
        s.flushing -= 1;
    }

    /// A save of `path` is about to be written directly (a Save, a new asset over it): drop the
    /// bytes still waiting for it and wait for a write of it already under way, so the direct
    /// write lands last. (Before, the older waiting bytes were written after it and the newer
    /// file was lost.)
    pub fn supersede(&self, path: &Path) {
        let mut s = self.shared.lock();
        s.pending.remove(path);
        while !s.stop && s.writing.as_deref() == Some(path) {
            s = self
                .shared
                .changed
                .wait(s)
                .unwrap_or_else(|e| e.into_inner());
        }
        self.shared.changed.notify_all();
    }

    /// [`Saver::flush`] that gives up after `limit` (a crash: don't hang the way out on a
    /// stuck disk). Returns whether everything was written.
    pub fn flush_within(&self, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        let mut s = self.shared.lock();
        s.flushing += 1;
        self.shared.changed.notify_all();
        while !s.stop && (!s.pending.is_empty() || s.writing.is_some()) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            s = self
                .shared
                .changed
                .wait_timeout(s, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        s.flushing -= 1;
        s.pending.is_empty() && s.writing.is_none()
    }

    /// Files written so far.
    pub fn writes(&self) -> u64 {
        self.shared.lock().writes
    }

    /// Writes that failed since the last call ("path: error").
    pub fn take_errors(&self) -> Vec<String> {
        std::mem::take(&mut self.shared.lock().errors)
    }
}

impl Drop for Saver {
    fn drop(&mut self) {
        self.flush();
        self.shared.lock().stop = true;
        self.shared.changed.notify_all();
    }
}

fn run(shared: &Shared) {
    let mut s = shared.lock();
    loop {
        if s.stop {
            return;
        }
        // A waiting path that may be written now, else how long until the first one may.
        let now = Instant::now();
        let mut next: Option<PathBuf> = None;
        let mut wait: Option<Duration> = None;
        for path in s.pending.keys() {
            let due = match s.written.get(path) {
                Some(t) if s.flushing == 0 => {
                    (*t + shared.min_interval).saturating_duration_since(now)
                }
                _ => Duration::ZERO,
            };
            if due.is_zero() {
                next = Some(path.clone());
                break;
            }
            wait = Some(wait.map_or(due, |w| w.min(due)));
        }
        let Some(path) = next else {
            s = match wait {
                Some(w) => {
                    let (guard, _) = shared
                        .changed
                        .wait_timeout(s, w)
                        .unwrap_or_else(|e| e.into_inner());
                    guard
                }
                None => shared.changed.wait(s).unwrap_or_else(|e| e.into_inner()),
            };
            continue;
        };
        let bytes = s.pending.remove(&path).unwrap_or_default();
        s.writing = Some(path.clone());
        drop(s);
        let result = super::write_atomic(&path, bytes);
        s = shared.lock();
        s.writing = None;
        s.writes += 1;
        s.written.insert(path.clone(), Instant::now());
        s.written
            .retain(|_, t| t.elapsed() < shared.min_interval * 4);
        if let Err(e) = result {
            s.errors.push(format!("{}: {e}", path.display()));
        }
        shared.changed.notify_all();
    }
}

static BACKGROUND: AtomicBool = AtomicBool::new(false);
static SAVER: LazyLock<Saver> = LazyLock::new(|| Saver::new(MIN_INTERVAL));

/// Save on the writer thread from now on (the editor turns this on at start).
pub fn set_background(on: bool) {
    if !on && BACKGROUND.swap(false, Ordering::Relaxed) {
        SAVER.flush();
    } else if on {
        BACKGROUND.store(true, Ordering::Relaxed);
    }
}

/// Write `bytes` to `path` as [`super::write_atomic`] does: on the writer thread when it is on
/// (errors then come from [`take_errors`]), else now.
pub fn save_later(path: &Path, bytes: Vec<u8>) -> std::io::Result<()> {
    if BACKGROUND.load(Ordering::Relaxed) {
        SAVER.save(path, bytes)
    } else {
        super::write_atomic(path, bytes)
    }
}

/// A save of `path` is still on its way to the disk.
pub fn pending(path: &Path) -> bool {
    BACKGROUND.load(Ordering::Relaxed) && SAVER.pending(path)
}

/// `path` is being written directly now: see [`Saver::supersede`].
pub fn supersede(path: &Path) {
    if BACKGROUND.load(Ordering::Relaxed) {
        SAVER.supersede(path);
    }
}

/// Wait at most `limit` for the saves handed over so far (the editor's panic hook: edits of
/// the last half second still reach the disk when the editor crashes).
pub fn flush_within(limit: Duration) -> bool {
    !BACKGROUND.load(Ordering::Relaxed) || SAVER.flush_within(limit)
}

/// Wait for every save handed over so far (cheap when nothing waits).
pub fn flush() {
    if BACKGROUND.load(Ordering::Relaxed) {
        SAVER.flush();
    }
}

/// Background writes that failed since the last call.
pub fn take_errors() -> Vec<String> {
    if BACKGROUND.load(Ordering::Relaxed) {
        SAVER.take_errors()
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("misty-saver-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A slider drag saves the material on every frame: the file is written a few times, not
    /// once per frame, and ends with the last value.
    #[test]
    fn a_drag_of_many_saves_writes_the_file_a_few_times() {
        let dir = temp_dir("drag");
        let path = dir.join("Drag.mat");
        let saver = Saver::new(MIN_INTERVAL);
        // 240 frames of a drag (a second or more of them).
        let started = Instant::now();
        for frame in 0..240 {
            saver
                .save(&path, format!("{{\"metallic\": {frame}}}").into_bytes())
                .unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
        // Released: the last value reaches the disk.
        saver.flush();
        assert!(!saver.pending(&path));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"metallic\": 239}"
        );
        // At most one write per interval, and the one on release.
        let most = (started.elapsed().as_secs_f32() / MIN_INTERVAL.as_secs_f32()).ceil() as u64 + 2;
        let writes = saver.writes();
        assert!(
            (1..=most).contains(&writes),
            "240 saves in a drag made {writes} writes (at most {most} expected)"
        );
        assert!(saver.take_errors().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_does_not_wait_for_the_disk_and_flush_does() {
        let dir = temp_dir("flush");
        let saver = Saver::new(MIN_INTERVAL);
        let started = Instant::now();
        for i in 0..50 {
            let path = dir.join(format!("File {}.mat", i % 5));
            saver.save(&path, vec![b'a' + (i % 26) as u8; 64]).unwrap();
        }
        let handing_over = started.elapsed();
        saver.flush();
        assert!(!saver.busy());
        for i in 45..50 {
            let text = std::fs::read(dir.join(format!("File {}.mat", i % 5))).unwrap();
            assert_eq!(text, vec![b'a' + (i % 26) as u8; 64]);
        }
        // Five files, each written once or twice (the first save may start at once).
        assert!(saver.writes() <= 10, "{} writes", saver.writes());
        // Loose: 50 map inserts, however slow the disk is.
        assert!(
            handing_over < Duration::from_millis(250),
            "{handing_over:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A direct save after background saves of the same file wins: the bytes still waiting
    /// are dropped instead of being written over it afterwards.
    #[test]
    fn a_direct_save_is_not_overwritten_by_older_waiting_bytes() {
        let dir = temp_dir("supersede");
        let path = dir.join("Mat.mat");
        let saver = Saver::new(Duration::from_millis(300));
        saver.save(&path, b"drag 1".to_vec()).unwrap();
        saver.flush();
        // The next drag frame waits out the interval...
        saver.save(&path, b"drag 2".to_vec()).unwrap();
        assert!(saver.pending(&path));
        // ...when the file is saved directly (Save, a script, a tool).
        saver.supersede(&path);
        super::super::write_atomic(&path, b"direct").unwrap();
        assert!(!saver.pending(&path));
        std::thread::sleep(Duration::from_millis(500));
        saver.flush();
        assert_eq!(std::fs::read(&path).unwrap(), b"direct");
        // A bounded flush returns at once with nothing waiting.
        assert!(saver.flush_within(Duration::from_millis(10)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_write_is_reported() {
        let dir = temp_dir("error");
        let saver = Saver::new(Duration::ZERO);
        let path = dir.join("No Such Folder").join("File.mat");
        saver.save(&path, b"x".to_vec()).unwrap();
        saver.flush();
        let errors = saver.take_errors();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("File.mat"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
