//! Terrain trees on the GPU: a tree draws, casts a shadow and sways (run with the other GPU
//! tests), plus MeadowDemo shots and the trees' GPU cost (ignored; run on purpose in release):
//! `cargo test --release -p misty_engine tree_ -- --ignored --nocapture --test-threads=1`
//! `MISTY_GFX_STEP=<name>` names the shots `docs/screenshots/gfx/trees_<name>_<shot>.png`.

use std::path::PathBuf;
use std::sync::Arc;

use glam::{Vec2, Vec3};

use crate::camera::{Camera, Projection};
use crate::quality::QualityLevel;
use crate::render::environment::Environment;
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{Frame, LightData, RenderTarget, Renderer};
use crate::render_scene::GameView;
use crate::scene::{LightType, Scene, Shadows, Transform};
use crate::terrain::trees::{TreeInstance, TreePrototype};
use crate::terrain::{TerrainData, TerrainDraw};
use crate::trees::TreeAsset;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn screenshots() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/gfx")
}

/// A flat 64 m terrain with one oak in the middle (its recipe written to a temp file; no
/// textures, so the leaves are solid cards).
fn one_tree() -> TerrainDraw {
    let dir = std::env::temp_dir().join("misty_tree_gpu_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Test Oak.tree");
    let tree = TreeAsset {
        bark_texture: String::new(),
        bark_normal: String::new(),
        leaf_texture: String::new(),
        leaf_normal: String::new(),
        bark_roughness: String::new(),
        bark_occlusion: String::new(),
        bark_height: String::new(),
        leaf_roughness: String::new(),
        leaf_translucency: String::new(),
        ..TreeAsset::oak()
    };
    tree.save(&path).unwrap();
    let mut d = TerrainData::new(65, Vec3::new(64.0, 10.0, 64.0), 64);
    d.trees.prototypes.push(TreePrototype {
        tree: path.to_string_lossy().into_owned(),
    });
    d.trees.instances.push(TreeInstance {
        position: Vec2::splat(0.5),
        ..Default::default()
    });
    TerrainDraw {
        key: "tree test terrain".into(),
        version: 1,
        data: Arc::new(d),
        position: Vec3::new(-32.0, 0.0, -32.0),
        cast_shadows: true,
    }
}

fn sun(direction: Vec3) -> LightData {
    LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: direction.normalize(),
        color: [1.0; 3],
        intensity: 2.0,
        range: 10.0,
        spot_angle: 30.0,
        shadows: Shadows::Hard,
        contact_shadows: false,
        area: Default::default(),
        shadow: Default::default(),
        volumetric: Default::default(),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_terrain(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    r: &mut Renderer,
    target: &RenderTarget,
    camera: &Camera,
    terrains: &[TerrainDraw],
    lights: &[LightData],
    time: f32,
) -> Vec<u8> {
    let environment = Environment {
        grass_time: Some(time),
        ..Default::default()
    };
    let frame = Frame {
        camera,
        grid: None,
        skybox: true,
        background: [0.1; 3],
        environment: &environment,
        post: Default::default(),
        lines: &[],
        overlay: &[],
        meshes: &[],
        particles: &[],
        terrains,
        shaded: true,
        lights,
        water: &[],
    };
    for _ in 0..4 {
        r.render(device, queue, target, &frame);
    }
    read_back(device, queue, target)
}

fn luminance(rgba: &[u8], w: u32, x: u32, y: u32) -> f32 {
    let i = ((y * w + x) * 4) as usize;
    0.2126 * rgba[i] as f32 + 0.7152 * rgba[i + 1] as f32 + 0.0722 * rgba[i + 2] as f32
}

/// The oak shows where it stands, and its shadow darkens the ground beside it.
#[test]
fn a_terrain_tree_draws_and_casts_a_shadow() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let with = [one_tree()];
    let without = [TerrainDraw {
        key: "tree test terrain bare".into(),
        data: Arc::new(TerrainData {
            trees: Default::default(),
            ..(*with[0].data).clone()
        }),
        ..with[0].clone()
    }];
    // Looking at the tree from 30 m, the sun low from the left so the shadow falls to the
    // right of the trunk.
    let camera = Camera {
        position: Vec3::new(0.0, 6.0, -30.0),
        rotation: glam::Quat::from_rotation_x(5f32.to_radians()),
        projection: Projection::Perspective { fov_y: 60.0 },
        near: 0.1,
        far: 500.0,
    };
    let lights = [sun(Vec3::new(1.0, -0.6, 0.0))];
    let (w, h) = (320, 180);
    let mut r = Renderer::with_queue(&device, &queue);
    let target = r.create_target(&device, w, h);
    let a = render_terrain(
        &device, &queue, &mut r, &target, &camera, &with, &lights, 1.0,
    );
    let b = render_terrain(
        &device, &queue, &mut r, &target, &camera, &without, &lights, 1.0,
    );
    if let Ok(dir) = std::env::var("MISTY_TREE_DEBUG") {
        for (name, img) in [("with", &a), ("without", &b)] {
            image::RgbaImage::from_raw(w, h, img.clone())
                .unwrap()
                .save(PathBuf::from(&dir).join(format!("tree_test_{name}.png")))
                .unwrap();
        }
    }
    // The crown fills the middle of the image.
    let (cx, cy) = (w / 2, h / 3);
    let diff = (luminance(&a, w, cx, cy) - luminance(&b, w, cx, cy)).abs();
    assert!(diff > 10.0, "tree visible: {diff}");
    // The shadow: ground pixels to the right of the trunk's foot are darker with the tree.
    let mut darker = 0;
    for x in w / 2 + 20..w - 10 {
        for y in h * 55 / 100..h * 70 / 100 {
            darker += usize::from(luminance(&a, w, x, y) + 8.0 < luminance(&b, w, x, y));
        }
    }
    assert!(darker > 100, "shadowed ground pixels: {darker}");
    // The wind moves it.
    let c = render_terrain(
        &device, &queue, &mut r, &target, &camera, &with, &lights, 2.5,
    );
    let moved = (0..(w * h) as usize)
        .filter(|i| a[i * 4 + 1] != c[i * 4 + 1])
        .count();
    assert!(moved > 50, "pixels changed by the wind: {moved}");
}

#[derive(Clone, Copy, Debug)]
pub(in crate::render) enum Shot {
    /// The player's view as saved.
    Wide,
    /// Turned towards the broad oak on the right, close.
    Close,
    /// Turned round: the trees behind the player.
    Behind,
    /// The pair on the left (the reference's trees), zoomed in.
    Pair,
}

impl Shot {
    const ALL: [Shot; 4] = [Shot::Wide, Shot::Close, Shot::Behind, Shot::Pair];

    pub(in crate::render) fn name(self) -> &'static str {
        match self {
            Shot::Wide => "wide",
            Shot::Close => "close",
            Shot::Behind => "behind",
            Shot::Pair => "pair",
        }
    }
}

/// MeadowDemo as saved, the player turned for `shot`.
pub(in crate::render) fn meadow_view(shot: Shot) -> GameView {
    crate::assets::set_project_root(&root());
    let (mut s, _) = Scene::load(&root().join("Assets/Scenes/MeadowDemo.scene")).expect("scene");
    let (yaw, pitch) = match shot {
        Shot::Wide => (0.0, 0.0),
        Shot::Close => (48.0, -9.0),
        Shot::Behind => (180.0, -2.0),
        Shot::Pair => (-35.0, -1.0),
    };
    if let Some(player) = s.find_by_name("Player") {
        let p = s.world_position(player);
        s.set_transform(
            player,
            Transform::from_position(p).with_euler(Vec3::new(0.0, yaw, 0.0)),
        );
    }
    if let Some(cam) = s.find_by_name("Camera") {
        s.set_transform(
            cam,
            Transform::default().with_euler(Vec3::new(pitch, 0.0, 0.0)),
        );
    }
    let materials = crate::assets::AssetCache::open(&root());
    let mut view = GameView::new(&s, &materials).expect("camera");
    if let Shot::Pair = shot
        && let Projection::Perspective { fov_y } = &mut view.camera.projection
    {
        *fov_y = 18.0;
    }
    // A fixed wind clock: renders compared with each other show the same frame.
    view.environment.grass_time = Some(3.0);
    view
}

/// The view's terrains without their trees (own store key).
fn without_trees(view: &GameView) -> Vec<TerrainDraw> {
    view.terrains
        .iter()
        .map(|t| TerrainDraw {
            key: format!("{}#notrees", t.key),
            data: Arc::new(TerrainData {
                trees: Default::default(),
                ..(*t.data).clone()
            }),
            ..t.clone()
        })
        .collect()
}

fn shot_image(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &GameView,
    quality: &QualityLevel,
    terrains: Option<&[TerrainDraw]>,
    (w, h): (u32, u32),
) -> image::RgbaImage {
    let mut r = Renderer::with_queue(device, queue);
    r.set_quality(device, quality);
    let target = r.create_target(device, w, h);
    let mut frame = view.frame();
    if let Some(t) = terrains {
        frame.terrains = t;
    }
    for _ in 0..48 {
        r.render(device, queue, &target, &frame);
    }
    image::RgbaImage::from_raw(w, h, read_back(device, queue, &target)).unwrap()
}

/// `docs/screenshots/gfx/trees_<step>_{wide,close,behind,pair}.png` (1600×900, High), and
/// the wide shot next to the reference (`trees_<step>_vs_reference.png`) when the reference
/// image is around (`MISTY_REFERENCE`, default the MistyDesign folder). `MISTY_TREES=0`
/// renders without trees (the "before" shots).
#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn tree_shots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let step = std::env::var("MISTY_GFX_STEP").unwrap_or_else(|_| "latest".into());
    let only = std::env::var("MISTY_GFX_SHOT").unwrap_or_default();
    let bare = std::env::var("MISTY_TREES").is_ok_and(|v| v == "0");
    let (w, h) = (1600, 900);
    std::fs::create_dir_all(screenshots()).unwrap();
    for shot in Shot::ALL {
        if !only.is_empty() && !shot.name().contains(&only) {
            continue;
        }
        let view = meadow_view(shot);
        let none = without_trees(&view);
        let terrains = bare.then_some(&none[..]);
        let img = shot_image(
            &device,
            &queue,
            &view,
            &QualityLevel::high(),
            terrains,
            (w, h),
        );
        let path = screenshots().join(format!("trees_{step}_{}.png", shot.name()));
        img.save(&path).unwrap();
        eprintln!("wrote {}", path.display());
        if let Shot::Wide = shot {
            let reference = std::env::var("MISTY_REFERENCE")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../../../../MistyDesign/grass_reference.png")
                });
            if let Ok(r) = image::open(&reference) {
                let (pw, ph) = (800, 450);
                let ours =
                    image::imageops::resize(&img, pw, ph, image::imageops::FilterType::Triangle);
                let theirs = image::imageops::resize(
                    &r.into_rgba8(),
                    pw,
                    ph,
                    image::imageops::FilterType::Triangle,
                );
                let mut both = image::RgbaImage::new(pw * 2, ph);
                image::imageops::replace(&mut both, &ours, 0, 0);
                image::imageops::replace(&mut both, &theirs, pw as i64, 0);
                let path = screenshots().join(format!("trees_{step}_vs_reference.png"));
                both.save(&path).unwrap();
                eprintln!("wrote {}", path.display());
            }
        }
    }
}

/// The world position of the MeadowDemo tree of prototype `name` nearest to the view's camera.
fn nearest_tree(view: &GameView, name: &str) -> Option<(Vec3, f32)> {
    let eye = view.camera.position;
    let mut best: Option<(Vec3, f32)> = None;
    for t in &view.terrains {
        let d = &t.data;
        let Some(proto) = d
            .trees
            .prototypes
            .iter()
            .position(|p| p.tree.contains(name))
        else {
            continue;
        };
        for inst in d
            .trees
            .instances
            .iter()
            .filter(|i| i.prototype as usize == proto)
        {
            let foot = t.position + crate::terrain::trees::local_position(d, inst);
            if best.is_none_or(|(b, _)| b.distance(eye) > foot.distance(eye)) {
                best = Some((foot, inst.height_scale));
            }
        }
    }
    best
}

/// A camera at `eye` looking at `at` (no roll).
fn look_at(view: &mut GameView, eye: Vec3, at: Vec3, fov: f32) {
    let d = (at - eye).normalize();
    view.camera.position = eye;
    view.camera.rotation = glam::Quat::from_rotation_y(d.x.atan2(d.z))
        * glam::Quat::from_rotation_x(-d.y.clamp(-1.0, 1.0).asin());
    view.camera.projection = Projection::Perspective { fov_y: fov };
    view.camera.near = 0.05;
}

/// Close-ups of the broad oak (trunk from the side, sunlit and from low down, the crown) plus
/// the wide and behind views: `docs/screenshots/gfx/treelook_<step>_{trunk_side,trunk_sun,
/// trunk_low,crown,wide,behind}.png` (1600×900, High).
#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn tree_look_shots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let step = std::env::var("MISTY_GFX_STEP").unwrap_or_else(|_| "latest".into());
    let (w, h) = (1600, 900);
    std::fs::create_dir_all(screenshots()).unwrap();
    let base = meadow_view(Shot::Close);
    let (foot, scale) = nearest_tree(&base, "Oak Broad").expect("a broad oak in MeadowDemo");
    // From the tree towards the player (the side the player sees), and a sunlit view of the
    // trunk: from the tree towards the sun, turned 40° (some of its shading shows).
    let sun = base
        .lights
        .iter()
        .find(|l| matches!(l.kind, LightType::Directional))
        .map_or(Vec3::Z, |l| -l.direction);
    let player = (base.camera.position - foot)
        .with_y(0.0)
        .normalize_or(Vec3::X);
    let sunny =
        glam::Quat::from_rotation_y(40f32.to_radians()) * sun.with_y(0.0).normalize_or(Vec3::X);
    let height = 17.0 * scale;
    let mut shots: Vec<(&str, GameView)> = Vec::new();
    for (name, toward) in [("trunk_side", player), ("trunk_sun", sunny)] {
        let side = Vec3::Y.cross(toward).normalize();
        let mut v = meadow_view(Shot::Close);
        look_at(
            &mut v,
            foot + toward * 3.4 + side * 0.6 + Vec3::Y * 1.6,
            foot + Vec3::Y * 1.7,
            55.0,
        );
        shots.push((name, v));
    }
    let side = Vec3::Y.cross(player).normalize();
    let mut v = meadow_view(Shot::Close);
    look_at(
        &mut v,
        foot + player * 2.2 - side * 0.8 + Vec3::Y * 0.45,
        foot + Vec3::Y * height * 0.42,
        70.0,
    );
    shots.push(("trunk_low", v));
    let mut v = meadow_view(Shot::Close);
    look_at(
        &mut v,
        foot + player * height * 0.9 + side * height * 0.3 + Vec3::Y * 2.0,
        foot + Vec3::Y * height * 0.62,
        50.0,
    );
    shots.push(("crown", v));
    shots.push(("wide", meadow_view(Shot::Wide)));
    shots.push(("behind", meadow_view(Shot::Behind)));
    for (name, view) in shots {
        let img = shot_image(&device, &queue, &view, &QualityLevel::high(), None, (w, h));
        let path = screenshots().join(format!("treelook_{step}_{name}.png"));
        img.save(&path).unwrap();
        eprintln!("wrote {}", path.display());
    }
}

/// Median GPU frame time of MeadowDemo at 1920×1080 per quality level with and without its
/// trees (the difference is what the trees cost), and the triangles drawn.
#[test]
#[ignore = "timing, prints only"]
fn tree_cost() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let shots = match std::env::var("MISTY_GFX_SHOT") {
        Ok(s) if s == "all" => vec![Shot::Wide, Shot::Close, Shot::Behind],
        _ => vec![Shot::Wide, Shot::Behind],
    };
    for shot in shots {
        let view = meadow_view(shot);
        let none = without_trees(&view);
        for (name, quality) in [
            ("high", QualityLevel::high()),
            ("medium", QualityLevel::medium()),
            ("low", QualityLevel::low()),
        ] {
            let mut ms = [0.0f32; 2];
            let mut tris = [0.0f64; 2];
            for (i, terrains) in [None, Some(&none[..])].into_iter().enumerate() {
                let mut r = Renderer::with_queue(&device, &queue);
                r.set_quality(&device, &quality);
                let target = r.create_target(&device, 1920, 1080);
                let mut frame = view.frame();
                if let Some(t) = terrains {
                    frame.terrains = t;
                }
                let mut times = Vec::new();
                for f in 0..160 {
                    r.render(&device, &queue, &target, &frame);
                    let _ = device.poll(wgpu::PollType::wait_indefinitely());
                    if f >= 40
                        && let Some(t) = r.gpu_ms()
                    {
                        times.push(t);
                    }
                }
                times.sort_by(f32::total_cmp);
                ms[i] = times.get(times.len() / 2).copied().unwrap_or(f32::NAN);
                tris[i] = r.stats().triangles as f64 / 1e6;
            }
            eprintln!(
                "{:>6} {name:>6}: trees {:.2} ms, no trees {:.2} ms => trees cost {:.2} ms ({:.2} M vs {:.2} M triangles)",
                shot.name(),
                ms[0],
                ms[1],
                ms[0] - ms[1],
                tris[0],
                tris[1]
            );
        }
    }
}
