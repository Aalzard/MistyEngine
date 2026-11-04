//! Loaded probe files shared by every view, re-read when a file changes on disk (after a bake),
//! plus the latest captures of Realtime probes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use super::ProbeData;

struct Entry {
    checked: Instant,
    modified: Option<SystemTime>,
    data: Option<Arc<ProbeData>>,
}

/// How often a cached file's modification time is looked at.
const RECHECK: Duration = Duration::from_millis(500);

static FILES: LazyLock<Mutex<HashMap<PathBuf, Entry>>> = LazyLock::new(Default::default);
static REALTIME: LazyLock<Mutex<HashMap<u64, Arc<ProbeData>>>> = LazyLock::new(Default::default);

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The probe file at `path` (`None` when missing or unreadable).
pub fn load(path: &Path) -> Option<Arc<ProbeData>> {
    let mut files = FILES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = files.get_mut(path) {
        if e.checked.elapsed() < RECHECK {
            return e.data.clone();
        }
        e.checked = Instant::now();
        let now = modified(path);
        if now == e.modified {
            return e.data.clone();
        }
    }
    let stamp = modified(path);
    let data = stamp.and_then(|_| ProbeData::load(path).ok()).map(Arc::new);
    files.insert(
        path.to_owned(),
        Entry {
            checked: Instant::now(),
            modified: stamp,
            data: data.clone(),
        },
    );
    data
}

/// A freshly baked probe saved to `path`.
pub fn insert(path: &Path, data: Arc<ProbeData>) {
    let mut files = FILES.lock().unwrap_or_else(|e| e.into_inner());
    files.insert(
        path.to_owned(),
        Entry {
            checked: Instant::now(),
            modified: modified(path),
            data: Some(data),
        },
    );
}

/// Forget a deleted file.
pub fn forget(path: &Path) {
    FILES.lock().unwrap_or_else(|e| e.into_inner()).remove(path);
}

/// Latest capture of the Realtime probe `key` (an entity's bits).
pub fn realtime(key: u64) -> Option<Arc<ProbeData>> {
    REALTIME
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .cloned()
}

pub fn set_realtime(key: u64, data: Arc<ProbeData>) {
    REALTIME
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, data);
}
