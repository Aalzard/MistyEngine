//! Lua audio API: mixers, snapshots, exposed parameters, output groups, new source fields.

use std::path::PathBuf;

use crate::audio::mixer::{MASTER, effect_key, volume_key};
use crate::audio::{Audio, AudioMixer, EffectKind};
use crate::runtime::{Runtime, Time};
use crate::scene::{AudioSource, Scene, ScriptRef, Scripts};
use crate::scripting::{Input, LogKind};
use crate::settings::ProjectSettings;

const SCRIPT: &str = r#"
local T = { frames = 0 }
function T:Start()
    local src = self.gameObject:GetComponent("AudioSource")
    self.src = src
    Debug.Log("output", src.outputAudioMixerGroup)
    src.outputAudioMixerGroup = "SFX"
    local g = src.outputAudioMixerGroup
    Debug.Log("group", g.name, g.audioMixer.name)
    local mixer = AudioMixer.Find("Main")
    self.mixer = mixer
    Debug.Log("missing", AudioMixer.Find("Nope") == nil, mixer:FindSnapshot("Nope") == nil)
    Debug.Log("get", mixer:GetFloat("MusicVol"), mixer:SetFloat("MusicVol", -20), mixer:GetFloat("MusicVol"))
    Debug.Log("unknown", mixer:SetFloat("Nope", 1), mixer:GetFloat("Nope"))
    local groups = mixer:FindMatchingGroups("SFX")
    Debug.Log("groups", #groups, groups[1].name, groups[1] == g)
    mixer:FindSnapshot("Muffled"):TransitionTo(0.1)
    src:Play()
    src.time = 0.5
    src.panStereo = -0.5
    src.spread = 90
    src.dopplerLevel = 2
    src.priority = 10
    src.rolloffMode = "Linear"
    src.minDistance = 2
    src.maxDistance = 20
    src.bypassEffects = true
    Debug.Log("fields", src.time, src.panStereo, src.spread, src.priority, src.rolloffMode, src.playOnAwake)
    AudioSource.PlayClipAtPoint("Assets/Audio/Boom.wav", Vector3.new(1, 2, 3), 0.5)
    src:PlayOneShot("Assets/Audio/Shot.wav", 0.5)
end
function T:Update()
    self.frames = self.frames + 1
    if self.frames == 20 then
        Debug.Log("cleared", self.mixer:ClearFloat("MusicVol"), self.mixer:GetFloat("MusicVol"))
    end
end
return T
"#;

fn project() -> (PathBuf, u32, u32) {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_audio_script_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts/T.lua"), SCRIPT).unwrap();
    let mut m = AudioMixer::default();
    let music = m.add_group(MASTER, "Music");
    let sfx = m.add_group(MASTER, "SFX");
    let lp = m.add_effect(sfx, EffectKind::Lowpass).unwrap();
    m.set_value(0, &volume_key(music), -6.0);
    let muffled = m.add_snapshot(0, "Muffled");
    m.set_value(muffled, &effect_key(sfx, lp, "Cutoff freq"), 500.0);
    m.expose(&volume_key(music));
    m.rename_exposed("MyExposedParam", "MusicVol");
    m.save(&root.join("Assets/Audio/Mixers/Main.mixer"))
        .unwrap();
    (root, sfx, lp)
}

#[test]
fn mixer_and_source_api_from_lua() {
    let (root, sfx, lp) = project();
    let mut scene = Scene::default_3d("t");
    let e = scene.spawn("Speaker", None);
    scene.add(
        e,
        AudioSource {
            clip: Some("Assets/Audio/Beep.wav".into()),
            play_on_awake: false,
            ..Default::default()
        },
    );
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/T.lua".into(),
                ..Default::default()
            }],
        },
    );
    let (audio, log) = Audio::null(root.clone());
    let mut rt =
        Runtime::start_with_audio(&mut scene, root.clone(), &ProjectSettings::default(), audio);
    let mut time = Time::default();
    let mut logs = Vec::new();
    for _ in 0..30 {
        time.advance(1.0 / 60.0);
        rt.update(&mut scene, &time, Input::default());
        logs.extend(rt.take_logs());
    }
    let text: Vec<&str> = logs
        .iter()
        .filter(|(k, _)| *k == LogKind::Info)
        .map(|(_, t)| t.as_str())
        .collect();
    assert_eq!(
        text,
        [
            "output nil",
            "group SFX Main",
            "missing true true",
            "get -6 true -20",
            "unknown false nil",
            "groups 1 SFX true",
            "fields 0.5 -0.5 90 10 Linear false",
            "cleared true -6",
        ],
        "{logs:?}"
    );
    let src = scene.world.get::<&AudioSource>(e).unwrap().clone();
    assert_eq!(
        src.output.as_deref(),
        Some("Assets/Audio/Mixers/Main.mixer#SFX")
    );
    assert!(src.bypass_effects && src.doppler_level == 2.0 && src.min_distance == 2.0);
    // The snapshot transition finished, and the one-shot plays on the SFX track.
    let (cutoff, sfx_track) = rt.with_audio(|a| {
        let m = a.mixer_at("Assets/Audio/Mixers/Main.mixer").unwrap();
        (m.value(&effect_key(sfx, lp, "Cutoff freq")), m.tracks[&sfx])
    });
    assert_eq!(cutoff, 500.0);
    let log = log.lock().unwrap();
    let shot = log
        .voices
        .values()
        .find(|v| v.path.ends_with("Shot.wav"))
        .unwrap();
    assert_eq!(shot.track, Some(sfx_track));
    assert!(
        log.played.iter().any(|p| p.ends_with("Boom.wav")),
        "PlayClipAtPoint"
    );
}

const BLEND_SCRIPT: &str = r#"
local T = {}
function T:Start()
    local mixer = AudioMixer.Find("World")
    local outdoor = mixer:FindSnapshot("Outdoor")
    local indoor = mixer:FindSnapshot("Indoor")
    mixer:TransitionToSnapshots({ outdoor, indoor }, { 0.25, 0.75 }, 0)
    Debug.Log("blend", mixer:GetFloat("ReverbSend"))
    Debug.Log("eq", mixer:SetFloat("EqGain", 2.5), mixer:GetFloat("EqGain"), mixer:SetFloat("EqWet", 40), mixer:GetFloat("EqWet"))
    Debug.Log("output", mixer.outputAudioMixerGroup)
    mixer.outputAudioMixerGroup = AudioMixer.Find("Main"):FindMatchingGroups("Music")[1]
    Debug.Log("nested", mixer.outputAudioMixerGroup.name)
    local src = self.gameObject:GetComponent("AudioSource")
    src.spatialize = true
    src:SetCustomCurve("CustomRolloff", { { 1, 0 }, { 0, 1 }, { 0.5, 0.25 } })
    local keys = src:GetCustomCurve("CustomRolloff")
    Debug.Log("curve", src.rolloffMode, #keys, keys[2][1], keys[2][2], src.spatialize)
    mixer:TransitionToSnapshots({ outdoor, indoor }, { 1, 0 }, 1)
end
return T
"#;

#[test]
fn snapshot_blends_effect_parameters_and_nested_output_from_lua() {
    let (root, ..) = project();
    std::fs::write(root.join("Assets/Scripts/Blend.lua"), BLEND_SCRIPT).unwrap();
    // World: Master ▸ SFX (ParamEQ, Send to Reverb), Reverb (Receive, Reverb).
    let mut m = AudioMixer::default();
    let sfx = m.add_group(MASTER, "SFX");
    let reverb = m.add_group(MASTER, "Reverb");
    let eq = m.add_effect(sfx, EffectKind::ParamEQ).unwrap();
    let send = m.add_effect(sfx, EffectKind::Send).unwrap();
    let recv = m.add_effect(reverb, EffectKind::Receive).unwrap();
    m.add_effect(reverb, EffectKind::Reverb);
    assert!(m.set_send_target(sfx, send, Some(recv)));
    let level = effect_key(sfx, send, "Send level");
    m.snapshots[0].name = "Outdoor".into();
    m.set_value(0, &level, -40.0);
    let indoor = m.add_snapshot(0, "Indoor");
    m.set_value(indoor, &level, 0.0);
    for (key, name) in [
        (level.clone(), "ReverbSend"),
        (effect_key(sfx, eq, "Frequency gain"), "EqGain"),
        (effect_key(sfx, eq, crate::audio::effects::WET), "EqWet"),
    ] {
        m.expose(&key);
        m.rename_exposed("MyExposedParam", name);
    }
    m.save(&root.join("Assets/Audio/Mixers/World.mixer"))
        .unwrap();
    let mut scene = Scene::default_3d("t");
    let e = scene.spawn("Speaker", None);
    scene.add(
        e,
        AudioSource {
            clip: Some("Assets/Audio/Beep.wav".into()),
            output: Some("Assets/Audio/Mixers/World.mixer#SFX".into()),
            ..Default::default()
        },
    );
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Blend.lua".into(),
                ..Default::default()
            }],
        },
    );
    let (audio, log) = Audio::null(root.clone());
    let mut rt =
        Runtime::start_with_audio(&mut scene, root.clone(), &ProjectSettings::default(), audio);
    let mut time = Time::default();
    let mut logs = Vec::new();
    for _ in 0..30 {
        time.advance(1.0 / 60.0);
        rt.update(&mut scene, &time, Input::default());
        logs.extend(rt.take_logs());
    }
    let text: Vec<&str> = logs
        .iter()
        .filter(|(k, _)| *k == LogKind::Info)
        .map(|(_, t)| t.as_str())
        .collect();
    assert_eq!(
        text,
        [
            "blend -10",
            "eq true 2.5 true 40",
            "output nil",
            "nested Music",
            "curve Custom 3 0.5 0.25 true",
        ],
        "{logs:?}"
    );
    let src = scene.world.get::<&AudioSource>(e).unwrap().clone();
    assert_eq!(src.custom_rolloff.keys[0], [0.0, 1.0], "sorted");
    // Halfway back to Outdoor after 0.5 s (from -10 dB towards -40 dB); the EQ override and
    // the wet level reach the SFX track; World's Master plays into Main's Music.
    let (send_db, specs, master, music) = rt.with_audio(|a| {
        let i = a.find_mixer("World").unwrap();
        let main = a.find_mixer("Main").unwrap();
        let w = a.mixer(i).unwrap();
        let music = a
            .mixer(main)
            .unwrap()
            .asset
            .group_by_name("Music")
            .unwrap()
            .id;
        (
            w.value(&level),
            w.effects(sfx),
            w.master_track().unwrap(),
            a.mixer(main).unwrap().tracks[&music],
        )
    });
    assert!(send_db < -20.0 && send_db > -40.0, "{send_db}");
    assert_eq!(specs[0].values[2], 2.5);
    assert!((specs[0].wet - 0.4).abs() < 1e-6);
    assert_eq!(log.lock().unwrap().tracks[&master].parent, Some(music));
}
