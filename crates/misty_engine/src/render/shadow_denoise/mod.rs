//! Temporal shadow mask: smooth soft shadows from few taps. For up to four soft-shadowed
//! lights (the sun first) a compute pass evaluates the shadow-map filter of shadows.wgsl at
//! every pixel of the screen-space depth + normal prepass with a sample pattern that moves
//! every frame, and blends it with last frame's result reprojected through the camera
//! motion (rejected where the depth disagrees, clamped to this frame's neighbourhood so
//! moving shadows follow at once). An edge-aware blur, as wide as half the penumbra, removes
//! what noise is left. The lit shaders read the result for those lights
//! (`shadow_mask_lookup` in shaders/shadow_lookup.wgsl) and fall back to the shadow maps
//! where it has nothing (transparent surfaces, MSAA edges, other lights).
//!
//! Runs only when Quality ▸ Soft Shadows is on and Temporal Shadows or Shadow Denoise is
//! ticked; otherwise the lit shaders read a zero header and nothing here costs anything.
//! The lookup the lit shaders append after shadows.wgsl is shaders/shadow_lookup.wgsl.

use std::sync::Mutex;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use super::shadows::ShadowPlan;
use super::{Frame, MAX_LIGHTS, RenderTarget, Renderer};
use crate::camera::{Camera, Projection};
use crate::quality::ShadowSettings;
use crate::scene::{LightType, Shadows};

/// The compute passes, with the shadow functions they share with the lit shaders.
const MASK_SRC: &str = concat!(
    include_str!("shaders/shadow_mask.wgsl"),
    "\n",
    include_str!("../shaders/shadows.wgsl")
);

/// Lights one mask covers (RGBA channels).
pub const CHANNELS: usize = 4;
/// Most frames of history (the newest frame weighs at least 1/16).
pub const MAX_HISTORY: u32 = 15;
/// Screen-space blur as a fraction of half the penumbra (shader `settings.y`).
const BLUR: f32 = 1.0;
/// Bind group binding of the mask in the lit shaders' group 1.
pub(super) const BINDING: u32 = 17;
const HEADER_BYTES: u64 = 64;
const TEXEL_BYTES: u64 = 16;
const NO_CHANNEL: u32 = 255;

fn buffer_bytes(pixels: u64) -> u64 {
    HEADER_BYTES + pixels.max(1) * TEXEL_BYTES
}

/// `ShadowMask` header (shadow_lookup.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Header {
    info: [u32; 4],
    channels: [[u32; 4]; 2],
    pad: [u32; 4],
}

/// `MaskParams` in shadow_mask.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MaskParams {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    prev_view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    forward: [f32; 4],
    prev_camera: [f32; 4],
    prev_forward: [f32; 4],
    size: [f32; 4],
    settings: [f32; 4],
    slots: [u32; 4],
}

/// Light slots (indices into the lights uniform, which skips lights without intensity and
/// stops at [`MAX_LIGHTS`]) the mask covers: soft-shadowed lights with a shadow map, the sun
/// first, at most [`CHANNELS`].
pub fn mask_slots(frame: &Frame, plan: &ShadowPlan) -> Vec<usize> {
    let mut slots = Vec::new();
    let mut n = 0;
    for (i, l) in frame.lights.iter().enumerate() {
        if n == MAX_LIGHTS || l.intensity <= 0.0 {
            continue;
        }
        let mapped = plan.light_layers.get(i).copied().flatten().is_some();
        if mapped && l.shadows == Shadows::Soft {
            slots.push((l.kind != LightType::Directional, n));
        }
        n += 1;
    }
    slots.sort_by_key(|(local, _)| *local);
    slots.into_iter().take(CHANNELS).map(|(_, n)| n).collect()
}

/// Compute pipelines and layouts (built the first time a view turns the mask on).
struct Passes {
    layout: wgpu::BindGroupLayout,
    lights_layout: wgpu::BindGroupLayout,
    trace: wgpu::ComputePipeline,
    filter: wgpu::ComputePipeline,
}

impl Passes {
    fn new(device: &wgpu::Device) -> Self {
        use wgpu::BindingType as B;
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty,
            count: None,
        };
        let uniform = B::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let storage = |read_only| B::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let texture = |sample_type, view_dimension| B::Texture {
            sample_type,
            view_dimension,
            multisampled: false,
        };
        let d2 = wgpu::TextureViewDimension::D2;
        let array = wgpu::TextureViewDimension::D2Array;
        let depth = wgpu::TextureSampleType::Depth;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty shadow mask"),
            entries: &[
                entry(0, uniform),
                entry(1, texture(depth, d2)),
                entry(
                    2,
                    texture(wgpu::TextureSampleType::Float { filterable: false }, d2),
                ),
                entry(3, storage(true)),
                entry(4, storage(false)),
            ],
        });
        // The lit shaders' group 1 bindings that shadows.wgsl reads.
        let lights_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty shadow mask lights"),
            entries: &[
                entry(0, uniform),
                entry(2, texture(depth, array)),
                entry(3, B::Sampler(wgpu::SamplerBindingType::Comparison)),
                entry(4, texture(depth, array)),
                entry(6, uniform),
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("misty shadow mask"),
            source: wgpu::ShaderSource::Wgsl(MASK_SRC.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("misty shadow mask"),
            bind_group_layouts: &[Some(&layout), Some(&lights_layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            trace: pipeline("cs_trace"),
            filter: pipeline("cs_filter"),
            layout,
            lights_layout,
        }
    }
}

/// Last frame of a view, for reprojection.
struct History {
    view_proj: Mat4,
    position: Vec3,
    forward: Vec3,
    /// The lights in the channels (another set = start afresh).
    slots: Vec<usize>,
}

/// A view's buffers (rebuilt when its size changes).
struct TargetState {
    size: (u32, u32),
    params: wgpu::Buffer,
    /// Ping-pong: the trace writes one (next frame's history), the blur writes the other
    /// (which the lit pass reads, and the trace overwrites next frame).
    buffers: [wgpu::Buffer; 2],
    frame: u32,
    history: Option<History>,
}

impl TargetState {
    fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        let bytes = buffer_bytes(size.0 as u64 * size.1 as u64);
        let buffer = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Self {
            size,
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty shadow mask params"),
                size: std::mem::size_of::<MaskParams>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            buffers: [buffer("misty shadow mask a"), buffer("misty shadow mask b")],
            frame: 0,
            history: None,
        }
    }
}

/// Per-target slot (inside [`RenderTarget`]).
#[derive(Default)]
pub(crate) struct TargetSlot(Mutex<Option<TargetState>>);

/// This render's mask: buffers in pass order.
struct MaskFrame {
    params: wgpu::Buffer,
    /// Last frame's history, the trace output, the blurred output (None = no blur: the lit
    /// pass reads the trace output).
    history: wgpu::Buffer,
    traced: wgpu::Buffer,
    blurred: Option<wgpu::Buffer>,
    size: (u32, u32),
}

/// The renderer's part.
pub(super) struct ShadowDenoise {
    passes: Option<Passes>,
    /// Zero header: bound while the mask is off.
    off_buffer: wgpu::Buffer,
    frame: Option<MaskFrame>,
}

impl ShadowDenoise {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            passes: None,
            off_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty shadow mask off"),
                size: buffer_bytes(1),
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
            frame: None,
        }
    }

    /// What the lit shaders bind at [`BINDING`] this render.
    pub fn lit_buffer(&self) -> &wgpu::Buffer {
        match &self.frame {
            Some(f) => f.blurred.as_ref().unwrap_or(&f.traced),
            None => &self.off_buffer,
        }
    }

    /// The off buffer (for groups that never see a mask).
    pub fn off_buffer(&self) -> &wgpu::Buffer {
        &self.off_buffer
    }

    /// The mask ran in the last render.
    #[cfg(test)]
    pub fn active(&self) -> bool {
        self.frame.is_some()
    }
}

/// A jump the history can't follow (teleport, switched camera).
fn camera_cut(prev: &History, cam: &Camera) -> bool {
    prev.position.distance(cam.position) > 5.0 || prev.forward.dot(cam.forward()) < 0.85
}

/// World size of a pixel: (per metre of depth, constant) for a view `height` pixels tall.
fn pixel_footprint(cam: &Camera, height: u32) -> (f32, f32) {
    let h = height.max(1) as f32;
    match cam.projection {
        Projection::Perspective { fov_y } => (2.0 * (fov_y.to_radians() * 0.5).tan() / h, 0.0),
        Projection::Orthographic { size } => (0.0, 2.0 * size / h),
    }
}

impl Renderer {
    /// The mask may run this render (decided before the shadow plan: it turns the
    /// screen-space prepass on and GPU culling off).
    pub(super) fn shadow_mask_wanted(&self, frame: &Frame) -> bool {
        let settings = self.quality.shadow_settings();
        frame.shaded
            && settings.mask()
            && frame
                .lights
                .iter()
                .any(|l| l.shadows == Shadows::Soft && l.intensity > 0.0)
    }

    /// Set up this render's mask (before `prepare_screen_space`, which binds
    /// [`ShadowDenoise::lit_buffer`] for the lit shaders).
    pub(super) fn prepare_shadow_mask(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        frame: &Frame,
        plan: &ShadowPlan,
        settings: &ShadowSettings,
    ) {
        self.shadow_denoise.frame = None;
        if !self.shadow_mask_wanted(frame) {
            return;
        }
        let slots = mask_slots(frame, plan);
        let size = target.size();
        let bytes = buffer_bytes(size.0 as u64 * size.1 as u64);
        if slots.is_empty() || bytes > device.limits().max_storage_buffer_binding_size {
            return;
        }
        if self.shadow_denoise.passes.is_none() {
            self.shadow_denoise.passes = Some(Passes::new(device));
        }
        let Ok(mut slot) = target.shadow_mask.0.lock() else {
            return;
        };
        if slot.as_ref().is_none_or(|s| s.size != size) {
            *slot = Some(TargetState::new(device, size));
        }
        let Some(state) = slot.as_mut() else {
            return;
        };

        let cam = frame.camera;
        let view_proj = cam.view_projection(target.aspect());
        let history = state.history.as_ref().filter(|h| {
            settings.temporal && h.slots == slots && !camera_cut(h, cam) && state.frame > 0
        });
        let (prev_view_proj, prev_pos, prev_fwd) = history
            .map_or((view_proj, cam.position, cam.forward()), |h| {
                (h.view_proj, h.position, h.forward)
            });
        let reuse = history.is_some();
        let mut lights = [NO_CHANNEL; 4];
        for (c, s) in slots.iter().enumerate() {
            lights[c] = *s as u32;
        }
        let (per_depth, constant) = pixel_footprint(cam, size.1);
        let v4 = |v: Vec3, w: f32| v.extend(w).to_array();
        let ortho = matches!(cam.projection, Projection::Orthographic { .. });
        let params = MaskParams {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            prev_view_proj: prev_view_proj.to_cols_array_2d(),
            camera: v4(cam.position, if ortho { 1.0 } else { 0.0 }),
            forward: v4(cam.forward(), 0.0),
            prev_camera: v4(prev_pos, 0.0),
            prev_forward: v4(prev_fwd, 0.0),
            size: [
                size.0 as f32,
                size.1 as f32,
                // A fixed pattern without accumulation (no crawling noise).
                if settings.temporal {
                    (state.frame % 64) as f32
                } else {
                    0.0
                },
                if reuse { 1.0 } else { 0.0 },
            ],
            settings: [
                MAX_HISTORY as f32,
                if settings.denoise { BLUR } else { 0.0 },
                per_depth,
                constant,
            ],
            slots: lights,
        };
        queue.write_buffer(&state.params, 0, bytemuck::bytes_of(&params));

        let mut channels = [[NO_CHANNEL; 4]; 2];
        for (c, s) in slots.iter().enumerate() {
            channels[s / 4][s % 4] = c as u32;
        }
        let header = Header {
            info: [1, size.0, size.1, 0],
            channels,
            pad: [0; 4],
        };
        for b in &state.buffers {
            queue.write_buffer(b, 0, bytemuck::bytes_of(&header));
        }
        // Even frames trace into A (history = B), odd frames into B.
        let current = (state.frame % 2) as usize;
        let traced = state.buffers[current].clone();
        let other = state.buffers[1 - current].clone();
        self.shadow_denoise.frame = Some(MaskFrame {
            params: state.params.clone(),
            history: other.clone(),
            traced,
            blurred: settings.denoise.then_some(other),
            size,
        });
        state.frame = state.frame.wrapping_add(1);
        state.history = Some(History {
            view_proj,
            position: cam.position,
            forward: cam.forward(),
            slots,
        });
    }

    /// Drop this render's mask (the screen-space prepass it needs isn't running).
    pub(super) fn cancel_shadow_mask(&mut self) {
        self.shadow_denoise.frame = None;
    }

    /// Trace + accumulate, then blur (after the shadow maps and the screen-space prepass).
    pub(super) fn shadow_mask_passes(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        normals: &wgpu::TextureView,
    ) {
        let (Some(passes), Some(f)) = (&self.shadow_denoise.passes, &self.shadow_denoise.frame)
        else {
            return;
        };
        let lights = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty shadow mask lights"),
            layout: &passes.lights_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.lights_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.shadows.cascade_array_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.shadows.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&self.shadows.local_array_view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: self.shadows.params_buffer.as_entire_binding(),
                },
            ],
        });
        let group = |src: &wgpu::Buffer, dst: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty shadow mask"),
                layout: &passes.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: f.params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(normals),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: src.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: dst.as_entire_binding(),
                    },
                ],
            })
        };
        let trace = group(&f.history, &f.traced);
        let blur = f.blurred.as_ref().map(|b| group(&f.traced, b));
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("misty shadow mask"),
            timestamp_writes: None,
        });
        let (x, y) = (f.size.0.div_ceil(8), f.size.1.div_ceil(8));
        pass.set_bind_group(1, &lights, &[]);
        pass.set_pipeline(&passes.trace);
        pass.set_bind_group(0, &trace, &[]);
        pass.dispatch_workgroups(x, y, 1);
        if let Some(blur) = &blur {
            pass.set_pipeline(&passes.filter);
            pass.set_bind_group(0, blur, &[]);
            pass.dispatch_workgroups(x, y, 1);
        }
    }
}

#[cfg(test)]
#[path = "gpu_tests.rs"]
mod gpu_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::naga;

    fn validate(src: &str) {
        let module = naga::front::wgsl::parse_str(src)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }

    #[test]
    fn shaders_validate_and_layouts_match() {
        validate(MASK_SRC);
        assert_eq!(std::mem::size_of::<Header>() as u64, HEADER_BYTES);
        assert_eq!(std::mem::size_of::<MaskParams>(), 3 * 64 + 6 * 16 + 16);
    }

    #[test]
    fn the_sun_takes_the_first_channel() {
        use crate::render::LightData;
        let light = |kind, shadows, intensity| LightData {
            kind,
            position: Vec3::ZERO,
            direction: Vec3::NEG_Y,
            color: [1.0; 3],
            intensity,
            range: 10.0,
            spot_angle: 30.0,
            shadows,
            shadow: Default::default(),
            volumetric: Default::default(),
            contact_shadows: false,
            area: Default::default(),
        };
        let lights = [
            light(LightType::Spot, Shadows::Soft, 1.0),
            light(LightType::Point, Shadows::Soft, 0.0),
            light(LightType::Spot, Shadows::Hard, 1.0),
            light(LightType::Directional, Shadows::Soft, 1.0),
            light(LightType::Rectangle, Shadows::Soft, 1.0),
        ];
        let camera = Camera {
            position: Vec3::ZERO,
            rotation: glam::Quat::IDENTITY,
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.1,
            far: 100.0,
        };
        let frame = Frame {
            camera: &camera,
            grid: None,
            skybox: false,
            background: [0.0; 3],
            environment: &Default::default(),
            post: Default::default(),
            lines: &[],
            overlay: &[],
            meshes: &[],
            particles: &[],
            terrains: &[],
            shaded: true,
            lights: &lights,
            water: &[],
        };
        let plan = ShadowPlan {
            light_layers: vec![Some(0), Some(1), Some(2), Some(-1), Some(3)],
            ..Default::default()
        };
        // Slots skip the dark point light: spot 0, hard spot 1, sun 2, rectangle 3.
        assert_eq!(mask_slots(&frame, &plan), vec![2, 0, 3]);
    }
}
