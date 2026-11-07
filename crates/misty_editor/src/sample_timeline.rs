//! The TimelineDemo sample: a 10 s sequence played by a Playable Director. An Animation track
//! moves a cube around a square path (easing in) and blends into a hop; a "Light & Sound"
//! group switches a lamp on with an Activation track and plays a chime on an Audio track at
//! the same moment; a Signal track and the markers row call `OnCue` on `TimelineDemo.lua`,
//! which logs each cue and "Timeline finished" when the director stops. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_timeline_sample`; the other test checks the
//! committed files still match and plays the scene headlessly.

use std::f32::consts::TAU;
use std::path::PathBuf;

use glam::Vec3;
use misty_engine::animation::{AnimationClip, Property, TangentMode};
use misty_engine::audio::wav::pcm16_mono;
use misty_engine::scene::{
    AudioSource, DirectorWrapMode, Light, MeshFilter, MeshRenderer, PlayableDirector,
    PrimitiveMesh, Scene, ScriptRef, Scripts, Transform,
};
use misty_engine::timeline::{Marker, TimelineAsset, TimelineClip, TrackKind};

const SCENE: &str = "Assets/Scenes/TimelineDemo.scene";
const TIMELINE: &str = "Assets/Timelines/TimelineDemo.timeline";
const PATH_CLIP: &str = "Assets/Animations/CubePath.anim";
const HOP_CLIP: &str = "Assets/Animations/CubeHop.anim";
const CHIME: &str = "Assets/Audio/Chime.wav";
const SCRIPT: &str = "Assets/Scripts/TimelineDemo.lua";
const RATE: u32 = 22050;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// 8 s around a square (corners every 2 s), turning once.
fn cube_path() -> AnimationClip {
    let mut c = AnimationClip {
        length: 8.0,
        looping: false,
        ..Default::default()
    };
    let corners = [
        (-3.0, -2.0),
        (3.0, -2.0),
        (3.0, 2.0),
        (-3.0, 2.0),
        (-3.0, -2.0),
    ];
    for (i, (x, z)) in corners.into_iter().enumerate() {
        let t = i as f32 * 2.0;
        c.set_key("", Property::Position(0), t, x);
        c.set_key("", Property::Position(1), t, 0.5);
        c.set_key("", Property::Position(2), t, z);
    }
    c.set_key("", Property::Rotation(1), 0.0, 0.0);
    c.set_key("", Property::Rotation(1), 8.0, 360.0);
    let spin = c.curves.len() - 1;
    for i in 0..2 {
        c.curves[spin].keys.set_tangent(i, TangentMode::Linear);
    }
    c
}

/// 3 s: two hops in place.
fn cube_hop() -> AnimationClip {
    let mut c = AnimationClip {
        length: 3.0,
        looping: false,
        ..Default::default()
    };
    for (t, y) in [
        (0.0, 0.5),
        (0.5, 2.0),
        (1.0, 0.5),
        (1.6, 1.4),
        (2.2, 0.5),
        (3.0, 0.5),
    ] {
        c.set_key("", Property::Position(1), t, y);
    }
    c
}

/// A soft bell: a few decaying partials, 1.5 s.
fn chime() -> Vec<u8> {
    let count = (1.5 * RATE as f32) as usize;
    let samples: Vec<i16> = (0..count)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let attack = (t / 0.004).min(1.0);
            let tone = [
                (880.0f32, 1.0f32, 3.0f32),
                (1320.0, 0.5, 4.5),
                (2217.0, 0.25, 6.0),
            ]
            .iter()
            .map(|(f, a, d)| a * (-d * t).exp() * (t * f * TAU).sin())
            .sum::<f32>();
            let tail = 1.0 - (t / 1.5).powi(4);
            (tone * attack * tail * 0.3 * i16::MAX as f32) as i16
        })
        .collect();
    pcm16_mono(&samples, RATE)
}

const SCRIPT_SOURCE: &str = r#"-- TimelineDemo: logs the timeline's signals, and when the director stops.
local TimelineDemo = {}

function TimelineDemo:Start()
    local director = self.gameObject:GetComponent("PlayableDirector")
    director.stopped:AddListener(function()
        print("Timeline finished")
    end)
end

-- Called by the Signal track and the markers row (the marker's text is the parameter).
function TimelineDemo:OnCue(text)
    print("Signal: " .. text)
end

return TimelineDemo
"#;

/// Track ids are fixed so the scene's bindings match.
fn timeline() -> TimelineAsset {
    let mut t = TimelineAsset::default();
    let cube = t.add_track(TrackKind::Animation, None);
    let group = t.add_track(TrackKind::Group, None);
    let lamp = t.add_track(TrackKind::Activation, Some(&group));
    let sound = t.add_track(TrackKind::Audio, Some(&group));
    let cues = t.add_track(TrackKind::Signal, None);
    let name = |t: &mut TimelineAsset, id: &str, n: &str| {
        t.track_mut(id).expect("added").name = n.into();
    };
    name(&mut t, &cube, "Cube");
    name(&mut t, &group, "Light & Sound");
    name(&mut t, &lamp, "Lamp");
    name(&mut t, &sound, "Chime");
    name(&mut t, &cues, "Cues");
    t.track_mut(&cube).expect("added").clips = vec![
        TimelineClip {
            name: "CubePath".into(),
            asset: Some(PATH_CLIP.into()),
            duration: 8.0,
            ease_in: 0.5,
            ..Default::default()
        },
        TimelineClip {
            name: "CubeHop".into(),
            asset: Some(HOP_CLIP.into()),
            start: 7.0,
            duration: 3.0,
            ..Default::default()
        },
    ];
    t.track_mut(&lamp).expect("added").clips = vec![TimelineClip {
        name: "Active".into(),
        start: 3.0,
        duration: 7.0,
        ..Default::default()
    }];
    t.track_mut(&sound).expect("added").clips = vec![TimelineClip {
        name: "Chime".into(),
        asset: Some(CHIME.into()),
        start: 3.0,
        duration: 1.5,
        ..Default::default()
    }];
    let cue = |time: f32, text: &str| Marker {
        time,
        method: "OnCue".into(),
        parameter: Some(text.into()),
        retroactive: false,
    };
    t.track_mut(&cues).expect("added").markers =
        vec![cue(3.0, "The light is on"), cue(9.0, "Last hop")];
    t.markers = vec![cue(0.0, "Action!")];
    t
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("TimelineDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 5.5, -9.0))
                .with_euler(Vec3::new(30.0, 0.0, 0.0)),
        );
    }
    let mesh = |s: &mut Scene, name: &str, p: PrimitiveMesh, pos: Vec3, mat: Option<&str>| {
        let e = s.spawn(name, None);
        s.set_transform(e, Transform::from_position(pos));
        s.add(e, MeshFilter::primitive(p));
        s.add(
            e,
            MeshRenderer {
                material: mat.map(Into::into),
                ..Default::default()
            },
        );
        e
    };
    let floor = mesh(&mut s, "Floor", PrimitiveMesh::Plane, Vec3::ZERO, None);
    let mut ft = s.transform(floor);
    ft.scale = Vec3::splat(1.2);
    s.set_transform(floor, ft);
    let cube = mesh(
        &mut s,
        "Cube",
        PrimitiveMesh::Cube,
        Vec3::new(-3.0, 0.5, -2.0),
        Some("Assets/Materials/Red.mat"),
    );
    mesh(
        &mut s,
        "Pedestal",
        PrimitiveMesh::Cylinder,
        Vec3::new(0.0, 0.0, 0.0),
        None,
    );

    // Off until the timeline switches it on.
    let lamp = s.spawn("Lamp", None);
    s.set_transform(lamp, Transform::from_position(Vec3::new(0.0, 2.2, 0.0)));
    s.add(
        lamp,
        Light {
            intensity: 4.0,
            range: 9.0,
            color: [1.0, 0.72, 0.35],
            ..Default::default()
        },
    );
    s.game_object_mut(lamp).active = false;

    let speaker = s.spawn("Speaker", None);
    s.set_transform(speaker, Transform::from_position(Vec3::new(0.0, 1.5, 0.0)));
    s.add(
        speaker,
        AudioSource {
            play_on_awake: false,
            ..Default::default()
        },
    );

    let director = s.spawn("Director", None);
    let mut d = PlayableDirector {
        playable: Some(TIMELINE.into()),
        wrap_mode: DirectorWrapMode::None,
        ..Default::default()
    };
    let t = timeline();
    for (track, object) in [
        ("Cube", cube),
        ("Lamp", lamp),
        ("Chime", speaker),
        ("Cues", director),
    ] {
        let id = t
            .tracks
            .iter()
            .find(|x| x.name == track)
            .expect("track")
            .id
            .clone();
        d.set_binding(&id, Some(object));
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

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_timeline_sample() {
    let root = project();
    cube_path().save(&root.join(PATH_CLIP)).unwrap();
    cube_hop().save(&root.join(HOP_CLIP)).unwrap();
    std::fs::write(root.join(CHIME), chime()).unwrap();
    std::fs::write(root.join(SCRIPT), SCRIPT_SOURCE).unwrap();
    timeline().save(&root.join(TIMELINE)).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn timeline_sample_matches_its_generator_and_plays() {
    let root = project();
    crate::test_util::assert_close(
        &AnimationClip::load(&root.join(PATH_CLIP)).unwrap(),
        &cube_path(),
        PATH_CLIP,
    );
    crate::test_util::assert_close(
        &AnimationClip::load(&root.join(HOP_CLIP)).unwrap(),
        &cube_hop(),
        HOP_CLIP,
    );
    assert_eq!(std::fs::read(root.join(CHIME)).unwrap(), chime());
    assert_eq!(
        std::fs::read_to_string(root.join(SCRIPT))
            .unwrap()
            .replace("\r\n", "\n"),
        SCRIPT_SOURCE
    );
    crate::test_util::assert_close(
        &TimelineAsset::load(&root.join(TIMELINE)).unwrap(),
        &timeline(),
        TIMELINE,
    );
    let (mut scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let (audio, log) = misty_engine::audio::Audio::null(root.clone());
    let mut rt = misty_engine::runtime::Runtime::start_with_audio(
        &mut scene,
        root.clone(),
        &Default::default(),
        audio,
    );
    let mut time = misty_engine::runtime::Time::default();
    let cube = scene.find_by_name("Cube").unwrap();
    let lamp = scene.find_by_name("Lamp").unwrap();
    let start = scene.transform(cube).position;
    let mut logs = Vec::new();
    let mut frame = |rt: &mut misty_engine::runtime::Runtime, scene: &mut Scene, n: usize| {
        for _ in 0..n {
            time.advance(1.0 / 30.0);
            rt.update(scene, &time, Default::default());
        }
        logs.extend(rt.take_logs().into_iter().map(|(_, t)| t));
    };
    frame(&mut rt, &mut scene, 60); // 2 s
    assert!(!scene.game_object(lamp).active, "lamp off before 3 s");
    let p = scene.transform(cube).position;
    assert!((p - start).length() > 2.0, "cube travels: {p}");
    frame(&mut rt, &mut scene, 60); // 4 s
    assert!(scene.game_object(lamp).active, "lamp on after 3 s");
    let played = log.lock().unwrap().played.clone();
    assert_eq!(played.len(), 1, "{played:?}");
    assert!(played[0].ends_with("Chime.wav"));
    frame(&mut rt, &mut scene, 200); // past the end
    assert_eq!(
        logs,
        [
            "Signal: Action!",
            "Signal: The light is on",
            "Signal: Last hop",
            "Timeline finished"
        ]
    );
}
