//! Temporal anti-aliasing (Quality ▸ Temporal Anti-aliasing): the camera's projection is
//! offset by a sub-pixel Halton (2, 3) jitter each frame, and `shaders/taa.wgsl` blends the
//! resolved HDR image with a history reprojected through the main pass's motion vectors
//! (see [`super::motion`]), clipped to the variance of the current neighbourhood and
//! sharpened so a still scene stays crisp. It runs before bloom and tonemapping, so
//! everything after sees the converged image. Each render target keeps its own history
//! (Scene and Game views). Quality ▸ TAA Blend and TAA Sharpness tune the blend.
//!
//! **Temporal upscaling** (Quality ▸ Upscaling, `fs_taau`): an upscaled target renders the
//! scene at a share of the output resolution. The jitter is a sub-pixel offset of the
//! *internal* pixel and cycles through more positions the lower the scale (8 / scale², so
//! every output pixel gets samples near its centre), the history is kept at the output
//! resolution, and each output pixel is rebuilt from the 3×3 internal samples around it:
//! a sharp kernel in output pixels (a Gaussian, σ ≈ 0.33 output px, see [`KERNEL`])
//! decides how much of this frame goes in — a sample right at the pixel's centre counts a
//! lot, one an internal pixel away hardly at all — so the history converges to the full
//! resolution instead of a stretched low-resolution image. Where the history can't be used
//! (first frame, off screen, disoccluded per the depth test, a camera cut) the pixel falls
//! back to a smooth spatial upsample (σ ≈ 0.43 internal px). The neighbourhood clip, the
//! nearest-depth velocity dilation and the motion-dependent blend work as in native TAA.
//! The sharpening pass (stronger the lower the scale, [`sharpen_gain`]) then writes the
//! output-resolution image that post-processing reads.
//! Vendor upscalers (FSR / DLSS / XeSS) would take the same [`Resolve`] inputs (jittered
//! colour, depth, motion vectors, the jitter and the reset flag) as another backend
//! (`crate::quality::UpscalerBackend`).

use std::collections::HashMap;

use bytemuck::Zeroable;
use glam::{Mat4, Vec2};
use wgpu::util::DeviceExt;

use super::HDR_FORMAT;
use super::pipelines::{Spec, build};
use crate::camera::{Camera, Projection};

const SRC: &str = include_str!("shaders/taa.wgsl");
/// Jitter positions before the pattern repeats.
const PHASES: u32 = 8;
/// Histories of targets not rendered for this many renders are dropped.
const STALE_AFTER: u64 = 600;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TaaParams {
    inv_view_proj: [[f32; 4]; 4],
    prev_view_proj: [[f32; 4]; 4],
    info: [f32; 4],
    /// xy = 1 / output size, zw = output size.
    texel: [f32; 4],
    clip: [f32; 4],
    /// xy = internal (render) size, zw = this frame's jitter in internal pixels.
    upscale: [f32; 4],
    /// Upscaler: x = [`KERNEL`], y = [`WEIGHT_GAIN`], z = [`WEIGHT_CAP`], w = sharpening
    /// gain ([`sharpen_gain`]).
    tune: [f32; 4],
}

/// How the history is blended (Quality ▸ TAA Blend / TAA Sharpness).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TaaSettings {
    /// Share of the current frame in the blend (lower = smoother, slower to react).
    pub blend: f32,
    /// Unsharp mask applied to the reprojected history (0 = none).
    pub sharpness: f32,
}

struct History {
    size: (u32, u32),
    /// Ping-pong history images; `current` is the one written this frame.
    views: [wgpu::TextureView; 2],
    current: usize,
    /// A history image holds a converged frame that can be reprojected.
    valid: bool,
    frame: u32,
    used: u64,
}

pub(super) struct Taa {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    /// Temporal upscaler: internal-resolution input, output-resolution history.
    upscale: wgpu::RenderPipeline,
    /// Bilinear stretch of the internal image to the output (upscaled target, no temporal
    /// resolve this frame: wireframe views, frame debugger replays).
    stretch: wgpu::RenderPipeline,
    /// History → image through the unsharp mask.
    sharpen_layout: wgpu::BindGroupLayout,
    sharpen: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    histories: HashMap<u64, History>,
    renders: u64,
}

/// Upscaler reconstruction kernel `exp(-KERNEL d²)`, d in output pixels: σ ≈ 0.33 px, about
/// the footprint a native pixel's jittered samples cover (σ of a 1 px box ≈ 0.29). A wider
/// kernel (Blackman-Harris' 2.29, σ 0.47) measured visibly softer grass at the same shimmer.
const KERNEL: f32 = 4.6;
/// This frame's share in an upscaled pixel = TAA Blend × the nearest sample's kernel weight ×
/// (output pixels per internal pixel)^WEIGHT_GAIN. 1 would make the average share over a
/// jitter cycle equal native TAA's, but lets single well-placed samples jump the pixel by
/// 40 % at 50 % scale: thin grass in the wind then flickered in 2.3 % of the pixels (native
/// without TAA: 0.2 %). 0.5 keeps it at native's level; 0 is steadier still but softer.
const WEIGHT_GAIN: f32 = 0.5;
/// Most of this frame an upscaled pixel takes.
const WEIGHT_CAP: f32 = 0.85;

/// Sharpening gain of the history → image pass for a target rendering at `scale` of its
/// output: 1 at native resolution, rising to 1.6 at 67 % and below (the upscaled image is
/// softer over 1–2 output pixels; the pass stays clamped to the neighbourhood, no halos).
fn sharpen_gain(scale: f32) -> f32 {
    1.0 + 0.6 * ((1.0 - scale) / 0.33).clamp(0.0, 1.0)
}

/// Halton sequence value `i` (1-based) in `base`, 0..1.
fn halton(mut i: u32, base: u32) -> f32 {
    let (mut f, mut r) = (1.0, 0.0);
    while i > 0 {
        f /= base as f32;
        r += f * (i % base) as f32;
        i /= base;
    }
    r
}

/// Sub-pixel offset of frame `n` in pixels (-0.5..0.5), cycling through `phases` positions.
pub(super) fn jitter_pixels(n: u32, phases: u32) -> Vec2 {
    let i = n % phases.max(1) + 1;
    Vec2::new(halton(i, 2), halton(i, 3)) - 0.5
}

/// Jitter positions per cycle for a target rendering at `scale` of its output: 8 at native
/// resolution, 8 / scale² below it (each internal pixel covers 1 / scale² output pixels,
/// and each of those needs samples near its centre), at most 64.
pub(super) fn jitter_phases(scale: f32) -> u32 {
    let s = scale.clamp(0.25, 1.0);
    ((PHASES as f32 / (s * s)).round() as u32).clamp(PHASES, 64)
}

/// A sub-pixel offset of `pixels` on a `size` image as an NDC translation.
pub(super) fn jitter_ndc(pixels: Vec2, size: (u32, u32)) -> Vec2 {
    Vec2::new(
        pixels.x * 2.0 / size.0.max(1) as f32,
        -pixels.y * 2.0 / size.1.max(1) as f32,
    )
}

/// `view_proj` moved by `pixels` on a `size` image (a clip-space translation, so it works
/// for perspective and orthographic cameras alike). No jitter returns the matrix unchanged,
/// bit for bit.
pub(super) fn jittered(view_proj: Mat4, pixels: Vec2, size: (u32, u32)) -> Mat4 {
    if pixels == Vec2::ZERO {
        return view_proj;
    }
    Mat4::from_translation(jitter_ndc(pixels, size).extend(0.0)) * view_proj
}

/// What one resolve reads (also what a vendor upscaler would need).
pub(super) struct Resolve<'a> {
    pub view_group: &'a wgpu::BindGroup,
    pub target: u64,
    /// The HDR image (texture + view) at the internal resolution.
    pub current: (&'a wgpu::Texture, &'a wgpu::TextureView),
    /// Where the result goes and its size: the HDR image itself for native TAA, the
    /// target's output-resolution HDR image when upscaling.
    pub output: (&'a wgpu::TextureView, (u32, u32)),
    /// This frame's jitter in internal pixels.
    pub jitter: Vec2,
    pub depth: &'a wgpu::TextureView,
    /// Motion vectors of the main pass.
    pub velocity: &'a wgpu::TextureView,
    /// This frame's unjittered view-projection and last frame's.
    pub view_proj: Mat4,
    pub prev_view_proj: Mat4,
    /// The camera moved continuously since last frame (no cut).
    pub continuous: bool,
    pub camera: &'a Camera,
    pub settings: TaaSettings,
}

impl Taa {
    /// `view_layout` = group 0 of fullscreen passes.
    pub(super) fn new(device: &wgpu::Device, view_layout: &wgpu::BindGroupLayout) -> Self {
        use wgpu::ShaderStages as S;
        let tex = |binding, sample_type, multisampled| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: S::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled,
            },
            count: None,
        };
        let float = wgpu::TextureSampleType::Float { filterable: true };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty taa"),
            entries: &[
                tex(0, float, false),
                tex(1, wgpu::TextureSampleType::Depth, true),
                tex(2, float, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: S::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: S::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex(
                    5,
                    wgpu::TextureSampleType::Float { filterable: false },
                    true,
                ),
            ],
        });
        let pipeline = build(
            device,
            Spec {
                fs: Some("fs_taa"),
                depth: None,
                samples: 1,
                color: Some(HDR_FORMAT),
                glow: None,
                ..Spec::new("taa", SRC, &[view_layout, &layout])
            },
        );
        let upscale = build(
            device,
            Spec {
                fs: Some("fs_taau"),
                depth: None,
                samples: 1,
                color: Some(HDR_FORMAT),
                glow: None,
                ..Spec::new("taau", SRC, &[view_layout, &layout])
            },
        );
        let sharpen_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty taa sharpen"),
            entries: &[
                tex(0, float, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: S::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: S::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let stretch = build(
            device,
            Spec {
                fs: Some("fs_taa_stretch"),
                depth: None,
                samples: 1,
                color: Some(HDR_FORMAT),
                glow: None,
                ..Spec::new("taa stretch", SRC, &[view_layout, &sharpen_layout])
            },
        );
        let sharpen = build(
            device,
            Spec {
                fs: Some("fs_taa_sharpen"),
                depth: None,
                samples: 1,
                color: Some(HDR_FORMAT),
                glow: None,
                ..Spec::new("taa sharpen", SRC, &[view_layout, &sharpen_layout])
            },
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty taa sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            layout,
            pipeline,
            upscale,
            stretch,
            sharpen_layout,
            sharpen,
            sampler,
            histories: HashMap::new(),
            renders: 0,
        }
    }

    /// This frame's jitter in (internal) pixels for `target`, which renders at `scale` of its
    /// output (zero before its first history).
    pub(super) fn jitter(&self, target: u64, scale: f32) -> Vec2 {
        self.histories
            .get(&target)
            .map_or(Vec2::ZERO, |h| jitter_pixels(h.frame, jitter_phases(scale)))
    }

    /// Drop every history (e.g. TAA was switched off).
    pub(super) fn clear(&mut self) {
        self.histories.clear();
    }

    /// Resolve the HDR image with the history into the next history image, then write that
    /// to the output (the HDR image itself, or the upscaled image) through the sharpening
    /// pass.
    pub(super) fn resolve(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        r: &Resolve,
    ) {
        self.renders += 1;
        let renders = self.renders;
        self.histories.retain(|_, h| renders - h.used < STALE_AFTER);
        let input = (r.current.0.width(), r.current.0.height());
        // The history lives at the output resolution.
        let size = r.output.1;
        let upscaling = input != size;
        let h = self
            .histories
            .entry(r.target)
            .or_insert_with(|| History::new(device, size));
        if h.size != size {
            *h = History::new(device, size);
        }
        h.used = renders;
        let valid = h.valid && r.continuous;
        let cam = r.camera;
        let ortho = matches!(cam.projection, Projection::Orthographic { .. });
        let params = TaaParams {
            inv_view_proj: r.view_proj.inverse().to_cols_array_2d(),
            prev_view_proj: r.prev_view_proj.to_cols_array_2d(),
            info: [
                if valid { 1.0 } else { 0.0 },
                r.settings.blend.clamp(0.02, 1.0),
                r.settings.sharpness.clamp(0.0, 1.0),
                if ortho { 1.0 } else { 0.0 },
            ],
            texel: [
                1.0 / size.0 as f32,
                1.0 / size.1 as f32,
                size.0 as f32,
                size.1 as f32,
            ],
            clip: [cam.near, cam.far, 0.0, 0.0],
            upscale: [input.0 as f32, input.1 as f32, r.jitter.x, r.jitter.y],
            tune: [
                KERNEL,
                WEIGHT_GAIN,
                WEIGHT_CAP,
                sharpen_gain(input.0 as f32 / size.0 as f32),
            ],
        };
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("misty taa params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let (read, write) = (1 - h.current, h.current);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty taa group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(r.current.1),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(r.depth),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&h.views[read]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(r.velocity),
                },
            ],
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("misty taa"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &h.views[write],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(if upscaling {
                &self.upscale
            } else {
                &self.pipeline
            });
            pass.set_bind_group(0, r.view_group, &[]);
            pass.set_bind_group(1, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        // The history back onto the image, sharpened on the way (never fed back).
        let sharpen_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty taa sharpen group"),
            layout: &self.sharpen_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&h.views[write]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty taa sharpen"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: r.output.0,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.sharpen);
        pass.set_bind_group(0, r.view_group, &[]);
        pass.set_bind_group(1, &sharpen_group, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        h.current = read;
        h.valid = true;
        h.frame = h.frame.wrapping_add(1);
    }

    /// Stretch `input` (internal resolution) over `output` with a bilinear filter: an
    /// upscaled target's frame without a temporal resolve.
    pub(super) fn stretch(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view_group: &wgpu::BindGroup,
        input: &wgpu::TextureView,
        output: &wgpu::TextureView,
    ) {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("misty taa stretch params"),
            contents: bytemuck::bytes_of(&TaaParams::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty taa stretch group"),
            layout: &self.sharpen_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty taa stretch"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.stretch);
        pass.set_bind_group(0, view_group, &[]);
        pass.set_bind_group(1, &group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// GPU bytes of the histories (Memory module).
    pub(super) fn bytes(&self) -> u64 {
        self.histories
            .values()
            .map(|h| 2 * h.size.0 as u64 * h.size.1 as u64 * 8)
            .sum()
    }
}

impl History {
    fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        let make = || {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("misty taa history"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        let images = [make(), make()];
        let views = [
            images[0].create_view(&Default::default()),
            images[1].create_view(&Default::default()),
        ];
        Self {
            size,
            views,
            current: 0,
            valid: false,
            frame: 0,
            used: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn jitter_covers_the_pixel_evenly() {
        let all: Vec<Vec2> = (0..PHASES).map(|n| jitter_pixels(n, PHASES)).collect();
        let mean = all.iter().copied().sum::<Vec2>() / PHASES as f32;
        assert!(mean.length() < 0.08, "{mean}");
        for j in &all {
            assert!(j.x.abs() <= 0.5 && j.y.abs() <= 0.5, "{j}");
        }
        // Distinct positions.
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(a.distance(*b) > 0.05);
            }
        }
    }

    #[test]
    fn sharpening_rises_with_the_upscale_ratio() {
        assert_eq!(sharpen_gain(1.0), 1.0);
        assert!((sharpen_gain(0.67) - 1.6).abs() < 1e-3);
        assert_eq!(sharpen_gain(0.5), 1.6);
        assert!(sharpen_gain(0.85) > 1.2 && sharpen_gain(0.85) < 1.4);
    }

    #[test]
    fn upscaling_cycles_through_more_jitter_positions() {
        assert_eq!(jitter_phases(1.0), 8);
        assert_eq!(jitter_phases(0.67), 18);
        assert_eq!(jitter_phases(0.58), 24);
        assert_eq!(jitter_phases(0.5), 32);
        assert_eq!(jitter_phases(0.25), 64);
        // At half resolution each internal pixel covers 2×2 output pixels: every one of them
        // gets several samples within half an output pixel of its centre per cycle.
        let phases = jitter_phases(0.5);
        let all: Vec<Vec2> = (0..phases).map(|n| jitter_pixels(n, phases)).collect();
        for oy in 0..2 {
            for ox in 0..2 {
                // Output pixel centre in internal-pixel units, relative to the pixel centre.
                let c = Vec2::new(ox as f32 - 0.5, oy as f32 - 0.5) * 0.5;
                // A sample jittered by j sees the scene at -j from the pixel centre.
                let near = all
                    .iter()
                    .filter(|j| (-**j - c).length() * 2.0 < 0.5)
                    .count();
                assert!(near >= 3, "output pixel {ox},{oy}: {near} near samples");
            }
        }
    }

    #[test]
    fn jitter_moves_projected_points_by_that_many_pixels() {
        // A plain right-handed perspective (f = 2, aspect 1.5, near 0.1).
        let vp = Mat4::from_cols_array(&[
            2.0 / 1.5,
            0.0,
            0.0,
            0.0, //
            0.0,
            2.0,
            0.0,
            0.0, //
            0.0,
            0.0,
            -1.0,
            -1.0, //
            0.0,
            0.0,
            -0.2,
            0.0,
        ]);
        let size = (300, 200);
        let p = Vec3::new(1.0, 0.5, -10.0);
        let px = |m: Mat4| {
            let c = m * p.extend(1.0);
            let ndc = c.truncate() / c.w;
            Vec2::new((ndc.x * 0.5 + 0.5) * 300.0, (0.5 - ndc.y * 0.5) * 200.0)
        };
        let moved = px(jittered(vp, Vec2::new(0.25, -0.5), size)) - px(vp);
        assert!((moved - Vec2::new(0.25, -0.5)).length() < 1e-3, "{moved}");
        // No jitter is the identity, bit for bit.
        assert_eq!(
            jittered(vp, Vec2::ZERO, size).to_cols_array(),
            vp.to_cols_array()
        );
    }

    #[test]
    fn shader_validates() {
        use wgpu::naga;
        let src = format!("{}\n{}", super::super::pipelines::COMMON, SRC);
        let module = naga::front::wgsl::parse_str(&src)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(std::mem::size_of::<TaaParams>(), 128 + 80);
    }
}
