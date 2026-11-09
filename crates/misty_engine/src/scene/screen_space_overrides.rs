//! Post-process Volume overrides for the screen-space lighting effects: Ambient Occlusion
//! (horizon-based, applied to ambient light) and Contact Shadows (short screen-space rays
//! towards each light that has them enabled). Both are off unless the volume ticks them.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AmbientOcclusionOverride {
    pub active: bool,
    /// 0..4: strength of the darkening (0 = none).
    pub intensity: f32,
    /// World-space search radius in metres: how far away an occluder still darkens.
    pub radius: f32,
    /// 0..1: how much AO also darkens direct light (0 = ambient only).
    pub direct_lighting_strength: f32,
    /// Quality: horizon samples per direction (2..16).
    pub step_count: u32,
    /// Trace every pixel instead of a half-resolution image.
    pub full_resolution: bool,
    /// Accumulate over frames (less noise; resets on camera cuts).
    pub temporal: bool,
}

impl Default for AmbientOcclusionOverride {
    fn default() -> Self {
        Self {
            active: false,
            intensity: 1.0,
            radius: 1.5,
            direct_lighting_strength: 0.0,
            step_count: 6,
            full_resolution: false,
            temporal: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContactShadowsOverride {
    pub active: bool,
    /// Ray length towards the light, in metres.
    pub length: f32,
    /// Camera distance (metres) where contact shadows have faded out.
    pub distance_fade: f32,
    /// 0..1: darkness of a contact shadow.
    pub opacity: f32,
    /// Steps along the ray (4..64).
    pub sample_count: u32,
    /// 0..1: distant shadows: past the shadow maps' reach, sunlight is ray-marched through
    /// the screen's depth over tens of metres, so far forests keep the shade between and
    /// under their crowns and distant slopes their shadows (0 = off). Left out of the file
    /// at 0.
    #[serde(skip_serializing_if = "is_zero")]
    pub far_shadows: f32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

impl Default for ContactShadowsOverride {
    fn default() -> Self {
        Self {
            active: false,
            length: 0.15,
            distance_fade: 50.0,
            opacity: 1.0,
            sample_count: 12,
            far_shadows: 0.0,
        }
    }
}

/// `skip_serializing_if` helper: untouched overrides stay out of scene files, so scenes
/// saved before these existed keep their exact text.
pub(crate) fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}
