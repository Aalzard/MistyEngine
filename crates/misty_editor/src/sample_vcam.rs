//! VirtualCameraDemo sample scene in `SampleProject`: a capsule "Runner" jogging round an oval
//! track (`PathRunner.lua`) past a wall, watched by three virtual cameras — a damped follow
//! camera with Composer, Collider and handheld Noise, a wide crane shot and a tilted close-up.
//! `CameraDirector.lua` raises a different camera's priority every few seconds so the Camera
//! Brain on the Main Camera blends between them. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_vcam_demo`; the other test checks the
//! committed files still match and plays the scene.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    BoxCollider, MeshFilter, MeshRenderer, PrimitiveMesh, RectTransform, Scene, ScriptRef, Scripts,
    Transform, UiText,
};
use misty_engine::vcam::{
    AimKind, BindingMode, BlendDefinition, BlendStyle, BodyKind, CameraBrain, CameraCollider,
    Composer, CustomBlend, Lens, Noise, NoiseProfile, Transposer, VirtualCamera,
};

use crate::actions::{Action, Create};
use crate::project::ProjectBrowser;
use crate::state::EditorState;
use crate::ui_create::UiElement;

const SCENE: &str = "Assets/Scenes/VirtualCameraDemo.scene";
const RUNNER_SCRIPT: &str = "Assets/Scripts/PathRunner.lua";
const DIRECTOR_SCRIPT: &str = "Assets/Scripts/CameraDirector.lua";
const GROUND: &str = "Assets/Materials/VCam Ground.mat";
const TRACK: &str = "Assets/Materials/VCam Track.mat";
const WALL: &str = "Assets/Materials/VCam Wall.mat";
const RUNNER: &str = "Assets/Materials/VCam Runner.mat";
const PILLAR: &str = "Assets/Materials/VCam Pillar.mat";

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn materials() -> [(&'static str, Material); 5] {
    let m = |color: [f32; 4], smoothness| Material {
        color,
        smoothness,
        ..Default::default()
    };
    [
        (GROUND, m([0.36, 0.42, 0.33, 1.0], 0.1)),
        (TRACK, m([0.62, 0.36, 0.26, 1.0], 0.2)),
        (WALL, m([0.78, 0.76, 0.72, 1.0], 0.25)),
        (RUNNER, m([0.95, 0.6, 0.12, 1.0], 0.6)),
        (PILLAR, m([0.3, 0.45, 0.7, 1.0], 0.4)),
    ]
}

fn mesh(s: &mut Scene, name: &str, kind: PrimitiveMesh, t: Transform, material: &str) -> Entity {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(kind));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
    e
}

fn scaled(position: Vec3, scale: Vec3) -> Transform {
    Transform {
        position,
        scale,
        ..Default::default()
    }
}

fn script(path: &str) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            ..Default::default()
        }],
    }
}

/// Damped follow camera behind and inside the runner's left shoulder: on the near side of the
/// track the wall comes between them and the Collider pulls the camera in.
fn follow_cam(runner: Entity) -> VirtualCamera {
    VirtualCamera {
        priority: 10,
        follow: Some(runner),
        look_at: Some(runner),
        lens: Lens {
            fov: 55.0,
            ..Default::default()
        },
        body: BodyKind::Transposer,
        transposer: Transposer {
            binding: BindingMode::LockToTargetWithWorldUp,
            offset: Vec3::new(-4.5, 2.2, -6.0),
            damping: Vec3::new(0.6, 0.4, 0.8),
        },
        aim: AimKind::Composer,
        composer: Composer {
            tracked_offset: Vec3::new(0.0, 0.4, 0.0),
            screen_x: 0.55,
            screen_y: 0.55,
            dead_zone_width: 0.08,
            dead_zone_height: 0.06,
            soft_zone_width: 0.6,
            soft_zone_height: 0.6,
            horizontal_damping: 0.6,
            vertical_damping: 0.5,
        },
        noise: Noise {
            enabled: true,
            profile: NoiseProfile::Handheld,
            amplitude_gain: 1.0,
            frequency_gain: 1.0,
            seed: 1,
        },
        collider: CameraCollider {
            enabled: true,
            camera_radius: 0.3,
            minimum_distance: 1.0,
            damping: 0.6,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// High crane shot over the whole track, turning gently to keep the runner framed.
fn wide_cam(runner: Entity) -> VirtualCamera {
    VirtualCamera {
        priority: 5,
        look_at: Some(runner),
        lens: Lens {
            fov: 50.0,
            ..Default::default()
        },
        body: BodyKind::DoNothing,
        aim: AimKind::Composer,
        composer: Composer {
            dead_zone_width: 0.3,
            dead_zone_height: 0.25,
            soft_zone_width: 0.8,
            soft_zone_height: 0.8,
            horizontal_damping: 1.5,
            vertical_damping: 1.5,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// In front of the runner looking back at it, tilted and shaking a little.
fn close_cam(runner: Entity) -> VirtualCamera {
    VirtualCamera {
        priority: 5,
        follow: Some(runner),
        look_at: Some(runner),
        lens: Lens {
            fov: 35.0,
            dutch: 8.0,
            ..Default::default()
        },
        body: BodyKind::Transposer,
        transposer: Transposer {
            binding: BindingMode::LockToTargetWithWorldUp,
            offset: Vec3::new(1.4, 1.5, 3.2),
            damping: Vec3::new(0.3, 0.3, 0.3),
        },
        aim: AimKind::HardLookAt,
        noise: Noise {
            enabled: true,
            profile: NoiseProfile::Shake,
            amplitude_gain: 0.25,
            frequency_gain: 0.6,
            seed: 7,
        },
        ..Default::default()
    }
}

fn label(s: &mut EditorState) {
    crate::commands::apply(
        s,
        Action::Create {
            what: Create::Ui(UiElement::Text),
            parent: None,
        },
    );
    let e = s.selection.active().expect("created object is selected");
    s.scene.game_object_mut(e).name = "Camera Label".into();
    let w = &mut s.scene.world;
    *w.get::<&mut RectTransform>(e).unwrap() = RectTransform {
        anchor_min: Vec2::new(0.0, 1.0),
        anchor_max: Vec2::new(0.0, 1.0),
        pivot: Vec2::new(0.0, 1.0),
        anchored_position: Vec2::new(20.0, -16.0),
        size_delta: Vec2::new(760.0, 36.0),
    };
    let mut t = w.get::<&mut UiText>(e).unwrap();
    t.text = "Live: Follow Cam".into();
    t.font_size = 22.0;
    t.color = [1.0; 4];
}

pub fn demo_scene() -> Scene {
    let mut st = EditorState::new(ProjectBrowser::open(
        std::env::temp_dir().join("misty_vcam_demo_build"),
    ));
    st.scene = Scene::default_3d("VirtualCameraDemo");
    let s = &mut st.scene;
    let cam = s.find_by_name("Main Camera").expect("default camera");
    s.set_transform(
        cam,
        Transform::from_position(Vec3::new(-4.0, 3.0, -14.0)).with_euler(Vec3::new(8.0, 10.0, 0.0)),
    );
    s.add(
        cam,
        CameraBrain {
            default_blend: BlendDefinition {
                style: BlendStyle::EaseInOut,
                duration: 2.0,
            },
            custom_blends: vec![CustomBlend {
                from: String::new(),
                to: "Close-Up Cam".into(),
                blend: BlendDefinition {
                    style: BlendStyle::Linear,
                    duration: 1.0,
                },
            }],
            ..Default::default()
        },
    );
    if let Some(sun) = s.find_by_name("Directional Light") {
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 10.0, 0.0))
                .with_euler(Vec3::new(45.0, -35.0, 0.0)),
        );
    }

    let ground = mesh(
        s,
        "Ground",
        PrimitiveMesh::Plane,
        scaled(Vec3::ZERO, Vec3::new(5.0, 1.0, 4.0)),
        GROUND,
    );
    s.add(
        ground,
        BoxCollider {
            size: Vec3::new(10.0, 0.02, 10.0),
            ..Default::default()
        },
    );
    // Track markers: low tiles along the oval.
    let track = s.spawn("Track", None);
    for i in 0..24 {
        let a = i as f32 / 24.0 * std::f32::consts::TAU;
        let p = Vec3::new(a.cos() * 11.0, 0.02, a.sin() * 7.0);
        let heading = (-a.sin() * 11.0).atan2(a.cos() * 7.0).to_degrees();
        let tile = mesh(
            s,
            &format!("Tile {}", i + 1),
            PrimitiveMesh::Cube,
            scaled(p, Vec3::new(1.4, 0.04, 2.6)).with_euler(Vec3::new(0.0, heading, 0.0)),
            TRACK,
        );
        s.set_parent(tile, Some(track), None, true);
    }
    // The wall between the near straight and the follow camera.
    let wall = mesh(
        s,
        "Wall",
        PrimitiveMesh::Cube,
        scaled(Vec3::new(0.0, 1.5, -4.4), Vec3::new(11.0, 3.0, 0.4)),
        WALL,
    );
    s.add(wall, BoxCollider::default());
    for (i, x) in [-8.0f32, 8.0].into_iter().enumerate() {
        let p = mesh(
            s,
            &format!("Pillar {}", i + 1),
            PrimitiveMesh::Cylinder,
            scaled(Vec3::new(x, 2.0, 0.0), Vec3::new(1.2, 2.0, 1.2)),
            PILLAR,
        );
        s.add(p, BoxCollider::default());
    }

    let runner = mesh(
        s,
        "Runner",
        PrimitiveMesh::Capsule,
        Transform::from_position(Vec3::new(0.0, 1.0, -7.0)).with_euler(Vec3::new(0.0, 90.0, 0.0)),
        RUNNER,
    );
    s.add(runner, script(RUNNER_SCRIPT));
    // A visor so the facing direction reads.
    let visor = s.spawn("Visor", Some(runner));
    s.set_transform(
        visor,
        scaled(Vec3::new(0.0, 0.45, 0.4), Vec3::new(0.7, 0.18, 0.3)),
    );
    s.add(visor, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.add(
        visor,
        MeshRenderer {
            material: Some(PILLAR.into()),
            ..Default::default()
        },
    );

    let rig = s.spawn("Cameras", None);
    for (name, vc, at) in [
        ("Follow Cam", follow_cam(runner), Vec3::new(-6.0, 3.2, -9.0)),
        (
            "Wide Shot Cam",
            wide_cam(runner),
            Vec3::new(0.0, 16.0, -24.0),
        ),
        ("Close-Up Cam", close_cam(runner), Vec3::new(3.0, 2.5, -6.0)),
    ] {
        let e = s.spawn(name, Some(rig));
        s.set_transform(
            e,
            Transform::from_position(at).with_euler(Vec3::new(30.0, 0.0, 0.0)),
        );
        s.add(e, vc);
    }
    let director = s.spawn("Camera Director", None);
    s.add(director, script(DIRECTOR_SCRIPT));
    label(&mut st);
    st.scene
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_vcam_demo() {
    let root = project();
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn vcam_demo_matches_its_generator_and_plays() {
    use misty_engine::audio::Audio;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::Input;

    let root = project();
    for (path, m) in materials() {
        assert_eq!(Material::load(&root.join(path)).unwrap(), m, "{path}");
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert!(
        text.replace("\r\n", "\n") == demo_scene().to_json(),
        "regenerate with write_vcam_demo"
    );
    let (mut scene, report) = Scene::from_json("VirtualCameraDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let mut rt = Runtime::start_with_audio(
        &mut scene,
        root.clone(),
        &Default::default(),
        Audio::null(root).0,
    );
    let cam = scene.find_by_name("Main Camera").unwrap();
    let names = |scene: &Scene, e: Option<Entity>| e.map(|e| scene.name(e));
    let mut time = Time::default();
    let mut live = Vec::new();
    let mut blended = false;
    let mut pulled_in = false;
    let follow = scene.find_by_name("Follow Cam").unwrap();
    let runner = scene.find_by_name("Runner").unwrap();
    for frame in 0..(19 * 30) {
        time.advance(1.0 / 30.0);
        let input = Input {
            screen_size: glam::Vec2::new(1280.0, 720.0),
            ..Default::default()
        };
        rt.update(&mut scene, &time, input);
        let now = rt.with_cameras(|r| r.live_camera(cam));
        let name = names(&scene, now);
        if live.last() != Some(&name) {
            live.push(name);
        }
        blended |= rt.with_cameras(|r| r.is_blending(cam));
        // Early on the runner is on the near straight with the wall between it and the
        // follow camera's offset: the collider pulls the camera in front of the wall.
        if frame == 30 {
            let out = rt.with_cameras(|r| r.camera_output(follow)).unwrap();
            let raw = scene.world_position(follow);
            assert!(
                raw.z > -4.2,
                "unobstructed offset is behind the wall: {raw}"
            );
            assert!(
                out.position.z < -4.6,
                "pulled in front of the wall: {}",
                out.position
            );
            pulled_in = true;
        }
    }
    assert!(pulled_in);
    assert!(blended, "the brain blended between shots");
    let expected: Vec<Option<String>> = [
        "Follow Cam",
        "Wide Shot Cam",
        "Follow Cam",
        "Close-Up Cam",
        "Follow Cam",
    ]
    .iter()
    .map(|n| Some(n.to_string()))
    .collect();
    assert_eq!(live, expected);
    // The runner moved round the track and the camera kept looking at it.
    let p = scene.world_position(runner);
    assert!(p.distance(Vec3::new(0.0, 1.0, -7.0)) > 3.0, "{p}");
    let label = scene.find_by_name("Camera Label").unwrap();
    let shown = scene.world.get::<&UiText>(label).unwrap().text.clone();
    assert!(shown.starts_with("Live: "), "{shown}");
    let logs = rt.take_logs();
    assert!(logs.is_empty(), "{logs:?}");
}
