//! GPU layouts of the cloud uniforms (mirrors of `cloud_common.wgsl` and
//! `cloud_shadow_lookup.wgsl`) and how a frame fills them.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec3};

use super::settings::{CloudQuality, EXTINCTION, HAZE_DISTANCE, MAX_DISTANCE, PLANET_RADIUS};
use crate::render::{Frame, srgb_to_linear};
use crate::scene::CloudsOverride;

/// `Clouds` in cloud_common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct MarchUniform {
    /// Inverse of the view-projection without the eye translation: NDC → view ray.
    pub inv_rays: [[f32; 4]; 4],
    pub prev_view_proj: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub layer: [f32; 4],
    pub shape: [f32; 4],
    pub wind: [f32; 4],
    pub wind_dir: [f32; 4],
    pub sun_dir: [f32; 4],
    pub sun_color: [f32; 4],
    pub ambient_top: [f32; 4],
    pub ambient_bottom: [f32; 4],
    pub haze: [f32; 4],
    pub trace: [f32; 4],
    pub misc: [f32; 4],
    pub shadow: [f32; 4],
}

/// `CloudApply` in cloud_shadow_lookup.wgsl (view group binding 6, froxel binding 8).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(super) struct ApplyUniform {
    pub mode: [f32; 4],
    pub shadow: [f32; 4],
    pub sun: [f32; 4],
}

/// Lowest sun height the shadow projection uses (a grazing sun would smear shadows for km).
const MIN_SUN_HEIGHT: f32 = 0.08;

/// Unit xz vector the wind blows towards (degrees clockwise from +Z).
pub(super) fn wind_direction(degrees: f32) -> Vec2 {
    let a = degrees.to_radians();
    Vec2::new(a.sin(), a.cos())
}

/// Cloud base and top altitude (metres).
pub(super) fn layer(o: &CloudsOverride) -> (f32, f32) {
    let base = o.altitude.clamp(0.0, 20_000.0);
    (base, base + o.thickness.clamp(50.0, 10_000.0))
}

/// Unit direction towards the sun (straight up without one); the moon at night.
fn sun_direction(frame: &Frame) -> Vec3 {
    if let Some((moon, _)) = crate::render::moon::cloud_light(frame.environment) {
        return moon;
    }
    frame
        .environment
        .sun
        .map_or(Vec3::Y, |s| s.direction.normalize_or(Vec3::Y))
}

/// World xz of the shadow map's corner: centred under the camera's spot on the cloud base,
/// snapped to whole texels so the shadows don't shimmer as the camera moves.
pub(super) fn shadow_origin(eye: Vec3, sun: Vec3, base: f32, q: &CloudQuality) -> Vec2 {
    let up = sun.y.max(MIN_SUN_HEIGHT);
    let centre = Vec2::new(eye.x, eye.z) + Vec2::new(sun.x, sun.z) / up * (base - eye.y);
    let texel = q.shadow_extent / q.shadow_size as f32;
    (centre / texel).floor() * texel - Vec2::splat(q.shadow_extent * 0.5)
}

/// What the forward shaders and the fog read (`None` = clouds off this view).
pub(super) fn apply_uniform(frame: &Frame, o: &CloudsOverride, q: &CloudQuality) -> ApplyUniform {
    let sun = sun_direction(frame);
    let (base, _) = layer(o);
    let origin = shadow_origin(frame.camera.position, sun, base, q);
    let shadows = o.shadows && frame.environment.sun.is_some();
    ApplyUniform {
        mode: [
            1.0,
            if shadows { 1.0 } else { 0.0 },
            o.shadow_opacity.clamp(0.0, 1.0),
            1.0 / q.shadow_extent,
        ],
        shadow: [origin.x, origin.y, base, sun.y.max(MIN_SUN_HEIGHT)],
        sun: sun
            .extend(clear_reach(frame.camera.position.y, base))
            .to_array(),
    }
}

/// How far from an eye at height `eye_y` geometry certainly hides no clouds (`clouds_at` in
/// clouds_apply.wgsl skips its lookups up to there). Below the layer, every cloud distance the
/// trace writes is at least the climb to the base (the base is a sphere around the planet's
/// centre and the eye is inside it), and a fragment hides the clouds completely up to 0.7 of
/// their distance; 0.6 leaves room for the image's half-float rounding and for history traced
/// from an eye a little lower. 0 at or above the base.
pub(super) fn clear_reach(eye_y: f32, base: f32) -> f32 {
    0.6 * (base - eye_y).max(0.0)
}

/// Inputs of [`march_uniform`] besides the frame.
pub(super) struct MarchInputs<'a> {
    pub frame: &'a Frame<'a>,
    pub o: &'a CloudsOverride,
    pub q: &'a CloudQuality,
    /// Inverse of the view-projection without the eye translation (view rays).
    pub rays: Mat4,
    pub prev_view_proj: Mat4,
    pub trace: (u32, u32),
    pub frame_index: u32,
    pub history: f32,
    pub time: f32,
}

/// Sunlight colour at the clouds: the sky's warm low sun, fading out below the horizon.
fn sun_light(frame: &Frame) -> (Vec3, bool) {
    // At night the moon lights them (render/moon.rs).
    if let Some((_, moon)) = crate::render::moon::cloud_light(frame.environment) {
        return (moon, true);
    }
    let Some(sun) = frame.environment.sun else {
        return (Vec3::ZERO, false);
    };
    let dir = sun.direction.normalize_or(Vec3::Y);
    let t = ((dir.y - 0.0) / 0.42).clamp(0.0, 1.0);
    let sunset = 1.0 - t * t * (3.0 - 2.0 * t);
    let warm = Vec3::ONE.lerp(Vec3::new(1.0, 0.62, 0.38), sunset);
    let above = ((dir.y + 0.08) / 0.12).clamp(0.0, 1.0);
    let c = Vec3::from(srgb_to_linear(sun.color)) * sun.intensity.max(0.0) * warm * above;
    (c, above > 0.0)
}

/// Sky light from above and from the horizon that lights the clouds. An HDRI sky lights them
/// through its diffuse SH (smooth, so a bright sun or a detail in the image does not tint the
/// whole layer); the procedural / solid skies use their zenith and horizon colours.
fn ambient_sky(env: &crate::render::Environment, zenith: Vec3, horizon: Vec3) -> (Vec3, Vec3) {
    let Some(sh) = env.hdri_world_sh() else {
        return (zenith, horizon);
    };
    let eval = |d: Vec3| crate::render::probes::sh::eval(&sh, d).max(Vec3::ZERO);
    let side = [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z]
        .iter()
        .map(|d| eval(*d))
        .sum::<Vec3>()
        / 4.0;
    (eval(Vec3::Y), side)
}

pub(super) fn march_uniform(m: &MarchInputs) -> MarchUniform {
    let (frame, o, q) = (m.frame, m.o, m.q);
    let env = frame.environment;
    let eye = frame.camera.position;
    let sun = sun_direction(frame);
    let (base, top) = layer(o);
    let wind = wind_direction(o.wind_direction);
    // Shapes drift with the wind; the detail moves a little faster so the clouds evolve.
    let drift = -wind * o.wind_speed.max(0.0) * m.time;
    let detail = drift * 1.4 + Vec2::new(0.0, -m.time * 0.6);
    let (sun_color, has_sun) = sun_light(frame);
    let ambient = o.ambient.clamp(0.0, 2.0) * env.settings.environment_lighting.intensity.max(0.0);
    let horizon = [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z]
        .iter()
        .map(|d| env.sky_color(*d))
        .sum::<Vec3>()
        / 4.0;
    let zenith = env.sky_color(Vec3::Y);
    let (sky_top, sky_side) = ambient_sky(env, zenith, horizon);
    let origin = shadow_origin(eye, sun, base, q);
    MarchUniform {
        inv_rays: m.rays.to_cols_array_2d(),
        prev_view_proj: m.prev_view_proj.to_cols_array_2d(),
        eye: eye.extend((m.frame_index % 1024) as f32).to_array(),
        layer: [PLANET_RADIUS, base, top, MAX_DISTANCE],
        shape: [
            o.coverage.clamp(0.0, 1.0),
            o.density.clamp(0.0, 2.0) * EXTINCTION,
            o.shape.clamp(0.0, 1.0),
            m.time,
        ],
        wind: [drift.x, drift.y, detail.x, detail.y],
        wind_dir: [wind.x, wind.y, (top - base) * 0.25, 0.0],
        sun_dir: sun.extend(o.silver_lining.clamp(0.0, 1.0)).to_array(),
        sun_color: sun_color.extend(if has_sun { 1.0 } else { 0.0 }).to_array(),
        ambient_top: (sky_top.lerp(sky_side, 0.3) * ambient)
            .extend(0.0)
            .to_array(),
        ambient_bottom: (sky_side * 0.35 * ambient).extend(0.0).to_array(),
        haze: horizon.extend(HAZE_DISTANCE).to_array(),
        trace: [
            m.trace.0 as f32,
            m.trace.1 as f32,
            q.steps as f32,
            q.light_steps as f32,
        ],
        misc: [
            m.history,
            q.shadow_size as f32,
            q.shadow_extent,
            q.shadow_steps as f32,
        ],
        shadow: [origin.x, origin.y, 0.0, 0.0],
    }
}
