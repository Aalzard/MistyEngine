//! Overrides: what a prefab instance changes compared with its asset, one property at a time.
//!
//! Both sides are object lists (the instance's subtree and the asset), matched by level id (see
//! [`super::ids`]). A property is a top-level field of a component's saved JSON
//! ("Transform" ▸ "scale", "Light" ▸ "intensity"). Components and objects can be added or
//! removed too. Some values are always the instance's own and never count (see
//! [`per_instance`]).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::file::{ObjectEntry, remap_refs};
use super::ids::{LINK, SOURCE, level_ids, name_of};

/// One change an instance makes to its prefab. Object ids are level ids of the instance root's
/// asset. Saved in scene files (sorted) as the instance's override record.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Override {
    /// A component field differs from the asset.
    Property {
        object: u64,
        component: String,
        property: String,
    },
    /// The instance has a component the asset's object doesn't.
    AddedComponent { object: u64, component: String },
    /// The asset's object has a component the instance removed.
    RemovedComponent { object: u64, component: String },
    /// An object (and its children) the instance added under the asset's object `parent`.
    AddedObject { parent: u64, name: String },
    /// An asset object (and its children) the instance deleted.
    RemovedObject { object: u64 },
}

impl Override {
    /// The asset object the change is on (the parent for added objects).
    pub fn object(&self) -> u64 {
        match self {
            Override::Property { object, .. }
            | Override::AddedComponent { object, .. }
            | Override::RemovedComponent { object, .. }
            | Override::RemovedObject { object } => *object,
            Override::AddedObject { parent, .. } => *parent,
        }
    }

    /// The component the change is on, if it is about one.
    pub fn component(&self) -> Option<&str> {
        match self {
            Override::Property { component, .. }
            | Override::AddedComponent { component, .. }
            | Override::RemovedComponent { component, .. } => Some(component),
            _ => None,
        }
    }
}

/// Values every instance keeps as its own: they are never overrides and never take the asset's
/// value when the instance follows a changed asset. The root's name, position and rotation (each
/// instance stands somewhere else), the component order (follows added and removed components)
/// and per-object save / network ids.
pub fn per_instance(is_root: bool, component: &str, field: &str) -> bool {
    match (component, field) {
        ("GameObject", "name") | ("Transform", "position" | "rotation" | "euler_hint") => is_root,
        ("GameObject", "component_order")
        | ("Saveable", "save_id")
        | ("Network Object", "global_id") => true,
        _ => false,
    }
}

/// A field that is only a display hint for another and goes with it (the Euler angles the
/// Inspector shows for a rotation).
pub fn follows(component: &str, field: &str) -> Option<&'static str> {
    match (component, field) {
        ("Transform", "euler_hint") => Some("rotation"),
        _ => None,
    }
}

/// Components the comparison never looks at: the prefab bookkeeping itself, and the Node links
/// (not saved).
pub(crate) fn ignored(component: &str) -> bool {
    matches!(component, LINK | SOURCE | "Node")
}

/// Reference value for objects outside the matched ones (added objects, other scene objects).
pub(crate) const OUTSIDE: u64 = u64::MAX;

/// One side of a comparison: its objects, their level ids, and components with object
/// references written as level ids.
pub(in crate::scene) struct Side {
    pub ids: Vec<Option<u64>>,
    pub by_id: HashMap<u64, usize>,
    pub components: Vec<BTreeMap<String, Value>>,
}

impl Side {
    pub fn new(entries: &[ObjectEntry]) -> Self {
        let ids = level_ids(entries);
        let by_id = ids
            .iter()
            .enumerate()
            .filter_map(|(i, id)| id.map(|id| (id, i)))
            .collect();
        let components = entries
            .iter()
            .map(|o| {
                let mut c = o.components.clone();
                remap_refs(&mut c, |index| {
                    Some(
                        ids.get(index as usize)
                            .copied()
                            .flatten()
                            .unwrap_or(OUTSIDE),
                    )
                });
                c
            })
            .collect();
        Self {
            ids,
            by_id,
            components,
        }
    }
}

/// Changes of `instance` (an instance's objects, root first) compared with `asset`, each with
/// the index of the instance object it is on (added objects: the added object; removed objects:
/// none). Removed and added objects are reported once for their whole subtree.
pub(in crate::scene) fn diff(
    instance: &[ObjectEntry],
    asset: &[ObjectEntry],
) -> Vec<(Override, Option<usize>)> {
    let mine = Side::new(instance);
    let theirs = Side::new(asset);
    let mut out = Vec::new();
    // Asset objects the instance lacks (and everything under them).
    let mut gone = vec![false; asset.len()];
    for (j, entry) in asset.iter().enumerate() {
        let Some(id) = theirs.ids[j] else {
            continue;
        };
        if entry.parent.is_some_and(|p| gone[p]) {
            gone[j] = true;
            continue;
        }
        let Some(&i) = mine.by_id.get(&id) else {
            gone[j] = true;
            out.push((Override::RemovedObject { object: id }, None));
            continue;
        };
        compare_object(
            id,
            j == 0,
            &mine.components[i],
            &theirs.components[j],
            i,
            &mut out,
        );
    }
    // Instance objects the asset doesn't have: report the top of each added subtree.
    let added = |i: usize| mine.ids[i].is_none_or(|id| !theirs.by_id.contains_key(&id));
    for (i, entry) in instance.iter().enumerate().skip(1) {
        let Some(p) = entry.parent else {
            continue;
        };
        if !added(i) || added(p) {
            continue;
        }
        if let Some(parent) = mine.ids[p] {
            let name = name_of(entry).to_owned();
            out.push((Override::AddedObject { parent, name }, Some(i)));
        }
    }
    out
}

/// Component and property changes of one matched object.
fn compare_object(
    object: u64,
    is_root: bool,
    mine: &BTreeMap<String, Value>,
    theirs: &BTreeMap<String, Value>,
    index: usize,
    out: &mut Vec<(Override, Option<usize>)>,
) {
    for (component, asset_value) in theirs {
        if ignored(component) {
            continue;
        }
        let Some(value) = mine.get(component) else {
            let component = component.clone();
            out.push((
                Override::RemovedComponent { object, component },
                Some(index),
            ));
            continue;
        };
        for property in changed_fields(is_root, component, value, asset_value) {
            let component = component.clone();
            let o = Override::Property {
                object,
                component,
                property,
            };
            out.push((o, Some(index)));
        }
    }
    for component in mine.keys() {
        if !ignored(component) && !theirs.contains_key(component) {
            let component = component.clone();
            out.push((Override::AddedComponent { object, component }, Some(index)));
        }
    }
}

/// Top-level fields that differ ("" for a component saved as a single value).
pub(crate) fn changed_fields(
    is_root: bool,
    component: &str,
    mine: &Value,
    theirs: &Value,
) -> Vec<String> {
    match (mine, theirs) {
        (Value::Object(a), Value::Object(b)) => {
            let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
            keys.into_iter()
                .filter(|k| !per_instance(is_root, component, k))
                .filter(|k| follows(component, k).is_none())
                .filter(|k| a.get(*k) != b.get(*k))
                .cloned()
                .collect()
        }
        _ if mine != theirs => vec![String::new()],
        _ => Vec::new(),
    }
}

/// The overrides as a sorted record (for scene files and for comparing records).
pub fn record(list: impl IntoIterator<Item = Override>) -> Vec<Override> {
    let mut v: Vec<Override> = list.into_iter().collect();
    v.sort();
    v.dedup();
    v
}

/// Whether `field` of `component` on the object `object` stays the instance's own under `keep`.
pub(crate) fn kept_field(
    keep: &HashSet<Override>,
    object: u64,
    is_root: bool,
    component: &str,
    field: &str,
) -> bool {
    let field = follows(component, field).unwrap_or(field);
    per_instance(is_root, component, field)
        || keep.contains(&Override::Property {
            object,
            component: component.to_owned(),
            property: field.to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn records_are_sorted_and_serialise_readably() {
        let r = record([
            Override::RemovedObject { object: 3 },
            Override::Property {
                object: 0,
                component: "Transform".into(),
                property: "scale".into(),
            },
        ]);
        assert!(matches!(r[0], Override::Property { .. }));
        let text = serde_json::to_value(&r[0]).unwrap();
        assert_eq!(
            text,
            json!({"kind": "property", "object": 0, "component": "Transform", "property": "scale"})
        );
        let back: Override = serde_json::from_value(text).unwrap();
        assert_eq!(back, r[0]);
    }

    #[test]
    fn changed_fields_skip_per_instance_values_and_hints() {
        let a = json!({"position": [1, 0, 0], "scale": [2, 2, 2], "euler_hint": [0, 9, 0]});
        let b = json!({"position": [0, 0, 0], "scale": [1, 1, 1], "euler_hint": [0, 0, 0]});
        assert_eq!(changed_fields(true, "Transform", &a, &b), ["scale"]);
        assert_eq!(
            changed_fields(false, "Transform", &a, &b),
            ["position", "scale"]
        );
        assert_eq!(changed_fields(false, "X", &json!(1), &json!(2)), [""]);
    }
}
