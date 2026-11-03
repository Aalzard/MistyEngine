//! Input Manager: virtual axes and buttons (`Input.GetAxis`, `Input.GetButton`), button names
//! for keys, mouse and joysticks, and gamepads through `gilrs`.

pub mod axes;
pub mod gamepad;
pub mod keys;
pub mod manager;

pub use axes::{AxisType, InputAxis, InputSettings};
pub use gamepad::Gamepads;
pub use keys::{ButtonName, Joystick};
pub use manager::InputManager;

#[cfg(test)]
mod tests;
