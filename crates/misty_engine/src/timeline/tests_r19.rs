//! Round 19 timeline: Camera tracks (shots, blends, the rig override), Control tracks
//! (activation, sub-timelines and their time mapping, impulses, post-playback), nested
//! signals and the `played` event.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use glam::Vec3;
use hecs::Entity;

use super::evaluate::{self, Baseline};
use super::tracks::{camera, control};
use super::*;
use crate::animation::AnimationClip;
use crate::audio::Audio;
use crate::scene::{Camera, DirectorWrapMode, PlayableDirector, Scene, Transform};
use crate::vcam::{
    AimKind, BlendDefinition, BlendStyle, BodyKind, BrainUpdate, CameraBrain, CameraOverride,
    CameraRig, ImpulseSource, SolveContext, VirtualCamera, solve,
};

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn clip(start: f32, duration: f32, reference: &str) -> TimelineClip {
    TimelineClip {
        start,
        duration,
        reference: Some(reference.into()),
        ..Default::default()
    }
}

/// Clips and timelines by path, for nested evaluation.
#[derive(Default)]
struct Sources {
    clips: HashMap<String, Arc<AnimationClip>>,
    timelines: HashMap<String, TimelineAsset>,
}

impl ClipSource for Sources {
    fn clip(&self, path: &str) -> Option<&AnimationClip> {
        self.clips.get(path).map(|c| c.as_ref())
    }

    fn timeline(&self, path: &str) -> Option<&TimelineAsset> {
        self.timelines.get(path)
    }
}

#[test]
fn camera_track_shots_cut_blend_and_ease_with_the_live_camera() {
    let mut t = TimelineAsset::default();
    let id = t.add_track(TrackKind::Camera, None);
    let mut eased = clip(8.0, 2.0, "r3");
    eased.ease_in = 1.0;
    t.track_mut(&id).unwrap().clips = vec![clip(0.0, 4.0, "r1"), clip(3.0, 4.0, "r2"), eased];
    let mut scene = Scene::new("t");
    let brain = scene.spawn("Main Camera", None);
    let (a, b, c) = (
        scene.spawn("A", None),
        scene.spawn("B", None),
        scene.spawn("C", None),
    );
    let mut d = PlayableDirector::default();
    d.set_binding(&id, Some(brain));
    for (r, e) in [("r1", a), ("r2", b), ("r3", c)] {
        d.set_reference(r, Some(e));
    }
    let track = t.track(&id).unwrap();
    let shot = |at: f32| camera::shot(track, &d, brain, at);

    // A alone: the brain shows A fully, nothing to blend from.
    let s = shot(1.0).unwrap();
    assert_eq!((s.from, s.to), (None, Some(a)));
    assert!(near(s.weight, 1.0));
    // Overlap 3..4: A → B with the smoothstep mix; halfway is an even blend.
    let s = shot(3.5).unwrap();
    assert_eq!((s.from, s.to), (Some(a), Some(b)));
    assert!(near(s.weight, 0.5), "{}", s.weight);
    let s = shot(3.25).unwrap();
    assert!(near(s.weight, evaluate::smooth(0.25)), "{}", s.weight);
    assert!(s.dominant() == Some(a));
    // A cut: right after the overlap B is alone.
    let s = shot(4.0).unwrap();
    assert_eq!((s.from, s.to), (None, Some(b)));
    // Gap: no override (the brain's own live camera).
    assert!(shot(7.5).is_none());
    // Ease in from the live camera (`None`).
    let s = shot(8.5).unwrap();
    assert_eq!((s.from, s.to), (None, Some(c)));
    assert!(near(s.weight, 0.5));
    assert_eq!(s.dominant(), Some(c));
    // evaluate::apply hands the shots back (and touches nothing else).
    let mut base = Baseline::default();
    let shots = evaluate::apply(&mut scene, &t, &d, &Sources::default(), 3.5, &mut base);
    assert_eq!(shots.len(), 1);
    assert_eq!(shots[0].brain, brain);
}

fn fixed_vcam(s: &mut Scene, name: &str, priority: i32, at: Vec3) -> Entity {
    let e = s.spawn(name, None);
    s.set_transform(e, Transform::from_position(at));
    s.add(
        e,
        VirtualCamera {
            priority,
            body: BodyKind::DoNothing,
            aim: AimKind::DoNothing,
            ..Default::default()
        },
    );
    e
}

fn step(rig: &mut CameraRig, s: &mut Scene, dt: f32) {
    let cx = SolveContext {
        dt,
        aspect: 16.0 / 9.0,
        physics: None,
        axis: &solve::no_axis,
        action: &solve::no_action,
    };
    rig.update(s, BrainUpdate::LateUpdate, &cx);
}

#[test]
fn the_rig_shows_timeline_overrides_and_blends_back_when_they_end() {
    let mut s = Scene::new("t");
    let brain = s.spawn("Main Camera", None);
    s.add(brain, Camera::default());
    s.add(
        brain,
        CameraBrain {
            default_blend: BlendDefinition {
                style: BlendStyle::Linear,
                duration: 1.0,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let live = fixed_vcam(&mut s, "Live", 20, Vec3::ZERO);
    let a = fixed_vcam(&mut s, "A", 1, Vec3::new(10.0, 0.0, 0.0));
    let b = fixed_vcam(&mut s, "B", 1, Vec3::new(20.0, 0.0, 0.0));
    let mut rig = CameraRig::default();
    step(&mut rig, &mut s, 0.1);
    assert!(s.world_position(brain).length() < 1e-4, "priority camera");

    // A low-priority camera wins while the timeline overrides the brain.
    rig.set_overrides(vec![CameraOverride {
        brain,
        from: Some(a),
        to: Some(b),
        weight: 0.25,
    }]);
    step(&mut rig, &mut s, 0.1);
    assert!((s.world_position(brain).x - 12.5).abs() < 1e-3);
    assert_eq!(rig.live_camera(brain), Some(a));
    assert!(rig.is_blending(brain));
    assert!(rig.is_live(a) && rig.is_live(b) && !rig.is_live(live));
    // Ease from the brain's own camera (`None`).
    rig.set_overrides(vec![CameraOverride {
        brain,
        from: None,
        to: Some(b),
        weight: 0.5,
    }]);
    step(&mut rig, &mut s, 0.1);
    assert!((s.world_position(brain).x - 10.0).abs() < 1e-3);
    rig.set_overrides(vec![CameraOverride {
        brain,
        from: None,
        to: Some(b),
        weight: 1.0,
    }]);
    step(&mut rig, &mut s, 0.1);
    assert!((s.world_position(brain).x - 20.0).abs() < 1e-3);
    assert!(!rig.is_blending(brain));
    // The override ends: the brain blends from B back to the live camera (1 s, linear).
    rig.set_overrides(Vec::new());
    step(&mut rig, &mut s, 0.5);
    let x = s.world_position(brain).x;
    assert!((x - 10.0).abs() < 1e-3, "half way back: {x}");
    assert_eq!(rig.live_camera(brain), Some(live));
    step(&mut rig, &mut s, 0.6);
    assert!(s.world_position(brain).length() < 1e-3);
}

#[test]
fn sub_timeline_time_maps_through_clip_in_speed_and_wrap() {
    let mut c = clip(5.0, 4.0, "r1");
    c.clip_in = 1.0;
    c.speed = 2.0;
    assert!(near(control::sub_time(&c, 5.0, 10.0, false), 1.0));
    assert!(near(control::sub_time(&c, 5.25, 10.0, false), 1.5));
    assert!(near(control::sub_time(&c, 6.0, 10.0, false), 3.0));
    // Past its end the sub-timeline holds its last frame, or loops.
    assert!(near(control::sub_time(&c, 8.0, 4.0, false), 4.0));
    assert!(near(control::sub_time(&c, 8.0, 4.0, true), 3.0));

    // A nested timeline switching a lamp on between its own 2 s and 3 s.
    let mut nested = TimelineAsset::default();
    let act = nested.add_track(TrackKind::Activation, None);
    nested.track_mut(&act).unwrap().clips.push(TimelineClip {
        start: 2.0,
        duration: 1.0,
        ..Default::default()
    });
    nested.duration_mode = DurationMode::FixedLength;
    nested.fixed_duration = 10.0;
    let mut parent = TimelineAsset::default();
    let ctl = parent.add_track(TrackKind::Control, None);
    let mut cc = c.clone();
    cc.control.activation = false;
    parent.track_mut(&ctl).unwrap().clips.push(cc);

    let mut scene = Scene::new("t");
    let lamp = scene.spawn("Lamp", None);
    let sub = scene.spawn("Sub Director", None);
    let mut sd = PlayableDirector {
        playable: Some("Assets/Sub.timeline".into()),
        play_on_awake: false,
        ..Default::default()
    };
    sd.set_binding(&act, Some(lamp));
    scene.add(sub, sd);
    let mut d = PlayableDirector::default();
    d.set_reference("r1", Some(sub));
    let mut src = Sources::default();
    src.timelines.insert("Assets/Sub.timeline".into(), nested);
    let mut base = Baseline::default();
    let mut lamp_at = |t: f32, scene: &mut Scene| {
        evaluate::apply(scene, &parent, &d, &src, t, &mut base);
        scene.game_object(lamp).active
    };
    // The lamp starts active: before the nested clip the activation track turns it off.
    assert!(
        !lamp_at(5.25, &mut scene),
        "local 1.5: before the nested clip"
    );
    assert!(lamp_at(5.6, &mut scene), "local 2.2: inside");
    assert!(!lamp_at(6.1, &mut scene), "local 3.2: after");
    base.restore(&mut scene);
    assert!(scene.game_object(lamp).active, "restored");
}

#[test]
fn control_track_activates_its_object_and_reverts_on_stop() {
    let mut t = TimelineAsset::default();
    let ctl = t.add_track(TrackKind::Control, None);
    t.track_mut(&ctl).unwrap().clips.push(clip(1.0, 2.0, "r1"));
    t.duration_mode = DurationMode::FixedLength;
    t.fixed_duration = 4.0;
    let root = project(&t, &[]);
    let mut scene = Scene::new("t");
    let fx = scene.spawn("Explosion", None);
    scene.add(fx, ImpulseSource::default());
    let d = scene.spawn("Director", None);
    let mut comp = PlayableDirector {
        playable: Some("Assets/T.timeline".into()),
        wrap_mode: DirectorWrapMode::None,
        ..Default::default()
    };
    comp.set_reference("r1", Some(fx));
    scene.add(d, comp);

    // Edit-mode style evaluation: on only inside the clip.
    let src = Sources::default();
    let comp = (*scene.world.get::<&PlayableDirector>(d).unwrap()).clone();
    let mut base = Baseline::default();
    for (at, on) in [(0.5, false), (1.0, true), (2.9, true), (3.0, false)] {
        evaluate::apply(&mut scene, &t, &comp, &src, at, &mut base);
        assert_eq!(scene.game_object(fx).active, on, "t = {at}");
    }
    base.restore(&mut scene);

    // Play mode: the impulse fires once when the clip starts; Stop reverts the object.
    let mut audio = Audio::null(root.clone()).0;
    let mut p = DirectorPlayer::new(root);
    p.play(&scene, d);
    let mut impulses = 0;
    let mut played = 0;
    for i in 0..25 {
        let ev = p.update(&mut scene, &mut audio, 0.1, 0.1);
        impulses += ev
            .iter()
            .filter(|e| **e == DirectorEvent::Impulse(fx))
            .count();
        played += ev
            .iter()
            .filter(|e| **e == DirectorEvent::Played(d))
            .count();
        if i == 15 {
            assert!(scene.game_object(fx).active, "on at 1.5 s");
            p.stop(&mut scene, &mut audio, d);
        }
    }
    assert_eq!(impulses, 1);
    assert_eq!(played, 1);
    assert!(
        scene.game_object(fx).active,
        "Revert: back to active as before"
    );
}

/// A project folder with `T.timeline` and extra timelines.
fn project(asset: &TimelineAsset, extra: &[(&str, &TimelineAsset)]) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_timeline_r19_{n}"));
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    asset.save(&root.join("Assets/T.timeline")).unwrap();
    for (path, t) in extra {
        t.save(&root.join(path)).unwrap();
    }
    root
}

#[test]
fn sub_timeline_signals_fire_at_their_mapped_time_in_play_mode() {
    let mut nested = TimelineAsset::default();
    nested.markers.push(Marker {
        time: 1.0,
        method: "Boom".into(),
        ..Default::default()
    });
    nested.duration_mode = DurationMode::FixedLength;
    nested.fixed_duration = 3.0;
    let mut t = TimelineAsset::default();
    let ctl = t.add_track(TrackKind::Control, None);
    t.track_mut(&ctl).unwrap().clips.push(clip(2.0, 3.0, "r1"));
    let root = project(&t, &[("Assets/Sub.timeline", &nested)]);
    let mut scene = Scene::new("t");
    let sub = scene.spawn("Sub", None);
    scene.add(
        sub,
        PlayableDirector {
            playable: Some("Assets/Sub.timeline".into()),
            play_on_awake: false,
            ..Default::default()
        },
    );
    let d = scene.spawn("Director", None);
    let mut comp = PlayableDirector {
        playable: Some("Assets/T.timeline".into()),
        ..Default::default()
    };
    comp.set_reference("r1", Some(sub));
    scene.add(d, comp);
    let mut audio = Audio::null(root.clone()).0;
    let mut p = DirectorPlayer::new(root);
    p.play(&scene, d);
    let mut fired_at = Vec::new();
    for i in 0..60 {
        for ev in p.update(&mut scene, &mut audio, 0.1, 0.1) {
            if let DirectorEvent::Signal {
                director,
                target,
                method,
                ..
            } = ev
            {
                assert_eq!((director, target, method.as_str()), (sub, sub, "Boom"));
                fired_at.push(i);
            }
        }
    }
    // Parent 3.0 s = nested 1.0 s; update i covers time (i − 1) × 0.1 … i × 0.1.
    assert_eq!(fired_at, [30]);
}

#[test]
fn camera_track_previews_in_edit_mode_and_puts_the_camera_back() {
    let mut scene = Scene::new("t");
    let brain = scene.spawn("Main Camera", None);
    scene.add(brain, Camera::default());
    scene.add(brain, CameraBrain::default());
    scene.set_transform(brain, Transform::from_position(Vec3::new(0.0, 1.0, -10.0)));
    let a = fixed_vcam(&mut scene, "A", 1, Vec3::new(5.0, 2.0, 0.0));
    scene.world.get::<&mut VirtualCamera>(a).unwrap().lens.fov = 30.0;
    let shots = [CameraOverride {
        brain,
        from: None,
        to: Some(a),
        weight: 1.0,
    }];
    let mut base = Baseline::default();
    camera::preview(&mut scene, &shots, &mut base, 16.0 / 9.0);
    assert!((scene.world_position(brain) - Vec3::new(5.0, 2.0, 0.0)).length() < 1e-4);
    assert!(near(scene.world.get::<&Camera>(brain).unwrap().fov, 30.0));
    // No shot any more: back where it was.
    camera::preview(&mut scene, &[], &mut base, 16.0 / 9.0);
    assert!((scene.world_position(brain) - Vec3::new(0.0, 1.0, -10.0)).length() < 1e-4);
    camera::preview(&mut scene, &shots, &mut base, 16.0 / 9.0);
    base.restore(&mut scene);
    assert!((scene.world_position(brain) - Vec3::new(0.0, 1.0, -10.0)).length() < 1e-4);
    assert!(near(scene.world.get::<&Camera>(brain).unwrap().fov, 60.0));
}

#[test]
fn new_clip_fields_round_trip_and_stay_out_of_old_files() {
    let mut t = TimelineAsset::default();
    let a = t.add_track(TrackKind::Activation, None);
    t.track_mut(&a).unwrap().clips.push(TimelineClip::default());
    let json = t.to_json();
    assert!(
        !json.contains("reference") && !json.contains("control"),
        "{json}"
    );
    let c = t.add_track(TrackKind::Control, None);
    assert_eq!(t.track(&c).unwrap().name, "Control Track");
    assert_eq!(t.next_reference(), "r1");
    let mut cc = clip(0.0, 1.0, "r1");
    cc.control.particles = false;
    t.track_mut(&c).unwrap().clips.push(cc);
    assert_eq!(t.next_reference(), "r2");
    let back = TimelineAsset::from_json(&t.to_json()).unwrap();
    assert_eq!(back, t);
    assert!(!TrackKind::Control.bindable() && TrackKind::Camera.bindable());
    assert_eq!(TrackKind::Camera.binding_component(), "Camera Brain");
}
