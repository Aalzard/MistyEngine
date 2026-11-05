//! Occlusion culling in the Scene view: the box of a selected Occlusion Area, and while the
//! Occlusion Culling window shows its Visualization page, the view volumes, the game camera's
//! view cell and the objects that camera culls (hidden), with a summary in the corner.

use glam::{Mat4, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::occlusion::{CellLookup, store};
use misty_engine::render::Environment;
use misty_engine::render::culling::Aabb;
use misty_engine::scene::{OcclusionArea, Scene};

use super::Lines;
use crate::panels::occlusion::OcclusionWindow;

const AREA: [f32; 4] = [0.55, 0.95, 0.65, 0.9];
const VOLUME: [f32; 4] = [0.55, 0.95, 0.65, 0.35];
const CELL: [f32; 4] = [0.3, 0.85, 1.0, 1.0];
const CELL_INVALID: [f32; 4] = [1.0, 0.45, 0.35, 1.0];

impl Lines {
    pub(super) fn occlusion_area(&mut self, scene: &Scene, e: Entity) {
        let Ok(a) = scene.world.get::<&OcclusionArea>(e) else {
            return;
        };
        let half = Vec3::from(a.size).abs() * 0.5;
        self.wire_box(&scene.world_matrix(e), Vec3::from(a.center), half, AREA);
    }

    fn aabb(&mut self, b: &Aabb, color: [f32; 4]) {
        let c = (b.min + b.max) * 0.5;
        self.wire_box(&Mat4::IDENTITY, c, (b.max - b.min) * 0.5, color);
    }
}

/// Visualization: view volumes and the camera's cell as lines; what the game camera culls is
/// hidden from the Scene view's draws (the view's draw cache maps draws to objects).
pub fn occlusion_visualize(
    scene: &Scene,
    draws: &mut misty_engine::draw_cache::DrawCache,
    window: &OcclusionWindow,
    lines: &mut Lines,
    environment: &mut Environment,
) {
    if window.show_volumes {
        for v in misty_engine::occlusion::bake::view_volumes(scene) {
            lines.aabb(&v, VOLUME);
        }
    }
    let Some(data) = store::for_scene(scene) else {
        return;
    };
    let Some((camera, settings)) = misty_engine::render_scene::game_camera(scene) else {
        return;
    };
    let lookup = data.lookup(camera.position);
    if window.show_cell
        && let Some(cell) = lookup.cell()
    {
        let b = data.cell_bounds(cell);
        let color = match lookup {
            CellLookup::Invalid { .. } => CELL_INVALID,
            _ => CELL,
        };
        lines.aabb(&b, color);
        // A cross through the camera so the cell reads at a distance.
        let p = camera.position;
        let r = data.cell_size * 0.15;
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            lines.line(p - axis * r, p + axis * r, CELL);
        }
    }
    if window.hide_culled && settings.occlusion_culling {
        let result = draws.occlusion(scene, &data, camera.position, false);
        environment.occlusion = Some(result.mask);
    }
}

/// Summary box in the Scene view's bottom-left corner while visualizing.
pub fn occlusion_overlay(ui: &egui::Ui, rect: egui::Rect, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let font = egui::FontId::monospace(11.0);
    let height = 10.0 + lines.len() as f32 * 14.0 + 14.0;
    let r = egui::Rect::from_min_size(
        rect.left_bottom() + egui::vec2(8.0, -height - 8.0),
        egui::vec2(340.0, height),
    );
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(
        r,
        4.0,
        egui::Color32::from_rgba_unmultiplied(20, 20, 20, 200),
    );
    let mut at = r.left_top() + egui::vec2(8.0, 5.0);
    painter.text(
        at,
        egui::Align2::LEFT_TOP,
        "Occlusion Culling",
        font.clone(),
        egui::Color32::from_rgb(120, 220, 255),
    );
    for line in lines {
        at.y += 14.0;
        painter.text(
            at,
            egui::Align2::LEFT_TOP,
            line,
            font.clone(),
            egui::Color32::from_gray(220),
        );
    }
}
