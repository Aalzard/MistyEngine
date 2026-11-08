//! 2D sprites: texture import settings (`.meta` sidecars), sprites cut from textures, built-in
//! shapes, Sorting Layers and the draws of Sprite Renderers.
//!
//! A sprite reference is the texture path, plus `#<sprite name>` for one sprite of a texture in
//! Multiple mode ("Assets/Sprites/Tiles.png#Tiles_3"). Sprites are drawn as unlit, alpha-blended,
//! two-sided quads ("Sprites-Default") in the transparent queue, sorted by Sorting Layer, Order
//! in Layer and then camera distance.

pub mod builtin;
pub mod compress;
pub mod import;
mod render;
pub mod slice;
pub mod sorting;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Instant, SystemTime};

use glam::Vec2;
use image::RgbaImage;

use crate::assets;
pub use import::{
    FilterMode, PlatformOverride, SpriteMode, SpriteRect, TextureCompression, TextureImporter,
    TextureType, WrapMode,
};
pub use render::{draw_of, draws, local_bounds, world_corners};

/// Name of the material every Sprite Renderer uses.
pub const DEFAULT_MATERIAL: &str = "Sprites-Default";

/// A sprite: a pixel rect of a texture with a pivot and a pixels-per-unit scale.
#[derive(Clone, Debug, PartialEq)]
pub struct Sprite {
    /// Project-relative texture path (or a built-in `Builtin/…` name).
    pub texture: String,
    pub name: String,
    /// x, y, width, height in pixels, y from the bottom edge.
    pub rect: [f32; 4],
    /// Normalised pivot inside the rect.
    pub pivot: Vec2,
    pub pixels_per_unit: f32,
    pub texture_size: [u32; 2],
}

impl Sprite {
    /// Size in world units.
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.rect[2], self.rect[3]) / self.pixels_per_unit
    }

    /// Local bounds (min, max) around the pivot, in world units.
    pub fn bounds(&self) -> (Vec2, Vec2) {
        let s = self.size();
        (-self.pivot * s, (Vec2::ONE - self.pivot) * s)
    }

    /// Texture scale and offset that map the unit quad (u right, v down) onto the rect.
    pub fn uv_transform(&self, flip_x: bool, flip_y: bool) -> ([f32; 2], [f32; 2]) {
        let [w, h] = self.texture_size.map(|v| v.max(1) as f32);
        let [x, y, rw, rh] = self.rect;
        // Image rows count down from the top.
        let top = (h - (y + rh)) / h;
        let (mut su, mut ou) = (rw / w, x / w);
        let (mut sv, mut ov) = (rh / h, top);
        if flip_x {
            ou += su;
            su = -su;
        }
        if flip_y {
            ov += sv;
            sv = -sv;
        }
        ([su, sv], [ou, ov])
    }

    /// Normalised uv corners (min, max) with v up, for UI quads.
    pub fn uv_rect(&self) -> (Vec2, Vec2) {
        let [w, h] = self.texture_size.map(|v| v.max(1) as f32);
        let [x, y, rw, rh] = self.rect;
        (
            Vec2::new(x / w, y / h),
            Vec2::new((x + rw) / w, (y + rh) / h),
        )
    }
}

struct Entry {
    checked: Instant,
    stamps: (Option<SystemTime>, Option<SystemTime>),
    importer: TextureImporter,
    size: Option<[u32; 2]>,
}

static CACHE: LazyLock<Mutex<HashMap<String, Entry>>> = LazyLock::new(Default::default);

fn stamps(rel: &str) -> (Option<SystemTime>, Option<SystemTime>) {
    let abs = assets::absolute(rel);
    let m = crate::vfs::modified;
    (m(&abs), m(&TextureImporter::meta_path(&abs)))
}

/// Import settings and pixel size of a texture, re-read when its files change (checked at most
/// twice a second).
pub fn texture_info(rel: &str) -> (TextureImporter, Option<[u32; 2]>) {
    if let Some(imp) = builtin::importer(rel) {
        return (imp, builtin::size(rel));
    }
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if let Some(e) = cache.get_mut(rel) {
        if now.duration_since(e.checked).as_secs_f32() < 0.5 {
            return (e.importer.clone(), e.size);
        }
        e.checked = now;
        if stamps(rel) == e.stamps {
            return (e.importer.clone(), e.size);
        }
    }
    let abs = assets::absolute(rel);
    let entry = Entry {
        checked: now,
        stamps: stamps(rel),
        importer: TextureImporter::load(&abs),
        size: crate::render::texture::image_size(&abs),
    };
    let out = (entry.importer.clone(), entry.size);
    cache.insert(rel.to_owned(), entry);
    out
}

/// Import settings of a texture (defaults without a `.meta` file).
pub fn importer(rel: &str) -> TextureImporter {
    texture_info(rel).0
}

/// Forget cached settings (the editor applied new import settings).
pub fn invalidate(rel: &str) {
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).remove(rel);
}

/// Decode a texture for the GPU: scaled to its import Max Size and the active quality level's
/// texture limit (built-in sprites as they are).
pub fn load_texture(rel: &str) -> Result<RgbaImage, String> {
    let img = load_image(rel)?;
    if builtin::is_builtin(rel) {
        return Ok(img);
    }
    let max = importer(rel).max_size_for(import::current_platform());
    let limit = crate::quality::active().texture_size_limit(max);
    Ok(crate::render::texture::fit(img, limit))
}

/// Decode a texture by project path at full size, including built-in sprites.
pub fn load_image(rel: &str) -> Result<RgbaImage, String> {
    match builtin::image(rel) {
        Some(img) => Ok(img),
        None => crate::render::texture::load_image(&assets::absolute(rel)),
    }
}

/// Split a sprite reference into the texture path and the sprite name.
pub fn split(reference: &str) -> (&str, Option<&str>) {
    match reference.split_once('#') {
        Some((t, n)) => (t, Some(n)),
        None => (reference, None),
    }
}

/// The sprite a reference names (`None` when the texture or the named sprite is missing).
pub fn resolve(reference: &str) -> Option<Sprite> {
    let (texture, name) = split(reference);
    let (imp, size) = texture_info(texture);
    let size = size?;
    let stem = std::path::Path::new(texture)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (name, rect, pivot) = match imp.sprite_mode {
        SpriteMode::Single => (stem, [0.0, 0.0, size[0] as f32, size[1] as f32], imp.pivot),
        SpriteMode::Multiple => {
            let r = match name {
                Some(n) => imp.find(n)?,
                None => imp.sprites.first()?,
            };
            (r.name.clone(), r.rect, r.pivot)
        }
    };
    Some(Sprite {
        texture: texture.to_owned(),
        name,
        rect,
        pivot: Vec2::from(pivot),
        pixels_per_unit: imp.pixels_per_unit,
        texture_size: size,
    })
}

/// Every sprite reference a texture provides (one in Single mode, one per rect in Multiple).
pub fn references(texture: &str) -> Vec<String> {
    let imp = importer(texture);
    match imp.sprite_mode {
        SpriteMode::Single => vec![texture.to_owned()],
        SpriteMode::Multiple => imp
            .sprites
            .iter()
            .map(|s| format!("{texture}#{}", s.name))
            .collect(),
    }
}

/// Display name of a sprite reference ("Tiles_3", "Player", "None (Sprite)").
pub fn display_name(reference: Option<&str>) -> String {
    match reference {
        None => "None (Sprite)".into(),
        Some(r) => match split(r) {
            (_, Some(n)) => n.to_owned(),
            (t, None) => std::path::Path::new(t)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        },
    }
}

#[cfg(test)]
mod tests;
