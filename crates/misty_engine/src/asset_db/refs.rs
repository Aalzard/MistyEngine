//! GUIDs of the assets a JSON asset file references.
//!
//! References stay readable project paths in the file's own fields. On save, a top-level
//! `"asset_guids": { path: guid }` table is appended listing the GUID of every referenced asset.
//! On load, a path whose GUID now belongs to another path (the asset was renamed or moved) is
//! rewritten to the new path before the file is parsed; unknown GUIDs fall back to the path.
//! Files without the table (older projects) load as before.

use std::borrow::Cow;
use std::collections::BTreeMap;

use serde_json::Value;

use super::{Guid, GuidDatabase};

/// Name of the table appended to referencing files.
pub const KEY: &str = "asset_guids";

/// "Assets/…" without a `#sub-asset` suffix, when `s` names a project asset file.
pub fn asset_path(s: &str) -> Option<&str> {
    let s = s.split('#').next().unwrap_or(s);
    (s.starts_with("Assets/") && std::path::Path::new(s).extension().is_some()).then_some(s)
}

/// Every string value in `v` (object keys not included), skipping a top-level GUID table.
pub fn strings<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
    match v {
        Value::String(s) => out.push(s),
        Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        Value::Object(o) => o
            .iter()
            .filter(|(k, _)| k.as_str() != KEY)
            .for_each(|(_, x)| strings(x, out)),
        _ => {}
    }
}

/// The `path → GUID` table of a parsed file (empty when it has none).
pub fn table(v: &Value) -> BTreeMap<String, Guid> {
    v.get(KEY).map(entries).unwrap_or_default()
}

/// Valid `path → GUID` entries of a table object.
fn entries(t: &Value) -> BTreeMap<String, Guid> {
    t.as_object()
        .map(|t| {
            t.iter()
                .filter_map(|(p, g)| Some((p.clone(), g.as_str()?.parse().ok()?)))
                .collect()
        })
        .unwrap_or_default()
}

/// `text` with a GUID table for the asset paths it names (unchanged when `db` knows none).
pub fn embed(db: &GuidDatabase, text: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return text.to_owned();
    };
    if !v.is_object() || v.get(KEY).is_some() {
        return text.to_owned();
    }
    let mut found = Vec::new();
    strings(&v, &mut found);
    let table: BTreeMap<&str, String> = found
        .into_iter()
        .filter_map(asset_path)
        .filter_map(|p| Some((p, db.guid_of(p)?.to_string())))
        .collect();
    if table.is_empty() {
        return text.to_owned();
    }
    let body = text.trim_end();
    let Some(inner) = body.strip_suffix('}') else {
        return text.to_owned();
    };
    let inner = inner.trim_end();
    let sep = if inner.ends_with('{') { "" } else { "," };
    let block = serde_json::to_string_pretty(&table)
        .unwrap_or_default()
        .replace('\n', "\n  ");
    let tail = &text[body.len()..];
    format!("{inner}{sep}\n  \"{KEY}\": {block}\n}}{tail}")
}

/// `text` with renamed or moved references pointed at their asset's current path, and the
/// GUID table removed. Borrowed when the file has no table.
pub fn resolve<'a>(db: &GuidDatabase, text: &'a str) -> Cow<'a, str> {
    if !text.contains(&format!("\"{KEY}\"")) {
        return Cow::Borrowed(text);
    }
    let Ok(Value::Object(mut map)) = serde_json::from_str::<Value>(text) else {
        return Cow::Borrowed(text);
    };
    let Some(t) = map.remove(KEY) else {
        return Cow::Borrowed(text);
    };
    let moves: BTreeMap<String, String> = entries(&t)
        .into_iter()
        .filter_map(|(old, g)| {
            let now = db.path_of(g)?;
            (now != old).then(|| (old, now.to_owned()))
        })
        .collect();
    let mut v = Value::Object(map);
    if !moves.is_empty() {
        repoint(&mut v, &moves);
    }
    Cow::Owned(serde_json::to_string(&v).unwrap_or_else(|_| text.to_owned()))
}

/// Rewrite every string naming a moved path (exactly, or with a `#sub-asset` suffix).
fn repoint(v: &mut Value, moves: &BTreeMap<String, String>) {
    match v {
        Value::String(s) => {
            let (base, sub) = s
                .split_once('#')
                .map_or((s.as_str(), None), |(b, r)| (b, Some(r)));
            if let Some(new) = moves.get(base) {
                *s = match sub {
                    Some(r) => format!("{new}#{r}"),
                    None => new.clone(),
                };
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| repoint(x, moves)),
        Value::Object(o) => o.values_mut().for_each(|x| repoint(x, moves)),
        _ => {}
    }
}
