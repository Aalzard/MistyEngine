//! Project Settings ▸ Input Manager: the list of virtual axes and buttons scripts read with
//! `Input.GetAxis("Horizontal")` / `Input.GetButton("Jump")`.

use serde::{Deserialize, Serialize};

/// Where an axis gets its value from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AxisType {
    /// Negative/positive buttons (keys, `mouse 0`, `joystick button 0`), smoothed by
    /// gravity and sensitivity.
    #[default]
    KeyOrMouseButton,
    /// Mouse delta: axis 0 = X, 1 = Y, 2 = scroll wheel.
    MouseMovement,
    /// A stick, trigger or d-pad axis of a joystick.
    JoystickAxis,
}

impl AxisType {
    pub const ALL: [AxisType; 3] = [
        AxisType::KeyOrMouseButton,
        AxisType::MouseMovement,
        AxisType::JoystickAxis,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AxisType::KeyOrMouseButton => "Key or Mouse Button",
            AxisType::MouseMovement => "Mouse Movement",
            AxisType::JoystickAxis => "Joystick Axis",
        }
    }
}

/// Joystick axes a joystick reports (see `gamepad` for the layout).
pub const JOYSTICK_AXES: usize = 10;
/// Joystick buttons (`joystick button 0` … `joystick button 19`).
pub const JOYSTICK_BUTTONS: usize = 20;
/// Joysticks `joystick 1` … `joystick 8` (0 = any joystick).
pub const MAX_JOYSTICKS: usize = 8;

/// Label of a 0-based axis index ("X axis", "Y axis", "3rd axis", …).
pub fn axis_label(i: usize) -> String {
    match i {
        0 => "X axis".into(),
        1 => "Y axis".into(),
        2 => "3rd axis (Joysticks and Scrollwheel)".into(),
        n => format!("{}th axis (Joysticks)", n + 1),
    }
}

/// One entry of the Input Manager's axes list. Several entries may share a name; the one
/// with the largest value wins (that's how keyboard and gamepad both drive "Horizontal").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputAxis {
    pub name: String,
    pub descriptive_name: String,
    pub descriptive_negative_name: String,
    pub negative_button: String,
    pub positive_button: String,
    pub alt_negative_button: String,
    pub alt_positive_button: String,
    /// Units per second the value falls back to 0 when no button is held.
    pub gravity: f32,
    /// Analog values smaller than this read as 0.
    pub dead: f32,
    /// Units per second the value moves towards the target (key axes), or a multiplier
    /// (mouse and joystick axes).
    pub sensitivity: f32,
    /// Pressing the opposite direction jumps to 0 first.
    pub snap: bool,
    pub invert: bool,
    #[serde(rename = "type")]
    pub kind: AxisType,
    /// 0-based axis index (mouse: 0 X, 1 Y, 2 wheel; joystick: see `gamepad`).
    pub axis: usize,
    /// 0 = any joystick, 1… = that joystick.
    pub joy_num: usize,
}

impl Default for InputAxis {
    fn default() -> Self {
        Self {
            name: String::new(),
            descriptive_name: String::new(),
            descriptive_negative_name: String::new(),
            negative_button: String::new(),
            positive_button: String::new(),
            alt_negative_button: String::new(),
            alt_positive_button: String::new(),
            gravity: 1000.0,
            dead: 0.001,
            sensitivity: 1000.0,
            snap: false,
            invert: false,
            kind: AxisType::KeyOrMouseButton,
            axis: 0,
            joy_num: 0,
        }
    }
}

impl InputAxis {
    /// A button axis (instant on/off) like Fire1 or Jump.
    fn button(name: &str, positive: &str, alt: &str) -> Self {
        Self {
            name: name.into(),
            positive_button: positive.into(),
            alt_positive_button: alt.into(),
            ..Default::default()
        }
    }

    /// A smoothed key axis like Horizontal.
    fn keys(name: &str, neg: &str, pos: &str, alt_neg: &str, alt_pos: &str) -> Self {
        Self {
            name: name.into(),
            negative_button: neg.into(),
            positive_button: pos.into(),
            alt_negative_button: alt_neg.into(),
            alt_positive_button: alt_pos.into(),
            gravity: 3.0,
            sensitivity: 3.0,
            snap: true,
            ..Default::default()
        }
    }

    fn mouse(name: &str, axis: usize) -> Self {
        Self {
            name: name.into(),
            gravity: 0.0,
            dead: 0.0,
            sensitivity: 0.1,
            kind: AxisType::MouseMovement,
            axis,
            ..Default::default()
        }
    }

    fn joystick(name: &str, axis: usize, invert: bool) -> Self {
        Self {
            name: name.into(),
            gravity: 0.0,
            dead: 0.19,
            sensitivity: 1.0,
            invert,
            kind: AxisType::JoystickAxis,
            axis,
            ..Default::default()
        }
    }
}

/// The axes list (Project Settings ▸ Input Manager ▸ Axes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputSettings {
    pub axes: Vec<InputAxis>,
}

impl Default for InputSettings {
    /// The usual starter set: keyboard + mouse entries, then the gamepad entries with the
    /// same names.
    fn default() -> Self {
        let axes = vec![
            InputAxis::keys("Horizontal", "left", "right", "a", "d"),
            InputAxis::keys("Vertical", "down", "up", "s", "w"),
            InputAxis::button("Fire1", "left ctrl", "mouse 0"),
            InputAxis::button("Fire2", "left alt", "mouse 1"),
            InputAxis::button("Fire3", "left shift", "mouse 2"),
            InputAxis::button("Jump", "space", ""),
            InputAxis::mouse("Mouse X", 0),
            InputAxis::mouse("Mouse Y", 1),
            InputAxis::mouse("Mouse ScrollWheel", 2),
            // Left stick; joystick Y is +1 when pushed down, so Vertical inverts it.
            InputAxis::joystick("Horizontal", 0, false),
            InputAxis::joystick("Vertical", 1, true),
            InputAxis::button("Fire1", "joystick button 0", ""),
            InputAxis::button("Fire2", "joystick button 1", ""),
            InputAxis::button("Fire3", "joystick button 2", ""),
            InputAxis::button("Jump", "joystick button 3", ""),
            InputAxis::button("Submit", "return", "joystick button 0"),
            InputAxis::button("Submit", "enter", "space"),
            InputAxis::button("Cancel", "escape", "joystick button 1"),
        ];
        Self { axes }
    }
}

impl InputSettings {
    /// Keep values usable (finite, non-negative rates, axis indices in range).
    pub fn sanitize(&mut self) {
        for a in &mut self.axes {
            let fix = |v: f32, fallback: f32| if v.is_finite() { v.max(0.0) } else { fallback };
            a.gravity = fix(a.gravity, 0.0);
            a.dead = fix(a.dead, 0.0).min(1.0);
            a.sensitivity = fix(a.sensitivity, 1.0);
            a.axis = a.axis.min(JOYSTICK_AXES - 1);
            a.joy_num = a.joy_num.min(MAX_JOYSTICKS);
        }
    }
}
