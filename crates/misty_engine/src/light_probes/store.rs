//! Loaded light probe files shared by every view (tetrahedralized once), re-read when the file
//! changes on disk (after a bake).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use super::{LightProbeData, LightProbeField};

struct Entry {
    checked: Instant,
    modified: Option<SystemTime>,
    field: Option<Arc<LightProbeField>>,
}

const RECHECK: Duration = Duration::from_millis(500);

static FILES: LazyLock<Mutex<HashMap<PathBuf, Entry>>> = LazyLock::new(Default::default);

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The probes in the file at `path` (`None` when missing or unreadable).
pub fn load(path: &Path) -> Option<Arc<LightProbeField>> {
    let mut files = FILES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = files.get_mut(path) {
        if e.checked.elapsed() < RECHECK {
            return e.field.clone();
        }
        e.checked = Instant::now();
        // The background file watch's stamp first; a difference is checked on disk.
        if crate::file_watch::cached_modified(path) == e.modified || modified(path) == e.modified {
            return e.field.clone();
        }
    }
    let stamp = modified(path);
    let field = stamp
        .and_then(|_| LightProbeData::load(path).ok())
        .map(|d| Arc::new(LightProbeField::from_data(&d)));
    files.insert(
        path.to_owned(),
        Entry {
            checked: Instant::now(),
            modified: stamp,
            field: field.clone(),
        },
    );
    field
}

/// A freshly baked file saved to `path`.
pub fn insert(path: &Path, field: Arc<LightProbeField>) {
    FILES.lock().unwrap_or_else(|e| e.into_inner()).insert(
        path.to_owned(),
        Entry {
            checked: Instant::now(),
            modified: modified(path),
            field: Some(field),
        },
    );
}

/// Forget a deleted file.
pub fn forget(path: &Path) {
    FILES.lock().unwrap_or_else(|e| e.into_inner()).remove(path);
}
