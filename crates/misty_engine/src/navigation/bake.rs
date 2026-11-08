//! Bake entry points: collected geometry + an agent type → a tiled [`NavMesh`]. Tiles build in
//! parallel on the background pool; tiles whose input didn't change since the previous bake
//! are reused (partial rebake).

use std::sync::atomic::{AtomicUsize, Ordering};

use glam::{Vec2, Vec3Swizzles};
use rayon::prelude::*;

use super::geometry::{self, BuildInput};
use super::navmesh::{BuildSettings, NavMesh, Tile};
use super::tiles::TileBuilder;
use super::{AgentSettings, DEFAULT_TILE_SIZE, NavMeshSurface, NavigationSettings};
use crate::scene::Scene;

/// Most voxel columns a bake covers; bigger levels get coarser voxels.
const MAX_COLUMNS: f32 = 16_000_000.0;

/// Progress callback: (fraction done, stage).
pub type Progress<'a> = &'a (dyn Fn(f32, &'static str) + Sync);

/// Settings for baking `agent` with `cell` voxels and `tile_size` voxel tiles over `input`.
pub fn settings_for(
    input: &BuildInput,
    agent: AgentSettings,
    agent_type: u32,
    cell: f32,
    tile_size: u32,
    min_region_area: f32,
) -> BuildSettings {
    let mut cell = cell.clamp(0.01, 10.0);
    if let Some((lo, hi)) = input.geometry.bounds() {
        let (lo, hi) = match input.clip {
            Some((clo, chi)) => (lo.max(clo), hi.min(chi)),
            None => (lo, hi),
        };
        let ext = (hi - lo).xz().max(Vec2::splat(cell));
        let columns = (ext.x / cell) * (ext.y / cell);
        if columns > MAX_COLUMNS {
            cell *= (columns / MAX_COLUMNS).sqrt();
        }
    }
    BuildSettings {
        agent,
        agent_type,
        cell_size: cell,
        tile_size: if tile_size == 0 {
            DEFAULT_TILE_SIZE
        } else {
            tile_size.clamp(16, 1024)
        },
        origin: Vec2::ZERO,
        min_region_area,
    }
}

/// Settings of the Navigation window's bake (the Humanoid, Navigation Static objects).
pub fn scene_settings(input: &BuildInput, nav: &NavigationSettings) -> BuildSettings {
    settings_for(
        input,
        nav.agent,
        0,
        nav.cell_size(),
        nav.tile_size,
        nav.min_region_area.max(0.0),
    )
}

/// Settings of a NavMesh Surface's bake.
pub fn surface_settings(
    input: &BuildInput,
    nav: &NavigationSettings,
    s: &NavMeshSurface,
) -> BuildSettings {
    let agent = nav.agent_settings(s.agent_type).unwrap_or(nav.agent);
    let cell = if s.override_voxel_size {
        s.voxel_size
    } else {
        agent.radius / 3.0
    };
    let tiles = if s.override_tile_size { s.tile_size } else { 0 };
    settings_for(
        input,
        agent,
        s.agent_type,
        cell,
        tiles,
        s.min_region_area.max(0.0),
    )
}

/// Bake `scene`'s Navigation Static geometry for the Humanoid (on the calling thread, tiles in
/// parallel).
pub fn bake_scene(scene: &Scene) -> NavMesh {
    let input = geometry::collect_static(scene);
    let settings = scene_settings(&input, &scene.navigation);
    bake(input, settings, None, &|_, _| {})
}

/// Bake the NavMesh Surface on `e`.
pub fn bake_surface(scene: &Scene, e: hecs::Entity) -> Option<NavMesh> {
    let s = scene.world.get::<&NavMeshSurface>(e).ok()?.clone();
    let input = geometry::collect_surface(scene, e, &s);
    let settings = surface_settings(&input, &scene.navigation, &s);
    Some(bake(input, settings, None, &|_, _| {}))
}

/// Bake prepared input. Tiles of `previous` with the same settings and input hash are kept.
pub fn bake(
    input: BuildInput,
    settings: BuildSettings,
    previous: Option<&NavMesh>,
    progress: Progress,
) -> NavMesh {
    progress(0.02, "Indexing geometry");
    let builder = TileBuilder::new(input, settings);
    let all = builder.tiles();
    let reuse = previous.filter(|p| p.settings == settings);
    let mut kept: Vec<Option<Tile>> = all
        .iter()
        .map(|&(x, z)| {
            let p = reuse?;
            let t = &p.tiles()[p.tile_index(x, z)? as usize];
            (t.hash == builder.hash(x, z, &[])).then(|| {
                let mut t = (**t).clone();
                t.polys.iter_mut().for_each(|p| p.links.clear());
                t
            })
        })
        .collect();
    let todo: Vec<usize> = (0..all.len()).filter(|&i| kept[i].is_none()).collect();
    let done = AtomicUsize::new(0);
    let total = todo.len().max(1);
    progress(0.05, "Building tiles");
    let built: Vec<(usize, Tile)> = crate::background::pool().install(|| {
        todo.par_iter()
            .map(|&i| {
                crate::profile_scope!("Navigation.BakeTile", crate::profiler::Category::Navigation);
                let (x, z) = all[i];
                let t = builder.build(x, z, &[]);
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                progress(0.05 + 0.9 * n as f32 / total as f32, "Building tiles");
                (i, t)
            })
            .collect()
    });
    for (i, t) in built {
        kept[i] = Some(t);
    }
    progress(0.97, "Linking tiles");
    let tiles = kept.into_iter().flatten().collect();
    let mesh = NavMesh::from_tiles(settings, tiles);
    progress(1.0, "Done");
    mesh
}

/// How many tiles a bake reused and built (status lines, tests).
pub fn reused_tiles(previous: &NavMesh, now: &NavMesh) -> usize {
    now.tiles()
        .iter()
        .filter(|t| {
            previous
                .tile_index(t.x, t.z)
                .is_some_and(|i| previous.tiles()[i as usize].hash == t.hash)
        })
        .count()
}
