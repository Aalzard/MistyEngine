//! Lay out every canvas in a scene for one view: rects in canvas units and canvas pixels, in
//! draw order (world-placed canvases far to near, then overlays by sort order, each in
//! Hierarchy order). Layout groups, Content Size Fitters, Aspect Ratio Fitters, widget-driven
//! anchors, masks, Canvas Groups and the Transform's rotation / scale are applied here.

use std::collections::HashMap;

use glam::{Affine2, EulerRot, Vec2};
use hecs::Entity;

use super::auto_layout;
use super::canvas::{CanvasPlacement, UiView, place, world_canvas_size};
use super::driven::driven_anchors;
use super::layout::{Rect, scale_factor};
use crate::scene::{
    AspectMode, AspectRatioFitter, Canvas, CanvasGroup, CanvasRenderMode, CanvasScaler,
    RectTransform, Scene, UiMask, UiRectMask2D,
};

/// One laid-out UI object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiNode {
    pub entity: Entity,
    /// Root canvas the object belongs to.
    pub canvas: Entity,
    /// Canvas units, y up, origin at the canvas' lower-left corner.
    pub rect: Rect,
    /// Canvas pixels, y down, origin top-left: what is drawn and clicked (an overlay's canvas
    /// pixels are screen pixels), before `xf`.
    pub screen: Rect,
    /// Canvas pixels per canvas unit.
    pub scale: f32,
    /// Canvas pixel rect a parent Mask / Rect Mask 2D clips this object to.
    pub clip: Option<Rect>,
    /// Rotation and scale of this object and its parents (their Transforms, about each pivot)
    /// in canvas pixels: drawn positions are `xf` applied to `screen`.
    pub xf: Affine2,
    /// Canvas Group alpha (product of the groups above, this object's included).
    pub alpha: f32,
    /// No Canvas Group above turns interaction off.
    pub interactable: bool,
    /// No Canvas Group above lets the pointer through.
    pub blocks_raycasts: bool,
}

impl UiNode {
    /// Does canvas pixel `p` fall on this object (its rect, rotation and mask)?
    pub fn contains(&self, p: Vec2) -> bool {
        let q = self.local(p);
        self.screen.contains(q) && self.clip.is_none_or(|c| c.contains(q))
    }

    /// Canvas pixel `p` in this object's unrotated canvas pixels.
    pub fn local(&self, p: Vec2) -> Vec2 {
        if self.xf == Affine2::IDENTITY {
            p
        } else {
            self.xf.inverse().transform_point2(p)
        }
    }

    /// Corners as drawn (canvas pixels): top-left, top-right, bottom-right, bottom-left.
    pub fn corners(&self) -> [Vec2; 4] {
        let (a, b) = (self.screen.min, self.screen.max);
        [a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)].map(|p| self.xf.transform_point2(p))
    }
}

/// All active canvases of a scene laid out for one view.
#[derive(Clone, Debug, Default)]
pub struct UiLayout {
    /// The screen size screen-space canvases are laid out for.
    pub screen: Vec2,
    /// Pixel size of the image drawn into.
    pub target: Vec2,
    /// Canvas roots first, then their objects; back to front.
    pub nodes: Vec<UiNode>,
    /// Where each root canvas sits, in draw order.
    pub canvases: Vec<CanvasPlacement>,
}

impl UiLayout {
    pub fn node(&self, e: Entity) -> Option<&UiNode> {
        self.nodes.iter().find(|n| n.entity == e)
    }

    pub fn placement(&self, canvas: Entity) -> Option<&CanvasPlacement> {
        self.canvases.iter().find(|c| c.canvas == canvas)
    }

    /// The pixel of `canvas` under image pixel `p` (an overlay's pixels are the image's).
    pub fn to_canvas(&self, canvas: Entity, p: Vec2) -> Option<Vec2> {
        match self.placement(canvas) {
            Some(c) => c.to_canvas(p, self.target),
            None => Some(p),
        }
    }

    /// The image pixel showing pixel `p` of `canvas`.
    pub fn to_target(&self, canvas: Entity, p: Vec2) -> Option<Vec2> {
        match self.placement(canvas) {
            Some(c) => c.to_target(p, self.target),
            None => Some(p),
        }
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

/// Lay out the scene's canvases for a game `screen` of this many pixels (world canvases seen by
/// the game camera).
pub fn layout(scene: &Scene, screen: Vec2) -> UiLayout {
    layout_view(scene, &UiView::game(scene, screen))
}

/// Lay out the scene's canvases for `view`.
pub fn layout_view(scene: &Scene, view: &UiView) -> UiLayout {
    let screen = view.screen;
    let mut out = UiLayout {
        screen,
        target: view.target,
        nodes: Vec::new(),
        canvases: Vec::new(),
    };
    if screen.x < 1.0 || screen.y < 1.0 {
        return out;
    }
    // Root canvases: no canvas above them.
    let mut roots: Vec<(usize, Entity, Canvas)> = scene
        .walk()
        .into_iter()
        .enumerate()
        .filter_map(|(i, (e, _))| {
            let c = scene.world.get::<&Canvas>(e).ok()?;
            let nested = scene
                .parent(e)
                .is_some_and(|p| canvas_of(scene, p).is_some());
            (!nested && c.enabled && scene.active_in_hierarchy(e)).then(|| (i, e, (*c).clone()))
        })
        .collect();
    let anchors = driven_anchors(scene);
    let mut placed = Vec::new();
    for (i, canvas, c) in roots.drain(..) {
        let scaler = scene
            .world
            .get::<&CanvasScaler>(canvas)
            .ok()
            .map(|s| (*s).clone());
        let (scale, rect) = if c.render_mode == CanvasRenderMode::WorldSpace {
            let rt = scene.world.get::<&RectTransform>(canvas).ok().map(|r| *r);
            let ppu = scaler
                .as_ref()
                .filter(|s| s.enabled)
                .map_or(1.0, |s| s.dynamic_pixels_per_unit.max(0.01));
            (ppu, Rect::from_size(world_canvas_size(rt.as_ref())))
        } else {
            let scale = scale_factor(scaler.as_ref(), screen);
            (scale, Rect::from_size(screen / scale))
        };
        let size_px = rect.size() * scale;
        let Some(p) = place(scene, view, canvas, &c, size_px, scale) else {
            continue;
        };
        placed.push((i, p, rect, scale));
    }
    // World-placed canvases first (sort order, then far to near), then overlays (sort order,
    // then Hierarchy order).
    placed.sort_by(|a, b| {
        let key = |p: &CanvasPlacement| (!p.in_world(), p.sort_order);
        key(&a.1).cmp(&key(&b.1)).then_with(|| {
            if a.1.in_world() {
                b.1.distance.total_cmp(&a.1.distance)
            } else {
                a.0.cmp(&b.0)
            }
        })
    });
    for (_, p, rect, scale) in placed {
        out.canvases.push(p);
        let mut ctx = Ctx {
            scene,
            canvas: p.canvas,
            scale,
            height: p.size.y,
            anchors: &anchors,
            out: &mut out,
        };
        ctx.visit(
            p.canvas,
            rect,
            None,
            Affine2::IDENTITY,
            Group::default(),
            true,
        );
    }
    out
}

/// Canvas Group state passed down the hierarchy.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Group {
    alpha: f32,
    interactable: bool,
    blocks: bool,
}

impl Default for Group {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            interactable: true,
            blocks: true,
        }
    }
}

impl Group {
    /// This group combined with `e`'s Canvas Group, if it has an enabled one.
    fn with(self, scene: &Scene, e: Entity) -> Self {
        let Some(g) = scene
            .world
            .get::<&CanvasGroup>(e)
            .ok()
            .filter(|g| g.enabled)
        else {
            return self;
        };
        let base = if g.ignore_parent_groups {
            Self::default()
        } else {
            self
        };
        Self {
            alpha: base.alpha * g.alpha.clamp(0.0, 1.0),
            interactable: base.interactable && g.interactable,
            blocks: base.blocks && g.blocks_raycasts,
        }
    }
}

/// Is `e` interactable as far as the Canvas Groups above it (itself included) are concerned?
pub fn groups_allow_interaction(scene: &Scene, e: Entity) -> bool {
    let mut cur = Some(e);
    while let Some(x) = cur {
        if let Ok(g) = scene.world.get::<&CanvasGroup>(x)
            && g.enabled
        {
            if !g.interactable {
                return false;
            }
            if g.ignore_parent_groups {
                return true;
            }
        }
        cur = scene.parent(x);
    }
    true
}

/// `rect` resized by an Aspect Ratio Fitter on `e` (inside `parent`), keeping the pivot point.
pub fn aspect_fit(scene: &Scene, e: Entity, rect: Rect, parent: Rect) -> Rect {
    let Some(f) = scene
        .world
        .get::<&AspectRatioFitter>(e)
        .ok()
        .filter(|f| f.enabled && f.aspect_mode != AspectMode::None)
        .map(|f| (*f).clone())
    else {
        return rect;
    };
    let pivot = scene
        .world
        .get::<&RectTransform>(e)
        .map_or(Vec2::splat(0.5), |rt| rt.pivot);
    fit_aspect(f.aspect_mode, f.aspect_ratio, rect, parent, pivot)
}

/// The Aspect Ratio Fitter maths: `rect` resized to `ratio` (width / height) by `mode`.
/// Width / Height Controls keep the pivot point where it was; Fit In / Envelope Parent size the
/// rect from the parent and line their pivots up (like anchors stretched over the parent).
pub fn fit_aspect(mode: AspectMode, ratio: f32, rect: Rect, parent: Rect, pivot: Vec2) -> Rect {
    let ratio = ratio.clamp(0.001, 1000.0);
    let (size, anchor) = match mode {
        AspectMode::None => return rect,
        AspectMode::WidthControlsHeight => (
            Vec2::new(rect.size().x, rect.size().x / ratio),
            rect.min + rect.size() * pivot,
        ),
        AspectMode::HeightControlsWidth => (
            Vec2::new(rect.size().y * ratio, rect.size().y),
            rect.min + rect.size() * pivot,
        ),
        AspectMode::FitInParent | AspectMode::EnvelopeParent => {
            let p = parent.size();
            let by_width = Vec2::new(p.x, p.x / ratio);
            let by_height = Vec2::new(p.y * ratio, p.y);
            let fits = by_width.y <= p.y;
            let size = match (mode, fits) {
                (AspectMode::FitInParent, true) | (AspectMode::EnvelopeParent, false) => by_width,
                _ => by_height,
            };
            (size, parent.min + p * pivot)
        }
    };
    let min = anchor - size * pivot;
    Rect::new(min, min + size)
}

struct Ctx<'a> {
    scene: &'a Scene,
    canvas: Entity,
    scale: f32,
    /// Canvas pixel height (y is flipped inside it).
    height: f32,
    /// Anchors set by sliders and scrollbars on their parts.
    anchors: &'a HashMap<Entity, (Vec2, Vec2)>,
    out: &'a mut UiLayout,
}

impl Ctx<'_> {
    fn visit(
        &mut self,
        e: Entity,
        rect: Rect,
        clip: Option<Rect>,
        parent_xf: Affine2,
        group: Group,
        root: bool,
    ) {
        let scene = self.scene;
        let screen = rect.scaled(self.scale).flip_y(self.height);
        // A root canvas is placed by its mode; everything else may turn and scale (Transform).
        let xf = if root {
            parent_xf
        } else {
            parent_xf * self.local_xf(e, screen)
        };
        let group = group.with(scene, e);
        self.out.nodes.push(UiNode {
            entity: e,
            canvas: self.canvas,
            rect,
            screen,
            scale: self.scale,
            clip,
            xf,
            alpha: group.alpha,
            interactable: group.interactable,
            blocks_raycasts: group.blocks,
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
            let r = aspect_fit(scene, child, r, rect);
            self.visit(child, r, child_clip, xf, group, false);
        }
    }

    /// Rotation (about z) and x / y scale of `e`'s Transform about its pivot, in canvas pixels.
    fn local_xf(&self, e: Entity, screen: Rect) -> Affine2 {
        let Ok(t) = self.scene.world.get::<&crate::scene::Transform>(e) else {
            return Affine2::IDENTITY;
        };
        let (_, _, angle) = t.rotation.to_euler(EulerRot::YXZ);
        let scale = t.scale.truncate();
        if angle.abs() < 1e-6 && scale == Vec2::ONE {
            return Affine2::IDENTITY;
        }
        let pivot = self
            .scene
            .world
            .get::<&RectTransform>(e)
            .map_or(Vec2::splat(0.5), |rt| rt.pivot);
        // Canvas pixels run down: the pivot is measured from the bottom edge and a turn that is
        // anticlockwise on screen is negative here.
        let p = Vec2::new(
            screen.min.x + screen.size().x * pivot.x,
            screen.max.y - screen.size().y * pivot.y,
        );
        Affine2::from_translation(p)
            * Affine2::from_angle(-angle)
            * Affine2::from_scale(scale)
            * Affine2::from_translation(-p)
    }

    /// Canvas pixel rect `e` clips its children to, if it is a mask.
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
