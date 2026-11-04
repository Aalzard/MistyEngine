//! How foggy the air is at a light (CPU): halos widen and brighten with it.
//! Reads the same inputs as the fog passes: the volume's Fog override (height fog), the local
//! Fog Volumes (volumetric mode only, like the froxels) and the Lighting window's fog.

use glam::Vec3;

use crate::render::volumetrics::FogMode;
use crate::render::{FogSettings, LocalFog};
use crate::scene::render_settings::{Fog, FogMode as LightingFogMode};

/// Metres of fog around the light that scatter its glow towards the eye.
const GLOW_REACH: f32 = 10.0;

/// Extinction (1/m) at world point `p`.
pub fn extinction_at(fog: &FogSettings, volumes: &[LocalFog], lighting: &Fog, p: Vec3) -> f32 {
    let mode = fog.mode();
    let mut sigma = 0.0;
    if mode != FogMode::Off {
        let above = (p.y - fog.volume.base_height).max(0.0);
        sigma += fog.extinction() * (-above * fog.inv_falloff()).exp();
    }
    if mode == FogMode::Volumetric {
        sigma += volumes
            .iter()
            .map(|v| v.extinction * v.weight(p))
            .sum::<f32>();
    }
    if lighting.enabled {
        sigma += match lighting.mode {
            // Linear fog: about as thick as a medium that hides things by `end`.
            LightingFogMode::Linear => 3.0 / (lighting.end - lighting.start).max(1.0),
            _ => lighting.density.max(0.0),
        };
    }
    sigma
}

/// 0 (clear air) .. 1 (thick fog): share of the glow reach that scatters, × `response`.
pub fn fogginess(sigma: f32, response: f32) -> f32 {
    1.0 - (-sigma * GLOW_REACH * response.max(0.0)).exp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{FogOverride, FogVolume};
    use glam::Mat4;

    fn fog(volumetric: bool) -> FogSettings {
        FogSettings::from_override(&FogOverride {
            active: true,
            attenuation_distance: 20.0,
            base_height: 2.0,
            maximum_height: 12.0,
            volumetric,
            ..Default::default()
        })
    }

    #[test]
    fn height_fog_thins_above_the_base() {
        let f = fog(false);
        let off = Fog::default();
        let low = extinction_at(&f, &[], &off, Vec3::new(0.0, 1.0, 0.0));
        let high = extinction_at(&f, &[], &off, Vec3::new(0.0, 12.0, 0.0));
        assert!((low - 0.05).abs() < 1e-5, "uniform below the base: {low}");
        assert!(high < low * 0.2, "{high} vs {low}");
        let none = extinction_at(&FogSettings::default(), &[], &off, Vec3::ZERO);
        assert_eq!(none, 0.0);
    }

    #[test]
    fn fog_volumes_count_only_in_volumetric_mode() {
        let v = LocalFog::new(
            &FogVolume {
                fog_distance: 2.0,
                ..Default::default()
            },
            Mat4::IDENTITY,
        );
        let off = Fog::default();
        let p = Vec3::new(0.0, 30.0, 0.0);
        let analytic = extinction_at(&fog(false), &[v], &off, Vec3::ZERO);
        let volumetric = extinction_at(&fog(true), &[v], &off, Vec3::ZERO);
        assert!(volumetric > analytic + 0.4, "{volumetric} vs {analytic}");
        let outside = extinction_at(&fog(true), &[v], &off, p);
        assert!(outside < 0.01);
    }

    #[test]
    fn fogginess_is_subtle_in_clear_air() {
        assert!(fogginess(1.0 / 400.0, 1.0) < 0.05);
        assert!(fogginess(1.0 / 10.0, 1.0) > 0.6);
        assert_eq!(fogginess(1.0, 0.0), 0.0);
    }
}
