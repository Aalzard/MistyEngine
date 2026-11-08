//! The CutsceneDemo sample: an 18 s cutscene played by a Playable Director, then a Free Look
//! camera. A Camera track cuts and blends between a wide shot, a close-up that follows the
//! Hero walking in (Animation track), a Group Composer shot that keeps all three characters
//! (a Target Group) in view, and an explosion shot. At 12 s a Signal marker calls
//! `OnExplosion` on `CutsceneDemo.lua`, which fires the Barrel's Impulse Source: every camera
//! listens, so the shot shakes. A Control track switches the explosion on and plays a
//! sub-timeline that flickers a flash light. When the director stops, the brain blends to the
//! Free Look camera (highest priority, held back by the timeline until then) with a custom
//! curve from a blend list asset. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_cutscene_demo`; the other test checks the
//! committed files still match and plays the scene headlessly.

use std::path::{Path, PathBuf};

use glam::Vec3;
use misty_engine::animation::{AnimationClip, Property};
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    Camera, DirectorWrapMode, Light, MeshFilter, MeshRenderer, PlayableDirector, PrimitiveMesh,
    Scene, ScriptRef, Scripts, Transform,
};
use misty_engine::timeline::{Marker, TimelineAsset, TimelineClip, TrackKind};
use misty_engine::vcam::{
    AimKind, BlendDefinition, BlendList, BlendStyle, BodyKind, CameraBrain, Composer, CustomBlend,
    GroupMember, ImpulseListener, ImpulseShape, ImpulseSource, TargetGroup, VirtualCamera,
};

const SCENE: &str = "Assets/Scenes/CutsceneDemo.scene";
const TIMELINE: &str = "Assets/Timelines/Cutscene.timeline";
const LIGHTS: &str = "Assets/Timelines/CutsceneLights.timeline";
const WALK: &str = "Assets/Animations/CutsceneHeroWalk.anim";
const BLENDS: &str = "Assets/Cameras/CutsceneBlends.blends";
const SCRIPT: &str = "Assets/Scripts/CutsceneDemo.lua";
const GROUND: &str = "Assets/Materials/VCam Ground.mat";
const PILLAR: &str = "Assets/Materials/VCam Pillar.mat";
const HERO_MAT: &str = "Assets/Materials/Red.mat";
const ALLY_MAT: &str = "Assets/Materials/VCam Runner.mat";
const RIVAL_MAT: &str = "Assets/Materials/Lightmap Green.mat";
const FIRE_MAT: &str = "Assets/Materials/Lantern Glow.mat";

/// When the barrel goes up.
const EXPLOSION_AT: f32 = 12.0;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// 10 s: the Hero walks in along +X and stops beside the others.
fn hero_walk() -> AnimationClip {
    let mut c = AnimationClip {
        length: 10.0,
        looping: false,
        ..Default::default()
    };
    for (t, x) in [(0.0, -7.0), (7.0, -1.6), (10.0, -1.6)] {
        c.set_key("", Property::Position(0), t, x);
    }
    c.set_key("", Property::Position(1), 0.0, 1.0);
    c.set_key("", Property::Position(2), 0.0, -0.5);
    c
}

const SCRIPT_SOURCE: &str = r#"-- CutsceneDemo: logs the cutscene's cues, shakes the cameras on the explosion signal, and
-- says when the director hands over to the Free Look camera.
local CutsceneDemo = {}

function CutsceneDemo:Start()
    local director = self.gameObject:GetComponent("PlayableDirector")
    director.stopped:AddListener(function()
        print("Cutscene over: Free Look camera")
    end)
end

-- Signal track markers (the marker's text is the parameter).
function CutsceneDemo:OnCue(text)
    print("Cue: " .. text)
end

-- The explosion signal: every virtual camera with an Impulse Listener shakes.
function CutsceneDemo:OnExplosion()
    print("Boom!")
    Impulse.GenerateImpulse(GameObject.Find("Barrel"))
end

return CutsceneDemo
"#;

/// The flash light's flicker (played as a sub-timeline by the Control track).
fn lights_timeline() -> TimelineAsset {
    let mut t = TimelineAsset::default();
    let flash = t.add_track(TrackKind::Activation, None);
    let tr = t.track_mut(&flash).expect("added");
    tr.name = "Flash".into();
    tr.clips = [(0.0, 0.15), (0.3, 0.15), (0.7, 0.2), (1.2, 0.3)]
        .into_iter()
        .map(|(start, duration)| TimelineClip {
            name: "On".into(),
            start,
            duration,
            ..Default::default()
        })
        .collect();
    t
}

/// A Camera shot or Control clip pointing at director reference `r`.
fn shot(name: &str, start: f32, duration: f32, r: &str) -> TimelineClip {
    TimelineClip {
        name: name.into(),
        start,
        duration,
        reference: Some(r.into()),
        ..Default::default()
    }
}

/// Clip references → scene object names (the director's references).
const REFERENCES: [(&str, &str); 6] = [
    ("r1", "Wide Cam"),
    ("r2", "Hero Cam"),
    ("r3", "Group Cam"),
    ("r4", "Explosion Cam"),
    ("r5", "Explosion"),
    ("r6", "Lights Director"),
];

fn timeline() -> TimelineAsset {
    let mut t = TimelineAsset::default();
    let cams = t.add_track(TrackKind::Camera, None);
    let walk = t.add_track(TrackKind::Animation, None);
    let turn = t.add_track(TrackKind::Animation, None);
    let fx = t.add_track(TrackKind::Control, None);
    let lights = t.add_track(TrackKind::Control, None);
    let cues = t.add_track(TrackKind::Signal, None);
    let name = |t: &mut TimelineAsset, id: &str, n: &str| {
        t.track_mut(id).expect("added").name = n.into();
    };
    name(&mut t, &cams, "Cameras");
    name(&mut t, &walk, "Hero Walk");
    name(&mut t, &turn, "Ally Turn");
    name(&mut t, &fx, "Effects");
    name(&mut t, &lights, "Lights");
    // Keys recorded straight onto the track (record mode): the Ally turns to greet the Hero.
    let mut greet = AnimationClip {
        looping: false,
        ..Default::default()
    };
    greet.set_key("", Property::Rotation(1), 6.0, 0.0);
    greet.set_key("", Property::Rotation(1), 8.0, -70.0);
    t.track_mut(&turn).expect("added").recorded = Some(greet);
    name(&mut t, &cues, "Cues");
    // Cut to the close-up at 4.5 s, blend to the group over 7.5–8.5 s, cut to the explosion,
    // blend back to the wide shot over 14.5–15.5 s.
    t.track_mut(&cams).expect("added").clips = vec![
        shot("Wide Cam", 0.0, 4.5, "r1"),
        shot("Hero Cam", 4.5, 4.0, "r2"),
        shot("Group Cam", 7.5, 4.5, "r3"),
        shot("Explosion Cam", EXPLOSION_AT, 3.5, "r4"),
        shot("Wide Cam", 14.5, 3.5, "r1"),
    ];
    t.track_mut(&walk).expect("added").clips = vec![TimelineClip {
        name: "HeroWalk".into(),
        asset: Some(WALK.into()),
        duration: 10.0,
        ..Default::default()
    }];
    let mut boom = shot("Explosion", EXPLOSION_AT, 2.5, "r5");
    boom.control.impulse = false;
    let mut flicker = shot("Flash Flicker", EXPLOSION_AT, 2.0, "r6");
    flicker.control.activation = false;
    t.track_mut(&fx).expect("added").clips = vec![boom];
    t.track_mut(&lights).expect("added").clips = vec![flicker];
    let cue = |time: f32, method: &str, text: Option<&str>| Marker {
        time,
        method: method.into(),
        parameter: text.map(Into::into),
        retroactive: false,
    };
    t.track_mut(&cues).expect("added").markers = vec![
        cue(0.0, "OnCue", Some("Action!")),
        cue(EXPLOSION_AT, "OnExplosion", None),
        cue(17.5, "OnCue", Some("That's a wrap")),
    ];
    t
}

/// Wide Cam → Free Look Cam: a slow-in, fast-out custom curve over 2.5 s.
fn blend_list() -> BlendList {
    BlendList {
        blends: vec![CustomBlend {
            from: "Wide Cam".into(),
            to: "Free Look Cam".into(),
            blend: BlendDefinition {
                style: BlendStyle::Custom,
                duration: 2.5,
                curve: [0.6, 0.0, 0.2, 1.0],
            },
        }],
    }
}

fn mesh(s: &mut Scene, name: &str, kind: PrimitiveMesh, t: Transform, mat: &str) -> Entity {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(kind));
    s.add(
        e,
        MeshRenderer {
            material: Some(mat.into()),
            ..Default::default()
        },
    );
    e
}

fn listening(vc: VirtualCamera) -> VirtualCamera {
    VirtualCamera {
        impulse: ImpulseListener {
            enabled: true,
            ..Default::default()
        },
        ..vc
    }
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("CutsceneDemo");
    let wide_at = Vec3::new(0.0, 7.0, -16.0);
    let main = s.find_by_name("Main Camera").expect("default camera");
    s.set_transform(
        main,
        Transform::from_position(wide_at).with_euler(Vec3::new(22.0, 0.0, 0.0)),
    );
    s.add(
        main,
        CameraBrain {
            default_blend: BlendDefinition {
                style: BlendStyle::EaseInOut,
                duration: 1.5,
                ..Default::default()
            },
            blend_list: Some(BLENDS.into()),
            ..Default::default()
        },
    );
    let mut ground = Transform::from_position(Vec3::ZERO);
    ground.scale = Vec3::new(3.0, 1.0, 3.0);
    mesh(&mut s, "Ground", PrimitiveMesh::Plane, ground, GROUND);
    for (i, x) in [-9.0f32, 9.0].into_iter().enumerate() {
        let mut t = Transform::from_position(Vec3::new(x, 2.0, 6.0));
        t.scale = Vec3::new(1.2, 4.0, 1.2);
        mesh(
            &mut s,
            &format!("Pillar {}", i + 1),
            PrimitiveMesh::Cube,
            t,
            PILLAR,
        );
    }

    // The three characters (capsules, 2 m tall).
    let at = |x: f32, z: f32| Transform::from_position(Vec3::new(x, 1.0, z));
    let hero = mesh(
        &mut s,
        "Hero",
        PrimitiveMesh::Capsule,
        at(-7.0, -0.5),
        HERO_MAT,
    );
    let ally = mesh(
        &mut s,
        "Ally",
        PrimitiveMesh::Capsule,
        at(0.4, 0.6),
        ALLY_MAT,
    );
    let rival = mesh(
        &mut s,
        "Rival",
        PrimitiveMesh::Capsule,
        at(2.6, -0.8),
        RIVAL_MAT,
    );
    let trio = s.spawn("Trio", None);
    s.add(
        trio,
        TargetGroup {
            members: [hero, ally, rival]
                .into_iter()
                .map(|m| GroupMember {
                    target: Some(m),
                    weight: 1.0,
                    radius: 0.8,
                })
                .collect(),
            ..Default::default()
        },
    );

    // The barrel that explodes, the explosion and its flash.
    let mut barrel_t = Transform::from_position(Vec3::new(6.0, 0.6, 4.0));
    barrel_t.scale = Vec3::new(0.8, 0.6, 0.8);
    let barrel = mesh(&mut s, "Barrel", PrimitiveMesh::Cylinder, barrel_t, PILLAR);
    s.add(
        barrel,
        ImpulseSource {
            shape: ImpulseShape::Explosion,
            default_velocity: Vec3::new(0.0, -0.35, 0.15),
            attack: 0.02,
            sustain: 0.15,
            decay: 1.1,
            impact_radius: 40.0,
            dissipation_distance: 40.0,
            ..Default::default()
        },
    );
    let mut fire_t = Transform::from_position(Vec3::new(6.0, 1.4, 4.0));
    fire_t.scale = Vec3::splat(2.4);
    let fire = mesh(&mut s, "Explosion", PrimitiveMesh::Sphere, fire_t, FIRE_MAT);
    s.game_object_mut(fire).active = false;
    let flash = s.spawn("Flash", None);
    s.set_transform(flash, Transform::from_position(Vec3::new(6.0, 3.0, 3.0)));
    s.add(
        flash,
        Light {
            intensity: 8.0,
            range: 16.0,
            color: [1.0, 0.6, 0.25],
            ..Default::default()
        },
    );
    s.game_object_mut(flash).active = false;
    let lights = s.spawn("Lights Director", None);
    let mut ld = PlayableDirector {
        playable: Some(LIGHTS.into()),
        play_on_awake: false,
        wrap_mode: DirectorWrapMode::None,
        ..Default::default()
    };
    let flash_track = lights_timeline().tracks[0].id.clone();
    ld.set_binding(&flash_track, Some(flash));
    s.add(lights, ld);

    // Virtual cameras (all listen for impulses).
    let rig = s.spawn("Cameras", None);
    let cam = |s: &mut Scene, name: &str, at: Vec3, vc: VirtualCamera| {
        let e = s.spawn(name, Some(rig));
        s.set_transform(
            e,
            Transform::from_position(at).with_euler(Vec3::new(20.0, 0.0, 0.0)),
        );
        s.add(e, listening(vc));
        e
    };
    let still = |look: Entity| VirtualCamera {
        priority: 5,
        look_at: Some(look),
        body: BodyKind::DoNothing,
        aim: AimKind::HardLookAt,
        ..Default::default()
    };
    cam(&mut s, "Wide Cam", wide_at, still(trio));
    let mut close = VirtualCamera {
        priority: 5,
        follow: Some(hero),
        look_at: Some(hero),
        composer: Composer {
            tracked_offset: Vec3::new(0.0, 0.6, 0.0),
            screen_x: 0.4,
            ..Default::default()
        },
        ..Default::default()
    };
    close.transposer.offset = Vec3::new(1.4, 1.6, -3.2);
    close.transposer.damping = Vec3::splat(0.4);
    cam(&mut s, "Hero Cam", Vec3::new(-5.6, 2.6, -3.7), close);
    let mut group = VirtualCamera {
        priority: 5,
        follow: Some(trio),
        look_at: Some(trio),
        ..VirtualCamera::group_framing()
    };
    group.transposer.offset = Vec3::new(0.0, 3.5, -8.0);
    group.group_framing.framing_size = 0.7;
    cam(&mut s, "Group Cam", Vec3::new(0.0, 4.5, -8.0), group);
    cam(
        &mut s,
        "Explosion Cam",
        Vec3::new(10.0, 3.0, -3.0),
        still(barrel),
    );
    let mut free = VirtualCamera {
        priority: 20,
        follow: Some(hero),
        look_at: Some(hero),
        ..VirtualCamera::free_look()
    };
    free.composer.tracked_offset = Vec3::new(0.0, 0.5, 0.0);
    free.orbital.x_axis.value = -20.0;
    cam(&mut s, "Free Look Cam", Vec3::new(-2.6, 3.5, -3.8), free);

    // The director plays the cutscene on awake.
    let director = s.spawn("Cutscene Director", None);
    let t = timeline();
    let mut d = PlayableDirector {
        playable: Some(TIMELINE.into()),
        wrap_mode: DirectorWrapMode::None,
        ..Default::default()
    };
    let id = |name: &str| {
        t.tracks
            .iter()
            .find(|x| x.name == name)
            .expect("track")
            .id
            .clone()
    };
    d.set_binding(&id("Cameras"), Some(main));
    d.set_binding(&id("Hero Walk"), Some(hero));
    d.set_binding(&id("Ally Turn"), Some(ally));
    d.set_binding(&id("Cues"), Some(director));
    for (r, name) in REFERENCES {
        d.set_reference(r, s.find_by_name(name));
    }
    s.add(director, d);
    s.add(
        director,
        Scripts {
            list: vec![ScriptRef {
                path: SCRIPT.into(),
                ..Default::default()
            }],
        },
    );
    s
}

/// A `.meta` with a fresh GUID unless the asset (or folder) has one.
fn ensure_meta(path: &Path) {
    if misty_engine::asset_db::meta::read_guid(path).is_none() {
        misty_engine::asset_db::meta::write_guid(path, misty_engine::asset_db::Guid::new())
            .unwrap();
    }
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_cutscene_demo() {
    let root = project();
    std::fs::create_dir_all(root.join("Assets/Cameras")).unwrap();
    ensure_meta(&root.join("Assets/Cameras"));
    hero_walk().save(&root.join(WALK)).unwrap();
    ensure_meta(&root.join(WALK));
    std::fs::write(root.join(SCRIPT), SCRIPT_SOURCE).unwrap();
    ensure_meta(&root.join(SCRIPT));
    blend_list().save(&root.join(BLENDS)).unwrap();
    ensure_meta(&root.join(BLENDS));
    lights_timeline().save(&root.join(LIGHTS)).unwrap();
    ensure_meta(&root.join(LIGHTS));
    timeline().save(&root.join(TIMELINE)).unwrap();
    ensure_meta(&root.join(TIMELINE));
    demo_scene().save(&root.join(SCENE)).unwrap();
    ensure_meta(&root.join(SCENE));
}

/// Screen position (0..1) of `p` for a camera pose, and whether it is in front.
fn on_screen(pose: &misty_engine::vcam::CamState, aspect: f32, p: Vec3) -> (f32, f32, bool) {
    let local = pose.rotation.inverse() * (p - pose.position);
    let tv = (pose.fov.to_radians() * 0.5).tan();
    let x = 0.5 + 0.5 * local.x / (local.z * tv * aspect);
    let y = 0.5 - 0.5 * local.y / (local.z * tv);
    (x, y, local.z > 0.0)
}

#[test]
fn cutscene_demo_matches_its_generator_and_plays() {
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::Input;

    let root = project();
    crate::test_util::assert_close(
        &AnimationClip::load(&root.join(WALK)).unwrap(),
        &hero_walk(),
        WALK,
    );
    assert_eq!(
        std::fs::read_to_string(root.join(SCRIPT))
            .unwrap()
            .replace("\r\n", "\n"),
        SCRIPT_SOURCE
    );
    assert_eq!(BlendList::load(&root.join(BLENDS)).unwrap(), blend_list());
    for (path, t) in [(TIMELINE, timeline()), (LIGHTS, lights_timeline())] {
        crate::test_util::assert_close(&TimelineAsset::load(&root.join(path)).unwrap(), &t, path);
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_cutscene_demo",
    );
    let (mut scene, report) = Scene::from_json("CutsceneDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let (audio, _) = misty_engine::audio::Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root.clone(), &Default::default(), audio);
    let find = |scene: &Scene, n: &str| scene.find_by_name(n).unwrap();
    let main = find(&scene, "Main Camera");
    let fire = find(&scene, "Explosion");
    let flash = find(&scene, "Flash");
    let boom_cam = find(&scene, "Explosion Cam");
    let members = ["Hero", "Ally", "Rival"].map(|n| find(&scene, n));
    let aspect = 16.0 / 9.0;
    let mut time = Time::default();
    let mut live: Vec<String> = Vec::new();
    let mut blended = false;
    let mut shake = 0.0f32;
    let mut flicker = (false, false);
    let mut framed = 0;
    let mut logs = Vec::new();
    let fps = 30.0;
    for frame in 0..(22 * 30) {
        time.advance(1.0 / fps);
        let input = Input {
            screen_size: glam::Vec2::new(1280.0, 720.0),
            ..Default::default()
        };
        rt.update(&mut scene, &time, input);
        logs.extend(rt.take_logs().into_iter().map(|(_, t)| t));
        // The director's time: its first update shows 0.
        let t = frame as f32 / fps;
        let name = rt
            .with_cameras(|r| r.live_camera(main))
            .map(|e| scene.name(e))
            .unwrap_or_default();
        if live.last() != Some(&name) {
            live.push(name);
        }
        if (7.5..8.5).contains(&t) {
            blended |= rt.with_cameras(|r| r.is_blending(main));
        }
        // The group shot keeps all three in view.
        if (9.5..11.5).contains(&t) {
            let pose = rt.with_cameras(|r| r.brain_output(main)).unwrap();
            for m in members {
                let (x, y, front) = on_screen(&pose, aspect, scene.world_position(m));
                assert!(
                    front && (0.05..0.95).contains(&x) && (0.05..0.95).contains(&y),
                    "t {t}: {} at ({x}, {y})",
                    scene.name(m)
                );
            }
            framed += 1;
        }
        // The explosion shot shakes: the brain moves off the camera's own (raw) pose.
        if (EXPLOSION_AT + 0.05..EXPLOSION_AT + 1.2).contains(&t) {
            let d = scene.world_position(main) - scene.world_position(boom_cam);
            shake = shake.max(d.length());
            assert!(scene.game_object(fire).active, "t {t}: exploding");
        }
        if (EXPLOSION_AT..EXPLOSION_AT + 1.5).contains(&t) {
            let on = scene.game_object(flash).active;
            flicker = (flicker.0 || on, flicker.1 || !on);
        }
        if t > 15.0 {
            assert!(!scene.game_object(fire).active, "t {t}: reverted");
        }
    }
    assert_eq!(
        live,
        [
            "Wide Cam",
            "Hero Cam",
            "Group Cam",
            "Explosion Cam",
            "Wide Cam",
            "Free Look Cam"
        ]
    );
    assert!(blended, "the Hero → Group shot blends");
    assert!(framed > 50);
    assert!(shake > 0.05, "the impulse shook the shot: {shake}");
    assert_eq!(flicker, (true, true), "the sub-timeline flickers the flash");
    assert!(
        !rt.with_cameras(|r| r.is_blending(main)),
        "settled on Free Look"
    );
    assert_eq!(
        logs,
        [
            "Cue: Action!",
            "Boom!",
            "Cue: That's a wrap",
            "Cutscene over: Free Look camera"
        ]
    );
    // Main Camera's lens follows the shots (Camera component, not just the transform).
    assert!(scene.world.get::<&Camera>(main).is_ok());
}
