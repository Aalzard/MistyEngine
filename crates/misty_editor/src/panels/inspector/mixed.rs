//! Mixed values while several objects are edited: fields whose value differs between the
//! targets show "—", like the familiar Inspector.
//!
//! Before a component's fields are drawn, [`begin`] compares the component's JSON on every target
//! and remembers the differing paths ("fov", "center/1", "surface/friction"). Editors wrap their
//! widgets with the helpers below, keyed by the field's JSON name.

use std::cell::RefCell;
use std::collections::HashSet;

use egui::{Checkbox, DragValue, Response, Slider, Ui, WidgetText};
use misty_engine::hecs::Entity;
use misty_engine::scene::registry;
use serde_json::Value;

use crate::state::EditorState;

pub const DASH: &str = "—";

thread_local! {
    static MIXED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// Compute the differing fields of component `name` across `targets` (empty for one target).
pub fn begin(state: &EditorState, targets: &[Entity], name: &str) {
    let mut out = HashSet::new();
    if let Some(info) = registry::find(name)
        && let Some(first) = (info.to_json)(&state.scene.world, targets[0])
    {
        for t in &targets[1..] {
            if let Some(other) = (info.to_json)(&state.scene.world, *t) {
                diff(&first, &other, String::new(), &mut out);
            }
        }
    }
    MIXED.with(|m| *m.borrow_mut() = out);
}

/// Forget the differing fields (after the component is drawn).
pub fn end() {
    MIXED.with(|m| m.borrow_mut().clear());
}

/// Leaf paths where `a` and `b` differ.
pub fn diff(a: &Value, b: &Value, path: String, out: &mut HashSet<String>) {
    let join = |k: &str| {
        if path.is_empty() {
            k.to_owned()
        } else {
            format!("{path}/{k}")
        }
    };
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, v) in x {
                match y.get(k) {
                    Some(w) => diff(v, w, join(k), out),
                    None => {
                        out.insert(join(k));
                    }
                }
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (v, w)) in x.iter().zip(y).enumerate() {
                diff(v, w, join(&i.to_string()), out);
            }
        }
        _ if a != b => {
            out.insert(path);
        }
        _ => {}
    }
}

/// The field at `key` (or anything inside it) differs between the targets.
pub fn is(key: &str) -> bool {
    MIXED.with(|m| {
        m.borrow()
            .iter()
            .any(|p| p == key || p.strip_prefix(key).is_some_and(|r| r.starts_with('/')))
    })
}

/// Per-axis flags for a vector field.
pub fn axes(key: &str) -> [bool; 3] {
    std::array::from_fn(|i| is(&format!("{key}/{i}")))
}

/// A number field that shows "—" when mixed.
pub fn drag<'a>(key: &str, dv: DragValue<'a>) -> DragValue<'a> {
    if is(key) {
        dv.custom_formatter(|_, _| DASH.to_owned())
    } else {
        dv
    }
}

/// A slider whose value box shows "—" when mixed.
pub fn slider<'a>(key: &str, s: Slider<'a>) -> Slider<'a> {
    if is(key) {
        s.custom_formatter(|_, _| DASH.to_owned())
    } else {
        s
    }
}

/// A checkbox drawn indeterminate when mixed.
pub fn checkbox(ui: &mut Ui, key: &str, value: &mut bool, text: impl Into<WidgetText>) -> Response {
    ui.add(Checkbox::new(value, text).indeterminate(is(key)))
}

/// Combo box / slot text: "—" when mixed.
pub fn text(key: &str, text: impl Into<WidgetText>) -> WidgetText {
    if is(key) { DASH.into() } else { text.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn diff_finds_differing_leaves() {
        let mut out = HashSet::new();
        diff(
            &json!({"fov": 60, "center": [0, 1, 2], "surface": {"friction": 0.5, "b": 1}}),
            &json!({"fov": 60, "center": [0, 5, 2], "surface": {"friction": 0.7, "b": 1}}),
            String::new(),
            &mut out,
        );
        let mut got: Vec<_> = out.into_iter().collect();
        got.sort();
        assert_eq!(got, ["center/1", "surface/friction"]);
    }

    #[test]
    fn is_matches_the_field_and_its_children() {
        MIXED.with(|m| *m.borrow_mut() = ["center/1".to_owned()].into());
        assert!(is("center") && is("center/1"));
        assert!(!is("cent") && !is("center/0"));
        assert_eq!(axes("center"), [false, true, false]);
        end();
        assert!(!is("center"));
    }
}
