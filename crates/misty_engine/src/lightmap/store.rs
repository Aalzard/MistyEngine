//! Loaded lightmap files shared by every view, re-read when a file changes on disk (after a
//! bake).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use super::LightmapData;

struct Entry {
    checked: Instant,
    modified: Option<SystemTime>,
    data: Option<Arc<LightmapData>>,
}

/// How often a cached file's modification time is looked at.
const RECHECK: Duration = Duration::from_millis(500);

static FILES: LazyLock<Mutex<HashMap<PathBuf, Entry>>> = LazyLock::new(Default::default);

fn modified(path: &Path) -> Option<SystemTime> {
    crate::vfs::modified(path)
}

/// The lightmap at `path` (`None` when missing or unreadable).
pub fn load(path: &Path) -> Option<Arc<LightmapData>> {
    let mut files = FILES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = files.get_mut(path) {
        if e.checked.elapsed() < RECHECK {
            return e.data.clone();
        }
        e.checked = Instant::now();
        if modified(path) == e.modified {
            return e.data.clone();
        }
    }
    let stamp = modified(path);
    let data = LightmapData::load(path).ok().map(Arc::new);
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

/// A freshly baked lightmap saved to `path`.
pub fn insert(path: &Path, data: Arc<LightmapData>) {
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
