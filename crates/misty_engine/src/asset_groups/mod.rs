//! Asset Groups: assets loaded by address or label at runtime, packed into bundle files that can
//! ship with the game or be downloaded later.
//!
//! - [`settings`]: which assets are addressable (`ProjectSettings/AssetGroups.json`), their
//!   addresses, labels and groups, and each group's packing options.
//! - [`catalog`]: address → bundle + dependencies, worked out by [`catalog::plan`].
//! - [`build`]: Build ▸ Asset Bundles (bundle files + `catalog.json`).
//! - [`runtime`]: handles, reference counts, mounting and unloading bundles.
//! - [`fetch`]: remote bundles (HTTPS download, hash check, local cache).
//!
//! Scripts use it through the Lua `Assets` table (`Assets.LoadAsync`, `InstantiateAsync`,
//! `Release`).

pub mod build;
pub mod catalog;
pub mod fetch;
pub mod runtime;
pub mod settings;
#[cfg(test)]
mod tests;

use std::path::Path;

pub use catalog::{Catalog, CatalogBundle, CatalogEntry, Duplicate};
pub use runtime::{AssetStats, AssetSystem, HandleKind, Provider, Status};
pub use settings::{
    AssetEntry, AssetGroup, AssetGroupSettings, BundleCompression, BundleLocation, BundleMode,
    DEFAULT_GROUP, PlayModeScript,
};

/// Analyze: files that would be packed into more than one bundle.
pub fn analyze(project: &Path, settings: &AssetGroupSettings) -> Vec<Duplicate> {
    catalog::plan(project, settings, "analyze").duplicates()
}
