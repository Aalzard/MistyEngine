//! Receivers rasterised into lightmap texels: each texel whose centre lies in a uv2 triangle
//! gets that point's world position and normal (texels outside every triangle stay empty and
//! are filled by dilation).

use glam::{Vec2, Vec3};

use super::input::Receiver;
use crate::lightmap::atlas::Placement;

#[derive(Clone, Copy, Debug, Default)]
pub struct Texel {
    pub valid: bool,
    pub position: Vec3,
    pub normal: Vec3,
    /// World size of the texel (denoise distance scale).
    pub size: f32,
    pub direct: Vec3,
    pub indirect: Vec3,
}

/// One lightmap's texels.
pub struct TexelMap {
    pub size: u32,
    pub texels: Vec<Texel>,
}

impl TexelMap {
    pub fn new(size: u32) -> Self {
        Self {
            size,
            texels: vec![Texel::default(); (size * size) as usize],
        }
    }

    pub fn valid_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.texels
            .iter()
            .enumerate()
            .filter(|(_, t)| t.valid)
            .map(|(i, _)| i)
    }
}

/// Write `r`'s texels into `map` at `p`.
pub fn rasterize(map: &mut TexelMap, r: &Receiver, p: &Placement) {
    let side = p.side as f32;
    let world_per_texel = (r.world_area() / r.uv_area().max(1e-6)).sqrt() / side.max(1.0);
    let origin = Vec2::new(p.x as f32, p.y as f32);
    for t in &r.triangles {
        let q = t.uv.map(|uv| origin + uv * side);
        let lo = q[0].min(q[1]).min(q[2]).floor().max(Vec2::ZERO);
        let hi = q[0].max(q[1]).max(q[2]).ceil();
        let area = (q[1] - q[0]).perp_dot(q[2] - q[0]);
        if area.abs() < 1e-9 {
            continue;
        }
        let face = (t.p[1] - t.p[0]).cross(t.p[2] - t.p[0]).normalize_or_zero();
        for y in lo.y as u32..(hi.y as u32).min(map.size) {
            for x in lo.x as u32..(hi.x as u32).min(map.size) {
                let c = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let w1 = (c - q[0]).perp_dot(q[2] - q[0]) / area;
                let w2 = (q[1] - q[0]).perp_dot(c - q[0]) / area;
                let w0 = 1.0 - w1 - w2;
                let eps = -1e-4;
                if w0 < eps || w1 < eps || w2 < eps {
                    continue;
                }
                let texel = &mut map.texels[(y * map.size + x) as usize];
                let mut n = (t.n[0] * w0 + t.n[1] * w1 + t.n[2] * w2).normalize_or(face);
                // Smoothed normals may lean below the face; keep them on its side.
                if n.dot(face) < 0.05 && face != Vec3::ZERO {
                    n = face;
                }
                *texel = Texel {
                    valid: true,
                    position: t.p[0] * w0 + t.p[1] * w1 + t.p[2] * w2,
                    normal: n,
                    size: world_per_texel,
                    ..Default::default()
                };
            }
        }
    }
}
