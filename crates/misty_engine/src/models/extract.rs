//! Material extraction: each model material becomes a `.mat` asset next to the model
//! ("Ship_Hull.mat"), with embedded textures written as PNG ("Ship_Hull_Albedo.png").
//! Existing files are never overwritten, so edits to extracted materials survive re-imports.

use std::path::{Path, PathBuf};

use super::{Model, TextureSource};
use crate::assets;

/// File-name-safe version of a name.
fn clean(name: &str) -> String {
    name.chars()
        .map(|c| {
            if r#"<>:"/\|?*"#.contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// Model folder (absolute) and file stem.
fn base(model_rel: &str) -> (PathBuf, String) {
    let abs = assets::absolute(model_rel);
    let stem = abs
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    (
        abs.parent().map(Path::to_path_buf).unwrap_or_default(),
        stem,
    )
}

/// Project-relative `.mat` path for material `index` of the model at `model_rel`.
pub fn material_asset_path(model_rel: &str, model: &Model, index: usize) -> Option<String> {
    let m = model.materials.get(index)?;
    let (dir, stem) = base(model_rel);
    // Two materials with the same name get the index appended.
    let dupes = model.materials.iter().filter(|o| o.name == m.name).count();
    let name = if dupes > 1 {
        format!("{stem}_{} {index}", clean(&m.name))
    } else {
        format!("{stem}_{}", clean(&m.name))
    };
    Some(assets::relative(&dir.join(format!("{name}.mat"))))
}

/// Write missing `.mat` files (and embedded textures) for the model's materials.
/// Returns the project-relative paths of the files created.
pub fn extract_materials(model_rel: &str, model: &Model) -> Result<Vec<String>, String> {
    let mut created = Vec::new();
    for (i, m) in model.materials.iter().enumerate() {
        let Some(rel) = material_asset_path(model_rel, model, i) else {
            continue;
        };
        let path = assets::absolute(&rel);
        if path.exists() {
            continue;
        }
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dir = path.parent().unwrap_or(Path::new("."));
        let mut material = m.material.clone();
        for (slot, source, target) in [
            ("Albedo", &m.albedo, &mut material.albedo_map),
            ("Normal", &m.normal, &mut material.normal_map),
        ] {
            *target = match source {
                Some(TextureSource::File(p)) => Some(assets::relative(p)),
                Some(TextureSource::Embedded(bytes)) => {
                    let png = dir.join(format!("{stem}_{slot}.png"));
                    if !png.exists() {
                        write_png(bytes, &png)?;
                        created.push(assets::relative(&png));
                    }
                    Some(assets::relative(&png))
                }
                None => None,
            };
        }
        material.save(&path)?;
        created.push(rel);
    }
    Ok(created)
}

/// Save image bytes as PNG (PNG data is copied as-is, other formats are re-encoded).
fn write_png(bytes: &[u8], path: &Path) -> Result<(), String> {
    if bytes.starts_with(b"\x89PNG") {
        return std::fs::write(path, bytes).map_err(|e| e.to_string());
    }
    image::load_from_memory(bytes)
        .and_then(|img| img.save_with_format(path, image::ImageFormat::Png))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Project-relative `.anim` path for animation `index` of the model ("Tentacle_Bend.anim").
pub fn animation_asset_path(model_rel: &str, model: &Model, index: usize) -> Option<String> {
    let a = model.animations.get(index)?;
    let (dir, stem) = base(model_rel);
    let dupes = model.animations.iter().filter(|o| o.name == a.name).count();
    let name = if dupes > 1 {
        format!("{stem}_{} {index}", clean(&a.name))
    } else {
        format!("{stem}_{}", clean(&a.name))
    };
    Some(assets::relative(&dir.join(format!("{name}.anim"))))
}

/// Write missing `.anim` clips for the model's animations (existing files are kept, so edits
/// survive re-imports). Returns the project-relative paths of the files created.
pub fn extract_animations(model_rel: &str, model: &Model) -> Result<Vec<String>, String> {
    let mut created = Vec::new();
    for (i, a) in model.animations.iter().enumerate() {
        let Some(rel) = animation_asset_path(model_rel, model, i) else {
            continue;
        };
        let path = assets::absolute(&rel);
        if path.exists() {
            continue;
        }
        a.clip.save(&path)?;
        created.push(rel);
    }
    Ok(created)
}
