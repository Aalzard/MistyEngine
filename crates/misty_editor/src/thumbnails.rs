//! Texture previews for the Project window, decoded once per file version, and the material
//! previews (rendered by `material_preview`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Longest side of a preview.
const SIZE: u32 = 96;

#[derive(Default)]
pub struct Thumbnails {
    /// None = the file could not be decoded (the icon is shown instead).
    cache: HashMap<PathBuf, (Option<SystemTime>, Option<egui::TextureHandle>)>,
    /// Material thumbnails and the Inspector's preview sphere.
    pub materials: crate::material_preview::MaterialPreviews,
}

impl Thumbnails {
    /// Preview of an image file (decoded on first request and when the file changes).
    pub fn get(&mut self, ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if let Some((m, tex)) = self.cache.get(path)
            && *m == modified
        {
            return tex.clone();
        }
        let tex = decode(path).map(|img| {
            ctx.load_texture(
                format!("thumb:{}", path.display()),
                img,
                egui::TextureOptions::LINEAR,
            )
        });
        self.cache
            .insert(path.to_path_buf(), (modified, tex.clone()));
        tex
    }
}

fn decode(path: &Path) -> Option<egui::ColorImage> {
    let img = misty_engine::image::open(path).ok()?;
    let small = img.thumbnail(SIZE, SIZE).into_rgba8();
    let size = [small.width() as usize, small.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(
        size,
        small.as_raw(),
    ))
}
