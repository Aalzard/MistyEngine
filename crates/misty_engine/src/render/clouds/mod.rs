//! Volumetric clouds: a raymarched layer between two altitudes on a curved planet.
//!
//! Per render, compute passes trace the clouds at reduced resolution (blue-noise ray offsets,
//! sun light with multiple-scattering octaves and a silver-lining phase, sky ambient), resolve
//! them temporally against last frame's reprojected image, and trace a cloud shadow map
//! around the camera. The forward shaders fold the clouds into `vfog_eval` / `vfog_sky`
//! (clouds_apply.wgsl): the sky gets them in full, geometry hides the clouds behind it, the
//! sun on meshes, terrain, water and the fog (froxels and analytic) is dimmed by the shadow map,
//! and the screen-space sun shafts only see the sky between clouds.
//!
//! Hooks: [`Renderer::prepare_clouds`] (uniforms, textures, view group) and
//! [`Renderer::cloud_passes`] (after the shadow maps, before the froxel fog).

pub(super) mod noise;
mod passes;
pub(crate) mod settings;
mod uniforms;

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use super::{Frame, RenderTarget, Renderer};
use settings::{CloudQuality, HISTORY_WEIGHT};

/// Renders after which an unused view's images are dropped.
const STALE_AFTER: u64 = 600;

/// View-group entries 8..=10, after the HDRI sky's 4–7 (cloud image, cloud shadow map, apply uniform).
pub(crate) fn view_layout_entries() -> [wgpu::BindGroupLayoutEntry; 3] {
    let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty,
        count: None,
    };
    let texture = |dim| wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable: true },
        view_dimension: dim,
        multisampled: false,
    };
    [
        entry(8, texture(wgpu::TextureViewDimension::D2Array)),
        entry(9, texture(wgpu::TextureViewDimension::D2)),
        entry(
            10,
            wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
        ),
    ]
}

/// What one render draws.
#[derive(Clone, Copy, PartialEq)]
struct Now {
    target: u64,
    trace: (u32, u32),
    /// Shadow map side when clouds cast shadows.
    shadow: Option<u32>,
}

pub(crate) struct Clouds {
    apply_buffer: wgpu::Buffer,
    march_buffer: wgpu::Buffer,
    repeat: wgpu::Sampler,
    clamp: wgpu::Sampler,
    /// Bound while there are no clouds.
    dummy_image: wgpu::TextureView,
    dummy_shadow: wgpu::TextureView,
    noise: Option<noise::NoiseTextures>,
    pipelines: Option<passes::CloudPipelines>,
    shadow_map: Option<(u32, wgpu::TextureView)>,
    targets: HashMap<u64, passes::CloudTarget>,
    renders: u64,
    clock: std::time::Instant,
    now: Option<Now>,
    /// What the view group binds: (target, parity), or `None` for the dummies.
    bound: Option<(u64, usize)>,
}

impl Clouds {
    pub fn new(device: &wgpu::Device) -> Self {
        let uniform = |label, size: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let sampler = |label, mode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: mode,
                address_mode_v: mode,
                address_mode_w: mode,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            })
        };
        Self {
            apply_buffer: uniform(
                "misty cloud apply",
                std::mem::size_of::<uniforms::ApplyUniform>(),
            ),
            march_buffer: uniform(
                "misty clouds",
                std::mem::size_of::<uniforms::MarchUniform>(),
            ),
            repeat: sampler("misty cloud noise sampler", wgpu::AddressMode::Repeat),
            clamp: sampler(
                "misty cloud history sampler",
                wgpu::AddressMode::ClampToEdge,
            ),
            dummy_image: passes::image(device, "misty clouds dummy", (1, 1)),
            dummy_shadow: passes::shadow_map(device, 1),
            noise: None,
            pipelines: None,
            shadow_map: None,
            targets: HashMap::new(),
            renders: 0,
            clock: std::time::Instant::now(),
            now: None,
            bound: None,
        }
    }

    /// The cloud image the forward shaders sample this render.
    pub fn image_view(&self) -> &wgpu::TextureView {
        self.now
            .and_then(|n| self.targets.get(&n.target))
            .map_or(&self.dummy_image, |t| t.pair().1)
    }

    /// The cloud shadow map (a dummy when clouds cast no shadows).
    pub fn shadow_view(&self) -> &wgpu::TextureView {
        match (self.now.and_then(|n| n.shadow), &self.shadow_map) {
            (Some(_), Some((_, view))) => view,
            _ => &self.dummy_shadow,
        }
    }

    /// `CloudApply` uniform (forward shaders and froxel fog).
    pub fn apply_buffer(&self) -> &wgpu::Buffer {
        &self.apply_buffer
    }

    /// GPU memory of the noise, shadow map and per-view images.
    pub fn texture_bytes(&self) -> u64 {
        let noise = if self.noise.is_some() {
            noise::NoiseTextures::bytes()
        } else {
            0
        };
        let shadow = self
            .shadow_map
            .as_ref()
            .map_or(0, |(s, _)| 4 * (*s as u64).pow(2));
        noise + shadow + self.targets.values().map(|t| t.bytes()).sum::<u64>()
    }
}

impl Renderer {
    /// Write the cloud uniforms, make sure this view's images exist and point the view group
    /// at them (after `prepare_volumetrics`).
    pub(super) fn prepare_clouds(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        frame: &Frame,
    ) {
        crate::profile_scope!("Render.Clouds");
        let c = &mut self.clouds;
        c.renders += 1;
        let o = frame.post.clouds;
        let ortho = matches!(
            frame.camera.projection,
            crate::camera::Projection::Orthographic { .. }
        );
        let quality = CloudQuality::of(self.quality.volumetric_clouds)
            .filter(|_| o.is_on() && frame.skybox && frame.shaded && !ortho);
        c.now = None;
        if let Some(q) = quality {
            if c.noise.is_none() {
                c.noise = Some(noise::NoiseTextures::new(device, queue));
            }
            if c.pipelines.is_none() {
                c.pipelines = Some(passes::CloudPipelines::new(device));
            }
            let shadow = (o.shadows && frame.environment.sun.is_some()).then_some(q.shadow_size);
            if let Some(size) = shadow
                && c.shadow_map.as_ref().map(|(s, _)| *s) != Some(size)
            {
                c.shadow_map = Some((size, passes::shadow_map(device, size)));
            }
            let trace = q.trace_size(target.size());
            let state = c
                .targets
                .entry(target.id)
                .or_insert_with(|| passes::CloudTarget::new(device, trace));
            if state.size != trace {
                *state = passes::CloudTarget::new(device, trace);
            }
            state.last_used = c.renders;
            let view_proj = frame.camera.view_projection(target.aspect());
            // Rays from the rotation alone: the eye translation in an inverted
            // view-projection costs the far plane its precision (see `view_ray`).
            let at_origin = crate::camera::Camera {
                position: glam::Vec3::ZERO,
                ..*frame.camera
            };
            let rays = at_origin.view_projection(target.aspect()).inverse();
            let history = if state.prev_view_proj.is_some() {
                HISTORY_WEIGHT
            } else {
                0.0
            };
            let m = uniforms::march_uniform(&uniforms::MarchInputs {
                frame,
                o: &o,
                q: &q,
                rays,
                prev_view_proj: state.prev_view_proj.unwrap_or(view_proj),
                trace,
                frame_index: state.frame,
                history,
                time: c.clock.elapsed().as_secs_f32(),
            });
            queue.write_buffer(&c.march_buffer, 0, bytemuck::bytes_of(&m));
            state.prev_view_proj = Some(view_proj);
            let apply = uniforms::apply_uniform(frame, &o, &q);
            queue.write_buffer(&c.apply_buffer, 0, bytemuck::bytes_of(&apply));
            c.now = Some(Now {
                target: target.id,
                trace,
                shadow,
            });
        } else {
            let off = uniforms::ApplyUniform::default();
            queue.write_buffer(&c.apply_buffer, 0, bytemuck::bytes_of(&off));
            // Stale history would ghost in when the clouds come back.
            c.targets.remove(&target.id);
        }
        let now = c.renders;
        c.targets.retain(|_, t| now - t.last_used < STALE_AFTER);
        let bound = c
            .now
            .and_then(|n| c.targets.get(&n.target).map(|t| (n.target, t.parity())));
        if bound != c.bound || self.view_group_dirty {
            c.bound = bound;
            self.rebuild_view_group(device);
        }
    }

    /// Cloud shadow map, trace and temporal resolve (after the shadow maps, before the fog).
    pub(super) fn cloud_passes(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let c = &mut self.clouds;
        let (Some(now), Some(pipelines), Some(noise)) = (c.now, &c.pipelines, &c.noise) else {
            return;
        };
        let Some(state) = c.targets.get_mut(&now.target) else {
            return;
        };
        if !super::frame_debug::whole(
            &self.capture,
            &["Clouds"],
            Default::default,
            super::frame_debug::EventTarget::None,
            || super::frame_debug::FrameEvent {
                shader: "cloud shadow map, raymarch and temporal resolve kernels".to_owned(),
                ..super::frame_debug::FrameEvent::new(
                    super::frame_debug::EventKind::Compute,
                    "Dispatch Volumetric Clouds",
                )
            },
        ) {
            return;
        }
        let (history, resolved) = state.pair();
        let shadow = match (now.shadow, &c.shadow_map) {
            (Some(size), Some((_, view))) => Some((view, size)),
            _ => None,
        };
        pipelines.encode(
            device,
            encoder,
            &passes::Dispatch {
                uniform: &c.march_buffer,
                noise,
                repeat: &c.repeat,
                clamp: &c.clamp,
                raw: state.raw(),
                history,
                resolved,
                trace: now.trace,
                shadow,
            },
        );
        state.advance();
    }
}
