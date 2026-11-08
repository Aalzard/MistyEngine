//! Area filters and surface queries: raycasts along the mesh (`NavMesh.Raycast`) and the
//! nearest wall (`NavMesh.FindClosestEdge`).

use glam::{Vec2, Vec3, Vec3Swizzles};

use super::navmesh::{NavMesh, PolyRef, on_segment, segment_t};
use super::{ALL_AREAS, AREA_COUNT};

/// Which areas a query may use and what they cost.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QueryFilter {
    /// Bit per area.
    pub area_mask: u32,
    /// Cost per metre per area.
    pub costs: [f32; AREA_COUNT],
}

impl Default for QueryFilter {
    fn default() -> Self {
        Self {
            area_mask: ALL_AREAS,
            costs: [1.0; AREA_COUNT],
        }
    }
}

impl QueryFilter {
    pub fn new(area_mask: u32, costs: [f32; AREA_COUNT]) -> Self {
        Self { area_mask, costs }
    }

    pub fn passes(&self, area: u8) -> bool {
        (area as usize) < AREA_COUNT && self.area_mask & (1 << area) != 0
    }

    pub fn cost(&self, area: u8) -> f32 {
        self.costs
            .get(area as usize)
            .copied()
            .unwrap_or(1.0)
            .max(1e-3)
    }
}

/// Result of [`NavMesh::raycast`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaycastHit {
    /// A wall (or a filtered area) stopped the ray.
    pub hit: bool,
    /// Where it stopped (the end point when nothing was hit).
    pub position: Vec3,
    /// Wall normal on x/z (zero when nothing was hit).
    pub normal: Vec3,
    /// Distance travelled (x/z).
    pub distance: f32,
    /// Area of the polygon it stopped on.
    pub area: u8,
}

/// Result of [`NavMesh::closest_edge`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeHit {
    pub position: Vec3,
    /// Points away from the wall, towards the query point (x/z).
    pub normal: Vec3,
    pub distance: f32,
}

/// Uncovered parts of polygon edge a..b (parameters 0..1) given the portals lying on it.
fn wall_parts(a: Vec2, b: Vec2, portals: &[(Vec2, Vec2)]) -> Vec<(f32, f32)> {
    let mut covered: Vec<(f32, f32)> = portals
        .iter()
        .map(|(p, q)| {
            let (s, t) = (segment_t(a, b, *p), segment_t(a, b, *q));
            (s.min(t), s.max(t))
        })
        .collect();
    covered.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out = Vec::new();
    let mut at = 0.0;
    for (s, t) in covered {
        if s > at + 1e-4 {
            out.push((at, s));
        }
        at = at.max(t);
    }
    if at < 1.0 - 1e-4 {
        out.push((at, 1.0));
    }
    out
}

impl NavMesh {
    /// Portals of edge k of polygon `r` that lead to areas `filter` allows.
    fn edge_portals(
        &self,
        r: PolyRef,
        k: usize,
        filter: &QueryFilter,
    ) -> Vec<(Vec2, Vec2, PolyRef)> {
        let p = self.poly(r);
        let n = p.vertices.len();
        let (a, b) = (p.vertices[k].xz(), p.vertices[(k + 1) % n].xz());
        p.links
            .iter()
            .filter(|l| on_segment(l.a.xz(), a, b) && on_segment(l.b.xz(), a, b))
            .filter(|l| filter.passes(self.area_of(l.to)))
            .map(|l| (l.a.xz(), l.b.xz(), l.to))
            .collect()
    }

    /// Walk the surface from `start` straight towards `end` (x/z) until a wall or an area the
    /// filter excludes; like `NavMesh.Raycast`.
    pub fn raycast(&self, start: Vec3, end: Vec3, filter: &QueryFilter) -> Option<RaycastHit> {
        self.raycast_walk(start, end, filter).map(|(h, _)| h)
    }

    /// [`NavMesh::raycast`] plus the polygons it crossed.
    pub(super) fn raycast_walk(
        &self,
        start: Vec3,
        end: Vec3,
        filter: &QueryFilter,
    ) -> Option<(RaycastHit, Vec<PolyRef>)> {
        let reach = self.agent.height.max(self.cell_size * 4.0);
        let s = self.nearest_filtered(start, reach, filter)?;
        let (sp, ep) = (s.position.xz(), end.xz());
        let dir = ep - sp;
        let mut cur = s.poly;
        let mut polys = Vec::new();
        loop {
            polys.push(cur);
            let visited = polys.len();
            let p = self.poly(cur);
            let v = &p.vertices;
            let n = v.len();
            // Where the segment leaves this convex polygon (largest entering-free parameter).
            let mut t_exit = 1.0f32;
            let mut edge = None;
            for k in 0..n {
                let (a, b) = (v[k].xz(), v[(k + 1) % n].xz());
                let e = b - a;
                // Inside is to the left of each edge (counter-clockwise).
                let num = e.perp_dot(sp - a);
                let den = e.perp_dot(dir);
                if den < -1e-9 {
                    // Leaving through this edge's line.
                    let t = -num / den;
                    if t < t_exit {
                        t_exit = t;
                        edge = Some(k);
                    }
                }
            }
            let Some(k) = edge else {
                let h = self.height(cur, ep.x, ep.y);
                let hit = RaycastHit {
                    hit: false,
                    position: Vec3::new(ep.x, h, ep.y),
                    normal: Vec3::ZERO,
                    distance: dir.length(),
                    area: p.area,
                };
                return Some((hit, polys));
            };
            let t_exit = t_exit.max(0.0);
            let x = sp + dir * t_exit;
            let next = self
                .edge_portals(cur, k, filter)
                .into_iter()
                .find(|(a, b, _)| {
                    let t = segment_t(*a, *b, x);
                    a.lerp(*b, t).distance_squared(x) < 1e-5
                })
                .map(|(_, _, to)| to);
            match next {
                Some(to) if visited < 4096 => cur = to,
                _ => {
                    let (a, b) = (v[k].xz(), v[(k + 1) % n].xz());
                    let e = (b - a).normalize_or_zero();
                    let normal = Vec2::new(-e.y, e.x);
                    let hit = RaycastHit {
                        hit: true,
                        position: Vec3::new(x.x, self.height(cur, x.x, x.y), x.y),
                        normal: Vec3::new(normal.x, 0.0, normal.y),
                        distance: (x - sp).length(),
                        area: p.area,
                    };
                    return Some((hit, polys));
                }
            }
        }
    }

    /// The nearest wall to `p` (edges with nothing walkable behind them); like
    /// `NavMesh.FindClosestEdge`.
    pub fn closest_edge(&self, p: Vec3, filter: &QueryFilter) -> Option<EdgeHit> {
        let reach = self.agent.height.max(self.cell_size * 4.0);
        let s = self.nearest_filtered(p, reach, filter)?;
        let q = s.position.xz();
        let mut best: Option<(f32, Vec2, Vec2)> = None;
        let mut queue = vec![s.poly];
        let mut i = 0;
        while i < queue.len() && queue.len() < 4096 {
            let cur = queue[i];
            i += 1;
            // Polygons further than the best wall can't hold a nearer one.
            let (lo, hi) = self.bounds(cur);
            let gap = (lo.xz() - q).max(q - hi.xz()).max(Vec2::ZERO).length();
            if best.is_some_and(|b| gap > b.0) {
                continue;
            }
            let poly = self.poly(cur);
            let n = poly.vertices.len();
            for k in 0..n {
                let (a, b) = (poly.vertices[k].xz(), poly.vertices[(k + 1) % n].xz());
                let portals = self.edge_portals(cur, k, filter);
                for (_, _, to) in &portals {
                    if !queue.contains(to) {
                        queue.push(*to);
                    }
                }
                let spans: Vec<(Vec2, Vec2)> = portals.iter().map(|(x, y, _)| (*x, *y)).collect();
                for (t0, t1) in wall_parts(a, b, &spans) {
                    let (wa, wb) = (a.lerp(b, t0), a.lerp(b, t1));
                    let t = segment_t(wa, wb, q);
                    let c = wa.lerp(wb, t);
                    let d = c.distance(q);
                    if best.is_none_or(|bb| d < bb.0) {
                        let e = (b - a).normalize_or_zero();
                        best = Some((d, c, Vec2::new(-e.y, e.x)));
                    }
                }
            }
        }
        let (d, c, normal) = best?;
        let r = self
            .locate(Vec3::new(c.x, s.position.y, c.y), self.agent.height)
            .unwrap_or(s.poly);
        Some(EdgeHit {
            position: Vec3::new(c.x, self.height(r, c.x, c.y), c.y),
            normal: Vec3::new(normal.x, 0.0, normal.y),
            distance: d,
        })
    }
}
