//! A `.scene` file read for comparing and merging: objects keyed by their Hierarchy path (scene
//! files store no ids, only list order and parent indices), with object references turned from
//! list indices into those paths so two versions compare by meaning, not by position.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Map, Value};

/// Marks a reference field's value once it names an object by path.
const REF: &str = "@object:";

/// One scene object.
#[derive(Clone, Debug, PartialEq)]
pub struct Obj {
    /// Hierarchy path: `Parent/Child`, duplicate sibling names numbered (`Cube [2]`).
    pub key: String,
    pub parent: Option<String>,
    pub name: String,
    /// Component name → fields, references as paths.
    pub components: BTreeMap<String, Value>,
}

/// A whole scene file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneDoc {
    /// Top-level blocks other than `objects` (lighting, navigation, version).
    pub settings: BTreeMap<String, Value>,
    /// In file (Hierarchy) order.
    pub objects: Vec<Obj>,
}

impl SceneDoc {
    pub fn parse(text: &str) -> Result<SceneDoc, String> {
        let root: Value =
            serde_json::from_str(text).map_err(|e| format!("not a scene file: {e}"))?;
        let Value::Object(mut top) = root else {
            return Err("not a scene file: expected an object".into());
        };
        let list = match top.remove("objects") {
            Some(Value::Array(list)) => list,
            _ => Vec::new(),
        };
        // Keys first (parents always come earlier in the list).
        let mut keys: Vec<String> = Vec::with_capacity(list.len());
        let mut parents: Vec<Option<usize>> = Vec::with_capacity(list.len());
        let mut seen: HashMap<String, usize> = HashMap::new();
        for o in &list {
            let parent = o
                .get("parent")
                .and_then(Value::as_u64)
                .map(|p| p as usize)
                .filter(|p| *p < keys.len());
            let name = object_name(o);
            let base = match parent {
                Some(p) => format!("{}/{name}", keys[p]),
                None => name,
            };
            let n = seen.entry(base.clone()).or_insert(0);
            *n += 1;
            let key = if *n == 1 {
                base
            } else {
                format!("{base} [{n}]")
            };
            keys.push(key);
            parents.push(parent);
        }
        let objects = list
            .into_iter()
            .enumerate()
            .map(|(i, o)| {
                let name = object_name(&o);
                let mut components: BTreeMap<String, Value> = match o.get("components") {
                    Some(Value::Object(m)) => m.clone().into_iter().collect(),
                    _ => BTreeMap::new(),
                };
                for_each_ref(&mut components, |v| {
                    *v = match v.as_u64().and_then(|i| keys.get(i as usize)) {
                        Some(k) => Value::String(format!("{REF}{k}")),
                        None => Value::Null,
                    };
                });
                Obj {
                    key: keys[i].clone(),
                    parent: parents[i].map(|p| keys[p].clone()),
                    name,
                    components,
                }
            })
            .collect();
        Ok(SceneDoc {
            settings: top.into_iter().collect(),
            objects,
        })
    }

    pub fn get(&self, key: &str) -> Option<&Obj> {
        self.objects.iter().find(|o| o.key == key)
    }

    /// The file text: objects in Hierarchy order (each parent before its children), references
    /// back to list indices (null when the object is gone), pretty JSON like the editor writes.
    pub fn write(&self) -> String {
        let ordered = hierarchy_order(&self.objects);
        let index: HashMap<&str, usize> = ordered
            .iter()
            .enumerate()
            .map(|(i, o)| (o.key.as_str(), i))
            .collect();
        let objects: Vec<Value> = ordered
            .iter()
            .map(|o| {
                let mut components = o.components.clone();
                for_each_ref(&mut components, |v| {
                    let target = v.as_str().and_then(|s| s.strip_prefix(REF));
                    *v = match target.and_then(|k| index.get(k)) {
                        Some(i) => Value::from(*i as u64),
                        None => Value::Null,
                    };
                });
                let parent = o
                    .parent
                    .as_deref()
                    .and_then(|p| index.get(p))
                    .map_or(Value::Null, |i| Value::from(*i as u64));
                let mut m = Map::new();
                m.insert(
                    "components".into(),
                    Value::Object(components.into_iter().collect()),
                );
                m.insert("parent".into(), parent);
                Value::Object(m)
            })
            .collect();
        let mut top: Map<String, Value> = self.settings.clone().into_iter().collect();
        top.insert("objects".into(), Value::Array(objects));
        top.entry("version").or_insert(Value::from(1));
        serde_json::to_string_pretty(&Value::Object(top)).expect("scene JSON serialises")
    }
}

/// The object's name from its GameObject block.
fn object_name(o: &Value) -> String {
    o.pointer("/components/GameObject/name")
        .and_then(Value::as_str)
        .unwrap_or("GameObject")
        .to_owned()
}

/// A reference value shown to people: the object's path.
pub fn ref_target(v: &Value) -> Option<&str> {
    v.as_str().and_then(|s| s.strip_prefix(REF))
}

/// Parents before children, siblings in their given order; objects whose parent is missing
/// become roots.
fn hierarchy_order(objects: &[Obj]) -> Vec<&Obj> {
    let present: std::collections::HashSet<&str> = objects.iter().map(|o| o.key.as_str()).collect();
    let mut children: HashMap<Option<&str>, Vec<&Obj>> = HashMap::new();
    for o in objects {
        let parent = o.parent.as_deref().filter(|p| present.contains(p));
        children.entry(parent).or_default().push(o);
    }
    let mut out = Vec::with_capacity(objects.len());
    fn visit<'a>(
        at: Option<&str>,
        children: &HashMap<Option<&str>, Vec<&'a Obj>>,
        out: &mut Vec<&'a Obj>,
    ) {
        for o in children.get(&at).into_iter().flatten() {
            out.push(o);
            visit(Some(o.key.as_str()), children, out);
        }
    }
    visit(None, &children, &mut out);
    out
}

/// Call `f` on every object-reference value (a single reference, or each item of a list).
fn for_each_ref(components: &mut BTreeMap<String, Value>, mut f: impl FnMut(&mut Value)) {
    let mut one = |v: &mut Value| match v {
        Value::Array(items) => items.iter_mut().for_each(&mut f),
        other => f(other),
    };
    for (comp, list, field) in misty_engine::scene::file::object_reference_fields() {
        let Some(c) = components.get_mut(comp) else {
            continue;
        };
        if list.is_empty() {
            if let Some(v) = c.get_mut(field) {
                one(v);
            }
            continue;
        }
        match c.get_mut(list) {
            Some(Value::Array(items)) => {
                for item in items {
                    if let Some(v) = item.get_mut(field) {
                        one(v);
                    }
                }
            }
            Some(obj @ Value::Object(_)) => {
                if let Some(v) = obj.get_mut(field) {
                    one(v);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A small scene: Main Camera, Player (with a Child), a Button aimed at Player, two Cubes.
    pub fn sample() -> String {
        r#"{
  "lighting": { "fog": { "enabled": false } },
  "objects": [
    { "components": { "GameObject": { "name": "Main Camera" }, "Camera": { "fov": 60.0 } }, "parent": null },
    { "components": { "GameObject": { "name": "Player" }, "Transform": { "position": [0.0, 1.0, 0.0] } }, "parent": null },
    { "components": { "GameObject": { "name": "Child" } }, "parent": 1 },
    { "components": { "GameObject": { "name": "Button" }, "Button": { "on_click": [ { "target": 1, "method": "Jump" } ] } }, "parent": null },
    { "components": { "GameObject": { "name": "Cube" } }, "parent": null },
    { "components": { "GameObject": { "name": "Cube" } }, "parent": null }
  ],
  "version": 1
}"#
        .to_owned()
    }

    #[test]
    fn keys_are_paths_and_refs_become_paths() {
        let doc = SceneDoc::parse(&sample()).unwrap();
        let keys: Vec<&str> = doc.objects.iter().map(|o| o.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "Main Camera",
                "Player",
                "Player/Child",
                "Button",
                "Cube",
                "Cube [2]"
            ]
        );
        assert_eq!(
            doc.get("Player/Child").unwrap().parent.as_deref(),
            Some("Player")
        );
        let target =
            doc.get("Button").unwrap().components["Button"]["on_click"][0]["target"].clone();
        assert_eq!(ref_target(&target), Some("Player"));
        assert!(doc.settings.contains_key("lighting"));
    }

    #[test]
    fn write_round_trips_and_reindexes_references() {
        let doc = SceneDoc::parse(&sample()).unwrap();
        let again = SceneDoc::parse(&doc.write()).unwrap();
        assert_eq!(again, doc);
        // Drop the Main Camera: the Button's reference now points at index 0.
        let mut fewer = doc.clone();
        fewer.objects.remove(0);
        let text = fewer.write();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            v["objects"][2]["components"]["Button"]["on_click"][0]["target"],
            0
        );
        assert_eq!(v["objects"][1]["parent"], 0, "Child under Player");
        assert_eq!(v["version"], 1);
    }

    #[test]
    fn a_real_sample_scene_round_trips() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../SampleProject/Assets/Scenes/AudioDemo.scene");
        let text = std::fs::read_to_string(path).unwrap();
        let doc = SceneDoc::parse(&text).unwrap();
        assert!(doc.objects.len() > 3);
        let back: Value = serde_json::from_str(&doc.write()).unwrap();
        let orig: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back, orig, "same JSON after a parse and write");
    }
}
