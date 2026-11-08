//! The baked nav mesh: tiles of convex polygons (with an area and detail heights each), portal
//! links inside and between tiles, off-mesh links, point queries and the `.navmesh` file format
//! (JSON, one tile per line).
//!
//! Polygons are addressed by [`PolyRef`]: tile index in the high bits, polygon in the low 16.
//! Rebuilding a tile keeps its index; links to and from it are made again.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use glam::{Vec2, Vec3, Vec3Swizzles};
use serde::{Deserialize, Serialize};

use super::query::QueryFilter;
use super::{AREA_WALKABLE, AgentSettings};

/// Current file format.
const VERSION: u32 = 2;
const POLY_BITS: u32 = 16;
const POLY_MASK: u32 = (1 << POLY_BITS) - 1;

/// A polygon: tile index << 16 | polygon index.
pub type PolyRef = u32;

pub fn poly_ref(tile: u32, poly: u32) -> PolyRef {
    (tile << POLY_BITS) | poly
}
pub fn tile_of(r: PolyRef) -> u32 {
    r >> POLY_BITS
}
pub fn poly_of(r: PolyRef) -> u32 {
    r & POLY_MASK
}

/// A shared edge to a neighbouring polygon (`a`..`b`, on this polygon's surface).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NavLink {
    pub to: PolyRef,
    pub a: Vec3,
    pub b: Vec3,
}

fn walkable(a: &u8) -> bool {
    *a == AREA_WALKABLE
}

/// One convex walkable polygon (counter-clockwise on x/z).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NavPoly {
    pub vertices: Vec<Vec3>,
    #[serde(default, skip_serializing_if = "walkable")]
    pub area: u8,
    /// Extra surface points of the detail mesh.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail_verts: Vec<Vec3>,
    /// Detail triangles: indices into the corners, then `detail_verts`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail_tris: Vec<[u8; 3]>,
    /// Portals (made when loading; older files stored them).
    #[serde(default, skip_serializing)]
    pub links: Vec<NavLink>,
}

impl NavPoly {
    pub fn new(vertices: Vec<Vec3>, area: u8) -> Self {
        let n = vertices.len();
        Self {
            vertices,
            area,
            detail_verts: Vec::new(),
            detail_tris: (1..n.saturating_sub(1))
                .map(|i| [0, i as u8, (i + 1) as u8])
                .collect(),
            links: Vec::new(),
        }
    }

    /// Detail triangles in world space.
    pub fn detail_triangles(&self) -> impl Iterator<Item = [Vec3; 3]> + '_ {
        self.detail_tris
            .iter()
            .map(|t| t.map(|k| self.detail_vertex(k as usize)))
    }
}

/// One tile of polygons.
#[derive(Clone, Debug, Default)]
pub struct Tile {
    pub x: i32,
    pub z: i32,
    pub polys: Vec<NavPoly>,
    /// Hash of the bake input that made it (unchanged input → no rebuild).
    pub hash: u64,
    bounds: Vec<(Vec3, Vec3)>,
}

impl Tile {
    pub fn new(x: i32, z: i32, polys: Vec<NavPoly>, hash: u64) -> Self {
        let mut t = Self {
            x,
            z,
            polys,
            hash,
            bounds: Vec::new(),
        };
        t.bounds = t.polys.iter().map(poly_bounds).collect();
        t
    }
}

fn poly_bounds(p: &NavPoly) -> (Vec3, Vec3) {
    let first = p.vertices.first().copied().unwrap_or_default();
    p.vertices
        .iter()
        .chain(&p.detail_verts)
        .fold((first, first), |(lo, hi), v| (lo.min(*v), hi.max(*v)))
}

/// An off-mesh connection (from a NavMesh Link) with the polygons its ends snapped to.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct OffMeshLink {
    pub start: Vec3,
    pub end: Vec3,
    pub width: f32,
    pub bidirectional: bool,
    /// Multiplies the link's length (negative = the area's cost).
    pub cost_modifier: f32,
    pub area: u8,
    /// Who made it (entity bits), for updates.
    pub owner: u64,
    pub start_poly: Option<PolyRef>,
    pub end_poly: Option<PolyRef>,
    /// Ends snapped onto the mesh.
    pub start_on_mesh: Vec3,
    pub end_on_mesh: Vec3,
}

impl OffMeshLink {
    pub fn connected(&self) -> bool {
        self.start_poly.is_some() && self.end_poly.is_some()
    }

    /// (from, to, from poly, to poly) walking forward or backward.
    pub fn ends(&self, forward: bool) -> (Vec3, Vec3, PolyRef, PolyRef) {
        let (s, e) = (
            self.start_poly.unwrap_or_default(),
            self.end_poly.unwrap_or_default(),
        );
        if forward {
            (self.start_on_mesh, self.end_on_mesh, s, e)
        } else {
            (self.end_on_mesh, self.start_on_mesh, e, s)
        }
    }
}

/// Result of a nearest-point query.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NearestHit {
    pub poly: PolyRef,
    pub position: Vec3,
    pub distance: f32,
}

/// What a nav mesh was baked with (saved in the file; runtime rebuilds reuse it).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildSettings {
    pub agent: AgentSettings,
    pub agent_type: u32,
    pub cell_size: f32,
    /// Tile width in voxels (0 = one tile, older files).
    pub tile_size: u32,
    /// World x/z of tile (0, 0)'s corner.
    pub origin: Vec2,
    pub min_region_area: f32,
}

impl Default for BuildSettings {
    fn default() -> Self {
        Self {
            agent: AgentSettings::default(),
            agent_type: 0,
            cell_size: 0.1667,
            tile_size: 0,
            origin: Vec2::ZERO,
            min_region_area: 2.0,
        }
    }
}

impl BuildSettings {
    /// Tile width in metres (infinite for single-tile meshes).
    pub fn tile_world(&self) -> f32 {
        if self.tile_size == 0 {
            f32::INFINITY
        } else {
            self.tile_size as f32 * self.cell_size
        }
    }

    /// Tile coordinates holding world x/z `p`.
    pub fn tile_at(&self, p: Vec2) -> (i32, i32) {
        let w = self.tile_world();
        if !w.is_finite() {
            return (0, 0);
        }
        let q = ((p - self.origin) / w).floor();
        (q.x as i32, q.y as i32)
    }

    /// World x/z bounds of tile (x, z) (from global voxel indices).
    pub fn tile_bounds(&self, x: i32, z: i32) -> (Vec2, Vec2) {
        let ts = self.tile_size as i64;
        let at = |i: i64, j: i64| {
            Vec2::new(
                self.origin.x + i as f32 * self.cell_size,
                self.origin.y + j as f32 * self.cell_size,
            )
        };
        (
            at(x as i64 * ts, z as i64 * ts),
            at((x as i64 + 1) * ts, (z as i64 + 1) * ts),
        )
    }
}

/// A baked nav mesh with lookup data.
#[derive(Debug, Default, Clone)]
pub struct NavMesh {
    pub settings: BuildSettings,
    /// Agent the mesh was baked for (same as `settings.agent`).
    pub agent: AgentSettings,
    pub cell_size: f32,
    tiles: Vec<Arc<Tile>>,
    lookup: HashMap<(i32, i32), u32>,
    pub off_links: Vec<OffMeshLink>,
    /// Off-mesh links leaving each polygon: (link, forward).
    link_index: HashMap<PolyRef, Vec<(u32, bool)>>,
}

/// 2D cross product of (b - a) × (c - a) on x/z.
pub(crate) fn cross2(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    (b - a).perp_dot(c - a)
}

/// Closest point to `p` on segment `a`..`b` (x/z), as a parameter 0..1.
pub(crate) fn segment_t(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    let d = b - a;
    let len2 = d.length_squared();
    if len2 < 1e-12 {
        0.0
    } else {
        ((p - a).dot(d) / len2).clamp(0.0, 1.0)
    }
}

impl NavMesh {
    /// A mesh from finished tiles (links are made here).
    pub fn from_tiles(settings: BuildSettings, tiles: Vec<Tile>) -> Self {
        let mut m = Self {
            settings,
            agent: settings.agent,
            cell_size: settings.cell_size,
            ..Default::default()
        };
        for t in tiles {
            let i = m.tiles.len() as u32;
            m.lookup.insert((t.x, t.z), i);
            m.tiles.push(Arc::new(t));
        }
        for i in 0..m.tiles.len() as u32 {
            m.link_internal(i);
        }
        for i in 0..m.tiles.len() as u32 {
            m.link_external(i);
        }
        m
    }

    /// A single-tile mesh from polygons whose `links` are already set (older files, tests).
    pub fn new(agent: AgentSettings, cell_size: f32, polys: Vec<NavPoly>) -> Self {
        let settings = BuildSettings {
            agent,
            cell_size,
            ..Default::default()
        };
        let mut m = Self {
            settings,
            agent,
            cell_size,
            ..Default::default()
        };
        m.lookup.insert((0, 0), 0);
        m.tiles.push(Arc::new(Tile::new(0, 0, polys, 0)));
        m
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.iter().all(|t| t.polys.is_empty())
    }

    pub fn tiles(&self) -> &[Arc<Tile>] {
        &self.tiles
    }

    pub fn tile_index(&self, x: i32, z: i32) -> Option<u32> {
        self.lookup.get(&(x, z)).copied()
    }

    /// Every polygon with its reference.
    pub fn polys(&self) -> impl Iterator<Item = (PolyRef, &NavPoly)> + '_ {
        self.tiles.iter().enumerate().flat_map(|(ti, t)| {
            t.polys
                .iter()
                .enumerate()
                .map(move |(pi, p)| (poly_ref(ti as u32, pi as u32), p))
        })
    }

    pub fn poly_count(&self) -> usize {
        self.tiles.iter().map(|t| t.polys.len()).sum()
    }

    pub fn poly(&self, r: PolyRef) -> &NavPoly {
        &self.tiles[tile_of(r) as usize].polys[poly_of(r) as usize]
    }

    /// Whether `r` names an existing polygon.
    pub fn valid(&self, r: PolyRef) -> bool {
        self.tiles
            .get(tile_of(r) as usize)
            .is_some_and(|t| (poly_of(r) as usize) < t.polys.len())
    }

    pub fn bounds(&self, r: PolyRef) -> (Vec3, Vec3) {
        self.tiles[tile_of(r) as usize].bounds[poly_of(r) as usize]
    }

    pub fn area_of(&self, r: PolyRef) -> u8 {
        self.poly(r).area
    }

    /// Surface height of polygon `r` above x/z (its detail mesh).
    pub fn height(&self, r: PolyRef, x: f32, z: f32) -> f32 {
        let p = self.poly(r);
        p.detail_at(Vec2::new(x, z))
            .map_or_else(|| p.vertices[0].y, |(_, h)| h)
    }

    /// Whether x/z of `p` lies inside polygon `r` (edges included).
    pub fn contains_xz(&self, r: PolyRef, p: Vec3) -> bool {
        let v = &self.poly(r).vertices;
        let q = p.xz();
        let (mut pos, mut neg) = (false, false);
        for i in 0..v.len() {
            let c = cross2(v[i].xz(), v[(i + 1) % v.len()].xz(), q);
            pos |= c > 1e-6;
            neg |= c < -1e-6;
        }
        !(pos && neg)
    }

    /// Closest point on polygon `r`'s surface to `p`.
    pub fn closest_point(&self, r: PolyRef, p: Vec3) -> Vec3 {
        if self.contains_xz(r, p) {
            return Vec3::new(p.x, self.height(r, p.x, p.z), p.z);
        }
        let v = &self.poly(r).vertices;
        let q = p.xz();
        let mut best = (f32::MAX, v[0]);
        for i in 0..v.len() {
            let (a, b) = (v[i], v[(i + 1) % v.len()]);
            let t = segment_t(a.xz(), b.xz(), q);
            let c = a.lerp(b, t);
            let d = c.xz().distance_squared(q);
            if d < best.0 {
                best = (d, c);
            }
        }
        let c = best.1;
        Vec3::new(c.x, self.height(r, c.x, c.z), c.z)
    }

    /// Centre of polygon `r` (vertex average).
    pub fn center(&self, r: PolyRef) -> Vec3 {
        let v = &self.poly(r).vertices;
        v.iter().copied().sum::<Vec3>() / v.len().max(1) as f32
    }

    /// Polygons whose tiles overlap the square `p ± r` (x/z), with their bounds.
    fn around(&self, p: Vec2, r: f32) -> Vec<(PolyRef, (Vec3, Vec3))> {
        let mut out = Vec::new();
        let w = self.settings.tile_world();
        let mut visit = |ti: u32| {
            let t = &self.tiles[ti as usize];
            for (pi, b) in t.bounds.iter().enumerate() {
                let gap = (b.0.xz() - p).max(p - b.1.xz()).max(Vec2::ZERO);
                if gap.x <= r && gap.y <= r {
                    out.push((poly_ref(ti, pi as u32), *b));
                }
            }
        };
        if !w.is_finite() || r > w * 8.0 {
            (0..self.tiles.len() as u32).for_each(&mut visit);
            return out;
        }
        let (x0, z0) = self.settings.tile_at(p - r);
        let (x1, z1) = self.settings.tile_at(p + r);
        for z in z0..=z1 {
            for x in x0..=x1 {
                if let Some(&ti) = self.lookup.get(&(x, z)) {
                    visit(ti);
                }
            }
        }
        out
    }

    /// Nearest point on the mesh within `max_distance` of `p` on an area `filter` allows
    /// (`NavMesh.SamplePosition`).
    pub fn nearest_filtered(
        &self,
        p: Vec3,
        max_distance: f32,
        filter: &QueryFilter,
    ) -> Option<NearestHit> {
        let r = max_distance.min(1e5);
        let mut best: Option<NearestHit> = None;
        for (i, (lo, hi)) in self.around(p.xz(), r) {
            if !filter.passes(self.area_of(i)) {
                continue;
            }
            let gap = (lo - p).max(p - hi).max(Vec3::ZERO);
            if gap.length() > r || best.is_some_and(|b| gap.length() >= b.distance) {
                continue;
            }
            let c = self.closest_point(i, p);
            let d = c.distance(p);
            if d <= r && best.is_none_or(|b| d < b.distance) {
                best = Some(NearestHit {
                    poly: i,
                    position: c,
                    distance: d,
                });
            }
        }
        best
    }

    /// Nearest point on any area.
    pub fn nearest(&self, p: Vec3, max_distance: f32) -> Option<NearestHit> {
        self.nearest_filtered(p, max_distance, &QueryFilter::default())
    }

    /// Polygon under `p` (x/z inside, surface within `max_dy` vertically), closest in height.
    pub fn locate(&self, p: Vec3, max_dy: f32) -> Option<PolyRef> {
        self.around(p.xz(), 0.0)
            .into_iter()
            .map(|(i, _)| i)
            .filter(|&i| self.contains_xz(i, p))
            .map(|i| (i, (self.height(i, p.x, p.z) - p.y).abs()))
            .filter(|(_, dy)| *dy <= max_dy)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Move from `from` (on polygon `poly`) towards x/z of `to` along the surface: through
    /// portals the move crosses (on areas `filter` allows), sliding along walls. Returns the
    /// new polygon and surface position.
    pub fn move_along(
        &self,
        poly: PolyRef,
        from: Vec3,
        to: Vec3,
        filter: &QueryFilter,
    ) -> (PolyRef, Vec3) {
        if !self.valid(poly) {
            return (poly, from);
        }
        let (start, target) = (from.xz(), to.xz());
        let search = start.distance(target) * 0.5 + self.cell_size * 2.0 + 1e-3;
        let mid = (start + target) * 0.5;
        let mut visited = vec![poly];
        let mut i = 0;
        let mut best: (f32, PolyRef, Vec2) = (f32::MAX, poly, start);
        let probe = Vec3::new(to.x, from.y, to.z);
        while i < visited.len() && visited.len() < 128 {
            let cur = visited[i];
            i += 1;
            if self.contains_xz(cur, probe) {
                best = (0.0, cur, target);
                break;
            }
            let p = self.poly(cur);
            let v = &p.vertices;
            for k in 0..v.len() {
                let (a, b) = (v[k].xz(), v[(k + 1) % v.len()].xz());
                // Neighbours across this edge.
                let mut open = 0.0;
                for l in &p.links {
                    if !on_segment(l.a.xz(), a, b) || !on_segment(l.b.xz(), a, b) {
                        continue;
                    }
                    if !filter.passes(self.area_of(l.to)) {
                        continue;
                    }
                    let (pa, pb) = (l.a.xz(), l.b.xz());
                    open += pa.distance(pb);
                    let t = segment_t(pa, pb, mid);
                    if pa.lerp(pb, t).distance(mid) <= search && !visited.contains(&l.to) {
                        visited.push(l.to);
                    }
                }
                if open >= a.distance(b) - 1e-4 {
                    continue;
                }
                // A wall (or a partly open tile seam): slide along it.
                let t = segment_t(a, b, target);
                let c = a.lerp(b, t);
                let d = c.distance_squared(target);
                if d < best.0 {
                    best = (d, cur, c);
                }
            }
        }
        let (_, r, xz) = best;
        (r, Vec3::new(xz.x, self.height(r, xz.x, xz.y), xz.y))
    }

    /// Fan triangles of every polygon (for drawing and area sums).
    pub fn triangles(&self) -> impl Iterator<Item = [Vec3; 3]> + '_ {
        self.polys().flat_map(|(_, p)| {
            (1..p.vertices.len().saturating_sub(1))
                .map(move |i| [p.vertices[0], p.vertices[i], p.vertices[i + 1]])
        })
    }

    /// Total walkable area in m² (x/z).
    pub fn area(&self) -> f32 {
        self.triangles()
            .map(|t| cross2(t[0].xz(), t[1].xz(), t[2].xz()).abs() * 0.5)
            .sum()
    }

    // ---- links ---------------------------------------------------------------------------

    /// Portals between polygons of tile `ti` that share an edge.
    fn link_internal(&mut self, ti: u32) {
        let climb = self.settings.agent.step_height.max(0.05) + 1e-3;
        let tile = Arc::make_mut(&mut self.tiles[ti as usize]);
        let key = |v: Vec3| (v.x.to_bits(), v.z.to_bits());
        type Key = (u32, u32);
        let mut edges: HashMap<(Key, Key), Vec<(usize, usize)>> = HashMap::new();
        for (pi, p) in tile.polys.iter().enumerate() {
            let n = p.vertices.len();
            for k in 0..n {
                edges
                    .entry((key(p.vertices[k]), key(p.vertices[(k + 1) % n])))
                    .or_default()
                    .push((pi, k));
            }
        }
        let mut new_links: Vec<Vec<NavLink>> = vec![Vec::new(); tile.polys.len()];
        for (pi, p) in tile.polys.iter().enumerate() {
            let n = p.vertices.len();
            for k in 0..n {
                let (a, b) = (p.vertices[k], p.vertices[(k + 1) % n]);
                let Some(others) = edges.get(&(key(b), key(a))) else {
                    continue;
                };
                for &(qi, qk) in others {
                    if qi == pi {
                        continue;
                    }
                    let q = &tile.polys[qi];
                    let (qa, qb) = (q.vertices[qk], q.vertices[(qk + 1) % q.vertices.len()]);
                    if (qa.y - b.y).abs() <= climb && (qb.y - a.y).abs() <= climb {
                        new_links[pi].push(NavLink {
                            to: poly_ref(ti, qi as u32),
                            a,
                            b,
                        });
                    }
                }
            }
        }
        for (p, links) in tile.polys.iter_mut().zip(new_links) {
            p.links.retain(|l| tile_of(l.to) != ti);
            p.links.extend(links);
        }
    }

    /// Portals between tile `ti` and its four neighbours along the shared tile edges.
    fn link_external(&mut self, ti: u32) {
        if self.settings.tile_size == 0 {
            return;
        }
        let (tx, tz) = (self.tiles[ti as usize].x, self.tiles[ti as usize].z);
        // (dx, dz, axis of the shared edge's coordinate: 0 = x, 1 = z)
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let Some(ni) = self.tile_index(tx + dx, tz + dz) else {
                continue;
            };
            // Forget old links both ways.
            for (a, b) in [(ti, ni), (ni, ti)] {
                let t = Arc::make_mut(&mut self.tiles[a as usize]);
                for p in &mut t.polys {
                    p.links.retain(|l| tile_of(l.to) != b);
                }
            }
            let (lo, hi) = self.settings.tile_bounds(tx, tz);
            // Boundary coordinate and the axis along it.
            let (fixed_axis, value) = match (dx, dz) {
                (1, 0) => (0, hi.x),
                (-1, 0) => (0, lo.x),
                (0, 1) => (1, hi.y),
                _ => (1, lo.y),
            };
            let links = self.border_pairs(ti, ni, fixed_axis, value);
            for (from, l) in links {
                let t = Arc::make_mut(&mut self.tiles[tile_of(from) as usize]);
                t.polys[poly_of(from) as usize].links.push(l);
            }
        }
    }

    /// Links between polygon edges of tiles `a` and `b` lying on the line `axis = value`.
    fn border_pairs(&self, a: u32, b: u32, axis: usize, value: f32) -> Vec<(PolyRef, NavLink)> {
        let eps = self.cell_size * 0.01;
        let climb = self.settings.agent.step_height.max(0.05) + 1e-3;
        let coord = |v: Vec3| if axis == 0 { v.x } else { v.z };
        let along = |v: Vec3| if axis == 0 { v.z } else { v.x };
        let edges = |ti: u32| {
            let t = &self.tiles[ti as usize];
            let mut out = Vec::new();
            for (pi, p) in t.polys.iter().enumerate() {
                let n = p.vertices.len();
                for k in 0..n {
                    let (va, vb) = (p.vertices[k], p.vertices[(k + 1) % n]);
                    if (coord(va) - value).abs() < eps && (coord(vb) - value).abs() < eps {
                        out.push((poly_ref(ti, pi as u32), va, vb));
                    }
                }
            }
            out
        };
        let (ea, eb) = (edges(a), edges(b));
        let mut out = Vec::new();
        for &(pa, a0, a1) in &ea {
            for &(pb, b0, b1) in &eb {
                let (alo, ahi) = (along(a0).min(along(a1)), along(a0).max(along(a1)));
                let (blo, bhi) = (along(b0).min(along(b1)), along(b0).max(along(b1)));
                let (lo, hi) = (alo.max(blo), ahi.min(bhi));
                if hi - lo < eps {
                    continue;
                }
                let at = |p: Vec3, q: Vec3, s: f32| {
                    let t = ((s - along(p)) / (along(q) - along(p))).clamp(0.0, 1.0);
                    p.lerp(q, t)
                };
                let (pa0, pa1) = (at(a0, a1, lo), at(a0, a1, hi));
                let (pb0, pb1) = (at(b0, b1, lo), at(b0, b1, hi));
                if (pa0.y - pb0.y).abs() > climb || (pa1.y - pb1.y).abs() > climb {
                    continue;
                }
                out.push((
                    pa,
                    NavLink {
                        to: pb,
                        a: pa0,
                        b: pa1,
                    },
                ));
                out.push((
                    pb,
                    NavLink {
                        to: pa,
                        a: pb1,
                        b: pb0,
                    },
                ));
            }
        }
        out
    }

    /// Put rebuilt tiles in (replacing those at the same coordinates) and link them.
    pub fn replace_tiles(&mut self, tiles: Vec<Tile>) {
        let mut changed = Vec::new();
        for t in tiles {
            let i = match self.lookup.get(&(t.x, t.z)) {
                Some(&i) => {
                    self.tiles[i as usize] = Arc::new(t);
                    i
                }
                None => {
                    let i = self.tiles.len() as u32;
                    self.lookup.insert((t.x, t.z), i);
                    self.tiles.push(Arc::new(t));
                    i
                }
            };
            changed.push(i);
        }
        for &i in &changed {
            self.link_internal(i);
        }
        for &i in &changed {
            self.link_external(i);
        }
        self.snap_links();
    }

    /// Replace the off-mesh links (their ends are snapped onto the mesh).
    pub fn set_off_mesh_links(&mut self, links: Vec<OffMeshLink>) {
        self.off_links = links;
        self.snap_links();
    }

    /// Snap off-mesh link ends to the nearest polygons and index them.
    fn snap_links(&mut self) {
        let reach = (self.agent.radius * 2.0).max(0.5);
        let snapped: Vec<_> = self
            .off_links
            .iter()
            .map(|l| {
                let r = reach + l.width * 0.5;
                let s = self.nearest(l.start, r.max(self.agent.height * 0.5));
                let e = self.nearest(l.end, r.max(self.agent.height * 0.5));
                (s, e)
            })
            .collect();
        self.link_index.clear();
        for (i, (l, (s, e))) in self.off_links.iter_mut().zip(snapped).enumerate() {
            l.start_poly = s.map(|h| h.poly);
            l.end_poly = e.map(|h| h.poly);
            l.start_on_mesh = s.map_or(l.start, |h| h.position);
            l.end_on_mesh = e.map_or(l.end, |h| h.position);
            if let (Some(s), Some(e)) = (l.start_poly, l.end_poly) {
                self.link_index.entry(s).or_default().push((i as u32, true));
                if l.bidirectional {
                    self.link_index
                        .entry(e)
                        .or_default()
                        .push((i as u32, false));
                }
            }
        }
    }

    /// Off-mesh links leaving polygon `r`.
    pub fn links_from(&self, r: PolyRef) -> &[(u32, bool)] {
        self.link_index.get(&r).map_or(&[], |v| v.as_slice())
    }

    // ---- file ---------------------------------------------------------------------------

    /// `.navmesh` text: the settings, then one tile per line.
    pub fn to_json(&self) -> String {
        let header = serde_json::to_string(&Header {
            version: VERSION,
            agent: self.agent,
            cell_size: self.cell_size,
            build: self.settings,
        })
        .expect("header serialises");
        let tiles: Vec<String> = self
            .tiles
            .iter()
            .map(|t| {
                serde_json::to_string(&TileFile {
                    x: t.x,
                    z: t.z,
                    hash: t.hash,
                    polygons: t.polys.clone(),
                })
                .expect("tile serialises")
            })
            .collect();
        let head = header.strip_suffix('}').unwrap_or(&header);
        format!("{head},\"tiles\":[\n{}\n]}}\n", tiles.join(",\n"))
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let f: File = serde_json::from_str(text).map_err(|e| format!("invalid nav mesh: {e}"))?;
        if f.version > VERSION {
            return Err(format!(
                "nav mesh was saved by a newer version (format {})",
                f.version
            ));
        }
        let broken = |p: &NavPoly| {
            p.vertices.len() < 3
                || p.vertices.len() > 255
                || p.detail_tris
                    .iter()
                    .flatten()
                    .any(|&k| k as usize >= p.vertices.len() + p.detail_verts.len())
        };
        if f.version < 2 {
            // One tile of polygons with stored links (global indices = refs of tile 0).
            let n = f.polygons.len() as u32;
            if n > POLY_MASK
                || f.polygons
                    .iter()
                    .any(|p| broken(p) || p.links.iter().any(|l| l.to >= n))
            {
                return Err("invalid nav mesh: broken polygon data".into());
            }
            let polys = f
                .polygons
                .into_iter()
                .map(|mut p| {
                    if p.detail_tris.is_empty() {
                        let fan = NavPoly::new(p.vertices.clone(), p.area);
                        p.detail_tris = fan.detail_tris;
                    }
                    p
                })
                .collect();
            return Ok(Self::new(f.agent, f.cell_size, polys));
        }
        let mut tiles = Vec::new();
        for t in f.tiles {
            if t.polygons.len() > POLY_MASK as usize || t.polygons.iter().any(broken) {
                return Err("invalid nav mesh: broken polygon data".into());
            }
            let polys = t
                .polygons
                .into_iter()
                .map(|mut p| {
                    p.links.clear();
                    if p.detail_tris.is_empty() {
                        p.detail_tris = NavPoly::new(p.vertices.clone(), p.area).detail_tris;
                    }
                    p
                })
                .collect();
            tiles.push(Tile::new(t.x, t.z, polys, t.hash));
        }
        let mut build = f.build;
        build.agent = f.agent;
        build.cell_size = f.cell_size;
        Ok(Self::from_tiles(build, tiles))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, self.to_json()).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text)
    }
}

/// Whether `p` lies on segment a..b (x/z, small tolerance).
pub(crate) fn on_segment(p: Vec2, a: Vec2, b: Vec2) -> bool {
    let t = segment_t(a, b, p);
    a.lerp(b, t).distance_squared(p) < 1e-6
}

#[derive(Serialize)]
struct Header {
    version: u32,
    agent: AgentSettings,
    cell_size: f32,
    build: BuildSettings,
}

#[derive(Serialize, Deserialize)]
struct TileFile {
    x: i32,
    z: i32,
    #[serde(default)]
    hash: u64,
    polygons: Vec<NavPoly>,
}

#[derive(Deserialize)]
struct File {
    version: u32,
    agent: AgentSettings,
    cell_size: f32,
    #[serde(default)]
    build: BuildSettings,
    /// Format 1.
    #[serde(default)]
    polygons: Vec<NavPoly>,
    #[serde(default)]
    tiles: Vec<TileFile>,
}
