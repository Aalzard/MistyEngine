//! Sprite Renderer: draws a sprite (see `crate::sprites`) as a quad in the object's XY plane.

use serde::{Deserialize, Serialize};

/// How the sprite fills its size (only Simple: the sprite at its natural size × scale).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpriteDrawMode {
    #[default]
    Simple,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpriteRenderer {
    pub enabled: bool,
    /// Sprite reference ("Assets/Sprites/Player.png" or "…/Tiles.png#Tiles_3").
    pub sprite: Option<String>,
    /// sRGB tint + alpha, multiplied with the sprite.
    pub color: [f32; 4],
    pub flip_x: bool,
    pub flip_y: bool,
    pub draw_mode: SpriteDrawMode,
    /// Name of a Sorting Layer from Project Settings (unknown names sort as Default).
    pub sorting_layer: String,
    /// Order inside the Sorting Layer: higher draws on top.
    pub order_in_layer: i32,
}

impl Default for SpriteRenderer {
    fn default() -> Self {
        Self {
            enabled: true,
            sprite: None,
            color: [1.0; 4],
            flip_x: false,
            flip_y: false,
            draw_mode: SpriteDrawMode::Simple,
            sorting_layer: crate::sprites::sorting::DEFAULT.to_owned(),
            order_in_layer: 0,
        }
    }
}
