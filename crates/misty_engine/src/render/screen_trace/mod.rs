//! Screen-space traced lighting: Screen Space Global Illumination (one bounce of diffuse light
//! from what is on screen) and Screen Space Reflections (Hi-Z traced mirror rays blurred by
//! roughness). Both read last frame's lit colour (the opaque scene, copied after opaque
//! geometry and box-filtered into mips) through reprojection, and this frame's depth + normal
//! prepass (shared with AO, `screen_space.rs`). Their results are packed into one texel per
//! pixel that the lit shaders read (mesh group binding 17, `shaders/screen_trace.wgsl`).
//! While both are off nothing here runs and the lit shaders read a zero header.
//!
//! Per render, after the prepass: [Hi-Z build, SSR trace, temporal, roughness blur],
//! [SSGI trace, temporal, three à-trous iterations], compose into the texels; after the
//! opaque part of the main pass: copy the lit colour into the history and build its mips.

use std::sync::Mutex;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use super::frame_debug::{self, EventKind, EventTarget, FrameEvent, TargetInfo};
use super::{Frame, RenderTarget, Renderer};
use crate::camera::{Camera, Projection};

mod resources;
mod settings;

pub use settings::{SsgiSettings, SsrSettings};

const COMMON_SRC: &str = include_str!("shaders/trace_common.wgsl");
const SSGI_SRC: &str = include_str!("shaders/ssgi.wgsl");
const SSR_SRC: &str = include_str!("shaders/ssr.wgsl");
const COMPOSE_SRC: &str = include_str!("shaders/compose.wgsl");
const MIPS_SRC: &str = include_str!("shaders/mips.wgsl");
/// Images of the passes (radiance + weight).
const IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const HIZ_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
/// Header (two vec4s) before the per-pixel texels.
const HEADER_BYTES: u64 = 32;
const TEXEL_BYTES: u64 = 16;
/// Mips of the lit colour history (cone-matched reads).
const MAX_COLOR_MIPS: u32 = 6;
/// Share of last frame's reflection in the temporal blend.
const SSR_HISTORY_WEIGHT: f32 = 0.85;

fn buffer_bytes(texels: u64) -> u64 {
    (HEADER_BYTES + texels * TEXEL_BYTES).next_multiple_of(16)
}

/// Index (in vec4s of `texels`) where Misty GI's probe copy starts for a `full`-sized view.
fn world_vec4(full: (u32, u32)) -> u64 {
    full.0 as u64 * full.1 as u64
}

/// What the lit shaders read (`ScreenTrace` in screen_trace.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Header {
    flags: [f32; 4],
    misc: [f32; 4],
}

/// Uniform of the trace passes (`TraceParams` in trace_common.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TraceParams {
    inv_view_proj: [[f32; 4]; 4],
    view_proj: [[f32; 4]; 4],
    prev_view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    forward: [f32; 4],
    prev_camera: [f32; 4],
    prev_forward: [f32; 4],
    size: [f32; 4],
    size2: [f32; 4],
    clip: [f32; 4],
    gi: [f32; 4],
    gi2: [f32; 4],
    ssr: [f32; 4],
    ssr2: [f32; 4],
    /// x = full pixels per Misty GI texel, y = 1 when Misty GI is on, z = its intensity,
    /// w = SSGI intensity (folded into the composed GI when Misty GI is on).
    misty: [f32; 4],
    /// x = 1 when Misty GI's glossy reflections join SSR's.
    misty2: [f32; 4],
}

/// Compute kernels (index into [`Passes::pipelines`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kernel {
    GiTrace,
    GiTemporal,
    GiDenoise1,
    GiDenoise2,
    GiDenoise4,
    SsrTrace,
    SsrTemporal,
    SsrBlur,
    Compose,
    HizCopy,
    HizDown,
    ColorDown,
}

/// Pipelines, layouts and placeholder resources, built the first time a view turns an effect on.
struct Passes {
    layout: wgpu::BindGroupLayout,
    hiz_copy_layout: wgpu::BindGroupLayout,
    hiz_down_layout: wgpu::BindGroupLayout,
    color_down_layout: wgpu::BindGroupLayout,
    pipelines: Vec<(Kernel, wgpu::ComputePipeline)>,
    sampler: wgpu::Sampler,
    /// 1×1 stand-ins for bindings a kernel doesn't use.
    dummy_in: wgpu::TextureView,
    dummy_out: [wgpu::TextureView; 2],
    dummy_buffer: wgpu::Buffer,
}

/// The renderer's part: pipelines and the "off" buffer.
pub(super) struct ScreenTrace {
    passes: Option<Passes>,
    /// Misty GI (world-space GI composed into the same texels).
    pub misty: super::misty_gi::MistyGi,
    /// Zero header: bound while both effects are off.
    pub off_buffer: wgpu::Buffer,
}

impl ScreenTrace {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            passes: None,
            misty: Default::default(),
            off_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty screen trace off"),
                size: buffer_bytes(1),
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
        }
    }
}

/// SSGI images at GI resolution.
struct GiImages {
    trace: wgpu::TextureView,
    /// Ping-pong accumulation (rgb + visibility) and (history length, linear depth).
    accum: [wgpu::TextureView; 2],
    info: [wgpu::TextureView; 2],
    /// À-trous ping-pong.
    blur: [wgpu::TextureView; 2],
}

/// SSR images at SSR resolution, and the closest-depth pyramid.
struct SsrImages {
    trace: wgpu::TextureView,
    history: [wgpu::TextureView; 2],
    out: wgpu::TextureView,
    hiz_all: wgpu::TextureView,
    hiz_mips: Vec<wgpu::TextureView>,
}

/// What a view's resources were made for (rebuilt when it changes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    /// Bytes of Misty GI's probe copy after the texels (0 = none).
    world: u64,
    full: (u32, u32),
    /// Full pixels per GI / SSR texel (`None` = effect off).
    gi: Option<u32>,
    ssr: Option<u32>,
}

/// Last frame of a view.
struct History {
    view_proj: Mat4,
    position: Vec3,
    forward: Vec3,
    gi: bool,
    ssr: bool,
}

/// A view's textures and buffers.
struct TargetState {
    key: Key,
    gi_size: (u32, u32),
    ssr_size: (u32, u32),
    color: wgpu::Texture,
    color_all: wgpu::TextureView,
    color_mips: Vec<wgpu::TextureView>,
    gi: Option<GiImages>,
    ssr: Option<SsrImages>,
    params: wgpu::Buffer,
    texels: wgpu::Buffer,
    frame: u32,
    history: Option<History>,
    /// The colour history holds a lit frame of this view.
    color_valid: bool,
}

/// Per-target slot (inside [`RenderTarget`]).
#[derive(Default)]
pub(crate) struct TargetSlot(Mutex<Option<TargetState>>);

/// One dispatch.
struct Step {
    kernel: Kernel,
    group: wgpu::BindGroup,
    size: (u32, u32),
}

/// This render's trace work.
pub(in crate::render) struct TraceFrame {
    /// Texels for the lit shaders (mesh group binding 17).
    pub buffer: wgpu::Buffer,
    /// The effects run this render (false on a view's first frame: no lit history yet).
    pub gi: bool,
    pub ssr: bool,
    /// The images are full resolution (Frame Debugger sizes).
    gi_full: bool,
    ssr_full: bool,
    gi_steps: Vec<Step>,
    ssr_steps: Vec<Step>,
    compose: Option<Step>,
    /// Misty GI work of this render (its gather feeds the compose).
    pub misty: Option<super::misty_gi::MistyFrame>,
    /// Misty GI's probes → the texel buffer (source, destination offset, bytes).
    world_copy: Option<(wgpu::Buffer, u64, u64)>,
    /// Lit colour history: copy destination and its mip steps.
    color: wgpu::Texture,
    color_steps: Vec<Step>,
}

impl ScreenTrace {
    /// Set up this render's SSGI / SSR (`None` when both are off or the view is too large).
    /// `normal_view` / `depth_view` / `velocity_view` = the prepass targets of this view.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        frame: &Frame,
        normal_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        velocity_view: &wgpu::TextureView,
    ) -> Option<TraceFrame> {
        let (gi, ssr, misty_gi) = (frame.post.ssgi, frame.post.ssr, frame.post.misty_gi);
        if !wanted(frame) {
            return None;
        }
        let full = target.size();
        let bytes = buffer_bytes(full.0 as u64 * full.1 as u64);
        if bytes > device.limits().max_storage_buffer_binding_size {
            return None;
        }
        if self.passes.is_none() {
            self.passes = Some(Passes::new(device));
        }
        let misty = self
            .misty
            .prepare(device, queue, target, frame, normal_view, depth_view);
        let misty_on = misty.is_some();
        let misty_reflections = misty.as_ref().is_some_and(|m| m.reflections);
        // Misty GI's probes follow the texels (the lit shaders' world lookup).
        let world_bytes = misty.as_ref().map_or(0, |m| {
            (super::misty_gi::WORLD_HEADER_VEC4S as u64) * 16 + m.probe_bytes
        });
        let bytes = bytes + world_bytes;
        if bytes > device.limits().max_storage_buffer_binding_size {
            return None;
        }
        let passes = self.passes.as_ref()?;
        let key = Key {
            world: world_bytes,
            full,
            gi: gi
                .active()
                .then_some(if gi.full_resolution { 1 } else { 2 }),
            ssr: ssr
                .active()
                .then_some(if ssr.full_resolution { 1 } else { 2 }),
        };
        let mut slot = target.screen_trace.0.lock().ok()?;
        if slot.as_ref().is_none_or(|s| s.key != key) {
            *slot = Some(TargetState::new(device, key, bytes));
        }
        let state = slot.as_mut()?;

        let cam = frame.camera;
        let view_proj = cam.view_projection(target.aspect());
        let cut = state.history.as_ref().is_none_or(|h| camera_cut(h, cam));
        let prev = state.history.as_ref().map_or(view_proj, |h| h.view_proj);
        let gi_history = !cut && state.history.as_ref().is_some_and(|h| h.gi);
        let ssr_history = !cut && ssr.temporal && state.history.as_ref().is_some_and(|h| h.ssr);
        // Without a lit frame of this view yet there is nothing to trace against.
        let run = state.color_valid;
        let (gi_on, ssr_on) = (run && key.gi.is_some(), run && key.ssr.is_some());
        let params = trace_params(
            frame,
            state,
            view_proj,
            prev,
            [gi_history, ssr_history],
            [gi_on, ssr_on],
            misty
                .as_ref()
                .map(|m| (m.scale, misty_gi.intensity, m.reflections)),
        );
        queue.write_buffer(&state.params, 0, bytemuck::bytes_of(&params));
        let flag = |on: bool| if on { 1.0 } else { 0.0 };
        // With Misty GI on, the composed GI already holds both intensities and replaces the
        // ambient everywhere (visibility 0); the lit shaders multiply it by the screen AO.
        let header = match misty_on {
            false => Header {
                flags: [flag(gi_on), gi.intensity, flag(ssr_on), ssr.intensity],
                misc: [flag(gi.lightmapped_surfaces), 0.0, 0.0, 0.0],
            },
            true => Header {
                // Misty GI's glossy reflections fill where SSR finds nothing (or SSR is off).
                flags: [
                    1.0,
                    1.0,
                    flag(ssr_on || misty_reflections),
                    if ssr_on { ssr.intensity } else { 1.0 },
                ],
                misc: [
                    flag(misty_gi.lightmapped_surfaces),
                    1.0,
                    world_vec4(full) as f32,
                    misty_gi.intensity,
                ],
            },
        };
        queue.write_buffer(&state.texels, 0, bytemuck::bytes_of(&header));
        let world_copy = misty.as_ref().map(|m| {
            let at = HEADER_BYTES + world_vec4(full) * 16;
            queue.write_buffer(&state.texels, at, bytemuck::cast_slice(&m.world_header));
            let probes_at = at + m.world_header.len() as u64 * 16;
            (m.probes.clone(), probes_at, m.probe_bytes)
        });

        let views = Views {
            depth: depth_view,
            normal: normal_view,
            velocity: velocity_view,
        };
        let parity = (state.frame % 2) as usize;
        let (gi_steps, gi_final) = match (gi_on, &state.gi) {
            (true, Some(images)) => {
                let denoise = frame.post.ssgi.denoise;
                gi_steps(device, passes, state, &views, images, parity, denoise)
            }
            _ => (Vec::new(), None),
        };
        let (ssr_steps, ssr_final) = match (ssr_on, &state.ssr) {
            (true, Some(images)) => ssr_steps(device, passes, state, &views, images, parity),
            _ => (Vec::new(), None),
        };
        let compose = (gi_on || ssr_on || misty_on).then(|| {
            let mut b = Binds::new(passes, state, &views);
            b.in_a = gi_final.unwrap_or(&passes.dummy_in);
            b.in_b = ssr_final.unwrap_or(&passes.dummy_in);
            b.in_c = misty.as_ref().map_or(&passes.dummy_in, |m| &m.gather);
            b.in_d = misty.as_ref().map_or(&passes.dummy_in, |m| &m.reflect);
            b.buffer = &state.texels;
            Step {
                kernel: Kernel::Compose,
                group: b.group(device),
                size: full,
            }
        });
        let color_steps = color_steps(device, passes, state, normal_view);

        state.frame = state.frame.wrapping_add(1);
        state.history = Some(History {
            view_proj,
            position: cam.position,
            forward: cam.forward(),
            gi: gi_on,
            ssr: ssr_on,
        });
        // The copy after the opaque pass fills it this render.
        state.color_valid = true;
        Some(TraceFrame {
            buffer: state.texels.clone(),
            gi: gi_on,
            ssr: ssr_on,
            gi_full: key.gi == Some(1),
            ssr_full: key.ssr == Some(1),
            gi_steps,
            ssr_steps,
            compose,
            misty,
            world_copy,
            color: state.color.clone(),
            color_steps,
        })
    }

    /// The SSR (`ssr`) and SSGI (`gi`) passes, then the compose into the texels.
    pub fn passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        t: &TraceFrame,
        (gi, ssr): (bool, bool),
    ) {
        let Some(passes) = self.passes.as_ref() else {
            return;
        };
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("misty screen trace"),
            timestamp_writes: None,
        });
        let steps = t.ssr_steps.iter().filter(|_| ssr);
        let steps = steps.chain(t.gi_steps.iter().filter(|_| gi));
        for step in steps.chain(t.compose.iter()) {
            passes.dispatch(&mut pass, step);
        }
    }

    /// Copy the resolved opaque scene into the colour history and build its mips.
    pub fn record_history(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
        t: &TraceFrame,
    ) {
        let Some(passes) = self.passes.as_ref() else {
            return;
        };
        let size = target.size();
        encoder.copy_texture_to_texture(
            target.hdr_view().texture().as_image_copy(),
            t.color.as_image_copy(),
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("misty screen trace history"),
            timestamp_writes: None,
        });
        for step in &t.color_steps {
            passes.dispatch(&mut pass, step);
        }
    }
}

impl Renderer {
    /// This render's SSR and SSGI passes (Frame Debugger events "Screen Space Reflections"
    /// and "Screen Space Global Illumination"), then the compose into the lit texels.
    pub(super) fn screen_trace_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
        t: &TraceFrame,
    ) {
        let image = |name: &str, scale_full: bool| {
            let (w, h) = target.size();
            let size = if scale_full {
                (w, h)
            } else {
                (w.div_ceil(2), h.div_ceil(2))
            };
            frame_debug::target(name, size, "Rgba16Float")
        };
        let event = |name: &str, shader: &str| FrameEvent {
            shader: shader.to_owned(),
            ..FrameEvent::new(EventKind::Compute, name)
        };
        let ssr = t.ssr
            && frame_debug::whole(
                &self.capture,
                &["Screen Space Reflections"],
                || image("SSR", t.ssr_full),
                EventTarget::None,
                || {
                    event(
                        "Hi-Z Pyramid + SSR Trace, Temporal, Roughness Blur",
                        "screen_trace/mips.wgsl, ssr.wgsl",
                    )
                },
            );
        let gi = t.gi
            && frame_debug::whole(
                &self.capture,
                &["Screen Space Global Illumination"],
                || image("SSGI", t.gi_full),
                EventTarget::None,
                || {
                    event(
                        "SSGI Trace, Temporal, À-trous Denoise",
                        "screen_trace/ssgi.wgsl",
                    )
                },
            );
        if (gi || ssr || t.misty.is_some())
            && frame_debug::record(&self.capture, |_| {
                event("Compose Screen Trace Texels", "screen_trace/compose.wgsl")
            })
        {
            self.screen_trace.passes(encoder, t, (gi, ssr));
        }
    }

    /// This render's Misty GI stages (Frame Debugger event "Misty GI"), each timed.
    pub(super) fn misty_gi_passes(&self, encoder: &mut wgpu::CommandEncoder, t: &TraceFrame) {
        let Some(m) = &t.misty else {
            return;
        };
        let run = frame_debug::whole(
            &self.capture,
            &["Misty GI"],
            TargetInfo::default,
            EventTarget::None,
            || FrameEvent {
                shader: "misty_gi/sdf.wgsl, surface.wgsl, probes.wgsl, apply.wgsl".to_owned(),
                ..FrameEvent::new(
                    EventKind::Compute,
                    "Scene SDF, Surface Cache, Probe Trace, Gather",
                )
            },
        );
        if !run {
            return;
        }
        for stage in super::misty_gi::Stage::ALL {
            if m.has(stage) {
                self.screen_trace.misty.record(encoder, m, stage);
                self.mark(encoder, stage.label());
            }
        }
        if let Some((probes, at, bytes)) = &t.world_copy {
            encoder.copy_buffer_to_buffer(probes, 0, &t.buffer, *at, *bytes);
        }
    }

    /// Copy the opaque scene into the SSGI / SSR colour history (Frame Debugger event).
    pub(super) fn screen_trace_history(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
        t: &TraceFrame,
    ) {
        let main = || super::main_target(target);
        frame_debug::pass(
            &self.capture,
            &["Screen Space Lighting History"],
            main,
            EventTarget::Hdr,
        );
        if frame_debug::record(&self.capture, |_| {
            FrameEvent::new(EventKind::Copy, "Copy Lit Color History + Mips")
        }) {
            self.screen_trace.record_history(encoder, target, t);
        }
    }
}

/// SSGI, SSR or Misty GI is on for this render.
pub(super) fn wanted(frame: &Frame) -> bool {
    frame.shaded
        && (frame.post.ssgi.active() || frame.post.ssr.active() || frame.post.misty_gi.active())
}

/// The prepass targets a step reads.
struct Views<'a> {
    depth: &'a wgpu::TextureView,
    normal: &'a wgpu::TextureView,
    velocity: &'a wgpu::TextureView,
}

/// Bindings of one trace step (layout in trace_common.wgsl); unused ones are placeholders.
struct Binds<'a> {
    layout: &'a wgpu::BindGroupLayout,
    params: &'a wgpu::Buffer,
    depth: &'a wgpu::TextureView,
    normal: &'a wgpu::TextureView,
    velocity: &'a wgpu::TextureView,
    color: &'a wgpu::TextureView,
    sampler: &'a wgpu::Sampler,
    hiz: &'a wgpu::TextureView,
    in_a: &'a wgpu::TextureView,
    in_b: &'a wgpu::TextureView,
    in_c: &'a wgpu::TextureView,
    in_d: &'a wgpu::TextureView,
    out_a: &'a wgpu::TextureView,
    out_b: &'a wgpu::TextureView,
    buffer: &'a wgpu::Buffer,
}

impl<'a> Binds<'a> {
    fn new(passes: &'a Passes, state: &'a TargetState, views: &Views<'a>) -> Self {
        Self {
            layout: &passes.layout,
            params: &state.params,
            depth: views.depth,
            normal: views.normal,
            velocity: views.velocity,
            color: &state.color_all,
            sampler: &passes.sampler,
            hiz: state.ssr.as_ref().map_or(&passes.dummy_in, |s| &s.hiz_all),
            in_a: &passes.dummy_in,
            in_b: &passes.dummy_in,
            in_c: &passes.dummy_in,
            in_d: &passes.dummy_in,
            out_a: &passes.dummy_out[0],
            out_b: &passes.dummy_out[1],
            buffer: &passes.dummy_buffer,
        }
    }

    fn group(&self, device: &wgpu::Device) -> wgpu::BindGroup {
        use resources::{buf, tex};
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty screen trace group"),
            layout: self.layout,
            entries: &[
                buf(0, self.params),
                tex(1, self.depth),
                tex(2, self.normal),
                tex(3, self.color),
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(self.sampler),
                },
                tex(5, self.hiz),
                tex(6, self.in_a),
                tex(7, self.in_b),
                tex(8, self.in_c),
                tex(9, self.out_a),
                tex(10, self.out_b),
                buf(11, self.buffer),
                tex(12, self.velocity),
                tex(13, self.in_d),
            ],
        })
    }
}

/// Trace, temporal, denoise; returns the steps and the final image.
fn gi_steps<'a>(
    device: &wgpu::Device,
    passes: &'a Passes,
    state: &'a TargetState,
    views: &Views<'a>,
    images: &'a GiImages,
    parity: usize,
    denoise: bool,
) -> (Vec<Step>, Option<&'a wgpu::TextureView>) {
    let size = state.gi_size;
    let step = |kernel, b: Binds| Step {
        kernel,
        group: b.group(device),
        size,
    };
    let (cur, prev) = (parity, 1 - parity);
    let mut steps = Vec::new();
    let mut b = Binds::new(passes, state, views);
    b.out_a = &images.trace;
    steps.push(step(Kernel::GiTrace, b));
    let mut b = Binds::new(passes, state, views);
    b.in_a = &images.trace;
    b.in_b = &images.accum[prev];
    b.in_c = &images.info[prev];
    b.out_a = &images.accum[cur];
    b.out_b = &images.info[cur];
    steps.push(step(Kernel::GiTemporal, b));
    if !denoise {
        return (steps, Some(&images.accum[cur]));
    }
    // À-trous: accumulation → blur 0 → blur 1 → blur 0.
    let chain = [
        (Kernel::GiDenoise1, &images.accum[cur], &images.blur[0]),
        (Kernel::GiDenoise2, &images.blur[0], &images.blur[1]),
        (Kernel::GiDenoise4, &images.blur[1], &images.blur[0]),
    ];
    for (kernel, input, output) in chain {
        let mut b = Binds::new(passes, state, views);
        b.in_a = input;
        b.out_a = output;
        steps.push(step(kernel, b));
    }
    (steps, Some(&images.blur[0]))
}

/// Hi-Z build, trace, temporal, blur; returns the steps and the final image.
fn ssr_steps<'a>(
    device: &wgpu::Device,
    passes: &'a Passes,
    state: &'a TargetState,
    views: &Views<'a>,
    images: &'a SsrImages,
    parity: usize,
) -> (Vec<Step>, Option<&'a wgpu::TextureView>) {
    use resources::tex;
    let mut steps = Vec::new();
    let mip_size = |i: usize| {
        let (w, h) = state.key.full;
        ((w >> i).max(1), (h >> i).max(1))
    };
    steps.push(Step {
        kernel: Kernel::HizCopy,
        group: device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty hiz copy"),
            layout: &passes.hiz_copy_layout,
            entries: &[tex(0, views.depth), tex(2, &images.hiz_mips[0])],
        }),
        size: state.key.full,
    });
    for i in 1..images.hiz_mips.len() {
        steps.push(Step {
            kernel: Kernel::HizDown,
            group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty hiz down"),
                layout: &passes.hiz_down_layout,
                entries: &[tex(1, &images.hiz_mips[i - 1]), tex(2, &images.hiz_mips[i])],
            }),
            size: mip_size(i),
        });
    }
    let size = state.ssr_size;
    let step = |kernel, b: Binds| Step {
        kernel,
        group: b.group(device),
        size,
    };
    let mut b = Binds::new(passes, state, views);
    b.out_a = &images.trace;
    steps.push(step(Kernel::SsrTrace, b));
    let mut b = Binds::new(passes, state, views);
    b.in_a = &images.trace;
    b.in_b = &images.history[1 - parity];
    b.out_a = &images.history[parity];
    steps.push(step(Kernel::SsrTemporal, b));
    let mut b = Binds::new(passes, state, views);
    b.in_a = &images.history[parity];
    b.out_a = &images.out;
    steps.push(step(Kernel::SsrBlur, b));
    (steps, Some(&images.out))
}

/// Box-filtered mips of the colour history (after the copy into level 0). The first step
/// also reads the prepass normals and leaves out surfaces that send no light through SSGI
/// (a light's emitter mesh), so wide cones near them don't pick up their glow.
fn color_steps(
    device: &wgpu::Device,
    passes: &Passes,
    state: &TargetState,
    normal: &wgpu::TextureView,
) -> Vec<Step> {
    use resources::tex;
    (1..state.color_mips.len())
        .map(|i| {
            let (w, h) = state.key.full;
            Step {
                kernel: Kernel::ColorDown,
                group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("misty colour history mip"),
                    layout: &passes.color_down_layout,
                    entries: &[
                        tex(1, &state.color_mips[i - 1]),
                        tex(3, &state.color_mips[i]),
                        tex(4, if i == 1 { normal } else { &passes.dummy_in }),
                    ],
                }),
                size: ((w >> i).max(1), (h >> i).max(1)),
            }
        })
        .collect()
}

/// A jump the history can't follow (teleport, switched camera): start afresh.
fn camera_cut(prev: &History, cam: &Camera) -> bool {
    prev.position.distance(cam.position) > 5.0 || prev.forward.dot(cam.forward()) < 0.85
}

fn trace_params(
    frame: &Frame,
    state: &TargetState,
    view_proj: Mat4,
    prev: Mat4,
    [gi_history, ssr_history]: [bool; 2],
    [gi_on, ssr_on]: [bool; 2],
    misty: Option<(u32, f32, bool)>,
) -> TraceParams {
    let cam = frame.camera;
    let (gi, ssr) = (&frame.post.ssgi, &frame.post.ssr);
    let ortho = matches!(cam.projection, Projection::Orthographic { .. });
    let v4 = |v: Vec3, w: f32| v.extend(w).to_array();
    let flag = |on: bool| if on { 1.0 } else { 0.0 };
    let (prev_position, prev_forward) = state
        .history
        .as_ref()
        .map_or((cam.position, cam.forward()), |h| (h.position, h.forward));
    let full = state.key.full;
    let hiz_mips = state.ssr.as_ref().map_or(1, |s| s.hiz_mips.len());
    TraceParams {
        inv_view_proj: view_proj.inverse().to_cols_array_2d(),
        view_proj: view_proj.to_cols_array_2d(),
        prev_view_proj: prev.to_cols_array_2d(),
        camera: v4(cam.position, flag(ortho)),
        forward: v4(cam.forward(), (state.frame % 1024) as f32),
        prev_camera: v4(prev_position, flag(gi_history)),
        prev_forward: v4(prev_forward, flag(state.color_valid)),
        size: [
            full.0 as f32,
            full.1 as f32,
            state.gi_size.0 as f32,
            state.gi_size.1 as f32,
        ],
        size2: [
            state.ssr_size.0 as f32,
            state.ssr_size.1 as f32,
            state.color_mips.len() as f32,
            hiz_mips as f32,
        ],
        clip: [
            cam.near,
            cam.far,
            state.key.gi.unwrap_or(1) as f32,
            state.key.ssr.unwrap_or(1) as f32,
        ],
        gi: [gi.radius, gi.rays() as f32, gi.steps() as f32, gi.thickness],
        gi2: [gi.max_history(), flag(gi_on), flag(ssr_on), 0.0],
        ssr: [
            ssr.max_roughness,
            ssr.thickness,
            ssr.steps() as f32,
            ssr.edge_fade,
        ],
        ssr2: [
            if ssr_history { SSR_HISTORY_WEIGHT } else { 0.0 },
            ssr.max_distance,
            0.0,
            0.0,
        ],
        misty: match misty {
            Some((scale, intensity, _)) => [
                scale as f32,
                1.0,
                intensity,
                if gi_on { gi.intensity } else { 0.0 },
            ],
            None => [1.0, 0.0, 0.0, 0.0],
        },
        misty2: [flag(misty.is_some_and(|m| m.2)), 0.0, 0.0, 0.0],
    }
}

#[cfg(test)]
mod gpu_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::naga;

    fn validate(label: &str, src: &str) {
        let module = naga::front::wgsl::parse_str(src)
            .unwrap_or_else(|e| panic!("{label}: {}", e.emit_to_string(src)));
        // Default capabilities: nothing beyond what every adapter has (no f16).
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{label}: {e:?}"));
    }

    #[test]
    fn shaders_validate() {
        validate("ssgi", &format!("{COMMON_SRC}\n{SSGI_SRC}"));
        validate("ssr", &format!("{COMMON_SRC}\n{SSR_SRC}"));
        validate("compose", &format!("{COMMON_SRC}\n{COMPOSE_SRC}"));
        validate("mips", MIPS_SRC);
        // The lit shaders with this include (mesh.wgsl and its includes).
        let lit = format!(
            "{}\n{}",
            super::super::pipelines::COMMON,
            super::super::pipelines::MESH_SRC
        );
        validate("lit", &lit);
        assert_eq!(std::mem::size_of::<TraceParams>(), 3 * 64 + 13 * 16);
        assert_eq!(std::mem::size_of::<Header>() as u64, HEADER_BYTES);
    }

    #[test]
    fn camera_cuts_reset_the_history() {
        let cam = Camera {
            position: Vec3::ZERO,
            rotation: glam::Quat::IDENTITY,
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.1,
            far: 100.0,
        };
        let prev = History {
            view_proj: Mat4::IDENTITY,
            position: Vec3::ZERO,
            forward: Vec3::Z,
            gi: true,
            ssr: true,
        };
        assert!(!camera_cut(&prev, &cam));
        let far = Camera {
            position: Vec3::new(9.0, 0.0, 0.0),
            ..cam
        };
        assert!(camera_cut(&prev, &far));
    }
}
