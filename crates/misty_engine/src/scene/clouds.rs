//! Post-process Volume ▸ Volumetric Clouds: a raymarched cloud layer between two altitudes
//! with a few artist controls and weather presets. The renderer side lives in `render::clouds`.

use serde::{Deserialize, Serialize};

/// Weather presets of the Presets dropdown (they only set the override's values).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudPreset {
    Sparse,
    Cloudy,
    Overcast,
    Stormy,
}

impl CloudPreset {
    pub const ALL: [Self; 4] = [Self::Sparse, Self::Cloudy, Self::Overcast, Self::Stormy];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sparse => "Sparse",
            Self::Cloudy => "Cloudy",
            Self::Overcast => "Overcast",
            Self::Stormy => "Stormy",
        }
    }

    /// `o` with this preset's look (wind, lighting and the enable flags are kept).
    pub fn apply(self, o: &mut CloudsOverride) {
        let (coverage, density, shape, altitude, thickness) = match self {
            Self::Sparse => (0.32, 0.45, 0.35, 1500.0, 1600.0),
            Self::Cloudy => (0.5, 0.55, 0.45, 1400.0, 2200.0),
            Self::Overcast => (0.82, 0.6, 0.2, 1100.0, 2000.0),
            Self::Stormy => (0.95, 1.0, 0.6, 700.0, 3800.0),
        };
        o.coverage = coverage;
        o.density = density;
        o.shape = shape;
        o.altitude = altitude;
        o.thickness = thickness;
    }

    /// The preset `o` currently matches (None = custom values).
    pub fn matching(o: &CloudsOverride) -> Option<Self> {
        Self::ALL.into_iter().find(|p| {
            let mut c = *o;
            p.apply(&mut c);
            c == *o
        })
    }
}

/// Post-process Volume ▸ Volumetric Clouds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CloudsOverride {
    pub active: bool,
    pub enabled: bool,
    /// 0 = clear sky, 1 = fully overcast.
    pub coverage: f32,
    /// Scales the cloud extinction (0..2): thin and bright ↔ thick and dark.
    pub density: f32,
    /// 0 = fluffy (soft billowy edges), 1 = sharp (crisp, hard-edged shapes).
    pub shape: f32,
    /// Cloud base above the ground (metres).
    pub altitude: f32,
    /// Layer height from the base to the tallest tops (metres).
    pub thickness: f32,
    /// Direction the wind blows towards, degrees clockwise from +Z (north).
    pub wind_direction: f32,
    /// Metres per second the clouds drift.
    pub wind_speed: f32,
    /// 0..1: bright rim where the sun is behind thin cloud edges.
    pub silver_lining: f32,
    /// 0..2: scales the sky light the clouds pick up.
    pub ambient: f32,
    /// Clouds shadow the ground, the fog and its light shafts.
    pub shadows: bool,
    /// 0..1: how dark the cloud shadows are.
    pub shadow_opacity: f32,
}

impl Default for CloudsOverride {
    fn default() -> Self {
        let mut o = Self {
            active: false,
            enabled: true,
            coverage: 0.0,
            density: 0.0,
            shape: 0.0,
            altitude: 0.0,
            thickness: 0.0,
            wind_direction: 60.0,
            wind_speed: 12.0,
            silver_lining: 0.6,
            ambient: 1.0,
            shadows: true,
            shadow_opacity: 0.8,
        };
        CloudPreset::Cloudy.apply(&mut o);
        o
    }
}

impl CloudsOverride {
    pub fn is_on(&self) -> bool {
        self.active && self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_round_trip_and_default_is_cloudy() {
        let mut o = CloudsOverride::default();
        assert_eq!(CloudPreset::matching(&o), Some(CloudPreset::Cloudy));
        for p in CloudPreset::ALL {
            p.apply(&mut o);
            assert_eq!(CloudPreset::matching(&o), Some(p));
        }
        o.coverage = 0.123;
        assert_eq!(CloudPreset::matching(&o), None);
        // Presets keep wind and lighting.
        o.wind_speed = 3.0;
        CloudPreset::Sparse.apply(&mut o);
        assert_eq!(o.wind_speed, 3.0);
    }

    #[test]
    fn old_files_load_with_defaults() {
        let o: CloudsOverride = serde_json::from_str(r#"{ "active": true }"#).unwrap();
        assert!(o.is_on());
        assert_eq!(o.altitude, CloudsOverride::default().altitude);
    }
}
