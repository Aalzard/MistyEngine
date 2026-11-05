//! Path tracing over the static triangles (the probe placement BVH).

use std::f32::consts::{PI, TAU};

use glam::Vec3;

use super::input::{BakeInput, BakeLight, BakeMaterial, LightShape, Sky};
use crate::navigation::geometry::BakeGeometry;
use crate::render::probes::bvh::Bvh;

/// Ray origin offset off surfaces, in metres.
const EPS: f32 = 2e-3;
/// Rays longer than this escape to the sky.
const FAR: f32 = 1e4;

/// Small, fast deterministic generator (PCG32).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut r = Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xDA3E_39CB_94B9_5BDB);
        r.next();
        r
    }

    fn next(&mut self) -> u32 {
        let old = self.0;
        self.0 = old
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let x = (((old >> 18) ^ old) >> 27) as u32;
        x.rotate_right((old >> 59) as u32)
    }

    /// Uniform in [0, 1).
    pub fn f(&mut self) -> f32 {
        (self.next() >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// Cosine-weighted direction around `n`.
pub fn cosine_dir(n: Vec3, rng: &mut Rng) -> Vec3 {
    let (u, v) = (rng.f(), rng.f());
    let r = u.sqrt();
    let phi = TAU * v;
    let (t, b) = n.any_orthonormal_pair();
    (t * (r * phi.cos()) + b * (r * phi.sin()) + n * (1.0 - u).max(0.0).sqrt()).normalize()
}

pub struct TraceScene<'a> {
    bvh: Bvh,
    input: &'a BakeInput,
    normals: Vec<Vec3>,
    bounces: u32,
}

impl<'a> TraceScene<'a> {
    pub fn new(input: &'a BakeInput) -> Self {
        let normals: Vec<Vec3> = input
            .triangles
            .iter()
            .map(|t| (t[1] - t[0]).cross(t[2] - t[0]).normalize_or(Vec3::Y))
            .collect();
        let geo = BakeGeometry {
            triangles: input.triangles.clone(),
            normals: normals.clone(),
            ..Default::default()
        };
        Self {
            bvh: Bvh::new(&geo),
            input,
            normals,
            bounces: input.settings.sanitized().bounces,
        }
    }

    fn sky(&self) -> &Sky {
        &self.input.sky
    }

    /// Is the segment from `from` towards `dir` blocked before `dist`?
    fn blocked(&self, from: Vec3, dir: Vec3, dist: f32) -> bool {
        self.bvh.cast(from, dir, dist).is_some()
    }

    /// Light from `l` arriving at `p` (normal `n`), with the realtime falloff; one shadow ray.
    pub fn light_at(&self, l: &BakeLight, p: Vec3, n: Vec3, rng: &mut Rng) -> Vec3 {
        let from = p + n * EPS;
        match l.shape {
            LightShape::Directional => {
                let to = -l.direction;
                let ndl = n.dot(to);
                if ndl <= 0.0 || (l.shadows && self.blocked(from, to, FAR)) {
                    return Vec3::ZERO;
                }
                l.color * ndl
            }
            LightShape::Point | LightShape::Spot { .. } => {
                let to_light = l.position - p;
                let dist = to_light.length();
                let dir = to_light / dist.max(1e-4);
                let x = dist / l.range;
                let window = (1.0 - x * x * x * x).clamp(0.0, 1.0);
                let mut atten = window * window / (1.0 + 25.0 * x * x);
                if let LightShape::Spot {
                    cos_outer,
                    cos_inner,
                } = l.shape
                {
                    atten *= smoothstep(cos_outer, cos_inner, (-dir).dot(l.direction));
                }
                let ndl = n.dot(dir);
                if ndl <= 0.0 || atten <= 0.0 {
                    return Vec3::ZERO;
                }
                if l.shadows {
                    // Soft shadows: aim at a random point of the source sphere.
                    let target = l.position + sphere_point(rng) * l.radius;
                    let d = target - from;
                    let len = d.length();
                    if len > 1e-4 && self.blocked(from, d / len, (len - EPS).max(0.0)) {
                        return Vec3::ZERO;
                    }
                }
                l.color * (ndl * atten)
            }
            LightShape::Rectangle { hx, hy, two_sided } => {
                let q = l.position + hx * (rng.f() * 2.0 - 1.0) + hy * (rng.f() * 2.0 - 1.0);
                let c = q - p;
                let d2 = c.length_squared().max(1e-6);
                let dist = d2.sqrt();
                let dir = c / dist;
                let forward = hx.cross(hy).normalize_or(l.direction);
                let mut cos_l = -dir.dot(forward);
                if two_sided {
                    cos_l = cos_l.abs();
                }
                let cos_r = n.dot(dir);
                if cos_l <= 0.0 || cos_r <= 0.0 {
                    return Vec3::ZERO;
                }
                // Realtime range window, measured to the closest point of the rectangle.
                let rel = p - l.position;
                let sx = (rel.dot(hx) / hx.length_squared().max(1e-8)).clamp(-1.0, 1.0);
                let sy = (rel.dot(hy) / hy.length_squared().max(1e-8)).clamp(-1.0, 1.0);
                let closest = l.position + hx * sx + hy * sy;
                let x = p.distance(closest) / l.range;
                let w = (1.0 - x * x * x * x).clamp(0.0, 1.0);
                let window = w * w;
                if window <= 0.0 {
                    return Vec3::ZERO;
                }
                if l.shadows && self.blocked(from, dir, (dist - EPS * 2.0).max(0.0)) {
                    return Vec3::ZERO;
                }
                // Form factor estimate: area / π · cosθ cosθ' / d².
                let area = 4.0 * hx.length() * hy.length();
                l.color * (area / PI * cos_l * cos_r / d2 * window)
            }
        }
    }

    /// Direct light at a point from the lights `keep` accepts.
    fn direct(&self, p: Vec3, n: Vec3, rng: &mut Rng, keep: impl Fn(&BakeLight) -> bool) -> Vec3 {
        self.input
            .lights
            .iter()
            .filter(|l| keep(l))
            .map(|l| self.light_at(l, p, n, rng))
            .sum()
    }

    /// Radiance arriving at `o` from direction `d`; `depth` = bounces left.
    pub fn radiance(&self, o: Vec3, d: Vec3, depth: u32, rng: &mut Rng) -> Vec3 {
        let Some(hit) = self.bvh.cast(o, d, FAR) else {
            return self.sky().radiance(d);
        };
        let i = hit.index as usize;
        let m: &BakeMaterial = &self.input.materials[self.input.triangle_material[i] as usize];
        let mut n = self.normals[i];
        if n.dot(d) > 0.0 {
            // Back face: the inside of a closed object, or the far side of a double-sided one.
            if !m.double_sided {
                return Vec3::ZERO;
            }
            n = -n;
        }
        let mut l = m.emission;
        if depth > 0 && m.albedo != Vec3::ZERO {
            let x = o + d * hit.t;
            let mut e = self.direct(x, n, rng, |_| true);
            e += self.radiance(x + n * EPS, cosine_dir(n, rng), depth - 1, rng);
            l += m.albedo * e;
        }
        l
    }

    /// (direct, indirect) light of a texel, averaged over `samples` paths.
    pub fn texel(&self, p: Vec3, n: Vec3, samples: u32, rng: &mut Rng) -> (Vec3, Vec3) {
        let (mut direct, mut indirect) = (Vec3::ZERO, Vec3::ZERO);
        let has_direct = self.input.lights.iter().any(|l| l.bake_direct);
        for _ in 0..samples {
            if has_direct {
                direct += self.direct(p, n, rng, |l| l.bake_direct);
            }
            indirect += self.radiance(p + n * EPS, cosine_dir(n, rng), self.bounces, rng);
        }
        let k = 1.0 / samples as f32;
        (direct * k, indirect * k)
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn sphere_point(rng: &mut Rng) -> Vec3 {
    loop {
        let p = Vec3::new(rng.f(), rng.f(), rng.f()) * 2.0 - 1.0;
        if p.length_squared() <= 1.0 {
            return p;
        }
    }
}
