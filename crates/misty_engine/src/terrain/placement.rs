//! Place Objects: a brush that scatters prefab instances (rocks, bushes, props) on a terrain
//! as ordinary scene objects under the terrain's "Placed Objects" child, and erases them
//! again. New objects keep a minimum spacing, get a random scale and turn, can tilt with the
//! ground, and only go where the slope and height limits allow (like Paint Trees, whose
//! trees live in the terrain data instead).

use glam::{Mat4, Quat, Vec2, Vec3};
use hecs::Entity;

use super::{TerrainData, noise};
use crate::scene::Scene;
use crate::scene::prefab::Prefab;

/// Name of the terrain child that holds placed objects.
pub const HOLDER: &str = "Placed Objects";
/// Most objects one brush step adds (so a held brush fills the area over a few frames).
pub const PER_STEP: usize = 3;

/// How new objects are placed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObjectPlacement {
    /// Objects per 100 m² the brush fills up to.
    pub density: f32,
    /// Closest two objects stand (metres, centre to centre).
    pub spacing: f32,
    /// Uniform scale range.
    pub scale: [f32; 2],
    /// Random turn about the up axis.
    pub random_rotation: bool,
    /// Tilt with the ground instead of standing upright.
    pub align_to_normal: bool,
    /// Ground steepness allowed (degrees).
    pub slope: [f32; 2],
    /// Terrain heights allowed (metres above the terrain's base).
    pub height: [f32; 2],
    pub seed: u32,
}

impl Default for ObjectPlacement {
    fn default() -> Self {
        Self {
            density: 2.0,
            spacing: 3.0,
            scale: [0.8, 1.3],
            random_rotation: true,
            align_to_normal: true,
            slope: [0.0, 45.0],
            height: [0.0, 10_000.0],
            seed: 1,
        }
    }
}

impl ObjectPlacement {
    /// Ground at terrain-local `(x, z)` allows an object (inside, no hole, slope and height
    /// within the limits).
    pub fn allows(&self, d: &TerrainData, x: f32, z: f32) -> bool {
        if x < 0.0 || z < 0.0 || x > d.size.x || z > d.size.z || d.hole_at(x, z) {
            return false;
        }
        let slope = super::trees::slope_at(d, x, z);
        let h = d.sample_height(x, z);
        (self.slope[0]..=self.slope[1]).contains(&slope)
            && (self.height[0]..=self.height[1]).contains(&h)
    }

    /// Terrain-local transform of a new object at `(x, z)`; `n` picks its random values.
    pub fn transform(&self, d: &TerrainData, x: f32, z: f32, n: u32) -> (Vec3, Quat, f32) {
        let r = |salt: i32| noise::hash(n as i32, salt, self.seed);
        let up = if self.align_to_normal {
            super::detail_layers::ground_normal(d, Vec2::new(x, z))
        } else {
            Vec3::Y
        };
        let yaw = if self.random_rotation {
            r(13) * std::f32::consts::TAU
        } else {
            0.0
        };
        let (lo, hi) = (self.scale[0], self.scale[1].max(self.scale[0]));
        (
            Vec3::new(x, d.sample_height(x, z), z),
            crate::world_gen::scatter_set::instance_rotation(yaw, up),
            lo + (hi - lo) * r(11),
        )
    }
}

/// Spots for new objects in a brush circle (terrain-local centre and radius) given the
/// objects already there (`existing`, local xz): up to [`PER_STEP`] while fewer than the
/// density asks for stand inside. `step` varies the random spots from step to step.
pub fn spots(
    d: &TerrainData,
    center: Vec2,
    radius: f32,
    existing: &[Vec2],
    p: &ObjectPlacement,
    step: u32,
) -> Vec<(Vec3, Quat, f32)> {
    let area = std::f32::consts::PI * radius * radius;
    let want = (area / 100.0 * p.density).round().max(1.0) as usize;
    let mut placed: Vec<Vec2> = existing.to_vec();
    let have = placed
        .iter()
        .filter(|q| q.distance(center) <= radius)
        .count();
    let mut out = Vec::new();
    if have >= want {
        return out;
    }
    let base = step.wrapping_mul(7919) ^ (center.x * 13.0 + center.y * 7.0) as u32;
    let tries = ((want - have) * 6).max(12) as u32;
    for i in 0..tries {
        if out.len() >= PER_STEP.min(want - have) {
            break;
        }
        let n = base.wrapping_add(i * 2_654_435_761);
        let a = noise::hash(n as i32, 5, p.seed) * std::f32::consts::TAU;
        let r = noise::hash(n as i32, 6, p.seed).sqrt() * radius;
        let q = center + Vec2::new(a.cos(), a.sin()) * r;
        if !p.allows(d, q.x, q.y) || placed.iter().any(|o| o.distance(q) < p.spacing) {
            continue;
        }
        placed.push(q);
        out.push(p.transform(d, q.x, q.y, n));
    }
    out
}

/// The terrain object's "Placed Objects" child (made when missing and `create`).
pub fn holder(scene: &mut Scene, terrain: Entity, create: bool) -> Option<Entity> {
    let found = (scene.children(terrain).into_iter()).find(|c| scene.name(*c) == HOLDER);
    match found {
        Some(h) => Some(h),
        None if create => Some(scene.spawn(HOLDER, Some(terrain))),
        None => None,
    }
}

/// Positions (terrain-local xz) of the objects under the terrain's holder.
pub fn placed(scene: &Scene, terrain: Entity) -> Vec<Vec2> {
    let corner = scene.world_position(terrain);
    let Some(h) = (scene.children(terrain).into_iter()).find(|c| scene.name(*c) == HOLDER) else {
        return Vec::new();
    };
    scene
        .children(h)
        .into_iter()
        .map(|c| {
            let p = scene.world_position(c) - corner;
            Vec2::new(p.x, p.z)
        })
        .collect()
}

/// One brush step of Place Objects: instances of `prefab` (asset `path`) at the free spots
/// in the circle. Returns how many were placed.
#[allow(clippy::too_many_arguments)]
pub fn place(
    scene: &mut Scene,
    terrain: Entity,
    d: &TerrainData,
    prefab: &Prefab,
    path: &str,
    center: Vec2,
    radius: f32,
    p: &ObjectPlacement,
    step: u32,
) -> usize {
    let existing = placed(scene, terrain);
    let new = spots(d, center, radius, &existing, p, step);
    if new.is_empty() {
        return 0;
    }
    let Some(h) = holder(scene, terrain, true) else {
        return 0;
    };
    let corner = scene.world_position(terrain);
    for (pos, rot, scale) in &new {
        let e = scene.instantiate_prefab(prefab, Some(path), Some(h));
        // The prefab root's own scale and turn stay under the brush's.
        let own = scene.transform(e);
        let m = Mat4::from_scale_rotation_translation(Vec3::splat(*scale), *rot, corner + *pos)
            * Mat4::from_quat(own.rotation)
            * Mat4::from_scale(own.scale);
        crate::world_gen::set_world_matrix(scene, e, m);
    }
    new.len()
}

/// Erase placed objects in the circle (terrain-local centre, radius); `only` = just the
/// instances of that prefab asset. Returns how many were removed.
pub fn erase(
    scene: &mut Scene,
    terrain: Entity,
    center: Vec2,
    radius: f32,
    only: Option<&str>,
) -> usize {
    let Some(h) = holder(scene, terrain, false) else {
        return 0;
    };
    let corner = scene.world_position(terrain);
    let doomed: Vec<Entity> = scene
        .children(h)
        .into_iter()
        .filter(|c| {
            let p = scene.world_position(*c) - corner;
            Vec2::new(p.x, p.z).distance(center) <= radius
                && only.is_none_or(|path| scene.prefab_link(*c).as_deref() == Some(path))
        })
        .collect();
    for e in &doomed {
        scene.despawn(*e);
    }
    doomed.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{MeshFilter, MeshRef, PrimitiveMesh, Terrain};

    /// Flat for x < 32 m, a 60° wall after; 64 m square.
    fn ground() -> TerrainData {
        let mut d = TerrainData::new(65, Vec3::new(64.0, 100.0, 64.0), 64);
        for iz in 0..65 {
            for ix in 0..65 {
                let rise = (ix as f32 - 32.0).max(0.0) * 3f32.sqrt();
                d.set_height(ix, iz, rise / 100.0);
            }
        }
        d
    }

    fn rock() -> Prefab {
        let mut s = Scene::new("p");
        let e = s.spawn("Rock", None);
        s.add(
            e,
            MeshFilter {
                mesh: Some(MeshRef::Primitive(PrimitiveMesh::Sphere)),
            },
        );
        Prefab::from_scene(&s, e)
    }

    fn scene(d: TerrainData) -> (Scene, Entity, String) {
        let path = std::env::temp_dir()
            .join(format!(
                "misty_place_{}_{}.terrain",
                std::process::id(),
                d.size.x
            ))
            .to_string_lossy()
            .into_owned();
        super::super::store::insert(&path, d);
        let mut s = Scene::new("t");
        let e = s.spawn("Terrain", None);
        s.set_transform(
            e,
            crate::scene::Transform::from_position(Vec3::new(100.0, 5.0, 0.0)),
        );
        s.add(
            e,
            Terrain {
                terrain_data: path.clone(),
                ..Default::default()
            },
        );
        (s, e, path)
    }

    #[test]
    fn spots_keep_spacing_and_limits() {
        let d = ground();
        let p = ObjectPlacement {
            density: 50.0,
            spacing: 2.0,
            slope: [0.0, 30.0],
            ..Default::default()
        };
        let mut have: Vec<Vec2> = Vec::new();
        for step in 0..200 {
            for (pos, rot, scale) in spots(&d, Vec2::new(32.0, 32.0), 12.0, &have, &p, step) {
                assert!(pos.x <= 32.5, "nothing on the wall: {pos}");
                assert!((0.8..=1.3).contains(&scale));
                assert!((pos.y - d.sample_height(pos.x, pos.z)).abs() < 1e-4);
                // Flat ground (clear of the wall's foot): aligned objects stand upright.
                if pos.x < 30.0 {
                    assert!((rot * Vec3::Y).dot(Vec3::Y) > 0.999, "{pos}");
                }
                have.push(Vec2::new(pos.x, pos.z));
            }
        }
        assert!(have.len() > 20, "{}", have.len());
        for (i, a) in have.iter().enumerate() {
            for b in &have[i + 1..] {
                assert!(a.distance(*b) >= 2.0 - 1e-4);
            }
        }
        // A height band below the ground's height places nothing.
        let high = ObjectPlacement {
            height: [50.0, 60.0],
            ..p
        };
        assert!(spots(&d, Vec2::new(16.0, 32.0), 10.0, &[], &high, 0).is_empty());
        // Steep-only objects (cliff rocks) go on the wall, tilted with it.
        let steep = ObjectPlacement {
            slope: [50.0, 90.0],
            ..p
        };
        let on_wall = spots(&d, Vec2::new(48.0, 32.0), 10.0, &[], &steep, 3);
        assert!(!on_wall.is_empty());
        for (pos, rot, _) in on_wall {
            assert!(pos.x > 32.0);
            assert!((rot * Vec3::Y).y < 0.9, "tilted with the 60° wall");
        }
    }

    #[test]
    fn brush_places_and_erases_prefab_instances() {
        let (mut s, e, path) = scene(ground());
        let d = super::super::store::get(&path).unwrap().0;
        let p = ObjectPlacement {
            density: 20.0,
            spacing: 1.5,
            ..Default::default()
        };
        let prefab = rock();
        let mut total = 0;
        for step in 0..60 {
            total += place(
                &mut s,
                e,
                &d,
                &prefab,
                "Assets/Rock.prefab",
                Vec2::new(16.0, 16.0),
                8.0,
                &p,
                step,
            );
        }
        assert!(total > 15, "{total}");
        let h = holder(&mut s, e, false).expect("holder made");
        assert_eq!(s.children(h).len(), total);
        // Placed on the ground in world space (the terrain sits at y 5).
        for c in s.children(h) {
            let w = s.world_position(c);
            assert!((w.y - 5.0).abs() < 0.01, "{w}");
            assert!((w.x - 116.0).abs() <= 8.5);
            assert_eq!(s.prefab_link(c).as_deref(), Some("Assets/Rock.prefab"));
        }
        // The density is reached: more steps add nothing.
        assert_eq!(
            place(
                &mut s,
                e,
                &d,
                &prefab,
                "Assets/Rock.prefab",
                Vec2::new(16.0, 16.0),
                8.0,
                &p,
                999
            ),
            0
        );
        // Erasing another prefab removes none; erasing all clears the circle.
        assert_eq!(
            erase(
                &mut s,
                e,
                Vec2::new(16.0, 16.0),
                4.0,
                Some("Assets/Other.prefab")
            ),
            0
        );
        let gone = erase(&mut s, e, Vec2::new(16.0, 16.0), 4.0, None);
        assert!(gone > 0 && gone < total);
        assert_eq!(s.children(h).len(), total - gone);
        super::super::store::unload(&path);
    }
}
