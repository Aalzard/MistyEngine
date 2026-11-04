//! HDRI sky on the GPU: the background and specular cubes built from the environment's HDRI
//! (`convert`), and the sky uniform (rotation, tint × exposure, diffuse SH). Bound in the view
//! group (bindings 4–7, `SkyEnv` in common.wgsl) so the skybox, lit surfaces, terrain, water
//! and custom shaders all see the same sky. Rebuilt only when the HDRI file changes.

pub mod convert;
#[cfg(test)]
mod gpu_tests;

use bytemuck::{Pod, Zeroable};

use super::Environment;
use super::probes::prefilter::LEVELS;
use crate::hdri::HdriHandle;

/// First view-group binding.
pub const FIRST_BINDING: u32 = 4;

/// `SkyEnv` in common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub(crate) struct SkyUniform {
    /// x = 1 when the sky is an HDRI, y / z = cos / sin of the rotation, w = highest
    /// specular level.
    pub params: [f32; 4],
    /// rgb = linear tint × 2^exposure.
    pub color: [f32; 4],
    /// Diffuse SH (irradiance / π, world space, intensity applied) for unprobed surfaces.
    pub sh: [[f32; 4]; 9],
}

impl SkyUniform {
    pub fn of(env: &Environment) -> Self {
        if env.hdri_sky().is_none() {
            return Self::zeroed();
        }
        let angle = env.settings.skybox.hdri.rotation.to_radians();
        let m = env.hdri_multiplier();
        Self {
            params: [1.0, angle.cos(), angle.sin(), (LEVELS - 1) as f32],
            color: m.extend(1.0).to_array(),
            sh: env.ambient_sh().map(|c| c.extend(0.0).to_array()),
        }
    }
}

/// View-group layout entries 4..=7: uniform, background cube, specular cube, sampler.
pub(crate) fn layout_entries() -> [wgpu::BindGroupLayoutEntry; 4] {
    let entry = |i: u32, ty| wgpu::BindGroupLayoutEntry {
        binding: FIRST_BINDING + i,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty,
        count: None,
    };
    let cube = wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable: true },
        view_dimension: wgpu::TextureViewDimension::Cube,
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
        entry(1, cube),
        entry(2, cube),
        entry(
            3,
            wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        ),
    ]
}

pub(crate) struct SkyGpu {
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    background: wgpu::TextureView,
    specular: wgpu::TextureView,
    /// The HDRI the cubes were built from (None = 1 × 1 black placeholders).
    current: Option<HdriHandle>,
    converter: Option<convert::Converter>,
    /// Last uniform written (skips identical uploads).
    written: Option<SkyUniform>,
    /// Wall time of the last conversion, including the GPU wait (ms).
    pub last_build_ms: Option<f32>,
}

fn cube_view(t: &wgpu::Texture) -> wgpu::TextureView {
    t.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::Cube),
        ..Default::default()
    })
}

fn placeholder(device: &wgpu::Device) -> wgpu::TextureView {
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("misty hdri placeholder"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 6,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: convert::FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    cube_view(&t)
}

impl SkyGpu {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty sky env"),
                size: std::mem::size_of::<SkyUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("misty sky sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
            background: placeholder(device),
            specular: placeholder(device),
            current: None,
            converter: None,
            written: None,
            last_build_ms: None,
        }
    }

    /// View-group entries 4..=7.
    pub fn entries(&self) -> [wgpu::BindGroupEntry<'_>; 4] {
        let at = |i: u32, resource| wgpu::BindGroupEntry {
            binding: FIRST_BINDING + i,
            resource,
        };
        [
            at(0, self.uniform.as_entire_binding()),
            at(1, wgpu::BindingResource::TextureView(&self.background)),
            at(2, wgpu::BindingResource::TextureView(&self.specular)),
            at(3, wgpu::BindingResource::Sampler(&self.sampler)),
        ]
    }

    /// Upload this frame's sky; builds the cubes when the HDRI changed. True when the view
    /// group must be rebuilt (new texture views).
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        env: &Environment,
    ) -> bool {
        let u = SkyUniform::of(env);
        if self.written != Some(u) {
            queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
            self.written = Some(u);
        }
        let wanted = env.hdri_sky().and(env.hdri.as_ref());
        if wanted == self.current.as_ref() {
            return false;
        }
        match wanted {
            Some(h) => {
                let started = std::time::Instant::now();
                let converter = self
                    .converter
                    .get_or_insert_with(|| convert::Converter::new(device));
                let cubes = converter.build(device, queue, &h.0.image);
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                self.last_build_ms = Some(started.elapsed().as_secs_f32() * 1000.0);
                self.background = cube_view(&cubes.background);
                self.specular = cube_view(&cubes.specular);
            }
            None => {
                self.background = placeholder(device);
                self.specular = placeholder(device);
            }
        }
        self.current = wanted.cloned();
        true
    }

    /// Cube memory in bytes.
    pub fn bytes(&self) -> u64 {
        let Some(h) = &self.current else {
            return 0;
        };
        let face = convert::face_size(h.0.image.width) as u64;
        let spec = convert::SPECULAR_FACE as u64;
        (face * face + spec * spec) * 6 * 8 * 4 / 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_size_matches_wgsl() {
        assert_eq!(std::mem::size_of::<SkyUniform>(), 16 * 11);
    }
}
