//! Misty GI settings of one view, resolved from the Post-process Volume override, and the
//! sizes each quality level uses.

use crate::quality::Tier;
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

    /// Lighting window ▸ Realtime Global Illumination: on at full intensity, at the quality
    /// the Quality level allows.
    pub fn scene_default() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }

    /// Capped by Quality ▸ Global Illumination: Off turns it off, otherwise the quality is at
    /// most the tier's.
    pub fn capped(self, tier: Tier) -> Self {
        let cap = match tier {
            Tier::Off => {
                return Self {
                    enabled: false,
                    ..self
                };
            }
            Tier::Low => ScreenSpaceQuality::Low,
            Tier::Medium => ScreenSpaceQuality::Medium,
            Tier::High => ScreenSpaceQuality::High,
        };
        let rank = |q: ScreenSpaceQuality| q as u8;
        Self {
            quality: if rank(self.quality) > rank(cap) {
                cap
            } else {
                self.quality
            },
            ..self
        }
    }

    /// Clipmap, probe and ray budget of the quality level. `MISTY_GI_SIZES=levels,dim,voxel,
    /// probes,rays,stride,apply,visibility[,reflections]` overrides it (tuning).
    pub fn sizes(&self) -> Sizes {
        if let Some(s) = std::env::var("MISTY_GI_SIZES")
            .ok()
            .and_then(|v| Sizes::parse(&v))
        {
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
                reflections: false,
            },
            ScreenSpaceQuality::Medium => Sizes {
                levels: 4,
                dim: 64,
                voxel: 0.09375,
                probes: 16,
                rays: 24,
                stride: 2,
                apply_scale: 2,
                visibility_steps: 3,
                reflections: true,
            },
            ScreenSpaceQuality::High => Sizes {
                levels: 5,
                dim: 64,
                voxel: 0.0625,
                probes: 24,
                rays: 32,
                stride: 2,
                apply_scale: 2,
                visibility_steps: 4,
                reflections: true,
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
    /// Glossy reflections traced at the gather texels.
    pub reflections: bool,
}

impl Sizes {
    fn parse(v: &str) -> Option<Self> {
        let mut n: Vec<f32> = v
            .split(',')
            .map(|x| x.trim().parse().ok())
            .collect::<Option<_>>()?;
        if n.len() == 8 {
            n.push(1.0);
        }
        let [
            levels,
            dim,
            voxel,
            probes,
            rays,
            stride,
            apply,
            vis,
            reflections,
        ] = n[..]
        else {
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
            reflections: reflections > 0.5,
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
        assert!(!s.capped(Tier::Off).active());
        assert_eq!(s.capped(Tier::Low).quality, ScreenSpaceQuality::Low);
        assert_eq!(s.capped(Tier::High).quality, ScreenSpaceQuality::High);
        let low = MistyGiSettings {
            quality: ScreenSpaceQuality::Low,
            ..s
        };
        assert_eq!(low.capped(Tier::High).quality, ScreenSpaceQuality::Low);
        assert!(MistyGiSettings::scene_default().active());
        let high = s.sizes();
        // Level 0 fits a room, the last level an outdoor view.
        assert_eq!(high.dim as f32 * high.voxel, 4.0);
        assert_eq!(
            high.dim as f32 * high.voxel * (1 << (high.levels - 1)) as f32,
            64.0
        );
    }
}
