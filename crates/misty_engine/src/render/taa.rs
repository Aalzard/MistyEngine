//! Temporal anti-aliasing (Quality ▸ Temporal Anti-aliasing): the camera's projection is
//! offset by a sub-pixel Halton (2, 3) jitter each frame, and `shaders/taa.wgsl` blends the
//! resolved HDR image with a history reprojected through the depth buffer, clipped to the
//! current neighbourhood. It runs before bloom and tonemapping, so everything after sees the
//! converged image. Each render target keeps its own history (Scene and Game views).

use std::collections::HashMap;

use glam::{Mat4, Vec2, Vec3};
use wgpu::util::DeviceExt;

use super::HDR_FORMAT;
use super::pipelines::{Spec, build};

const SRC: &str = include_str!("shaders/taa.wgsl");
/// Jitter positions before the pattern repeats.
const PHASES: u32 = 8;
/// Share of the current frame in the blend (lower = smoother, slower to react).
const CURRENT_WEIGHT: f32 = 0.12;
/// Histories of targets not rendered for this many renders are dropped.
const STALE_AFTER: u64 = 600;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TaaParams {
    inv_view_proj: [[f32; 4]; 4],
    prev_view_proj: [[f32; 4]; 4],
    info: [f32; 4],
}

struct History {
    size: (u32, u32),
    /// Ping-pong history images; `current` is the one written this frame.
    images: [wgpu::Texture; 2],
    views: [wgpu::TextureView; 2],
    current: usize,
    /// Last frame's unjittered view-projection and camera (None = no usable history).
    prev: Option<(Mat4, Vec3, Vec3)>,
    frame: u32,
    used: u64,
}

pub(super) struct Taa {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    histories: HashMap<u64, History>,
    renders: u64,
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

/// Sub-pixel offset of frame `n` in pixels (-0.5..0.5).
pub(super) fn jitter_pixels(n: u32) -> Vec2 {
    let i = n % PHASES + 1;
    Vec2::new(halton(i, 2), halton(i, 3)) - 0.5
}

/// `view_proj` moved by `pixels` on a `size` image (a clip-space translation, so it works
/// for perspective and orthographic cameras alike).
pub(super) fn jittered(view_proj: Mat4, pixels: Vec2, size: (u32, u32)) -> Mat4 {
    let ndc = Vec2::new(
        pixels.x * 2.0 / size.0.max(1) as f32,
        -pixels.y * 2.0 / size.1.max(1) as f32,
    );
    Mat4::from_translation(ndc.extend(0.0)) * view_proj
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty taa sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            layout,
            pipeline,
            sampler,
            histories: HashMap::new(),
            renders: 0,
        }
    }

    /// This frame's jitter in pixels for `target` (zero before its first history).
    pub(super) fn jitter(&self, target: u64) -> Vec2 {
        self.histories
            .get(&target)
            .map_or(Vec2::ZERO, |h| jitter_pixels(h.frame))
    }

    /// Drop every history (e.g. TAA was switched off).
    pub(super) fn clear(&mut self) {
        self.histories.clear();
    }

    /// Resolve `current` (the HDR image) with the history into the next history image, then
    /// copy it back over `current`. `view_proj` = this frame's unjittered camera.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view_group: &wgpu::BindGroup,
        target: u64,
        current: (&wgpu::Texture, &wgpu::TextureView),
        depth: &wgpu::TextureView,
        view_proj: Mat4,
        camera: (Vec3, Vec3),
    ) {
        self.renders += 1;
        let renders = self.renders;
        self.histories.retain(|_, h| renders - h.used < STALE_AFTER);
        let size = (current.0.width(), current.0.height());
        let h = self
            .histories
            .entry(target)
            .or_insert_with(|| History::new(device, size));
        if h.size != size {
            *h = History::new(device, size);
        }
        h.used = renders;
        // A jump the history can't follow (teleport, switched camera): start afresh.
        let valid = h
            .prev
            .filter(|(_, pos, fwd)| pos.distance(camera.0) < 5.0 && fwd.dot(camera.1) > 0.85);
        let params = TaaParams {
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            prev_view_proj: valid.map_or(view_proj, |p| p.0).to_cols_array_2d(),
            info: [
                if valid.is_some() { 1.0 } else { 0.0 },
                CURRENT_WEIGHT,
                1.0 / size.0 as f32,
                1.0 / size.1 as f32,
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
                    resource: wgpu::BindingResource::TextureView(current.1),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(depth),
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
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, view_group, &[]);
            pass.set_bind_group(1, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_texture(
            h.images[write].as_image_copy(),
            current.0.as_image_copy(),
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
        h.current = read;
        h.prev = Some((view_proj, camera.0, camera.1));
        h.frame = h.frame.wrapping_add(1);
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
            images,
            views,
            current: 0,
            prev: None,
            frame: 0,
            used: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_covers_the_pixel_evenly() {
        let all: Vec<Vec2> = (0..PHASES).map(jitter_pixels).collect();
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
    }
}
