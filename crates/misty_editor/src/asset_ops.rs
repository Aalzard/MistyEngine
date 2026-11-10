//! Project file operations: unique names, rename, move, duplicate, delete to the OS trash, and
//! rewriting path references after a rename or move.
//!
//! Every asset has a `.meta` sidecar holding its GUID; it moves, copies (with a new GUID) and is
//! deleted with the asset. Files saved with a GUID table (see `misty_engine::asset_db::refs`)
//! follow a rename on their next load, so a rename or move only rewrites the referencing files
//! whose references to the old path are not all covered by GUIDs (older path-only files; this
//! includes `ProjectSettings/BuildSettings.json`). Text replacement keeps each file's formatting untouched.

use std::path::{Path, PathBuf};

use misty_engine::asset_db::{Guid, meta, refs};
use serde_json::Value;

/// Files whose text can hold asset paths.
const REFERENCING: [&str; 12] = [
    "scene",
    "prefab",
    "mat",
    "controller",
    "anim",
    "mixer",
    "terrain",
    "biome",
    "timeline",
    "tile",
    "palette",
    "tree",
];
const BUILD_SETTINGS: &str = "ProjectSettings/BuildSettings.json";

/// "Assets/…" path of `abs` inside `root`, with forward slashes.
pub fn rel(root: &Path, abs: &Path) -> String {
    abs.strip_prefix(root)
        .unwrap_or(abs)
        .to_string_lossy()
        .replace('\\', "/")
}

/// `dir/stem.ext`, or `dir/stem 1.ext`, `dir/stem 2.ext`… whichever is free.
/// `ext` is "" for folders.
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let file = |n: usize| {
        let name = if n == 0 {
            stem.to_owned()
        } else {
            format!("{stem} {n}")
        };
        if ext.is_empty() {
            dir.join(name)
        } else {
            dir.join(format!("{name}.{ext}"))
        }
    };
    (0..).map(file).find(|p| !p.exists()).expect("a free name")
}

/// Check a new file name (without extension) typed by the user.
pub fn valid_name(name: &str) -> Result<(), String> {
    let bad = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    if name.trim().is_empty() {
        return Err("The name cannot be empty.".into());
    }
    if name.chars().any(|c| bad.contains(&c) || c.is_control()) || name.starts_with('.') {
        return Err(format!("'{name}' is not a valid file name."));
    }
    Ok(())
}

/// Rename `from` to `new_stem` (keeping its extension). Returns the new path.
pub fn rename(from: &Path, new_stem: &str) -> Result<PathBuf, String> {
    valid_name(new_stem)?;
    // Appended, not `set_extension`: a name with a dot ("Red.v2") would lose its last part.
    let to = match from.extension().filter(|_| from.is_file()) {
        Some(ext) => from.with_file_name(format!("{new_stem}.{}", ext.to_string_lossy())),
        None => from.with_file_name(new_stem),
    };
    if to == from {
        return Ok(to);
    }
    // Case-only renames on case-insensitive file systems: `exists` is true for the same file.
    let case_only = to.to_string_lossy().to_lowercase() == from.to_string_lossy().to_lowercase();
    if to.exists() && !case_only {
        return Err(format!("'{}' already exists.", file_name(&to)));
    }
    move_with_meta(from, &to)?;
    Ok(to)
}

/// Rename the file and its meta; if the meta can't follow, the file goes back (otherwise the
/// asset would be moved on disk while the error left the database and references on the old
/// path, and its GUID behind in a stray meta).
fn move_with_meta(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::rename(from, to).map_err(|e| e.to_string())?;
    if let Err(e) = move_meta(from, to) {
        return Err(match std::fs::rename(to, from) {
            Ok(()) => e,
            Err(back) => format!("{e} (and it could not be moved back: {back})"),
        });
    }
    Ok(())
}

/// Move `from` into `folder` (same name). Returns the new path.
pub fn move_into(from: &Path, folder: &Path) -> Result<PathBuf, String> {
    if folder.starts_with(from) {
        return Err("A folder cannot be moved into itself.".into());
    }
    let to = folder.join(from.file_name().ok_or("no file name")?);
    if to == from {
        return Ok(to);
    }
    if to.exists() {
        return Err(format!("'{}' already exists there.", file_name(&to)));
    }
    move_with_meta(from, &to)?;
    Ok(to)
}

/// Copy a file or folder next to itself as "Name 1", "Name 2"… Returns the copy's path.
pub fn duplicate(from: &Path) -> Result<PathBuf, String> {
    let dir = from.parent().ok_or("no parent folder")?;
    let stem = from
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (stem, ext) = if from.is_dir() {
        (file_name(from), String::new())
    } else {
        let ext = from
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        (stem, ext)
    };
    let to = unique_path(dir, strip_number(&stem), &ext);
    copy_all(from, &to).map_err(|e| e.to_string())?;
    copy_meta(from, &to)?;
    Ok(to)
}

/// Move files or folders to the OS recycle bin / trash (never a permanent delete), with their
/// `.meta` files.
#[cfg(not(test))]
pub fn delete_to_trash(paths: &[PathBuf]) -> Result<(), String> {
    trash::delete_all(with_metas(paths)).map_err(|e| e.to_string())
}

/// Tests move "deleted" files into a temp folder instead of filling the user's Recycle Bin.
#[cfg(test)]
pub fn delete_to_trash(paths: &[PathBuf]) -> Result<(), String> {
    let bin = std::env::temp_dir().join("misty_test_trash");
    std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    for p in with_metas(paths) {
        let name = p.file_name().ok_or("no file name")?.to_string_lossy();
        let to = unique_path(&bin, &name, "");
        std::fs::rename(&p, to).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// `paths` plus the meta files next to them.
fn with_metas(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = paths.to_vec();
    out.extend(
        paths
            .iter()
            .map(|p| meta::meta_path(p))
            .filter(|m| m.is_file()),
    );
    out
}

/// Move the meta file of `from` along with it.
fn move_meta(from: &Path, to: &Path) -> Result<(), String> {
    let m = meta::meta_path(from);
    if !m.is_file() {
        return Ok(());
    }
    std::fs::rename(&m, meta::meta_path(to)).map_err(|e| format!("{}: {e}", m.display()))
}

/// A copy keeps the import settings of `from` but gets new GUIDs (for a folder, everything in it).
fn copy_meta(from: &Path, to: &Path) -> Result<(), String> {
    let m = meta::meta_path(from);
    if m.is_file() {
        std::fs::copy(&m, meta::meta_path(to)).map_err(|e| format!("{}: {e}", m.display()))?;
    }
    let mut metas = vec![meta::meta_path(to)];
    if to.is_dir() {
        collect_metas(to, &mut metas);
    }
    for m in metas.into_iter().filter(|m| m.is_file()) {
        meta::write_guid(&m.with_extension(""), Guid::new())?;
    }
    Ok(())
}

fn collect_metas(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_metas(&path, out);
        } else if meta::is_meta(&path) {
            out.push(path);
        }
    }
}

/// `path` after renaming/moving `old` to `new` (a file, or a folder and everything in it).
pub fn remap(path: &str, old: &str, new: &str) -> Option<String> {
    if path == old {
        return Some(new.to_owned());
    }
    path.strip_prefix(old)
        .filter(|rest| rest.starts_with('/'))
        .map(|rest| format!("{new}{rest}"))
}

/// Rewrite every string in `v` that refers to `old` (see [`remap`]). Returns true if any changed.
pub fn rewrite_value(v: &mut Value, old: &str, new: &str) -> bool {
    match v {
        Value::String(s) => match remap(s, old, new) {
            Some(n) => {
                *s = n;
                true
            }
            None => false,
        },
        Value::Array(a) => a
            .iter_mut()
            .fold(false, |ch, x| rewrite_value(x, old, new) | ch),
        Value::Object(o) => o
            .values_mut()
            .fold(false, |ch, x| rewrite_value(x, old, new) | ch),
        _ => false,
    }
}

/// Every reference to `old` (or inside it) in the JSON `text` has a GUID in the file's table,
/// so loading the file follows the move without rewriting it.
pub fn covered_by_guids(text: &str, old: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    let table = refs::table(&v);
    let mut strings = Vec::new();
    refs::strings(&v, &mut strings);
    !table.is_empty()
        && strings
            .into_iter()
            .filter_map(refs::asset_path)
            .filter(|p| remap(p, old, "").is_some())
            .all(|p| table.contains_key(p))
}

/// Replace quoted occurrences of `old` (and `old/…`, `old#sub-asset`) in JSON text.
pub fn rewrite_text(text: &str, old: &str, new: &str) -> Option<String> {
    let exact = (format!("\"{old}\""), format!("\"{new}\""));
    let prefix = (format!("\"{old}/"), format!("\"{new}/"));
    let sub = (format!("\"{old}#"), format!("\"{new}#"));
    if ![&exact, &prefix, &sub]
        .iter()
        .any(|(o, _)| text.contains(o.as_str()))
    {
        return None;
    }
    Some(
        text.replace(&exact.0, &exact.1)
            .replace(&prefix.0, &prefix.1)
            .replace(&sub.0, &sub.1),
    )
}

/// Rewrite references to `old` in every referencing file of the project at `root`, skipping
/// files whose references are all covered by GUIDs. Returns the project-relative paths of the
/// files that changed.
pub fn rewrite_references(root: &Path, old: &str, new: &str) -> Result<Vec<String>, String> {
    let mut files = vec![root.join(BUILD_SETTINGS)];
    collect(&root.join("Assets"), &mut files);
    let mut changed = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if covered_by_guids(&text, old) {
            continue;
        }
        if let Some(updated) = rewrite_text(&text, old, new) {
            std::fs::write(&path, updated).map_err(|e| format!("{}: {e}", path.display()))?;
            changed.push(rel(root, &path));
        }
    }
    Ok(changed)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| REFERENCING.iter().any(|r| e == *r))
        {
            out.push(path);
        }
    }
}

pub fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// "Wall 2" → "Wall", so duplicates count up instead of growing "Wall 2 1".
fn strip_number(stem: &str) -> &str {
    match stem.rsplit_once(' ') {
        Some((base, n)) if !base.is_empty() && n.parse::<u32>().is_ok() => base,
        _ => stem,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("misty_asset_ops_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Assets")).unwrap();
        dir
    }

    #[test]
    fn remap_handles_files_and_folder_prefixes() {
        let (old, new) = ("Assets/Mats", "Assets/Materials");
        assert_eq!(
            remap("Assets/Mats", old, new).as_deref(),
            Some("Assets/Materials")
        );
        assert_eq!(
            remap("Assets/Mats/Red.mat", old, new).as_deref(),
            Some("Assets/Materials/Red.mat")
        );
        assert_eq!(remap("Assets/Mats2/Red.mat", old, new), None);
        let text = r#"{"material": "Assets/Mats/Red.mat", "other": "Assets/Mats2/x.mat"}"#;
        assert_eq!(
            rewrite_text(text, old, new).unwrap(),
            r#"{"material": "Assets/Materials/Red.mat", "other": "Assets/Mats2/x.mat"}"#
        );
        let mut v = serde_json::json!({"a": ["Assets/Mats/Red.mat", 3], "b": "Assets/Mats"});
        assert!(rewrite_value(&mut v, old, new));
        assert_eq!(v["a"][0], "Assets/Materials/Red.mat");
        assert_eq!(v["b"], "Assets/Materials");
    }

    /// A new name with a dot keeps all of it and the extension ("Red.v2" → "Red.v2.mat"; it
    /// used to become "Red.mat", a silent no-op).
    #[test]
    fn names_with_dots_keep_the_extension() {
        let root = temp("dots");
        let assets = root.join("Assets");
        let mat = assets.join("Red.mat");
        std::fs::write(&mat, "{}").unwrap();
        let v2 = rename(&mat, "Red.v2").unwrap();
        assert_eq!(v2, assets.join("Red.v2.mat"));
        assert!(v2.is_file() && !mat.exists());
        assert_eq!(
            rename(&v2, "Red.v2.x").unwrap(),
            assets.join("Red.v2.x.mat")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// The meta can't follow (its new name is taken by a folder): the rename is rolled back,
    /// so the asset stays where the database and the references say it is.
    #[test]
    fn a_failed_meta_move_rolls_the_rename_back() {
        let root = temp("rollback");
        let assets = root.join("Assets");
        let mat = assets.join("Red.mat");
        std::fs::write(&mat, "{}").unwrap();
        std::fs::write(meta::meta_path(&mat), "{\"guid\": \"x\"}").unwrap();
        std::fs::create_dir_all(assets.join("Crimson.mat.meta/inside")).unwrap();
        assert!(rename(&mat, "Crimson").is_err());
        assert!(mat.is_file(), "moved back");
        assert!(meta::meta_path(&mat).is_file());
        assert!(!assets.join("Crimson.mat").exists());
        let folder = assets.join("Sub");
        std::fs::create_dir_all(folder.join("Red.mat.meta/inside")).unwrap();
        assert!(move_into(&mat, &folder).is_err());
        assert!(mat.is_file(), "moved back");
        assert!(!folder.join("Red.mat").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unique_names_rename_duplicate_and_move() {
        let root = temp("ops");
        let assets = root.join("Assets");
        let mat = assets.join("Red.mat");
        std::fs::write(&mat, "{}").unwrap();
        assert_eq!(unique_path(&assets, "Red", "mat"), assets.join("Red 1.mat"));
        assert_eq!(
            unique_path(&assets, "New Folder", ""),
            assets.join("New Folder")
        );

        let copy = duplicate(&mat).unwrap();
        assert_eq!(copy, assets.join("Red 1.mat"));
        assert_eq!(duplicate(&copy).unwrap(), assets.join("Red 2.mat"));

        let renamed = rename(&copy, "Crimson").unwrap();
        assert_eq!(renamed, assets.join("Crimson.mat"));
        assert!(rename(&renamed, "Red").is_err(), "name taken");
        assert!(rename(&renamed, "a/b").is_err(), "invalid name");

        let folder = assets.join("Materials");
        std::fs::create_dir(&folder).unwrap();
        let moved = move_into(&renamed, &folder).unwrap();
        assert_eq!(moved, folder.join("Crimson.mat"));
        assert!(move_into(&folder, &folder).is_err(), "not into itself");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn references_are_rewritten_in_scenes_prefabs_materials_and_build_settings() {
        let root = temp("refs");
        let a = root.join("Assets");
        std::fs::create_dir_all(root.join("ProjectSettings")).unwrap();
        std::fs::write(
            a.join("Level.scene"),
            "{\n  \"material\": \"Assets/Red.mat\",\n  \"keep\": \"Assets/Red.mat.bak\"\n}",
        )
        .unwrap();
        std::fs::write(a.join("Crate.prefab"), r#"{"m":"Assets/Red.mat"}"#).unwrap();
        std::fs::write(a.join("Notes.txt"), r#""Assets/Red.mat""#).unwrap();
        std::fs::write(
            root.join(BUILD_SETTINGS),
            r#"{"scenes":[{"path":"Assets/Level.scene"}]}"#,
        )
        .unwrap();

        let mut changed =
            rewrite_references(&root, "Assets/Red.mat", "Assets/Crimson.mat").unwrap();
        changed.sort();
        assert_eq!(changed, ["Assets/Crate.prefab", "Assets/Level.scene"]);
        let scene = std::fs::read_to_string(a.join("Level.scene")).unwrap();
        assert!(scene.contains("\"Assets/Crimson.mat\"") && scene.contains("Red.mat.bak"));
        assert!(
            std::fs::read_to_string(a.join("Notes.txt"))
                .unwrap()
                .contains("Red.mat")
        );

        let changed = rewrite_references(&root, "Assets/Level.scene", "Assets/Main.scene").unwrap();
        assert_eq!(changed, [BUILD_SETTINGS]);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn metas_move_copy_and_delete_with_their_assets() {
        let root = temp("metas");
        let a = root.join("Assets");
        std::fs::write(a.join("Red.mat"), "{}").unwrap();
        let (db, _) = misty_engine::asset_db::GuidDatabase::scan(&root);
        let red = db.guid_of("Assets/Red.mat").unwrap();

        let renamed = rename(&a.join("Red.mat"), "Crimson").unwrap();
        assert_eq!(meta::read_guid(&renamed), Some(red), "the meta moves along");
        assert!(!a.join("Red.mat.meta").exists());
        let folder = a.join("Mats");
        std::fs::create_dir(&folder).unwrap();
        let moved = move_into(&renamed, &folder).unwrap();
        assert_eq!(meta::read_guid(&moved), Some(red));

        let copy = duplicate(&moved).unwrap();
        let copied = meta::read_guid(&copy).unwrap();
        assert_ne!(copied, red, "a copy gets its own GUID");
        let folder_copy = duplicate(&folder).unwrap();
        let inner = meta::read_guid(&folder_copy.join("Crimson.mat")).unwrap();
        assert!(inner != red && inner != copied);

        delete_to_trash(std::slice::from_ref(&copy)).unwrap();
        assert!(!copy.exists() && !meta::meta_path(&copy).exists());
        std::fs::remove_dir_all(root).ok();
    }

    /// Files saved with GUIDs are left alone by a rename; path-only files are rewritten.
    #[test]
    fn renames_skip_files_whose_references_have_guids() {
        let root = temp("covered");
        let a = root.join("Assets");
        let g = Guid::new();
        let with = format!(
            "{{
  \"m\": \"Assets/Red.mat\",
  \"asset_guids\": {{ \"Assets/Red.mat\": \"{g}\" }}
}}"
        );
        std::fs::write(a.join("New.scene"), &with).unwrap();
        std::fs::write(
            a.join("Old.scene"),
            r#"{"m":"Assets/Red.mat","s":"Assets/Red.mat#0"}"#,
        )
        .unwrap();
        assert!(covered_by_guids(&with, "Assets/Red.mat"));
        assert!(!covered_by_guids(
            &with.replace("asset_guids", "x"),
            "Assets/Red.mat"
        ));
        let changed = rewrite_references(&root, "Assets/Red.mat", "Assets/Crimson.mat").unwrap();
        assert_eq!(changed, ["Assets/Old.scene"]);
        assert_eq!(std::fs::read_to_string(a.join("New.scene")).unwrap(), with);
        assert_eq!(
            std::fs::read_to_string(a.join("Old.scene")).unwrap(),
            r#"{"m":"Assets/Crimson.mat","s":"Assets/Crimson.mat#0"}"#,
            "sub-asset references are rewritten too"
        );
        std::fs::remove_dir_all(root).ok();
    }
}
