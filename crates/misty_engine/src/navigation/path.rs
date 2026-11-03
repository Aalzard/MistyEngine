//! Path queries: A* over polygons (through portal midpoints), then string pulling.
//! Unreachable targets give a partial path to the reachable polygon closest to them.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use glam::{Vec3, Vec3Swizzles};

use super::funnel;
use super::navmesh::{NavMesh, cross2};

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

/// Corner points from start to end and the polygons walked through.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NavPath {
    pub corners: Vec<Vec3>,
    pub polys: Vec<u32>,
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
    poly: u32,
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

/// How far from the mesh the start and end may be (horizontally a few cells, vertically the
/// agent height).
fn snap(mesh: &NavMesh, p: Vec3) -> Option<(u32, Vec3)> {
    let reach = (mesh.agent.height).max(mesh.cell_size * 4.0);
    mesh.nearest(p, reach).map(|h| (h.poly, h.position))
}

impl NavMesh {
    /// Shortest path from `start` to `end` (both snapped onto the mesh).
    pub fn find_path(&self, start: Vec3, end: Vec3) -> NavPath {
        let (Some((sp, s)), end_hit) = (snap(self, start), snap(self, end)) else {
            return NavPath::invalid();
        };
        // An end off the mesh still steers towards the closest reachable point.
        let (ep, e) = end_hit.unwrap_or_else(|| {
            self.nearest(end, f32::INFINITY)
                .map_or((sp, s), |h| (h.poly, h.position))
        });
        let (polys, reached) = self.search(sp, s, ep, e);
        let last = *polys.last().expect("start polygon");
        let target = if reached {
            e
        } else {
            self.closest_point(last, e)
        };
        let corners = funnel::pull(&self.portals(&polys, s, target));
        let status = if reached && end_hit.is_some() {
            PathStatus::Complete
        } else {
            PathStatus::Partial
        };
        NavPath {
            corners,
            polys,
            status,
        }
    }

    /// A* from polygon `sp` (at `s`) to `ep`; returns the polygon chain and whether it reached
    /// `ep` (else it ends at the explored polygon closest to `e`).
    fn search(&self, sp: u32, s: Vec3, ep: u32, e: Vec3) -> (Vec<u32>, bool) {
        let mut pos: HashMap<u32, Vec3> = HashMap::from([(sp, s)]);
        let mut cost: HashMap<u32, f32> = HashMap::from([(sp, 0.0)]);
        let mut parent: HashMap<u32, u32> = HashMap::new();
        let mut open = BinaryHeap::from([Open {
            f: s.distance(e),
            poly: sp,
        }]);
        let mut best = (s.distance(e), sp);
        let mut reached = false;
        while let Some(Open { poly, .. }) = open.pop() {
            if poly == ep {
                reached = true;
                break;
            }
            let (g, p) = (cost[&poly], pos[&poly]);
            for l in &self.polys[poly as usize].links {
                let mid = (l.a + l.b) * 0.5;
                // The last step goes straight to the target point.
                let step = if l.to == ep {
                    p.distance(mid) + mid.distance(e)
                } else {
                    p.distance(mid)
                };
                let ng = g + step;
                if cost.get(&l.to).is_some_and(|c| *c <= ng) {
                    continue;
                }
                cost.insert(l.to, ng);
                pos.insert(l.to, mid);
                parent.insert(l.to, poly);
                let h = mid.distance(e);
                if h < best.0 {
                    best = (h, l.to);
                }
                open.push(Open {
                    f: ng + h,
                    poly: l.to,
                });
            }
        }
        let mut chain = vec![if reached { ep } else { best.1 }];
        while let Some(p) = parent.get(chain.last().expect("chain")) {
            chain.push(*p);
        }
        chain.reverse();
        (chain, reached)
    }

    /// (left, right) portals along `polys`, framed by the start and end points.
    fn portals(&self, polys: &[u32], s: Vec3, e: Vec3) -> Vec<(Vec3, Vec3)> {
        let mut out = vec![(s, s)];
        for w in polys.windows(2) {
            let Some(l) = self.polys[w[0] as usize]
                .links
                .iter()
                .find(|l| l.to == w[1])
            else {
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
