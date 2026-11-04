//! What each Quality ▸ Volumetric Clouds tier traces, and the fixed physical constants.

use crate::quality::Tier;

/// Radius of the planet the cloud layer curves around (metres).
pub(super) const PLANET_RADIUS: f32 = 6_360_000.0;
/// Clouds farther than this along the ray are not traced (metres).
pub(super) const MAX_DISTANCE: f32 = 60_000.0;
/// Distance where half of a cloud's own light has faded into the horizon haze (metres).
pub(super) const HAZE_DISTANCE: f32 = 28_000.0;
/// Extinction at full density with Density = 1 (1/m).
pub(super) const EXTINCTION: f32 = 0.045;
/// Weight of the reprojected history.
pub(super) const HISTORY_WEIGHT: f32 = 0.9;

/// Trace settings of one tier.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CloudQuality {
    /// The trace image is the target size divided by this (per axis).
    pub divisor: u32,
    /// Raymarch steps through the layer.
    pub steps: u32,
    /// Steps of each sample's march towards the sun.
    pub light_steps: u32,
    /// Side of the cloud shadow map (texels) and the ground it covers (metres).
    pub shadow_size: u32,
    pub shadow_extent: f32,
    pub shadow_steps: u32,
}

impl CloudQuality {
    /// `None` = clouds off at this tier.
    pub fn of(tier: Tier) -> Option<Self> {
        let q = |divisor, steps, light_steps, shadow_size, shadow_extent, shadow_steps| Self {
            divisor,
            steps,
            light_steps,
            shadow_size,
            shadow_extent,
            shadow_steps,
        };
        match tier {
            Tier::Off => None,
            Tier::Low => Some(q(4, 40, 4, 128, 8000.0, 8)),
            Tier::Medium => Some(q(4, 64, 5, 256, 12000.0, 12)),
            Tier::High => Some(q(2, 96, 6, 512, 16000.0, 16)),
        }
    }

    /// Trace image size for a target of `size`.
    pub fn trace_size(&self, size: (u32, u32)) -> (u32, u32) {
        (
            size.0.div_ceil(self.divisor).max(1),
            size.1.div_ceil(self.divisor).max(1),
        )
    }
}
