//! Runtime textures: images that change every frame (video frames), published under a name
//! that materials, Raw Images and the UI renderer use like a texture path.
//!
//! Names start with `video://` (one per Video Player, made up when it prepares) or
//! `runtime://` (a Video Player's Render Texture target, named in the Inspector). The GPU
//! caches keep one texture per name and re-upload it when [`version`] changes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use image::RgbaImage;

/// A published frame: sRGB RGBA pixels.
#[derive(Clone, Debug)]
pub struct RuntimeFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<Vec<u8>>,
    /// Bumped by every publish.
    pub version: u64,
}

static FRAMES: Mutex<Option<HashMap<String, RuntimeFrame>>> = Mutex::new(None);
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn with<R>(f: impl FnOnce(&mut HashMap<String, RuntimeFrame>) -> R) -> R {
    let mut g = FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

/// Pixels of the published runtime textures: (name, bytes) (Memory Profiler).
pub fn memory() -> Vec<(String, u64)> {
    with(|frames| {
        let mut out: Vec<(String, u64)> = frames
            .iter()
            .map(|(k, f)| (k.clone(), f.pixels.len() as u64))
            .collect();
        out.sort();
        out
    })
}

/// Is `name` a runtime texture (rather than a project file)?
pub fn is_runtime(name: &str) -> bool {
    name.starts_with("video://") || name.starts_with("runtime://")
}

/// A fresh `video://` name.
pub fn new_name() -> String {
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("video://{id}")
}

/// The texture name a Render Texture target string stands for (`runtime://` added when
/// missing).
pub fn target_name(target: &str) -> Option<String> {
    let t = target.trim();
    if t.is_empty() {
        None
    } else if is_runtime(t) {
        Some(t.to_owned())
    } else {
        Some(format!("runtime://{t}"))
    }
}

/// Publish a new image under `name`.
pub fn publish(name: &str, image: &RgbaImage) {
    let pixels = Arc::new(image.as_raw().clone());
    with(|m| {
        let version = m.get(name).map_or(1, |f| f.version + 1);
        m.insert(
            name.to_owned(),
            RuntimeFrame {
                width: image.width(),
                height: image.height(),
                pixels,
                version,
            },
        );
    });
}

/// The latest frame under `name`.
pub fn get(name: &str) -> Option<RuntimeFrame> {
    with(|m| m.get(name).cloned())
}

/// Version of the latest frame (0 = nothing published).
pub fn version(name: &str) -> u64 {
    with(|m| m.get(name).map_or(0, |f| f.version))
}

/// Forget a name (its player stopped for good).
pub fn remove(name: &str) {
    with(|m| {
        m.remove(name);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_bumps_versions_and_names_are_unique() {
        let a = new_name();
        assert_ne!(a, new_name());
        assert!(is_runtime(&a));
        assert_eq!(version(&a), 0);
        publish(&a, &RgbaImage::new(2, 2));
        publish(&a, &RgbaImage::new(4, 2));
        let f = get(&a).unwrap();
        assert_eq!((f.width, f.height, f.version), (4, 2, 2));
        remove(&a);
        assert!(get(&a).is_none());
        assert_eq!(target_name(" Screen "), Some("runtime://Screen".into()));
        assert_eq!(target_name("video://3"), Some("video://3".into()));
        assert_eq!(target_name(""), None);
    }
}
