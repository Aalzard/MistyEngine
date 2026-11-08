//! Timeline evaluation: clip weights, activation, animation blends, wrap modes, signals and
//! the file format.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use super::evaluate::{self, Baseline, blend_in, blend_out, samples, smooth, weight};
use super::*;
use crate::animation::{AnimationClip, Property};
use crate::audio::Audio;
use crate::scene::{AudioSource, DirectorWrapMode, PlayableDirector, Scene};

fn clip(start: f32, duration: f32) -> TimelineClip {
    TimelineClip {
        start,
        duration,
        ..Default::default()
    }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn ease_curve_is_symmetric() {
    for x in [0.0, 0.1, 0.25, 0.5, 0.8, 1.0] {
        assert!(near(smooth(x) + smooth(1.0 - x), 1.0));
    }
    assert!(near(smooth(0.5), 0.5));
}

#[test]
fn overlapping_clips_cross_fade_and_sum_to_one() {
    // A: 0..2, B: 1.5..3.5 → blend 1.5..2.
    let clips = vec![clip(0.0, 2.0), clip(1.5, 2.0)];
    assert!(near(blend_out(&clips, 0), 0.5));
    assert!(near(blend_in(&clips, 1), 0.5));
    assert!(near(blend_in(&clips, 0), 0.0));
    assert!(near(weight(&clips, 0, 1.0), 1.0));
    assert!(near(weight(&clips, 0, 1.75), 0.5));
    assert!(near(weight(&clips, 1, 1.75), 0.5));
    for t in [1.55, 1.6, 1.8, 1.95] {
        let sum: f32 = samples(&clips, t).iter().map(|s| s.weight).sum();
        assert!(near(sum, 1.0), "t={t} sum={sum}");
    }
    assert!(weight(&clips, 0, 1.6) > weight(&clips, 1, 1.6));
    assert!(near(weight(&clips, 1, 3.0), 1.0));
    assert!(near(weight(&clips, 1, 3.5), 0.0), "end is exclusive");
}

#[test]
fn ease_in_and_out_fade_a_lone_clip() {
    let mut c = clip(1.0, 4.0);
    c.ease_in = 1.0;
    c.ease_out = 2.0;
    let clips = vec![c];
    assert!(near(weight(&clips, 0, 0.5), 0.0));
    assert!(near(weight(&clips, 0, 1.0), 0.0));
    assert!(near(weight(&clips, 0, 1.5), 0.5));
    assert!(near(weight(&clips, 0, 2.5), 1.0));
    assert!(near(weight(&clips, 0, 4.0), 0.5));
}

#[test]
fn clip_in_and_speed_set_the_local_time() {
    let mut c = clip(2.0, 3.0);
    c.clip_in = 0.5;
    c.speed = 2.0;
    assert!(near(evaluate::local_time(&c, 3.0), 2.5));
    let s = samples(&[c], 3.0);
    assert!(near(s[0].local, 2.5));
}

#[test]
fn hold_extrapolation_fills_the_gap_after_a_clip() {
    let mut a = clip(0.0, 1.0);
    a.post_extrapolation = Extrapolation::Hold;
    let clips = vec![a.clone(), clip(3.0, 1.0)];
    let s = samples(&clips, 2.0);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].clip, 0);
    assert!(near(s[0].local, 1.0));
    assert!(
        samples(&[clip(0.0, 1.0)], 2.0).is_empty(),
        "None leaves the gap empty"
    );
    let s = samples(&clips, 3.5);
    assert_eq!(s[0].clip, 1, "the next clip takes over");
}

fn asset_with(kind: TrackKind, clips: Vec<TimelineClip>) -> (TimelineAsset, String) {
    let mut t = TimelineAsset::default();
    let id = t.add_track(kind, None);
    t.track_mut(&id).unwrap().clips = clips;
    (t, id)
}

#[test]
fn activation_track_switches_the_bound_object() {
    let (asset, id) = asset_with(TrackKind::Activation, vec![clip(1.0, 2.0), clip(4.0, 1.0)]);
    let mut scene = Scene::new("t");
    let lamp = scene.spawn("Lamp", None);
    let bind = |tid: &str| (tid == id).then_some(lamp);
    let clips: HashMap<String, Arc<AnimationClip>> = HashMap::new();
    let mut base = Baseline::default();
    let mut at = |t: f32, scene: &mut Scene| {
        evaluate::apply(
            scene,
            &asset,
            &evaluate::Bindings(&bind),
            &clips,
            t,
            &mut base,
        );
        scene.game_object(lamp).active
    };
    assert!(!at(0.5, &mut scene));
    assert!(at(1.0, &mut scene));
    assert!(at(2.9, &mut scene));
    assert!(!at(3.5, &mut scene));
    assert!(at(4.5, &mut scene));
    assert!(
        at(5.0, &mut scene),
        "the end of the timeline holds the last frame"
    );
    base.restore(&mut scene);
    assert!(
        scene.game_object(lamp).active,
        "restored to its value before"
    );
}

#[test]
fn muted_tracks_and_groups_do_nothing() {
    let (mut asset, id) = asset_with(TrackKind::Activation, vec![clip(1.0, 2.0)]);
    let g = asset.add_track(TrackKind::Group, None);
    asset.track_mut(&id).unwrap().group = Some(g.clone());
    asset.track_mut(&g).unwrap().muted = true;
    let mut scene = Scene::new("t");
    let lamp = scene.spawn("Lamp", None);
    let bind = |tid: &str| (tid == id).then_some(lamp);
    let clips: HashMap<String, Arc<AnimationClip>> = HashMap::new();
    let mut base = Baseline::default();
    evaluate::apply(
        &mut scene,
        &asset,
        &evaluate::Bindings(&bind),
        &clips,
        0.0,
        &mut base,
    );
    assert!(scene.game_object(lamp).active, "group muted: untouched");
}

fn slide(from: f32, to: f32) -> AnimationClip {
    let mut c = AnimationClip {
        length: 1.0,
        looping: false,
        ..Default::default()
    };
    c.set_key("", Property::Position(0), 0.0, from);
    c.set_key("", Property::Position(0), 1.0, to);
    c
}

#[test]
fn animation_track_blends_clips_and_returns_to_the_base_pose() {
    let mut a = clip(0.0, 2.0);
    a.asset = Some("A.anim".into());
    let mut b = clip(1.0, 2.0);
    b.asset = Some("B.anim".into());
    let (asset, id) = asset_with(TrackKind::Animation, vec![a, b]);
    let mut clips: HashMap<String, Arc<AnimationClip>> = HashMap::new();
    // A holds x = 10 (non-looping, clamped); B holds x = 20.
    clips.insert("A.anim".into(), Arc::new(slide(10.0, 10.0)));
    clips.insert("B.anim".into(), Arc::new(slide(20.0, 20.0)));
    let mut scene = Scene::new("t");
    let cube = scene.spawn("Cube", None);
    let mut t0 = scene.transform(cube);
    t0.position.x = -5.0;
    scene.set_transform(cube, t0);
    let bind = |tid: &str| (tid == id).then_some(cube);
    let mut base = Baseline::default();
    let mut x = |t: f32, scene: &mut Scene| {
        evaluate::apply(
            scene,
            &asset,
            &evaluate::Bindings(&bind),
            &clips,
            t,
            &mut base,
        );
        scene.transform(cube).position.x
    };
    assert!(near(x(0.5, &mut scene), 10.0));
    assert!(near(x(1.5, &mut scene), 15.0), "half way through the blend");
    assert!(near(x(2.5, &mut scene), 20.0));
    // Past the end the timeline holds its last frame: B.
    assert!(near(x(9.0, &mut scene), 20.0));
    base.restore(&mut scene);
    assert!(near(scene.transform(cube).position.x, -5.0));
}

#[test]
fn ease_in_blends_from_the_scene_value() {
    let mut a = clip(0.0, 2.0);
    a.asset = Some("A.anim".into());
    a.ease_in = 1.0;
    let (mut asset, id) = asset_with(TrackKind::Animation, vec![a]);
    asset.duration_mode = DurationMode::FixedLength;
    asset.fixed_duration = 4.0;
    let mut clips: HashMap<String, Arc<AnimationClip>> = HashMap::new();
    clips.insert("A.anim".into(), Arc::new(slide(10.0, 10.0)));
    let mut scene = Scene::new("t");
    let cube = scene.spawn("Cube", None);
    let bind = |tid: &str| (tid == id).then_some(cube);
    let mut base = Baseline::default();
    evaluate::apply(
        &mut scene,
        &asset,
        &evaluate::Bindings(&bind),
        &clips,
        0.5,
        &mut base,
    );
    assert!(near(scene.transform(cube).position.x, 5.0));
    evaluate::apply(
        &mut scene,
        &asset,
        &evaluate::Bindings(&bind),
        &clips,
        3.0,
        &mut base,
    );
    assert!(
        near(scene.transform(cube).position.x, 0.0),
        "gap: back to base"
    );
}

#[test]
fn recorded_keys_play_on_a_track_without_clips() {
    let (mut asset, id) = asset_with(TrackKind::Animation, vec![]);
    asset.track_mut(&id).unwrap().recorded = Some(slide(0.0, 4.0));
    asset.duration_mode = DurationMode::FixedLength;
    asset.fixed_duration = 1.0;
    let mut scene = Scene::new("t");
    let cube = scene.spawn("Cube", None);
    let bind = |tid: &str| (tid == id).then_some(cube);
    let clips: HashMap<String, Arc<AnimationClip>> = HashMap::new();
    let mut base = Baseline::default();
    evaluate::apply(
        &mut scene,
        &asset,
        &evaluate::Bindings(&bind),
        &clips,
        0.5,
        &mut base,
    );
    assert!(near(scene.transform(cube).position.x, 2.0));
}

#[test]
fn duration_follows_clips_and_markers_or_is_fixed() {
    let (mut asset, id) = asset_with(TrackKind::Activation, vec![clip(1.0, 2.0)]);
    assert!(near(asset.duration(), 3.0));
    asset.markers.push(Marker {
        time: 4.5,
        ..Default::default()
    });
    assert!(near(asset.duration(), 4.5));
    asset.track_mut(&id).unwrap().clips.push(clip(5.0, 1.0));
    assert!(near(asset.duration(), 6.0));
    asset.duration_mode = DurationMode::FixedLength;
    asset.fixed_duration = 2.0;
    assert!(near(asset.duration(), 2.0));
}

#[test]
fn file_round_trip_and_repairs() {
    let mut t = TimelineAsset::default();
    let g = t.add_track(TrackKind::Group, None);
    let a = t.add_track(TrackKind::Animation, Some(&g));
    let s = t.add_track(TrackKind::Signal, None);
    t.add_track(TrackKind::Activation, Some(&g));
    {
        let tr = t.track_mut(&a).unwrap();
        tr.clips.push(TimelineClip {
            name: "Walk".into(),
            asset: Some("Assets/Animations/Walk.anim".into()),
            start: 0.5,
            duration: 2.0,
            ease_in: 0.25,
            ..Default::default()
        });
        tr.locked = true;
    }
    t.track_mut(&s).unwrap().markers.push(Marker {
        time: 1.0,
        method: "OnCue".into(),
        parameter: Some("hello".into()),
        retroactive: false,
    });
    let back = TimelineAsset::from_json(&t.to_json()).unwrap();
    assert_eq!(back, t);
    // Grouped tracks sit right after their group.
    let order: Vec<_> = t.tracks.iter().map(|x| x.kind).collect();
    assert_eq!(
        order,
        [
            TrackKind::Group,
            TrackKind::Animation,
            TrackKind::Activation,
            TrackKind::Signal
        ]
    );
    t.remove_track(&g);
    assert_eq!(t.tracks.len(), 1, "a group goes with its tracks");

    let broken = r#"{"frame_rate": 0, "tracks": [
        {"name": "A", "clips": [{"start": 3, "duration": 1}, {"start": 1, "duration": 1, "speed": 0}]},
        {"name": "B"}, {"id": "t1", "name": "C"}]}"#;
    let t = TimelineAsset::from_json(broken).unwrap();
    assert!(near(t.frame_rate, 60.0));
    let ids: Vec<_> = t.tracks.iter().map(|x| x.id.as_str()).collect();
    assert_eq!(ids.len(), 3);
    assert!(
        ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2],
        "{ids:?}"
    );
    assert!(near(t.tracks[0].clips[0].start, 1.0), "clips sorted");
    assert!(near(t.tracks[0].clips[0].speed, 1.0));
    assert!(is_timeline("Assets/Intro.timeline"));
    assert!(!is_timeline("Assets/Intro.anim"));
}

// ---- Play mode ------------------------------------------------------------------------------

/// A project folder with the asset as `T.timeline` and `Move.anim` (x: 0 → 10 over 1 s).
fn project(asset: &TimelineAsset) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_timeline_{n}"));
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    asset.save(&root.join("Assets/T.timeline")).unwrap();
    slide(0.0, 10.0)
        .save(&root.join("Assets/Move.anim"))
        .unwrap();
    root
}

fn director_scene(wrap: DirectorWrapMode) -> (Scene, hecs::Entity) {
    let mut scene = Scene::new("t");
    let d = scene.spawn("Director", None);
    scene.add(
        d,
        PlayableDirector {
            playable: Some("Assets/T.timeline".into()),
            wrap_mode: wrap,
            ..Default::default()
        },
    );
    (scene, d)
}

/// One second: an Activation clip and a marker at 0.5 s.
fn marker_asset() -> TimelineAsset {
    let mut t = TimelineAsset::default();
    let id = t.add_track(TrackKind::Activation, None);
    t.track_mut(&id).unwrap().clips.push(clip(0.0, 1.0));
    t.markers.push(Marker {
        time: 0.5,
        method: "Cue".into(),
        ..Default::default()
    });
    t
}

fn signals(events: &[DirectorEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, DirectorEvent::Signal { .. }))
        .count()
}

#[test]
fn wrap_none_stops_at_the_end_and_fires_once() {
    let root = project(&marker_asset());
    let (mut scene, d) = director_scene(DirectorWrapMode::None);
    let mut audio = Audio::null(root.clone()).0;
    let mut p = DirectorPlayer::new(root);
    p.play_on_awake(&scene, None);
    assert_eq!(p.state(d), PlayState::Playing);
    let mut fired = 0;
    let mut stopped = 0;
    for _ in 0..90 {
        let ev = p.update(&mut scene, &mut audio, 1.0 / 60.0, 1.0 / 60.0);
        fired += signals(&ev);
        stopped += ev
            .iter()
            .filter(|e| **e == DirectorEvent::Stopped(d))
            .count();
    }
    assert_eq!(fired, 1);
    assert_eq!(stopped, 1);
    assert_eq!(p.state(d), PlayState::Stopped);
    assert_eq!(p.time(d), 0.0);
}

#[test]
fn wrap_hold_stays_on_the_last_frame() {
    let root = project(&marker_asset());
    let (mut scene, d) = director_scene(DirectorWrapMode::Hold);
    let mut audio = Audio::null(root.clone()).0;
    let mut p = DirectorPlayer::new(root);
    p.play(&scene, d);
    let mut fired = 0;
    for _ in 0..120 {
        fired += signals(&p.update(&mut scene, &mut audio, 1.0 / 60.0, 1.0 / 60.0));
    }
    assert_eq!(fired, 1);
    assert_eq!(p.state(d), PlayState::Playing);
    assert!((p.time(d) - 1.0).abs() < 1e-6);
}

#[test]
fn wrap_loop_starts_over_and_fires_every_loop() {
    let root = project(&marker_asset());
    let (mut scene, d) = director_scene(DirectorWrapMode::Loop);
    let mut audio = Audio::null(root.clone()).0;
    let mut p = DirectorPlayer::new(root);
    p.play(&scene, d);
    let mut fired = 0;
    // First frame evaluates 0, then 0.1 s steps up to 2.45 s: markers at 0.5, 1.5 (not 2.5).
    for _ in 0..25 {
        fired += signals(&p.update(&mut scene, &mut audio, 0.1, 0.1));
    }
    assert_eq!(p.state(d), PlayState::Playing);
    assert!(p.time(d) < 1.0);
    assert_eq!(fired, 2);
    for _ in 0..2 {
        fired += signals(&p.update(&mut scene, &mut audio, 0.1, 0.1));
    }
    assert_eq!(fired, 3);
}

#[test]
fn pause_resume_stop_and_unscaled_time() {
    let root = project(&marker_asset());
    let (mut scene, d) = director_scene(DirectorWrapMode::Hold);
    scene
        .world
        .get::<&mut PlayableDirector>(d)
        .unwrap()
        .update_method = crate::scene::DirectorUpdateMode::UnscaledGameTime;
    let mut audio = Audio::null(root.clone()).0;
    let mut p = DirectorPlayer::new(root);
    p.play(&scene, d);
    p.update(&mut scene, &mut audio, 0.0, 0.1); // first frame: start time
    p.update(&mut scene, &mut audio, 0.0, 0.1);
    assert!(
        (p.time(d) - 0.1).abs() < 1e-6,
        "unscaled clock moves at time scale 0"
    );
    p.pause(&mut audio, d);
    p.update(&mut scene, &mut audio, 0.0, 0.1);
    assert_eq!(p.state(d), PlayState::Paused);
    assert!((p.time(d) - 0.1).abs() < 1e-6);
    p.resume(&mut audio, d);
    p.update(&mut scene, &mut audio, 0.0, 0.1);
    assert!((p.time(d) - 0.2).abs() < 1e-6);
    p.stop(&mut scene, &mut audio, d);
    let ev = p.update(&mut scene, &mut audio, 0.0, 0.1);
    assert!(ev.contains(&DirectorEvent::Stopped(d)));
    assert_eq!(p.state(d), PlayState::Stopped);
}

#[test]
fn audio_track_plays_through_the_bound_source() {
    let mut t = TimelineAsset::default();
    let id = t.add_track(TrackKind::Audio, None);
    t.track_mut(&id).unwrap().clips.push(TimelineClip {
        asset: Some("Assets/Beep.wav".into()),
        start: 0.5,
        duration: 0.5,
        ..Default::default()
    });
    t.duration_mode = DurationMode::FixedLength;
    t.fixed_duration = 3.0;
    let root = project(&t);
    let (mut scene, d) = director_scene(DirectorWrapMode::None);
    let speaker = scene.spawn("Speaker", None);
    scene.add(speaker, AudioSource::default());
    scene
        .world
        .get::<&mut PlayableDirector>(d)
        .unwrap()
        .set_binding(&id, Some(speaker));
    let (mut audio, log) = Audio::null(root.clone());
    let mut p = DirectorPlayer::new(root);
    p.play(&scene, d);
    for _ in 0..5 {
        p.update(&mut scene, &mut audio, 0.1, 0.1);
    }
    assert!(log.lock().unwrap().played.is_empty(), "not before the clip");
    for _ in 0..3 {
        p.update(&mut scene, &mut audio, 0.1, 0.1);
    }
    let played = log.lock().unwrap().played.clone();
    assert_eq!(played.len(), 1, "{played:?}");
    assert!(played[0].ends_with("Assets/Beep.wav"));
    assert!(audio.is_playing(speaker));
    assert_eq!(
        scene.world.get::<&AudioSource>(speaker).unwrap().clip,
        None,
        "the source keeps its own clip"
    );
    for _ in 0..5 {
        p.update(&mut scene, &mut audio, 0.1, 0.1);
    }
    assert!(!audio.is_playing(speaker), "stopped after the clip");
}

#[test]
fn animation_track_moves_the_bound_object_in_play_mode() {
    let mut t = TimelineAsset::default();
    let id = t.add_track(TrackKind::Animation, None);
    t.track_mut(&id).unwrap().clips.push(TimelineClip {
        asset: Some("Assets/Move.anim".into()),
        duration: 1.0,
        ..Default::default()
    });
    let root = project(&t);
    let (mut scene, d) = director_scene(DirectorWrapMode::Hold);
    let cube = scene.spawn("Cube", None);
    scene
        .world
        .get::<&mut PlayableDirector>(d)
        .unwrap()
        .set_binding(&id, Some(cube));
    let mut audio = Audio::null(root.clone()).0;
    let mut p = DirectorPlayer::new(root);
    p.play(&scene, d);
    for _ in 0..6 {
        p.update(&mut scene, &mut audio, 0.1, 0.1);
    }
    let x = scene.transform(cube).position.x;
    assert!((x - 5.0).abs() < 0.2, "x = {x}");
}
