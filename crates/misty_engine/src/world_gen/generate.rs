//! The generation pipeline: height → erosion → stamps → water → classify → textures (splat) →
//! scatter. Pure data in, data out (runs on a worker thread); the same request gives the same
//! result.
//!
//! Incremental regeneration: [`run_cached`] keeps every stage's result in a [`Cache`] keyed by a
//! hash of the stage's inputs (its settings plus the key of the stage before it). A stage whose
//! key matches the cached one is reused instead of recomputed, so moving a Biome Zone only
//! re-runs classify / textures / scatter, and dragging a stamp skips base noise and erosion. Each
//! stage is a pure function of its inputs, so a cached result is identical to a fresh one.

use std::collections::hash_map::DefaultHasher;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::time::Instant;

use glam::{Vec2, Vec3};

use super::biome::Biome;
use super::classify::{self, Fields, Regions, WaterMap};
use super::paint::{BiomeRules, Painted, SplatPainter};
use super::scatter::{self, ScatterBatch};
use super::stamps::{self, HeightGrid, PlacedStamp, RiverPath, StampKind};
use super::water::{self, WaterBody};
use super::{Progress, WorldBuilder, base, erosion};
use crate::terrain::TerrainData;

/// Which parts to rebuild.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stages {
    /// Everything.
    All,
    /// Heightmap and water (keeps the splat map).
    Heights,
    /// Splat map from the current heights.
    Textures,
    /// Scatter from the current heights.
    Scatter,
}

impl Stages {
    fn heights(self) -> bool {
        matches!(self, Self::All | Self::Heights)
    }
    fn textures(self) -> bool {
        matches!(self, Self::All | Self::Textures)
    }
    fn scatter(self) -> bool {
        matches!(self, Self::All | Self::Scatter)
    }
}

/// Everything a generation reads.
#[derive(Clone)]
pub struct Request {
    pub settings: WorldBuilder,
    /// Stamps in hierarchy order.
    pub stamps: Vec<PlacedStamp>,
    /// Index 0 = the default biome; Biome Zones refer to the others.
    pub biomes: Vec<Biome>,
    /// Biome index per stamp (Biome Zones; 0 for other stamps).
    pub zone_biome: Vec<usize>,
    pub stages: Stages,
    /// Current terrain data (kept parts come from it).
    pub current: Option<TerrainData>,
    /// Low-resolution preview (no scatter, no water objects).
    pub preview: bool,
    /// Heightmap resolution of a preview (capped by the settings' resolution).
    pub preview_resolution: usize,
}

/// What a generation produced.
pub struct Output {
    pub data: TerrainData,
    pub water: Vec<WaterBody>,
    /// Water level per splat texel (preview images draw it).
    pub wet: WaterMap,
    /// `None` when scatter was not rebuilt.
    pub scatter: Option<Vec<ScatterBatch>>,
    /// Stage name and milliseconds (stages that ran).
    pub timings: Vec<(&'static str, f32)>,
    /// Stages taken from the cache instead of recomputed.
    pub reused: Vec<&'static str>,
}

/// Default preview heightmap resolution.
pub const PREVIEW_RESOLUTION: usize = 129;

/// Stage names, in pipeline order (progress labels).
pub const STAGES: [&str; 8] = [
    "Base heightmap",
    "Hydraulic erosion",
    "Thermal erosion",
    "Stamps",
    "Water",
    "Classify",
    "Textures",
    "Scatter",
];

/// Heights after the stamps, and the rivers' water lines.
type Stamped = (Vec<f32>, Vec<RiverPath>);

/// Results of earlier runs (see the module docs). Keys include the resolution, so use one
/// cache per resolution (a cache shared between sizes just misses).
#[derive(Default)]
pub struct Cache {
    base: Option<(u64, Vec<f32>)>,
    hydraulic: Option<(u64, Vec<f32>)>,
    thermal: Option<(u64, Vec<f32>)>,
    stamped: Option<(u64, Stamped)>,
    water: Option<(u64, (Vec<WaterBody>, WaterMap))>,
    fields: Option<(u64, Fields)>,
    painted: Option<(u64, Painted)>,
    scatter: Option<(u64, Vec<ScatterBatch>)>,
}

impl Cache {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Stable-within-the-process hash of a stage's inputs (their `Debug` text: floats print
/// exactly, so different values give different keys).
fn key(parts: &[&dyn Debug]) -> u64 {
    let mut h = DefaultHasher::new();
    for p in parts {
        format!("{p:?}").hash(&mut h);
    }
    h.finish()
}

/// The cached value when its key matches.
fn hit<T>(slot: &Option<(u64, T)>, k: u64) -> Option<&T> {
    slot.as_ref().filter(|(kk, _)| *kk == k).map(|(_, v)| v)
}

/// Relative cost of each stage (about ms per million samples at the defaults in release
/// builds), for the overall progress and its ETA.
fn stage_cost(name: &str, s: &WorldBuilder, res: usize, ares: usize) -> f32 {
    let m = (res * res) as f32 / 1e6;
    let a = (ares * ares) as f32 / 1e6;
    let e = &s.erosion;
    match name {
        "Base heightmap" => 70.0 * m * (s.base.octaves.max(1) as f32 / 7.0),
        "Hydraulic erosion" => 80.0 * m * e.hydraulic_iterations as f32,
        "Thermal erosion" => 4.0 * m * e.thermal_iterations as f32,
        "Stamps" => 20.0 * m,
        "Water" => 25.0 * a,
        "Classify" => 50.0 * a,
        "Textures" => 18.0 * a,
        _ => 10.0 * a,
    }
}

/// Run the pipeline without a cache. `None` when cancelled.
pub fn run(req: &Request, progress: &Progress) -> Option<Output> {
    run_cached(req, progress, &mut Cache::default())
}

/// Run the pipeline, reusing (and refreshing) `cache`. `None` when cancelled; the cache then
/// keeps the stages that finished.
pub fn run_cached(req: &Request, progress: &Progress, cache: &mut Cache) -> Option<Output> {
    let s = &req.settings;
    let mut timings = Vec::new();
    let mut reused = Vec::new();
    let mut clock = Instant::now();
    let mut lap = |name: &'static str, timings: &mut Vec<(&'static str, f32)>| {
        timings.push((name, clock.elapsed().as_secs_f32() * 1000.0));
        clock = Instant::now();
    };
    let (res, ares) = if req.preview {
        let r = s.heightmap_resolution.min(req.preview_resolution.max(33));
        (r, s.alphamap_resolution.min(r - 1))
    } else {
        (s.heightmap_resolution, s.alphamap_resolution)
    };
    let size = Vec3::from(s.size);
    let mut data = TerrainData::new(res, size, ares);
    let (res, ares) = (data.heightmap_resolution, data.alphamap_resolution);
    let cell = data.spacing();
    let compute_heights = req.stages.heights() || req.current.is_none();
    let paint = req.stages.textures() || req.current.is_none();
    let scatter_wanted = req.stages.scatter() && !req.preview;
    let e = &s.erosion;

    // Stage keys: each includes the previous stage's key, so a change flows downstream.
    let k_base = key(&[&res, &s.size, &s.base]);
    let k_hyd = if e.hydraulic {
        key(&[
            &k_base,
            &e.hydraulic_iterations,
            &e.hydraulic_strength,
            &s.base.seed,
        ])
    } else {
        k_base
    };
    let k_thermal = if e.thermal {
        key(&[
            &k_hyd,
            &e.thermal_iterations,
            &e.talus_angle,
            &e.thermal_strength,
        ])
    } else {
        k_hyd
    };
    let shaping: Vec<&PlacedStamp> = req
        .stamps
        .iter()
        .filter(|st| st.stamp.kind.shapes_height())
        .collect();
    let k_stamps = key(&[&k_thermal, &shaping]);
    let k_heights = if compute_heights {
        k_stamps
    } else {
        // Heights from the current terrain: hash them (bits, not text: fast for big maps).
        let cur = req.current.as_ref().expect("checked");
        let mut hasher = DefaultHasher::new();
        cur.heightmap_resolution.hash(&mut hasher);
        for v in &cur.heights {
            v.to_bits().hash(&mut hasher);
        }
        key(&[&hasher.finish(), &res, &cur.size, &shaping])
    };
    let watery: Vec<&PlacedStamp> = req
        .stamps
        .iter()
        .filter(|st| matches!(st.stamp.kind, StampKind::Water(_) | StampKind::River(_)))
        .collect();
    let k_water = key(&[&k_heights, &ares, &watery]);
    let regional: Vec<(&usize, &PlacedStamp)> = req
        .zone_biome
        .iter()
        .zip(&req.stamps)
        .filter(|(_, st)| {
            matches!(
                st.stamp.kind,
                StampKind::BiomeZone(_) | StampKind::RockArea(_)
            )
        })
        .collect();
    let k_fields = key(&[
        &k_water,
        &regional,
        &s.biome_breakup,
        &s.base.seed,
        &req.biomes.len(),
    ]);
    let k_paint = key(&[&k_fields, &req.biomes]);
    let k_scatter = key(&[&k_fields, &req.biomes, &s.base.seed]);

    // Overall progress: the stages that will run, weighted by their expected cost.
    let mut plan: Vec<(&'static str, f32)> = Vec::new();
    let mut plan_stage = |name: &'static str, cached: bool| {
        if !cached {
            plan.push((name, stage_cost(name, s, res, ares)));
        }
    };
    if compute_heights {
        let stamped = hit(&cache.stamped, k_stamps).is_some();
        let thermal = stamped || hit(&cache.thermal, k_thermal).is_some();
        let hyd = thermal || hit(&cache.hydraulic, k_hyd).is_some();
        plan_stage(STAGES[0], hyd || hit(&cache.base, k_base).is_some());
        if e.hydraulic {
            plan_stage(STAGES[1], hyd);
        }
        if e.thermal {
            plan_stage(STAGES[2], thermal);
        }
        plan_stage(STAGES[3], stamped);
    }
    plan_stage(STAGES[4], hit(&cache.water, k_water).is_some());
    if paint || scatter_wanted {
        plan_stage(STAGES[5], hit(&cache.fields, k_fields).is_some());
    }
    if paint {
        plan_stage(STAGES[6], hit(&cache.painted, k_paint).is_some());
    }
    if scatter_wanted {
        plan_stage(STAGES[7], hit(&cache.scatter, k_scatter).is_some());
    }
    progress.set_plan(plan);

    // Heights (metres while shaping).
    let mut rivers = Vec::new();
    let mut h: Vec<f32> = if compute_heights {
        if let Some((h, r)) = hit(&cache.stamped, k_stamps) {
            reused.extend(&STAGES[..4]);
            rivers = r.clone();
            h.clone()
        } else {
            let mut h = if let Some(h) = hit(&cache.thermal, k_thermal) {
                reused.extend(&STAGES[..3]);
                h.clone()
            } else {
                let mut h = if let Some(h) = hit(&cache.hydraulic, k_hyd) {
                    reused.extend(&STAGES[..2]);
                    h.clone()
                } else {
                    let mut h = if let Some(h) = hit(&cache.base, k_base) {
                        reused.push(STAGES[0]);
                        h.clone()
                    } else {
                        progress.set(STAGES[0], 0.0);
                        let h: Vec<f32> = base::generate(&s.base, res, size)
                            .into_iter()
                            .map(|v| v * size.y)
                            .collect();
                        lap("Base", &mut timings);
                        cache.base = Some((k_base, h.clone()));
                        h
                    };
                    if e.hydraulic {
                        progress.set(STAGES[1], 0.0);
                        if !erosion::hydraulic(&mut h, res, cell.x, e, s.base.seed, progress) {
                            return None;
                        }
                        lap("Hydraulic erosion", &mut timings);
                        cache.hydraulic = Some((k_hyd, h.clone()));
                    }
                    h
                };
                if e.thermal {
                    progress.set(STAGES[2], 0.0);
                    if !erosion::thermal(&mut h, res, cell.x, e, progress) {
                        return None;
                    }
                    lap("Thermal erosion", &mut timings);
                    cache.thermal = Some((k_thermal, h.clone()));
                }
                h
            };
            progress.set(STAGES[3], 0.0);
            let mut grid = HeightGrid {
                h: &mut h,
                res,
                cell,
            };
            for (i, st) in req.stamps.iter().enumerate() {
                if let Some(r) = stamps::apply(&mut grid, st) {
                    rivers.push(r);
                }
                progress.set_fraction((i + 1) as f32 / req.stamps.len().max(1) as f32);
            }
            lap("Stamps", &mut timings);
            cache.stamped = Some((k_stamps, (h.clone(), rivers.clone())));
            h
        }
    } else {
        let mut cur = req.current.clone().expect("checked");
        cur.set_heightmap_resolution(res);
        // Rivers still need their lines for water.
        let mut copy: Vec<f32> = cur.heights.iter().map(|v| v * cur.size.y).collect();
        let grid = HeightGrid {
            h: &mut copy,
            res,
            cell,
        };
        for st in &req.stamps {
            if let StampKind::River(r) = &st.stamp.kind
                && st.stamp.enabled
                && r.water
            {
                rivers.push(river_water_only(&grid, st, r));
            }
        }
        copy
    };
    if progress.cancelled() {
        return None;
    }
    for (dst, v) in data.heights.iter_mut().zip(&h) {
        *dst = (v / size.y).clamp(0.0, 1.0);
    }
    h.iter_mut().for_each(|v| *v = v.clamp(0.0, size.y));

    let (water, wet) = match hit(&cache.water, k_water) {
        Some((water, wet)) => {
            reused.push(STAGES[4]);
            (water.clone(), wet.clone())
        }
        None => {
            progress.set(STAGES[4], 0.0);
            let (water, wet) = water::bodies(&h, res, cell, &req.stamps, &rivers, ares);
            lap("Water", &mut timings);
            cache.water = Some((k_water, (water.clone(), wet.clone())));
            (water, wet)
        }
    };

    // Classify, then repaint or keep the current splat map.
    if paint || scatter_wanted {
        if hit(&cache.fields, k_fields).is_some() {
            reused.push(STAGES[5]);
        } else {
            let regions = Regions {
                zones: req
                    .stamps
                    .iter()
                    .zip(&req.zone_biome)
                    .filter(|(st, _)| {
                        st.stamp.enabled && matches!(st.stamp.kind, StampKind::BiomeZone(_))
                    })
                    .map(|(st, b)| (*b, st))
                    .collect(),
                rocks: req
                    .stamps
                    .iter()
                    .filter(|st| {
                        st.stamp.enabled && matches!(st.stamp.kind, StampKind::RockArea(_))
                    })
                    .collect(),
                breakup: s.biome_breakup,
                seed: s.base.seed,
            };
            progress.set(STAGES[5], 0.0);
            let f = classify::fields(&data, &wet, &regions, req.biomes.len());
            lap("Classify", &mut timings);
            cache.fields = Some((k_fields, f));
        }
    }
    if paint {
        if hit(&cache.painted, k_paint).is_some() {
            reused.push(STAGES[6]);
        } else {
            progress.set(STAGES[6], 0.0);
            let f = hit(&cache.fields, k_fields).expect("classified");
            let painted = BiomeRules.paint(f, &req.biomes);
            lap("Textures", &mut timings);
            cache.painted = Some((k_paint, painted));
        }
        let painted = hit(&cache.painted, k_paint).expect("painted");
        data.layers = painted.layers.clone();
        data.alphamap = painted.alphamap.clone();
        data.alphamap_extra.clear();
    } else if let Some(cur) = &req.current {
        let mut keep = cur.clone();
        keep.set_alphamap_resolution(ares);
        data.layers = keep.layers;
        data.alphamap = keep.alphamap;
        data.alphamap_extra = keep.alphamap_extra;
    }
    if progress.cancelled() {
        return None;
    }

    let scatter = if scatter_wanted {
        if let Some(sc) = hit(&cache.scatter, k_scatter) {
            reused.push(STAGES[7]);
            Some(sc.clone())
        } else {
            progress.set(STAGES[7], 0.0);
            let f = hit(&cache.fields, k_fields).expect("classified");
            let sc = scatter::scatter(&data, f, &req.biomes, s.base.seed ^ 0x5ca7_7e12);
            lap("Scatter", &mut timings);
            cache.scatter = Some((k_scatter, sc.clone()));
            Some(sc)
        }
    } else {
        None
    };
    // Scatter ▸ Terrain Trees: tree items become the terrain's own trees; otherwise the trees
    // already on the terrain (painted by hand, an earlier generation's) stay.
    if let Some(cur) = &req.current {
        data.trees = cur.trees.clone();
    }
    if let Some(sc) = &scatter
        && req.settings.scatter_storage == super::ScatterStorage::TerrainTrees
    {
        super::scatter_to_trees(&mut data, &req.biomes, sc);
    }
    progress.set("Done", 1.0);
    Some(Output {
        data,
        water,
        wet,
        scatter,
        timings,
        reused,
    })
}

/// A river's water line without carving (heights already contain the river).
fn river_water_only(grid: &HeightGrid, st: &PlacedStamp, r: &stamps::River) -> RiverPath {
    let line = stamps::river_line(st, r);
    // The carved bed is the lowest ground along the line; water sits above it.
    let mut water: Vec<f32> = line
        .iter()
        .map(|p| grid.sample(*p) + r.depth * 0.7)
        .collect();
    for i in 1..water.len() {
        water[i] = water[i].min(water[i - 1]);
    }
    RiverPath {
        points: line,
        water,
        width: r.width,
    }
}

/// Bounds helper for tests and tools: terrain-local point of a heightmap sample.
pub fn sample_point(d: &TerrainData, ix: usize, iz: usize) -> Vec2 {
    let s = d.spacing();
    Vec2::new(ix as f32 * s.x, iz as f32 * s.y)
}
