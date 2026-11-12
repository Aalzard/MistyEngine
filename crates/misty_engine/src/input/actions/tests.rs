//! Input System tests: control paths, composites, processors, interactions on a simulated
//! clock, action types, control schemes and pairing, rebinding and override persistence.

use glam::Vec2;

use super::asset::*;
use super::composites;
use super::interactions::{Interaction, InteractionState, Transition};
use super::path::*;
use super::processors;
use super::state::ActionSet;
use super::system::{DeviceId, InputSystem, SystemEvent};
use super::value::{Value, parse_list};
use super::{Phase, PlayerInput};
use crate::input::Joystick;
use crate::scene::Scene;
use crate::scripting::Input;

fn keys(held: &[&str]) -> Input {
    let mut i = Input::default();
    i.held.extend(held.iter().map(|k| (*k).to_owned()));
    i
}

/// `held` this frame, given what was held the frame before (pressed / released edges).
fn frame(prev: &[&str], held: &[&str]) -> Input {
    let mut i = keys(held);
    i.pressed.extend(
        held.iter()
            .filter(|k| !prev.contains(k))
            .map(|k| (*k).to_owned()),
    );
    i.released.extend(
        prev.iter()
            .filter(|k| !held.contains(k))
            .map(|k| (*k).to_owned()),
    );
    i
}

fn pad(name: &str) -> Joystick {
    Joystick {
        name: name.into(),
        ..Default::default()
    }
}

fn one_action(action: Action) -> ActionSet {
    let mut asset = InputActionAsset {
        name: "Test".into(),
        maps: vec![ActionMap {
            name: "Player".into(),
            id: String::new(),
            actions: vec![action],
        }],
        control_schemes: default_schemes(),
    };
    asset.ensure_ids();
    let mut set = ActionSet::new(asset, "");
    set.set_all_enabled(true);
    set
}

fn phases(events: &[super::ActionEvent]) -> Vec<Phase> {
    events.iter().map(|e| e.phase).collect()
}

// ---- paths -----------------------------------------------------------------------------------

#[test]
fn binding_paths_parse_and_display() {
    let p = ControlPath::parse("<Keyboard>/w").unwrap();
    assert_eq!(p.device, DeviceKind::Keyboard);
    assert_eq!(p.control, Control::Keys(vec!["w"]));
    assert_eq!(p.display(), "W [Keyboard]");
    assert_eq!(
        ControlPath::parse("<keyboard>/LEFTSHIFT").unwrap().control,
        Control::Keys(vec!["left shift"])
    );
    assert_eq!(
        ControlPath::parse("<Keyboard>/shift")
            .unwrap()
            .control_name(),
        "Shift"
    );
    assert_eq!(
        ControlPath::parse("<Keyboard>/f5").unwrap().control,
        Control::Keys(vec!["f5"])
    );
    let delta = ControlPath::parse("<Mouse>/delta").unwrap();
    assert!(delta.is_vector() && delta.is_noisy());
    assert_eq!(
        ControlPath::parse("<Mouse>/delta/x").unwrap().control,
        Control::MouseDelta(Part::X)
    );
    let stick = ControlPath::parse("<Gamepad>/leftStick").unwrap();
    assert_eq!(stick.control, Control::PadStick(Stick::Left, Part::Whole));
    assert_eq!(stick.display(), "Left Stick [Gamepad]");
    assert_eq!(
        ControlPath::parse("<Gamepad>/buttonSouth").unwrap().control,
        Control::PadButton(0)
    );
    // Xbox letters are aliases.
    assert_eq!(
        ControlPath::parse("<Gamepad>/a").unwrap().control,
        Control::PadButton(0)
    );
    assert_eq!(
        ControlPath::parse("<Gamepad>/rightStick/up")
            .unwrap()
            .control,
        Control::PadStick(Stick::Right, Part::Up)
    );
    assert_eq!(
        ControlPath::parse("<Touchscreen>/primaryTouch/position")
            .unwrap()
            .control,
        Control::TouchPosition(None, Part::Whole)
    );
    assert_eq!(
        ControlPath::parse("<Touchscreen>/touch1/press")
            .unwrap()
            .control,
        Control::TouchPress(Some(1))
    );
    // Usage qualifiers are accepted.
    assert!(ControlPath::parse("<Gamepad>{LeftHand}/leftTrigger").is_ok());
    for bad in [
        "w",
        "<Keyboard>",
        "<Keyboard>/nope",
        "<Toaster>/x",
        "<Gamepad>/leftStick/z",
    ] {
        assert!(ControlPath::parse(bad).is_err(), "{bad}");
    }
    assert_eq!(display_path(""), "<No Binding>");
    assert_eq!(short_name("<Gamepad>/buttonEast"), "Button East");
    assert_eq!(normalize("<keyboard>/LEFTSHIFT"), "<Keyboard>/leftShift");
    assert_eq!(
        keyboard_path("left shift").as_deref(),
        Some("<Keyboard>/leftShift")
    );
    assert_eq!(keyboard_path("w").as_deref(), Some("<Keyboard>/w"));
    assert_eq!(pad_button_path(12).as_deref(), Some("<Gamepad>/dpad/down"));
    // Every picker entry parses back.
    for d in DeviceKind::ALL {
        for (path, _) in controls(d) {
            assert!(ControlPath::parse(&path).is_ok(), "{path}");
        }
    }
}

#[test]
fn controls_read_keyboard_mouse_gamepad_and_touch() {
    let all = DeviceFilter::all();
    let read = |p: &str, i: &Input| ControlPath::parse(p).unwrap().read(i, &all);
    let mut i = keys(&["space", "right shift"]);
    i.mouse_delta = Vec2::new(4.0, 3.0);
    i.screen_size = Vec2::new(800.0, 600.0);
    i.mouse_position = Some(Vec2::new(100.0, 50.0));
    let mut j = pad("Pad");
    j.held[0] = true;
    j.axes[0] = 0.5;
    j.axes[1] = 1.0; // joystick +1 = down
    j.axes[9] = 0.7;
    i.joysticks = vec![j];
    i.touches = vec![crate::input::Touch {
        finger_id: 1,
        position: Vec2::new(10.0, 20.0),
        ..Default::default()
    }];
    assert_eq!(read("<Keyboard>/space", &i), Value::Float(1.0));
    assert_eq!(read("<Keyboard>/shift", &i), Value::Float(1.0));
    assert_eq!(read("<Keyboard>/leftShift", &i), Value::Float(0.0));
    // Mouse delta and position are y-up.
    assert_eq!(
        read("<Mouse>/delta", &i),
        Value::Vector2(Vec2::new(4.0, -3.0))
    );
    assert_eq!(
        read("<Mouse>/position", &i),
        Value::Vector2(Vec2::new(100.0, 550.0))
    );
    assert_eq!(read("<Gamepad>/buttonSouth", &i), Value::Float(1.0));
    assert_eq!(
        read("<Gamepad>/leftStick", &i),
        Value::Vector2(Vec2::new(0.5, -1.0))
    );
    assert_eq!(read("<Gamepad>/leftStick/down", &i), Value::Float(1.0));
    assert_eq!(read("<Gamepad>/rightTrigger", &i), Value::Float(0.7));
    assert_eq!(
        read("<Touchscreen>/primaryTouch/press", &i),
        Value::Float(1.0)
    );
    assert_eq!(
        read("<Touchscreen>/primaryTouch/position", &i),
        Value::Vector2(Vec2::new(10.0, 580.0))
    );
    // A filter without the gamepad reads its default.
    let keyboard_only = DeviceFilter {
        gamepads: Some(Vec::new()),
        ..DeviceFilter::all()
    };
    let c = ControlPath::parse("<Gamepad>/buttonSouth").unwrap();
    assert_eq!(c.read(&i, &keyboard_only), Value::Float(0.0));
}

// ---- composites ------------------------------------------------------------------------------

#[test]
fn composites_combine_their_parts() {
    let parts = |held: &'static [&'static str]| {
        move |name: &str| Value::Float(if held.contains(&name) { 1.0 } else { 0.0 })
    };
    let v = |kind: &str, params: &str, held: &'static [&'static str]| {
        composites::evaluate(kind, params, parts(held))
    };
    let diag = v("2DVector", "", &["up", "right"]).as_vec2();
    assert!((diag.length() - 1.0).abs() < 1e-5, "normalised: {diag}");
    assert_eq!(
        v("2DVector", "mode=Digital", &["up", "right"]),
        Value::Vector2(Vec2::new(1.0, 1.0))
    );
    assert_eq!(
        v("2DVector", "", &["up", "down"]),
        Value::Vector2(Vec2::ZERO)
    );
    // Analog keeps part values.
    let analog = composites::evaluate("2DVector", "mode=Analog", |n| {
        Value::Float(if n == "right" { 0.3 } else { 0.0 })
    });
    assert_eq!(analog, Value::Vector2(Vec2::new(0.3, 0.0)));
    assert_eq!(v("1DAxis", "", &["positive"]), Value::Float(1.0));
    assert_eq!(v("1DAxis", "", &["negative"]), Value::Float(-1.0));
    assert_eq!(
        v("1DAxis", "", &["negative", "positive"]),
        Value::Float(0.0)
    );
    assert_eq!(
        v(
            "1DAxis",
            "whichSideWins=Positive",
            &["negative", "positive"]
        ),
        Value::Float(1.0)
    );
    assert_eq!(
        v("1DAxis", "minValue=0,maxValue=10", &["positive"]),
        Value::Float(10.0)
    );
    assert_eq!(
        v("ButtonWithOneModifier", "", &["button"]),
        Value::Float(0.0)
    );
    assert_eq!(
        v("ButtonWithOneModifier", "", &["modifier", "button"]),
        Value::Float(1.0)
    );
    assert_eq!(
        v("ButtonWithTwoModifiers", "", &["modifier1", "button"]),
        Value::Float(0.0)
    );
    assert_eq!(
        v(
            "ButtonWithTwoModifiers",
            "",
            &["modifier1", "modifier2", "button"]
        ),
        Value::Float(1.0)
    );
}

#[test]
fn wasd_composite_and_shift_modifier_drive_actions() {
    let mut set = one_action(
        Action::new("Move", ActionType::Value, ControlType::Vector2).with(Binding::composite(
            "WASD",
            "2DVector",
            &[
                ("up", "<Keyboard>/w"),
                ("down", "<Keyboard>/s"),
                ("left", "<Keyboard>/a"),
                ("right", "<Keyboard>/d"),
            ],
            KEYBOARD_MOUSE,
        )),
    );
    let ev = set.update(&frame(&[], &["w"]), 0.0);
    assert_eq!(phases(&ev), [Phase::Started, Phase::Performed]);
    assert_eq!(ev[1].value, Value::Vector2(Vec2::Y));
    let ev = set.update(&frame(&["w"], &["w", "d"]), 0.1);
    assert_eq!(phases(&ev), [Phase::Performed]);
    let diag = ev[0].value.as_vec2();
    assert!((diag - Vec2::new(1.0, 1.0).normalize()).length() < 1e-5);
    assert!(
        set.update(&frame(&["w", "d"], &["w", "d"]), 0.2).is_empty(),
        "no change"
    );
    let ev = set.update(&frame(&["w", "d"], &[]), 0.3);
    assert_eq!(phases(&ev), [Phase::Canceled]);
    assert_eq!(ev[0].value, Value::Vector2(Vec2::ZERO));

    let mut set = one_action(
        Action::new("Build", ActionType::Button, ControlType::Button).with(Binding::composite(
            "",
            "ButtonWithOneModifier",
            &[("modifier", "<Keyboard>/shift"), ("button", "<Keyboard>/b")],
            KEYBOARD_MOUSE,
        )),
    );
    assert!(set.update(&frame(&[], &["b"]), 0.0).is_empty(), "B alone");
    set.update(&frame(&["b"], &[]), 0.1);
    let ev = set.update(&frame(&[], &["left shift", "b"]), 0.2);
    assert_eq!(phases(&ev), [Phase::Started, Phase::Performed]);
    assert_eq!(set.display_string(0, 0, None), "Shift+B");
}

// ---- processors ------------------------------------------------------------------------------

#[test]
fn processors_filter_values_in_order() {
    let f = |text: &str, v: f32| processors::apply(text, Value::Float(v)).as_float();
    let v2 = |text: &str, v: Vec2| processors::apply(text, Value::Vector2(v)).as_vec2();
    assert_eq!(f("invert", 0.5), -0.5);
    assert_eq!(f("scale(factor=3)", 0.5), 1.5);
    assert_eq!(f("invert,scale(factor=2)", 0.25), -0.5);
    assert_eq!(f("normalize(min=0,max=10)", 5.0), 0.5);
    assert_eq!(f("normalize(min=-1,max=1,zero=0)", -0.5), -0.5);
    assert_eq!(f("clamp(min=-1,max=0.5)", 0.9), 0.5);
    assert_eq!(f("axisDeadzone", 0.1), 0.0);
    assert_eq!(f("axisDeadzone", 0.95), 1.0);
    assert!((f("axisDeadzone(min=0.2,max=0.8)", -0.5) + 0.5).abs() < 1e-5);
    assert_eq!(
        v2("invertVector2(invertX=false)", Vec2::new(1.0, 2.0)),
        Vec2::new(1.0, -2.0)
    );
    assert_eq!(
        v2("scaleVector2(x=2,y=3)", Vec2::new(1.0, 1.0)),
        Vec2::new(2.0, 3.0)
    );
    assert!((v2("normalizeVector2", Vec2::new(3.0, 4.0)).length() - 1.0).abs() < 1e-6);
    // The stick dead zone keeps the direction and rescales the length.
    assert_eq!(v2("stickDeadzone", Vec2::new(0.1, 0.0)), Vec2::ZERO);
    let s = v2("stickDeadzone(min=0.2,max=0.8)", Vec2::new(0.0, 0.5));
    assert!((s - Vec2::new(0.0, 0.5)).length() < 1e-5, "{s}");
    assert_eq!(
        v2("stickDeadzone", Vec2::new(0.0, -1.0)),
        Vec2::new(0.0, -1.0)
    );
    // Unknown names leave values alone; lists round-trip.
    assert_eq!(f("wobble", 0.3), 0.3);
    let list = parse_list("hold(duration=0.5,pressPoint=0.3), tap ,scale(factor=2)");
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].num("duration", 0.0), 0.5);
    assert_eq!(list[1].name, "tap");
    assert_eq!(
        super::value::list_to_text(&list),
        "hold(duration=0.5,pressPoint=0.3),tap,scale(factor=2)"
    );
}

#[test]
fn binding_and_action_processors_apply_on_the_way() {
    let mut b = Binding::to("<Gamepad>/leftStick/y", GAMEPAD);
    b.processors = "invert".into();
    let mut action = Action::new("Throttle", ActionType::Value, ControlType::Axis).with(b);
    action.processors = "scale(factor=2)".into();
    let mut set = one_action(action);
    let mut i = Input::default();
    let mut j = pad("Pad");
    j.axes[1] = -0.5; // stick pushed up half way: y = +0.5
    i.joysticks = vec![j];
    set.update(&i, 0.0);
    assert_eq!(set.state(0, 0).unwrap().value, Value::Float(-1.0));
}

// ---- interactions ----------------------------------------------------------------------------

#[test]
fn hold_tap_slow_tap_and_multi_tap_on_a_simulated_clock() {
    let hold = Interaction::parse_list("hold(duration=0.5)").remove(0);
    let mut s = InteractionState::default();
    assert_eq!(s.step(&hold, 1.0, 0.0), [Transition::Started]);
    assert!(s.step(&hold, 1.0, 0.4).is_empty());
    assert_eq!(s.step(&hold, 1.0, 0.5), [Transition::Performed]);
    assert!(s.step(&hold, 1.0, 0.9).is_empty(), "stays performed");
    assert_eq!(s.step(&hold, 0.0, 1.0), [Transition::Canceled]);
    // Released early: canceled.
    assert_eq!(s.step(&hold, 1.0, 2.0), [Transition::Started]);
    assert_eq!(s.step(&hold, 0.0, 2.2), [Transition::Canceled]);

    let tap = Interaction::parse_list("tap").remove(0);
    let mut s = InteractionState::default();
    s.step(&tap, 1.0, 0.0);
    assert_eq!(s.step(&tap, 0.0, 0.1), [Transition::Performed]);
    s.step(&tap, 1.0, 1.0);
    // Held past the tap time: canceled without waiting for the release.
    assert_eq!(s.step(&tap, 1.0, 1.3), [Transition::Canceled]);
    assert!(s.step(&tap, 0.0, 1.4).is_empty());

    let slow = Interaction::parse_list("slowTap(duration=0.5)").remove(0);
    let mut s = InteractionState::default();
    s.step(&slow, 1.0, 0.0);
    assert_eq!(s.step(&slow, 0.0, 0.3), [Transition::Canceled]);
    s.step(&slow, 1.0, 1.0);
    assert_eq!(s.step(&slow, 0.0, 1.6), [Transition::Performed]);

    let double = Interaction::parse_list("multiTap(tapCount=2,tapTime=0.2,tapDelay=0.5)").remove(0);
    let mut s = InteractionState::default();
    assert_eq!(s.step(&double, 1.0, 0.0), [Transition::Started]);
    assert!(s.step(&double, 0.0, 0.1).is_empty());
    assert!(s.step(&double, 1.0, 0.3).is_empty());
    assert_eq!(s.step(&double, 0.0, 0.4), [Transition::Performed]);
    // Too slow between taps.
    s.step(&double, 1.0, 1.0);
    s.step(&double, 0.0, 1.1);
    assert_eq!(s.step(&double, 0.0, 1.7), [Transition::Canceled]);

    let release = Interaction::parse_list("press(behavior=ReleaseOnly)").remove(0);
    let mut s = InteractionState::default();
    assert_eq!(s.step(&release, 1.0, 0.0), [Transition::Started]);
    assert_eq!(s.step(&release, 0.0, 0.5), [Transition::Performed]);
    let both = Interaction::parse_list("press(behavior=PressAndRelease)").remove(0);
    let mut s = InteractionState::default();
    assert_eq!(
        s.step(&both, 1.0, 0.0),
        [Transition::Started, Transition::Performed]
    );
    assert_eq!(s.step(&both, 0.0, 0.5), [Transition::Performed]);
}

#[test]
fn hold_and_tap_on_one_binding_hand_over() {
    let mut b = Binding::to("<Keyboard>/e", KEYBOARD_MOUSE);
    b.interactions = "hold(duration=0.4),tap(duration=0.2)".into();
    let mut set = one_action(Action::new("Use", ActionType::Button, ControlType::Button).with(b));
    // A quick press: hold starts, tap takes over and performs.
    let ev = set.update(&frame(&[], &["e"]), 0.0);
    assert_eq!(phases(&ev), [Phase::Started]);
    assert_eq!(ev[0].interaction, Some("Hold"));
    let ev = set.update(&frame(&["e"], &[]), 0.1);
    assert_eq!(phases(&ev), [Phase::Performed]);
    assert_eq!(ev[0].interaction, Some("Tap"));
    // A long press: hold performs at 0.4 s, canceled on release.
    set.update(&frame(&[], &["e"]), 1.0);
    let mut seen = Vec::new();
    for k in 1..=6 {
        seen.extend(set.update(&frame(&["e"], &["e"]), 1.0 + f64::from(k) * 0.1));
    }
    assert_eq!(phases(&seen), [Phase::Performed]);
    assert_eq!(seen[0].interaction, Some("Hold"));
    assert!((seen[0].time - seen[0].start_time - 0.4).abs() < 1e-9);
    let ev = set.update(&frame(&["e"], &[]), 1.7);
    assert_eq!(phases(&ev), [Phase::Canceled]);
}

// ---- action types ----------------------------------------------------------------------------

#[test]
fn button_value_and_pass_through_actions() {
    let mut jump = one_action(
        Action::new("Jump", ActionType::Button, ControlType::Button)
            .with(Binding::to("<Keyboard>/space", KEYBOARD_MOUSE))
            .with(Binding::to("<Gamepad>/buttonSouth", GAMEPAD)),
    );
    let ev = jump.update(&frame(&[], &["space"]), 0.0);
    assert_eq!(phases(&ev), [Phase::Started, Phase::Performed]);
    let st = jump.state(0, 0).unwrap();
    assert!(st.pressed && st.pressed_this_frame && st.performed_this_frame);
    assert_eq!(st.active_control.as_deref(), Some("<Keyboard>/space"));
    assert!(jump.update(&frame(&["space"], &["space"]), 0.1).is_empty());
    assert!(!jump.state(0, 0).unwrap().pressed_this_frame);
    let ev = jump.update(&frame(&["space"], &[]), 0.2);
    assert_eq!(phases(&ev), [Phase::Canceled]);
    assert!(jump.state(0, 0).unwrap().released_this_frame);
    // An analog trigger crossing the press point.
    let mut trigger = one_action(
        Action::new("Fire", ActionType::Button, ControlType::Button)
            .with(Binding::to("<Gamepad>/rightTrigger", GAMEPAD)),
    );
    let mut i = Input::default();
    let mut j = pad("Pad");
    j.axes[9] = 0.2;
    i.joysticks = vec![j.clone()];
    assert_eq!(phases(&trigger.update(&i, 0.0)), [Phase::Started]);
    i.joysticks[0].axes[9] = 0.8;
    assert_eq!(phases(&trigger.update(&i, 0.1)), [Phase::Performed]);
    i.joysticks[0].axes[9] = 0.0;
    assert_eq!(phases(&trigger.update(&i, 0.2)), [Phase::Canceled]);

    // Pass-through: every change of every control, no start or cancel.
    let mut point = one_action(
        Action::new("Point", ActionType::PassThrough, ControlType::Vector2)
            .with(Binding::to("<Mouse>/delta", KEYBOARD_MOUSE)),
    );
    let mut i = Input {
        mouse_delta: Vec2::new(1.0, 0.0),
        ..Default::default()
    };
    assert_eq!(phases(&point.update(&i, 0.0)), [Phase::Performed]);
    i.mouse_delta = Vec2::ZERO;
    let ev = point.update(&i, 0.1);
    assert_eq!(phases(&ev), [Phase::Performed]);
    assert_eq!(ev[0].value, Value::Vector2(Vec2::ZERO));

    // The most actuated control drives a value action.
    let mut move_ = one_action(
        Action::new("Move", ActionType::Value, ControlType::Vector2)
            .with(Binding::to("<Gamepad>/leftStick", GAMEPAD))
            .with(Binding::to("<Gamepad>/rightStick", GAMEPAD)),
    );
    let mut i = Input::default();
    let mut j = pad("Pad");
    j.axes[0] = 0.3;
    j.axes[3] = -0.9;
    i.joysticks = vec![j];
    move_.update(&i, 0.0);
    assert_eq!(
        move_.state(0, 0).unwrap().value,
        Value::Vector2(Vec2::new(-0.9, 0.0))
    );

    // Disabling an action in progress cancels it on the next update.
    jump.update(&frame(&[], &["space"]), 1.0);
    jump.set_action_enabled(0, 0, false);
    assert_eq!(jump.state(0, 0).unwrap().phase, Phase::Disabled);
    let ev = jump.update(&frame(&["space"], &["space"]), 1.1);
    assert_eq!(phases(&ev), [Phase::Canceled]);
}

#[test]
fn a_button_held_while_its_action_is_enabled_is_not_a_press() {
    let mut set = one_action(
        Action::new("Menu", ActionType::Button, ControlType::Button)
            .with(Binding::to("<Keyboard>/tab", KEYBOARD_MOUSE)),
    );
    set.set_all_enabled(false);
    let held = frame(&["tab"], &["tab"]);
    set.update_from(&held, Some(&held), 0.0);
    set.set_all_enabled(true);
    let ev = set.update_from(&held, Some(&held), 0.1);
    assert!(ev.is_empty(), "{ev:?}");
    assert!(set.state(0, 0).unwrap().pressed && !set.state(0, 0).unwrap().pressed_this_frame);
    set.update_from(&frame(&["tab"], &[]), Some(&held), 0.2);
    let ev = set.update_from(&frame(&[], &["tab"]), Some(&keys(&[])), 0.3);
    assert_eq!(phases(&ev), [Phase::Started, Phase::Performed]);
    // Pressed on the frame it was enabled (not held before): that is a press (after the
    // cancel of the press the disable interrupted).
    set.set_all_enabled(false);
    set.set_all_enabled(true);
    let ev = set.update_from(&frame(&[], &["tab"]), Some(&keys(&[])), 0.4);
    assert_eq!(
        phases(&ev),
        [Phase::Canceled, Phase::Started, Phase::Performed]
    );
}

#[test]
fn binding_mask_keeps_to_the_control_scheme() {
    let mut set = one_action(
        Action::new("Jump", ActionType::Button, ControlType::Button)
            .with(Binding::to("<Keyboard>/space", KEYBOARD_MOUSE))
            .with(Binding::to("<Gamepad>/buttonSouth", GAMEPAD)),
    );
    set.binding_mask = Some(GAMEPAD.into());
    assert!(set.update(&frame(&[], &["space"]), 0.0).is_empty());
    let mut i = Input::default();
    let mut j = pad("Pad");
    j.held[0] = true;
    i.joysticks = vec![j];
    assert_eq!(
        phases(&set.update(&i, 0.1)),
        [Phase::Started, Phase::Performed]
    );
    assert_eq!(set.display_string(0, 0, None), "Button South");
    set.binding_mask = None;
    assert_eq!(set.display_string(0, 0, None), "Space | Button South");
}

// ---- assets ----------------------------------------------------------------------------------

#[test]
fn starter_asset_round_trips_and_ids_are_stable() {
    let a = InputActionAsset::starter("Player");
    assert_eq!(a.maps.len(), 2);
    assert_eq!(a.find_action("Player/Move"), Some((0, 0)));
    assert_eq!(a.find_action("jump"), Some((0, 3)));
    assert_eq!(a.find_action("UI/Submit"), Some((1, 1)));
    let text = a.to_json();
    let back = InputActionAsset::from_json(&text).unwrap();
    assert_eq!(back, a);
    // Ids filled on load are the same each time.
    let mut bare = a.clone();
    for m in &mut bare.maps {
        for act in &mut m.actions {
            for b in &mut act.bindings {
                b.id.clear();
            }
        }
    }
    let t = bare.to_json();
    assert_eq!(
        InputActionAsset::from_json(&t).unwrap(),
        InputActionAsset::from_json(&t).unwrap()
    );
    let flats = a.maps[0].actions[0].flat_bindings();
    // WASD + 4 parts, Arrows + 4 parts, the stick.
    assert_eq!(flats.len(), 11);
    assert!(is_actions_path("Assets/Input/Player.inputactions"));
}

// ---- rebinding and persistence -----------------------------------------------------------------

#[test]
fn rebinding_overrides_save_to_prefs_and_load_back() {
    let _guard = crate::prefs::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let asset = InputActionAsset::starter("Player");
    let mut sys = InputSystem::new(std::path::Path::new("."), Default::default());
    let set = sys.add_set(ActionSet::new(asset.clone(), ""));
    sys.set_mut(set).unwrap().set_all_enabled(true);
    let scene = Scene::new("t");
    let (m, a) = asset.find_action("Player/Jump").unwrap();
    let id = sys.new_rebind(set, m, a, Some(0)).unwrap();
    sys.start_rebind(id);
    assert!(
        !sys.set(set).unwrap().state(m, a).unwrap().enabled,
        "disabled while listening"
    );
    // The frame Start was called in doesn't count; the next key press does.
    let ev = sys.update(&scene, &frame(&[], &["j"]), 0.016);
    assert_eq!(
        ev.iter()
            .filter(|e| matches!(e, SystemEvent::RebindComplete { .. }))
            .count(),
        1,
        "{ev:?}"
    );
    assert!(ev.contains(&SystemEvent::RebindComplete {
        id,
        path: "<Keyboard>/j".into()
    }));
    // The press that picked J doesn't jump: the action comes back once J is let go.
    assert!(
        !ev.iter().any(|e| matches!(e, SystemEvent::Action { .. })),
        "{ev:?}"
    );
    assert!(!sys.set(set).unwrap().state(m, a).unwrap().enabled);
    sys.update(&scene, &frame(&["j"], &["j"]), 0.016);
    assert!(
        !sys.set(set).unwrap().state(m, a).unwrap().enabled,
        "J still held"
    );
    let s = sys.set(set).unwrap();
    assert_eq!(s.effective_path(m, a, 0).as_deref(), Some("<Keyboard>/j"));
    assert_eq!(s.display_string(m, a, Some(0)), "J");
    // J jumps now, Space doesn't.
    let ev = sys.update(&scene, &frame(&["j"], &[]), 0.016);
    assert!(!ev.iter().any(|e| matches!(e, SystemEvent::Action { .. })));
    assert!(
        sys.set(set).unwrap().state(m, a).unwrap().enabled,
        "enabled again"
    );
    let ev = sys.update(&scene, &frame(&[], &["space"]), 0.016);
    assert!(!ev.iter().any(|e| matches!(e, SystemEvent::Action { .. })));
    let ev = sys.update(&scene, &frame(&["space"], &["j"]), 0.016);
    assert!(ev.iter().any(
        |e| matches!(e, SystemEvent::Action { event, .. } if event.phase == Phase::Performed)
    ));

    // Escape cancels; the binding stays.
    let id2 = sys.new_rebind(set, m, a, Some(0)).unwrap();
    sys.start_rebind(id2);
    let ev = sys.update(&scene, &frame(&[], &["escape"]), 0.016);
    assert!(ev.contains(&SystemEvent::RebindCanceled { id: id2 }));
    // Excluded controls are skipped.
    let id3 = sys.new_rebind(set, m, a, Some(0)).unwrap();
    sys.rebind_mut(id3)
        .unwrap()
        .excluded
        .push("<Keyboard>/k".into());
    sys.start_rebind(id3);
    let ev = sys.update(&scene, &frame(&[], &["k"]), 0.016);
    assert!(
        ev.iter()
            .all(|e| !matches!(e, SystemEvent::RebindComplete { .. }))
    );
    sys.cancel_rebind(id3);

    // Save to PlayerPrefs, reopen the prefs file, load into a fresh copy.
    let json = sys.set(set).unwrap().overrides_json();
    assert!(
        json.contains("<Keyboard>/j") && json.contains("Player/Jump"),
        "{json}"
    );
    let file = std::env::temp_dir().join(format!("misty_rebind_prefs_{}.json", std::process::id()));
    let _ = std::fs::remove_file(&file);
    crate::prefs::open(&file);
    crate::prefs::set("rebinds", crate::prefs::PrefValue::String(json));
    crate::prefs::save().unwrap();
    crate::prefs::open(&file);
    let saved = crate::prefs::get_string("rebinds", "");
    let mut fresh = ActionSet::new(asset, "");
    assert_eq!(fresh.load_overrides_json(&saved), Ok(1));
    assert_eq!(
        fresh.effective_path(m, a, 0).as_deref(),
        Some("<Keyboard>/j")
    );
    fresh.clear_overrides(Some((m, a)));
    assert_eq!(
        fresh.effective_path(m, a, 0).as_deref(),
        Some("<Keyboard>/space")
    );
    let _ = std::fs::remove_file(&file);
}

#[test]
fn rebinding_a_composite_part_and_a_stick() {
    let asset = InputActionAsset::starter("Player");
    let mut sys = InputSystem::new(std::path::Path::new("."), Default::default());
    let set = sys.add_set(ActionSet::new(asset.clone(), ""));
    let scene = Scene::new("t");
    // Move: flat 1 is WASD's "up".
    let id = sys.new_rebind(set, 0, 0, Some(1)).unwrap();
    assert!(!sys.rebind(id).unwrap().expect_vector);
    sys.start_rebind(id);
    let ev = sys.update(&scene, &frame(&[], &["i"]), 0.016);
    assert!(ev.contains(&SystemEvent::RebindComplete {
        id,
        path: "<Keyboard>/i".into()
    }));
    assert_eq!(
        sys.set(set).unwrap().display_string(0, 0, Some(0)),
        "I/S/A/D"
    );
    // A composite itself can't be rebound.
    assert!(sys.new_rebind(set, 0, 0, Some(0)).is_err());
    // The stick binding wants a whole stick.
    let id = sys.new_rebind(set, 0, 0, Some(10)).unwrap();
    assert!(sys.rebind(id).unwrap().expect_vector);
    sys.start_rebind(id);
    let mut i = Input::default();
    let mut j = pad("Pad");
    j.axes[3] = 0.9;
    i.joysticks = vec![j];
    let ev = sys.update(&scene, &i, 0.016);
    assert!(ev.contains(&SystemEvent::RebindComplete {
        id,
        path: "<Gamepad>/rightStick".into()
    }));
}

// ---- players -----------------------------------------------------------------------------------

fn players_project(n: usize) -> (std::path::PathBuf, Scene, Vec<hecs::Entity>) {
    let root = std::env::temp_dir().join(format!(
        "misty_players_{n}_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    InputActionAsset::starter("Player")
        .save(&root.join("Assets/Player.inputactions"))
        .unwrap();
    let mut scene = Scene::new("t");
    let players = (0..n)
        .map(|i| {
            let e = scene.spawn(&format!("Player {i}"), None);
            scene.add(
                e,
                PlayerInput {
                    actions: Some("Assets/Player.inputactions".into()),
                    ..Default::default()
                },
            );
            e
        })
        .collect();
    (root, scene, players)
}

#[test]
fn single_player_auto_switches_control_scheme() {
    let (root, scene, players) = players_project(1);
    let mut sys = InputSystem::new(&root, Default::default());
    let mut i = Input {
        joysticks: vec![pad("Pad")],
        ..Default::default()
    };
    sys.update(&scene, &i, 0.016);
    let p = sys.player(players[0]).unwrap().clone();
    assert_eq!(p.scheme.as_deref(), Some(KEYBOARD_MOUSE));
    assert!(p.devices.contains(&DeviceId::Gamepad(0)));
    assert_eq!(p.map, Some(0), "default map: the first");
    // Pressing a gamepad button switches to the Gamepad scheme.
    i.joysticks[0].held[0] = true;
    i.joysticks[0].pressed[0] = true;
    let ev = sys.update(&scene, &i, 0.016);
    assert!(ev.contains(&SystemEvent::ControlsChanged(players[0])));
    assert_eq!(
        sys.player(players[0]).unwrap().scheme.as_deref(),
        Some(GAMEPAD)
    );
    assert!(ev.iter().any(
        |e| matches!(e, SystemEvent::Action { event, .. } if event.phase == Phase::Performed)
    ));
    // Keys switch back.
    let ev = sys.update(&scene, &frame(&[], &["space"]), 0.016);
    assert!(ev.contains(&SystemEvent::ControlsChanged(players[0])));
    assert_eq!(
        sys.player(players[0]).unwrap().scheme.as_deref(),
        Some(KEYBOARD_MOUSE)
    );
    // Switching maps.
    sys.switch_map(players[0], "UI").unwrap();
    let s = sys.set(p.set).unwrap();
    assert!(!s.map_enabled(0) && s.map_enabled(1));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn local_multiplayer_pairs_separate_devices() {
    let (root, scene, players) = players_project(3);
    let mut sys = InputSystem::new(&root, Default::default());
    let mut i = Input {
        joysticks: vec![pad("Pad A"), pad("Pad B")],
        ..Default::default()
    };
    sys.update(&scene, &i, 0.016);
    let p: Vec<_> = players
        .iter()
        .map(|e| sys.player(*e).unwrap().clone())
        .collect();
    assert_eq!(p[0].scheme.as_deref(), Some(KEYBOARD_MOUSE));
    assert_eq!(p[0].devices, [DeviceId::Keyboard, DeviceId::Mouse]);
    assert_eq!(p[1].scheme.as_deref(), Some(GAMEPAD));
    assert_eq!(p[1].devices, [DeviceId::Gamepad(0)]);
    assert_eq!(p[2].devices, [DeviceId::Gamepad(1)]);
    assert_eq!((p[0].index, p[1].index, p[2].index), (0, 1, 2));
    // Each player has its own copy of the asset.
    assert!(p[0].set != p[1].set && p[1].set != p[2].set);
    // Pad B's A button only jumps for player 3.
    i.joysticks[1].held[0] = true;
    i.joysticks[1].pressed[0] = true;
    let ev = sys.update(&scene, &i, 0.016);
    let jumped: Vec<usize> = ev
        .iter()
        .filter_map(|e| match e {
            SystemEvent::Action { set, event } if event.phase == Phase::Performed => Some(*set),
            _ => None,
        })
        .collect();
    assert_eq!(jumped, [p[2].set]);
    // Unplugging pad A: player 2 loses it, gets it back on reconnect.
    i.joysticks[1].pressed[0] = false;
    i.joysticks[0].name.clear();
    let ev = sys.update(&scene, &i, 0.016);
    assert!(ev.contains(&SystemEvent::DeviceLost(players[1])));
    i.joysticks[0].name = "Pad A".into();
    let ev = sys.update(&scene, &i, 0.016);
    assert!(ev.contains(&SystemEvent::DeviceRegained(players[1])));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn rumble_requests_reach_the_host_once() {
    let mut sys = InputSystem::new(std::path::Path::new("."), Default::default());
    assert_eq!(sys.take_rumble(), None);
    sys.set_motor_speeds(Some(1), 0.25, 2.0);
    assert_eq!(sys.take_rumble(), Some(vec![(1, 0.25, 1.0)]));
    assert_eq!(sys.take_rumble(), None, "unchanged");
    sys.set_motor_speeds(Some(1), 0.25, 1.0);
    assert_eq!(sys.take_rumble(), None);
    sys.reset_haptics();
    assert_eq!(sys.take_rumble(), Some(vec![(1, 0.0, 0.0)]));
}

#[test]
fn input_manager_only_keeps_actions_quiet() {
    let mut sys = InputSystem::new(
        std::path::Path::new("."),
        super::ActiveInputHandling::InputManager,
    );
    let asset = InputActionAsset::starter("Player");
    let set = sys.add_set(ActionSet::new(asset, ""));
    sys.set_mut(set).unwrap().set_all_enabled(true);
    let ev = sys.update(&Scene::new("t"), &frame(&[], &["space"]), 0.016);
    assert!(ev.is_empty());
}

#[test]
fn respawned_players_do_not_pile_up_action_sets() {
    let (root, mut scene, players) = players_project(2);
    let mut sys = InputSystem::new(&root, Default::default());
    let i = Input::default();
    sys.update(&scene, &i, 0.016);
    assert_eq!(sys.live_sets(), 2);
    for _ in 0..20 {
        scene.game_object_mut(players[1]).active = false;
        sys.update(&scene, &i, 0.016);
        scene.game_object_mut(players[1]).active = true;
        sys.update(&scene, &i, 0.016);
    }
    assert!(sys.player(players[1]).is_some());
    assert_eq!(sys.live_sets(), 2);
    // The first player's shared copy stays when it goes and comes back.
    scene.game_object_mut(players[0]).active = false;
    sys.update(&scene, &i, 0.016);
    scene.game_object_mut(players[0]).active = true;
    sys.update(&scene, &i, 0.016);
    assert!(sys.live_sets() <= 3);
    let _ = std::fs::remove_dir_all(&root);
}
