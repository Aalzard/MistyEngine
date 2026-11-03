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
