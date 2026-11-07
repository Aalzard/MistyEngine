//! The driver's compiled pipelines kept on disk between launches (wgpu's pipeline cache:
//! Vulkan only; elsewhere nothing happens).
//!
//! Drivers keep their own shader caches, but NVIDIA's is per executable: the first launch
//! of every new build of the editor compiled every pipeline from scratch (MeadowDemo's first
//! frame: 2.5 s of the 3.5 s load frame). The editor opens a cache file in the project's
//! `Library/ShaderCache` (named by `wgpu::util::pipeline_cache_key`: one per GPU and driver)
//! and every pipeline is created through it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

struct Installed {
    device: wgpu::Device,
    cache: wgpu::PipelineCache,
    file: PathBuf,
    /// `pipelines_built()` when the file was last written (or read).
    saved: u64,
}

static CACHE: Mutex<Option<Installed>> = Mutex::new(None);

fn lock() -> std::sync::MutexGuard<'static, Option<Installed>> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Use the cache file in `dir` for pipelines made on `device` from now on. Nothing when the
/// device was made without `Features::PIPELINE_CACHE` (see [`pipeline_cache_features`]).
pub fn open_pipeline_cache(device: &wgpu::Device, adapter: &wgpu::AdapterInfo, dir: &Path) {
    if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
        return;
    }
    let Some(key) = wgpu::util::pipeline_cache_key(adapter) else {
        return;
    };
    let file = dir.join(key);
    let data = std::fs::read(&file).ok();
    // SAFETY: the file is only ever written by `save_pipeline_cache` with what
    // `PipelineCache::get_data` returned (atomically, through a renamed temporary file);
    // wgpu checks its header (device, driver, size) and starts empty when it does not match.
    let cache = unsafe {
        device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
            label: Some("misty pipeline cache"),
            data: data.as_deref(),
            fallback: true,
        })
    };
    *lock() = Some(Installed {
        device: device.clone(),
        cache,
        file,
        saved: super::pipelines_built(),
    });
}

/// The features to ask for at device creation so the cache can be used (none elsewhere).
pub fn pipeline_cache_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    adapter.features() & wgpu::Features::PIPELINE_CACHE
}

/// The cache for pipelines made on `device`, if one is open for it.
pub(crate) fn pipeline_cache(device: &wgpu::Device) -> Option<wgpu::PipelineCache> {
    let guard = lock();
    guard
        .as_ref()
        .filter(|c| c.device == *device)
        .map(|c| c.cache.clone())
}

/// Write the cache file if pipelines were built since it was last written (the editor calls
/// this a few seconds after shaders compiled, and on exit).
pub fn save_pipeline_cache() -> std::io::Result<()> {
    let (data, file) = {
        let mut guard = lock();
        let Some(c) = guard.as_mut() else {
            return Ok(());
        };
        let built = super::pipelines_built();
        if built == c.saved {
            return Ok(());
        }
        c.saved = built;
        let Some(data) = c.cache.get_data() else {
            return Ok(());
        };
        (data, c.file.clone())
    };
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, &file)
}
