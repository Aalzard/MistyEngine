//! Bake input: world-space triangles of the scene's static geometry.
//!
//! Objects count when they are active and marked Static: Mesh Renderers with a mesh (primitives
//! and imported models) and Terrains. Triangle normals follow the mesh's vertex normals, so the
//! winding convention doesn't matter. Closed primitives (cube, sphere, capsule, cylinder) are
//! baked as solids, so nothing walkable appears inside them.

use glam::{Mat3, Vec3};
use hecs::Entity;

use crate::scene::{MeshFilter, MeshRenderer, Scene, Terrain};

/// Triangles to bake, each with an upward-or-not face normal.
#[derive(Clone, Debug, Default)]
pub struct BakeGeometry {
    pub triangles: Vec<[Vec3; 3]>,
    /// Unit face normals (same order as `triangles`).
    pub normals: Vec<Vec3>,
    /// Triangle ranges of objects whose inside is solid (closed primitives).
    pub solids: Vec<std::ops::Range<usize>>,
    /// Objects that contributed (for the status line).
    pub objects: usize,
}

impl BakeGeometry {
    /// Add a triangle; `hint` is any vector on the side the face looks at.
    pub fn push(&mut self, t: [Vec3; 3], hint: Vec3) {
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        let Some(n) = n.try_normalize() else {
            return;
        };
        let n = if n.dot(hint) < 0.0 { -n } else { n };
        self.triangles.push(t);
        self.normals.push(n);
    }

    /// World bounds of every triangle.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut it = self.triangles.iter().flatten();
        let first = *it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))))
    }
}

/// Whether `e` takes part in the bake (active and Static).
pub fn is_nav_static(scene: &Scene, e: Entity) -> bool {
    scene.active_in_hierarchy(e) && scene.game_object(e).is_static
}

/// Static triangles of `scene`.
pub fn collect(scene: &Scene) -> BakeGeometry {
    let mut g = BakeGeometry::default();
    for (e, _) in scene.walk() {
        if !is_nav_static(scene, e) {
            continue;
        }
        let before = g.triangles.len();
        add_mesh(scene, e, &mut g);
        add_terrain(scene, e, &mut g);
        if g.triangles.len() > before {
            g.objects += 1;
        }
        if is_closed_primitive(scene, e) {
            g.solids.push(before..g.triangles.len());
        }
    }
    g
}

fn is_closed_primitive(scene: &Scene, e: Entity) -> bool {
    use crate::scene::PrimitiveMesh as P;
    let prim = scene
        .world
        .get::<&MeshFilter>(e)
        .ok()
        .and_then(|f| f.mesh.as_ref().and_then(|m| m.primitive()));
    matches!(prim, Some(P::Cube | P::Sphere | P::Capsule | P::Cylinder))
}

fn add_mesh(scene: &Scene, e: Entity, g: &mut BakeGeometry) {
    let w = &scene.world;
    if !w.get::<&MeshRenderer>(e).is_ok_and(|r| r.enabled) {
        return;
    }
    let Some(mesh_ref) = w.get::<&MeshFilter>(e).ok().and_then(|f| f.mesh.clone()) else {
        return;
    };
    let Some(data) = crate::models::mesh(&mesh_ref) else {
        return;
    };
    let m = scene.world_matrix(e);
    let normal_m = Mat3::from_mat4(m).inverse().transpose();
    for t in data.indices.as_chunks::<3>().0 {
        let v = t.map(|i| &data.vertices[i as usize]);
        let p = v.map(|v| m.transform_point3(Vec3::from(v.position)));
        let hint = v
            .iter()
            .map(|v| normal_m * Vec3::from(v.normal))
            .fold(Vec3::ZERO, |a, b| a + b);
        g.push(p, hint);
    }
}

/// Terrain heightmap as triangles (position only, like the renderer and collider).
fn add_terrain(scene: &Scene, e: Entity, g: &mut BakeGeometry) {
    if !scene.world.get::<&Terrain>(e).is_ok_and(|t| t.enabled) {
        return;
    }
    let Some(d) = crate::terrain::data_of(scene, e) else {
        return;
    };
    let origin = scene.world_position(e);
    let r = d.heightmap_resolution;
    let s = d.spacing();
    let at = |ix: usize, iz: usize| {
        origin
            + Vec3::new(
                ix as f32 * s.x,
                d.height(ix, iz) * d.size.y,
                iz as f32 * s.y,
            )
    };
    for iz in 0..r - 1 {
        for ix in 0..r - 1 {
            // Same split as the terrain mesh: diagonal from (x0, z1) to (x1, z0).
            let (a, b, c, dd) = (
                at(ix, iz),
                at(ix + 1, iz),
                at(ix, iz + 1),
                at(ix + 1, iz + 1),
            );
            g.push([a, c, b], Vec3::Y);
            g.push([b, c, dd], Vec3::Y);
        }
    }
}
