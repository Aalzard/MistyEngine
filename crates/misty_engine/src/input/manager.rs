//! Runtime side of the Input Manager: per-frame axis values with gravity/sensitivity
//! smoothing, and virtual buttons.

use super::axes::{AxisType, InputAxis, InputSettings};
use crate::scripting::Input;

/// Axis values of the running game (lives in the scripting world).
#[derive(Clone, Debug, Default)]
pub struct InputManager {
    axes: Vec<InputAxis>,
    /// Smoothed value per axis entry.
    values: Vec<f32>,
    /// Unsmoothed value per axis entry.
    raw: Vec<f32>,
}

impl InputManager {
    pub fn new(settings: &InputSettings) -> Self {
        let n = settings.axes.len();
        Self {
            axes: settings.axes.clone(),
            values: vec![0.0; n],
            raw: vec![0.0; n],
        }
    }

    /// Advance every axis by `dt` real seconds with this frame's input.
    pub fn update(&mut self, input: &Input, dt: f32) {
        for (i, a) in self.axes.iter().enumerate() {
            let (raw, value) = step(a, input, self.values[i], dt);
            self.raw[i] = raw;
            self.values[i] = value;
        }
    }

    fn entries(&self, name: &str) -> Option<Vec<usize>> {
        let found: Vec<usize> = (0..self.axes.len())
            .filter(|i| self.axes[*i].name == name)
            .collect();
        (!found.is_empty()).then_some(found)
    }

    /// `Input.GetAxis`: the entry with the largest magnitude; `None` = no such axis.
    pub fn axis(&self, name: &str) -> Option<f32> {
        Some(largest(self.entries(name)?.iter().map(|i| self.values[*i])))
    }

    /// `Input.GetAxisRaw`: like [`InputManager::axis`] without smoothing.
    pub fn axis_raw(&self, name: &str) -> Option<f32> {
        Some(largest(self.entries(name)?.iter().map(|i| self.raw[*i])))
    }

    /// `Input.GetButton` / `GetButtonDown` / `GetButtonUp`: any entry's positive button.
    pub fn button(
        &self,
        name: &str,
        input: &Input,
        test: impl Fn(&Input, &str) -> bool,
    ) -> Option<bool> {
        let entries = self.entries(name)?;
        Some(entries.iter().any(|i| {
            let a = &self.axes[*i];
            a.kind == AxisType::KeyOrMouseButton
                && (test(input, &a.positive_button) || test(input, &a.alt_positive_button))
        }))
    }
}

fn largest(values: impl Iterator<Item = f32>) -> f32 {
    values.fold(0.0, |a, v| if v.abs() > a.abs() { v } else { a })
}

fn move_towards(v: f32, target: f32, max_delta: f32) -> f32 {
    if (target - v).abs() <= max_delta {
        target
    } else {
        v + (target - v).signum() * max_delta
    }
}

/// New (raw, smoothed) value of one axis entry.
fn step(a: &InputAxis, input: &Input, value: f32, dt: f32) -> (f32, f32) {
    let flip = if a.invert { -1.0 } else { 1.0 };
    match a.kind {
        AxisType::KeyOrMouseButton => {
            let held = |b: &str| input.button_held(b);
            let pos = held(&a.positive_button) || held(&a.alt_positive_button);
            let neg = held(&a.negative_button) || held(&a.alt_negative_button);
            let target = (pos as i32 - neg as i32) as f32 * flip;
            let value = if target != 0.0 {
                // Snap: reversing direction starts from 0 instead of gliding through it.
                let from = if a.snap && value != 0.0 && value.signum() != target.signum() {
                    0.0
                } else {
                    value
                };
                move_towards(from, target, a.sensitivity * dt)
            } else {
                move_towards(value, 0.0, a.gravity * dt)
            };
            (target, value)
        }
        AxisType::MouseMovement => {
            let v = match a.axis {
                0 => input.mouse_delta.x,
                // Screen y grows downwards; moving the mouse up is positive.
                1 => -input.mouse_delta.y,
                2 => input.scroll_notches().y,
                _ => 0.0,
            } * a.sensitivity
                * flip;
            (v, v)
        }
        AxisType::JoystickAxis => {
            let joystick = a.joy_num.checked_sub(1);
            let mut v = input.joystick_axis(joystick, a.axis);
            if v.abs() < a.dead {
                v = 0.0;
            }
            let v = v * a.sensitivity * flip;
            (v, v)
        }
    }
}
