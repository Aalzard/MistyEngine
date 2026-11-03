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
use super::terrain_grass::{Grass, GrassParams};
use super::terrain_layers::LayerCache;
use super::terrain_lod;
use super::{DrawUniform, RenderStats};
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
}

/// One chunk drawn this frame.
#[derive(Clone, Copy)]
struct ChunkDraw {
    base: i32,
    lod: u8,
    mask: u8,
    /// Inside the camera frustum (shadow passes draw every chunk).
    visible: bool,
}

/// Shared index patterns for one heightmap resolution.
struct Patterns {
    buffer: wgpu::Buffer,
    ranges: Vec<[std::ops::Range<u32>; 16]>,
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
    crate::terrain::auto_paint::par_rows(&mut out, w, |row, texels| {
        let iz = zs.start + row;
        for (i, t) in texels.iter_mut().enumerate() {
            let n = d.vertex_normal(xs.start + i, iz);
            *t = [enc(n.x), enc(n.y), enc(n.z), 0];
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
        }
    }

    fn per_side(&self) -> usize {
        (self.res - 1) / terrain_lod::chunk_cells(self.res)
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
            }
        }
    }

    fn bytes(&self) -> u64 {
        (self.res * self.res * 8 + self.ares * self.ares * 4) as u64 + self.grass.bytes()
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
    frustum: Option<Frustum>,
    started: Instant,
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
            frustum: None,
            started: Instant::now(),
        }
    }

    /// Bring a terrain's GPU data up to date with its store version.
    fn sync(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, t: &TerrainDraw) {
        let d = &t.data;
        let stale = self
            .entries
            .get(&t.key)
            .is_none_or(|g| g.res != d.heightmap_resolution || g.ares != d.alphamap_resolution);
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
            }
        });
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
        for t in terrains {
            self.sync(device, queue, t);
            let layers = self.layers.get(device, queue, &t.data.layers);
            let gpu = &self.entries[&t.key];
            let params = params(t, &layers.means, gpu.coverage, light);
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("misty terrain params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let view = |t: &wgpu::Texture| t.create_view(&Default::default());
            let (heights, normals, splat) =
                (view(&gpu.heights), view(&gpu.normals), view(&gpu.splat));
            let tex = wgpu::BindingResource::TextureView;
            let entries = [
                (0, buffer.as_entire_binding()),
                (1, tex(&splat)),
                (2, wgpu::BindingResource::Sampler(&self.splat_sampler)),
                (3, tex(&layers.albedo)),
                (4, tex(&layers.normal)),
                (5, tex(&layers.height)),
                (6, wgpu::BindingResource::Sampler(textures.sampler())),
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

    /// Pick each chunk's LOD and visibility for the camera, and update grass around it.
    pub fn select(
        &mut self,
        device: &wgpu::Device,
        camera: Vec3,
        view_proj: &Mat4,
        terrains: &[TerrainDraw],
        grass_layout: &wgpu::BindGroupLayout,
    ) {
        crate::profile_scope!("Terrain.Select");
        let frustum = Frustum::from_matrix(view_proj);
        self.chunks.clear();
        self.grass_groups.clear();
        let time = self.started.elapsed().as_secs_f32();
        for t in terrains {
            let Some(g) = self.entries.get_mut(&t.key) else {
                self.chunks.push(Vec::new());
                self.grass_groups.push(None);
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
            let lods = terrain_lod::select(per, chunk_m, terrain_lod::max_lod(cells), |c| {
                let b = bounds(c);
                camera.clamp(b.min, b.max).distance(camera)
            });
            let masks = terrain_lod::masks(per, &lods);
            self.chunks.push(
                (0..per * per)
                    .map(|c| ChunkDraw {
                        base: ((c / per) * cells * g.res + (c % per) * cells) as i32,
                        lod: lods[c],
                        mask: masks[c],
                        visible: frustum.intersects(&bounds(c)),
                    })
                    .collect(),
            );
            g.grass.update(device, d, camera - t.position);
            let grass = (d.details.enabled && !d.detail_map.is_empty()).then(|| {
                let p = GrassParams::new(&d.details, t.position, time);
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("misty grass params"),
                    contents: bytemuck::bytes_of(&p),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("misty grass group"),
                    layout: grass_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    }],
                })
            });
            self.grass_groups.push(grass);
        }
        self.frustum = Some(frustum);
    }

    /// World bounds of a terrain (cached once uploaded).
    pub fn world_bounds(&self, t: &TerrainDraw) -> (Vec3, Vec3) {
        match self.entries.get(&t.key).filter(|g| g.version == t.version) {
            Some(g) => {
                let lo = Vec3::new(0.0, g.range.0 * t.data.size.y, 0.0);
                let hi = Vec3::new(t.data.size.x, g.range.1 * t.data.size.y, t.data.size.z);
                (lo + t.position, hi + t.position)
            }
            None => world_bounds(t),
        }
    }

    /// Uploaded terrains and their GPU bytes (Memory module).
    pub fn memory(&self) -> (usize, u64) {
        let bytes = self.entries.values().map(|g| g.bytes()).sum::<u64>()
            + self.patterns.values().map(|p| p.buffer.size()).sum::<u64>()
            + self.layers.bytes();
        (self.entries.len(), bytes)
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
            for c in chunks.iter().filter(|c| c.visible || !camera) {
                let range = p.ranges[c.lod as usize][c.mask as usize].clone();
                let count = range.len() as u32;
                pass.draw_indexed(range, c.base, index..index + 1);
                stats.draw(1, count, count / 6 + 1);
            }
        }
    }

    /// Draw the frame's grass (camera pass; pipeline, groups 0 and 1 bound by the caller).
    pub fn draw_grass(
        &self,
        pass: &mut wgpu::RenderPass,
        terrains: &[TerrainDraw],
        stats: &mut RenderStats,
    ) {
        let Some(frustum) = &self.frustum else {
            return;
        };
        for (i, t) in terrains.iter().enumerate() {
            let (Some(gpu), Some(Some(group))) =
                (self.entries.get(&t.key), self.grass_groups.get(i))
            else {
                continue;
            };
            pass.set_bind_group(2, group, &[]);
            gpu.grass.draw(pass, t.position, frustum, stats);
        }
    }

    /// Any terrain of the frame has grass to draw.
    pub fn has_grass(&self) -> bool {
        self.grass_groups.iter().any(|g| g.is_some())
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
    }
}

/// World bounds of a terrain (for fitting the directional shadow map).
pub(super) fn world_bounds(t: &TerrainDraw) -> (Vec3, Vec3) {
    let (lo, hi) = t.data.bounds();
    (lo + t.position, hi + t.position)
}

fn params(
    t: &TerrainDraw,
    means: &[Option<Vec3>; MAX_LAYERS],
    coverage: [f32; MAX_LAYERS],
    light: Option<Vec3>,
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
    };
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
        p.surface[i] = [l.metallic, l.smoothness, scale, 0.0];
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
