//! Baked light probes file (`Assets/Scenes/<Scene>/LightProbes.probes`): JSON with the world
//! position of every probe and the radiance it sees projected onto order-2 spherical harmonics
//! (9 coefficients × linear RGB, same basis as `render::probes::sh`).

use std::path::Path;

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// Current file format.
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LightProbeData {
    pub version: u32,
    /// World positions, in Hierarchy order of the groups.
    pub positions: Vec<Vec3>,
    /// Radiance SH per probe (not yet convolved with the cosine lobe).
    pub sh: Vec<[[f32; 3]; 9]>,
}

impl Default for LightProbeData {
    fn default() -> Self {
        Self {
            version: VERSION,
            positions: Vec::new(),
            sh: Vec::new(),
        }
    }
}

impl LightProbeData {
    pub fn new(positions: Vec<Vec3>, sh: &[[Vec3; 9]]) -> Self {
        Self {
            version: VERSION,
            positions,
            sh: sh.iter().map(|c| c.map(|v| v.to_array())).collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.positions.len().min(self.sh.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Radiance SH of probe `i`.
    pub fn radiance(&self, i: usize) -> [Vec3; 9] {
        self.sh[i].map(Vec3::from)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("light probes serialise")
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let d: Self =
            serde_json::from_str(text).map_err(|e| format!("invalid light probes file: {e}"))?;
        if d.version > VERSION {
            return Err(format!(
                "light probes saved by a newer version (format {}, this reads {VERSION})",
                d.version
            ));
        }
        if d.positions.len() != d.sh.len() {
            return Err("light probes file: positions and coefficients differ in count".into());
        }
        Ok(d)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let err = |e: std::io::Error| format!("{}: {e}", path.display());
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        std::fs::write(path, self.to_json()).map_err(err)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_round_trips() {
        let mut sh = [Vec3::ZERO; 9];
        for (i, c) in sh.iter_mut().enumerate() {
            *c = Vec3::new(i as f32 * 0.1, -0.25, 1.0 / (i as f32 + 3.0));
        }
        let d = LightProbeData::new(vec![Vec3::new(1.0, 2.0, 3.0), Vec3::NEG_Y], &[sh, sh]);
        let path =
            std::env::temp_dir().join("misty_light_probes_round_trip/Level/LightProbes.probes");
        d.save(&path).unwrap();
        let back = LightProbeData::load(&path).unwrap();
        assert_eq!(back, d);
        assert_eq!(back.radiance(1), sh);
        assert!(LightProbeData::from_json(r#"{"version":99,"positions":[],"sh":[]}"#).is_err());
        assert!(
            LightProbeData::from_json(r#"{"version":1,"positions":[[0,0,0]],"sh":[]}"#).is_err()
        );
    }
}
