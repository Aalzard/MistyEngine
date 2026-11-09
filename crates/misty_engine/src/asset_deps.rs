//! Which assets a build needs: a walk from the build scenes through everything they reference.
//!
//! Scenes, prefabs, materials, animations, controllers, mixers, terrains, tree recipes and nav
//! meshes are JSON,
//! and every reference in them is a project path string ("Assets/…", optionally with a
//! `#sub-asset` suffix), so the walk reads every string of every JSON file it reaches. Lua
//! scripts are scanned for string literals naming assets, `.obj` files for their `.mtl` and
//! textures, `.gltf` files for their buffers and images. Every file under a `Resources` folder is
//! included as well (assets loaded by name at runtime), and so is each file's `.meta` sidecar.
//! Files under an `Editor` folder are left out (see [`is_editor_only`]).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use crate::asset_db::{GuidDatabase, refs};

/// The result of a walk.
#[derive(Debug, Default, PartialEq)]
pub struct Dependencies {
    /// Project-relative paths of the files to pack, sorted.
    pub files: BTreeSet<String>,
    /// Broken references: referenced path → the file that names it.
    pub missing: BTreeMap<String, String>,
}

/// Extensions whose files are JSON documents that can reference other assets.
const JSON_ASSETS: [&str; 17] = [
    "scene",
    "prefab",
    "mat",
    "anim",
    "controller",
    "mixer",
    "terrain",
    "navmesh",
    "json",
    "gltf",
    "physicmaterial",
    "timeline",
    "tile",
    "palette",
    "effect",
    "strings",
    // Tree recipes name their bark and leaf textures (terrain trees).
    "tree",
];

fn ext(rel: &str) -> String {
    Path::new(rel)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Every file `roots` (project-relative) need, plus `Resources` folders under `Assets/`.
pub fn collect(project: &Path, roots: &[String]) -> Dependencies {
    collect_with(project, roots, true, &GuidDatabase::read(project))
}

/// Every file `roots` need, without the `Resources` folders (asset bundles: one asset's
/// dependencies), resolving GUID references with `guids`.
pub fn collect_only(project: &Path, roots: &[String], guids: &GuidDatabase) -> Dependencies {
    collect_with(project, roots, false, guids)
}

fn collect_with(
    project: &Path,
    roots: &[String],
    resources: bool,
    guids: &GuidDatabase,
) -> Dependencies {
    let mut roots = roots.to_vec();
    let resource_files = if resources {
        crate::vfs::files_under(&project.join("Assets"))
    } else {
        Vec::new()
    };
    for p in resource_files {
        let Ok(rel) = p.strip_prefix(project) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        // String Tables are found by name at runtime, like Resources.
        let table = rel.ends_with(".strings");
        if (table || rel.split('/').any(|part| part == "Resources")) && !rel.ends_with(".meta") {
            roots.push(rel);
        }
    }
    collect_in(&Disk(project), &roots, guids)
}

/// Where a walk reads files from, by project-relative path ("Assets/…"): the project folder
/// on disk, or another copy of a project (an archive, a project template's source).
pub trait ProjectFiles {
    fn is_file(&self, rel: &str) -> bool;
    /// The file's text (`None` when it can't be read).
    fn read_to_string(&self, rel: &str) -> Option<String>;
}

/// The project folder (through [`crate::vfs`], so a mounted pak works too).
struct Disk<'a>(&'a Path);

impl ProjectFiles for Disk<'_> {
    fn is_file(&self, rel: &str) -> bool {
        crate::vfs::is_file(&self.0.join(rel))
    }

    fn read_to_string(&self, rel: &str) -> Option<String> {
        crate::vfs::read_to_string(self.0.join(rel)).ok()
    }
}

/// Every file `roots` need, read from `files` (no `Resources` folders), resolving GUID
/// references with `guids`.
pub fn collect_in(
    files: &dyn ProjectFiles,
    roots: &[String],
    guids: &GuidDatabase,
) -> Dependencies {
    let mut deps = Dependencies::default();
    // Stale paths in files saved with GUIDs follow the renamed or moved asset.
    let mut queue: VecDeque<(String, String)> =
        roots.iter().map(|r| (r.clone(), String::new())).collect();
    while let Some((rel, from)) = queue.pop_front() {
        let rel = normalize(&rel);
        // Editor scripts and other editor-only files never ship.
        if deps.files.contains(&rel) || is_editor_only(&rel) {
            continue;
        }
        if !files.is_file(&rel) {
            deps.missing.entry(rel).or_insert(from);
            continue;
        }
        deps.files.insert(rel.clone());
        let meta = format!("{rel}.meta");
        if files.is_file(&meta) {
            deps.files.insert(meta);
        }
        for r in references(files, &rel, guids) {
            if !deps.files.contains(&r) {
                queue.push_back((r, rel.clone()));
            }
        }
    }
    deps
}

/// Files under a folder named `Editor` (anywhere in the project) are for the editor only:
/// editor scripts run in the editor's own Lua state and are never packed into a build.
pub fn is_editor_only(rel: &str) -> bool {
    let mut parts: Vec<&str> = rel.split(['/', '\\']).collect();
    parts.pop();
    parts.contains(&"Editor")
}

/// `Assets/A/../B/c.png` → `Assets/B/c.png`, forward slashes.
fn normalize(rel: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for p in rel.split(['/', '\\']) {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// Project paths named inside the file `rel`.
fn references(files: &dyn ProjectFiles, rel: &str, guids: &GuidDatabase) -> Vec<String> {
    let dir = Path::new(rel).parent().map_or(String::new(), |d| {
        let d = d.to_string_lossy().replace('\\', "/");
        if d.is_empty() { d } else { d + "/" }
    });
    let ext = ext(rel);
    let read = || files.read_to_string(rel).unwrap_or_default();
    let mut out = Vec::new();
    if JSON_ASSETS.contains(&ext.as_str()) {
        let text = read();
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&refs::resolve(guids, &text))
        else {
            return out;
        };
        let mut strings = Vec::new();
        json_strings(&json, None, &mut strings);
        // A World Partition names its folder, not its files: the cell sub-scenes, HLOD proxies
        // and their material come from its cell list.
        let (cells, folders) = world_partition_files(&json);
        out.extend(cells);
        strings.retain(|(_, s)| !folders.contains(s));
        for (key, s) in strings {
            if let Some(p) = asset_path(&s) {
                out.push(p);
            } else if ext == "gltf" && key.as_deref() == Some("uri") && !s.starts_with("data:") {
                out.push(format!("{dir}{}", s.replace("%20", " ")));
            }
        }
    } else if ext == "lua" {
        out.extend(lua_strings(&read()).iter().filter_map(|s| asset_path(s)));
    } else if ext == "obj" || ext == "mtl" {
        for line in read().lines() {
            let mut words = line.split_whitespace();
            let Some(first) = words.next() else { continue };
            let wanted = if ext == "obj" {
                first == "mtllib"
            } else {
                first.starts_with("map_") || matches!(first, "bump" | "norm" | "disp")
            };
            // The file name is the last word (options such as `-bm 1` come first).
            if wanted && let Some(name) = line.split_whitespace().last() {
                out.push(format!("{dir}{}", name.replace('\\', "/")));
            }
        }
    }
    out
}

/// "Assets/…" (with any `#sub-asset` removed) when `s` names a project asset.
fn asset_path(s: &str) -> Option<String> {
    refs::asset_path(s).map(str::to_owned)
}

/// The files a scene's World Partition components stand for, and their folders.
fn world_partition_files(json: &serde_json::Value) -> (Vec<String>, Vec<String>) {
    let mut files = Vec::new();
    let mut folders = Vec::new();
    let Some(objects) = json.get("objects").and_then(|o| o.as_array()) else {
        return (files, folders);
    };
    for o in objects {
        let Some(wp) = o.get("components").and_then(|c| c.get("World Partition")) else {
            continue;
        };
        let Ok(wp) = serde_json::from_value::<crate::world_partition::WorldPartition>(wp.clone())
        else {
            continue;
        };
        for c in &wp.cells {
            files.push(wp.cell_path(c.coord()));
            if c.hlod {
                files.push(wp.hlod_path(c.coord()));
            }
        }
        if wp.cells.iter().any(|c| c.hlod) {
            files.push(crate::world_partition::hlod::material_path(&wp));
        }
        folders.push(wp.folder.clone());
    }
    (files, folders)
}

fn json_strings(v: &serde_json::Value, key: Option<&str>, out: &mut Vec<(Option<String>, String)>) {
    match v {
        serde_json::Value::String(s) => out.push((key.map(str::to_owned), s.clone())),
        serde_json::Value::Array(a) => a.iter().for_each(|x| json_strings(x, key, out)),
        serde_json::Value::Object(o) => o.iter().for_each(|(k, x)| json_strings(x, Some(k), out)),
        _ => {}
    }
}

/// Contents of the quoted string literals of a Lua source (no escapes processed).
fn lua_strings(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '-' && chars.peek() == Some(&'-') {
            // Comment to end of line.
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
        } else if c == '"' || c == '\'' {
            let mut s = String::new();
            while let Some(d) = chars.next() {
                match d {
                    '\\' => {
                        chars.next();
                    }
                    d if d == c || d == '\n' => break,
                    d => s.push(d),
                }
            }
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn walks_scenes_materials_textures_scripts_models_and_resources() {
        let root = std::env::temp_dir().join(format!("misty_deps_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write(
            &root,
            "Assets/Scenes/A.scene",
            r#"{ "objects": [ { "mat": "Assets/Mats/Red.mat", "mesh": "Assets/Models/Ramp.obj#0",
                 "script": "Assets/Scripts/S.lua", "gone": "Assets/Missing.png",
                 "tool": "Assets/Scripts/T.lua", "terrain": "Assets/Terrains/T.terrain",
                 "sprite": "Assets/Sprites/Tiles.png#Tiles_3" } ] }"#,
        );
        write(
            &root,
            "Assets/Mats/Red.mat",
            r#"{ "albedo_map": "Assets/Tex/Red.png" }"#,
        );
        write(&root, "Assets/Tex/Red.png", "png");
        write(&root, "Assets/Tex/Red.png.meta", "{}");
        write(&root, "Assets/Tex/Unused.png", "png");
        write(&root, "Assets/Sprites/Tiles.png", "png");
        write(
            &root,
            "Assets/Scripts/S.lua",
            "-- \"Assets/Commented.prefab\"\nlocal p = 'Assets/Prefabs/Ball.prefab'\n",
        );
        write(
            &root,
            "Assets/Prefabs/Ball.prefab",
            r#"{ "m": "Assets/Mats/Red.mat" }"#,
        );
        write(
            &root,
            "Assets/Models/Ramp.obj",
            "mtllib Ramp.mtl\nv 0 0 0\n",
        );
        write(
            &root,
            "Assets/Models/Ramp.mtl",
            "newmtl A\nmap_Kd -bm 1 ../Tex/Wood.png\n",
        );
        write(&root, "Assets/Tex/Wood.png", "png");
        // Terrain trees: the terrain names its tree recipes, each recipe its textures.
        write(
            &root,
            "Assets/Terrains/T.terrain",
            r#"{ "trees": { "prototypes": [ { "tree": "Assets/Trees/Oak.tree" } ] } }"#,
        );
        write(
            &root,
            "Assets/Trees/Oak.tree",
            r#"{ "bark_texture": "Assets/Trees/Textures/OakBark.png" }"#,
        );
        write(&root, "Assets/Trees/Textures/OakBark.png", "png");
        write(&root, "Assets/Resources/Levels/Data.json", "{}");
        write(&root, "Assets/Unused.scene", "{}");
        // Editor scripts never ship, even when named or under Resources.
        write(&root, "Assets/Resources/Editor/Tool.lua", "-- editor");
        write(&root, "Assets/Editor/Menu.lua", "-- editor");
        write(
            &root,
            "Assets/Scripts/T.lua",
            "local tool = 'Assets/Editor/Menu.lua'
",
        );

        let deps = collect(&root, &["Assets/Scenes/A.scene".into()]);
        let files: Vec<&str> = deps.files.iter().map(String::as_str).collect();
        assert_eq!(
            files,
            [
                "Assets/Mats/Red.mat",
                "Assets/Models/Ramp.mtl",
                "Assets/Models/Ramp.obj",
                "Assets/Prefabs/Ball.prefab",
                "Assets/Resources/Levels/Data.json",
                "Assets/Scenes/A.scene",
                "Assets/Scripts/S.lua",
                "Assets/Scripts/T.lua",
                "Assets/Sprites/Tiles.png",
                "Assets/Terrains/T.terrain",
                "Assets/Tex/Red.png",
                "Assets/Tex/Red.png.meta",
                "Assets/Tex/Wood.png",
                "Assets/Trees/Oak.tree",
                "Assets/Trees/Textures/OakBark.png",
            ]
        );
        assert_eq!(
            deps.missing.get("Assets/Missing.png").map(String::as_str),
            Some("Assets/Scenes/A.scene")
        );
    }

    #[test]
    fn lua_literals_and_paths() {
        assert_eq!(lua_strings(r#"a("x\"y", 'z') -- "c""#), ["xy", "z"]);
        assert_eq!(normalize("Assets/A/../B/./c.png"), "Assets/B/c.png");
        assert_eq!(
            asset_path("Assets/a.mixer#Music").as_deref(),
            Some("Assets/a.mixer")
        );
        assert_eq!(asset_path("Builtin/Square"), None);
    }
}
