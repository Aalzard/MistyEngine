//! Asset types stored as files in a project's `Assets` folder.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::render::MaterialParams;

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
        let e = if self.emission_enabled {
            self.emission.map(|c| c * self.emission_intensity)
        } else {
            [0.0; 3]
        };
        MaterialParams {
            color: self.color,
            metallic: self.metallic,
            smoothness: self.smoothness,
            emission: e,
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

    #[test]
    fn emission_only_when_enabled() {
        let mut m = Material {
            emission: [1.0, 0.5, 0.0],
            emission_intensity: 2.0,
            ..Default::default()
        };
        assert_eq!(m.params().emission, [0.0; 3]);
        m.emission_enabled = true;
        assert_eq!(m.params().emission, [2.0, 1.0, 0.0]);
    }
}
