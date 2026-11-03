//! Keyframe animation: `.anim` clips, curve evaluation, binding curves to objects and play-mode
//! playback of Animation components.
//!
//! A clip holds curves; each curve animates one property ("Transform.position.y", "Light.intensity",
//! "GameObject.active", …) of the animated object or of a child found by its name path.

pub mod binding;
pub mod clip;
pub mod curve;
pub mod player;

pub use clip::{AnimationClip, AnimationEvent, Curve, Property, is_clip};
pub use curve::{Keyframe, Keys, TangentMode};
pub use player::AnimationPlayer;

#[cfg(test)]
mod tests;
