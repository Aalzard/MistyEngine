//! The moon (Lighting window ▸ Environment ▸ Moon): where it is, how much of it is lit, and
//! the moonlight that replaces the sun at night.
//!
//! - **Where**: Follow Sun (default) puts it on the sun's daily path, `phase` × 360° east of
//!   it (a full moon rises as the sun sets, a first quarter stands in the south at sunset);
//!   Manual takes an azimuth and altitude; Date and Time computes the real sun and moon for a
//!   date, time, latitude and longitude ([`ephemeris`]).
//! - **Phase**: the moon is a sphere lit by the sun's direction, so one texture shows every
//!   phase with a correct terminator. Follow Sun and Date and Time light it from the real
//!   sun; Manual keeps the bright limb towards the sun and turns the light so the lit
//!   fraction matches the Phase slider.
//! - **Moonlight** (render/moon.rs): while the sun is below the horizon the main Directional
//!   Light (its shadows included) turns to the moon: cool, dim, scaled by the phase (Allen's
//!   phase law: a quarter moon is about a tenth of a full one) and by the air mass towards the
//!   moon (dimmer and warmer low down, the same air-mass transmittance as a low sun).
//!   Twilight crosses over smoothly: the sun fades out just below the horizon, then the
//!   moon fades in, so the light changes direction while it is dark.
//!
//! The texture is the built-in one ([`texture`], made by [`texture_gen`]) or a project image.

pub mod ephemeris;
pub mod texture;
pub mod texture_gen;

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// How the moon's position (and phase) is chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoonMode {
    /// On the sun's daily path at the Phase's angle east of the sun.
    #[default]
    FollowSun,
    /// Azimuth / altitude and the Phase slider.
    Manual,
    /// Real sun and moon for a date, time and place.
    DateTime,
}

impl MoonMode {
    pub const ALL: [MoonMode; 3] = [Self::FollowSun, Self::Manual, Self::DateTime];

    pub fn label(self) -> &'static str {
        match self {
            Self::FollowSun => "Follow Sun",
            Self::Manual => "Manual",
            Self::DateTime => "Date and Time",
        }
    }

    /// Lua / file name.
    pub fn name(self) -> &'static str {
        match self {
            Self::FollowSun => "FollowSun",
            Self::Manual => "Manual",
            Self::DateTime => "DateTime",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|m| m.name().eq_ignore_ascii_case(s))
    }
}

/// Lighting ▸ Environment ▸ Moon. Left out of scene files while default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MoonSettings {
    pub enabled: bool,
    pub mode: MoonMode,
    /// 0 = new, 0.25 = first quarter, 0.5 = full, 0.75 = last quarter (Follow Sun, Manual).
    pub phase: f32,
    /// Manual: degrees from north (+Z) towards east (+X), and above the horizon.
    pub azimuth: f32,
    pub altitude: f32,
    /// Date and Time: local calendar date and time (hours), hours east of UTC.
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub time: f32,
    pub utc_offset: f32,
    /// Degrees north (also tilts the sky's pole for Follow Sun) and east.
    pub latitude: f32,
    pub longitude: f32,
    /// × the real angular size (0.52° across).
    pub size: f32,
    /// Disc brightness multiplier.
    pub brightness: f32,
    /// Sunlight reflected by the Earth on the moon's dark side (0 = none).
    pub earthshine: f32,
    /// Moonlight intensity for a full moon high in the sky.
    pub light_intensity: f32,
    /// sRGB tint of the moonlight.
    pub light_color: [f32; 3],
    /// Star field brightness at night (0 = none).
    pub stars: f32,
    /// Project image ("Assets/Textures/Moon.png"): 2:1 = equirectangular (near side at the
    /// centre), square = a photo of the near side filling the image. None = built-in.
    pub texture: Option<String>,
}

impl Default for MoonSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: MoonMode::FollowSun,
            phase: 0.5,
            azimuth: 135.0,
            altitude: 30.0,
            year: 2026,
            month: 11,
            day: 24,
            time: 22.0,
            utc_offset: 0.0,
            latitude: 45.0,
            longitude: 0.0,
            size: 3.0,
            brightness: 1.0,
            earthshine: 1.0,
            light_intensity: 0.35,
            light_color: [0.62, 0.72, 1.0],
            stars: 1.0,
            texture: None,
        }
    }
}

impl MoonSettings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Mean angular radius of the moon (radians).
pub const ANGULAR_RADIUS: f32 = 0.002_53;

/// The moon this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoonState {
    /// Unit vector towards the moon.
    pub direction: Vec3,
    /// The disc's frame as seen from the ground: lunar north and east (right).
    pub up: Vec3,
    pub right: Vec3,
    /// Unit vector towards the sun as it lights the moon (the phase).
    pub light_dir: Vec3,
    /// Lit fraction of the disc (0 new .. 1 full).
    pub illumination: f32,
    /// Sun–moon–observer angle (radians): 0 full, π new.
    pub phase_angle: f32,
    /// Angular radius (radians).
    pub radius: f32,
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Lit fraction of a sphere at `moon` lit from `light` (both unit vectors from the observer):
/// (1 − cos ψ) / 2 for the elongation ψ.
pub fn lit_fraction(moon: Vec3, light: Vec3) -> f32 {
    (1.0 - moon.dot(light).clamp(-1.0, 1.0)) * 0.5
}

/// Brightness of the whole moon relative to full at phase angle `i` (radians): Allen's phase
/// law, 10^(−0.4 (0.026 |i°| + 4·10⁻⁹ i°⁴)). Quarter ≈ 0.09, crescent ≈ 0.01, new ≈ 0.
pub fn phase_brightness(i: f32) -> f32 {
    let a = i.abs().to_degrees().min(180.0);
    10f32.powf(-0.4 * (0.026 * a + 4.0e-9 * a.powi(4)))
}

/// Kasten–Young air mass for an altitude whose sine is `sin_alt` (1 overhead, ~38 at the
/// horizon; capped just below it).
pub fn air_mass(sin_alt: f32) -> f32 {
    let h = sin_alt.clamp(-1.0, 1.0).asin().to_degrees().max(-0.5);
    1.0 / (h.to_radians().sin().max(0.0) + 0.505_72 * (h + 6.079_95).powf(-1.6364))
}

/// Linear RGB transmittance of the air towards a body at `sin_alt`, relative to overhead (1
/// high up; dimmer and redder low down). `thickness` = the sky's Atmosphere Thickness.
pub fn transmittance(sin_alt: f32, thickness: f32) -> Vec3 {
    let tau = Vec3::new(0.10, 0.16, 0.30) * thickness.clamp(0.0, 5.0);
    let x = air_mass(sin_alt) - 1.0;
    Vec3::new((-tau.x * x).exp(), (-tau.y * x).exp(), (-tau.z * x).exp())
}

/// How much of the sun's light is left as it sets (1 above the horizon, 0 about 3.4° below).
pub fn sun_fade(sun_y: f32) -> f32 {
    smoothstep(-0.06, 0.0, sun_y)
}

/// How much moonlight there is (0 until the sun is 3.4° down, 1 from about 11.5° down).
pub fn moon_fade(sun_y: f32) -> f32 {
    1.0 - smoothstep(-0.2, -0.06, sun_y)
}

/// The night sky's share (0 until the sun is 1° down, 1 from about 11.5° down).
pub fn night(sun_y: f32) -> f32 {
    1.0 - smoothstep(-0.2, -0.02, sun_y)
}

/// Sky pole (celestial north) at latitude `lat` degrees.
fn pole(lat: f32) -> Vec3 {
    let l = lat.clamp(-89.9, 89.9).to_radians();
    Vec3::new(0.0, l.sin(), l.cos())
}

/// Hour angle (radians, westward) and declination of a world direction at latitude `lat`.
fn to_equatorial(d: Vec3, lat: f32) -> (f32, f32) {
    let l = lat.clamp(-89.9, 89.9).to_radians();
    let p = pole(lat);
    let q = Vec3::new(0.0, l.cos(), -l.sin());
    let dec = d.dot(p).clamp(-1.0, 1.0).asin();
    (-(d.x).atan2(d.dot(q)), dec)
}

fn from_equatorial(h: f32, dec: f32, lat: f32) -> Vec3 {
    let l = lat.clamp(-89.9, 89.9).to_radians();
    let q = Vec3::new(0.0, l.cos(), -l.sin());
    (q * dec.cos() * h.cos() - Vec3::X * dec.cos() * h.sin() + pole(lat) * dec.sin())
        .normalize_or(Vec3::Y)
}

/// Follow Sun: the moon `phase` × 360° east of the sun along its daily circle, 5° north of
/// it at new moon (the moon's orbit is tilted, so a new moon does not eclipse the sun).
pub fn follow_sun(sun: Vec3, phase: f32, lat: f32) -> Vec3 {
    let (h, dec) = to_equatorial(sun.normalize_or(Vec3::Y), lat);
    let psi = phase.rem_euclid(1.0) * std::f32::consts::TAU;
    let dec_m = (dec + 5f32.to_radians() * psi.cos()).clamp(-1.55, 1.55);
    from_equatorial(h - psi, dec_m, lat)
}

/// Unit vector for an azimuth (from north towards east) and altitude in degrees.
pub fn from_az_alt(azimuth: f32, altitude: f32) -> Vec3 {
    ephemeris::direction(altitude as f64, azimuth as f64)
}

/// A light direction that lights `moon` with lit fraction of `phase` (0..1 cycle, 0.5 = full),
/// its bright limb turned towards `sun`.
fn phase_light(moon: Vec3, sun: Option<Vec3>, phase: f32) -> Vec3 {
    let psi = phase.rem_euclid(1.0) * std::f32::consts::TAU;
    let towards = sun.unwrap_or(Vec3::Y) - moon * moon.dot(sun.unwrap_or(Vec3::Y));
    let t = towards
        .try_normalize()
        .or_else(|| (Vec3::Y - moon * moon.y).try_normalize())
        .unwrap_or(Vec3::X);
    (moon * psi.cos() + t * psi.sin().abs()).normalize_or(moon)
}

/// Where the moon is and how it is lit, or None when it is off (or Follow Sun without a sun).
/// `sun` = unit vector towards the scene's sun (the main Directional Light).
pub fn state(s: &MoonSettings, sun: Option<Vec3>) -> Option<MoonState> {
    if !s.enabled {
        return None;
    }
    let (direction, light_dir) = match s.mode {
        MoonMode::FollowSun => {
            let sun = sun?;
            (
                follow_sun(sun, s.phase, s.latitude),
                sun.normalize_or(Vec3::Y),
            )
        }
        MoonMode::Manual => {
            let m = from_az_alt(s.azimuth, s.altitude);
            (m, phase_light(m, sun, s.phase))
        }
        MoonMode::DateTime => {
            let sky = ephemeris::sky(
                s.year,
                s.month.clamp(1, 12),
                s.day.clamp(1, 31),
                s.time as f64,
                s.utc_offset as f64,
                s.latitude as f64,
                s.longitude as f64,
            );
            (sky.moon, sky.sun)
        }
    };
    let p = pole(s.latitude);
    let up = (p - direction * direction.dot(p))
        .try_normalize()
        .unwrap_or_else(|| (Vec3::Z - direction * direction.z).normalize_or(Vec3::Y));
    let right = up.cross(direction).normalize_or(Vec3::X);
    let phase_angle = light_dir.dot(-direction).clamp(-1.0, 1.0).acos();
    Some(MoonState {
        direction,
        up,
        right,
        light_dir,
        illumination: lit_fraction(direction, light_dir),
        phase_angle,
        radius: ANGULAR_RADIUS * s.size.clamp(0.0, 40.0),
    })
}

/// 1 when the moon is clearly above the horizon, 0 below it.
pub fn above_horizon(moon: &MoonState) -> f32 {
    smoothstep(-0.01, 0.04, moon.direction.y)
}

/// The moonlight as the main light: linear colour × intensity (before the twilight fade).
pub fn light(s: &MoonSettings, moon: &MoonState, thickness: f32) -> Vec3 {
    let tint = Vec3::from(s.light_color.map(|c| c.max(0.0).powf(2.2)));
    tint * transmittance(moon.direction.y, thickness)
        * s.light_intensity.max(0.0)
        * phase_brightness(moon.phase_angle)
        * above_horizon(moon)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sun_at(alt: f32, az: f32) -> Vec3 {
        from_az_alt(az, alt)
    }

    #[test]
    fn lit_fraction_follows_the_sun_moon_angle() {
        let m = Vec3::Z;
        for deg in [0.0f32, 30.0, 60.0, 90.0, 120.0, 150.0, 180.0] {
            let a = deg.to_radians();
            let s = Vec3::new(a.sin(), 0.0, a.cos());
            let k = lit_fraction(m, s);
            assert!((k - (1.0 - a.cos()) / 2.0).abs() < 1e-6, "{deg}: {k}");
        }
        assert!(
            lit_fraction(m, Vec3::Z) < 1e-6,
            "new: same direction as the sun"
        );
        assert!(
            (lit_fraction(m, Vec3::NEG_Z) - 1.0).abs() < 1e-6,
            "full: opposite"
        );
        assert!(
            (lit_fraction(m, Vec3::X) - 0.5).abs() < 1e-6,
            "quarter: 90°"
        );
    }

    #[test]
    fn phase_law_dims_the_quarters_and_darkens_the_new_moon() {
        assert_eq!(phase_brightness(0.0), 1.0);
        let quarter = phase_brightness(90f32.to_radians());
        assert!((0.07..0.12).contains(&quarter), "{quarter}");
        assert!(phase_brightness(150f32.to_radians()) < 0.01);
        assert!(phase_brightness(std::f32::consts::PI) < 0.001);
    }

    /// Follow Sun: the phase slider gives the matching lit fraction, waxing moons sit east of
    /// the sun, and a full moon rises (east) as the sun sets (west).
    #[test]
    fn follow_sun_places_the_moon_by_phase() {
        let sunset = sun_at(0.0, 270.0);
        let s = MoonSettings::default();
        for phase in [0.1f32, 0.25, 0.4, 0.5, 0.6, 0.75, 0.9] {
            let st = state(&MoonSettings { phase, ..s.clone() }, Some(sunset)).unwrap();
            let want = (1.0 - (phase * std::f32::consts::TAU).cos()) / 2.0;
            assert!(
                (st.illumination - want).abs() < 0.04,
                "{phase}: {}",
                st.illumination
            );
        }
        let full = state(&s, Some(sunset)).unwrap();
        assert!(
            full.direction.x > 0.9 && full.direction.y.abs() < 0.1,
            "{:?}",
            full.direction
        );
        // First quarter at sunset: high in the south.
        let q = state(
            &MoonSettings {
                phase: 0.25,
                ..s.clone()
            },
            Some(sunset),
        )
        .unwrap();
        assert!(
            q.direction.z < -0.3 && q.direction.y > 0.3,
            "{:?}",
            q.direction
        );
        // At midnight the full moon is up while the sun is down.
        let midnight = follow_sun(sun_at(-40.0, 0.0), 0.5, 45.0);
        assert!(midnight.y > 0.5, "{midnight}");
        // Manual: the slider sets the lit fraction whatever the geometry.
        let manual = state(
            &MoonSettings {
                mode: MoonMode::Manual,
                phase: 0.25,
                ..s.clone()
            },
            Some(sun_at(-20.0, 300.0)),
        )
        .unwrap();
        assert!((manual.illumination - 0.5).abs() < 1e-3);
        assert!(state(&s, None).is_none(), "Follow Sun needs a sun");
        let off = MoonSettings {
            enabled: false,
            ..s
        };
        assert!(state(&off, Some(sunset)).is_none());
    }

    /// Moonlight: dark at new moon, brightest at full; dimmer and redder low in the sky.
    #[test]
    fn moonlight_scales_with_phase_and_height() {
        let midnight = sun_at(-40.0, 0.0);
        let at = |phase: f32| {
            let s = MoonSettings {
                phase,
                ..Default::default()
            };
            light(&s, &state(&s, Some(midnight)).unwrap(), 1.0)
        };
        let (full, quarter, new) = (at(0.5), at(0.25), at(0.0));
        assert!(full.max_element() > 0.2, "{full}");
        assert!(quarter.max_element() < full.max_element() * 0.2);
        assert!(new.max_element() < 1e-3, "{new}");
        let low = transmittance(5f32.to_radians().sin(), 1.0);
        let high = transmittance(1.0, 1.0);
        assert_eq!(high, Vec3::ONE);
        assert!(low.x > low.z && low.z < 0.4, "{low}");
        assert!(transmittance(-0.1, 1.0).max_element() < 0.05);
    }

    /// The twilight cross-over is continuous: the sun fades out before the moon fades in, so
    /// the main light's intensity never jumps while its direction changes.
    #[test]
    fn day_night_transition_is_smooth() {
        let mut last: Option<(f32, f32)> = None;
        let mut y = 0.3f32;
        while y > -0.4 {
            let (s, m) = (sun_fade(y), moon_fade(y));
            assert!(s * m == 0.0, "both lights at {y}");
            if let Some((ls, lm)) = last {
                assert!(
                    (s - ls).abs() < 0.02 && (m - lm).abs() < 0.02,
                    "jump at {y}"
                );
            }
            assert!((0.0..=1.0).contains(&night(y)));
            last = Some((s, m));
            y -= 0.001;
        }
        assert_eq!(sun_fade(0.0), 1.0, "daytime light unchanged");
        assert_eq!(night(-0.02), 0.0, "sunset sky unchanged");
        assert_eq!(moon_fade(-0.3), 1.0);
    }

    #[test]
    fn modes_have_names_and_settings_round_trip() {
        for m in MoonMode::ALL {
            assert_eq!(MoonMode::from_name(m.name()), Some(m));
        }
        let s = MoonSettings {
            mode: MoonMode::DateTime,
            texture: Some("Assets/Moon.png".into()),
            ..Default::default()
        };
        let back: MoonSettings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let old: MoonSettings = serde_json::from_str("{}").unwrap();
        assert!(old.is_default());
    }
}
