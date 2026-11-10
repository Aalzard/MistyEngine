//! Light ▸ Intensity from Sun Height: a Directional Light dims and warms as the sun sinks,
//! from the atmosphere's transmittance along the sun's path (Kasten–Young air mass, Rayleigh
//! and haze optical depths per colour channel). The light's Intensity and Colour are its
//! values with the sun overhead. The sky's brightness follows the same curve, so the ambient
//! light, clouds and haze stay consistent with the sun.

use glam::Vec3;

/// Vertical optical depth of a clear atmosphere for red, green and blue light (Rayleigh plus
/// a light haze).
const DEPTH: Vec3 = Vec3::new(0.125, 0.2, 0.375);

/// Relative air mass towards a sun `elevation` degrees above the horizon (1 overhead, ~38 at
/// the horizon; Kasten & Young 1989).
pub fn air_mass(elevation: f32) -> f32 {
    let e = elevation.clamp(-1.0, 90.0);
    let s = e.to_radians().sin().max(0.0);
    1.0 / (s + 0.50572 * (e + 6.07995).powf(-1.6364))
}

/// Linear per-channel transmittance relative to the sun overhead (1 overhead, warmer and
/// dimmer as the sun sinks, 0 once it is 2° under the horizon).
pub fn transmittance(elevation: f32) -> Vec3 {
    let m = air_mass(elevation);
    let t = (-DEPTH * (m - 1.0)).exp();
    let set = ((elevation + 2.0) / 2.0).clamp(0.0, 1.0);
    t * set * set * (3.0 - 2.0 * set)
}

/// Brightness of the sky light relative to the sun overhead.
pub fn sky_scale(elevation: f32) -> f32 {
    let t = transmittance(elevation);
    let lum = t.dot(Vec3::new(0.2126, 0.7152, 0.0722));
    0.15 + 0.85 * lum.sqrt()
}

/// Elevation in degrees of a sun whose light travels along `direction`.
pub fn elevation(direction: Vec3) -> f32 {
    (-direction.normalize_or(Vec3::NEG_Y).y)
        .clamp(-1.0, 1.0)
        .asin()
        .to_degrees()
}

/// sRGB colour and intensity of the light after the atmosphere (`color` sRGB as set).
pub fn apply(color: [f32; 3], intensity: f32, direction: Vec3) -> ([f32; 3], f32) {
    let t = transmittance(elevation(direction));
    let peak = t.max_element();
    if peak <= 0.0 {
        return (color, 0.0);
    }
    let lin = Vec3::from(crate::render::srgb_to_linear(color)) * (t / peak);
    let c = lin.to_array().map(|v| {
        let v = v.clamp(0.0, 1.0);
        if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    });
    (c, intensity * peak)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overhead_sun_is_unchanged_and_low_sun_is_dimmer_and_warmer() {
        assert!((air_mass(90.0) - 1.0).abs() < 1e-3);
        assert!((air_mass(30.0) - 2.0).abs() < 0.05, "{}", air_mass(30.0));
        assert!(air_mass(0.0) > 30.0);
        let top = transmittance(90.0);
        assert!((top - Vec3::ONE).abs().max_element() < 1e-3);
        let mut last = top;
        for e in [70.0, 45.0, 25.0, 10.0, 3.0, 0.0] {
            let t = transmittance(e);
            assert!(
                t.x <= last.x && t.y <= last.y && t.z <= last.z,
                "{e}: {t} vs {last}"
            );
            // Blue goes first: the light warms.
            assert!(t.z <= t.y && t.y <= t.x, "{e}: {t}");
            last = t;
        }
        assert_eq!(transmittance(-3.0), Vec3::ZERO);
        let (c, i) = apply([1.0, 1.0, 1.0], 3.0, Vec3::new(0.0, -0.17, 0.98));
        assert!(i < 3.0 && i > 0.5, "{i}");
        assert!(c[0] >= c[1] && c[1] > c[2], "{c:?}");
        let (c, i) = apply([1.0, 0.9, 0.8], 2.0, Vec3::NEG_Y);
        assert!(
            (i - 2.0).abs() < 1e-3 && (c[1] - 0.9).abs() < 2e-3,
            "{c:?} {i}"
        );
        assert!(sky_scale(90.0) > 0.999 && sky_scale(10.0) < sky_scale(45.0));
        assert!(sky_scale(-5.0) > 0.1);
    }
}
