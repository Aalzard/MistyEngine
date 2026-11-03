//! Scene-view outlines of screen-space canvases and selected UI rects. An overlay canvas sits in
//! the world at one unit per screen pixel, lower-left corner at the origin, facing −Z.

use glam::{Vec2, Vec3};
use misty_engine::scene::Scene;
use misty_engine::ui::{Rect, UiLayout, layout};

use crate::gizmos::Lines;
use crate::selection::Selection;

const CANVAS: [f32; 4] = [0.85, 0.85, 0.85, 0.6];
const SELECTED: [f32; 4] = [0.35, 0.65, 1.0, 1.0];
const ANCHOR: [f32; 4] = [1.0, 1.0, 1.0, 0.9];

/// World position of a canvas point (canvas units × scale = pixels, y up).
fn world(p: Vec2, scale: f32) -> Vec3 {
    (p * scale).extend(0.0)
}

fn rect(lines: &mut Lines, r: Rect, scale: f32, color: [f32; 4]) {
    let c = [
        r.min,
        Vec2::new(r.max.x, r.min.y),
        r.max,
        Vec2::new(r.min.x, r.max.y),
    ];
    for i in 0..4 {
        lines.line(world(c[i], scale), world(c[(i + 1) % 4], scale), color);
    }
}

/// Canvas rects for the game screen size `screen`, plus the selected UI objects' rects and
/// anchor positions.
pub fn ui_lines(lines: &mut Lines, scene: &Scene, selection: &Selection, screen: Vec2) {
    let l: UiLayout = layout(scene, screen);
    for n in &l.nodes {
        if n.entity == n.canvas {
            rect(lines, n.rect, n.scale, CANVAS);
        }
        if !selection.contains(n.entity) || n.entity == n.canvas {
            continue;
        }
        rect(lines, n.rect, n.scale, SELECTED);
        // Anchor markers: small crosses at the anchor corners inside the parent rect.
        let Some(parent) = scene.parent(n.entity).and_then(|p| l.node(p)) else {
            continue;
        };
        let Ok(rt) = scene
            .world
            .get::<&misty_engine::scene::RectTransform>(n.entity)
        else {
            continue;
        };
        let ps = parent.rect.size();
        let arm = 6.0 / n.scale;
        for a in [
            rt.anchor_min,
            rt.anchor_max,
            Vec2::new(rt.anchor_min.x, rt.anchor_max.y),
            Vec2::new(rt.anchor_max.x, rt.anchor_min.y),
        ] {
            let p = parent.rect.min + a * ps;
            lines.line(
                world(p - Vec2::new(arm, 0.0), n.scale),
                world(p + Vec2::new(arm, 0.0), n.scale),
                ANCHOR,
            );
            lines.line(
                world(p - Vec2::new(0.0, arm), n.scale),
                world(p + Vec2::new(0.0, arm), n.scale),
                ANCHOR,
            );
        }
    }
}

/// Centre and radius of a UI object's rect in the Scene view (for Frame Selected).
pub fn ui_bounds(
    scene: &Scene,
    e: misty_engine::hecs::Entity,
    screen: Vec2,
) -> Option<(Vec3, f32)> {
    scene
        .world
        .get::<&misty_engine::scene::RectTransform>(e)
        .ok()?;
    let l = layout(scene, screen);
    let n = l.node(e)?;
    let r = n.rect.scaled(n.scale);
    Some((r.center().extend(0.0), (r.size().length() * 0.5).max(1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use misty_engine::scene::{Canvas, RectTransform};

    #[test]
    fn canvas_and_selected_rects_are_outlined() {
        let mut scene = Scene::new("UI");
        let canvas = scene.spawn("Canvas", None);
        scene.add(canvas, Canvas::default());
        let image = scene.spawn("Image", Some(canvas));
        scene.add(image, RectTransform::sized(100.0, 50.0));
        let mut selection = Selection::default();
        let mut lines = Lines::default();
        ui_lines(&mut lines, &scene, &selection, Vec2::new(800.0, 600.0));
        assert_eq!(lines.0.len(), 8, "canvas outline only");
        selection.set(image);
        let mut lines = Lines::default();
        ui_lines(&mut lines, &scene, &selection, Vec2::new(800.0, 600.0));
        assert_eq!(lines.0.len(), 8 + 8 + 4 * 4);
        let (c, r) = ui_bounds(&scene, image, Vec2::new(800.0, 600.0)).unwrap();
        assert_eq!(c, Vec3::new(400.0, 300.0, 0.0));
        assert!(r > 50.0);
    }
}
