//! Lighting window ▸ Environment ▸ Night Sky: the stars, the Milky Way, light pollution, the
//! aurora and meteors, drawn by the skybox as the sun goes down (render/moon.rs packs them,
//! sky.wgsl draws them). Stored in the scene under `lighting.night_sky`, left out while default.
//!
//! - The stars and the Milky Way sit on the celestial sphere: they turn with the sidereal time
//!   (Date and Time: from the date, time and longitude; otherwise the Sky Time setting) and the
//!   latitude, and the Milky Way is placed from the real galactic pole and centre.
//! - Sky brightness (moonlight, twilight, light pollution) washes out the faint stars and the
//!   Milky Way; Night Sky Darkness darkens the sky colour while they stay.
//! - The aurora is a slab of glowing curtains about 100 km up, ray-marched through (folds that
//!   wave and drift, vertical rays, pulsing), green low and a second colour at the top.

use glam::{DVec3, Vec3};
use serde::{Deserialize, Serialize};

/// Night Sky settings. Defaults: mostly faint stars with a few bright ones; no Milky Way,
/// light pollution, aurora or meteors (daytime skies never change).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NightSkySettings {
    /// The night sky (dark blue sky, stars, Milky Way, aurora, meteors).
    pub enabled: bool,
    /// Darkens the night sky's colour (0 = as is, 1 = black) without dimming the stars.
    pub darkness: f32,
    // ---- Stars ----
    /// Brightness of the star field (0 = none).
    pub star_brightness: f32,
    /// How many stars (1 = default, up to 3).
    pub star_density: f32,
    /// 0 = all about as bright, 1 = a few very bright stars among many faint ones.
    pub star_magnitude_spread: f32,
    /// Glow around the bright stars (0 = points).
    pub star_glow: f32,
    /// Colour temperature spread, bluish to orange (0 = white).
    pub star_color_variation: f32,
    /// Twinkle (scintillation) strength, stronger near the horizon, and its speed.
    pub star_twinkle: f32,
    pub star_twinkle_speed: f32,
    /// How strongly a bright sky (moonlight, twilight, light pollution) washes the stars and
    /// the Milky Way out (0 = never).
    pub star_fade: f32,
    /// Local sidereal time in hours: turns the stars and the Milky Way (not used in Date and
    /// Time mode, where the date sets it).
    pub sky_time: f32,
    // ---- Milky Way ----
    /// Visibility of the Milky Way (0 = none).
    pub milky_way: f32,
    /// Brightness of its core (Sagittarius).
    pub milky_way_core: f32,
    /// Strength of the dark dust lanes.
    pub milky_way_dust: f32,
    /// sRGB tint.
    pub milky_way_tint: [f32; 3],
    /// Width multiplier.
    pub milky_way_width: f32,
    // ---- Light pollution ----
    /// Horizon glow of towns (0 = none); it also washes out stars and the Milky Way.
    pub light_pollution: f32,
    /// sRGB colour of the glow.
    pub light_pollution_color: [f32; 3],
    // ---- Aurora ----
    /// Visibility of the aurora (0 = none).
    pub aurora: f32,
    /// sRGB colour low in the curtains (the green oxygen line) and at their tops.
    pub aurora_color: [f32; 3],
    pub aurora_top_color: [f32; 3],
    /// Height of the curtains' lower edge and their thickness (km).
    pub aurora_height: f32,
    pub aurora_thickness: f32,
    /// Number of curtains (1–6).
    pub aurora_curtains: u32,
    /// Fold amplitude (0 = straight arcs, 1 = default, up to 3).
    pub aurora_waviness: f32,
    /// Vertical ray detail (0 = smooth, 1 = default, up to 3).
    pub aurora_rays: f32,
    /// Direction of the display's centre (degrees from north towards east) and how far away it
    /// is over the ground (km): far = low on the sky.
    pub aurora_azimuth: f32,
    pub aurora_distance: f32,
    /// Animation speed (1 = default) and pulsing / flicker strength.
    pub aurora_speed: f32,
    pub aurora_pulse: f32,
    /// How much the aurora tints the scene's sky light.
    pub aurora_ambient: f32,
    // ---- Meteors ----
    /// Shooting stars per minute (0 = none) and their brightness.
    pub meteor_rate: f32,
    pub meteor_brightness: f32,
}

impl Default for NightSkySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            darkness: 0.0,
            star_brightness: 1.0,
            star_density: 0.7,
            star_magnitude_spread: 0.8,
            star_glow: 0.5,
            star_color_variation: 0.6,
            star_twinkle: 0.0,
            star_twinkle_speed: 1.0,
            star_fade: 1.0,
            sky_time: 20.0,
            milky_way: 0.0,
            milky_way_core: 1.0,
            milky_way_dust: 1.0,
            milky_way_tint: [1.0, 1.0, 1.0],
            milky_way_width: 1.0,
            light_pollution: 0.0,
            light_pollution_color: [1.0, 0.62, 0.32],
            aurora: 0.0,
            aurora_color: [0.42, 1.0, 0.6],
            aurora_top_color: [0.7, 0.32, 0.55],
            aurora_height: 100.0,
            aurora_thickness: 160.0,
            aurora_curtains: 3,
            aurora_waviness: 1.0,
            aurora_rays: 1.0,
            aurora_azimuth: 0.0,
            aurora_distance: 300.0,
            aurora_speed: 1.0,
            aurora_pulse: 0.5,
            aurora_ambient: 0.5,
            meteor_rate: 0.0,
            meteor_brightness: 1.0,
        }
    }
}

impl NightSkySettings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Equatorial unit vector (x towards RA 0, z towards the north celestial pole).
fn equatorial(ra_deg: f64, dec_deg: f64) -> DVec3 {
    let (a, d) = (ra_deg.to_radians(), dec_deg.to_radians());
    DVec3::new(d.cos() * a.cos(), d.cos() * a.sin(), d.sin())
}

/// The sky's frame: the world directions of the equatorial axes (x = RA 0h on the equator,
/// y = RA 6h, z = north celestial pole) at latitude `lat` and local sidereal time `lst_hours`.
pub fn celestial_axes(lat: f32, lst_hours: f64) -> [Vec3; 3] {
    let l = (lat.clamp(-89.9, 89.9) as f64).to_radians();
    let pole = DVec3::new(0.0, l.sin(), l.cos());
    // The equator's highest point (towards the south for the north) and east.
    let q = DVec3::new(0.0, l.cos(), -l.sin());
    let e = DVec3::X;
    let t = (lst_hours * 15.0).to_radians();
    [
        (q * t.cos() - e * t.sin()).as_vec3(),
        (q * t.sin() + e * t.cos()).as_vec3(),
        pole.as_vec3(),
    ]
}

/// A celestial (equatorial) unit vector in the world for the given axes.
pub fn to_world(axes: &[Vec3; 3], eq: DVec3) -> Vec3 {
    (axes[0] * eq.x as f32 + axes[1] * eq.y as f32 + axes[2] * eq.z as f32).normalize()
}

/// The galaxy's frame in the world: towards the galactic centre (in Sagittarius), the third
/// axis, and the north galactic pole (J2000: pole RA 192.86°, Dec +27.13°; centre RA 266.41°,
/// Dec −28.94°).
pub fn galactic_axes(axes: &[Vec3; 3]) -> [Vec3; 3] {
    let centre = to_world(axes, equatorial(266.405_1, -28.936_17));
    let pole = to_world(axes, equatorial(192.859_48, 27.128_25));
    let y = pole.cross(centre).normalize();
    [centre, y, pole]
}

/// Local sidereal time (hours) for a local date and time and longitude.
pub fn sidereal_hours(
    year: i32,
    month: u32,
    day: u32,
    hours: f64,
    utc_offset: f64,
    lon: f64,
) -> f64 {
    let jd = super::ephemeris::julian_day(year, month, day, hours - utc_offset);
    ((super::ephemeris::sidereal_time(jd) + lon).rem_euclid(360.0)) / 15.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Polaris (RA 2h32m, Dec +89.26°) sits due north at the latitude's height; the axes are
    /// orthonormal; the sky turns westward with sidereal time.
    #[test]
    fn the_sky_frame_puts_polaris_north_and_turns_west() {
        let axes = celestial_axes(52.0, 3.0);
        for (i, a) in axes.iter().enumerate() {
            assert!((a.length() - 1.0).abs() < 1e-5);
            for b in &axes[i + 1..] {
                assert!(a.dot(*b).abs() < 1e-5);
            }
        }
        let polaris = to_world(&axes, equatorial(38.0, 89.26));
        let alt = polaris.y.asin().to_degrees();
        assert!((alt - 52.0).abs() < 1.0, "{alt}");
        assert!(polaris.z > 0.0, "north");
        // A star on the equator at RA = LST is on the meridian, due south.
        let on_meridian = to_world(&axes, equatorial(45.0, 0.0));
        assert!(
            on_meridian.x.abs() < 1e-4 && on_meridian.z < 0.0,
            "{on_meridian}"
        );
        // An hour later it is west of the meridian.
        let later = to_world(&celestial_axes(52.0, 4.0), equatorial(45.0, 0.0));
        assert!(later.x < -0.2, "{later}");
    }

    /// The galactic centre is low in the south from mid-northern latitudes when its RA
    /// (17h46m) crosses the meridian.
    #[test]
    fn the_galactic_centre_is_where_sagittarius_is() {
        let axes = celestial_axes(45.0, 17.76);
        let [centre, _, pole] = galactic_axes(&axes);
        let alt = centre.y.asin().to_degrees();
        assert!((alt - (90.0 - 45.0 - 28.94)).abs() < 0.5, "{alt}");
        assert!(centre.z < 0.0 && centre.x.abs() < 0.01);
        assert!(centre.dot(pole).abs() < 1e-4);
    }

    #[test]
    fn settings_default_and_round_trip() {
        let s = NightSkySettings {
            aurora: 1.0,
            meteor_rate: 6.0,
            ..Default::default()
        };
        let back: NightSkySettings =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let old: NightSkySettings = serde_json::from_str("{}").unwrap();
        assert!(old.is_default());
        // Local sidereal time at Greenwich, 1987-04-10 0h UT (Meeus 12.a): 13.1795 h.
        let lst = sidereal_hours(1987, 4, 10, 0.0, 0.0, 0.0);
        assert!((lst - 13.179_55).abs() < 1e-3, "{lst}");
    }
}
