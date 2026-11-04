//! Screen-space ambient occlusion settings (horizon-based, GTAO-style) resolved from the
//! Post-process Volume, plus CPU copies of the shader's occlusion math for tests.
//! The passes themselves live in `screen_space.rs` and `shaders/ao.wgsl`.

use glam::Vec3;

use crate::scene::AmbientOcclusionOverride;

/// AO of one view. `Default` = off (scenes without an active override render as before).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AoSettings {
    pub enabled: bool,
    /// Exponent on the visibility (0 = no darkening).
    pub intensity: f32,
    /// World-space search radius (metres).
    pub radius: f32,
    /// 0..1: how much AO also darkens direct light.
    pub direct_lighting_strength: f32,
    /// Horizon samples per side of each slice.
    pub step_count: u32,
    pub full_resolution: bool,
    pub temporal: bool,
    /// Scene view's Ambient Occlusion draw mode: lit surfaces show the AO as grey.
    pub debug: bool,
}

impl Default for AoSettings {
    fn default() -> Self {
        Self::from_override(&AmbientOcclusionOverride::default())
    }
}

impl AoSettings {
    /// The override's values (`enabled` follows its active checkbox).
    pub fn from_override(o: &AmbientOcclusionOverride) -> Self {
        Self {
            enabled: o.active,
            intensity: o.intensity.clamp(0.0, 4.0),
            radius: o.radius.clamp(0.05, 10.0),
            direct_lighting_strength: o.direct_lighting_strength.clamp(0.0, 1.0),
            step_count: o.step_count.clamp(2, 16),
            full_resolution: o.full_resolution,
            temporal: o.temporal,
            debug: false,
        }
    }

    /// The AO debug view: these settings switched on (with the override's values or the defaults).
    pub fn debug_view(self) -> Self {
        Self {
            enabled: true,
            debug: true,
            ..self
        }
    }

    /// Effective: on with a visible strength (or the debug view).
    pub fn active(&self) -> bool {
        self.enabled && (self.intensity > 0.0 || self.debug)
    }

    /// Directions per pixel: temporal accumulation rotates two per frame; without it four
    /// fixed ones keep the noise down.
    pub fn slices(&self) -> u32 {
        if self.temporal { 2 } else { 4 }
    }
}

/// CPU copy of `ao_multi_bounce` (screen_space.wgsl).
pub fn multi_bounce(ao: f32, albedo: Vec3) -> Vec3 {
    let a = albedo * 2.0404 - 0.3324;
    let b = albedo * -4.7951 + 0.6417;
    let c = albedo * 2.7552 + 0.6903;
    Vec3::splat(ao).max(((a * ao + b) * ao + c) * ao)
}

/// CPU copy of the cosine-weighted visible arc between horizons `h0 <= n <= h1` (angles
/// from the view vector) for a normal at angle `n`, as integrated in `cs_gtao`.
pub fn visible_arc(n: f32, h0: f32, h1: f32) -> f32 {
    let half = std::f32::consts::FRAC_PI_2;
    let (h0, h1) = (
        n + (h0 - n).clamp(-half, half),
        n + (h1 - n).clamp(-half, half),
    );
    let arc = |h: f32| (n.cos() + 2.0 * h * n.sin() - (2.0 * h - n).cos()) * 0.25;
    arc(h0) + arc(h1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn off_by_default_and_clamped() {
        let s = AoSettings::default();
        assert!(!s.enabled && !s.active());
        let o = AmbientOcclusionOverride {
            active: true,
            intensity: 9.0,
            step_count: 100,
            ..Default::default()
        };
        let s = AoSettings::from_override(&o);
        assert!(s.active());
        assert_eq!((s.intensity, s.step_count), (4.0, 16));
        assert!(AoSettings::default().debug_view().active());
    }

    #[test]
    fn open_hemisphere_is_fully_visible() {
        // Facing the camera with nothing above the tangent plane.
        assert!((visible_arc(0.0, -FRAC_PI_2, FRAC_PI_2) - 1.0).abs() < 1e-6);
        // Horizons below the tangent plane are clamped to it.
        let open = visible_arc(0.3, -3.0, 3.0);
        assert!((open - visible_arc(0.3, 0.3 - FRAC_PI_2, 0.3 + FRAC_PI_2)).abs() < 1e-6);
        // A wall at 45° on one side hides part of the sky, a crease hides more.
        let one = visible_arc(0.0, -FRAC_PI_2, FRAC_PI_2 * 0.5);
        let both = visible_arc(0.0, -FRAC_PI_2 * 0.5, FRAC_PI_2 * 0.5);
        assert!(both < one && one < 1.0, "{both} {one}");
    }

    #[test]
    fn multi_bounce_brightens_light_surfaces_only_a_little() {
        let dark = multi_bounce(0.5, Vec3::splat(0.05));
        let white = multi_bounce(0.5, Vec3::ONE);
        assert!((dark.x - 0.5).abs() < 0.03, "{dark}");
        assert!(white.x > 0.6 && white.x < 1.0, "{white}");
        assert_eq!(multi_bounce(1.0, Vec3::splat(0.5)), Vec3::ONE);
    }
}
