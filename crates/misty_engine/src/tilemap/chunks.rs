//! Tilemap Renderer draws: tiles merged into one mesh per [`CHUNK`]×[`CHUNK`] cells and texture
//! (Chunk mode), rebuilt only when a chunk or one around it changes (Rule Tiles look across chunk
//! borders). Animated tiles change every frame, so they are drawn as their own quads instead of
//! being baked into the chunk meshes. Individual mode draws every tile as its own quad.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};

use glam::{IVec2, Mat4, Vec2, Vec3};
use hecs::Entity;

use super::tile::{self, Resolved};
use super::{Grid, TileTransform, Tilemap, TilemapRenderMode, TilemapRenderer};
use crate::render::mesh::{MeshData, Vertex};
use crate::render::surface::SortingKey;
use crate::render::{MaterialParams, MeshDraw, RenderingMode};
use crate::scene::{MeshRef, PrimitiveMesh, Scene};
use crate::sprites::{self, Sprite};

/// Chunk edge in cells.
pub const CHUNK: i32 = 16;

/// Chunk holding a cell.
pub fn chunk_of(c: IVec2) -> IVec2 {
    IVec2::new(c.x.div_euclid(CHUNK), c.y.div_euclid(CHUNK))
}

/// One batched mesh of a chunk: every static tile using one texture and tint.
#[derive(Clone)]
pub struct ChunkMesh {
    pub texture: String,
    /// Tile tint (sRGB + alpha), multiplied by the Tilemap's colour.
    pub color: [f32; 4],
    pub data: MeshData,
}

impl ChunkMesh {
    pub fn quads(&self) -> usize {
        self.data.vertices.len() / 4
    }
}

/// A chunk's meshes plus its animated cells (drawn separately).
#[derive(Clone, Default)]
pub struct BuiltChunk {
    pub meshes: Vec<ChunkMesh>,
    pub animated: Vec<IVec2>,
}

/// Where a tile's quad corners go (local), from the cell, its anchor and its sprite.
pub fn quad_corners(
    grid: &Grid,
    anchor: Vec2,
    c: IVec2,
    sprite: &Sprite,
    t: TileTransform,
) -> [Vec2; 4] {
    let at = grid.cell_center(c) - grid.cell_size * 0.5 + grid.cell_size * anchor;
    let (lo, hi) = sprite.bounds();
    let m = t.matrix();
    [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)].map(|p| at + m * p)
}

fn push_quad(data: &mut MeshData, corners: [Vec2; 4], sprite: &Sprite) {
    let (scale, offset) = sprite.uv_transform(false, false);
    let uv = |u: f32, v: f32| [u * scale[0] + offset[0], v * scale[1] + offset[1]];
    let uvs = [uv(0.0, 1.0), uv(1.0, 1.0), uv(1.0, 0.0), uv(0.0, 0.0)];
    let base = data.vertices.len() as u32;
    for (p, uv) in corners.into_iter().zip(uvs) {
        data.vertices.push(Vertex {
            position: [p.x, p.y, 0.0],
            normal: [0.0, 0.0, -1.0],
            uv,
            tangent: [1.0, 0.0, 0.0, 1.0],
        });
    }
    data.indices
        .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// Build the static meshes of one chunk (grouped by texture and tint) and list its animated
/// cells.
pub fn build_chunk(map: &Tilemap, grid: &Grid, chunk: IVec2) -> BuiltChunk {
    let lo = chunk * CHUNK;
    let hi = lo + IVec2::splat(CHUNK - 1);
    let mut out = BuiltChunk::default();
    let mut groups: HashMap<(String, [u32; 4]), usize> = HashMap::new();
    for (c, cell) in map.cells_in(lo, hi) {
        let Some(asset) = tile::get(&cell.tile) else {
            continue;
        };
        let r = tile::resolve_with(&asset, &cell.tile, map, c, cell.transform, 0.0);
        if r.animated {
            out.animated.push(c);
            continue;
        }
        let Some(sprite) = r.sprite.as_deref().and_then(sprites::resolve) else {
            continue;
        };
        let key = (sprite.texture.clone(), r.color.map(f32::to_bits));
        let i = *groups.entry(key).or_insert_with(|| {
            out.meshes.push(ChunkMesh {
                texture: sprite.texture.clone(),
                color: r.color,
                data: MeshData::default(),
            });
            out.meshes.len() - 1
        });
        let corners = quad_corners(grid, map.tile_anchor, c, &sprite, r.transform);
        push_quad(&mut out.meshes[i].data, corners, &sprite);
    }
    out
}

fn tint(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]]
}

fn material(texture: &str, color: [f32; 4]) -> MaterialParams {
    MaterialParams {
        color,
        albedo_map: Some(texture.to_owned()),
        unlit: true,
        mode: RenderingMode::Fade,
        double_sided: true,
        lit_2d: true,
        ..Default::default()
    }
}

/// A single tile drawn as a quad (animated tiles, Individual mode).
fn tile_draw(
    map: &Tilemap,
    grid: &Grid,
    c: IVec2,
    r: &Resolved,
    world: Mat4,
    sorting: SortingKey,
    outline: bool,
) -> Option<MeshDraw> {
    let sprite = r.sprite.as_deref().and_then(sprites::resolve)?;
    let [a, b, _, d] = quad_corners(grid, map.tile_anchor, c, &sprite, r.transform);
    // Unit quad (−0.5..0.5) → the tile's corners: x axis a→b, y axis a→d.
    let x = b - a;
    let y = d - a;
    let center = a + (x + y) * 0.5;
    let local = Mat4::from_cols(
        x.extend(0.0).extend(0.0),
        y.extend(0.0).extend(0.0),
        Vec3::Z.extend(0.0),
        center.extend(0.0).extend(1.0),
    );
    let (tiling, offset) = sprite.uv_transform(false, false);
    Some(MeshDraw {
        mesh: PrimitiveMesh::Quad.into(),
        model: world * local,
        material: MaterialParams {
            tiling,
            offset,
            ..material(&sprite.texture, tint(map.color, r.color))
        },
        cast_shadows: false,
        receive_shadows: false,
        outline,
        sorting,
        lod: None,
    })
}

struct Cached {
    hash: u64,
    /// Registered mesh key, texture and tint per batched mesh.
    meshes: Vec<(String, String, [f32; 4])>,
    animated: Vec<IVec2>,
}

#[derive(Default)]
struct Cache {
    chunks: HashMap<(u64, IVec2), Cached>,
    /// Calls since each tilemap was last drawn (stale ones are dropped).
    seen: HashMap<u64, u64>,
    frame: u64,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Default::default);

/// Hash of each chunk's own cells.
fn chunk_hashes(map: &Tilemap) -> HashMap<IVec2, u64> {
    let mut hashers: HashMap<IVec2, std::collections::hash_map::DefaultHasher> = HashMap::new();
    for (&(x, y), cell) in &map.tiles.0 {
        let h = hashers.entry(chunk_of(IVec2::new(x, y))).or_default();
        (x, y).hash(h);
        cell.hash(h);
    }
    hashers.into_iter().map(|(k, h)| (k, h.finish())).collect()
}

/// Chunk hash including the ring of cells around it (Rule Tiles at the border see them), the
/// grid, anchor and tile asset versions.
fn full_hash(own: &HashMap<IVec2, u64>, chunk: IVec2, map: &Tilemap, grid: &Grid) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    own.get(&chunk).hash(&mut h);
    let lo = chunk * CHUNK - IVec2::ONE;
    let hi = lo + IVec2::splat(CHUNK + 1);
    for x in lo.x..=hi.x {
        for y in [lo.y, hi.y] {
            map.get(IVec2::new(x, y)).hash(&mut h);
        }
    }
    for y in lo.y + 1..hi.y {
        for x in [lo.x, hi.x] {
            map.get(IVec2::new(x, y)).hash(&mut h);
        }
    }
    [grid.cell_size, grid.cell_gap, map.tile_anchor]
        .map(|v| v.to_array().map(f32::to_bits))
        .hash(&mut h);
    (grid.layout as u8).hash(&mut h);
    tile::generation().hash(&mut h);
    h.finish()
}

/// Draws of every enabled Tilemap Renderer on active objects that `keep`; `world(e)` = the
/// object's world matrix, `outline(e)` = selected.
pub fn draws(
    scene: &Scene,
    keep: &dyn Fn(Entity) -> bool,
    world: &dyn Fn(Entity) -> Mat4,
    outline: &dyn Fn(Entity) -> bool,
    out: &mut Vec<MeshDraw>,
) {
    let maps = super::rendered(scene);
    if maps.is_empty() {
        return;
    }
    let time = crate::clock::now() as f32;
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.frame += 1;
    for e in maps {
        if !keep(e) {
            continue;
        }
        let (Ok(map), Ok(r)) = (
            scene.world.get::<&Tilemap>(e),
            scene.world.get::<&TilemapRenderer>(e),
        ) else {
            continue;
        };
        let grid = super::grid_of(scene, e);
        let sorting = SortingKey {
            layer: sprites::sorting::value(&r.sorting_layer),
            order: r.order_in_layer,
        };
        let m = world(e);
        let selected = outline(e);
        if r.mode == TilemapRenderMode::Individual {
            for &(x, y) in map.tiles.0.keys() {
                let c = IVec2::new(x, y);
                if let Some(res) = tile::resolve(&map, c, time) {
                    out.extend(tile_draw(&map, &grid, c, &res, m, sorting, selected));
                }
            }
            continue;
        }
        let id = e.to_bits().get();
        let frame = cache.frame;
        cache.seen.insert(id, frame);
        let own = chunk_hashes(&map);
        let mut stale = Vec::new();
        // Chunks that no longer hold tiles.
        cache.chunks.retain(|(owner, c), v| {
            let keep = *owner != id || own.contains_key(c);
            if !keep {
                stale.extend(v.meshes.drain(..).map(|m| m.0));
            }
            keep
        });
        let mut chunks: Vec<IVec2> = own.keys().copied().collect();
        chunks.sort_by_key(|c| (c.y, c.x));
        for chunk in chunks {
            let hash = full_hash(&own, chunk, &map, &grid);
            let entry = cache.chunks.get(&(id, chunk));
            if entry.is_none_or(|c| c.hash != hash) {
                let built = build_chunk(&map, &grid, chunk);
                let meshes = built
                    .meshes
                    .into_iter()
                    .enumerate()
                    .map(|(i, cm)| {
                        let name = format!("tilemap/{id}/{}_{}/{i}/{hash:x}", chunk.x, chunk.y);
                        let key = crate::models::runtime::register_new(&name, cm.data);
                        (key, cm.texture, cm.color)
                    })
                    .collect();
                let old = cache.chunks.insert(
                    (id, chunk),
                    Cached {
                        hash,
                        meshes,
                        animated: built.animated,
                    },
                );
                if let Some(old) = old {
                    stale.extend(old.meshes.into_iter().map(|m| m.0));
                }
            }
            let c = &cache.chunks[&(id, chunk)];
            for (key, texture, color) in &c.meshes {
                out.push(MeshDraw {
                    mesh: MeshRef::Asset(key.clone()),
                    model: m,
                    material: material(texture, tint(map.color, *color)),
                    cast_shadows: false,
                    receive_shadows: false,
                    outline: selected,
                    sorting,
                    lod: None,
                });
            }
            for &cell in &c.animated {
                if let Some(res) = tile::resolve(&map, cell, time) {
                    out.extend(tile_draw(&map, &grid, cell, &res, m, sorting, selected));
                }
            }
        }
        crate::models::runtime::retire(&stale);
    }
    // Forget tilemaps not drawn for a while (deleted objects, closed scenes).
    let frame = cache.frame;
    let gone: Vec<u64> = cache
        .seen
        .iter()
        .filter(|(_, f)| frame - **f > 600)
        .map(|(id, _)| *id)
        .collect();
    for id in gone {
        cache.seen.remove(&id);
        let mut stale = Vec::new();
        cache.chunks.retain(|(owner, _), v| {
            if *owner == id {
                stale.extend(v.meshes.drain(..).map(|m| m.0));
                false
            } else {
                true
            }
        });
        crate::models::runtime::retire(&stale);
    }
}

/// Draws of one scene with world matrices from the scene (tests, previews).
pub fn scene_draws(scene: &Scene) -> Vec<MeshDraw> {
    let mut out = Vec::new();
    draws(
        scene,
        &|_| true,
        &|e| scene.world_matrix(e),
        &|_| false,
        &mut out,
    );
    out
}
