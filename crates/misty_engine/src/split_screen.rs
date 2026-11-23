//! Split screen and picture-in-picture: cameras whose Viewport Rect covers only part of the
//! screen each render into their own target, which is then copied into its rect of the full
//! image, before the UI is drawn on top.
//!
//! The host (the player, the editor's Game view) renders the full-screen camera as before
//! ([`crate::render_scene::screen_camera`]), then calls [`SplitScreen::render`]. When no
//! camera covers the screen the full image is cleared to black first, so only the viewport
//! cameras render: two players at half the screen each cost two half-size renders.

use std::collections::HashMap;

use hecs::Entity;

use crate::draw_cache::DrawCache;
use crate::render::{RenderTarget, Renderer};
use crate::render_scene::{GameView, MaterialSource, viewport_cameras, viewport_pixels};
use crate::scene::Scene;

/// One viewport camera's draws and target, kept between frames.
struct View {
    cache: DrawCache,
    target: Option<RenderTarget>,
}

/// The viewport cameras' views (lives with the host's renderer).
#[derive(Default)]
pub struct SplitScreen {
    views: HashMap<Entity, View>,
}

impl SplitScreen {
    pub fn new() -> Self {
        Self::default()
    }

    /// Does `scene` have cameras with a partial Viewport Rect?
    pub fn active(scene: &Scene) -> bool {
        !viewport_cameras(scene).is_empty()
    }

    /// Render each viewport camera of `scene` into its rect of `main`, lowest Depth first.
    /// `main_rendered` = the full-screen camera already drew `main` this frame (otherwise it is
    /// cleared to black first). With `ui`, each view also draws the World Space canvases its
    /// camera sees. Returns how many views were drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        renderer: &mut Renderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        main: &RenderTarget,
        scene: &Scene,
        materials: &dyn MaterialSource,
        main_rendered: bool,
        mut ui: Option<&mut crate::render::ui::UiRenderer>,
    ) -> usize {
        let cams = viewport_cameras(scene);
        self.views.retain(|e, _| cams.contains(e));
        if cams.is_empty() {
            return 0;
        }
        if !main_rendered {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("split screen clear"),
            });
            main.clear_output(&mut enc, [0.0, 0.0, 0.0, 1.0]);
            queue.submit([enc.finish()]);
        }
        let mut drawn = 0;
        for e in cams {
            let Some(vp) = scene
                .world
                .get::<&crate::scene::Camera>(e)
                .ok()
                .map(|c| c.viewport)
            else {
                continue;
            };
            let Some((x, y, w, h)) = viewport_pixels(vp, main.output_size()) else {
                continue;
            };
            let view = self.views.entry(e).or_insert_with(|| View {
                cache: DrawCache::new(),
                target: None,
            });
            let Some(game) = GameView::cached_for(scene, materials, &mut view.cache, e) else {
                continue;
            };
            if !view
                .target
                .as_ref()
                .is_some_and(|t| renderer.view_target_fits(t, w, h))
            {
                view.target = Some(renderer.create_view_target(device, w, h));
            }
            let Some(target) = view.target.as_ref() else {
                continue;
            };
            renderer.render_with_effects(device, queue, target, &game.frame(), &game.effects);
            // World Space canvases (name tags, health bars) as this camera sees them, depth
            // tested against its own view.
            if let Some(ui) = ui.as_deref_mut()
                && let Some(view) =
                    crate::ui::UiView::split_view(scene, e, glam::Vec2::new(w as f32, h as f32))
            {
                ui.render_view(device, queue, target, scene, None, &view);
            }
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("split screen copy"),
            });
            target.copy_output_into(&mut enc, main, (x, y));
            queue.submit([enc.finish()]);
            drawn += 1;
        }
        drawn
    }
}

#[cfg(test)]
#[path = "split_screen_gpu_tests.rs"]
mod gpu_tests;

#[cfg(test)]
mod tests {
    use crate::render_scene::{covers_screen, screen_camera, viewport_cameras, viewport_pixels};
    use crate::scene::{Camera, Scene};

    fn cam(s: &mut Scene, name: &str, viewport: [f32; 4], depth: f32) -> hecs::Entity {
        let e = s.spawn(name, None);
        s.add(
            e,
            Camera {
                viewport,
                depth,
                ..Default::default()
            },
        );
        e
    }

    #[test]
    fn viewport_rects_map_to_pixels_with_y_flipped() {
        assert!(covers_screen([0.0, 0.0, 1.0, 1.0]));
        assert!(!covers_screen([0.0, 0.0, 0.5, 1.0]));
        // Left half, right half, top half of 800×600.
        assert_eq!(
            viewport_pixels([0.0, 0.0, 0.5, 1.0], (800, 600)),
            Some((0, 0, 400, 600))
        );
        assert_eq!(
            viewport_pixels([0.5, 0.0, 0.5, 1.0], (800, 600)),
            Some((400, 0, 400, 600))
        );
        assert_eq!(
            viewport_pixels([0.0, 0.5, 1.0, 0.5], (800, 600)),
            Some((0, 0, 800, 300))
        );
        // Bottom-right quarter, an off-screen part clipped, an empty one.
        assert_eq!(
            viewport_pixels([0.5, 0.0, 0.5, 0.5], (800, 600)),
            Some((400, 300, 400, 300))
        );
        assert_eq!(
            viewport_pixels([0.75, 0.0, 0.5, 1.0], (800, 600)),
            Some((600, 0, 200, 600))
        );
        assert_eq!(viewport_pixels([1.0, 0.0, 0.5, 1.0], (800, 600)), None);
    }

    #[test]
    fn full_screen_camera_and_viewport_cameras() {
        let mut s = Scene::new("Split");
        // One ordinary camera: it renders the screen, as before.
        let main = cam(&mut s, "Main", [0.0, 0.0, 1.0, 1.0], -1.0);
        assert_eq!(screen_camera(&s), Some(main));
        assert!(viewport_cameras(&s).is_empty());
        // A minimap on top: the main camera still fills the screen.
        let map = cam(&mut s, "Minimap", [0.75, 0.75, 0.25, 0.25], 5.0);
        assert_eq!(screen_camera(&s), Some(main));
        assert_eq!(viewport_cameras(&s), [map]);
        // Two players and no full-screen camera: only the halves render, lowest Depth first.
        s.despawn(main);
        s.despawn(map);
        let p2 = cam(&mut s, "P2", [0.5, 0.0, 0.5, 1.0], 1.0);
        let p1 = cam(&mut s, "P1", [0.0, 0.0, 0.5, 1.0], 0.0);
        assert_eq!(screen_camera(&s), None);
        assert_eq!(viewport_cameras(&s), [p1, p2]);
        // A disabled camera doesn't count.
        s.world.get::<&mut Camera>(p2).unwrap().enabled = false;
        assert_eq!(viewport_cameras(&s), [p1]);
    }
}
