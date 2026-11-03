//! Button names shared by `Input.GetKey` and the Input Manager: keyboard keys ("w",
//! "left shift"), mouse buttons ("mouse 0") and joystick buttons ("joystick button 0" = any
//! joystick, "joystick 2 button 0" = the second one).

use super::axes::{JOYSTICK_AXES, JOYSTICK_BUTTONS};
use crate::scripting::Input;

/// One connected joystick / gamepad this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Joystick {
    /// Device name (`Input.GetJoystickNames()`); empty for a slot whose pad was unplugged.
    pub name: String,
    pub held: [bool; JOYSTICK_BUTTONS],
    pub pressed: [bool; JOYSTICK_BUTTONS],
    pub released: [bool; JOYSTICK_BUTTONS],
    /// -1..1 (triggers 0..1), layout in `gamepad`.
    pub axes: [f32; JOYSTICK_AXES],
}

/// A parsed button name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ButtonName {
    Key(String),
    Mouse(usize),
    /// `joystick` = None for any joystick, Some(0-based index) for "joystick N".
    Joystick {
        joystick: Option<usize>,
        button: usize,
    },
}

impl ButtonName {
    /// Parse a lower-case-insensitive button name; `None` for "".
    pub fn parse(name: &str) -> Option<Self> {
        let n = name.trim().to_lowercase();
        if n.is_empty() {
            return None;
        }
        let words: Vec<&str> = n.split_whitespace().collect();
        let num = |s: &str| s.parse::<usize>().ok();
        Some(match words.as_slice() {
            ["mouse", b] if num(b).is_some() => ButtonName::Mouse(num(b)?),
            ["joystick", "button", b] if num(b).is_some() => ButtonName::Joystick {
                joystick: None,
                button: num(b)?,
            },
            ["joystick", j, "button", b] if num(j).is_some_and(|j| j > 0) && num(b).is_some() => {
                ButtonName::Joystick {
                    joystick: Some(num(j)? - 1),
                    button: num(b)?,
                }
            }
            _ => ButtonName::Key(n),
        })
    }
}

/// Which per-frame state to read.
#[derive(Clone, Copy)]
enum Phase {
    Held,
    Pressed,
    Released,
}

impl Input {
    fn button_phase(&self, name: &str, phase: Phase) -> bool {
        let Some(b) = ButtonName::parse(name) else {
            return false;
        };
        match b {
            ButtonName::Key(k) => match phase {
                Phase::Held => self.held.contains(&k),
                Phase::Pressed => self.pressed.contains(&k),
                Phase::Released => self.released.contains(&k),
            },
            ButtonName::Mouse(i) => {
                let set = match phase {
                    Phase::Held => &self.mouse_held,
                    Phase::Pressed => &self.mouse_pressed,
                    Phase::Released => &self.mouse_released,
                };
                set.get(i).copied().unwrap_or(false)
            }
            ButtonName::Joystick { joystick, button } => {
                let pads: Vec<&Joystick> = match joystick {
                    Some(j) => self.joysticks.get(j).into_iter().collect(),
                    None => self.joysticks.iter().collect(),
                };
                pads.iter().any(|p| {
                    let set = match phase {
                        Phase::Held => &p.held,
                        Phase::Pressed => &p.pressed,
                        Phase::Released => &p.released,
                    };
                    set.get(button).copied().unwrap_or(false)
                })
            }
        }
    }

    /// Is the named key / mouse / joystick button held?
    pub fn button_held(&self, name: &str) -> bool {
        self.button_phase(name, Phase::Held)
    }

    /// Was it pressed this frame?
    pub fn button_pressed(&self, name: &str) -> bool {
        self.button_phase(name, Phase::Pressed)
    }

    /// Was it released this frame?
    pub fn button_released(&self, name: &str) -> bool {
        self.button_phase(name, Phase::Released)
    }

    /// `Input.anyKey`: any key, mouse or joystick button held.
    pub fn any_key(&self) -> bool {
        !self.held.is_empty()
            || self.mouse_held.iter().any(|b| *b)
            || self.joysticks.iter().any(|j| j.held.iter().any(|b| *b))
    }

    /// `Input.anyKeyDown`: any of them pressed this frame.
    pub fn any_key_down(&self) -> bool {
        !self.pressed.is_empty()
            || self.mouse_pressed.iter().any(|b| *b)
            || self.joysticks.iter().any(|j| j.pressed.iter().any(|b| *b))
    }

    /// Joystick axis value: one joystick (0-based) or, with `None`, the largest of all.
    pub fn joystick_axis(&self, joystick: Option<usize>, axis: usize) -> f32 {
        let value = |j: &Joystick| j.axes.get(axis).copied().unwrap_or(0.0);
        match joystick {
            Some(i) => self.joysticks.get(i).map_or(0.0, value),
            None => self
                .joysticks
                .iter()
                .map(value)
                .fold(0.0, |a, v| if v.abs() > a.abs() { v } else { a }),
        }
    }

    /// Wheel movement in notches (`Input.mouseScrollDelta`; y > 0 = away from the user).
    pub fn scroll_notches(&self) -> glam::Vec2 {
        self.scroll_delta / SCROLL_PIXELS_PER_NOTCH
    }
}

/// Wheel pixels per notch (the player reports ≈ 50 px per notch).
pub const SCROLL_PIXELS_PER_NOTCH: f32 = 50.0;
