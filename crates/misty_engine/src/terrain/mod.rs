//! Terrain: the `Terrain` and `Terrain Collider` components, the `.terrain` data asset
//! (`data`, `file`), the shared data store, mesh generation, brushes and the heightfield collider.
//!
//! Like the reference engine, a terrain only uses its object's world position: rotation and scale
//! are ignored by the renderer, the collider and height sampling.

pub mod auto_paint;
pub mod brush;
pub mod cliffs;
mod data;
pub mod details;
mod file;
pub mod harmony;
pub mod mesh;
pub mod noise;
pub mod store;
#[cfg(test)]
mod tests;
pub mod trees;

use std::sync::Arc;

use glam::Vec3;
use hecs::Entity;
use rapier3d::prelude as rp;
use serde::{Deserialize, Serialize};

pub use data::{
    ALPHAMAP_RESOLUTIONS, HEIGHTMAP_RESOLUTIONS, MAX_LAYERS, TerrainData, TerrainLayer,
};
pub use file::EXTENSION;

use crate::scene::{PhysicsSurface, Scene};

/// Draws a terrain data asset at the object's position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Terrain {
    pub enabled: bool,
    /// Project-relative `.terrain` asset.
    pub terrain_data: String,
    pub cast_shadows: bool,
}

impl Default for Terrain {
    fn default() -> Self {
        Self {
            enabled: true,
            terrain_data: String::new(),
            cast_shadows: true,
        }
    }
}

/// Static heightfield collider for a terrain data asset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerrainCollider {
    pub enabled: bool,
    /// Empty = the Terrain on the same object.
    pub terrain_data: String,
    pub surface: PhysicsSurface,
}

impl Default for TerrainCollider {
    fn default() -> Self {
        Self {
            enabled: true,
            terrain_data: String::new(),
            surface: PhysicsSurface::default(),
        }
    }
}

/// A terrain ready to draw.
#[derive(Clone, Debug)]
pub struct TerrainDraw {
    /// Asset path (GPU data is cached per asset).
    pub key: String,
    /// Store version of `data`; the renderer rebuilds its mesh when it changes.
    pub version: u64,
    pub data: Arc<TerrainData>,
    /// World position of the terrain's corner.
    pub position: Vec3,
    pub cast_shadows: bool,
}

/// Enabled terrains of active objects whose data loads.
pub fn draws(scene: &Scene) -> Vec<TerrainDraw> {
    draws_where(scene, |_| true)
}

/// [`draws`] of the terrains `keep` accepts (a camera's culling mask).
pub fn draws_where(scene: &Scene, keep: impl Fn(Entity) -> bool) -> Vec<TerrainDraw> {
    terrains(scene)
        .into_iter()
        .filter(|(e, _)| keep(*e))
        .filter_map(|(e, t)| {
            let (data, version) = store::get(&t.terrain_data)?;
            Some(TerrainDraw {
                key: t.terrain_data,
                version,
                data,
                position: scene.world_position(e),
                cast_shadows: t.cast_shadows,
            })
        })
        .collect()
}

/// Enabled Terrain components of active objects, in Hierarchy order.
pub fn terrains(scene: &Scene) -> Vec<(Entity, Terrain)> {
    scene
        .active_with::<Terrain>()
        .into_iter()
        .filter_map(|e| {
            let t = scene.world.get::<&Terrain>(e).ok()?;
            t.enabled.then(|| (e, (*t).clone()))
        })
        .collect()
}

/// The first enabled terrain (the scripting API's `Terrain.activeTerrain`).
pub fn active_terrain(scene: &Scene) -> Option<Entity> {
    terrains(scene).first().map(|(e, _)| *e)
}

/// Data asset drawn by the object's Terrain.
pub fn data_of(scene: &Scene, e: Entity) -> Option<Arc<TerrainData>> {
    let path = scene.world.get::<&Terrain>(e).ok()?.terrain_data.clone();
    store::get(&path).map(|(d, _)| d)
}

/// Terrain height under a world point, relative to the terrain's position (like
/// `Terrain.SampleHeight`); points outside use the nearest edge.
pub fn sample_height(scene: &Scene, e: Entity, world: Vec3) -> Option<f32> {
    let d = data_of(scene, e)?;
    let local = world - scene.world_position(e);
    Some(d.sample_height(local.x, local.z))
}

/// First hit of a world ray on any terrain: (object, point).
pub fn raycast(scene: &Scene, origin: Vec3, dir: Vec3) -> Option<(Entity, Vec3)> {
    terrains(scene)
        .into_iter()
        .filter_map(|(e, t)| {
            let (d, _) = store::get(&t.terrain_data)?;
            let pos = scene.world_position(e);
            raycast_data(&d, origin - pos, dir).map(|p| (e, p + pos))
        })
        .min_by(|a, b| a.1.distance(origin).total_cmp(&b.1.distance(origin)))
}

/// Ray (terrain-local) against the height surface: marched in half-cell steps inside the
/// bounds, then refined by bisection.
pub fn raycast_data(d: &TerrainData, origin: Vec3, dir: Vec3) -> Option<Vec3> {
    let dir = dir.normalize_or_zero();
    let (lo, hi) = d.bounds();
    let (t0, t1) = ray_box(origin, dir, lo - Vec3::splat(0.01), hi + Vec3::splat(0.01))?;
    let above = |t: f32| {
        let p = origin + dir * t;
        p.y - d.sample_height(p.x, p.z)
    };
    let step = (d.spacing().min_element() * 0.5).max(1e-3);
    let mut prev = t0.max(0.0);
    if above(prev) < 0.0 {
        // Started under the surface: no hit from below.
        return None;
    }
    let mut t = prev;
    while t < t1 {
        t = (t + step).min(t1);
        if above(t) <= 0.0 {
            let (mut a, mut b) = (prev, t);
            for _ in 0..24 {
                let m = (a + b) * 0.5;
                if above(m) > 0.0 { a = m } else { b = m }
            }
            return Some(origin + dir * b);
        }
        prev = t;
    }
    None
}

/// Slab test: entry and exit distances of a ray through a box.
fn ray_box(o: Vec3, d: Vec3, lo: Vec3, hi: Vec3) -> Option<(f32, f32)> {
    let inv = d.recip();
    let a = (lo - o) * inv;
    let b = (hi - o) * inv;
    let near = a.min(b).max_element();
    let far = a.max(b).min_element();
    (far >= near.max(0.0)).then_some((near, far))
}

/// rapier heightfield collider for `e`'s Terrain Collider, in world space (static).
pub fn collider(scene: &Scene, e: Entity) -> Option<rp::ColliderBuilder> {
    let c = (*scene.world.get::<&TerrainCollider>(e).ok()?).clone();
    if !c.enabled {
        return None;
    }
    let path = if c.terrain_data.is_empty() {
        scene.world.get::<&Terrain>(e).ok()?.terrain_data.clone()
    } else {
        c.terrain_data
    };
    let (d, _) = store::get(&path)?;
    let pos = scene.world_position(e);
    Some(
        heightfield(&d)
            .translation(rp::Vector::new(
                pos.x + d.size.x * 0.5,
                pos.y,
                pos.z + d.size.z * 0.5,
            ))
            .friction(c.surface.friction)
            .restitution(c.surface.bounciness),
    )
}

/// Trunk colliders of the terrain's trees (static capsules from the foot up to where the crown
/// starts, as thick as the trunk), placed in world space with the Terrain Collider; the player
/// can't walk through trees.
pub fn tree_colliders(scene: &Scene, e: Entity) -> Vec<rp::ColliderBuilder> {
    let Ok(c) = scene.world.get::<&TerrainCollider>(e).map(|c| (*c).clone()) else {
        return Vec::new();
    };
    if !c.enabled {
        return Vec::new();
    }
    let path = if c.terrain_data.is_empty() {
        match scene.world.get::<&Terrain>(e) {
            Ok(t) => t.terrain_data.clone(),
            Err(_) => return Vec::new(),
        }
    } else {
        c.terrain_data
    };
    let Some((d, _)) = store::get(&path) else {
        return Vec::new();
    };
    let pos = scene.world_position(e);
    trees::trunks(&d)
        .into_iter()
        .map(|t| {
            let half = (t.height * 0.5 - t.radius).max(0.05);
            let center = pos + t.foot + Vec3::Y * t.height * 0.5;
            rp::ColliderBuilder::capsule_y(half, t.radius)
                .translation(rp::Vector::new(center.x, center.y, center.z))
                .friction(c.surface.friction)
                .restitution(c.surface.bounciness)
        })
        .collect()
}

/// Heightfield shape centred on x/z (rapier's convention): rows along z, columns along x.
pub fn heightfield(d: &TerrainData) -> rp::ColliderBuilder {
    let r = d.heightmap_resolution;
    let heights = rp::Array2::from_fn(r, r, |iz, ix| d.height(ix, iz));
    rp::ColliderBuilder::heightfield(heights, rp::Vector::new(d.size.x, d.size.y, d.size.z))
}
