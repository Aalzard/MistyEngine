//! The Player Input component: plays an Input Actions asset for one player, with a default
//! action map and control scheme, and tells the object's scripts about actions (messages,
//! or event calls set up in the Inspector).

use hecs::Entity;
use serde::{Deserialize, Serialize};

/// How Player Input reports actions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerInputBehavior {
    /// Call `On<Action>(value)` on this object's scripts (`OnMove(v)`, `OnJump()`), plus
    /// `OnDeviceLost`, `OnDeviceRegained` and `OnControlsChanged`.
    #[default]
    SendMessages,
    /// Like Send Messages, on this object and its children.
    BroadcastMessages,
    /// Call the methods listed under Events with a callback context (started, performed and
    /// canceled), and the `onActionTriggered` listeners.
    InvokeEvents,
}

impl PlayerInputBehavior {
    pub const ALL: [Self; 3] = [
        Self::SendMessages,
        Self::BroadcastMessages,
        Self::InvokeEvents,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::SendMessages => "Send Messages",
            Self::BroadcastMessages => "Broadcast Messages",
            Self::InvokeEvents => "Invoke Events",
        }
    }
}

/// One event entry: call `method` on the scripts of `target` for `action`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerInputCall {
    /// `Map/Action`.
    pub action: String,
    #[serde(with = "crate::scene::components::entity_bits::option")]
    pub target: Option<Entity>,
    pub method: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerInput {
    /// Input Actions asset path.
    pub actions: Option<String>,
    /// Action map enabled at start ("" = the first one).
    pub default_map: String,
    /// Control scheme to start with ("" = the first one whose devices are there).
    pub default_control_scheme: String,
    /// Switch control scheme when another device is used (single player only).
    pub auto_switch: bool,
    pub behavior: PlayerInputBehavior,
    /// Event calls (Invoke Events).
    pub events: Vec<PlayerInputCall>,
}

impl Default for PlayerInput {
    fn default() -> Self {
        Self {
            actions: None,
            default_map: String::new(),
            default_control_scheme: String::new(),
            auto_switch: true,
            behavior: PlayerInputBehavior::SendMessages,
            events: Vec::new(),
        }
    }
}

impl crate::scene::registry::SceneComponent for PlayerInput {
    const NAME: &'static str = "Player Input";
    const CATEGORY: &'static str = "Input";
}

/// Object references held by Player Input (scene files remap them).
pub const PLAYER_INPUT_REFS: &[(&str, &str, &str)] = &[("Player Input", "events", "target")];

/// `OnMove` for the action "Move" (spaces and symbols dropped: "Look Around" → `OnLookAround`).
pub fn message_name(action: &str) -> String {
    let clean: String = action.chars().filter(|c| c.is_alphanumeric()).collect();
    format!("On{clean}")
}
