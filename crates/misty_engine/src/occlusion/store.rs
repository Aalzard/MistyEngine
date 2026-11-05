//! Loaded occlusion data shared by every view, re-read when the file changes (after a bake).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use super::OcclusionData;

struct Entry {
    checked: Instant,
    modified: Option<SystemTime>,
    data: Option<Arc<OcclusionData>>,
}

/// How often a cached file's modification time is looked at.
const RECHECK: Duration = Duration::from_millis(500);

static FILES: LazyLock<Mutex<HashMap<PathBuf, Entry>>> = LazyLock::new(Default::default);

/// The occlusion data at `path` (`None` when missing or unreadable).
pub fn load(path: &Path) -> Option<Arc<OcclusionData>> {
    let mut files = FILES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = files.get_mut(path) {
        if e.checked.elapsed() < RECHECK {
            return e.data.clone();
        }
        e.checked = Instant::now();
        if crate::vfs::modified(path) == e.modified {
            return e.data.clone();
        }
    }
    let stamp = crate::vfs::modified(path);
    let data = stamp
        .and_then(|_| OcclusionData::load(path).ok())
        .map(Arc::new);
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

/// Freshly baked data saved to `path`.
pub fn insert(path: &Path, data: Arc<OcclusionData>) {
    let mut files = FILES.lock().unwrap_or_else(|e| e.into_inner());
    files.insert(
        path.to_owned(),
        Entry {
            checked: Instant::now(),
            modified: crate::vfs::modified(path),
            data: Some(data),
        },
    );
}

/// Forget a deleted file.
pub fn forget(path: &Path) {
    FILES.lock().unwrap_or_else(|e| e.into_inner()).remove(path);
}

/// The scene's baked data, if it names a file that loads.
pub fn for_scene(scene: &crate::scene::Scene) -> Option<Arc<OcclusionData>> {
    let rel = scene.render_settings.occlusion.data.as_ref()?;
    load(&crate::assets::absolute(rel))
}
