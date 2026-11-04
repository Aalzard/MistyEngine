//! Decoded HDRI files shared by every view, keyed by project path and re-read when the file
//! changes on disk (the same scheme as the probe store).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use super::{HdrImage, HdriHandle, HdriSky};

struct Entry {
    checked: Instant,
    modified: Option<SystemTime>,
    sky: Result<HdriHandle, String>,
}

/// How often a cached file's modification time is looked at.
const RECHECK: Duration = Duration::from_millis(500);

static FILES: LazyLock<Mutex<HashMap<String, Entry>>> = LazyLock::new(Default::default);

/// The sky of the HDRI at project path `rel` ("Assets/HDRI/Sunset.hdr"); `None` when missing
/// or unreadable (see [`error`]). Decodes on first use.
pub fn load(rel: &str) -> Option<HdriHandle> {
    let path = crate::assets::absolute(rel);
    let mut files = FILES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = files.get_mut(rel) {
        if e.checked.elapsed() < RECHECK {
            return e.sky.clone().ok();
        }
        e.checked = Instant::now();
        if crate::vfs::modified(&path) == e.modified {
            return e.sky.clone().ok();
        }
    }
    let modified = crate::vfs::modified(&path);
    let sky = HdrImage::load(&path).map(|img| HdriHandle(Arc::new(HdriSky::new(img))));
    files.insert(
        rel.to_owned(),
        Entry {
            checked: Instant::now(),
            modified,
            sky: sky.clone(),
        },
    );
    sky.ok()
}

/// Why `rel` could not be loaded (after a failed [`load`]).
pub fn error(rel: &str) -> Option<String> {
    let files = FILES.lock().unwrap_or_else(|e| e.into_inner());
    files.get(rel).and_then(|e| e.sky.as_ref().err().cloned())
}
