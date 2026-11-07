//! Water from Water Body stamps and rivers: lake flood fill, the wet-texel map used for
//! classification, and the water objects placed in the scene.
//!
//! [`spawn_water`] is the only place that builds the scene object: a Water Surface (the ocean
//! shader) of the matching kind.

use std::collections::VecDeque;

use glam::{Quat, Vec2, Vec3};

use crate::water::WaterKind;

use super::classify::WaterMap;
use super::stamps::{PlacedStamp, RiverPath, StampKind, WaterMode};
use crate::hecs::Entity;
use crate::scene::{Scene, WaterSurface};

/// Material the water planes use.
pub const MATERIAL: &str = "Assets/WorldBuilder/Materials/Water.mat";

/// One water surface (terrain-local).
#[derive(Clone, Debug, PartialEq)]
pub struct WaterBody {
    pub name: String,
    /// Surface height (metres above the terrain's corner).
    pub level: f32,
    /// Centre and size of the plane on x/z.
    pub center: Vec2,
    pub size: Vec2,
    /// Direction of the plane's local X axis (rivers follow their flow).
    pub axis: Vec2,
    /// Sea = the endless Ocean, lakes and river strips = bounded surfaces.
    pub kind: WaterKind,
}

/// Cells of a heightmap (metres) below `level` connected to `seed` (4-neighbour flood fill).
/// `None` when the seed itself is above the level.
pub fn flood_fill(h: &[f32], res: usize, seed: (usize, usize), level: f32) -> Option<Vec<bool>> {
    let (sx, sz) = (seed.0.min(res - 1), seed.1.min(res - 1));
    if h[sz * res + sx] >= level {
        return None;
    }
    let mut wet = vec![false; res * res];
    let mut queue = VecDeque::from([(sx, sz)]);
    wet[sz * res + sx] = true;
    while let Some((x, z)) = queue.pop_front() {
        let mut visit = |nx: usize, nz: usize| {
            let i = nz * res + nx;
            if !wet[i] && h[i] < level {
                wet[i] = true;
                queue.push_back((nx, nz));
            }
        };
        if x > 0 {
            visit(x - 1, z);
        }
        if x + 1 < res {
            visit(x + 1, z);
        }
        if z > 0 {
            visit(x, z - 1);
        }
        if z + 1 < res {
            visit(x, z + 1);
        }
    }
    Some(wet)
}

/// Water bodies and the wet map for a heightmap (metres, `res²`, `cell` apart).
pub fn bodies(
    h: &[f32],
    res: usize,
    cell: Vec2,
    stamps: &[PlacedStamp],
    rivers: &[RiverPath],
    splat_res: usize,
) -> (Vec<WaterBody>, WaterMap) {
    let size = cell * (res - 1) as f32;
    let mut map = WaterMap::dry(splat_res);
    let mut out = Vec::new();
    // Splat texel → heightmap sample.
    let sample_of = |tx: usize, tz: usize| {
        let fx = ((tx as f32 + 0.5) / splat_res as f32 * (res - 1) as f32).round() as usize;
        let fz = ((tz as f32 + 0.5) / splat_res as f32 * (res - 1) as f32).round() as usize;
        fz.min(res - 1) * res + fx.min(res - 1)
    };
    let mut wet_texels = |mask: &dyn Fn(usize) -> bool, level: f32| {
        for tz in 0..splat_res {
            for tx in 0..splat_res {
                let s = sample_of(tx, tz);
                if mask(s) && h[s] < level {
                    let l = &mut map.level[tz * splat_res + tx];
                    *l = Some(l.map_or(level, |o: f32| o.max(level)));
                }
            }
        }
    };
    for s in stamps {
        let StampKind::Water(w) = &s.stamp.kind else {
            continue;
        };
        if !s.stamp.enabled {
            continue;
        }
        let level = s.at.y;
        match w.mode {
            WaterMode::Sea => {
                wet_texels(&|_| true, level);
                out.push(WaterBody {
                    name: "Sea".into(),
                    level,
                    center: size * 0.5,
                    // Past the coast so the horizon is water.
                    size: size * 8.0,
                    axis: Vec2::X,
                    kind: WaterKind::Ocean,
                });
            }
            WaterMode::Lake => {
                let ix = (s.at.center.x / cell.x)
                    .round()
                    .clamp(0.0, (res - 1) as f32) as usize;
                let iz = (s.at.center.y / cell.y)
                    .round()
                    .clamp(0.0, (res - 1) as f32) as usize;
                let Some(wet) = flood_fill(h, res, (ix, iz), level) else {
                    continue;
                };
                let (mut lo, mut hi) = ((res, res), (0, 0));
                for (i, w) in wet.iter().enumerate() {
                    if *w {
                        let (x, z) = (i % res, i / res);
                        lo = (lo.0.min(x), lo.1.min(z));
                        hi = (hi.0.max(x), hi.1.max(z));
                    }
                }
                wet_texels(&|i| wet[i], level);
                // One cell of margin so the shore line meets the plane under the ground.
                let a = Vec2::new(lo.0 as f32 - 1.0, lo.1 as f32 - 1.0) * cell;
                let b = Vec2::new(hi.0 as f32 + 1.0, hi.1 as f32 + 1.0) * cell;
                out.push(WaterBody {
                    name: "Lake".into(),
                    level,
                    center: (a + b) * 0.5,
                    size: (b - a).max(Vec2::splat(1.0)),
                    axis: Vec2::X,
                    kind: WaterKind::Lake,
                });
            }
        }
    }
    for r in rivers {
        river_water(r, &mut out, &mut map, size);
    }
    (out, map)
}

/// Water strips along a river (one per pair of line samples) and its wet texels.
fn river_water(r: &RiverPath, out: &mut Vec<WaterBody>, map: &mut WaterMap, size: Vec2) {
    let n = map.res;
    let half = r.width * 0.5;
    let step = 2;
    let mut i = 0;
    while i + 1 < r.points.len() {
        let j = (i + step).min(r.points.len() - 1);
        let (a, b) = (r.points[i], r.points[j]);
        let level = (r.water[i] + r.water[j]) * 0.5;
        let axis = (b - a).try_normalize().unwrap_or(Vec2::X);
        out.push(WaterBody {
            name: "River".into(),
            level,
            center: (a + b) * 0.5,
            // Overlap neighbours a little so there are no gaps at bends.
            size: Vec2::new(a.distance(b) + half, r.width * 1.1),
            axis,
            kind: WaterKind::River,
        });
        i = j;
    }
    // Wet texels: within half the width of the line (only rows and columns near it).
    if r.points.is_empty() || n == 0 {
        return;
    }
    let lo = r.points.iter().fold(Vec2::MAX, |a, p| a.min(*p)) - Vec2::splat(half);
    let hi = r.points.iter().fold(Vec2::MIN, |a, p| a.max(*p)) + Vec2::splat(half);
    let texel = |v: f32, extent: f32| ((v / extent * n as f32).floor().max(0.0) as usize).min(n);
    let (x0, x1) = (texel(lo.x, size.x), (texel(hi.x, size.x) + 1).min(n));
    let (z0, z1) = (texel(lo.y, size.y), (texel(hi.y, size.y) + 1).min(n));
    if x0 >= x1 || z0 >= z1 {
        return;
    }
    super::par_rows(&mut map.level[z0 * n..z1 * n], n, |row, out| {
        let tz = z0 + row;
        for (tx, cell) in out.iter_mut().enumerate().take(x1).skip(x0) {
            let p = Vec2::new(
                (tx as f32 + 0.5) / n as f32 * size.x,
                (tz as f32 + 0.5) / n as f32 * size.y,
            );
            for k in 0..r.points.len().saturating_sub(1) {
                let (d, t) = super::stamps::segment_distance(p, r.points[k], r.points[k + 1]);
                if d < half {
                    *cell = Some(r.water[k] + (r.water[k + 1] - r.water[k]) * t);
                    break;
                }
            }
        }
    });
}

/// Place a Water Surface for `body` under `parent` (`corner` = the terrain's world corner): the
/// sea becomes the Ocean, a lake a Lake rectangle, a river strip a River flowing along its axis.
pub fn spawn_water(
    scene: &mut Scene,
    parent: Option<Entity>,
    body: &WaterBody,
    corner: Vec3,
) -> Entity {
    let world = corner + Vec3::new(body.center.x, body.level, body.center.y);
    let e = crate::water::spawn_water(scene, &body.name, body.kind, Vec3::ZERO, body.size);
    if parent.is_some() {
        scene.set_parent(e, parent, None, false);
    }
    // Lakes keep their local X on `axis`; rivers flow along local +Z.
    let (yaw, size) = match body.kind {
        WaterKind::River => (
            super::portable::atan2(body.axis.x, body.axis.y),
            Vec2::new(body.size.y, body.size.x),
        ),
        _ => (super::portable::atan2(-body.axis.y, body.axis.x), body.size),
    };
    let mut t = scene.transform(e);
    t.rotation = Quat::from_rotation_y(yaw);
    scene.set_transform(e, t);
    scene.set_world_position(e, world);
    if body.kind != WaterKind::Ocean
        && let Ok(mut w) = scene.world.get::<&mut WaterSurface>(e)
    {
        w.size = size.to_array();
    }
    e
}

/// The water material: transparent, glossy blue.
pub fn material() -> crate::assets::Material {
    crate::assets::Material {
        color: [0.16, 0.42, 0.55, 0.62],
        smoothness: 0.92,
        rendering_mode: crate::render::surface::RenderingMode::Transparent,
        ..Default::default()
    }
}
