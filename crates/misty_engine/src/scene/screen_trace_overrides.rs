//! Post-process Volume overrides for the screen-space traced lighting effects: Screen Space
//! Global Illumination (one bounce of diffuse light from what is on screen) and Screen Space
//! Reflections (Hi-Z traced mirror rays blurred by roughness). Both are off unless the volume
//! ticks them; the passes live in `render/screen_trace`.

use serde::{Deserialize, Serialize};

/// Ray budget of a screen-space trace.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenSpaceQuality {
    Low,
    #[default]
    Medium,
    High,
}

impl ScreenSpaceQuality {
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }

    /// 0, 1, 2.
    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenSpaceGlobalIlluminationOverride {
    pub active: bool,
    /// 0..4: strength of the bounced light.
    pub intensity: f32,
    /// World-space ray length in metres: how far light still bounces from.
    pub radius: f32,
    /// Assumed depth of the surfaces seen on screen, in metres (a ray behind a surface by
    /// less than this hits it).
    pub thickness: f32,
    /// Rays and steps per pixel.
    pub quality: ScreenSpaceQuality,
    /// Trace every pixel instead of a half-resolution image.
    pub full_resolution: bool,
    /// Accumulate over frames with reprojection (less noise; resets on camera cuts).
    pub temporal: bool,
    /// Edge-aware spatial blur after the accumulation.
    pub denoise: bool,
    /// Lightmapped surfaces also take the screen-space bounce (their lightmap already holds
    /// baked bounce light, so they skip it by default).
    pub lightmapped_surfaces: bool,
}

impl Default for ScreenSpaceGlobalIlluminationOverride {
    fn default() -> Self {
        Self {
            active: false,
            intensity: 1.0,
            radius: 3.0,
            thickness: 0.5,
            quality: ScreenSpaceQuality::Medium,
            full_resolution: false,
            temporal: true,
            denoise: true,
            lightmapped_surfaces: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenSpaceReflectionsOverride {
    pub active: bool,
    /// 0..1: how much the traced reflection replaces the probe / sky reflection.
    pub intensity: f32,
    /// 0..1 perceptual roughness (1 - smoothness) above which surfaces keep the probe / sky
    /// reflection only.
    pub max_roughness: f32,
    /// Assumed depth of the surfaces seen on screen, in metres.
    pub thickness: f32,
    /// Longest reflection ray in metres.
    pub max_distance: f32,
    /// 0..0.5: share of the screen over which reflections fade out at its edges.
    pub edge_fade: f32,
    /// Hi-Z steps per ray.
    pub quality: ScreenSpaceQuality,
    /// Trace every pixel instead of a half-resolution image.
    pub full_resolution: bool,
    /// Blend with reprojected history (steadier; resets on camera cuts).
    pub temporal: bool,
}

impl Default for ScreenSpaceReflectionsOverride {
    fn default() -> Self {
        Self {
            active: false,
            intensity: 1.0,
            max_roughness: 0.6,
            thickness: 0.3,
            max_distance: 50.0,
            edge_fade: 0.1,
            quality: ScreenSpaceQuality::Medium,
            full_resolution: true,
            temporal: true,
        }
    }
}

/// Misty GI: fully dynamic world-space global illumination (global distance field, radiance
/// cache and probe gather, `render/misty_gi`). Off unless the volume ticks it. With Screen Space
/// GI also on, on-screen hits keep SSGI's detail and Misty GI fills the rays that leave the
/// screen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MistyGlobalIlluminationOverride {
    pub active: bool,
    /// 0..2: strength of the indirect light (1 = physically based).
    pub intensity: f32,
    /// Clipmap size, probe count and rays (Low ≈ 1 ms, High ≈ 3–4 ms at 1080p).
    pub quality: ScreenSpaceQuality,
    /// Lightmapped surfaces also take Misty GI (their lightmap already holds bounce light).
    pub lightmapped_surfaces: bool,
}

impl Default for MistyGlobalIlluminationOverride {
    fn default() -> Self {
        Self {
            active: false,
            intensity: 1.0,
            quality: ScreenSpaceQuality::High,
            lightmapped_surfaces: false,
        }
    }
}
