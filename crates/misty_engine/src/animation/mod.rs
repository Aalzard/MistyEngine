//! Keyframe animation: `.anim` clips, curve evaluation, binding curves to objects and play-mode
//! playback of Animation components and Animator state machines (`.controller` assets, with
//! layers, Avatar Masks (`.mask`), blend trees, sub-state machines and root motion).
//!
//! A clip holds curves; each curve animates one property ("Transform.position.y", "Light.intensity",
//! "GameObject.active", …) of the animated object or of a child found by its name path.

pub mod animator;
pub mod binding;
pub mod blend;
pub mod clip;
pub mod controller;
pub mod curve;
pub mod fsm;
pub mod mask;
pub mod player;
pub mod root_motion;

pub use animator::AnimatorPlayer;
pub use clip::{AnimationClip, AnimationEvent, Curve, Property, is_clip};
pub use controller::{AnimatorController, is_controller};
pub use curve::{Keyframe, Keys, TangentMode};
pub use mask::{AvatarMask, is_mask};
pub use player::AnimationPlayer;

#[cfg(test)]
mod animator_tests;
#[cfg(test)]
mod blend_tests;
#[cfg(test)]
mod layer_tests;
#[cfg(test)]
mod tests;
