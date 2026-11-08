//! Gamepads through the `gilrs` crate, mapped to an Xbox-style joystick layout.
//!
//! Buttons: 0 A (south), 1 B (east), 2 X (west), 3 Y (north), 4 left bumper, 5 right bumper,
//! 6 back/select, 7 start, 8 left stick click, 9 right stick click, 10 guide,
//! 11–14 d-pad up/down/left/right.
//! Axes (0-based): 0 left stick X, 1 left stick Y (+1 = down), 2 triggers (left − right),
//! 3 right stick X, 4 right stick Y (+1 = down), 5 d-pad X, 6 d-pad Y (+1 = up),
//! 8 left trigger (0..1), 9 right trigger (0..1).

use std::collections::HashMap;

use gilrs::ff::{BaseEffect, BaseEffectType, Effect, EffectBuilder, Repeat, Replay, Ticks};
use gilrs::{Axis, Button, GamepadId, Gilrs};

use super::keys::Joystick;

/// Button index of a gamepad button in the joystick layout.
pub fn button_index(b: Button) -> Option<usize> {
    Some(match b {
        Button::South => 0,
        Button::East => 1,
        Button::West => 2,
        Button::North => 3,
        Button::LeftTrigger => 4,
        Button::RightTrigger => 5,
        Button::Select => 6,
        Button::Start => 7,
        Button::LeftThumb => 8,
        Button::RightThumb => 9,
        Button::Mode => 10,
        Button::DPadUp => 11,
        Button::DPadDown => 12,
        Button::DPadLeft => 13,
        Button::DPadRight => 14,
        _ => return None,
    })
}

const BUTTONS: [Button; 15] = [
    Button::South,
    Button::East,
    Button::West,
    Button::North,
    Button::LeftTrigger,
    Button::RightTrigger,
    Button::Select,
    Button::Start,
    Button::LeftThumb,
    Button::RightThumb,
    Button::Mode,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
];

/// Read one pad: `pressed(b)` = button held, `axis(a)` = stick value (y up = +1),
/// `analog(b)` = analog button value 0..1 (triggers).
pub fn read_pad(
    name: &str,
    pressed: impl Fn(Button) -> bool,
    axis: impl Fn(Axis) -> f32,
    analog: impl Fn(Button) -> f32,
) -> Joystick {
    let mut j = Joystick {
        name: name.to_owned(),
        ..Default::default()
    };
    for b in BUTTONS {
        if let Some(i) = button_index(b) {
            j.held[i] = pressed(b);
        }
    }
    let lt = analog(Button::LeftTrigger2)
        .max(axis(Axis::LeftZ))
        .clamp(0.0, 1.0);
    let rt = analog(Button::RightTrigger2)
        .max(axis(Axis::RightZ))
        .clamp(0.0, 1.0);
    let dir = |neg: Button, pos: Button| pressed(pos) as i32 as f32 - pressed(neg) as i32 as f32;
    let or = |a: f32, b: f32| if a != 0.0 { a } else { b };
    j.axes[0] = axis(Axis::LeftStickX);
    j.axes[1] = -axis(Axis::LeftStickY);
    j.axes[2] = lt - rt;
    j.axes[3] = axis(Axis::RightStickX);
    j.axes[4] = -axis(Axis::RightStickY);
    j.axes[5] = or(axis(Axis::DPadX), dir(Button::DPadLeft, Button::DPadRight));
    j.axes[6] = or(axis(Axis::DPadY), dir(Button::DPadDown, Button::DPadUp));
    j.axes[8] = lt;
    j.axes[9] = rt;
    j
}

/// Fill `pressed` / `released` from the previous frame's state of the same slot.
pub fn diff(prev: Option<&Joystick>, now: &mut Joystick) {
    for i in 0..now.held.len() {
        let was = prev.is_some_and(|p| p.held[i]);
        now.pressed[i] = now.held[i] && !was;
        now.released[i] = !now.held[i] && was;
    }
}

/// Connected gamepads, polled once per game frame. Each pad keeps its joystick number while
/// it stays plugged in; an unplugged pad leaves an empty slot.
#[derive(Default)]
pub struct Gamepads {
    gilrs: Option<Gilrs>,
    started: bool,
    slots: Vec<Option<GamepadId>>,
    last: Vec<Joystick>,
    /// Running rumble effects by slot (dropping one stops it).
    rumble: HashMap<usize, Effect>,
}

impl Gamepads {
    /// This frame's joysticks (empty when the platform has no gamepad support).
    pub fn poll(&mut self) -> Vec<Joystick> {
        if !self.started {
            self.started = true;
            self.gilrs = match Gilrs::new() {
                Ok(g) => Some(g),
                Err(gilrs::Error::NotImplemented(g)) => Some(g),
                Err(_) => None,
            };
        }
        let Some(g) = self.gilrs.as_mut() else {
            return Vec::new();
        };
        // Events update gilrs' cached state.
        while g.next_event().is_some() {}
        let connected: Vec<GamepadId> = g.gamepads().map(|(id, _)| id).collect();
        for (i, s) in self.slots.iter_mut().enumerate() {
            if s.is_some_and(|id| !connected.contains(&id)) {
                *s = None;
                self.rumble.remove(&i);
            }
        }
        for id in connected {
            if !self.slots.contains(&Some(id)) {
                match self.slots.iter().position(Option::is_none) {
                    Some(free) => self.slots[free] = Some(id),
                    None => self.slots.push(Some(id)),
                }
            }
        }
        let mut now: Vec<Joystick> = self
            .slots
            .iter()
            .map(|slot| match slot {
                Some(id) => {
                    let pad = g.gamepad(*id);
                    read_pad(
                        pad.name(),
                        |b| pad.is_pressed(b),
                        |a| pad.value(a),
                        |b| pad.button_data(b).map_or(0.0, |d| d.value()),
                    )
                }
                None => Joystick::default(),
            })
            .collect();
        for (i, j) in now.iter_mut().enumerate() {
            diff(self.last.get(i), j);
        }
        self.last = now.clone();
        now
    }
}

/// 0..1 motor speed → force feedback magnitude.
fn magnitude(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16
}

impl Gamepads {
    /// Rumble a pad: `low` drives the low-frequency (strong, left) motor, `high` the
    /// high-frequency (weak, right) one, 0..1; both 0 stops. Silently does nothing where the
    /// platform or the pad has no force feedback.
    pub fn set_motor_speeds(&mut self, slot: usize, low: f32, high: f32) {
        self.rumble.remove(&slot);
        let (Some(g), Some(Some(id))) = (self.gilrs.as_mut(), self.slots.get(slot).copied()) else {
            return;
        };
        if (low <= 0.0 && high <= 0.0) || !g.gamepad(id).is_ff_supported() {
            return;
        }
        let scheduling = Replay {
            play_for: Ticks::from_ms(1000),
            ..Default::default()
        };
        let effect = EffectBuilder::new()
            .add_effect(BaseEffect {
                kind: BaseEffectType::Strong {
                    magnitude: magnitude(low),
                },
                scheduling,
                envelope: Default::default(),
            })
            .add_effect(BaseEffect {
                kind: BaseEffectType::Weak {
                    magnitude: magnitude(high),
                },
                scheduling,
                envelope: Default::default(),
            })
            .gamepads(&[id])
            .repeat(Repeat::Infinitely)
            .finish(g);
        if let Ok(e) = effect
            && e.play().is_ok()
        {
            self.rumble.insert(slot, e);
        }
    }

    /// Stop every pad's motors (Play stopped).
    pub fn stop_rumble(&mut self) {
        self.rumble.clear();
    }
}
