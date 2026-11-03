use gilrs::{Axis, Button};

use super::*;
use crate::scripting::Input;

fn held(keys: &[&str]) -> Input {
    let mut i = Input::default();
    for k in keys {
        i.held.insert((*k).into());
    }
    i
}

fn run(m: &mut InputManager, input: &Input, frames: usize) {
    for _ in 0..frames {
        m.update(input, 0.1);
    }
}

#[test]
fn key_axes_ramp_up_with_sensitivity_and_fall_with_gravity() {
    let mut m = InputManager::new(&InputSettings::default());
    let right = held(&["d"]);
    run(&mut m, &right, 1);
    // Sensitivity 3/s: 0.3 after 0.1 s, raw jumps straight to 1.
    assert!((m.axis("Horizontal").unwrap() - 0.3).abs() < 1e-5);
    assert_eq!(m.axis_raw("Horizontal"), Some(1.0));
    run(&mut m, &right, 5);
    assert_eq!(m.axis("Horizontal"), Some(1.0), "clamped at the target");
    run(&mut m, &Input::default(), 2);
    assert!(
        (m.axis("Horizontal").unwrap() - 0.4).abs() < 1e-5,
        "gravity 3/s"
    );
    assert_eq!(m.axis_raw("Horizontal"), Some(0.0));
    // Snap: pressing the other way restarts from 0.
    run(&mut m, &held(&["left"]), 1);
    assert!((m.axis("Horizontal").unwrap() + 0.3).abs() < 1e-5);
    // Both directions cancel out.
    run(&mut m, &held(&["a", "d"]), 1);
    assert_eq!(m.axis_raw("Horizontal"), Some(0.0));
    assert_eq!(m.axis("Missing"), None);
}

#[test]
fn invert_and_no_snap() {
    let mut s = InputSettings::default();
    let h = &mut s.axes[0];
    h.invert = true;
    h.snap = false;
    let mut m = InputManager::new(&s);
    run(&mut m, &held(&["right"]), 2);
    assert!((m.axis("Horizontal").unwrap() + 0.6).abs() < 1e-5);
    run(&mut m, &held(&["left"]), 1);
    assert!(
        (m.axis("Horizontal").unwrap() + 0.3).abs() < 1e-5,
        "glides back"
    );
}

#[test]
fn buttons_and_mouse_axes() {
    let m = InputManager::new(&InputSettings::default());
    let mut i = Input::default();
    i.mouse_held[0] = true;
    i.mouse_pressed[0] = true;
    i.pressed.insert("space".into());
    i.held.insert("space".into());
    let held = |i: &Input, b: &str| i.button_held(b);
    let down = |i: &Input, b: &str| i.button_pressed(b);
    assert_eq!(
        m.button("Fire1", &i, held),
        Some(true),
        "alt button mouse 0"
    );
    assert_eq!(m.button("Fire1", &i, down), Some(true));
    assert_eq!(m.button("Fire2", &i, held), Some(false));
    assert_eq!(m.button("Jump", &i, down), Some(true));
    assert_eq!(m.button("Nope", &i, held), None);

    let mut m = InputManager::new(&InputSettings::default());
    let mut i = Input {
        mouse_delta: glam::Vec2::new(20.0, -10.0),
        scroll_delta: glam::Vec2::new(0.0, 100.0),
        ..Default::default()
    };
    m.update(&i, 0.016);
    assert!((m.axis("Mouse X").unwrap() - 2.0).abs() < 1e-5);
    assert!(
        (m.axis("Mouse Y").unwrap() - 1.0).abs() < 1e-5,
        "up is positive"
    );
    assert!(
        (m.axis("Mouse ScrollWheel").unwrap() - 0.2).abs() < 1e-5,
        "two notches"
    );
    i.mouse_delta = glam::Vec2::ZERO;
    m.update(&i, 0.016);
    assert_eq!(m.axis("Mouse X"), Some(0.0));
}

#[test]
fn button_names_cover_mouse_and_joysticks() {
    assert_eq!(ButtonName::parse("Mouse 1"), Some(ButtonName::Mouse(1)));
    assert_eq!(
        ButtonName::parse("joystick button 3"),
        Some(ButtonName::Joystick {
            joystick: None,
            button: 3
        })
    );
    assert_eq!(
        ButtonName::parse("joystick 2 button 0"),
        Some(ButtonName::Joystick {
            joystick: Some(1),
            button: 0
        })
    );
    assert_eq!(
        ButtonName::parse("left shift"),
        Some(ButtonName::Key("left shift".into()))
    );
    assert_eq!(ButtonName::parse(" "), None);

    let mut pad = Joystick::default();
    pad.held[3] = true;
    pad.pressed[3] = true;
    let i = Input {
        joysticks: vec![Joystick::default(), pad],
        ..Default::default()
    };
    assert!(i.button_held("joystick button 3"));
    assert!(i.button_held("joystick 2 button 3"));
    assert!(!i.button_held("joystick 1 button 3"));
    assert!(i.button_pressed("joystick button 3") && !i.button_released("joystick button 3"));
    assert!(i.any_key() && i.any_key_down());
    assert!(!Input::default().any_key());
}

/// A fake pad: left stick pushed up-right, A held, left trigger half-way.
fn fake_pad() -> Joystick {
    read_pad_with(&[Button::South, Button::DPadLeft], 0.5)
}

fn read_pad_with(buttons: &[Button], trigger: f32) -> Joystick {
    gamepad::read_pad(
        "Fake Pad",
        |b| buttons.contains(&b),
        |a| match a {
            Axis::LeftStickX => 0.8,
            Axis::LeftStickY => 0.6,
            Axis::RightStickY => -1.0,
            _ => 0.0,
        },
        |b| {
            if b == Button::LeftTrigger2 {
                trigger
            } else {
                0.0
            }
        },
    )
}

#[test]
fn gamepads_map_to_the_joystick_layout() {
    let j = fake_pad();
    assert_eq!(j.name, "Fake Pad");
    assert!(j.held[0] && !j.held[1] && j.held[13]);
    assert_eq!(gamepad::button_index(Button::Start), Some(7));
    assert_eq!(gamepad::button_index(Button::Unknown), None);
    assert_eq!(j.axes[0], 0.8);
    assert_eq!(j.axes[1], -0.6, "joystick Y is +1 down");
    assert_eq!(j.axes[4], 1.0);
    assert_eq!(j.axes[5], -1.0, "d-pad X from the buttons");
    assert_eq!(j.axes[2], 0.5);
    assert_eq!(j.axes[8], 0.5);

    // Presses come from comparing with the previous frame.
    let mut first = fake_pad();
    gamepad::diff(None, &mut first);
    assert!(first.pressed[0]);
    let mut second = read_pad_with(&[], 0.0);
    gamepad::diff(Some(&first), &mut second);
    assert!(!second.pressed[0] && second.released[0]);
}

#[test]
fn default_axes_read_gamepads() {
    let mut m = InputManager::new(&InputSettings::default());
    let mut pad = fake_pad();
    gamepad::diff(None, &mut pad);
    let i = Input {
        joysticks: vec![pad],
        ..Default::default()
    };
    m.update(&i, 0.016);
    assert_eq!(m.axis("Horizontal"), Some(0.8));
    assert_eq!(m.axis("Vertical"), Some(0.6), "inverted: stick up = +1");
    assert_eq!(
        m.button("Fire1", &i, |i, b| i.button_pressed(b)),
        Some(true),
        "A = joystick button 0"
    );
    // Inside the dead zone reads 0.
    let mut small = Joystick::default();
    small.axes[0] = 0.1;
    let i = Input {
        joysticks: vec![small],
        ..Default::default()
    };
    m.update(&i, 0.016);
    assert_eq!(m.axis_raw("Horizontal"), Some(0.0));
}

#[test]
fn settings_round_trip_and_sanitize() {
    let mut s = InputSettings::default();
    s.axes[0].gravity = f32::NAN;
    s.axes[1].axis = 99;
    s.sanitize();
    assert_eq!(s.axes[0].gravity, 0.0);
    assert_eq!(s.axes[1].axis, axes::JOYSTICK_AXES - 1);
    let json = serde_json::to_string(&s).unwrap();
    let back: InputSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
    // A partial entry fills the rest with defaults.
    let one: InputSettings =
        serde_json::from_str(r#"{"axes":[{"name":"Dash","positive_button":"e"}]}"#).unwrap();
    assert_eq!(one.axes[0].sensitivity, 1000.0);
    assert_eq!(one.axes[0].kind, AxisType::KeyOrMouseButton);
}
