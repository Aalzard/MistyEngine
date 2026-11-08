//! The Animator component: plays an Animator Controller state machine on this object and its
//! children (evaluated by `animation::AnimatorPlayer` in Play mode).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Animator {
    pub enabled: bool,
    /// Controller asset path ("Assets/Animations/Hero.controller").
    pub controller: Option<String>,
    /// Multiplies every state's speed.
    pub speed: f32,
    /// When disabled and enabled again, continue where it was instead of restarting in the
    /// default state with default parameter values.
    pub keep_state_on_disable: bool,
    /// Move the object by the motion of its root (see `root_motion_bone`) instead of letting
    /// the clips set its position; with a Character Controller the move collides.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub apply_root_motion: bool,
    /// Path of the bone whose horizontal travel and turning become root motion ("" = the
    /// object's own position and Y rotation curves).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub root_motion_bone: String,
    /// Humanoid Avatar asset (`.avatar`): humanoid clips play on this rig through it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

impl Default for Animator {
    fn default() -> Self {
        Self {
            enabled: true,
            controller: None,
            speed: 1.0,
            keep_state_on_disable: false,
            apply_root_motion: false,
            root_motion_bone: String::new(),
            avatar: None,
        }
    }
}
