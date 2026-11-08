//! Avatar Mask asset (`.mask`, JSON): which transforms an Animator layer animates. Each entry is
//! a path below the Animator's object ("" = the object itself, "Hips/Spine" = a bone) with an
//! on/off flag. An empty list lets everything through; otherwise only listed paths that are on
//! are animated. Only Transform curves are masked (lights, blend shapes and the active flag
//! always pass), like the familiar editor's transform masks.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::clip::Property;

/// File extension of avatar masks.
pub const MASK_EXTENSION: &str = "mask";

pub fn is_mask(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(MASK_EXTENSION))
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MaskEntry {
    pub path: String,
    pub active: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AvatarMask {
    /// Transform paths in hierarchy order.
    pub transforms: Vec<MaskEntry>,
}

/// Whether `path` is `root` or below it ("" is the root of everything).
pub fn path_within(path: &str, root: &str) -> bool {
    root.is_empty()
        || path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

impl AvatarMask {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(&crate::asset_db::resolve_json(text)).map_err(|e| e.to_string())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, crate::asset_db::embed_json(&self.to_json()))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Whether a curve on `path` animating `property` passes the mask.
    pub fn allows(&self, path: &str, property: Property) -> bool {
        if self.transforms.is_empty() || property.component() != "Transform" {
            return true;
        }
        self.transforms
            .iter()
            .find(|t| t.path == path)
            .is_some_and(|t| t.active)
    }

    pub fn is_active(&self, path: &str) -> bool {
        self.transforms
            .iter()
            .find(|t| t.path == path)
            .is_some_and(|t| t.active)
    }

    /// Switch a path on or off; with `recursive`, everything below it too.
    pub fn set_active(&mut self, path: &str, active: bool, recursive: bool) {
        for t in &mut self.transforms {
            if t.path == path || (recursive && path_within(&t.path, path)) {
                t.active = active;
            }
        }
    }

    /// Replace the list with a skeleton's paths (in order), keeping the flags of paths that were
    /// already listed; new paths start on.
    pub fn import(&mut self, paths: &[String]) {
        self.transforms = paths
            .iter()
            .map(|p| MaskEntry {
                path: p.clone(),
                active: self
                    .transforms
                    .iter()
                    .find(|t| t.path == *p)
                    .is_none_or(|t| t.active),
            })
            .collect();
    }

    /// Paths of `root` and everything below it in the scene, in hierarchy order ("" first).
    pub fn skeleton(scene: &crate::scene::Scene, root: hecs::Entity) -> Vec<String> {
        let mut out = vec![String::new()];
        fn walk(scene: &crate::scene::Scene, e: hecs::Entity, prefix: &str, out: &mut Vec<String>) {
            for c in scene.children(e) {
                let name = scene.name(c);
                let path = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                };
                out.push(path.clone());
                walk(scene, c, &path, out);
            }
        }
        walk(scene, root, "", &mut out);
        out
    }
}
