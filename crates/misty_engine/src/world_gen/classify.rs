//! Per-texel analysis of a generated terrain: slope, height, curvature, distance to water, which
//! biome applies (zones blended with a noisy breakup) and how much a Rock Area claims it. Feature
//! classification turns these into Flat / Slope / Cliff / Shore / Peak weights for a biome.

use glam::Vec2;

use super::biome::{Biome, Feature};
use super::noise::{Fractal, fractal, smoothstep};
use super::par_rows;
use super::stamps::PlacedStamp;
use crate::terrain::TerrainData;

/// What the painter and scatter know about one splat texel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TexelInfo {
    /// Height in metres above the terrain's corner.
    pub height: f32,
    /// Height as a fraction of the terrain's maximum height.
    pub height01: f32,
    /// Steepness in degrees.
    pub slope: f32,
    /// Laplacian of the height (1/m): positive in hollows, negative on ridges.
    pub curvature: f32,
    /// Distance to the nearest water in metres (0 in water, infinite without water).
    pub water_dist: f32,
    /// Height above the nearest water's surface (negative underwater, infinite without water).
    pub above_water: f32,
}

/// Water surfaces sampled onto the splat grid: the level per texel where the texel is wet.
pub struct WaterMap {
    pub res: usize,
    /// Water surface height (metres, terrain-local) or `None` where dry.
    pub level: Vec<Option<f32>>,
}

impl WaterMap {
    pub fn dry(res: usize) -> Self {
        Self {
            res,
            level: vec![None; res * res],
        }
    }
}

/// Everything the painter and scatter read, on the splat grid.
pub struct Fields {
    pub res: usize,
    /// Terrain size in metres (x, max height, z).
    pub size: glam::Vec3,
    pub info: Vec<TexelInfo>,
    pub biome_count: usize,
    /// `res² × biome_count` weights (each texel sums to 1).
    pub biomes: Vec<f32>,
    /// Rock Area claim per texel (0..1).
    pub rock: Vec<f32>,
}

impl Fields {
    /// Texel under a terrain-local point.
    pub fn index(&self, p: Vec2) -> usize {
        let n = self.res;
        let tx = ((p.x / self.size.x) * n as f32).clamp(0.0, n as f32 - 1.0) as usize;
        let tz = ((p.y / self.size.z) * n as f32).clamp(0.0, n as f32 - 1.0) as usize;
        tz * n + tx
    }

    pub fn biome_weight(&self, texel: usize, biome: usize) -> f32 {
        self.biomes[texel * self.biome_count + biome]
    }

    /// Terrain-local centre of a texel.
    pub fn texel_point(&self, tx: usize, tz: usize) -> Vec2 {
        let n = self.res as f32;
        Vec2::new(
            (tx as f32 + 0.5) / n * self.size.x,
            (tz as f32 + 0.5) / n * self.size.z,
        )
    }
}

/// Zones and areas that mark regions (biome index per Biome Zone).
pub struct Regions<'a> {
    pub zones: Vec<(usize, &'a PlacedStamp)>,
    pub rocks: Vec<&'a PlacedStamp>,
    /// Noise breakup of zone edges in metres.
    pub breakup: f32,
    pub seed: u32,
}

/// Analyse `d` on its splat grid.
pub fn fields(d: &TerrainData, water: &WaterMap, regions: &Regions, biome_count: usize) -> Fields {
    let n = d.alphamap_resolution;
    let size = d.size;
    let mut f = Fields {
        res: n,
        size,
        info: vec![TexelInfo::default(); n * n],
        biome_count: biome_count.max(1),
        biomes: vec![0.0; n * n * biome_count.max(1)],
        rock: vec![0.0; n * n],
    };
    let (dist, nearest) = distance_to_water(water, size.x / n as f32);
    let step = d.spacing();
    par_rows(&mut f.info, n, |tz, row| {
        for (tx, out) in row.iter_mut().enumerate() {
            let x = (tx as f32 + 0.5) / n as f32 * size.x;
            let z = (tz as f32 + 0.5) / n as f32 * size.z;
            let h = |dx: f32, dz: f32| d.sample_height(x + dx, z + dz);
            let c = h(0.0, 0.0);
            let (l, r, b, t) = (
                h(-step.x, 0.0),
                h(step.x, 0.0),
                h(0.0, -step.y),
                h(0.0, step.y),
            );
            let gx = (r - l) / (2.0 * step.x);
            let gz = (t - b) / (2.0 * step.y);
            let i = tz * n + tx;
            *out = TexelInfo {
                height: c,
                height01: c / size.y.max(1e-3),
                slope: (gx * gx + gz * gz).sqrt().atan().to_degrees(),
                curvature: (l + r - 2.0 * c) / (step.x * step.x)
                    + (b + t - 2.0 * c) / (step.y * step.y),
                water_dist: dist[i],
                above_water: match nearest[i] {
                    Some(level) => c - level,
                    None => f32::INFINITY,
                },
            };
        }
    });
    let bc = f.biome_count;
    let warp = Fractal {
        octaves: 3,
        ..Default::default()
    };
    let points: Vec<Vec2> = (0..n * n).map(|i| f.texel_point(i % n, i / n)).collect();
    // Breakup: zone edges are evaluated at a point pushed around by noise.
    let jitter = |p: Vec2| {
        if regions.breakup <= 0.0 {
            return p;
        }
        let q = p / 40.0;
        p + Vec2::new(
            fractal(q.x, q.y, regions.seed ^ 0xb10e, &warp),
            fractal(q.x + 7.3, q.y - 2.1, regions.seed ^ 0xb10f, &warp),
        ) * regions.breakup
    };
    par_rows(&mut f.biomes, n * bc, |tz, row| {
        for tx in 0..n {
            let w = &mut row[tx * bc..(tx + 1) * bc];
            w[0] = 1.0;
            let p = jitter(points[tz * n + tx]);
            // Later zones paint over earlier ones.
            for (b, zone) in &regions.zones {
                let m = zone.mask(p);
                if m > 0.0 {
                    w.iter_mut().for_each(|v| *v *= 1.0 - m);
                    w[(*b).min(bc - 1)] += m;
                }
            }
        }
    });
    par_rows(&mut f.rock, n, |tz, row| {
        for (tx, out) in row.iter_mut().enumerate() {
            let p = jitter(points[tz * n + tx]);
            *out = regions
                .rocks
                .iter()
                .map(|s| match &s.stamp.kind {
                    super::stamps::StampKind::RockArea(r) => s.mask(p) * r.strength.clamp(0.0, 1.0),
                    _ => 0.0,
                })
                .fold(0.0, f32::max);
        }
    });
    f
}

/// Chamfer distance (metres) from every texel to the nearest wet texel, and that water's level.
fn distance_to_water(w: &WaterMap, texel: f32) -> (Vec<f32>, Vec<Option<f32>>) {
    let n = w.res;
    let mut dist: Vec<f32> = w
        .level
        .iter()
        .map(|l| if l.is_some() { 0.0 } else { f32::INFINITY })
        .collect();
    let mut near = w.level.clone();
    let (a, b) = (texel, texel * std::f32::consts::SQRT_2);
    let relax =
        |i: usize, j: usize, cost: f32, dist: &mut Vec<f32>, near: &mut Vec<Option<f32>>| {
            if dist[j] + cost < dist[i] {
                dist[i] = dist[j] + cost;
                near[i] = near[j];
            }
        };
    for z in 0..n {
        for x in 0..n {
            let i = z * n + x;
            if x > 0 {
                relax(i, i - 1, a, &mut dist, &mut near);
            }
            if z > 0 {
                relax(i, i - n, a, &mut dist, &mut near);
                if x > 0 {
                    relax(i, i - n - 1, b, &mut dist, &mut near);
                }
                if x + 1 < n {
                    relax(i, i - n + 1, b, &mut dist, &mut near);
                }
            }
        }
    }
    for z in (0..n).rev() {
        for x in (0..n).rev() {
            let i = z * n + x;
            if x + 1 < n {
                relax(i, i + 1, a, &mut dist, &mut near);
            }
            if z + 1 < n {
                relax(i, i + n, a, &mut dist, &mut near);
                if x + 1 < n {
                    relax(i, i + n + 1, b, &mut dist, &mut near);
                }
                if x > 0 {
                    relax(i, i + n - 1, b, &mut dist, &mut near);
                }
            }
        }
    }
    (dist, near)
}

/// Feature weights (Flat, Slope, Cliff, Shore, Peak; summing to 1) of a texel in a biome.
/// `rock` (Rock Area claim) pushes the texel towards Cliff.
pub fn classify(t: &TexelInfo, b: &Biome, rock: f32) -> [f32; 5] {
    let mut w = [0.0f32; 5];
    let idx = |f: Feature| Feature::ALL.iter().position(|x| *x == f).expect("feature");
    if t.above_water < 0.0 {
        w[idx(Feature::Shore)] = 1.0;
        return w;
    }
    // Ridges (negative curvature) expose rock a little earlier.
    let exposed = smoothstep(0.0, 0.08, -t.curvature) * 4.0;
    let cliff = smoothstep(b.cliff_angle - 4.0 - exposed, b.cliff_angle + 4.0, t.slope)
        .max(rock * smoothstep(b.slope_angle * 0.3, b.slope_angle, t.slope).max(0.6 * rock));
    let sw = b.shore_width.max(0.1);
    let shore = (1.0 - smoothstep(0.5 * sw, sw, t.above_water))
        * (1.0 - smoothstep(sw * 3.0, sw * 6.0, t.water_dist));
    let peak = smoothstep(b.peak_height - 0.04, b.peak_height + 0.04, t.height01);
    let slope = smoothstep(b.slope_angle - 4.0, b.slope_angle + 4.0, t.slope);
    let mut rest = 1.0;
    for (f, v) in [
        (Feature::Cliff, cliff),
        (Feature::Shore, shore),
        (Feature::Peak, peak),
        (Feature::Slope, slope),
    ] {
        let take = v.clamp(0.0, 1.0) * rest;
        w[idx(f)] = take;
        rest -= take;
    }
    w[idx(Feature::Flat)] = rest.max(0.0);
    w
}
