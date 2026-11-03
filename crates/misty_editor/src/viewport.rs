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
}

/// wgpu device access plus the engine renderer.
pub struct Gpu {
    pub state: egui_wgpu::RenderState,
    pub renderer: Renderer,
    /// Canvas UI drawn over the Game view.
    pub ui: UiRenderer,
    pub scene: Viewport,
    pub game: Viewport,
}

impl Gpu {
    pub fn new(state: egui_wgpu::RenderState) -> Self {
        let renderer = Renderer::new(&state.device);
        let ui = UiRenderer::new(&state.device, &state.queue);
        Self {
            state,
            renderer,
            ui,
            scene: Viewport::default(),
            game: Viewport::default(),
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
        vp.texture.expect("texture registered above")
    }
}
