//! Volumetric fog scene data: the Fog Volume component (a local box of fog, GameObject ▸
//! Rendering ▸ Local Volumetric Fog), the Post-process Volume's Fog and Sun Shafts overrides,
//! and the per-light volumetric values. The renderer side lives in `render::volumetrics`.

use serde::{Deserialize, Serialize};

use super::SceneComponent;

/// `skip_serializing_if` helpers: fields added after scenes were saved stay out of files
/// while they hold their defaults, so old scenes round-trip unchanged.
pub mod defaults {
    pub fn is_one(v: &f32) -> bool {
        *v == 1.0
    }
    pub fn is_default<T: Default + PartialEq>(v: &T) -> bool {
        *v == T::default()
    }
}

/// A box of fog centred on its object (rotated and scaled with it). Only seen when the
/// active volume's Fog override has Volumetric on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FogVolume {
    pub enabled: bool,
    /// Box size in local units (× the object's scale).
    pub size: [f32; 3],
    /// Mean free path in metres: smaller = thicker fog.
    pub fog_distance: f32,
    /// sRGB colour of the light the fog scatters.
    pub albedo: [f32; 3],
    /// Width of the soft edge inside each face, in metres (0 = hard box).
    pub blend_distance: f32,
}

impl Default for FogVolume {
    fn default() -> Self {
        Self {
            enabled: true,
            size: [10.0, 4.0, 10.0],
            fog_distance: 8.0,
            albedo: [1.0, 1.0, 1.0],
            blend_distance: 1.5,
        }
    }
}

impl SceneComponent for FogVolume {
    const NAME: &'static str = "Fog Volume";
    const CATEGORY: &'static str = "Rendering";
}

/// Grid resolution of the froxel volume.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FogQuality {
    Low,
    #[default]
    Medium,
    High,
}

impl FogQuality {
    pub const ALL: [FogQuality; 3] = [Self::Low, Self::Medium, Self::High];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }
}

/// How the main sun lights the volumetric fog.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SunShaftsQuality {
    /// The sun is one of the froxel lights (soft, low-resolution shafts).
    Off,
    /// The sun is raymarched per pixel through its shadow map: crisp light shafts.
    #[default]
    Sharp,
}

impl SunShaftsQuality {
    pub const ALL: [Self; 2] = [Self::Off, Self::Sharp];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off (froxels)",
            Self::Sharp => "Sharp",
        }
    }
}

/// Post-process Volume ▸ Fog: global height fog, optionally volumetric (lit froxels with
/// shadows) with local Fog Volumes and noise.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FogOverride {
    pub active: bool,
    pub enabled: bool,
    /// Distance the sky and anything farther are fogged at (metres).
    pub max_fog_distance: f32,
    /// Mean free path at the base height (metres): smaller = thicker fog.
    pub attenuation_distance: f32,
    /// Fog is uniform below this height and thins out above it.
    pub base_height: f32,
    /// Height where the fog has thinned to about an eighth.
    pub maximum_height: f32,
    /// sRGB tint of the scattered light.
    pub albedo: [f32; 3],
    /// Henyey-Greenstein g: 0 scatters evenly, towards 1 glows around lights.
    pub anisotropy: f32,
    /// Scales the sky ambient the fog picks up.
    pub ambient_dimmer: f32,
    /// 0..1: aerial perspective: the height fog beyond the volumetric part takes the sky's own
    /// colour just above the horizon in each direction instead of the flat ambient, so far
    /// hills fade into the horizon sky (pale blue, brighter towards the sun). Left out of the
    /// file at 0.
    #[serde(skip_serializing_if = "defaults::is_default")]
    pub sky_haze: f32,
    /// Lit froxel volume with shadows; off = cheap analytic height fog.
    pub volumetric: bool,
    /// How far the froxel volume reaches (metres); analytic fog continues past it.
    pub volumetric_distance: f32,
    pub quality: FogQuality,
    /// Temporal reprojection with per-frame jitter (removes banding and flicker).
    pub denoising: bool,
    /// 0 = smooth fog, 1 = fully broken up by noise.
    pub noise_intensity: f32,
    /// Size of the noise features (metres).
    pub noise_scale: f32,
    /// Noise drift in metres per second (wind).
    pub noise_scroll: [f32; 3],
    /// Volumetric only: march the main sun per pixel for sharp shafts (the Quality settings
    /// can still turn it off).
    #[serde(skip_serializing_if = "defaults::is_default")]
    pub sun_shafts_quality: SunShaftsQuality,
}

impl Default for FogOverride {
    fn default() -> Self {
        Self {
            active: false,
            enabled: true,
            max_fog_distance: 400.0,
            attenuation_distance: 150.0,
            base_height: 0.0,
            maximum_height: 50.0,
            albedo: [1.0, 1.0, 1.0],
            anisotropy: 0.4,
            ambient_dimmer: 1.0,
            sky_haze: 0.0,
            volumetric: true,
            volumetric_distance: 64.0,
            quality: FogQuality::Medium,
            denoising: true,
            noise_intensity: 0.0,
            noise_scale: 8.0,
            noise_scroll: [0.6, 0.0, 0.3],
            sun_shafts_quality: SunShaftsQuality::Sharp,
        }
    }
}

/// Post-process Volume ▸ Sun Shafts (Screen): a cheap screen-space radial blur of the bright sky
/// around the sun.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SunShaftsOverride {
    pub active: bool,
    pub intensity: f32,
    /// Sky brightness below which nothing streaks.
    pub threshold: f32,
    /// 0..1: streak length as a fraction of the way to the sun.
    pub length: f32,
    /// Screen-space radius around the sun that feeds the shafts (fraction of the height).
    pub radius: f32,
    /// sRGB.
    pub tint: [f32; 3],
}

impl Default for SunShaftsOverride {
    fn default() -> Self {
        Self {
            active: false,
            intensity: 1.0,
            threshold: 0.6,
            length: 0.8,
            radius: 0.5,
            tint: [1.0, 0.92, 0.8],
        }
    }
}
