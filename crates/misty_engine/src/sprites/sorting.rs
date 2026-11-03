//! Sorting Layers (Project Settings ▸ Sorting Layers): the order 2D renderers are drawn in.
//!
//! The project's list is installed process-wide when settings load. A layer's value is its
//! position relative to "Default", so Default is always 0, layers above it in the list are
//! negative (drawn first) and layers below it positive (drawn last).

use std::sync::RwLock;

pub const DEFAULT: &str = "Default";

static LAYERS: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// The default list: just "Default".
pub fn default_layers() -> Vec<String> {
    vec![DEFAULT.to_owned()]
}

/// Install the project's Sorting Layers (Default is added at the top when missing).
pub fn set_layers(layers: &[String]) {
    *LAYERS.write().unwrap_or_else(|e| e.into_inner()) = normalized(layers);
}

/// The installed layer names in draw order.
pub fn layers() -> Vec<String> {
    let l = LAYERS.read().unwrap_or_else(|e| e.into_inner());
    if l.is_empty() {
        default_layers()
    } else {
        l.clone()
    }
}

/// Unique names with "Default" present.
pub fn normalized(layers: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in layers {
        let l = l.trim();
        if !l.is_empty() && !out.iter().any(|o| o == l) {
            out.push(l.to_owned());
        }
    }
    if !out.iter().any(|o| o == DEFAULT) {
        out.insert(0, DEFAULT.to_owned());
    }
    out
}

/// Sort value of a layer in `list`; unknown names sort as Default.
pub fn value_in(list: &[String], name: &str) -> i32 {
    let default = list.iter().position(|l| l == DEFAULT).unwrap_or(0) as i32;
    list.iter()
        .position(|l| l == name)
        .map_or(0, |i| i as i32 - default)
}

/// Sort value of a layer in the installed list.
pub fn value(name: &str) -> i32 {
    let l = LAYERS.read().unwrap_or_else(|e| e.into_inner());
    value_in(&l, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_relative_to_default() {
        let list = normalized(&["Background".into(), "Default".into(), "Foreground".into()]);
        assert_eq!(value_in(&list, "Background"), -1);
        assert_eq!(value_in(&list, "Default"), 0);
        assert_eq!(value_in(&list, "Foreground"), 1);
        assert_eq!(
            value_in(&list, "Missing"),
            0,
            "unknown layers sort as Default"
        );
        let added = normalized(&["Back".into(), "Back".into(), " ".into()]);
        assert_eq!(added, vec!["Default".to_owned(), "Back".to_owned()]);
    }
}
