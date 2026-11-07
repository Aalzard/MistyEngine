//! The AnimatorDemo sample in `SampleProject`: a little "Hero" cube driven by an Animator
//! Controller (a Locomotion 1D blend tree of Idle and Walk on `Speed`, a Jump from Any State on
//! a trigger, and a Dance state on a bool), with `AnimatorControl.lua` reading the keyboard and a
//! UI Text hint. Regenerate with `cargo test -p misty_editor -- --ignored write_animator_demo`;
//! the other test checks the committed files still match and plays the scene.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::animation::controller::{
    BlendChild, BlendTree, Condition, ConditionMode, Node, NodePositions, ParamKind, Parameter,
    State, Transition,
};
use misty_engine::animation::{AnimationClip, AnimatorController, Property, TangentMode};
use misty_engine::scene::{
    Animator, MeshFilter, MeshRenderer, PrimitiveMesh, RectTransform, Scene, ScriptRef, Scripts,
    Transform, UiText,
};

use crate::actions::{Action, Create};
use crate::project::ProjectBrowser;
use crate::state::EditorState;
use crate::ui_create::UiElement;

const SCENE: &str = "Assets/Scenes/AnimatorDemo.scene";
const CONTROLLER: &str = "Assets/Animations/Hero.controller";
const IDLE: &str = "Assets/Animations/HeroIdle.anim";
const WALK: &str = "Assets/Animations/HeroWalk.anim";
const JUMP: &str = "Assets/Animations/HeroJump.anim";
const DANCE: &str = "Assets/Animations/HeroDance.anim";
const SCRIPT: &str = "Assets/Scripts/AnimatorControl.lua";

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// A clip from (property, [(time, value)]) curves on the child "Body".
fn clip(length: f32, looping: bool, curves: &[(Property, &[(f32, f32)])]) -> AnimationClip {
    let mut c = AnimationClip {
        length,
        looping,
        ..Default::default()
    };
    for (p, keys) in curves {
        for (t, v) in *keys {
            c.set_key("Body", *p, *t, *v);
        }
    }
    c
}

/// Breathing: the body stretches a little, slowly.
fn idle() -> AnimationClip {
    clip(
        2.0,
        true,
        &[
            (
                Property::Position(1),
                &[(0.0, 0.5), (1.0, 0.56), (2.0, 0.5)],
            ),
            (Property::Scale(1), &[(0.0, 1.0), (1.0, 1.1), (2.0, 1.0)]),
        ],
    )
}

/// Bouncy steps with a side-to-side wobble.
fn walk() -> AnimationClip {
    clip(
        0.6,
        true,
        &[
            (
                Property::Position(1),
                &[(0.0, 0.5), (0.15, 0.8), (0.3, 0.5), (0.45, 0.8), (0.6, 0.5)],
            ),
            (
                Property::Rotation(2),
                &[
                    (0.0, 0.0),
                    (0.15, 10.0),
                    (0.3, 0.0),
                    (0.45, -10.0),
                    (0.6, 0.0),
                ],
            ),
            (Property::Scale(1), &[(0.0, 1.0), (0.6, 1.0)]),
        ],
    )
}

/// Up, a full flip, and down (plays once).
fn jump() -> AnimationClip {
    let mut c = clip(
        0.8,
        false,
        &[
            (Property::Position(1), &[(0.0, 0.5), (0.4, 2.6), (0.8, 0.5)]),
            (Property::Rotation(0), &[(0.0, 0.0), (0.8, 360.0)]),
        ],
    );
    let flip = c.curves.len() - 1;
    for i in 0..2 {
        c.curves[flip].keys.set_tangent(i, TangentMode::Linear);
    }
    c
}

/// A spin with a pulsing width.
fn dance() -> AnimationClip {
    let mut c = clip(
        1.0,
        true,
        &[
            (Property::Rotation(1), &[(0.0, 0.0), (1.0, 360.0)]),
            (
                Property::Scale(0),
                &[(0.0, 1.0), (0.25, 1.3), (0.5, 1.0), (0.75, 1.3), (1.0, 1.0)],
            ),
            (
                Property::Scale(2),
                &[(0.0, 1.0), (0.25, 1.3), (0.5, 1.0), (0.75, 1.3), (1.0, 1.0)],
            ),
        ],
    );
    for i in 0..2 {
        c.curves[0].keys.set_tangent(i, TangentMode::Linear);
    }
    c
}

fn cond(parameter: &str, mode: ConditionMode) -> Condition {
    Condition {
        parameter: parameter.into(),
        mode,
        threshold: 0.0,
    }
}

fn controller() -> AnimatorController {
    let param = |name: &str, kind| Parameter {
        name: name.into(),
        kind,
        default: 0.0,
    };
    let fade = |from: Node, to: Node, duration: f32, c: Vec<Condition>| Transition {
        has_exit_time: false,
        duration,
        conditions: c,
        ..Transition::new(from, to)
    };
    AnimatorController {
        parameters: vec![
            param("Speed", ParamKind::Float),
            param("Jump", ParamKind::Trigger),
            param("Dance", ParamKind::Bool),
        ],
        states: vec![
            State {
                name: "Locomotion".into(),
                blend_tree: Some(BlendTree {
                    parameter: "Speed".into(),
                    children: vec![
                        BlendChild {
                            motion: Some(IDLE.into()),
                            threshold: 0.0,
                        },
                        BlendChild {
                            motion: Some(WALK.into()),
                            threshold: 1.0,
                        },
                    ],
                }),
                position: [0.0, 0.0],
                ..Default::default()
            },
            State {
                name: "Jump".into(),
                motion: Some(JUMP.into()),
                looping: false,
                position: [0.0, -140.0],
                ..Default::default()
            },
            State {
                name: "Dance".into(),
                motion: Some(DANCE.into()),
                position: [260.0, 110.0],
                ..Default::default()
            },
        ],
        default_state: Some("Locomotion".into()),
        transitions: vec![
            fade(
                Node::AnyState,
                Node::state("Jump"),
                0.1,
                vec![cond("Jump", ConditionMode::If)],
            ),
            Transition {
                has_exit_time: true,
                exit_time: 0.9,
                duration: 0.1,
                ..Transition::new(Node::state("Jump"), Node::Exit)
            },
            fade(
                Node::state("Locomotion"),
                Node::state("Dance"),
                0.25,
                vec![cond("Dance", ConditionMode::If)],
            ),
            fade(
                Node::state("Dance"),
                Node::state("Locomotion"),
                0.25,
                vec![cond("Dance", ConditionMode::IfNot)],
            ),
        ],
        nodes: NodePositions {
            entry: [-280.0, 0.0],
            any_state: [-280.0, -140.0],
            exit: [280.0, -140.0],
        },
    }
}

fn create(s: &mut EditorState, what: Create) -> misty_engine::hecs::Entity {
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
    let mut s = EditorState::new(ProjectBrowser::open(
        std::env::temp_dir().join("misty_animator_demo_build"),
    ));
    s.scene = Scene::default_3d("AnimatorDemo");
    if let Some(cam) = s.scene.find_by_name("Main Camera") {
        s.scene.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 2.2, -6.0))
                .with_euler(Vec3::new(12.0, 0.0, 0.0)),
        );
    }
    let floor = s.scene.spawn("Floor", None);
    s.scene
        .add(floor, MeshFilter::primitive(PrimitiveMesh::Plane));
    s.scene.add(floor, MeshRenderer::default());

    // The Animator sits on "Hero"; its clips move the child "Body".
    let hero = s.scene.spawn("Hero", None);
    let body = s.scene.spawn("Body", Some(hero));
    s.scene
        .set_transform(body, Transform::from_position(Vec3::new(0.0, 0.5, 0.0)));
    s.scene
        .add(body, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.scene.add(
        body,
        MeshRenderer {
            material: Some("Assets/Materials/Red.mat".into()),
            ..Default::default()
        },
    );
    s.scene.add(
        hero,
        Animator {
            controller: Some(CONTROLLER.into()),
            ..Default::default()
        },
    );
    s.scene.add(
        hero,
        Scripts {
            list: vec![ScriptRef {
                path: SCRIPT.into(),
                ..Default::default()
            }],
        },
    );

    text(
        &mut s,
        "Hint Text",
        16.0,
        22.0,
        "Space: Jump    1 / 2 / 3: Idle / Jog / Walk    4: Dance on / off",
    );
    text(&mut s, "State Text", 52.0, 26.0, "State: Locomotion");
    s.scene
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_animator_demo() {
    let root = project();
    for (path, c) in [
        (IDLE, idle()),
        (WALK, walk()),
        (JUMP, jump()),
        (DANCE, dance()),
    ] {
        c.save(&root.join(path)).unwrap();
    }
    controller().save(&root.join(CONTROLLER)).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn animator_demo_matches_its_generator_and_plays() {
    use misty_engine::audio::Audio;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::Input;

    let root = project();
    for (path, c) in [
        (IDLE, idle()),
        (WALK, walk()),
        (JUMP, jump()),
        (DANCE, dance()),
    ] {
        crate::test_util::assert_close(&AnimationClip::load(&root.join(path)).unwrap(), &c, path);
    }
    crate::test_util::assert_close(
        &AnimatorController::load(&root.join(CONTROLLER)).unwrap(),
        &controller(),
        CONTROLLER,
    );
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_animator_demo",
    );

    let (mut scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let mut rt = Runtime::start_with_audio(
        &mut scene,
        root.clone(),
        &Default::default(),
        Audio::null(root).0,
    );
    let hero = scene.find_by_name("Hero").unwrap();
    let body = scene.find_by_name("Body").unwrap();
    let label = scene.find_by_name("State Text").unwrap();
    let mut time = Time::default();
    // `n` frames; `key` is pressed on the first one.
    let mut frames = |rt: &mut Runtime, scene: &mut Scene, n: usize, key: Option<&str>| {
        for i in 0..n {
            let mut input = Input::default();
            if let (0, Some(k)) = (i, key) {
                input.pressed.insert(k.into());
            }
            time.advance(1.0 / 60.0);
            rt.update(scene, &time, input);
        }
    };
    frames(&mut rt, &mut scene, 30, None);
    let state = |rt: &Runtime, scene: &Scene| {
        rt.animator_status(scene, hero)
            .unwrap()
            .current
            .unwrap()
            .name
    };
    assert_eq!(state(&rt, &scene), "Locomotion");
    assert!(scene.transform(body).scale.y > 1.0, "breathing");
    // Space jumps; the hint follows the state.
    frames(&mut rt, &mut scene, 20, Some("space"));
    assert_eq!(state(&rt, &scene), "Jump");
    assert!(scene.transform(body).position.y > 1.0, "in the air");
    let shown = scene.world.get::<&UiText>(label).unwrap().text.clone();
    assert!(shown.starts_with("State: Jump"), "{shown}");
    // After the jump it returns to Locomotion; 4 starts dancing.
    frames(&mut rt, &mut scene, 60, None);
    assert_eq!(state(&rt, &scene), "Locomotion");
    frames(&mut rt, &mut scene, 30, Some("4"));
    assert_eq!(state(&rt, &scene), "Dance");
    let logs = rt.take_logs();
    assert!(logs.is_empty(), "{logs:?}");
}
