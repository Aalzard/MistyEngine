//! Round 19 UI checks across layout, raycasts and drawing: Canvas Scaler modes, World Space and
//! Screen Space - Camera canvases (placement, hit tests, depth-tested batches), Canvas Groups,
//! the Aspect Ratio Fitter and rotated Rect Transforms.

use glam::{Quat, Vec2, Vec3};
use hecs::Entity;

use super::draw::build;
use super::input::raycast;
use super::layout::{Rect, scale_factor_at, sprite_pixels_per_unit};
use super::text::{GlyphAtlas, UiFont};
use super::tree::{fit_aspect, layout};
use crate::scene::{
    AspectMode, AspectRatioFitter, Camera as CameraComponent, Canvas, CanvasGroup,
    CanvasRenderMode, CanvasScaler, PhysicalUnit, RectTransform, Scene, ScreenMatchMode, Transform,
    UiButton, UiImage, UiScaleMode,
};
use crate::ui::{ButtonState, UiInteraction};

const SCREEN: Vec2 = Vec2::new(800.0, 600.0);

/// A game camera at (0, 0, -10) looking down +Z (60° vertical field of view).
fn camera(s: &mut Scene) -> Entity {
    let cam = s.spawn("Main Camera", None);
    s.add(cam, CameraComponent::default());
    s.set_transform(cam, Transform::from_position(Vec3::new(0.0, 0.0, -10.0)));
    cam
}

fn overlay(s: &mut Scene) -> Entity {
    let canvas = s.spawn("Canvas", None);
    s.add(canvas, RectTransform::stretched());
    s.add(canvas, Canvas::default());
    s.add(
        canvas,
        CanvasScaler {
            mode: UiScaleMode::ConstantPixelSize,
            ..Default::default()
        },
    );
    canvas
}

fn image(s: &mut Scene, parent: Entity, rt: RectTransform) -> Entity {
    let e = s.spawn("Image", Some(parent));
    s.add(e, rt);
    s.add(e, UiImage::default());
    e
}

#[test]
fn canvas_scaler_modes() {
    let mut s = CanvasScaler {
        reference_resolution: Vec2::new(800.0, 600.0),
        ..Default::default()
    };
    // A wide screen: twice the reference width, the same height.
    let wide = Vec2::new(1600.0, 600.0);
    s.screen_match_mode = ScreenMatchMode::Expand;
    assert!(
        (scale_factor_at(Some(&s), wide, 0.0) - 1.0).abs() < 1e-5,
        "smaller ratio"
    );
    s.screen_match_mode = ScreenMatchMode::Shrink;
    assert!(
        (scale_factor_at(Some(&s), wide, 0.0) - 2.0).abs() < 1e-5,
        "larger ratio"
    );
    // Constant Physical Size: points at 144 DPI = 2 px per point; unknown DPI uses the fallback.
    s.mode = UiScaleMode::ConstantPhysicalSize;
    assert!((scale_factor_at(Some(&s), wide, 144.0) - 2.0).abs() < 1e-5);
    assert!((scale_factor_at(Some(&s), wide, 0.0) - 96.0 / 72.0).abs() < 1e-5);
    s.physical_unit = PhysicalUnit::Centimeters;
    assert!((scale_factor_at(Some(&s), wide, 254.0) - 100.0).abs() < 1e-3);
    // Sprite pixels per canvas unit: 100 PPU sprites at the default reference are 1:1, a
    // multiplier of 2 halves borders; physical sizes count sprite pixels at their DPI.
    let px = CanvasScaler::default();
    assert!((sprite_pixels_per_unit(Some(&px), 100.0, 1.0) - 1.0).abs() < 1e-6);
    assert!((sprite_pixels_per_unit(Some(&px), 100.0, 2.0) - 2.0).abs() < 1e-6);
    assert!((sprite_pixels_per_unit(Some(&px), 50.0, 1.0) - 0.5).abs() < 1e-6);
    let pts = CanvasScaler {
        mode: UiScaleMode::ConstantPhysicalSize,
        ..Default::default()
    };
    // 96 DPI sprites on a points canvas: 96 / 72 sprite pixels per point.
    assert!((sprite_pixels_per_unit(Some(&pts), 100.0, 1.0) - 96.0 / 72.0).abs() < 1e-5);
}

#[test]
fn world_space_canvas_is_hit_through_the_camera() {
    let mut s = Scene::new("World UI");
    camera(&mut s);
    let canvas = s.spawn("World Canvas", None);
    s.add(
        canvas,
        Canvas {
            render_mode: CanvasRenderMode::WorldSpace,
            ..Default::default()
        },
    );
    // 400×200 canvas units at 0.01: a 4×2 m sign at the origin, facing the camera.
    s.add(canvas, RectTransform::sized(400.0, 200.0));
    s.set_transform(
        canvas,
        Transform {
            scale: Vec3::splat(0.01),
            ..Default::default()
        },
    );
    let panel = image(&mut s, canvas, RectTransform::stretched());
    let left = image(&mut s, canvas, {
        let mut rt = RectTransform::sized(100.0, 100.0);
        rt.anchored_position.x = -150.0;
        rt
    });
    let l = layout(&s, SCREEN);
    let p = l.placement(canvas).unwrap();
    assert!(p.in_world() && (p.distance - 10.0).abs() < 1e-3);
    assert_eq!(
        raycast(&l, &s, SCREEN * 0.5),
        Some(panel),
        "centre of the view"
    );
    // The sign is 4 m wide 10 m away: its left image (x −1.5 m) is left of centre.
    let half_w = 10.0 * (30f32.to_radians()).tan() * (SCREEN.x / SCREEN.y);
    let x = SCREEN.x * 0.5 * (1.0 - 1.5 / half_w);
    assert_eq!(raycast(&l, &s, Vec2::new(x, 300.0)), Some(left));
    assert_eq!(
        raycast(&l, &s, Vec2::new(5.0, 5.0)),
        None,
        "outside the sign"
    );
    // Its quads come out depth-tested in perspective (w = distance).
    let list = build(&l, &s, None, &UiFont::default(), &mut GlyphAtlas::new(64));
    assert!(list.batches.iter().all(|b| b.depth));
    assert!((list.vertices[0].pos[3] - 10.0).abs() < 1e-3);
    // Turned edge-on, it can't be hit; turned away (behind the camera), neither.
    s.set_transform(
        canvas,
        Transform {
            scale: Vec3::splat(0.01),
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            ..Default::default()
        },
    );
    assert_eq!(raycast(&layout(&s, SCREEN), &s, SCREEN * 0.5), None);
    s.set_transform(
        canvas,
        Transform {
            position: Vec3::new(0.0, 0.0, -20.0),
            scale: Vec3::splat(0.01),
            ..Default::default()
        },
    );
    assert_eq!(raycast(&layout(&s, SCREEN), &s, SCREEN * 0.5), None);
}

#[test]
fn camera_canvas_fills_the_view_at_its_plane_distance() {
    let mut s = Scene::new("Camera UI");
    let cam = camera(&mut s);
    let canvas = overlay(&mut s);
    *s.world.get::<&mut Canvas>(canvas).unwrap() = Canvas {
        render_mode: CanvasRenderMode::ScreenSpaceCamera,
        world_camera: Some(cam),
        plane_distance: 5.0,
        ..Default::default()
    };
    let corner = image(&mut s, canvas, {
        let mut rt = RectTransform::sized(100.0, 100.0);
        rt.anchor_min = Vec2::ZERO;
        rt.anchor_max = Vec2::ZERO;
        rt.pivot = Vec2::ZERO;
        rt
    });
    let l = layout(&s, SCREEN);
    let p = l.placement(canvas).unwrap();
    assert_eq!(p.mode, CanvasRenderMode::ScreenSpaceCamera);
    // Laid out like an overlay (same pixels), but 5 units in front of the camera.
    assert_eq!(l.node(corner).unwrap().screen.min, Vec2::new(0.0, 500.0));
    assert_eq!(raycast(&l, &s, Vec2::new(50.0, 550.0)), Some(corner));
    let world = p.world_point(Vec2::new(400.0, 300.0));
    assert!(
        (world - Vec3::new(0.0, 0.0, -5.0)).length() < 1e-3,
        "{world}"
    );
    let list = build(&l, &s, None, &UiFont::default(), &mut GlyphAtlas::new(64));
    assert!(list.batches[0].depth, "sorted with the scene");
    // Without its camera it falls back to an overlay.
    s.world.get::<&mut Canvas>(canvas).unwrap().world_camera = None;
    let l = layout(&s, SCREEN);
    assert_eq!(
        l.placement(canvas).unwrap().mode,
        CanvasRenderMode::ScreenSpaceOverlay
    );
    let list = build(&l, &s, None, &UiFont::default(), &mut GlyphAtlas::new(64));
    assert!(!list.batches[0].depth);
}

#[test]
fn world_canvases_draw_before_overlays_far_to_near() {
    let mut s = Scene::new("Order");
    camera(&mut s);
    let over = overlay(&mut s);
    let world = |s: &mut Scene, name: &str, z: f32| {
        let c = s.spawn(name, None);
        s.add(
            c,
            Canvas {
                render_mode: CanvasRenderMode::WorldSpace,
                ..Default::default()
            },
        );
        s.add(c, RectTransform::sized(100.0, 100.0));
        s.set_transform(c, Transform::from_position(Vec3::new(0.0, 0.0, z)));
        c
    };
    let near = world(&mut s, "Near", 0.0);
    let far = world(&mut s, "Far", 20.0);
    let l = layout(&s, SCREEN);
    let order: Vec<Entity> = l.canvases.iter().map(|c| c.canvas).collect();
    assert_eq!(order, [far, near, over]);
}

#[test]
fn canvas_groups_fade_disable_and_let_clicks_through() {
    let mut s = Scene::new("Groups");
    let canvas = overlay(&mut s);
    let back = image(&mut s, canvas, RectTransform::stretched());
    let panel = s.spawn("Panel", Some(canvas));
    s.add(panel, RectTransform::stretched());
    s.add(
        panel,
        CanvasGroup {
            alpha: 0.5,
            ..Default::default()
        },
    );
    let button = image(&mut s, panel, RectTransform::sized(200.0, 100.0));
    s.add(button, UiButton::default());
    let centre = SCREEN * 0.5;
    let l = layout(&s, SCREEN);
    assert_eq!(raycast(&l, &s, centre), Some(button));
    // Alpha multiplies the graphics below the group.
    let list = build(&l, &s, None, &UiFont::default(), &mut GlyphAtlas::new(64));
    assert_eq!(
        list.vertices[0].color[3], 1.0,
        "the background isn't in the group"
    );
    assert_eq!(list.vertices[6].color[3], 0.5);
    // Blocks Raycasts off: the pointer reaches the background.
    s.world
        .get::<&mut CanvasGroup>(panel)
        .unwrap()
        .blocks_raycasts = false;
    assert_eq!(raycast(&layout(&s, SCREEN), &s, centre), Some(back));
    // Interactable off: the button shows Disabled and can't be clicked.
    s.world
        .get::<&mut CanvasGroup>(panel)
        .unwrap()
        .blocks_raycasts = true;
    s.world.get::<&mut CanvasGroup>(panel).unwrap().interactable = false;
    assert_eq!(
        UiInteraction::state_of(None, &s, button),
        ButtonState::Disabled
    );
    // A nested group that ignores its parents is interactable and opaque again.
    let inner = s.spawn("Inner", Some(panel));
    s.add(inner, RectTransform::stretched());
    s.add(
        inner,
        CanvasGroup {
            ignore_parent_groups: true,
            ..Default::default()
        },
    );
    let b2 = image(&mut s, inner, RectTransform::sized(50.0, 50.0));
    s.add(b2, UiButton::default());
    assert_eq!(UiInteraction::state_of(None, &s, b2), ButtonState::Normal);
    assert_eq!(layout(&s, SCREEN).node(b2).unwrap().alpha, 1.0);
    // Alpha 0 draws nothing for the group.
    s.world.get::<&mut CanvasGroup>(panel).unwrap().alpha = 0.0;
    let l = layout(&s, SCREEN);
    let list = build(&l, &s, None, &UiFont::default(), &mut GlyphAtlas::new(64));
    assert_eq!(
        list.vertices.len(),
        12,
        "background and the ignoring group's button"
    );
}

#[test]
fn aspect_ratio_fitter_modes() {
    let parent = Rect::new(Vec2::ZERO, Vec2::new(400.0, 200.0));
    let r = Rect::new(Vec2::new(10.0, 10.0), Vec2::new(110.0, 60.0));
    let c = Vec2::splat(0.5);
    let w = fit_aspect(AspectMode::WidthControlsHeight, 2.0, r, parent, c);
    assert_eq!(w.size(), Vec2::new(100.0, 50.0));
    let h = fit_aspect(AspectMode::HeightControlsWidth, 1.0, r, parent, c);
    assert_eq!(h.size(), Vec2::new(50.0, 50.0));
    assert_eq!(h.center(), r.center(), "kept around the pivot");
    let fit = fit_aspect(AspectMode::FitInParent, 1.0, r, parent, c);
    assert_eq!(
        fit,
        Rect::new(Vec2::new(100.0, 0.0), Vec2::new(300.0, 200.0))
    );
    let env = fit_aspect(AspectMode::EnvelopeParent, 1.0, r, parent, c);
    assert_eq!(
        env,
        Rect::new(Vec2::new(0.0, -100.0), Vec2::new(400.0, 300.0))
    );
    // Through the layout.
    let mut s = Scene::new("Fitter");
    let canvas = overlay(&mut s);
    let e = image(&mut s, canvas, RectTransform::sized(300.0, 100.0));
    s.add(
        e,
        AspectRatioFitter {
            aspect_mode: AspectMode::FitInParent,
            aspect_ratio: 4.0 / 3.0,
            ..Default::default()
        },
    );
    let n = *layout(&s, SCREEN).node(e).unwrap();
    assert_eq!(n.rect.size(), SCREEN, "4:3 fills the 800×600 canvas");
}

#[test]
fn rotated_and_scaled_rects_draw_and_hit_turned() {
    let mut s = Scene::new("Turned");
    let canvas = overlay(&mut s);
    let bar = image(&mut s, canvas, RectTransform::sized(200.0, 20.0));
    s.set_transform(
        bar,
        Transform {
            rotation: crate::camera::euler_to_quat(Vec3::new(0.0, 0.0, 90.0)),
            ..Default::default()
        },
    );
    let l = layout(&s, SCREEN);
    let centre = SCREEN * 0.5;
    // Upright now: hit 80 px above the centre, not 80 px to the side.
    assert_eq!(raycast(&l, &s, centre - Vec2::new(0.0, 80.0)), Some(bar));
    assert_eq!(raycast(&l, &s, centre + Vec2::new(80.0, 0.0)), None);
    let c = l.node(bar).unwrap().corners();
    assert!((c[0].x - c[1].x).abs() < 1e-3, "top edge vertical: {c:?}");
    // Scale 2 on x doubles the drawn width.
    s.set_transform(
        bar,
        Transform {
            scale: Vec3::new(2.0, 1.0, 1.0),
            ..Default::default()
        },
    );
    let l = layout(&s, SCREEN);
    assert_eq!(raycast(&l, &s, centre + Vec2::new(150.0, 0.0)), Some(bar));
    let list = build(&l, &s, None, &UiFont::default(), &mut GlyphAtlas::new(64));
    let xs: Vec<f32> = list.vertices.iter().map(|v| v.pos[0]).collect();
    let span =
        xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min);
    assert!(
        (span - 1.0).abs() < 1e-4,
        "400 px of 800 = half the clip width: {span}"
    );
}
