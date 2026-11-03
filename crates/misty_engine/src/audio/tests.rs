//! Audio playback bookkeeping against the silent backend.

use glam::Vec3;

use super::*;
use crate::scene::Transform;

fn scene_with_source(source: AudioSource) -> (Scene, Entity) {
    let mut scene = Scene::default_3d("t");
    let e = scene.spawn("Speaker", None);
    scene.add(e, source);
    (scene, e)
}

fn beep() -> AudioSource {
    AudioSource {
        clip: Some("Assets/Audio/Beep.wav".into()),
        ..Default::default()
    }
}

#[test]
fn defaults_match_the_familiar_inspector() {
    let s = AudioSource::default();
    assert!(s.enabled && s.play_on_awake && !s.looping && !s.mute);
    assert_eq!((s.volume, s.pitch, s.spatial_blend), (1.0, 1.0, 0.0));
    assert_eq!((s.min_distance, s.max_distance), (1.0, 500.0));
    assert_eq!(s.rolloff, crate::scene::AudioRolloff::Logarithmic);
    assert!(AudioListener::default().enabled);
}

#[test]
fn play_on_awake_starts_only_flagged_sources_with_clips() {
    let (mut scene, a) = scene_with_source(beep());
    let quiet = scene.spawn("Quiet", None);
    scene.add(
        quiet,
        AudioSource {
            play_on_awake: false,
            ..beep()
        },
    );
    let empty = scene.spawn("Empty", None);
    scene.add(empty, AudioSource::default());
    let (mut audio, log) = Audio::null("root".into());
    audio.play_on_awake(&scene, None);
    assert!(audio.is_playing(a));
    assert!(!audio.is_playing(quiet) && !audio.is_playing(empty));
    let log = log.lock().unwrap();
    assert_eq!(log.played, [PathBuf::from("root/Assets/Audio/Beep.wav")]);
}

#[test]
fn stop_pause_and_finish() {
    let (scene, e) = scene_with_source(beep());
    let (mut audio, log) = Audio::null("root".into());
    assert!(audio.play(&scene, e));
    audio.pause(e);
    assert!(!audio.is_playing(e));
    audio.unpause(e);
    assert!(audio.is_playing(e));
    audio.stop(e);
    assert!(!audio.is_playing(e));
    assert!(log.lock().unwrap().voices.is_empty());

    audio.play(&scene, e);
    let id = *log.lock().unwrap().voices.keys().next().unwrap();
    log.lock().unwrap().finish(id);
    assert!(!audio.is_playing(e), "a finished clip is no longer playing");
    audio.update(&scene);
    assert_eq!(audio.voice_count(), 0);
}

#[test]
fn update_follows_listener_and_source_positions() {
    let (mut scene, e) = scene_with_source(AudioSource {
        spatial_blend: 1.0,
        ..beep()
    });
    // Main Camera (listener) sits at (0, 1, -10) looking +Z.
    scene.set_transform(e, Transform::from_position(Vec3::new(4.0, 1.0, -10.0)));
    let (mut audio, log) = Audio::null("root".into());
    audio.play(&scene, e);
    audio.update(&scene);
    let params = |log: &std::sync::Arc<std::sync::Mutex<NullLog>>| {
        log.lock().unwrap().voices.values().next().unwrap().params
    };
    let p = params(&log);
    assert!((p.volume - 0.25).abs() < 1e-4 && (p.pan - 1.0).abs() < 1e-4);

    scene.set_transform(e, Transform::from_position(Vec3::new(-2.0, 1.0, -10.0)));
    audio.set_muted(true);
    audio.update(&scene);
    let p = params(&log);
    assert_eq!(p.volume, 0.0, "Mute Audio silences");
    assert!((p.pan + 1.0).abs() < 1e-4);
}

#[test]
fn destroyed_or_disabled_sources_stop() {
    let (mut scene, e) = scene_with_source(beep());
    let (mut audio, _log) = Audio::null("root".into());
    audio.play(&scene, e);
    scene.world.get::<&mut AudioSource>(e).unwrap().enabled = false;
    audio.update(&scene);
    assert!(!audio.is_playing(e));

    scene.world.get::<&mut AudioSource>(e).unwrap().enabled = true;
    audio.play(&scene, e);
    scene.despawn(e);
    audio.update(&scene);
    assert_eq!(audio.voice_count(), 0);
}

#[test]
fn one_shots_and_editor_pause() {
    let (scene, e) = scene_with_source(beep());
    let (mut audio, log) = Audio::null("root".into());
    assert!(audio.play_one_shot(&scene, e, "Assets/Audio/Other.wav", 0.5));
    assert!(
        !audio.is_playing(e),
        "one-shots don't count as the source playing"
    );
    assert_eq!(audio.voice_count(), 1);
    audio.set_paused(true);
    assert!(log.lock().unwrap().voices.values().all(|v| v.paused));
    audio.set_paused(false);
    assert!(log.lock().unwrap().voices.values().all(|v| !v.paused));
    let v = log.lock().unwrap().voices.values().next().unwrap().clone();
    assert_eq!(v.params.volume, 0.5);
    drop(audio);
    assert!(
        log.lock().unwrap().voices.is_empty(),
        "dropping stops everything"
    );
}

#[test]
fn audio_files_are_clips() {
    assert!(is_clip("Assets/a.WAV") && is_clip("x.ogg") && is_clip("x.mp3"));
    assert!(!is_clip("x.lua"));
}
