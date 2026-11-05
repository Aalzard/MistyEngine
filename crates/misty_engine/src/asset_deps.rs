//! Which assets a build needs: a walk from the build scenes through everything they reference.
//!
//! Scenes, prefabs, materials, animations, controllers, mixers, terrains and nav meshes are JSON,
//! and every reference in them is a project path string ("Assets/…", optionally with a
//! `#sub-asset` suffix), so the walk reads every string of every JSON file it reaches. Lua
//! scripts are scanned for string literals naming assets, `.obj` files for their `.mtl` and
//! textures, `.gltf` files for their buffers and images. Every file under a `Resources` folder is
//! included as well (assets loaded by name at runtime), and so is each file's `.meta` sidecar.

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
const JSON_ASSETS: [&str; 15] = [
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
];

fn ext(rel: &str) -> String {
    Path::new(rel)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Every file `roots` (project-relative) need, plus `Resources` folders under `Assets/`.
pub fn collect(project: &Path, roots: &[String]) -> Dependencies {
    let mut deps = Dependencies::default();
    // Stale paths in files saved with GUIDs follow the renamed or moved asset.
    let guids = GuidDatabase::read(project);
    let mut queue: VecDeque<(String, String)> =
        roots.iter().map(|r| (r.clone(), String::new())).collect();
    for p in crate::vfs::files_under(&project.join("Assets")) {
        let Ok(rel) = p.strip_prefix(project) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if rel.split('/').any(|part| part == "Resources") && !rel.ends_with(".meta") {
            queue.push_back((rel, String::new()));
        }
    }
    while let Some((rel, from)) = queue.pop_front() {
        let rel = normalize(&rel);
        if deps.files.contains(&rel) {
            continue;
        }
        let abs = project.join(&rel);
        if !crate::vfs::is_file(&abs) {
            deps.missing.entry(rel).or_insert(from);
            continue;
        }
        deps.files.insert(rel.clone());
        let meta = format!("{rel}.meta");
        if crate::vfs::is_file(&project.join(&meta)) {
            deps.files.insert(meta);
        }
        for r in references(project, &rel, &guids) {
            if !deps.files.contains(&r) {
                queue.push_back((r, rel.clone()));
            }
        }
    }
    deps
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
fn references(project: &Path, rel: &str, guids: &GuidDatabase) -> Vec<String> {
    let abs = project.join(rel);
    let dir = Path::new(rel).parent().map_or(String::new(), |d| {
        let d = d.to_string_lossy().replace('\\', "/");
        if d.is_empty() { d } else { d + "/" }
    });
    let ext = ext(rel);
    let read = || crate::vfs::read_to_string(&abs).unwrap_or_default();
    let mut out = Vec::new();
    if JSON_ASSETS.contains(&ext.as_str()) {
        let text = read();
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&refs::resolve(guids, &text))
        else {
            return out;
        };
        let mut strings = Vec::new();
        json_strings(&json, None, &mut strings);
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
        write(&root, "Assets/Resources/Levels/Data.json", "{}");
        write(&root, "Assets/Unused.scene", "{}");

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
                "Assets/Sprites/Tiles.png",
                "Assets/Tex/Red.png",
                "Assets/Tex/Red.png.meta",
                "Assets/Tex/Wood.png",
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
