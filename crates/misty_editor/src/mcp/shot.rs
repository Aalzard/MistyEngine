//! Screenshots of the Scene and Game views for `editor.screenshot`.
//!
//! The live editor reads back the view's last frame (what the user sees, without the egui
//! overlays) or, at another size or for a view that is not open, renders one offscreen with
//! the editor's renderer. `misty --mcp` makes a windowless GPU device on the first screenshot.

use misty_engine::render::{Frame, Renderer};
use misty_engine::render_scene::{self, GameView};
use misty_engine::wgpu;

use crate::state::EditorState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Scene,
    Game,
}

/// `(width, height, RGBA8)`.
pub type Pixels = (u32, u32, Vec<u8>);

/// Default offscreen size.
pub const DEFAULT_SIZE: (u32, u32) = (960, 540);

pub trait Shooter {
    /// The view as RGBA8; `size` = render at that size (else the view's own, or the default).
    fn capture(
        &mut self,
        s: &mut EditorState,
        view: View,
        size: Option<(u32, u32)>,
    ) -> Result<Pixels, String>;
}

/// No GPU (tests that never take screenshots).
#[cfg(test)]
pub struct NoGpu;

#[cfg(test)]
impl Shooter for NoGpu {
    fn capture(
        &mut self,
        _: &mut EditorState,
        _: View,
        _: Option<(u32, u32)>,
    ) -> Result<Pixels, String> {
        Err("no GPU is available for screenshots".into())
    }
}

/// `misty --mcp`: a device without a window, made on first use.
#[derive(Default)]
pub struct HeadlessGpu {
    gpu: Option<Result<(wgpu::Device, wgpu::Queue, Renderer), String>>,
    /// Kept alive with the device (the loader unloads the driver with its last instance).
    instance: Option<wgpu::Instance>,
}

impl HeadlessGpu {
    fn device(&mut self) -> Result<&mut (wgpu::Device, wgpu::Queue, Renderer), String> {
        if self.gpu.is_none() {
            let instance = wgpu::Instance::new(
                wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
            );
            let made = (|| {
                let adapter = pollster::block_on(
                    instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
                )
                .map_err(|e| format!("no GPU adapter for screenshots: {e}"))?;
                let desc = wgpu::DeviceDescriptor {
                    required_limits: misty_engine::render::device_limits(
                        &adapter,
                        Default::default(),
                    ),
                    ..Default::default()
                };
                let (device, queue) = pollster::block_on(adapter.request_device(&desc))
                    .map_err(|e| format!("no GPU device for screenshots: {e}"))?;
                // Complete first frames: grass is built in the frame that needs it.
                misty_engine::render::set_grass_streaming(false);
                let renderer = Renderer::new(&device);
                Ok((device, queue, renderer))
            })();
            self.instance = Some(instance);
            self.gpu = Some(made);
        }
        match self.gpu.as_mut() {
            Some(Ok(g)) => Ok(g),
            Some(Err(e)) => Err(e.clone()),
            None => Err("no GPU".into()),
        }
    }
}

impl Shooter for HeadlessGpu {
    fn capture(
        &mut self,
        s: &mut EditorState,
        view: View,
        size: Option<(u32, u32)>,
    ) -> Result<Pixels, String> {
        let (device, queue, renderer) = self.device()?;
        render_offscreen(
            device,
            queue,
            renderer,
            s,
            view,
            size.unwrap_or(DEFAULT_SIZE),
        )
    }
}

/// The live editor's views.
pub struct LiveShots<'a>(pub Option<&'a mut crate::viewport::Gpu>);

impl Shooter for LiveShots<'_> {
    fn capture(
        &mut self,
        s: &mut EditorState,
        view: View,
        size: Option<(u32, u32)>,
    ) -> Result<Pixels, String> {
        let gpu = self
            .0
            .as_deref_mut()
            .ok_or("the editor has no renderer (wgpu unavailable)")?;
        let id = match view {
            View::Scene => crate::viewport::ViewId::Scene,
            View::Game => crate::viewport::ViewId::Game,
        };
        if size.is_none()
            && let Some(p) = gpu.view_pixels(id)
        {
            return Ok(p);
        }
        let (device, queue) = (gpu.state.device.clone(), gpu.state.queue.clone());
        render_offscreen(
            &device,
            &queue,
            &mut gpu.renderer,
            s,
            view,
            size.unwrap_or(DEFAULT_SIZE),
        )
    }
}

/// Render `view` of the editor's scene into a new target and read it back.
pub fn render_offscreen(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
    s: &mut EditorState,
    view: View,
    (w, h): (u32, u32),
) -> Result<Pixels, String> {
    let target = renderer.create_target(device, w.max(16), h.max(16));
    // Temporal effects settle and textures finish loading over a few frames.
    let mut draw = |frame: &Frame, effects: &[misty_engine::vfx::EffectDraw]| {
        for i in 0..240 {
            renderer.render_with_effects(device, queue, &target, frame, effects);
            if i >= 8 && renderer.stats().loading == 0 {
                break;
            }
        }
    };
    match view {
        View::Game => {
            let game = GameView::new(&s.scene, &s.assets)
                .ok_or("the scene has no enabled Camera for the Scene Viewer")?;
            draw(&game.frame(), &game.effects);
        }
        View::Scene => {
            let scene = &s.scene;
            let camera = s.scene_view.camera.camera();
            let scene_lights = render_scene::lights(scene);
            let environment = render_scene::environment(scene, &scene_lights);
            let lights = if scene_lights.is_empty() {
                render_scene::headlight(&camera)
            } else {
                scene_lights
            };
            let meshes = misty_engine::render_scene::mesh_draws(scene, &s.assets, |_| false);
            let particles = misty_engine::particles::batches_where(scene, |_| true);
            let terrains = misty_engine::terrain::draws_where(scene, |_| true);
            let water = misty_engine::water::draws_where(scene, |_| true);
            let effects = misty_engine::vfx::draws_where(scene, |_| true);
            let frame = Frame {
                camera: &camera,
                grid: None,
                skybox: !s.scene_view.camera.is_2d(),
                background: [0.28, 0.28, 0.28],
                environment: &environment,
                post: render_scene::post_settings(scene),
                lines: &[],
                overlay: &[],
                meshes: &meshes,
                particles: &particles,
                terrains: &terrains,
                shaded: true,
                lights: &lights,
                water: &water,
            };
            draw(&frame, &effects);
        }
    }
    let pixels = crate::project_thumbnail::read_pixels(device, queue, &target);
    let (tw, th) = target.output_size();
    Ok((tw, th, pixels))
}
