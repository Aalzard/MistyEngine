//! Asset types stored as files in a project's `Assets` folder.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::render::MaterialParams;

static PROJECT_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Set the folder that project-relative asset paths ("Assets/…") resolve against.
/// The renderer, model importer and physics read textures and models through it.
pub fn set_project_root(root: &Path) {
    *PROJECT_ROOT.write().unwrap_or_else(|e| e.into_inner()) = Some(root.to_path_buf());
}

/// Absolute path of a project-relative asset path (unchanged when already absolute or no root is set).
pub fn absolute(rel: &str) -> PathBuf {
    match &*PROJECT_ROOT.read().unwrap_or_else(|e| e.into_inner()) {
        Some(root) => root.join(rel),
        None => PathBuf::from(rel),
    }
}

/// "Assets/…" path with forward slashes for a file inside the project (else the full path).
pub fn relative(path: &Path) -> String {
    let root = PROJECT_ROOT.read().unwrap_or_else(|e| e.into_inner());
    root.as_deref()
        .and_then(|r| path.strip_prefix(r).ok())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// File extensions loaded as textures.
pub const TEXTURE_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

pub fn is_texture(path: &str) -> bool {
    has_extension(path, &TEXTURE_EXTENSIONS)
}

pub(crate) fn has_extension(path: &str, list: &[&str]) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| list.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

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
    /// Albedo texture ("Assets/…/Name.png"), multiplied by `color`.
    pub albedo_map: Option<String>,
    /// Tangent-space normal map (OpenGL convention: green = up).
    pub normal_map: Option<String>,
    pub normal_scale: f32,
    /// UV scale and offset applied to both maps.
    pub tiling: [f32; 2],
    pub offset: [f32; 2],
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
            albedo_map: None,
            normal_map: None,
            normal_scale: 1.0,
            tiling: [1.0, 1.0],
            offset: [0.0, 0.0],
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
            albedo_map: self.albedo_map.clone(),
            normal_map: self.normal_map.clone(),
            normal_scale: self.normal_scale,
            tiling: self.tiling,
            offset: self.offset,
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

#[cfg(test)]
mod tests {
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

    /// A `.mat` written before textures existed loads with no maps and identity UVs.
    #[test]
    fn old_material_files_load_without_texture_fields() {
        let old = r#"{
  "shader": "Standard",
  "color": [0.8, 0.1, 0.1, 1.0],
  "metallic": 0.0,
  "smoothness": 0.6,
  "emission_enabled": false,
  "emission": [0.0, 0.0, 0.0],
  "emission_intensity": 1.0
}"#;
        let m: Material = serde_json::from_str(old).unwrap();
        assert_eq!(m.color, [0.8, 0.1, 0.1, 1.0]);
        assert_eq!(m.albedo_map, None);
        assert_eq!(m.normal_map, None);
        assert_eq!(m.normal_scale, 1.0);
        assert_eq!(m.tiling, [1.0, 1.0]);
        assert_eq!(m.offset, [0.0, 0.0]);
        let p = m.params();
        assert_eq!(p.tiling, [1.0, 1.0]);
        assert!(p.albedo_map.is_none());
    }

    #[test]
    fn texture_paths_round_trip() {
        let m = Material {
            albedo_map: Some("Assets/Textures/Checker.png".into()),
            normal_map: Some("Assets/Textures/Bumps.png".into()),
            normal_scale: 0.5,
            tiling: [4.0, 2.0],
            offset: [0.25, 0.0],
            ..Default::default()
        };
        let text = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Material>(&text).unwrap(), m);
        assert!(is_texture("Assets/a.PNG") && is_texture("b.jpeg") && !is_texture("c.mat"));
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
}
