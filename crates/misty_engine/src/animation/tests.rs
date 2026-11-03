//! Curve math, clip files, path binding and playback.

use glam::Vec3;

use super::player::{effective_wrap, wrap_time};
use super::*;
use crate::scene::{Animation, Light, Scene, WrapMode};

fn keys(pairs: &[(f32, f32)]) -> Keys {
    Keys::new(pairs.iter().map(|&(t, v)| Keyframe::new(t, v)).collect())
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn evaluation_hits_keys_and_clamps_outside() {
    let k = keys(&[(0.0, 1.0), (1.0, 3.0), (2.0, 2.0)]);
    assert!(close(k.evaluate(-1.0), 1.0));
    assert!(close(k.evaluate(0.0), 1.0));
    assert!(close(k.evaluate(1.0), 3.0));
    assert!(close(k.evaluate(2.0), 2.0));
    assert!(close(k.evaluate(5.0), 2.0));
    assert!(close(Keys::default().evaluate(1.0), 0.0));
    assert!(close(keys(&[(0.5, 7.0)]).evaluate(0.0), 7.0));
}

#[test]
fn auto_tangents_are_smooth_without_overshoot() {
    // Two keys with flat ends: a smoothstep, exactly half way at the middle.
    let k = keys(&[(0.0, 0.0), (1.0, 1.0)]);
    assert!(close(k.evaluate(0.5), 0.5));
    assert!(k.evaluate(0.25) < 0.25, "eases in");
    // A peak key is a local maximum: neighbours never go above it.
    let k = keys(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)]);
    for i in 0..=40 {
        let v = k.evaluate(i as f32 / 20.0);
        assert!(v <= 1.0 + 1e-5, "overshoot at {i}: {v}");
    }
    // A middle key on a straight rising line keeps its slope (Catmull-Rom).
    let k = keys(&[(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)]);
    assert!(close(k.out_slope(1), 1.0));
}

#[test]
fn linear_and_constant_tangents() {
    let mut k = keys(&[(0.0, 0.0), (2.0, 4.0)]);
    k.set_tangent(0, TangentMode::Linear);
    k.set_tangent(1, TangentMode::Linear);
    assert!(close(k.evaluate(0.5), 1.0));
    assert!(close(k.evaluate(1.5), 3.0));
    k.set_tangent(0, TangentMode::Constant);
    assert!(close(k.evaluate(1.99), 0.0), "holds until the next key");
    assert!(close(k.evaluate(2.0), 4.0));
}

#[test]
fn keys_stay_sorted_and_unique() {
    let mut k = keys(&[(1.0, 1.0), (0.0, 0.0), (1.0, 5.0)]);
    assert_eq!(k.len(), 2, "duplicate time dropped");
    assert_eq!(k.set(0.5, 2.0), 1);
    assert_eq!(k.set(0.5, 3.0), 1, "same time replaces");
    assert_eq!(k.len(), 3);
    assert!(close(k.get(1).unwrap().value, 3.0));
    let j = k.move_key(0, 2.0, 9.0);
    assert_eq!(j, 2);
    let times: Vec<f32> = k.as_slice().iter().map(|x| x.time).collect();
    assert_eq!(times, [0.5, 1.0, 2.0]);
    // Moving onto another key replaces it.
    k.move_key(0, 1.0, 4.0);
    assert_eq!(k.len(), 2);
    assert!(close(k.evaluate(1.0), 4.0));
}

#[test]
fn wrap_modes() {
    assert!(close(wrap_time(2.5, 1.0, WrapMode::Loop).0, 0.5));
    assert!(close(wrap_time(-0.25, 1.0, WrapMode::Loop).0, 0.75));
    assert!(close(wrap_time(1.25, 1.0, WrapMode::PingPong).0, 0.75));
    assert!(close(wrap_time(2.25, 1.0, WrapMode::PingPong).0, 0.25));
    assert_eq!(wrap_time(1.5, 1.0, WrapMode::Once), (1.0, true));
    assert_eq!(wrap_time(0.5, 1.0, WrapMode::Once), (0.5, false));
    assert_eq!(wrap_time(9.0, 1.0, WrapMode::ClampForever), (1.0, false));
    let mut clip = AnimationClip::default();
    assert_eq!(effective_wrap(WrapMode::Default, &clip), WrapMode::Loop);
    clip.looping = false;
    assert_eq!(effective_wrap(WrapMode::Default, &clip), WrapMode::Once);
    assert_eq!(
        effective_wrap(WrapMode::PingPong, &clip),
        WrapMode::PingPong
    );
}

#[test]
fn active_curve_steps() {
    let mut c = Curve::new("", Property::Active);
    c.keys = keys(&[(0.0, 1.0), (1.0, 0.0), (2.0, 1.0)]);
    assert_eq!(c.evaluate(0.99), 1.0);
    assert_eq!(c.evaluate(1.0), 0.0);
    assert_eq!(c.evaluate(1.5), 0.0);
    assert_eq!(c.evaluate(3.0), 1.0);
}

fn bob_clip() -> AnimationClip {
    let mut clip = AnimationClip {
        length: 2.0,
        ..Default::default()
    };
    clip.set_key("", Property::Position(1), 0.0, 0.0);
    clip.set_key("", Property::Position(1), 1.0, 2.0);
    clip.set_key("", Property::Position(1), 2.0, 0.0);
    clip.set_key("Lamp", Property::LightIntensity, 0.0, 1.0);
    clip.set_key("Lamp", Property::LightIntensity, 1.0, 0.0);
    clip.curves[1].keys.set_tangent(0, TangentMode::Constant);
    clip.set_key("Lamp/Bulb", Property::Rotation(1), 0.0, 0.0);
    clip.set_key("Lamp/Bulb", Property::Rotation(1), 2.0, 360.0);
    clip.events.push(AnimationEvent {
        time: 1.0,
        function: "Peak".into(),
    });
    clip
}

#[test]
fn clip_file_round_trip() {
    let clip = bob_clip();
    let json = clip.to_json();
    assert!(json.contains("\"Transform.position.y\""));
    assert!(json.contains("\"Light.intensity\""));
    assert!(json.contains("\"tangent\": \"Constant\""));
    assert_eq!(AnimationClip::from_json(&json).unwrap(), clip);

    let dir = std::env::temp_dir().join(format!("misty_anim_{}", std::process::id()));
    let path = dir.join("Bob.anim");
    clip.save(&path).unwrap();
    assert_eq!(AnimationClip::load(&path).unwrap(), clip);
    std::fs::remove_dir_all(dir).ok();

    // Hand-written files: unsorted keys are sorted, length grows to the last key.
    let text = r#"{"curves":[{"property":"Transform.scale.x","keys":[{"time":3,"value":1},{"time":1,"value":2}]}]}"#;
    let c = AnimationClip::from_json(text).unwrap();
    assert_eq!(c.curves[0].keys.get(0).unwrap().time, 1.0);
    assert_eq!(c.length, 3.0);
    assert!(AnimationClip::from_json(r#"{"curves":[{"property":"Nope","keys":[]}]}"#).is_err());
    assert!(is_clip("Assets/Bob.anim") && !is_clip("Assets/Bob.mat"));
}

#[test]
fn property_names_round_trip() {
    for p in Property::ALL {
        assert_eq!(Property::from_name(&p.name()), Some(p), "{}", p.name());
    }
    assert_eq!(Property::Rotation(2).label(), "Rotation.z");
    assert_eq!(Property::LightColor(0).siblings().len(), 3);
}

/// Root "Bobber" with child "Lamp" (Light) and grandchild "Bulb".
fn rig() -> (Scene, hecs::Entity, hecs::Entity, hecs::Entity) {
    let mut s = Scene::new("t");
    let root = s.spawn("Bobber", None);
    let lamp = s.spawn("Lamp", Some(root));
    s.add(lamp, Light::default());
    let bulb = s.spawn("Bulb", Some(lamp));
    (s, root, lamp, bulb)
}

#[test]
fn path_binding_resolves_children() {
    let (mut s, root, lamp, bulb) = rig();
    assert_eq!(binding::resolve(&s, root, ""), Some(root));
    assert_eq!(binding::resolve(&s, root, "Lamp/Bulb"), Some(bulb));
    assert_eq!(binding::resolve(&s, root, "Missing"), None);
    assert_eq!(
        binding::path_of(&s, root, bulb).as_deref(),
        Some("Lamp/Bulb")
    );
    assert_eq!(binding::path_of(&s, lamp, root), None);

    binding::sample(&mut s, root, &bob_clip(), 0.5);
    assert!(close(s.transform(root).position.y, 1.0));
    assert_eq!(s.world.get::<&Light>(lamp).unwrap().intensity, 1.0, "step");
    assert!(close(
        s.transform(bulb).euler().y,
        360.0 * (3.0 * 0.0625 - 2.0 * 0.015625)
    ));
    binding::sample(&mut s, root, &bob_clip(), 1.0);
    assert!(close(s.transform(root).position.y, 2.0));
    assert_eq!(binding::read(&s, lamp, Property::LightIntensity), Some(0.0));
    assert_eq!(binding::read(&s, root, Property::LightIntensity), None);

    // Renaming the child breaks its binding; other curves still apply.
    s.game_object_mut(lamp).name = "Other".into();
    binding::sample(&mut s, root, &bob_clip(), 0.0);
    assert_eq!(s.transform(root).position, Vec3::ZERO);
    assert_eq!(s.world.get::<&Light>(lamp).unwrap().intensity, 0.0);
}

fn player_with(clip: &AnimationClip, anim: Animation) -> (AnimationPlayer, Scene, hecs::Entity) {
    let dir = std::env::temp_dir().join(format!(
        "misty_anim_player_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    clip.save(&dir.join("Assets/Bob.anim")).unwrap();
    let (mut s, root, _, _) = rig();
    s.add(root, anim);
    (AnimationPlayer::new(dir), s, root)
}

#[test]
fn player_loops_fires_events_and_stops_once() {
    let anim = Animation {
        clip: Some("Assets/Bob.anim".into()),
        ..Default::default()
    };
    let (mut p, mut s, root) = player_with(&bob_clip(), anim);
    p.play_automatically(&s, None);
    assert!(p.is_playing(root));
    p.update(&mut s, 0.1);
    assert_eq!(p.time(root), 0.0, "first frame samples the start");
    let mut fired = 0;
    for _ in 0..45 {
        fired += p.update(&mut s, 0.1).len();
    }
    assert_eq!(fired, 2, "event at 1 s fires once per loop (t = 1, 3)");
    assert!(close(p.time(root), 4.5));
    assert!(close(s.transform(root).position.y, 1.0));

    s.world.get::<&mut Animation>(root).unwrap().wrap_mode = WrapMode::Once;
    p.set_speed(root, 2.0);
    assert!(p.play(&s, root, Some("Bob")));
    p.update(&mut s, 0.0);
    for _ in 0..20 {
        p.update(&mut s, 0.1);
    }
    assert!(!p.is_playing(root), "Once stops at the end");
    assert!((2.0..2.3).contains(&p.time(root)), "stopped at the end");
    assert!(close(s.transform(root).position.y, 0.0));
    assert!(!p.play(&s, root, Some("Missing")));
    assert_eq!(p.take_logs().len(), 1);
}
