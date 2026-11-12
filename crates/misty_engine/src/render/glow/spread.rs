//! Light ▸ Glow: the glow of a light's visible shape.
//!
//! A Rectangle light's emitter quad glows through bloom, which stacks every bloom level at
//! full weight: a bright quad gets a hot blob as bright as the quad itself right around it
//! and a tail that ends at the widest level. Glow Spread moves a share of that bloom energy
//! into a halo of its own, drawn here: a mix of three Gaussians around the projected quad
//! (distance to its outline in screen space), normalised so it carries exactly the energy
//! taken out of bloom. Wider spread = wider Gaussians and a larger share, so the core cools
//! down while the total glow stays the same.
//!
//! Point and spot lights glow through their Halo sprite, which feeds bloom the same way: its
//! Spread share goes to Gaussians around the light's centre instead. Soft Core dims the
//! sprite's hot centre (energy kept); for Rectangle lights it compresses the emitter quad's
//! visible highlights (shaders/mesh.wgsl `light_glow_surface`).

use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};

use crate::render::area::polygon::rect_corners;
use crate::render::{Frame, LightData, PostSettings, srgb_to_linear};
use crate::scene::{LightGlow, LightType};

/// Standard deviations of the three Gaussians in screen heights at [`reach`] 1 …
pub(super) const SIGMAS: [f32; 3] = [0.04, 0.12, 0.3];
/// … and the share of the energy each carries.
const WEIGHTS: [f32; 3] = [0.25, 0.35, 0.4];
/// The halo ends this many widest sigmas past the outline (faded to exactly 0 there).
pub(super) const REACH_SIGMAS: f32 = 3.5;
/// The prefilter clamps the bloom source at this value (bloom.wgsl `fs_prefilter`).
const BLOOM_CLAMP: f32 = 64.0;

/// `SpreadIn` in glow.wgsl (an instance vertex buffer). Positions are in "q space":
/// NDC with x scaled by the aspect ratio (1 = half the screen height).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable, PartialEq)]
pub(super) struct SpreadInstance {
    /// Corners 0 and 1 of the projected quad (q space).
    pub c01: [f32; 4],
    /// Corners 2 and 3.
    pub c23: [f32; 4],
    /// rgb = linear radiance of the glow, w = occlusion bias in metres.
    pub color: [f32; 4],
    /// xyz = the Gaussians' sigmas (q space), w = reach past the outline (q space).
    pub sigma: [f32; 4],
    /// xyz = peak weight of each Gaussian, w = occlusion tap radius (NDC height units).
    pub weight: [f32; 4],
    /// xyz = light centre (world), w = 1 when the centre is in front of the camera.
    pub center: [f32; 4],
}

/// Share of a Rectangle light's emission that feeds bloom (the rest of its glow goes to the
/// spread halo).
pub fn emitter_feed(g: &LightGlow) -> f32 {
    let g = g.clamped();
    g.intensity * (1.0 - g.spread)
}

/// Width multiplier of the Gaussians at `spread` (0..1).
pub(super) fn reach(spread: f32) -> f32 {
    0.6 + 0.8 * spread.clamp(0.0, 1.0)
}

/// Bloom's energy gain: the screen integral of what bloom adds (after Intensity) over the
/// integral of its source. Each level keeps its source's energy and the upsample chain adds
/// level k with weight `scatter^k` (bloom.wgsl, `Renderer::bloom_passes`).
pub(crate) fn bloom_gain(post: &PostSettings, levels: usize) -> f32 {
    let w = post.scatter_weight();
    let (mut sum, mut p) = (0.0, 1.0);
    for _ in 0..levels {
        sum += p;
        p *= w;
    }
    post.bloom_intensity.max(0.0) * sum
}

/// bloom.wgsl `threshold` (soft knee): the share of `c` that blooms.
fn threshold_share(c: Vec3, t: f32) -> f32 {
    if t <= 0.0 {
        return 1.0;
    }
    let b = c.max_element();
    let knee = t * 0.5;
    let soft = (b - t + knee).clamp(0.0, 2.0 * knee);
    (soft * soft / (4.0 * knee + 1e-4)).max(b - t) / b.max(1e-4)
}

/// Area and perimeter of a simple polygon.
fn area_perimeter(p: &[Vec2; 4]) -> (f32, f32) {
    let (mut a, mut l) = (0.0, 0.0);
    for i in 0..4 {
        let (u, v) = (p[i], p[(i + 1) % 4]);
        a += u.perp_dot(v);
        l += u.distance(v);
    }
    (a.abs() * 0.5, l)
}

/// Peak weights of the Gaussians so that the glow around a convex shape of `area` and
/// `perimeter` integrates to `energy` (Steiner: the band at distance r around it has length
/// perimeter + 2πr, so a Gaussian of distance with peak a and sigma s integrates to
/// a·(area + perimeter·s·√(π/2) + 2π·s²)).
pub(super) fn weights(area: f32, perimeter: f32, sigmas: [f32; 3], energy: f32) -> [f32; 3] {
    let k = (std::f32::consts::PI * 0.5).sqrt();
    std::array::from_fn(|i| {
        let s = sigmas[i];
        let e = area + perimeter * s * k + std::f32::consts::TAU * s * s;
        WEIGHTS[i] * energy / e.max(1e-12)
    })
}

/// Bloom's colour response to a source of linear radiance `c` (its prefilter clamp,
/// threshold and tint).
fn bloomed(post: &PostSettings, c: Vec3) -> Vec3 {
    let c = c.min(Vec3::splat(BLOOM_CLAMP));
    c * threshold_share(c, post.bloom_threshold) * Vec3::from(srgb_to_linear(post.bloom_tint))
}

/// A spread halo around the q-space polygon `q` carrying `energy` (its q-space integral, in
/// units of `color`), or `None` off screen. `center` = world point for the occlusion and fog
/// lookups and whether it is in front of the camera, `bias` = occlusion bias in metres.
fn spread_around(
    q: [Vec2; 4],
    color: Vec3,
    energy: f32,
    spread: f32,
    aspect: f32,
    center: (Vec3, bool),
    bias: f32,
) -> Option<SpreadInstance> {
    let (area, perimeter) = area_perimeter(&q);
    // Screen heights → q space (×2).
    let sigmas = SIGMAS.map(|s| s * reach(spread) * 2.0);
    let w = weights(area, perimeter, sigmas, energy);
    let far = reach_until_faint(sigmas, w, color.max_element());
    let (lo, hi) = q.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| {
        (lo.min(*p), hi.max(*p))
    });
    if lo.x - far > aspect || hi.x + far < -aspect || lo.y - far > 1.0 || hi.y + far < -1.0 {
        return None;
    }
    let size = (hi - lo).max_element();
    Some(SpreadInstance {
        c01: [q[0].x, q[0].y, q[1].x, q[1].y],
        c23: [q[2].x, q[2].y, q[3].x, q[3].y],
        color: color.extend(bias).to_array(),
        sigma: [sigmas[0], sigmas[1], sigmas[2], far],
        weight: [w[0], w[1], w[2], (size * 0.25).clamp(0.003, 0.08)],
        center: center.0.extend(if center.1 { 1.0 } else { 0.0 }).to_array(),
    })
}

/// Below this linear brightness the halo is not drawn (dim lights get a smaller quad).
const FAINT: f32 = 2e-4;

/// How far past the outline the halo reaches: [`REACH_SIGMAS`] widest sigmas, or less where
/// its brightness (`peak` × the Gaussians) has fallen below [`FAINT`].
fn reach_until_faint(sigmas: [f32; 3], w: [f32; 3], peak: f32) -> f32 {
    let max = sigmas[2] * REACH_SIGMAS;
    let at = |d: f32| -> f32 {
        (0..3)
            .map(|i| w[i] * (-d * d / (2.0 * sigmas[i] * sigmas[i])).exp())
            .sum::<f32>()
            * peak
    };
    const STEPS: usize = 32;
    (1..=STEPS)
        .map(|k| max * k as f32 / STEPS as f32)
        .find(|d| at(*d) < FAINT)
        .unwrap_or(max)
}

/// q space of a clip-space point (its w kept at least `min_w`).
fn to_q(c: glam::Vec4, min_w: f32, aspect: f32) -> Vec2 {
    let n = Vec2::new(c.x, c.y) / c.w.max(min_w);
    Vec2::new(n.x * aspect, n.y)
}

/// The spread halo of a Rectangle light, or `None` when it has none or it would not show.
/// `gain` = [`bloom_gain`] of this render.
pub(super) fn spread_instance(
    frame: &Frame,
    l: &LightData,
    aspect: f32,
    gain: f32,
) -> Option<SpreadInstance> {
    if l.kind != LightType::Rectangle || !l.area.show_emitter || l.intensity <= 0.0 {
        return None;
    }
    let g = l.area.glow.clamped();
    let share = g.intensity * g.spread * gain;
    if share <= 0.0 {
        return None;
    }
    let cam = frame.camera;
    let a = &l.area;
    let forward = a.right.cross(a.up);
    if !a.two_sided && forward.dot(cam.position - l.position) <= 0.0 {
        return None;
    }
    let vp = cam.view_projection(aspect);
    let (hx, hy) = a.half_axes();
    let clips = rect_corners(l.position, hx, hy).map(|c| vp * c.extend(1.0));
    if clips.iter().all(|c| c.w <= 0.0) {
        return None;
    }
    // Corners behind the camera are pushed far out (the quad then fills the view).
    let q = clips.map(|c| to_q(c, cam.near * 0.25, aspect));
    let (area, _) = area_perimeter(&q);
    let color = bloomed(
        &frame.post,
        Vec3::from(srgb_to_linear(l.color)) * l.intensity,
    );
    let center_clip = vp * l.position.extend(1.0);
    // Bloom of a quad of radiance 1: its area × the gain; this halo takes `share` of it.
    spread_around(
        q,
        color,
        share * area,
        g.spread,
        aspect,
        (l.position, center_clip.w > 1e-4),
        (a.size.max_element() * 0.35).clamp(0.1, 1.0),
    )
}

/// What a point / spot light's Halo sprite looks like on screen, for its spread halo.
pub(super) struct HaloSprite {
    pub position: Vec3,
    /// World radius.
    pub radius: f32,
    /// Linear colour at the centre (before the falloff).
    pub color: Vec3,
    /// Integral of the falloff over the unit disc (see [`halo_energy`]).
    pub falloff: f32,
}

/// The spread halo of a point / spot light's Halo sprite: `glow.spread` of the bloom the
/// sprite would get, as Gaussians around its centre.
pub(super) fn halo_spread(
    frame: &Frame,
    h: &HaloSprite,
    glow: &LightGlow,
    aspect: f32,
    gain: f32,
) -> Option<SpreadInstance> {
    let g = glow.clamped();
    let share = g.spread * gain;
    if share <= 0.0 {
        return None;
    }
    let cam = frame.camera;
    let vp = cam.view_projection(aspect);
    let c = vp * h.position.extend(1.0);
    if c.w <= 1e-4 {
        return None;
    }
    let p = to_q(c, 1e-4, aspect);
    // Projected radius: a point the radius away along the camera's up axis.
    let up = vp * (h.position + cam.rotation * Vec3::Y * h.radius).extend(1.0);
    let r = to_q(up, 1e-4, aspect).distance(p);
    spread_around(
        [p; 4],
        bloomed(&frame.post, h.color),
        h.falloff * r * r * share,
        g.spread,
        aspect,
        (h.position, true),
        (h.radius * 0.35).clamp(0.1, 1.0),
    )
}

/// Screen integral of the halo sprite's falloff (glow.wgsl `fs_halo`) over the unit disc,
/// at `softness` with its hot core scaled by `core`.
pub(super) fn halo_energy(softness: f32, core: f32) -> f32 {
    let mix = |a: f32, b: f32| a + (b - a) * softness;
    let p = mix(3.2, 1.5);
    let k = mix(90.0, 25.0);
    let m = mix(1.0, 0.35);
    let wide = 0.55 * std::f32::consts::TAU / ((p + 1.0) * (p + 2.0));
    let hot = core * m * std::f32::consts::PI * (1.0 - (-k).exp()) / k;
    wide + hot
}

/// Point / spot halos under Light ▸ Glow: (core scale, brightness multiplier) for a halo of
/// `softness`. Soft Core dims its hot centre and the brightness keeps the sprite's energy
/// (× Glow Intensity). The defaults give exactly (1, 1).
pub(super) fn halo_glow(g: &LightGlow, softness: f32) -> (f32, f32) {
    let g = g.clamped();
    let core = 1.0 - 0.85 * g.soft_core;
    let before = halo_energy(softness, 1.0);
    let after = halo_energy(softness, core);
    (core, g.intensity * before / after)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Distance from `p` to the axis-aligned rectangle of half size `h` (0 inside).
    fn rect_distance(p: Vec2, h: Vec2) -> f32 {
        (p.abs() - h).max(Vec2::ZERO).length()
    }

    /// Numeric screen integral of the three-Gaussian glow around a rectangle.
    fn integral(h: Vec2, sigmas: [f32; 3], w: [f32; 3]) -> f64 {
        let reach = sigmas[2] * 6.0;
        let step = sigmas[0].min(h.min_element()) / 8.0;
        let n = ((h.max_element() + reach) / step) as i32;
        let mut sum = 0.0f64;
        for y in -n..=n {
            for x in -n..=n {
                let d = rect_distance(Vec2::new(x as f32, y as f32) * step, h);
                let k: f32 = (0..3)
                    .map(|i| w[i] * (-d * d / (2.0 * sigmas[i] * sigmas[i])).exp())
                    .sum();
                sum += k as f64;
            }
        }
        sum * (step * step) as f64
    }

    #[test]
    fn spread_weights_carry_the_requested_energy() {
        for (h, spread) in [
            (Vec2::new(0.1, 0.05), 0.0),
            (Vec2::new(0.1, 0.05), 1.0),
            (Vec2::new(0.4, 0.3), 0.5),
            (Vec2::new(0.01, 0.01), 0.5),
        ] {
            let sigmas = SIGMAS.map(|s| s * reach(spread) * 2.0);
            let area = 4.0 * h.x * h.y;
            let perimeter = 4.0 * (h.x + h.y);
            let w = weights(area, perimeter, sigmas, 2.5);
            let e = integral(h, sigmas, w);
            assert!((e - 2.5).abs() < 0.03 * 2.5, "{h} spread {spread}: {e}");
        }
    }

    #[test]
    fn wider_spread_lowers_the_peak() {
        let peak = |spread: f32| {
            let sigmas = SIGMAS.map(|s| s * reach(spread) * 2.0);
            weights(0.01, 0.4, sigmas, 1.0).iter().sum::<f32>()
        };
        assert!(peak(0.5) < peak(0.1) * 0.8);
        assert!(peak(1.0) < peak(0.5) * 0.8);
    }

    #[test]
    fn bloom_gain_follows_the_upsample_chain() {
        let post = PostSettings::default();
        // Scatter 0.7 weighs every level 1: Intensity × level count.
        assert!((bloom_gain(&post, 6) - 0.6 * 6.0).abs() < 1e-5);
        let off = PostSettings {
            bloom_intensity: 0.0,
            ..post
        };
        assert_eq!(bloom_gain(&off, 6), 0.0);
        let tight = PostSettings {
            bloom_scatter: 0.2,
            ..post
        };
        assert!((bloom_gain(&tight, 3) - 0.6 * (1.0 + 0.5 + 0.25)).abs() < 1e-5);
    }

    #[test]
    fn default_glow_changes_nothing() {
        let g = LightGlow::default();
        assert_eq!(emitter_feed(&g), 1.0);
        for soft in [0.0, 0.5, 1.0] {
            assert_eq!(halo_glow(&g, soft), (1.0, 1.0));
        }
    }

    #[test]
    fn soft_core_keeps_the_halo_energy() {
        for soft in [0.0, 0.5, 0.9] {
            let g = LightGlow {
                intensity: 2.0,
                soft_core: 1.0,
                ..Default::default()
            };
            let (core, k) = halo_glow(&g, soft);
            assert!(core < 0.2 && k > 2.0);
            let after = halo_energy(soft, core) * k;
            let want = 2.0 * halo_energy(soft, 1.0);
            assert!((after - want).abs() < 1e-4 * want, "{soft}");
        }
    }

    #[test]
    fn halo_energy_matches_a_numeric_integral() {
        for soft in [0.0, 0.5, 1.0] {
            let mix = |a: f32, b: f32| a + (b - a) * soft;
            let n = 400;
            let mut sum = 0.0f64;
            for y in -n..=n {
                for x in -n..=n {
                    let r = Vec2::new(x as f32, y as f32).length() / n as f32;
                    if r < 1.0 {
                        let wide = (1.0 - r).powf(mix(3.2, 1.5)) * 0.55;
                        let core = (-r * r * mix(90.0, 25.0)).exp() * mix(1.0, 0.35);
                        sum += (wide + core) as f64;
                    }
                }
            }
            let e = sum / (n * n) as f64;
            let want = halo_energy(soft, 1.0) as f64;
            assert!((e - want).abs() < 0.01 * want, "{soft}: {e} vs {want}");
        }
    }
}
