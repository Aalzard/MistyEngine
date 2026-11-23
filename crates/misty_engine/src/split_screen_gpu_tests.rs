//! Headless split-screen checks (skipped without an adapter): each viewport camera's rect of
//! the full image must look like that camera rendered alone at the rect's size, at every
//! quality level (temporal effects, upscaling, MSAA).
//!
//! `MISTY_SPLIT_SHOTS=1` also writes the images to `target/split_screen/` to look at.

use std::collections::BTreeMap;
use std::path::PathBuf;

use glam::{EulerRot, Quat, Vec3};

use super::SplitScreen;
use crate::draw_cache::DrawCache;
use crate::quality::QualityLevel;
use crate::render::golden_tests::{Drift, drift};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{MaterialParams, RenderTarget, Renderer};
use crate::render_scene::{GameView, MaterialSource, viewport_pixels};
use crate::scene::{
    Camera, Light, LightType, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, Shadows, Transform,
};
use hecs::Entity;

struct Materials(BTreeMap<&'static str, MaterialParams>);

impl MaterialSource for Materials {
    fn params(&self, rel: Option<&str>) -> MaterialParams {
        rel.and_then(|r| self.0.get(r)).cloned().unwrap_or_default()
    }
}

fn materials() -> Materials {
    let m = |color: [f32; 3]| MaterialParams {
        color: [color[0], color[1], color[2], 1.0],
        smoothness: 0.2,
        ..Default::default()
    };
    Materials(BTreeMap::from([
        ("ground", m([0.5, 0.5, 0.5])),
        ("red", m([0.9, 0.05, 0.05])),
        ("blue", m([0.05, 0.1, 0.9])),
        ("green", m([0.05, 0.8, 0.1])),
    ]))
}

fn block(s: &mut Scene, at: Vec3, size: Vec3, mat: &'static str) {
    let e = s.spawn(mat, None);
    let mut t = Transform::from_position(at);
    t.scale = size;
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.add(
        e,
        MeshRenderer {
            material: Some(mat.into()),
            ..Default::default()
        },
    );
}

/// A camera at `eye` looking at `target` with Viewport Rect `viewport`.
fn camera(s: &mut Scene, name: &str, eye: Vec3, target: Vec3, viewport: [f32; 4]) -> Entity {
    let e = s.spawn(name, None);
    let d = (target - eye).normalize();
    let mut t = Transform::from_position(eye);
    t.rotation = Quat::from_euler(EulerRot::YXZ, d.x.atan2(d.z), -d.y.asin(), 0.0);
    s.set_transform(e, t);
    s.add(
        e,
        Camera {
            viewport,
            ..Default::default()
        },
    );
    e
}

/// Grey ground, a red block on the left (−X), a blue one on the right, a green one far
/// behind, and a sun with shadows.
fn arena() -> Scene {
    let mut s = Scene::new("Split");
    block(
        &mut s,
        Vec3::new(0.0, -0.1, 0.0),
        Vec3::new(30.0, 0.2, 30.0),
        "ground",
    );
    block(&mut s, Vec3::new(-3.0, 0.75, 0.0), Vec3::splat(1.5), "red");
    block(&mut s, Vec3::new(3.0, 0.75, 0.0), Vec3::splat(1.5), "blue");
    block(
        &mut s,
        Vec3::new(0.0, 1.5, 8.0),
        Vec3::new(2.0, 3.0, 2.0),
        "green",
    );
    let sun = s.spawn("Sun", None);
    let mut t = Transform::from_position(Vec3::new(0.0, 10.0, 0.0));
    t.rotation = Quat::from_euler(EulerRot::YXZ, 0.6, 0.9, 0.0);
    s.set_transform(sun, t);
    s.add(
        sun,
        Light {
            kind: LightType::Directional,
            intensity: 1.5,
            shadows: Shadows::Soft,
            ..Default::default()
        },
    );
    s
}

const FRAMES: usize = 24;

fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/split_screen")
}

fn save(name: &str, rgba: Vec<u8>, w: u32, h: u32) {
    if std::env::var("MISTY_SPLIT_SHOTS").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(out_dir()).unwrap();
        let img = image::RgbaImage::from_raw(w, h, rgba).unwrap();
        img.save(out_dir().join(format!("{name}.png"))).unwrap();
    }
}

/// `rgba` (`w` wide) cropped to the pixel rect.
fn crop(rgba: &[u8], w: u32, (x, y, cw, ch): (u32, u32, u32, u32)) -> image::RgbaImage {
    image::RgbaImage::from_fn(cw, ch, |px, py| {
        let i = (((y + py) * w + x + px) * 4) as usize;
        image::Rgba([rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]])
    })
}

/// Mean RGB of an image.
fn mean(img: &image::RgbaImage) -> [f32; 3] {
    let mut sum = [0f64; 3];
    for p in img.pixels() {
        for c in 0..3 {
            sum[c] += f64::from(p[c]);
        }
    }
    let n = f64::from(img.width() * img.height());
    sum.map(|s| (s / n) as f32)
}

/// `camera` rendered alone, full screen, into a fresh `w` × `h` view target (its own
/// renderer, `FRAMES` frames).
fn alone(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    quality: &QualityLevel,
    scene: &Scene,
    mats: &Materials,
    camera: Entity,
    (w, h): (u32, u32),
) -> image::RgbaImage {
    let mut r = Renderer::with_queue(device, queue);
    r.set_quality(device, quality);
    let target = r.create_view_target(device, w, h);
    let mut cache = DrawCache::new();
    for _ in 0..FRAMES {
        let game = GameView::cached_for(scene, mats, &mut cache, camera).expect("camera");
        r.render_with_effects(device, queue, &target, &game.frame(), &game.effects);
    }
    image::RgbaImage::from_raw(w, h, read_back(device, queue, &target)).unwrap()
}

/// The split image of `scene` (`size`), rendered `FRAMES` times like the player does.
fn split(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    quality: &QualityLevel,
    scene: &Scene,
    mats: &Materials,
    size: (u32, u32),
) -> (Vec<u8>, RenderTarget) {
    let mut r = Renderer::with_queue(device, queue);
    r.set_quality(device, quality);
    let main = r.create_view_target(device, size.0, size.1);
    let mut split = SplitScreen::new();
    let mut cache = DrawCache::new();
    for _ in 0..FRAMES {
        let game = GameView::cached(scene, mats, &mut cache);
        if let Some(game) = &game {
            r.render_with_effects(device, queue, &main, &game.frame(), &game.effects);
        }
        split.render(&mut r, device, queue, &main, scene, mats, game.is_some());
    }
    (read_back(device, queue, &main), main)
}

fn levels() -> [(&'static str, QualityLevel); 4] {
    [
        ("low", QualityLevel::low()),
        ("medium", QualityLevel::medium()),
        ("high", QualityLevel::high()),
        ("ultra", QualityLevel::ultra()),
    ]
}

/// Loose: the halves go through the same passes, but screen-space effects near the rect's
/// edges and TAA jitter may round a little differently.
fn close(d: Drift) -> bool {
    d.mean <= 3.0 && d.changed <= 0.03
}

/// Two players side by side and no full-screen camera: the left half shows P1's view (the
/// red block), the right half P2's (the blue block), each like its camera rendered alone.
#[test]
fn split_screen_halves_match_each_camera_alone() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut s = arena();
    let p1 = camera(
        &mut s,
        "P1",
        Vec3::new(-3.0, 2.5, -6.0),
        Vec3::new(-3.0, 0.75, 0.0),
        [0.0, 0.0, 0.5, 1.0],
    );
    let p2 = camera(
        &mut s,
        "P2",
        Vec3::new(3.0, 2.5, -6.0),
        Vec3::new(3.0, 0.75, 0.0),
        [0.5, 0.0, 0.5, 1.0],
    );
    let mats = materials();
    // Odd width: the halves are 321 and 320 pixels.
    let size = (641, 360);
    let mut failures = Vec::new();
    for (level, quality) in levels() {
        let (img, _) = split(&device, &queue, &quality, &s, &mats, size);
        save(&format!("halves_{level}"), img.clone(), size.0, size.1);
        for (name, cam, vp, tint) in [
            ("P1", p1, [0.0, 0.0, 0.5, 1.0], 0),
            ("P2", p2, [0.5, 0.0, 0.5, 1.0], 2),
        ] {
            let rect = viewport_pixels(vp, size).unwrap();
            let got = crop(&img, size.0, rect);
            let want = alone(&device, &queue, &quality, &s, &mats, cam, (rect.2, rect.3));
            if std::env::var("MISTY_SPLIT_SHOTS").is_ok_and(|v| v == "1") {
                want.save(out_dir().join(format!("halves_{level}_{name}_alone.png")))
                    .unwrap();
            }
            let d = drift(&got, &want);
            eprintln!(
                "{level:6} {name}: mean {:.2} changed {:.2} %",
                d.mean,
                d.changed * 100.0
            );
            if !close(d) {
                failures.push(format!("{level} {name}: {d:?}"));
            }
            // Its block's colour dominates the middle of its half.
            let c = mean(&crop(
                &img,
                size.0,
                (
                    rect.0 + rect.2 / 3,
                    rect.1 + rect.3 / 3,
                    rect.2 / 3,
                    rect.3 / 3,
                ),
            ));
            let other = 2 - tint;
            if c[tint] < c[other] + 20.0 {
                failures.push(format!("{level} {name}: middle colour {c:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A full-screen camera and a minimap in the top-right corner: the corner shows the
/// minimap's view, the rest the main camera's.
#[test]
fn picture_in_picture_draws_over_the_full_screen_camera() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut s = arena();
    let main = camera(
        &mut s,
        "Main",
        Vec3::new(0.0, 3.0, -9.0),
        Vec3::new(0.0, 0.5, 0.0),
        [0.0, 0.0, 1.0, 1.0],
    );
    // Straight down over the red block.
    let map_vp = [0.7, 0.6, 0.3, 0.4];
    let map = camera(
        &mut s,
        "Minimap",
        Vec3::new(-3.0, 8.0, -0.01),
        Vec3::new(-3.0, 0.0, 0.0),
        map_vp,
    );
    s.world.get::<&mut Camera>(map).unwrap().depth = 5.0;
    let mats = materials();
    let size = (640, 360);
    let mut failures = Vec::new();
    for (level, quality) in levels() {
        let (img, _) = split(&device, &queue, &quality, &s, &mats, size);
        save(&format!("pip_{level}"), img.clone(), size.0, size.1);
        let rect = viewport_pixels(map_vp, size).unwrap();
        assert_eq!(rect, (448, 0, 192, 144));
        let got = crop(&img, size.0, rect);
        let want = alone(&device, &queue, &quality, &s, &mats, map, (rect.2, rect.3));
        let d = drift(&got, &want);
        eprintln!(
            "{level:6} minimap: mean {:.2} changed {:.2} %",
            d.mean,
            d.changed * 100.0
        );
        if !close(d) {
            failures.push(format!("{level} minimap: {d:?}"));
        }
        // Outside the corner: the main camera alone.
        let full = alone(&device, &queue, &quality, &s, &mats, main, size);
        let left = (0, 0, 440, 360);
        let d = drift(
            &crop(&img, size.0, left),
            &crop(full.as_raw(), size.0, left),
        );
        eprintln!(
            "{level:6} main: mean {:.2} changed {:.2} %",
            d.mean,
            d.changed * 100.0
        );
        if !close(d) {
            failures.push(format!("{level} main: {d:?}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

const LOBBY_LUA: &str = r#"-- Joins two players at the start and puts each in front of its own block.
local Lobby = {}
function Lobby:Start()
    local pim = self.gameObject:GetComponent("PlayerInputManager")
    pim:JoinPlayer()
    pim:JoinPlayer()
end
function Lobby:OnPlayerJoined(p)
    local n = self.gameObject:GetComponent("PlayerInputManager").playerCount
    local x = (n == 1) and -3 or 3
    p.transform.position = Vector3.new(x, 0, -6)
    Debug.Log("joined " .. p.name .. " as player " .. n)
end
return Lobby
"#;

/// Writes a small project (`MISTY_SPLIT_PROJECT` folder) whose scene joins two players with a
/// Player Input Manager, for running the real player on a GPU:
/// `MISTY_SCREENSHOT=shot.png misty_player <folder> --scene Assets/Scenes/Split.scene`.
#[test]
#[ignore = "writes a project for a by-hand player run"]
fn write_split_screen_project() {
    use crate::input::actions::{InputActionAsset, PlayerInput, PlayerInputManager};
    use crate::scene::{ScriptRef, Scripts};
    let root = PathBuf::from(std::env::var("MISTY_SPLIT_PROJECT").expect("MISTY_SPLIT_PROJECT"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::create_dir_all(root.join("Assets/Scenes")).unwrap();
    std::fs::write(root.join("Assets/Scripts/Lobby.lua"), LOBBY_LUA).unwrap();
    InputActionAsset::starter("Player")
        .save(&root.join("Assets/Player.inputactions"))
        .unwrap();
    // The player: a capsule with a camera behind and above it.
    let mut asset = Scene::new("asset");
    let player = asset.spawn("Player", None);
    asset.add(
        player,
        PlayerInput {
            actions: Some("Assets/Player.inputactions".into()),
            ..Default::default()
        },
    );
    let body = asset.spawn("Body", Some(player));
    asset.set_transform(body, Transform::from_position(Vec3::new(0.0, 1.0, 0.0)));
    asset.add(body, MeshFilter::primitive(PrimitiveMesh::Capsule));
    asset.add(body, MeshRenderer::default());
    let cam = asset.spawn("Cam", Some(player));
    let mut t = Transform::from_position(Vec3::new(0.0, 2.5, -3.0));
    t.rotation = Quat::from_euler(EulerRot::YXZ, 0.0, 0.25, 0.0);
    asset.set_transform(cam, t);
    asset.add(cam, Camera::default());
    crate::scene::prefab::Prefab::from_scene(&asset, player)
        .save(&root.join("Assets/Prefabs/Player.prefab"))
        .unwrap();
    // The level: the test arena (untextured materials) and the manager.
    let mut s = arena();
    s.name = "Split".into();
    let m = s.spawn("Player Input Manager", None);
    s.add(
        m,
        PlayerInputManager {
            player_prefab: Some("Assets/Prefabs/Player.prefab".into()),
            ..Default::default()
        },
    );
    s.add(
        m,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Lobby.lua".into(),
                ..Default::default()
            }],
        },
    );
    s.save(&root.join("Assets/Scenes/Split.scene")).unwrap();
}
