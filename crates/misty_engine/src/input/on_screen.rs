//! On-screen controls: UI objects that act as a gamepad or keyboard for touch screens, like
//! Unity's On-Screen Stick and On-Screen Button.
//!
//! An On-Screen Stick or Button sits on a UI object with a graphic (an Image) and names a
//! control path: `<Gamepad>/leftStick`, `<Gamepad>/buttonSouth`, `<Gamepad>/dpad/up`,
//! `<Keyboard>/space`. Fingers (and the left mouse button, so it works in the editor) pressing
//! it drive that control on a virtual gamepad named [`DEVICE_NAME`], added to the frame's
//! joysticks, or on the keyboard. Game code reads it like a real pad: Input System actions,
//! `Input.GetAxis`, `Input.GetKey`.
//!
//! A stick follows its finger up to Movement Range (canvas units) from where it started and
//! springs back on release; its value is the offset over the range. With Dynamic Origin the
//! stick first jumps to where the finger landed inside it, so a large invisible area can hold a
//! floating stick.

use std::collections::{HashMap, HashSet};

use glam::Vec2;
use hecs::Entity;
use serde::{Deserialize, Serialize};

use super::actions::path::{Control, ControlPath, DeviceKind, Part, Stick};
use super::axes::{JOYSTICK_AXES, JOYSTICK_BUTTONS};
use super::keys::Joystick;
use crate::scene::{RectTransform, Scene};
use crate::scripting::Input;
use crate::ui::input::{ancestor, raycast_hit};
use crate::ui::tree::{UiLayout, layout};

/// Name of the virtual gamepad the on-screen controls drive (`Input.GetJoystickNames()`).
pub const DEVICE_NAME: &str = "On-Screen Controls";

/// Finger id the left mouse button presses on-screen controls with.
pub const MOUSE_FINGER: u64 = u64::MAX;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OnScreenStick {
    pub enabled: bool,
    /// Control path it drives: a stick or the d-pad (`<Gamepad>/leftStick`).
    pub control_path: String,
    /// How far (canvas units) the stick moves from where it started; full tilt there.
    pub movement_range: f32,
    /// Jump to where the finger lands inside the object first (a floating stick).
    pub dynamic_origin: bool,
}

impl Default for OnScreenStick {
    fn default() -> Self {
        Self {
            enabled: true,
            control_path: "<Gamepad>/leftStick".into(),
            movement_range: 50.0,
            dynamic_origin: false,
        }
    }
}

impl crate::scene::registry::SceneComponent for OnScreenStick {
    const NAME: &'static str = "On-Screen Stick";
    const CATEGORY: &'static str = "Input";
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OnScreenButton {
    pub enabled: bool,
    /// Control path it presses: a gamepad button, trigger or d-pad direction, or a key
    /// (`<Gamepad>/buttonSouth`, `<Keyboard>/space`).
    pub control_path: String,
}

impl Default for OnScreenButton {
    fn default() -> Self {
        Self {
            enabled: true,
            control_path: "<Gamepad>/buttonSouth".into(),
        }
    }
}

impl crate::scene::registry::SceneComponent for OnScreenButton {
    const NAME: &'static str = "On-Screen Button";
    const CATEGORY: &'static str = "Input";
}

/// What a control path drives, for on-screen controls.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A 2D control: joystick axes (x, y), `y_down` when the axis reads +1 down.
    Axes {
        x: usize,
        y: usize,
        y_down: bool,
    },
    Button(usize),
    /// A trigger axis (0..1).
    Trigger(usize),
    Keys(Vec<&'static str>),
}

/// The target of `path`, or why it can't be driven.
pub fn target(path: &str) -> Result<Target, String> {
    let p = ControlPath::parse(path)?;
    let bad = || format!("'{path}': an on-screen control drives a gamepad or keyboard control");
    Ok(match (p.device, p.control) {
        (DeviceKind::Keyboard, Control::Keys(k)) => Target::Keys(k),
        (DeviceKind::Gamepad, Control::PadButton(i)) => Target::Button(i),
        (DeviceKind::Gamepad, Control::PadTrigger(a)) => Target::Trigger(a),
        (DeviceKind::Gamepad, Control::PadStick(s, Part::Whole)) => {
            let (x, y) = match s {
                Stick::Left => (0, 1),
                Stick::Right => (3, 4),
            };
            Target::Axes { x, y, y_down: true }
        }
        (DeviceKind::Gamepad, Control::PadDpad(Part::Whole)) => Target::Axes {
            x: 5,
            y: 6,
            y_down: false,
        },
        (DeviceKind::Gamepad, Control::PadDpad(part)) => Target::Button(match part {
            Part::Up => 11,
            Part::Down => 12,
            Part::Left => 13,
            Part::Right => 14,
            _ => return Err(bad()),
        }),
        _ => return Err(bad()),
    })
}

/// A stick being held.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Grab {
    finger: u64,
    /// The stick's anchored position before it was touched.
    home: Vec2,
    /// Where the finger counts from (canvas pixels).
    origin: Vec2,
    /// Offset of `origin` from the stick's resting centre (canvas units), for Dynamic Origin.
    shift: Vec2,
}

/// Fingers on on-screen controls between frames (lives while playing).
#[derive(Clone, Debug, Default)]
pub struct OnScreenControls {
    /// Which control each finger is pressing.
    owners: HashMap<u64, Entity>,
    sticks: HashMap<Entity, Grab>,
    /// Last frame's virtual pad buttons and keys (for pressed / released).
    pad_held: [bool; JOYSTICK_BUTTONS],
    keys_held: HashSet<&'static str>,
    /// Paths already reported as bad (one log each).
    reported: HashSet<String>,
    /// Problems to show in the Console.
    pub logs: Vec<String>,
}

/// A pointer this frame: (finger, image position, pressed this frame, still down).
type Pointer = (u64, Vec2, bool, bool);

fn pointers(input: &Input) -> Vec<Pointer> {
    let mut out: Vec<Pointer> = input
        .touches
        .iter()
        .map(|t| {
            let began = t.phase == super::TouchPhase::Began;
            (t.finger_id, t.position, began, t.phase.is_down())
        })
        .collect();
    if let Some(p) = input.mouse_position
        && (input.mouse_held[0] || input.mouse_pressed[0] || input.mouse_released[0])
    {
        let down = input.mouse_held[0] || (input.mouse_pressed[0] && !input.mouse_released[0]);
        out.push((MOUSE_FINGER, p, input.mouse_pressed[0], down));
    }
    out
}

fn is_control(scene: &Scene, e: Entity) -> bool {
    let w = &scene.world;
    w.get::<&OnScreenStick>(e).is_ok_and(|s| s.enabled)
        || w.get::<&OnScreenButton>(e).is_ok_and(|b| b.enabled)
}

impl OnScreenControls {
    /// Apply the on-screen controls to this frame's `input`: adds the virtual gamepad (while
    /// the scene has on-screen controls) and the keys they hold, and moves held sticks.
    pub fn apply(&mut self, scene: &mut Scene, input: &mut Input) {
        let any = scene.has_any::<OnScreenStick>() || scene.has_any::<OnScreenButton>();
        if !any && self.owners.is_empty() && self.keys_held.is_empty() {
            return;
        }
        let l = layout(scene, input.screen_size);
        let ptrs = pointers(input);
        // Lifted fingers let go; new ones grab the control under them.
        let down: HashSet<u64> = ptrs.iter().filter(|p| p.3).map(|p| p.0).collect();
        let released: Vec<u64> = self
            .owners
            .keys()
            .copied()
            .filter(|f| !down.contains(f))
            .collect();
        for f in released {
            if let Some(e) = self.owners.remove(&f) {
                self.let_go(scene, e);
            }
        }
        self.owners
            .retain(|_, e| scene.contains(*e) && scene.active_in_hierarchy(*e));
        self.sticks.retain(|e, _| scene.contains(*e));
        for &(finger, p, began, still) in &ptrs {
            if !began || !still || self.owners.contains_key(&finger) {
                continue;
            }
            let Some(e) = raycast_hit(&l, scene, p)
                .and_then(|(h, _)| ancestor(scene, h, is_control))
                .filter(|e| !self.owners.values().any(|o| o == e))
            else {
                continue;
            };
            self.owners.insert(finger, e);
            if let Ok(stick) = scene.world.get::<&OnScreenStick>(e).map(|s| (*s).clone()) {
                self.grab(scene, &l, e, &stick, finger, p);
            }
        }
        // Drive the controls.
        let mut pad = Joystick {
            name: DEVICE_NAME.into(),
            ..Default::default()
        };
        let mut keys: HashSet<&'static str> = HashSet::new();
        let owners: Vec<(u64, Entity)> = self.owners.iter().map(|(f, e)| (*f, *e)).collect();
        for (finger, e) in owners {
            let path = match (
                scene.world.get::<&OnScreenStick>(e),
                scene.world.get::<&OnScreenButton>(e),
            ) {
                (Ok(s), _) => s.control_path.clone(),
                (_, Ok(b)) => b.control_path.clone(),
                _ => continue,
            };
            let t = match target(&path) {
                Ok(t) => t,
                Err(err) => {
                    if self.reported.insert(path.clone()) {
                        self.logs.push(format!("{}: {err}", scene.name(e)));
                    }
                    continue;
                }
            };
            let value = match self.sticks.get(&e).copied() {
                Some(g) => {
                    let p = ptrs.iter().find(|p| p.0 == finger).map(|p| p.1);
                    self.stick_value(scene, &l, e, g, p)
                }
                None => Vec2::Y,
            };
            match t {
                Target::Axes { x, y, y_down } => {
                    pad.axes[x] = value.x;
                    pad.axes[y] = if y_down { -value.y } else { value.y };
                }
                Target::Button(i) if i < JOYSTICK_BUTTONS => pad.held[i] = true,
                Target::Trigger(a) if a < JOYSTICK_AXES => pad.axes[a] = 1.0,
                Target::Keys(k) => keys.extend(k.first().copied()),
                _ => {}
            }
        }
        for i in 0..JOYSTICK_BUTTONS {
            pad.pressed[i] = pad.held[i] && !self.pad_held[i];
            pad.released[i] = !pad.held[i] && self.pad_held[i];
        }
        self.pad_held = pad.held;
        if any {
            input.joysticks.push(pad);
        }
        for k in &keys {
            if !self.keys_held.contains(k) {
                input.pressed.insert((*k).to_owned());
            }
            input.held.insert((*k).to_owned());
        }
        for k in self.keys_held.difference(&keys) {
            if !input.held.contains(*k) {
                input.released.insert((*k).to_owned());
            }
        }
        self.keys_held = keys;
    }

    fn grab(
        &mut self,
        scene: &mut Scene,
        l: &UiLayout,
        e: Entity,
        stick: &OnScreenStick,
        finger: u64,
        p: Vec2,
    ) {
        let (Some(n), Ok(home)) = (
            l.node(e),
            scene
                .world
                .get::<&RectTransform>(e)
                .map(|r| r.anchored_position),
        ) else {
            return;
        };
        let Some(at) = l.to_canvas(n.canvas, p) else {
            return;
        };
        let (origin, shift) = if stick.dynamic_origin {
            // Canvas pixels are y down, anchored positions y up.
            let centre = n.screen.center();
            let d = (at - centre) / n.scale.max(1e-3);
            (at, Vec2::new(d.x, -d.y))
        } else {
            (at, Vec2::ZERO)
        };
        self.sticks.insert(
            e,
            Grab {
                finger,
                home,
                origin,
                shift,
            },
        );
    }

    /// The held stick's value (y up, length ≤ 1) for the finger at `p`, moving the stick.
    fn stick_value(
        &self,
        scene: &mut Scene,
        l: &UiLayout,
        e: Entity,
        g: Grab,
        p: Option<Vec2>,
    ) -> Vec2 {
        let Some(n) = l.node(e) else {
            return Vec2::ZERO;
        };
        let range = scene
            .world
            .get::<&OnScreenStick>(e)
            .map_or(50.0, |s| s.movement_range)
            .max(1e-3);
        let Some(at) = p.and_then(|p| l.to_canvas(n.canvas, p)) else {
            return Vec2::ZERO;
        };
        let d = (at - g.origin) / n.scale.max(1e-3);
        let offset = Vec2::new(d.x, -d.y).clamp_length_max(range);
        if let Ok(mut rt) = scene.world.get::<&mut RectTransform>(e) {
            rt.anchored_position = g.home + g.shift + offset;
        }
        offset / range
    }

    fn let_go(&mut self, scene: &mut Scene, e: Entity) {
        if let Some(g) = self.sticks.remove(&e)
            && let Ok(mut rt) = scene.world.get::<&mut RectTransform>(e)
        {
            rt.anchored_position = g.home;
        }
    }

    /// Is a finger pressing `e`?
    pub fn is_pressed(&self, e: Entity) -> bool {
        self.owners.values().any(|o| *o == e)
    }

    /// The finger holding stick `e`, if one is.
    pub fn stick_finger(&self, e: Entity) -> Option<u64> {
        self.sticks.get(&e).map(|g| g.finger)
    }
}

#[cfg(test)]
#[path = "on_screen_tests.rs"]
mod tests;
