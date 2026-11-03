//! Cliff rendering settings: triplanar projection on steep slopes (no stretched textures on
//! cliffs) and height-based layer blending (rock shows through the cracks of grass).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CliffSettings {
    /// Project textures from three axes where the slope is steeper than `triplanar_angle`.
    pub triplanar: bool,
    /// Slope (degrees) above which triplanar projection takes over.
    pub triplanar_angle: f32,
    /// Exponent of the three projections' weights (higher = sharper seams, less blur).
    pub sharpness: f32,
    /// Height Blend contrast 0..1: 0 = plain weight blend, 1 = sharp height-map edges.
    pub height_blend: f32,
}

impl Default for CliffSettings {
    fn default() -> Self {
        Self {
            triplanar: true,
            triplanar_angle: 45.0,
            sharpness: 4.0,
            height_blend: 0.5,
        }
    }
}

impl CliffSettings {
    /// Shader uniform: x = triplanar on, y = sin of the start angle (|normal.xz| where it
    /// starts), z = sharpness, w = height-blend transition width (1 = linear blend).
    pub fn uniform(&self) -> [f32; 4] {
        let angle = self.triplanar_angle.clamp(0.0, 89.0).to_radians();
        let contrast = self.height_blend.clamp(0.0, 1.0);
        [
            if self.triplanar { 1.0 } else { 0.0 },
            angle.sin(),
            self.sharpness.clamp(1.0, 32.0),
            // Width of the height-lerp window: wide = smooth, narrow = sharp.
            1.0 - contrast * 0.97,
        ]
    }
}
