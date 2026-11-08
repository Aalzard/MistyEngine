//! The Input System: action-based input next to the Input Manager.
//!
//! An Input Actions asset ([`asset`]) holds action maps; each action has bindings to control
//! paths ([`path`]) or composites ([`composites`]), with processors ([`processors`]) and
//! interactions ([`interactions`]). An [`ActionSet`] plays a copy of an asset each frame;
//! the [`InputSystem`] owns every set of a running game, the Player Inputs
//! ([`player_input`]) with their control schemes and paired devices, interactive rebinding and
//! rumble requests.

pub mod asset;
pub mod composites;
pub mod interactions;
pub mod path;
pub mod player_input;
pub mod processors;
pub mod state;
pub mod system;
pub mod ui_module;
pub mod value;

pub use asset::{
    Action, ActionMap, ActionType, Binding, CompositePart, ControlScheme, ControlType,
    DeviceRequirement, InputActionAsset,
};
pub use interactions::Phase;
pub use player_input::{PlayerInput, PlayerInputBehavior, PlayerInputCall};
pub use state::{ActionEvent, ActionSet};
pub use system::{ActiveInputHandling, DeviceId, InputSystem, SystemEvent};
pub use value::Value;

#[cfg(test)]
mod tests;
