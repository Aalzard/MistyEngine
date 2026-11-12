//! Terrain drawing. Per terrain asset the GPU keeps a heightmap texture (R32Float), a normal
//! texture and the splat map; when the asset's version changes only the region the store
//! reports as dirty is re-uploaded (a brush stroke touches a few hundred texels, not the whole
//! terrain). The surface is drawn as chunks with distance LOD from shared index patterns
//! (`terrain_lod`), grass is scattered around the camera (`terrain_grass`) and layer textures
//! live in 2D arrays (`terrain_layers`). Group 3 of the terrain pipelines holds all of it.

use std::collections::HashMap;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use super::culling::{Aabb, Frustum};
use super::environment::Environment;
use super::grass_cull::{self, CullPipelines};
use super::grass_near_shadow::{self, RADIUS as NEAR_RADIUS};
use super::terrain_grass::{Atlas, Grass, GrassParams, Limit};
use super::terrain_layers::LayerCache;
use super::terrain_lod;
use super::{DrawUniform, RenderStats};
use crate::sprites::compress::generated::Ctx;
use crate::terrain::store::{self, Region};
use crate::terrain::{MAX_LAYERS, TerrainData, TerrainDraw, harmony};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TerrainParams {
    /// xyz = size, w = layer count.
    size: [f32; 4],
    /// x = heightmap resolution, y/z = sample spacing.
    grid: [f32; 4],
    /// Per layer: xy = tile size, zw = tile offset.
    tiles: [[f32; 4]; MAX_LAYERS],
    /// Per layer: x = metallic, y = smoothness, z = normal scale (0 = no normal map).
    surface: [[f32; 4]; MAX_LAYERS],
    /// Per layer Color Harmony adjustment (see `harmony::adjustments`).
    harmony: [[f32; 4]; MAX_LAYERS],
    /// Cliff settings (see `CliffSettings::uniform`).
    cliff: [f32; 4],
    /// Ground under grass (terrain_layers.wgsl): rgb = linear healthy grass colour,
    /// w = ground tint (0 = none).
    grass_healthy: [f32; 4],
    /// rgb = linear dry colour, w = patch size.
    grass_dry: [f32; 4],
    /// rgb = linear lush colour, w = grass draw distance.
    grass_lush: [f32; 4],
    /// x = lush amount, y = dry amount, z = the quality's share of the blades with the
    /// ground cover (0 = off), w = the grass's full-density distance.
    grass_amounts: [f32; 4],
    /// Quality ▸ Grass Ground Cover (see `GROUND_COVER`): x = how much of the ground the
    /// fans cover, y / z = where that fades in, w = where it has faded out (× the grass's
    /// full-density distance).
    ground_cover: [f32; 4],
    /// rgb = the fans' colour against the ground's looking into the sun / with it behind.
    ground_tint: [f32; 4],
    ground_front: [f32; 4],
    /// x = chunks per side (the chunk layer masks of terrains with more than four layers).
    chunks: [f32; 4],
}

/// Quality ▸ Grass Ground Cover (terrain_layers.wgsl `grass_ground_cover`), fitted against
/// renders with the fans (meadow_perf_tests.rs): how much of the ground the fans cover, where
/// that fades in and out (× the full-density distance); their colour against the ground's
/// looking into the sun and with the sun behind.
const GROUND_COVER: [f32; 4] = [0.5, 0.8, 2.0, 7.0];
const GROUND_TINT: [f32; 4] = [0.78, 0.75, 0.6, 0.0];
const GROUND_FRONT: [f32; 4] = [1.35, 1.3, 1.2, 0.0];

/// The ground cover values (tests can try others: `MISTY_GROUND_COVER` = 10 numbers).
fn ground_cover_values() -> [[f32; 4]; 3] {
    #[cfg(test)]
    if let Ok(v) = std::env::var("MISTY_GROUND_COVER") {
        let n: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if n.len() == 10 {
            return [
                [n[0], n[1], n[2], n[3]],
                [n[4], n[5], n[6], 0.0],
                [n[7], n[8], n[9], 0.0],
            ];
        }
    }
    [GROUND_COVER, GROUND_TINT, GROUND_FRONT]
}

/// Per terrain: key, version (and position), (lod, edge mask) of every chunk in the shadow
/// maps this frame.
pub(super) type ChunkStates = Vec<(String, u64, Vec<(u8, u8)>)>;

/// One chunk drawn this frame.
#[derive(Clone, Copy)]
struct ChunkDraw {
    base: i32,
    /// Level of detail and edge mask in the camera's passes.
    lod: u8,
    mask: u8,
    /// ... and in the shadow maps (`terrain_lod::shadow_lods`: full detail unless the chunk
    /// receives sun shadows at a coarser level), so casters keep their shape as the camera
    /// moves.
    shadow_lod: u8,
    shadow_mask: u8,
    /// Inside the camera frustum (shadow passes draw every chunk).
    visible: bool,
    /// Distance from the camera (the camera pass draws near chunks first).
    dist: f32,
    /// World bounds (the virtual shadow map culls chunks per page rectangle).
    bounds: Aabb,
    /// Has hole cells: drawn at full detail from its own index list (`HoleIndices`).
    holed: bool,
}

/// Shared index patterns for one heightmap resolution.
struct Patterns {
    buffer: wgpu::Buffer,
    ranges: Vec<[std::ops::Range<u32>; 16]>,
    /// The indices (chunks with holes filter them into lists of their own).
    indices: Vec<u32>,
}

/// Index lists of the chunks with holes: the shared pattern of the chunk's (lod, edge mask)
/// without the triangles over hole cells, so holes cut every pass (camera, depth, shadows,
/// prepass) with no shader change. Chunks with holes always draw at full detail, so the
/// hole edges follow the heightmap cells exactly.
#[derive(Default)]
struct HoleIndices {
    /// (chunk, lod, mask) -> range in `buffer`.
    ranges: HashMap<(usize, u8, u8), std::ops::Range<u32>>,
    buffer: Option<wgpu::Buffer>,
}

/// The triangles of `pattern` (a chunk's indices, relative to its first sample) whose cell is
/// not a hole; the chunk's first cell is `(x0, z0)`.
fn without_holes(pattern: &[u32], res: usize, x0: usize, z0: usize, d: &TerrainData) -> Vec<u32> {
    let mut out = Vec::with_capacity(pattern.len());
    for tri in pattern.as_chunks::<3>().0 {
        let (mut sx, mut sz) = (0usize, 0usize);
        for i in tri {
            sx += *i as usize % res;
            sz += *i as usize / res;
        }
        // The centroid's cell (a third of the way in, so never on a cell border).
        let (cx, cz) = (sx / 3, sz / 3);
        if !d.is_hole(x0 + cx, z0 + cz) {
            out.extend_from_slice(tri);
        }
    }
    out
}

struct GpuTerrain {
    version: u64,
    res: usize,
    ares: usize,
    heights: wgpu::Texture,
    normals: wgpu::Texture,
    splat: wgpu::Texture,
    /// Normalised height range (min, max).
    range: (f32, f32),
    /// Per chunk: normalised height range.
    chunk_range: Vec<(f32, f32)>,
    /// Splat coverage per layer (Color Harmony weights).
    coverage: [f32; MAX_LAYERS],
    grass: Grass,
    /// The grass passes' uniforms and groups (camera, cascade casters, the near map's
    /// slices), made once and rewritten every frame.
    grass_uniforms: Option<[(wgpu::Buffer, wgpu::BindGroup); 2 + grass_near_shadow::SLICES]>,
    /// Id of the grass atlas those groups bind (they are made again when it changes: the
    /// flower layers arrived or the flowers' textures changed).
    grass_atlas_id: u64,
    /// Per chunk: some of its cells are holes.
    holed: Vec<bool>,
    /// Index lists of the chunks with holes (rebuilt when holes or their LODs change).
    holes: HoleIndices,
    /// Splat maps of layers 5.. (one array slice per map; a single unused slice with four
    /// layers or fewer).
    splat_extra: wgpu::Texture,
    /// Splat maps the data has (the arrays are made again when it changes).
    maps: usize,
    /// Per chunk: bit mask of the layers painted in it (R32Uint, chunks² texels). With more
    /// than four layers a pixel reads only the maps of its chunk's layers and blends the four
    /// strongest of them, so a chunk costs what its own layers cost.
    chunk_layers: wgpu::Texture,
    /// Frame of [`TerrainCache::prepare`] that last drew it (unused ones are dropped).
    used: u64,
}

/// Frames a terrain may go undrawn before its GPU data is dropped (streamed tiles that went
/// away; a terrain hidden for a moment keeps its data).
const UNUSED_FRAMES: u64 = 600;

/// Bit mask of the layers with weight in splat texels `xs` × `zs` (every map).
fn layer_mask(d: &TerrainData, xs: std::ops::Range<usize>, zs: std::ops::Range<usize>) -> u32 {
    let n = d.alphamap_resolution;
    let mut mask = 0u32;
    for tz in zs {
        for tx in xs.clone() {
            for (l, w) in d.layer_weights(tz * n + tx).iter().enumerate() {
                if *w > 0 {
                    mask |= 1 << l;
                }
            }
        }
    }
    mask
}

/// Write `data` (rows of `w` texels) at `(x, z)` of slice `layer` of an array texture.
#[allow(clippy::too_many_arguments)]
fn write_layer<T: Pod>(
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    layer: u32,
    x: usize,
    z: usize,
    w: usize,
    data: &[T],
) {
    let texel = std::mem::size_of::<T>();
    let h = data.len() / w.max(1);
    if h == 0 {
        return;
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: x as u32,
                y: z as u32,
                z: layer,
            },
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(data),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some((w * texel) as u32),
            rows_per_image: Some(h as u32),
        },
        wgpu::Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
    );
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    side: usize,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: side as u32,
            height: side as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Write `data` (rows of `w` texels) at `(x, z)` of a texture.
fn write<T: Pod>(
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    x: usize,
    z: usize,
    w: usize,
    data: &[T],
) {
    let texel = std::mem::size_of::<T>();
    let h = data.len() / w.max(1);
    if h == 0 {
        return;
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: x as u32,
                y: z as u32,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(data),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some((w * texel) as u32),
            rows_per_image: Some(h as u32),
        },
        wgpu::Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
    );
}

/// Rows `zs` × columns `xs` of a row-major grid.
fn rect<T: Copy>(
    grid: &[T],
    side: usize,
    xs: &std::ops::Range<usize>,
    zs: &std::ops::Range<usize>,
) -> Vec<T> {
    zs.clone()
        .flat_map(|z| grid[z * side + xs.start..z * side + xs.end].iter().copied())
        .collect()
}

/// Encoded normals of the samples in `xs` × `zs`.
fn normals(
    d: &TerrainData,
    xs: &std::ops::Range<usize>,
    zs: &std::ops::Range<usize>,
) -> Vec<[i8; 4]> {
    let w = xs.len();
    let mut out = vec![[0i8; 4]; w * zs.len()];
    let enc = |v: f32| (v * 127.0).round() as i8;
    // w = grass density at the sample (the detail map; 0 without grass): the ground under
    // grass takes the grass colour (terrain_layers.wgsl).
    let (a, res) = (d.alphamap_resolution, d.heightmap_resolution);
    let grass = d.details.enabled && d.detail_map.len() == a * a;
    let density = |ix: usize, iz: usize| {
        if !grass {
            return 0.0;
        }
        let at = |i: usize| (i * a / (res - 1).max(1)).min(a - 1);
        d.detail_map[at(iz) * a + at(ix)] as f32 / 255.0
    };
    crate::terrain::auto_paint::par_rows(&mut out, w, |row, texels| {
        let iz = zs.start + row;
        for (i, t) in texels.iter_mut().enumerate() {
            let n = d.vertex_normal(xs.start + i, iz);
            *t = [enc(n.x), enc(n.y), enc(n.z), enc(density(xs.start + i, iz))];
        }
    });
    out
}

impl GpuTerrain {
    fn new(device: &wgpu::Device, d: &TerrainData) -> Self {
        let res = d.heightmap_resolution;
        let per_side = (res - 1) / terrain_lod::chunk_cells(res);
        Self {
            version: 0,
            res,
            ares: d.alphamap_resolution,
            heights: texture(
                device,
                "misty terrain heights",
                res,
                wgpu::TextureFormat::R32Float,
            ),
            normals: texture(
                device,
                "misty terrain normals",
                res,
                wgpu::TextureFormat::Rgba8Snorm,
            ),
            splat: texture(
                device,
                "misty terrain splat",
                d.alphamap_resolution,
                wgpu::TextureFormat::Rgba8Unorm,
            ),
            range: (0.0, 0.0),
            chunk_range: vec![(0.0, 0.0); per_side * per_side],
            coverage: [0.0; MAX_LAYERS],
            grass: Grass::default(),
            grass_uniforms: None,
            grass_atlas_id: 0,
            holed: vec![false; per_side * per_side],
            holes: HoleIndices::default(),
            splat_extra: device.create_texture(&wgpu::TextureDescriptor {
                label: Some("misty terrain splat (layers 5+)"),
                size: wgpu::Extent3d {
                    width: d.alphamap_resolution as u32,
                    height: d.alphamap_resolution as u32,
                    depth_or_array_layers: (d.splat_maps() as u32 - 1).max(1),
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }),
            maps: d.splat_maps(),
            used: 0,
            chunk_layers: texture(
                device,
                "misty terrain chunk layers",
                per_side,
                wgpu::TextureFormat::R32Uint,
            ),
        }
    }

    fn per_side(&self) -> usize {
        (self.res - 1) / terrain_lod::chunk_cells(self.res)
    }

    /// Upload the extra splat maps' texels `xs` × `zs` and the layer masks of the chunks over
    /// them (terrains with more than four layers only).
    fn upload_extra(
        &mut self,
        queue: &wgpu::Queue,
        d: &TerrainData,
        xs: std::ops::Range<usize>,
        zs: std::ops::Range<usize>,
    ) {
        let n = self.ares;
        if d.alphamap_extra.len() < n * n || xs.is_empty() || zs.is_empty() {
            return;
        }
        for (m, map) in d.alphamap_extra.chunks(n * n).enumerate() {
            write_layer(
                queue,
                &self.splat_extra,
                m as u32,
                xs.start,
                zs.start,
                xs.len(),
                &rect(map, n, &xs, &zs),
            );
        }
        let per = self.per_side();
        // A chunk's texels and one more around them (the bilinear splat lookup reads them).
        let chunk_of = |t: usize| (t * per / n).min(per - 1);
        let (cx0, cx1) = (chunk_of(xs.start.saturating_sub(1)), chunk_of(xs.end));
        let (cz0, cz1) = (chunk_of(zs.start.saturating_sub(1)), chunk_of(zs.end));
        for cz in cz0..=cz1 {
            let mut row = Vec::with_capacity(cx1 - cx0 + 1);
            for cx in cx0..=cx1 {
                let tx = (cx * n / per).saturating_sub(1)..((cx + 1) * n / per + 1).min(n);
                let tz = (cz * n / per).saturating_sub(1)..((cz + 1) * n / per + 1).min(n);
                row.push(layer_mask(d, tx, tz));
            }
            write(queue, &self.chunk_layers, cx0, cz, row.len(), &row);
        }
    }

    /// Re-upload everything.
    fn upload_all(&mut self, queue: &wgpu::Queue, d: &TerrainData) {
        crate::profile_scope!("Terrain.UploadAll");
        let all = 0..self.res;
        write(queue, &self.heights, 0, 0, self.res, &d.heights);
        write(
            queue,
            &self.normals,
            0,
            0,
            self.res,
            &normals(d, &all, &all),
        );
        write(queue, &self.splat, 0, 0, self.ares, &d.alphamap);
        self.upload_extra(queue, d, 0..self.ares, 0..self.ares);
        self.range = d.height_range();
        let per = self.per_side();
        self.update_chunks(d, 0..per, 0..per);
        self.coverage = harmony::coverage(d);
        self.grass.invalidate(d, None);
    }

    /// Re-upload the texels of `r` only.
    fn upload_region(&mut self, queue: &wgpu::Queue, d: &TerrainData, r: Region) {
        crate::profile_scope!("Terrain.UploadRegion");
        let (xs, zs) = r.cells(self.res, 2);
        if !xs.is_empty() && !zs.is_empty() {
            write(
                queue,
                &self.heights,
                xs.start,
                zs.start,
                xs.len(),
                &rect(&d.heights, self.res, &xs, &zs),
            );
            write(
                queue,
                &self.normals,
                xs.start,
                zs.start,
                xs.len(),
                &normals(d, &xs, &zs),
            );
            for h in rect(&d.heights, self.res, &xs, &zs) {
                self.range = (self.range.0.min(h), self.range.1.max(h));
            }
            let cells = terrain_lod::chunk_cells(self.res);
            let per = self.per_side();
            let chunks = |s: &std::ops::Range<usize>| {
                (s.start.saturating_sub(1) / cells).min(per)..((s.end + 1) / cells + 1).min(per)
            };
            self.update_chunks(d, chunks(&xs), chunks(&zs));
        }
        let (sx, sz) = r.cells(self.ares, 1);
        if !sx.is_empty() && !sz.is_empty() {
            write(
                queue,
                &self.splat,
                sx.start,
                sz.start,
                sx.len(),
                &rect(&d.alphamap, self.ares, &sx, &sz),
            );
            self.upload_extra(queue, d, sx, sz);
        }
        self.coverage = harmony::coverage(d);
        self.grass.invalidate(d, Some(r));
    }

    /// Recompute the height ranges of chunks `xs` × `zs`.
    fn update_chunks(
        &mut self,
        d: &TerrainData,
        xs: std::ops::Range<usize>,
        zs: std::ops::Range<usize>,
    ) {
        let cells = terrain_lod::chunk_cells(self.res);
        let per = self.per_side();
        for cz in zs {
            for cx in xs.clone() {
                let mut range = (f32::MAX, f32::MIN);
                for iz in cz * cells..=(cz + 1) * cells {
                    let row =
                        &d.heights[iz * self.res + cx * cells..=iz * self.res + (cx + 1) * cells];
                    for h in row {
                        range = (range.0.min(*h), range.1.max(*h));
                    }
                }
                self.chunk_range[cz * per + cx] = range;
                let holed = !d.holes.is_empty()
                    && (cz * cells..(cz + 1) * cells)
                        .any(|z| (cx * cells..(cx + 1) * cells).any(|x| d.is_hole(x, z)));
                if holed || self.holed[cz * per + cx] {
                    // Its index lists are made again.
                    self.holes = HoleIndices::default();
                }
                self.holed[cz * per + cx] = holed;
            }
        }
    }

    fn bytes(&self) -> u64 {
        let extra = self.ares * self.ares * 4 * (self.maps - 1);
        (self.res * self.res * 8 + self.ares * self.ares * 4 + extra) as u64 + self.grass.bytes()
    }
}

pub(super) struct TerrainCache {
    entries: HashMap<String, GpuTerrain>,
    /// Index patterns per heightmap resolution.
    patterns: HashMap<usize, Patterns>,
    layers: LayerCache,
    splat_sampler: wgpu::Sampler,
    /// Bind group per terrain of the current frame (None = data not uploaded).
    groups: Vec<Option<wgpu::BindGroup>>,
    /// Chunks per terrain of the current frame.
    chunks: Vec<Vec<ChunkDraw>>,
    /// Grass bind group per terrain of the current frame (None = no grass).
    grass_groups: Vec<Option<wgpu::BindGroup>>,
    /// Grass shadow caster bind group per terrain (None = casts none).
    grass_shadow_groups: Vec<Option<wgpu::BindGroup>>,
    /// Near-camera grass shadow map casters per terrain, one group per slice (None = none).
    grass_near_groups: Vec<Option<[wgpu::BindGroup; grass_near_shadow::SLICES]>>,
    /// Range and share of this frame's grass shadow casters (Quality).
    grass_casters: Option<(f32, f32)>,
    /// The grass uniforms of this frame's passes per terrain: camera, cascade casters, near
    /// map slices (the GPU culling tests the same numbers).
    grass_params: Vec<Option<GrassFrameParams>>,
    /// Grass culling pipelines (made with the first grass).
    grass_cull: Option<CullPipelines>,
    /// The cull pipelines made on a worker since the renderer was made (streaming only).
    grass_cull_prebuilt: super::prebuilt::Prebuilt<CullPipelines>,
    /// The camera's view-projection of the current frame.
    view_proj: Mat4,
    /// Quality ▸ Grass Ground Cover (with the quality's blade share): the terrain paints the
    /// undergrowth the grass LOD's flat fans would draw, and the lit grass skips them.
    ground_cover: Option<f32>,
    /// This frame the lit lane holds only the flat fans (the grass visibility raster draws
    /// the standing blades from the depth lane: `Renderer::grass_vis_passes`).
    lit_fans_only: bool,
    /// The built-in grass atlas with empty flower layers (uploaded with the first grass) and
    /// its id.
    grass_atlas: Option<(Atlas, u64)>,
    /// Grass atlases with a terrain's flower layers, per set of flower textures.
    flower_atlases: HashMap<Vec<String>, FlowerAtlas>,
    /// Ids handed to grass atlases.
    next_atlas_id: u64,
    /// Camera of the current frame (grass LOD).
    camera: Vec3,
    /// How far from the camera the sun's shadows reach (chunks within it keep their camera
    /// LOD in the shadow maps; see [`ChunkDraw::shadow_lod`]).
    shadow_reach: f32,
    frustum: Option<Frustum>,
    started: Instant,
    /// Calls of [`TerrainCache::prepare`] (when each terrain was last drawn).
    frame: u64,
}

/// Flower layers being built on a worker thread. (Behind a lock only so the renderer stays
/// `Sync`.)
type LayersRx = std::sync::Mutex<std::sync::mpsc::Receiver<super::terrain_grass::FlowerLayers>>;

/// A grass atlas with flower layers: being built on a worker thread, or uploaded (with its id;
/// `next`: built again on a worker now that its blocks are in the texture cache, the uploaded
/// one drawing meanwhile).
enum FlowerAtlas {
    Building(LayersRx),
    Ready {
        atlas: Atlas,
        id: u64,
        next: Option<LayersRx>,
    },
}

/// Build the flower layers of `kinds` on a worker thread.
fn spawn_flower_layers(kinds: &[crate::terrain::flowers::FlowerKind], ctx: Ctx) -> LayersRx {
    let (tx, rx) = std::sync::mpsc::channel();
    let kinds = kinds.to_vec();
    let spawned = std::thread::Builder::new()
        .name("misty flower atlas".into())
        .spawn(move || {
            let _ = tx.send(super::terrain_grass::flower_layers(&kinds, &ctx));
        });
    if let Err(e) = spawned {
        eprintln!("flower atlas worker: {e}");
    }
    std::sync::Mutex::new(rx)
}

/// The grass atlas with the flower layers of `kinds` once it is ready (the layers are built on
/// a worker thread while grass streams, in place otherwise); None meanwhile.
fn flower_atlas<'a>(
    atlases: &'a mut HashMap<Vec<String>, FlowerAtlas>,
    next_id: &mut u64,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    kinds: &[crate::terrain::flowers::FlowerKind],
) -> Option<(&'a wgpu::TextureView, &'a wgpu::Sampler, u64)> {
    let key = super::terrain_grass::flower_key(kinds);
    let streaming = super::terrain_grass::grass_streaming();
    let ctx = Ctx::runtime(
        device
            .features()
            .contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
    );
    if !atlases.contains_key(&key) {
        // (A handful of terrains at most; forget atlases of texture sets no longer used.)
        if atlases.len() > 8 {
            atlases.clear();
        }
        let state = if streaming {
            FlowerAtlas::Building(spawn_flower_layers(kinds, ctx.clone()))
        } else {
            let layers = super::terrain_grass::flower_layers(kinds, &ctx);
            *next_id += 1;
            FlowerAtlas::Ready {
                atlas: super::terrain_grass::atlas_array(device, queue, Some(&layers)),
                id: *next_id,
                next: None,
            }
        };
        atlases.insert(key.clone(), state);
    }
    let entry = atlases.get_mut(&key)?;
    match entry {
        FlowerAtlas::Building(rx) => {
            let got = rx.get_mut().unwrap_or_else(|e| e.into_inner()).try_recv();
            let atlas = match got {
                Ok(layers) => {
                    crate::profile_scope!("Grass.FlowerAtlas");
                    super::terrain_grass::atlas_array(device, queue, Some(&layers))
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return None,
                // The worker failed: draw without the flowers' textures from now on.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    super::terrain_grass::atlas_array(device, queue, None)
                }
            };
            *next_id += 1;
            *entry = FlowerAtlas::Ready {
                atlas,
                id: *next_id,
                next: None,
            };
        }
        FlowerAtlas::Ready { atlas, id, next } => {
            // Went up uncompressed while its blocks were encoded: build it again (cache hits).
            if next.is_none() && atlas.outdated() {
                if streaming {
                    *next = Some(spawn_flower_layers(kinds, ctx));
                } else {
                    let layers = super::terrain_grass::flower_layers(kinds, &ctx);
                    *atlas = super::terrain_grass::atlas_array(device, queue, Some(&layers));
                    *next_id += 1;
                    *id = *next_id;
                }
            }
            if let Some(rx) = next {
                let got = rx.get_mut().unwrap_or_else(|e| e.into_inner()).try_recv();
                match got {
                    Ok(layers) => {
                        crate::profile_scope!("Grass.FlowerAtlas");
                        *atlas = super::terrain_grass::atlas_array(device, queue, Some(&layers));
                        *next_id += 1;
                        *id = *next_id;
                        *next = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        atlas.keep();
                        *next = None;
                    }
                }
            }
        }
    }
    match entry {
        FlowerAtlas::Ready { atlas, id, .. } => Some((&atlas.view, &atlas.sampler, *id)),
        FlowerAtlas::Building(_) => None,
    }
}

/// The grass uniforms of a terrain's passes this frame.
struct GrassFrameParams {
    camera: GrassParams,
    /// The sun cascades' casters (None = the grass casts none).
    shadow: Option<GrassParams>,
    /// The near grass shadow map's slices (None = off).
    near: Option<[GrassParams; grass_near_shadow::SLICES]>,
}

impl TerrainCache {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            entries: HashMap::new(),
            patterns: HashMap::new(),
            layers: LayerCache::default(),
            splat_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("misty splat sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            groups: Vec::new(),
            chunks: Vec::new(),
            grass_groups: Vec::new(),
            grass_shadow_groups: Vec::new(),
            grass_near_groups: Vec::new(),
            grass_casters: None,
            grass_params: Vec::new(),
            grass_cull: None,
            grass_cull_prebuilt: Default::default(),
            view_proj: Mat4::IDENTITY,
            ground_cover: None,
            lit_fans_only: false,
            grass_atlas: None,
            flower_atlases: HashMap::new(),
            next_atlas_id: 1,
            camera: Vec3::ZERO,
            shadow_reach: 0.0,
            frustum: None,
            started: Instant::now(),
            frame: 0,
        }
    }

    /// Bring a terrain's GPU data up to date with its store version.
    fn sync(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, t: &TerrainDraw) {
        let d = &t.data;
        let stale = self.entries.get(&t.key).is_none_or(|g| {
            g.res != d.heightmap_resolution
                || g.ares != d.alphamap_resolution
                || g.maps != d.splat_maps()
        });
        if stale {
            self.entries
                .insert(t.key.clone(), GpuTerrain::new(device, d));
        }
        let g = self.entries.get_mut(&t.key).expect("just inserted");
        if g.version == t.version && !stale {
            return;
        }
        let change = (!stale)
            .then(|| store::changes_since(&t.key, g.version))
            .flatten();
        match change {
            Some(r) if r.is_empty() => {}
            Some(r) => g.upload_region(queue, d, r),
            None => g.upload_all(queue, d),
        }
        g.version = t.version;
        let res = d.heightmap_resolution;
        self.patterns.entry(res).or_insert_with(|| {
            let (indices, ranges) = terrain_lod::patterns(res, terrain_lod::chunk_cells(res));
            Patterns {
                buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("misty terrain patterns"),
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                ranges,
                indices,
            }
        });
    }

    /// Quality ▸ Grass Ground Cover for the next renders: the quality's share of the blades
    /// when on.
    /// The lit lane holds only the flat fans this frame (planned by `plan_grass`).
    pub fn lit_fans_only(&self) -> bool {
        self.lit_fans_only
    }

    /// The lit grass draws the flat fans (not with Quality ▸ Grass Ground Cover).
    pub fn draws_fans(&self) -> bool {
        self.ground_cover.is_none()
    }

    pub fn set_ground_cover(&mut self, cover: Option<f32>) {
        self.ground_cover = cover;
    }

    /// Upload changed terrains and build this frame's bind groups.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        textures: &mut super::texture::TextureCache,
        terrains: &[TerrainDraw],
        environment: &Environment,
    ) {
        crate::profile_scope!("Terrain.Prepare");
        self.groups.clear();
        let light = environment.sun.as_ref().map(|s| {
            Vec3::from(s.color.map(|c| {
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            }))
        });
        self.frame += 1;
        let frame = self.frame;
        // GPU data of terrains no longer drawn (streamed tiles that went away) is dropped.
        self.entries
            .retain(|_, g| g.used + UNUSED_FRAMES > frame || g.used == 0);
        // While streaming, at most one terrain new to the GPU per frame (streamed tiles arrive
        // one at a time; the rest wait a frame or two instead of piling into one).
        let mut fresh = 0;
        for t in terrains {
            if !self.entries.contains_key(&t.key) && super::terrain_grass::grass_streaming() {
                fresh += 1;
                if fresh > 1 {
                    self.groups.push(None);
                    continue;
                }
            }
            self.sync(device, queue, t);
            let layers = self.layers.get(device, queue, &t.data.layers);
            if let Some(g) = self.entries.get_mut(&t.key) {
                g.used = frame;
            }
            let gpu = &self.entries[&t.key];
            let mut params = params(t, &layers.means, gpu.coverage, light, self.ground_cover);
            params.chunks = [gpu.per_side() as f32, 0.0, 0.0, 0.0];
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("misty terrain params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let view = |t: &wgpu::Texture| t.create_view(&Default::default());
            let (heights, normals, splat) =
                (view(&gpu.heights), view(&gpu.normals), view(&gpu.splat));
            let extra = gpu.splat_extra.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });
            let chunk_layers = view(&gpu.chunk_layers);
            let tex = wgpu::BindingResource::TextureView;
            let entries = [
                (0, buffer.as_entire_binding()),
                (1, tex(&splat)),
                (2, wgpu::BindingResource::Sampler(&self.splat_sampler)),
                (3, tex(&layers.albedo)),
                (4, tex(&layers.normal)),
                (6, wgpu::BindingResource::Sampler(textures.sampler())),
                (7, tex(&extra)),
                (8, tex(&chunk_layers)),
                (12, tex(&heights)),
                (13, tex(&normals)),
            ]
            .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
            self.groups
                .push(Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("misty terrain group"),
                    layout,
                    entries: &entries,
                })));
        }
    }

    /// Start making the grass cull pipelines on a worker thread (streaming only).
    pub fn prebuild(&mut self, device: &wgpu::Device) {
        let device = device.clone();
        self.grass_cull_prebuilt =
            super::prebuilt::Prebuilt::spawn("misty grass cull pipelines", move || {
                CullPipelines::new(&device)
            });
    }

    /// Layer texture arrays still being built on worker threads.
    pub fn layers_pending(&self) -> usize {
        self.layers.pending()
    }

    /// The grass clock this render: the environment's fixed one, else seconds since the cache
    /// was made.
    pub fn grass_time(&self, environment: &Environment) -> f32 {
        environment
            .grass_time
            .unwrap_or_else(|| self.started.elapsed().as_secs_f32())
    }

    /// Distance the sun's shadows reach this frame (Quality ▸ Shadow Distance), for the
    /// chunks' shadow-map detail in the next [`TerrainCache::select`].
    pub fn set_shadow_reach(&mut self, distance: f32) {
        self.shadow_reach = distance;
    }

    /// Pick each chunk's LOD and visibility for the camera, and update grass around it
    /// (`time` = [`TerrainCache::grass_time`]).
    #[allow(clippy::too_many_arguments)]
    pub fn select(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera: Vec3,
        view_proj: &Mat4,
        terrains: &[TerrainDraw],
        first_draw: usize,
        grass_layout: &wgpu::BindGroupLayout,
        environment: &Environment,
        casters: Option<(f32, f32)>,
        screen: [f32; 3],
        near_map: bool,
        time: f32,
        density: f32,
    ) {
        crate::profile_scope!("Terrain.Select");
        let frustum = Frustum::from_matrix(view_proj);
        self.chunks.clear();
        self.grass_groups.clear();
        self.grass_shadow_groups.clear();
        self.grass_near_groups.clear();
        self.grass_params.clear();
        self.camera = camera;
        self.view_proj = *view_proj;
        self.grass_casters = casters;
        // Chunk LODs of every terrain first: tiles of a larger terrain that share an edge are
        // stitched across it like the chunks inside one terrain.
        let mut base: Vec<Vec<u8>> = Vec::with_capacity(terrains.len());
        let mut geoms: Vec<Option<terrain_lod::TileGeom>> = Vec::with_capacity(terrains.len());
        for t in terrains {
            let Some(g) = self.entries.get(&t.key) else {
                base.push(Vec::new());
                geoms.push(None);
                continue;
            };
            let d = &t.data;
            let cells = terrain_lod::chunk_cells(g.res);
            let per = g.per_side();
            let spacing = d.spacing();
            let chunk_m = cells as f32 * spacing.x.max(spacing.y);
            let lods = terrain_lod::select(per, chunk_m, terrain_lod::max_lod(cells), |c| {
                let b = chunk_bounds(g, t, c);
                camera.clamp(b.min, b.max).distance(camera)
            });
            base.push(lods);
            geoms.push(Some(terrain_lod::TileGeom {
                corner: [t.position.x, t.position.z],
                size: [d.size.x, d.size.z],
                per_side: per,
            }));
        }
        let neighbours = terrain_lod::tile_neighbours(&geoms);
        let tiled = neighbours.iter().any(|n| n.iter().any(Option::is_some));
        let mut across: Vec<Vec<u8>> = vec![Vec::new(); terrains.len()];
        if tiled {
            terrain_lod::limit_across(&mut base, &geoms, &neighbours);
            for (i, m) in across.iter_mut().enumerate() {
                *m = terrain_lod::masks_across(i, &base, &geoms, &neighbours);
            }
        }
        for (i, t) in terrains.iter().enumerate() {
            let Some(g) = self.entries.get_mut(&t.key) else {
                self.chunks.push(Vec::new());
                self.grass_groups.push(None);
                self.grass_shadow_groups.push(None);
                self.grass_near_groups.push(None);
                self.grass_params.push(None);
                continue;
            };
            let d = &t.data;
            let cells = terrain_lod::chunk_cells(g.res);
            let per = g.per_side();
            let spacing = d.spacing();
            let chunk_m = cells as f32 * spacing.x.max(spacing.y);
            let bounds = |c: usize| {
                let (cx, cz) = (c % per, c / per);
                let (lo, hi) = g.chunk_range[c];
                let min = Vec3::new(
                    cx as f32 * cells as f32 * spacing.x,
                    lo * d.size.y,
                    cz as f32 * cells as f32 * spacing.y,
                );
                let max = Vec3::new(
                    min.x + cells as f32 * spacing.x,
                    hi * d.size.y,
                    min.z + cells as f32 * spacing.y,
                );
                Aabb::new(min + t.position, max + t.position)
            };
            let _ = chunk_m;
            let mut lods = std::mem::take(&mut base[i]);
            // Chunks with holes draw at full detail (their neighbours step down to them).
            let any_holes = g.holed.iter().any(|h| *h);
            let full = |mut l: Vec<u8>| {
                for (c, h) in g.holed.iter().enumerate() {
                    if *h {
                        l[c] = 0;
                    }
                }
                terrain_lod::limit(per, l)
            };
            if any_holes {
                lods = full(lods);
            }
            let mut masks = terrain_lod::masks(per, &lods);
            if across[i].len() == masks.len() {
                for (m, a) in masks.iter_mut().zip(&across[i]) {
                    *m |= a;
                }
            }
            let reach = self.shadow_reach;
            let mut shadow_lods = terrain_lod::shadow_lods(per, &lods, |c| {
                let b = bounds(c);
                camera.clamp(b.min, b.max).distance(camera) < reach
            });
            if any_holes {
                shadow_lods = full(shadow_lods);
            }
            let shadow_masks = terrain_lod::masks(per, &shadow_lods);
            if any_holes && let Some(p) = self.patterns.get(&g.res) {
                let wanted: Vec<(usize, u8, u8)> = (0..per * per)
                    .filter(|c| g.holed[*c])
                    .flat_map(|c| [(c, lods[c], masks[c]), (c, shadow_lods[c], shadow_masks[c])])
                    .collect();
                if wanted.iter().any(|k| !g.holes.ranges.contains_key(k)) {
                    crate::profile_scope!("Terrain.HoleIndices");
                    g.holes = hole_indices(device, p, &wanted, g.res, per, cells, d);
                }
            }
            self.chunks.push(
                (0..per * per)
                    .map(|c| {
                        let b = bounds(c);
                        ChunkDraw {
                            base: ((c / per) * cells * g.res + (c % per) * cells) as i32,
                            lod: lods[c],
                            mask: masks[c],
                            shadow_lod: shadow_lods[c],
                            shadow_mask: shadow_masks[c],
                            visible: frustum.intersects(&b),
                            dist: camera.clamp(b.min, b.max).distance(camera),
                            bounds: b,
                            holed: g.holed[c],
                        }
                    })
                    .collect(),
            );
            {
                crate::profile_scope!("Grass.Update");
                g.grass
                    .update(device, queue, &t.data, camera - t.position, density);
            }
            let has_grass = crate::terrain::detail_layers::draws_grass(d);
            // (Uploaded again when it went up uncompressed and its blocks are ready now.)
            if has_grass && self.grass_atlas.as_ref().is_none_or(|(a, _)| a.outdated()) {
                crate::profile_scope!("Grass.Atlas");
                self.next_atlas_id += 1;
                let atlas = super::terrain_grass::atlas_array(device, queue, None);
                self.grass_atlas = Some((atlas, self.next_atlas_id));
            }
            let flowers = (has_grass && d.flowers.enabled && !d.flowers.kinds.is_empty())
                .then(|| {
                    flower_atlas(
                        &mut self.flower_atlases,
                        &mut self.next_atlas_id,
                        device,
                        queue,
                        &d.flowers.kinds,
                    )
                })
                .flatten();
            let (atlas_view, atlas_sampler, atlas_id) = match (flowers, &self.grass_atlas) {
                (Some(f), _) => f,
                (None, Some((a, id))) if has_grass => (&a.view, &a.sampler, *id),
                _ => {
                    self.grass_groups.push(None);
                    self.grass_shadow_groups.push(None);
                    self.grass_near_groups.push(None);
                    self.grass_params.push(None);
                    continue;
                }
            };
            if self.grass_cull.is_none() {
                crate::profile_scope!("Grass.CullPipelines");
                let built =
                    (self.grass_cull_prebuilt.take()).unwrap_or_else(|| CullPipelines::new(device));
                self.grass_cull = Some(built);
            }
            let casts = d.details.cast_shadows && t.cast_shadows;
            let near = near_map && casts;
            let params = |shadow: Option<(f32, f32)>, near_radius: f32| {
                GrassParams::new(
                    &d.details,
                    t.position,
                    time,
                    first_draw + i,
                    camera,
                    environment.wind.as_ref(),
                    shadow,
                    [screen[0], screen[1], near_radius, screen[2]],
                    density,
                )
                .with_ground_cover(self.ground_cover.is_some())
                .with_vis_index(i as u32)
                .with_flowers(d.flowers.lod_distance)
                .with_species(&d.detail_layers)
            };
            // (Made once per terrain, rewritten every frame: no buffers or groups to create,
            // track and drop per frame; again when the atlas changes.)
            if g.grass_atlas_id != atlas_id {
                g.grass_uniforms = None;
                g.grass_atlas_id = atlas_id;
            }
            let uniforms = g.grass_uniforms.get_or_insert_with(|| {
                std::array::from_fn(|_| {
                    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("misty grass params"),
                        size: std::mem::size_of::<GrassParams>() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("misty grass group"),
                        layout: grass_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: buffer.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(atlas_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::Sampler(atlas_sampler),
                            },
                        ],
                    });
                    (buffer, group)
                })
            });
            let group = |slot: usize, p: &GrassParams| {
                let (buffer, group) = &uniforms[slot];
                queue.write_buffer(buffer, 0, bytemuck::bytes_of(p));
                group.clone()
            };
            let camera_params = params(None, 0.0);
            // Cascade casters leave the near map's square to it.
            let inner = if near { NEAR_RADIUS } else { 0.0 };
            let shadow = casters.filter(|_| casts).map(|c| params(Some(c), inner));
            // The near map's slices: the blades within reach of each (fading out well past
            // its edge), the inner slice every one, the outer a widened share.
            let near_params: Option<[GrassParams; grass_near_shadow::SLICES]> = near.then(|| {
                std::array::from_fn(|i| {
                    let radius = grass_near_shadow::SLICE_RADII[i];
                    params(
                        Some((
                            radius * grass_near_shadow::CASTER_REACH,
                            grass_near_shadow::SLICE_SHARES[i],
                        )),
                        0.0,
                    )
                    .with_caster_fade(grass_near_shadow::CASTER_FADE)
                })
            });
            self.grass_shadow_groups
                .push(shadow.as_ref().map(|p| group(1, p)));
            self.grass_near_groups.push(near_params.as_ref().map(|p| {
                let mut slot = 1;
                p.each_ref().map(|p| {
                    slot += 1;
                    group(slot, p)
                })
            }));
            self.grass_groups.push(Some(group(0, &camera_params)));
            self.grass_params.push(Some(GrassFrameParams {
                camera: camera_params,
                shadow,
                near: near_params,
            }));
        }
        self.frustum = Some(frustum);
    }

    /// World bounds of a terrain (cached once uploaded).
    pub fn world_bounds(&self, t: &TerrainDraw) -> (Vec3, Vec3) {
        match self.entries.get(&t.key).filter(|g| g.version == t.version) {
            Some(g) => {
                let lo = Vec3::new(0.0, g.range.0 * t.data.size.y, 0.0);
                let hi = Vec3::new(t.data.size.x, g.range.1 * t.data.size.y, t.data.size.z);
                (lo + t.position, hi + t.position + Vec3::Y * tree_top(t))
            }
            None => world_bounds(t),
        }
    }

    /// Uploaded terrains and their GPU bytes (Memory module).
    pub fn memory(&self) -> (usize, u64) {
        let bytes = self.entries.values().map(|g| g.bytes()).sum::<u64>()
            + self.patterns.values().map(|p| p.buffer.size()).sum::<u64>()
            + self.layers.bytes()
            + self.atlas_bytes();
        (self.entries.len(), bytes)
    }

    /// GPU bytes of the terrain layer arrays.
    pub fn layer_bytes(&self) -> u64 {
        self.layers.bytes()
    }

    /// GPU bytes of the grass atlases (built-in grass and the flower layers).
    pub fn atlas_bytes(&self) -> u64 {
        let flowers = self.flower_atlases.values().map(|f| match f {
            FlowerAtlas::Ready { atlas, .. } => atlas.bytes,
            FlowerAtlas::Building(_) => 0,
        });
        self.grass_atlas.as_ref().map_or(0, |(a, _)| a.bytes) + flowers.sum::<u64>()
    }

    /// Draw the frame's terrains `filter(i)` keeps (`first` = draw index of terrain 0).
    /// The camera pass (`camera`) skips chunks outside the view; shadow passes draw them all.
    /// Groups 0–2 and the pipeline are bound by the caller.
    pub fn draw(
        &self,
        pass: &mut wgpu::RenderPass,
        terrains: &[TerrainDraw],
        first: usize,
        camera: bool,
        filter: impl Fn(usize) -> bool,
        stats: &mut RenderStats,
    ) {
        for (i, t) in terrains.iter().enumerate() {
            let (Some(gpu), Some(Some(group)), Some(chunks)) = (
                self.entries.get(&t.key),
                self.groups.get(i),
                self.chunks.get(i),
            ) else {
                continue;
            };
            let Some(p) = self.patterns.get(&gpu.res) else {
                continue;
            };
            if !filter(i) {
                continue;
            }
            pass.set_bind_group(3, group, &[]);
            pass.set_index_buffer(p.buffer.slice(..), wgpu::IndexFormat::Uint32);
            let index = (first + i) as u32;
            // The camera pass draws near chunks first: hills in front hide the ground behind
            // them before it is shaded.
            let mut order: Vec<&ChunkDraw> = chunks
                .iter()
                .filter(|c| (c.visible || !camera) && !c.holed)
                .collect();
            if camera {
                order.sort_by(|a, b| a.dist.total_cmp(&b.dist));
            }
            for c in order {
                let (lod, mask) = if camera {
                    (c.lod, c.mask)
                } else {
                    (c.shadow_lod, c.shadow_mask)
                };
                let range = p.ranges[lod as usize][mask as usize].clone();
                let count = range.len() as u32;
                pass.draw_indexed(range, c.base, index..index + 1);
                stats.draw(1, count, count / 6 + 1);
            }
            // Chunks with holes, from their own index lists.
            let mut bound = false;
            for (ci, c) in chunks.iter().enumerate() {
                if !c.holed || !(c.visible || !camera) {
                    continue;
                }
                let key = if camera {
                    (ci, c.lod, c.mask)
                } else {
                    (ci, c.shadow_lod, c.shadow_mask)
                };
                let (Some(buffer), Some(range)) = (&gpu.holes.buffer, gpu.holes.ranges.get(&key))
                else {
                    continue;
                };
                if !bound {
                    pass.set_index_buffer(buffer.slice(..), wgpu::IndexFormat::Uint32);
                    bound = true;
                }
                let count = range.len() as u32;
                pass.draw_indexed(range.clone(), c.base, index..index + 1);
                stats.draw(1, count, count / 6 + 1);
            }
        }
    }

    /// Draw the chunks of the frame's terrains `filter(i)` keeps that meet `frustum` (a virtual
    /// shadow map page rectangle; same groups and pipeline as [`TerrainCache::draw`]).
    pub fn draw_in(
        &self,
        pass: &mut wgpu::RenderPass,
        terrains: &[TerrainDraw],
        first: usize,
        filter: impl Fn(usize) -> bool,
        frustum: &Frustum,
        stats: &mut RenderStats,
    ) {
        for (i, t) in terrains.iter().enumerate() {
            let (Some(gpu), Some(Some(group)), Some(chunks)) = (
                self.entries.get(&t.key),
                self.groups.get(i),
                self.chunks.get(i),
            ) else {
                continue;
            };
            let Some(p) = self.patterns.get(&gpu.res) else {
                continue;
            };
            if !filter(i) {
                continue;
            }
            let mut bound = false;
            let index = (first + i) as u32;
            for c in chunks
                .iter()
                .filter(|c| !c.holed && frustum.intersects(&c.bounds))
            {
                if !bound {
                    pass.set_bind_group(3, group, &[]);
                    pass.set_index_buffer(p.buffer.slice(..), wgpu::IndexFormat::Uint32);
                    bound = true;
                }
                let range = p.ranges[c.shadow_lod as usize][c.shadow_mask as usize].clone();
                let count = range.len() as u32;
                pass.draw_indexed(range, c.base, index..index + 1);
                stats.draw(1, count, count / 6 + 1);
            }
            // Chunks with holes, from their own index lists.
            let mut holes_bound = false;
            for (ci, c) in chunks.iter().enumerate() {
                if !c.holed || !frustum.intersects(&c.bounds) {
                    continue;
                }
                let key = (ci, c.shadow_lod, c.shadow_mask);
                let (Some(buffer), Some(range)) = (&gpu.holes.buffer, gpu.holes.ranges.get(&key))
                else {
                    continue;
                };
                if !bound {
                    pass.set_bind_group(3, group, &[]);
                    bound = true;
                }
                if !holes_bound {
                    pass.set_index_buffer(buffer.slice(..), wgpu::IndexFormat::Uint32);
                    holes_bound = true;
                }
                let count = range.len() as u32;
                pass.draw_indexed(range.clone(), c.base, index..index + 1);
                stats.draw(1, count, count / 6 + 1);
            }
        }
    }

    /// This frame's chunk detail of every terrain: (key, version, (lod, edge mask) per chunk)
    /// — the virtual shadow map renders the pages under a chunk again when it changes.
    pub fn chunk_states(&self, terrains: &[TerrainDraw]) -> ChunkStates {
        terrains
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let chunks = self.chunks.get(i).map_or(Vec::new(), |c| {
                    c.iter().map(|c| (c.shadow_lod, c.shadow_mask)).collect()
                });
                let pos = t.position;
                let moved = (pos.x.to_bits() as u64)
                    ^ ((pos.y.to_bits() as u64) << 21)
                    ^ ((pos.z.to_bits() as u64) << 42);
                (t.key.clone(), t.version ^ moved.rotate_left(7), chunks)
            })
            .collect()
    }

    /// World bounds of chunk `c` of terrain `i` this frame.
    pub fn chunk_bounds(&self, i: usize, c: usize) -> Option<Aabb> {
        self.chunks.get(i)?.get(c).map(|c| c.bounds)
    }

    /// Plan this frame's grass passes and their GPU culling (after `select`): the camera's
    /// lit and depth passes, the AO prepass within `prepass` metres, the sun cascades
    /// `cascades` (slot, view-projection) that take grass casters and the near grass shadow
    /// map's slices (`near`).
    #[allow(clippy::too_many_arguments)]
    pub fn plan_grass(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        blades_layout: &wgpu::BindGroupLayout,
        terrains: &[TerrainDraw],
        cascades: &[(usize, Mat4)],
        near: Option<&[Mat4; grass_near_shadow::SLICES]>,
        prepass: Option<f32>,
        fans_only: bool,
    ) {
        self.lit_fans_only = false;
        let Some(pipelines) = &self.grass_cull else {
            return;
        };
        // (With the ground cover the lit lane draws no fans at all.)
        self.lit_fans_only = fans_only && self.ground_cover.is_none();
        let Some(frustum) = &self.frustum else {
            return;
        };
        for (i, t) in terrains.iter().enumerate() {
            let (Some(gpu), Some(Some(p))) =
                (self.entries.get_mut(&t.key), self.grass_params.get(i))
            else {
                continue;
            };
            let lod = t.data.details.lod_distance;
            let vp = &self.view_proj;
            let mut passes: [Option<(grass_cull::Kind, Frustum, Option<Limit>)>;
                grass_cull::KINDS] = Default::default();
            // (With the ground cover the lit grass skips the flat fans too.)
            let fans = self.ground_cover.is_none();
            let mut lit = p.camera.cull_kind(vp, true, !fans);
            if self.lit_fans_only {
                lit.lod[1] = 2.0;
            }
            passes[grass_cull::LIT] = Some((lit, *frustum, None));
            passes[grass_cull::DEPTH] = Some((p.camera.cull_kind(vp, true, true), *frustum, None));
            passes[grass_cull::PREPASS] = prepass.map(|range| {
                let limit = Limit {
                    range,
                    share: 1.0,
                    coarse: false,
                };
                (p.camera.cull_kind(vp, true, true), *frustum, Some(limit))
            });
            if let (Some(shadow), Some(casters)) = (&p.shadow, self.grass_casters) {
                for (slot, m) in cascades {
                    if *slot >= 4 {
                        continue;
                    }
                    let limit = Limit {
                        range: casters.0,
                        share: casters.1,
                        coarse: true,
                    };
                    let kind = shadow.cull_kind(m, false, true);
                    passes[grass_cull::CASCADE + slot] =
                        Some((kind, Frustum::from_matrix(m), Some(limit)));
                }
            }
            if let (Some(slices), Some(near)) = (&p.near, near) {
                for (s, m) in near.iter().enumerate() {
                    // Coarse three-segment cards (half the triangles; the arc keeps the shape
                    // close to the drawn blade's).
                    let limit = Limit {
                        range: grass_near_shadow::SLICE_RADII[s] * grass_near_shadow::CASTER_REACH,
                        share: grass_near_shadow::SLICE_SHARES[s],
                        coarse: true,
                    };
                    let kind = slices[s].cull_kind(m, false, true);
                    passes[grass_cull::NEAR + s] =
                        Some((kind, Frustum::from_matrix(m), Some(limit)));
                }
            }
            gpu.grass.plan(
                device,
                queue,
                blades_layout,
                pipelines,
                t.position,
                self.camera,
                lod,
                &passes,
            );
        }
    }

    /// Per terrain (by key): what its grass holds (tests).
    #[cfg(test)]
    pub fn grass_load(&self) -> Vec<(String, super::terrain_grass::GrassLoad)> {
        let mut out: Vec<_> = (self.entries.iter())
            .map(|(k, g)| (k.clone(), g.grass.load()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Per terrain: grass chunks and blades held (tests).
    #[cfg(test)]
    pub fn grass_resident(&self) -> Vec<(usize, u64)> {
        self.entries.values().map(|g| g.grass.resident()).collect()
    }

    /// Blades and density level of grass chunk `chunk` of terrain `key` (None: no chunk held)
    /// (tests).
    #[cfg(test)]
    pub fn grass_chunk(&self, key: &str, chunk: (u32, u32)) -> Option<(u32, u32)> {
        self.entries.get(key)?.grass.chunk(chunk)
    }

    /// Grass of terrain `key` at terrain-local point `p`: the edit count of the blades drawn
    /// there and the edits so far (tests).
    #[cfg(test)]
    pub fn grass_stamps_at(&self, key: &str, p: glam::Vec2) -> Option<(u64, u64)> {
        self.entries.get(key)?.grass.stamps_at(p)
    }

    /// Every chunk of `chunks` holds the blades of the terrain's latest data (tests).
    #[cfg(test)]
    pub fn grass_fresh(&self, key: &str, chunks: &[(u32, u32)]) -> bool {
        self.entries
            .get(key)
            .is_some_and(|g| chunks.iter().all(|c| g.grass.fresh(*c)))
    }

    /// Per terrain: blades each grass pass drew after / before the GPU culling (tests).
    #[cfg(test)]
    pub fn read_grass_cull_counts(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Vec<[(u64, u64); grass_cull::KINDS]> {
        self.entries
            .values()
            .filter_map(|g| g.grass.read_cull_counts(device, queue))
            .collect()
    }

    /// Record this frame's grass culling (before the first pass that draws grass).
    pub fn encode_grass_cull(&self, terrains: &[TerrainDraw], encoder: &mut wgpu::CommandEncoder) {
        let Some(pipelines) = &self.grass_cull else {
            return;
        };
        for (i, t) in terrains.iter().enumerate() {
            if let (Some(gpu), Some(Some(_))) = (self.entries.get(&t.key), self.grass_params.get(i))
            {
                gpu.grass.encode_cull(pipelines, encoder);
            }
        }
    }

    /// Draw the frame's grass for the camera's `kind` of pass (grass_cull::LIT or DEPTH;
    /// pipeline, groups 0 and 1 bound by the caller).
    pub fn draw_grass(
        &self,
        pass: &mut wgpu::RenderPass,
        terrains: &[TerrainDraw],
        kind: usize,
        stats: &mut RenderStats,
    ) {
        for (i, t) in terrains.iter().enumerate() {
            let (Some(gpu), Some(Some(group))) =
                (self.entries.get(&t.key), self.grass_groups.get(i))
            else {
                continue;
            };
            pass.set_bind_group(2, group, &[]);
            gpu.grass.draw(pass, kind, stats);
        }
    }

    /// Draw the frame's grass shadow casters into sun cascade `slot` (pipeline and groups
    /// 0–1 bound by the caller). Only for cascades planned with casters.
    pub fn draw_grass_shadow(
        &self,
        pass: &mut wgpu::RenderPass,
        terrains: &[TerrainDraw],
        slot: usize,
        stats: &mut RenderStats,
    ) {
        if slot >= 4 {
            return;
        }
        for (i, t) in terrains.iter().enumerate() {
            let (Some(gpu), Some(Some(group))) =
                (self.entries.get(&t.key), self.grass_shadow_groups.get(i))
            else {
                continue;
            };
            pass.set_bind_group(2, group, &[]);
            gpu.grass.draw(pass, grass_cull::CASCADE + slot, stats);
        }
    }

    /// Draw the grass near the camera with the camera pass's groups (the screen-space
    /// prepass, as planned; pipeline and groups 0–1 bound by the caller).
    pub fn draw_grass_near(
        &self,
        pass: &mut wgpu::RenderPass,
        terrains: &[TerrainDraw],
        stats: &mut RenderStats,
    ) {
        self.draw_grass(pass, terrains, grass_cull::PREPASS, stats);
    }

    /// Draw the blades of slice `slice` of the near-camera grass shadow map (pipeline and
    /// groups 0–1 bound by the caller): every blade within the inner slice's reach, a widened
    /// share within the outer's.
    pub fn draw_grass_near_shadow(
        &self,
        pass: &mut wgpu::RenderPass,
        terrains: &[TerrainDraw],
        slice: usize,
        stats: &mut RenderStats,
    ) {
        for (i, t) in terrains.iter().enumerate() {
            let (Some(gpu), Some(Some(groups))) =
                (self.entries.get(&t.key), self.grass_near_groups.get(i))
            else {
                continue;
            };
            pass.set_bind_group(2, &groups[slice], &[]);
            gpu.grass.draw(pass, grass_cull::NEAR + slice, stats);
        }
    }

    /// Some terrain casts grass into the near-camera grass shadow map this frame.
    pub fn has_grass_near_shadow(&self) -> bool {
        self.grass_near_groups.iter().any(|g| g.is_some())
    }

    /// Range of this frame's grass shadow casters (None = none cast).
    pub fn grass_caster_range(&self) -> Option<f32> {
        self.grass_shadow_groups
            .iter()
            .any(|g| g.is_some())
            .then_some(self.grass_casters.map(|c| c.0))
            .flatten()
    }

    /// This frame's lit grass can go through the visibility buffer (grass_vis.rs): every
    /// terrain with grass is culled on the GPU with its drawn chunks in its blade pages, and
    /// there are few enough of them for the ids.
    pub fn grass_vis_ready(&self, terrains: &[TerrainDraw]) -> bool {
        terrains.iter().enumerate().all(|(i, t)| {
            let (Some(gpu), Some(Some(_))) = (self.entries.get(&t.key), self.grass_groups.get(i))
            else {
                return true;
            };
            i < super::grass_vis::MAX_TERRAINS && gpu.grass.vis_ready()
        })
    }

    /// The terrains whose grass the visibility resolve shades: their number in the ids,
    /// grass group and blade pages.
    pub fn grass_vis_terrains<'a>(
        &'a self,
        terrains: &[TerrainDraw],
    ) -> Vec<super::grass_vis::ResolveTerrain<'a>> {
        (terrains.iter().enumerate())
            .filter_map(|(i, t)| {
                let gpu = self.entries.get(&t.key)?;
                let group = self.grass_groups.get(i)?.as_ref()?;
                Some(super::grass_vis::ResolveTerrain {
                    index: i as u32,
                    grass_group: group,
                    pages: gpu.grass.pages()?,
                })
            })
            .collect()
    }

    /// Some terrain's grass draws indirectly this frame (culled on the GPU).
    pub fn grass_indirect(&self) -> bool {
        self.entries.values().any(|e| e.grass.indirect())
    }

    /// Any terrain of the frame has grass to draw.
    pub fn has_grass(&self) -> bool {
        self.grass_groups.iter().any(|g| g.is_some())
    }

    /// Grass streaming counters of the last [`TerrainCache::select`], summed over terrains.
    pub fn grass_stats(&self) -> super::GrassStats {
        let mut out = super::GrassStats::default();
        for g in self.entries.values() {
            let s = g.grass.stats;
            out.scheduled += s.scheduled;
            out.uploaded += s.uploaded;
            out.pending += s.pending;
            out.upload_ms += s.upload_ms;
            out.upload_last_start_ms = out.upload_last_start_ms.max(s.upload_last_start_ms);
            out.upload_piece_ms = out.upload_piece_ms.max(s.upload_piece_ms);
            out.upload_alloc_ms = out.upload_alloc_ms.max(s.upload_alloc_ms);
            out.scatter_ms = out.scatter_ms.max(s.scatter_ms);
            out.trimmed += s.trimmed;
            out.refused += s.refused;
            out.coarser = out.coarser.max(s.coarser);
            out.chunks += s.chunks;
            out.blades += s.blades;
        }
        out
    }
}

/// The index lists of the chunks with holes (`wanted`: chunk, lod, edge mask) in one buffer.
#[allow(clippy::too_many_arguments)]
/// World bounds of chunk `c` of a terrain.
fn chunk_bounds(g: &GpuTerrain, t: &TerrainDraw, c: usize) -> Aabb {
    let d = &t.data;
    let cells = terrain_lod::chunk_cells(g.res);
    let per = g.per_side();
    let spacing = d.spacing();
    let (cx, cz) = (c % per, c / per);
    let (lo, hi) = g.chunk_range[c];
    let min = Vec3::new(
        cx as f32 * cells as f32 * spacing.x,
        lo * d.size.y,
        cz as f32 * cells as f32 * spacing.y,
    );
    let max = Vec3::new(
        min.x + cells as f32 * spacing.x,
        hi * d.size.y,
        min.z + cells as f32 * spacing.y,
    );
    Aabb::new(min + t.position, max + t.position)
}

fn hole_indices(
    device: &wgpu::Device,
    p: &Patterns,
    wanted: &[(usize, u8, u8)],
    res: usize,
    per: usize,
    cells: usize,
    d: &TerrainData,
) -> HoleIndices {
    let mut indices: Vec<u32> = Vec::new();
    let mut ranges = HashMap::new();
    for &k @ (c, lod, mask) in wanted {
        if ranges.contains_key(&k) {
            continue;
        }
        let r = p.ranges[lod as usize][mask as usize].clone();
        let pattern = &p.indices[r.start as usize..r.end as usize];
        let start = indices.len() as u32;
        indices.extend(without_holes(
            pattern,
            res,
            (c % per) * cells,
            (c / per) * cells,
            d,
        ));
        ranges.insert(k, start..indices.len() as u32);
    }
    // (A buffer can't be empty: one collapsed triangle.)
    if indices.is_empty() {
        indices.extend([0, 0, 0]);
    }
    HoleIndices {
        ranges,
        buffer: Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("misty terrain hole indices"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
        ),
    }
}

/// Per-draw values of a terrain: translation only, white, receives shadows.
pub(super) fn draw_uniform(t: &TerrainDraw) -> DrawUniform {
    let model = Mat4::from_translation(t.position);
    DrawUniform {
        model: model.to_cols_array_2d(),
        normal: Mat4::IDENTITY.to_cols_array_2d(),
        color: [1.0; 4],
        params: [0.0, 0.0, 1.0, 0.0],
        emission: [0.0, 0.0, 0.0, 1.0],
        uv_st: [1.0, 1.0, 0.0, 0.0],
        surface: [0.0; 4],
        extra: [1.0, 0.0, 0.0, 0.0],
        lightmap_st: [1.0, 1.0, 0.0, 0.0],
        lightmap: [0.0; 4],
        prev_model: model.to_cols_array_2d(),
    }
}

/// World bounds of a terrain (for fitting the directional shadow map).
pub(super) fn world_bounds(t: &TerrainDraw) -> (Vec3, Vec3) {
    let (lo, hi) = t.data.bounds();
    (lo + t.position, hi + t.position + Vec3::Y * tree_top(t))
}

/// Room above the heightmap for the terrain's trees (they cast shadows and get culled with it).
fn tree_top(t: &TerrainDraw) -> f32 {
    if t.data.trees.instances.is_empty() {
        0.0
    } else {
        crate::terrain::trees::MAX_TREE_HEIGHT
    }
}

fn params(
    t: &TerrainDraw,
    means: &[Option<Vec3>; MAX_LAYERS],
    coverage: [f32; MAX_LAYERS],
    light: Option<Vec3>,
    ground_cover: Option<f32>,
) -> TerrainParams {
    let d = &t.data;
    let s = d.spacing();
    let mut p = TerrainParams {
        size: [
            d.size.x,
            d.size.y,
            d.size.z,
            d.layers.len().min(MAX_LAYERS) as f32,
        ],
        grid: [d.heightmap_resolution as f32, s.x, s.y, 0.0],
        tiles: [[10.0, 10.0, 0.0, 0.0]; MAX_LAYERS],
        surface: [[0.0; 4]; MAX_LAYERS],
        harmony: [harmony::IDENTITY; MAX_LAYERS],
        cliff: d.cliffs.uniform(),
        grass_healthy: [0.0; 4],
        grass_dry: [0.0; 4],
        grass_lush: [0.0; 4],
        grass_amounts: [0.0; 4],
        ground_cover: ground_cover_values()[0],
        ground_tint: ground_cover_values()[1],
        ground_front: ground_cover_values()[2],
        chunks: [1.0, 0.0, 0.0, 0.0],
    };
    let g = &d.details;
    if g.enabled && d.detail_map.len() == d.alphamap_resolution.pow(2) {
        let lin = |c: [f32; 3]| super::srgb_to_linear(c);
        let ([hr, hg, hb], [dr, dg, db], [lr, lg, lb]) =
            (lin(g.healthy_color), lin(g.dry_color), lin(g.lush_color));
        p.grass_healthy = [hr, hg, hb, g.ground_tint.clamp(0.0, 1.0)];
        p.grass_dry = [dr, dg, db, g.patch_scale.max(0.5)];
        p.grass_lush = [lr, lg, lb, g.draw_distance.max(1.0)];
        p.grass_amounts = [
            g.lush_amount.clamp(0.0, 1.0),
            g.dry_amount.clamp(0.0, 1.0),
            ground_cover.map_or(0.0, |share| share.max(0.01)),
            g.lod_distance.max(1.0),
        ];
    }
    for (i, l) in d.layers.iter().take(MAX_LAYERS).enumerate() {
        p.tiles[i] = [
            l.tile_size[0].max(0.01),
            l.tile_size[1].max(0.01),
            l.tile_offset[0],
            l.tile_offset[1],
        ];
        let scale = if l.normal_map.is_some() {
            l.normal_scale
        } else {
            0.0
        };
        p.surface[i] = [l.metallic, l.smoothness, scale, l.occlusion.clamp(0.0, 1.0)];
    }
    if d.harmony.enabled {
        let count = d.layers.len().min(MAX_LAYERS);
        let tones: Vec<harmony::LayerTone> = (0..count)
            .map(|i| harmony::LayerTone {
                mean: means[i].unwrap_or(Vec3::splat(0.5)),
                weight: if d.harmony.weight_by_coverage {
                    coverage[i]
                } else {
                    1.0
                },
            })
            .collect();
        p.harmony = harmony::adjustments(&d.harmony, &tones, light);
    }
    p
}

#[cfg(test)]
mod hole_tests {
    use super::*;

    /// A chunk's pattern without its hole cells keeps every other cell's two triangles.
    #[test]
    fn hole_cells_leave_the_chunk_pattern() {
        let mut d = TerrainData::new(129, Vec3::splat(128.0), 128);
        let cells = terrain_lod::chunk_cells(129);
        let (indices, ranges) = terrain_lod::patterns(129, cells);
        let full = &indices[ranges[0][0].start as usize..ranges[0][0].end as usize];
        assert_eq!(
            without_holes(full, 129, 0, 0, &d),
            full,
            "no holes: all kept"
        );
        // Three holes in the second chunk along x, one in the first.
        for (x, z) in [(70, 3), (71, 3), (100, 40), (5, 5)] {
            d.set_hole(x, z, true);
        }
        let kept = without_holes(full, 129, cells, 0, &d);
        assert_eq!(kept.len(), full.len() - 3 * 6);
        let first = without_holes(full, 129, 0, 0, &d);
        assert_eq!(first.len(), full.len() - 6);
        // The cut cell's samples no longer form a triangle there.
        let cell = (5 * 129 + 5) as u32;
        assert!(
            !first
                .chunks(3)
                .any(|t| t.contains(&cell) && t.contains(&(cell + 1)) && t.contains(&(cell + 129))),
            "cell (5, 5) left out"
        );
    }
}
