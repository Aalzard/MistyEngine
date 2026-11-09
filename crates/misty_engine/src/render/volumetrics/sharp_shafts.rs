//! Sharp volumetric sun shafts: the main sun's in-scatter is raymarched per pixel (half or
//! quarter resolution) through its cascade shadow map instead of the coarse froxel grid, then
//! accumulated over frames with a tight neighbourhood clamp and added onto the HDR image with a
//! depth-aware upsample. The froxels keep the sun's extinction, the ambient light and every
//! other light (they skip the sun while this runs, see `Froxels::sun`).

use bytemuck::{Pod, Zeroable};

use super::froxels::{self, entry, texture};
use super::settings::ShaftQuality;
use crate::render::pipelines::{self, Spec};
use crate::render::{Frame, HDR_FORMAT};

const COMMON_SRC: &str = include_str!("shaders/sharp_common.wgsl");
const MARCH_SRC: &str = include_str!("shaders/sharp_shafts.wgsl");
const COMPOSITE_SRC: &str = include_str!("shaders/sharp_composite.wgsl");
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Weight of the reprojected history (after clamping).
const HISTORY_WEIGHT: f32 = 0.85;
/// A march sample reads the first sun cascade whose texel is at least this share of the
/// march texel's footprint at the sample (performance round 22, `fast_paths::shaft_cascades`).
/// The tightest cascade's finer texels only alias at the march's spacing (the temporal
/// accumulation averages them away), and reading a 4096² cascade there was most of the
/// pass's cost on Ultra.
const FOOTPRINT_SHARE: f32 = 0.5;

/// Compute source (after the froxel common code).
pub(super) fn compute_source() -> String {
    format!("{}\n{COMMON_SRC}\n{MARCH_SRC}", froxels::MEDIUM_SRC)
}

/// Composite source (after the forward common code).
pub(super) fn composite_source() -> String {
    format!("{COMMON_SRC}\n{COMPOSITE_SRC}")
}

/// `SharpShafts` in sharp_common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct SharpUniform {
    pub size: [f32; 4],
    pub params: [f32; 4],
    pub range: [f32; 4],
    pub cascades: [f32; 4],
}

impl SharpUniform {
    /// `reach` = froxel reach (view depth); `history` = false on the first frame of a view;
    /// `cascades` = [`cascade_thresholds`].
    pub fn new(
        frame: &Frame,
        tex: &SharpTextures,
        q: ShaftQuality,
        reach: f32,
        cascades: [f32; 4],
    ) -> Self {
        let denoise = frame.post.fog.volume.denoising;
        let weight = if denoise && tex.has_history {
            HISTORY_WEIGHT
        } else {
            0.0
        };
        Self {
            size: [
                tex.size.0 as f32,
                tex.size.1 as f32,
                tex.full.0 as f32,
                tex.full.1 as f32,
            ],
            params: [
                q.steps as f32,
                (tex.frame % 4096) as f32,
                weight,
                q.scale as f32,
            ],
            range: [
                reach,
                frame.camera.near,
                frame.camera.far,
                if denoise { 1.0 } else { 0.0 },
            ],
            cascades,
        }
    }
}

/// Per sun cascade (nearest first, from its texel size in metres): the march texel footprint
/// past which a sample may read the next cascade (`cascades` in sharp_common.wgsl). Never
/// past one when `texels` is `None` or the shortcut is off.
pub(super) fn cascade_thresholds(texels: Option<&[f32]>) -> [f32; 4] {
    let mut out = [f32::MAX; 4];
    if let Some(texels) = texels.filter(|_| crate::render::fast_paths::shaft_cascades()) {
        for (o, t) in out.iter_mut().zip(texels) {
            *o = t / FOOTPRINT_SHARE;
        }
    }
    out
}

/// March size for a full-resolution target.
pub(super) fn march_size(full: (u32, u32), scale: u32) -> (u32, u32) {
    (full.0.div_ceil(scale).max(1), full.1.div_ceil(scale).max(1))
}

/// Per-view images: this frame's raw march and two accumulation buffers (ping-pong).
pub(super) struct SharpTextures {
    pub size: (u32, u32),
    pub full: (u32, u32),
    raw: wgpu::TextureView,
    accum: [wgpu::TextureView; 2],
    /// Which of `accum` is written this frame.
    parity: usize,
    frame: u32,
    has_history: bool,
}

impl SharpTextures {
    pub fn new(device: &wgpu::Device, full: (u32, u32), scale: u32) -> Self {
        let size = march_size(full, scale);
        let make = |label| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size.0,
                        height: size.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::STORAGE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        Self {
            size,
            full,
            raw: make("misty sharp shafts raw"),
            accum: [make("misty sharp shafts a"), make("misty sharp shafts b")],
            parity: 0,
            frame: 0,
            has_history: false,
        }
    }

    /// Start a frame: swap the accumulation buffers (call once per render, before the uniform).
    pub fn begin_frame(&mut self) {
        if self.frame > 0 {
            self.parity = 1 - self.parity;
            self.has_history = true;
        }
        self.frame = self.frame.wrapping_add(1);
    }

    /// (read = last frame, write = this frame).
    fn pair(&self) -> (&wgpu::TextureView, &wgpu::TextureView) {
        (&self.accum[1 - self.parity], &self.accum[self.parity])
    }

    pub fn bytes(&self) -> u64 {
        3 * 8 * self.size.0 as u64 * self.size.1 as u64
    }
}

pub(super) struct SharpPipelines {
    march_layout: wgpu::BindGroupLayout,
    accum_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    march: wgpu::ComputePipeline,
    accumulate: wgpu::ComputePipeline,
    composite: wgpu::RenderPipeline,
    blue_noise: wgpu::TextureView,
    pub uniform: wgpu::Buffer,
}

/// Resources of one frame's shafts.
pub(super) struct Inputs<'a> {
    pub froxel_uniform: &'a wgpu::Buffer,
    pub linear: &'a wgpu::Sampler,
    pub shadow_cmp: &'a wgpu::Sampler,
    pub sun_shadows: &'a wgpu::TextureView,
    /// Cloud shadow map and its `CloudApply` uniform.
    pub clouds: (&'a wgpu::TextureView, &'a wgpu::Buffer),
    pub depth: &'a wgpu::TextureView,
    /// This frame's lit froxels (extinction with the fog noise).
    pub froxel_lit: &'a wgpu::TextureView,
}

fn storage_2d() -> wgpu::BindingType {
    wgpu::BindingType::StorageTexture {
        access: wgpu::StorageTextureAccess::WriteOnly,
        format: FORMAT,
        view_dimension: wgpu::TextureViewDimension::D2,
    }
}

fn uniform() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

fn depth_ms() -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Depth,
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled: true,
    }
}

fn blue_noise_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    use crate::render::clouds::noise::{BLUE_SIZE, blue_noise};
    let n = BLUE_SIZE as u32;
    let size = wgpu::Extent3d {
        width: n,
        height: n,
        depth_or_array_layers: 1,
    };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("misty sharp shafts blue noise"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        tex.as_image_copy(),
        &blue_noise(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(n),
            rows_per_image: None,
        },
        size,
    );
    tex.create_view(&Default::default())
}

impl SharpPipelines {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        use wgpu::TextureSampleType as T;
        use wgpu::TextureViewDimension as D;
        let float = T::Float { filterable: true };
        let sampler = |kind| wgpu::BindingType::Sampler(kind);
        let layout = |label, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            })
        };
        let march_layout = layout(
            "misty sharp shafts march",
            &[
                entry(0, uniform()),
                entry(2, sampler(wgpu::SamplerBindingType::Filtering)),
                entry(4, texture(T::Depth, D::D2Array)),
                entry(6, sampler(wgpu::SamplerBindingType::Comparison)),
                entry(7, texture(float, D::D2)),
                entry(8, uniform()),
                entry(9, uniform()),
                entry(10, depth_ms()),
                entry(11, texture(float, D::D2)),
                entry(12, storage_2d()),
                entry(16, texture(float, D::D3)),
            ],
        );
        let accum_layout = layout(
            "misty sharp shafts accumulate",
            &[
                entry(0, uniform()),
                entry(2, sampler(wgpu::SamplerBindingType::Filtering)),
                entry(9, uniform()),
                entry(13, texture(float, D::D2)),
                entry(14, texture(float, D::D2)),
                entry(15, storage_2d()),
            ],
        );
        let fragment = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty,
            count: None,
        };
        let composite_layout = layout(
            "misty sharp shafts composite",
            &[
                fragment(0, texture(float, D::D2)),
                fragment(1, depth_ms()),
                fragment(2, uniform()),
            ],
        );
        let src = compute_source();
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let composite_src = composite_source();
        let composite = pipelines::build(
            device,
            Spec {
                depth: None,
                samples: 1,
                blend: Some(add),
                color: Some(HDR_FORMAT),
                glow: None,
                ..Spec::new(
                    "sharp sun shafts composite",
                    &composite_src,
                    &[view_layout, &composite_layout],
                )
            },
        );
        Self {
            march: froxels::compute(device, "misty sharp shafts", &src, "march", &march_layout),
            accumulate: froxels::compute(
                device,
                "misty sharp shafts accumulate",
                &src,
                "accumulate",
                &accum_layout,
            ),
            composite,
            march_layout,
            accum_layout,
            composite_layout,
            blue_noise: blue_noise_texture(device, queue),
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty sharp shafts"),
                size: std::mem::size_of::<SharpUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        }
    }

    /// March and accumulate (compute), then return the composite's pipeline and bind group
    /// (a fullscreen pass adding onto the HDR image).
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        tex: &SharpTextures,
        i: &Inputs,
    ) -> (&wgpu::RenderPipeline, wgpu::BindGroup) {
        let view = wgpu::BindingResource::TextureView;
        let sampler = wgpu::BindingResource::Sampler;
        let group = |layout, entries: &[wgpu::BindGroupEntry]| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty sharp shafts group"),
                layout,
                entries,
            })
        };
        let e = |binding, resource| wgpu::BindGroupEntry { binding, resource };
        let (read, write) = tex.pair();
        let march = group(
            &self.march_layout,
            &[
                e(0, i.froxel_uniform.as_entire_binding()),
                e(2, sampler(i.linear)),
                e(4, view(i.sun_shadows)),
                e(6, sampler(i.shadow_cmp)),
                e(7, view(i.clouds.0)),
                e(8, i.clouds.1.as_entire_binding()),
                e(9, self.uniform.as_entire_binding()),
                e(10, view(i.depth)),
                e(11, view(&self.blue_noise)),
                e(12, view(&tex.raw)),
                e(16, view(i.froxel_lit)),
            ],
        );
        let accumulate = group(
            &self.accum_layout,
            &[
                e(0, i.froxel_uniform.as_entire_binding()),
                e(2, sampler(i.linear)),
                e(9, self.uniform.as_entire_binding()),
                e(13, view(&tex.raw)),
                e(14, view(read)),
                e(15, view(write)),
            ],
        );
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("misty sharp sun shafts"),
                timestamp_writes: None,
            });
            let (w, h) = (tex.size.0.div_ceil(8), tex.size.1.div_ceil(8));
            pass.set_pipeline(&self.march);
            pass.set_bind_group(0, &march, &[]);
            pass.dispatch_workgroups(w, h, 1);
            pass.set_pipeline(&self.accumulate);
            pass.set_bind_group(0, &accumulate, &[]);
            pass.dispatch_workgroups(w, h, 1);
        }
        let composite = group(
            &self.composite_layout,
            &[
                e(0, view(write)),
                e(1, view(i.depth)),
                e(2, self.uniform.as_entire_binding()),
            ],
        );
        (&self.composite, composite)
    }
}
