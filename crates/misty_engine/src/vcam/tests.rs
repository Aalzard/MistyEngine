use glam::{Quat, Vec3};
use hecs::Entity;

use super::*;
use crate::camera::{Camera as ViewCamera, Projection};
use crate::physics::Physics;
use crate::scene::{BoxCollider, Camera, Scene, Transform};

const DT: f32 = 1.0 / 60.0;

fn no_axis(_: &str) -> f32 {
    0.0
}

fn step(rig: &mut CameraRig, s: &mut Scene, physics: Option<&Physics>, dt: f32) {
    let cx = SolveContext {
        dt,
        aspect: 16.0 / 9.0,
        physics,
        axis: &no_axis,
        action: &solve::no_action,
    };
    rig.update(s, BrainUpdate::LateUpdate, &cx);
}

fn brain_scene(blend: BlendDefinition) -> (Scene, Entity) {
    let mut s = Scene::new("t");
    let cam = s.spawn("Main Camera", None);
    s.add(cam, Camera::default());
    s.add(
        cam,
        CameraBrain {
            default_blend: blend,
            ..Default::default()
        },
    );
    (s, cam)
}

/// A camera that stays where it is put.
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

fn set_priority(s: &mut Scene, e: Entity, p: i32) {
    s.world.get::<&mut VirtualCamera>(e).unwrap().priority = p;
}

#[test]
fn highest_priority_wins_and_ties_go_to_the_latest_enabled() {
    let (mut s, brain) = brain_scene(BlendDefinition {
        style: BlendStyle::Cut,
        duration: 0.0,
        ..Default::default()
    });
    let low = fixed_vcam(&mut s, "Low", 5, Vec3::X);
    let a = fixed_vcam(&mut s, "A", 20, Vec3::Y);
    let b = fixed_vcam(&mut s, "B", 20, Vec3::Z);
    let mut rig = CameraRig::default();
    step(&mut rig, &mut s, None, DT);
    // Same priority: B comes later in the Hierarchy, so it counts as enabled last.
    assert_eq!(rig.live_camera(brain), Some(b));
    assert!((s.world_position(brain) - Vec3::Z).length() < 1e-5, "cut");
    // Re-enabling A makes it the most recent.
    s.world.get::<&mut VirtualCamera>(a).unwrap().enabled = false;
    step(&mut rig, &mut s, None, DT);
    assert_eq!(rig.live_camera(brain), Some(b));
    s.world.get::<&mut VirtualCamera>(a).unwrap().enabled = true;
    step(&mut rig, &mut s, None, DT);
    assert_eq!(rig.live_camera(brain), Some(a));
    // Raising a priority beats recency; an inactive object never wins.
    set_priority(&mut s, low, 30);
    step(&mut rig, &mut s, None, DT);
    assert_eq!(rig.live_camera(brain), Some(low));
    s.game_object_mut(low).active = false;
    step(&mut rig, &mut s, None, DT);
    assert_eq!(rig.live_camera(brain), Some(a));
    assert!(rig.is_live(a) && !rig.is_live(low));
}

#[test]
fn blend_weights_follow_the_curve_over_time() {
    let (mut s, brain) = brain_scene(BlendDefinition {
        style: BlendStyle::Linear,
        duration: 1.0,
        ..Default::default()
    });
    let a = fixed_vcam(&mut s, "A", 20, Vec3::ZERO);
    let b = fixed_vcam(&mut s, "B", 10, Vec3::new(10.0, 0.0, 0.0));
    let mut rig = CameraRig::default();
    step(&mut rig, &mut s, None, 0.25);
    assert_eq!(rig.live_camera(brain), Some(a));
    assert!(
        !rig.is_blending(brain),
        "the first camera is not blended in"
    );
    set_priority(&mut s, b, 30);
    for k in 1..=3 {
        step(&mut rig, &mut s, None, 0.25);
        let w = rig.blend_weight(brain).unwrap();
        assert!((w - 0.25 * k as f32).abs() < 1e-4, "{k}: {w}");
        let x = s.world_position(brain).x;
        assert!(
            (x - 10.0 * w).abs() < 1e-3,
            "position follows the weight: {x}"
        );
    }
    step(&mut rig, &mut s, None, 0.25);
    assert!(!rig.is_blending(brain));
    assert!((s.world_position(brain).x - 10.0).abs() < 1e-4);

    // Ease in-out is slow at both ends and half way at the middle.
    let e = BlendStyle::EaseInOut;
    assert!((e.ease(0.5) - 0.5).abs() < 1e-6);
    assert!(e.ease(0.1) < 0.1 && e.ease(0.9) > 0.9);
    assert_eq!(BlendStyle::Cut.ease(0.0), 1.0);
}

#[test]
fn custom_blends_match_pairs_before_any() {
    let brain = CameraBrain {
        custom_blends: vec![
            CustomBlend {
                from: String::new(),
                to: "Close".into(),
                blend: BlendDefinition {
                    style: BlendStyle::Cut,
                    duration: 0.0,
                    ..Default::default()
                },
            },
            CustomBlend {
                from: "Wide".into(),
                to: "Close".into(),
                blend: BlendDefinition {
                    style: BlendStyle::Linear,
                    duration: 3.0,
                    ..Default::default()
                },
            },
        ],
        ..Default::default()
    };
    assert_eq!(brain.blend_for("Wide", "Close").duration, 3.0);
    assert!(brain.blend_for("Follow", "Close").is_cut());
    assert_eq!(brain.blend_for("Close", "Wide"), brain.default_blend);
}

#[test]
fn a_blend_interrupted_half_way_starts_from_where_the_camera_was() {
    let (mut s, brain) = brain_scene(BlendDefinition {
        style: BlendStyle::Linear,
        duration: 1.0,
        ..Default::default()
    });
    let a = fixed_vcam(&mut s, "A", 20, Vec3::ZERO);
    let b = fixed_vcam(&mut s, "B", 10, Vec3::new(10.0, 0.0, 0.0));
    let mut rig = CameraRig::default();
    step(&mut rig, &mut s, None, DT);
    set_priority(&mut s, b, 30);
    step(&mut rig, &mut s, None, 0.5);
    let mid = s.world_position(brain).x;
    assert!((mid - 5.0).abs() < 1e-3);
    // Back to A: no jump, the blend continues from x = 5.
    set_priority(&mut s, a, 40);
    step(&mut rig, &mut s, None, 1e-4);
    assert!((s.world_position(brain).x - mid).abs() < 0.01);
    step(&mut rig, &mut s, None, 0.5);
    assert!((s.world_position(brain).x - 2.5).abs() < 0.01);
}

#[test]
fn transposer_damping_converges_on_the_offset() {
    let mut s = Scene::new("t");
    let target = s.spawn("Target", None);
    let vcam = s.spawn("Follow", None);
    s.add(
        vcam,
        VirtualCamera {
            follow: Some(target),
            aim: AimKind::DoNothing,
            transposer: Transposer {
                binding: BindingMode::World,
                offset: Vec3::new(0.0, 2.0, -6.0),
                damping: Vec3::splat(1.0),
            },
            ..Default::default()
        },
    );
    let mut rig = CameraRig::default();
    step(&mut rig, &mut s, None, DT);
    assert!((s.world_position(vcam) - Vec3::new(0.0, 2.0, -6.0)).length() < 1e-4);
    // The target jumps; the camera lags, then closes the gap steadily.
    s.set_world_position(target, Vec3::new(10.0, 0.0, 0.0));
    let desired = Vec3::new(10.0, 2.0, -6.0);
    let mut last = f32::MAX;
    for _ in 0..60 {
        step(&mut rig, &mut s, None, DT);
        let gap = s.world_position(vcam).distance(desired);
        assert!(gap < last, "gap shrinks: {gap}");
        last = gap;
    }
    assert!(last < 0.1 + 1e-3, "≈99 % after the damping time: {last}");
    for _ in 0..120 {
        step(&mut rig, &mut s, None, DT);
    }
    assert!(s.world_position(vcam).distance(desired) < 1e-3);
}

/// Where `p` shows in virtual camera `e`'s last pose (0..1, y down).
fn screen_pos(rig: &CameraRig, e: Entity, p: Vec3) -> glam::Vec2 {
    let out = rig.camera_output(e).unwrap();
    let cam = ViewCamera {
        position: out.position,
        rotation: out.rotation,
        projection: Projection::Perspective { fov_y: out.fov },
        near: out.near,
        far: out.far,
    };
    cam.world_to_screen(16.0 / 9.0, p).unwrap().0
}

#[test]
fn composer_keeps_the_target_inside_the_soft_zone() {
    let mut s = Scene::new("t");
    let target = s.spawn("Target", None);
    s.set_world_position(target, Vec3::new(0.0, 1.0, 10.0));
    let vcam = s.spawn("Aim", None);
    let composer = Composer {
        dead_zone_width: 0.1,
        dead_zone_height: 0.1,
        soft_zone_width: 0.5,
        soft_zone_height: 0.5,
        horizontal_damping: 2.0,
        vertical_damping: 2.0,
        ..Default::default()
    };
    s.add(
        vcam,
        VirtualCamera {
            look_at: Some(target),
            body: BodyKind::DoNothing,
            aim: AimKind::Composer,
            composer,
            ..Default::default()
        },
    );
    let mut rig = CameraRig::default();
    step(&mut rig, &mut s, None, DT);
    let uv = screen_pos(&rig, vcam, s.world_position(target));
    assert!(
        (uv - glam::vec2(0.5, 0.5)).length() < 1e-3,
        "starts centred: {uv}"
    );

    // The target runs sideways and up much faster than the heavy damping can turn.
    let (_, soft) = composer.zones();
    let mut left_dead_zone = false;
    for i in 1..=90 {
        let p = Vec3::new(i as f32 * 0.25, 1.0 + i as f32 * 0.05, 10.0);
        s.set_world_position(target, p);
        step(&mut rig, &mut s, None, DT);
        let uv = screen_pos(&rig, vcam, p);
        let eps = 0.01;
        assert!(
            uv.x >= soft[0] - eps
                && uv.x <= soft[2] + eps
                && uv.y >= soft[1] - eps
                && uv.y <= soft[3] + eps,
            "frame {i}: {uv} outside {soft:?}"
        );
        left_dead_zone |= uv.x > 0.56;
    }
    assert!(
        left_dead_zone,
        "damping lets the target drift into the soft zone"
    );
    // Once it stops, the camera settles with the target back at the dead zone's edge.
    for _ in 0..600 {
        step(&mut rig, &mut s, None, DT);
    }
    let uv = screen_pos(&rig, vcam, s.world_position(target));
    assert!(
        (uv.x - 0.5).abs() <= 0.06 && (uv.y - 0.5).abs() <= 0.06,
        "{uv}"
    );
}

#[test]
fn hard_look_at_centres_the_target() {
    let mut s = Scene::new("t");
    let target = s.spawn("Target", None);
    s.set_world_position(target, Vec3::new(3.0, -2.0, 7.0));
    let vcam = s.spawn("Look", None);
    s.add(
        vcam,
        VirtualCamera {
            look_at: Some(target),
            body: BodyKind::DoNothing,
            aim: AimKind::HardLookAt,
            ..Default::default()
        },
    );
    let mut rig = CameraRig::default();
    step(&mut rig, &mut s, None, DT);
    let fwd = s.world_rotation(vcam) * Vec3::Z;
    assert!((fwd - Vec3::new(3.0, -2.0, 7.0).normalize()).length() < 1e-4);
}

#[test]
fn collider_pulls_the_camera_in_front_of_a_wall() {
    let mut s = Scene::new("t");
    let target = s.spawn("Target", None);
    s.set_world_position(target, Vec3::new(0.0, 1.0, 0.0));
    // A wall between the target and where the camera wants to be (z = -8).
    let wall = s.spawn("Wall", None);
    s.set_transform(
        wall,
        Transform {
            position: Vec3::new(0.0, 1.0, -4.0),
            scale: Vec3::new(10.0, 10.0, 0.5),
            ..Default::default()
        },
    );
    s.add(wall, BoxCollider::default());
    let vcam = s.spawn("Follow", None);
    let vc = VirtualCamera {
        follow: Some(target),
        look_at: Some(target),
        aim: AimKind::HardLookAt,
        transposer: Transposer {
            binding: BindingMode::World,
            offset: Vec3::new(0.0, 0.0, -8.0),
            damping: Vec3::ZERO,
        },
        collider: CameraCollider {
            enabled: true,
            camera_radius: 0.25,
            damping: 0.0,
            ..Default::default()
        },
        ..Default::default()
    };
    s.add(vcam, vc);
    let physics = Physics::build(&s, Vec3::ZERO);
    let mut rig = CameraRig::default();
    step(&mut rig, &mut s, Some(&physics), DT);
    let out = rig.camera_output(vcam).unwrap();
    // The wall's front face is at z = -3.75; the camera stays on the target's side.
    assert!(out.position.z > -3.75, "{}", out.position);
    assert!((out.position.z - (-3.5)).abs() < 0.05, "{}", out.position);
    // The object keeps the unobstructed pose.
    assert!((s.world_position(vcam).z + 8.0).abs() < 1e-4);

    // Without the wall it goes back out.
    let open = Physics::build(&Scene::new("empty"), Vec3::ZERO);
    step(&mut rig, &mut s, Some(&open), DT);
    assert!((rig.camera_output(vcam).unwrap().position.z + 8.0).abs() < 1e-4);
}

#[test]
fn noise_is_deterministic_per_seed() {
    let n = |seed| Noise {
        enabled: true,
        profile: NoiseProfile::Shake,
        seed,
        ..Default::default()
    };
    let a: Vec<_> = (0..50)
        .map(|i| noise::sample(&n(1), i as f32 * 0.037))
        .collect();
    let b: Vec<_> = (0..50)
        .map(|i| noise::sample(&n(1), i as f32 * 0.037))
        .collect();
    let c: Vec<_> = (0..50)
        .map(|i| noise::sample(&n(2), i as f32 * 0.037))
        .collect();
    assert_eq!(a, b);
    assert_ne!(a, c);
    let biggest = a
        .iter()
        .map(|(_, r)| r.abs().max_element())
        .fold(0.0, f32::max);
    assert!(
        biggest > 0.3 && biggest < 5.0,
        "shake is visible but bounded: {biggest}"
    );
    // Handheld is gentler than Shake.
    let hand = Noise {
        profile: NoiseProfile::Handheld,
        ..n(1)
    };
    let gentle = (0..50)
        .map(|i| noise::sample(&hand, i as f32 * 0.037).1.abs().max_element())
        .fold(0.0, f32::max);
    assert!(gentle < biggest);
    // Amplitude gain 0 switches it off.
    let off = Noise {
        amplitude_gain: 0.0,
        ..n(1)
    };
    assert_eq!(noise::sample(&off, 1.3), (Vec3::ZERO, Vec3::ZERO));
}

#[test]
fn third_person_sits_behind_the_shoulder() {
    let mut s = Scene::new("t");
    let target = s.spawn("Hero", None);
    s.set_transform(
        target,
        Transform {
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            ..Default::default()
        },
    );
    let vcam = s.spawn("Shoulder", None);
    s.add(
        vcam,
        VirtualCamera {
            follow: Some(target),
            body: BodyKind::ThirdPerson,
            aim: AimKind::DoNothing,
            ..Default::default()
        },
    );
    let mut rig = CameraRig::default();
    step(&mut rig, &mut s, None, DT);
    // Facing +X: right shoulder is −Z, 4 m behind is −X.
    let p = s.world_position(vcam);
    assert!((p - Vec3::new(-4.0, 1.9, -0.5)).length() < 1e-3, "{p}");
    let fwd = s.world_rotation(vcam) * Vec3::Z;
    assert!((fwd - Vec3::X).length() < 1e-4);
}

#[test]
fn orbital_turns_with_its_input_axis() {
    let mut s = Scene::new("t");
    let target = s.spawn("Hero", None);
    let vcam = s.spawn("Free Look", None);
    let mut vc = VirtualCamera {
        body: BodyKind::Orbital,
        ..Default::default()
    };
    vc.follow = Some(target);
    vc.look_at = Some(target);
    vc.orbital.y_axis.value = 0.0;
    vc.orbital.height_offset = 0.0;
    vc.composer.dead_zone_width = 0.0;
    vc.composer.dead_zone_height = 0.0;
    s.add(vcam, vc);
    let mut rig = CameraRig::default();
    let push = |_: &str| 0.5;
    let cx = SolveContext {
        dt: 0.1,
        aspect: 1.0,
        physics: None,
        axis: &push,
        action: &solve::no_action,
    };
    rig.update(&mut s, BrainUpdate::LateUpdate, &cx);
    let v = s.world.get::<&VirtualCamera>(vcam).unwrap().orbital.clone();
    // 0.5 × 300 °/s × 0.1 s = 15 ° of yaw; pitch stays clamped in range.
    assert!((v.x_axis.value - 15.0).abs() < 1e-3, "{}", v.x_axis.value);
    assert!(v.y_axis.value >= v.y_axis.min && v.y_axis.value <= v.y_axis.max);
    let p = s.world_position(vcam);
    assert!((p.length() - 6.0).abs() < 0.05, "stays on the orbit: {p}");
}

#[test]
fn virtual_cameras_round_trip_through_scene_files() {
    let mut s = Scene::new("t");
    let hero = s.spawn("Hero", None);
    let cam = s.spawn("Main Camera", None);
    s.add(cam, Camera::default());
    s.add(
        cam,
        CameraBrain {
            update_method: BrainUpdate::FixedUpdate,
            custom_blends: vec![CustomBlend {
                from: "Wide".into(),
                to: String::new(),
                blend: BlendDefinition {
                    style: BlendStyle::Linear,
                    duration: 0.7,
                    ..Default::default()
                },
            }],
            ..Default::default()
        },
    );
    let vcam = s.spawn("Follow", None);
    let vc = VirtualCamera {
        priority: 15,
        follow: Some(hero),
        look_at: Some(hero),
        body: BodyKind::ThirdPerson,
        noise: Noise {
            enabled: true,
            profile: NoiseProfile::Shake,
            seed: 7,
            ..Default::default()
        },
        collider: CameraCollider {
            enabled: true,
            ignore_tag: "Player".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    s.add(vcam, vc.clone());
    let json = s.to_json();
    let (back, report) = Scene::from_json("t", &json).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let hero2 = back.find_by_name("Hero").unwrap();
    let v2 = (*back
        .world
        .get::<&VirtualCamera>(back.find_by_name("Follow").unwrap())
        .unwrap())
    .clone();
    assert_eq!(v2.follow, Some(hero2));
    assert_eq!(v2.look_at, Some(hero2));
    assert_eq!(
        VirtualCamera {
            follow: None,
            look_at: None,
            ..v2
        },
        VirtualCamera {
            follow: None,
            look_at: None,
            ..vc
        }
    );
    let b2 = (*back
        .world
        .get::<&CameraBrain>(back.find_by_name("Main Camera").unwrap())
        .unwrap())
    .clone();
    assert_eq!(b2.update_method, BrainUpdate::FixedUpdate);
    assert_eq!(b2.custom_blends.len(), 1);
    assert_eq!(back.to_json(), json);
    // Old files and empty objects load with defaults.
    let d: VirtualCamera = serde_json::from_str("{}").unwrap();
    assert_eq!(d, VirtualCamera::default());
}
