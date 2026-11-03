//! Terrain drawing: one GPU mesh and splat texture per terrain asset (rebuilt when the asset's
//! version changes) and a per-frame bind group with the layer textures (group 3 of the terrain
//! pipeline). Terrains share the mesh pipeline's per-draw storage, lights and shadow maps.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use super::texture::TextureCache;
use super::{DrawUniform, GpuMesh};
use crate::terrain::{MAX_LAYERS, TerrainDraw};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TerrainParams {
    /// xyz = size, w = layer count.
    size: [f32; 4],
    /// Per layer: xy = tile size, zw = tile offset.
    tiles: [[f32; 4]; MAX_LAYERS],
    /// Per layer: x = metallic, y = smoothness, z = normal scale (0 = no normal map).
    surface: [[f32; 4]; MAX_LAYERS],
}

struct GpuTerrain {
    version: u64,
    mesh: GpuMesh,
    splat: wgpu::TextureView,
}

pub(super) struct TerrainCache {
    entries: HashMap<String, GpuTerrain>,
    splat_sampler: wgpu::Sampler,
    /// Bind group per terrain of the current frame (None = data not uploaded).
    groups: Vec<Option<wgpu::BindGroup>>,
}

impl TerrainCache {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            entries: HashMap::new(),
            splat_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("misty splat sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            groups: Vec::new(),
        }
    }

    /// Upload changed terrains and build this frame's bind groups.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        textures: &mut TextureCache,
        terrains: &[TerrainDraw],
    ) {
        self.groups.clear();
        for t in terrains {
            if self
                .entries
                .get(&t.key)
                .is_none_or(|g| g.version != t.version)
            {
                let gpu = GpuTerrain {
                    version: t.version,
                    mesh: GpuMesh::new(device, &t.key, &crate::terrain::mesh::build(&t.data)),
                    splat: splat_texture(device, queue, &t.key, &t.data),
                };
                self.entries.insert(t.key.clone(), gpu);
            }
            let gpu = &self.entries[&t.key];
            let params = params(t);
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("misty terrain params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let mut views = Vec::with_capacity(MAX_LAYERS * 2);
            let mut normals = Vec::with_capacity(MAX_LAYERS);
            for i in 0..MAX_LAYERS {
                let layer = t.data.layers.get(i);
                let (albedo, normal) = textures.views(
                    device,
                    queue,
                    layer.and_then(|l| l.diffuse.as_deref()),
                    layer.and_then(|l| l.normal_map.as_deref()),
                );
                views.push(albedo);
                normals.push(normal);
            }
            views.extend(normals);
            let mut entries = vec![
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&gpu.splat),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.splat_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 11,
                    resource: wgpu::BindingResource::Sampler(textures.sampler()),
                },
            ];
            for (i, v) in views.iter().enumerate() {
                entries.push(wgpu::BindGroupEntry {
                    binding: 3 + i as u32,
                    resource: wgpu::BindingResource::TextureView(v),
                });
            }
            self.groups
                .push(Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("misty terrain group"),
                    layout,
                    entries: &entries,
                })));
        }
    }

    /// Draw the frame's terrains (`first` = draw index of terrain 0). `textured` binds group 3.
    pub fn draw(
        &self,
        pass: &mut wgpu::RenderPass,
        terrains: &[TerrainDraw],
        first: usize,
        textured: bool,
        filter: impl Fn(&TerrainDraw) -> bool,
    ) {
        for (i, t) in terrains.iter().enumerate() {
            let Some(gpu) = self.entries.get(&t.key) else {
                continue;
            };
            if !filter(t) {
                continue;
            }
            if textured {
                let Some(Some(group)) = self.groups.get(i) else {
                    continue;
                };
                pass.set_bind_group(3, group, &[]);
            }
            let m = &gpu.mesh;
            pass.set_vertex_buffer(0, m.vertices.slice(..));
            pass.set_index_buffer(m.indices.slice(..), wgpu::IndexFormat::Uint32);
            let index = (first + i) as u32;
            pass.draw_indexed(0..m.count, 0, index..index + 1);
        }
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

fn params(t: &TerrainDraw) -> TerrainParams {
    let d = &t.data;
    let mut p = TerrainParams {
        size: [
            d.size.x,
            d.size.y,
            d.size.z,
            d.layers.len().min(MAX_LAYERS) as f32,
        ],
        tiles: [[10.0, 10.0, 0.0, 0.0]; MAX_LAYERS],
        surface: [[0.0; 4]; MAX_LAYERS],
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
    p
}

/// The splat map as an RGBA texture (row 0 = z 0).
fn splat_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    d: &crate::terrain::TerrainData,
) -> wgpu::TextureView {
    let n = d.alphamap_resolution as u32;
    let bytes: Vec<u8> = d.alphamap.iter().flatten().copied().collect();
    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: n,
                    height: n,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &bytes,
        )
        .create_view(&Default::default())
}
