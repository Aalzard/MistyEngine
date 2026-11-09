//! Terrain tiles: a large terrain made of equal tile terrains, one per World Partition cell,
//! that stream like any other object.
//!
//! Neighbouring tiles share their edge samples (the same heights on both sides), and each tile
//! keeps the samples one step past its edges ([`TerrainData::border`]) so the normals along an
//! edge are the same on both tiles. Together with the renderer stitching chunk LODs across tile
//! edges (a tile next to a coarser neighbour snaps its edge vertices onto the coarse grid, as
//! chunks inside one terrain do), there are no seams: no cracks and no lighting lines.

use std::collections::HashMap;
use std::path::Path;

use glam::{Vec2, Vec3};
use hecs::Entity;

use super::{CellCoord, WorldPartition};
use crate::scene::{Scene, Terrain, TerrainCollider, Transform};
use crate::terrain::TerrainData;

/// One tile of a tiled terrain.
#[derive(Clone, Debug)]
pub struct Tile {
    pub coord: CellCoord,
    /// World position of the tile's corner (the terrain object's position).
    pub corner: Vec3,
    pub data: TerrainData,
}

/// Tiles `coords` of a grid of `tile_size`-metre tiles whose (0, 0) corner is at `origin`
/// (x/z), sampled from a world height function (`height(x, z)` in metres, between `base_y`
/// and `base_y + template.size.y`), `res` samples per side. Layers, grass, trees settings and
/// the rest come from `template`; each tile's splat map is left to the caller.
pub fn from_fn(
    template: &TerrainData,
    origin: Vec2,
    base_y: f32,
    tile_size: f32,
    res: usize,
    coords: &[CellCoord],
    height: impl Fn(f32, f32) -> f32 + Sync,
) -> Vec<Tile> {
    let max_h = template.size.y.max(1.0);
    coords
        .iter()
        .map(|&c| {
            let corner = Vec2::new(
                origin.x + c.0 as f32 * tile_size,
                origin.y + c.1 as f32 * tile_size,
            );
            let mut d = TerrainData::new(
                res,
                Vec3::new(tile_size, max_h, tile_size),
                template.alphamap_resolution,
            );
            copy_settings(template, &mut d);
            let r = d.heightmap_resolution;
            let step = tile_size / (r - 1) as f32;
            let norm = |x: f32, z: f32| (height(x, z) - base_y) / max_h;
            for iz in 0..r {
                for ix in 0..r {
                    let (x, z) = (corner.x + ix as f32 * step, corner.y + iz as f32 * step);
                    d.set_height(ix, iz, norm(x, z));
                }
            }
            let mut border = Vec::with_capacity(4 * r);
            for iz in 0..r {
                border.push(norm(corner.x - step, corner.y + iz as f32 * step));
            }
            for iz in 0..r {
                border.push(norm(
                    corner.x + tile_size + step,
                    corner.y + iz as f32 * step,
                ));
            }
            for ix in 0..r {
                border.push(norm(corner.x + ix as f32 * step, corner.y - step));
            }
            for ix in 0..r {
                border.push(norm(
                    corner.x + ix as f32 * step,
                    corner.y + tile_size + step,
                ));
            }
            d.border = border;
            Tile {
                coord: c,
                corner: Vec3::new(corner.x, base_y, corner.y),
                data: d,
            }
        })
        .collect()
}

/// Settings every tile shares with the terrain it was made from (not the arrays).
fn copy_settings(from: &TerrainData, to: &mut TerrainData) {
    to.layers = from.layers.clone();
    to.rule_seed = from.rule_seed;
    to.cliffs = from.cliffs.clone();
    to.harmony = from.harmony.clone();
    to.details = from.details.clone();
    to.flowers = from.flowers.clone();
    to.trees.prototypes = from.trees.prototypes.clone();
    to.trees.draw_distance = from.trees.draw_distance;
    to.trees.lod_distance = from.trees.lod_distance;
    to.trees.billboard_start = from.trees.billboard_start;
    to.trees.cast_shadows = from.trees.cast_shadows;
    to.detail_layers = from
        .detail_layers
        .iter()
        .map(|l| {
            let mut l = l.clone();
            l.map.clear();
            l
        })
        .collect();
    to.fit_splat_maps();
}

/// Split a terrain (its object at world `corner`) into `n × n` tiles of `res` samples. Heights,
/// splat maps, grass density, holes and trees are carried over; tiles are stitched.
pub fn split(data: &TerrainData, corner: Vec3, n: usize, res: usize) -> Vec<Tile> {
    let n = n.max(1);
    let tile = data.size.x / n as f32;
    let coords: Vec<CellCoord> = (0..n as i32)
        .flat_map(|z| (0..n as i32).map(move |x| (x, z)))
        .collect();
    let max_h = data.size.y;
    let mut tiles = from_fn(
        data,
        Vec2::new(corner.x, corner.z),
        corner.y,
        tile,
        res,
        &coords,
        |x, z| {
            // Past the big terrain's edge: its edge (the border of outer tiles).
            corner.y + data.sample_normalized(x - corner.x, z - corner.z) * max_h
        },
    );
    let a = data.alphamap_resolution;
    for t in &mut tiles {
        let ta = t.data.alphamap_resolution;
        let local = Vec2::new(t.corner.x - corner.x, t.corner.z - corner.z);
        // Nearest texel of the big map under texel (x, z) of the tile's map.
        let src = |x: usize, z: usize| {
            let p = local
                + Vec2::new((x as f32 + 0.5) / ta as f32, (z as f32 + 0.5) / ta as f32) * tile;
            let sx = ((p.x / data.size.x * a as f32) as usize).min(a - 1);
            let sz = ((p.y / data.size.z * a as f32) as usize).min(a - 1);
            sz * a + sx
        };
        let maps = data.splat_maps();
        for z in 0..ta {
            for x in 0..ta {
                let s = src(x, z);
                let w = data.layer_weights(s);
                t.data.set_layer_weights(z * ta + x, &w);
                if data.detail_map.len() == a * a {
                    if t.data.detail_map.len() != ta * ta {
                        t.data.detail_map = vec![0; ta * ta];
                    }
                    t.data.detail_map[z * ta + x] = data.detail_map[s];
                }
                for (i, l) in data.detail_layers.iter().enumerate() {
                    if l.map.len() == a * a {
                        let m = &mut t.data.detail_layers[i].map;
                        if m.len() != ta * ta {
                            *m = vec![0; ta * ta];
                        }
                        m[z * ta + x] = l.map[s];
                    }
                }
            }
        }
        let _ = maps;
        // Holes: the big terrain's cell under each tile cell's middle.
        if data.has_holes() {
            let cells = t.data.cells();
            let step = tile / cells as f32;
            for cz in 0..cells {
                for cx in 0..cells {
                    let p = local + Vec2::new((cx as f32 + 0.5) * step, (cz as f32 + 0.5) * step);
                    if data.hole_at(p.x, p.y) {
                        t.data.set_hole(cx, cz, true);
                    }
                }
            }
        }
        // Trees standing on the tile, in its own 0..1 coordinates.
        t.data.trees.instances = data
            .trees
            .instances
            .iter()
            .filter_map(|i| {
                let p = i.position * Vec2::new(data.size.x, data.size.z) - local;
                let inside = p.x >= 0.0 && p.y >= 0.0 && p.x < tile && p.y < tile;
                inside.then(|| crate::terrain::trees::TreeInstance {
                    position: p / tile,
                    ..*i
                })
            })
            .collect();
    }
    stitch(&mut tiles);
    tiles
}

/// Make neighbouring tiles meet: shared edge samples get the same height (the mean of both
/// sides), and each tile's border holds its neighbours' samples one step in (or continues its
/// own slope where it has no neighbour). Tiles must share a size and resolution.
pub fn stitch(tiles: &mut [Tile]) {
    let index: HashMap<CellCoord, usize> = tiles
        .iter()
        .enumerate()
        .map(|(i, t)| (t.coord, i))
        .collect();
    // Shared edges first.
    for i in 0..tiles.len() {
        let c = tiles[i].coord;
        for (dx, dz) in [(1, 0), (0, 1)] {
            let Some(&j) = index.get(&(c.0 + dx, c.1 + dz)) else {
                continue;
            };
            let r = tiles[i].data.heightmap_resolution;
            if tiles[j].data.heightmap_resolution != r {
                continue;
            }
            for k in 0..r {
                let (a, b) = if dx == 1 {
                    ((r - 1, k), (0, k))
                } else {
                    ((k, r - 1), (k, 0))
                };
                let mean = (tiles[i].data.height(a.0, a.1) + tiles[j].data.height(b.0, b.1)) * 0.5;
                tiles[i].data.set_height(a.0, a.1, mean);
                tiles[j].data.set_height(b.0, b.1, mean);
            }
        }
    }
    // Then the borders, from the (now matching) neighbours.
    let borders: Vec<Vec<f32>> = (0..tiles.len())
        .map(|i| {
            let t = &tiles[i].data;
            let r = t.heightmap_resolution;
            let c = tiles[i].coord;
            let neighbour = |dx: i32, dz: i32| {
                index
                    .get(&(c.0 + dx, c.1 + dz))
                    .map(|&j| &tiles[j].data)
                    .filter(|n| n.heightmap_resolution == r)
            };
            let mut b = Vec::with_capacity(4 * r);
            // x = -1: the left neighbour's column r - 2.
            for iz in 0..r {
                b.push(match neighbour(-1, 0) {
                    Some(n) => n.height(r - 2, iz),
                    None => 2.0 * t.height(0, iz) - t.height(1, iz),
                });
            }
            for iz in 0..r {
                b.push(match neighbour(1, 0) {
                    Some(n) => n.height(1, iz),
                    None => 2.0 * t.height(r - 1, iz) - t.height(r - 2, iz),
                });
            }
            for ix in 0..r {
                b.push(match neighbour(0, -1) {
                    Some(n) => n.height(ix, r - 2),
                    None => 2.0 * t.height(ix, 0) - t.height(ix, 1),
                });
            }
            for ix in 0..r {
                b.push(match neighbour(0, 1) {
                    Some(n) => n.height(ix, 1),
                    None => 2.0 * t.height(ix, r - 1) - t.height(ix, r - 2),
                });
            }
            b
        })
        .collect();
    for (t, b) in tiles.iter_mut().zip(borders) {
        t.data.border = b.into_iter().map(|h| h.clamp(0.0, 1.0)).collect();
    }
}

/// Largest mismatch along the edge two neighbouring tiles share: (height in metres, angle
/// between the normals in degrees). `b` is right of (`+x`) or in front of (`+z`) `a`.
pub fn seam_error(a: &Tile, b: &Tile) -> (f32, f32) {
    let (da, db) = (&a.data, &b.data);
    let r = da.heightmap_resolution;
    let along_x = b.coord.0 == a.coord.0 + 1;
    let mut worst = (0.0f32, 0.0f32);
    for k in 0..r {
        let ((ax, az), (bx, bz)) = if along_x {
            ((r - 1, k), (0, k))
        } else {
            ((k, r - 1), (k, 0))
        };
        let ha = a.corner.y + da.height(ax, az) * da.size.y;
        let hb = b.corner.y + db.height(bx, bz) * db.size.y;
        let na = da.vertex_normal(ax, az);
        let nb = db.vertex_normal(bx, bz);
        worst.0 = worst.0.max((ha - hb).abs());
        worst.1 = worst.1.max(na.angle_between(nb).to_degrees());
    }
    worst
}

/// Project-relative path of a tile's terrain data.
pub fn tile_path(wp: &WorldPartition, c: CellCoord) -> String {
    format!("{}/Terrain/Tile_{}_{}.terrain", wp.folder, c.0, c.1)
}

/// Add a top-level object per tile (Terrain and Terrain Collider) named "Terrain Tile x z".
pub fn spawn_tiles(scene: &mut Scene, wp: &WorldPartition, tiles: &[Tile]) -> Vec<Entity> {
    tiles
        .iter()
        .map(|t| {
            let e = scene.spawn(&format!("Terrain Tile {} {}", t.coord.0, t.coord.1), None);
            scene.set_transform(e, Transform::from_position(t.corner));
            scene.add(
                e,
                Terrain {
                    terrain_data: tile_path(wp, t.coord),
                    ..Default::default()
                },
            );
            scene.add(e, TerrainCollider::default());
            e
        })
        .collect()
}

/// Editor action: split the terrain on `e` into tiles of the partition's cell size (the
/// terrain must be a whole number of cells wide and start on a cell corner), write their
/// data under `<folder>/Terrain` in `root`, and replace the object with one per tile.
pub fn split_object(
    scene: &mut Scene,
    e: Entity,
    wp: &WorldPartition,
    res: usize,
    root: &Path,
) -> Result<Vec<Entity>, String> {
    let path = scene
        .world
        .get::<&Terrain>(e)
        .map(|t| t.terrain_data.clone())
        .map_err(|_| "the object has no Terrain".to_string())?;
    let (data, _) =
        crate::terrain::store::get(&path).ok_or_else(|| format!("{path} doesn't load"))?;
    let corner = scene.world_position(e);
    let size = wp.cell_size();
    let n = (data.size.x / size).round();
    if n < 1.0 || (data.size.x - n * size).abs() > 0.01 || (data.size.z - data.size.x).abs() > 0.01
    {
        return Err(format!(
            "the terrain ({} × {} m) isn't a whole number of {size} m cells",
            data.size.x, data.size.z
        ));
    }
    let first = wp.cell_of(corner + Vec3::new(size * 0.5, 0.0, size * 0.5));
    let mut tiles = split(&data, corner, n as usize, res);
    for t in &mut tiles {
        t.coord = (first.0 + t.coord.0, first.1 + t.coord.1);
    }
    for t in &tiles {
        let rel = tile_path(wp, t.coord);
        t.data.save(&root.join(&rel))?;
        crate::terrain::store::insert_loaded(&rel, t.data.clone());
    }
    scene.despawn(e);
    Ok(spawn_tiles(scene, wp, &tiles))
}
