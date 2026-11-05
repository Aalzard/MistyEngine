//! GPU side of the probes: a cube-array atlas (one cube per probe, all roughness levels), the
//! probe uniform, each draw's short probe list and the split-sum BRDF LUT. Bound in the lit mesh
//! group (bindings 20–24, see `probes.wgsl`). Cubes are uploaded once per probe data and kept
//! while later frames use them.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use super::cube::Cube;
use super::prefilter::LEVELS;
use super::{ProbeHandle, ProbeInstance, brdf};
use crate::render::Frame;
use crate::render::culling::Aabb;

/// Probes one view can use (the shader's array size).
pub const MAX_PROBES: usize = 64;
/// Probes blended per draw.
pub const PER_DRAW: usize = 4;
/// First binding in the mesh group.
pub const FIRST_BINDING: u32 = 20;
const NONE: u32 = u32::MAX;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ProbeItem {
    /// xyz = capture point, w = intensity.
    position: [f32; 4],
    /// w = blend distance.
    box_min: [f32; 4],
    /// w = 1 with box projection.
    box_max: [f32; 4],
    /// x = atlas cube, y = importance, z = index (debug colour).
    params: [f32; 4],
    sh: [[f32; 4]; 9],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ProbesUniform {
    /// x = count, y = debug view, z = highest roughness level.
    header: [f32; 4],
    items: [ProbeItem; MAX_PROBES],
}

pub(crate) struct ProbeGpu {
    uniform: wgpu::Buffer,
    lists: wgpu::Buffer,
    lists_capacity: usize,
    atlas: wgpu::Texture,
    atlas_view: wgpu::TextureView,
    atlas_size: u32,
    /// Data held by each atlas cube.
    slots: Vec<Option<ProbeHandle>>,
    sampler: wgpu::Sampler,
    lut: wgpu::Texture,
    lut_view: wgpu::TextureView,
    lut_ready: bool,
    scratch: Box<ProbesUniform>,
    /// Light probes: the tetrahedron each mesh draw was in last frame (walk start).
    light_hints: Vec<u32>,
}

/// Layout entries for the mesh group.
pub(crate) fn layout_entries() -> [wgpu::BindGroupLayoutEntry; 5] {
    use wgpu::ShaderStages as S;
    let entry = |i: u32, ty| wgpu::BindGroupLayoutEntry {
        binding: FIRST_BINDING + i,
        visibility: S::FRAGMENT,
        ty,
        count: None,
    };
    let float = |view_dimension| wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable: true },
        view_dimension,
        multisampled: false,
    };
    [
        entry(
            0,
            wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
        ),
        entry(1, float(wgpu::TextureViewDimension::CubeArray)),
        entry(
            2,
            wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        ),
        entry(
            3,
            wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
        ),
        entry(4, float(wgpu::TextureViewDimension::D2)),
    ]
}

fn atlas(device: &wgpu::Device, size: u32, cubes: usize) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("misty probe atlas"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6 * cubes as u32,
        },
        mip_level_count: LEVELS as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("misty probe atlas view"),
        dimension: Some(wgpu::TextureViewDimension::CubeArray),
        ..Default::default()
    });
    (texture, view)
}

fn lists_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("misty probe lists"),
        size: (capacity * 16) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// RGBA16F bytes of a cube face.
fn face_bytes(texels: &[Vec3]) -> Vec<u8> {
    let halves: Vec<half::f16> = texels
        .iter()
        .flat_map(|c| [c.x, c.y, c.z, 1.0].map(half::f16::from_f32))
        .collect();
    bytemuck::cast_slice(&halves).to_vec()
}

impl ProbeGpu {
    pub fn new(device: &wgpu::Device) -> Self {
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty probes"),
            size: std::mem::size_of::<ProbesUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (atlas, atlas_view) = atlas(device, 64, 1);
        let lut = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("misty brdf lut"),
            size: wgpu::Extent3d {
                width: brdf::LUT_SIZE,
                height: brdf::LUT_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let lut_view = lut.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty probe sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Self {
            uniform,
            lists: lists_buffer(device, 256),
            lists_capacity: 256,
            atlas,
            atlas_view,
            atlas_size: 64,
            slots: vec![None],
            sampler,
            lut,
            lut_view,
            lut_ready: false,
            scratch: Box::new(ProbesUniform::zeroed()),
            light_hints: Vec::new(),
        }
    }

    /// Entries for the mesh bind group.
    pub fn entries(&self) -> [wgpu::BindGroupEntry<'_>; 5] {
        let at = |i: u32, resource| wgpu::BindGroupEntry {
            binding: FIRST_BINDING + i,
            resource,
        };
        [
            at(0, self.uniform.as_entire_binding()),
            at(1, wgpu::BindingResource::TextureView(&self.atlas_view)),
            at(2, wgpu::BindingResource::Sampler(&self.sampler)),
            at(3, self.lists.as_entire_binding()),
            at(4, wgpu::BindingResource::TextureView(&self.lut_view)),
        ]
    }

    /// Atlas, list and LUT memory.
    pub fn bytes(&self) -> u64 {
        let s = self.atlas_size as u64;
        s * s * 8 * 6 * self.slots.len() as u64 * 4 / 3 + self.lists.size() + self.uniform.size()
    }

    /// Upload this frame's probes and per-draw lists. True when the bind group must be rebuilt
    /// (the atlas or list buffer was reallocated).
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        meshes: &[Option<Aabb>],
        terrains: &[Aabb],
    ) -> bool {
        if !self.lut_ready {
            self.upload_lut(queue);
        }
        let set = &frame.environment.probes;
        let probes = &set.probes[..set.probes.len().min(MAX_PROBES)];
        let mut rebuilt = self.fit_atlas(device, probes);
        let cubes: Vec<usize> = probes
            .iter()
            .map(|p| self.cube_for(queue, probes, &p.data))
            .collect();

        let draws = meshes.len() + terrains.len();
        // Light probes only while some draw uses them.
        let light = set
            .light_probes
            .as_ref()
            .filter(|_| frame.meshes.iter().any(|m| m.material.light_probes));
        let u = &mut *self.scratch;
        u.header = [
            probes.len() as f32,
            set.debug as u32 as f32,
            (LEVELS - 1) as f32,
            light.map_or(0.0, |_| draws as f32),
        ];
        for (i, (p, cube)) in probes.iter().zip(&cubes).enumerate() {
            let sh = p.data.0.sh.map(|c| c.extend(0.0).to_array());
            u.items[i] = ProbeItem {
                position: p.position.extend(p.intensity).to_array(),
                box_min: p.box_min.extend(p.blend_distance).to_array(),
                box_max: p.box_max.extend(p.box_projection as u32 as f32).to_array(),
                params: [*cube as f32, p.importance as f32, i as f32, 0.0],
                sh,
            };
        }
        let used = 16 + probes.len() * std::mem::size_of::<ProbeItem>();
        queue.write_buffer(&self.uniform, 0, &bytemuck::bytes_of(u)[..used]);
        if probes.is_empty() && light.is_none() {
            return rebuilt;
        }

        let mut lists = if probes.is_empty() {
            vec![[NONE; 4]; draws]
        } else {
            draw_lists(probes, meshes, terrains)
        };
        if let Some(field) = light {
            light_probe_entries(
                &field.0,
                frame.meshes,
                meshes,
                terrains.len(),
                &mut self.light_hints,
                &mut lists,
            );
        }
        if lists.len() > self.lists_capacity {
            self.lists_capacity = lists.len().next_power_of_two();
            self.lists = lists_buffer(device, self.lists_capacity);
            rebuilt = true;
        }
        if !lists.is_empty() {
            queue.write_buffer(&self.lists, 0, bytemuck::cast_slice(&lists));
        }
        rebuilt
    }

    fn upload_lut(&mut self, queue: &wgpu::Queue) {
        let data = brdf::lut_rgba16();
        queue.write_texture(
            self.lut.as_image_copy(),
            bytemuck::cast_slice(&data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(brdf::LUT_SIZE * 8),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: brdf::LUT_SIZE,
                height: brdf::LUT_SIZE,
                depth_or_array_layers: 1,
            },
        );
        self.lut_ready = true;
    }

    /// Reallocate the atlas when the probes need bigger faces or more cubes.
    fn fit_atlas(&mut self, device: &wgpu::Device, probes: &[ProbeInstance]) -> bool {
        let Some(size) = probes.iter().map(|p| p.data.0.size().clamp(64, 512)).max() else {
            return false;
        };
        if size == self.atlas_size && probes.len() <= self.slots.len() {
            return false;
        }
        let cubes = probes.len().max(self.slots.len()).next_power_of_two();
        (self.atlas, self.atlas_view) = atlas(device, size, cubes);
        self.atlas_size = size;
        self.slots = vec![None; cubes];
        true
    }

    /// Atlas cube holding `data`, uploading it into a cube no other probe of this frame uses.
    fn cube_for(
        &mut self,
        queue: &wgpu::Queue,
        probes: &[ProbeInstance],
        data: &ProbeHandle,
    ) -> usize {
        if let Some(i) = self.slots.iter().position(|s| s.as_ref() == Some(data)) {
            return i;
        }
        let in_use = |s: &Option<ProbeHandle>| {
            s.as_ref()
                .is_some_and(|h| probes.iter().any(|p| &p.data == h))
        };
        let i = self
            .slots
            .iter()
            .position(|s| !in_use(s))
            .expect("the atlas has a cube per probe");
        self.upload(queue, i, &data.0);
        self.slots[i] = Some(data.clone());
        i
    }

    fn upload(&self, queue: &wgpu::Queue, cube: usize, data: &super::ProbeData) {
        for mip in 0..LEVELS {
            let size = (self.atlas_size >> mip).max(1);
            let level: Cube = match data.levels.get(mip) {
                Some(l) if l.size == size => l.clone(),
                Some(l) => l.resized(size),
                None => Cube::new(size),
            };
            for (face, texels) in level.faces.iter().enumerate() {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.atlas,
                        mip_level: mip as u32,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: 0,
                            z: (cube * 6 + face) as u32,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &face_bytes(texels),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(size * 8),
                        rows_per_image: None,
                    },
                    wgpu::Extent3d {
                        width: size,
                        height: size,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
    }
}

/// Up to [`PER_DRAW`] probes per draw whose boxes touch the draw's bounds: most important first,
/// then the smaller box. Draws without bounds get none.
pub(crate) fn draw_lists(
    probes: &[ProbeInstance],
    meshes: &[Option<Aabb>],
    terrains: &[Aabb],
) -> Vec<[u32; 4]> {
    let mut order: Vec<usize> = (0..probes.len()).collect();
    order.sort_by(|&a, &b| {
        let (pa, pb) = (&probes[a], &probes[b]);
        pb.importance
            .cmp(&pa.importance)
            .then(pa.volume().total_cmp(&pb.volume()))
    });
    let touches =
        |p: &ProbeInstance, b: &Aabb| p.box_min.cmple(b.max).all() && p.box_max.cmpge(b.min).all();
    let bounds = meshes
        .iter()
        .copied()
        .chain(terrains.iter().map(|b| Some(*b)));
    bounds
        .map(|b| {
            let mut list = [NONE; 4];
            if let Some(b) = b {
                let hits = order.iter().filter(|&&i| touches(&probes[i], &b));
                for (slot, &i) in list.iter_mut().zip(hits.take(PER_DRAW)) {
                    *slot = i as u32;
                }
            }
            list
        })
        .collect()
}

/// Light probes (after the draws' probe lists in the same buffer): one entry per draw, `x` =
/// index of its SH block or [`NONE`] (ambient light), then 9 coefficients (f32 bits, irradiance
/// / π) per block. Flagged mesh draws sample the probes at their bounds centre, walking from
/// the tetrahedron they were in last frame.
pub(crate) fn light_probe_entries(
    field: &crate::light_probes::LightProbeField,
    draws: &[crate::render::MeshDraw],
    bounds: &[Option<Aabb>],
    terrains: usize,
    hints: &mut Vec<u32>,
    out: &mut Vec<[u32; 4]>,
) {
    hints.resize(draws.len(), 0);
    let mut blocks: Vec<[u32; 4]> = Vec::new();
    let mut entries = Vec::with_capacity(draws.len() + terrains);
    for (i, d) in draws.iter().enumerate() {
        let sh = d.material.light_probes.then(|| {
            let at = match bounds.get(i).copied().flatten() {
                Some(b) => (b.min + b.max) * 0.5,
                None => d.model.w_axis.truncate(),
            };
            field.sample(at, &mut hints[i])
        });
        match sh.flatten() {
            Some(sh) => {
                entries.push([(blocks.len() / 9) as u32, 0, 0, 0]);
                blocks.extend(sh.map(|c| c.extend(0.0).to_array().map(f32::to_bits)));
            }
            None => entries.push([NONE; 4]),
        }
    }
    entries.resize(draws.len() + terrains, [NONE; 4]);
    out.extend(entries);
    out.extend(blocks);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn probe(lo: f32, hi: f32, importance: i32) -> ProbeInstance {
        let data = Arc::new(super::super::ProbeData {
            levels: vec![Cube::new(1)],
            sh: [Vec3::ZERO; 9],
        });
        ProbeInstance {
            position: Vec3::splat((lo + hi) * 0.5),
            box_min: Vec3::splat(lo),
            box_max: Vec3::splat(hi),
            blend_distance: 1.0,
            importance,
            intensity: 1.0,
            box_projection: true,
            data: ProbeHandle(data),
        }
    }

    #[test]
    fn gpu_struct_sizes_match_wgsl() {
        assert_eq!(std::mem::size_of::<ProbeItem>(), 13 * 16);
        assert_eq!(
            std::mem::size_of::<ProbesUniform>(),
            16 + 13 * 16 * MAX_PROBES
        );
    }

    #[test]
    fn lists_sort_by_importance_then_size() {
        let probes = [
            probe(-10.0, 10.0, 0),
            probe(-1.0, 1.0, 0),
            probe(-5.0, 5.0, 2),
        ];
        let at = |c: f32| Some(Aabb::new(Vec3::splat(c - 0.1), Vec3::splat(c + 0.1)));
        let lists = draw_lists(&probes, &[at(0.0), at(8.0), None], &[]);
        assert_eq!(lists[0], [2, 1, 0, NONE]);
        assert_eq!(lists[1], [0, NONE, NONE, NONE]);
        assert_eq!(lists[2], [NONE; 4]);
    }
}
