//! Post-process Volume component: a global set of effect overrides (tonemapping, bloom,
//! colour adjustments, vignette). Each section only applies when its override is ticked;
//! otherwise the engine's baseline value is used (see `render::post`).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TonemapMode {
    /// No curve: over-bright colours keep their hue and fade to white (the engine's baseline).
    #[default]
    None,
    /// Gentle filmic curve that keeps colours close to the input.
    Neutral,
    /// Filmic ACES curve: more contrast and saturated highlights.
    Aces,
}

impl TonemapMode {
    pub const ALL: [TonemapMode; 3] = [Self::None, Self::Neutral, Self::Aces];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Neutral => "Neutral",
            Self::Aces => "ACES",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TonemappingOverride {
    pub active: bool,
    pub mode: TonemapMode,
}

impl Default for TonemappingOverride {
    fn default() -> Self {
        Self {
            active: false,
            mode: TonemapMode::Aces,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BloomOverride {
    pub active: bool,
    pub intensity: f32,
    /// Emission brightness below which nothing glows.
    pub threshold: f32,
    /// 0..1: how far the glow spreads.
    pub scatter: f32,
    /// sRGB.
    pub tint: [f32; 3],
}

impl Default for BloomOverride {
    fn default() -> Self {
        Self {
            active: false,
            intensity: 0.6,
            threshold: 0.0,
            scatter: 0.7,
            tint: [1.0, 1.0, 1.0],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorAdjustmentsOverride {
    pub active: bool,
    /// Exposure in EV (stops).
    pub post_exposure: f32,
    /// -100..100.
    pub contrast: f32,
    /// -100..100.
    pub saturation: f32,
    /// sRGB multiplier.
    pub color_filter: [f32; 3],
}

impl Default for ColorAdjustmentsOverride {
    fn default() -> Self {
        Self {
            active: false,
            post_exposure: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            color_filter: [1.0, 1.0, 1.0],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VignetteOverride {
    pub active: bool,
    /// 0..1.
    pub intensity: f32,
    /// 0.01..1.
    pub smoothness: f32,
    /// sRGB.
    pub color: [f32; 3],
}

impl Default for VignetteOverride {
    fn default() -> Self {
        Self {
            active: false,
            intensity: 0.25,
            smoothness: 0.2,
            color: [0.0, 0.0, 0.0],
        }
    }
}

/// Global post-processing volume. The enabled volume with the highest priority wins.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PostProcessVolume {
    pub enabled: bool,
    pub priority: f32,
    pub tonemapping: TonemappingOverride,
    pub bloom: BloomOverride,
    pub color_adjustments: ColorAdjustmentsOverride,
    pub vignette: VignetteOverride,
    /// Volumetric / height fog (left out of files while untouched).
    #[serde(skip_serializing_if = "super::fog_volume::defaults::is_default")]
    pub fog: super::FogOverride,
    #[serde(skip_serializing_if = "super::fog_volume::defaults::is_default")]
    pub sun_shafts: super::SunShaftsOverride,
}

impl Default for PostProcessVolume {
    fn default() -> Self {
        Self {
            enabled: true,
            priority: 0.0,
            tonemapping: TonemappingOverride::default(),
            bloom: BloomOverride::default(),
            color_adjustments: ColorAdjustmentsOverride::default(),
            vignette: VignetteOverride::default(),
            fog: Default::default(),
            sun_shafts: Default::default(),
        }
    }
}
