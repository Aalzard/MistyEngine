//! Save games: `SaveSystem.Save(slot)` / `Load(slot)` from scripts.
//!
//! A save holds the state of every object with a [`Saveable`] component (by Save ID), the
//! prefab instances spawned while playing (recreated from their prefab, by GUID when the asset
//! database knows it), the Save IDs of scene objects that were destroyed, and the custom tables
//! scripts return from `OnSave(data)`. Loading reloads the saved scene from its file, then
//! applies all of that (see [`apply`]); `OnLoad(data)` runs after `Awake`, before `Start`.
//!
//! Files live in a per-game folder: built games use the per-user folder next to PlayerPrefs
//! (`%APPDATA%\<Company>\<Product>\saves` on Windows), the editor `<Project>/Library/Saves`.
//! Each slot is `<slot>.save` (JSON, or gzip-compressed JSON when `SaveSystem.compress` is set),
//! `<slot>.info.json` (metadata for slot lists) and an optional `<slot>.png` thumbnail of the
//! last rendered frame.

mod saveable;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use glam::Vec3;
use hecs::Entity;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use saveable::{Saveable, by_id, fix_duplicate_ids, fresh_ids_in, new_id};

use crate::scene::{Scene, Scripts, Transform, registry};

/// Version of the file layout written by this engine (the game's own data version is separate).
pub const FORMAT: u32 = 1;
/// Folder of the editor's saves inside a project.
pub const EDITOR_FOLDER: &str = "Library/Saves";
const DATA_EXT: &str = "save";
const INFO_EXT: &str = "info.json";
const THUMB_EXT: &str = "png";
/// Thumbnails are scaled to this width (height keeps the aspect).
pub const THUMBNAIL_WIDTH: u32 = 320;

/// Slot metadata, written next to the save so slot lists don't parse whole saves.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlotInfo {
    pub slot: String,
    /// Seconds since 1970-01-01 UTC.
    pub time: u64,
    /// Name of the active scene.
    pub scene: String,
    /// Project-relative scene file, reloaded by `Load`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_path: Option<String>,
    /// Seconds played (carried over from the save that was loaded, if any).
    pub play_time: f64,
    /// The game's data version (`SaveSystem.version`).
    pub version: u32,
    /// A thumbnail PNG was asked for.
    pub thumbnail: bool,
    /// Bytes of the save file (filled in by [`list_slots`]).
    #[serde(skip)]
    pub size: u64,
}

/// One object's saved state.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ObjectState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    /// Local transform.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<Transform>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub velocity: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angular_velocity: Option<[f32; 3]>,
    /// `"Component"` or `"Component.field"` → value.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<String, Value>,
    /// Script key (its path, `#n` for repeats on one object) → table from `OnSave(data)`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub scripts: BTreeMap<String, Value>,
}

/// A prefab instance created while playing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpawnedState {
    /// Project-relative prefab path when saved.
    pub prefab: String,
    /// The prefab's GUID (follows renames), when the asset database knew it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefab_guid: Option<String>,
    pub name: String,
    /// Save ID of the parent object (`None` = scene root).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub active: bool,
    pub transform: Transform,
    /// Save IDs of the Saveables in the instance, in Hierarchy order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<String>,
}

/// A whole save game.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SaveData {
    pub format: u32,
    /// The game's data version (`SaveSystem.version`), checked by the migration hook.
    pub version: u32,
    pub info: SlotInfo,
    pub objects: BTreeMap<String, ObjectState>,
    /// Save IDs of scene objects that were destroyed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub destroyed: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spawned: Vec<SpawnedState>,
}

// ---- per-game runtime state -----------------------------------------------------------------

/// What a running game remembers for its saves (kept with the script world).
#[derive(Debug, Default)]
pub struct SaveRuntime {
    /// Save IDs of the scene's objects when it started (missing later = destroyed).
    pub baseline: HashSet<String>,
    /// Prefab instances made while playing: root and prefab path.
    pub spawned: Vec<(Entity, String)>,
    /// Play time carried over from a loaded save.
    pub play_time_base: f64,
    started_at: Option<f64>,
    now: f64,
    /// `Load` was called: the save to restore after this frame.
    pub pending_load: Option<SaveData>,
    /// Folder override for this game (tests); else [`folder_for`] of the project.
    pub folder: Option<PathBuf>,
}

impl SaveRuntime {
    /// Remember the scene's Save IDs as it starts.
    pub fn begin(scene: &Scene) -> Self {
        Self {
            baseline: by_id(scene).into_keys().collect(),
            ..Default::default()
        }
    }

    /// Game time of the current frame (the first call marks the start of this scene).
    pub fn mark_time(&mut self, t: f64) {
        self.now = t;
        self.started_at.get_or_insert(t);
    }

    /// Seconds played, including the loaded save's play time.
    pub fn play_time(&self) -> f64 {
        self.play_time_base + (self.now - self.started_at.unwrap_or(self.now)).max(0.0)
    }

    /// Restart the play-time clock from a loaded save's value.
    pub fn resume_play_time(&mut self, base: f64) {
        self.play_time_base = base;
        self.started_at = None;
    }

    pub fn folder(&self, root: &Path) -> PathBuf {
        self.folder.clone().unwrap_or_else(|| folder_for(root))
    }
}

// ---- folders ---------------------------------------------------------------------------------

static FOLDER_OVERRIDE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Built games: their per-user saves folder (set once at startup).
pub fn set_folder_override(folder: Option<PathBuf>) {
    *FOLDER_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner()) = folder;
}

/// Where a game whose project is at `root` keeps its saves.
pub fn folder_for(root: &Path) -> PathBuf {
    FOLDER_OVERRIDE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| root.join(EDITOR_FOLDER))
}

/// The per-user saves folder of a built game (next to its PlayerPrefs).
pub fn player_folder(company: &str, product: &str) -> Option<PathBuf> {
    crate::prefs::default_path(company, product).and_then(|p| p.parent().map(|d| d.join("saves")))
}

/// File-name-safe form of a slot name.
pub fn slot_file_name(slot: &str) -> String {
    let s: String = slot
        .trim()
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim_end_matches('.').to_owned();
    if s.is_empty() { "slot".into() } else { s }
}

fn slot_path(folder: &Path, slot: &str, ext: &str) -> PathBuf {
    folder.join(format!("{}.{ext}", slot_file_name(slot)))
}

/// Path of a slot's thumbnail (whether or not it exists).
pub fn thumbnail_path(folder: &Path, slot: &str) -> PathBuf {
    slot_path(folder, slot, THUMB_EXT)
}

/// Path of a slot's data file.
pub fn data_path(folder: &Path, slot: &str) -> PathBuf {
    slot_path(folder, slot, DATA_EXT)
}

// ---- reading and writing slots ---------------------------------------------------------------

/// Write a save (and its metadata). Returns the data file's path.
pub fn write_slot(
    folder: &Path,
    slot: &str,
    data: &SaveData,
    compress: bool,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let text = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    let bytes = if compress {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        enc.finish().map_err(|e| e.to_string())?
    } else {
        (text + "\n").into_bytes()
    };
    let path = data_path(folder, slot);
    // Write then rename, so a crash mid-save keeps the previous save.
    let tmp = path.with_extension("save.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    let info = serde_json::to_string_pretty(&data.info).map_err(|e| e.to_string())?;
    let info_path = slot_path(folder, slot, INFO_EXT);
    std::fs::write(&info_path, info + "\n").map_err(|e| format!("{}: {e}", info_path.display()))?;
    if !data.info.thumbnail {
        let _ = std::fs::remove_file(thumbnail_path(folder, slot));
    }
    Ok(path)
}

/// Text of a save file, uncompressing it when needed.
fn decode(bytes: &[u8]) -> Result<String, String> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut s = String::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_string(&mut s)
            .map_err(|e| format!("corrupt compressed save: {e}"))?;
        Ok(s)
    } else {
        String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())
    }
}

/// A slot's save as JSON (before migration).
pub fn read_slot_value(folder: &Path, slot: &str) -> Result<Value, String> {
    let path = data_path(folder, slot);
    let bytes = std::fs::read(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!("no save in slot '{slot}'"),
        _ => format!("{}: {e}", path.display()),
    })?;
    let text = decode(&bytes)?;
    serde_json::from_str(&text).map_err(|e| format!("slot '{slot}': invalid save: {e}"))
}

/// Parse save JSON (after migration).
pub fn from_value(v: Value) -> Result<SaveData, String> {
    let data: SaveData = serde_json::from_value(v).map_err(|e| format!("invalid save: {e}"))?;
    if data.format > FORMAT {
        return Err(format!(
            "the save was written by a newer engine (format {}, this one reads {FORMAT})",
            data.format
        ));
    }
    Ok(data)
}

/// A slot's save, with the registered migrations run up to `current_version`.
pub fn read_slot(folder: &Path, slot: &str, current_version: u32) -> Result<SaveData, String> {
    let mut v = read_slot_value(folder, slot)?;
    migrate(&mut v, current_version);
    from_value(v)
}

pub fn exists(folder: &Path, slot: &str) -> bool {
    data_path(folder, slot).is_file()
}

/// Remove a slot's files. Returns false when there was nothing to delete.
pub fn delete_slot(folder: &Path, slot: &str) -> bool {
    let mut any = false;
    for ext in [DATA_EXT, INFO_EXT, THUMB_EXT] {
        any |= std::fs::remove_file(slot_path(folder, slot, ext)).is_ok();
    }
    any
}

/// Every slot in `folder`, newest first.
pub fn list_slots(folder: &Path) -> Vec<SlotInfo> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(&format!(".{DATA_EXT}")) else {
            continue;
        };
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let info = std::fs::read_to_string(slot_path(folder, stem, INFO_EXT))
            .ok()
            .and_then(|t| serde_json::from_str::<SlotInfo>(&t).ok())
            .or_else(|| {
                // No metadata file: read it from the save itself.
                let v = read_slot_value(folder, stem).ok()?;
                serde_json::from_value::<SlotInfo>(v.get("info")?.clone()).ok()
            });
        let mut info = info.unwrap_or_else(|| SlotInfo {
            slot: stem.to_owned(),
            ..Default::default()
        });
        if info.slot.is_empty() {
            info.slot = stem.to_owned();
        }
        info.thumbnail = thumbnail_path(folder, stem).is_file();
        info.size = size;
        out.push(info);
    }
    out.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| a.slot.cmp(&b.slot)));
    out
}

// ---- versions and migration ------------------------------------------------------------------

/// A migration step: change `save` (JSON) written with data version `from`.
pub type Migration = fn(save: &mut Value, from: u32);

static MIGRATIONS: Mutex<Vec<Migration>> = Mutex::new(Vec::new());

/// Run `hook` on every save older than the version being loaded (Rust games and tools).
pub fn register_migration(hook: Migration) {
    MIGRATIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(hook);
}

/// Run the registered migrations on an older save and stamp it with `current`.
pub fn migrate(save: &mut Value, current: u32) -> bool {
    let hooks = MIGRATIONS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    migrate_with(save, current, &hooks)
}

/// Data version a save JSON was written with (0 = none).
pub fn version_of(save: &Value) -> u32 {
    save.get("version")
        .and_then(Value::as_u64)
        .map_or(0, |v| v as u32)
}

/// Run `hooks` when `save` is older than `current`; returns whether it was migrated.
pub fn migrate_with(save: &mut Value, current: u32, hooks: &[Migration]) -> bool {
    let from = version_of(save);
    if from >= current {
        return false;
    }
    for hook in hooks {
        hook(save, from);
    }
    set_version(save, current);
    true
}

/// Stamp a save JSON with a data version (also in its metadata).
pub fn set_version(save: &mut Value, version: u32) {
    if let Some(o) = save.as_object_mut() {
        o.insert("version".into(), version.into());
        if let Some(info) = o.get_mut("info").and_then(Value::as_object_mut) {
            info.insert("version".into(), version.into());
        }
    }
}

// ---- thumbnails ------------------------------------------------------------------------------

static THUMBNAIL_REQUEST: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Ask whoever renders the game to save the next frame as `path`.
pub fn request_thumbnail(path: PathBuf) {
    *THUMBNAIL_REQUEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
}

/// The thumbnail asked for since the last call (the player and editor call this after drawing).
pub fn take_thumbnail_request() -> Option<PathBuf> {
    THUMBNAIL_REQUEST
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
}

/// Scale an RGBA frame to [`THUMBNAIL_WIDTH`] and write it as a PNG.
pub fn write_thumbnail(path: &Path, width: u32, height: u32, rgba: Vec<u8>) -> Result<(), String> {
    let img = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "thumbnail: wrong pixel count".to_owned())?;
    let w = THUMBNAIL_WIDTH.min(width.max(1));
    let h = ((height as f32 * w as f32 / width.max(1) as f32).round() as u32).max(1);
    let small = image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle);
    let rgb = image::DynamicImage::ImageRgba8(small).to_rgb8();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    rgb.save(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

// ---- capture ---------------------------------------------------------------------------------

/// Seconds since 1970 now.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// "2026-11-05 14:03 UTC" for a time from [`unix_now`].
pub fn format_time(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil date from days since 1970 (proleptic Gregorian).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        rem / 3600,
        (rem / 60) % 60
    )
}

/// "1:02:03" / "4:05" for a play time in seconds.
pub fn format_duration(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Keys of the scripts on an object: their path, `path#2`, `path#3`… for repeats.
pub fn script_keys(scripts: &Scripts) -> Vec<String> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    scripts
        .list
        .iter()
        .map(|s| {
            let n = seen.entry(s.path.as_str()).or_insert(0);
            *n += 1;
            if *n == 1 {
                s.path.clone()
            } else {
                format!("{}#{n}", s.path)
            }
        })
        .collect()
}

/// Everything [`capture`] reads besides the scene.
pub struct CaptureInput<'a> {
    pub slot: &'a str,
    pub runtime: &'a SaveRuntime,
    /// Rigidbody (linear, angular) velocity of an object.
    pub velocity: &'a dyn Fn(Entity) -> Option<(Vec3, Vec3)>,
    pub version: u32,
}

/// The current state of the scene's Saveables, spawned prefabs and destroyed objects (script
/// tables are added by the caller from `OnSave`).
pub fn capture(scene: &Scene, input: &CaptureInput) -> SaveData {
    let mut objects = BTreeMap::new();
    let ids = by_id(scene);
    for (id, &e) in &ids {
        let Ok(s) = scene.world.get::<&Saveable>(e).map(|s| (*s).clone()) else {
            continue;
        };
        let mut state = ObjectState::default();
        if s.save_active {
            state.active = Some(scene.game_object(e).active);
        }
        if s.save_transform {
            state.transform = Some(scene.transform(e));
        }
        if s.save_velocity
            && let Some((v, w)) = (input.velocity)(e)
        {
            state.velocity = Some(v.to_array());
            state.angular_velocity = Some(w.to_array());
        }
        for entry in &s.components {
            if let Some(v) = read_component_field(scene, e, entry) {
                state.components.insert(entry.clone(), v);
            }
        }
        objects.insert(id.clone(), state);
    }
    let mut destroyed: Vec<String> = input
        .runtime
        .baseline
        .iter()
        .filter(|id| !ids.contains_key(*id))
        .cloned()
        .collect();
    destroyed.sort();
    let spawned = input
        .runtime
        .spawned
        .iter()
        .filter(|(e, _)| scene.contains(*e))
        .map(|(e, path)| spawned_state(scene, *e, path))
        .collect();
    let active = scene.active_scene();
    SaveData {
        format: FORMAT,
        version: input.version,
        info: SlotInfo {
            slot: input.slot.to_owned(),
            time: unix_now(),
            scene: scene.scene_name(active),
            scene_path: scene.scene_path(active),
            play_time: input.runtime.play_time(),
            version: input.version,
            thumbnail: false,
            size: 0,
        },
        objects,
        destroyed,
        spawned,
    }
}

fn spawned_state(scene: &Scene, root: Entity, path: &str) -> SpawnedState {
    let ids = scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| scene.is_ancestor(root, *e))
        .filter_map(|e| {
            scene
                .world
                .get::<&Saveable>(e)
                .ok()
                .map(|s| s.save_id.clone())
        })
        .collect();
    let parent = scene.parent(root).and_then(|p| {
        scene
            .world
            .get::<&Saveable>(p)
            .ok()
            .map(|s| s.save_id.clone())
    });
    SpawnedState {
        prefab: path.to_owned(),
        prefab_guid: crate::asset_db::guid_of(path).map(|g| g.to_string()),
        name: scene.name(root),
        parent,
        active: scene.game_object(root).active,
        transform: scene.transform(root),
        ids,
    }
}

/// `"Component"` (whole) or `"Component.field"` of `e`, as JSON.
pub fn read_component_field(scene: &Scene, e: Entity, entry: &str) -> Option<Value> {
    let (comp, field) = split_entry(entry);
    let info = registry::find(comp)?;
    let v = (info.to_json)(&scene.world, e)?;
    match field {
        None => Some(v),
        Some(f) => v.get(f).cloned(),
    }
}

/// Write back what [`read_component_field`] read (only onto a component the object has).
pub fn write_component_field(scene: &mut Scene, e: Entity, entry: &str, value: Value) -> bool {
    let (comp, field) = split_entry(entry);
    let Some(info) = registry::find(comp) else {
        return false;
    };
    if !(info.has)(&scene.world, e) {
        return false;
    }
    let v = match field {
        None => value,
        Some(f) => {
            let Some(mut current) = (info.to_json)(&scene.world, e) else {
                return false;
            };
            let Some(o) = current.as_object_mut() else {
                return false;
            };
            o.insert(f.to_owned(), value);
            current
        }
    };
    (info.from_json)(&mut scene.world, e, v).is_ok()
}

fn split_entry(entry: &str) -> (&str, Option<&str>) {
    match entry.split_once('.') {
        Some((c, f)) if !f.is_empty() => (c.trim(), Some(f.trim())),
        _ => (entry.trim(), None),
    }
}

// ---- apply -----------------------------------------------------------------------------------

/// What [`apply`] did that the caller must finish (physics, scripts).
#[derive(Debug, Default)]
pub struct Applied {
    /// Scene objects to destroy (with their children and bodies).
    pub destroy: Vec<Entity>,
    /// Recreated prefab instances: root and prefab path (they need bodies and `Awake`).
    pub spawned: Vec<(Entity, String)>,
    /// Objects whose transform was set (bodies must be moved).
    pub moved: Vec<Entity>,
    /// Rigidbody velocities to set: object, linear, angular.
    pub velocities: Vec<(Entity, Vec3, Vec3)>,
    /// Script tables for `OnLoad`: object, script key, table.
    pub scripts: Vec<(Entity, String, Value)>,
    pub warnings: Vec<String>,
}

/// Apply a save to a freshly loaded scene: recreate spawned prefabs (from `root`), set the
/// saved states, and list what's left for physics and scripts.
pub fn apply(scene: &mut Scene, data: &SaveData, root: &Path) -> Applied {
    let mut out = Applied::default();
    let ids = by_id(scene);
    let destroyed: HashSet<Entity> = data
        .destroyed
        .iter()
        .filter_map(|id| ids.get(id).copied())
        .collect();
    out.destroy = destroyed.iter().copied().collect();

    for s in &data.spawned {
        let path = s
            .prefab_guid
            .as_deref()
            .and_then(|g| g.parse().ok())
            .and_then(crate::asset_db::path_of)
            .unwrap_or_else(|| s.prefab.clone());
        let prefab = match crate::scene::prefab::Prefab::load(&root.join(&path)) {
            Ok(p) => p,
            Err(e) => {
                out.warnings.push(format!("spawned '{}': {e}", s.name));
                continue;
            }
        };
        let parent = s
            .parent
            .as_ref()
            .and_then(|id| by_id(scene).get(id).copied());
        let e = scene.instantiate_prefab(&prefab, None, parent);
        scene.game_object_mut(e).name = s.name.clone();
        scene.game_object_mut(e).active = s.active;
        scene.set_transform(e, s.transform);
        let subtree: Vec<Entity> = scene
            .walk()
            .into_iter()
            .map(|(x, _)| x)
            .filter(|x| scene.is_ancestor(e, *x))
            .filter(|x| scene.world.get::<&Saveable>(*x).is_ok())
            .collect();
        for (x, id) in subtree.into_iter().zip(&s.ids) {
            if let Ok(mut sv) = scene.world.get::<&mut Saveable>(x) {
                sv.save_id = id.clone();
            }
        }
        out.moved.push(e);
        out.spawned.push((e, path));
    }

    let ids = by_id(scene);
    for (id, state) in &data.objects {
        let Some(&e) = ids.get(id) else {
            continue;
        };
        if destroyed.contains(&e) {
            continue;
        }
        if let Some(a) = state.active {
            scene.game_object_mut(e).active = a;
        }
        if let Some(t) = state.transform {
            scene.set_transform(e, t);
            out.moved.push(e);
        }
        for (entry, value) in &state.components {
            if !write_component_field(scene, e, entry, value.clone()) {
                out.warnings
                    .push(format!("{}: couldn't restore {entry}", scene.name(e)));
            }
        }
        if let Some(v) = state.velocity {
            let w = state.angular_velocity.unwrap_or_default();
            out.velocities
                .push((e, Vec3::from_array(v), Vec3::from_array(w)));
        }
        for (key, table) in &state.scripts {
            out.scripts.push((e, key.clone(), table.clone()));
        }
    }
    out
}
