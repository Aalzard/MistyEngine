//! CPU side of the glow pass: one instance per visible halo, a handful per lens flare.
//! Spot facing, fog response and screen-edge fades are decided here; occlusion is left to
//! the vertex shader (it reads the depth buffer).

use bytemuck::{Pod, Zeroable};
use glam::{Vec3, Vec4Swizzles};

use super::GlowSource;
use super::fog::{extinction_at, fogginess};
use crate::render::{Frame, srgb_to_linear};
use crate::scene::{LensFlare, LightHalo, LightType};

/// `GlowIn` in glow.wgsl (an instance vertex buffer).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable, PartialEq)]
pub(super) struct GlowInstance {
    /// xyz = light position (world), w = halo radius in metres (0 for flares).
    pub center: [f32; 4],
    /// rgb = linear HDR colour, w = occlusion bias in metres.
    pub color: [f32; 4],
    /// Halo: x = softness. Flare: x = element kind, y = position on the screen-centre axis
    /// (1 = at the light, -1 = mirrored), z = size (fraction of the screen height), w = x stretch.
    pub shape: [f32; 4],
}

/// Flare element kinds (`shape.x`).
const GLOW: f32 = 1.0;
const RING: f32 = 2.0;
const STREAK: f32 = 3.0;
const GHOST: f32 = 4.0;

/// Upper bounds per render (beyond these the farthest are dropped).
pub(super) const MAX_HALOS: usize = 256;
pub(super) const MAX_FLARES: usize = 32;

/// Halos widen by up to this factor in thick fog …
const FOG_WIDEN: f32 = 1.5;
/// … and their brightness goes from `CLEAR_GAIN` (clear air) to `CLEAR_GAIN + FOG_GAIN`.
const CLEAR_GAIN: f32 = 0.35;
const FOG_GAIN: f32 = 1.4;
/// Overall flare brightness relative to the light.
const FLARE_GAIN: f32 = 0.6;

#[derive(Default)]
pub(super) struct Instances {
    pub halos: Vec<GlowInstance>,
    pub flares: Vec<GlowInstance>,
}

impl Instances {
    pub fn is_empty(&self) -> bool {
        self.halos.is_empty() && self.flares.is_empty()
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 0..1: how much of a spot light's glow faces `eye` (1 for other lights). Fades in from a
/// little outside the cone to well inside it.
pub(super) fn facing(s: &GlowSource, eye: Vec3) -> f32 {
    if s.kind != LightType::Spot {
        return 1.0;
    }
    let to_eye = (eye - s.position).normalize_or_zero();
    let half = (s.spot_angle * 0.5).clamp(0.5, 89.0);
    let outer = (half + 12.0).min(89.5).to_radians().cos();
    let inner = (half * 0.6).to_radians().cos();
    smoothstep(outer, inner, s.direction.normalize_or_zero().dot(to_eye))
}

/// Light colour × intensity in linear HDR.
fn light_color(s: &GlowSource) -> Vec3 {
    Vec3::from(srgb_to_linear(s.color)) * s.intensity.max(0.0)
}

/// The halo instance of `s`, or `None` when it would not show.
pub(super) fn halo(frame: &Frame, s: &GlowSource, h: &LightHalo) -> Option<GlowInstance> {
    // Directional lights have no position; rectangles show their emitter quad instead.
    let pointlike = matches!(s.kind, LightType::Point | LightType::Spot);
    if !h.enabled || !pointlike || h.size <= 0.0 || h.intensity <= 0.0 {
        return None;
    }
    let cam = frame.camera;
    let face = facing(s, cam.position);
    if face <= 0.0 {
        return None;
    }
    let env = frame.environment;
    let sigma = extinction_at(
        &frame.post.fog,
        &env.fog_volumes,
        &env.settings.fog,
        s.position,
    );
    let g = fogginess(sigma, h.fog_response);
    let radius = h.size * (1.0 + FOG_WIDEN * g);
    // Behind the camera (with its whole radius) or past the far plane: nothing to draw.
    let depth = (s.position - cam.position).dot(cam.forward());
    if depth < -radius || depth > cam.far + radius {
        return None;
    }
    let tint = Vec3::from(srgb_to_linear(h.color.unwrap_or(s.color)));
    let gain = h.intensity * (CLEAR_GAIN + FOG_GAIN * g) * face;
    let color = tint * s.intensity.max(0.0) * gain;
    let softness = h.softness.clamp(0.0, 1.0);
    Some(GlowInstance {
        center: s.position.extend(radius).to_array(),
        color: color.extend(occlusion_bias(radius)).to_array(),
        shape: [softness + (1.0 - softness) * 0.5 * g, 0.0, 0.0, 0.0],
    })
}

/// Scene depth closer than the light by more than this hides it: lamp shades and bulbs right
/// around the light don't.
fn occlusion_bias(radius: f32) -> f32 {
    (radius * 0.35).clamp(0.1, 1.0)
}

/// Where a flare's source is on screen (NDC) and its world position; `None` when behind the
/// camera. Directional lights sit far away, opposite their direction.
fn flare_anchor(frame: &Frame, s: &GlowSource, aspect: f32) -> Option<(Vec3, glam::Vec2)> {
    let cam = frame.camera;
    let world = match s.kind {
        LightType::Directional => cam.position - s.direction.normalize_or(Vec3::Z) * cam.far * 0.5,
        _ => s.position,
    };
    let clip = cam.view_projection(aspect) * world.extend(1.0);
    (clip.w > 1e-4).then(|| (world, clip.xy() / clip.w))
}

/// 1 inside the screen, fading to 0 as the source leaves it.
pub(super) fn edge_fade(ndc: glam::Vec2) -> f32 {
    1.0 - smoothstep(0.8, 1.1, ndc.x.abs().max(ndc.y.abs()))
}

/// Ghost positions on the axis, sizes and tints (a fixed, pleasant spread).
const GHOSTS: [(f32, f32, [f32; 3]); 8] = [
    (0.55, 0.05, [1.0, 0.75, 0.45]),
    (0.2, 0.09, [0.55, 0.8, 1.0]),
    (-0.15, 0.035, [0.7, 1.0, 0.6]),
    (-0.4, 0.12, [1.0, 0.55, 0.8]),
    (-0.7, 0.06, [0.6, 0.7, 1.0]),
    (-1.0, 0.16, [1.0, 0.85, 0.55]),
    (-1.3, 0.08, [0.65, 1.0, 0.9]),
    (0.85, 0.03, [1.0, 0.6, 0.5]),
];

/// The flare elements of `s` appended to `out`.
pub(super) fn flare(
    frame: &Frame,
    s: &GlowSource,
    f: &LensFlare,
    aspect: f32,
    out: &mut Vec<GlowInstance>,
) {
    if !f.enabled || f.intensity <= 0.0 || f.scale <= 0.0 {
        return;
    }
    let Some((world, ndc)) = flare_anchor(frame, s, aspect) else {
        return;
    };
    let fade = edge_fade(ndc) * facing(s, frame.camera.position);
    if fade <= 0.0 {
        return;
    }
    let base = light_color(s) * Vec3::from(srgb_to_linear(f.tint)) * f.intensity * fade;
    let base = base * FLARE_GAIN;
    let bias = match s.kind {
        LightType::Directional => 0.0,
        _ => 0.25,
    };
    let mut push = |kind, axis: f32, size: f32, stretch, color: Vec3| {
        out.push(GlowInstance {
            center: world.extend(0.0).to_array(),
            color: color.extend(bias).to_array(),
            shape: [kind, axis, size * f.scale, stretch],
        });
    };
    push(GLOW, 1.0, 0.16, 1.0, base * 0.6);
    if f.streak {
        push(STREAK, 1.0, 0.035, 14.0, base * 0.35);
    }
    if f.ring {
        push(RING, 1.0, 0.24, 1.0, base * 0.07);
    }
    for (axis, size, tint) in GHOSTS
        .iter()
        .take(f.ghosts.min(GHOSTS.len() as u32) as usize)
    {
        push(GHOST, *axis, *size, 1.0, base * Vec3::from(*tint) * 0.07);
    }
}

/// Every instance of this render. `flares` = the Quality level allows lens flares.
pub(super) fn build(frame: &Frame, aspect: f32, flares: bool) -> Instances {
    let mut out = Instances::default();
    if !frame.shaded {
        return out;
    }
    let eye = frame.camera.position;
    let mut sources: Vec<&GlowSource> = frame.environment.glows.iter().collect();
    // Nearest first, so the caps drop the far ones.
    sources.sort_by(|a, b| {
        a.position
            .distance_squared(eye)
            .total_cmp(&b.position.distance_squared(eye))
    });
    let mut flaring = 0;
    for s in sources {
        if let Some(h) = &s.halo
            && out.halos.len() < MAX_HALOS
            && let Some(i) = halo(frame, s, h)
        {
            out.halos.push(i);
        }
        if flares
            && flaring < MAX_FLARES
            && let Some(f) = &s.flare
        {
            let before = out.flares.len();
            flare(frame, s, f, aspect, &mut out.flares);
            flaring += usize::from(out.flares.len() > before);
        }
    }
    out
}
