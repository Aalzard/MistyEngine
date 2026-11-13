//! `asset.*`: find, read and create project files. Writes stay inside `Assets/`; overwriting or
//! deleting a file is confirmed in the live editor (files have no undo).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::scene::last_log;
use super::{
    Cx, Kind, Outcome, Tool, ToolResult, assets_path, object_schema, opt_bool, opt_str, opt_u64,
    project_path, str_arg,
};
use crate::actions::Action;
use crate::state::EditorState;

/// Text read by asset.read_text is cut here.
const READ_LIMIT: usize = 256 * 1024;

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "asset.search",
        title: "Search assets",
        description: "Find files under Assets/ by name words and kind (texture, material, script, model, audio, scene, prefab, or a file extension). \"query\" may also hold \"t:kind\".",
        kind: Kind::Read,
        idempotent: true,
        schema: search_schema,
        run: search,
    },
    Tool {
        name: "asset.read_text",
        title: "Read text asset",
        description: "Read a text file of the project (a script, scene, material, settings file…) by its project-relative path.",
        kind: Kind::Read,
        idempotent: true,
        schema: read_schema,
        run: read_text,
    },
    Tool {
        name: "asset.create_script",
        title: "Create Lua script",
        description: "Write a Lua script under Assets/ (the standard behaviour template when \"source\" is left out). Reports Lua syntax errors. Replacing an existing file needs \"overwrite\": true and the user's OK in the editor.",
        kind: Kind::Edit,
        idempotent: false,
        schema: script_schema,
        run: create_script,
    },
    Tool {
        name: "asset.create_material",
        title: "Create material",
        description: "Write a .mat material under Assets/: the default Standard material with an optional color and other material fields (shader, metallic, smoothness, textures…).",
        kind: Kind::Edit,
        idempotent: false,
        schema: material_schema,
        run: create_material,
    },
    Tool {
        name: "asset.import",
        title: "Import file",
        description: "Copy a file from elsewhere on this computer (absolute path: a texture, model, audio clip…) into an Assets/ folder, where the editor imports it. Never overwrites.",
        kind: Kind::Edit,
        idempotent: false,
        schema: import_schema,
        run: import,
    },
    Tool {
        name: "asset.delete",
        title: "Delete asset",
        description: "Move an asset (file or folder under Assets/) to the Recycle Bin / Trash, after the user allows it in the editor.",
        kind: Kind::Destructive,
        idempotent: false,
        schema: delete_schema,
        run: delete,
    },
];

fn search_schema() -> Value {
    object_schema(
        json!({
            "query": { "type": "string", "description": "Words that must appear in the file name; may include \"t:kind\"" },
            "kind": { "type": "string", "description": "texture, material, script, model, audio, scene, prefab, or an extension (\"json\")" },
            "folder": { "type": "string", "description": "Only under this folder (\"Assets/Scripts\")" },
            "limit": { "type": "integer", "minimum": 1, "description": "Most results (default 200)" },
        }),
        &[],
    )
}

fn read_schema() -> Value {
    object_schema(
        json!({
            "path": { "type": "string", "description": "Project-relative path (\"Assets/Scripts/Player.lua\")" },
        }),
        &["path"],
    )
}

fn script_schema() -> Value {
    object_schema(
        json!({
            "path": { "type": "string", "description": "Where to write it, under Assets/ (\".lua\" is added if missing)" },
            "source": { "type": "string", "description": "The script text (default: the behaviour template)" },
            "overwrite": { "type": "boolean", "description": "Replace an existing file (asks the user in the editor)" },
        }),
        &["path"],
    )
}

fn material_schema() -> Value {
    object_schema(
        json!({
            "path": { "type": "string", "description": "Where to write it, under Assets/ (\".mat\" is added if missing)" },
            "color": {
                "type": "array",
                "items": { "type": "number" },
                "minItems": 3,
                "maxItems": 4,
                "description": "Albedo [r, g, b] or [r, g, b, a], 0..1 sRGB",
            },
            "fields": { "type": "object", "description": "Other material fields by name (as in a .mat file)" },
            "overwrite": { "type": "boolean", "description": "Replace an existing file (asks the user in the editor)" },
        }),
        &["path"],
    )
}

fn import_schema() -> Value {
    object_schema(
        json!({
            "source": { "type": "string", "description": "Absolute path of the file to copy in" },
            "folder": { "type": "string", "description": "Destination folder under Assets (default \"Assets\")" },
            "name": { "type": "string", "description": "File name in the project (default: the source's)" },
        }),
        &["source"],
    )
}

fn delete_schema() -> Value {
    object_schema(
        json!({ "path": { "type": "string", "description": "Project path under Assets/" } }),
        &["path"],
    )
}

/// Does `rel` (not a `.meta`) match an asset kind?
fn kind_matches(rel: &str, kind: &str) -> bool {
    let ext = rel.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match kind.to_ascii_lowercase().trim_start_matches('.') {
        "" | "any" | "asset" => true,
        "texture" => misty_engine::assets::is_texture(rel),
        "material" => ext == "mat",
        "script" => ext == "lua",
        "model" => misty_engine::models::is_model(rel),
        "audio" => misty_engine::audio::is_clip(rel),
        k => ext == k,
    }
}

fn search(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let root = cx.s.project.root.clone();
    let mut kind = opt_str(args, "kind").unwrap_or("").to_owned();
    let mut words = Vec::new();
    for w in opt_str(args, "query").unwrap_or("").split_whitespace() {
        match w.strip_prefix("t:") {
            Some(k) => kind = k.to_owned(),
            None => words.push(w.to_lowercase()),
        }
    }
    let folder = opt_str(args, "folder")
        .map(|f| project_path(f.trim_end_matches('/')).map(|f| f + "/"))
        .transpose()?;
    let limit = opt_u64(args, "limit").unwrap_or(200) as usize;
    let mut found: Vec<String> = misty_engine::vfs::files_under(&root.join("Assets"))
        .into_iter()
        .map(|p| crate::asset_ops::rel(&root, &p))
        .filter(|rel| !rel.ends_with(".meta"))
        .filter(|rel| kind_matches(rel, &kind))
        .filter(|rel| folder.as_ref().is_none_or(|f| rel.starts_with(f.as_str())))
        .filter(|rel| {
            let name = rel.rsplit('/').next().unwrap_or(rel).to_lowercase();
            words.iter().all(|w| name.contains(w.as_str()))
        })
        .collect();
    found.sort();
    let total = found.len();
    found.truncate(limit);
    Ok(Outcome::ok(json!({ "total": total, "paths": found })))
}

fn read_text(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let rel = project_path(str_arg(args, "path")?)?;
    let abs = cx.s.project.root.join(&rel);
    if abs.is_dir() {
        return Err(format!("{rel} is a folder (asset.search lists files)"));
    }
    let (shown, size) = read_prefix(&abs, READ_LIMIT).map_err(|e| format!("{rel}: {e}"))?;
    let truncated = size > READ_LIMIT as u64;
    let shown = &shown[..];
    let text = match std::str::from_utf8(shown) {
        Ok(t) => t.to_owned(),
        // Cut inside a character at the limit: drop the partial one.
        Err(e) if truncated && e.error_len().is_none() => {
            String::from_utf8_lossy(&shown[..e.valid_up_to()]).into_owned()
        }
        Err(_) => return Err(format!("{rel} is not a text file")),
    };
    let mut out = ToolResult::json(json!({
        "path": rel,
        "bytes": size,
        "truncated": truncated,
    }));
    // The text itself as its own block, unescaped, so it reads like the file.
    out.content.push(json!({ "type": "text", "text": text }));
    Ok(Outcome::Done(out))
}

/// The first `limit` bytes of a file and its whole size: a huge file (a baked lightmap, a
/// video, a log someone left in Assets) is not read whole just to show its start.
pub(crate) fn read_prefix(path: &Path, limit: usize) -> std::io::Result<(Vec<u8>, u64)> {
    use std::io::Read;
    if misty_engine::vfs::is_mounted() {
        let bytes = misty_engine::vfs::read(path)?;
        let size = bytes.len() as u64;
        return Ok((bytes.into_iter().take(limit).collect(), size));
    }
    let file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    let mut out = Vec::with_capacity(limit.min(size as usize));
    file.take(limit as u64).read_to_end(&mut out)?;
    Ok((out, size))
}

/// After writing an asset: rescan and let importers and editor-script hooks see it.
fn created(s: &mut EditorState, rel: &str) {
    s.refresh_assets();
    s.assets.imported.push(rel.to_owned());
}

/// `rel` with `ext` added when it has another or none.
fn with_ext(mut rel: String, ext: &str) -> String {
    if !rel.to_ascii_lowercase().ends_with(ext) {
        rel.push_str(ext);
    }
    rel
}

fn write_file(abs: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(dir) = abs.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(abs, data).map_err(|e| format!("{}: {e}", abs.display()))
}

/// Run `write` now, or after the user's OK when it replaces a file.
fn write_or_confirm(
    cx: &mut Cx<'_>,
    rel: &str,
    overwrite: bool,
    write: impl FnOnce(&mut Cx<'_>) -> Outcome + 'static,
) -> Result<Outcome, String> {
    let abs = cx.s.project.root.join(rel);
    if abs.is_dir() {
        return Err(format!("{rel} is a folder"));
    }
    if !abs.exists() {
        return Ok(write(cx));
    }
    if !overwrite {
        return Err(format!(
            "{rel} already exists: pass \"overwrite\": true to replace it"
        ));
    }
    Ok(Outcome::Confirm {
        what: format!("overwrite {rel}"),
        then: Box::new(write),
    })
}

fn create_script(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let rel = with_ext(assets_path(str_arg(args, "path")?)?, ".lua");
    let stem = Path::new(&rel)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    crate::asset_ops::valid_name(&stem)?;
    let source = opt_str(args, "source")
        .map(str::to_owned)
        .unwrap_or_else(|| misty_engine::scripting::template(&stem));
    let syntax_error = mlua::Lua::new()
        .load(&source)
        .set_name(&rel)
        .into_function()
        .err()
        .map(|e| crate::editor_scripts::describe(&e));
    let replacing = cx.s.project.root.join(&rel).exists();
    let overwrite = opt_bool(args, "overwrite").unwrap_or(false);
    let rel2 = rel.clone();
    write_or_confirm(cx, &rel, overwrite, move |cx| {
        let abs = cx.s.project.root.join(&rel2);
        if let Err(e) = write_file(&abs, source.as_bytes()) {
            return Outcome::Done(ToolResult::error(e));
        }
        created(cx.s, &rel2);
        Outcome::ok(json!({
            "path": rel2,
            "replaced": replacing,
            "syntax_error": syntax_error,
        }))
    })
}

fn create_material(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let rel = with_ext(assets_path(str_arg(args, "path")?)?, ".mat");
    let mut v = serde_json::to_value(misty_engine::assets::Material::default())
        .map_err(|e| e.to_string())?;
    if let Some(c) = args.get("color").and_then(Value::as_array) {
        let mut rgba: Vec<f64> = c.iter().filter_map(Value::as_f64).collect();
        if rgba.len() == 3 {
            rgba.push(1.0);
        }
        if rgba.len() != 4 {
            return Err("\"color\" must be [r, g, b] or [r, g, b, a]".into());
        }
        v["color"] = json!(rgba);
    }
    if let Some(fields) = args.get("fields").and_then(Value::as_object) {
        for (k, val) in fields {
            if v.get(k).is_none() {
                let names: Vec<&String> = v
                    .as_object()
                    .map(|m| m.keys().collect())
                    .unwrap_or_default();
                return Err(format!(
                    "no material field \"{k}\" (fields: {})",
                    names
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            v[k] = val.clone();
        }
    }
    let material: misty_engine::assets::Material =
        serde_json::from_value(v).map_err(|e| format!("material: {e}"))?;
    let overwrite = opt_bool(args, "overwrite").unwrap_or(false);
    let rel2 = rel.clone();
    write_or_confirm(cx, &rel, overwrite, move |cx| {
        let abs = cx.s.project.root.join(&rel2);
        if let Some(dir) = abs.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            return Outcome::Done(ToolResult::error(e.to_string()));
        }
        if let Err(e) = material.save(&abs) {
            return Outcome::Done(ToolResult::error(format!("{rel2}: {e}")));
        }
        created(cx.s, &rel2);
        let _ = cx.s.assets.reload_material(&rel2);
        Outcome::ok(json!({ "path": rel2 }))
    })
}

fn import(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let s = &mut *cx.s;
    let source = PathBuf::from(str_arg(args, "source")?);
    if !source.is_absolute() {
        return Err("\"source\" must be an absolute path".into());
    }
    if !source.is_file() {
        return Err(format!("{} is not a file", source.display()));
    }
    let folder = match opt_str(args, "folder").map(|f| f.trim_end_matches(['/', '\\'])) {
        None | Some("Assets") => "Assets".to_owned(),
        Some(f) => assets_path(f)?,
    };
    let name = match opt_str(args, "name") {
        Some(n) => n.to_owned(),
        None => source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or("the source has no file name")?,
    };
    if name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(format!("\"{name}\" is not a file name"));
    }
    let rel = format!("{folder}/{name}");
    let dest = s.project.root.join(&rel);
    if dest.exists() {
        return Err(format!("{rel} already exists"));
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::copy(&source, &dest).map_err(|e| format!("copy failed: {e}"))?;
    created(s, &rel);
    s.console.info(format!("Imported {rel}"));
    Ok(Outcome::ok(json!({ "path": rel })))
}

fn delete(cx: &mut Cx<'_>, args: &Value) -> Result<Outcome, String> {
    let rel = assets_path(str_arg(args, "path")?)?;
    let abs = cx.s.project.root.join(&rel);
    if !abs.exists() {
        return Err(format!("no asset at {rel}"));
    }
    if cx.s.project.is_read_only(&abs) {
        return Err(format!("{rel} is read-only"));
    }
    let what = format!("delete {rel} (to the Recycle Bin)");
    Ok(Outcome::Confirm {
        what,
        then: Box::new(move |cx| {
            crate::commands::apply(cx.s, Action::DeleteAsset(abs.clone()));
            if abs.exists() {
                Outcome::Done(ToolResult::error(last_log(cx.s)))
            } else {
                Outcome::ok(json!({ "deleted": rel }))
            }
        }),
    })
}

#[cfg(test)]
mod tests {
    /// asset.read_text reads a big file's start only, and still tells its whole size (it used
    /// to read the whole file, then cut it).
    #[test]
    fn big_files_are_read_only_up_to_the_limit() {
        let path =
            std::env::temp_dir().join(format!("misty_read_prefix_{}.txt", std::process::id()));
        std::fs::write(&path, "x".repeat(3 * 1024 * 1024)).unwrap();
        let (bytes, size) = super::read_prefix(&path, super::READ_LIMIT).unwrap();
        assert_eq!(size, 3 * 1024 * 1024);
        assert_eq!(bytes.len(), super::READ_LIMIT);
        assert!(
            bytes.capacity() <= super::READ_LIMIT,
            "{} bytes held",
            bytes.capacity()
        );
        let (small, size) = super::read_prefix(&path, 10).unwrap();
        assert_eq!((small.len(), size), (10, 3 * 1024 * 1024));
        std::fs::remove_file(path).ok();
    }
}
