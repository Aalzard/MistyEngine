//! Built tree models per `.tree` asset, shared by every terrain that places the tree.
//!
//! Building (meshes and the impostor bake) runs on a worker thread: [`get`] returns `None`
//! until the first build is done, then keeps returning the last good model while a newer one is
//! built after the asset or one of its textures changed on disk (checked twice a second).
//! With grass streaming off (`render::set_grass_streaming(false)`: tests, screenshots) [`get`]
//! builds in place, so a render is complete on its first frame.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime};

use image::RgbaImage;

use super::TreeAsset;
use super::generate::{self, TreeModel};
use super::impostor::{BakeTextures, lin};

/// A built tree: its recipe, the model and a version that changes with every rebuild.
#[derive(Debug)]
pub struct Built {
    pub asset: TreeAsset,
    pub model: TreeModel,
    pub version: u64,
}

#[derive(Default)]
struct Slot {
    ready: Option<Arc<Built>>,
    error: Option<String>,
    building: bool,
    /// Modification times of the asset and its textures when the last build started.
    stamps: Vec<Option<SystemTime>>,
    checked: Option<Instant>,
}

fn slots() -> &'static Mutex<HashMap<String, Slot>> {
    static SLOTS: OnceLock<Mutex<HashMap<String, Slot>>> = OnceLock::new();
    SLOTS.get_or_init(Default::default)
}

fn next_version() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn stamp(rel: &str) -> Option<SystemTime> {
    crate::vfs::modified(&crate::assets::absolute(rel))
}

/// The asset file and the textures it names (read from the file as it is now).
fn stamps(path: &str) -> Vec<Option<SystemTime>> {
    let mut out = vec![stamp(path)];
    if let Ok(t) = TreeAsset::load(&crate::assets::absolute(path)) {
        for tex in [&t.bark_texture, &t.bark_normal, &t.leaf_texture, &t.leaf_normal] {
            out.push(stamp(tex));
        }
    }
    out
}

fn load_image(rel: &str) -> Option<RgbaImage> {
    if rel.is_empty() {
        return None;
    }
    let bytes = crate::vfs::read(crate::assets::absolute(rel)).ok()?;
    image::load_from_memory(&bytes).ok().map(|i| i.into_rgba8())
}

/// Mean linear colour of the opaque texels.
fn mean(img: &RgbaImage) -> [f32; 3] {
    let (mut sum, mut n) = ([0.0f64; 3], 0.0f64);
    for p in img.pixels().step_by(7) {
        if p[3] > 127 {
            for k in 0..3 {
                sum[k] += lin(p[k] as f32 / 255.0) as f64;
            }
            n += 1.0;
        }
    }
    let n = n.max(1.0);
    sum.map(|s| (s / n) as f32)
}

/// What the impostor bake samples from the asset's textures.
pub fn bake_textures(t: &TreeAsset) -> BakeTextures {
    let leaf = load_image(&t.leaf_texture).map(|img| {
        image::imageops::resize(&img, 256, 256, image::imageops::FilterType::Triangle)
    });
    BakeTextures {
        leaf,
        bark_mean: load_image(&t.bark_texture).map(|b| mean(&b)),
    }
}

/// Build the model of a recipe (any thread).
pub fn build(asset: TreeAsset) -> Built {
    let textures = bake_textures(&asset);
    let model = generate::build(&asset, &textures);
    Built {
        asset,
        model,
        version: next_version(),
    }
}

fn build_path(path: &str) -> Result<Built, String> {
    let asset = TreeAsset::load(&crate::assets::absolute(path))?;
    Ok(build(asset))
}

fn finish(path: &str, result: Result<Built, String>) {
    let mut slots = slots().lock().unwrap_or_else(|e| e.into_inner());
    let slot = slots.entry(path.to_owned()).or_default();
    slot.building = false;
    match result {
        Ok(b) => {
            slot.ready = Some(Arc::new(b));
            slot.error = None;
        }
        Err(e) => slot.error = Some(e),
    }
}

/// The built model of the `.tree` asset at `path` (project-relative), or None while it is
/// first built or when it fails to load ([`error`]).
pub fn get(path: &str) -> Option<Arc<Built>> {
    if path.is_empty() {
        return None;
    }
    let wait = !crate::render::grass_streaming();
    let start = {
        let mut slots = slots().lock().unwrap_or_else(|e| e.into_inner());
        let slot = slots.entry(path.to_owned()).or_default();
        let now = Instant::now();
        let poll = slot
            .checked
            .is_none_or(|t| now.duration_since(t).as_secs_f32() > 0.5);
        let mut start = false;
        if poll && !slot.building {
            slot.checked = Some(now);
            let current = stamps(path);
            let stale = slot.ready.is_none() && slot.error.is_none() || current != slot.stamps;
            if stale {
                slot.stamps = current;
                slot.building = true;
                start = true;
            }
        }
        if !start {
            return slot.ready.clone();
        }
        start
    };
    if start {
        let owned = path.to_owned();
        if wait {
            finish(&owned, build_path(&owned));
        } else {
            let spawned = std::thread::Builder::new()
                .name("misty tree build".into())
                .spawn(move || finish(&owned, build_path(&owned)));
            if let Err(e) = spawned {
                finish(path, Err(e.to_string()));
            }
        }
    }
    let slots = slots().lock().unwrap_or_else(|e| e.into_inner());
    slots.get(path).and_then(|s| s.ready.clone())
}

/// Why the asset at `path` could not be built (None = fine or not tried).
pub fn error(path: &str) -> Option<String> {
    let slots = slots().lock().unwrap_or_else(|e| e.into_inner());
    slots.get(path).and_then(|s| s.error.clone())
}

/// Forget the model of `path` so the next [`get`] rebuilds it (after an edit in memory was
/// saved, without waiting for the file poll).
pub fn invalidate(path: &str) {
    let mut slots = slots().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = slots.get_mut(path) {
        slot.checked = None;
        slot.stamps.clear();
    }
}

/// Is a model of `path` being built right now?
pub fn building(path: &str) -> bool {
    let slots = slots().lock().unwrap_or_else(|e| e.into_inner());
    slots.get(path).is_some_and(|s| s.building)
}

/// Every tree asset path known to the store (Memory module, tests).
pub fn paths() -> Vec<String> {
    let slots = slots().lock().unwrap_or_else(|e| e.into_inner());
    slots.keys().cloned().collect()
}
