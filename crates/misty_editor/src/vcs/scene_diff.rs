//! A readable per-object diff of two versions of a scene: objects added, removed or changed,
//! components added or removed, and each changed field with its old and new value.

use serde_json::Value;

use super::scene_doc::{Obj, SceneDoc, ref_target};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Added,
    Removed,
    Changed,
}

impl Kind {
    pub fn sign(self) -> &'static str {
        match self {
            Kind::Added => "+",
            Kind::Removed => "-",
            Kind::Changed => "~",
        }
    }
}

/// One field that differs (`Transform.position`, `lighting.fog.enabled`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldChange {
    pub field: String,
    /// Shown values; `None` = the field didn't exist on that side.
    pub before: Option<String>,
    pub after: Option<String>,
}

impl FieldChange {
    pub fn text(&self) -> String {
        let show = |v: &Option<String>| v.clone().unwrap_or_else(|| "(none)".into());
        format!(
            "{}: {} → {}",
            self.field,
            show(&self.before),
            show(&self.after)
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectChange {
    /// Hierarchy path of the object.
    pub path: String,
    pub kind: Kind,
    pub components_added: Vec<String>,
    pub components_removed: Vec<String>,
    pub fields: Vec<FieldChange>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SceneDiff {
    pub objects: Vec<ObjectChange>,
    /// Lighting / navigation changes.
    pub settings: Vec<FieldChange>,
    /// The objects both versions have are in a different Hierarchy order.
    pub reordered: bool,
}

impl SceneDiff {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.settings.is_empty() && !self.reordered
    }

    pub fn count(&self, kind: Kind) -> usize {
        self.objects.iter().filter(|o| o.kind == kind).count()
    }

    /// Plain-text report, one object per block.
    pub fn text(&self) -> String {
        if self.is_empty() {
            return "No differences.\n".into();
        }
        let mut out = format!(
            "{} added, {} removed, {} changed\n",
            self.count(Kind::Added),
            self.count(Kind::Removed),
            self.count(Kind::Changed)
        );
        for o in &self.objects {
            out.push_str(&format!("{} {}\n", o.kind.sign(), o.path));
            for c in &o.components_added {
                out.push_str(&format!("    + component {c}\n"));
            }
            for c in &o.components_removed {
                out.push_str(&format!("    - component {c}\n"));
            }
            for f in &o.fields {
                out.push_str(&format!("    {}\n", f.text()));
            }
        }
        if !self.settings.is_empty() {
            out.push_str("~ Scene settings\n");
            for f in &self.settings {
                out.push_str(&format!("    {}\n", f.text()));
            }
        }
        if self.reordered {
            out.push_str("~ Hierarchy order changed\n");
        }
        out
    }
}

/// Compare two scene file texts.
pub fn diff_text(before: &str, after: &str) -> Result<SceneDiff, String> {
    Ok(diff(&SceneDoc::parse(before)?, &SceneDoc::parse(after)?))
}

pub fn diff(before: &SceneDoc, after: &SceneDoc) -> SceneDiff {
    let mut out = SceneDiff::default();
    for b in &before.objects {
        match after.get(&b.key) {
            None => out.objects.push(ObjectChange {
                path: b.key.clone(),
                kind: Kind::Removed,
                components_added: Vec::new(),
                components_removed: b.components.keys().cloned().collect(),
                fields: Vec::new(),
            }),
            Some(a) => {
                if let Some(c) = object_change(b, a) {
                    out.objects.push(c);
                }
            }
        }
    }
    for a in &after.objects {
        if before.get(&a.key).is_none() {
            out.objects.push(ObjectChange {
                path: a.key.clone(),
                kind: Kind::Added,
                components_added: a.components.keys().cloned().collect(),
                components_removed: Vec::new(),
                fields: Vec::new(),
            });
        }
    }
    let empty = Value::Null;
    let keys: std::collections::BTreeSet<&String> = before
        .settings
        .keys()
        .chain(after.settings.keys())
        .collect();
    for k in keys.into_iter().filter(|k| *k != "version") {
        let (b, a) = (before.settings.get(k), after.settings.get(k));
        field_changes(
            k,
            b.unwrap_or(&empty),
            a.unwrap_or(&empty),
            b.is_some(),
            a.is_some(),
            &mut out.settings,
        );
    }
    let common_b: Vec<&str> = before
        .objects
        .iter()
        .filter(|o| after.get(&o.key).is_some())
        .map(|o| o.key.as_str())
        .collect();
    let common_a: Vec<&str> = after
        .objects
        .iter()
        .filter(|o| before.get(&o.key).is_some())
        .map(|o| o.key.as_str())
        .collect();
    out.reordered = common_b != common_a;
    out
}

/// What changed on one object both versions have.
pub fn object_change(b: &Obj, a: &Obj) -> Option<ObjectChange> {
    if b.components == a.components {
        return None;
    }
    let mut c = ObjectChange {
        path: a.key.clone(),
        kind: Kind::Changed,
        components_added: Vec::new(),
        components_removed: Vec::new(),
        fields: Vec::new(),
    };
    let names: std::collections::BTreeSet<&String> =
        b.components.keys().chain(a.components.keys()).collect();
    for name in names {
        match (b.components.get(name), a.components.get(name)) {
            (Some(x), Some(y)) => field_changes(name, x, y, true, true, &mut c.fields),
            (None, Some(_)) => c.components_added.push(name.clone()),
            (Some(_), None) => c.components_removed.push(name.clone()),
            (None, None) => {}
        }
    }
    Some(c)
}

/// Changes between two values at `path`, recursing into objects and lists of objects; plain
/// values and number lists (vectors, colours) are compared whole.
fn field_changes(
    path: &str,
    b: &Value,
    a: &Value,
    has_b: bool,
    has_a: bool,
    out: &mut Vec<FieldChange>,
) {
    if has_b && has_a && b == a {
        return;
    }
    match (b, a) {
        (Value::Object(x), Value::Object(y)) if has_b && has_a => {
            let keys: std::collections::BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            let null = Value::Null;
            for k in keys {
                let (vx, vy) = (x.get(k), y.get(k));
                field_changes(
                    &format!("{path}.{k}"),
                    vx.unwrap_or(&null),
                    vy.unwrap_or(&null),
                    vx.is_some(),
                    vy.is_some(),
                    out,
                );
            }
        }
        (Value::Array(x), Value::Array(y))
            if has_b && has_a && x.len() == y.len() && x.iter().chain(y).any(Value::is_object) =>
        {
            for (i, (vx, vy)) in x.iter().zip(y).enumerate() {
                field_changes(&format!("{path}[{i}]"), vx, vy, true, true, out);
            }
        }
        _ => out.push(FieldChange {
            field: path.to_owned(),
            before: has_b.then(|| show(b)),
            after: has_a.then(|| show(a)),
        }),
    }
}

/// A value as people read it: numbers trimmed, references as object paths, long values cut.
pub fn show(v: &Value) -> String {
    let s = match v {
        Value::String(_) if ref_target(v).is_some() => format!("→ {}", ref_target(v).unwrap_or("")),
        Value::Array(items) if items.iter().all(Value::is_number) => {
            let parts: Vec<String> = items.iter().map(short_number).collect();
            format!("({})", parts.join(", "))
        }
        Value::Number(_) => short_number(v),
        other => {
            let mut v = other.clone();
            show_refs(&mut v);
            serde_json::to_string(&v).unwrap_or_default()
        }
    };
    if s.chars().count() > 120 {
        let cut: String = s.chars().take(117).collect();
        format!("{cut}…")
    } else {
        s
    }
}

fn short_number(v: &Value) -> String {
    match v.as_f64() {
        Some(f) if v.is_f64() => {
            let r = (f * 1000.0).round() / 1000.0;
            format!("{r}")
        }
        _ => v.to_string(),
    }
}

fn show_refs(v: &mut Value) {
    match v {
        Value::String(s) => {
            if let Some(t) = ref_target(&Value::String(s.clone())) {
                *s = format!("→ {t}");
            }
        }
        Value::Array(items) => items.iter_mut().for_each(show_refs),
        Value::Object(m) => m.values_mut().for_each(show_refs),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::scene_doc::tests::sample;
    use super::*;

    #[test]
    fn same_scene_has_no_differences() {
        let d = diff_text(&sample(), &sample()).unwrap();
        assert!(d.is_empty());
        assert_eq!(d.text(), "No differences.\n");
    }

    #[test]
    fn objects_added_removed_changed_and_field_changes() {
        let after = sample()
            .replace(r#""fov": 60.0"#, r#""fov": 75.0"#)
            .replace(
                r#"{ "components": { "GameObject": { "name": "Child" } }, "parent": 1 },"#,
                "",
            )
            .replace(
                r#""Transform": { "position": [0.0, 1.0, 0.0] }"#,
                r#""Transform": { "position": [0.0, 2.5, 0.0] }, "Rigidbody": { "mass": 1.0 }"#,
            )
            .replace(r#""target": 1"#, r#""target": 0"#)
            .replace(r#""enabled": false"#, r#""enabled": true"#)
            .replace(
                r#"{ "components": { "GameObject": { "name": "Cube" } }, "parent": null }
  ],"#,
                r#"{ "components": { "GameObject": { "name": "Cube" } }, "parent": null },
    { "components": { "GameObject": { "name": "Enemy" } }, "parent": null }
  ],"#,
            );
        // Removing Child shifted every later index: the Button now aims at index 0, the camera.
        let d = diff_text(&sample(), &after).unwrap();
        let text = d.text();
        assert!(text.starts_with("1 added, 1 removed, 3 changed"), "{text}");
        assert!(text.contains("- Player/Child"), "{text}");
        assert!(text.contains("+ Enemy"), "{text}");
        assert!(
            text.contains("~ Main Camera\n    Camera.fov: 60 → 75"),
            "{text}"
        );
        assert!(text.contains("~ Player\n    + component Rigidbody\n    Transform.position: (0, 1, 0) → (0, 2.5, 0)"), "{text}");
        assert!(
            text.contains("Button.on_click[0].target: → Player → → Main Camera"),
            "references compare by object: {text}"
        );
        assert!(
            text.contains("~ Scene settings\n    lighting.fog.enabled: false → true"),
            "{text}"
        );
        assert!(!d.reordered);
    }

    #[test]
    fn a_shifted_index_is_not_a_change() {
        // Insert an object before Player: every index after it moves, the Button still aims at
        // Player.
        let after = sample()
            .replace(
                r#"{ "components": { "GameObject": { "name": "Player" }"#,
                r#"{ "components": { "GameObject": { "name": "Sun" } }, "parent": null },
    { "components": { "GameObject": { "name": "Player" }"#,
            )
            .replace(r#""parent": 1 }"#, r#""parent": 2 }"#)
            .replace(r#""target": 1"#, r#""target": 2"#);
        let d = diff_text(&sample(), &after).unwrap();
        assert_eq!(
            d.text(),
            "1 added, 0 removed, 0 changed\n+ Sun\n    + component GameObject\n"
        );
    }

    #[test]
    fn reorder_is_reported() {
        let doc = SceneDoc::parse(&sample()).unwrap();
        let mut moved = doc.clone();
        let cam = moved.objects.remove(0);
        moved.objects.push(cam);
        let d = diff(&doc, &moved);
        assert!(d.reordered && d.objects.is_empty());
        assert!(d.text().contains("Hierarchy order changed"));
    }
}
