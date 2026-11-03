//! Mixer routing, per-source filters, reverb zones and the rest against the silent backend.

use std::sync::{Arc, Mutex};

use glam::Vec3;

use super::mixer::{MASTER, effect_key, volume_key};
use super::*;
use crate::scene::{AudioEchoFilter, AudioLowPassFilter, AudioReverbZone, Transform};

/// A project folder with `Assets/Audio/Main.mixer`: Master ▸ Music, SFX (lowpass on SFX) and
/// a "Muffled" snapshot with the lowpass at 500 Hz. Returns the root and group ids.
fn project(tag: &str) -> (PathBuf, u32, u32, u32) {
    let root = std::env::temp_dir().join(format!("misty_mixer_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut m = AudioMixer::default();
    let music = m.add_group(MASTER, "Music");
    let sfx = m.add_group(MASTER, "SFX");
    let lp = m.add_effect(sfx, EffectKind::Lowpass).unwrap();
    m.set_value(0, &volume_key(music), -6.0206);
    let muffled = m.add_snapshot(0, "Muffled");
    m.set_value(muffled, &effect_key(sfx, lp, "Cutoff freq"), 500.0);
    m.expose(&volume_key(music));
    m.rename_exposed("MyExposedParam", "MusicVol");
    m.save(&root.join("Assets/Audio/Main.mixer")).unwrap();
    (root, music, sfx, lp)
}

fn source(output: &str) -> AudioSource {
    AudioSource {
        clip: Some("Assets/Audio/Beep.wav".into()),
        output: Some(format!("Assets/Audio/Main.mixer#{output}")),
        ..Default::default()
    }
}

fn voice_track(log: &Arc<Mutex<NullLog>>) -> Option<TrackId> {
    log.lock().unwrap().voices.values().next().unwrap().track
}

#[test]
fn outputs_play_on_their_group_track_under_master() {
    let (root, music, sfx, _) = project("route");
    let mut scene = Scene::default_3d("t");
    let a = scene.spawn("Music", None);
    scene.add(a, source("Music"));
    let (mut audio, log) = Audio::null(root.clone());
    assert!(audio.play(&scene, a));
    let m = audio.mixer(0).expect("loaded on first use");
    let (master_t, music_t, sfx_t) = (m.tracks[&MASTER], m.tracks[&music], m.tracks[&sfx]);
    assert_eq!(voice_track(&log), Some(music_t));
    {
        let log = log.lock().unwrap();
        assert_eq!(log.tracks[&music_t].parent, Some(master_t));
        assert_eq!(log.tracks[&master_t].parent, None);
        assert_eq!(log.tracks[&sfx_t].effects[0].kind, EffectKind::Lowpass);
        assert!(
            (log.tracks[&music_t].volume - 0.5).abs() < 1e-4,
            "-6 dB fader"
        );
    }
    // Meters read through the tree; muting the group silences it and Master.
    audio.update(&scene);
    let peaks = audio.mixer_peaks("Assets/Audio/Main.mixer");
    assert!((peaks[&music] - 0.5).abs() < 1e-4 && (peaks[&MASTER] - 0.5).abs() < 1e-4);
    assert_eq!(peaks[&sfx], 0.0);
    let mut edited = audio.mixer(0).unwrap().asset.clone();
    edited.group_mut(music).unwrap().mute = true;
    audio.replace_mixer("Assets/Audio/Main.mixer", edited);
    audio.update(&scene);
    assert_eq!(audio.mixer_peaks("Assets/Audio/Main.mixer")[&MASTER], 0.0);
    assert!(audio.is_playing(a), "values-only edits keep the voice");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn snapshots_and_exposed_parameters_reach_the_tracks() {
    let (root, music, sfx, _) = project("snap");
    let mut scene = Scene::default_3d("t");
    let a = scene.spawn("Shot", None);
    scene.add(a, source("SFX"));
    let (mut audio, log) = Audio::null(root.clone());
    audio.play(&scene, a);
    let i = audio.find_mixer("Main").expect("found by name");
    let sfx_t = audio.mixer(i).unwrap().tracks[&sfx];
    let music_t = audio.mixer(i).unwrap().tracks[&music];
    let muffled = audio
        .mixer(i)
        .unwrap()
        .asset
        .snapshot_index("Muffled")
        .unwrap();
    audio.mixer_mut(i).unwrap().transition_to(muffled, 1.0);
    audio.tick(0.5);
    audio.update(&scene);
    let cutoff =
        |log: &Arc<Mutex<NullLog>>| log.lock().unwrap().tracks[&sfx_t].effects[0].values[0];
    assert!(
        (cutoff(&log) - 2750.0).abs() < 0.5,
        "halfway: {}",
        cutoff(&log)
    );
    audio.tick(0.5);
    audio.update(&scene);
    assert_eq!(cutoff(&log), 500.0);
    assert!(audio.mixer_mut(i).unwrap().set_float("MusicVol", 0.0));
    audio.update(&scene);
    assert_eq!(log.lock().unwrap().tracks[&music_t].volume, 1.0);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn solo_silences_other_groups_and_structure_edits_restart() {
    let (root, music, sfx, _) = project("solo");
    let mut scene = Scene::default_3d("t");
    let a = scene.spawn("Music", None);
    scene.add(
        a,
        AudioSource {
            looping: true,
            ..source("Music")
        },
    );
    let (mut audio, log) = Audio::null(root.clone());
    audio.play(&scene, a);
    let mut edited = audio.mixer(0).unwrap().asset.clone();
    edited.group_mut(sfx).unwrap().solo = true;
    audio.replace_mixer("Assets/Audio/Main.mixer", edited.clone());
    audio.update(&scene);
    let volume = |log: &Arc<Mutex<NullLog>>| {
        log.lock()
            .unwrap()
            .voices
            .values()
            .next()
            .unwrap()
            .params
            .volume
    };
    assert_eq!(volume(&log), 0.0, "Music isn't soloed");

    // Adding an effect rebuilds the tracks; the looping source starts again on the new ones.
    edited.add_effect(music, EffectKind::Echo);
    let old = audio.mixer(0).unwrap().tracks[&music];
    audio.replace_mixer("Assets/Audio/Main.mixer", edited);
    audio.update(&scene);
    let new = audio.mixer(0).unwrap().tracks[&music];
    assert_ne!(old, new);
    assert!(audio.is_playing(a));
    assert_eq!(voice_track(&log), Some(new));
    assert!(
        !log.lock().unwrap().tracks.contains_key(&old),
        "old tracks removed"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn missing_outputs_warn_once_and_play_directly() {
    let (root, ..) = project("missing");
    let mut scene = Scene::default_3d("t");
    let a = scene.spawn("A", None);
    scene.add(a, source("Nope"));
    let (mut audio, log) = Audio::null(root.clone());
    assert!(audio.play(&scene, a));
    assert!(audio.play(&scene, a));
    assert_eq!(voice_track(&log), None);
    assert_eq!(audio.take_logs().len(), 1);
    assert_eq!(
        audio.find_group("SFX", None).as_deref(),
        Some("Assets/Audio/Main.mixer#SFX")
    );
    assert_eq!(audio.find_group("Nope", None), None);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn filters_and_reverb_zones_get_a_track_of_their_own() {
    let (root, _, sfx, _) = project("filters");
    let mut scene = Scene::default_3d("t");
    let a = scene.spawn("Radio", None);
    scene.add(a, source("SFX"));
    scene.add(
        a,
        AudioLowPassFilter {
            cutoff_frequency: 900.0,
            ..Default::default()
        },
    );
    scene.add(a, AudioEchoFilter::default());
    // The listener (Main Camera at 0,1,-10) stands 2.5 m into the fade of this zone.
    let zone = scene.spawn("Cave", None);
    scene.add(zone, AudioReverbZone::default());
    scene.set_transform(zone, Transform::from_position(Vec3::new(0.0, 1.0, 2.5)));
    let (mut audio, log) = Audio::null(root.clone());
    audio.play(&scene, a);
    let own = voice_track(&log).unwrap();
    let sfx_t = audio.mixer(0).unwrap().tracks[&sfx];
    {
        let log = log.lock().unwrap();
        let t = &log.tracks[&own];
        assert_eq!(t.parent, Some(sfx_t), "own track feeds the Output group");
        let kinds: Vec<EffectKind> = t.effects.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            [EffectKind::Lowpass, EffectKind::Echo, EffectKind::Reverb]
        );
        assert_eq!(t.effects[0].values[0], 900.0);
        assert!(
            (t.effects[2].values[3] - 15.0).abs() < 1e-3,
            "half the Generic wet mix"
        );
    }
    scene
        .world
        .get::<&mut AudioSource>(a)
        .unwrap()
        .bypass_effects = true;
    audio.update(&scene);
    {
        let log = log.lock().unwrap();
        let fx = &log.tracks[&own].effects;
        assert!(
            fx[0].bypass && fx[1].bypass && !fx[2].bypass,
            "zones aren't source effects"
        );
    }
    audio.stop(a);
    assert!(
        !log.lock().unwrap().tracks.contains_key(&own),
        "removed with the voice"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn clip_at_point_time_and_doppler() {
    let mut scene = Scene::default_3d("t");
    let (mut audio, log) = Audio::null("root".into());
    // Listener at (0, 1, -10); 4 m to its right.
    assert!(audio.play_clip_at_point(
        &scene,
        "Assets/Audio/Beep.wav",
        Vec3::new(4.0, 1.0, -10.0),
        1.0
    ));
    let p = log.lock().unwrap().voices.values().next().unwrap().params;
    assert!((p.volume - 0.25).abs() < 1e-4 && (p.pan - 1.0).abs() < 1e-4);
    assert_eq!(audio.voice_count(), 1);

    let e = scene.spawn("Car", None);
    scene.add(
        e,
        AudioSource {
            clip: Some("Assets/Audio/Beep.wav".into()),
            spatial_blend: 1.0,
            ..Default::default()
        },
    );
    scene.set_transform(e, Transform::from_position(Vec3::new(0.0, 1.0, 30.0)));
    audio.play(&scene, e);
    audio.set_time(e, 0.25);
    assert_eq!(audio.time(e), 0.25);
    audio.tick(0.1);
    audio.update(&scene);
    // Driving 3.43 m towards the listener in 0.1 s = 34.3 m/s.
    scene.set_transform(e, Transform::from_position(Vec3::new(0.0, 1.0, 26.57)));
    audio.tick(0.1);
    audio.update(&scene);
    let log = log.lock().unwrap();
    let car = log
        .voices
        .values()
        .find(|v| v.params.pitch != 1.0)
        .expect("shifted");
    assert!(
        (car.params.pitch - 343.0 / 308.7).abs() < 1e-3,
        "{}",
        car.params.pitch
    );
}

#[test]
fn voices_past_the_limit_are_silenced_by_priority() {
    let mut scene = Scene::default_3d("t");
    let (mut audio, log) = Audio::null("root".into());
    let mut important = None;
    for i in 0..(MAX_REAL_VOICES + 2) {
        let e = scene.spawn("S", None);
        scene.add(
            e,
            AudioSource {
                clip: Some(format!("Assets/{i}.wav")),
                priority: if i == 0 { 0 } else { 200 },
                volume: if i == 0 { 0.1 } else { 1.0 },
                ..Default::default()
            },
        );
        important.get_or_insert(e);
        audio.play(&scene, e);
    }
    audio.update(&scene);
    let log = log.lock().unwrap();
    let silent = log
        .voices
        .values()
        .filter(|v| v.params.volume == 0.0)
        .count();
    assert_eq!(silent, 2);
    let first = log
        .voices
        .values()
        .find(|v| v.path.ends_with("0.wav"))
        .unwrap();
    assert_eq!(
        first.params.volume, 0.1,
        "priority 0 stays audible even when quiet"
    );
}
