//! Misty GI settings of one view, resolved from the Post-process Volume override, and the
//! sizes each quality level uses.

use crate::scene::{MistyGlobalIlluminationOverride, ScreenSpaceQuality};

/// Misty GI of one view. `Default` = off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MistyGiSettings {
    pub enabled: bool,
    pub intensity: f32,
    pub quality: ScreenSpaceQuality,
    pub lightmapped_surfaces: bool,
}

impl Default for MistyGiSettings {
    fn default() -> Self {
        Self::from_override(&MistyGlobalIlluminationOverride::default())
    }
}

impl MistyGiSettings {
    pub fn from_override(o: &MistyGlobalIlluminationOverride) -> Self {
        Self {
            enabled: o.active,
            intensity: o.intensity.clamp(0.0, 2.0),
            quality: o.quality,
            lightmapped_surfaces: o.lightmapped_surfaces,
        }
    }

    pub fn active(&self) -> bool {
        self.enabled && self.intensity > 0.0
    }

    /// Clipmap, probe and ray budget of the quality level. `MISTY_GI_SIZES=levels,dim,voxel,
    /// probes,rays,stride,apply,visibility` overrides it (tuning).
    pub fn sizes(&self) -> Sizes {
        if let Some(s) = std::env::var("MISTY_GI_SIZES").ok().and_then(|v| Sizes::parse(&v)) {
            return s;
        }
        match self.quality {
            ScreenSpaceQuality::Low => Sizes {
                levels: 4,
                dim: 48,
                voxel: 0.125,
                probes: 16,
                rays: 16,
                stride: 4,
                apply_scale: 4,
                visibility_steps: 3,
            },
            ScreenSpaceQuality::Medium => Sizes {
                levels: 4,
                dim: 64,
                voxel: 0.09375,
                probes: 20,
                rays: 32,
                stride: 4,
                apply_scale: 2,
                visibility_steps: 4,
            },
            ScreenSpaceQuality::High => Sizes {
                levels: 5,
                dim: 64,
                voxel: 0.0625,
                probes: 24,
                rays: 32,
                stride: 4,
                apply_scale: 2,
                visibility_steps: 6,
            },
        }
    }
}

/// What a quality level allocates and traces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sizes {
    /// Clipmap levels (each twice the voxel size of the last).
    pub levels: u32,
    /// Voxels per side of one level.
    pub dim: u32,
    /// Voxel size of level 0 (metres), stored as an exact binary fraction.
    pub voxel: f32,
    /// Probes per side of one level.
    pub probes: u32,
    /// Rays per probe update.
    pub rays: u32,
    /// One probe in `stride` is updated per frame.
    pub stride: u32,
    /// Full pixels per gather texel.
    pub apply_scale: u32,
    /// SDF steps of the probe visibility test per pixel.
    pub visibility_steps: u32,
}

impl Sizes {
    fn parse(v: &str) -> Option<Self> {
        let n: Vec<f32> = v.split(',').map(|x| x.trim().parse().ok()).collect::<Option<_>>()?;
        let [levels, dim, voxel, probes, rays, stride, apply, vis] = n[..] else {
            return None;
        };
        Some(Self {
            levels: (levels as u32).clamp(1, 6),
            dim: (dim as u32).clamp(8, 128) / 4 * 4,
            voxel,
            probes: (probes as u32).clamp(2, 64) / 2 * 2,
            rays: (rays as u32).clamp(1, 32),
            stride: (stride as u32).clamp(1, 8),
            apply_scale: (apply as u32).clamp(1, 8),
            visibility_steps: vis as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_by_default_and_clamped() {
        assert!(!MistyGiSettings::default().active());
        let s = MistyGiSettings::from_override(&MistyGlobalIlluminationOverride {
            active: true,
            intensity: 7.0,
            ..Default::default()
        });
        assert!(s.active());
        assert_eq!(s.intensity, 2.0);
        let high = s.sizes();
        // Level 0 fits a room, the last level an outdoor view.
        assert_eq!(high.dim as f32 * high.voxel, 4.0);
        assert_eq!(
            high.dim as f32 * high.voxel * (1 << (high.levels - 1)) as f32,
            64.0
        );
    }
}
