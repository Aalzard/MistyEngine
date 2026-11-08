//! Input Manager: virtual axes and buttons (`Input.GetAxis`, `Input.GetButton`), button names
//! for keys, mouse and joysticks, gamepads through `gilrs`, touches, and the action-based
//! Input System ([`actions`]).

pub mod actions;
pub mod axes;
pub mod gamepad;
pub mod keys;
pub mod manager;
pub mod touch;

pub use axes::{AxisType, InputAxis, InputSettings};
pub use gamepad::Gamepads;
pub use keys::{ButtonName, Joystick};
pub use manager::InputManager;
pub use touch::{Touch, TouchPhase, TouchTracker};

#[cfg(test)]
mod tests;
