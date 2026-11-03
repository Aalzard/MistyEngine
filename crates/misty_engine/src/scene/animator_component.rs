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
}

impl Default for Animator {
    fn default() -> Self {
        Self {
            enabled: true,
            controller: None,
            speed: 1.0,
            keep_state_on_disable: false,
        }
    }
}
