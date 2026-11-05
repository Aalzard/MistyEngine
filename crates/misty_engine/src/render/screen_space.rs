//! Per-view resources and passes of the screen-space lighting effects (ambient occlusion and
//! contact shadows): a depth + normal prepass of the opaque geometry, the GTAO compute
//! passes (shaders/ao.wgsl) and the texel buffer the lit shaders read
//! (shaders/screen_space.wgsl). While both effects are off nothing here runs and the lit
//! shaders read a zero header, so the image is exactly what it was without them.

use std::sync::Mutex;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use super::frame_lists::FrameLists;
use super::pipelines::{self, Depth, Spec};
use super::surface::ShadowKey;
use super::{DEPTH_FORMAT, Frame, RenderStats, RenderTarget, Renderer, draw_groups};
use crate::camera::{Camera, Projection};

mod resources;

/// The prepass after the shared per-draw struct (draw.wgsl).
pub(super) const PREPASS_SRC: &str = concat!(
    include_str!("shaders/draw.wgsl"),
    "\n",
    include_str!("shaders/ao_prepass.wgsl")
);
/// The prepass with terrain vertex pulling (terrain_vertex.wgsl).
pub(super) const TERRAIN_PREPASS_SRC: &str = concat!(
    include_str!("shaders/draw.wgsl"),
    "\n",
    include_str!("shaders/ao_prepass.wgsl"),
    "\n",
    include_str!("shaders/terrain_vertex.wgsl"),
    "\n",
    include_str!("shaders/ao_prepass_terrain.wgsl")
);
const AO_SRC: &str = include_str!("shaders/ao.wgsl");
const NORMAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// AO images: x = occlusion, y = linear depth.
const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Float;
/// Header (four vec4s) before the per-pixel texels.
const HEADER_BYTES: u64 = 64;
const TEXEL_BYTES: u64 = 8;

/// Buffer size for `texels` texels (WGSL rounds the struct up to its 16-byte alignment).
fn buffer_bytes(texels: u64) -> u64 {
    (HEADER_BYTES + texels * TEXEL_BYTES).next_multiple_of(16)
}
/// Share of last frame's AO in the temporal blend (about ten frames of history).
const HISTORY_WEIGHT: f32 = 0.9;

/// What the lit shaders read (`ScreenSpace` in screen_space.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Header {
    ao: [f32; 4],
    contact: [f32; 4],
    misc: [f32; 4],
    forward: [f32; 4],
}

/// Uniform of the AO compute passes (`AoParams` in ao.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct AoParams {
    inv_view_proj: [[f32; 4]; 4],
    prev_view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    forward: [f32; 4],
    params: [f32; 4],
    size: [f32; 4],
    temporal: [f32; 4],
    proj: [f32; 4],
}

/// Prepass and compute pipelines, built the first time a view turns an effect on.
struct Passes {
    /// Prepass variants by (cutout, double sided).
    prepass: Vec<(ShadowKey, wgpu::RenderPipeline)>,
    /// Terrains (heightmap vertex pulling).
    terrain: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    gtao: wgpu::ComputePipeline,
    denoise: wgpu::ComputePipeline,
    resolve: wgpu::ComputePipeline,
}

/// The renderer's part: pipelines, the "off" buffer and this render's lit-pass group.
pub(super) struct ScreenSpace {
    passes: Option<Passes>,
    /// Zero header: bound while the effects are off.
    pub off_buffer: wgpu::Buffer,
    /// Group 1 of the lit passes for the current render when an effect is on.
    frame_group: Option<wgpu::BindGroup>,
}

impl ScreenSpace {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            passes: None,
            off_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty screen-space off"),
                size: buffer_bytes(1),
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
            frame_group: None,
        }
    }
}

/// Last frame of a view, for reprojection and camera-cut detection.
struct History {
    view_proj: Mat4,
    position: Vec3,
    forward: Vec3,
    /// AO was traced (the history buffer holds AO, not just depth).
    ao: bool,
}

/// A view's textures and buffers (created on first use, rebuilt when the AO resolution changes).
struct TargetState {
    /// Full-resolution pixels per AO texel.
    scale: u32,
    full: (u32, u32),
    ao_size: (u32, u32),
    normal_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    params: wgpu::Buffer,
    /// Ping-pong texel buffers: one is written, the other is last frame's history.
    buffers: [wgpu::Buffer; 2],
    /// GTAO, denoise, then resolve into buffer 0 / buffer 1.
    groups: [wgpu::BindGroup; 4],
    frame: u32,
    history: Option<History>,
}

/// Per-target slot (inside [`RenderTarget`]).
#[derive(Default)]
pub(crate) struct TargetSlot(Mutex<Option<TargetState>>);

/// What this render's screen-space passes need.
pub(super) struct ScreenFrame {
    normal_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    /// GTAO and denoise groups (None = contact shadows only: the resolve writes depth).
    ao: Option<[wgpu::BindGroup; 2]>,
    resolve: wgpu::BindGroup,
    ao_size: (u32, u32),
    full: (u32, u32),
}

impl Renderer {
    /// Group 1 of the lit mesh and terrain passes.
    pub(super) fn lit_group(&self) -> &wgpu::BindGroup {
        self.screen_space
            .frame_group
            .as_ref()
            .unwrap_or(&self.mesh_bind_group)
    }

    /// AO or contact shadows will run this render (when the view fits one texel buffer).
    pub(super) fn screen_space_wanted(&self, frame: &Frame) -> bool {
        let ao =
            frame.post.ao.active() && self.quality.ambient_occlusion != crate::quality::Tier::Off;
        let contact =
            frame.post.contact_shadows.active() && frame.lights.iter().any(|l| l.contact_shadows);
        frame.shaded && (ao || contact)
    }

    /// Set up this render's screen-space effects; `None` when they are off (or the view is
    /// too large for one texel buffer). Call after the draw buffers are uploaded.
    pub(super) fn prepare_screen_space(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        frame: &Frame,
    ) -> Option<ScreenFrame> {
        self.screen_space.frame_group = None;
        let ao = frame.post.ao;
        let contact = frame.post.contact_shadows;
        // Quality ▸ Ambient Occlusion Off turns AO off whatever the volume says.
        let ao_on = ao.active() && self.quality.ambient_occlusion != crate::quality::Tier::Off;
        let contact_on = contact.active() && frame.lights.iter().any(|l| l.contact_shadows);
        if !self.screen_space_wanted(frame) {
            return None;
        }
        let full = target.size();
        let bytes = buffer_bytes(full.0 as u64 * full.1 as u64);
        if bytes > device.limits().max_storage_buffer_binding_size {
            return None;
        }
        if self.screen_space.passes.is_none() {
            self.screen_space.passes = Some(Passes::new(device, &self.pipelines));
        }
        let passes = self.screen_space.passes.as_ref()?;
        let scale = if ao.full_resolution { 1 } else { 2 };
        let mut slot = target.screen_space.0.lock().ok()?;
        if slot.as_ref().is_none_or(|s| s.scale != scale) {
            *slot = Some(TargetState::new(device, passes, full, scale, bytes));
        }
        let state = slot.as_mut()?;

        let cam = frame.camera;
        let aspect = target.aspect();
        let view_proj = cam.view_projection(aspect);
        let history = state
            .history
            .as_ref()
            .filter(|h| ao.temporal && h.ao && !camera_cut(h, cam));
        let weight = if history.is_some() {
            HISTORY_WEIGHT
        } else {
            0.0
        };
        let prev = history.map_or(view_proj, |h| h.view_proj);
        let params = ao_params(frame, aspect, state, view_proj, prev, weight, ao_on);
        queue.write_buffer(&state.params, 0, bytemuck::bytes_of(&params));

        let current = (state.frame % 2) as usize;
        let header = Header {
            ao: [
                if ao_on { 1.0 } else { 0.0 },
                ao.direct_lighting_strength,
                if ao_on && ao.debug { 1.0 } else { 0.0 },
                0.0,
            ],
            contact: [
                if contact_on { 1.0 } else { 0.0 },
                contact.length,
                contact.opacity,
                contact.sample_count as f32,
            ],
            misc: [
                contact.distance_fade,
                contact.thickness(),
                full.0 as f32,
                full.1 as f32,
            ],
            forward: cam.forward().extend(0.0).to_array(),
        };
        let buffer = &state.buffers[current];
        queue.write_buffer(buffer, 0, bytemuck::bytes_of(&header));
        let (lit, _) = draw_groups(
            device,
            &self.pipelines,
            [&self.lights_buffer, &self.ltc_table, &self.lights_2d],
            [&self.draws_buffer, &self.slots_buffer],
            &self.shadows,
            &self.probes,
            buffer,
        );
        self.screen_space.frame_group = Some(lit);

        state.frame = state.frame.wrapping_add(1);
        state.history = Some(History {
            view_proj,
            position: cam.position,
            forward: cam.forward(),
            ao: ao_on,
        });
        let [gtao, denoise, resolve0, resolve1] = &state.groups;
        Some(ScreenFrame {
            normal_view: state.normal_view.clone(),
            depth_view: state.depth_view.clone(),
            ao: ao_on.then(|| [gtao.clone(), denoise.clone()]),
            resolve: if current == 0 { resolve0 } else { resolve1 }.clone(),
            ao_size: state.ao_size,
            full,
        })
    }

    /// Depth + normal prepass, then AO and the resolve into the texel buffer.
    pub(super) fn screen_space_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        lists: &FrameLists,
        ss: &ScreenFrame,
        stats: &mut RenderStats,
    ) {
        let Some(passes) = self.screen_space.passes.as_ref() else {
            return;
        };
        self.prepass(encoder, frame, lists, ss, passes, stats);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("misty ambient occlusion"),
            timestamp_writes: None,
        });
        let groups = |(w, h): (u32, u32)| (w.div_ceil(8), h.div_ceil(8));
        if let Some([gtao, denoise]) = &ss.ao {
            let (x, y) = groups(ss.ao_size);
            pass.set_pipeline(&passes.gtao);
            pass.set_bind_group(0, gtao, &[]);
            pass.dispatch_workgroups(x, y, 1);
            pass.set_pipeline(&passes.denoise);
            pass.set_bind_group(0, denoise, &[]);
            pass.dispatch_workgroups(x, y, 1);
        }
        let (x, y) = groups(ss.full);
        pass.set_pipeline(&passes.resolve);
        pass.set_bind_group(0, &ss.resolve, &[]);
        pass.dispatch_workgroups(x, y, 1);
    }

    /// Opaque and Cutout meshes (the camera's batches) and terrains, normals + depth.
    fn prepass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        lists: &FrameLists,
        ss: &ScreenFrame,
        passes: &Passes,
        stats: &mut RenderStats,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty depth normal prepass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &ss.normal_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &ss.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.view_bind_group, &[]);
        pass.set_bind_group(1, &self.draws_bind_group, &[]);
        for b in &lists.opaque {
            let d = &frame.meshes[b.draw];
            let key = prepass_key(&d.material);
            let (Some(pipeline), Some(m)) = (passes.prepass_variant(key), self.gpu_mesh(&d.mesh))
            else {
                continue;
            };
            if key.cutout {
                let Some(group) = self.textures.get_set(&d.material.textures()) else {
                    continue;
                };
                pass.set_bind_group(2, group, &[]);
            }
            pass.set_pipeline(pipeline);
            m.draw(&mut pass, b.first..b.first + b.count);
            stats.draw(b.count, m.count, m.vertex_count);
        }
        // Terrains pull heights from their own textures (group 3; group 2 unused).
        if !frame.terrains.is_empty()
            && let Some(material) = self.textures.get(None, None)
        {
            pass.set_pipeline(&passes.terrain);
            pass.set_bind_group(2, material, &[]);
            let first = frame.meshes.len();
            self.terrains.draw(
                &mut pass,
                frame.terrains,
                first,
                false,
                |i| lists.terrains[i],
                stats,
            );
        }
    }
}

/// Prepass variant of a material: alpha-tested for lit Cutout, culled like the main pass.
fn prepass_key(m: &super::MaterialParams) -> ShadowKey {
    ShadowKey {
        cutout: !m.unlit && m.mode == super::RenderingMode::Cutout,
        double_sided: m.double_sided,
    }
}

/// A jump the history can't follow (teleport, switched camera): start the AO afresh.
fn camera_cut(prev: &History, cam: &Camera) -> bool {
    prev.position.distance(cam.position) > 5.0 || prev.forward.dot(cam.forward()) < 0.85
}

fn ao_params(
    frame: &Frame,
    aspect: f32,
    state: &TargetState,
    view_proj: Mat4,
    prev: Mat4,
    weight: f32,
    ao_on: bool,
) -> AoParams {
    let cam = frame.camera;
    let ao = &frame.post.ao;
    let ortho = matches!(cam.projection, Projection::Orthographic { .. });
    let (half_x, half_y) = match cam.projection {
        Projection::Perspective { fov_y } => {
            let t = (fov_y.to_radians() * 0.5).tan();
            (t * aspect, t)
        }
        Projection::Orthographic { size } => (size * aspect, size),
    };
    let v4 = |v: Vec3, w: f32| v.extend(w).to_array();
    // Without history a wider blur stands in for the accumulation.
    let denoise = if weight > 0.0 { 2.0 } else { 3.0 };
    AoParams {
        inv_view_proj: view_proj.inverse().to_cols_array_2d(),
        prev_view_proj: prev.to_cols_array_2d(),
        camera: v4(cam.position, if ortho { 1.0 } else { 0.0 }),
        right: v4(cam.right(), 0.0),
        up: v4(cam.up(), 0.0),
        forward: v4(cam.forward(), 0.0),
        params: [
            ao.radius,
            ao.intensity,
            ao.step_count as f32,
            ao.slices() as f32,
        ],
        size: [
            state.ao_size.0 as f32,
            state.ao_size.1 as f32,
            state.full.0 as f32,
            state.full.1 as f32,
        ],
        temporal: [
            weight,
            if ao.temporal {
                (state.frame % 64) as f32
            } else {
                0.0
            },
            state.scale as f32,
            denoise,
        ],
        proj: [half_x, half_y, if ao_on { 1.0 } else { 0.0 }, 0.0],
    }
}

#[cfg(test)]
mod gpu_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;
    use wgpu::naga;

    #[test]
    fn ao_shader_validates() {
        let module = naga::front::wgsl::parse_str(AO_SRC)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(AO_SRC)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
        assert_eq!(std::mem::size_of::<AoParams>(), 128 + 16 * 8);
        assert_eq!(std::mem::size_of::<Header>() as u64, HEADER_BYTES);
    }

    #[test]
    fn camera_cuts_reset_the_history() {
        let cam = Camera {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.1,
            far: 100.0,
        };
        let prev = History {
            view_proj: Mat4::IDENTITY,
            position: Vec3::ZERO,
            forward: Vec3::Z,
            ao: true,
        };
        let moved = |position, yaw: f32| Camera {
            position,
            rotation: Quat::from_rotation_y(yaw.to_radians()),
            ..cam
        };
        assert!(!camera_cut(&prev, &moved(Vec3::new(0.2, 0.0, 0.1), 5.0)));
        assert!(camera_cut(&prev, &moved(Vec3::new(8.0, 0.0, 0.0), 0.0)));
        assert!(camera_cut(&prev, &moved(Vec3::ZERO, 60.0)));
    }
}
