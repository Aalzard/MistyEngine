//! Baked lightmaps on the GPU: one rgba16f texture array (a layer per lightmap) at binding 25 of
//! the mesh group, plus a small uniform at 26 (x = debug view, y = layer count, z = bit mask
//! of loaded layers). Draws pick their layer and uv2 scale-offset through `Draw.lightmap` /
//! `Draw.lightmap_st` (draw.wgsl); mesh.wgsl samples with the probe sampler.

use crate::lightmap::{LightmapHandle, LightmapSet};

pub(crate) const TEXTURE_BINDING: u32 = 25;
pub(crate) const UNIFORM_BINDING: u32 = 26;
/// Layers addressable through the uniform's bit mask.
pub(crate) const MAX_LAYERS: usize = 32;

pub(crate) struct LightmapGpu {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    uniform: wgpu::Buffer,
    size: u32,
    layers: u32,
    uploaded: Vec<Option<LightmapHandle>>,
    info: [u32; 4],
}

pub(crate) fn layout_entries() -> [wgpu::BindGroupLayoutEntry; 2] {
    [
        wgpu::BindGroupLayoutEntry {
            binding: TEXTURE_BINDING,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: UNIFORM_BINDING,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
    ]
}

fn array(device: &wgpu::Device, size: u32, layers: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("misty lightmaps"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("misty lightmaps view"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    (texture, view)
}

impl LightmapGpu {
    pub fn new(device: &wgpu::Device) -> Self {
        let (texture, view) = array(device, 1, 1);
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty lightmap info"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            texture,
            view,
            uniform,
            size: 1,
            layers: 1,
            uploaded: Vec::new(),
            info: [u32::MAX; 4],
        }
    }

    pub fn entries(&self) -> [wgpu::BindGroupEntry<'_>; 2] {
        [
            wgpu::BindGroupEntry {
                binding: TEXTURE_BINDING,
                resource: wgpu::BindingResource::TextureView(&self.view),
            },
            wgpu::BindGroupEntry {
                binding: UNIFORM_BINDING,
                resource: self.uniform.as_entire_binding(),
            },
        ]
    }

    pub fn bytes(&self) -> u64 {
        self.size as u64 * self.size as u64 * 8 * self.layers as u64
    }

    /// Upload this frame's lightmaps. True when the texture was reallocated (rebuild the
    /// bind group).
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        set: &LightmapSet,
    ) -> bool {
        let maps = &set.maps[..set.maps.len().min(MAX_LAYERS)];
        // Every layer has the first loaded map's size; others are left out.
        let size = maps
            .iter()
            .flatten()
            .map(|m| m.0.width.max(m.0.height))
            .next()
            .unwrap_or(1);
        let layers = (maps.len() as u32).max(1);
        let mut rebuilt = false;
        if size != self.size || layers != self.layers {
            (self.texture, self.view) = array(device, size, layers);
            self.size = size;
            self.layers = layers;
            self.uploaded.clear();
            rebuilt = true;
        }
        let mut mask = 0u32;
        self.uploaded.resize(maps.len(), None);
        for (i, m) in maps.iter().enumerate() {
            let Some(m) = m
                .as_ref()
                .filter(|m| m.0.width == size && m.0.height == size)
            else {
                self.uploaded[i] = None;
                continue;
            };
            mask |= 1 << i;
            if self.uploaded[i].as_ref() == Some(m) {
                continue;
            }
            let texels = m.0.rgba_f16();
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: i as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&texels),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size * 8),
                    rows_per_image: Some(size),
                },
                wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
            );
            self.uploaded[i] = Some(m.clone());
        }
        let info = [set.debug.index() as u32, maps.len() as u32, mask, 0];
        if info != self.info {
            queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&info));
            self.info = info;
        }
        rebuilt
    }
}
