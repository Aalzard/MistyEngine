//! Water rendering: the FFT ocean simulation, per-surface uniforms, and the surface and
//! underwater pipelines. Water is drawn right after the opaque scene is copied (see
//! `scene_copy`), so it can refract and foam against what is below it.
//!
//! All surfaces share the first surface's FFT simulation (scaled to their own wave height);
//! their Gerstner waves, colours and foam are their own.

mod fft;
pub mod mesh;

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec3};
use wgpu::util::DeviceExt;

use super::pipelines::{self, Depth, Pipelines, Spec};
use super::scene_copy::SceneCopy;
use super::{Frame, RenderStats, srgb_to_linear};
use crate::water::waves::{SpectrumParams, developed_height, gerstner_waves};
use crate::water::{WaterDraw, WaterKind};

/// Surfaces drawn per view.
pub const MAX_SURFACES: usize = 8;
const STRIDE: u64 = 512;
/// The ocean's waves flatten out by this distance (m).
const OCEAN_FADE: f32 = 3500.0;

/// The water shader after mesh.wgsl and its includes (same lighting as `MESH_SRC`).
const WATER_SRC: &str = concat!(
    include_str!("../shaders/draw.wgsl"),
    "\n",
    include_str!("../shaders/mesh.wgsl"),
    "\n",
    include_str!("../shaders/shadows.wgsl"),
    "\n",
    include_str!("../probes/probes.wgsl"),
    "\n",
    include_str!("../shaders/screen_space.wgsl"),
    "\n",
    include_str!("../shaders/water.wgsl")
);

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct WaterUniform {
    model: [[f32; 4]; 4],
    p: [[f32; 4]; 7],
    colors: [[f32; 4]; 5],
    wave_a: [[f32; 4]; 8],
    wave_b: [[f32; 4]; 8],
}

struct GpuGrid {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    vertex_count: u32,
}

impl GpuGrid {
    fn new(device: &wgpu::Device, label: &str, g: &mesh::GridData) -> Self {
        Self {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&g.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&g.indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            count: g.indices.len() as u32,
            vertex_count: g.vertices.len() as u32,
        }
    }
}

/// GPU resources, created when a frame first has water.
struct Inner {
    layout: wgpu::BindGroupLayout,
    surface: wgpu::RenderPipeline,
    underwater: wgpu::RenderPipeline,
    ocean: GpuGrid,
    bounded: GpuGrid,
    uniforms: wgpu::Buffer,
    sampler: wgpu::Sampler,
    clamp: wgpu::Sampler,
    fft: fft::Fft,
    /// Bind group per scene copy id.
    groups: HashMap<u64, wgpu::BindGroup>,
}

/// What this render draws: (uniform offset, ocean?) per surface, the underwater pass's offset.
#[derive(Default)]
struct Planned {
    group: Option<u64>,
    surfaces: Vec<(u32, bool)>,
    underwater: Option<u32>,
}

#[derive(Default)]
pub struct WaterRenderer {
    inner: Option<Inner>,
    planned: Planned,
}

fn layout_entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty,
        count: None,
    }
}

fn texture(filterable: bool) -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable },
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled: false,
    }
}

impl Inner {
    fn new(device: &wgpu::Device, p: &Pipelines) -> Self {
        let filtering = wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty water"),
            entries: &[
                layout_entry(
                    10,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(STRIDE),
                    },
                ),
                layout_entry(11, texture(true)),
                layout_entry(12, texture(true)),
                layout_entry(13, filtering),
                layout_entry(14, texture(true)),
                layout_entry(15, texture(false)),
                layout_entry(16, filtering),
            ],
        });
        let layouts = [&p.view_layout, &p.mesh_layout, &layout];
        let buffers = [wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x4],
        }];
        let surface = pipelines::build(
            device,
            Spec {
                vs: "vs_water",
                fs: Some("fs_water"),
                buffers: &buffers,
                // Both sides: the underside is seen from below the surface.
                cull: None,
                depth: Depth::main(true, wgpu::CompareFunction::Less),
                glow: Some(true),
                ..Spec::new("water", WATER_SRC, &layouts)
            },
        );
        let underwater = pipelines::build(
            device,
            Spec {
                fs: Some("fs_underwater"),
                depth: Depth::main(false, wgpu::CompareFunction::Always),
                glow: Some(false),
                ..Spec::new("underwater", WATER_SRC, &layouts)
            },
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty water sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let clamp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("misty water clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            layout,
            surface,
            underwater,
            ocean: GpuGrid::new(device, "misty ocean grid", &mesh::ocean()),
            bounded: GpuGrid::new(device, "misty water grid", &mesh::bounded()),
            uniforms: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("misty water uniforms"),
                size: STRIDE * MAX_SURFACES as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            sampler,
            clamp,
            fft: fft::Fft::new(device),
            groups: HashMap::new(),
        }
    }

    fn group(&mut self, device: &wgpu::Device, copy: &SceneCopy) {
        if self.groups.len() > 8 && !self.groups.contains_key(&copy.id) {
            self.groups.clear();
        }
        if self.groups.contains_key(&copy.id) {
            return;
        }
        let view = |binding, v| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(v),
        };
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty water"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.uniforms,
                        offset: 0,
                        size: wgpu::BufferSize::new(STRIDE),
                    }),
                },
                view(11, &self.fft.displacement),
                view(12, &self.fft.slope),
                wgpu::BindGroupEntry {
                    binding: 13,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                view(14, &copy.color_view),
                view(15, &copy.depth_view),
                wgpu::BindGroupEntry {
                    binding: 16,
                    resource: wgpu::BindingResource::Sampler(&self.clamp),
                },
            ],
        });
        self.groups.insert(copy.id, group);
    }
}

fn linear(c: [f32; 3]) -> [f32; 4] {
    let l = srgb_to_linear(c);
    [l[0], l[1], l[2], 1.0]
}

/// The ocean grid follows the camera in 4 m steps (finer rings stay aligned to their cells).
fn ocean_origin(camera: Vec3) -> Vec2 {
    let snap = 4.0;
    Vec2::new(
        (camera.x / snap).round() * snap,
        (camera.z / snap).round() * snap,
    )
}

/// Is the camera below this surface (and over it)?
pub fn camera_under(d: &WaterDraw, camera: Vec3) -> bool {
    let p = Vec2::new(camera.x, camera.z);
    d.settings.underwater && d.contains(p) && camera.y < d.height(p)
}

fn uniform(
    d: &WaterDraw,
    primary: &WaterDraw,
    tile: f32,
    camera: Vec3,
    under: bool,
) -> WaterUniform {
    let s = &d.settings;
    let kind = match d.kind {
        WaterKind::Ocean => 0.0,
        WaterKind::Lake => 1.0,
        WaterKind::River => 2.0,
    };
    let (model, cell, fade) = match d.kind {
        WaterKind::Ocean => {
            let o = ocean_origin(camera);
            (
                Mat4::from_translation(Vec3::new(o.x, d.level(), o.y)),
                0.0,
                OCEAN_FADE,
            )
        }
        _ => {
            let scale = d.model.to_scale_rotation_translation().0.abs();
            let extent = (d.size[0] * scale.x).max(d.size[1] * scale.z);
            (
                d.model * Mat4::from_scale(Vec3::new(d.size[0], 1.0, d.size[1])),
                extent / mesh::BOUNDED_CELLS as f32,
                1.0e6,
            )
        }
    };
    let own = developed_height(s.wind_speed) * s.wave_height;
    let shared = developed_height(primary.settings.wind_speed) * primary.settings.wave_height;
    let height_scale = if shared > 1e-4 {
        (own / shared).clamp(0.0, 4.0)
    } else {
        0.0
    };
    let flow = d.flow();
    let flow_dir = (d.kind == WaterKind::River).then(|| flow.normalize_or(Vec2::Y));
    let waves = gerstner_waves(s, flow_dir);
    let flag = |b: bool| if b { 1.0 } else { 0.0 };
    WaterUniform {
        model: model.to_cols_array_2d(),
        p: [
            [kind, d.time, tile, height_scale],
            [
                s.ripples,
                s.smoothness,
                s.reflections,
                flag(s.screen_space_reflections),
            ],
            [s.refraction, s.clarity, s.scattering, flag(under)],
            [s.crest_foam, s.shore_foam, s.shore_foam_width, s.caustics],
            [
                s.caustics_scale,
                s.caustics_depth,
                s.underwater_visibility,
                s.underwater_distortion,
            ],
            [flow.x, flow.y, cell, fade],
            [1.0 / (0.5 * own).max(0.1), 0.0, 0.0, 0.0],
        ],
        colors: [
            linear(s.shallow_color),
            linear(s.deep_color),
            linear(s.scattering_color),
            linear(s.foam_color),
            linear(s.underwater_color),
        ],
        wave_a: waves.map(|w| [w.dir.x, w.dir.y, w.k, w.amplitude]),
        wave_b: waves.map(|w| [w.omega, w.phase, w.steepness, 0.0]),
    }
}

impl WaterRenderer {
    /// Simulate the waves for this frame's time and write each surface's uniform.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        p: &Pipelines,
        frame: &Frame,
        _size: (u32, u32),
        copy: Option<&SceneCopy>,
    ) {
        crate::profile_scope!("Water.Prepare");
        self.planned = Planned::default();
        let (Some(primary), Some(copy)) = (frame.water.first(), copy) else {
            return;
        };
        let inner = self.inner.get_or_insert_with(|| Inner::new(device, p));
        let s = &primary.settings;
        inner.fft.prepare(
            queue,
            SpectrumParams::of(s),
            fft::FftStep {
                time: primary.time,
                choppiness: s.choppiness,
                delta: 0.0,
                crest_foam: s.crest_foam,
            },
        );
        inner.group(device, copy);
        let camera = frame.camera.position;
        let tile = inner.fft.tile();
        let mut data = Vec::with_capacity(frame.water.len().min(MAX_SURFACES) * STRIDE as usize);
        let mut planned = Planned {
            group: Some(copy.id),
            ..Default::default()
        };
        for (i, d) in frame.water.iter().take(MAX_SURFACES).enumerate() {
            let under = camera_under(d, camera);
            let offset = (i as u64 * STRIDE) as u32;
            if under && planned.underwater.is_none() {
                planned.underwater = Some(offset);
            }
            let u = uniform(d, primary, tile, camera, under);
            data.extend_from_slice(bytemuck::bytes_of(&u));
            data.resize((i + 1) * STRIDE as usize, 0);
            planned.surfaces.push((offset, d.kind == WaterKind::Ocean));
        }
        queue.write_buffer(&inner.uniforms, 0, &data);
        self.planned = planned;
    }

    /// Record the wave simulation's compute passes (once per simulated time).
    pub fn simulate(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if let Some(inner) = self.inner.as_mut()
            && self.planned.group.is_some()
        {
            inner.fft.record(encoder);
        }
    }

    /// Draw the underwater view (camera below a surface) and the surfaces; group 1 (lights)
    /// must be bound.
    pub fn draw(&self, pass: &mut wgpu::RenderPass, frame: &Frame, stats: &mut RenderStats) {
        let (Some(inner), Some(id)) = (self.inner.as_ref(), self.planned.group) else {
            return;
        };
        let Some(group) = inner.groups.get(&id) else {
            return;
        };
        let _ = frame;
        crate::profile_scope!("Water.Draw");
        if let Some(offset) = self.planned.underwater {
            pass.set_pipeline(&inner.underwater);
            pass.set_bind_group(2, group, &[offset]);
            pass.draw(0..3, 0..1);
            stats.batches += 1;
        }
        pass.set_pipeline(&inner.surface);
        stats.set_pass += 1;
        for &(offset, ocean) in &self.planned.surfaces {
            let grid = if ocean { &inner.ocean } else { &inner.bounded };
            pass.set_bind_group(2, group, &[offset]);
            pass.set_vertex_buffer(0, grid.vertices.slice(..));
            pass.set_index_buffer(grid.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..grid.count, 0, 0..1);
            stats.draw(1, grid.count, grid.vertex_count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_shaders_validate() {
        use wgpu::naga;
        let common = super::super::pipelines::COMMON;
        for (name, src) in [
            ("water", format!("{common}\n{WATER_SRC}")),
            ("fft", include_str!("../shaders/water_fft.wgsl").to_owned()),
            (
                "mips",
                include_str!("../shaders/water_mips.wgsl").to_owned(),
            ),
        ] {
            let module = naga::front::wgsl::parse_str(&src)
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&src)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&src)));
        }
        assert_eq!(std::mem::size_of::<WaterUniform>() as u64, STRIDE);
    }
}
