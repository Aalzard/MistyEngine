//! Weather rendering (see [`crate::weather`] for the scene side).
//!
//! Per render, when the frame's environment carries a [`WeatherFrame`]:
//! 1. **Globals** (view group bindings 24–25): the weather amounts, area volumes, lightning
//!    and the occlusion map, read by every lit shader (`weather_surface.wgsl`: wet, puddled and
//!    snow-covered surfaces, the lightning flash) and by the weather passes.
//! 2. **Occlusion map**: the shadow-casting meshes within 48 m drawn from straight above into
//!    a depth map (`occlusion.wgsl`), after the shadow maps. Rain and snow stop at the first
//!    surface from the sky, splashes land on roofs rather than under them, and floors under
//!    cover stay dry.
//! 3. **Splashes** (compute, `splash_sim.wgsl`): random pixels of the opaque scene copy's
//!    depth become crown splashes and mist puffs in a ring buffer where it rains and the
//!    surface faces up and is open to the sky.
//! 4. **Composite** after the temporal resolve, into the HDR image (`precipitation.wgsl`):
//!    haze, far sheets and storm curtains; rain streaks and snow flakes (procedural GPU
//!    particles recycled in boxes round the camera, no CPU work per particle); splashes;
//!    lightning bolts (also into the glow image so they bloom); drops on the lens.
//! 5. The sun and sky light are dimmed by the weather's dimming (Drive Clouds).

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use super::pipelines::{self, Spec};
use super::{Frame, HDR_FORMAT, RenderTarget, Renderer};
use crate::quality::Tier;
use crate::weather::WeatherFrame;
use crate::weather::frame::MAX_REGIONS;

#[cfg(test)]
mod gpu_tests;
mod scene_blur;

const PRECIP_SRC: &str = concat!(
    include_str!("shaders/weather_common.wgsl"),
    "\n",
    include_str!("shaders/precipitation.wgsl")
);
/// The composite shader. Tests can read the two files from disk instead (`MISTY_WEATHER_SHADERS`
/// = the shaders folder), to tune the look of the weather without rebuilding.
fn precip_src() -> std::borrow::Cow<'static, str> {
    #[cfg(test)]
    if let Ok(dir) = std::env::var("MISTY_WEATHER_SHADERS") {
        let read = |name: &str| {
            std::fs::read_to_string(std::path::Path::new(&dir).join(name))
                .unwrap_or_else(|e| panic!("{name}: {e}"))
        };
        return format!(
            "{}
{}",
            read("weather_common.wgsl"),
            read("precipitation.wgsl")
        )
        .into();
    }
    PRECIP_SRC.into()
}
const SPLASH_SRC: &str = concat!(
    include_str!("shaders/weather_common.wgsl"),
    "\n",
    include_str!("shaders/splash_sim.wgsl")
);
const OCCLUSION_SRC: &str = concat!(
    include_str!("../shaders/draw.wgsl"),
    "\n",
    include_str!("shaders/occlusion.wgsl")
);

/// Metres the occlusion map covers (centred on the camera).
pub const OCCLUSION_SIZE: f32 = 96.0;
/// Occlusion map top above the camera and its depth range (metres).
const OCCLUSION_TOP: f32 = 80.0;
const OCCLUSION_RANGE: f32 = 240.0;
/// Particle boxes (match precipitation.wgsl): near and far layer volumes in m³.
const NEAR_VOLUME: f32 = 32.0 * 16.0 * 32.0;
const FAR_VOLUME: f32 = 64.0 * 32.0 * 64.0;
/// Most drops / flakes drawn in one render (all layers).
pub const MAX_PARTICLES: u32 = 700_000;
/// Base exposure of a streak (s) at Streak Length 1: a film camera's shutter.
const SHUTTER: f32 = 1.0 / 55.0;
/// Depth of field of the drops: the blur disc of a drop 1 m from the lens as a share of the
/// picture height (the lens is focused far away, so only drops within a few metres blur).
const DEFOCUS: f32 = 0.028;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
struct RegionGpu {
    inverse: [[f32; 4]; 4],
    shape: [f32; 4],
    blend: [f32; 4],
    precip: [f32; 4],
    surface: [f32; 4],
}

/// `WeatherGlobals` in weather_common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub(super) struct GlobalsGpu {
    info: [f32; 4],
    precip: [f32; 4],
    surface: [f32; 4],
    snow: [f32; 4],
    snow_line: [f32; 4],
    flash: [f32; 4],
    flash_dir: [f32; 4],
    occ: [f32; 4],
    occ2: [f32; 4],
    regions: [RegionGpu; MAX_REGIONS],
}

/// `Precip` in precipitation.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
struct PrecipGpu {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    forward: [f32; 4],
    sizes: [f32; 4],
    rain: [f32; 4],
    rain2: [f32; 4],
    snow: [f32; 4],
    snow2: [f32; 4],
    wind: [f32; 4],
    drift: [f32; 4],
    haze: [f32; 4],
    misc: [f32; 4],
    bolt: [f32; 4],
    rain3: [f32; 4],
}

/// `SplashSim` in splash_sim.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
struct SplashSimGpu {
    inv_view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    sizes: [f32; 4],
    params: [f32; 4],
    ring: [f32; 4],
}

/// One splash in the ring buffer (`Splash` in the shaders).
const SPLASH_BYTES: u64 = 32;
/// One bolt segment (`BoltSegment`).
const SEGMENT_BYTES: u64 = 32;
/// Bolt segments drawn at most (all strikes).
const MAX_SEGMENTS: usize = 1024;

/// View group entries: the weather uniform (24) and the occlusion map (25).
pub(super) fn view_layout_entries() -> [wgpu::BindGroupLayoutEntry; 2] {
    let vis = wgpu::ShaderStages::VERTEX_FRAGMENT;
    [
        wgpu::BindGroupLayoutEntry {
            binding: 24,
            visibility: vis,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 25,
            visibility: vis,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        },
    ]
}

/// Particles per layer for a quality tier (× the weather's density).
pub fn tier_scale(tier: Tier) -> f32 {
    match tier {
        Tier::Off => 0.0,
        Tier::Low => 0.3,
        Tier::Medium => 0.6,
        Tier::High => 1.0,
    }
}

fn occlusion_texels(tier: Tier) -> u32 {
    match tier {
        Tier::Off | Tier::Low => 512,
        Tier::Medium => 768,
        Tier::High => 1024,
    }
}

fn splash_slots(tier: Tier) -> u32 {
    match tier {
        Tier::Off => 0,
        Tier::Low => 4096,
        Tier::Medium => 8192,
        Tier::High => 16384,
    }
}

/// Drop and flake counts: (near rain, far rain, near snow, far snow).
pub fn particle_counts(w: &WeatherFrame, tier: Tier) -> [u32; 4] {
    let s = tier_scale(tier);
    let n = |d: f32, v: f32, k: f32| (d.max(0.0) * v * k * s).round() as u32;
    let mut c = [
        n(w.max_rain, NEAR_VOLUME, 0.32),
        n(w.max_rain, FAR_VOLUME, 0.02),
        n(w.max_snow, NEAR_VOLUME, 0.45),
        n(w.max_snow, FAR_VOLUME, 0.03),
    ];
    let total: u32 = c.iter().sum();
    if total > MAX_PARTICLES {
        let k = MAX_PARTICLES as f32 / total as f32;
        for v in &mut c {
            *v = (*v as f32 * k) as u32;
        }
    }
    c
}

/// The occlusion map's view-projection: x and z across, depth = (top − y) / range.
pub fn occlusion_matrix(centre: Vec3, half: f32) -> Mat4 {
    let top = centre.y + OCCLUSION_TOP;
    Mat4::from_cols(
        glam::Vec4::new(1.0 / half, 0.0, 0.0, 0.0),
        glam::Vec4::new(0.0, 0.0, -1.0 / OCCLUSION_RANGE, 0.0),
        glam::Vec4::new(0.0, 1.0 / half, 0.0, 0.0),
        glam::Vec4::new(
            -centre.x / half,
            -centre.z / half,
            top / OCCLUSION_RANGE,
            1.0,
        ),
    )
}

/// The map's centre for a camera at `eye` with `texels` per side (snapped to whole texels:
/// the map doesn't shimmer as the camera moves).
pub fn occlusion_centre(eye: Vec3, texels: u32) -> Vec3 {
    let texel = OCCLUSION_SIZE / texels as f32;
    Vec3::new(
        (eye.x / texel).round() * texel,
        eye.y,
        (eye.z / texel).round() * texel,
    )
}

fn linear(c: [f32; 3]) -> [f32; 3] {
    super::srgb_to_linear(c)
}

struct Passes {
    layout: wgpu::BindGroupLayout,
    screen: wgpu::RenderPipeline,
    rain: wgpu::RenderPipeline,
    snow: wgpu::RenderPipeline,
    splash: wgpu::RenderPipeline,
    bolt: wgpu::RenderPipeline,
    lens: wgpu::RenderPipeline,
    sim_layout: wgpu::BindGroupLayout,
    sim: wgpu::ComputePipeline,
}

struct Occlusion {
    texels: u32,
    view: wgpu::TextureView,
    uniform: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

/// What this render draws.
struct Now {
    frame: std::sync::Arc<WeatherFrame>,
    counts: [u32; 4],
    occlusion: bool,
    centre: Vec3,
    splash_spawn: u32,
    segments: u32,
}

pub(super) struct WeatherGpu {
    globals: wgpu::Buffer,
    /// Globals are all zero (no weather) on the GPU.
    globals_off: bool,
    dummy: wgpu::TextureView,
    occlusion: Option<Occlusion>,
    passes: Option<Passes>,
    /// The blurred scene the rain is seen through (made with the passes).
    blur: Option<scene_blur::SceneBlur>,
    precip: wgpu::Buffer,
    precip_glow: wgpu::Buffer,
    sim_uniform: wgpu::Buffer,
    splashes: Option<(wgpu::Buffer, u32)>,
    ring: u32,
    bolts: wgpu::Buffer,
    now: Option<Now>,
    /// Weather clock of the last splash spawn (spawns follow the weather's own time).
    last_spawn: Option<f32>,
    frame_seed: u32,
}

fn depth_texture(device: &wgpu::Device, size: u32, label: &str) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

impl WeatherGpu {
    pub fn new(device: &wgpu::Device) -> Self {
        let buffer = |label, size: u64, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let uniform = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        Self {
            globals: buffer(
                "misty weather globals",
                std::mem::size_of::<GlobalsGpu>() as u64,
                uniform,
            ),
            globals_off: true,
            dummy: depth_texture(device, 1, "misty weather occlusion (none)"),
            occlusion: None,
            passes: None,
            blur: None,
            precip: buffer(
                "misty weather precip",
                std::mem::size_of::<PrecipGpu>() as u64,
                uniform,
            ),
            precip_glow: buffer(
                "misty weather precip (glow)",
                std::mem::size_of::<PrecipGpu>() as u64,
                uniform,
            ),
            sim_uniform: buffer(
                "misty weather splash sim",
                std::mem::size_of::<SplashSimGpu>() as u64,
                uniform,
            ),
            splashes: None,
            ring: 0,
            bolts: buffer(
                "misty weather bolts",
                SEGMENT_BYTES * MAX_SEGMENTS as u64,
                storage,
            ),
            now: None,
            last_spawn: None,
            frame_seed: 1,
        }
    }

    /// View group entries 24 (globals) and 25 (occlusion map, or a 1×1 stand-in).
    pub fn view_entries(&self) -> [wgpu::BindGroupEntry<'_>; 2] {
        let view = self.occlusion.as_ref().map_or(&self.dummy, |o| &o.view);
        [
            wgpu::BindGroupEntry {
                binding: 24,
                resource: self.globals.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 25,
                resource: wgpu::BindingResource::TextureView(view),
            },
        ]
    }

    /// Drawing weather this render?
    #[cfg(test)]
    pub fn active(&self) -> bool {
        self.now.is_some()
    }

    /// The weather passes read the opaque scene copy (colour for refraction, depth for
    /// collisions and splashes).
    pub fn needs_scene(&self) -> bool {
        self.now.as_ref().is_some_and(|n| {
            n.counts.iter().any(|&c| c > 0)
                || n.splash_spawn > 0
                || n.frame.params.visibility < 10_000.0
                || n.frame.effects.lens_drops
                || n.segments > 0
        })
    }
}

/// The lit passes' lights with the weather's dimming of the sun and sky (Drive Clouds).
pub(super) fn dim_lights(mut u: super::LightsUniform, frame: &Frame) -> super::LightsUniform {
    let Some(w) = frame.environment.weather.as_deref() else {
        return u;
    };
    if !w.effects.drive_clouds {
        return u;
    }
    let sun = 1.0 - w.params.sun_dimming.clamp(0.0, 1.0);
    let sky = 1.0 - w.params.ambient_dimming.clamp(0.0, 1.0);
    let n = (u.info[0] as usize).min(u.items.len());
    for item in &mut u.items[..n] {
        if item.dir_type[3] == 0.0 {
            for c in &mut item.color[..3] {
                *c *= sun;
            }
        }
    }
    for c in [&mut u.sky, &mut u.equator, &mut u.ground] {
        for v in &mut c[..3] {
            *v *= sky;
        }
    }
    u
}

impl Renderer {
    /// Decide what weather this render draws (before the scene copy is planned).
    pub(super) fn weather_begin(&mut self, frame: &Frame) {
        let w = &mut self.weather;
        w.now = None;
        let Some(wf) = frame.environment.weather.clone() else {
            return;
        };
        if !frame.shaded || self.wire_only || self.capture.is_some() {
            return;
        }
        let tier = self.quality.weather;
        let counts = particle_counts(&wf, tier);
        let splash_spawn = if wf.effects.splashes && tier != Tier::Off && wf.max_rain > 0.0 {
            let dt = match w.last_spawn {
                Some(t) if wf.time >= t => (wf.time - t).min(0.1),
                _ => 0.0,
            };
            let rate = wf.params.splashes.max(wf.global.splashes)
                * (wf.max_rain / 30.0).min(1.5)
                * 60_000.0
                * tier_scale(tier);
            let slots = splash_slots(tier) as f32;
            (rate * dt).min(slots * 0.5) as u32
        } else {
            0
        };
        w.last_spawn = Some(wf.time);
        let segments = if wf.effects.bolts {
            wf.bolts
                .iter()
                .map(|(b, _)| b.segments.len())
                .sum::<usize>()
                .min(MAX_SEGMENTS) as u32
        } else {
            0
        };
        let texels = occlusion_texels(tier);
        w.now = Some(Now {
            centre: occlusion_centre(frame.camera.position, texels),
            occlusion: wf.effects.occlusion,
            frame: wf,
            counts,
            splash_spawn,
            segments,
        });
    }

    /// Globals, the occlusion map and the per-render buffers (before the clouds rebuild the
    /// view group: a new occlusion map marks it dirty).
    pub(super) fn prepare_weather(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let tier = self.quality.weather;
        let Some(now) = self.weather.now.as_ref() else {
            if !self.weather.globals_off {
                queue.write_buffer(
                    &self.weather.globals,
                    0,
                    bytemuck::bytes_of(&GlobalsGpu::default()),
                );
                self.weather.globals_off = true;
            }
            return;
        };
        let texels = occlusion_texels(tier);
        if now.occlusion
            && self
                .weather
                .occlusion
                .as_ref()
                .is_none_or(|o| o.texels != texels)
        {
            self.weather.occlusion = Some(make_occlusion(device, texels));
            self.view_group_dirty = true;
        }
        let w = &now.frame;
        let mut g = GlobalsGpu {
            info: [
                1.0,
                w.time,
                w.regions.len().min(MAX_REGIONS) as f32,
                if now.occlusion { 1.0 } else { 0.0 },
            ],
            precip: [
                w.global.rain,
                w.global.snow,
                w.global.splashes,
                w.params.rain_intensity,
            ],
            surface: [
                w.global.wetness,
                w.global.puddles,
                w.global.snow_cover,
                if w.surfaces.ripples { 1.0 } else { 0.0 },
            ],
            snow: {
                let c = linear(w.surfaces.snow_color);
                [c[0], c[1], c[2], w.surfaces.puddle_scale.max(0.1)]
            },
            snow_line: [
                if w.surfaces.snow_line { 1.0 } else { 0.0 },
                w.surfaces.snow_line_height,
                w.surfaces.puddles.clamp(0.0, 1.0),
                if w.surfaces.wetness { 1.0 } else { 0.0 },
            ],
            flash: {
                let b = w.flash * 4.0;
                [0.78 * b, 0.84 * b, 1.0 * b, w.flash]
            },
            flash_dir: w.flash_dir.extend(0.0).to_array(),
            occ: [
                now.centre.x - OCCLUSION_SIZE * 0.5,
                now.centre.y + OCCLUSION_TOP,
                now.centre.z - OCCLUSION_SIZE * 0.5,
                OCCLUSION_SIZE,
            ],
            occ2: [
                OCCLUSION_RANGE,
                texels as f32,
                if w.effects.drive_clouds {
                    w.params.sun_dimming
                } else {
                    0.0
                },
                if w.effects.drive_clouds {
                    w.params.ambient_dimming
                } else {
                    0.0
                },
            ],
            regions: Default::default(),
        };
        for (i, r) in w.regions.iter().take(MAX_REGIONS).enumerate() {
            g.regions[i] = RegionGpu {
                inverse: r.inverse.to_cols_array_2d(),
                shape: [
                    r.half.x,
                    r.half.y,
                    r.half.z,
                    if r.sphere { 1.0 } else { 0.0 },
                ],
                blend: [r.blend, r.weight, 0.0, 0.0],
                precip: [r.amounts.rain, r.amounts.snow, r.amounts.splashes, 0.0],
                surface: [
                    r.amounts.wetness,
                    r.amounts.puddles,
                    r.amounts.snow_cover,
                    0.0,
                ],
            };
        }
        queue.write_buffer(&self.weather.globals, 0, bytemuck::bytes_of(&g));
        self.weather.globals_off = false;
        if let Some(o) = &self.weather.occlusion {
            let m = occlusion_matrix(now.centre, OCCLUSION_SIZE * 0.5);
            queue.write_buffer(&o.uniform, 0, bytemuck::bytes_of(&m.to_cols_array_2d()));
        }
        // Bolt segments.
        if now.segments > 0 {
            let mut data: Vec<[f32; 8]> = Vec::with_capacity(now.segments as usize);
            'bolts: for (bolt, brightness) in &w.bolts {
                for s in &bolt.segments {
                    if data.len() >= MAX_SEGMENTS {
                        break 'bolts;
                    }
                    let b = s.brightness * brightness;
                    data.push([s.a.x, s.a.y, s.a.z, s.width, s.b.x, s.b.y, s.b.z, b]);
                }
            }
            queue.write_buffer(&self.weather.bolts, 0, bytemuck::cast_slice(&data));
        }
        let slots = splash_slots(tier);
        if slots > 0 && self.weather.splashes.as_ref().is_none_or(|s| s.1 != slots) {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty weather splashes"),
                size: SPLASH_BYTES * u64::from(slots),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            // Zeroed: birth time 0 = no splash.
            self.weather.splashes = Some((buffer, slots));
            self.weather.ring = 0;
        }
        if self.weather.passes.is_none()
            && (now.counts.iter().any(|&c| c > 0)
                || now.splash_spawn > 0
                || now.segments > 0
                || w.params.visibility < 10_000.0
                || w.effects.lens_drops)
        {
            self.weather.passes = Some(make_passes(device, &self.pipelines.view_layout));
            self.weather.blur = Some(scene_blur::SceneBlur::new(
                device,
                &self.pipelines.view_layout,
            ));
        }
    }

    /// The occlusion map: shadow casters near the camera drawn from above (after the shadow
    /// maps, before anything shades with it).
    pub(super) fn weather_occlusion_pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        prep: &super::draw_prep::DrawPrep,
    ) {
        let (Some(now), Some(o)) = (self.weather.now.as_ref(), self.weather.occlusion.as_ref())
        else {
            return;
        };
        if !now.occlusion {
            return;
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty weather occlusion"),
            layout: &o.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: o.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.draws_buffer.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty weather occlusion"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &o.view,
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
        pass.set_pipeline(&o.pipeline);
        pass.set_bind_group(0, &group, &[]);
        let half = OCCLUSION_SIZE * 0.5;
        let (lo, hi) = (
            now.centre - Vec3::new(half, 1.0e6, half),
            now.centre + Vec3::new(half, OCCLUSION_TOP, half),
        );
        for (i, d) in frame.meshes.iter().enumerate() {
            let (Some(Some(b)), Some(Some(_))) = (prep.bounds.get(i), prep.shadow.get(i)) else {
                continue;
            };
            if b.max.x < lo.x
                || b.min.x > hi.x
                || b.max.z < lo.z
                || b.min.z > hi.z
                || b.min.y > hi.y
            {
                continue;
            }
            if b.max.y < lo.y {
                continue;
            }
            if let Some(m) = self.gpu_mesh(&d.mesh) {
                m.draw(&mut pass, i as u32..i as u32 + 1);
            }
        }
    }

    /// Splashes, then the composite into the HDR image (after the temporal resolve) and the
    /// bolts' glow for bloom.
    pub(super) fn weather_passes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
        frame: &Frame,
    ) {
        let Some(now) = self.weather.now.as_ref() else {
            return;
        };
        let Some(passes) = self.weather.passes.as_ref() else {
            return;
        };
        let Some(copy) = self.scene_copies.current(target.size()) else {
            return;
        };
        let Some(blur) = self.weather.blur.as_mut() else {
            return;
        };
        // Rain, its far sheets, splashes and lens drops show the scene through water: the
        // blurred chain.
        let wet = now.frame.max_rain > 0.0 || now.frame.effects.lens_drops;
        if wet {
            blur.record(
                device,
                encoder,
                &self.view_bind_group,
                &copy.color_view,
                target.size(),
            );
        }
        let blur = self.weather.blur.as_ref().unwrap();
        let blur_view = blur
            .current(target.size())
            .filter(|_| wet)
            .map_or(&copy.color_view, |c| &c.view);
        let w = &now.frame;
        let cam = frame.camera;
        let out = target.output_size();
        let internal = target.size();
        let aspect = out.0 as f32 / out.1.max(1) as f32;
        let view_proj = cam.view_projection(aspect);
        let ortho = matches!(
            cam.projection,
            crate::camera::Projection::Orthographic { .. }
        );
        let tan_half = match cam.projection {
            crate::camera::Projection::Perspective { fov_y } => (fov_y.to_radians() * 0.5).tan(),
            crate::camera::Projection::Orthographic { size } => size,
        };
        let p = &w.params;
        let ext = if w.effects.haze {
            (3.0 / p.visibility.max(10.0) - 3.0 / 10_000.0).max(0.0)
        } else {
            0.0
        };
        let splash_slots = self.weather.splashes.as_ref().map_or(0, |s| s.1);
        let mut u = PrecipGpu {
            view_proj: view_proj.to_cols_array_2d(),
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            camera: cam.position.extend(w.time).to_array(),
            right: cam.right().extend(tan_half).to_array(),
            up: cam.up().extend(if ortho { 1.0 } else { 0.0 }).to_array(),
            forward: cam.forward().extend(cam.near).to_array(),
            sizes: [
                out.0 as f32,
                out.1 as f32,
                internal.0 as f32,
                internal.1 as f32,
            ],
            rain: [
                now.counts[0] as f32,
                now.counts[1] as f32,
                w.max_rain.max(1e-3),
                p.drop_size * 0.001,
            ],
            rain2: [
                p.rain_speed,
                SHUTTER * p.streak_length,
                p.rain_opacity,
                p.refraction,
            ],
            snow: [
                now.counts[2] as f32,
                now.counts[3] as f32,
                w.max_snow.max(1e-3),
                p.flake_size * 0.01,
            ],
            snow2: [
                p.snow_speed,
                p.snow_turbulence,
                p.snow_opacity,
                // Flakes are photographed faster than rain is (they'd all be dashes otherwise).
                SHUTTER * 0.53 * p.streak_length,
            ],
            wind: w.wind.extend(w.rain_fall).to_array(),
            drift: w.wind_offset.extend(w.snow_fall).to_array(),
            haze: [
                ext,
                if w.effects.mist { p.mist } else { 0.0 },
                if w.effects.far_sheets { 1.0 } else { 0.0 },
                splash_slots as f32,
            ],
            misc: [
                cam.far,
                1.0,
                if w.effects.lens_drops {
                    (p.rain_intensity * 1.2).min(1.0)
                } else {
                    0.0
                },
                p.rain_intensity,
            ],
            bolt: [
                now.segments as f32,
                0.0,
                if w.effects.splashes { 1.0 } else { 0.0 },
                if w.effects.mist { 1.0 } else { 0.0 },
            ],
            rain3: [
                p.wind_turbulence,
                DEFOCUS,
                (scene_blur::LEVELS - 1) as f32,
                if w.effects.haze {
                    p.rain_intensity
                } else {
                    0.0
                },
            ],
        };
        queue.write_buffer(&self.weather.precip, 0, bytemuck::bytes_of(&u));
        u.sizes[0] = internal.0 as f32;
        u.sizes[1] = internal.1 as f32;
        u.bolt[1] = 1.0;
        queue.write_buffer(&self.weather.precip_glow, 0, bytemuck::bytes_of(&u));

        // Splash spawning.
        let spawn = now.splash_spawn;
        if let (true, Some((splashes, slots))) = (spawn > 0, self.weather.splashes.as_ref()) {
            self.weather.frame_seed = self
                .weather
                .frame_seed
                .wrapping_mul(1_103_515_245)
                .wrapping_add(12345);
            let sim = SplashSimGpu {
                inv_view_proj: view_proj.inverse().to_cols_array_2d(),
                camera: cam.position.extend(w.time).to_array(),
                sizes: [
                    internal.0 as f32,
                    internal.1 as f32,
                    *slots as f32,
                    spawn as f32,
                ],
                params: [
                    w.max_rain.max(1e-3),
                    45.0,
                    (self.weather.frame_seed >> 8) as f32,
                    if w.effects.mist { p.mist * 0.08 } else { 0.0 },
                ],
                ring: [
                    self.weather.ring as f32,
                    0.01 + p.drop_size * 0.004,
                    0.0,
                    0.0,
                ],
            };
            queue.write_buffer(&self.weather.sim_uniform, 0, bytemuck::bytes_of(&sim));
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty weather splash sim"),
                layout: &passes.sim_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.weather.sim_uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&copy.depth_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: splashes.as_entire_binding(),
                    },
                ],
            });
            let mut cp = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("misty weather splashes"),
                timestamp_writes: None,
            });
            cp.set_pipeline(&passes.sim);
            cp.set_bind_group(0, &self.view_bind_group, &[]);
            cp.set_bind_group(1, &group, &[]);
            cp.dispatch_workgroups(spawn.div_ceil(64), 1, 1);
            drop(cp);
            self.weather.ring = (self.weather.ring + spawn) % slots;
        }

        let splash_buffer = self
            .weather
            .splashes
            .as_ref()
            .map_or(&self.weather.bolts, |s| &s.0);
        let group = |uniform: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("misty weather composite"),
                layout: &passes.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&copy.color_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&copy.depth_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&self.post_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: self.lights_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: splash_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: self.weather.bolts.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: wgpu::BindingResource::TextureView(blur_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: wgpu::BindingResource::Sampler(&blur.sampler),
                    },
                ],
            })
        };
        let main_group = group(&self.weather.precip);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("misty weather"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.post_hdr_view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.view_bind_group, &[]);
            pass.set_bind_group(1, &main_group, &[]);
            let screen = ext > 0.0
                || (w.effects.far_sheets && (w.max_rain > 0.0 || w.max_snow > 0.0))
                || !w.regions.is_empty()
                || (w.effects.mist && p.mist > 0.0)
                || w.flash > 0.0;
            if screen {
                pass.set_pipeline(&passes.screen);
                pass.draw(0..3, 0..1);
            }
            let rain = now.counts[0] + now.counts[1];
            if rain > 0 {
                pass.set_pipeline(&passes.rain);
                pass.draw(0..rain * 6, 0..1);
            }
            let snow = now.counts[2] + now.counts[3];
            if snow > 0 {
                pass.set_pipeline(&passes.snow);
                pass.draw(0..snow * 6, 0..1);
            }
            if splash_slots > 0 && w.max_rain > 0.0 && (w.effects.splashes || w.effects.mist) {
                pass.set_pipeline(&passes.splash);
                pass.draw(0..6 * 7, 0..splash_slots);
            }
            if now.segments > 0 {
                pass.set_pipeline(&passes.bolt);
                pass.draw(0..now.segments * 6, 0..1);
            }
            if w.effects.lens_drops && p.rain_intensity > 0.0 {
                pass.set_pipeline(&passes.lens);
                pass.draw(0..3, 0..1);
            }
        }
        if now.segments > 0 {
            let glow_group = group(&self.weather.precip_glow);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("misty weather bolt glow"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.glow_view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.view_bind_group, &[]);
            pass.set_bind_group(1, &glow_group, &[]);
            pass.set_pipeline(&passes.bolt);
            pass.draw(0..now.segments * 6, 0..1);
        }
    }
}

fn make_occlusion(device: &wgpu::Device, texels: u32) -> Occlusion {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("misty weather occlusion"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("misty weather occlusion"),
        source: wgpu::ShaderSource::Wgsl(OCCLUSION_SRC.into()),
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("misty weather occlusion"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let attributes = [wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 0,
        shader_location: 0,
    }];
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("misty weather occlusion"),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<super::mesh::Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: None,
        multiview_mask: None,
        cache: None,
    });
    Occlusion {
        texels,
        view: depth_texture(device, texels, "misty weather occlusion"),
        uniform: device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty weather occlusion view"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        layout,
        pipeline,
    }
}

fn make_passes(device: &wgpu::Device, view_layout: &wgpu::BindGroupLayout) -> Passes {
    use wgpu::ShaderStages as S;
    let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty,
        count: None,
    };
    let uniform = wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: None,
    };
    let read = wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only: true },
        has_dynamic_offset: false,
        min_binding_size: None,
    };
    let tex = |filterable| wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable },
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled: false,
    };
    let vf = S::VERTEX_FRAGMENT;
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("misty weather composite"),
        entries: &[
            entry(0, vf, uniform),
            entry(1, S::FRAGMENT, tex(true)),
            entry(2, vf, tex(false)),
            entry(
                3,
                S::FRAGMENT,
                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            ),
            entry(4, vf, uniform),
            entry(5, S::VERTEX, read),
            entry(6, S::VERTEX, read),
            entry(7, S::FRAGMENT, tex(true)),
            entry(
                8,
                S::FRAGMENT,
                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            ),
        ],
    });
    let premultiplied = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    };
    let additive = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: premultiplied.alpha,
    };
    let layouts = [view_layout, &layout];
    let source = precip_src();
    let build = |name: &'static str, vs: &'static str, fs: &'static str, blend| {
        pipelines::build(
            device,
            Spec {
                vs,
                fs: Some(fs),
                depth: None,
                color: Some(HDR_FORMAT),
                blend: Some(blend),
                samples: 1,
                glow: None,
                ..Spec::new(name, &source, &layouts)
            },
        )
    };
    let sim_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("misty weather splash sim"),
        entries: &[
            entry(0, S::COMPUTE, uniform),
            entry(1, S::COMPUTE, tex(false)),
            entry(
                2,
                S::COMPUTE,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            ),
        ],
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("misty weather splash sim"),
        source: wgpu::ShaderSource::Wgsl(SPLASH_SRC.into()),
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("misty weather splash sim"),
        bind_group_layouts: &[Some(view_layout), Some(&sim_layout)],
        immediate_size: 0,
    });
    let sim = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("misty weather splash sim"),
        layout: Some(&pl),
        module: &module,
        entry_point: Some("cs_spawn"),
        compilation_options: Default::default(),
        cache: None,
    });
    Passes {
        screen: build(
            "weather screen",
            "vs_fullscreen",
            "fs_screen",
            premultiplied,
        ),
        rain: build("weather rain", "vs_rain", "fs_rain", premultiplied),
        snow: build("weather snow", "vs_snow", "fs_snow", premultiplied),
        splash: build("weather splashes", "vs_splash", "fs_splash", premultiplied),
        bolt: build("weather bolts", "vs_bolt", "fs_bolt", additive),
        lens: build("weather lens", "vs_fullscreen", "fs_lens", premultiplied),
        layout,
        sim_layout,
        sim,
    }
}

#[cfg(test)]
mod tests {
    use wgpu::naga;

    use super::*;

    fn validate(name: &str, src: &str) {
        let module = naga::front::wgsl::parse_str(src)
            .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
    }

    #[test]
    fn weather_shaders_validate() {
        use crate::render::pipelines::{COMMON, GRASS_SRC, MESH_SRC, TERRAIN_SRC};
        validate("weather composite", &format!("{COMMON}\n{PRECIP_SRC}"));
        validate("weather splashes", SPLASH_SRC);
        validate("weather occlusion", OCCLUSION_SRC);
        validate(
            "weather scene blur",
            &format!(
                "{COMMON}
{}",
                include_str!("shaders/scene_blur.wgsl")
            ),
        );
        // The lit shaders with the surface hook.
        validate("mesh", &format!("{COMMON}\n{MESH_SRC}"));
        validate("terrain", &format!("{COMMON}\n{TERRAIN_SRC}"));
        validate("grass", &format!("{COMMON}\n{GRASS_SRC}"));
    }

    #[test]
    fn uniforms_match_wgsl() {
        // WeatherGlobals: 9 vec4s + 4 regions of a matrix and 4 vec4s.
        assert_eq!(
            std::mem::size_of::<GlobalsGpu>(),
            9 * 16 + 4 * (64 + 4 * 16)
        );
        // Precip: 2 matrices + 15 vec4s.
        assert_eq!(std::mem::size_of::<PrecipGpu>(), 2 * 64 + 15 * 16);
        assert_eq!(std::mem::size_of::<SplashSimGpu>(), 64 + 4 * 16);
    }

    #[test]
    fn occlusion_matrix_maps_the_area_and_heights() {
        let centre = Vec3::new(10.0, 2.0, -4.0);
        let m = occlusion_matrix(centre, 48.0);
        let p = m.project_point3(Vec3::new(10.0 + 48.0, 2.0 + OCCLUSION_TOP, -4.0 - 48.0));
        assert!((p - Vec3::new(1.0, -1.0, 0.0)).length() < 1e-5, "{p}");
        let q = m.project_point3(Vec3::new(10.0, 2.0 + OCCLUSION_TOP - OCCLUSION_RANGE, -4.0));
        assert!((q - Vec3::new(0.0, 0.0, 1.0)).length() < 1e-5, "{q}");
        // Snapped to whole texels.
        let c = occlusion_centre(Vec3::new(0.04, 1.0, 0.13), 1024);
        let texel = OCCLUSION_SIZE / 1024.0;
        assert!(
            (c.x / texel).fract().abs() < 1e-4
                && (c.z / texel - (c.z / texel).round()).abs() < 1e-4
        );
    }

    #[test]
    fn particle_counts_follow_density_and_quality() {
        let mut world = crate::weather::WeatherWorld::default();
        let mut w = crate::weather::Weather::with_preset(crate::weather::WeatherPreset::HeavyRain);
        w.timeline.mode = crate::weather::TimelineMode::Off;
        let mut ecs = hecs::World::new();
        let e = ecs.spawn(());
        world.update(&[(e, w, Mat4::IDENTITY)], 0.1, Vec3::ZERO);
        let f = crate::weather::frame::build(&world).unwrap();
        let high = particle_counts(&f, Tier::High);
        let low = particle_counts(&f, Tier::Low);
        assert!(
            high[0] > 100_000 && high[0] + high[1] < MAX_PARTICLES + 1,
            "{high:?}"
        );
        assert!(low[0] < high[0] / 2);
        assert_eq!(particle_counts(&f, Tier::Off), [0; 4]);
        assert_eq!(high[2], 0);
    }
}
