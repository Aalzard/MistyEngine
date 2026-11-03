//! Layers: names, masks (`LayerMask.GetMask`) and collision groups from the Layer Collision Matrix.

use rapier3d::prelude as rp;

use crate::settings::PhysicsSettings;

/// Built-in layer names by index (the GameObject ▸ Layer dropdown).
pub const LAYERS: [&str; 6] = [
    "Default",
    "TransparentFX",
    "Ignore Raycast",
    "Water",
    "UI",
    "Player",
];

pub const IGNORE_RAYCAST_LAYER: u8 = 2;
/// Every layer.
pub const ALL_LAYERS: u32 = u32::MAX;
/// Raycasts skip the Ignore Raycast layer unless a mask says otherwise.
pub const DEFAULT_RAYCAST_LAYERS: u32 = !(1 << IGNORE_RAYCAST_LAYER);

/// Index of a named layer, or -1.
pub fn name_to_layer(name: &str) -> i32 {
    LAYERS
        .iter()
        .position(|l| *l == name)
        .map_or(-1, |i| i as i32)
}

/// Name of a layer index ("" when unnamed).
pub fn layer_to_name(layer: i32) -> &'static str {
    usize::try_from(layer)
        .ok()
        .and_then(|i| LAYERS.get(i))
        .copied()
        .unwrap_or("")
}

/// Mask with the bits of the named layers set (unknown names are ignored).
pub fn get_mask<S: AsRef<str>>(names: &[S]) -> u32 {
    names
        .iter()
        .map(|n| name_to_layer(n.as_ref()))
        .filter(|l| *l >= 0)
        .fold(0, |m, l| m | 1 << l)
}

/// Collision groups of a collider on `layer`: member of its layer, colliding with the layers
/// the matrix allows.
pub(crate) fn groups(settings: &PhysicsSettings, layer: u8) -> rp::InteractionGroups {
    let layer = layer.min(31);
    rp::InteractionGroups::new(
        rp::Group::from_bits_retain(1 << layer),
        rp::Group::from_bits_retain(settings.collision_mask(layer)),
        rp::InteractionTestMode::And,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_masks() {
        assert_eq!(name_to_layer("Default"), 0);
        assert_eq!(name_to_layer("Player"), 5);
        assert_eq!(name_to_layer("Nope"), -1);
        assert_eq!(layer_to_name(2), "Ignore Raycast");
        assert_eq!(layer_to_name(17), "");
        assert_eq!(layer_to_name(-1), "");
        assert_eq!(get_mask(&["Default", "Water"]), 0b1001);
        assert_eq!(get_mask(&["Nope"]), 0);
        assert_eq!(DEFAULT_RAYCAST_LAYERS & (1 << 2), 0);
    }
}
