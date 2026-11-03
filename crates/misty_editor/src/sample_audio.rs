//! The AudioDemo sample: a looping music track routed to Master ▸ Music, a humming sphere
//! orbiting the listener on Master ▸ SFX (3D: panning, distance rolloff, Doppler), and
//! `AudioDemo.lua` switching mixer snapshots (Normal / Muffled / Cave) and driving exposed
//! parameters from the keyboard, with a UI Text hint. The mixer and the WAV clips are made here
//! too. Regenerate with `cargo test -p misty_editor -- --ignored write_audio_demo`; the other
//! tests check the committed files still match and play the scene headlessly.

use std::f32::consts::TAU;
use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::audio::mixer::{AudioMixer, MASTER, effect_key, volume_key};
use misty_engine::audio::wav::{pcm16_mono, sine_beep};
use misty_engine::audio::{EffectKind, output_ref};
use misty_engine::scene::{
    AudioRolloff, AudioSource, MeshFilter, MeshRenderer, PrimitiveMesh, RectTransform, Scene,
    ScriptRef, Scripts, Transform, UiText,
};

use crate::actions::{Action, Create};
use crate::project::ProjectBrowser;
use crate::state::EditorState;
use crate::ui_create::UiElement;

const SCENE: &str = "Assets/Scenes/AudioDemo.scene";
const MIXER: &str = "Assets/Audio/AudioDemo.mixer";
const RATE: u32 = 22050;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

// ---- clips ------------------------------------------------------------------------------------

/// Mono samples of `seconds` from `f(t)` (-1..1).
fn synth(seconds: f32, f: impl Fn(f32) -> f32) -> Vec<i16> {
    let count = (seconds * RATE as f32) as usize;
    (0..count)
        .map(|i| (f(i as f32 / RATE as f32).clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect()
}

/// 4 s loop: a plucked A-minor pentatonic arpeggio over a soft bass. Every note and the bass
/// end on whole cycles, so the loop is seamless.
fn music_loop() -> Vec<u8> {
    const NOTES: [f32; 16] = [
        220.0, 261.63, 293.66, 329.63, 392.0, 329.63, 293.66, 261.63, 220.0, 329.63, 392.0, 440.0,
        392.0, 329.63, 293.66, 261.63,
    ];
    let step = 0.25;
    let samples = synth(4.0, |t| {
        let i = ((t / step) as usize).min(NOTES.len() - 1);
        let local = t - i as f32 * step;
        let env = (local / 0.005).min(1.0) * (-7.0 * local).exp() * (1.0 - local / step);
        let f = NOTES[i];
        let pluck = (local * f * TAU).sin() + 0.3 * (local * f * 2.0 * TAU).sin();
        let bass = (t * 55.0 * TAU).sin() * 0.25;
        pluck * env * 0.35 + bass
    });
    pcm16_mono(&samples, RATE)
}

/// 1 s loop: a 110 Hz hum with harmonics and a 4 Hz wobble (whole cycles: seamless).
fn hum_loop() -> Vec<u8> {
    let samples = synth(1.0, |t| {
        let wobble = 0.8 + 0.2 * (t * 4.0 * TAU).sin();
        let tone = (t * 110.0 * TAU).sin()
            + 0.5 * (t * 220.0 * TAU).sin()
            + 0.25 * (t * 330.0 * TAU).sin();
        tone * wobble * 0.3
    });
    pcm16_mono(&samples, RATE)
}

fn clips() -> [(&'static str, Vec<u8>); 3] {
    [
        ("Assets/Audio/MusicLoop.wav", music_loop()),
        ("Assets/Audio/Hum.wav", hum_loop()),
        ("Assets/Audio/Blip.wav", sine_beep(1320.0, 0.12, RATE)),
    ]
}

// ---- mixer ------------------------------------------------------------------------------------

/// Master (lowpass) ▸ Music, SFX (reverb). Snapshots: Normal (start), Muffled (Master lowpass
/// at 700 Hz, music down), Cave (wet reverb on SFX). Exposed: MusicVol, SFXVol.
fn mixer() -> AudioMixer {
    let mut m = AudioMixer::default();
    let music = m.add_group(MASTER, "Music");
    let sfx = m.add_group(MASTER, "SFX");
    let lowpass = m.add_effect(MASTER, EffectKind::Lowpass).expect("Master");
    let reverb = m.add_effect(sfx, EffectKind::Reverb).expect("SFX");
    let cutoff = effect_key(MASTER, lowpass, "Cutoff freq");
    let wet = effect_key(sfx, reverb, "Wet mix");
    m.snapshots[0].name = "Normal".into();
    m.set_value(0, &cutoff, 20000.0);
    m.set_value(0, &wet, 0.0);
    m.set_value(0, &volume_key(music), -4.0);
    let muffled = m.add_snapshot(0, "Muffled");
    m.set_value(muffled, &cutoff, 700.0);
    m.set_value(muffled, &volume_key(music), -8.0);
    let cave = m.add_snapshot(0, "Cave");
    m.set_value(cave, &wet, 60.0);
    m.set_value(cave, &effect_key(sfx, reverb, "Room size"), 0.92);
    m.expose(&volume_key(music));
    m.rename_exposed("MyExposedParam", "MusicVol");
    m.expose(&volume_key(sfx));
    m.rename_exposed("MyExposedParam", "SFXVol");
    m
}

// ---- scene ------------------------------------------------------------------------------------

fn script(path: &str) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            ..Default::default()
        }],
    }
}

fn demo_scene() -> Scene {
    let mut s = EditorState::new(ProjectBrowser::open(
        std::env::temp_dir().join("misty_audio_demo_build"),
    ));
    s.scene = Scene::default_3d("AudioDemo");
    let scene = &mut s.scene;
    // The Main Camera carries the Audio Listener.
    if let Some(cam) = scene.find_by_name("Main Camera") {
        scene.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 4.0, -3.0))
                .with_euler(Vec3::new(25.0, 0.0, 0.0)),
        );
    }
    let ground = scene.spawn("Ground", None);
    scene.add(ground, MeshFilter::primitive(PrimitiveMesh::Plane));
    scene.add(
        ground,
        MeshRenderer {
            material: Some("Assets/Materials/Checker.mat".into()),
            ..Default::default()
        },
    );
    let t = Transform {
        scale: Vec3::splat(3.0),
        ..Default::default()
    };
    scene.set_transform(ground, t);

    // The humming sphere: fully 3D on the SFX group.
    let orbiter = scene.spawn("Orbiter", None);
    scene.add(orbiter, MeshFilter::primitive(PrimitiveMesh::Sphere));
    scene.add(
        orbiter,
        MeshRenderer {
            material: Some("Assets/Materials/Red.mat".into()),
            ..Default::default()
        },
    );
    let mut t = Transform::from_position(Vec3::new(7.0, 1.0, 1.0));
    t.scale = Vec3::splat(0.6);
    scene.set_transform(orbiter, t);
    scene.add(
        orbiter,
        AudioSource {
            clip: Some("Assets/Audio/Hum.wav".into()),
            output: Some(output_ref(MIXER, "SFX")),
            looping: true,
            spatial_blend: 1.0,
            min_distance: 2.0,
            max_distance: 30.0,
            rolloff: AudioRolloff::Logarithmic,
            ..Default::default()
        },
    );
    scene.add(orbiter, script("Assets/Scripts/Orbiter.lua"));

    // Background music: 2D on the Music group.
    let music = scene.spawn("Music", None);
    scene.add(
        music,
        AudioSource {
            clip: Some("Assets/Audio/MusicLoop.wav".into()),
            output: Some(output_ref(MIXER, "Music")),
            looping: true,
            volume: 0.6,
            ..Default::default()
        },
    );
    let controller = scene.spawn("Audio Controller", None);
    scene.add(controller, script("Assets/Scripts/AudioDemo.lua"));

    // Key hint in the top-left corner (GameObject ▸ UI ▸ Text brings a Canvas).
    crate::commands::apply(
        &mut s,
        Action::Create {
            what: Create::Ui(UiElement::Text),
            parent: None,
        },
    );
    let text = s.selection.active().expect("created");
    s.scene.game_object_mut(text).name = "Hint".into();
    let w = &mut s.scene.world;
    *w.get::<&mut RectTransform>(text).unwrap() = RectTransform {
        anchor_min: Vec2::new(0.0, 1.0),
        anchor_max: Vec2::new(0.0, 1.0),
        pivot: Vec2::new(0.0, 1.0),
        anchored_position: Vec2::new(16.0, -12.0),
        size_delta: Vec2::new(900.0, 60.0),
    };
    {
        let mut t = w.get::<&mut UiText>(text).unwrap();
        t.text = "1 Normal   2 Muffled   3 Cave   M music   Up/Down SFX volume   Space blip".into();
        t.font_size = 20.0;
        t.color = [1.0; 4];
    }
    s.scene
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_audio_demo() {
    let root = project();
    for (path, bytes) in clips() {
        std::fs::write(root.join(path), bytes).unwrap();
    }
    mixer().save(&root.join(MIXER)).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn audio_demo_matches_its_generator() {
    let root = project();
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert_eq!(
        text,
        demo_scene().to_json(),
        "regenerate with write_audio_demo"
    );
    let (scene, report) = Scene::from_json("AudioDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(scene.find_by_name("Hint").is_some());
    let file = std::fs::read_to_string(root.join(MIXER)).unwrap();
    assert_eq!(file, mixer().to_json(), "regenerate with write_audio_demo");
    // WAVs: compare size and header only (`sin` may differ in the last bit between platforms).
    for (path, expected) in clips() {
        let found = std::fs::read(root.join(path)).unwrap_or_default();
        assert!(
            found.len() == expected.len() && found[..44] == expected[..44],
            "{path}: regenerate with write_audio_demo"
        );
    }
    for script in ["Orbiter.lua", "AudioDemo.lua"] {
        assert!(root.join("Assets/Scripts").join(script).is_file());
    }
}

/// Playing headlessly: both sources start on their groups, the sphere orbits, and the keys
/// switch snapshots and exposed parameters; the hint follows.
#[test]
fn audio_demo_plays_and_keys_drive_the_mixer() {
    use misty_engine::audio::Audio;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::{Input, LogKind};
    use misty_engine::settings::ProjectSettings;

    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let (audio, log) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root, &ProjectSettings::default(), audio);
    let mut time = Time::default();
    let mut frame = |scene: &mut Scene, key: Option<&str>| {
        let mut input = Input::default();
        if let Some(k) = key {
            input.pressed.insert(k.into());
            input.held.insert(k.into());
        }
        time.advance(1.0 / 60.0);
        rt.update(scene, &time, input);
    };
    let orbiter = scene.find_by_name("Orbiter").unwrap();
    let start = scene.world_position(orbiter);
    for _ in 0..30 {
        frame(&mut scene, None);
    }
    assert!(
        scene.world_position(orbiter).distance(start) > 0.5,
        "orbits"
    );
    frame(&mut scene, Some("2"));
    frame(&mut scene, Some("m"));
    frame(&mut scene, Some("up"));
    for _ in 0..60 {
        frame(&mut scene, None);
    }
    let hint = scene.find_by_name("Hint").unwrap();
    let text = scene.world.get::<&UiText>(hint).unwrap().text.clone();
    assert!(
        text.contains("Snapshot: Muffled")
            && text.contains("Music: off")
            && text.contains("SFX: +3 dB"),
        "{text}"
    );
    let (cutoff, music_db, sfx_db, voices) = rt.with_audio(|a| {
        let m = a.mixer_at(MIXER).unwrap();
        let lp = m.asset.groups[0].effects[0].id;
        let sfx = m.asset.group_by_name("SFX").unwrap().id;
        let music = m.asset.group_by_name("Music").unwrap().id;
        (
            m.value(&effect_key(MASTER, lp, "Cutoff freq")),
            m.value(&volume_key(music)),
            m.value(&volume_key(sfx)),
            a.voice_count(),
        )
    });
    assert_eq!(cutoff, 700.0, "Muffled reached");
    assert_eq!((music_db, sfx_db), (-80.0, 3.0));
    assert_eq!(voices, 2, "music and hum");
    let log = log.lock().unwrap();
    assert!(
        log.voices.values().all(|v| v.track.is_some()),
        "both routed to the mixer"
    );
    drop(log);
    let errors: Vec<_> = rt
        .take_logs()
        .into_iter()
        .filter(|(k, _)| *k != LogKind::Info)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
}
