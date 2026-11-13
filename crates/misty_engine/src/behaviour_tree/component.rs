//! The Behaviour Tree Agent component: runs a `.btree` asset on its object while playing.

use hecs::Entity;
use serde::{Deserialize, Serialize};

use super::blackboard::BbValue;
use crate::scene::components::entity_bits;

/// A blackboard value this agent starts with instead of the tree's default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlackboardOverride {
    pub key: String,
    /// The value (for every type but Object).
    pub value: BbValue,
    /// The object, for Object keys.
    #[serde(with = "entity_bits::option", skip_serializing_if = "Option::is_none")]
    pub object: Option<Entity>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BehaviourTreeAgent {
    pub enabled: bool,
    /// Tree asset path ("Assets/AI/Guard.btree").
    pub tree: Option<String>,
    /// Ticks per second (0 = every frame).
    pub tick_rate: f32,
    /// Start when Play starts (else wait for `BehaviourTree.Start`).
    pub start_on_play: bool,
    /// Start over when the tree finishes (else it stops with its result).
    pub restart_when_done: bool,
    /// Agents with the same group share the tree's shared keys (empty = the tree's path).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub share_group: String,
    /// Picks the random sequence (Random Selector, Wait's random part); 0 = from the name.
    #[serde(skip_serializing_if = "is_zero")]
    pub seed: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<BlackboardOverride>,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

impl Default for BehaviourTreeAgent {
    fn default() -> Self {
        Self {
            enabled: true,
            tree: None,
            tick_rate: 0.0,
            start_on_play: true,
            restart_when_done: true,
            share_group: String::new(),
            seed: 0,
            overrides: Vec::new(),
        }
    }
}

impl BehaviourTreeAgent {
    /// Seconds between ticks (0 = every frame).
    pub fn interval(&self) -> f32 {
        if self.tick_rate > 0.0 && self.tick_rate.is_finite() {
            1.0 / self.tick_rate
        } else {
            0.0
        }
    }

    /// The share group used for shared keys.
    pub fn group(&self) -> &str {
        if self.share_group.is_empty() {
            self.tree.as_deref().unwrap_or("")
        } else {
            &self.share_group
        }
    }
}

/// FNV-1a of a name: a seed that is the same on every run.
pub fn name_seed(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}
