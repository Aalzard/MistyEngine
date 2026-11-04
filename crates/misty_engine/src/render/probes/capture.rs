//! Probe capture: render the scene from the probe position with this renderer (one 90° view
//! per cube face), read back the linear HDR image (before bloom and tonemapping) and turn the
//! cube into [`ProbeData`]. `bake_scene` bakes every probe of a scene, optionally a second time
//! so probes see each other's reflections (a second bounce).

use std::sync::Arc;

use glam::Vec3;
use hecs::Entity;

use super::cube::{self, Cube};
use super::{ProbeData, ProbeInstance, ProbeSet};
use crate::camera::{Camera, Projection};
use crate::render::{
    Environment, Frame, LightData, MeshDraw, PostSettings, RenderTarget, Renderer,
};
use crate::render_scene::{self, MaterialSource};
use crate::scene::reflection_probe::ProbeClearFlags;
use crate::scene::{ReflectionProbe, Scene};
use crate::terrain::TerrainDraw;

/// GGX samples per texel for baked probes.
pub const BAKE_SAMPLES: u32 = 64;
/// Fewer for realtime probes (re-captured while running).
pub const REALTIME_SAMPLES: u32 = 24;

/// What a probe sees: the scene's renderers, lights and environment.
pub struct CaptureInputs {
    pub meshes: Vec<MeshDraw>,
    pub lights: Vec<LightData>,
    pub terrains: Vec<TerrainDraw>,
    pub environment: Environment,
}

impl CaptureInputs {
    /// Every active renderer and terrain, lit by the scene's lights and sky (no probes).
    pub fn from_scene(scene: &Scene, materials: &dyn MaterialSource) -> Self {
        let lights = render_scene::lights(scene);
        Self {
            meshes: render_scene::mesh_draws(scene, materials, |_| false),
            terrains: crate::terrain::draws(scene),
            environment: Environment::new(scene.render_settings.clone(), &lights),
            lights,
        }
    }
}

/// Where and how one probe captures.
#[derive(Clone, Copy, Debug)]
pub struct CaptureSettings {
    pub position: Vec3,
    pub size: u32,
    pub near: f32,
    pub far: f32,
    pub clear: ProbeClearFlags,
    /// sRGB, for Solid Color.
    pub background: [f32; 3],
}

impl CaptureSettings {
    pub fn of(p: &ReflectionProbe, position: Vec3) -> Self {
        let near = p.near_clip.max(0.01);
        Self {
            position,
            size: p.face_size(),
            near,
            far: p.far_clip.max(near + 0.1),
            clear: p.clear_flags,
            background: p.background,
        }
    }
}

/// Offscreen target and readback buffer of one face size.
pub struct ProbeCapture {
    target: RenderTarget,
    buffer: wgpu::Buffer,
    size: u32,
}

impl ProbeCapture {
    pub fn new(renderer: &Renderer, device: &wgpu::Device, size: u32) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("misty probe readback"),
            size: (Self::row(size) * size) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            target: renderer.create_target(device, size, size),
            buffer,
            size,
        }
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    /// Bytes per padded RGBA16F row.
    fn row(size: u32) -> u32 {
        (size * 8).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
    }

    /// Render and read back one face (rows top to bottom, linear RGB).
    pub fn face(
        &self,
        renderer: &mut Renderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        inputs: &CaptureInputs,
        s: &CaptureSettings,
        face: usize,
    ) -> Vec<Vec3> {
        let camera = Camera {
            position: s.position,
            rotation: cube::face_rotation(face),
            projection: Projection::Perspective { fov_y: 90.0 },
            near: s.near,
            far: s.far,
        };
        let frame = Frame {
            camera: &camera,
            grid: None,
            skybox: s.clear == ProbeClearFlags::Skybox,
            background: s.background,
            environment: &inputs.environment,
            post: PostSettings::default(),
            lines: &[],
            overlay: &[],
            meshes: &inputs.meshes,
            particles: &[],
            terrains: &inputs.terrains,
            water: &[],
            shaded: true,
            lights: &inputs.lights,
        };
        renderer.render(device, queue, &self.target, &frame);
        self.read(device, queue)
    }

    fn read(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<Vec3> {
        let (size, row) = (self.size, Self::row(self.size));
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.target.hdr_texture().as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(size),
                },
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let slice = self.buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let mut out = Vec::with_capacity((size * size) as usize);
        {
            let data = slice.get_mapped_range().expect("mapped");
            for y in 0..size as usize {
                let line = &data[y * row as usize..][..size as usize * 8];
                let halves: &[half::f16] = bytemuck::cast_slice(line);
                for px in halves.as_chunks::<4>().0 {
                    let c = Vec3::new(px[0].to_f32(), px[1].to_f32(), px[2].to_f32());
                    // Sanitise: a NaN would spread through the whole prefilter.
                    out.push(if c.is_finite() {
                        c.max(Vec3::ZERO)
                    } else {
                        Vec3::ZERO
                    });
                }
            }
        }
        self.buffer.unmap();
        out
    }

    /// All six faces.
    pub fn cube(
        &self,
        renderer: &mut Renderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        inputs: &CaptureInputs,
        s: &CaptureSettings,
    ) -> Cube {
        let mut c = Cube::new(self.size);
        for (i, face) in c.faces.iter_mut().enumerate() {
            *face = self.face(renderer, device, queue, inputs, s, i);
        }
        c
    }
}

/// Probes of `scene` lit by `data` (same order as `probes`), for a second bounce.
pub fn probe_set(
    scene: &Scene,
    probes: &[(Entity, ReflectionProbe)],
    data: &[Arc<ProbeData>],
) -> ProbeSet {
    ProbeSet {
        probes: probes
            .iter()
            .zip(data)
            .map(|((e, p), d)| ProbeInstance::new(p, scene.world_position(*e), d.clone()))
            .collect(),
        debug: false,
    }
}

/// Bake every active probe of `scene` (Hierarchy order); `second_bounce` re-captures with the
/// first results in place. The editor does the same one probe per frame (`probe_tools`).
pub fn bake_scene(
    renderer: &mut Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    materials: &dyn MaterialSource,
    second_bounce: bool,
) -> Vec<(Entity, Arc<ProbeData>)> {
    let probes = super::scene_probe_list(scene);
    let mut inputs = CaptureInputs::from_scene(scene, materials);
    let mut captures: Vec<ProbeCapture> = Vec::new();
    let mut data: Vec<Arc<ProbeData>> = Vec::new();
    for pass in 0..1 + second_bounce as usize {
        if pass == 1 {
            inputs.environment.probes = probe_set(scene, &probes, &data);
        }
        let mut next = Vec::new();
        for (e, p) in &probes {
            let s = CaptureSettings::of(p, scene.world_position(*e));
            if !captures.iter().any(|c| c.size() == s.size) {
                captures.push(ProbeCapture::new(renderer, device, s.size));
            }
            let capture = captures.iter().find(|c| c.size() == s.size).expect("made");
            let cube = capture.cube(renderer, device, queue, &inputs, &s);
            next.push(Arc::new(ProbeData::from_capture(&cube, BAKE_SAMPLES)));
        }
        data = next;
    }
    probes.iter().map(|(e, _)| *e).zip(data).collect()
}
