//! UI in the Scene view: every canvas is laid out in the world (an overlay canvas at one unit
//! per screen pixel, lower-left corner at the origin, facing −Z; camera canvases in front of
//! their camera; world canvases where they are), drawn by the UI pass (`viewport`), outlined on
//! top here, clickable to select, and with the Visualize Navigation arrows. The Rect tool's
//! handles are in `ui_rect_tool`.

use egui::{Color32, Pos2, Rect, Stroke, Ui};
use glam::{Vec2, Vec3};
use misty_engine::camera::{Camera, Projection};
use misty_engine::hecs::Entity;
use misty_engine::scene::{RectTransform, Scene, UiImage, UiRawImage, UiText};
use misty_engine::ui::{UiLayout, UiNode, UiView, layout_view};

use crate::state::EditorState;

const CANVAS: Color32 = Color32::from_rgba_premultiplied(200, 200, 200, 150);
const SELECTED: Color32 = Color32::from_rgb(90, 165, 255);
const NAV: Color32 = Color32::from_rgb(255, 230, 60);

/// Visualize Navigation (the toggle beside a selectable's Navigation in the Inspector).
static VISUALIZE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Is Visualize Navigation on?
pub fn visualizing_navigation() -> bool {
    VISUALIZE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Turn Visualize Navigation on or off.
pub fn set_visualizing_navigation(on: bool) {
    VISUALIZE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The game screen size UI is laid out for (the Game view's, or 1920×1080 before it drew).
pub fn game_screen(state: &EditorState) -> Vec2 {
    let s = state.game_view.screen_px;
    if s.x >= 1.0 && s.y >= 1.0 {
        s
    } else {
        Vec2::new(1920.0, 1080.0)
    }
}

/// The scene's canvases laid out in the world and seen by the Scene view `camera` through a
/// view of `rect` (egui points: layout pixels are points here).
pub fn scene_layout(state: &EditorState, camera: &Camera, rect: Rect) -> UiLayout {
    let view = UiView::scene_view(
        game_screen(state),
        Vec2::new(rect.width().max(1.0), rect.height().max(1.0)),
        *camera,
    );
    layout_view(&state.scene, &view)
}

/// Canvas pixel `p` of `canvas` on screen (`None` behind the camera).
pub fn to_screen(l: &UiLayout, rect: Rect, canvas: Entity, p: Vec2) -> Option<Pos2> {
    let t = l.to_target(canvas, p)?;
    Some(rect.min + egui::vec2(t.x, t.y))
}

/// The canvas pixel of `canvas` under screen point `at`.
pub fn to_canvas(l: &UiLayout, rect: Rect, canvas: Entity, at: Pos2) -> Option<Vec2> {
    l.to_canvas(canvas, Vec2::new(at.x - rect.min.x, at.y - rect.min.y))
}

/// A node's four drawn corners on screen (top-left, top-right, bottom-right, bottom-left).
pub fn screen_corners(l: &UiLayout, rect: Rect, n: &UiNode) -> Option<[Pos2; 4]> {
    let c = n.corners();
    Some([
        to_screen(l, rect, n.canvas, c[0])?,
        to_screen(l, rect, n.canvas, c[1])?,
        to_screen(l, rect, n.canvas, c[2])?,
        to_screen(l, rect, n.canvas, c[3])?,
    ])
}

fn outline(painter: &egui::Painter, c: [Pos2; 4], stroke: Stroke) {
    for i in 0..4 {
        painter.line_segment([c[i], c[(i + 1) % 4]], stroke);
    }
}

/// Canvas outlines, and the selected UI objects' rects and anchors.
pub fn paint(ui: &Ui, rect: Rect, state: &EditorState, l: &UiLayout) {
    let painter = ui.painter().with_clip_rect(rect);
    for n in &l.nodes {
        let selected = state.selection.contains(n.entity);
        if n.entity == n.canvas
            && let Some(c) = screen_corners(l, rect, n)
        {
            outline(&painter, c, Stroke::new(1.0, CANVAS));
        }
        if !selected || n.entity == n.canvas {
            continue;
        }
        if let Some(c) = screen_corners(l, rect, n) {
            outline(&painter, c, Stroke::new(1.5, SELECTED));
        }
        // Anchor markers when the Rect tool doesn't draw its own.
        if state.tool != crate::state::Tool::Rect {
            for p in anchor_points(&state.scene, l, n).into_iter().flatten() {
                if let Some(s) = to_screen(l, rect, n.canvas, p) {
                    painter.circle_stroke(s, 3.0, Stroke::new(1.0, Color32::WHITE));
                }
            }
        }
    }
}

/// The four anchor corners of `n` in canvas pixels (min-min, max-min, min-max, max-max with y
/// up), inside its parent's rect.
pub fn anchor_points(scene: &Scene, l: &UiLayout, n: &UiNode) -> Option<[Vec2; 4]> {
    let parent = scene.parent(n.entity).and_then(|p| l.node(p))?;
    let rt = scene.world.get::<&RectTransform>(n.entity).ok()?;
    let (s, xf) = (parent.screen, parent.xf);
    let at = |a: Vec2| {
        xf.transform_point2(Vec2::new(
            s.min.x + a.x * s.size().x,
            s.max.y - a.y * s.size().y,
        ))
    };
    let (lo, hi) = (rt.anchor_min, rt.anchor_max);
    Some([
        at(lo),
        at(Vec2::new(hi.x, lo.y)),
        at(Vec2::new(lo.x, hi.y)),
        at(hi),
    ])
}

/// Visualize Navigation: an arrow from each selectable to where each move goes.
pub fn paint_navigation(ui: &Ui, rect: Rect, scene: &Scene, l: &UiLayout) {
    let painter = ui.painter().with_clip_rect(rect);
    let centre = |e: Entity| {
        let n = l.node(e)?;
        let c = n.corners();
        to_screen(l, rect, n.canvas, (c[0] + c[2]) * 0.5)
    };
    for (from, to, dir) in misty_engine::ui::navigation::links(scene, l) {
        let (Some(a), Some(b)) = (centre(from), centre(to)) else {
            continue;
        };
        // Offset each direction a little so the two ways of a pair don't overlap.
        let side = match dir {
            misty_engine::ui::navigation::NavDir::Up => egui::vec2(-4.0, 0.0),
            misty_engine::ui::navigation::NavDir::Down => egui::vec2(4.0, 0.0),
            misty_engine::ui::navigation::NavDir::Left => egui::vec2(0.0, 4.0),
            misty_engine::ui::navigation::NavDir::Right => egui::vec2(0.0, -4.0),
        };
        let (a, b) = (a + side, b + side);
        painter.arrow(a, (b - a) * 0.85, Stroke::new(1.5, NAV));
    }
}

/// Does `e` draw something (an enabled Image, Raw Image or Text)?
fn has_graphic(scene: &Scene, e: Entity) -> bool {
    let w = &scene.world;
    w.get::<&UiImage>(e).is_ok_and(|i| i.enabled)
        || w.get::<&UiRawImage>(e).is_ok_and(|i| i.enabled)
        || w.get::<&UiText>(e).is_ok_and(|t| t.enabled)
}

/// The topmost UI graphic under screen point `at` and its distance from the camera.
pub fn pick(
    scene: &Scene,
    l: &UiLayout,
    camera: &Camera,
    rect: Rect,
    at: Pos2,
) -> Option<(Entity, f32)> {
    l.nodes.iter().rev().find_map(|n| {
        if n.alpha <= 0.0 || !has_graphic(scene, n.entity) {
            return None;
        }
        let q = to_canvas(l, rect, n.canvas, at)?;
        if !n.contains(q) {
            return None;
        }
        let w = l.placement(n.canvas)?.world_point(q);
        Some((n.entity, w.distance(camera.position)))
    })
}

/// Centre and radius of a UI object's rect in the Scene view's world (for Frame Selected).
pub fn ui_bounds(scene: &Scene, e: Entity, screen: Vec2) -> Option<(Vec3, f32)> {
    scene.world.get::<&RectTransform>(e).ok()?;
    // Any camera will do: only the world placement matters here.
    let camera = Camera {
        position: Vec3::new(0.0, 0.0, -10.0),
        rotation: glam::Quat::IDENTITY,
        projection: Projection::Perspective { fov_y: 60.0 },
        near: 0.1,
        far: 1000.0,
    };
    let l = layout_view(scene, &UiView::scene_view(screen, screen, camera));
    let n = l.node(e)?;
    let p = l.placement(n.canvas)?;
    let c = n.corners().map(|q| p.world_point(q));
    let centre = (c[0] + c[2]) * 0.5;
    let r = (c[0] - c[2]).length() * 0.5;
    Some((centre, r.max(0.01)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use misty_engine::scene::{Canvas, CanvasRenderMode};

    #[test]
    fn overlay_canvases_stand_in_the_world_and_world_canvases_where_they_are() {
        let mut scene = Scene::new("UI");
        let canvas = scene.spawn("Canvas", None);
        scene.add(canvas, Canvas::default());
        let image = scene.spawn("Image", Some(canvas));
        scene.add(image, RectTransform::sized(100.0, 50.0));
        let screen = Vec2::new(800.0, 600.0);
        let (c, r) = ui_bounds(&scene, image, screen).unwrap();
        // The default scaler matches 800 wide: one unit per pixel, centred on the canvas.
        assert!((c - Vec3::new(400.0, 300.0, 0.0)).length() < 1e-3, "{c}");
        assert!(r > 50.0);
        // A World Space canvas of 200×100 at 0.01 scale, 5 units up.
        let world = scene.spawn("World", None);
        scene.add(
            world,
            Canvas {
                render_mode: CanvasRenderMode::WorldSpace,
                ..Default::default()
            },
        );
        scene.add(world, RectTransform::sized(200.0, 100.0));
        let mut t = scene.transform(world);
        t.position = Vec3::new(0.0, 5.0, 0.0);
        t.scale = Vec3::splat(0.01);
        scene.set_transform(world, t);
        let (c, r) = ui_bounds(&scene, world, screen).unwrap();
        assert!((c - Vec3::new(0.0, 5.0, 0.0)).length() < 1e-4, "{c}");
        assert!(
            (r - (2.0f32.powi(2) + 1.0).sqrt() * 0.5).abs() < 1e-3,
            "{r}"
        );
    }
}
