//! Package registries: a JSON index at a URL (Project Settings ▸ Package Manager) listing
//! packages and, per version, the `.zip` to download, its SHA-256 and its dependencies.
//!
//! ```json
//! { "packages": [ {
//!     "name": "com.studio.dialogue", "displayName": "Dialogue", "description": "…",
//!     "author": "Studio", "license": "MIT",
//!     "versions": { "1.0.0": { "url": "https://…/dialogue-1.0.0.zip", "sha256": "…",
//!                              "dependencies": { "com.misty.modules.ui": "*" } } } } ] }
//! ```
//! Relative `url`s are relative to the index.

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, channel};

use serde::{Deserialize, Serialize};

use super::semver::Version;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegistryVersion {
    pub url: String,
    pub sha256: String,
    pub dependencies: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RegistryPackage {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub versions: BTreeMap<String, RegistryVersion>,
}

impl RegistryPackage {
    /// Versions, newest first.
    pub fn sorted_versions(&self) -> Vec<Version> {
        let mut v: Vec<Version> = self
            .versions
            .keys()
            .filter_map(|k| Version::parse(k).ok())
            .collect();
        v.sort();
        v.reverse();
        v
    }

    pub fn latest(&self) -> Option<Version> {
        self.sorted_versions()
            .into_iter()
            .find(|v| v.pre.is_empty())
            .or_else(|| self.sorted_versions().into_iter().next())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegistryIndex {
    pub packages: Vec<RegistryPackage>,
}

impl RegistryIndex {
    /// Parse an index fetched from `base_url` (relative archive URLs become absolute).
    pub fn parse(text: &str, base_url: &str) -> Result<Self, String> {
        let mut index: Self =
            serde_json::from_str(text).map_err(|e| format!("registry index: {e}"))?;
        let base = base_url.rsplit_once('/').map_or("", |(b, _)| b);
        for p in &mut index.packages {
            for v in p.versions.values_mut() {
                if !v.url.contains("://") && !base.is_empty() {
                    v.url = format!("{base}/{}", v.url.trim_start_matches("./"));
                }
            }
        }
        Ok(index)
    }

    pub fn package(&self, name: &str) -> Option<&RegistryPackage> {
        self.packages.iter().find(|p| p.name == name)
    }

    pub fn version(&self, name: &str, version: &Version) -> Option<&RegistryVersion> {
        let p = self.package(name)?;
        p.versions
            .iter()
            .find(|(k, _)| Version::parse(k).ok().as_ref() == Some(version))
            .map(|(_, v)| v)
    }
}

/// Fetch and parse the index at `url` (https://, http:// or file://).
pub fn fetch_index(url: &str) -> Result<RegistryIndex, String> {
    let bytes = misty_engine::asset_groups::fetch::fetch(url)?;
    let text = String::from_utf8(bytes).map_err(|_| format!("{url}: not UTF-8 text"))?;
    RegistryIndex::parse(&text, url)
}

/// Fetch the index on a background thread.
pub fn fetch_index_async(url: String) -> Receiver<Result<RegistryIndex, String>> {
    let (tx, rx) = channel();
    let _ = std::thread::Builder::new()
        .name("registry index".into())
        .spawn(move || {
            let _ = tx.send(fetch_index(&url));
        });
    rx
}
