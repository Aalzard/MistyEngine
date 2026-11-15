//! `.meta` sidecar files: `<asset>.meta` next to every asset and folder, JSON with the asset's
//! `guid` first and its import settings (textures) after it.
//!
//! Edits are textual where possible, so a meta file's other settings keep their order.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use super::Guid;

pub const EXTENSION: &str = "meta";

/// Where the meta file of `asset` (a file or folder) lives.
pub fn meta_path(asset: &Path) -> PathBuf {
    let mut name = asset.as_os_str().to_owned();
    name.push(".meta");
    PathBuf::from(name)
}

/// A `.meta` sidecar (by name).
pub fn is_meta(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == EXTENSION)
}

/// The GUID in meta-file text (`None` when missing, invalid, or the text isn't JSON).
pub fn guid_in(text: &str) -> Option<Guid> {
    let v: Value = serde_json::from_str(text).ok()?;
    v.get("guid")?.as_str()?.parse().ok()
}

/// The GUID of `asset` from its meta file.
pub fn read_guid(asset: &Path) -> Option<Guid> {
    let text = crate::vfs::read_to_string(meta_path(asset)).ok()?;
    guid_in(&text)
}

/// `text` (a JSON object) with its `guid` set to `guid`; `None` when it isn't a JSON object.
pub fn with_guid(text: &str, guid: Guid) -> Option<String> {
    let Value::Object(mut map) = serde_json::from_str::<Value>(text).ok()? else {
        return None;
    };
    match map.get("guid").and_then(Value::as_str) {
        // Valid or not, replace the old value in place.
        Some(old) if text.matches(&format!("\"{old}\"")).count() == 1 => {
            Some(text.replacen(&format!("\"{old}\""), &format!("\"{guid}\""), 1))
        }
        Some(_) => {
            map.remove("guid");
            Some(compose(guid, &map))
        }
        None if map.is_empty() => Some(compose(guid, &map)),
        None => {
            // Insert right after the opening brace so the other keys keep their order.
            let i = text.find('{')?;
            Some(format!(
                "{}\n  \"guid\": \"{guid}\",{}",
                &text[..=i],
                &text[i + 1..]
            ))
        }
    }
}

/// Pretty meta text: the GUID, then `rest` (sorted).
pub fn compose(guid: Guid, rest: &Map<String, Value>) -> String {
    if rest.is_empty() {
        return format!("{{\n  \"guid\": \"{guid}\"\n}}\n");
    }
    let body = serde_json::to_string_pretty(&Value::Object(rest.clone())).unwrap_or_default();
    format!("{{\n  \"guid\": \"{guid}\",{}\n", &body[1..])
}

/// Write `guid` into the meta file of `asset`, creating it or keeping its other settings.
pub fn write_guid(asset: &Path, guid: Guid) -> Result<(), String> {
    let path = meta_path(asset);
    let text = match std::fs::read_to_string(&path) {
        // An empty meta file (cut short by a crash or a power cut) holds nothing to keep.
        Ok(old) if old.trim().is_empty() => compose(guid, &Map::new()),
        // A Unity meta file (an asset pack copied in with its `.meta` files): taken over,
        // keeping the import settings Misty has too.
        Ok(old) if is_unity(&old) => compose(guid, &unity_settings(&old)),
        Ok(old) => with_guid(&old, guid)
            .ok_or_else(|| format!("{} is not a JSON object", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => compose(guid, &Map::new()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    super::write_atomic(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Whether `text` is a Unity `.meta` file (YAML; they come along with Unity asset packs).
pub fn is_unity(text: &str) -> bool {
    text.trim_start().starts_with("fileFormatVersion:")
}

/// The first `key: value` line of a Unity meta file.
fn unity_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|l| {
        let (k, v) = l.trim().split_once(':')?;
        (k == key).then_some(v.trim())
    })
}

/// The import settings of a Unity meta file that Misty has as well, as Misty meta fields:
/// models keep their Scale Factor and Convert Units (`globalScale`, `useFileScale`), textures
/// their type (Normal Map), sRGB and Max Size.
pub fn unity_settings(text: &str) -> Map<String, Value> {
    let mut out = Map::new();
    let number = |key: &str| unity_value(text, key).and_then(|v| v.parse::<f64>().ok());
    if text.contains("ModelImporter:") {
        if let Some(s) = number("globalScale").filter(|s| s.is_finite() && *s > 0.0 && *s != 1.0) {
            out.insert("scale_factor".into(), Value::from(s));
        }
        if number("useFileScale") == Some(0.0) {
            out.insert("convert_units".into(), Value::from(false));
        }
    } else if text.contains("TextureImporter:") {
        if number("textureType") == Some(1.0) {
            out.insert("texture_type".into(), Value::from("NormalMap"));
            out.insert("srgb".into(), Value::from(false));
        } else if number("sRGBTexture") == Some(0.0) {
            out.insert("srgb".into(), Value::from(false));
        }
        if let Some(size) = number("maxTextureSize")
            .map(|s| s as u32)
            .filter(|s| *s != 2048 && crate::sprites::import::MAX_SIZES.contains(s))
        {
            out.insert("max_size".into(), Value::from(size));
        }
    }
    out
}

/// Write import `settings` for `asset`, keeping the GUID already in its meta file. Without one,
/// the project's GUID database (when open) assigns it; otherwise the next scan does.
pub fn write_settings<T: Serialize>(asset: &Path, settings: &T) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())? + "\n";
    let guid = read_guid(asset).or_else(|| super::register(asset));
    let text = match guid {
        Some(g) => with_guid(&text, g).unwrap_or(text),
        None => text,
    };
    super::write_atomic(&meta_path(asset), text).map_err(|e| e.to_string())
}
