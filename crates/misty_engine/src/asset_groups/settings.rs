//! The Asset Groups settings file: which assets are addressable, their addresses and labels, and
//! how each group is packed into bundles.
//!
//! Stored in `<Project>/ProjectSettings/AssetGroups.json`. Entries remember the asset's GUID
//! (when the project has one for it) and its path; on load the GUID wins, so renamed or moved
//! assets stay addressable.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::asset_db::GuidDatabase;

/// Project-relative location of the settings file.
pub const SETTINGS_FILE: &str = "ProjectSettings/AssetGroups.json";

/// Name of the group every project has (new addressables land here).
pub const DEFAULT_GROUP: &str = "Default Local Group";

/// How a group's entries become bundle files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BundleMode {
    /// One bundle with every entry of the group.
    #[default]
    PackTogether,
    /// One bundle per entry.
    PackSeparately,
    /// One bundle per label (entries without labels share one).
    PackByLabel,
}

impl BundleMode {
    pub const ALL: [BundleMode; 3] = [
        BundleMode::PackTogether,
        BundleMode::PackSeparately,
        BundleMode::PackByLabel,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BundleMode::PackTogether => "Pack Together",
            BundleMode::PackSeparately => "Pack Separately",
            BundleMode::PackByLabel => "Pack Together By Label",
        }
    }
}

/// Compression of a group's bundles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BundleCompression {
    Uncompressed,
    /// Fast to unpack (LZ4 blocks).
    #[default]
    Lz4,
    /// Smaller files, slower to unpack (zlib).
    Zlib,
}

impl BundleCompression {
    pub const ALL: [BundleCompression; 3] = [
        BundleCompression::Uncompressed,
        BundleCompression::Lz4,
        BundleCompression::Zlib,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BundleCompression::Uncompressed => "Uncompressed",
            BundleCompression::Lz4 => "LZ4",
            BundleCompression::Zlib => "Zlib",
        }
    }

    pub fn pak(self) -> crate::pak::Compression {
        match self {
            BundleCompression::Uncompressed => crate::pak::Compression::None,
            BundleCompression::Lz4 => crate::pak::Compression::Lz4,
            BundleCompression::Zlib => crate::pak::Compression::Zlib,
        }
    }
}

/// Where a group's bundles are loaded from at runtime.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BundleLocation {
    /// Shipped with the game (copied into the player build).
    #[default]
    Local,
    /// Downloaded from [`AssetGroup::remote_url`] and cached.
    Remote,
}

/// "Play Mode Script": how the editor's Play mode loads addressables.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayModeScript {
    /// Straight from the project files (fastest, nothing to build).
    #[default]
    UseAssetDatabase,
    /// Project files, but bundles, dependencies and reference counts are tracked as if built.
    SimulateGroups,
    /// The bundles of the last Build ▸ Asset Bundles for this platform.
    UseBuiltBundles,
}

impl PlayModeScript {
    pub const ALL: [PlayModeScript; 3] = [
        PlayModeScript::UseAssetDatabase,
        PlayModeScript::SimulateGroups,
        PlayModeScript::UseBuiltBundles,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PlayModeScript::UseAssetDatabase => "Use Asset Database (fastest)",
            PlayModeScript::SimulateGroups => "Simulate Groups",
            PlayModeScript::UseBuiltBundles => "Use Built Bundles",
        }
    }
}

/// One addressable asset.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetEntry {
    /// GUID from the asset's meta file (empty for assets without one, e.g. in Packages).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub guid: String,
    /// Project-relative path ("Assets/Prefabs/Ghost.prefab").
    pub path: String,
    /// The key scripts load it by (defaults to the path).
    pub address: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
}

/// A group of addressables packed with the same settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetGroup {
    pub name: String,
    pub entries: Vec<AssetEntry>,
    pub bundle_mode: BundleMode,
    pub compression: BundleCompression,
    /// Unticked groups are skipped by Build ▸ Asset Bundles.
    pub include_in_build: bool,
    pub location: BundleLocation,
    /// Remote groups: the folder URL bundles are fetched from. `{Platform}` becomes the build
    /// platform ("windows-x64").
    pub remote_url: String,
}

impl Default for AssetGroup {
    fn default() -> Self {
        Self {
            name: DEFAULT_GROUP.into(),
            entries: Vec::new(),
            bundle_mode: BundleMode::PackTogether,
            compression: BundleCompression::Lz4,
            include_in_build: true,
            location: BundleLocation::Local,
            remote_url: "https://example.com/bundles/{Platform}".into(),
        }
    }
}

impl AssetGroup {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// The remote folder URL for `platform` (template expanded, no trailing slash).
    pub fn remote_url_for(&self, platform: &str) -> String {
        self.remote_url
            .replace("{Platform}", platform)
            .trim_end_matches('/')
            .to_owned()
    }
}

/// The whole settings file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetGroupSettings {
    pub groups: Vec<AssetGroup>,
    /// Editor only: how Play mode loads addressables.
    pub play_mode: PlayModeScript,
}

impl Default for AssetGroupSettings {
    fn default() -> Self {
        Self {
            groups: vec![AssetGroup::default()],
            play_mode: PlayModeScript::default(),
        }
    }
}

impl AssetGroupSettings {
    pub fn path(root: &Path) -> std::path::PathBuf {
        root.join(SETTINGS_FILE)
    }

    /// The project's settings (defaults when the file is missing), with paths refreshed from
    /// GUIDs when the project has meta files.
    pub fn load(root: &Path) -> Result<Self, String> {
        let text = match crate::vfs::read_to_string(Self::path(root)) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.to_string()),
        };
        let mut s = Self::from_json(&text)?;
        if !crate::vfs::is_mounted() {
            crate::asset_db::with_project(root, |db| s.refresh_paths(db));
        }
        Ok(s)
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut s: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        s.sanitize();
        Ok(s)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let path = Self::path(root);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, self.to_json()).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The default group exists; group names are unique; empty addresses become the path.
    pub fn sanitize(&mut self) {
        if !self.groups.iter().any(|g| g.name == DEFAULT_GROUP) {
            self.groups.insert(0, AssetGroup::default());
        }
        let mut seen = BTreeSet::new();
        for g in &mut self.groups {
            let base = g.name.trim().to_owned();
            let base = if base.is_empty() {
                "Group".into()
            } else {
                base
            };
            let mut name = base.clone();
            let mut n = 1;
            while !seen.insert(name.clone()) {
                n += 1;
                name = format!("{base} {n}");
            }
            g.name = name;
            for e in &mut g.entries {
                if e.address.trim().is_empty() {
                    e.address = e.path.clone();
                }
                e.labels.retain(|l| !l.trim().is_empty());
                e.labels.dedup();
            }
        }
    }

    /// Follow renamed / moved assets by GUID.
    pub fn refresh_paths(&mut self, guids: &GuidDatabase) {
        for e in self.groups.iter_mut().flat_map(|g| g.entries.iter_mut()) {
            if let Ok(g) = e.guid.parse()
                && let Some(p) = guids.path_of(g)
                && p != e.path
            {
                if e.address == e.path {
                    e.address = p.to_owned();
                }
                e.path = p.to_owned();
            }
        }
    }

    pub fn group(&self, name: &str) -> Option<&AssetGroup> {
        self.groups.iter().find(|g| g.name == name)
    }

    pub fn group_mut(&mut self, name: &str) -> Option<&mut AssetGroup> {
        self.groups.iter_mut().find(|g| g.name == name)
    }

    /// Every entry with its group.
    pub fn entries(&self) -> impl Iterator<Item = (&AssetGroup, &AssetEntry)> {
        self.groups
            .iter()
            .flat_map(|g| g.entries.iter().map(move |e| (g, e)))
    }

    /// The entry of asset `path` and the name of its group.
    pub fn entry(&self, path: &str) -> Option<(&str, &AssetEntry)> {
        self.entries()
            .find(|(_, e)| e.path == path)
            .map(|(g, e)| (g.name.as_str(), e))
    }

    pub fn entry_mut(&mut self, path: &str) -> Option<&mut AssetEntry> {
        self.groups
            .iter_mut()
            .flat_map(|g| g.entries.iter_mut())
            .find(|e| e.path == path)
    }

    /// Mark `path` addressable (in the default group, address = path) or not.
    /// Returns whether anything changed.
    pub fn set_addressable(&mut self, path: &str, on: bool, guid: Option<String>) -> bool {
        let has = self.entry(path).is_some();
        if on == has {
            return false;
        }
        if on {
            self.sanitize();
            let entry = AssetEntry {
                guid: guid.unwrap_or_default(),
                path: path.to_owned(),
                address: path.to_owned(),
                labels: Vec::new(),
            };
            if let Some(g) = self.group_mut(DEFAULT_GROUP) {
                g.entries.push(entry);
            }
        } else {
            for g in &mut self.groups {
                g.entries.retain(|e| e.path != path);
            }
        }
        true
    }

    /// Move the entry of `path` to `group` (which must exist).
    pub fn move_entry(&mut self, path: &str, group: &str) -> bool {
        if self.group(group).is_none() {
            return false;
        }
        let mut taken = None;
        for g in &mut self.groups {
            if let Some(i) = g.entries.iter().position(|e| e.path == path) {
                if g.name == group {
                    return false;
                }
                taken = Some(g.entries.remove(i));
            }
        }
        match (taken, self.group_mut(group)) {
            (Some(e), Some(g)) => {
                g.entries.push(e);
                true
            }
            _ => false,
        }
    }

    /// Add a new group named "New Group" (numbered when taken); returns its name.
    pub fn add_group(&mut self) -> String {
        let mut name = "New Group".to_owned();
        let mut n = 1;
        while self.group(&name).is_some() {
            n += 1;
            name = format!("New Group {n}");
        }
        self.groups.push(AssetGroup::new(name.clone()));
        name
    }

    /// Remove a group; its entries move to the default group (which can't be removed).
    pub fn remove_group(&mut self, name: &str) -> bool {
        if name == DEFAULT_GROUP {
            return false;
        }
        let Some(i) = self.groups.iter().position(|g| g.name == name) else {
            return false;
        };
        let g = self.groups.remove(i);
        if let Some(d) = self.group_mut(DEFAULT_GROUP) {
            d.entries.extend(g.entries);
        }
        true
    }

    /// Every label used by an entry, sorted.
    pub fn labels(&self) -> BTreeSet<String> {
        self.entries()
            .flat_map(|(_, e)| e.labels.iter().cloned())
            .collect()
    }

    /// Asset paths for `key`: the entry with that address, else every entry with that label.
    pub fn resolve(&self, key: &str) -> Vec<String> {
        if let Some((_, e)) = self.entries().find(|(_, e)| e.address == key) {
            return vec![e.path.clone()];
        }
        self.entries()
            .filter(|(_, e)| e.labels.iter().any(|l| l == key))
            .map(|(_, e)| e.path.clone())
            .collect()
    }
}
