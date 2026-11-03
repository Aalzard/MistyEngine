//! Per-scene lighting environment (Window ▸ Rendering ▸ Lighting ▸ Environment): skybox,
//! ambient light, reflections and fog. Stored in the scene file under `"lighting"`; the
//! defaults reproduce the look scenes had before these settings existed.

use serde::{Deserialize, Serialize};

/// What the camera's "Skybox" clear flag draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkyboxKind {
    /// Gradient sky driven by the sun (main Directional Light).
    #[default]
    Procedural,
    SolidColor,
}

impl SkyboxKind {
    pub const ALL: [SkyboxKind; 2] = [Self::Procedural, Self::SolidColor];

    pub fn label(self) -> &'static str {
        match self {
            Self::Procedural => "Procedural",
            Self::SolidColor => "Solid Color",
        }
    }
}

/// How the sun disc is drawn in the procedural sky.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SunDisk {
    None,
    /// Hard-edged disc.
    Simple,
    /// Disc plus a soft halo.
    #[default]
    HighQuality,
}

impl SunDisk {
    pub const ALL: [SunDisk; 3] = [Self::None, Self::Simple, Self::HighQuality];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Simple => "Simple",
            Self::HighQuality => "High Quality",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Skybox {
    pub kind: SkyboxKind,
    pub sun_disk: SunDisk,
    /// 0..1; angular size of the disc.
    pub sun_size: f32,
    /// 0..5; 0 = thin (dark sky), 1 = default, higher = hazier.
    pub atmosphere_thickness: f32,
    /// sRGB; mid grey = no tint.
    pub sky_tint: [f32; 3],
    /// sRGB colour below the horizon.
    pub ground: [f32; 3],
    pub exposure: f32,
    /// sRGB colour of the Solid Color skybox.
    pub color: [f32; 3],
}

impl Default for Skybox {
    fn default() -> Self {
        Self {
            kind: SkyboxKind::Procedural,
            sun_disk: SunDisk::HighQuality,
            sun_size: 0.04,
            atmosphere_thickness: 1.0,
            sky_tint: [0.5, 0.5, 0.5],
            ground: [0.32, 0.30, 0.29],
            exposure: 1.0,
            color: [0.19, 0.30, 0.47],
        }
    }
}

/// Where ambient (indirect diffuse) light comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmbientSource {
    #[default]
    Skybox,
    /// Sky / equator / ground colours.
    Gradient,
    Color,
}

impl AmbientSource {
    pub const ALL: [AmbientSource; 3] = [Self::Skybox, Self::Gradient, Self::Color];

    pub fn label(self) -> &'static str {
        match self {
            Self::Skybox => "Skybox",
            Self::Gradient => "Gradient",
            Self::Color => "Color",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EnvironmentLighting {
    pub source: AmbientSource,
    /// Multiplier on the ambient light (all sources).
    pub intensity: f32,
    /// sRGB colours for the Gradient source; `sky_color` is also the Color source.
    pub sky_color: [f32; 3],
    pub equator_color: [f32; 3],
    pub ground_color: [f32; 3],
}

impl Default for EnvironmentLighting {
    fn default() -> Self {
        Self {
            source: AmbientSource::Skybox,
            intensity: 1.0,
            sky_color: [0.212, 0.227, 0.259],
            equator_color: [0.114, 0.125, 0.133],
            ground_color: [0.047, 0.043, 0.035],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FogMode {
    Linear,
    Exponential,
    #[default]
    ExponentialSquared,
}

impl FogMode {
    pub const ALL: [FogMode; 3] = [Self::Linear, Self::Exponential, Self::ExponentialSquared];

    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Exponential => "Exponential",
            Self::ExponentialSquared => "Exponential Squared",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fog {
    pub enabled: bool,
    /// sRGB.
    pub color: [f32; 3],
    pub mode: FogMode,
    /// Exponential modes.
    pub density: f32,
    /// Linear mode: distances from the camera.
    pub start: f32,
    pub end: f32,
}

impl Default for Fog {
    fn default() -> Self {
        Self {
            enabled: false,
            color: [0.5, 0.5, 0.5],
            mode: FogMode::ExponentialSquared,
            density: 0.01,
            start: 0.0,
            end: 300.0,
        }
    }
}

impl Fog {
    /// How much of the surface stays visible at `distance` (1 = no fog, 0 = fog colour).
    pub fn visibility(&self, distance: f32) -> f32 {
        if !self.enabled {
            return 1.0;
        }
        let d = distance.max(0.0);
        let f = match self.mode {
            FogMode::Linear => (self.end - d) / (self.end - self.start).max(1e-4),
            FogMode::Exponential => (-self.density * d).exp(),
            FogMode::ExponentialSquared => (-(self.density * d).powi(2)).exp(),
        };
        f.clamp(0.0, 1.0)
    }
}

/// Everything in the Lighting window's Environment section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderSettings {
    pub skybox: Skybox,
    pub environment_lighting: EnvironmentLighting,
    /// Multiplier on sky reflections.
    pub reflection_intensity: f32,
    pub fog: Fog,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            skybox: Skybox::default(),
            environment_lighting: EnvironmentLighting::default(),
            reflection_intensity: 1.0,
            fog: Fog::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fog_modes_follow_the_usual_formulas() {
        let mut f = Fog {
            enabled: true,
            mode: FogMode::Linear,
            start: 10.0,
            end: 20.0,
            ..Default::default()
        };
        assert_eq!(f.visibility(5.0), 1.0);
        assert!((f.visibility(15.0) - 0.5).abs() < 1e-6);
        assert_eq!(f.visibility(30.0), 0.0);
        f.mode = FogMode::Exponential;
        f.density = 0.1;
        assert!((f.visibility(10.0) - (-1.0f32).exp()).abs() < 1e-6);
        f.mode = FogMode::ExponentialSquared;
        assert!((f.visibility(20.0) - (-4.0f32).exp()).abs() < 1e-6);
        f.enabled = false;
        assert_eq!(f.visibility(1e6), 1.0);
    }
}
