//! The Player Input Manager component: local multiplayer. A button pressed on a device no
//! player uses yet spawns the Player Prefab (which has a Player Input) paired with that
//! device; with Split Screen on, each player's camera gets its own part of the screen.
//! The manager's scripts hear `OnPlayerJoined(player)` and `OnPlayerLeft(player)`.

use serde::{Deserialize, Serialize};

/// When players join.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerJoinBehavior {
    /// A button pressed on an unused device spawns a player for it.
    #[default]
    JoinPlayersWhenButtonIsPressed,
    /// Only scripts add players (`manager:JoinPlayer()`, or by spawning Player Inputs).
    JoinPlayersManually,
}

impl PlayerJoinBehavior {
    pub const ALL: [Self; 2] = [
        Self::JoinPlayersWhenButtonIsPressed,
        Self::JoinPlayersManually,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::JoinPlayersWhenButtonIsPressed => "Join Players When Button Is Pressed",
            Self::JoinPlayersManually => "Join Players Manually",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerInputManager {
    pub enabled: bool,
    pub join_behavior: PlayerJoinBehavior,
    /// Prefab spawned for each joining player (it should have a Player Input).
    pub player_prefab: Option<String>,
    /// Joining is allowed (`EnableJoining` / `DisableJoining`).
    pub joining_enabled: bool,
    /// Most players at once (0 = no limit).
    pub max_player_count: u32,
    /// Give each player's camera its own part of the screen.
    pub split_screen: bool,
    /// Split into at least this many parts even with fewer players (0 = as many as players).
    pub fixed_number_of_screens: u32,
}

impl Default for PlayerInputManager {
    fn default() -> Self {
        Self {
            enabled: true,
            join_behavior: PlayerJoinBehavior::JoinPlayersWhenButtonIsPressed,
            player_prefab: None,
            joining_enabled: true,
            max_player_count: 4,
            split_screen: true,
            fixed_number_of_screens: 0,
        }
    }
}

impl crate::scene::registry::SceneComponent for PlayerInputManager {
    const NAME: &'static str = "Player Input Manager";
    const CATEGORY: &'static str = "Input";
}

/// Normalised viewport rect (x, y, w, h; origin bottom-left) of player `index` when the screen
/// is split into `screens` parts: one fills it, two sit side by side, more fill a grid from the
/// top-left, row by row.
pub fn split_rect(index: usize, screens: usize) -> [f32; 4] {
    let screens = screens.max(1);
    if screens == 1 {
        return [0.0, 0.0, 1.0, 1.0];
    }
    let (cols, rows) = if screens == 2 {
        (2, 1)
    } else {
        let cols = (screens as f32).sqrt().ceil() as usize;
        (cols, screens.div_ceil(cols))
    };
    let (w, h) = (1.0 / cols as f32, 1.0 / rows as f32);
    let (col, row) = (index % cols, index / cols);
    // Row 0 is the top one: viewport y grows up.
    let y = 1.0 - (row as f32 + 1.0) * h;
    [col as f32 * w, y.max(0.0), w, h]
}

#[cfg(test)]
mod tests {
    use super::split_rect;

    #[test]
    fn split_rects_side_by_side_then_grid() {
        assert_eq!(split_rect(0, 1), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(split_rect(0, 2), [0.0, 0.0, 0.5, 1.0]);
        assert_eq!(split_rect(1, 2), [0.5, 0.0, 0.5, 1.0]);
        // Three or four players: a 2×2 grid from the top-left.
        assert_eq!(split_rect(0, 4), [0.0, 0.5, 0.5, 0.5]);
        assert_eq!(split_rect(1, 4), [0.5, 0.5, 0.5, 0.5]);
        assert_eq!(split_rect(2, 3), [0.0, 0.0, 0.5, 0.5]);
        assert_eq!(split_rect(3, 4), [0.5, 0.0, 0.5, 0.5]);
        // Six: 3 columns × 2 rows.
        let r = split_rect(5, 6);
        assert!((r[0] - 2.0 / 3.0).abs() < 1e-6 && r[1] == 0.0 && r[3] == 0.5);
    }
}
