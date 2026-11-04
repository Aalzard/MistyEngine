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
    /// When this view last drew a frame (the Hub thumbnail takes the most recent one).
    last_render: Option<std::time::Instant>,
}

/// wgpu device access plus the engine renderer.
pub struct Gpu {
    pub state: egui_wgpu::RenderState,
    pub renderer: Renderer,
    /// Canvas UI drawn over the Game view.
    pub ui: UiRenderer,
    pub scene: Viewport,
    pub game: Viewport,
    /// [`misty_engine::quality::generation`] the renderer was set up for.
    quality: u64,
}

impl Gpu {
    pub fn new(state: egui_wgpu::RenderState) -> Self {
        let renderer = Renderer::with_queue(&state.device, &state.queue);
        let ui = UiRenderer::new(&state.device, &state.queue);
        Self {
            state,
            renderer,
            ui,
            scene: Viewport::default(),
            game: Viewport::default(),
            quality: u64::MAX,
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
    /// Render `frame` at `size_px` and return the texture to show with `ui.image`.
    pub fn render(&mut self, view: ViewId, size_px: [u32; 2], frame: &Frame) -> egui::TextureId {
        // The previewed quality level (shadow resolution, effects).
        let generation = misty_engine::quality::generation();
        if generation != self.quality {
            self.quality = generation;
            let level = misty_engine::quality::active();
            self.renderer.set_quality(&self.state.device, &level);
        }
        let vp = match view {
            ViewId::Scene => &mut self.scene,
            ViewId::Game => &mut self.game,
        };
        let size = (size_px[0].max(1), size_px[1].max(1));
        if vp.target.as_ref().map(|t| t.size()) != Some(size) {
            let target = self
                .renderer
                .create_target(&self.state.device, size.0, size.1);
            let mut egui_renderer = self.state.renderer.write();
            let filter = wgpu::FilterMode::Linear;
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
            vp.target = Some(target);
        }
        let target = vp.target.as_ref().expect("target created above");
        self.renderer
            .render(&self.state.device, &self.state.queue, target, frame);
        vp.last_render = Some(std::time::Instant::now());
        vp.texture.expect("texture registered above")
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
        let (w, h) = target.size();
        let pixels =
            crate::project_thumbnail::read_pixels(&self.state.device, &self.state.queue, target);
        Some((w, h, pixels))
    }
}
