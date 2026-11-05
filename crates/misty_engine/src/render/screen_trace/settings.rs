//! SSGI and SSR settings of one view, resolved from the Post-process Volume overrides.

use crate::scene::{
    ScreenSpaceGlobalIlluminationOverride, ScreenSpaceQuality, ScreenSpaceReflectionsOverride,
};

/// Screen Space Global Illumination of one view. `Default` = off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SsgiSettings {
    pub enabled: bool,
    pub intensity: f32,
    pub radius: f32,
    pub thickness: f32,
    pub quality: ScreenSpaceQuality,
    pub full_resolution: bool,
    pub temporal: bool,
    pub denoise: bool,
    pub lightmapped_surfaces: bool,
}

impl Default for SsgiSettings {
    fn default() -> Self {
        Self::from_override(&ScreenSpaceGlobalIlluminationOverride::default())
    }
}

impl SsgiSettings {
    pub fn from_override(o: &ScreenSpaceGlobalIlluminationOverride) -> Self {
        Self {
            enabled: o.active,
            intensity: o.intensity.clamp(0.0, 4.0),
            radius: o.radius.clamp(0.1, 20.0),
            thickness: o.thickness.clamp(0.01, 5.0),
            quality: o.quality,
            full_resolution: o.full_resolution,
            temporal: o.temporal,
            denoise: o.denoise,
            lightmapped_surfaces: o.lightmapped_surfaces,
        }
    }

    pub fn active(&self) -> bool {
        self.enabled && self.intensity > 0.0
    }

    /// Rays per pixel per frame.
    pub fn rays(&self) -> u32 {
        [1, 2, 4][self.quality.index()]
    }

    /// March steps per ray.
    pub fn steps(&self) -> u32 {
        [8, 12, 16][self.quality.index()]
    }

    /// Frames of history at most (the blend weight of a new frame never drops below 1/n).
    pub fn max_history(&self) -> f32 {
        if self.temporal { 24.0 } else { 1.0 }
    }
}

/// Screen Space Reflections of one view. `Default` = off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SsrSettings {
    pub enabled: bool,
    pub intensity: f32,
    pub max_roughness: f32,
    pub thickness: f32,
    pub max_distance: f32,
    pub edge_fade: f32,
    pub quality: ScreenSpaceQuality,
    pub full_resolution: bool,
    pub temporal: bool,
}

impl Default for SsrSettings {
    fn default() -> Self {
        Self::from_override(&ScreenSpaceReflectionsOverride::default())
    }
}

impl SsrSettings {
    pub fn from_override(o: &ScreenSpaceReflectionsOverride) -> Self {
        Self {
            enabled: o.active,
            intensity: o.intensity.clamp(0.0, 1.0),
            max_roughness: o.max_roughness.clamp(0.0, 1.0),
            thickness: o.thickness.clamp(0.01, 5.0),
            max_distance: o.max_distance.clamp(0.5, 1000.0),
            edge_fade: o.edge_fade.clamp(0.0, 0.5),
            quality: o.quality,
            full_resolution: o.full_resolution,
            temporal: o.temporal,
        }
    }

    pub fn active(&self) -> bool {
        self.enabled && self.intensity > 0.0 && self.max_roughness > 0.0
    }

    /// Hi-Z traversal iterations per ray.
    pub fn steps(&self) -> u32 {
        [40, 64, 96][self.quality.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_by_default_and_clamped() {
        assert!(!SsgiSettings::default().active());
        assert!(!SsrSettings::default().active());
        let gi = SsgiSettings::from_override(&ScreenSpaceGlobalIlluminationOverride {
            active: true,
            intensity: 9.0,
            radius: 0.0,
            quality: ScreenSpaceQuality::High,
            ..Default::default()
        });
        assert!(gi.active());
        assert_eq!(
            (gi.intensity, gi.radius, gi.rays(), gi.steps()),
            (4.0, 0.1, 4, 16)
        );
        let ssr = SsrSettings::from_override(&ScreenSpaceReflectionsOverride {
            active: true,
            max_roughness: 0.0,
            ..Default::default()
        });
        assert!(!ssr.active(), "nothing is smooth enough");
    }
}
