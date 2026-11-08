//! Path queries: A* over polygons (and off-mesh links) with per-area costs and an area mask,
//! then string pulling per stretch between links. Unreachable targets give a partial path to
//! the reachable polygon closest to them.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use glam::{Vec2, Vec3, Vec3Swizzles};

use super::funnel;
use super::navmesh::{NavMesh, PolyRef, cross2, segment_t};
use super::query::QueryFilter;

/// Like the familiar NavMeshPathStatus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PathStatus {
    #[default]
    Complete,
    /// The target can't be reached; the path ends as close as possible.
    Partial,
    /// No path (start or end not on the mesh).
    Invalid,
}

impl PathStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Complete => "PathComplete",
            Self::Partial => "PathPartial",
            Self::Invalid => "PathInvalid",
        }
    }
}

/// Crossing off-mesh link `link` (forward = start to end) from this corner to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkStep {
    pub link: u32,
    pub forward: bool,
}

/// Corner points from start to end and the polygons walked through.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NavPath {
    pub corners: Vec<Vec3>,
    /// Same length as `corners`: Some when the stretch from this corner to the next is an
    /// off-mesh link.
    pub links: Vec<Option<LinkStep>>,
    pub polys: Vec<PolyRef>,
    pub status: PathStatus,
}

impl NavPath {
    pub fn invalid() -> Self {
        Self {
            status: PathStatus::Invalid,
            ..Default::default()
        }
    }

    /// Length along the corners.
    pub fn length(&self) -> f32 {
        self.corners.windows(2).map(|w| w[0].distance(w[1])).sum()
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Open {
    f: f32,
    poly: PolyRef,
}

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        // Min-heap on f.
        o.f.total_cmp(&self.f).then(o.poly.cmp(&self.poly))
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// How a polygon was reached.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Via {
    Start,
    Portal,
    Link(LinkStep),
}

#[derive(Clone, Copy)]
struct Node {
    g: f32,
    pos: Vec3,
    parent: Option<PolyRef>,
    via: Via,
}

/// The point of portal a..b a taut path from `from` to `goal` most likely crosses.
fn portal_point(a: Vec3, b: Vec3, from: Vec3, goal: Vec3) -> Vec3 {
    let (pa, pb, f, g) = (a.xz(), b.xz(), from.xz(), goal.xz());
    let d = g - f;
    let e = pb - pa;
    let den = d.perp_dot(e);
    if den.abs() > 1e-9 {
        let t = (pa - f).perp_dot(d) / den;
        let s = (pa - f).perp_dot(e) / den;
        if (0.0..=1.0).contains(&t) && s >= 0.0 {
            return a.lerp(b, t);
        }
    }
    let cost = |p: Vec2| p.distance(f) + p.distance(g);
    let t = segment_t(pa, pb, (f + g) * 0.5);
    [(0.0, cost(pa)), (1.0, cost(pb)), (t, cost(pa.lerp(pb, t)))]
        .into_iter()
        .min_by(|x, y| x.1.total_cmp(&y.1))
        .map_or((a + b) * 0.5, |(t, _)| a.lerp(b, t))
}

impl NavMesh {
    /// Snap a point onto the mesh (horizontally a few cells, vertically the agent height).
    pub fn snap(&self, p: Vec3, filter: &QueryFilter) -> Option<(PolyRef, Vec3)> {
        let reach = self.agent.height.max(self.cell_size * 4.0);
        self.nearest_filtered(p, reach, filter)
            .map(|h| (h.poly, h.position))
    }

    /// Shortest path from `start` to `end` on any area at cost 1.
    pub fn find_path(&self, start: Vec3, end: Vec3) -> NavPath {
        self.find_path_filtered(start, end, &QueryFilter::default())
    }

    /// Cheapest path from `start` to `end` (both snapped onto the mesh) over the areas
    /// `filter` allows.
    pub fn find_path_filtered(&self, start: Vec3, end: Vec3, filter: &QueryFilter) -> NavPath {
        let (Some((sp, s)), end_hit) = (self.snap(start, filter), self.snap(end, filter)) else {
            return NavPath::invalid();
        };
        // An end off the mesh still steers towards the closest reachable point.
        let (ep, e) = end_hit.unwrap_or_else(|| {
            self.nearest_filtered(end, f32::INFINITY, filter)
                .map_or((sp, s), |h| (h.poly, h.position))
        });
        let (chain, reached) = self.search(sp, s, ep, e, filter);
        let last = chain.last().expect("start polygon").0;
        let target = if reached {
            e
        } else {
            self.closest_point(last, e)
        };
        let mut path = self.straighten(&chain, s, target);
        self.shortcut(&mut path, filter);
        path.status = if reached && end_hit.is_some() {
            PathStatus::Complete
        } else {
            PathStatus::Partial
        };
        path
    }

    /// A* from polygon `sp` (at `s`) to `ep`; returns the polygon chain (with how each was
    /// reached) and whether it reached `ep` (else it ends at the explored polygon closest to
    /// `e`).
    fn search(
        &self,
        sp: PolyRef,
        s: Vec3,
        ep: PolyRef,
        e: Vec3,
        filter: &QueryFilter,
    ) -> (Vec<(PolyRef, Via)>, bool) {
        let mut nodes: HashMap<PolyRef, Node> = HashMap::from([(
            sp,
            Node {
                g: 0.0,
                pos: s,
                parent: None,
                via: Via::Start,
            },
        )]);
        let mut open = BinaryHeap::from([Open {
            f: s.distance(e),
            poly: sp,
        }]);
        let mut closed: std::collections::HashSet<PolyRef> = Default::default();
        let mut best = (s.distance(e), sp);
        let mut reached = sp == ep;
        while let Some(Open { poly, .. }) = open.pop() {
            if poly == ep {
                reached = true;
                break;
            }
            if !closed.insert(poly) {
                continue;
            }
            let node = nodes[&poly];
            let here = filter.cost(self.area_of(poly));
            let mut relax = |to: PolyRef, g: f32, pos: Vec3, via: Via| {
                if nodes.get(&to).is_some_and(|n| n.g <= g) {
                    return;
                }
                nodes.insert(
                    to,
                    Node {
                        g,
                        pos,
                        parent: Some(poly),
                        via,
                    },
                );
                let h = pos.distance(e);
                if h < best.0 {
                    best = (h, to);
                }
                open.push(Open { f: g + h, poly: to });
            };
            for l in &self.poly(poly).links {
                let area = self.area_of(l.to);
                if !filter.passes(area) {
                    continue;
                }
                let x = portal_point(l.a, l.b, node.pos, e);
                let mut g = node.g + node.pos.distance(x) * here;
                // The last step goes straight to the target point.
                if l.to == ep {
                    g += x.distance(e) * filter.cost(area);
                }
                relax(l.to, g, x, Via::Portal);
            }
            for &(li, forward) in self.links_from(poly) {
                let link = &self.off_links[li as usize];
                if !filter.passes(link.area) || !link.connected() {
                    continue;
                }
                let (from, to, _, to_poly) = link.ends(forward);
                let area = self.area_of(to_poly);
                if !filter.passes(area) {
                    continue;
                }
                let factor = if link.cost_modifier >= 0.0 {
                    link.cost_modifier
                } else {
                    filter.cost(link.area)
                };
                let mut g = node.g + node.pos.distance(from) * here + from.distance(to) * factor;
                if to_poly == ep {
                    g += to.distance(e) * filter.cost(area);
                }
                relax(to_poly, g, to, Via::Link(LinkStep { link: li, forward }));
            }
        }
        let mut chain = Vec::new();
        let mut cur = if reached { ep } else { best.1 };
        loop {
            let n = nodes[&cur];
            chain.push((cur, n.via));
            match n.parent {
                Some(p) if chain.len() < 100_000 => cur = p,
                _ => break,
            }
        }
        chain.reverse();
        (chain, reached)
    }

    /// Corners along a polygon chain: string pulled between off-mesh links.
    fn straighten(&self, chain: &[(PolyRef, Via)], s: Vec3, e: Vec3) -> NavPath {
        let mut path = NavPath {
            polys: chain.iter().map(|c| c.0).collect(),
            ..Default::default()
        };
        let mut seg_start = s;
        let mut seg: Vec<PolyRef> = Vec::new();
        let push = |path: &mut NavPath, p: Vec3, link: Option<LinkStep>| {
            if let Some(last) = path.corners.last()
                && last.distance_squared(p) < 1e-10
                && path.links.last() == Some(&None)
            {
                *path.links.last_mut().expect("links") = link;
                return;
            }
            path.corners.push(p);
            path.links.push(link);
        };
        for (i, &(poly, via)) in chain.iter().enumerate() {
            if let Via::Link(step) = via {
                let (from, to, _, _) = self.off_links[step.link as usize].ends(step.forward);
                for c in funnel::pull(&self.portals(&seg, seg_start, from)) {
                    push(&mut path, c, None);
                }
                *path.links.last_mut().expect("corner") = Some(step);
                seg_start = to;
                seg.clear();
            }
            seg.push(poly);
            if i + 1 == chain.len() {
                for c in funnel::pull(&self.portals(&seg, seg_start, e)) {
                    push(&mut path, c, None);
                }
            }
        }
        path
    }

    /// Skip corners the surface can see past (A* over large polygons can pick a staircase of
    /// them where a straight line fits), never through areas dearer than the path's cheapest
    /// and never across off-mesh links.
    fn shortcut(&self, path: &mut NavPath, filter: &QueryFilter) {
        let n = path.corners.len();
        if n < 3 {
            return;
        }
        let cheapest = path
            .polys
            .iter()
            .map(|&p| filter.cost(self.area_of(p)))
            .fold(f32::INFINITY, f32::min);
        let mut keep = vec![0usize];
        let mut i = 0;
        while i + 1 < n {
            // The last corner reachable from i without passing a link.
            let mut limit = i + 1;
            while limit < n - 1 && path.links[limit].is_none() && path.links[i].is_none() {
                limit += 1;
            }
            let mut next = i + 1;
            if path.links[i].is_none() {
                for j in (i + 2..=limit).rev() {
                    let clear = self
                        .raycast_walk(path.corners[i], path.corners[j], filter)
                        .is_some_and(|(h, polys)| {
                            !h.hit
                                && polys
                                    .iter()
                                    .all(|&p| filter.cost(self.area_of(p)) <= cheapest + 1e-4)
                        });
                    if clear {
                        next = j;
                        break;
                    }
                }
            }
            keep.push(next);
            i = next;
        }
        path.corners = keep.iter().map(|&k| path.corners[k]).collect();
        path.links = keep.iter().map(|&k| path.links[k]).collect();
    }

    /// (left, right) portals along `polys`, framed by the start and end points.
    pub(super) fn portals(&self, polys: &[PolyRef], s: Vec3, e: Vec3) -> Vec<(Vec3, Vec3)> {
        let mut out = vec![(s, s)];
        for w in polys.windows(2) {
            let Some(l) = self.poly(w[0]).links.iter().find(|l| l.to == w[1]) else {
                continue;
            };
            // Seen walking from this polygon into the next, which end is on the left?
            let (from, to) = (self.center(w[0]).xz(), self.center(w[1]).xz());
            // cross2 > 0 = left of from → to (x right, z forward).
            let a_left = cross2(from, to, l.a.xz()) > cross2(from, to, l.b.xz());
            out.push(if a_left { (l.a, l.b) } else { (l.b, l.a) });
        }
        out.push((e, e));
        out
    }
}
