//! Built-in primitive meshes, sized like the familiar defaults:
//! Cube 1×1×1, Sphere Ø1, Capsule/Cylinder 2 tall Ø1, Plane 10×10 (+Y), Quad 1×1 facing −Z.
//!
//! Triangles wind so that `cross(b - a, c - a)` points outward. In our left-handed world that is
//! clockwise on screen, so pipelines use `FrontFace::Cw`.

use std::collections::HashSet;
use std::f32::consts::{PI, TAU};
use std::sync::OnceLock;

use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};

use crate::scene::PrimitiveMesh;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[derive(Default, Clone)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl MeshData {
    fn vertex(&mut self, p: Vec3, n: Vec3, uv: Vec2) -> u32 {
        self.vertices.push(Vertex {
            position: p.into(),
            normal: n.into(),
            uv: uv.into(),
        });
        self.vertices.len() as u32 - 1
    }

    /// Triangle with outward winding (flips the order if needed to match `normal`).
    fn tri(&mut self, a: u32, b: u32, c: u32) {
        let p = |i: u32| Vec3::from(self.vertices[i as usize].position);
        let n = Vec3::from(self.vertices[a as usize].normal)
            + Vec3::from(self.vertices[b as usize].normal)
            + Vec3::from(self.vertices[c as usize].normal);
        if (p(b) - p(a)).cross(p(c) - p(a)).dot(n) >= 0.0 {
            self.indices.extend([a, b, c]);
        } else {
            self.indices.extend([a, c, b]);
        }
    }

    /// Flat quad from four corners (in order around the edge) with one normal.
    fn quad(&mut self, corners: [Vec3; 4], normal: Vec3) {
        let uvs = [
            Vec2::new(0.0, 1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 0.0),
        ];
        let i: Vec<u32> = (0..4)
            .map(|k| self.vertex(corners[k], normal, uvs[k]))
            .collect();
        self.tri(i[0], i[1], i[2]);
        self.tri(i[0], i[2], i[3]);
    }

    /// Unique triangle edges, for wireframe drawing.
    pub fn edges(&self) -> Vec<[Vec3; 2]> {
        let key = |v: &Vertex| v.position.map(|x| (x * 1e4).round() as i64);
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for t in self.indices.chunks(3) {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let (va, vb) = (&self.vertices[a as usize], &self.vertices[b as usize]);
                let (ka, kb) = (key(va), key(vb));
                let k = if ka < kb { (ka, kb) } else { (kb, ka) };
                if seen.insert(k) {
                    out.push([Vec3::from(va.position), Vec3::from(vb.position)]);
                }
            }
        }
        out
    }
}

pub fn generate(p: PrimitiveMesh) -> MeshData {
    match p {
        PrimitiveMesh::Cube => cube(),
        PrimitiveMesh::Sphere => sphere(0.5, 24, 16),
        PrimitiveMesh::Capsule => capsule(0.5, 2.0, 24, 8),
        PrimitiveMesh::Cylinder => cylinder(0.5, 2.0, 24),
        PrimitiveMesh::Plane => plane(),
        PrimitiveMesh::Quad => quad(),
    }
}

/// Cached mesh data per primitive (shared by the GPU uploader and wireframe/edge drawing).
pub fn primitive(p: PrimitiveMesh) -> &'static MeshData {
    static CACHE: OnceLock<Vec<MeshData>> = OnceLock::new();
    let all = CACHE.get_or_init(|| PrimitiveMesh::ALL.iter().map(|m| generate(*m)).collect());
    &all[PrimitiveMesh::ALL.iter().position(|m| *m == p).unwrap_or(0)]
}

/// Cached unique edges per primitive (wireframe draw modes).
pub fn primitive_edges(p: PrimitiveMesh) -> &'static [[Vec3; 2]] {
    static CACHE: OnceLock<Vec<Vec<[Vec3; 2]>>> = OnceLock::new();
    let all = CACHE.get_or_init(|| {
        PrimitiveMesh::ALL
            .iter()
            .map(|m| primitive(*m).edges())
            .collect()
    });
    &all[PrimitiveMesh::ALL.iter().position(|m| *m == p).unwrap_or(0)]
}

fn cube() -> MeshData {
    let mut m = MeshData::default();
    for axis in 0..3 {
        for sign in [-1.0f32, 1.0] {
            let mut n = Vec3::ZERO;
            n[axis] = sign;
            let (u, v) = (Vec3::new(n.y, n.z, n.x), Vec3::new(n.z, n.x, n.y));
            let c = n * 0.5;
            let (u, v) = (u * 0.5, v * 0.5);
            m.quad([c - u - v, c + u - v, c + u + v, c - u + v], n);
        }
    }
    m
}

fn sphere(r: f32, segments: u32, rings: u32) -> MeshData {
    let mut m = MeshData::default();
    for y in 0..=rings {
        let v = y as f32 / rings as f32;
        let theta = v * PI;
        for x in 0..=segments {
            let u = x as f32 / segments as f32;
            let phi = u * TAU;
            let n = Vec3::new(
                theta.sin() * phi.cos(),
                theta.cos(),
                theta.sin() * phi.sin(),
            );
            m.vertex(n * r, n, Vec2::new(u, v));
        }
    }
    grid_tris(&mut m, 0, segments, rings);
    m
}

/// Triangles for a (segments+1) × (rows+1) vertex grid starting at `base`.
fn grid_tris(m: &mut MeshData, base: u32, segments: u32, rows: u32) {
    let w = segments + 1;
    for y in 0..rows {
        for x in 0..segments {
            let a = base + y * w + x;
            let (b, c, d) = (a + 1, a + w, a + w + 1);
            m.tri(a, c, b);
            m.tri(b, c, d);
        }
    }
}

fn cylinder(r: f32, h: f32, segments: u32) -> MeshData {
    let mut m = MeshData::default();
    let half = h * 0.5;
    // Side.
    for y in [half, -half] {
        for x in 0..=segments {
            let a = x as f32 / segments as f32 * TAU;
            let n = Vec3::new(a.cos(), 0.0, a.sin());
            m.vertex(
                n * r + Vec3::Y * y,
                n,
                Vec2::new(x as f32 / segments as f32, (y > 0.0) as u8 as f32),
            );
        }
    }
    grid_tris(&mut m, 0, segments, 1);
    // Caps.
    for (y, n) in [(half, Vec3::Y), (-half, -Vec3::Y)] {
        let center = m.vertex(Vec3::Y * y, n, Vec2::splat(0.5));
        let ring: Vec<u32> = (0..=segments)
            .map(|x| {
                let a = x as f32 / segments as f32 * TAU;
                let d = Vec3::new(a.cos(), 0.0, a.sin());
                m.vertex(d * r + Vec3::Y * y, n, Vec2::new(d.x, d.z) * 0.5 + 0.5)
            })
            .collect();
        for w in ring.windows(2) {
            m.tri(center, w[0], w[1]);
        }
    }
    m
}

fn capsule(r: f32, h: f32, segments: u32, cap_rings: u32) -> MeshData {
    let mut m = MeshData::default();
    let half = h * 0.5 - r;
    // Rings from top pole to bottom pole: top hemisphere, then bottom hemisphere.
    let rows = cap_rings * 2 + 1;
    for y in 0..=rows {
        let (theta, offset) = if y <= cap_rings {
            (y as f32 / cap_rings as f32 * PI * 0.5, half)
        } else {
            ((y - 1) as f32 / cap_rings as f32 * PI * 0.5, -half)
        };
        for x in 0..=segments {
            let phi = x as f32 / segments as f32 * TAU;
            let n = Vec3::new(
                theta.sin() * phi.cos(),
                theta.cos(),
                theta.sin() * phi.sin(),
            );
            let p = n * r + Vec3::Y * offset;
            m.vertex(
                p,
                n,
                Vec2::new(x as f32 / segments as f32, y as f32 / rows as f32),
            );
        }
    }
    grid_tris(&mut m, 0, segments, rows);
    m
}

fn plane() -> MeshData {
    let mut m = MeshData::default();
    let n = 10;
    for z in 0..=n {
        for x in 0..=n {
            let (u, v) = (x as f32 / n as f32, z as f32 / n as f32);
            m.vertex(
                Vec3::new(5.0 - u * 10.0, 0.0, 5.0 - v * 10.0),
                Vec3::Y,
                Vec2::new(u, v),
            );
        }
    }
    grid_tris(&mut m, 0, n, n);
    m
}

fn quad() -> MeshData {
    let mut m = MeshData::default();
    let n = -Vec3::Z;
    m.quad(
        [
            Vec3::new(-0.5, -0.5, 0.0),
            Vec3::new(0.5, -0.5, 0.0),
            Vec3::new(0.5, 0.5, 0.0),
            Vec3::new(-0.5, 0.5, 0.0),
        ],
        n,
    );
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{Camera, Projection};
    use glam::Quat;

    #[test]
    fn primitives_have_outward_winding_and_unity_sizes() {
        for p in PrimitiveMesh::ALL {
            let m = generate(p);
            assert!(!m.indices.is_empty(), "{p:?}");
            let mut max = Vec3::splat(f32::MIN);
            for v in &m.vertices {
                max = max.max(Vec3::from(v.position));
            }
            assert!(
                (max - p.half_extents()).abs().max_element() < 1e-4,
                "{p:?} bounds {max:?}"
            );
            for t in m.indices.chunks(3) {
                let v = |i: u32| m.vertices[i as usize];
                let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
                let (pa, pb, pc) = (
                    Vec3::from(a.position),
                    Vec3::from(b.position),
                    Vec3::from(c.position),
                );
                let face = (pb - pa).cross(pc - pa);
                if face.length() < 1e-9 {
                    continue; // degenerate pole triangles
                }
                assert!(
                    face.dot(Vec3::from(a.normal) + Vec3::from(b.normal) + Vec3::from(c.normal))
                        > 0.0,
                    "{p:?}"
                );
            }
        }
    }

    #[test]
    fn outward_faces_are_clockwise_on_screen() {
        // Quad faces -Z; a camera at -Z looking +Z sees its front.
        let cam = Camera {
            position: Vec3::new(0.0, 0.0, -5.0),
            rotation: Quat::IDENTITY,
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.1,
            far: 100.0,
        };
        let m = generate(PrimitiveMesh::Quad);
        let vp = cam.view_projection(1.0);
        let ndc = |i: u32| vp.project_point3(Vec3::from(m.vertices[i as usize].position));
        let (a, b, c) = (ndc(m.indices[0]), ndc(m.indices[1]), ndc(m.indices[2]));
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        assert!(area < 0.0, "front faces must be clockwise in NDC (y up)");
    }

    #[test]
    fn cube_has_twelve_triangles_and_wire_edges() {
        let m = generate(PrimitiveMesh::Cube);
        assert_eq!(m.indices.len(), 36);
        // 12 box edges + 6 face diagonals.
        assert_eq!(m.edges().len(), 18);
    }
}
