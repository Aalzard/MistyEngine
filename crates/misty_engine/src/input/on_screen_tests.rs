use glam::Vec2;
use hecs::Entity;

use super::*;
use crate::input::{Touch, TouchPhase};
use crate::scene::{Canvas, CanvasScaler, RectTransform, Scene, UiImage, UiScaleMode};

/// 800×600 canvas at scale 1: a 100×100 stick centred at image (100, 500), buttons at
/// (700, 500) (South) and (700, 100) (Space).
fn hud() -> (Scene, Entity, Entity, Entity) {
    let mut s = Scene::new("HUD");
    let canvas = s.spawn("Canvas", None);
    s.add(canvas, Canvas::default());
    s.add(
        canvas,
        CanvasScaler {
            mode: UiScaleMode::ConstantPixelSize,
            ..Default::default()
        },
    );
    let mut widget = |name: &str, at: Vec2| {
        let e = s.spawn(name, Some(canvas));
        let mut rt = RectTransform::sized(100.0, 100.0);
        rt.anchored_position = at;
        s.add(e, rt);
        s.add(e, UiImage::default());
        e
    };
    let stick = widget("Stick", Vec2::new(-300.0, -200.0));
    let south = widget("Jump", Vec2::new(300.0, -200.0));
    let space = widget("Space", Vec2::new(300.0, 200.0));
    s.add(stick, OnScreenStick::default());
    s.add(south, OnScreenButton::default());
    s.add(
        space,
        OnScreenButton {
            control_path: "<Keyboard>/space".into(),
            ..Default::default()
        },
    );
    (s, stick, south, space)
}

fn touch(id: u64, x: f32, y: f32, phase: TouchPhase) -> Touch {
    Touch {
        finger_id: id,
        position: Vec2::new(x, y),
        delta: Vec2::ZERO,
        phase,
    }
}

fn frame(c: &mut OnScreenControls, s: &mut Scene, touches: Vec<Touch>) -> Input {
    let mut input = Input {
        screen_size: Vec2::new(800.0, 600.0),
        touches,
        ..Default::default()
    };
    c.apply(s, &mut input);
    input
}

fn pad(input: &Input) -> &Joystick {
    input
        .joysticks
        .iter()
        .find(|j| j.name == DEVICE_NAME)
        .expect("virtual pad")
}

fn anchored(s: &Scene, e: Entity) -> Vec2 {
    s.world.get::<&RectTransform>(e).unwrap().anchored_position
}

#[test]
fn stick_follows_the_finger_up_to_its_range_and_springs_back() {
    let (mut s, stick, _, _) = hud();
    let mut c = OnScreenControls::default();
    let i = frame(
        &mut c,
        &mut s,
        vec![touch(1, 100.0, 500.0, TouchPhase::Began)],
    );
    assert_eq!(pad(&i).axes[0], 0.0);
    // 25 px right of where it landed: half tilt; the stick moves with the finger.
    let i = frame(
        &mut c,
        &mut s,
        vec![touch(1, 125.0, 500.0, TouchPhase::Moved)],
    );
    assert!((pad(&i).axes[0] - 0.5).abs() < 1e-4, "{:?}", pad(&i).axes);
    assert_eq!(anchored(&s, stick), Vec2::new(-275.0, -200.0));
    // Far up and right: clamped to the range, and +y up reads as joystick axis -1 (down +).
    let i = frame(
        &mut c,
        &mut s,
        vec![touch(1, 100.0, 300.0, TouchPhase::Moved)],
    );
    assert!((pad(&i).axes[1] + 1.0).abs() < 1e-4, "{:?}", pad(&i).axes);
    assert!(pad(&i).axes[0].abs() < 1e-4);
    assert_eq!(anchored(&s, stick), Vec2::new(-300.0, -150.0));
    // The finger leaves the stick's rect but keeps holding it.
    let i = frame(
        &mut c,
        &mut s,
        vec![touch(1, 400.0, 500.0, TouchPhase::Moved)],
    );
    assert!((pad(&i).axes[0] - 1.0).abs() < 1e-4);
    // Lifted: centred and back home.
    let i = frame(
        &mut c,
        &mut s,
        vec![touch(1, 400.0, 500.0, TouchPhase::Ended)],
    );
    assert_eq!(pad(&i).axes[0], 0.0);
    assert_eq!(anchored(&s, stick), Vec2::new(-300.0, -200.0));
    let i = frame(&mut c, &mut s, vec![]);
    assert_eq!(pad(&i).axes, [0.0; JOYSTICK_AXES]);
}

#[test]
fn buttons_press_hold_and_release_with_two_fingers_beside_the_stick() {
    let (mut s, _, south, space) = hud();
    let mut c = OnScreenControls::default();
    // Finger 1 on the stick, finger 2 on Jump, both at once.
    let i = frame(
        &mut c,
        &mut s,
        vec![
            touch(1, 100.0, 500.0, TouchPhase::Began),
            touch(2, 700.0, 500.0, TouchPhase::Began),
        ],
    );
    assert!(pad(&i).pressed[0] && pad(&i).held[0]);
    assert!(c.is_pressed(south));
    let i = frame(
        &mut c,
        &mut s,
        vec![
            touch(1, 140.0, 500.0, TouchPhase::Moved),
            touch(2, 700.0, 500.0, TouchPhase::Stationary),
        ],
    );
    assert!(!pad(&i).pressed[0] && pad(&i).held[0]);
    assert!((pad(&i).axes[0] - 0.8).abs() < 1e-4);
    let i = frame(
        &mut c,
        &mut s,
        vec![
            touch(1, 140.0, 500.0, TouchPhase::Stationary),
            touch(2, 700.0, 500.0, TouchPhase::Ended),
        ],
    );
    assert!(pad(&i).released[0] && !pad(&i).held[0]);
    assert!((pad(&i).axes[0] - 0.8).abs() < 1e-4, "stick still held");
    // A key button with the mouse (the editor's Game view).
    let mut input = Input {
        screen_size: Vec2::new(800.0, 600.0),
        mouse_position: Some(Vec2::new(700.0, 100.0)),
        mouse_pressed: [true, false, false],
        mouse_held: [true, false, false],
        ..Default::default()
    };
    c.apply(&mut s, &mut input);
    assert!(input.pressed.contains("space") && input.held.contains("space"));
    assert!(c.is_pressed(space));
    let mut input = Input {
        screen_size: Vec2::new(800.0, 600.0),
        mouse_position: Some(Vec2::new(700.0, 100.0)),
        mouse_released: [true, false, false],
        ..Default::default()
    };
    c.apply(&mut s, &mut input);
    assert!(input.released.contains("space") && !input.held.contains("space"));
}

#[test]
fn dynamic_origin_starts_where_the_finger_lands() {
    let (mut s, stick, _, _) = hud();
    s.world
        .get::<&mut OnScreenStick>(stick)
        .unwrap()
        .dynamic_origin = true;
    let mut c = OnScreenControls::default();
    // Lands 30 px left of the centre: no tilt, but the stick jumps under the finger.
    let i = frame(
        &mut c,
        &mut s,
        vec![touch(1, 70.0, 500.0, TouchPhase::Began)],
    );
    assert_eq!(pad(&i).axes[0], 0.0);
    let i = frame(
        &mut c,
        &mut s,
        vec![touch(1, 70.0, 500.0, TouchPhase::Stationary)],
    );
    assert_eq!(pad(&i).axes[0], 0.0);
    assert_eq!(anchored(&s, stick), Vec2::new(-330.0, -200.0));
    let i = frame(
        &mut c,
        &mut s,
        vec![touch(1, 95.0, 500.0, TouchPhase::Moved)],
    );
    assert!((pad(&i).axes[0] - 0.5).abs() < 1e-4);
    frame(
        &mut c,
        &mut s,
        vec![touch(1, 95.0, 500.0, TouchPhase::Ended)],
    );
    assert_eq!(anchored(&s, stick), Vec2::new(-300.0, -200.0));
}

#[test]
fn control_paths_map_to_pad_axes_buttons_and_keys() {
    assert_eq!(
        target("<Gamepad>/rightStick").unwrap(),
        Target::Axes {
            x: 3,
            y: 4,
            y_down: true
        }
    );
    assert_eq!(target("<Gamepad>/a").unwrap(), Target::Button(0));
    assert_eq!(target("<Gamepad>/dpad/left").unwrap(), Target::Button(13));
    assert_eq!(
        target("<Gamepad>/rightTrigger").unwrap(),
        Target::Trigger(9)
    );
    assert_eq!(
        target("<Keyboard>/shift").unwrap(),
        Target::Keys(vec!["left shift", "right shift"])
    );
    assert!(target("<Mouse>/delta").is_err());
    assert!(target("<Gamepad>/leftStick/x").is_err());
    assert!(target("nonsense").is_err());
}

#[test]
fn no_virtual_pad_without_on_screen_controls() {
    let mut s = Scene::new("Empty");
    let mut c = OnScreenControls::default();
    let i = frame(&mut c, &mut s, vec![touch(1, 1.0, 1.0, TouchPhase::Began)]);
    assert!(i.joysticks.is_empty());
}

/// Scripts see an on-screen button as a joystick button.
#[test]
fn scripts_read_on_screen_buttons_as_a_gamepad() {
    use crate::physics::Physics;
    use crate::runtime::Time;
    use crate::scene::{ScriptRef, Scripts};
    use crate::scripting::ScriptHost;
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_on_screen_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(
        root.join("Assets/Scripts/Jumper.lua"),
        r#"
local J = {}
function J:Update()
    if Input.GetKeyDown("joystick button 0") then Debug.Log("jump") end
end
return J
"#,
    )
    .unwrap();
    let (mut s, _, _, _) = hud();
    let runner = s.spawn("Runner", None);
    s.add(
        runner,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Jumper.lua".into(),
                ..Default::default()
            }],
        },
    );
    let physics = Physics::build(&s, glam::Vec3::ZERO);
    let mut host = ScriptHost::start(&mut s, physics, root);
    let mut time = Time::default();
    let mut step = |s: &mut Scene, touches: Vec<Touch>| {
        time.advance(1.0 / 60.0);
        let input = Input {
            screen_size: Vec2::new(800.0, 600.0),
            touches,
            ..Default::default()
        };
        host.update(s, &time, input);
        host.take_logs()
            .into_iter()
            .map(|(_, t)| t)
            .collect::<Vec<_>>()
    };
    assert!(step(&mut s, vec![]).is_empty());
    assert_eq!(
        step(&mut s, vec![touch(5, 700.0, 500.0, TouchPhase::Began)]),
        ["jump"]
    );
    assert!(step(&mut s, vec![touch(5, 700.0, 500.0, TouchPhase::Stationary)]).is_empty());
}
