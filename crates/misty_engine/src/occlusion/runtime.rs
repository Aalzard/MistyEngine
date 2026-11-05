//! Using baked data: match the scene's occludees to the baked ones, find the camera's cell and
//! mark the draws of occludees outside the cell's potentially visible set.

use std::collections::HashMap;
use std::sync::Arc;

use glam::Vec3;
use hecs::Entity;

use super::{CellLookup, OcclusionData, OcclusionMask};
use crate::render::culling::Aabb;
use crate::scene::Scene;

/// Scene occludee → baked occludee index. Objects that moved, changed or were added since the
/// bake have no entry and are always drawn.
#[derive(Clone, Debug, Default)]
pub struct OccludeeMap {
    pub map: HashMap<Entity, u32>,
    /// Occludees whose box no longer matches the bake.
    pub stale: usize,
}

fn same_box(a: &Aabb, b: &Aabb) -> bool {
    let tol = 1e-3 * (1.0 + (a.max - a.min).max_element());
    a.min.abs_diff_eq(b.min, tol) && a.max.abs_diff_eq(b.max, tol)
}

impl OccludeeMap {
    pub fn build(scene: &Scene, data: &OcclusionData) -> Self {
        let mut out = Self::default();
        let list = super::bake::occludees(scene);
        for (i, (e, b)) in list.iter().enumerate() {
            match data.occludees.get(i) {
                Some(baked) if same_box(baked, b) => {
                    out.map.insert(*e, i as u32);
                }
                _ => out.stale += 1,
            }
        }
        out
    }
}

/// One view's occlusion result.
#[derive(Clone, Debug)]
pub struct OcclusionResult {
    pub mask: OcclusionMask,
    pub lookup: CellLookup,
    /// World box of the camera's cell.
    pub cell_bounds: Option<Aabb>,
    /// Baked occludees matched in the scene, and how many of them the cell hides.
    pub occludees: usize,
    pub culled: usize,
    /// Occludees changed since the bake (always drawn).
    pub stale: usize,
}

/// Hide the draws (by owner) of occludees outside the PVS of `eye`'s cell.
pub fn compute(
    data: &OcclusionData,
    map: &OccludeeMap,
    owners: &[Option<Entity>],
    eye: Vec3,
    dynamic: bool,
) -> OcclusionResult {
    let lookup = data.lookup(eye);
    let mut hidden = vec![false; owners.len()];
    let mut culled = 0;
    if let CellLookup::Cell { set, .. } = lookup {
        let mut counted: HashMap<Entity, bool> = HashMap::new();
        for (h, owner) in hidden.iter_mut().zip(owners) {
            let Some(e) = owner else { continue };
            let Some(&i) = map.map.get(e) else { continue };
            *h = !data.visible(set, i as usize);
            if *h && counted.insert(*e, true).is_none() {
                culled += 1;
            }
        }
    }
    OcclusionResult {
        mask: OcclusionMask {
            hidden: Arc::new(hidden),
            dynamic,
        },
        cell_bounds: lookup.cell().map(|c| data.cell_bounds(c)),
        lookup,
        occludees: map.map.len(),
        culled,
        stale: map.stale,
    }
}
