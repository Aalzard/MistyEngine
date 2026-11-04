//! Volumetric fog and light shafts.
//!
//! Froxel fog: a camera-aligned grid (exponential depth slices) is filled by a compute pass
//! with density (global height fog, Fog Volumes, noise) lit by the scene lights through their
//! shadow maps, blended with last frame's reprojected result (per-frame jitter), then
//! integrated front to back. Every forward shader (meshes, terrain, sky, particles) samples
//! the result through `fog_apply.wgsl` at group 0. Without Volumetric, the same shaders use an
//! analytic height fog. Sharp sun shafts (on by default) take the main sun out of the froxels
//! and raymarch it per pixel through its shadow map, added after the main pass. The screen-space
//! Sun Shafts are a separate, cheap radial blur.
//!
//! The renderer calls three hooks per render: [`Renderer::prepare_volumetrics`] (uniforms and
//! textures), [`Renderer::froxel_passes`] (after the shadow maps) and
//! [`Renderer::sun_shaft_passes`] (after the main pass: sharp shafts, then screen-space ones).

mod froxels;
mod settings;
mod shadow_access;
mod sharp_shafts;
mod sun_shafts;
mod uniforms;

#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod sharp_gpu_tests;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

pub use settings::{FogDebug, FogSettings, LocalFog, SunShaftSettings, VolumetricLight};

use super::{Frame, RenderTarget, Renderer};
pub(crate) use settings::FogMode;
use uniforms::Slices;

/// View-group entries 1..=3 (fog volume, its sampler, the fog uniform).
pub(crate) fn view_layout_entries() -> [wgpu::BindGroupLayoutEntry; 3] {
    let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty,
        count: None,
    };
    [
        entry(
            1,
            wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D3,
                multisampled: false,
            },
        ),
        entry(
            2,
            wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        ),
        entry(
            3,
            wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
        ),
    ]
}

/// The view bind group: the view uniform, the fog resources (1–3), the HDRI sky (4–7) and the
/// volumetric clouds (8–10).
pub(crate) fn view_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view_buffer: &wgpu::Buffer,
    v: &Volumetrics,
    sky: &super::hdri_sky::SkyGpu,
    clouds: &super::clouds::Clouds,
) -> wgpu::BindGroup {
    let volume = v.volume.as_ref().map_or(&v.dummy, |(_, view)| view);
    let [s0, s1, s2, s3] = sky.entries();
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("misty view group"),
        layout,
        entries: &[
            s0,
            s1,
            s2,
            s3,
            wgpu::BindGroupEntry {
                binding: 0,
                resource: view_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(volume),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&v.sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: v.apply_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::TextureView(clouds.image_view()),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::TextureView(clouds.shadow_view()),
            },
            wgpu::BindGroupEntry {
                binding: 10,
                resource: clouds.apply_buffer().as_entire_binding(),
            },
        ],
    })
}

/// Per-view state (keyed by [`RenderTarget::id`]).
#[derive(Default)]
struct TargetState {
    history: Option<froxels::History>,
    shafts: Option<sun_shafts::ShaftTextures>,
    sharp: Option<sharp_shafts::SharpTextures>,
    last_used: u64,
}

/// Renders after which an unused view's state is dropped (its target was resized or closed).
const STALE_AFTER: u64 = 600;
/// Weight of the reprojected history when denoising.
const HISTORY_WEIGHT: f32 = 0.9;

pub(crate) struct Volumetrics {
    apply_buffer: wgpu::Buffer,
    froxel_buffer: wgpu::Buffer,
    /// Linear clamp (fog volume and history lookups, shaft blurs).
    sampler: wgpu::Sampler,
    /// Bound while there is no froxel volume.
    dummy: wgpu::TextureView,
    /// Integrated froxels shared by every view (each render fills it before its main pass).
    volume: Option<([u32; 3], wgpu::TextureView)>,
    froxels: Option<froxels::FroxelPipelines>,
    shafts: Option<sun_shafts::ShaftPipelines>,
    sharp: Option<sharp_shafts::SharpPipelines>,
    targets: HashMap<u64, TargetState>,
    renders: u64,
    clock: std::time::Instant,
    /// What the current render does.
    froxels_now: Option<[u32; 3]>,
    shafts_now: bool,
    /// The sharp sun shafts run this render.
    sharp_now: bool,
}

impl Volumetrics {
    pub fn new(device: &wgpu::Device) -> Self {
        let uniform = |label, size: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Self {
            apply_buffer: uniform(
                "misty fog apply",
                std::mem::size_of::<uniforms::ApplyUniform>(),
            ),
            froxel_buffer: uniform(
                "misty froxels",
                std::mem::size_of::<uniforms::FroxelUniform>(),
            ),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("misty fog sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            dummy: froxels::volume_texture(device, "misty fog dummy", [1, 1, 1]),
            volume: None,
            froxels: None,
            shafts: None,
            sharp: None,
            targets: HashMap::new(),
            renders: 0,
            clock: std::time::Instant::now(),
            froxels_now: None,
            shafts_now: false,
            sharp_now: false,
        }
    }

    /// The main pass must keep its depth (both kinds of sun shafts read it).
    pub fn keeps_depth(&self) -> bool {
        self.shafts_now || self.sharp_now
    }

    /// Make this view's sharp-shaft images and write their uniform (`None` = off this render:
    /// the images are dropped so no stale history comes back).
    #[allow(clippy::too_many_arguments)]
    fn prepare_sharp(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view_layout: &wgpu::BindGroupLayout,
        target: &RenderTarget,
        frame: &Frame,
        quality: Option<settings::ShaftQuality>,
        reach: f32,
    ) {
        let Some(q) = quality else {
            if let Some(state) = self.targets.get_mut(&target.id) {
                state.sharp = None;
            }
            return;
        };
        let pipelines = self
            .sharp
            .get_or_insert_with(|| sharp_shafts::SharpPipelines::new(device, queue, view_layout));
        let full = target.size();
        let state = self.targets.entry(target.id).or_default();
        let size = sharp_shafts::march_size(full, q.scale);
        if state
            .sharp
            .as_ref()
            .is_none_or(|t| t.full != full || t.size != size)
        {
            state.sharp = Some(sharp_shafts::SharpTextures::new(device, full, q.scale));
        }
        let tex = state.sharp.as_mut().expect("sharp textures");
        tex.begin_frame();
        let u = sharp_shafts::SharpUniform::new(frame, tex, q, reach);
        queue.write_buffer(&pipelines.uniform, 0, bytemuck::bytes_of(&u));
        self.sharp_now = true;
    }

    /// GPU memory of the fog volumes and shaft images.
    pub fn texture_bytes(&self) -> u64 {
        let volume = self
            .volume
            .as_ref()
            .map_or(0, |(s, _)| froxels::volume_bytes(*s));
        let views: u64 = self
            .targets
            .values()
            .map(|t| {
                t.history
                    .as_ref()
                    .map_or(0, |h| 2 * froxels::volume_bytes(h.size))
                    + t.shafts.as_ref().map_or(0, |s| s.bytes())
                    + t.sharp.as_ref().map_or(0, |s| s.bytes())
            })
            .sum();
        volume + views
    }
}

impl Renderer {
    /// Write the fog uniforms and make sure this view's textures exist. Rebuilds the view bind
    /// group when the shared volume changes size.
    pub(super) fn prepare_volumetrics(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        frame: &Frame,
        plan: &super::shadows::ShadowPlan,
    ) {
        crate::profile_scope!("Render.Volumetrics");
        let aspect = target.aspect();
        let sharp_quality = (frame.post.fog.volume.sun_shafts_quality
            == crate::scene::SunShaftsQuality::Sharp)
            .then(|| settings::shaft_quality(self.quality.sun_shafts))
            .flatten();
        let v = &mut self.volumetrics;
        v.renders += 1;
        let fog = frame.post.fog;
        let ortho = matches!(
            frame.camera.projection,
            crate::camera::Projection::Orthographic { .. }
        );
        let mut mode = fog.mode();
        if ortho && mode == FogMode::Volumetric {
            // Froxels follow a perspective eye; orthographic views get the analytic fog.
            mode = FogMode::Analytic;
        }
        if super::water::camera_underwater(frame) {
            // The underwater pass replaces the air fog (it would apply twice).
            mode = FogMode::Off;
        }
        let size = settings::grid_size(fog.volume.quality);
        let reach = fog.volumetric_distance().min(frame.camera.far);
        let slices = Slices::new(reach, size[2]);
        let apply = uniforms::apply_uniform(frame, &fog, mode, &slices);
        queue.write_buffer(&v.apply_buffer, 0, bytemuck::bytes_of(&apply));

        v.froxels_now = None;
        v.sharp_now = false;
        let mut rebuild = false;
        if mode == FogMode::Volumetric {
            if v.volume.as_ref().map(|(s, _)| *s) != Some(size) {
                let view = froxels::volume_texture(device, "misty fog volume", size);
                v.volume = Some((size, view));
                rebuild = true;
            }
            if v.froxels.is_none() {
                v.froxels = Some(froxels::FroxelPipelines::new(device));
            }
            let state = v.targets.entry(target.id).or_default();
            if state.history.as_ref().map(|h| h.size) != Some(size) {
                state.history = Some(froxels::History::new(device, size));
            }
            let history = state.history.as_mut().expect("history");
            let view_proj = frame.camera.view_projection(aspect);
            let reuse = fog.volume.denoising
                && history
                    .prev
                    .is_some_and(|p| p.slices == slices && p.size == size);
            let mut u = uniforms::froxel_uniform(&uniforms::FroxelInputs {
                frame,
                fog: &fog,
                plan,
                view_proj,
                slices,
                size,
                frame_index: history.frame,
                prev: history.prev,
                history: if reuse { HISTORY_WEIGHT } else { 0.0 },
                time: v.clock.elapsed().as_secs_f32(),
                sharp: sharp_quality.is_some(),
            });
            // No sun among the froxel lights: nothing to march.
            let sharp = sharp_quality.filter(|_| u.sun[0] >= 0.0);
            if sharp.is_none() {
                u.sun[1] = 0.0;
            }
            queue.write_buffer(&v.froxel_buffer, 0, bytemuck::bytes_of(&u));
            history.prev = Some(uniforms::PrevView {
                view_proj,
                eye: frame.camera.position,
                forward: frame.camera.forward(),
                slices,
                size,
            });
            v.froxels_now = Some(size);
            let reach = slices.far;
            v.prepare_sharp(
                device,
                queue,
                &self.pipelines.view_layout,
                target,
                frame,
                sharp,
                reach,
            );
        } else if let Some(state) = v.targets.get_mut(&target.id) {
            // Stale history would ghost in when the fog comes back.
            state.history = None;
            state.sharp = None;
        }

        v.shafts_now = false;
        if let Some(u) = sun_shafts::uniform(frame, aspect) {
            if v.shafts.is_none() {
                v.shafts = Some(sun_shafts::ShaftPipelines::new(
                    device,
                    &self.pipelines.view_layout,
                ));
            }
            let pipelines = v.shafts.as_ref().expect("shaft pipelines");
            queue.write_buffer(&pipelines.uniform, 0, bytemuck::bytes_of(&u));
            let state = v.targets.entry(target.id).or_default();
            let half = (target.size().0 / 2, target.size().1 / 2);
            if state.shafts.as_ref().map(|s| s.size) != Some((half.0.max(1), half.1.max(1))) {
                state.shafts = Some(sun_shafts::ShaftTextures::new(device, target.size()));
            }
            v.shafts_now = true;
        }

        if let Some(state) = v.targets.get_mut(&target.id) {
            state.last_used = v.renders;
        }
        let now = v.renders;
        v.targets.retain(|_, s| now - s.last_used < STALE_AFTER);
        if rebuild {
            // Rebuilt by `prepare_clouds`, which runs next.
            self.view_group_dirty = true;
        }
    }

    /// Light and integrate the froxels (after the shadow maps, before the main pass).
    pub(super) fn froxel_passes(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
    ) {
        let v = &mut self.volumetrics;
        let (Some(size), Some(pipelines), Some((_, integrated))) =
            (v.froxels_now, v.froxels.as_ref(), v.volume.as_ref())
        else {
            return;
        };
        let Some(history) = v
            .targets
            .get_mut(&target.id)
            .and_then(|t| t.history.as_mut())
        else {
            return;
        };
        let (read, write) = history.pair();
        pipelines.encode(
            device,
            encoder,
            &froxels::Dispatch {
                uniform: &v.froxel_buffer,
                sampler: &v.sampler,
                history: read,
                lit: write,
                integrated,
                shadows: shadow_access::views(&self.shadows),
                clouds: (self.clouds.shadow_view(), self.clouds.apply_buffer()),
                size,
            },
        );
        history.advance();
    }

    /// Sharp, then screen-space sun shafts onto the HDR image (after the main pass, before
    /// bloom).
    pub(super) fn sun_shaft_passes(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
    ) {
        self.sharp_shaft_passes(device, encoder, target);
        let v = &self.volumetrics;
        if !v.shafts_now {
            return;
        }
        let (Some(pipelines), Some(textures)) = (
            v.shafts.as_ref(),
            v.targets.get(&target.id).and_then(|t| t.shafts.as_ref()),
        ) else {
            return;
        };
        let steps = pipelines.steps(
            device,
            textures,
            target.hdr_view(),
            target.depth_view(),
            &v.sampler,
        );
        for s in &steps {
            self.fullscreen(encoder, s.output, s.pipeline, &s.group, s.clear, None);
        }
    }

    /// Sharp sun shafts: march + accumulate at reduced resolution, upsample-add at full.
    fn sharp_shaft_passes(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
    ) {
        let v = &self.volumetrics;
        if !v.sharp_now {
            return;
        }
        let Some(state) = v.targets.get(&target.id) else {
            return;
        };
        let (Some(pipelines), Some(froxels), Some(tex), Some(history)) = (
            v.sharp.as_ref(),
            v.froxels.as_ref(),
            state.sharp.as_ref(),
            state.history.as_ref(),
        ) else {
            return;
        };
        let inputs = sharp_shafts::Inputs {
            froxel_uniform: &v.froxel_buffer,
            linear: &v.sampler,
            shadow_cmp: &froxels.shadow_sampler,
            sun_shadows: shadow_access::views(&self.shadows).0,
            clouds: (self.clouds.shadow_view(), self.clouds.apply_buffer()),
            depth: target.depth_view(),
            froxel_lit: history.latest(),
        };
        let (pipeline, group) = pipelines.encode(device, encoder, tex, &inputs);
        self.fullscreen(encoder, target.hdr_view(), pipeline, &group, false, None);
    }
}
