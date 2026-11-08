//! Bake input: world-space triangles (with an area each) and area volumes.
//!
//! The Navigation window's Bake collects active objects marked Navigation Static; a NavMesh
//! Surface collects by its own rules (all objects, a volume or its hierarchy, a layer mask) and
//! never takes objects with a Nav Mesh Agent or Obstacle. Mesh Renderers with a mesh (primitives
//! and imported models) and Terrains count. Triangle normals follow the mesh's vertex normals,
//! so the winding convention doesn't matter. Closed primitives (cube, sphere, capsule, cylinder)
//! are baked as solids, so nothing walkable appears inside them. Nav Mesh Modifiers change the
//! area of (or leave out) an object and its children; Modifier Volumes become area boxes.

use glam::{Mat3, Mat4, Vec3};
use hecs::Entity;

use super::{
    AREA_NOT_WALKABLE, CollectObjects, NavMeshAgent, NavMeshModifier, NavMeshModifierVolume,
    NavMeshObstacle, NavMeshSurface,
};
use crate::scene::{MeshFilter, MeshRenderer, Scene, Terrain};

/// Triangles to bake, each with an upward-or-not face normal and an area.
#[derive(Clone, Debug, Default)]
pub struct BakeGeometry {
    pub triangles: Vec<[Vec3; 3]>,
    /// Unit face normals (same order as `triangles`).
    pub normals: Vec<Vec3>,
    /// Area per triangle (missing entries are Walkable).
    pub areas: Vec<u8>,
    /// Triangle ranges of objects whose inside is solid (closed primitives).
    pub solids: Vec<std::ops::Range<usize>>,
    /// Objects that contributed (for the status line).
    pub objects: usize,
}

impl BakeGeometry {
    /// Add a Walkable triangle; `hint` is any vector on the side the face looks at.
    pub fn push(&mut self, t: [Vec3; 3], hint: Vec3) {
        self.push_area(t, hint, super::AREA_WALKABLE);
    }

    /// Add a triangle with an area.
    pub fn push_area(&mut self, t: [Vec3; 3], hint: Vec3, area: u8) {
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        let Some(n) = n.try_normalize() else {
            return;
        };
        let n = if n.dot(hint) < 0.0 { -n } else { n };
        // Keep `areas` in step even when earlier triangles came without one.
        self.areas
            .resize(self.triangles.len(), super::AREA_WALKABLE);
        self.triangles.push(t);
        self.normals.push(n);
        self.areas.push(area);
    }

    /// Area of triangle `i`.
    pub fn area(&self, i: usize) -> u8 {
        self.areas.get(i).copied().unwrap_or(super::AREA_WALKABLE)
    }

    /// World bounds of every triangle.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut it = self.triangles.iter().flatten();
        let first = *it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))))
    }
}

/// A box that gives the nav mesh inside it an area (Not Walkable removes it).
#[derive(Clone, Debug, PartialEq)]
pub struct AreaVolume {
    /// World → box space (the box is `-half..half`).
    pub to_local: Mat4,
    pub half: Vec3,
    pub area: u8,
    /// World bounds.
    pub lo: Vec3,
    pub hi: Vec3,
}

impl AreaVolume {
    /// A box `size` at `center`, local to `world`.
    pub fn new(world: Mat4, center: Vec3, size: Vec3, area: u8) -> Self {
        let m = world * Mat4::from_translation(center);
        let half = size.abs() * 0.5;
        let (lo, hi) = obb_bounds(m, half);
        Self {
            to_local: m.inverse(),
            half,
            area,
            lo,
            hi,
        }
    }

    pub fn contains(&self, p: Vec3) -> bool {
        let q = self.to_local.transform_point3(p);
        q.abs().cmple(self.half + Vec3::splat(1e-4)).all()
    }
}

/// World bounds of the box `-half..half` under `m`.
pub fn obb_bounds(m: Mat4, half: Vec3) -> (Vec3, Vec3) {
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for i in 0..8 {
        let c = Vec3::new(
            if i & 1 == 0 { -half.x } else { half.x },
            if i & 2 == 0 { -half.y } else { half.y },
            if i & 4 == 0 { -half.z } else { half.z },
        );
        let p = m.transform_point3(c);
        lo = lo.min(p);
        hi = hi.max(p);
    }
    (lo, hi)
}

/// Everything one bake needs from the scene.
#[derive(Clone, Debug, Default)]
pub struct BuildInput {
    pub geometry: BakeGeometry,
    pub volumes: Vec<AreaVolume>,
    /// Only this world box is baked (Collect Objects ▸ Volume).
    pub clip: Option<(Vec3, Vec3)>,
}

/// Whether `e` takes part in the Navigation window's bake (active and Navigation Static).
pub fn is_nav_static(scene: &Scene, e: Entity) -> bool {
    scene.active_in_hierarchy(e)
        && scene
            .game_object(e)
            .has_static(crate::scene::static_flags::StaticFlags::NAVIGATION)
}

/// Static triangles of `scene` (Walkable, no modifiers).
pub fn collect(scene: &Scene) -> BakeGeometry {
    collect_where(scene, is_nav_static)
}

/// Triangles of the objects `keep` accepts (reflection probe placement takes every active one).
pub fn collect_where(scene: &Scene, keep: impl Fn(&Scene, Entity) -> bool) -> BakeGeometry {
    let mut g = BakeGeometry::default();
    for (e, _) in scene.walk() {
        if keep(scene, e) {
            add_object(scene, e, super::AREA_WALKABLE, &mut g);
        }
    }
    g
}

/// The modifier in charge of `e` for `agent_type`: its own, else the nearest ancestor's that
/// applies to children.
fn modifier_of(scene: &Scene, e: Entity, agent_type: u32) -> Option<NavMeshModifier> {
    let mut cur = Some(e);
    while let Some(c) = cur {
        if let Ok(m) = scene.world.get::<&NavMeshModifier>(c)
            && m.enabled
            && m.affects(agent_type)
            && (c == e || m.apply_to_children)
        {
            return Some((*m).clone());
        }
        cur = scene.parent(c);
    }
    None
}

/// Area for `e`'s triangles (None = left out).
fn object_area(scene: &Scene, e: Entity, agent_type: u32, default_area: u8) -> Option<u8> {
    match modifier_of(scene, e, agent_type) {
        Some(m) if m.ignore_from_build => None,
        Some(m) if m.override_area => Some(m.area),
        _ => Some(default_area),
    }
}

/// The Navigation window's bake input: Navigation Static objects for the Humanoid.
pub fn collect_static(scene: &Scene) -> BuildInput {
    let mut g = BakeGeometry::default();
    for (e, _) in scene.walk() {
        if !is_nav_static(scene, e) {
            continue;
        }
        if let Some(area) = object_area(scene, e, 0, super::AREA_WALKABLE) {
            add_object(scene, e, area, &mut g);
        }
    }
    BuildInput {
        geometry: g,
        volumes: volumes(scene, 0, None),
        clip: None,
    }
}

/// A NavMesh Surface's bake input (`e` holds the surface).
pub fn collect_surface(scene: &Scene, e: Entity, s: &NavMeshSurface) -> BuildInput {
    let clip = (s.collect == CollectObjects::Volume).then(|| {
        obb_bounds(
            scene.world_matrix(e) * Mat4::from_translation(s.center),
            s.size.abs() * 0.5,
        )
    });
    let mut g = BakeGeometry::default();
    for (o, _) in scene.walk() {
        if !scene.active_in_hierarchy(o)
            || scene.world.get::<&NavMeshAgent>(o).is_ok()
            || scene.world.get::<&NavMeshObstacle>(o).is_ok()
        {
            continue;
        }
        let layer = scene.game_object(o).layer as u32;
        if layer < 32 && s.layer_mask & (1 << layer) == 0 {
            continue;
        }
        if s.collect == CollectObjects::Children && o != e && !scene.is_ancestor(e, o) {
            continue;
        }
        let Some(area) = object_area(scene, o, s.agent_type, s.default_area) else {
            continue;
        };
        let mut part = BakeGeometry::default();
        add_object(scene, o, area, &mut part);
        if let Some((lo, hi)) = clip {
            let Some((plo, phi)) = part.bounds() else {
                continue;
            };
            if plo.cmpgt(hi).any() || phi.cmplt(lo).any() {
                continue;
            }
        }
        append(&mut g, part);
    }
    let within = (s.collect == CollectObjects::Children).then_some(e);
    BuildInput {
        geometry: g,
        volumes: volumes(scene, s.agent_type, within),
        clip,
    }
}

fn append(g: &mut BakeGeometry, part: BakeGeometry) {
    let base = g.triangles.len();
    g.areas.resize(base, super::AREA_WALKABLE);
    g.triangles.extend(part.triangles);
    g.normals.extend(part.normals);
    g.areas.extend(part.areas);
    g.solids.extend(
        part.solids
            .into_iter()
            .map(|r| r.start + base..r.end + base),
    );
    g.objects += part.objects;
}

/// Enabled Modifier Volumes for `agent_type` (only under `within`, when given).
pub fn volumes(scene: &Scene, agent_type: u32, within: Option<Entity>) -> Vec<AreaVolume> {
    let mut out = Vec::new();
    for (e, _) in scene.walk() {
        if !scene.active_in_hierarchy(e) {
            continue;
        }
        if let Some(root) = within
            && e != root
            && !scene.is_ancestor(root, e)
        {
            continue;
        }
        let Ok(v) = scene.world.get::<&NavMeshModifierVolume>(e) else {
            continue;
        };
        if v.enabled && v.affects(agent_type) {
            out.push(AreaVolume::new(
                scene.world_matrix(e),
                v.center,
                v.size,
                v.area,
            ));
        }
    }
    out
}

/// Add `e`'s mesh and terrain triangles with `area`.
fn add_object(scene: &Scene, e: Entity, area: u8, g: &mut BakeGeometry) {
    let before = g.triangles.len();
    add_mesh(scene, e, area, g);
    add_terrain(scene, e, area, g);
    if g.triangles.len() > before {
        g.objects += 1;
    }
    if is_closed_primitive(scene, e) {
        g.solids.push(before..g.triangles.len());
    }
}

/// Not Walkable objects still block (they are rasterised, but never walkable).
pub(super) fn walkable_area(area: u8) -> bool {
    area != AREA_NOT_WALKABLE
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

fn add_mesh(scene: &Scene, e: Entity, area: u8, g: &mut BakeGeometry) {
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
        g.push_area(p, hint, area);
    }
}

/// Terrain heightmap as triangles (position only, like the renderer and collider).
fn add_terrain(scene: &Scene, e: Entity, area: u8, g: &mut BakeGeometry) {
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
            g.push_area([a, c, b], Vec3::Y, area);
            g.push_area([b, c, dd], Vec3::Y, area);
        }
    }
}
