//! Round 19 virtual cameras: Free Look rings, Input System axes, impulses, group framing,
//! the confiner and custom blend curves / blend list assets.

use glam::{Vec2, Vec3};
use hecs::Entity;

use super::impulse::{ImpulseEvent, envelope, falloff};
use super::*;
use crate::scene::{BoxCollider, Camera, Scene, Transform};

const DT: f32 = 1.0 / 60.0;

fn step(rig: &mut CameraRig, s: &mut Scene, dt: f32, aspect: f32) {
    let cx = SolveContext {
        dt,
        aspect,
        physics: None,
        axis: &solve::no_axis,
        action: &solve::no_action,
    };
    rig.update(s, BrainUpdate::LateUpdate, &cx);
}

fn brain(s: &mut Scene) -> Entity {
    let cam = s.spawn("Main Camera", None);
    s.add(cam, Camera::default());
    s.add(cam, CameraBrain::default());
    cam
}

#[test]
fn free_look_rings_interpolate_between_bottom_middle_and_top() {
    let mut o = Orbital::three_rings();
    o.orbits = [
        Orbit {
            height: 5.0,
            radius: 2.0,
        },
        Orbit {
            height: 2.0,
            radius: 4.0,
        },
        Orbit {
            height: 0.0,
            radius: 1.0,
        },
    ];
    for k in [0.0, 0.2, 1.0] {
        o.spline_curvature = k;
        let at = |y: f32| o.ring_at(y);
        assert_eq!(at(0.0), (0.0, 1.0), "bottom ring");
        assert_eq!(at(0.5), (2.0, 4.0), "middle ring");
        assert_eq!(at(1.0), (5.0, 2.0), "top ring");
        // Height rises steadily from bottom to top.
        let hs: Vec<f32> = (0..=20).map(|i| at(i as f32 / 20.0).0).collect();
        assert!(hs.windows(2).all(|w| w[1] >= w[0] - 1e-4), "{hs:?}");
    }
    o.spline_curvature = 0.0;
    assert_eq!(
        o.ring_at(0.25),
        (1.0, 2.5),
        "straight lines without curvature"
    );
    o.spline_curvature = 1.0;
    let (_, r) = o.ring_at(0.25);
    assert!(
        r > 2.5,
        "the curve bulges out towards the wide middle ring: {r}"
    );

    // Solved: the camera sits on the ring for its axis value and looks at the target.
    let mut s = Scene::new("t");
    let hero = s.spawn("Hero", None);
    s.set_transform(hero, Transform::from_position(Vec3::new(1.0, 0.0, 2.0)));
    let cam = s.spawn("Free Look", None);
    let mut vc = VirtualCamera::free_look();
    vc.follow = Some(hero);
    vc.look_at = Some(hero);
    vc.aim = AimKind::HardLookAt;
    vc.orbital.orbits = o.orbits;
    vc.orbital.spline_curvature = 0.0;
    vc.orbital.y_axis.value = 1.0;
    vc.orbital.x_axis.value = 90.0;
    vc.orbital.x_axis.input_axis.clear();
    vc.orbital.y_axis.input_axis.clear();
    s.add(cam, vc);
    let mut rig = CameraRig::default();
    step(&mut rig, &mut s, DT, 1.0);
    let p = s.world_position(cam);
    // Yaw 90°: the camera is on the −X side of the target, 2 m out, 5 m up.
    assert!((p - Vec3::new(-1.0, 5.0, 2.0)).length() < 1e-3, "{p}");
    let fwd = s.world_rotation(cam) * Vec3::Z;
    assert!(fwd.dot((Vec3::new(1.0, 0.0, 2.0) - p).normalize()) > 0.999);
}

#[test]
fn free_look_axes_follow_an_input_system_action() {
    let mut s = Scene::new("t");
    let hero = s.spawn("Hero", None);
    let cam = s.spawn("Free Look", None);
    let mut vc = VirtualCamera::free_look();
    vc.follow = Some(hero);
    vc.orbital.x_axis.input_action = "Player/Look".into();
    vc.orbital.y_axis.input_action = "Player/Look".into();
    s.add(cam, vc);
    let mut rig = CameraRig::default();
    let look = |name: &str| {
        if name == "Player/Look" {
            Vec2::new(0.5, -0.25)
        } else {
            Vec2::ZERO
        }
    };
    let cx = SolveContext {
        dt: 0.1,
        aspect: 1.0,
        physics: None,
        axis: &|_| 9.0,
        action: &look,
    };
    rig.update(&mut s, BrainUpdate::LateUpdate, &cx);
    let o = s.world.get::<&VirtualCamera>(cam).unwrap().orbital.clone();
    // x: 0.5 × 300 °/s × 0.1 s; y (inverted): 0.5 + 0.25 × 2 /s × 0.1 s. The action wins over
    // the Input Manager axis.
    assert!((o.x_axis.value - 15.0).abs() < 1e-3, "{}", o.x_axis.value);
    assert!((o.y_axis.value - 0.55).abs() < 1e-4, "{}", o.y_axis.value);
}

#[test]
fn impulses_rise_hold_and_decay_to_nothing() {
    // Envelope: smooth rise, flat sustain, smooth fall to exactly zero.
    assert_eq!(envelope(0.1, 0.2, 0.5, -0.01), 0.0);
    assert!((envelope(0.1, 0.2, 0.5, 0.05) - 0.5).abs() < 1e-4);
    assert_eq!(envelope(0.1, 0.2, 0.5, 0.2), 1.0);
    let decay: Vec<f32> = (0..=10)
        .map(|i| envelope(0.1, 0.2, 0.5, 0.3 + i as f32 * 0.05))
        .collect();
    assert!(decay.windows(2).all(|w| w[1] <= w[0]), "{decay:?}");
    assert_eq!(envelope(0.1, 0.2, 0.5, 0.8), 0.0);
    // Distance falloff.
    assert_eq!(falloff(3.0, 5.0, 10.0), 1.0);
    assert!((falloff(10.0, 5.0, 10.0) - 0.5).abs() < 1e-4);
    assert_eq!(falloff(15.0, 5.0, 10.0), 0.0);

    let src = ImpulseSource {
        shape: ImpulseShape::Bump,
        default_velocity: Vec3::new(0.0, -1.0, 0.0),
        attack: 0.1,
        sustain: 0.2,
        decay: 0.5,
        impact_radius: 5.0,
        dissipation_distance: 10.0,
        ..Default::default()
    };
    let mut s = Scene::new("t");
    let boom = s.spawn("Boom", None);
    s.add(boom, src.clone());
    let mut m = ImpulseManager::default();
    assert!(m.generate(&s, boom, None, 2.0));
    let near = ImpulseListener {
        enabled: true,
        ..Default::default()
    };
    m.advance(0.2);
    let (p, _) = m.offset(&near, Vec3::new(1.0, 0.0, 0.0));
    assert!(
        (p - Vec3::new(0.0, -2.0, 0.0)).length() < 1e-4,
        "full push: {p}"
    );
    let (far, _) = m.offset(&near, Vec3::new(0.0, 0.0, 30.0));
    assert_eq!(far, Vec3::ZERO, "out of range");
    let deaf = ImpulseListener {
        channel_mask: 2,
        ..near
    };
    assert_eq!(m.offset(&deaf, Vec3::ZERO).0, Vec3::ZERO, "other channel");
    let half = ImpulseListener { gain: 0.5, ..near };
    assert!((m.offset(&half, Vec3::ZERO).0.y + 1.0).abs() < 1e-4, "gain");
    m.advance(0.4);
    let (p2, _) = m.offset(&near, Vec3::ZERO);
    assert!(p2.length() < 2.0 && p2.length() > 0.0, "decaying: {p2}");
    m.advance(0.3);
    assert!(m.events.is_empty(), "forgotten once it has died away");
    assert_eq!(m.offset(&near, Vec3::ZERO).0, Vec3::ZERO);

    // The other shapes are deterministic and also fade to nothing.
    for shape in [ImpulseShape::Explosion, ImpulseShape::Rumble] {
        let ev = ImpulseEvent::from_source(
            &ImpulseSource {
                shape,
                ..src.clone()
            },
            Vec3::ZERO,
            Vec3::Y,
            0.0,
            7,
        );
        let a = ev.signal(Vec3::ZERO, false, 0.25);
        assert_eq!(a, ev.signal(Vec3::ZERO, false, 0.25));
        assert!(a.0.length() > 0.0, "{shape:?} moves");
        assert_eq!(ev.signal(Vec3::ZERO, false, 0.9).0, Vec3::ZERO);
    }
}

#[test]
fn listeners_shake_the_brain_camera_while_playing() {
    let mut s = Scene::new("t");
    let cam = brain(&mut s);
    let vcam = s.spawn("Shot", None);
    s.add(
        vcam,
        VirtualCamera {
            body: BodyKind::DoNothing,
            aim: AimKind::DoNothing,
            impulse: ImpulseListener {
                enabled: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let boom = s.spawn("Boom", None);
    s.add(boom, ImpulseSource::default());
    let mut rig = CameraRig::default();
    step(&mut rig, &mut s, DT, 1.0);
    assert!(s.world_position(cam).length() < 1e-5);
    rig.impulses.generate(&s, boom, None, 1.0);
    let mut moved = 0.0f32;
    for _ in 0..12 {
        step(&mut rig, &mut s, DT, 1.0);
        moved = moved.max(s.world_position(cam).length());
    }
    assert!(moved > 0.2, "shook by {moved}");
    for _ in 0..120 {
        step(&mut rig, &mut s, DT, 1.0);
    }
    assert!(s.world_position(cam).length() < 1e-5, "settled");
    assert!(
        s.world_position(vcam).length() < 1e-5,
        "the vcam itself never moves"
    );
}

/// Screen position (0..1, y down) of `p` and the angular radius in screen fractions of a
/// sphere of radius `r` there, for a camera pose.
fn on_screen(pose: &CamState, aspect: f32, p: Vec3, r: f32) -> (f32, f32, f32, f32) {
    let local = pose.rotation.inverse() * (p - pose.position);
    let tv = (pose.fov.to_radians() * 0.5).tan();
    let th = tv * aspect;
    let x = 0.5 + 0.5 * local.x / (local.z * th);
    let y = 0.5 - 0.5 * local.y / (local.z * tv);
    let ang = (r / local.z).atan().tan();
    (x, y, 0.5 * ang / th, 0.5 * ang / tv)
}

#[test]
fn group_framing_keeps_every_member_in_view() {
    let mut s = Scene::new("t");
    let cam = brain(&mut s);
    let names = ["Hero", "Ally", "Rival"];
    let members: Vec<Entity> = names.iter().map(|n| s.spawn(n, None)).collect();
    let starts = [
        Vec3::new(-1.0, 1.0, 0.0),
        Vec3::new(1.0, 1.0, 0.5),
        Vec3::new(0.0, 1.0, 1.5),
    ];
    for (e, p) in members.iter().zip(starts) {
        s.set_transform(*e, Transform::from_position(p));
    }
    let group = s.spawn("Trio", None);
    s.add(
        group,
        TargetGroup {
            members: members
                .iter()
                .map(|m| GroupMember {
                    target: Some(*m),
                    weight: 1.0,
                    radius: 0.6,
                })
                .collect(),
            ..Default::default()
        },
    );
    let vcam = s.spawn("Group Cam", None);
    let mut vc = VirtualCamera::group_framing();
    vc.follow = Some(group);
    vc.look_at = Some(group);
    vc.transposer.offset = Vec3::new(0.0, 3.0, -6.0);
    vc.transposer.damping = Vec3::ZERO;
    vc.group_framing.damping = 0.3;
    s.add(vcam, vc);
    let aspect = 16.0 / 9.0;
    let mut rig = CameraRig::default();
    let mut checked = 0;
    for frame in 0..240 {
        // The three spread apart, then wander.
        let t = frame as f32 * DT;
        let spread = 1.0 + t * 2.0;
        for (i, e) in members.iter().enumerate() {
            let p = starts[i] * Vec3::new(spread, 1.0, spread) + Vec3::new(t, 0.0, 0.0);
            s.set_transform(*e, Transform::from_position(p));
        }
        step(&mut rig, &mut s, DT, aspect);
        if frame < 60 {
            continue; // let the damping settle
        }
        let pose = rig.brain_output(cam).unwrap();
        for e in &members {
            let (x, y, rx, ry) = on_screen(&pose, aspect, s.world_position(*e), 0.6);
            assert!(
                x - rx > 0.0 && x + rx < 1.0 && y - ry > 0.0 && y + ry < 1.0,
                "frame {frame}: {} at ({x}, {y}) ± ({rx}, {ry})",
                s.name(*e)
            );
        }
        checked += 1;
    }
    assert_eq!(checked, 180);
    // The group object follows the members' centre.
    let b = group::group_bounds(&s, group).unwrap();
    assert!((s.world_position(group) - b.center).length() < 0.2);
    // Zoom only: the field of view widens instead.
    let mut z = s.world.get::<&mut VirtualCamera>(vcam).unwrap();
    z.group_framing.adjustment = GroupAdjustment::ZoomOnly;
    z.group_framing.damping = 0.0;
    drop(z);
    step(&mut rig, &mut s, DT, aspect);
    let pose = rig.brain_output(cam).unwrap();
    assert!(pose.fov > 3.0 && pose.fov <= 60.0);
    let d = pose.position.distance(b.center);
    assert!(
        (d - Vec3::new(0.0, 3.0, -6.0).length()).abs() < 0.5,
        "no dolly: {d}"
    );
}

#[test]
fn group_bounds_weigh_and_pad_members() {
    let mut s = Scene::new("t");
    let a = s.spawn("A", None);
    let b = s.spawn("B", None);
    s.set_transform(a, Transform::from_position(Vec3::new(-2.0, 0.0, 0.0)));
    s.set_transform(b, Transform::from_position(Vec3::new(2.0, 0.0, 0.0)));
    let g = s.spawn("G", None);
    let group = |wb: f32| TargetGroup {
        position_mode: GroupPositionMode::GroupAverage,
        members: vec![
            GroupMember {
                target: Some(a),
                weight: 1.0,
                radius: 1.0,
            },
            GroupMember {
                target: Some(b),
                weight: wb,
                radius: 1.0,
            },
        ],
    };
    let full = group(1.0).bounds(&s, g).unwrap();
    assert!(full.center.length() < 1e-5);
    assert!((full.radius - 3.0).abs() < 1e-4);
    let light = group(0.5).bounds(&s, g).unwrap();
    assert!(light.average.x < 0.0, "pulled towards the heavier member");
    assert!(light.radius < full.radius);
    assert!(
        group(0.0).bounds(&s, g).unwrap().radius <= 1.0 + 1e-4,
        "weight 0 ignored"
    );
    s.game_object_mut(a).active = false;
    assert!(group(0.0).bounds(&s, g).is_none(), "nobody left");
}

#[test]
fn the_confiner_keeps_the_camera_inside_its_volume() {
    let mut s = Scene::new("t");
    let room = s.spawn("Room", None);
    s.set_transform(room, Transform::from_position(Vec3::new(0.0, 2.0, 0.0)));
    s.add(
        room,
        BoxCollider {
            size: Vec3::new(10.0, 4.0, 10.0),
            ..Default::default()
        },
    );
    let hero = s.spawn("Hero", None);
    let vcam = s.spawn("Cam", None);
    let mut vc = VirtualCamera {
        follow: Some(hero),
        aim: AimKind::DoNothing,
        ..Default::default()
    };
    vc.transposer.offset = Vec3::new(0.0, 8.0, -20.0);
    vc.transposer.damping = Vec3::ZERO;
    vc.confiner = Confiner {
        enabled: true,
        volume: Some(room),
        damping: 0.0,
    };
    s.add(vcam, vc);
    let mut rig = CameraRig::default();
    step(&mut rig, &mut s, DT, 1.0);
    let p = rig.camera_output(vcam).unwrap().position;
    assert!(
        (p - Vec3::new(0.0, 4.0, -5.0)).length() < 1e-3,
        "clamped to the box: {p}"
    );
    // Inside the volume nothing changes.
    s.world
        .get::<&mut VirtualCamera>(vcam)
        .unwrap()
        .transposer
        .offset = Vec3::new(1.0, 1.0, -2.0);
    step(&mut rig, &mut s, DT, 1.0);
    let p = rig.camera_output(vcam).unwrap().position;
    assert!((p - Vec3::new(1.0, 1.0, -2.0)).length() < 1e-3, "{p}");
}

#[test]
fn custom_blend_curves_and_blend_list_assets() {
    // Bézier ease: ends fixed, monotonic, shaped by its control points.
    for c in [DEFAULT_CURVE, [0.0, 0.8, 0.2, 1.0], [0.9, 0.0, 1.0, 0.1]] {
        let w: Vec<f32> = (0..=20).map(|i| bezier_ease(c, i as f32 / 20.0)).collect();
        assert!(w[0].abs() < 1e-3 && (w[20] - 1.0).abs() < 1e-3, "{w:?}");
        assert!(w.windows(2).all(|p| p[1] >= p[0] - 1e-4), "{w:?}");
    }
    assert!(bezier_ease([0.0, 0.8, 0.2, 1.0], 0.2) > 0.6, "fast start");
    assert!(bezier_ease([0.9, 0.0, 1.0, 0.1], 0.5) < 0.1, "slow start");
    let custom = BlendDefinition {
        style: BlendStyle::Custom,
        duration: 1.0,
        curve: [0.0, 0.8, 0.2, 1.0],
    };
    assert!((custom.weight(0.2) - bezier_ease(custom.curve, 0.2)).abs() < 1e-6);

    // A blend list asset wins over the brain's inline blends.
    let root = std::env::temp_dir().join(format!("misty_blends_{}", std::process::id()));
    let list = BlendList {
        blends: vec![CustomBlend {
            from: "A".into(),
            to: "B".into(),
            blend: custom,
        }],
    };
    list.save(&root.join("Assets/Cameras/Cuts.blends")).unwrap();
    assert_eq!(
        BlendList::load(&root.join("Assets/Cameras/Cuts.blends")).unwrap(),
        list
    );
    let mut s = Scene::new("t");
    let cam = s.spawn("Main Camera", None);
    s.add(cam, Camera::default());
    s.add(
        cam,
        CameraBrain {
            blend_list: Some("Assets/Cameras/Cuts.blends".into()),
            custom_blends: vec![CustomBlend {
                from: String::new(),
                to: String::new(),
                blend: BlendDefinition {
                    style: BlendStyle::Cut,
                    ..Default::default()
                },
            }],
            ..Default::default()
        },
    );
    let place = |s: &mut Scene, name: &str, x: f32, p: i32| {
        let e = s.spawn(name, None);
        s.set_transform(e, Transform::from_position(Vec3::new(x, 0.0, 0.0)));
        s.add(
            e,
            VirtualCamera {
                priority: p,
                body: BodyKind::DoNothing,
                aim: AimKind::DoNothing,
                ..Default::default()
            },
        );
        e
    };
    let _a = place(&mut s, "A", 0.0, 10);
    let b = place(&mut s, "B", 10.0, 1);
    let mut rig = CameraRig::new(root.clone());
    step(&mut rig, &mut s, 0.1, 1.0);
    s.world.get::<&mut VirtualCamera>(b).unwrap().priority = 20;
    step(&mut rig, &mut s, 0.2, 1.0);
    let x = s.world_position(cam).x;
    let want = 10.0 * bezier_ease(custom.curve, 0.2);
    assert!(
        (x - want).abs() < 1e-3,
        "custom curve from the asset: {x} vs {want}"
    );
    assert!(rig.take_logs().is_empty());
    std::fs::remove_dir_all(root).ok();
}
