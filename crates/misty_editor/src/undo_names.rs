//! Human names for undo steps ("Change Light Intensity", "Move Cube", "Delete 3 Objects").

use misty_engine::scene::Scene;
use serde_json::Value;

use crate::selection::Selection;

/// "Cube" for one selected object, "3 Objects" for several, "Selection" for none.
pub fn objects(scene: &Scene, selection: &Selection) -> String {
    match selection.list() {
        [] => "Selection".to_owned(),
        [one] if scene.contains(*one) => scene.name(*one),
        list => format!("{} Objects", list.len()),
    }
}

/// "halo_size" / "haloSize" → "Halo Size".
pub fn humanize(key: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut prev_lower = false;
    for c in key.chars() {
        if c == '_' || c == ' ' || c == '-' {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        word.push(c);
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
        .iter()
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(f) => f.to_uppercase().chain(cs).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The keys leading to the first value that differs between `before` and `after`.
fn changed_path<'a>(before: &'a Value, after: &'a Value, path: &mut Vec<&'a str>) -> bool {
    match (before, after) {
        (Value::Object(b), Value::Object(a)) => {
            for (k, av) in a {
                match b.get(k) {
                    Some(bv) if bv == av => continue,
                    Some(bv) => {
                        path.push(k);
                        changed_path(bv, av, path);
                        return true;
                    }
                    None => {
                        path.push(k);
                        return true;
                    }
                }
            }
            // A key was removed.
            if let Some(k) = b.keys().find(|k| !a.contains_key(*k)) {
                path.push(k);
                return true;
            }
            false
        }
        (b, a) => b != a,
    }
}

/// Name of the first field that changed: the last key, prefixed with its parent object when
/// nested ("Halo Size"), or `None` when nothing changed. Array indices are skipped.
pub fn changed_field(before: &Value, after: &Value) -> Option<String> {
    let mut path = Vec::new();
    if !changed_path(before, after, &mut path) {
        return None;
    }
    let named: Vec<&str> = path
        .into_iter()
        .filter(|k| k.parse::<usize>().is_err())
        .collect();
    match named.as_slice() {
        [] => None,
        [one] => Some(humanize(one)),
        [.., parent, leaf] => {
            let (p, l) = (humanize(parent), humanize(leaf));
            // "Shadows Shadow Strength" reads badly: keep the leaf when it already says it.
            if l.split(' ')
                .any(|w| w.len() > 2 && p.starts_with(w.trim_end_matches('s')))
            {
                Some(l)
            } else {
                Some(format!("{p} {l}"))
            }
        }
    }
}

/// "Change Fog Density": `prefix` (may be empty) and the first field that changed between two
/// values of a settings struct.
pub fn struct_change<T: serde::Serialize>(prefix: &str, before: &T, after: &T) -> String {
    let field = match (serde_json::to_value(before), serde_json::to_value(after)) {
        (Ok(b), Ok(a)) => changed_field(&b, &a),
        _ => None,
    };
    match (prefix, field) {
        ("", Some(f)) => format!("Change {f}"),
        ("", None) => "Change Settings".to_owned(),
        (p, Some(f)) => format!("Change {p} {f}"),
        (p, None) => format!("Change {p}"),
    }
}

/// "Change Light Intensity": the component name and the field that changed.
pub fn component_change(component: &str, before: &Value, after: &Value) -> String {
    match changed_field(before, after) {
        Some(field) if field.eq_ignore_ascii_case(component) => format!("Change {component}"),
        Some(field) => format!("Change {component} {field}"),
        None => format!("Change {component}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn humanize_snake_and_camel_case() {
        assert_eq!(humanize("intensity"), "Intensity");
        assert_eq!(humanize("halo_size"), "Halo Size");
        assert_eq!(humanize("castShadows"), "Cast Shadows");
    }

    #[test]
    fn names_the_first_changed_field() {
        let b = json!({"intensity": 1.0, "range": 10.0, "halo": {"size": 1.0}});
        let a = json!({"intensity": 2.0, "range": 10.0, "halo": {"size": 1.0}});
        assert_eq!(component_change("Light", &b, &a), "Change Light Intensity");
        let a = json!({"intensity": 1.0, "range": 10.0, "halo": {"size": 3.0}});
        assert_eq!(component_change("Light", &b, &a), "Change Light Halo Size");
        assert_eq!(component_change("Light", &b, &b), "Change Light");
        // Array indices are not names.
        let b = json!({"materials": ["a"]});
        let a = json!({"materials": ["b"]});
        assert_eq!(changed_field(&b, &a).as_deref(), Some("Materials"));
    }

    #[test]
    fn names_the_selection() {
        let mut scene = Scene::default_3d("t");
        let mut sel = Selection::default();
        assert_eq!(objects(&scene, &sel), "Selection");
        let cube = scene.spawn("Cube", None);
        sel.set(cube);
        assert_eq!(objects(&scene, &sel), "Cube");
        let other = scene.spawn("Other", None);
        sel.set_many(vec![cube, other]);
        assert_eq!(objects(&scene, &sel), "2 Objects");
    }
}
