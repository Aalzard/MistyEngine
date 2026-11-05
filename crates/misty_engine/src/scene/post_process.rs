//! Post-process Volume component: a global set of effect overrides (tonemapping, bloom,
//! colour adjustments, vignette). Each section only applies when its override is ticked;
//! otherwise the engine's baseline value is used (see `render::post`).

use serde::{Deserialize, Serialize};

use super::screen_space_overrides::{AmbientOcclusionOverride, ContactShadowsOverride, is_default};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TonemapMode {
    /// No curve: over-bright colours keep their hue and fade to white (the engine's baseline).
    #[default]
    None,
    /// Gentle filmic curve that keeps colours close to the input.
    Neutral,
    /// Filmic ACES curve: more contrast and saturated highlights.
    Aces,
    /// AgX (the curve Blender uses by default): wide latitude, bright colours desaturate
    /// smoothly towards white instead of skewing hue. Has looks (Base / Punchy).
    #[serde(rename = "AgX")]
    Agx,
    /// Khronos PBR Neutral: keeps base colours almost exact below the highlights.
    PbrNeutral,
}

impl TonemapMode {
    pub const ALL: [TonemapMode; 5] = [
        Self::None,
        Self::Neutral,
        Self::Aces,
        Self::Agx,
        Self::PbrNeutral,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Neutral => "Neutral",
            Self::Aces => "ACES",
            Self::Agx => "AgX",
            Self::PbrNeutral => "Khronos PBR Neutral",
        }
    }
}

/// AgX look: a grade applied inside the curve (like Blender's AgX looks).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TonemapLook {
    /// The plain curve: soft, low contrast.
    #[default]
    Base,
    /// More contrast and saturation.
    Punchy,
}

impl TonemapLook {
    pub const ALL: [TonemapLook; 2] = [Self::Base, Self::Punchy];

    pub fn label(self) -> &'static str {
        match self {
            Self::Base => "Base",
            Self::Punchy => "Punchy",
        }
    }

    pub fn is_base(&self) -> bool {
        *self == Self::Base
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TonemappingOverride {
    pub active: bool,
    pub mode: TonemapMode,
    /// AgX only (left out of files while Base).
    #[serde(skip_serializing_if = "TonemapLook::is_base")]
    pub look: TonemapLook,
}

impl Default for TonemappingOverride {
    fn default() -> Self {
        Self {
            active: false,
            mode: TonemapMode::Aces,
            look: TonemapLook::Base,
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
    /// White balance, -100..100: negative = cooler (bluer), positive = warmer (left out of
    /// files while 0).
    #[serde(skip_serializing_if = "is_zero")]
    pub temperature: f32,
    /// White balance, -100..100: negative = greener, positive = more magenta.
    #[serde(skip_serializing_if = "is_zero")]
    pub tint: f32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

impl Default for ColorAdjustmentsOverride {
    fn default() -> Self {
        Self {
            active: false,
            post_exposure: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            color_filter: [1.0, 1.0, 1.0],
            temperature: 0.0,
            tint: 0.0,
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
    #[serde(skip_serializing_if = "is_default")]
    pub ambient_occlusion: AmbientOcclusionOverride,
    #[serde(skip_serializing_if = "is_default")]
    pub contact_shadows: ContactShadowsOverride,
    /// Raymarched cloud layer (left out of files while untouched).
    #[serde(skip_serializing_if = "is_default")]
    pub clouds: super::CloudsOverride,
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
            ambient_occlusion: Default::default(),
            contact_shadows: Default::default(),
            clouds: Default::default(),
        }
    }
}
