//! Project Settings ▸ Tags and Layers: user tags next to the built-in ones, and the 32 layer
//! names (0–7 built in, 8–31 named by the user).

use serde::{Deserialize, Serialize};

use crate::physics::layers::LAYERS;

/// Tags every project has.
pub const BUILTIN_TAGS: [&str; 7] = [
    "Untagged",
    "Respawn",
    "Finish",
    "EditorOnly",
    "MainCamera",
    "Player",
    "GameController",
];

pub const LAYER_COUNT: usize = 32;
/// Layers below this index are built in (0–5 named, 6–7 reserved).
pub const FIRST_USER_LAYER: usize = 8;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TagsAndLayers {
    /// User tags (the built-in ones are not stored).
    pub tags: Vec<String>,
    /// Names of all 32 layers; built-in entries are fixed, "" = unnamed.
    pub layers: Vec<String>,
}

impl Default for TagsAndLayers {
    fn default() -> Self {
        let mut s = Self {
            tags: Vec::new(),
            layers: Vec::new(),
        };
        s.sanitize();
        s
    }
}

impl TagsAndLayers {
    /// 32 layer names with the built-in ones in place; unique, non-empty user tags.
    pub fn sanitize(&mut self) {
        self.layers.resize(LAYER_COUNT, String::new());
        for (i, name) in self.layers.iter_mut().enumerate() {
            if i < FIRST_USER_LAYER {
                *name = LAYERS.get(i).copied().unwrap_or("").to_owned();
            } else {
                *name = name.trim().to_owned();
            }
        }
        let mut seen: Vec<String> = BUILTIN_TAGS.iter().map(|t| t.to_string()).collect();
        self.tags.retain_mut(|t| {
            *t = t.trim().to_owned();
            if t.is_empty() || seen.contains(t) {
                return false;
            }
            seen.push(t.clone());
            true
        });
    }

    /// Built-in then user tags (the Tag dropdown).
    pub fn all_tags(&self) -> Vec<String> {
        BUILTIN_TAGS
            .iter()
            .map(|t| t.to_string())
            .chain(self.tags.iter().cloned())
            .collect()
    }

    pub fn has_tag(&self, tag: &str) -> bool {
        BUILTIN_TAGS.contains(&tag) || self.tags.iter().any(|t| t == tag)
    }

    /// Name of a layer ("" when unnamed or out of range).
    pub fn layer_name(&self, layer: i32) -> &str {
        usize::try_from(layer)
            .ok()
            .and_then(|i| self.layers.get(i))
            .map_or("", |s| s.as_str())
    }

    /// `LayerMask.NameToLayer`: index of a named layer, or -1.
    pub fn name_to_layer(&self, name: &str) -> i32 {
        if name.is_empty() {
            return -1;
        }
        self.layers
            .iter()
            .position(|l| l == name)
            .map_or(-1, |i| i as i32)
    }

    /// `LayerMask.GetMask`: bits of the named layers (unknown names are ignored).
    pub fn get_mask<S: AsRef<str>>(&self, names: &[S]) -> u32 {
        names
            .iter()
            .map(|n| self.name_to_layer(n.as_ref()))
            .filter(|l| *l >= 0)
            .fold(0, |m, l| m | 1 << l)
    }

    /// Layers that have a name, in index order (dropdowns, the collision matrix).
    pub fn named_layers(&self) -> Vec<(u8, &str)> {
        self.layers
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.is_empty())
            .map(|(i, n)| (i as u8, n.as_str()))
            .collect()
    }

    /// First unnamed user layer (for "Add Layer…").
    pub fn free_user_layer(&self) -> Option<usize> {
        (FIRST_USER_LAYER..LAYER_COUNT).find(|i| self.layers[*i].is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_stay_and_user_layers_are_named() {
        let mut t = TagsAndLayers::default();
        assert_eq!(t.layers.len(), 32);
        assert_eq!(t.name_to_layer("UI"), 4, "old scenes keep layers 0–5");
        assert_eq!(t.name_to_layer("Player"), 5);
        assert_eq!(t.name_to_layer(""), -1);
        t.layers[0] = "Renamed".into();
        t.layers[9] = " Hidden ".into();
        t.tags = vec!["Enemy".into(), "".into(), "Enemy".into(), "Player".into()];
        t.sanitize();
        assert_eq!(t.layer_name(0), "Default", "built-in names can't change");
        assert_eq!(t.name_to_layer("Hidden"), 9);
        assert_eq!(
            t.get_mask(&["Hidden", "Water", "Nope"]),
            (1 << 9) | (1 << 3)
        );
        assert_eq!(t.tags, ["Enemy"], "no blanks, duplicates or built-ins");
        assert!(t.has_tag("Enemy") && t.has_tag("MainCamera") && !t.has_tag("Boss"));
        assert_eq!(t.free_user_layer(), Some(8));
        assert_eq!(t.named_layers().len(), 7);
    }

    #[test]
    fn short_or_missing_lists_load() {
        let t: TagsAndLayers = serde_json::from_str(r#"{"layers":["x"]}"#).unwrap();
        let mut t = t;
        t.sanitize();
        assert_eq!(t.layers.len(), 32);
        assert_eq!(t.layer_name(1), "TransparentFX");
    }
}
