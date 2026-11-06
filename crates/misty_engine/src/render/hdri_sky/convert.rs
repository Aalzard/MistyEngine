//! GPU conversion of an equirect HDRI into the two sky cubes: the background (full box-filtered
//! mip chain) and the specular cube (GGX roughness per mip, like a reflection probe). Runs once
//! per HDRI file in its own submission (`convert.wgsl`).

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::hdri::HdrImage;
use crate::render::probes::prefilter::{LEVELS, level_roughness};

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Largest background face (512² per face ≈ 17 MB with mips).
pub const MAX_FACE: u32 = 512;
/// Face size of the specular cube's sharpest level.
pub const SPECULAR_FACE: u32 = 128;
/// GGX samples per specular texel.
pub const SAMPLES: u32 = 512;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    a: [f32; 4],
    b: [f32; 4],
}

/// The two cubes built from one HDRI.
pub struct SkyCubes {
    pub background: wgpu::Texture,
    pub specular: wgpu::Texture,
}

/// The conversion pipelines (created on the first HDRI).
pub struct Converter {
    to_cube: wgpu::ComputePipeline,
    downsample: wgpu::ComputePipeline,
    prefilter: wgpu::ComputePipeline,
    sampler: wgpu::Sampler,
}

/// Background face size for an image `width` wide: a quarter of it, a power of two.
pub fn face_size(width: u32) -> u32 {
    (width / 4).next_power_of_two().clamp(64, MAX_FACE)
}

fn cube_texture(device: &wgpu::Device, label: &str, size: u32, mips: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
        view_formats: &[],
    })
}

/// View of one mip (`count` = 1) or all mips, as a cube or a 2D array.
fn view(
    t: &wgpu::Texture,
    dim: wgpu::TextureViewDimension,
    mip: u32,
    count: Option<u32>,
) -> wgpu::TextureView {
    t.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(dim),
        base_mip_level: mip,
        mip_level_count: count,
        ..Default::default()
    })
}

impl Converter {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("misty hdri convert"),
            source: wgpu::ShaderSource::Wgsl(include_str!("convert.wgsl").into()),
        });
        let pipeline = |entry: &str| {
            crate::render::pipelines::note_pipeline();
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            to_cube: pipeline("to_cube"),
            downsample: pipeline("downsample"),
            prefilter: pipeline("prefilter"),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("misty hdri convert sampler"),
                address_mode_u: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
        }
    }

    /// Upload `img` and build both cubes (submitted before returning).
    pub fn build(&self, device: &wgpu::Device, queue: &wgpu::Queue, img: &HdrImage) -> SkyCubes {
        use wgpu::TextureViewDimension::{Cube, D2Array};
        let equirect = upload(device, queue, img);
        let equirect_view = equirect.create_view(&Default::default());
        let size = face_size(img.width);
        let mips = size.ilog2() + 1;
        let background = cube_texture(device, "misty hdri background", size, mips);
        let specular = cube_texture(device, "misty hdri specular", SPECULAR_FACE, LEVELS as u32);
        let mut encoder = device.create_command_encoder(&Default::default());

        // Mip 0 from the equirect, then the box chain.
        let dst = view(&background, D2Array, 0, Some(1));
        let params = self.params(device, [size as f32, 0.0, 0.0, 0.0], [0.0; 4]);
        let group = self.group(
            device,
            &self.to_cube,
            &[
                (0, params.as_entire_binding()),
                (1, wgpu::BindingResource::TextureView(&equirect_view)),
                (4, wgpu::BindingResource::Sampler(&self.sampler)),
                (5, wgpu::BindingResource::TextureView(&dst)),
            ],
        );
        dispatch(&mut encoder, &self.to_cube, &group, size);
        for mip in 1..mips {
            let level = (size >> mip).max(1);
            let src = view(&background, D2Array, mip - 1, Some(1));
            let dst = view(&background, D2Array, mip, Some(1));
            let params = self.params(device, [level as f32, 0.0, 0.0, 0.0], [0.0; 4]);
            let group = self.group(
                device,
                &self.downsample,
                &[
                    (0, params.as_entire_binding()),
                    (3, wgpu::BindingResource::TextureView(&src)),
                    (5, wgpu::BindingResource::TextureView(&dst)),
                ],
            );
            dispatch(&mut encoder, &self.downsample, &group, level);
        }

        // Roughness levels read the whole background chain.
        let src = view(&background, Cube, 0, None);
        let base_mip = (size / SPECULAR_FACE).max(1).ilog2() as f32;
        for level in 0..LEVELS as u32 {
            let out = (SPECULAR_FACE >> level).max(1);
            let dst = view(&specular, D2Array, level, Some(1));
            let rough = level_roughness(level as usize);
            let params = self.params(
                device,
                [out as f32, rough, SAMPLES as f32, size as f32],
                [base_mip, mips as f32, 0.0, 0.0],
            );
            let group = self.group(
                device,
                &self.prefilter,
                &[
                    (0, params.as_entire_binding()),
                    (2, wgpu::BindingResource::TextureView(&src)),
                    (4, wgpu::BindingResource::Sampler(&self.sampler)),
                    (5, wgpu::BindingResource::TextureView(&dst)),
                ],
            );
            dispatch(&mut encoder, &self.prefilter, &group, out);
        }
        queue.submit([encoder.finish()]);
        SkyCubes {
            background,
            specular,
        }
    }

    fn params(&self, device: &wgpu::Device, a: [f32; 4], b: [f32; 4]) -> wgpu::Buffer {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("misty hdri params"),
            contents: bytemuck::bytes_of(&Params { a, b }),
            usage: wgpu::BufferUsages::UNIFORM,
        })
    }

    fn group(
        &self,
        device: &wgpu::Device,
        pipeline: &wgpu::ComputePipeline,
        entries: &[(u32, wgpu::BindingResource<'_>)],
    ) -> wgpu::BindGroup {
        let entries: Vec<wgpu::BindGroupEntry> = entries
            .iter()
            .map(|(binding, resource)| wgpu::BindGroupEntry {
                binding: *binding,
                resource: resource.clone(),
            })
            .collect();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty hdri convert"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        })
    }
}

fn dispatch(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    group: &wgpu::BindGroup,
    size: u32,
) {
    let mut pass = encoder.begin_compute_pass(&Default::default());
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    let n = size.div_ceil(8);
    pass.dispatch_workgroups(n, n, 6);
}

/// The equirect image as an RGBA16F texture.
fn upload(device: &wgpu::Device, queue: &wgpu::Queue, img: &HdrImage) -> wgpu::Texture {
    let halves: Vec<half::f16> = img
        .pixels
        .iter()
        .flat_map(|c| [c.x, c.y, c.z, 1.0].map(half::f16::from_f32))
        .collect();
    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("misty hdri equirect"),
            size: wgpu::Extent3d {
                width: img.width,
                height: img.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        bytemuck::cast_slice(&halves),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn face_sizes() {
        assert_eq!(super::face_size(2048), 512);
        assert_eq!(super::face_size(1000), 256);
        assert_eq!(super::face_size(8192), super::MAX_FACE);
        assert_eq!(super::face_size(16), 64);
    }
}
