//! Voxel heightfield of one tile (plus a border so erosion sees past the tile edge): triangles
//! become solid spans per grid column; the tops an agent can stand on become cells linked to
//! their walkable neighbours. Cells then get eroded by the agent radius, marked by area volumes,
//! carved by obstacles and small islands are dropped. Removed cells keep their slot with
//! [`NULL_AREA`], so links and column ranges stay valid.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use glam::{Vec2, Vec3, Vec3Swizzles};

use super::geometry::{AreaVolume, BakeGeometry};
use super::{AREA_NOT_WALKABLE, geometry};

/// No neighbour in that direction.
pub(super) const NONE: u32 = u32::MAX;
/// Neighbour offsets: 0 = -x, 1 = +z, 2 = +x, 3 = -z.
pub(super) const DIRS: [(i32, i32); 4] = [(-1, 0), (0, 1), (1, 0), (0, -1)];
/// Not walkable (steep surface, eroded, carved or cut out).
pub(super) const NULL_AREA: u8 = u8::MAX;

/// Bake parameters in world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Config {
    pub cell: f32,
    pub climb: f32,
    pub height: f32,
    /// Cosine of the steepest walkable slope.
    pub walkable_cos: f32,
    pub radius: f32,
    pub min_region_area: f32,
}

impl Config {
    /// Cells of padding around a tile: erosion and corner heights need to see past its edge.
    pub fn border(&self) -> u32 {
        (self.radius / self.cell).ceil() as u32 + 3
    }
}

#[derive(Clone, Copy, Debug)]
struct Span {
    min: f32,
    max: f32,
    area: u8,
}

/// One walkable surface sample: the top of a span with enough room above it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Cell {
    pub x: u32,
    pub z: u32,
    /// Surface height.
    pub y: f32,
    /// Bottom of the next span up (infinite when open).
    pub ceiling: f32,
    pub links: [u32; 4],
    pub area: u8,
}

/// Walkable cells of a tile's grid with their connections.
pub(super) struct CompactField {
    /// World x/z of the grid's corner (the border included).
    pub origin: Vec2,
    pub cell: f32,
    pub width: u32,
    pub depth: u32,
    /// Padding cells on each side of the tile proper.
    pub border: u32,
    pub cells: Vec<Cell>,
    /// First cell and count per column (`z * width + x`).
    pub columns: Vec<(u32, u32)>,
}

impl CompactField {
    /// World x/z of a cell's centre.
    pub fn center(&self, c: &Cell) -> Vec2 {
        self.origin + (Vec2::new(c.x as f32, c.z as f32) + 0.5) * self.cell
    }

    pub fn alive(&self, i: u32) -> bool {
        i != NONE && self.cells[i as usize].area != NULL_AREA
    }

    /// Live neighbour of cell `i` in direction `d`.
    pub fn neighbour(&self, i: u32, d: usize) -> Option<u32> {
        let n = self.cells[i as usize].links[d];
        self.alive(n).then_some(n)
    }

    /// Whether a cell lies in the tile proper (not its border).
    pub fn in_core(&self, c: &Cell) -> bool {
        let b = self.border;
        c.x >= b && c.z >= b && c.x < self.width - b && c.z < self.depth - b
    }

    /// Cells of column (x, z).
    pub fn column(&self, x: i32, z: i32) -> std::ops::Range<u32> {
        if x < 0 || z < 0 || x >= self.width as i32 || z >= self.depth as i32 {
            return 0..0;
        }
        let (s, n) = self.columns[(z as u32 * self.width + x as u32) as usize];
        s..s + n
    }
}

struct Heightfield {
    width: usize,
    depth: usize,
    origin: Vec2,
    cell: f32,
    columns: Vec<Vec<Span>>,
}

/// The grid a tile is voxelised on.
#[derive(Clone, Copy, Debug)]
pub(super) struct Grid {
    /// World x/z of the corner (border included).
    pub origin: Vec2,
    pub width: u32,
    pub depth: u32,
    pub border: u32,
}

/// Merge the areas of two spans whose tops meet: Not Walkable wins, then any walkable area
/// over none, then the higher area.
fn merge_area(a: u8, b: u8) -> u8 {
    if a == AREA_NOT_WALKABLE || b == AREA_NOT_WALKABLE {
        AREA_NOT_WALKABLE
    } else if a == NULL_AREA {
        b
    } else if b == NULL_AREA {
        a
    } else {
        a.max(b)
    }
}

/// Voxelise triangles `tris` of `geo` (indices; `solid_of[i]` groups closed objects) on `grid`
/// and return the walkable cells (not eroded yet). `clip` limits the cells to a world box.
pub(super) fn rasterize(
    geo: &BakeGeometry,
    tris: &[u32],
    solid_of: &[u32],
    grid: Grid,
    cfg: &Config,
    clip: Option<(Vec3, Vec3)>,
) -> CompactField {
    let mut hf = Heightfield {
        width: grid.width as usize,
        depth: grid.depth as usize,
        origin: grid.origin,
        cell: cfg.cell,
        columns: vec![Vec::new(); (grid.width * grid.depth) as usize],
    };
    let mut solids: HashMap<u32, Vec<u32>> = HashMap::new();
    for &i in tris {
        let s = solid_of.get(i as usize).copied().unwrap_or(NONE);
        if s != NONE {
            solids.entry(s).or_default().push(i);
            continue;
        }
        let (t, n) = (&geo.triangles[i as usize], geo.normals[i as usize]);
        let area = span_area(geo.area(i as usize), n, cfg);
        let mut spans = Vec::new();
        hf.cover(t, n, area, |c, s| spans.push((c, s)));
        for (c, s) in spans {
            hf.add_span(c, s, cfg.climb);
        }
    }
    let mut ids: Vec<_> = solids.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        hf.rasterize_solid(geo, &solids[&id], cfg);
    }
    hf.filter_low_hanging(cfg.climb);
    hf.compact(cfg, grid.border, clip)
}

/// Area a triangle's top gets: none on steep faces; Not Walkable always stays.
fn span_area(area: u8, n: Vec3, cfg: &Config) -> u8 {
    if area == AREA_NOT_WALKABLE {
        AREA_NOT_WALKABLE
    } else if n.y >= cfg.walkable_cos && geometry::walkable_area(area) {
        area
    } else {
        NULL_AREA
    }
}

/// Sutherland–Hodgman clip of a polygon against `p[axis] >= v` (keep_above) or `<= v`.
fn clip(poly: &[Vec3], axis: usize, v: f32, keep_above: bool) -> Vec<Vec3> {
    let inside = |p: &Vec3| {
        if keep_above {
            p[axis] >= v
        } else {
            p[axis] <= v
        }
    };
    let mut out = Vec::with_capacity(poly.len() + 2);
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let (ia, ib) = (inside(&a), inside(&b));
        if ia {
            out.push(a);
        }
        if ia != ib {
            let t = (v - a[axis]) / (b[axis] - a[axis]);
            out.push(a + (b - a) * t);
        }
    }
    // A face only touching the boundary leaves repeated points: no area there.
    out.dedup();
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

impl Heightfield {
    /// A closed object fills each column it covers from its lowest to its highest point; the
    /// top is walkable if the face there is.
    fn rasterize_solid(&mut self, geo: &BakeGeometry, tris: &[u32], cfg: &Config) {
        let mut cols: HashMap<usize, Span> = HashMap::new();
        for &i in tris {
            let n = geo.normals[i as usize];
            let area = span_area(geo.area(i as usize), n, cfg);
            self.cover(&geo.triangles[i as usize], n, area, |c, s| {
                let e = cols.entry(c).or_insert(s);
                if (s.max - e.max).abs() < 1e-4 {
                    e.area = merge_area(e.area, s.area);
                } else if s.max > e.max {
                    e.area = s.area;
                }
                e.min = e.min.min(s.min);
                e.max = e.max.max(s.max);
            });
        }
        let mut cols: Vec<_> = cols.into_iter().collect();
        cols.sort_by_key(|(c, _)| *c);
        for (c, s) in cols {
            self.add_span(c, s, cfg.climb);
        }
    }

    /// Call `f(column, span)` for every column the triangle covers.
    fn cover(&self, t: &[Vec3; 3], n: Vec3, area: u8, mut f: impl FnMut(usize, Span)) {
        let cs = self.cell;
        let (lo, hi) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
        let gx1 = self.origin.x + self.width as f32 * cs;
        let gz1 = self.origin.y + self.depth as f32 * cs;
        if hi.x < self.origin.x || hi.z < self.origin.y || lo.x > gx1 || lo.z > gz1 {
            return;
        }
        let index =
            |v: f32, o: f32, max: usize| (((v - o) / cs).floor().max(0.0) as usize).min(max - 1);
        let (x0, x1) = (
            index(lo.x, self.origin.x, self.width),
            index(hi.x, self.origin.x, self.width),
        );
        let (z0, z1) = (
            index(lo.z, self.origin.y, self.depth),
            index(hi.z, self.origin.y, self.depth),
        );
        let walkable = area != NULL_AREA && area != AREA_NOT_WALKABLE;
        for z in z0..=z1 {
            let zlo = self.origin.y + z as f32 * cs;
            let row = clip(&clip(t, 2, zlo, true), 2, zlo + cs, false);
            if row.len() < 3 {
                continue;
            }
            for x in x0..=x1 {
                let xlo = self.origin.x + x as f32 * cs;
                let p = clip(&clip(&row, 0, xlo, true), 0, xlo + cs, false);
                if p.len() < 3 {
                    continue;
                }
                let (ymin, ymax) = p
                    .iter()
                    .fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v.y), b.max(v.y)));
                // Walkable tops use the surface height at the cell centre.
                let top = if walkable {
                    let (cx, cz) = (xlo + cs * 0.5, zlo + cs * 0.5);
                    let y = t[0].y - (n.x * (cx - t[0].x) + n.z * (cz - t[0].z)) / n.y;
                    y.clamp(ymin, ymax)
                } else {
                    ymax
                };
                let span = Span {
                    min: ymin.min(top),
                    max: top,
                    area,
                };
                f(z * self.width + x, span);
            }
        }
    }

    /// Insert a span, merging it with the spans it overlaps (the higher top decides the area;
    /// tops within a step merge their areas).
    fn add_span(&mut self, column: usize, mut s: Span, climb: f32) {
        let col = &mut self.columns[column];
        let mut i = 0;
        while i < col.len() {
            let c = col[i];
            if c.min > s.max {
                break;
            }
            if c.max < s.min {
                i += 1;
                continue;
            }
            if (c.max - s.max).abs() <= climb {
                s.area = merge_area(s.area, c.area);
            } else if c.max > s.max {
                s.area = c.area;
            }
            s.min = s.min.min(c.min);
            s.max = s.max.max(c.max);
            col.remove(i);
        }
        col.insert(i, s);
    }

    /// Obstacles a step high on top of walkable ground (kerbs, stair edges) become walkable.
    fn filter_low_hanging(&mut self, climb: f32) {
        for col in &mut self.columns {
            let mut prev: Option<Span> = None;
            for s in col.iter_mut() {
                let was = *s;
                if s.area == NULL_AREA
                    && let Some(p) = prev
                    && p.area != NULL_AREA
                    && p.area != AREA_NOT_WALKABLE
                    && s.max - p.max <= climb
                {
                    s.area = p.area;
                }
                prev = Some(was);
            }
        }
    }

    /// Cells for walkable spans with room for the agent, linked to reachable neighbours.
    fn compact(&self, cfg: &Config, border: u32, clip: Option<(Vec3, Vec3)>) -> CompactField {
        let mut cells = Vec::new();
        let mut columns = vec![(0u32, 0u32); self.columns.len()];
        for z in 0..self.depth {
            for x in 0..self.width {
                let ci = z * self.width + x;
                let col = &self.columns[ci];
                let start = cells.len() as u32;
                let center = self.origin + (Vec2::new(x as f32, z as f32) + 0.5) * self.cell;
                for (i, s) in col.iter().enumerate() {
                    let ceiling = col.get(i + 1).map_or(f32::INFINITY, |n| n.min);
                    if s.area == NULL_AREA
                        || s.area == AREA_NOT_WALKABLE
                        || ceiling - s.max < cfg.height
                    {
                        continue;
                    }
                    if let Some((lo, hi)) = clip {
                        let p = Vec3::new(center.x, s.max, center.y);
                        if p.cmplt(lo).any() || p.cmpgt(hi).any() {
                            continue;
                        }
                    }
                    cells.push(Cell {
                        x: x as u32,
                        z: z as u32,
                        y: s.max,
                        ceiling,
                        links: [NONE; 4],
                        area: s.area,
                    });
                }
                columns[ci] = (start, cells.len() as u32 - start);
            }
        }
        for i in 0..cells.len() {
            let c = cells[i];
            for (d, (dx, dz)) in DIRS.iter().enumerate() {
                let (nx, nz) = (c.x as i32 + dx, c.z as i32 + dz);
                if nx < 0 || nz < 0 || nx as usize >= self.width || nz as usize >= self.depth {
                    continue;
                }
                let (start, count) = columns[nz as usize * self.width + nx as usize];
                let best = (start..start + count)
                    .filter(|&j| {
                        let n = &cells[j as usize];
                        let gap = c.ceiling.min(n.ceiling) - c.y.max(n.y);
                        (n.y - c.y).abs() <= cfg.climb && gap >= cfg.height
                    })
                    .min_by(|&a, &b| {
                        let da = (cells[a as usize].y - c.y).abs();
                        let db = (cells[b as usize].y - c.y).abs();
                        da.total_cmp(&db)
                    });
                if let Some(j) = best {
                    cells[i].links[d] = j;
                }
            }
        }
        CompactField {
            origin: self.origin,
            cell: self.cell,
            width: self.width as u32,
            depth: self.depth as u32,
            border,
            cells,
            columns,
        }
    }
}

/// Remove cells closer than the agent radius to any edge (chamfer distance: 2 per straight
/// step, 3 per diagonal step, like the usual voxel nav mesh bakers).
pub(super) fn erode(f: &mut CompactField, radius: f32) {
    let n = f.cells.len();
    let radius_cells = (radius / f.cell).ceil() as u32;
    if radius_cells == 0 {
        return;
    }
    let mut dist = vec![u32::MAX; n];
    let mut heap = BinaryHeap::new();
    for (i, d) in dist.iter_mut().enumerate() {
        if f.cells[i].area == NULL_AREA {
            continue;
        }
        if (0..4).any(|k| f.neighbour(i as u32, k).is_none()) {
            *d = 0;
            heap.push(Reverse((0u32, i as u32)));
        }
    }
    while let Some(Reverse((d, i))) = heap.pop() {
        if d > dist[i as usize] {
            continue;
        }
        for dir in 0..4 {
            let Some(a) = f.neighbour(i, dir) else {
                continue;
            };
            let mut relax = |j: u32, cost: u32, heap: &mut BinaryHeap<_>| {
                let nd = d + cost;
                if nd < dist[j as usize] {
                    dist[j as usize] = nd;
                    heap.push(Reverse((nd, j)));
                }
            };
            relax(a, 2, &mut heap);
            if let Some(diag) = f.neighbour(a, (dir + 1) % 4) {
                relax(diag, 3, &mut heap);
            }
        }
    }
    for (i, c) in f.cells.iter_mut().enumerate() {
        if c.area != NULL_AREA && dist[i] < radius_cells * 2 {
            c.area = NULL_AREA;
        }
    }
}

/// Give cells inside area volumes their area (Not Walkable removes them; among overlapping
/// volumes Not Walkable wins, then the higher area).
pub(super) fn mark_volumes(f: &mut CompactField, volumes: &[AreaVolume]) {
    if volumes.is_empty() {
        return;
    }
    for i in 0..f.cells.len() {
        let c = f.cells[i];
        if c.area == NULL_AREA {
            continue;
        }
        let xz = f.center(&c);
        let p = Vec3::new(xz.x, c.y, xz.y);
        let mut area: Option<u8> = None;
        for v in volumes {
            if p.cmplt(v.lo - Vec3::Y * 0.3).any() || p.cmpgt(v.hi).any() {
                continue;
            }
            if v.contains(p) || v.contains(p + Vec3::Y * 0.25) {
                area = Some(area.map_or(v.area, |a| merge_area(a, v.area)));
            }
        }
        if let Some(a) = area {
            f.cells[i].area = if a == AREA_NOT_WALKABLE { NULL_AREA } else { a };
        }
    }
}

/// A carving obstacle's footprint (world).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarveShape {
    pub center: Vec2,
    /// Box: unit x axis on x/z and half sizes; capsule: `radius` > 0 and `half` unused.
    pub axis: Vec2,
    pub half: Vec2,
    pub radius: f32,
    pub capsule: bool,
    pub y_min: f32,
    pub y_max: f32,
}

impl CarveShape {
    /// World x/z bounds grown by `pad`.
    pub fn bounds(&self, pad: f32) -> (Vec2, Vec2) {
        let r = if self.capsule {
            Vec2::splat(self.radius)
        } else {
            let a = self.axis.abs();
            let b = Vec2::new(a.y, a.x);
            a * self.half.x + b * self.half.y
        };
        (self.center - r - pad, self.center + r + pad)
    }

    /// Whether x/z `p` lies within the footprint grown by `pad`.
    pub fn contains(&self, p: Vec2, pad: f32) -> bool {
        let q = p - self.center;
        if self.capsule {
            return q.length() <= self.radius + pad;
        }
        let y_axis = Vec2::new(-self.axis.y, self.axis.x);
        q.dot(self.axis).abs() <= self.half.x + pad && q.dot(y_axis).abs() <= self.half.y + pad
    }
}

/// Cut obstacles (grown by the agent radius) out of the cells their height range overlaps.
pub(super) fn carve(f: &mut CompactField, shapes: &[CarveShape], cfg: &Config) {
    for s in shapes {
        for i in 0..f.cells.len() {
            let c = f.cells[i];
            if c.area == NULL_AREA || c.y > s.y_max || c.y + cfg.height < s.y_min {
                continue;
            }
            if s.contains(f.center(&c), cfg.radius) {
                f.cells[i].area = NULL_AREA;
            }
        }
    }
}

/// Drop connected islands smaller than the minimum region area (islands reaching the edge of
/// the grid may go on in the next tile and are kept).
pub(super) fn drop_small_regions(f: &mut CompactField, min_area: f32) {
    let min_cells = (min_area / (f.cell * f.cell)).max(1.0) as usize;
    let mut seen = vec![false; f.cells.len()];
    for start in 0..f.cells.len() {
        if seen[start] || f.cells[start].area == NULL_AREA {
            continue;
        }
        let mut island = vec![start];
        let mut queue = VecDeque::from([start]);
        seen[start] = true;
        let mut open = false;
        while let Some(i) = queue.pop_front() {
            let c = &f.cells[i];
            open |= c.x == 0 || c.z == 0 || c.x + 1 == f.width || c.z + 1 == f.depth;
            for d in 0..4 {
                if let Some(l) = f.neighbour(i as u32, d)
                    && !seen[l as usize]
                {
                    seen[l as usize] = true;
                    island.push(l as usize);
                    queue.push_back(l as usize);
                }
            }
        }
        if !open && island.len() < min_cells {
            for i in island {
                f.cells[i].area = NULL_AREA;
            }
        }
    }
}

/// Height of the walkable cell nearest to `y` in the column under world x/z `p`.
pub(super) fn sample_height(f: &CompactField, p: Vec3) -> Option<f32> {
    let q = (p.xz() - f.origin) / f.cell;
    let (x, z) = (q.x.floor() as i32, q.y.floor() as i32);
    f.column(x, z)
        .map(|i| f.cells[i as usize])
        .filter(|c| c.area != NULL_AREA)
        .map(|c| c.y)
        .min_by(|a, b| (a - p.y).abs().total_cmp(&(b - p.y).abs()))
}
