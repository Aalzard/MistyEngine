//! Asset GUIDs and `.meta` files: every asset keeps a stable id, so it can be renamed or moved
//! without breaking the files that reference it.
//!
//! - [`meta`]: the `<asset>.meta` sidecars (GUID plus import settings).
//! - [`GuidDatabase`]: GUID ↔ path map of a project, built by scanning the meta files.
//! - [`refs`]: the GUID table saved into referencing files and resolved on load.
//!
//! The project's database is a global like the project root: the editor opens it with
//! [`open`] (scanning and creating meta files), built games with [`open_built`] (read only).
//! Nothing is installed by default, so tools and tests that never open one read and write
//! plain path references.

mod database;
mod guid;
pub mod meta;
pub mod refs;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::RwLock;

pub use database::{GuidDatabase, ScanReport};
pub use guid::Guid;

/// `guid → path` map packed into built games (pak-relative).
pub const MAP_FILE: &str = "AssetGuids.json";

static DB: RwLock<Option<GuidDatabase>> = RwLock::new(None);

fn read() -> std::sync::RwLockReadGuard<'static, Option<GuidDatabase>> {
    DB.read().unwrap_or_else(|e| e.into_inner())
}

fn write() -> std::sync::RwLockWriteGuard<'static, Option<GuidDatabase>> {
    DB.write().unwrap_or_else(|e| e.into_inner())
}

/// Editor: scan the project at `root` (creating missing meta files) and make it current.
pub fn open(root: &Path) -> ScanReport {
    let (db, report) = GuidDatabase::scan(root);
    install(db);
    report
}

/// Built game: the packed GUID map when there is one, else the project's meta files.
pub fn open_built(root: &Path) {
    let map = crate::vfs::read_to_string(root.join(MAP_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<BTreeMap<String, String>>(&t).ok());
    install(match map {
        Some(m) => GuidDatabase::from_map(root, &m),
        None => GuidDatabase::read(root),
    });
}

pub fn install(db: GuidDatabase) {
    *write() = Some(db);
}

pub fn close() {
    *write() = None;
}

pub fn is_open() -> bool {
    read().is_some()
}

/// Run `f` on the current database.
pub fn with<R>(f: impl FnOnce(&GuidDatabase) -> R) -> Option<R> {
    read().as_ref().map(f)
}

/// Scan the current project again (after file operations). `None` when none is open.
pub fn rescan() -> Option<ScanReport> {
    let root = with(|db| db.root().to_path_buf())?;
    Some(open(&root))
}

/// File watcher: rescan when files or folders appeared or vanished under `Assets/` since the
/// last scan (created, copied or moved outside the editor). Returns the scan's report then.
pub fn poll() -> Option<ScanReport> {
    let (root, known) = with(|db| {
        let known: BTreeSet<String> = db.paths().map(str::to_owned).collect();
        (db.root().to_path_buf(), known)
    })?;
    let mut now = BTreeSet::new();
    database::walk(&root.join("Assets"), &mut |p| {
        now.insert(database::rel(&root, p));
    });
    (now != known).then(|| open(&root))
}

pub fn guid_of(rel: &str) -> Option<Guid> {
    with(|db| db.guid_of(rel)).flatten()
}

pub fn path_of(guid: Guid) -> Option<String> {
    with(|db| db.path_of(guid).map(str::to_owned)).flatten()
}

/// `old` (a file or folder) was renamed or moved to `new` with its meta files.
pub fn moved(old: &str, new: &str) {
    if let Some(db) = write().as_mut() {
        db.moved(old, new);
    }
}

/// `rel` (and everything under it) was deleted.
pub fn removed(rel: &str) {
    if let Some(db) = write().as_mut() {
        db.remove(rel);
    }
}

/// A new GUID for `asset` in the open project (`None` when none is open or it's outside).
pub(crate) fn register(asset: &Path) -> Option<Guid> {
    let mut lock = write();
    let db = lock.as_mut()?;
    let rel = database::rel(db.root(), asset);
    if !rel.starts_with("Assets/") {
        return None;
    }
    let g = db.guid_of(&rel).unwrap_or_default();
    db.insert(&rel, g);
    Some(g)
}

/// Saving a referencing file: add the GUID table (unchanged without an open project).
pub fn embed_json(text: &str) -> String {
    with(|db| refs::embed(db, text)).unwrap_or_else(|| text.to_owned())
}

/// Loading a referencing file: follow renamed or moved assets by GUID (see [`refs::resolve`]).
pub fn resolve_json(text: &str) -> Cow<'_, str> {
    if !text.contains(refs::KEY) {
        return Cow::Borrowed(text);
    }
    let lock = read();
    match lock.as_ref() {
        Some(db) => Cow::Owned(refs::resolve(db, text).into_owned()),
        // No project open: the paths are used as they are.
        None => Cow::Borrowed(text),
    }
}
