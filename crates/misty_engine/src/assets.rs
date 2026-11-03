//! Asset types stored as files in a project's `Assets` folder.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::render::MaterialParams;
use crate::render_scene::MaterialSource;

/// A material asset (`.mat`, JSON). Mirrors the Standard shader's main properties.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Material {
    pub shader: String,
    /// Albedo, sRGB + alpha.
    pub color: [f32; 4],
    pub metallic: f32,
    pub smoothness: f32,
    pub emission_enabled: bool,
    /// sRGB emission colour.
    pub emission: [f32; 3],
    pub emission_intensity: f32,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            shader: "Standard".into(),
            color: [1.0; 4],
            metallic: 0.0,
            smoothness: 0.5,
            emission_enabled: false,
            emission: [0.0; 3],
            emission_intensity: 1.0,
        }
    }
}

impl Material {
    pub fn params(&self) -> MaterialParams {
        MaterialParams {
            color: self.color,
            metallic: self.metallic,
            smoothness: self.smoothness,
            emission: self.emission,
            emission_intensity: if self.emission_enabled {
                self.emission_intensity
            } else {
                0.0
            },
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| e.to_string())
    }
}

/// Read-only asset lookups for a built game: every material under `Assets/`, loaded once.
#[derive(Default)]
pub struct AssetCache {
    /// Project-relative path ("Assets/Materials/Red.mat") → material.
    pub materials: BTreeMap<String, Material>,
}

impl AssetCache {
    /// Load the materials of the project at `root`. Unreadable files are skipped.
    pub fn open(root: &Path) -> Self {
        let mut cache = Self::default();
        let mut stack = vec![root.join("Assets")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "mat")
                    && let Ok(m) = Material::load(&path)
                {
                    let rel = path.strip_prefix(root).unwrap_or(&path);
                    let rel = rel.to_string_lossy().replace('\\', "/");
                    cache.materials.insert(rel, m);
                }
            }
        }
        cache
    }
}

impl MaterialSource for AssetCache {
    fn params(&self, rel: Option<&str>) -> MaterialParams {
        rel.and_then(|r| self.materials.get(r))
            .map(Material::params)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn material_round_trips_and_fills_missing_fields() {
        let dir = std::env::temp_dir().join("misty_mat_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Red.mat");
        let m = Material {
            color: [1.0, 0.0, 0.0, 1.0],
            smoothness: 0.8,
            ..Default::default()
        };
        m.save(&path).unwrap();
        assert_eq!(Material::load(&path).unwrap(), m);

        std::fs::write(&path, r#"{ "metallic": 1.0 }"#).unwrap();
        let partial = Material::load(&path).unwrap();
        assert_eq!(partial.metallic, 1.0);
        assert_eq!(partial.smoothness, 0.5, "missing fields use defaults");
    }

    #[test]
    fn emission_only_when_enabled() {
        let mut m = Material {
            emission: [1.0, 0.5, 0.0],
            emission_intensity: 2.0,
            ..Default::default()
        };
        assert_eq!(m.params().emission_intensity, 0.0);
        m.emission_enabled = true;
        assert_eq!(m.params().emission, [1.0, 0.5, 0.0]);
        assert_eq!(m.params().emission_intensity, 2.0);
    }

    #[test]
    fn cache_loads_project_materials() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
        let cache = AssetCache::open(&root);
        let red = cache.params(Some("Assets/Materials/Red.mat"));
        assert_ne!(red, MaterialParams::default());
        assert_eq!(cache.params(None), MaterialParams::default());
        assert_eq!(
            cache.params(Some("Assets/Missing.mat")),
            MaterialParams::default()
        );
    }
}
