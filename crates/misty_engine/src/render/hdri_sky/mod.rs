//! HDRI sky on the GPU: the background and specular cubes built from the environment's HDRI
//! (`convert`), and the sky uniform (rotation, tint × exposure, diffuse SH). Bound in the view
//! group (bindings 4–7, `SkyEnv` in common.wgsl) so the skybox, lit surfaces, terrain, water
//! and custom shaders all see the same sky. Rebuilt only when the HDRI file changes.
//! The uniform also carries the moon and night sky, and the group the moon's texture
//! (binding 11, see render/moon.rs).

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
    /// The moon and night sky (render/moon.rs).
    pub moon: super::moon::MoonUniform,
}

impl SkyUniform {
    /// `moon` = the moon texture bound (None: the placeholder, no disc).
    pub fn of(env: &Environment, moon: Option<&crate::moon::texture::MoonImage>) -> Self {
        let moon = super::moon::uniform(env, moon);
        if env.hdri_sky().is_none() {
            return Self {
                moon,
                ..Self::zeroed()
            };
        }
        let angle = env.settings.skybox.hdri.rotation.to_radians();
        let m = env.hdri_multiplier();
        Self {
            params: [1.0, angle.cos(), angle.sin(), (LEVELS - 1) as f32],
            color: m.extend(1.0).to_array(),
            sh: env.ambient_sh().map(|c| c.extend(0.0).to_array()),
            moon,
        }
    }
}

/// View-group layout entry of the moon texture (fragment only: sky.wgsl).
pub(crate) fn moon_layout_entry() -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: super::moon::BINDING,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// The moon texture: the built-in or a project image, uploaded once the moon can show.
struct MoonTexture {
    view: wgpu::TextureView,
    /// What is uploaded: (texture setting, image); None = the 1 × 1 placeholder.
    current: Option<(Option<String>, crate::moon::texture::MoonImage)>,
    /// When a project image was last checked for changes on disk, and its time stamp.
    checked: Option<(std::time::Instant, Option<std::time::SystemTime>)>,
}

fn moon_placeholder(device: &wgpu::Device) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("misty moon placeholder"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

impl MoonTexture {
    /// Upload the wanted texture; true when the view changed.
    fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, env: &Environment) -> bool {
        if !super::moon::wanted(env) {
            return false;
        }
        let rel = env.settings.moon.texture.clone();
        let same = self.current.as_ref().is_some_and(|(r, _)| *r == rel);
        if same {
            // A project image is read again when its file changes (checked twice a second).
            let Some(r) = rel.as_deref() else {
                return false;
            };
            if self
                .checked
                .is_some_and(|(at, _)| at.elapsed() < std::time::Duration::from_millis(500))
            {
                return false;
            }
            let stamp = crate::vfs::modified(&crate::assets::absolute(r));
            let old = self.checked.and_then(|(_, s)| s);
            self.checked = Some((std::time::Instant::now(), stamp));
            if stamp == old {
                return false;
            }
        }
        let image = match rel.as_deref().map(crate::moon::texture::load) {
            Some(Ok(img)) => img,
            Some(Err(e)) => {
                // (Once per change of the setting or the file.)
                eprintln!("moon texture: {e}; drawing the built-in moon");
                crate::moon::texture::built_in()
            }
            None => crate::moon::texture::built_in(),
        };
        let stamp = rel
            .as_deref()
            .and_then(|r| crate::vfs::modified(&crate::assets::absolute(r)));
        self.checked = Some((std::time::Instant::now(), stamp));
        self.view = upload_mips(device, queue, &image);
        self.current = Some((rel, image));
        true
    }
}

fn upload_mips(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image: &crate::moon::texture::MoonImage,
) -> wgpu::TextureView {
    use wgpu::util::DeviceExt;
    let data: Vec<u8> = image
        .mips
        .iter()
        .flat_map(|m| m.as_raw().iter().copied())
        .collect();
    let base = &image.mips[0];
    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("misty moon"),
                size: wgpu::Extent3d {
                    width: base.width(),
                    height: base.height(),
                    depth_or_array_layers: 1,
                },
                mip_level_count: image.mips.len() as u32,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &data,
        )
        .create_view(&Default::default())
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
    moon: MoonTexture,
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
            moon: MoonTexture {
                view: moon_placeholder(device),
                current: None,
                checked: None,
            },
        }
    }

    /// View-group entries 4..=7 and the moon texture.
    pub fn entries(&self) -> [wgpu::BindGroupEntry<'_>; 5] {
        let at = |i: u32, resource| wgpu::BindGroupEntry {
            binding: FIRST_BINDING + i,
            resource,
        };
        [
            at(0, self.uniform.as_entire_binding()),
            at(1, wgpu::BindingResource::TextureView(&self.background)),
            at(2, wgpu::BindingResource::TextureView(&self.specular)),
            at(3, wgpu::BindingResource::Sampler(&self.sampler)),
            wgpu::BindGroupEntry {
                binding: super::moon::BINDING,
                resource: wgpu::BindingResource::TextureView(&self.moon.view),
            },
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
        let moon_changed = self.moon.prepare(device, queue, env);
        let u = SkyUniform::of(env, self.moon.current.as_ref().map(|(_, m)| m));
        if self.written != Some(u) {
            queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
            self.written = Some(u);
        }
        let wanted = env.hdri_sky().and(env.hdri.as_ref());
        if wanted == self.current.as_ref() {
            return moon_changed;
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

    /// Cube and moon texture memory in bytes.
    pub fn bytes(&self) -> u64 {
        let moon = self.moon.current.as_ref().map_or(0, |(_, m)| m.bytes());
        let Some(h) = &self.current else {
            return moon;
        };
        let face = convert::face_size(h.0.image.width) as u64;
        let spec = convert::SPECULAR_FACE as u64;
        (face * face + spec * spec) * 6 * 8 * 4 / 3 + moon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_size_matches_wgsl() {
        assert_eq!(std::mem::size_of::<SkyUniform>(), 16 * 19);
    }
}
