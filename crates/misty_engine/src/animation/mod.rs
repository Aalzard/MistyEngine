//! Keyframe animation: `.anim` clips, curve evaluation, binding curves to objects and play-mode
//! playback of Animation components and Animator state machines (`.controller` assets).
//!
//! A clip holds curves; each curve animates one property ("Transform.position.y", "Light.intensity",
//! "GameObject.active", …) of the animated object or of a child found by its name path.

pub mod animator;
pub mod binding;
pub mod clip;
pub mod controller;
pub mod curve;
pub mod player;

pub use animator::AnimatorPlayer;
pub use clip::{AnimationClip, AnimationEvent, Curve, Property, is_clip};
pub use controller::{AnimatorController, is_controller};
pub use curve::{Keyframe, Keys, TangentMode};
pub use player::AnimationPlayer;

#[cfg(test)]
mod animator_tests;
#[cfg(test)]
mod tests;
