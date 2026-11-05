//! The lightmap baker: a multi-threaded CPU path tracer.
//!
//! Why the CPU: a bake runs on a background thread while the editor keeps rendering, so it
//! never competes with the editor's wgpu queue or risks a GPU timeout on long dispatches; it
//! reuses the probe placement BVH; and it is deterministic and testable without a GPU. Scenes
//! this engine targets bake in seconds at the default 10 texels per unit (rayon spreads texels
//! over every core).
//!
//! Per texel (position and normal from the rasterised uv2 triangles, [`raster`]), every sample:
//! - direct light of Baked lights (shadow rays; point / spot sources jittered over their shadow
//!   radius, rectangle lights sampled over their area), with the realtime falloff curves so a
//!   baked light matches the same light in realtime;
//! - one cosine-weighted path: the sky on a miss, else the surface's emission plus its albedo ×
//!   (direct light of Baked and Mixed lights + the next bounce), `bounces` deep.
//!
//! Then the indirect part is denoised ([`filter::denoise`], an edge-aware À-trous blur guided
//! by position and normal), direct and indirect are added and empty texels around charts are
//! filled ([`filter::dilate`]) so bilinear sampling never reads black at seams.

pub mod filter;
pub mod input;
pub mod raster;
pub mod trace;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use glam::Vec3;
use rayon::prelude::*;

pub use input::{BakeInput, BakeLight, BakeMaterial, LightShape, Receiver, ReceiverTriangle};

use super::LightmapData;
use super::atlas;

/// Shared progress of a running bake.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    pub cancel: AtomicBool,
    /// 0 = placing, 1 = tracing, 2 = filtering.
    pub stage: AtomicUsize,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed).max(1);
        (self.done.load(Ordering::Relaxed) as f32 / total as f32).min(1.0)
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Where one receiver landed: its lightmap and uv2 scale-offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReceiverResult {
    /// [`Receiver::id`] (the caller's renderer key).
    pub id: u64,
    pub lightmap: usize,
    pub scale_offset: [f32; 4],
}

pub struct BakeOutput {
    pub size: u32,
    pub maps: Vec<LightmapData>,
    pub receivers: Vec<ReceiverResult>,
    /// Texels traced.
    pub texels: usize,
}

/// Where every receiver goes (no tracing): lightmap count and size, placements.
pub fn layout(input: &BakeInput) -> atlas::Atlas {
    let s = input.settings.sanitized();
    let sides: Vec<u32> = input
        .receivers
        .iter()
        .map(|r| {
            atlas::renderer_side(
                r.world_area(),
                r.uv_area(),
                s.resolution * r.scale.max(0.0),
                s.max_size,
            )
        })
        .collect();
    atlas::pack(&sides, s.max_size, s.padding)
}

/// Each receiver's lightmap and scale-offset in `atlas`.
pub fn receiver_results(input: &BakeInput, atlas: &atlas::Atlas) -> Vec<ReceiverResult> {
    input
        .receivers
        .iter()
        .zip(&atlas.placements)
        .map(|(r, p)| ReceiverResult {
            id: r.id,
            lightmap: p.lightmap,
            scale_offset: p.scale_offset(atlas.size),
        })
        .collect()
}

/// Bake `input`; `None` when cancelled.
pub fn bake(input: &BakeInput, progress: &Progress) -> Option<BakeOutput> {
    let s = input.settings.sanitized();
    let atlas = layout(input);
    let size = atlas.size;
    let mut maps: Vec<raster::TexelMap> = (0..atlas.count)
        .map(|_| raster::TexelMap::new(size))
        .collect();
    for (r, p) in input.receivers.iter().zip(&atlas.placements) {
        raster::rasterize(&mut maps[p.lightmap], r, p);
    }
    let texels: Vec<(usize, usize)> = maps
        .iter()
        .enumerate()
        .flat_map(|(m, map)| map.valid_indices().map(move |i| (m, i)))
        .collect();
    progress.total.store(texels.len().max(1), Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    progress.stage.store(1, Ordering::Relaxed);

    let scene = trace::TraceScene::new(input);
    let samples = s.samples.max(1);
    // (direct, indirect) per traced texel.
    let lit: Vec<Option<(Vec3, Vec3)>> = texels
        .par_iter()
        .map(|&(m, i)| {
            if progress.cancelled() {
                return None;
            }
            let t = &maps[m].texels[i];
            let mut rng = trace::Rng::new((m as u64) << 32 | i as u64);
            let r = scene.texel(t.position, t.normal, samples, &mut rng);
            progress.done.fetch_add(1, Ordering::Relaxed);
            Some(r)
        })
        .collect();
    if progress.cancelled() {
        return None;
    }
    progress.stage.store(2, Ordering::Relaxed);
    for (&(m, i), r) in texels.iter().zip(lit) {
        let (direct, indirect) = r?;
        maps[m].texels[i].direct = direct;
        maps[m].texels[i].indirect = indirect;
    }
    let k = s.indirect_intensity;
    let out: Vec<LightmapData> = maps
        .iter()
        .map(|map| {
            let indirect = filter::denoise(map, 3);
            let mut data = LightmapData::new(size, size);
            let mut valid = vec![false; (size * size) as usize];
            for (i, t) in map.texels.iter().enumerate() {
                if t.valid {
                    data.texels[i] = t.direct + indirect[i] * k;
                    valid[i] = true;
                }
            }
            filter::dilate(&mut data, &mut valid, 4 + s.padding);
            data
        })
        .collect();
    let receivers = receiver_results(input, &atlas);
    Some(BakeOutput {
        size,
        maps: out,
        receivers,
        texels: texels.len(),
    })
}

/// [`bake`] on its own thread: the handle yields the output (`None` = cancelled).
pub fn spawn(
    input: BakeInput,
    progress: Arc<Progress>,
) -> std::thread::JoinHandle<Option<BakeOutput>> {
    std::thread::Builder::new()
        .name("misty lightmapper".into())
        .spawn(move || bake(&input, &progress))
        .expect("spawning the lightmapper thread")
}
