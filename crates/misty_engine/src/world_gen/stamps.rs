//! Stamps and areas: the `World Stamp` component and how each kind shapes the heightmap or marks
//! a region. A stamp's object gives its centre, height, yaw and x/z scale; `radius` and
//! `falloff` give its footprint (falloff = soft fraction of the edge).

use glam::{Mat4, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::noise::{Fractal, fractal, smoothstep};

/// Mountain / hill shape presets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MountainShape {
    #[default]
    Cone,
    /// Long crest along the object's local X axis.
    Ridge,
    /// Cone with a crater on top.
    Volcano,
    /// Flat top with steep sides.
    Mesa,
}

impl MountainShape {
    pub const ALL: [Self; 4] = [Self::Cone, Self::Ridge, Self::Volcano, Self::Mesa];
    pub fn label(self) -> &'static str {
        match self {
            Self::Cone => "Cone",
            Self::Ridge => "Ridge",
            Self::Volcano => "Volcano",
            Self::Mesa => "Mesa",
        }
    }
}

/// How a mountain stamp combines with the ground below it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlendMode {
    /// Raise the ground by the shape.
    #[default]
    Add,
    /// Keep the higher of the ground and the shape (shape sits on the object's height).
    Max,
    /// Keep the lower of the two (carves down to the shape).
    Min,
    /// Replace the ground with the shape.
    Replace,
}

impl BlendMode {
    pub const ALL: [Self; 4] = [Self::Add, Self::Max, Self::Min, Self::Replace];
    pub fn label(self) -> &'static str {
        match self {
            Self::Add => "Add",
            Self::Max => "Max",
            Self::Min => "Min",
            Self::Replace => "Replace",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mountain {
    pub shape: MountainShape,
    /// Peak height in metres above the object (Add: above the ground).
    pub height: f32,
    pub blend: BlendMode,
    /// Noise on the shape (0..1).
    pub roughness: f32,
}

impl Default for Mountain {
    fn default() -> Self {
        Self {
            shape: MountainShape::Cone,
            height: 40.0,
            blend: BlendMode::Add,
            roughness: 0.3,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Flatten {
    /// Use the ground height under the centre at generation time instead of the object's height.
    pub sample: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct River {
    /// Control points in the object's local x/z (metres before scale); flows from first to last.
    pub points: Vec<[f32; 2]>,
    pub width: f32,
    pub depth: f32,
    /// Width of the sloped bank on each side (metres).
    pub bank: f32,
    /// Also place a water strip along the river.
    pub water: bool,
}

impl Default for River {
    fn default() -> Self {
        Self {
            points: vec![[-40.0, 0.0], [0.0, 15.0], [40.0, 0.0]],
            width: 8.0,
            depth: 3.0,
            bank: 10.0,
            water: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WaterMode {
    /// Flood-fill the basin around the object up to its height.
    #[default]
    Lake,
    /// Everything below the object's height is water.
    Sea,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Water {
    pub mode: WaterMode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RockArea {
    /// How strongly rock textures and rock scatter take over (0..1).
    pub strength: f32,
}

impl Default for RockArea {
    fn default() -> Self {
        Self { strength: 1.0 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BiomeZone {
    /// `.biome` asset assigned inside the zone.
    pub biome: String,
    /// Optional outline in local x/z (3+ points); empty = the circle of `radius`.
    pub polygon: Vec<[f32; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StampKind {
    Mountain(Mountain),
    Flatten(Flatten),
    River(River),
    Water(Water),
    RockArea(RockArea),
    BiomeZone(BiomeZone),
}

impl StampKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Mountain(_) => "Mountain / Hill",
            Self::Flatten(_) => "Flatten Area",
            Self::River(_) => "River / Valley",
            Self::Water(_) => "Water Body",
            Self::RockArea(_) => "Rock Area",
            Self::BiomeZone(_) => "Biome Zone",
        }
    }

    /// Gizmo colour (linear RGBA).
    pub fn color(&self) -> [f32; 4] {
        match self {
            Self::Mountain(_) => [0.95, 0.65, 0.3, 1.0],
            Self::Flatten(_) => [0.9, 0.9, 0.4, 1.0],
            Self::River(_) => [0.35, 0.7, 1.0, 1.0],
            Self::Water(_) => [0.3, 0.55, 1.0, 1.0],
            Self::RockArea(_) => [0.7, 0.7, 0.7, 1.0],
            Self::BiomeZone(_) => [0.45, 0.95, 0.45, 1.0],
        }
    }

    /// Changes the heightmap (else it only marks a region).
    pub fn shapes_height(&self) -> bool {
        matches!(self, Self::Mountain(_) | Self::Flatten(_) | Self::River(_))
    }
}

/// A stamp or area of the World Builder (scene object under the "World Builder" parent).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldStamp {
    pub enabled: bool,
    /// Footprint radius in metres (times the object's x/z scale).
    pub radius: f32,
    /// Soft fraction of the edge (0 = hard, 1 = soft all the way to the centre).
    pub falloff: f32,
    pub kind: StampKind,
}

impl Default for WorldStamp {
    fn default() -> Self {
        Self {
            enabled: true,
            radius: 60.0,
            falloff: 0.5,
            kind: StampKind::Mountain(Mountain::default()),
        }
    }
}

/// Where a stamp sits on the terrain (terrain-local metres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub center: Vec2,
    /// Height of the object above the terrain's corner.
    pub y: f32,
    /// World x/z directions of the object's local X and Z axes (unit length).
    pub axis_x: Vec2,
    pub axis_z: Vec2,
    pub scale: Vec2,
}

impl Placement {
    /// From an object's world matrix and the terrain corner.
    pub fn from_matrix(m: &Mat4, terrain_corner: Vec3) -> Self {
        let (scale, rot, pos) = m.to_scale_rotation_translation();
        let flat = |v: Vec3| Vec2::new(v.x, v.z).try_normalize();
        let axis_x = flat(rot * Vec3::X).unwrap_or(Vec2::X);
        let axis_z = flat(rot * Vec3::Z).unwrap_or(Vec2::Y);
        let local = pos - terrain_corner;
        Self {
            center: Vec2::new(local.x, local.z),
            y: local.y,
            axis_x,
            axis_z,
            scale: Vec2::new(scale.x.abs(), scale.z.abs()).max(Vec2::splat(1e-3)),
        }
    }

    pub fn at(center: Vec2, y: f32) -> Self {
        Self {
            center,
            y,
            axis_x: Vec2::X,
            axis_z: Vec2::Y,
            scale: Vec2::ONE,
        }
    }

    /// Terrain-local point → the stamp's local x/z (metres, before scale).
    pub fn to_local(&self, p: Vec2) -> Vec2 {
        let d = p - self.center;
        Vec2::new(d.dot(self.axis_x), d.dot(self.axis_z)) / self.scale
    }

    /// Stamp-local x/z → terrain-local point.
    pub fn to_terrain(&self, l: Vec2) -> Vec2 {
        self.center + self.axis_x * l.x * self.scale.x + self.axis_z * l.y * self.scale.y
    }

    /// Terrain-local bounds of a local-space circle of `radius`.
    pub fn bounds(&self, radius: f32) -> (Vec2, Vec2) {
        let r = radius * self.scale.max_element();
        (self.center - Vec2::splat(r), self.center + Vec2::splat(r))
    }
}

/// A stamp ready to apply: its settings and placement.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedStamp {
    pub stamp: WorldStamp,
    pub at: Placement,
    /// Seed for the stamp's noise (its order in the hierarchy).
    pub seed: u32,
}

/// Soft circular mask: 1 inside, fading to 0 over the outer `falloff` of the radius.
pub fn circle_mask(local: Vec2, radius: f32, falloff: f32) -> f32 {
    let d = local.length() / radius.max(1e-3);
    1.0 - smoothstep(1.0 - falloff.clamp(0.001, 1.0), 1.0, d)
}

impl PlacedStamp {
    /// Region mask (0..1) of an area stamp at a terrain-local point.
    pub fn mask(&self, p: Vec2) -> f32 {
        let s = &self.stamp;
        let l = self.at.to_local(p);
        if let StampKind::BiomeZone(z) = &s.kind
            && z.polygon.len() >= 3
        {
            let fade = (s.falloff * s.radius).max(0.01);
            let d = polygon_distance(&z.polygon, l);
            return smoothstep(-fade, 0.0, -d);
        }
        circle_mask(l, s.radius, s.falloff)
    }
}

/// Signed distance to a polygon (negative inside).
pub fn polygon_distance(poly: &[[f32; 2]], p: Vec2) -> f32 {
    let mut inside = false;
    let mut best = f32::MAX;
    let n = poly.len();
    for i in 0..n {
        let a = Vec2::from(poly[i]);
        let b = Vec2::from(poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
        best = best.min(segment_distance(p, a, b).0);
    }
    if inside { -best } else { best }
}

/// Distance from `p` to segment ab and the parameter (0..1) of the closest point.
pub fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
    ((a + ab * t).distance(p), t)
}

/// A heightmap being shaped: metres, `res²` samples `cell` metres apart.
pub struct HeightGrid<'a> {
    pub h: &'a mut [f32],
    pub res: usize,
    pub cell: Vec2,
}

impl HeightGrid<'_> {
    fn point(&self, ix: usize, iz: usize) -> Vec2 {
        Vec2::new(ix as f32 * self.cell.x, iz as f32 * self.cell.y)
    }

    /// Bilinear height at a terrain-local point (clamped to the edges).
    pub fn sample(&self, p: Vec2) -> f32 {
        let max = (self.res - 1) as f32;
        let fx = (p.x / self.cell.x).clamp(0.0, max);
        let fz = (p.y / self.cell.y).clamp(0.0, max);
        let (x0, z0) = (
            (fx as usize).min(self.res - 2),
            (fz as usize).min(self.res - 2),
        );
        let (u, v) = (fx - x0 as f32, fz - z0 as f32);
        let at = |x: usize, z: usize| self.h[z * self.res + x];
        let a = at(x0, z0) + (at(x0 + 1, z0) - at(x0, z0)) * u;
        let b = at(x0, z0 + 1) + (at(x0 + 1, z0 + 1) - at(x0, z0 + 1)) * u;
        a + (b - a) * v
    }

    /// Sample indices whose points fall inside the terrain-local box.
    fn range(&self, lo: Vec2, hi: Vec2) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        let r = self.res;
        let ix = |v: f32, c: f32| (v / c).clamp(0.0, (r - 1) as f32);
        let x0 = ix(lo.x, self.cell.x).floor() as usize;
        let x1 = ix(hi.x, self.cell.x).ceil() as usize;
        let z0 = ix(lo.y, self.cell.y).floor() as usize;
        let z1 = ix(hi.y, self.cell.y).ceil() as usize;
        (x0..x1 + 1, z0..z1 + 1)
    }
}

/// Apply a height-shaping stamp. Returns the river's water line when a river asks for water.
pub fn apply(grid: &mut HeightGrid, s: &PlacedStamp) -> Option<RiverPath> {
    if !s.stamp.enabled {
        return None;
    }
    match &s.stamp.kind {
        StampKind::Mountain(m) => {
            mountain(grid, s, m);
            None
        }
        StampKind::Flatten(f) => {
            flatten(grid, s, f);
            None
        }
        StampKind::River(r) => river(grid, s, r),
        _ => None,
    }
}

/// Shape value (0..1) of a mountain preset at local `l` (radius units).
fn mountain_shape(shape: MountainShape, l: Vec2) -> f32 {
    let d = l.length();
    match shape {
        MountainShape::Cone => {
            let t = (1.0 - d).max(0.0);
            // Rounded top, concave flanks.
            t * t * (2.0 - t)
        }
        MountainShape::Ridge => {
            let dr = (l.x * l.x + (l.y / 0.3).powi(2)).sqrt();
            let t = (1.0 - dr).max(0.0);
            t * (1.0 - 0.4 * l.x.abs())
        }
        MountainShape::Volcano => {
            let rim = 0.25;
            let t = super::portable::powf((1.0 - d).max(0.0), 1.2);
            if d < rim {
                let top = super::portable::powf(1.0f32 - rim, 1.2);
                top - 0.4 * (1.0 - (d / rim).powi(2))
            } else {
                t
            }
        }
        MountainShape::Mesa => 1.0 - smoothstep(0.72, 0.95, d),
    }
}

fn mountain(grid: &mut HeightGrid, s: &PlacedStamp, m: &Mountain) {
    let st = &s.stamp;
    let r = st.radius.max(0.1);
    let (lo, hi) = s.at.bounds(r);
    let (xs, zs) = grid.range(lo, hi);
    let noise = Fractal {
        octaves: 4,
        ..Default::default()
    };
    for iz in zs {
        for ix in xs.clone() {
            let l = s.at.to_local(grid.point(ix, iz)) / r;
            if l.length_squared() > 1.0 && m.shape != MountainShape::Ridge {
                continue;
            }
            let edge = 1.0 - smoothstep(1.0 - st.falloff.clamp(0.001, 1.0), 1.0, l.length());
            let mut p = mountain_shape(m.shape, l);
            if m.roughness > 0.0 {
                p *= 1.0 + 0.35 * m.roughness * fractal(l.x * 3.0, l.y * 3.0, s.seed, &noise);
            }
            let v = m.height * p.max(0.0);
            let h = &mut grid.h[iz * grid.res + ix];
            let target = s.at.y + v;
            *h = match m.blend {
                BlendMode::Add => *h + v * edge,
                BlendMode::Max => *h + (h.max(target) - *h) * edge,
                BlendMode::Min => *h + (h.min(target) - *h) * edge,
                BlendMode::Replace => *h + (target - *h) * edge,
            };
        }
    }
}

fn flatten(grid: &mut HeightGrid, s: &PlacedStamp, f: &Flatten) {
    let st = &s.stamp;
    let target = if f.sample {
        grid.sample(s.at.center)
    } else {
        s.at.y
    };
    let (lo, hi) = s.at.bounds(st.radius);
    let (xs, zs) = grid.range(lo, hi);
    for iz in zs {
        for ix in xs.clone() {
            let l = s.at.to_local(grid.point(ix, iz));
            let k = circle_mask(l, st.radius, st.falloff);
            if k > 0.0 {
                let h = &mut grid.h[iz * grid.res + ix];
                *h += (target - *h) * k;
            }
        }
    }
}

/// A river's centre line in terrain-local metres with its water height at each point.
#[derive(Clone, Debug, PartialEq)]
pub struct RiverPath {
    pub points: Vec<Vec2>,
    pub water: Vec<f32>,
    pub width: f32,
}

/// Catmull-Rom curve through the control points (terrain-local), `steps` samples per span.
pub fn spline(points: &[Vec2], steps: usize) -> Vec<Vec2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let n = points.len();
    let p = |i: isize| points[i.clamp(0, n as isize - 1) as usize];
    let mut out = Vec::with_capacity((n - 1) * steps + 1);
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (
            p(i as isize - 1),
            p(i as isize),
            p(i as isize + 1),
            p(i as isize + 2),
        );
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            let (t2, t3) = (t * t, t * t * t);
            out.push(
                0.5 * (2.0 * p1
                    + (p2 - p0) * t
                    + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                    + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3),
            );
        }
    }
    out.push(points[n - 1]);
    out
}

/// The river's centre line in terrain-local metres.
pub fn river_line(s: &PlacedStamp, r: &River) -> Vec<Vec2> {
    let control: Vec<Vec2> = r
        .points
        .iter()
        .map(|p| s.at.to_terrain(Vec2::from(*p)))
        .collect();
    spline(&control, 8)
}

fn river(grid: &mut HeightGrid, s: &PlacedStamp, r: &River) -> Option<RiverPath> {
    let line = river_line(s, r);
    if line.len() < 2 {
        return None;
    }
    // Bed: the ground minus the depth, never rising downstream.
    let mut bed: Vec<f32> = line.iter().map(|p| grid.sample(*p) - r.depth).collect();
    for i in 1..bed.len() {
        bed[i] = bed[i].min(bed[i - 1]);
    }
    let half = (r.width * 0.5).max(0.1);
    let reach = half + r.bank.max(0.0);
    let lo = line.iter().fold(Vec2::MAX, |a, p| a.min(*p)) - Vec2::splat(reach);
    let hi = line.iter().fold(Vec2::MIN, |a, p| a.max(*p)) + Vec2::splat(reach);
    let (xs, zs) = grid.range(lo, hi);
    for iz in zs {
        for ix in xs.clone() {
            let p = grid.point(ix, iz);
            let (mut best, mut bed_here) = (f32::MAX, 0.0);
            for i in 0..line.len() - 1 {
                let (d, t) = segment_distance(p, line[i], line[i + 1]);
                if d < best {
                    best = d;
                    bed_here = bed[i] + (bed[i + 1] - bed[i]) * t;
                }
            }
            if best >= reach {
                continue;
            }
            let h = &mut grid.h[iz * grid.res + ix];
            // Rounded channel inside the width, then a bank blending back to the ground.
            let channel = bed_here + 0.25 * r.depth * (best.min(half) / half).powi(2);
            let k = smoothstep(half, reach, best);
            let target = channel + (*h - channel) * k;
            *h = h.min(target);
        }
    }
    r.water.then(|| RiverPath {
        water: bed.iter().map(|b| b + r.depth * 0.7).collect(),
        points: line,
        width: r.width,
    })
}
