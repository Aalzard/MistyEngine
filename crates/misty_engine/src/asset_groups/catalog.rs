//! The catalog: every address and label, the bundle holding each asset, the bundles each bundle
//! needs, and the bundle files' hashes. Built by Build ▸ Asset Bundles next to the bundles
//! (`catalog.json`) and read by the runtime to turn an address into bundles to load.
//!
//! [`plan`] works the layout out from the settings without writing anything: explicit entries go
//! to their group's bundle(s); the files they reference that are not addressable themselves go
//! into every bundle that uses them (the duplicates the Analyze tool reports); references to
//! another bundle's addressable become a bundle dependency.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::settings::{AssetGroupSettings, BundleCompression, BundleLocation, BundleMode};
use crate::asset_db::GuidDatabase;

/// File name of the catalog in a bundle folder.
pub const CATALOG_FILE: &str = "catalog.json";
/// Project-relative folder Build ▸ Asset Bundles writes to (`<this>/<platform>/`).
pub const BUILD_DIR: &str = "Builds/Bundles";
/// Sub-folder of a platform's build folder holding the remote groups' bundles (to upload).
pub const REMOTE_DIR: &str = "Remote";
/// Folder (relative to a built game's data folder) holding the catalog and local bundles.
pub const PLAYER_DIR: &str = "Bundles";
/// Bumped when the catalog layout changes.
pub const CATALOG_VERSION: u32 = 1;

/// One bundle file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogBundle {
    /// File name ("default_local_group_all.bundle").
    pub name: String,
    pub group: String,
    /// SHA-256 of the file (hex); empty in a simulated catalog.
    pub hash: String,
    pub size: u64,
    pub compression: BundleCompression,
    /// Downloaded from `url` instead of shipped with the game.
    pub remote: bool,
    /// Remote bundles: the folder URL (the file is `<url>/<name>`).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
    /// Bundles that must be loaded first (they hold addressables this one references).
    pub dependencies: Vec<String>,
    /// Every file packed into it (addressables and the files only they use).
    pub assets: Vec<String>,
}

/// One addressable.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogEntry {
    pub address: String,
    pub path: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub guid: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    pub bundle: String,
    /// Files it references (directly or not), without its own path.
    pub dependencies: Vec<String>,
}

/// The whole catalog.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Catalog {
    pub version: u32,
    /// Build platform slug ("windows-x64").
    pub platform: String,
    pub bundles: Vec<CatalogBundle>,
    pub entries: Vec<CatalogEntry>,
}

impl Catalog {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let c: Self = serde_json::from_str(text).map_err(|e| format!("catalog: {e}"))?;
        if c.version > CATALOG_VERSION {
            return Err(format!(
                "catalog version {} is newer than this engine ({CATALOG_VERSION})",
                c.version
            ));
        }
        Ok(c)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text)
    }

    pub fn bundle(&self, name: &str) -> Option<&CatalogBundle> {
        self.bundles.iter().find(|b| b.name == name)
    }

    /// Entries for `key`: the one with that address, else every entry with that label.
    pub fn resolve(&self, key: &str) -> Vec<&CatalogEntry> {
        if let Some(e) = self.entries.iter().find(|e| e.address == key) {
            return vec![e];
        }
        self.entries
            .iter()
            .filter(|e| e.labels.iter().any(|l| l == key))
            .collect()
    }

    /// Bundles to load for `bundle`, dependencies first (each once, cycles ignored).
    pub fn load_order(&self, bundle: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut visiting = BTreeSet::new();
        self.visit(bundle, &mut visiting, &mut out);
        out
    }

    fn visit(&self, name: &str, visiting: &mut BTreeSet<String>, out: &mut Vec<String>) {
        if out.iter().any(|n| n == name) || !visiting.insert(name.to_owned()) {
            return;
        }
        if let Some(b) = self.bundle(name) {
            for d in &b.dependencies {
                self.visit(d, visiting, out);
            }
        }
        out.push(name.to_owned());
    }

    /// Every label, sorted.
    pub fn labels(&self) -> BTreeSet<String> {
        self.entries
            .iter()
            .flat_map(|e| e.labels.iter().cloned())
            .collect()
    }
}

/// A file packed into several bundles because more than one uses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Duplicate {
    pub path: String,
    pub bundles: Vec<String>,
}

/// What a build would produce.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// Hashes and sizes are left empty.
    pub catalog: Catalog,
    /// Missing references: path → the addressable that names it.
    pub missing: BTreeMap<String, String>,
}

impl Plan {
    /// Files that end up in more than one bundle (implicit dependencies shared by bundles).
    pub fn duplicates(&self) -> Vec<Duplicate> {
        let mut owners: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for b in &self.catalog.bundles {
            for a in &b.assets {
                if !a.ends_with(".meta") {
                    owners.entry(a).or_default().push(b.name.clone());
                }
            }
        }
        owners
            .into_iter()
            .filter(|(_, b)| b.len() > 1)
            .map(|(p, bundles)| Duplicate {
                path: p.to_owned(),
                bundles,
            })
            .collect()
    }
}

/// `"Default Local Group"` → `"default_local_group"` (file-name safe).
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_owned();
    if out.is_empty() { "bundle".into() } else { out }
}

/// Work out the bundles, their contents and the catalog for `settings` (nothing is written).
pub fn plan(project: &Path, settings: &AssetGroupSettings, platform: &str) -> Plan {
    // A copy: the dependency walk below is long, the open database stays unlocked meanwhile.
    let guids: GuidDatabase = crate::asset_db::with_project(project, Clone::clone);
    let mut out = Plan::default();
    out.catalog.version = CATALOG_VERSION;
    out.catalog.platform = platform.to_owned();

    // Explicit entries → bundle names.
    struct Pending {
        entry: CatalogEntry,
        files: BTreeSet<String>,
    }
    let mut pending: Vec<Pending> = Vec::new();
    let mut bundles: BTreeMap<String, CatalogBundle> = BTreeMap::new();
    for g in settings.groups.iter().filter(|g| g.include_in_build) {
        for e in &g.entries {
            let suffix = match g.bundle_mode {
                BundleMode::PackTogether => "all".to_owned(),
                BundleMode::PackSeparately => slug(&e.address),
                BundleMode::PackByLabel => match e.labels.first() {
                    Some(l) => slug(l),
                    None => "unlabeled".into(),
                },
            };
            let name = format!("{}_{suffix}.bundle", slug(&g.name));
            bundles
                .entry(name.clone())
                .or_insert_with(|| CatalogBundle {
                    name: name.clone(),
                    group: g.name.clone(),
                    compression: g.compression,
                    remote: g.location == BundleLocation::Remote,
                    url: if g.location == BundleLocation::Remote {
                        g.remote_url_for(platform)
                    } else {
                        String::new()
                    },
                    ..Default::default()
                });
            let deps =
                crate::asset_deps::collect_only(project, std::slice::from_ref(&e.path), &guids);
            for (m, from) in deps.missing {
                let from = if from.is_empty() {
                    e.path.clone()
                } else {
                    from
                };
                out.missing.entry(m).or_insert(from);
            }
            pending.push(Pending {
                entry: CatalogEntry {
                    address: e.address.clone(),
                    path: e.path.clone(),
                    guid: e.guid.clone(),
                    labels: e.labels.clone(),
                    bundle: name,
                    dependencies: Vec::new(),
                },
                files: deps.files,
            });
        }
    }
    let owner: BTreeMap<String, String> = pending
        .iter()
        .map(|p| (p.entry.path.clone(), p.entry.bundle.clone()))
        .collect();

    let closures: BTreeMap<String, BTreeSet<String>> = pending
        .iter()
        .map(|p| (p.entry.path.clone(), p.files.clone()))
        .collect();
    let mut contents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut needs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for p in &mut pending {
        let own = &p.entry.bundle;
        // What another bundle's addressable brings along comes with that bundle.
        let mut covered: BTreeSet<&String> = BTreeSet::new();
        for f in &p.files {
            if f != &p.entry.path
                && owner.get(f.as_str()).is_some_and(|b| b != own)
                && let Some(c) = closures.get(f)
            {
                covered.extend(c.iter());
            }
        }
        let files = contents.entry(own.clone()).or_default();
        for f in &p.files {
            let base = f.strip_suffix(".meta").unwrap_or(f);
            match owner.get(base) {
                // Another bundle's addressable: load that bundle first.
                Some(b) if b != own => {
                    needs.entry(own.clone()).or_default().insert(b.clone());
                }
                _ if covered.contains(f) && !owner.contains_key(base) => {}
                _ => {
                    files.insert(f.clone());
                }
            }
        }
        p.entry.dependencies = p
            .files
            .iter()
            .filter(|f| !f.ends_with(".meta") && **f != p.entry.path)
            .cloned()
            .collect();
    }
    for (name, mut b) in bundles {
        b.assets = contents
            .remove(&name)
            .unwrap_or_default()
            .into_iter()
            .collect();
        b.dependencies = needs
            .remove(&name)
            .unwrap_or_default()
            .into_iter()
            .collect();
        out.catalog.bundles.push(b);
    }
    out.catalog.entries = pending.into_iter().map(|p| p.entry).collect();
    out
}
