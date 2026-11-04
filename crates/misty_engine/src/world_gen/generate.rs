//! The generation pipeline: height → erosion → stamps → water → textures (splat) → scatter.
//! Pure data in, data out (runs on a worker thread); the same request gives the same result.

use std::time::Instant;

use glam::{Vec2, Vec3};

use super::biome::Biome;
use super::classify::{self, Regions};
use super::paint::{BiomeRules, SplatPainter};
use super::scatter::{self, ScatterBatch};
use super::stamps::{self, HeightGrid, PlacedStamp, StampKind};
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
    /// Low-resolution live preview (no scatter).
    pub preview: bool,
}

/// What a generation produced.
pub struct Output {
    pub data: TerrainData,
    pub water: Vec<WaterBody>,
    /// `None` when scatter was not rebuilt.
    pub scatter: Option<Vec<ScatterBatch>>,
    /// Stage name and milliseconds.
    pub timings: Vec<(&'static str, f32)>,
}

/// Preview heightmap resolution.
pub const PREVIEW_RESOLUTION: usize = 129;

/// Run the pipeline. `None` when cancelled.
pub fn run(req: &Request, progress: &Progress) -> Option<Output> {
    let s = &req.settings;
    let mut timings = Vec::new();
    let mut clock = Instant::now();
    let mut lap = |name: &'static str, timings: &mut Vec<(&'static str, f32)>| {
        timings.push((name, clock.elapsed().as_secs_f32() * 1000.0));
        clock = Instant::now();
    };
    let (res, ares) = if req.preview {
        (
            s.heightmap_resolution.min(PREVIEW_RESOLUTION),
            s.alphamap_resolution.min(128),
        )
    } else {
        (s.heightmap_resolution, s.alphamap_resolution)
    };
    let size = Vec3::from(s.size);
    let mut data = TerrainData::new(res, size, ares);
    let res = data.heightmap_resolution;
    let cell = data.spacing();
    let mut rivers = Vec::new();

    // Heights (metres while shaping).
    let mut h: Vec<f32> = if req.stages.heights() || req.current.is_none() {
        progress.set("Base heightmap", 0.0);
        let mut h: Vec<f32> = base::generate(&s.base, res, size)
            .into_iter()
            .map(|v| v * size.y)
            .collect();
        lap("Base", &mut timings);
        let e = &s.erosion;
        if e.hydraulic {
            progress.set("Hydraulic erosion", 0.0);
            if !erosion::hydraulic(&mut h, res, cell.x, e, s.base.seed, progress) {
                return None;
            }
            lap("Hydraulic erosion", &mut timings);
        }
        if e.thermal {
            progress.set("Thermal erosion", 0.0);
            if !erosion::thermal(&mut h, res, cell.x, e, progress) {
                return None;
            }
            lap("Thermal erosion", &mut timings);
        }
        progress.set("Stamps", 0.0);
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
        h
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

    progress.set("Water", 0.0);
    let (water, wet) = water::bodies(&h, res, cell, &req.stamps, &rivers, ares);
    lap("Water", &mut timings);

    // Textures: repaint, or keep the current splat map.
    let regions = Regions {
        zones: req
            .stamps
            .iter()
            .zip(&req.zone_biome)
            .filter(|(st, _)| st.stamp.enabled && matches!(st.stamp.kind, StampKind::BiomeZone(_)))
            .map(|(st, b)| (*b, st))
            .collect(),
        rocks: req
            .stamps
            .iter()
            .filter(|st| st.stamp.enabled && matches!(st.stamp.kind, StampKind::RockArea(_)))
            .collect(),
        breakup: s.biome_breakup,
        seed: s.base.seed,
    };
    let need_fields = req.stages.textures() || req.stages.scatter() || req.current.is_none();
    let fields = need_fields.then(|| {
        progress.set("Classify", 0.0);
        classify::fields(&data, &wet, &regions, req.biomes.len())
    });
    if req.stages.textures() || req.current.is_none() {
        progress.set("Textures", 0.0);
        let painted = BiomeRules.paint(fields.as_ref().expect("fields"), &req.biomes);
        data.layers = painted.layers;
        data.alphamap = painted.alphamap;
        lap("Textures", &mut timings);
    } else if let Some(cur) = &req.current {
        let mut keep = cur.clone();
        keep.set_alphamap_resolution(ares);
        data.layers = keep.layers;
        data.alphamap = keep.alphamap;
    }
    if progress.cancelled() {
        return None;
    }

    let scatter = (req.stages.scatter() && !req.preview).then(|| {
        progress.set("Scatter", 0.0);
        scatter::scatter(
            &data,
            fields.as_ref().expect("fields"),
            &req.biomes,
            s.base.seed ^ 0x5ca7_7e12,
        )
    });
    if scatter.is_some() {
        lap("Scatter", &mut timings);
    }
    progress.set("Done", 1.0);
    Some(Output {
        data,
        water,
        scatter,
        timings,
    })
}

/// A river's water line without carving (heights already contain the river).
fn river_water_only(grid: &HeightGrid, st: &PlacedStamp, r: &stamps::River) -> stamps::RiverPath {
    let line = stamps::river_line(st, r);
    // The carved bed is the lowest ground along the line; water sits above it.
    let mut water: Vec<f32> = line
        .iter()
        .map(|p| grid.sample(*p) + r.depth * 0.7)
        .collect();
    for i in 1..water.len() {
        water[i] = water[i].min(water[i - 1]);
    }
    stamps::RiverPath {
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
