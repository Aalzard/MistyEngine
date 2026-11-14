//! Optional built-in packages: engine features that ship with the editor but that a project only
//! gets when someone clicks **Install** in the Package Manager. Unlike the built-in modules
//! ([`super::builtin`]) they are not part of every project: new projects, templates and existing
//! projects don't list them, and **Remove** takes them out again.
//!
//! Installing writes `"<name>": "*"` to `Packages/manifest.json`; resolution then finds the
//! engine's copy (origin built-in) and the lock file records it with source `builtin`. Nothing
//! is copied into `Packages/`. A feature asks [`installed`] (or the editor's cached
//! [`Installed`]) before showing its windows and menu items.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::manifest::{Manifest, PackageJson};

/// The Recorder: video, GIF and image sequences from the game or a camera.
pub const RECORDER: &str = "com.misty.recorder";

/// Behaviour Trees: visual AI for enemies and NPCs (`.btree` assets, the Behaviour Tree window,
/// the Behaviour Tree Agent component).
pub const BEHAVIOUR_TREES: &str = "com.misty.behaviour-trees";

/// (name, display name, description).
const PACKAGES: [(&str, &str, &str); 2] = [
    (
        RECORDER,
        "Recorder",
        "Record the Scene Viewer, a camera or the Editor Scene to MP4, GIF, or PNG / JPG \
         sequences at a fixed frame rate, by hand or from a Timeline.",
    ),
    (
        BEHAVIOUR_TREES,
        "Behaviour Trees",
        "Visual AI for enemies and NPCs: Behaviour Tree assets and their editor, blackboards, \
         the Behaviour Tree Agent component and live debugging in Play mode.",
    ),
];

/// What the manifest asks for when an optional package is installed (any engine version).
pub const REQUIREMENT: &str = "*";

/// Every optional package at the engine's version.
pub fn all() -> Vec<PackageJson> {
    PACKAGES
        .iter()
        .map(|(name, title, desc)| PackageJson {
            name: (*name).into(),
            display_name: (*title).into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: (*desc).into(),
            dependencies: BTreeMap::new(),
            author: super::manifest::Author::Name("Misty Engine".into()),
            license: "GPL-3.0-or-later with the Misty Engine runtime exception".into(),
            samples: Vec::new(),
        })
        .collect()
}

pub fn is_optional(name: &str) -> bool {
    PACKAGES.iter().any(|(n, _, _)| *n == name)
}

/// Whether `project`'s manifest lists the optional package `name`.
#[cfg(test)]
pub fn installed(project: &Path, name: &str) -> bool {
    Manifest::load(project).is_ok_and(|m| m.dependencies.contains_key(name))
}

/// Add an optional package to the manifest (resolve with `install::sync` afterwards).
pub fn install(project: &Path, name: &str) -> Result<PackageJson, String> {
    let info = all()
        .into_iter()
        .find(|p| p.name == name)
        .ok_or_else(|| format!("{name} is not an optional package"))?;
    let mut m = Manifest::load(project)?;
    m.dependencies
        .insert(name.to_owned(), REQUIREMENT.to_owned());
    m.save(project)?;
    Ok(info)
}

/// Take an optional package out of the manifest, with the settings files it kept in the project
/// (`files`, relative to the project). Recordings and other output stay.
pub fn remove(project: &Path, name: &str, files: &[&str]) -> Result<(), String> {
    let mut m = Manifest::load(project)?;
    m.dependencies.remove(name);
    m.save(project)?;
    for f in files {
        let p = project.join(f);
        if p.is_file() {
            std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        }
    }
    Ok(())
}

/// Settings files each optional package keeps in the project (removed with it).
pub fn settings_files(name: &str) -> &'static [&'static str] {
    match name {
        RECORDER => &["ProjectSettings/Recorder.json"],
        _ => &[],
    }
}

/// The optional packages a project has, read once and refreshed after Install / Remove.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Installed(BTreeSet<String>);

impl Installed {
    pub fn load(project: &Path) -> Self {
        let m = Manifest::load(project).unwrap_or_default();
        Self(
            m.dependencies
                .keys()
                .filter(|n| is_optional(n))
                .cloned()
                .collect(),
        )
    }

    pub fn has(&self, name: &str) -> bool {
        self.0.contains(name)
    }
}
