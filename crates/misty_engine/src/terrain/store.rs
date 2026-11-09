//! Process-wide store of loaded terrain data, keyed by asset path. The renderer, physics, scripts
//! and the editor's brushes all read (and the editor writes) the same copy; each change bumps a
//! version so the renderer knows what to re-upload. Brush strokes record the region they touched
//! ([`modify_region`]) so the renderer uploads only that part; other changes count as full.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Instant, SystemTime};

use super::data::TerrainData;
use crate::assets;

/// Changed part of a terrain in normalised coordinates (0..1 along x and z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl Region {
    pub const EMPTY: Region = Region {
        min: [f32::MAX; 2],
        max: [f32::MIN; 2],
    };

    pub fn union(self, o: Region) -> Region {
        Region {
            min: [self.min[0].min(o.min[0]), self.min[1].min(o.min[1])],
            max: [self.max[0].max(o.max[0]), self.max[1].max(o.max[1])],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0] || self.min[1] > self.max[1]
    }

    /// Covered cells of an `n`-wide grid spanning 0..1 (`grow` extra cells on each side):
    /// (x range, z range), clamped to the grid.
    pub fn cells(&self, n: usize, grow: usize) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        let range = |lo: f32, hi: f32| {
            let a = ((lo * n as f32).floor().max(0.0) as usize).saturating_sub(grow);
            let b = (((hi * n as f32).ceil().max(0.0) as usize) + 1 + grow).min(n);
            a.min(b)..b
        };
        (
            range(self.min[0], self.max[0]),
            range(self.min[1], self.max[1]),
        )
    }
}

/// Region changes kept per terrain (older ones fall back to a full upload).
const LOG_LEN: usize = 256;

struct Entry {
    data: Result<Arc<TerrainData>, String>,
    version: u64,
    /// (version, region) of recent partial changes.
    log: std::collections::VecDeque<(u64, Region)>,
    /// Readers older than this version need everything again.
    full_at: u64,
    /// File time when the data was last read from or written to disk.
    disk: Option<SystemTime>,
    /// `version` at that moment: a higher version has changes not on disk yet.
    clean: u64,
}

impl Entry {
    fn new(data: Result<Arc<TerrainData>, String>, version: u64) -> Self {
        Self {
            data,
            version,
            log: Default::default(),
            full_at: version,
            disk: None,
            clean: version,
        }
    }

    /// Read from disk just now (`path` is the file).
    fn loaded(mut self, path: &Path) -> Self {
        self.disk = mtime(path);
        self
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[derive(Default)]
struct Store {
    entries: HashMap<PathBuf, Entry>,
    next_version: u64,
    last_poll: Option<Instant>,
}

impl Store {
    fn bump(&mut self) -> u64 {
        self.next_version += 1;
        self.next_version
    }

    /// The entry for `path`, loading the file on first use.
    fn entry(&mut self, path: &str) -> &mut Entry {
        let key = assets::absolute(path);
        if !self.entries.contains_key(&key) {
            let version = self.bump();
            let data = TerrainData::load(&key).map(Arc::new);
            self.entries
                .insert(key.clone(), Entry::new(data, version).loaded(&key));
        }
        self.entries.get_mut(&key).expect("just inserted")
    }
}

static STORE: LazyLock<Mutex<Store>> = LazyLock::new(Default::default);

fn store() -> std::sync::MutexGuard<'static, Store> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The terrain data at `path` and its version (`None` when the file can't be loaded).
pub fn get(path: &str) -> Option<(Arc<TerrainData>, u64)> {
    if path.is_empty() {
        return None;
    }
    let mut s = store();
    let e = s.entry(path);
    e.data.as_ref().ok().map(|d| (d.clone(), e.version))
}

/// Why `path` could not be loaded.
pub fn error(path: &str) -> Option<String> {
    let mut s = store();
    s.entry(path).data.as_ref().err().cloned()
}

/// Replace the data at `path` (a new asset, an undo); not written to disk.
pub fn insert(path: &str, data: TerrainData) {
    let mut s = store();
    let version = s.bump();
    let key = assets::absolute(path);
    let mut entry = Entry::new(Ok(Arc::new(data)), version).loaded(&key);
    // Not what the file holds (until it is saved).
    entry.clean = 0;
    s.entries.insert(key, entry);
}

/// Change the data at `path` in place; `None` when it isn't loaded.
pub fn modify<R>(path: &str, f: impl FnOnce(&mut TerrainData) -> R) -> Option<R> {
    let mut s = store();
    let version = s.bump();
    let e = s.entry(path);
    let data = e.data.as_mut().ok()?;
    e.version = version;
    e.full_at = version;
    e.log.clear();
    Some(f(Arc::make_mut(data)))
}

/// [`modify`] that only changes `region` (renderers re-upload just that part).
pub fn modify_region<R>(
    path: &str,
    region: Region,
    f: impl FnOnce(&mut TerrainData) -> R,
) -> Option<R> {
    let mut s = store();
    let version = s.bump();
    let e = s.entry(path);
    let data = e.data.as_mut().ok()?;
    e.version = version;
    e.log.push_back((version, region));
    if e.log.len() > LOG_LEN
        && let Some((v, _)) = e.log.pop_front()
    {
        e.full_at = e.full_at.max(v);
    }
    Some(f(Arc::make_mut(data)))
}

/// What changed at `path` since version `since`: `Some(region)` (possibly empty) when only
/// regions changed, `None` when everything must be refreshed.
pub fn changes_since(path: &str, since: u64) -> Option<Region> {
    let mut s = store();
    let e = s.entry(path);
    if since < e.full_at {
        return None;
    }
    Some(
        e.log
            .iter()
            .filter(|(v, _)| *v > since)
            .fold(Region::EMPTY, |acc, (_, r)| acc.union(*r)),
    )
}

/// Write the data at `path` to its file.
pub fn save(path: &str) -> Result<(), String> {
    let (data, version) = get(path).ok_or_else(|| format!("{path} is not loaded"))?;
    let key = assets::absolute(path);
    data.save(&key)?;
    // The file now holds this version: it doesn't count as changed on disk.
    if let Some(e) = store().entries.get_mut(&key)
        && e.version == version
    {
        e.disk = mtime(&key);
        e.clean = version;
    }
    Ok(())
}

/// Re-read terrains whose files changed on disk since they were read or saved (checked at most
/// twice a second). A terrain with edits not saved yet keeps them (its next save wins).
/// Returns the reloaded files.
pub fn poll_changes() -> Vec<PathBuf> {
    let mut s = store();
    let now = Instant::now();
    if s.last_poll
        .is_some_and(|t| now.duration_since(t).as_secs_f32() < 0.5)
    {
        return Vec::new();
    }
    s.last_poll = Some(now);
    let changed: Vec<PathBuf> = s
        .entries
        .iter()
        // The background file watch's stamps first; a difference is checked on disk.
        .filter(|(key, e)| {
            e.version == e.clean
                && e.disk.is_some()
                && crate::file_watch::cached_modified(key) != e.disk
                && mtime(key) != e.disk
        })
        .map(|(key, _)| key.clone())
        .collect();
    let mut reloaded = Vec::new();
    for key in changed {
        // Still being written (or unreadable): try again on a later poll.
        let Ok(data) = TerrainData::load(&key) else {
            continue;
        };
        let version = s.bump();
        s.entries.insert(
            key.clone(),
            Entry::new(Ok(Arc::new(data)), version).loaded(&key),
        );
        reloaded.push(key);
    }
    reloaded
}

/// Forget `path` so the next use reads the file again.
pub fn unload(path: &str) {
    store().entries.remove(&assets::absolute(path));
}

/// [`modify`] for settings that leave heights and splat weights alone (layer properties,
/// cliffs, harmony, detail look): renderers re-upload nothing.
pub fn modify_settings<R>(path: &str, f: impl FnOnce(&mut TerrainData) -> R) -> Option<R> {
    modify_region(path, Region::EMPTY, f)
}

/// Is the data at `path` in the store (loaded or failed) without reading the file?
pub fn is_loaded(path: &str) -> bool {
    store().entries.contains_key(&assets::absolute(path))
}

/// Put data read from `path`'s file elsewhere (a World Partition cell's background load) into
/// the store, as the file holds it. Data with edits not saved yet wins.
pub fn insert_loaded(path: &str, data: TerrainData) {
    let mut s = store();
    let key = assets::absolute(path);
    if s.entries.get(&key).is_some_and(|e| e.version != e.clean) {
        return;
    }
    let version = s.bump();
    let entry = Entry::new(Ok(Arc::new(data)), version).loaded(&key);
    s.entries.insert(key, entry);
}

/// [`unload`] unless the data has edits not saved yet (a streamed cell going away keeps them).
pub fn unload_if_saved(path: &str) {
    let mut s = store();
    let key = assets::absolute(path);
    if s.entries.get(&key).is_some_and(|e| e.version == e.clean) {
        s.entries.remove(&key);
    }
}
