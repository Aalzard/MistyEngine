//! The AudioDemo2 sample: the mixer's routing and spatial features in one scene.
//!
//! - **Ducking by a send**: the radio's Dialogue group sends to a Duck Volume on Music, so the
//!   music dips while someone talks.
//! - **Reverb send zone**: Dialogue and SFX send to a Reverb bus (Receive + Reverb); walking into
//!   the cave (an Audio Reverb Zone) raises those sends.
//! - **Snapshot blending**: `AudioDemo2.lua` blends the Outdoor and Indoor snapshots with
//!   `TransitionToSnapshots` by how far inside the cave the listener is.
//! - **HRTF**: a buzzing sphere with Spatialize on orbits the listener's head.
//! - **Streaming**: the music is a 12 s track imported with Load Type Streaming.
//! - **Custom rolloff**: the radio fades over distance along its own curve.
//!
//! Everything is made here: the clips are synthesised (tones and noise; CC0), the mixer, the
//! scene, the scripts and the asset `.meta` files. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_audio_demo2`; the other tests check the
//! committed files still match and play the scene headlessly through the offline mixer.

use std::f32::consts::TAU;
use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::audio::clip::{AudioImporter, LoadType};
use misty_engine::audio::mixer::{AudioMixer, MASTER, effect_key, volume_key};
use misty_engine::audio::spatial::RolloffCurve;
use misty_engine::audio::wav::pcm16_mono;
use misty_engine::audio::{EffectKind, output_ref};
use misty_engine::scene::{
    AudioReverbZone, AudioRolloff, AudioSource, MeshFilter, MeshRenderer, PrimitiveMesh,
    RectTransform, ReverbPreset, Scene, ScriptRef, Scripts, Transform, UiText,
};

use crate::actions::{Action, Create};
use crate::project::ProjectBrowser;
use crate::state::EditorState;
use crate::ui_create::UiElement;

const SCENE: &str = "Assets/Scenes/AudioDemo2.scene";
const MIXER: &str = "Assets/Audio/AudioDemo2.mixer";
const SCORE: &str = "Assets/Audio/Score.wav";
const RATE: u32 = 22050;
/// The cave's reverb zone: centre and radii (the script blends snapshots over the same range).
const CAVE: Vec3 = Vec3::new(0.0, 1.0, 8.0);
const ZONE: (f32, f32) = (3.5, 8.0);

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

// ---- clips ------------------------------------------------------------------------------------

fn synth(seconds: f32, mut f: impl FnMut(f32) -> f32) -> Vec<u8> {
    let n = (seconds * RATE as f32) as usize;
    let samples: Vec<i16> = (0..n)
        .map(|i| (f(i as f32 / RATE as f32).clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect();
    pcm16_mono(&samples, RATE)
}

/// Deterministic noise in -1..1.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// 12 s score: four chords (3 s each) as slow-swelling pads, a plucked arpeggio and a soft
/// pulse. Each chord swells from and back to silence, so the loop is seamless.
fn score() -> Vec<u8> {
    const CHORDS: [[f32; 3]; 4] = [
        [220.0, 261.63, 329.63],
        [174.61, 220.0, 261.63],
        [196.0, 246.94, 293.66],
        [164.81, 207.65, 246.94],
    ];
    synth(12.0, |t| {
        let i = ((t / 3.0) as usize).min(3);
        let local = t - i as f32 * 3.0;
        let swell = (std::f32::consts::PI * local / 3.0).sin();
        let chord = CHORDS[i];
        let pad: f32 = chord
            .iter()
            .map(|f| (TAU * f * t).sin() + 0.3 * (TAU * 2.0 * f * t).sin())
            .sum::<f32>()
            * 0.08
            * swell;
        let step = 0.25;
        let n = (local / step) as usize;
        let note = chord[n % 3] * if n % 6 >= 3 { 2.0 } else { 1.0 };
        let lt = local - n as f32 * step;
        let pluck = (TAU * note * lt).sin() * (lt / 0.004).min(1.0) * (-9.0 * lt).exp() * 0.25;
        let beat = local % 0.5;
        let pulse = (TAU * 55.0 * beat).sin() * (-14.0 * beat).exp() * 0.2;
        (pad + pluck * swell.max(0.3) + pulse) * 0.9
    })
}

/// 6 s of "speech": two phrases of syllables (a buzzing voice through two vowel formants that
/// change every syllable), silence between them; loops.
fn dialogue() -> Vec<u8> {
    const VOWELS: [(f32, f32); 5] = [
        (730.0, 1090.0),
        (270.0, 2290.0),
        (530.0, 1840.0),
        (570.0, 840.0),
        (440.0, 1020.0),
    ];
    let mut noise = Noise(9);
    synth(6.0, |t| {
        let phrase = (0.3..2.4).contains(&t) || (3.4..5.5).contains(&t);
        if !phrase {
            return 0.0;
        }
        let syl = (t * 4.5) as usize;
        let st = t * 4.5 - syl as f32;
        let env = (std::f32::consts::PI * st).sin().powf(0.6);
        let (f1, f2) = VOWELS[syl % VOWELS.len()];
        let pitch = 125.0 + 15.0 * (TAU * 0.7 * t).sin();
        // Harmonics of the voice weighted by closeness to the formants.
        let mut v = 0.0;
        for h in 1..=30 {
            let f = pitch * h as f32;
            let w = (-((f - f1) / 120.0).powi(2)).exp() + 0.6 * (-((f - f2) / 180.0).powi(2)).exp();
            v += w * (TAU * f * t).sin();
        }
        (v * 0.12 + noise.next() * 0.02) * env
    })
}

/// 1 s buzzing loop for the orbiting source: a 180 Hz saw with noise bursts at 8 Hz (broadband,
/// so the head model has plenty to work with).
fn buzz() -> Vec<u8> {
    let mut noise = Noise(21);
    synth(1.0, |t| {
        let saw = 2.0 * (t * 180.0).fract() - 1.0;
        let burst = (0.5 - 0.5 * (TAU * 8.0 * t).cos()).powi(2);
        (saw * 0.25 + noise.next() * 0.35 * burst) * 0.8
    })
}

/// A clap: a quarter second of noise with a sharp attack.
fn clap() -> Vec<u8> {
    let mut noise = Noise(5);
    synth(0.25, |t| {
        noise.next() * (-30.0 * t).exp() * (t / 0.002).min(1.0) * 0.9
    })
}

fn clips() -> [(&'static str, Vec<u8>); 4] {
    [
        (SCORE, score()),
        ("Assets/Audio/Dialogue.wav", dialogue()),
        ("Assets/Audio/Buzz.wav", buzz()),
        ("Assets/Audio/Clap.wav", clap()),
    ]
}

// ---- mixer ------------------------------------------------------------------------------------

/// Master ▸ Music (Lowpass, Duck Volume), Dialogue (Send → Duck, Send → Reverb), SFX (Send →
/// Reverb), Reverb (Receive, Reverb). Snapshots Outdoor (start) and Indoor (music muffled,
/// reverb sends up). Exposed: MusicVol, ReverbSend (the dialogue's reverb send).
fn mixer() -> AudioMixer {
    let mut m = AudioMixer::default();
    let music = m.add_group(MASTER, "Music");
    let dialogue = m.add_group(MASTER, "Dialogue");
    let sfx = m.add_group(MASTER, "SFX");
    let reverb = m.add_group(MASTER, "Reverb");
    let lowpass = m.add_effect(music, EffectKind::Lowpass).expect("Music");
    let duck = m.add_effect(music, EffectKind::DuckVolume).expect("Music");
    let to_duck = m.add_effect(dialogue, EffectKind::Send).expect("Dialogue");
    let dialogue_verb = m.add_effect(dialogue, EffectKind::Send).expect("Dialogue");
    let sfx_verb = m.add_effect(sfx, EffectKind::Send).expect("SFX");
    let receive = m.add_effect(reverb, EffectKind::Receive).expect("Reverb");
    let room = m.add_effect(reverb, EffectKind::Reverb).expect("Reverb");
    m.set_send_target(dialogue, to_duck, Some(duck));
    m.set_send_target(dialogue, dialogue_verb, Some(receive));
    m.set_send_target(sfx, sfx_verb, Some(receive));
    let level = |g, e| effect_key(g, e, "Send level");
    let duck_key = |p| effect_key(music, duck, p);
    m.snapshots[0].name = "Outdoor".into();
    for (key, v) in [
        (volume_key(music), -4.0),
        (effect_key(music, lowpass, "Cutoff freq"), 22000.0),
        (duck_key("Threshold"), -32.0),
        (duck_key("Ratio"), 6.0),
        (duck_key("Attack"), 40.0),
        (duck_key("Release"), 600.0),
        (duck_key("Knee"), 6.0),
        (level(dialogue, to_duck), 0.0),
        (level(dialogue, dialogue_verb), -30.0),
        (level(sfx, sfx_verb), -24.0),
        (effect_key(reverb, room, "Room size"), 0.9),
        (effect_key(reverb, room, "Wet mix"), 100.0),
        (effect_key(reverb, room, "Damping"), 0.35),
    ] {
        m.set_value(0, &key, v);
    }
    let indoor = m.add_snapshot(0, "Indoor");
    m.set_value(indoor, &effect_key(music, lowpass, "Cutoff freq"), 900.0);
    m.set_value(indoor, &volume_key(music), -10.0);
    m.set_value(indoor, &level(dialogue, dialogue_verb), -4.0);
    m.set_value(indoor, &level(sfx, sfx_verb), 0.0);
    m.expose(&volume_key(music));
    m.rename_exposed("MyExposedParam", "MusicVol");
    m.expose(&level(dialogue, dialogue_verb));
    m.rename_exposed("MyExposedParam", "ReverbSend");
    m
}

// ---- scripts ----------------------------------------------------------------------------------

const CONTROLLER_LUA: &str = r#"-- AudioDemo2: blends the Outdoor and Indoor mixer snapshots by how far into the cave the
-- listener is (TransitionToSnapshots with weights), and drives the demo from the keyboard.
--   D       radio dialogue on / off (it ducks the music through a send)
--   Space   a clap from the orbiting buzzer (its SFX send feeds the reverb bus)
--   M       music on / off (exposed "MusicVol")
--   W / S   walk in / out by hand (otherwise the camera walks on its own)
local AudioDemo2 = {
    inner = 3.5,                     -- fully Indoor inside this distance from the cave centre
    outer = 8.0,                     -- fully Outdoor beyond this one
    cave = Vector3.new(0, 1, 8),
}

function AudioDemo2:Start()
    self.mixer = AudioMixer.Find("AudioDemo2")
    self.outdoor = self.mixer:FindSnapshot("Outdoor")
    self.indoor = self.mixer:FindSnapshot("Indoor")
    self.ear = GameObject.Find("Main Camera").transform
    self.radio = GameObject.Find("Radio"):GetComponent("AudioSource")
    self.buzzer = GameObject.Find("Buzzer"):GetComponent("AudioSource")
    self.hint = GameObject.Find("Hint"):GetComponent("Text")
    self.weight = -1
    self.talking = true
    self.musicOff = false
end

function AudioDemo2:Update()
    local d = Vector3.Distance(self.ear.position, self.cave)
    local w = Mathf.Clamp01((self.outer - d) / (self.outer - self.inner))
    if math.abs(w - self.weight) > 0.005 then
        self.mixer:TransitionToSnapshots({ self.outdoor, self.indoor }, { 1 - w, w }, 0.1)
        self.weight = w
    end
    if Input.GetKeyDown("d") then
        self.talking = not self.talking
        if self.talking then self.radio:Play() else self.radio:Stop() end
    end
    if Input.GetKeyDown("space") then
        self.buzzer:PlayOneShot("Assets/Audio/Clap.wav", 1)
    end
    if Input.GetKeyDown("m") then
        self.musicOff = not self.musicOff
        if self.musicOff then self.mixer:SetFloat("MusicVol", -80) else self.mixer:ClearFloat("MusicVol") end
    end
    self.hint.text = "D dialogue   Space clap   M music   W/S walk\n"
        .. string.format("Outdoor %.2f   Indoor %.2f   Dialogue: %s   Music: %s",
            1 - w, w, self.talking and "on" or "off", self.musicOff and "off" or "on")
end

return AudioDemo2
"#;

const WALKER_LUA: &str = r#"-- Walker: carries the listener (this camera) from the meadow into the cave and back over
-- `period` seconds. Holding W or S walks by hand instead.
local Walker = {
    near = -12,
    far = 9,
    period = 24,
    speed = 4,
    manual = false,
    z = -12,
}

function Walker:Update()
    if Input.GetKey("w") or Input.GetKey("s") then self.manual = true end
    if self.manual then
        if Input.GetKey("w") then self.z = math.min(self.z + self.speed * Time.deltaTime, self.far) end
        if Input.GetKey("s") then self.z = math.max(self.z - self.speed * Time.deltaTime, self.near) end
    else
        local k = 0.5 - 0.5 * Mathf.Cos(Time.time / self.period * 2 * math.pi)
        self.z = self.near + (self.far - self.near) * k
    end
    local p = self.transform.position
    self.transform.position = Vector3.new(p.x, p.y, self.z)
end

return Walker
"#;

const HEAD_ORBIT_LUA: &str = r#"-- HeadOrbit: circles the listener's head. With Spatialize on its Audio Source the sound is
-- rendered binaurally (time and level differences, head shadow): on headphones it goes round
-- your head, and behind you sounds duller than in front.
local HeadOrbit = {
    radius = 2.5,
    speed = 1.2,                     -- radians per second
}

function HeadOrbit:Start()
    self.ear = GameObject.Find("Main Camera").transform
end

function HeadOrbit:Update()
    local a = Time.time * self.speed
    local c = self.ear.position
    self.transform.position = Vector3.new(c.x + Mathf.Cos(a) * self.radius, c.y,
        c.z + Mathf.Sin(a) * self.radius)
end

return HeadOrbit
"#;

fn scripts() -> [(&'static str, &'static str); 3] {
    [
        ("Assets/Scripts/AudioDemo2.lua", CONTROLLER_LUA),
        ("Assets/Scripts/Walker.lua", WALKER_LUA),
        ("Assets/Scripts/HeadOrbit.lua", HEAD_ORBIT_LUA),
    ]
}

/// `.meta` files of the new assets: fixed GUIDs, and the score's Streaming import setting.
fn metas() -> Vec<(String, String)> {
    let guid = |n: u32| format!("a0d10de2{n:024x}");
    let mut out = Vec::new();
    let assets = [
        SCENE,
        MIXER,
        "Assets/Audio/Dialogue.wav",
        "Assets/Audio/Buzz.wav",
        "Assets/Audio/Clap.wav",
        "Assets/Scripts/AudioDemo2.lua",
        "Assets/Scripts/Walker.lua",
        "Assets/Scripts/HeadOrbit.lua",
    ];
    for (i, a) in assets.iter().enumerate() {
        out.push((
            format!("{a}.meta"),
            format!("{{\n  \"guid\": \"{}\"\n}}\n", guid(i as u32 + 1)),
        ));
    }
    let import = AudioImporter {
        load_type: LoadType::Streaming,
        ..Default::default()
    };
    let mut v = serde_json::to_value(&import).expect("settings");
    v["guid"] = serde_json::Value::String(guid(100));
    out.push((
        format!("{SCORE}.meta"),
        serde_json::to_string_pretty(&v).expect("json") + "\n",
    ));
    out
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

fn block(scene: &mut Scene, name: &str, at: Vec3, size: Vec3, material: &str) {
    let e = scene.spawn(name, None);
    scene.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
    scene.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
    let t = Transform {
        scale: size,
        ..Transform::from_position(at)
    };
    scene.set_transform(e, t);
}

fn demo_scene() -> Scene {
    let mut s = EditorState::new(ProjectBrowser::open(
        std::env::temp_dir().join("misty_audio_demo2_build"),
    ));
    s.scene = Scene::default_3d("AudioDemo2");
    let scene = &mut s.scene;
    // The Main Camera carries the Audio Listener and walks into the cave and back.
    if let Some(cam) = scene.find_by_name("Main Camera") {
        scene.set_transform(cam, Transform::from_position(Vec3::new(0.0, 1.6, -12.0)));
        scene.add(cam, script("Assets/Scripts/Walker.lua"));
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
        scale: Vec3::new(2.0, 1.0, 4.0),
        ..Transform::from_position(Vec3::new(0.0, 0.0, 0.0))
    };
    scene.set_transform(ground, t);

    // The cave: walls, roof and a reverb zone (open towards the meadow).
    let rock = "Assets/Materials/AO Concrete.mat";
    block(
        scene,
        "Cave Left Wall",
        Vec3::new(-4.5, 2.0, 8.0),
        Vec3::new(1.0, 4.0, 12.0),
        rock,
    );
    block(
        scene,
        "Cave Right Wall",
        Vec3::new(4.5, 2.0, 8.0),
        Vec3::new(1.0, 4.0, 12.0),
        rock,
    );
    block(
        scene,
        "Cave Back Wall",
        Vec3::new(0.0, 2.0, 14.5),
        Vec3::new(10.0, 4.0, 1.0),
        rock,
    );
    block(
        scene,
        "Cave Roof",
        Vec3::new(0.0, 4.5, 8.0),
        Vec3::new(10.0, 1.0, 13.0),
        rock,
    );
    let zone = scene.spawn("Cave Reverb Zone", None);
    scene.set_transform(zone, Transform::from_position(CAVE));
    scene.add(
        zone,
        AudioReverbZone {
            min_distance: ZONE.0,
            max_distance: ZONE.1,
            preset: ReverbPreset::Cave,
            ..Default::default()
        },
    );

    // Streamed music (2D) on Music.
    let music = scene.spawn("Music", None);
    scene.add(
        music,
        AudioSource {
            clip: Some(SCORE.into()),
            output: Some(output_ref(MIXER, "Music")),
            looping: true,
            volume: 0.7,
            ..Default::default()
        },
    );
    // The radio: dialogue, mostly 3D, by the path.
    let radio = scene.spawn("Radio", None);
    scene.add(radio, MeshFilter::primitive(PrimitiveMesh::Cube));
    scene.add(
        radio,
        MeshRenderer {
            material: Some("Assets/Materials/Red.mat".into()),
            ..Default::default()
        },
    );
    let t = Transform {
        scale: Vec3::new(0.5, 0.35, 0.25),
        ..Transform::from_position(Vec3::new(2.0, 0.6, -6.0))
    };
    scene.set_transform(radio, t);
    scene.add(
        radio,
        AudioSource {
            clip: Some("Assets/Audio/Dialogue.wav".into()),
            output: Some(output_ref(MIXER, "Dialogue")),
            looping: true,
            spatial_blend: 0.6,
            min_distance: 4.0,
            max_distance: 40.0,
            // A custom curve: holds up near the radio, then falls away quickly.
            rolloff: AudioRolloff::Custom,
            custom_rolloff: RolloffCurve {
                keys: vec![[0.0, 1.0], [0.12, 0.85], [0.35, 0.3], [1.0, 0.0]],
            },
            ..Default::default()
        },
    );
    // The buzzer: spatialized, orbiting the listener's head.
    let buzzer = scene.spawn("Buzzer", None);
    scene.add(buzzer, MeshFilter::primitive(PrimitiveMesh::Sphere));
    scene.add(
        buzzer,
        MeshRenderer {
            material: Some("Assets/Materials/Red.mat".into()),
            ..Default::default()
        },
    );
    let mut t = Transform::from_position(Vec3::new(2.5, 1.6, -12.0));
    t.scale = Vec3::splat(0.3);
    scene.set_transform(buzzer, t);
    scene.add(
        buzzer,
        AudioSource {
            clip: Some("Assets/Audio/Buzz.wav".into()),
            output: Some(output_ref(MIXER, "SFX")),
            looping: true,
            volume: 0.5,
            spatial_blend: 1.0,
            spatialize: true,
            min_distance: 3.0,
            max_distance: 30.0,
            ..Default::default()
        },
    );
    scene.add(buzzer, script("Assets/Scripts/HeadOrbit.lua"));
    let controller = scene.spawn("Audio Controller", None);
    scene.add(controller, script("Assets/Scripts/AudioDemo2.lua"));

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
        t.text = "D dialogue   Space clap   M music   W/S walk".into();
        t.font_size = 20.0;
        t.color = [1.0; 4];
    }
    s.scene
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_audio_demo2() {
    let root = project();
    for (path, bytes) in clips() {
        std::fs::write(root.join(path), bytes).unwrap();
    }
    for (path, text) in scripts() {
        std::fs::write(root.join(path), text).unwrap();
    }
    mixer().save(&root.join(MIXER)).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
    for (path, text) in metas() {
        std::fs::write(root.join(path), text).unwrap();
    }
}

#[test]
fn audio_demo2_matches_its_generator() {
    let root = project();
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_audio_demo2",
    );
    let (scene, report) = Scene::from_json("AudioDemo2", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(scene.find_by_name("Cave Reverb Zone").is_some());
    let file = std::fs::read_to_string(root.join(MIXER)).unwrap();
    crate::test_util::assert_json_close(
        &file,
        &mixer().to_json(),
        "regenerate with write_audio_demo2",
    );
    for (path, expected) in clips() {
        let found = std::fs::read(root.join(path)).unwrap_or_default();
        assert!(
            found.len() == expected.len() && found[..44] == expected[..44],
            "{path}: regenerate with write_audio_demo2"
        );
    }
    for (path, expected) in scripts() {
        let found = std::fs::read_to_string(root.join(path)).unwrap_or_default();
        assert_eq!(
            found.replace("\r\n", "\n"),
            expected,
            "{path}: regenerate with write_audio_demo2"
        );
    }
    for (path, expected) in metas() {
        let found = std::fs::read_to_string(root.join(&path)).unwrap_or_default();
        assert_eq!(found.replace("\r\n", "\n"), expected, "{path}");
    }
    assert_eq!(
        AudioImporter::load(&root.join(SCORE)).load_type,
        LoadType::Streaming,
        "the score streams"
    );
}

/// Played headlessly through the offline mixer: the dialogue ducks the music, the walk into the
/// cave blends the snapshots towards Indoor, the buzzer orbits the listener binaurally and the
/// streamed score keeps playing.
#[test]
fn audio_demo2_plays_ducks_and_blends() {
    use misty_engine::audio::Audio;
    use misty_engine::audio::dsp::analysis::{rms, side};
    use misty_engine::audio::effects::WET;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::{Input, LogKind};
    use misty_engine::settings::ProjectSettings;

    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let audio = Audio::offline(root.clone(), 48000.0);
    let mut rt = Runtime::start_with_audio(&mut scene, root, &ProjectSettings::default(), audio);
    let mut time = Time::default();
    let mut rendered = Vec::new();
    let mut deepest = 0.0f32;
    let mut weights = Vec::new();
    let buzzer = scene.find_by_name("Buzzer").unwrap();
    let cam = scene.find_by_name("Main Camera").unwrap();
    let mut orbit = Vec::new();
    // Twelve seconds: the walker reaches the cave's centre at the half period (12 s).
    for frame in 0..720 {
        time.advance(1.0 / 60.0);
        rt.update(&mut scene, &time, Input::default());
        let (out, red, w) = rt.with_audio(|a| {
            let out = a.render(800).unwrap_or_default();
            let red = a.mixer_reductions(MIXER);
            let m = a.mixer_at(MIXER).unwrap();
            let music = m.asset.group_by_name("Music").unwrap().id;
            let w = m
                .weights
                .iter()
                .find(|(i, _)| m.asset.snapshots[*i].name == "Indoor")
                .map_or(0.0, |(_, w)| *w);
            (out, red.get(&music).copied().unwrap_or(0.0), w)
        });
        deepest = deepest.min(red);
        weights.push(w);
        if frame % 60 == 0 {
            rendered.extend(out);
            let rel = scene.world_position(buzzer) - scene.world_position(cam);
            orbit.push(rel);
        }
    }
    assert!(
        deepest < -6.0,
        "the dialogue ducked the music by {deepest} dB"
    );
    assert!(weights[30] < 0.05, "starts Outdoor: {}", weights[30]);
    assert!(
        *weights.last().unwrap() > 0.95,
        "Indoor in the cave: {}",
        weights.last().unwrap()
    );
    assert!(
        weights.windows(2).all(|w| w[1] >= w[0] - 1e-4),
        "blends in steadily"
    );
    // The buzzer circles 2.5 m from the listener, round to both sides.
    assert!(orbit.iter().all(|r| (r.length() - 2.5).abs() < 0.3));
    assert!(orbit.iter().any(|r| r.x > 1.5) && orbit.iter().any(|r| r.x < -1.5));
    assert!(rms(&side(&rendered, false)) > 0.01, "audible");
    let (voices, specs) = rt.with_audio(|a| {
        let m = a.mixer_at(MIXER).unwrap();
        let reverb = m.asset.group_by_name("Reverb").unwrap().id;
        (a.voice_count(), m.effects(reverb))
    });
    assert_eq!(voices, 3, "score (streamed), dialogue and buzzer");
    assert_eq!(specs[1].wet, 1.0, "{WET}: the bus reverb is fully wet");
    let errors: Vec<_> = rt
        .take_logs()
        .into_iter()
        .filter(|(k, _)| *k != LogKind::Info)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
}
