//! GPU context shared by the Scene and Game views, and per-view offscreen targets.

use misty_engine::render::ui::UiRenderer;
use misty_engine::render::{Frame, RenderTarget, Renderer};
use misty_engine::scene::Scene;
use misty_engine::ui::UiInteraction;
use misty_engine::wgpu;

/// One view's render target, registered as an egui texture.
#[derive(Default)]
pub struct Viewport {
    target: Option<RenderTarget>,
    texture: Option<egui::TextureId>,
    /// Filter the texture is registered with (Quality ▸ Upscale Filter).
    filter: Option<wgpu::FilterMode>,
    /// When this view last drew a frame (the Hub thumbnail takes the most recent one).
    last_render: Option<std::time::Instant>,
}

/// wgpu device access plus the engine renderer.
pub struct Gpu {
    pub state: egui_wgpu::RenderState,
    pub renderer: Renderer,
    /// Canvas UI drawn over the Game view.
    pub ui: UiRenderer,
    /// Colour-blind filter over the Game view, UI included (Player Settings ▸ Accessibility).
    pub color_filter: misty_engine::render::color_filter::ColorFilter,
    pub scene: Viewport,
    pub game: Viewport,
    /// [`misty_engine::quality::generation`] the renderer was set up for.
    quality: u64,
    /// How view images are stretched to their panel (Quality ▸ Upscale Filter).
    filter: wgpu::FilterMode,
}

/// Pipelines built when the renderer started being made on its worker (`u64::MAX`: no
/// renderer is being made). The status bar counts the compiles since.
static STARTING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(u64::MAX);

/// While the renderer is being made: the shader pipelines compiled for it so far.
pub fn renderer_starting() -> Option<u64> {
    let at = STARTING.load(std::sync::atomic::Ordering::Relaxed);
    (at != u64::MAX).then(|| misty_engine::render::pipelines_built().saturating_sub(at))
}

/// What the renderer's worker hands back.
type Parts = Result<(Renderer, UiRenderer), String>;

/// The renderer being made on a worker thread when a project opens. Its pipelines took 3 to
/// 10 s to compile on a first launch (the terrain, grass and mesh shaders take seconds each),
/// all of it on the main thread before round 44: the window showed nothing and Windows called
/// it Not Responding. Meanwhile the editor draws its panels and the views show
/// "Compiling shaders".
pub struct GpuStart {
    state: egui_wgpu::RenderState,
    rx: std::sync::mpsc::Receiver<Parts>,
    started: std::time::Instant,
}

impl GpuStart {
    pub fn spawn(state: egui_wgpu::RenderState) -> Self {
        misty_engine::render::cache_shader_modules(&state.device);
        // Compiled pipelines from earlier launches (Vulkan): a new build of the editor starts
        // with the driver's own cache empty.
        misty_engine::render::open_pipeline_cache(
            &state.device,
            &state.adapter.get_info(),
            &misty_engine::assets::absolute("Library/ShaderCache"),
        );
        STARTING.store(
            misty_engine::render::pipelines_built(),
            std::sync::atomic::Ordering::Relaxed,
        );
        let (tx, rx) = std::sync::mpsc::channel();
        let (device, queue) = (state.device.clone(), state.queue.clone());
        let spawned = std::thread::Builder::new()
            .name("misty background renderer".into())
            .spawn(move || {
                let parts = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let renderer = {
                        misty_engine::profile_scope!(
                            "Renderer.New",
                            misty_engine::profiler::Category::Rendering
                        );
                        let mut r = Renderer::with_queue(&device, &queue);
                        // The quality level the views draw with, then one throwaway frame:
                        // what a first render makes for itself is made here too.
                        r.set_quality(&device, &misty_engine::quality::active());
                        r.warm_up(&device, &queue);
                        r
                    };
                    let ui = {
                        misty_engine::profile_scope!("UiRenderer.New");
                        UiRenderer::new(&device, &queue)
                    };
                    (renderer, ui)
                }))
                .map_err(|p| {
                    p.downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| p.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "the renderer could not start".to_owned())
                });
                let _ = tx.send(parts);
            });
        if let Err(e) = spawned {
            let (tx, failed) = std::sync::mpsc::channel();
            let _ = tx.send(Err(e.to_string()));
            return Self {
                state,
                rx: failed,
                started: std::time::Instant::now(),
            };
        }
        Self {
            state,
            rx,
            started: std::time::Instant::now(),
        }
    }

    /// The GPU context once the worker is done (`Err`: it failed; the views stay off).
    pub fn poll(&mut self) -> Option<Result<Gpu, String>> {
        let parts = match self.rx.try_recv() {
            Ok(parts) => parts,
            Err(std::sync::mpsc::TryRecvError::Empty) => return None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("the renderer thread stopped".to_owned())
            }
        };
        STARTING.store(u64::MAX, std::sync::atomic::Ordering::Relaxed);
        Some(parts.map(|(mut renderer, ui)| {
            // The variants most scenes draw with compile next, in the background.
            renderer.warm_common_pipelines(&self.state.device);
            Gpu::from_parts(self.state.clone(), renderer, ui)
        }))
    }

    /// The GPU the renderer is being made for.
    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.state.adapter.get_info()
    }

    /// How long the renderer has been starting.
    pub fn elapsed(&self) -> std::time::Duration {
        self.started.elapsed()
    }
}

impl Drop for GpuStart {
    fn drop(&mut self) {
        STARTING.store(u64::MAX, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Gpu {
    fn from_parts(state: egui_wgpu::RenderState, renderer: Renderer, ui: UiRenderer) -> Self {
        Self {
            state,
            renderer,
            ui,
            color_filter: Default::default(),
            scene: Viewport::default(),
            game: Viewport::default(),
            quality: u64::MAX,
            filter: wgpu::FilterMode::Linear,
        }
    }

    /// Draw the scene's canvases over the Game view image rendered this frame.
    pub fn render_ui(&mut self, scene: &Scene, interaction: Option<&UiInteraction>) {
        if let Some(target) = self.game.target.as_ref() {
            self.ui.render(
                &self.state.device,
                &self.state.queue,
                target,
                scene,
                interaction,
            );
            self.color_filter
                .apply(&self.state.device, &self.state.queue, target);
        }
    }
}

impl Gpu {
    /// Draw the scene's canvases into the Scene view image rendered this frame: overlays in the
    /// world at one unit per pixel of a `screen`-sized game view, the others where they are, all
    /// seen by the scene `camera` and hidden behind nearer scene depth.
    pub fn render_scene_ui(
        &mut self,
        scene: &Scene,
        camera: &misty_engine::camera::Camera,
        screen: glam::Vec2,
    ) {
        if let Some(target) = self.scene.target.as_ref() {
            let (w, h) = target.output_size();
            let size = glam::Vec2::new(w as f32, h as f32);
            let view = misty_engine::ui::UiView::scene_view(screen, size, *camera);
            self.ui.render_view(
                &self.state.device,
                &self.state.queue,
                target,
                scene,
                None,
                &view,
            );
        }
    }
}

/// Which view to draw into.
#[derive(Clone, Copy)]
pub enum ViewId {
    Scene,
    Game,
}

impl Gpu {
    /// Render `frame` (and the Visual Effects in `effects`) at `size_px` and return the texture
    /// to show with `ui.image`.
    pub fn render(
        &mut self,
        view: ViewId,
        size_px: [u32; 2],
        frame: &Frame,
        effects: &[misty_engine::vfx::EffectDraw],
    ) -> egui::TextureId {
        misty_engine::profile_scope!(
            match view {
                ViewId::Scene => "Editor.SceneView",
                ViewId::Game => "Editor.GameView",
            },
            misty_engine::profiler::Category::Rendering
        );
        // The previewed quality level (shadow resolution, effects).
        let generation = misty_engine::quality::generation();
        if generation != self.quality {
            self.quality = generation;
            let level = misty_engine::quality::active();
            self.renderer.set_quality(&self.state.device, &level);
            self.filter = if level.upscale_filter.nearest() {
                wgpu::FilterMode::Nearest
            } else {
                wgpu::FilterMode::Linear
            };
        }
        let filter = self.filter;
        self.renderer.set_view_label(match view {
            ViewId::Scene => "Scene",
            ViewId::Game => "Game",
        });
        let vp = match view {
            ViewId::Scene => &mut self.scene,
            ViewId::Game => &mut self.game,
        };
        let size = (size_px[0].max(1), size_px[1].max(1));
        // Quality ▸ Upscaling renders the scene smaller and rebuilds this size temporally; a
        // new target too when Quality ▸ Anti-Aliasing rebuilt the renderer (sample count).
        let fits = vp
            .target
            .as_ref()
            .is_some_and(|t| self.renderer.view_target_fits(t, size.0, size.1));
        if !fits {
            vp.target = Some(
                self.renderer
                    .create_view_target(&self.state.device, size.0, size.1),
            );
            vp.filter = None;
        }
        // (Re)register the image with egui when the target or the filter changed.
        if vp.filter != Some(filter) {
            let target = vp.target.as_ref().expect("target created above");
            let mut egui_renderer = self.state.renderer.write();
            vp.texture = Some(match vp.texture {
                Some(id) => {
                    egui_renderer.update_egui_texture_from_wgpu_texture(
                        &self.state.device,
                        target.view(),
                        filter,
                        id,
                    );
                    id
                }
                None => {
                    egui_renderer.register_native_texture(&self.state.device, target.view(), filter)
                }
            });
            vp.filter = Some(filter);
        }
        let target = vp.target.as_ref().expect("target created above");
        // The Scene view shares the Game view's Misty GI clipmaps when its camera is near.
        target.set_misty_gi_secondary(matches!(view, ViewId::Scene));
        // The views draw what is ready and swap the rest in as the background queues finish
        // it (new shader variants, models, textures); previews, thumbnails and bakes, which
        // render through the same renderer, always wait for everything.
        self.renderer.set_background_loads(true);
        self.renderer.render_with_effects(
            &self.state.device,
            &self.state.queue,
            target,
            frame,
            effects,
        );
        self.renderer.set_background_loads(false);
        vp.last_render = Some(std::time::Instant::now());
        vp.texture.expect("texture registered above")
    }

    /// The Scene and Game views' render targets (Memory Profiler).
    pub fn view_target_items(&self) -> Vec<misty_engine::memory_snapshot::MemItem> {
        use misty_engine::memory_snapshot::{MemCategory, MemItem};
        [("Editor Scene", &self.scene), ("Scene Viewer", &self.game)]
            .into_iter()
            .filter_map(|(name, vp)| {
                let t = vp.target.as_ref()?;
                let (w, h) = t.output_size();
                let name = match t.upscaled() {
                    true => {
                        let (rw, rh) = t.size();
                        format!("{name} target ({w}×{h}, rendered at {rw}×{rh})")
                    }
                    false => format!("{name} target ({w}×{h})"),
                };
                Some(MemItem::new(
                    MemCategory::RenderTargets,
                    "RenderTexture",
                    name,
                    t.memory_bytes(),
                ))
            })
            .collect()
    }

    /// The Game view's last frame `(width, height, RGBA8)` (save game thumbnails).
    pub fn game_view_pixels(&self) -> Option<(u32, u32, Vec<u8>)> {
        let target = self.game.target.as_ref()?;
        let (w, h) = target.output_size();
        let pixels =
            crate::project_thumbnail::read_pixels(&self.state.device, &self.state.queue, target);
        Some((w, h, pixels))
    }

    /// One view's last frame `(width, height, RGBA8)`, `None` before it has drawn (MCP
    /// screenshots).
    pub fn view_pixels(&self, view: ViewId) -> Option<(u32, u32, Vec<u8>)> {
        let vp = match view {
            ViewId::Scene => &self.scene,
            ViewId::Game => &self.game,
        };
        let target = vp.target.as_ref()?;
        let (w, h) = target.output_size();
        let pixels =
            crate::project_thumbnail::read_pixels(&self.state.device, &self.state.queue, target);
        Some((w, h, pixels))
    }

    /// The last frame of the view the user was looking at: the one drawn most recently, the
    /// Scene view when both were on screen. `(width, height, RGBA8)`.
    pub fn last_view_pixels(&self) -> Option<(u32, u32, Vec<u8>)> {
        let recent = |vp: &Viewport| vp.target.as_ref().and(vp.last_render);
        let vp = match (recent(&self.scene), recent(&self.game)) {
            (Some(s), Some(g)) if g > s + std::time::Duration::from_millis(250) => &self.game,
            (Some(_), _) => &self.scene,
            (None, Some(_)) => &self.game,
            (None, None) => return None,
        };
        let target = vp.target.as_ref()?;
        let (w, h) = target.output_size();
        let pixels =
            crate::project_thumbnail::read_pixels(&self.state.device, &self.state.queue, target);
        Some((w, h, pixels))
    }
}
