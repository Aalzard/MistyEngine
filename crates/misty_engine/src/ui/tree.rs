//! Lay out every canvas in a scene for one screen size: rects in canvas units and screen pixels,
//! in draw order (canvas sort order, then Hierarchy order). Layout groups, Content Size Fitters,
//! widget-driven anchors and masks are applied here.

use std::collections::HashMap;

use glam::Vec2;
use hecs::Entity;

use super::auto_layout;
use super::driven::driven_anchors;
use super::layout::{Rect, scale_factor};
use crate::scene::{Canvas, CanvasScaler, RectTransform, Scene, UiMask, UiRectMask2D};

/// One laid-out UI object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiNode {
    pub entity: Entity,
    /// Root canvas the object belongs to.
    pub canvas: Entity,
    /// Canvas units, y up, origin at the canvas' lower-left corner.
    pub rect: Rect,
    /// Screen pixels, y down, origin top-left (what is drawn and clicked).
    pub screen: Rect,
    /// Screen pixels per canvas unit.
    pub scale: f32,
    /// Screen rect a parent Mask / Rect Mask 2D clips this object to.
    pub clip: Option<Rect>,
}

/// All active canvases of a scene laid out for one screen.
#[derive(Clone, Debug, Default)]
pub struct UiLayout {
    pub screen: Vec2,
    /// Canvas roots first, then their objects; back to front.
    pub nodes: Vec<UiNode>,
}

impl UiLayout {
    pub fn node(&self, e: Entity) -> Option<&UiNode> {
        self.nodes.iter().find(|n| n.entity == e)
    }
}

/// The canvas `e` sits under (itself included), if any.
pub fn canvas_of(scene: &Scene, e: Entity) -> Option<Entity> {
    let mut cur = Some(e);
    let mut found = None;
    while let Some(c) = cur {
        if scene.world.get::<&Canvas>(c).is_ok() {
            found = Some(c);
        }
        cur = scene.parent(c);
    }
    // The outermost canvas is the root (nested canvases just add a rect).
    found
}

/// Lay out the scene's canvases for a `screen` of this many pixels.
pub fn layout(scene: &Scene, screen: Vec2) -> UiLayout {
    let mut out = UiLayout {
        screen,
        nodes: Vec::new(),
    };
    if screen.x < 1.0 || screen.y < 1.0 {
        return out;
    }
    // Root canvases: no canvas above them.
    let mut roots: Vec<(i32, usize, Entity)> = scene
        .walk()
        .into_iter()
        .enumerate()
        .filter_map(|(i, (e, _))| {
            let c = scene.world.get::<&Canvas>(e).ok()?;
            let nested = scene
                .parent(e)
                .is_some_and(|p| canvas_of(scene, p).is_some());
            (!nested && c.enabled && scene.active_in_hierarchy(e)).then_some((c.sort_order, i, e))
        })
        .collect();
    roots.sort();
    for (_, _, canvas) in roots {
        let scaler = scene
            .world
            .get::<&CanvasScaler>(canvas)
            .ok()
            .map(|s| (*s).clone());
        let scale = scale_factor(scaler.as_ref(), screen);
        let rect = Rect::from_size(screen / scale);
        let mut ctx = Ctx {
            scene,
            canvas,
            scale,
            anchors: driven_anchors(scene),
            out: &mut out,
        };
        ctx.visit(canvas, rect, None);
    }
    out
}

struct Ctx<'a> {
    scene: &'a Scene,
    canvas: Entity,
    scale: f32,
    /// Anchors set by sliders and scrollbars on their parts.
    anchors: HashMap<Entity, (Vec2, Vec2)>,
    out: &'a mut UiLayout,
}

impl Ctx<'_> {
    fn visit(&mut self, e: Entity, rect: Rect, clip: Option<Rect>) {
        let scene = self.scene;
        let screen = rect.scaled(self.scale).flip_y(self.out.screen.y);
        self.out.nodes.push(UiNode {
            entity: e,
            canvas: self.canvas,
            rect,
            screen,
            scale: self.scale,
            clip,
        });
        let child_clip = match self.mask_rect(e, screen) {
            Some(m) => Some(clip.map_or(m, |c| c.intersect(&m))),
            None => clip,
        };
        // A layout group places its children; the rest follow their Rect Transforms.
        let placed: HashMap<Entity, Rect> = auto_layout::arrange(scene, e, rect)
            .unwrap_or_default()
            .into_iter()
            .collect();
        for child in scene.children(e) {
            if !scene.game_object(child).active {
                continue;
            }
            let r = match placed.get(&child) {
                Some(r) => *r,
                None => match scene.world.get::<&RectTransform>(child) {
                    Ok(rt) => {
                        let mut rt = *rt;
                        if let Some((min, max)) = self.anchors.get(&child) {
                            rt.anchor_min = *min;
                            rt.anchor_max = *max;
                        }
                        rt.rect_in(rect)
                    }
                    // Objects without a Rect Transform take their parent's rect.
                    Err(_) => rect,
                },
            };
            let r = auto_layout::fit(scene, child, r);
            self.visit(child, r, child_clip);
        }
    }

    /// Screen rect `e` clips its children to, if it is a mask.
    fn mask_rect(&self, e: Entity, screen: Rect) -> Option<Rect> {
        let w = &self.scene.world;
        if w.get::<&UiMask>(e).is_ok_and(|m| m.enabled) {
            return Some(screen);
        }
        let m = w.get::<&UiRectMask2D>(e).ok().filter(|m| m.enabled)?;
        let [l, b, r, t] = m.padding.map(|v| v * self.scale);
        Some(Rect::new(
            screen.min + Vec2::new(l, t),
            screen.max - Vec2::new(r, b),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{CanvasScaler, UiScaleMode};

    #[test]
    fn canvas_children_are_laid_out_in_pixels() {
        let mut scene = Scene::new("UI");
        let canvas = scene.spawn("Canvas", None);
        scene.add(canvas, Canvas::default());
        scene.add(canvas, RectTransform::default());
        scene.add(
            canvas,
            CanvasScaler {
                mode: UiScaleMode::ScaleWithScreenSize,
                reference_resolution: Vec2::new(800.0, 600.0),
                ..Default::default()
            },
        );
        let button = scene.spawn("Button", Some(canvas));
        scene.add(button, RectTransform::sized(160.0, 30.0));
        let label = scene.spawn("Text", Some(button));
        scene.add(label, RectTransform::stretched());

        let l = layout(&scene, Vec2::new(1600.0, 1200.0));
        assert_eq!(l.nodes.len(), 3);
        let b = l.node(button).unwrap();
        assert_eq!(b.scale, 2.0);
        assert_eq!(b.rect.min, Vec2::new(320.0, 285.0));
        // 2x scale, y flipped: top edge at 1200 - 315 * 2.
        assert_eq!(b.screen.min, Vec2::new(640.0, 570.0));
        assert_eq!(b.screen.size(), Vec2::new(320.0, 60.0));
        assert_eq!(l.node(label).unwrap().screen, b.screen);

        scene.game_object_mut(button).active = false;
        assert_eq!(layout(&scene, Vec2::new(800.0, 600.0)).nodes.len(), 1);
    }

    #[test]
    fn higher_sort_order_draws_last_and_nested_canvases_join_their_root() {
        let mut scene = Scene::new("UI");
        let a = scene.spawn("A", None);
        scene.add(
            a,
            Canvas {
                sort_order: 5,
                ..Default::default()
            },
        );
        let b = scene.spawn("B", None);
        scene.add(b, Canvas::default());
        let nested = scene.spawn("Nested", Some(b));
        scene.add(nested, Canvas::default());
        let l = layout(&scene, Vec2::new(100.0, 100.0));
        let order: Vec<Entity> = l.nodes.iter().map(|n| n.entity).collect();
        assert_eq!(order, [b, nested, a]);
        assert_eq!(l.node(nested).unwrap().canvas, b);
        assert_eq!(canvas_of(&scene, nested), Some(b));
    }
}
