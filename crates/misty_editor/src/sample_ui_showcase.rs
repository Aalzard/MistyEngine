//! The UIShowcase sample scene: every canvas mode at once. A pause menu on an overlay canvas
//! (sliced panel, buttons navigable with the arrows or a gamepad, Enter / South to press), a
//! Screen Space - Camera HUD (a radial-fill health ring and a horizontal stamina bar), and a
//! World Space sign standing in the scene (sliced panel, rich text and a button clicked through
//! the camera). `Assets/Scripts/UIShowcase.lua` animates the bars and reports presses. The
//! sprites (a panel, a button and a ring, with 9-slice borders) are generated too.
//! Regenerate with `cargo test -p misty_editor -- --ignored write_ui_showcase`; the other tests
//! check the committed files still match and play the scene headlessly.

use std::path::{Path, PathBuf};

use glam::{Vec2, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::image::{Rgba, RgbaImage};
use misty_engine::scene::{
    ButtonCall, Canvas, CanvasGroup, CanvasRenderMode, CanvasScaler, FillMethod, ImageFill,
    ImageType, LayoutElement, LinearLayout, MeshFilter, MeshRenderer, Navigation, NavigationMode,
    Padding, PrimitiveMesh, RectTransform, Scene, ScriptRef, Scripts, TextAnchor, Transform,
    UiButton, UiImage, UiText, VerticalLayoutGroup,
};
use misty_engine::sprites::TextureImporter;
use misty_engine::ui::presets;

const SCENE: &str = "Assets/Scenes/UIShowcase.scene";
const SCRIPT: &str = "Assets/Scripts/UIShowcase.lua";
const PANEL: &str = "Assets/Sprites/UIPanel.png";
const BUTTON: &str = "Assets/Sprites/UIButton.png";
const RING: &str = "Assets/Sprites/UIRing.png";

const SHOWCASE_SCRIPT: &str = include_str!("sample_ui_showcase.lua");

const MENU_ITEMS: [&str; 4] = ["Resume", "Options", "Credits", "Quit"];

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Signed distance from `p` to a rounded rectangle centred at `c` with half size `h` and corner
/// radius `r` (negative inside).
fn rounded(p: Vec2, c: Vec2, h: Vec2, r: f32) -> f32 {
    let q = (p - c).abs() - (h - Vec2::splat(r));
    q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - r
}

/// Coverage of a shape with signed distance `d` (one pixel of anti-aliasing).
fn cover(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

fn px(v: f32, a: f32) -> Rgba<u8> {
    let v = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgba([v, v, v, (a.clamp(0.0, 1.0) * 255.0).round() as u8])
}

/// 64×64 panel: light rounded rectangle with a brighter 3-pixel rim (16-pixel corners).
pub fn panel_sprite() -> RgbaImage {
    RgbaImage::from_fn(64, 64, |x, y| {
        let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
        let d = rounded(p, Vec2::splat(32.0), Vec2::splat(31.0), 12.0);
        let rim = cover(-(d + 3.0));
        px(0.82 + 0.18 * (1.0 - rim), cover(d))
    })
}

/// 48×48 button: rounded, a soft top-to-bottom gradient and a darker 2-pixel edge.
pub fn button_sprite() -> RgbaImage {
    RgbaImage::from_fn(48, 48, |x, y| {
        let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
        let d = rounded(p, Vec2::splat(24.0), Vec2::splat(23.0), 9.0);
        let inner = cover(-(d + 2.0));
        let shade = 1.0 - 0.18 * (y as f32 / 47.0);
        px(shade * (0.72 + 0.28 * inner), cover(d))
    })
}

/// 128×128 ring (outer radius 62, inner 46) for the radial health bar.
pub fn ring_sprite() -> RgbaImage {
    RgbaImage::from_fn(128, 128, |x, y| {
        let r = Vec2::new(x as f32 + 0.5 - 64.0, y as f32 + 0.5 - 64.0).length();
        let a = cover(r - 62.0) * cover(46.0 - r);
        px(1.0, a)
    })
}

/// Import settings of a generated sprite with a 9-slice `border` (pixels).
pub fn sprite_importer(border: f32) -> TextureImporter {
    TextureImporter {
        border: [border; 4],
        ..TextureImporter::sprite()
    }
}

/// The sprites, their borders and paths.
fn sprites() -> [(&'static str, RgbaImage, TextureImporter); 3] {
    [
        (PANEL, panel_sprite(), sprite_importer(16.0)),
        (BUTTON, button_sprite(), sprite_importer(12.0)),
        (RING, ring_sprite(), sprite_importer(0.0)),
    ]
}

fn ui(s: &mut Scene, name: &str, parent: Entity, rt: RectTransform) -> Entity {
    presets::spawn(s, name, Some(parent), rt)
}

fn anchored(anchor: Vec2, pos: Vec2, size: Vec2) -> RectTransform {
    RectTransform {
        anchor_min: anchor,
        anchor_max: anchor,
        pivot: anchor,
        anchored_position: pos,
        size_delta: size,
    }
}

fn sliced(sprite: &str, color: [f32; 4]) -> UiImage {
    UiImage {
        sprite: Some(sprite.into()),
        color,
        image_type: ImageType::Sliced,
        ..Default::default()
    }
}

fn text(s: &mut Scene, e: Entity, t: &str, size: f32, color: [f32; 4], align: TextAnchor) {
    s.add(
        e,
        UiText {
            text: t.into(),
            font_size: size,
            color,
            alignment: align,
            raycast_target: false,
            ..Default::default()
        },
    );
}

fn canvas(s: &mut Scene, name: &str, c: Canvas, scaler: CanvasScaler, rt: RectTransform) -> Entity {
    let e = presets::spawn(s, name, None, rt);
    s.add(e, c);
    s.add(e, scaler);
    e
}

fn screen_scaler() -> CanvasScaler {
    CanvasScaler {
        reference_resolution: Vec2::new(1280.0, 720.0),
        match_width_or_height: 0.5,
        ..Default::default()
    }
}

const WHITE: [f32; 4] = [1.0; 4];
const INK: [f32; 4] = [0.93, 0.95, 0.98, 1.0];

/// A menu button: sliced sprite, Vertical navigation that wraps, a label.
fn menu_button(s: &mut Scene, panel: Entity, label: &str) -> Entity {
    let b = ui(s, label, panel, RectTransform::default());
    s.add(b, sliced(BUTTON, [0.33, 0.45, 0.78, 1.0]));
    s.add(
        b,
        UiButton {
            highlighted_color: [1.15, 1.15, 1.15, 1.0].map(|v: f32| v.min(1.0)),
            selected_color: [1.0, 0.86, 0.45, 1.0],
            navigation: Navigation {
                mode: NavigationMode::Vertical,
                wrap_around: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    s.add(
        b,
        LayoutElement {
            preferred_height: Some(46.0),
            ..Default::default()
        },
    );
    let t = ui(s, "Label", b, RectTransform::stretched());
    text(s, t, label, 20.0, INK, TextAnchor::MiddleCenter);
    b
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("UIShowcase");
    let cam = s.find_by_name("Main Camera").expect("default camera");
    s.set_transform(
        cam,
        Transform::from_position(Vec3::new(0.0, 1.6, -6.0)).with_euler(Vec3::new(6.0, 0.0, 0.0)),
    );
    // A floor, a post for the sign and a couple of blocks.
    let floor = s.spawn("Floor", None);
    s.add(floor, MeshFilter::primitive(PrimitiveMesh::Plane));
    s.add(floor, MeshRenderer::default());
    s.set_transform(
        floor,
        Transform {
            scale: Vec3::new(3.0, 1.0, 3.0),
            ..Default::default()
        },
    );
    for (name, pos, scale) in [
        (
            "Sign Post",
            Vec3::new(-2.4, 0.9, 2.0),
            Vec3::new(0.12, 1.8, 0.12),
        ),
        ("Block A", Vec3::new(1.6, 0.5, 3.5), Vec3::splat(1.0)),
        ("Block B", Vec3::new(-0.6, 0.35, 5.0), Vec3::splat(0.7)),
    ] {
        let e = s.spawn(name, None);
        s.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
        s.add(e, MeshRenderer::default());
        s.set_transform(
            e,
            Transform {
                position: pos,
                scale,
                ..Default::default()
            },
        );
    }
    let showcase = s.spawn("UI Showcase", None);
    s.add(
        showcase,
        Scripts {
            list: vec![ScriptRef {
                path: SCRIPT.into(),
                ..Default::default()
            }],
        },
    );

    // ---- World Space sign -------------------------------------------------------------------
    let sign = canvas(
        &mut s,
        "Welcome Sign",
        Canvas {
            render_mode: CanvasRenderMode::WorldSpace,
            ..Default::default()
        },
        CanvasScaler {
            dynamic_pixels_per_unit: 2.0,
            ..Default::default()
        },
        RectTransform::sized(400.0, 220.0),
    );
    s.set_transform(
        sign,
        Transform {
            position: Vec3::new(-2.4, 2.4, 2.0),
            scale: Vec3::splat(0.008),
            ..Default::default()
        }
        .with_euler(Vec3::new(0.0, 25.0, 0.0)),
    );
    let board = ui(&mut s, "Sign Panel", sign, RectTransform::stretched());
    s.add(board, sliced(PANEL, [0.16, 0.24, 0.42, 1.0]));
    let title = ui(
        &mut s,
        "Sign Title",
        board,
        anchored(
            Vec2::new(0.5, 1.0),
            Vec2::new(0.0, -18.0),
            Vec2::new(360.0, 56.0),
        ),
    );
    text(
        &mut s,
        title,
        "<b>Misty Engine</b>",
        40.0,
        WHITE,
        TextAnchor::MiddleCenter,
    );
    let body = ui(
        &mut s,
        "Sign Body",
        board,
        anchored(
            Vec2::new(0.5, 0.5),
            Vec2::new(0.0, 8.0),
            Vec2::new(360.0, 60.0),
        ),
    );
    text(
        &mut s,
        body,
        "A <color=#ffd866>World Space</color> canvas:\nlit by nothing, hidden by the scene",
        18.0,
        INK,
        TextAnchor::MiddleCenter,
    );
    let wave = ui(
        &mut s,
        "Wave Button",
        board,
        anchored(
            Vec2::new(0.5, 0.0),
            Vec2::new(0.0, 18.0),
            Vec2::new(150.0, 44.0),
        ),
    );
    s.add(wave, sliced(BUTTON, [0.95, 0.6, 0.3, 1.0]));
    s.add(
        wave,
        UiButton {
            on_click: vec![ButtonCall {
                target: Some(showcase),
                method: "Waved".into(),
            }],
            ..Default::default()
        },
    );
    let wl = ui(&mut s, "Label", wave, RectTransform::stretched());
    text(
        &mut s,
        wl,
        "Wave",
        20.0,
        [0.15, 0.1, 0.05, 1.0],
        TextAnchor::MiddleCenter,
    );

    // ---- Screen Space - Camera HUD ----------------------------------------------------------
    let hud = canvas(
        &mut s,
        "HUD",
        Canvas {
            render_mode: CanvasRenderMode::ScreenSpaceCamera,
            world_camera: Some(cam),
            plane_distance: 1.0,
            ..Default::default()
        },
        screen_scaler(),
        RectTransform::stretched(),
    );
    let top_left = Vec2::new(0.0, 1.0);
    let ring_back = ui(
        &mut s,
        "Health Ring Back",
        hud,
        anchored(top_left, Vec2::new(24.0, -24.0), Vec2::splat(104.0)),
    );
    s.add(
        ring_back,
        UiImage {
            sprite: Some(RING.into()),
            color: [0.05, 0.05, 0.08, 0.55],
            raycast_target: false,
            ..Default::default()
        },
    );
    let ring = ui(&mut s, "Health Ring", ring_back, RectTransform::stretched());
    s.add(
        ring,
        UiImage {
            sprite: Some(RING.into()),
            color: [0.35, 0.92, 0.45, 1.0],
            raycast_target: false,
            image_type: ImageType::Filled,
            fill: ImageFill {
                method: FillMethod::Radial360,
                origin: 2,
                amount: 0.72,
                clockwise: true,
            },
            ..Default::default()
        },
    );
    let hp = ui(&mut s, "Health Text", ring_back, RectTransform::stretched());
    text(&mut s, hp, "72", 30.0, WHITE, TextAnchor::MiddleCenter);
    let frame = ui(
        &mut s,
        "Stamina Frame",
        hud,
        anchored(top_left, Vec2::new(140.0, -52.0), Vec2::new(240.0, 26.0)),
    );
    s.add(frame, sliced(BUTTON, [0.08, 0.08, 0.1, 0.75]));
    let mut fill_rt = RectTransform::stretched();
    fill_rt.set_offsets(Vec2::splat(4.0), Vec2::splat(-4.0));
    let stamina = ui(&mut s, "Stamina Fill", frame, fill_rt);
    s.add(
        stamina,
        UiImage {
            color: [1.0, 0.82, 0.25, 1.0],
            raycast_target: false,
            image_type: ImageType::Filled,
            fill: ImageFill {
                method: FillMethod::Horizontal,
                origin: 0,
                amount: 0.6,
                clockwise: true,
            },
            ..Default::default()
        },
    );
    let hud_label = ui(
        &mut s,
        "HUD Label",
        hud,
        anchored(top_left, Vec2::new(140.0, -24.0), Vec2::new(300.0, 24.0)),
    );
    text(
        &mut s,
        hud_label,
        "Screen Space - Camera HUD",
        16.0,
        INK,
        TextAnchor::MiddleLeft,
    );

    // ---- Overlay pause menu -----------------------------------------------------------------
    let overlay = canvas(
        &mut s,
        "Menu Canvas",
        Canvas {
            sort_order: 1,
            ..Default::default()
        },
        screen_scaler(),
        RectTransform::stretched(),
    );
    let panel = ui(
        &mut s,
        "Menu Panel",
        overlay,
        anchored(
            Vec2::new(1.0, 0.5),
            Vec2::new(-40.0, 0.0),
            Vec2::new(300.0, 400.0),
        ),
    );
    s.add(panel, sliced(PANEL, [0.12, 0.13, 0.19, 0.94]));
    s.add(
        panel,
        CanvasGroup {
            alpha: 0.97,
            ..Default::default()
        },
    );
    s.add(
        panel,
        VerticalLayoutGroup(LinearLayout {
            padding: Padding {
                left: 26.0,
                right: 26.0,
                top: 22.0,
                bottom: 22.0,
            },
            spacing: 12.0,
            child_force_expand_height: false,
            ..Default::default()
        }),
    );
    let heading = ui(&mut s, "Menu Title", panel, RectTransform::default());
    text(
        &mut s,
        heading,
        "<b>Paused</b>",
        30.0,
        WHITE,
        TextAnchor::MiddleCenter,
    );
    s.add(
        heading,
        LayoutElement {
            preferred_height: Some(48.0),
            ..Default::default()
        },
    );
    let buttons: Vec<Entity> = MENU_ITEMS
        .iter()
        .map(|label| menu_button(&mut s, panel, label))
        .collect();
    let status = ui(&mut s, "Status", panel, RectTransform::default());
    text(
        &mut s,
        status,
        "Arrows / d-pad to move, Enter / A to press",
        14.0,
        [0.7, 0.74, 0.82, 1.0],
        TextAnchor::MiddleCenter,
    );
    s.add(
        status,
        LayoutElement {
            preferred_height: Some(40.0),
            ..Default::default()
        },
    );
    let es = presets::event_system(&mut s, "EventSystem");
    s.world
        .get::<&mut misty_engine::scene::EventSystem>(es)
        .expect("event system")
        .first_selected = Some(buttons[0]);
    s
}

fn write_all(root: &Path) {
    for (path, img, imp) in sprites() {
        img.save(root.join(path)).unwrap();
        imp.save(&root.join(path)).unwrap();
    }
    std::fs::write(root.join(SCRIPT), SHOWCASE_SCRIPT).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
    // New assets get their GUIDs now rather than at the editor's next scan.
    use misty_engine::asset_db::{Guid, meta};
    for path in [PANEL, BUTTON, RING, SCRIPT, SCENE] {
        let abs = root.join(path);
        if meta::read_guid(&abs).is_none() {
            meta::write_guid(&abs, Guid::new()).unwrap();
        }
    }
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_ui_showcase() {
    write_all(&project());
}

#[test]
fn ui_showcase_matches_its_generator() {
    let root = project();
    for (path, img, imp) in sprites() {
        let png = misty_engine::image::open(root.join(path))
            .unwrap()
            .to_rgba8();
        assert!(png == img, "{path}: regenerate with write_ui_showcase");
        assert_eq!(
            TextureImporter::try_load(&root.join(path)).unwrap(),
            imp,
            "{path}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(root.join(SCRIPT))
            .unwrap()
            .replace("\r\n", "\n"),
        SHOWCASE_SCRIPT
    );
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_ui_showcase",
    );
    let (scene, report) = Scene::from_json("UIShowcase", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let hud = scene.find_by_name("HUD").unwrap();
    let c = scene.world.get::<&Canvas>(hud).unwrap().clone();
    assert_eq!(
        c.world_camera,
        scene.find_by_name("Main Camera"),
        "camera reference kept"
    );
}

/// Play it headlessly: a gamepad moves through the menu and presses, the bars animate, and the
/// World Space sign's button is clicked through the camera.
#[test]
fn ui_showcase_plays_with_a_gamepad() {
    use misty_engine::audio::Audio;
    use misty_engine::input::Joystick;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::Input;
    use misty_engine::settings::ProjectSettings;

    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let (audio, _) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root, &ProjectSettings::default(), audio);
    let screen = Vec2::new(1280.0, 720.0);
    let mut time = Time::default();
    let pad = |dpad_y: f32, south: bool| {
        let mut j = Joystick {
            name: "Test Pad".into(),
            ..Default::default()
        };
        j.axes[6] = dpad_y;
        j.held[0] = south;
        j.pressed[0] = south;
        j
    };
    let mut frame = |scene: &mut Scene, rt: &mut Runtime, input: Input| {
        time.advance(1.0 / 60.0);
        rt.update(
            scene,
            &time,
            Input {
                screen_size: screen,
                ..input
            },
        );
    };
    let with_pad = |j: Joystick| Input {
        joysticks: vec![j],
        ..Default::default()
    };
    let find = |scene: &Scene, n: &str| scene.find_by_name(n).unwrap();
    let text_of = |scene: &Scene, n: &str| {
        scene
            .world
            .get::<&UiText>(find(scene, n))
            .unwrap()
            .text
            .clone()
    };
    for _ in 0..3 {
        frame(&mut scene, &mut rt, with_pad(pad(0.0, false)));
    }
    assert_eq!(rt.ui_interaction().selected, Some(find(&scene, "Resume")));
    // D-pad down once: Options. Held a while: it repeats down the list and wraps.
    frame(&mut scene, &mut rt, with_pad(pad(-1.0, false)));
    frame(&mut scene, &mut rt, with_pad(pad(0.0, false)));
    assert_eq!(rt.ui_interaction().selected, Some(find(&scene, "Options")));
    frame(&mut scene, &mut rt, with_pad(pad(1.0, false)));
    frame(&mut scene, &mut rt, with_pad(pad(0.0, false)));
    frame(&mut scene, &mut rt, with_pad(pad(1.0, false)));
    frame(&mut scene, &mut rt, with_pad(pad(0.0, false)));
    assert_eq!(
        rt.ui_interaction().selected,
        Some(find(&scene, "Quit")),
        "up wraps to the end"
    );
    // South presses the selection.
    frame(&mut scene, &mut rt, with_pad(pad(0.0, true)));
    frame(&mut scene, &mut rt, with_pad(pad(0.0, false)));
    assert_eq!(text_of(&scene, "Status"), "Quit pressed");
    // The bars move over time.
    let fill = |scene: &Scene| {
        scene
            .world
            .get::<&UiImage>(find(scene, "Health Ring"))
            .unwrap()
            .fill
            .amount
    };
    let before = fill(&scene);
    for _ in 0..30 {
        frame(&mut scene, &mut rt, Input::default());
    }
    assert!((fill(&scene) - before).abs() > 1e-3, "health animates");
    // Click the sign's button where the game camera sees it.
    let l = misty_engine::ui::layout(&scene, screen);
    let wave = find(&scene, "Wave Button");
    let n = l.node(wave).unwrap();
    let c = n.corners();
    let at = l.to_target(n.canvas, (c[0] + c[2]) * 0.5).unwrap();
    assert!(l.placement(n.canvas).unwrap().in_world());
    frame(
        &mut scene,
        &mut rt,
        Input {
            mouse_position: Some(at),
            mouse_held: [true, false, false],
            mouse_pressed: [true, false, false],
            ..Default::default()
        },
    );
    frame(
        &mut scene,
        &mut rt,
        Input {
            mouse_position: Some(at),
            mouse_released: [true, false, false],
            ..Default::default()
        },
    );
    assert_eq!(text_of(&scene, "Status"), "Hello from the sign!");
}

/// Render it headlessly: the World Space sign and the camera HUD reach the image (drawn in the
/// depth-tested UI pass), not only the overlay menu.
#[test]
fn ui_showcase_renders_every_canvas_mode() {
    use misty_engine::render::Renderer;
    use misty_engine::render::ui::UiRenderer;
    struct NoMaterials;
    impl misty_engine::render_scene::MaterialSource for NoMaterials {
        fn params(&self, _: Option<&str>) -> misty_engine::render::MaterialParams {
            Default::default()
        }
    }
    let root = project();
    misty_engine::assets::set_project_root(&root);
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    let (scene, _) = Scene::from_json("UIShowcase", &text).unwrap();
    let Some(gpu) = crate::sample_render::headless() else {
        return;
    };
    let (device, queue) = &gpu;
    let view = misty_engine::render_scene::GameView::new(&scene, &NoMaterials).unwrap();
    let mut renderer = Renderer::with_queue(device, queue);
    let (w, h) = (640u32, 360u32);
    let target = renderer.create_target(device, w, h);
    misty_engine::render::ui::set_keeps_depth(misty_engine::render::ui::needs_depth(&scene, false));
    renderer.render(device, queue, &target, &view.frame());
    misty_engine::render::ui::set_keeps_depth(false);
    let before = crate::project_thumbnail::read_pixels(device, queue, &target);
    let mut ui = UiRenderer::new(device, queue);
    ui.render(device, queue, &target, &scene, None);
    let after = crate::project_thumbnail::read_pixels(device, queue, &target);
    let screen = Vec2::new(w as f32, h as f32);
    let l = misty_engine::ui::layout(&scene, screen);
    // How much the pixel at (fx, fy) across an element changed (a spot on its graphic, clear
    // of text: the ring's top, inside the panels' borders, the middle of the stamina bar).
    let changed_at = |name: &str, fx: f32, fy: f32| {
        let n = l.node(scene.find_by_name(name).unwrap()).unwrap();
        let c = n.corners();
        let q = c[0] + (c[1] - c[0]) * fx + (c[3] - c[0]) * fy;
        let p = l.to_target(n.canvas, q).unwrap();
        let i = ((p.y as u32 * w + p.x as u32) * 4) as usize;
        let d: i32 = (0..3)
            .map(|k| (before[i + k] as i32 - after[i + k] as i32).abs())
            .sum();
        (p, d)
    };
    for (name, fx, fy) in [
        ("Sign Panel", 0.08, 0.08),
        ("Health Ring Back", 0.5, 0.06),
        ("Stamina Frame", 0.5, 0.5),
        ("Menu Panel", 0.08, 0.08),
    ] {
        let (p, d) = changed_at(name, fx, fy);
        assert!(d > 30, "{name} drawn at {p}: colour change {d}");
    }
}
