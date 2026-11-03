//! Voxel heightfield: triangles become solid spans per grid column; the tops an agent can stand
//! on become cells linked to their walkable neighbours, eroded by the agent radius.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use glam::{Vec2, Vec3};

use super::geometry::BakeGeometry;

/// No neighbour in that direction.
pub(super) const NONE: u32 = u32::MAX;
/// Neighbour offsets: 0 = -x, 1 = +z, 2 = +x, 3 = -z.
pub(super) const DIRS: [(i32, i32); 4] = [(-1, 0), (0, 1), (1, 0), (0, -1)];

/// Bake parameters in world units.
#[derive(Clone, Copy, Debug)]
pub(super) struct Config {
    pub cell: f32,
    pub climb: f32,
    pub height: f32,
    /// Cosine of the steepest walkable slope.
    pub walkable_cos: f32,
    pub radius: f32,
    pub min_region_area: f32,
}

#[derive(Clone, Copy, Debug)]
struct Span {
    min: f32,
    max: f32,
    walkable: bool,
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
}

/// Walkable cells of the grid with their connections.
pub(super) struct CompactField {
    /// World x/z of the grid's corner.
    pub origin: Vec2,
    pub cell: f32,
    pub cells: Vec<Cell>,
}

impl CompactField {
    /// World x/z of a cell's centre.
    pub fn center(&self, c: &Cell) -> Vec2 {
        self.origin + (Vec2::new(c.x as f32, c.z as f32) + 0.5) * self.cell
    }
}

struct Heightfield {
    width: usize,
    depth: usize,
    origin: Vec3,
    cell: f32,
    columns: Vec<Vec<Span>>,
}

/// Largest grid the bake allocates; bigger levels get coarser voxels.
const MAX_COLUMNS: f32 = 4_000_000.0;

/// Voxelise `geo` and return the eroded walkable cells. The voxel size may grow for huge
/// levels (the returned field has the size used).
pub(super) fn build(geo: &BakeGeometry, cfg: &Config) -> Option<CompactField> {
    let (lo, hi) = geo.bounds()?;
    let mut cell = cfg.cell;
    let extent = (hi - lo).max(Vec3::splat(cell));
    let columns = (extent.x / cell) * (extent.z / cell);
    if columns > MAX_COLUMNS {
        cell *= (columns / MAX_COLUMNS).sqrt();
    }
    let width = (extent.x / cell).ceil() as usize + 1;
    let depth = (extent.z / cell).ceil() as usize + 1;
    let mut hf = Heightfield {
        width,
        depth,
        origin: lo,
        cell,
        columns: vec![Vec::new(); width * depth],
    };
    let mut in_solid = vec![false; geo.triangles.len()];
    for r in &geo.solids {
        in_solid[r.clone()].fill(true);
        hf.rasterize_solid(geo, r.clone(), cfg);
    }
    for (i, (t, n)) in geo.triangles.iter().zip(&geo.normals).enumerate() {
        if !in_solid[i] {
            let mut spans = Vec::new();
            hf.cover(t, *n, n.y >= cfg.walkable_cos, |c, s| spans.push((c, s)));
            for (c, s) in spans {
                hf.add_span(c, s, cfg.climb);
            }
        }
    }
    hf.filter_low_hanging(cfg.climb);
    let mut field = hf.compact(cfg);
    let alive = erode(&field, cfg);
    let alive = drop_small_regions(&field, alive, cfg);
    field.retain(&alive);
    Some(field)
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
    out
}

impl Heightfield {
    /// A closed object fills each column it covers from its lowest to its highest point; the
    /// top is walkable if the face there is.
    fn rasterize_solid(&mut self, geo: &BakeGeometry, tris: std::ops::Range<usize>, cfg: &Config) {
        let mut cols: std::collections::HashMap<usize, Span> = Default::default();
        for i in tris {
            let n = geo.normals[i];
            self.cover(&geo.triangles[i], n, n.y >= cfg.walkable_cos, |c, s| {
                let e = cols.entry(c).or_insert(s);
                if (s.max - e.max).abs() < 1e-4 {
                    e.walkable |= s.walkable;
                } else if s.max > e.max {
                    e.walkable = s.walkable;
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
    fn cover(&self, t: &[Vec3; 3], n: Vec3, walkable: bool, mut f: impl FnMut(usize, Span)) {
        let cs = self.cell;
        let (lo, hi) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
        let index =
            |v: f32, o: f32, max: usize| (((v - o) / cs).floor().max(0.0) as usize).min(max - 1);
        let (x0, x1) = (
            index(lo.x, self.origin.x, self.width),
            index(hi.x, self.origin.x, self.width),
        );
        let (z0, z1) = (
            index(lo.z, self.origin.z, self.depth),
            index(hi.z, self.origin.z, self.depth),
        );
        for z in z0..=z1 {
            let zlo = self.origin.z + z as f32 * cs;
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
                    walkable,
                };
                f(z * self.width + x, span);
            }
        }
    }

    /// Insert a span, merging it with the spans it overlaps (the higher top decides the flag).
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
                s.walkable |= c.walkable;
            } else if c.max > s.max {
                s.walkable = c.walkable;
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
                if !s.walkable
                    && let Some(p) = prev
                    && p.walkable
                    && s.max - p.max <= climb
                {
                    s.walkable = true;
                }
                prev = Some(was);
            }
        }
    }

    /// Cells for walkable spans with room for the agent, linked to reachable neighbours.
    fn compact(&self, cfg: &Config) -> CompactField {
        let mut cells = Vec::new();
        // First cell index and count per column.
        let mut ranges = vec![(0u32, 0u32); self.columns.len()];
        for z in 0..self.depth {
            for x in 0..self.width {
                let ci = z * self.width + x;
                let col = &self.columns[ci];
                let start = cells.len() as u32;
                for (i, s) in col.iter().enumerate() {
                    let ceiling = col.get(i + 1).map_or(f32::INFINITY, |n| n.min);
                    if s.walkable && ceiling - s.max >= cfg.height {
                        cells.push(Cell {
                            x: x as u32,
                            z: z as u32,
                            y: s.max,
                            ceiling,
                            links: [NONE; 4],
                        });
                    }
                }
                ranges[ci] = (start, cells.len() as u32 - start);
            }
        }
        for i in 0..cells.len() {
            let c = cells[i];
            for (d, (dx, dz)) in DIRS.iter().enumerate() {
                let (nx, nz) = (c.x as i32 + dx, c.z as i32 + dz);
                if nx < 0 || nz < 0 || nx as usize >= self.width || nz as usize >= self.depth {
                    continue;
                }
                let (start, count) = ranges[nz as usize * self.width + nx as usize];
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
            origin: Vec2::new(self.origin.x, self.origin.z),
            cell: self.cell,
            cells,
        }
    }
}

impl CompactField {
    /// Keep only the `alive` cells (links are renumbered; links to removed cells vanish).
    fn retain(&mut self, alive: &[bool]) {
        let mut remap = vec![NONE; self.cells.len()];
        let mut next = 0u32;
        for (i, a) in alive.iter().enumerate() {
            if *a {
                remap[i] = next;
                next += 1;
            }
        }
        let old = std::mem::take(&mut self.cells);
        self.cells = old
            .into_iter()
            .enumerate()
            .filter(|(i, _)| alive[*i])
            .map(|(_, mut c)| {
                for l in &mut c.links {
                    if *l != NONE {
                        *l = remap[*l as usize];
                    }
                }
                c
            })
            .collect();
    }
}

/// Cells at least the agent radius away from any edge (chamfer distance: 2 per straight step,
/// 3 per diagonal step, like the usual voxel nav mesh bakers).
fn erode(f: &CompactField, cfg: &Config) -> Vec<bool> {
    let n = f.cells.len();
    let radius_cells = (cfg.radius / f.cell).ceil() as u32;
    if radius_cells == 0 {
        return vec![true; n];
    }
    let mut dist = vec![u32::MAX; n];
    let mut heap = BinaryHeap::new();
    for (i, c) in f.cells.iter().enumerate() {
        if c.links.contains(&NONE) {
            dist[i] = 0;
            heap.push(Reverse((0u32, i as u32)));
        }
    }
    while let Some(Reverse((d, i))) = heap.pop() {
        if d > dist[i as usize] {
            continue;
        }
        let c = &f.cells[i as usize];
        for dir in 0..4 {
            let a = c.links[dir];
            if a == NONE {
                continue;
            }
            let mut relax = |j: u32, cost: u32, heap: &mut BinaryHeap<_>| {
                let nd = d + cost;
                if nd < dist[j as usize] {
                    dist[j as usize] = nd;
                    heap.push(Reverse((nd, j)));
                }
            };
            relax(a, 2, &mut heap);
            let diag = f.cells[a as usize].links[(dir + 1) % 4];
            if diag != NONE {
                relax(diag, 3, &mut heap);
            }
        }
    }
    dist.iter().map(|d| *d >= radius_cells * 2).collect()
}

/// Drop connected islands smaller than the minimum region area.
fn drop_small_regions(f: &CompactField, mut alive: Vec<bool>, cfg: &Config) -> Vec<bool> {
    let min_cells = (cfg.min_region_area / (f.cell * f.cell)).max(1.0) as usize;
    let mut seen = vec![false; f.cells.len()];
    for start in 0..f.cells.len() {
        if seen[start] || !alive[start] {
            continue;
        }
        let mut island = vec![start];
        let mut queue = VecDeque::from([start]);
        seen[start] = true;
        while let Some(i) = queue.pop_front() {
            for &l in &f.cells[i].links {
                if l != NONE && alive[l as usize] && !seen[l as usize] {
                    seen[l as usize] = true;
                    island.push(l as usize);
                    queue.push_back(l as usize);
                }
            }
        }
        if island.len() < min_cells {
            for i in island {
                alive[i] = false;
            }
        }
    }
    alive
}
