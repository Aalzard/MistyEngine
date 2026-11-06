//! GUID ↔ path map of one project, built from the `.meta` files under `Assets/`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{Guid, meta};

/// What a scan changed on disk.
#[derive(Debug, Default, PartialEq)]
pub struct ScanReport {
    /// Assets that got a new meta file (or a GUID added to one).
    pub created: Vec<String>,
    /// Copies that shared another asset's GUID and were given a new one.
    pub reassigned: Vec<String>,
    /// Meta files that could not be read or written.
    pub errors: Vec<String>,
}

/// Maps GUIDs to project-relative paths ("Assets/Materials/Red.mat") and back.
#[derive(Clone, Debug, Default)]
pub struct GuidDatabase {
    root: PathBuf,
    by_guid: HashMap<Guid, String>,
    by_path: HashMap<String, Guid>,
}

impl GuidDatabase {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            ..Default::default()
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Read the meta files of the project at `root` without writing anything (builds, the
    /// player). Meta files whose asset is gone are ignored.
    pub fn read(root: &Path) -> Self {
        let mut db = Self::new(root);
        for p in crate::vfs::files_under(&root.join("Assets")) {
            if !meta::is_meta(&p) {
                continue;
            }
            let asset = p.with_extension("");
            if !(crate::vfs::is_file(&asset) || asset.is_dir()) {
                continue;
            }
            let guid = crate::vfs::read_to_string(&p)
                .ok()
                .and_then(|t| meta::guid_in(&t));
            if let Some(g) = guid
                && !db.by_guid.contains_key(&g)
            {
                db.insert(&rel(root, &asset), g);
            }
        }
        db
    }

    /// Scan the project at `root` (editor): every file and folder under `Assets/` without a
    /// meta file (or GUID) gets one, and copies sharing a GUID get a new one. Of the assets
    /// sharing a GUID, the one whose meta file was created first keeps it (then the shorter path).
    pub fn scan(root: &Path) -> (Self, ScanReport) {
        let mut report = ScanReport::default();
        let mut found: Vec<(String, Option<Guid>, Option<SystemTime>)> = Vec::new();
        walk(&root.join("Assets"), &mut |asset| {
            let m = meta::meta_path(asset);
            let guid = std::fs::read_to_string(&m)
                .ok()
                .and_then(|t| meta::guid_in(&t));
            // Copies keep the modification time (Explorer, `fs::copy`) but get a new creation time.
            let time = std::fs::metadata(&m)
                .and_then(|x| x.created().or_else(|_| x.modified()))
                .ok();
            found.push((rel(root, asset), guid, time));
        });
        // Oldest meta first, then the shorter path ("Red" before "Red 1"), so the original of a
        // copied pair keeps its GUID.
        found.sort_by(|a, b| (a.2, a.0.len(), &a.0).cmp(&(b.2, b.0.len(), &b.0)));
        let mut db = Self::new(root);
        for (rel, guid, _) in found {
            let taken = guid.is_some_and(|g| db.by_guid.contains_key(&g));
            let guid = match guid {
                Some(g) if !taken => g,
                _ => {
                    let g = Guid::new();
                    match meta::write_guid(&root.join(&rel), g) {
                        Ok(()) if taken => report.reassigned.push(rel.clone()),
                        Ok(()) => report.created.push(rel.clone()),
                        Err(e) => {
                            report.errors.push(e);
                            continue;
                        }
                    }
                    g
                }
            };
            db.insert(&rel, guid);
        }
        report.created.sort();
        report.reassigned.sort();
        (db, report)
    }

    /// A database from a saved `guid → path` map (see [`Self::to_map`]).
    pub fn from_map(root: &Path, map: &BTreeMap<String, String>) -> Self {
        let mut db = Self::new(root);
        for (g, p) in map {
            if let Ok(g) = g.parse() {
                db.insert(p, g);
            }
        }
        db
    }

    /// `guid → path`, sorted (written into built games).
    pub fn to_map(&self) -> BTreeMap<String, String> {
        self.by_guid
            .iter()
            .map(|(g, p)| (g.to_string(), p.clone()))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.by_guid.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_guid.is_empty()
    }

    pub fn guid_of(&self, rel: &str) -> Option<Guid> {
        self.by_path.get(rel).copied()
    }

    pub fn path_of(&self, guid: Guid) -> Option<&str> {
        self.by_guid.get(&guid).map(String::as_str)
    }

    /// Paths currently known (unsorted).
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.by_path.keys().map(String::as_str)
    }

    /// Map `rel` to `guid` (replacing what either was mapped to).
    pub fn insert(&mut self, rel: &str, guid: Guid) {
        if let Some(old) = self.by_path.insert(rel.to_owned(), guid) {
            self.by_guid.remove(&old);
        }
        if let Some(old) = self.by_guid.insert(guid, rel.to_owned())
            && old != rel
        {
            self.by_path.remove(&old);
        }
    }

    /// Forget `rel` and everything under it (a folder).
    pub fn remove(&mut self, rel: &str) {
        let gone: Vec<String> = self
            .by_path
            .keys()
            .filter(|p| under(p, rel))
            .cloned()
            .collect();
        for p in gone {
            if let Some(g) = self.by_path.remove(&p) {
                self.by_guid.remove(&g);
            }
        }
    }

    /// `old` (and everything under it) was renamed or moved to `new`: GUIDs follow.
    pub fn moved(&mut self, old: &str, new: &str) {
        let moved: Vec<(String, Guid)> = self
            .by_path
            .iter()
            .filter(|(p, _)| under(p, old))
            .map(|(p, g)| (format!("{new}{}", &p[old.len()..]), *g))
            .collect();
        self.remove(old);
        for (p, g) in moved {
            self.insert(&p, g);
        }
    }
}

/// `p` is `dir` or inside it.
fn under(p: &str, dir: &str) -> bool {
    p == dir || p.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

/// Project-relative path with forward slashes.
pub(super) fn rel(root: &Path, abs: &Path) -> String {
    abs.strip_prefix(root)
        .unwrap_or(abs)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Every asset file and folder below `dir` (not `.meta` files or hidden names).
pub(super) fn walk(dir: &Path, f: &mut dyn FnMut(&Path)) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') || meta::is_meta(&path) {
            continue;
        }
        f(&path);
        // The directory listing already knows the type: no extra stat per entry.
        let is_dir = entry
            .file_type()
            .map_or_else(|_| path.is_dir(), |t| t.is_dir());
        if is_dir {
            walk(&path, f);
        }
    }
}
