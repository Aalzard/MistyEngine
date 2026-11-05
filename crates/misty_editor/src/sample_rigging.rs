//! RiggingDemo sample scene in `SampleProject`: a little creature built from primitives whose
//! arm reaches a moving target with Two Bone IK (an elbow hint keeps it bending up), whose head
//! follows the target and the camera with Multi-Aim, whose tail follows a second target with
//! Chain IK and whose antenna lags behind with Damped Transforms. `RiggingDemo.lua` moves the
//! targets, sways the body and blends the rig in and out (Space) or the hint (1).
//! Regenerate with `cargo test -p misty_editor -- --ignored write_rigging_demo`; the other test
//! checks the committed files still match and plays the scene.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::rigging::{
    Axis, ChainIk, DampedTransform, MultiAim, Rig, RigBuilder, RigLayer, TwoBoneIk, WeightedSource,
};
use misty_engine::scene::{
    MeshFilter, MeshRenderer, PrimitiveMesh, RectTransform, Scene, ScriptRef, Scripts, Transform,
    UiText,
};

use crate::actions::{Action, Create};
use crate::project::ProjectBrowser;
use crate::state::EditorState;
use crate::ui_create::UiElement;

const SCENE: &str = "Assets/Scenes/RiggingDemo.scene";
const SCRIPT: &str = "Assets/Scripts/RiggingDemo.lua";
const SKIN: &str = "Assets/Materials/Rig Skin.mat";
const JOINT: &str = "Assets/Materials/Rig Joint.mat";
const EYE: &str = "Assets/Materials/Rig Eye.mat";
const TARGET: &str = "Assets/Materials/Rig Target.mat";
const HINT: &str = "Assets/Materials/Rig Hint.mat";
const FLOOR: &str = "Assets/Materials/Checker.mat";

const SCRIPT_SOURCE: &str = r#"-- RiggingDemo: moves the IK targets, sways the creature and blends the rig.
-- Space = blend the whole Rig in / out, 1 = arm hint on / off.
local RiggingDemo = {
    speed = 1,       -- target motion speed
    blend = 4,       -- how fast the rig weight eases to its goal
}

function RiggingDemo:Start()
    self._arm = GameObject.Find("Arm Target")
    self._tail = GameObject.Find("Tail Target")
    self._body = GameObject.Find("Creature")
    self._rig = GameObject.Find("Rig"):GetComponent("Rig")
    self._ik = GameObject.Find("Arm IK"):GetComponent("TwoBoneIKConstraint")
    local label = GameObject.Find("State Text")
    self._label = label and label:GetComponent("Text")
    self._goal = 1
end

function RiggingDemo:Update()
    local t = Time.time * self.speed
    -- The arm target circles in front of the shoulder; the circle grows past the arm's reach.
    local r = 0.5 + 0.4 * math.sin(t * 0.45)
    self._arm.transform.position = Vector3.new(
        1.3 + r * math.cos(t), 1.4 + r * math.sin(t * 1.3), -0.7 + 0.3 * math.sin(t * 0.7))
    -- The tail target draws a figure of eight behind the creature.
    self._tail.transform.position = Vector3.new(
        0.8 * math.sin(t * 0.9), 1.45 + 0.35 * math.sin(t * 1.8), 2.0)
    self._body.transform.eulerAngles = Vector3.new(0, 16 * math.sin(t * 0.6), 0)

    if Input.GetKeyDown("space") then self._goal = 1 - self._goal end
    if Input.GetKeyDown("1") then self._ik.hintWeight = 1 - self._ik.hintWeight end
    local w = self._rig.weight
    self._rig.weight = w + (self._goal - w) * math.min(1, Time.deltaTime * self.blend)
    if self._label then
        self._label.text = string.format("Rig weight %.2f    Hint weight %.0f",
            self._rig.weight, self._ik.hintWeight)
    end
end

return RiggingDemo
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn materials() -> [(&'static str, Material); 5] {
    let m = |color: [f32; 4], metallic, smoothness| Material {
        color,
        metallic,
        smoothness,
        ..Default::default()
    };
    let glow = |color: [f32; 3], intensity| Material {
        emission_enabled: true,
        emission: color,
        emission_intensity: intensity,
        ..m([color[0], color[1], color[2], 1.0], 0.0, 0.6)
    };
    [
        (SKIN, m([0.22, 0.58, 0.62, 1.0], 0.0, 0.45)),
        (JOINT, m([0.12, 0.13, 0.16, 1.0], 0.6, 0.55)),
        (EYE, m([0.95, 0.95, 0.92, 1.0], 0.0, 0.85)),
        (TARGET, glow([1.0, 0.42, 0.2], 1.6)),
        (HINT, glow([1.0, 0.84, 0.25], 1.2)),
    ]
}

fn mesh(
    s: &mut Scene,
    name: &str,
    parent: Option<Entity>,
    kind: PrimitiveMesh,
    t: Transform,
    material: &str,
) -> Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(kind));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
    e
}

fn scaled(position: Vec3, scale: Vec3) -> Transform {
    Transform {
        position,
        scale,
        ..Default::default()
    }
}

/// An empty pivot (bone) at a local position.
fn pivot(s: &mut Scene, name: &str, parent: Entity, at: Vec3) -> Entity {
    let e = s.spawn(name, Some(parent));
    s.set_transform(e, Transform::from_position(at));
    e
}

/// A bone of `len` along local +X: joint ball at the pivot and a rod to the next joint.
fn arm_segment(s: &mut Scene, bone: Entity, len: f32, radius: f32) {
    let rod = Transform {
        position: Vec3::new(len * 0.5, 0.0, 0.0),
        scale: Vec3::new(radius, len * 0.5, radius),
        ..Default::default()
    }
    .with_euler(Vec3::new(0.0, 0.0, 90.0));
    mesh(s, "Rod", Some(bone), PrimitiveMesh::Cylinder, rod, SKIN);
    let ball = scaled(Vec3::ZERO, Vec3::splat(radius * 1.6));
    mesh(s, "Joint", Some(bone), PrimitiveMesh::Sphere, ball, JOINT);
}

/// A small glowing marker object (an IK target or hint the Move tool can drag).
fn marker(s: &mut Scene, name: &str, parent: Option<Entity>, at: Vec3, material: &str) -> Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, Transform::from_position(at));
    let ball = scaled(Vec3::ZERO, Vec3::splat(0.16));
    let m = mesh(s, "Marker", Some(e), PrimitiveMesh::Sphere, ball, material);
    s.world.get::<&mut MeshRenderer>(m).unwrap().cast_shadows = false;
    e
}

fn constraint<T: misty_engine::scene::SceneComponent>(
    s: &mut Scene,
    rig: Entity,
    name: &str,
    c: T,
) -> Entity {
    let e = s.spawn(name, Some(rig));
    s.add(e, c);
    e
}

fn create(s: &mut EditorState, what: Create) -> Entity {
    crate::commands::apply(s, Action::Create { what, parent: None });
    s.selection.active().expect("created object is selected")
}

/// A Text anchored to the top-left corner at `y` pixels down.
fn text(s: &mut EditorState, name: &str, y: f32, size: f32, body: &str) {
    let e = create(s, Create::Ui(UiElement::Text));
    s.scene.game_object_mut(e).name = name.into();
    let w = &mut s.scene.world;
    *w.get::<&mut RectTransform>(e).unwrap() = RectTransform {
        anchor_min: Vec2::new(0.0, 1.0),
        anchor_max: Vec2::new(0.0, 1.0),
        pivot: Vec2::new(0.0, 1.0),
        anchored_position: Vec2::new(20.0, -y),
        size_delta: Vec2::new(760.0, size * 1.6),
    };
    let mut t = w.get::<&mut UiText>(e).unwrap();
    t.text = body.into();
    t.font_size = size;
    t.color = [1.0; 4];
}

fn demo_scene() -> Scene {
    let mut st = EditorState::new(ProjectBrowser::open(
        std::env::temp_dir().join("misty_rigging_demo_build"),
    ));
    st.scene = Scene::default_3d("RiggingDemo");
    let s = &mut st.scene;
    let cam = s.find_by_name("Main Camera").unwrap();
    s.set_transform(
        cam,
        Transform::from_position(Vec3::new(4.4, 2.9, -3.4)).with_euler(Vec3::new(15.0, -52.0, 0.0)),
    );
    mesh(
        s,
        "Floor",
        None,
        PrimitiveMesh::Plane,
        scaled(Vec3::ZERO, Vec3::splat(2.0)),
        FLOOR,
    );

    // The creature: a Rig Builder on the root, bones as plain child Transforms.
    let creature = s.spawn("Creature", None);
    let body = scaled(Vec3::new(0.0, 1.1, 0.2), Vec3::new(0.9, 0.7, 1.3));
    mesh(s, "Body", Some(creature), PrimitiveMesh::Cube, body, SKIN);
    for x in [-0.3, 0.3] {
        for z in [-0.25, 0.65] {
            let leg = scaled(Vec3::new(x, 0.38, z), Vec3::new(0.14, 0.38, 0.14));
            mesh(
                s,
                "Leg",
                Some(creature),
                PrimitiveMesh::Cylinder,
                leg,
                JOINT,
            );
        }
    }

    // Arm along +X: Shoulder ▸ Elbow ▸ Hand.
    let shoulder = pivot(s, "Shoulder", creature, Vec3::new(0.5, 1.3, -0.15));
    arm_segment(s, shoulder, 0.75, 0.09);
    let elbow = pivot(s, "Elbow", shoulder, Vec3::new(0.75, 0.0, 0.0));
    arm_segment(s, elbow, 0.7, 0.075);
    let hand = pivot(s, "Hand", elbow, Vec3::new(0.7, 0.0, 0.0));
    let palm = scaled(Vec3::new(0.08, 0.0, 0.0), Vec3::new(0.22, 0.12, 0.18));
    mesh(s, "Palm", Some(hand), PrimitiveMesh::Cube, palm, JOINT);

    // Head with eyes facing -Z (the camera) and a three-part antenna.
    let neck = scaled(Vec3::new(0.0, 1.55, -0.3), Vec3::new(0.16, 0.15, 0.16));
    mesh(
        s,
        "Neck",
        Some(creature),
        PrimitiveMesh::Cylinder,
        neck,
        JOINT,
    );
    let head = pivot(s, "Head", creature, Vec3::new(0.0, 1.85, -0.35));
    let skull = scaled(Vec3::ZERO, Vec3::splat(0.55));
    mesh(s, "Skull", Some(head), PrimitiveMesh::Sphere, skull, SKIN);
    for x in [-0.12, 0.12] {
        let eye = scaled(Vec3::new(x, 0.06, -0.22), Vec3::splat(0.14));
        let e = mesh(s, "Eye", Some(head), PrimitiveMesh::Sphere, eye, EYE);
        let pupil = scaled(Vec3::new(0.0, 0.0, -0.4), Vec3::splat(0.45));
        mesh(s, "Pupil", Some(e), PrimitiveMesh::Sphere, pupil, JOINT);
    }
    let mut antenna = Vec::new();
    let mut parent = head;
    for (i, at) in [0.26f32, 0.24, 0.24].into_iter().enumerate() {
        let seg = pivot(
            s,
            &format!("Antenna {}", i + 1),
            parent,
            Vec3::new(0.0, at, 0.0),
        );
        let rod = scaled(Vec3::new(0.0, 0.12, 0.0), Vec3::new(0.035, 0.12, 0.035));
        mesh(s, "Rod", Some(seg), PrimitiveMesh::Cylinder, rod, JOINT);
        antenna.push(seg);
        parent = seg;
    }
    let bulb = scaled(Vec3::new(0.0, 0.27, 0.0), Vec3::splat(0.11));
    mesh(s, "Bulb", Some(parent), PrimitiveMesh::Sphere, bulb, HINT);

    // Tail: six bones going back and slightly up.
    let mut tail = vec![pivot(s, "Tail Root", creature, Vec3::new(0.0, 1.05, 0.85))];
    for i in 1..6 {
        let parent = tail[i - 1];
        tail.push(pivot(
            s,
            &format!("Tail {i}"),
            parent,
            Vec3::new(0.0, 0.04, 0.3),
        ));
    }
    for (i, &b) in tail.iter().enumerate() {
        let r = 0.24 - i as f32 * 0.03;
        mesh(
            s,
            "Segment",
            Some(b),
            PrimitiveMesh::Sphere,
            scaled(Vec3::ZERO, Vec3::splat(r)),
            SKIN,
        );
    }

    // Effectors: ordinary objects (moved by RiggingDemo.lua, or by hand with the Move tool).
    let arm_target = marker(s, "Arm Target", None, Vec3::new(1.6, 1.5, -0.7), TARGET);
    let hint = marker(
        s,
        "Elbow Hint",
        Some(creature),
        Vec3::new(1.0, 2.2, 0.5),
        HINT,
    );
    let tail_target = marker(s, "Tail Target", None, Vec3::new(0.0, 1.45, 2.0), TARGET);

    // The rig and its constraints (evaluated top to bottom).
    let rig = s.spawn("Rig", Some(creature));
    s.add(rig, Rig::default());
    constraint(
        s,
        rig,
        "Arm IK",
        TwoBoneIk {
            root: Some(shoulder),
            mid: Some(elbow),
            tip: Some(hand),
            target: Some(arm_target),
            hint: Some(hint),
            target_rotation_weight: 0.0,
            ..Default::default()
        },
    );
    constraint(
        s,
        rig,
        "Head Aim",
        MultiAim {
            constrained_object: Some(head),
            aim_axis: Axis::NegZ,
            up_axis: Axis::Y,
            sources: vec![
                WeightedSource {
                    object: Some(arm_target),
                    weight: 1.0,
                },
                WeightedSource {
                    object: Some(cam),
                    weight: 0.35,
                },
            ],
            constrained_axes: [true, true, false],
            min_limit: -75.0,
            max_limit: 75.0,
            ..Default::default()
        },
    );
    constraint(
        s,
        rig,
        "Tail IK",
        ChainIk {
            root: Some(tail[0]),
            tip: Some(tail[5]),
            target: Some(tail_target),
            tip_rotation_weight: 0.0,
            tolerance: 0.001,
            ..Default::default()
        },
    );
    let mut source = head;
    for (i, &seg) in antenna.iter().enumerate() {
        constraint(
            s,
            rig,
            &format!("Antenna Damp {}", i + 1),
            DampedTransform {
                constrained_object: Some(seg),
                source: Some(source),
                damp_position: 0.0,
                damp_rotation: 0.86 - i as f32 * 0.04,
                maintain_aim: false,
                ..Default::default()
            },
        );
        source = seg;
    }
    s.add(
        creature,
        RigBuilder {
            layers: vec![RigLayer {
                rig: Some(rig),
                active: true,
            }],
            ..Default::default()
        },
    );

    let driver = s.spawn("Rig Driver", None);
    s.add(
        driver,
        Scripts {
            list: vec![ScriptRef {
                path: SCRIPT.into(),
                ..Default::default()
            }],
        },
    );

    text(
        &mut st,
        "Hint Text",
        16.0,
        22.0,
        "Space: blend the rig in / out    1: elbow hint on / off",
    );
    text(
        &mut st,
        "State Text",
        52.0,
        26.0,
        "Rig weight 1.00    Hint weight 1",
    );
    st.scene
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_rigging_demo() {
    let root = project();
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
    }
    std::fs::write(root.join(SCRIPT), SCRIPT_SOURCE).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn rigging_demo_matches_its_generator_and_plays() {
    use misty_engine::audio::Audio;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::Input;

    let root = project();
    for (path, m) in materials() {
        assert_eq!(Material::load(&root.join(path)).unwrap(), m, "{path}");
    }
    let script = std::fs::read_to_string(root.join(SCRIPT)).unwrap();
    assert_eq!(script.replace("\r\n", "\n"), SCRIPT_SOURCE);
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert_eq!(
        text,
        demo_scene().to_json(),
        "regenerate with write_rigging_demo"
    );

    let (mut scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let mut rt = Runtime::start_with_audio(
        &mut scene,
        root.clone(),
        &Default::default(),
        Audio::null(root).0,
    );
    let find = |s: &Scene, n: &str| s.find_by_name(n).unwrap();
    let (shoulder, hand, head) = (
        find(&scene, "Shoulder"),
        find(&scene, "Hand"),
        find(&scene, "Head"),
    );
    let (arm_target, tail_target) = (find(&scene, "Arm Target"), find(&scene, "Tail Target"));
    let tail_tip = find(&scene, "Tail 5");
    let mut time = Time::default();
    let (mut reached, mut stretched) = (0, 0);
    for frame in 0..600 {
        time.advance(1.0 / 60.0);
        let mut input = Input::default();
        if frame == 400 {
            input.pressed.insert("space".into());
        }
        rt.update(&mut scene, &time, input);
        if frame >= 400 {
            continue;
        }
        let t = scene.world_position(arm_target);
        let s = scene.world_position(shoulder);
        let h = scene.world_position(hand);
        if (t - s).length() < 1.4 {
            assert!(
                (h - t).length() < 1e-2,
                "frame {frame}: hand {h} target {t}"
            );
            reached += 1;
        } else if (t - s).length() > 1.5 {
            let dir = (t - s).normalize();
            assert!(
                (h - s).normalize().dot(dir) > 0.999,
                "frame {frame}: straight"
            );
            stretched += 1;
        }
        // The head's -Z looks towards its sources (limits allow it in this scene).
        let fwd = scene.world_rotation(head) * Vec3::NEG_Z;
        let cam = scene.world_position(find(&scene, "Main Camera"));
        let goal = (t + cam * 0.35) / 1.35 - scene.world_position(head);
        assert!(fwd.dot(goal.normalize()) > 0.97, "frame {frame}: head");
        let tail = scene.world_position(tail_tip);
        assert!(
            (tail - scene.world_position(tail_target)).length() < 0.05,
            "frame {frame}: tail {tail}"
        );
    }
    assert!(reached > 50 && stretched > 10, "{reached} / {stretched}");
    // Space blended the rig out.
    let rig = find(&scene, "Rig");
    assert!(scene.world.get::<&Rig>(rig).unwrap().weight < 0.05);
    let logs = rt.take_logs();
    assert!(logs.is_empty(), "{logs:?}");
}
