//! A main pass without its glow target in frames without emission.
//!
//! The main pass resolves a second HDR target, the glow (emission only, the bloom's input), a
//! 4× MSAA image as big as the colour (66 MB at 1080p). In a scene without emissive surfaces
//! the only thing that writes it is the sky's sun disc (and stars, moon, aurora). After
//! [`QUIET_FRAMES`] frames without anything else that writes glow, the renderer swaps in
//! pipelines built without the glow target (an empty colour slot, [`Spec::glowless`]), drops
//! the MSAA glow image, and writes the resolved glow in one full-screen pass after the main
//! pass instead: the sky's glow times the share of each pixel's samples the sky shaded (those
//! left at the far plane), which is what the resolve averaged before. Same bloom input,
//! without the image and its per-sample writes.
//!
//! The glowless pipelines are built the first time a scene goes quiet (on a worker while the
//! background loads are on; the frames keep the glow target meanwhile) and kept: the
//! renderer's pipeline owners hold one set and this keeps the other, and switching swaps them,
//! so emission coming back (an emissive material, particles, water, a custom shader...) costs
//! nothing but the image's reallocation.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError};

use super::pipelines::{self, MainSet, Spec};
use super::surface::PipelineKey;
use super::trees::TreeMain;
use super::{Frame, RenderTarget, Renderer, draw_prep, fast_paths, lod};

/// Frames in a row without emission before the main pass drops its glow target.
const QUIET_FRAMES: u32 = 30;

/// The sky glow pass (see the module docs).
pub(super) struct SkyGlow {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

impl SkyGlow {
    fn new(device: &wgpu::Device, view_layout: &wgpu::BindGroupLayout, samples: u32) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("misty sky glow"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: samples > 1,
                },
                count: None,
            }],
        });
        let depth_texture = match samples > 1 {
            true => "texture_depth_multisampled_2d",
            false => "texture_depth_2d",
        };
        let source = format!(
            "{}\n{}",
            include_str!("shaders/sky.wgsl"),
            include_str!("shaders/sky_glow.wgsl")
                .replace("DEPTH_TEXTURE", depth_texture)
                .replace("GLOW_SAMPLES", &format!("{}u", samples.max(1)))
        );
        let pipeline = pipelines::build(
            device,
            Spec {
                fs: Some("fs_sky_glow"),
                depth: None,
                samples: 1,
                glow: None,
                ..Spec::new("sky glow", &source, &[view_layout, &layout])
            },
        );
        Self { pipeline, layout }
    }
}

/// A finished job: pipelines for the mode an owner isn't using (`bool`: built glowless).
enum Built {
    Main(MainSet),
    Trees(TreeMain, bool),
    Grass(wgpu::RenderPipeline, bool),
    Sky(SkyGlow),
}

/// Job slots (one job each at a time).
const MAIN: usize = 0;
const TREES: usize = 1;
const GRASS: usize = 2;
const SKY: usize = 3;

/// The renderer's part.
#[derive(Default)]
pub(super) struct Glowless {
    /// This frame's main pass has no glow target.
    pub active: bool,
    /// Frames in a row without emission.
    quiet: u32,
    /// The main-pass pipelines for the mode [`super::pipelines::Pipelines`] isn't using.
    main: Option<MainSet>,
    /// The same for the trees and the grass composite (with the mode they were built for).
    trees: Option<(TreeMain, bool)>,
    grass: Option<(wgpu::RenderPipeline, bool)>,
    sky: Option<SkyGlow>,
    /// (Behind a lock only so the renderer stays `Sync`.)
    jobs: Mutex<Vec<Receiver<Built>>>,
    building: [bool; 4],
}

impl Glowless {
    /// Start `job` (on a worker with `background`, else now), once per slot at a time.
    fn start(
        &mut self,
        slot: usize,
        background: bool,
        job: impl FnOnce() -> Built + Send + 'static,
    ) {
        if self.building[slot] {
            return;
        }
        if !background {
            self.store(job());
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("misty glowless pipelines".to_owned())
            .spawn(move || {
                let _ = tx.send(job());
            });
        if spawned.is_ok() {
            self.building[slot] = true;
            let jobs = self.jobs.get_mut().unwrap_or_else(|e| e.into_inner());
            jobs.push(rx);
        }
    }

    fn store(&mut self, built: Built) {
        match built {
            Built::Main(m) => {
                self.building[MAIN] = false;
                self.main = Some(m);
            }
            Built::Trees(t, g) => {
                self.building[TREES] = false;
                self.trees = Some((t, g));
            }
            Built::Grass(p, g) => {
                self.building[GRASS] = false;
                self.grass = Some((p, g));
            }
            Built::Sky(s) => {
                self.building[SKY] = false;
                self.sky = Some(s);
            }
        }
    }

    /// Take the jobs that finished. (A job that panicked keeps its slot busy: not retried.)
    fn collect(&mut self) {
        let mut done = Vec::new();
        let jobs = self.jobs.get_mut().unwrap_or_else(|e| e.into_inner());
        jobs.retain(|rx| match rx.try_recv() {
            Ok(built) => {
                done.push(built);
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        for built in done {
            self.store(built);
        }
    }
}

impl Renderer {
    /// Something in this frame writes the glow target, or needs pipelines that only exist
    /// with it.
    fn frame_glows(&self, frame: &Frame) -> bool {
        !frame.shaded
            || !frame.particles.is_empty()
            || !frame.water.is_empty()
            || self.effects.has_draws()
            || self.capture.is_some()
            || self.wire.is_some()
            || self.wire_only
            || self.options.depth_prepass
            || self.vgeo.on
            || !fast_paths::sky_late()
            || frame.meshes.iter().any(|d| {
                let m = &d.material;
                m.custom.is_some()
                    || m.mode.is_blended()
                    || (m.emission_intensity != 0.0 && m.emission.iter().any(|&c| c > 0.0))
            })
    }

    /// Pick this frame's glow mode (after `prepare_assets`, before the frame's draw lists):
    /// glowless once the scene has been quiet for [`QUIET_FRAMES`] and every pipeline the
    /// frame may draw with has its glowless version (missing ones are started).
    pub(super) fn update_glowless(
        &mut self,
        device: &wgpu::Device,
        frame: &Frame,
        lod: &[lod::LodState],
    ) {
        self.glowless.collect();
        let glows = self.frame_glows(frame);
        let g = &mut self.glowless;
        g.quiet = if glows { 0 } else { g.quiet.saturating_add(1) };
        let want = g.quiet >= QUIET_FRAMES && fast_paths::glowless();
        let background = self.background_loads;
        let mut ready = want;
        if want {
            let samples = super::sample_count();
            if !self.glowless.active && self.glowless.main.is_none() {
                ready = false;
                let (device, layouts) = (device.clone(), self.pipelines.main_layouts());
                self.glowless.start(MAIN, background, move || {
                    Built::Main(MainSet::new(&device, &layouts, samples, true))
                });
            }
            if self.glowless.sky.is_none() {
                ready = false;
                let (device, view) = (device.clone(), self.pipelines.view_layout.clone());
                self.glowless.start(SKY, background, move || {
                    Built::Sky(SkyGlow::new(&device, &view, samples))
                });
            }
            // The frame's distinct surface keys (a handful), then their glowless variants.
            let mut keys: Vec<PipelineKey> = Vec::new();
            let mut add = |key: PipelineKey| {
                let key = PipelineKey {
                    glowless: true,
                    ..key
                };
                if !keys.contains(&key) {
                    keys.push(key);
                }
            };
            for (d, state) in frame.meshes.iter().zip(lod) {
                add(draw_prep::pipeline_key(d, *state));
                if d.lod.is_some() {
                    add(draw_prep::pipeline_key(d, lod::LodState::Fading(0.5)));
                }
            }
            for key in keys {
                self.pipelines.ensure_mesh(device, key);
                ready &= self.pipelines.has_mesh_variant(key);
            }
            // Trees and the grass visibility buffer's pipelines made later in this frame
            // would be the glow ones: wait for them first.
            let trees = frame
                .terrains
                .iter()
                .any(|t| !t.data.trees.instances.is_empty());
            match self.trees.glowless() {
                None if trees => ready = false,
                Some(false) if !self.glowless.trees.as_ref().is_some_and(|t| t.1) => {
                    ready = false;
                    if let Some(job) = self.trees.other_main_job(device, &self.pipelines) {
                        self.glowless
                            .start(TREES, background, move || Built::Trees(job(), true));
                    }
                }
                _ => {}
            }
            match &self.grass_vis {
                None if !frame.terrains.is_empty() => ready = false,
                Some(Some(v))
                    if !v.glowless() && !self.glowless.grass.as_ref().is_some_and(|g| g.1) =>
                {
                    ready = false;
                    let job = v.other_composite_job(device, &self.pipelines);
                    self.glowless
                        .start(GRASS, background, move || Built::Grass(job(), true));
                }
                _ => {}
            }
        }
        self.set_glowless(ready);
    }

    /// Swap every owner's main-pass pipelines to the glowless (`on`) or glow ones.
    fn set_glowless(&mut self, on: bool) {
        let g = &mut self.glowless;
        if g.active != on {
            let Some(set) = g.main.as_mut() else {
                return;
            };
            self.pipelines.swap_main(set);
            g.active = on;
        }
        if let Some(mode) = self.trees.glowless()
            && mode != on
            && let Some((t, built)) = g.trees.as_mut()
            && *built == on
        {
            self.trees.swap_main(t);
            *built = mode;
        }
        if let Some(Some(v)) = self.grass_vis.as_mut()
            && v.glowless() != on
            && let Some((p, built)) = g.grass.as_mut()
            && *built == on
        {
            v.swap_composite(p);
            *built = !on;
        }
        self.pipelines.variants.glowless = g.active;
    }

    /// After a glowless main pass: the resolved glow image from the sky (module docs).
    pub(super) fn sky_glow_pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
    ) {
        if !self.glowless.active {
            return;
        }
        let Some(sky) = &self.glowless.sky else {
            return;
        };
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("misty sky glow"),
            layout: &sky.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(target.depth_view()),
            }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("misty sky glow"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.glow_view(),
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
        pass.set_pipeline(&sky.pipeline);
        pass.set_bind_group(0, &self.view_bind_group, &[]);
        pass.set_bind_group(1, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::super::pipelines::COMMON;
    use wgpu::naga;

    /// The sky glow shader (both depth texture kinds) parses and validates.
    #[test]
    fn sky_glow_validates() {
        for (depth, samples) in [("texture_depth_multisampled_2d", "4u"), ("texture_depth_2d", "1u")] {
            let src = format!(
                "{COMMON}\n{}\n{}",
                include_str!("shaders/sky.wgsl"),
                include_str!("shaders/sky_glow.wgsl")
                    .replace("DEPTH_TEXTURE", depth)
                    .replace("GLOW_SAMPLES", samples)
            );
            let module = naga::front::wgsl::parse_str(&src)
                .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{e:?}"));
        }
    }

    /// A static scene without emission, looking at the sun: after `QUIET_FRAMES` the main pass
    /// drops its glow target and the sky glow pass feeds the bloom; the image matches a
    /// renderer kept on the glow target.
    #[test]
    fn glowless_frames_match_the_glow_target() {
        use crate::camera::{Camera, Projection};
        use crate::render::gpu_tests::{draw, headless, material, read_back};
        use crate::render::*;
        use crate::scene::PrimitiveMesh;
        use glam::{Mat4, Quat, Vec3};
        use super::QUIET_FRAMES;
        let Some((device, queue)) = headless() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let camera = Camera {
            position: Vec3::new(0.0, 1.5, -6.0),
            rotation: Quat::from_rotation_x(-8f32.to_radians()),
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.1,
            far: 200.0,
        };
        let grey = material([0.6, 0.6, 0.6, 1.0], RenderingMode::Opaque);
        let meshes = [
            draw(PrimitiveMesh::Cube, Mat4::from_translation(Vec3::new(0.0, 0.5, 0.0)), &grey),
            draw(
                PrimitiveMesh::Plane,
                Mat4::from_scale(Vec3::splat(20.0)),
                &grey,
            ),
        ];
        // The sun low in front of the camera: its disc is in view and glows.
        let lights = [LightData {
            kind: LightType::Directional,
            position: Vec3::ZERO,
            direction: -Vec3::new(0.15, 0.12, 1.0).normalize(),
            color: [1.0; 3],
            intensity: 1.0,
            range: 10.0,
            spot_angle: 45.0,
            shadows: Shadows::Soft,
            shadow: Default::default(),
            volumetric: Default::default(),
            contact_shadows: false,
            area: Default::default(),
        }];
        let environment = Environment::default();
        let frame = Frame {
            camera: &camera,
            grid: None,
            skybox: true,
            background: [0.1; 3],
            environment: &environment,
            post: PostSettings::default(),
            lines: &[],
            overlay: &[],
            meshes: &meshes,
            particles: &[],
            terrains: &[],
            shaded: true,
            lights: &lights,
            water: &[],
        };
        let run = |glowless: bool| {
            let mut r = Renderer::with_queue(&device, &queue);
            let target = r.create_target(&device, 256, 160);
            for _ in 0..QUIET_FRAMES + 8 {
                if !glowless {
                    r.glowless.quiet = 0;
                }
                r.render(&device, &queue, &target, &frame);
            }
            assert_eq!(r.glowless.active, glowless);
            assert_eq!(target.glow_msaa_view().is_none(), glowless);
            read_back(&device, &queue, &target)
        };
        let (with, without) = (run(false), run(true));
        let worst = with
            .iter()
            .zip(&without)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(worst <= 2, "glowless frame differs by up to {worst}");
    }
}
