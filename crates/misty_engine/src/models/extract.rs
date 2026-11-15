//! Material extraction: each model material becomes a `.mat` asset next to the model
//! ("Ship_Hull.mat"), with embedded textures written as PNG ("Ship_Hull_Albedo.png").
//! Existing files are never overwritten, so edits to extracted materials survive re-imports.
//!
//! Paths are project-relative. The plain functions resolve them against the global project
//! root ([`assets::set_project_root`]); the `_in` ones against the root they are given, so an
//! import of one project can't write into another one that a different thread set as the
//! global root meanwhile (the editor's tests open temp copies of the sample project while
//! others set the root to the repo's own: `Crate_Metal.mat` and friends were extracted into
//! the repo).

use std::path::{Path, PathBuf};

use super::{ExtraMap, Model, TextureSource};
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

/// A project root to resolve paths against (`None` = the global one).
#[derive(Clone, Copy)]
struct Root<'a>(Option<&'a Path>);

impl Root<'_> {
    fn absolute(self, rel: &str) -> PathBuf {
        match self.0 {
            Some(root) => root.join(rel),
            None => assets::absolute(rel),
        }
    }

    fn relative(self, path: &Path) -> String {
        match self.0 {
            Some(root) => path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/"),
            None => assets::relative(path),
        }
    }
}

/// Model folder (absolute) and file stem.
fn base(root: Root, model_rel: &str) -> (PathBuf, String) {
    let abs = root.absolute(model_rel);
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
    material_path(Root(None), model_rel, model, index)
}

fn material_path(root: Root, model_rel: &str, model: &Model, index: usize) -> Option<String> {
    let m = model.materials.get(index)?;
    let (mut dir, stem) = base(root, model_rel);
    // Import setting: materials extracted to another folder ("Extract Materials…").
    if !model.material_folder.is_empty() {
        dir = root.absolute(&model.material_folder);
    }
    // Two materials with the same name get the index appended.
    let dupes = model.materials.iter().filter(|o| o.name == m.name).count();
    let name = if dupes > 1 {
        format!("{stem}_{} {index}", clean(&m.name))
    } else {
        format!("{stem}_{}", clean(&m.name))
    };
    Some(root.relative(&dir.join(format!("{name}.mat"))))
}

/// Write missing `.mat` files (and embedded textures) for the model's materials.
/// Returns the project-relative paths of the files created.
pub fn extract_materials(model_rel: &str, model: &Model) -> Result<Vec<String>, String> {
    extract_materials_at(Root(None), model_rel, model)
}

/// [`extract_materials`] in the project at `root`.
pub fn extract_materials_in(
    root: &Path,
    model_rel: &str,
    model: &Model,
) -> Result<Vec<String>, String> {
    extract_materials_at(Root(Some(root)), model_rel, model)
}

fn extract_materials_at(root: Root, model_rel: &str, model: &Model) -> Result<Vec<String>, String> {
    let mut created = Vec::new();
    for (i, m) in model.materials.iter().enumerate() {
        let Some(rel) = material_path(root, model_rel, model, i) else {
            continue;
        };
        let path = root.absolute(&rel);
        if path.exists() {
            continue;
        }
        let dir = path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut material = m.material.clone();
        for (slot, source, target) in [
            ("Albedo", &m.albedo, &mut material.albedo_map),
            ("Normal", &m.normal, &mut material.normal_map),
        ] {
            *target = match source {
                Some(TextureSource::File(p)) => Some(root.relative(p)),
                Some(TextureSource::Embedded(bytes)) => {
                    let png = dir.join(format!("{stem}_{slot}.png"));
                    if !png.exists() {
                        write_png(bytes, &png)?;
                        created.push(root.relative(&png));
                    }
                    Some(root.relative(&png))
                }
                None => None,
            };
        }
        // Feature maps from glTF extensions (Clear Coat normal).
        for (map, source) in &m.extra_maps {
            let rel = match source {
                // Maps the material reads differently (gloss, translucency): a converted
                // copy next to the material.
                // A map that can't be read is left out (the material is still extracted):
                // without its gloss map the material keeps the usual smoothness.
                source if map.converted() => {
                    let png = dir.join(format!("{stem}_{}.png", map.label()));
                    if !png.exists() {
                        let read = match source {
                            TextureSource::File(p) => crate::vfs::read(p).ok(),
                            TextureSource::Embedded(bytes) => Some(bytes.clone()),
                        };
                        let Some(img) = read
                            .and_then(|b| crate::render::texture::decode(&b).ok())
                            .map(|i| i.into_rgba8())
                        else {
                            if matches!(map, ExtraMap::Gloss | ExtraMap::Roughness) {
                                material.smoothness = crate::assets::Material::default().smoothness;
                            }
                            continue;
                        };
                        // (The 2K the material would be drawn at.)
                        let img = crate::render::texture::fit(img, 2048);
                        map.convert(&img)
                            .save_with_format(&png, image::ImageFormat::Png)
                            .map_err(|e| format!("{}: {e}", png.display()))?;
                        created.push(root.relative(&png));
                    }
                    root.relative(&png)
                }
                TextureSource::File(p) => root.relative(p),
                TextureSource::Embedded(bytes) => {
                    let png = dir.join(format!("{stem}_{}.png", map.label()));
                    if !png.exists() {
                        write_png(bytes, &png)?;
                        created.push(root.relative(&png));
                    }
                    root.relative(&png)
                }
            };
            *map.slot(&mut material) = Some(rel);
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
    crate::render::texture::decode(bytes)
        .and_then(|img| img.save_with_format(path, image::ImageFormat::Png))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Project-relative `.anim` path for animation `index` of the model ("Tentacle_Bend.anim").
pub fn animation_asset_path(model_rel: &str, model: &Model, index: usize) -> Option<String> {
    animation_path(Root(None), model_rel, model, index)
}

fn animation_path(root: Root, model_rel: &str, model: &Model, index: usize) -> Option<String> {
    let a = model.animations.get(index)?;
    let (dir, stem) = base(root, model_rel);
    let dupes = model.animations.iter().filter(|o| o.name == a.name).count();
    let name = if dupes > 1 {
        format!("{stem}_{} {index}", clean(&a.name))
    } else {
        format!("{stem}_{}", clean(&a.name))
    };
    Some(root.relative(&dir.join(format!("{name}.anim"))))
}

/// Write missing `.anim` clips for the model's animations (existing files are kept, so edits
/// survive re-imports). Returns the project-relative paths of the files created.
pub fn extract_animations(model_rel: &str, model: &Model) -> Result<Vec<String>, String> {
    extract_animations_at(Root(None), model_rel, model)
}

/// [`extract_animations`] in the project at `root`.
pub fn extract_animations_in(
    root: &Path,
    model_rel: &str,
    model: &Model,
) -> Result<Vec<String>, String> {
    extract_animations_at(Root(Some(root)), model_rel, model)
}

fn extract_animations_at(
    root: Root,
    model_rel: &str,
    model: &Model,
) -> Result<Vec<String>, String> {
    let mut created = Vec::new();
    for (i, a) in model.animations.iter().enumerate() {
        let Some(rel) = animation_path(root, model_rel, model, i) else {
            continue;
        };
        let path = root.absolute(&rel);
        if path.exists() {
            continue;
        }
        a.clip.save(&path)?;
        created.push(rel);
    }
    Ok(created)
}

/// Write every clip of the model, replacing extracted `.anim` files (the clip settings were
/// applied: the clips are made again from the file). Returns the paths written.
pub fn refresh_animations(model_rel: &str, model: &Model) -> Result<Vec<String>, String> {
    let mut written = Vec::new();
    for (i, a) in model.animations.iter().enumerate() {
        let Some(rel) = animation_asset_path(model_rel, model, i) else {
            continue;
        };
        a.clip.save(&assets::absolute(&rel))?;
        written.push(rel);
    }
    Ok(written)
}
