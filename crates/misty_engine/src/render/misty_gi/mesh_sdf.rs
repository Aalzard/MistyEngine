//! Mesh distance fields: one 32³ signed distance volume per mesh (in mesh space), baked on the
//! CPU the first time a mesh is traced and kept in a 3D atlas shared by every view and every
//! instance. Distance = exact distance to the closest triangle; sign from the generalised
//! winding number (inside where the mesh's triangles wrap the point more than halfway), which
//! also gives open and badly closed meshes a sensible inside.
//!
//! Small meshes bake on the render thread (a few milliseconds); bigger ones on a worker thread,
//! left out of the global SDF until they are ready.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use glam::Vec3;
use rayon::prelude::*;

use super::scene::Brick;
use crate::scene::MeshRef;

/// Voxels per side of one mesh SDF.
pub const BRICK: u32 = 32;
/// Atlas size in bricks (x, y, z).
const BRICKS: [u32; 3] = [4, 4, 8];
/// Meshes up to this many triangles bake inline.
const INLINE_TRIANGLES: usize = 512;
/// Meshes above this many triangles are not traced (the bake would take too long).
const MAX_TRIANGLES: usize = 200_000;

pub fn atlas_size() -> [u32; 3] {
    BRICKS.map(|b| b * BRICK)
}

/// A finished bake: distances (mesh units) and the box they cover.
pub struct Baked {
    pub min: Vec3,
    pub max: Vec3,
    pub distances: Vec<f32>,
}

enum Entry {
    Baking(Arc<Mutex<Option<Baked>>>),
    Ready(Brick),
    /// No data or too big: never traced.
    Skipped,
}

/// The atlas and what is in it.
pub struct MeshSdfs {
    pub atlas: wgpu::Texture,
    pub view: wgpu::TextureView,
    entries: HashMap<String, Entry>,
    next: u32,
}

fn key(r: &MeshRef) -> String {
    match r {
        MeshRef::Primitive(p) => format!("primitive:{p:?}"),
        MeshRef::Asset(k) => k.clone(),
    }
}

impl MeshSdfs {
    pub fn new(device: &wgpu::Device) -> Self {
        let [w, h, d] = atlas_size();
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("misty gi mesh sdf atlas"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: d,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::R16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = atlas.create_view(&Default::default());
        Self {
            atlas,
            view,
            entries: HashMap::new(),
            next: 0,
        }
    }

    /// The brick of a mesh, baking it if it was never seen (None while it bakes or when it
    /// can't be traced).
    pub fn brick(&mut self, queue: &wgpu::Queue, r: &MeshRef) -> Option<Brick> {
        let k = key(r);
        if !self.entries.contains_key(&k) {
            let entry = match triangles(r) {
                None => Entry::Skipped,
                Some(tris) if tris.len() > MAX_TRIANGLES => Entry::Skipped,
                Some(tris) if tris.len() <= INLINE_TRIANGLES => {
                    Entry::Baking(Arc::new(Mutex::new(Some(bake(&tris)))))
                }
                Some(tris) => {
                    let slot = Arc::new(Mutex::new(None));
                    let out = slot.clone();
                    std::thread::spawn(move || {
                        let baked = bake(&tris);
                        if let Ok(mut s) = out.lock() {
                            *s = Some(baked);
                        }
                    });
                    Entry::Baking(slot)
                }
            };
            self.entries.insert(k.clone(), entry);
        }
        let done = match self.entries.get(&k)? {
            Entry::Ready(b) => return Some(*b),
            Entry::Skipped => return None,
            Entry::Baking(slot) => slot.lock().ok()?.take()?,
        };
        let entry = match self.upload(queue, &done) {
            Some(b) => Entry::Ready(b),
            None => Entry::Skipped,
        };
        self.entries.insert(k, entry);
        match self.entries.get(&key(r))? {
            Entry::Ready(b) => Some(*b),
            _ => None,
        }
    }

    fn upload(&mut self, queue: &wgpu::Queue, b: &Baked) -> Option<Brick> {
        let n = self.next;
        if n >= BRICKS[0] * BRICKS[1] * BRICKS[2] {
            return None;
        }
        self.next += 1;
        let origin = [
            (n % BRICKS[0]) * BRICK,
            (n / BRICKS[0] % BRICKS[1]) * BRICK,
            (n / (BRICKS[0] * BRICKS[1])) * BRICK,
        ];
        let halves: Vec<u16> = b
            .distances
            .iter()
            .map(|&d| half::f16::from_f32(d).to_bits())
            .collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.atlas,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin[0],
                    y: origin[1],
                    z: origin[2],
                },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&halves),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(BRICK * 2),
                rows_per_image: Some(BRICK),
            },
            wgpu::Extent3d {
                width: BRICK,
                height: BRICK,
                depth_or_array_layers: BRICK,
            },
        );
        Some(Brick {
            origin,
            min: b.min,
            max: b.max,
        })
    }
}

/// Mesh-space triangles of a mesh.
fn triangles(r: &MeshRef) -> Option<Vec<[Vec3; 3]>> {
    let data = crate::models::mesh(r)?;
    let p = |i: u32| data.vertices.get(i as usize).map(|v| Vec3::from(v.position));
    let tris: Vec<[Vec3; 3]> = data
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|t| Some([p(t[0])?, p(t[1])?, p(t[2])?]))
        .collect();
    (!tris.is_empty()).then_some(tris)
}

/// Closest point of triangle `t` to `p` (Ericson, Real-Time Collision Detection 5.1.5).
fn closest_on_triangle(p: Vec3, [a, b, c]: [Vec3; 3]) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// Signed solid angle of triangle `t` seen from `p` (Van Oosterom & Strackee).
fn solid_angle(p: Vec3, [a, b, c]: [Vec3; 3]) -> f32 {
    let (a, b, c) = (a - p, b - p, c - p);
    let (la, lb, lc) = (a.length(), b.length(), c.length());
    let num = a.dot(b.cross(c));
    let den = la * lb * lc + a.dot(b) * lc + b.dot(c) * la + c.dot(a) * lb;
    2.0 * num.atan2(den)
}

/// Bake a 32³ SDF over the triangles' bounds (grown by two voxels so the outside is covered).
pub fn bake(tris: &[[Vec3; 3]]) -> Baked {
    let (mut lo, mut hi) = tris.iter().flatten().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(a, b), p| (a.min(*p), b.max(*p)),
    );
    // Flat meshes still get a volume.
    let extent = (hi - lo).max_element().max(1e-3);
    let pad = Vec3::splat(extent / (BRICK as f32 - 4.0) * 2.0);
    lo -= pad;
    hi += pad;
    let n = BRICK as usize;
    let step = (hi - lo) / (BRICK - 1) as f32;
    let distances: Vec<f32> = (0..n * n * n)
        .into_par_iter()
        .map(|i| {
            let p = lo
                + Vec3::new((i % n) as f32, (i / n % n) as f32, (i / (n * n)) as f32) * step;
            let mut best = f32::MAX;
            let mut winding = 0.0;
            for t in tris {
                best = best.min(closest_on_triangle(p, *t).distance_squared(p));
                winding += solid_angle(p, *t);
            }
            let inside = (winding / (4.0 * std::f32::consts::PI)).abs() > 0.5;
            let d = best.sqrt();
            if inside { -d } else { d }
        })
        .collect();
    Baked {
        min: lo,
        max: hi,
        distances,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cube_bakes_to_a_box_distance() {
        let data = crate::render::mesh::generate(crate::scene::PrimitiveMesh::Cube);
        let p = |i: u32| Vec3::from(data.vertices[i as usize].position);
        let tris: Vec<[Vec3; 3]> = data
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| [p(t[0]), p(t[1]), p(t[2])])
            .collect();
        let b = bake(&tris);
        let n = BRICK as usize;
        let step = (b.max - b.min) / (BRICK - 1) as f32;
        for (i, d) in b.distances.iter().enumerate() {
            let q = b.min
                + Vec3::new((i % n) as f32, (i / n % n) as f32, (i / (n * n)) as f32) * step;
            let e = q.abs() - Vec3::splat(0.5);
            let exact = e.max(Vec3::ZERO).length() + e.max_element().min(0.0);
            assert!((d - exact).abs() < 1e-3, "{q} {d} {exact}");
        }
    }
}
