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

/// Editor Pause holds ambient voices too (weather rain, wind and thunder): they used to keep
/// playing through Pause while every other sound stopped.
#[test]
fn editor_pause_holds_ambient_voices() {
    let (mut audio, log) = Audio::null("root".into());
    assert!(
        audio
            .play_ambient("Assets/Audio/Rain.wav", "", 0.5, true)
            .is_some()
    );
    assert!(audio.play_clip("Assets/Audio/Thunder.wav", 0.8));
    assert_eq!(log.lock().unwrap().voices.len(), 2);
    audio.set_paused(true);
    assert!(log.lock().unwrap().voices.values().all(|v| v.paused));
    audio.set_paused(false);
    assert!(log.lock().unwrap().voices.values().all(|v| !v.paused));
}

#[test]
fn audio_files_are_clips() {
    assert!(is_clip("Assets/a.WAV") && is_clip("x.ogg") && is_clip("x.mp3"));
    assert!(!is_clip("x.lua"));
}

/// A Play On Awake source plays whenever its object is switched on (or the component is
/// enabled again), like the familiar engine's OnEnable: a pooled explosion switched off and on
/// again stayed silent, and so did a source whose object started inactive.
#[test]
fn play_on_awake_sources_play_when_switched_on() {
    let (mut scene, e) = scene_with_source(beep());
    let late = scene.spawn("Late", None);
    scene.add(late, beep());
    scene.game_object_mut(late).active = false;
    let (mut audio, log) = Audio::null("root".into());
    audio.play_on_awake(&scene, None);
    audio.update(&scene);
    assert!(audio.is_playing(e) && !audio.is_playing(late));

    scene.game_object_mut(e).active = false;
    audio.update(&scene);
    assert!(!audio.is_playing(e), "switched off: stopped");
    scene.game_object_mut(e).active = true;
    scene.game_object_mut(late).active = true;
    audio.update(&scene);
    assert!(
        audio.is_playing(e),
        "switched on again: plays from the start"
    );
    assert!(
        audio.is_playing(late),
        "first switched on while playing: plays"
    );
    assert_eq!(log.lock().unwrap().played.len(), 3);

    // The component disabled and enabled again.
    scene.world.get::<&mut AudioSource>(e).unwrap().enabled = false;
    audio.update(&scene);
    assert!(!audio.is_playing(e));
    scene.world.get::<&mut AudioSource>(e).unwrap().enabled = true;
    audio.update(&scene);
    assert!(audio.is_playing(e));

    // A source that finished (or that a script stopped) stays quiet while it stays on.
    audio.stop(late);
    audio.update(&scene);
    audio.update(&scene);
    assert!(!audio.is_playing(late));
    assert_eq!(log.lock().unwrap().played.len(), 4);
}

/// Switching to another Audio Listener (a cut to another camera) moved the "ear" across the
/// level in one frame: Doppler took that jump as the listener's speed and bent the pitch of
/// every 3D sound for a frame.
#[test]
fn switching_listener_does_not_bend_the_pitch() {
    let (mut scene, e) = scene_with_source(AudioSource {
        spatial_blend: 1.0,
        looping: true,
        ..beep()
    });
    scene.set_transform(e, Transform::from_position(Vec3::new(0.0, 1.0, 40.0)));
    let first = scene
        .world
        .query::<(Entity, &AudioListener)>()
        .iter()
        .map(|(l, _)| l)
        .next()
        .unwrap();
    let other = scene.spawn("Far Camera", None);
    scene.set_transform(other, Transform::from_position(Vec3::new(0.0, 1.0, 100.0)));
    scene.add(other, AudioListener::default());
    scene.game_object_mut(other).active = false;
    let (mut audio, log) = Audio::null("root".into());
    audio.play(&scene, e);
    let pitch = |log: &std::sync::Arc<std::sync::Mutex<NullLog>>| {
        log.lock()
            .unwrap()
            .voices
            .values()
            .next()
            .unwrap()
            .params
            .pitch
    };
    for _ in 0..3 {
        audio.tick(1.0 / 60.0);
        audio.update(&scene);
    }
    assert!((pitch(&log) - 1.0).abs() < 1e-4);
    scene.game_object_mut(first).active = false;
    scene.game_object_mut(other).active = true;
    audio.tick(1.0 / 60.0);
    audio.update(&scene);
    let p = pitch(&log);
    assert!((p - 1.0).abs() < 1e-4, "no Doppler from the cut, pitch {p}");
}
