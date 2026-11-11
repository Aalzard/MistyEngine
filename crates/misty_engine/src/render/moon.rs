//! The moon and the night sky on the GPU and the moonlight (see [`crate::moon`]).
//!
//! - The moon disc and the star field are drawn by the skybox (sky.wgsl `moon_disc`,
//!   `star_field`) before the clouds and fog are composited over it, so clouds in front hide
//!   it and fog and haze dim it. Its uniform rides in the sky uniform (`SkyEnv`, binding 4)
//!   and its texture is view-group binding 11 (only sky.wgsl reads it).
//! - The night sky (dark blue, brighter under a high full moon) replaces the procedural
//!   sky's colours as the sun goes down (`sky_night` in common.wgsl, [`night_mix`] on the CPU),
//!   so sky ambient, reflections and the clouds' sky light follow it.
//! - Moonlight reuses the main Directional Light, shadows and all ([`night_lights`]): once the
//!   sun is down, the renderer's copy of that light turns to the moon. One shadowed light
//!   costs the same as by day; a second shadowed light would double the cascades.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use super::{Environment, LightData};
use crate::moon::{self, MoonState, texture::MoonImage};
use crate::scene::LightType;
use crate::scene::render_settings::SkyboxKind;

/// View-group binding of the moon texture.
pub(crate) const BINDING: u32 = 11;

/// The moon / night part of `SkyEnv` in common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub(crate) struct MoonUniform {
    /// xyz = towards the moon, w = angular radius (0 = no moon).
    pub dir: [f32; 4],
    /// xyz = east on the disc, w = texels across the moon's diameter.
    pub right: [f32; 4],
    /// xyz = north on the disc, w = flags (1 = height in alpha, 2 = square near-side image).
    pub up: [f32; 4],
    /// xyz = towards the sun that lights it, w = earthshine (fraction of the lit radiance).
    pub light: [f32; 4],
    /// rgb = linear radiance of the lit disc (brightness × air × exposure).
    pub color: [f32; 4],
    /// rgb = night zenith colour, w = night share (0 = the day sky as before).
    pub zenith: [f32; 4],
    /// rgb = night horizon colour, w = star brightness.
    pub horizon: [f32; 4],
    /// rgb = night ground colour.
    pub ground: [f32; 4],
    /// Stars, Milky Way, aurora and meteors (`night` in common.wgsl; layout in [`night_params`]).
    pub night: [[f32; 4]; NIGHT_VEC4S],
}

/// vec4s of the night-sky parameters.
pub(crate) const NIGHT_VEC4S: usize = 14;

impl Default for MoonUniform {
    fn default() -> Self {
        bytemuck::Zeroable::zeroed()
    }
}

/// Linear night colours (zenith, horizon) without a moon, and what a full moon high up adds.
const NIGHT_ZENITH: Vec3 = Vec3::new(0.0040, 0.0065, 0.0170);
const NIGHT_HORIZON: Vec3 = Vec3::new(0.0075, 0.0100, 0.0185);
const MOONLIT_ZENITH: Vec3 = Vec3::new(0.007, 0.012, 0.028);
const MOONLIT_HORIZON: Vec3 = Vec3::new(0.011, 0.015, 0.026);
/// Disc radiance of a full moon at Brightness 1 (× the texture's albedo).
const DISC: Vec3 = Vec3::new(1.1, 1.07, 1.01);

/// The scene's moon this frame (None: off, solid-colour sky, or Follow Sun without a sun).
pub fn moon_state(env: &Environment) -> Option<MoonState> {
    if env.settings.skybox.kind == SkyboxKind::SolidColor {
        return None;
    }
    moon::state(&env.settings.moon, env.sun.map(|s| s.direction))
}

/// The night sky this frame: share (0 = day), zenith, horizon and ground colours with the
/// sky's exposure applied, star brightness. None while the share is 0 or the sky is not
/// procedural.
pub struct NightSky {
    pub share: f32,
    pub zenith: Vec3,
    pub horizon: Vec3,
    pub ground: Vec3,
    pub stars: f32,
}

pub fn night_sky(env: &Environment) -> Option<NightSky> {
    let s = &env.settings;
    if !s.night_sky.enabled || s.skybox.kind != SkyboxKind::Procedural {
        return None;
    }
    let sun = env.sun?;
    let share = moon::night(sun.direction.y);
    if share <= 0.0 {
        return None;
    }
    // How much the moon lights the sky: its phase, height and air, relative to the default
    // moonlight.
    let lit = moon_state(env).map_or(0.0, |m| {
        moon::phase_brightness(m.phase_angle)
            * moon::above_horizon(&m)
            * moon::transmittance(m.direction.y, s.skybox.atmosphere_thickness).y
            * (s.moon.light_intensity.max(0.0) / 0.35)
    });
    let n = &s.night_sky;
    let exposure = s.skybox.exposure.max(0.0);
    // Night Sky Darkness dims the sky's own colour, not what glows in it.
    let dark = 1.0 - n.darkness.clamp(0.0, 1.0);
    let lin = |c: [f32; 3]| Vec3::from(super::srgb_to_linear(c));
    // Town glow on the horizon, and the aurora's light in the sky.
    let towns = lin(n.light_pollution_color) * n.light_pollution.max(0.0);
    let aurora = lin(n.aurora_color) * n.aurora.max(0.0) * n.aurora_ambient.max(0.0) * 0.012;
    let zenith = ((NIGHT_ZENITH + MOONLIT_ZENITH * lit) * dark + towns * 0.004 + aurora) * exposure;
    let horizon =
        ((NIGHT_HORIZON + MOONLIT_HORIZON * lit) * dark + towns * 0.03 + aurora) * exposure;
    let ground =
        lin(s.skybox.ground) * ((0.012 + 0.025 * lit) * dark + 0.01 * n.light_pollution) * exposure;
    Some(NightSky {
        share,
        zenith,
        horizon,
        ground,
        stars: n.star_brightness.max(0.0) * share * share,
    })
}

/// The sky colour `day` (in `dir`) blended towards the night sky (mirror of `sky_night` in
/// common.wgsl).
pub fn night_mix(env: &Environment, dir: Vec3, day: Vec3) -> Vec3 {
    let Some(n) = night_sky(env) else {
        return day;
    };
    let night = if dir.y >= 0.0 {
        n.horizon.lerp(n.zenith, dir.y.sqrt())
    } else {
        n.horizon.lerp(n.ground, (-dir.y * 6.0).min(1.0).sqrt())
    };
    day.lerp(night, n.share)
}

/// Stars, Milky Way, aurora and meteors for the night share `share`, at `time` seconds;
/// `high` = the full-detail path (Quality ▸ Volumetric Clouds Medium or High).
///
/// - 0: star density, -, magnitude spread, glow
/// - 1: colour variation, twinkle, twinkle speed, time (s)
/// - 2: star fade, high (1/0), meteors per minute, meteor brightness
/// - 3–5: world directions of the celestial x / y / z axes; w = Milky Way, core, dust
/// - 6–8: world directions of the galactic x (centre) / y / z (pole); w = width, -, -
/// - 9: Milky Way tint (linear); w = aurora visibility
/// - 10: aurora colour; w = lower edge (km)
/// - 11: aurora top colour; w = thickness (km)
/// - 12: xz of the direction to the display's centre, its distance (km), curtains
/// - 13: waviness, ray detail, speed, pulse
pub(crate) fn night_params(
    env: &Environment,
    share: f32,
    time: f32,
    high: bool,
) -> [[f32; 4]; NIGHT_VEC4S] {
    let s = &env.settings;
    let n = &s.night_sky;
    let m = &s.moon;
    let lst = if m.mode == crate::moon::MoonMode::DateTime {
        crate::moon::night_sky::sidereal_hours(
            m.year,
            m.month.clamp(1, 12),
            m.day.clamp(1, 31),
            m.time as f64,
            m.utc_offset as f64,
            m.longitude as f64,
        )
    } else {
        n.sky_time as f64
    };
    let axes = crate::moon::night_sky::celestial_axes(m.latitude, lst);
    let gal = crate::moon::night_sky::galactic_axes(&axes);
    let lin = |c: [f32; 3]| Vec3::from(super::srgb_to_linear(c));
    let az = n.aurora_azimuth.to_radians();
    let v = |a: Vec3, w: f32| a.extend(w).to_array();
    [
        [
            n.star_density.clamp(0.0, 3.0),
            0.0,
            n.star_magnitude_spread.clamp(0.0, 1.0),
            n.star_glow.clamp(0.0, 4.0),
        ],
        [
            n.star_color_variation.clamp(0.0, 1.0),
            n.star_twinkle.clamp(0.0, 2.0),
            n.star_twinkle_speed.max(0.0),
            time,
        ],
        [
            n.star_fade.clamp(0.0, 4.0),
            if high { 1.0 } else { 0.0 },
            n.meteor_rate.clamp(0.0, 600.0),
            n.meteor_brightness.max(0.0) * share,
        ],
        v(axes[0], n.milky_way.max(0.0) * share),
        v(axes[1], n.milky_way_core.max(0.0)),
        v(axes[2], n.milky_way_dust.clamp(0.0, 2.0)),
        v(gal[0], n.milky_way_width.clamp(0.1, 4.0)),
        v(gal[1], 0.0),
        v(gal[2], 0.0),
        v(lin(n.milky_way_tint), n.aurora.max(0.0) * share),
        v(lin(n.aurora_color), n.aurora_height.clamp(10.0, 1000.0)),
        v(
            lin(n.aurora_top_color),
            n.aurora_thickness.clamp(10.0, 1000.0),
        ),
        [
            az.sin(),
            az.cos(),
            n.aurora_distance.clamp(0.0, 3000.0),
            n.aurora_curtains.clamp(1, 6) as f32,
        ],
        [
            n.aurora_waviness.clamp(0.0, 3.0),
            n.aurora_rays.clamp(0.0, 3.0),
            n.aurora_speed.max(0.0),
            n.aurora_pulse.clamp(0.0, 2.0),
        ],
    ]
}

/// The uniform for this frame; `image` = the texture bound (for its size and layout), `time`
/// = the renderer's clock (s), `high` = the full-detail night sky.
pub(crate) fn uniform(
    env: &Environment,
    image: Option<&MoonImage>,
    time: f32,
    high: bool,
) -> MoonUniform {
    let mut u = MoonUniform::default();
    if let Some(n) = night_sky(env) {
        u.zenith = n.zenith.extend(n.share).to_array();
        u.horizon = n.horizon.extend(n.stars).to_array();
        u.ground = n.ground.extend(0.0).to_array();
        u.night = night_params(env, n.share, time, high);
    }
    let (Some(m), Some(img)) = (moon_state(env), image) else {
        return u;
    };
    let s = &env.settings;
    let flags = f32::from(u8::from(img.has_height))
        + 2.0 * f32::from(u8::from(img.projection == moon::texture::Projection::Disc));
    // Earthshine: the Earth seen from the moon is full when the moon is new.
    let earthshine = s.moon.earthshine.max(0.0) * 0.015 * (1.0 - m.illumination);
    let exposure = s.skybox.exposure.max(0.0);
    let color = DISC
        * s.moon.brightness.max(0.0)
        * moon::transmittance(m.direction.y, s.skybox.atmosphere_thickness)
        * exposure;
    u.dir = m.direction.extend(m.radius).to_array();
    u.right = m.right.extend(img.texels_across()).to_array();
    u.up = m.up.extend(flags).to_array();
    u.light = m.light_dir.extend(earthshine).to_array();
    u.color = color.extend(0.0).to_array();
    u
}

/// True when the moon could show this frame (worth loading its texture).
pub(crate) fn wanted(env: &Environment) -> bool {
    moon_state(env).is_some_and(|m| m.direction.y > -m.radius - 0.02)
}

/// The light of the clouds at night: (direction towards the moon, linear colour × intensity),
/// once the sun is far enough down that its own light on the clouds is gone (they fade
/// out by 4.6° below the horizon); None by day or without a moon.
pub(crate) fn cloud_light(env: &Environment) -> Option<(Vec3, Vec3)> {
    let s = &env.settings;
    if !s.moon.enabled || s.skybox.kind == SkyboxKind::SolidColor {
        return None;
    }
    let y = env.sun?.direction.y;
    if y > -0.08 {
        return None;
    }
    let m = moon_state(env)?;
    let fade = moon::moon_fade(y) * (1.0 - moon::smoothstep(-0.12, -0.08, y));
    Some((
        m.direction,
        moon::light(&s.moon, &m, s.skybox.atmosphere_thickness) * fade,
    ))
}

fn linear_to_srgb(c: Vec3) -> [f32; 3] {
    c.to_array().map(|v| v.max(0.0).powf(1.0 / 2.2))
}

/// The frame's lights with the main Directional Light (the sun) turned towards the moon at
/// night: the sun fades out just below the horizon, then the moonlight fades in (cool, dim,
/// scaled by phase and air mass), keeping the light's shadow settings. None by day, with the
/// moon off or a solid-colour sky (the lights are used as they are).
pub fn night_lights(lights: &[LightData], env: &Environment) -> Option<Vec<LightData>> {
    let s = &env.settings;
    if !s.moon.enabled || s.skybox.kind == SkyboxKind::SolidColor {
        return None;
    }
    let sun = env.sun?;
    let y = sun.direction.y;
    let fade = moon::sun_fade(y);
    if fade >= 1.0 {
        return None;
    }
    // The light Environment::new took as the sun.
    let i = lights
        .iter()
        .enumerate()
        .filter(|(_, l)| l.kind == LightType::Directional && l.intensity > 0.0)
        .max_by(|a, b| a.1.intensity.total_cmp(&b.1.intensity))
        .map(|(i, _)| i)?;
    let mut out = lights.to_vec();
    let l = &mut out[i];
    if fade > 0.0 {
        l.intensity *= fade;
        return Some(out);
    }
    let c = moon_state(env).map_or(Vec3::ZERO, |m| {
        let c = moon::light(&s.moon, &m, s.skybox.atmosphere_thickness) * moon::moon_fade(y);
        l.direction = -m.direction;
        c
    });
    let k = c.max_element();
    if k > 0.0 {
        l.color = linear_to_srgb(c / k);
    }
    l.intensity = k;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::environment::Sun;
    use crate::scene::render_settings::RenderSettings;

    fn env_with_sun(y: f32, phase: f32) -> Environment {
        let mut settings = RenderSettings::default();
        settings.moon.phase = phase;
        let mut e = Environment::new(settings, &[]);
        // Below the northern horizon: at night the sun is north of the observer.
        let d = Vec3::new(0.0, y, (1.0 - y * y).max(0.0).sqrt());
        e.sun = Some(Sun {
            direction: d,
            color: [1.0, 0.95, 0.9],
            intensity: 1.2,
        });
        e
    }

    fn sun_light(y: f32) -> LightData {
        let d = Vec3::new(0.0, y, (1.0 - y * y).max(0.0).sqrt());
        LightData {
            kind: LightType::Directional,
            position: Vec3::ZERO,
            direction: -d,
            color: [1.0, 0.95, 0.9],
            intensity: 1.2,
            range: 0.0,
            spot_angle: 0.0,
            shadows: crate::scene::Shadows::Soft,
            shadow: Default::default(),
            volumetric: Default::default(),
            contact_shadows: false,
            area: Default::default(),
        }
    }

    /// Main light (linear colour × intensity, direction) for a sun height.
    fn main_light(y: f32, phase: f32) -> (Vec3, Vec3) {
        let env = env_with_sun(y, phase);
        let lights = [sun_light(y)];
        let out = night_lights(&lights, &env);
        let l = out.as_ref().map_or(&lights[0], |v| &v[0]);
        (
            Vec3::from(crate::render::srgb_to_linear(l.color)) * l.intensity,
            l.direction,
        )
    }

    #[test]
    fn daytime_lights_are_untouched() {
        let env = env_with_sun(0.5, 0.5);
        assert!(night_lights(&[sun_light(0.5)], &env).is_none());
        // Nothing in the sky uniform's night part by day either.
        let u = uniform(&env, None, 0.0, true);
        assert_eq!(u.zenith[3], 0.0);
        assert_eq!(u.night, [[0.0; 4]; NIGHT_VEC4S]);
        assert_eq!(u.dir, [0.0; 4]);
        assert_eq!(night_mix(&env, Vec3::Y, Vec3::ONE), Vec3::ONE);
    }

    #[test]
    fn full_moon_lights_the_night_and_new_moon_does_not() {
        let (full, dir) = main_light(-0.6, 0.5);
        let (new, _) = main_light(-0.6, 0.0);
        assert!(full.max_element() > 0.15, "{full}");
        assert!(full.z > full.x, "cool moonlight: {full}");
        assert!(dir.y < 0.0, "shines down from the moon");
        assert!(new.max_element() < 1e-3, "{new}");
        // Far dimmer than the sun.
        assert!(full.max_element() < 0.3 * 1.2);
    }

    /// Sweeping the sun through sunset, the main light's colour × intensity changes in small
    /// steps: no pop when it switches from the sun to the moon.
    #[test]
    fn sunset_to_moonlight_has_no_jump() {
        let mut last: Option<Vec3> = None;
        let mut y = 0.2f32;
        while y > -0.5 {
            let (c, _) = main_light(y, 0.5);
            if let Some(l) = last {
                assert!(
                    (c - l).abs().max_element() < 0.03,
                    "jump at sun y {y}: {l} -> {c}"
                );
            }
            last = Some(c);
            y -= 0.0005;
        }
        // The sky goes dark blue at night, and brighter with the full moon up.
        let night = night_mix(&env_with_sun(-0.6, 0.5), Vec3::Y, Vec3::ZERO);
        let moonless = night_mix(&env_with_sun(-0.6, 0.0), Vec3::Y, Vec3::ZERO);
        assert!(
            night.z > night.x && night.z > moonless.z,
            "{night} {moonless}"
        );
        assert!(night.max_element() < 0.1);
    }

    /// Night Sky Darkness darkens the sky but not the stars; light pollution brightens the
    /// horizon most; the aurora tints the sky light; defaults pack the old star field.
    #[test]
    fn night_sky_controls_change_the_sky_as_meant() {
        let base = env_with_sun(-0.6, 0.0);
        let sky = |f: &dyn Fn(&mut crate::moon::night_sky::NightSkySettings)| {
            let mut e = base.clone();
            f(&mut e.settings.night_sky);
            night_sky(&e).unwrap()
        };
        let plain = sky(&|_| {});
        let dark = sky(&|n| n.darkness = 0.8);
        assert!(dark.zenith.max_element() < plain.zenith.max_element() * 0.3);
        assert_eq!(dark.stars, plain.stars, "the stars stay");
        let towns = sky(&|n| n.light_pollution = 1.0);
        assert!(towns.horizon.x - plain.horizon.x > 3.0 * (towns.zenith.x - plain.zenith.x));
        let aurora = sky(&|n| n.aurora = 1.0);
        assert!(aurora.zenith.y > plain.zenith.y, "green sky light");
        let off = sky(&|n| n.star_brightness = 0.0);
        assert_eq!(off.stars, 0.0);
        let mut e = base.clone();
        e.settings.night_sky.enabled = false;
        assert!(night_sky(&e).is_none());
        // Packed parameters: default stars, the sky's axes orthonormal, aurora off.
        let p = night_params(&base, 1.0, 3.0, true);
        assert_eq!(p[0][0], 0.7);
        assert_eq!(p[1][3], 3.0);
        assert_eq!(p[9][3], 0.0, "no aurora by default");
        assert_eq!(p[3][3], 0.0, "no Milky Way by default");
        for i in 3..9 {
            let a = Vec3::new(p[i][0], p[i][1], p[i][2]);
            assert!((a.length() - 1.0).abs() < 1e-4, "{i}");
        }
    }
}

/// Headless renders of the moon disc (skipped without an adapter).
#[cfg(test)]
mod gpu_tests {
    use glam::{Quat, Vec3};

    use super::*;
    use crate::camera::{Camera, Projection};
    use crate::moon::MoonMode;
    use crate::render::gpu_tests::{headless, read_back};
    use crate::render::{Frame, PostSettings, Renderer};
    use crate::scene::render_settings::RenderSettings;

    const SIZE: u32 = 256;

    /// The sky towards a Manual moon (straight ahead, 30° up) at `phase`, sun 30° below the
    /// western horizon; returns the image and where the disc's centre and radius fall.
    fn render(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut Renderer,
        phase: f32,
    ) -> (Vec<u8>, f32) {
        let mut settings = RenderSettings::default();
        let m = &mut settings.moon;
        m.mode = MoonMode::Manual;
        m.azimuth = 180.0;
        m.altitude = 30.0;
        m.phase = phase;
        m.size = 20.0;
        settings.night_sky.star_brightness = 0.0;
        let sun = Vec3::new(-1.0, -0.6, 0.0).normalize();
        let light = LightData {
            kind: LightType::Directional,
            position: Vec3::ZERO,
            direction: -sun,
            color: [1.0; 3],
            intensity: 1.0,
            range: 0.0,
            spot_angle: 0.0,
            shadows: crate::scene::Shadows::None,
            shadow: Default::default(),
            volumetric: Default::default(),
            contact_shadows: false,
            area: Default::default(),
        };
        let lights = [light];
        let environment = Environment::new(settings, &lights);
        let state = moon_state(&environment).unwrap();
        // Looking straight at the moon, its north up.
        let camera = Camera {
            position: Vec3::ZERO,
            rotation: Quat::from_mat3(&glam::Mat3::from_cols(
                state.right,
                state.up,
                state.direction,
            )),
            projection: Projection::Perspective { fov_y: 30.0 },
            near: 0.1,
            far: 100.0,
        };
        let frame = Frame {
            camera: &camera,
            grid: None,
            skybox: true,
            background: [0.0; 3],
            environment: &environment,
            post: PostSettings::default(),
            lines: &[],
            overlay: &[],
            meshes: &[],
            particles: &[],
            terrains: &[],
            shaded: true,
            lights: &lights,
            water: &[],
        };
        let target = renderer.create_target(device, SIZE, SIZE);
        for _ in 0..3 {
            renderer.render(device, queue, &target, &frame);
        }
        // Disc radius in pixels.
        let r_px = state.radius.tan() / 15f32.to_radians().tan() * SIZE as f32 * 0.5;
        (read_back(device, queue, &target), r_px)
    }

    /// The night sky looking north, 25° up, at `time` s (no moon): mean colour of the image.
    fn night(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut Renderer,
        n: &crate::moon::night_sky::NightSkySettings,
        time: f32,
    ) -> Vec<u8> {
        night_towards(device, queue, renderer, n, time, 0.0)
    }

    /// [`night`] looking towards `yaw` degrees (0 = north).
    fn night_towards(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut Renderer,
        n: &crate::moon::night_sky::NightSkySettings,
        time: f32,
        yaw: f32,
    ) -> Vec<u8> {
        let mut settings = RenderSettings::default();
        settings.moon.enabled = false;
        settings.night_sky = n.clone();
        let sun = Vec3::new(0.0, -0.6, -0.8);
        let lights = [LightData {
            kind: LightType::Directional,
            position: Vec3::ZERO,
            direction: -sun,
            color: [1.0; 3],
            intensity: 1.0,
            range: 0.0,
            spot_angle: 0.0,
            shadows: crate::scene::Shadows::None,
            shadow: Default::default(),
            volumetric: Default::default(),
            contact_shadows: false,
            area: Default::default(),
        }];
        let mut environment = Environment::new(settings, &lights);
        environment.grass_time = Some(time);
        let camera = Camera {
            position: Vec3::ZERO,
            rotation: crate::camera::euler_to_quat(Vec3::new(-25.0, yaw, 0.0)),
            projection: Projection::Perspective { fov_y: 70.0 },
            near: 0.1,
            far: 100.0,
        };
        let frame = Frame {
            camera: &camera,
            grid: None,
            skybox: true,
            background: [0.0; 3],
            environment: &environment,
            post: PostSettings::default(),
            lines: &[],
            overlay: &[],
            meshes: &[],
            particles: &[],
            terrains: &[],
            shaded: true,
            lights: &lights,
            water: &[],
        };
        let target = renderer.create_target(device, SIZE, SIZE);
        for _ in 0..3 {
            renderer.render(device, queue, &target, &frame);
        }
        read_back(device, queue, &target)
    }

    fn mean(img: &[u8]) -> [f32; 3] {
        let mut s = [0.0f32; 3];
        for px in img.chunks(4) {
            for c in 0..3 {
                s[c] += px[c] as f32;
            }
        }
        s.map(|v| v / (img.len() / 4) as f32)
    }

    /// The aurora shows green in the north and moves; the Milky Way and light pollution
    /// brighten the sky; meteors add a streak.
    #[test]
    fn night_sky_features_render() {
        let Some((device, queue)) = headless() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        use crate::moon::night_sky::NightSkySettings;
        let mut renderer = Renderer::new(&device);
        let dark = NightSkySettings {
            star_brightness: 0.0,
            ..Default::default()
        };
        let base = mean(&night(&device, &queue, &mut renderer, &dark, 5.0));
        let aurora = NightSkySettings {
            aurora: 2.0,
            ..dark.clone()
        };
        let a0 = night(&device, &queue, &mut renderer, &aurora, 5.0);
        let a1 = night(&device, &queue, &mut renderer, &aurora, 25.0);
        let m = mean(&a0);
        assert!(
            m[1] > base[1] + 3.0 && m[1] > m[0] && m[1] > m[2],
            "green: {m:?} vs {base:?}"
        );
        let moved = a0
            .chunks(4)
            .zip(a1.chunks(4))
            .filter(|(p, q)| p[1].abs_diff(q[1]) > 6)
            .count();
        assert!(
            moved > (SIZE * SIZE / 50) as usize,
            "animated: {moved} pixels changed"
        );
        // The summer Milky Way's core low in the south-south-west (Sky Time 20 h).
        let milky = NightSkySettings {
            milky_way: 3.0,
            sky_time: 20.0,
            ..dark.clone()
        };
        let south = mean(&night_towards(
            &device,
            &queue,
            &mut renderer,
            &dark,
            5.0,
            200.0,
        ));
        let mw = mean(&night_towards(
            &device,
            &queue,
            &mut renderer,
            &milky,
            5.0,
            200.0,
        ));
        assert!(mw[0] > south[0] + 3.0, "Milky Way: {mw:?} vs {south:?}");
        let towns = NightSkySettings {
            light_pollution: 1.5,
            ..dark.clone()
        };
        let t = mean(&night(&device, &queue, &mut renderer, &towns, 5.0));
        assert!(t[0] > base[0] + 2.0, "town glow: {t:?}");
        let meteors = NightSkySettings {
            meteor_rate: 600.0,
            meteor_brightness: 4.0,
            ..dark.clone()
        };
        let lit = (0..16)
            .map(|i| {
                let img = night(
                    &device,
                    &queue,
                    &mut renderer,
                    &meteors,
                    3.0 + i as f32 * 0.37,
                );
                img.chunks(4).filter(|p| p[0] > 120).count()
            })
            .max()
            .unwrap();
        assert!(lit > 8, "a meteor streak: {lit} bright pixels");
    }

    /// Mean brightness of a small block at (dx, dy) disc radii from the centre.
    fn at(img: &[u8], r_px: f32, dx: f32, dy: f32) -> f32 {
        let c = SIZE as f32 * 0.5;
        let (x, y) = ((c + dx * r_px) as i32, (c - dy * r_px) as i32);
        let mut sum = 0.0;
        for oy in -2..=2 {
            for ox in -2..=2 {
                let i = (((y + oy) as u32 * SIZE + (x + ox) as u32) * 4) as usize;
                sum += img[i..i + 3].iter().map(|&v| v as f32).sum::<f32>() / 3.0;
            }
        }
        sum / 25.0
    }

    /// Full: evenly lit across; quarters: one half lit (the side towards the sun, west = right
    /// here as the camera faces south); new: only faint earthshine; the sky beside it stays dark.
    #[test]
    fn moon_phases_render() {
        let Some((device, queue)) = headless() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let mut renderer = Renderer::new(&device);
        let (full, r) = render(&device, &queue, &mut renderer, 0.5);
        assert!(r > 10.0, "disc radius {r}px");
        let sky = at(&full, r, 2.5, 0.0);
        let (l, c, rr) = (
            at(&full, r, -0.6, 0.0),
            at(&full, r, 0.0, 0.0),
            at(&full, r, 0.6, 0.0),
        );
        assert!(
            l > sky + 60.0 && rr > sky + 60.0 && c > sky + 60.0,
            "{l} {c} {rr} vs {sky}"
        );
        let (quarter, _) = render(&device, &queue, &mut renderer, 0.25);
        let (ql, qr) = (at(&quarter, r, -0.6, 0.0), at(&quarter, r, 0.6, 0.0));
        let lit = ql.max(qr);
        let dark = ql.min(qr);
        assert!(lit > dark + 50.0, "quarter: {ql} {qr}");
        // The sun is in the west (the camera's right as it faces south).
        assert!(qr > ql, "lit towards the sun: {ql} {qr}");
        let (new, _) = render(&device, &queue, &mut renderer, 0.0);
        let n = at(&new, r, 0.0, 0.0);
        assert!(n < c * 0.25, "new moon {n} vs full {c}");
        assert!(
            n >= at(&new, r, 2.5, 0.0),
            "earthshine no darker than the sky"
        );
        let (crescent, _) = render(&device, &queue, &mut renderer, 0.12);
        let edge = at(&crescent, r, 0.85, 0.0);
        let middle = at(&crescent, r, -0.3, 0.0);
        assert!(
            edge > middle + 30.0,
            "crescent: limb {edge} vs middle {middle}"
        );
    }

    /// GPU cost of the moon disc and the stars (ignored; run on purpose, under the GPU lock):
    /// a 1920 × 1080 night sky looking at a large moon, with the moon and stars on and off.
    ///
    /// `cargo test --release -p misty_engine --lib moon_gpu_cost -- --ignored --nocapture`
    #[test]
    #[ignore = "timing: prints only"]
    fn moon_gpu_cost() {
        let Some((device, queue)) = headless() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let sun = Vec3::new(-1.0, -0.6, 0.0).normalize();
        let lights = [LightData {
            kind: LightType::Directional,
            position: Vec3::ZERO,
            direction: -sun,
            color: [1.0; 3],
            intensity: 1.0,
            range: 0.0,
            spot_angle: 0.0,
            shadows: crate::scene::Shadows::None,
            shadow: Default::default(),
            volumetric: Default::default(),
            contact_shadows: false,
            area: Default::default(),
        }];
        let camera = Camera {
            position: Vec3::ZERO,
            rotation: crate::camera::euler_to_quat(Vec3::new(-30.0, 180.0, 0.0)),
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.1,
            far: 100.0,
        };
        let mut renderer = Renderer::with_queue(&device, &queue);
        renderer.set_quality(&device, &crate::quality::QualityLevel::high());
        let target = renderer.create_target(&device, 1920, 1080);
        // (moon on, its size, the night sky's settings)
        let time = |renderer: &mut Renderer,
                    moon: bool,
                    size: f32,
                    n: &crate::moon::night_sky::NightSkySettings| {
            let mut settings = RenderSettings::default();
            settings.moon.enabled = moon;
            settings.moon.mode = MoonMode::Manual;
            settings.moon.azimuth = 180.0;
            settings.moon.altitude = 30.0;
            settings.moon.size = size;
            settings.night_sky = n.clone();
            let environment = Environment::new(settings, &lights);
            let frame = Frame {
                camera: &camera,
                grid: None,
                skybox: true,
                background: [0.0; 3],
                environment: &environment,
                post: PostSettings::default(),
                lines: &[],
                overlay: &[],
                meshes: &[],
                particles: &[],
                terrains: &[],
                shaded: true,
                lights: &lights,
                water: &[],
            };
            let mut times = Vec::new();
            for i in 0..240 {
                renderer.render(&device, &queue, &target, &frame);
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                if let (true, Some(ms)) = (i >= 40, renderer.gpu_ms()) {
                    times.push(ms);
                }
            }
            times.sort_by(f32::total_cmp);
            times.get(times.len() / 2).copied().unwrap_or(f32::NAN)
        };
        use crate::moon::night_sky::NightSkySettings;
        let none = NightSkySettings {
            star_brightness: 0.0,
            ..Default::default()
        };
        let stars = NightSkySettings::default();
        // Everything on, the aurora in view (south, like the moon).
        let all = NightSkySettings {
            star_magnitude_spread: 1.0,
            star_glow: 2.0,
            star_twinkle: 0.8,
            star_density: 2.0,
            milky_way: 1.5,
            light_pollution: 0.2,
            aurora: 1.5,
            aurora_azimuth: 180.0,
            aurora_curtains: 4,
            meteor_rate: 60.0,
            ..Default::default()
        };
        let aurora_only = NightSkySettings {
            star_brightness: 0.0,
            aurora: 1.5,
            aurora_azimuth: 180.0,
            aurora_curtains: 4,
            ..Default::default()
        };
        let milky_only = NightSkySettings {
            star_brightness: 0.0,
            milky_way: 1.5,
            ..Default::default()
        };
        let low = crate::quality::QualityLevel {
            volumetric_clouds: crate::quality::Tier::Low,
            ..crate::quality::QualityLevel::high()
        };
        for round in 0..2 {
            renderer.set_quality(&device, &crate::quality::QualityLevel::high());
            let off = time(&mut renderer, false, 3.0, &none);
            let star = time(&mut renderer, false, 3.0, &stars);
            let moon = time(&mut renderer, true, 3.0, &none);
            let milky = time(&mut renderer, false, 3.0, &milky_only);
            let aur = time(&mut renderer, false, 3.0, &aurora_only);
            let everything = time(&mut renderer, true, 3.0, &all);
            renderer.set_quality(&device, &low);
            let off_low = time(&mut renderer, false, 3.0, &none);
            let everything_low = time(&mut renderer, true, 3.0, &all);
            eprintln!("round {round}: night sky 1080p, GPU median ms (High):");
            eprintln!("  nothing {off:.3}, stars {star:.3}, moon {moon:.3}, Milky Way {milky:.3}");
            eprintln!("  aurora (4 curtains) {aur:.3}, everything {everything:.3}");
            eprintln!("  Low path: nothing {off_low:.3}, everything {everything_low:.3}");
        }
    }
}
