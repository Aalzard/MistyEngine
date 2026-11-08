//! Stable object ids inside prefabs.
//!
//! Every object of a prefab asset can carry a `"Prefab Source": { "id": … }` component: its id in
//! the asset of the nearest prefab instance that contains it. An instance copies the ids, so each
//! instance object knows which asset object it came from even after renames and added children.
//!
//! The id of an object *at the level of a list's first object* (its "level id") is:
//! - the root: [`ROOT`];
//! - an object directly in the prefab: its own id;
//! - an object inside a nested instance `N`: `combine(level id of N, its own id)`, so two instances
//!   of the same prefab inside one asset never share ids;
//! - an object without an id (files saved before ids, objects added by hand): a hash of its
//!   parent's level id, its name and how many same-named siblings without ids come before it.
//!   Both sides of a comparison compute the same fallback, so older prefabs keep working.
//!
//! Ids are kept below 2^53 so the files stay exact for readers that parse numbers as doubles.

use std::collections::{HashMap, HashSet};

use super::super::file::ObjectEntry;

/// Level id of the first object of a list.
pub const ROOT: u64 = 0;
/// Registry name of the id component ([`super::super::PrefabSource`]).
pub const SOURCE: &str = "Prefab Source";
/// Registry name of the instance link ([`super::super::PrefabLink`]).
pub const LINK: &str = "Prefab";

const MASK: u64 = (1 << 53) - 1;

/// Keep an id in range and away from [`ROOT`].
fn tidy(id: u64) -> u64 {
    match id & MASK {
        ROOT => 1,
        id => id,
    }
}

/// A new random id (std's randomly keyed hasher over a counter and the clock; no new dependency).
pub fn fresh_id() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    h.write_u64(nanos);
    tidy(h.finish())
}

/// SplitMix64 finaliser: deterministic across runs and machines.
fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Level id of an object with id `inner` inside a nested instance whose level id is `outer`.
pub fn combine(outer: u64, inner: u64) -> u64 {
    tidy(mix(outer.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ mix(inner)))
}

/// Fallback level id of an object without an id (FNV-1a, then mixed).
fn fallback(parent: u64, name: &str, occurrence: usize) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01B3);
        }
    };
    feed(&parent.to_le_bytes());
    feed(name.as_bytes());
    feed(&(occurrence as u64).to_le_bytes());
    tidy(mix(h))
}

/// The object's own `Prefab Source` id, if it has one.
pub(in crate::scene) fn source_of(entry: &ObjectEntry) -> Option<u64> {
    entry.components.get(SOURCE)?.get("id")?.as_u64()
}

/// The object is the root of a prefab instance.
pub(in crate::scene) fn is_link(entry: &ObjectEntry) -> bool {
    entry.components.contains_key(LINK)
}

pub(in crate::scene) fn name_of(entry: &ObjectEntry) -> &str {
    entry
        .components
        .get("GameObject")
        .and_then(|g| g.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("GameObject")
}

/// Level ids of a list of objects (the first one is the level's root), and for each object the
/// nested instance root (index) it lives in, if any. Repeated ids (a duplicated object) keep the
/// first one; the others get `None` and count as objects the prefab doesn't have.
pub(in crate::scene) fn scoped_level_ids(
    entries: &[ObjectEntry],
) -> (Vec<Option<u64>>, Vec<Option<usize>>) {
    let n = entries.len();
    let mut ids: Vec<Option<u64>> = vec![None; n];
    let mut scope: Vec<Option<usize>> = vec![None; n];
    if n == 0 {
        return (ids, scope);
    }
    ids[0] = Some(ROOT);
    let mut seen = HashSet::from([ROOT]);
    let mut occurrences: HashMap<(u64, &str), usize> = HashMap::new();
    for i in 1..n {
        let Some(p) = entries[i].parent.filter(|p| *p < i) else {
            continue;
        };
        scope[i] = if p != 0 && is_link(&entries[p]) {
            Some(p)
        } else {
            scope[p]
        };
        let id = match (source_of(&entries[i]), scope[i]) {
            (Some(own), None) => Some(own),
            (Some(own), Some(s)) => ids[s].map(|outer| combine(outer, own)),
            (None, _) => ids[p].map(|parent| {
                let name = name_of(&entries[i]);
                let k = occurrences.entry((parent, name)).or_insert(0);
                *k += 1;
                fallback(parent, name, *k - 1)
            }),
        };
        ids[i] = id.filter(|id| seen.insert(*id));
    }
    (ids, scope)
}

/// Level ids of a list of objects (see [`scoped_level_ids`]).
pub(in crate::scene) fn level_ids(entries: &[ObjectEntry]) -> Vec<Option<u64>> {
    scoped_level_ids(entries).0
}

/// Write each object's level id as its `Prefab Source` id where it has none, for the objects
/// directly in the prefab (objects inside nested instances keep their own ids). The root never
/// stores one: an instance root's id belongs to whatever contains the instance.
pub(in crate::scene) fn materialize(entries: &mut [ObjectEntry]) {
    let (ids, scope) = scoped_level_ids(entries);
    if let Some(root) = entries.first_mut() {
        root.components.remove(SOURCE);
    }
    for i in 1..entries.len() {
        if scope[i].is_some() || source_of(&entries[i]).is_some() {
            continue;
        }
        if let Some(id) = ids[i] {
            entries[i]
                .components
                .insert(SOURCE.to_owned(), serde_json::json!({ "id": id }));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    fn entry(parent: Option<usize>, name: &str, id: Option<u64>, link: bool) -> ObjectEntry {
        let mut components = BTreeMap::new();
        components.insert("GameObject".to_owned(), json!({ "name": name }));
        if let Some(id) = id {
            components.insert(SOURCE.to_owned(), json!({ "id": id }));
        }
        if link {
            components.insert(LINK.to_owned(), json!({ "path": "Assets/B.prefab" }));
        }
        ObjectEntry { parent, components }
    }

    #[test]
    fn explicit_nested_and_fallback_ids() {
        let entries = vec![
            entry(None, "A", None, false),
            entry(Some(0), "Lid", Some(7), false),
            entry(Some(0), "B1", Some(20), true),
            entry(Some(2), "Part", Some(5), false),
            entry(Some(0), "B2", Some(21), true),
            entry(Some(4), "Part", Some(5), false),
            entry(Some(0), "Loose", None, false),
            entry(Some(0), "Loose", None, false),
        ];
        let ids = level_ids(&entries);
        assert_eq!(ids[0], Some(ROOT));
        assert_eq!(ids[1], Some(7));
        assert_eq!(ids[3], Some(combine(20, 5)));
        assert_eq!(ids[5], Some(combine(21, 5)));
        assert_ne!(ids[3], ids[5], "two nested instances never share ids");
        assert_eq!(ids[6], Some(fallback(ROOT, "Loose", 0)));
        assert_eq!(ids[7], Some(fallback(ROOT, "Loose", 1)));
        assert!(ids.iter().all(|id| id.is_some_and(|id| id <= MASK)));
    }

    #[test]
    fn duplicates_keep_the_first_id() {
        let entries = vec![
            entry(None, "A", None, false),
            entry(Some(0), "Lid", Some(7), false),
            entry(Some(0), "Lid (1)", Some(7), false),
        ];
        assert_eq!(level_ids(&entries), [Some(ROOT), Some(7), None]);
    }

    #[test]
    fn materialize_matches_the_fallback_and_skips_nested_objects() {
        let mut entries = vec![
            entry(None, "A", Some(99), false),
            entry(Some(0), "Lid", None, false),
            entry(Some(0), "B", None, true),
            entry(Some(2), "Part", None, false),
        ];
        let before = level_ids(&entries);
        materialize(&mut entries);
        assert_eq!(source_of(&entries[0]), None, "the root stores no id");
        assert_eq!(source_of(&entries[1]), before[1]);
        assert_eq!(source_of(&entries[2]), before[2]);
        assert_eq!(
            source_of(&entries[3]),
            None,
            "nested objects keep their own ids"
        );
        assert_eq!(level_ids(&entries), before);
    }

    #[test]
    fn fresh_ids_differ_and_stay_in_range() {
        let a = fresh_id();
        let b = fresh_id();
        assert_ne!(a, b);
        assert!(a <= MASK && a != ROOT);
    }
}
