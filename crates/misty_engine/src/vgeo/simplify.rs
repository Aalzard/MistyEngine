//! Quadric-error edge-collapse simplification with locked vertices.
//!
//! Half-edge collapses only: a vertex moves onto a neighbour, so the result uses a subset of
//! the input's vertices (every level of a virtual mesh indexes the source vertex buffer).
//! Topology works on welded position ids; a position with several vertices (a UV or normal
//! seam) is locked, so attributes never smear across a seam.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use glam::{DVec3, Vec3};

/// Symmetric 4×4 plane quadric: sum of w · (n·p + d)².
#[derive(Clone, Copy, Default)]
struct Quadric {
    a: [f64; 10],
    weight: f64,
}

impl Quadric {
    fn plane(n: DVec3, d: f64, w: f64) -> Self {
        Self {
            a: [
                n.x * n.x * w,
                n.x * n.y * w,
                n.x * n.z * w,
                n.x * d * w,
                n.y * n.y * w,
                n.y * n.z * w,
                n.y * d * w,
                n.z * n.z * w,
                n.z * d * w,
                d * d * w,
            ],
            weight: w,
        }
    }

    fn add(&mut self, o: &Self) {
        for (a, b) in self.a.iter_mut().zip(o.a) {
            *a += b;
        }
        self.weight += o.weight;
    }

    /// Mean squared distance of `p` to the planes (weighted).
    fn eval(&self, p: DVec3) -> f64 {
        let a = &self.a;
        let s = a[0] * p.x * p.x
            + 2.0 * a[1] * p.x * p.y
            + 2.0 * a[2] * p.x * p.z
            + 2.0 * a[3] * p.x
            + a[4] * p.y * p.y
            + 2.0 * a[5] * p.y * p.z
            + 2.0 * a[6] * p.y
            + a[7] * p.z * p.z
            + 2.0 * a[8] * p.z
            + a[9];
        (s / self.weight.max(1e-30)).max(0.0)
    }
}

struct Candidate {
    cost: f64,
    from: u32,
    to: u32,
    stamp: (u32, u32),
}

impl PartialEq for Candidate {
    fn eq(&self, o: &Self) -> bool {
        self.cost == o.cost
    }
}
impl Eq for Candidate {}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Candidate {
    // Min-heap on cost.
    fn cmp(&self, o: &Self) -> Ordering {
        o.cost.total_cmp(&self.cost)
    }
}

pub(super) struct Simplified {
    /// Triangles left (vertex ids of the input).
    pub tris: Vec<[u32; 3]>,
    /// Largest distance estimate of a collapse (object units).
    pub error: f32,
}

/// Collapse edges of `tris` until at most `target` triangles are left (or nothing more can go
/// without folding the surface), never moving a position for which `locked` is true.
pub(super) fn simplify(
    tris: &[[u32; 3]],
    pos: &[Vec3],
    weld: &[u32],
    locked: impl Fn(u32) -> bool,
    target: usize,
) -> Simplified {
    // Local positions.
    let mut local: HashMap<u32, u32> = HashMap::new();
    let mut world: Vec<u32> = Vec::new();
    let mut corners: Vec<[u32; 3]> = Vec::with_capacity(tris.len());
    for t in tris {
        let mut c = [0; 3];
        for k in 0..3 {
            let w = weld[t[k] as usize];
            c[k] = *local.entry(w).or_insert_with(|| {
                world.push(w);
                world.len() as u32 - 1
            });
        }
        corners.push(c);
    }
    let n = world.len();
    // The vertex (not position) each local position uses; several = seam = locked.
    let mut vertex = vec![u32::MAX; n];
    let mut lock: Vec<bool> = world.iter().map(|&w| locked(w)).collect();
    for (t, c) in tris.iter().zip(&corners) {
        for k in 0..3 {
            let l = c[k] as usize;
            if vertex[l] == u32::MAX {
                vertex[l] = t[k];
            } else if vertex[l] != t[k] {
                lock[l] = true;
            }
        }
    }
    // Each corner's vertex (a collapse gives the moved corners the vertex of the target).
    let mut cv: Vec<[u32; 3]> = tris.to_vec();
    let mut p: Vec<DVec3> = vec![DVec3::ZERO; n];
    for (t, c) in tris.iter().zip(&corners) {
        for k in 0..3 {
            p[c[k] as usize] = pos[t[k] as usize].as_dvec3();
        }
    }
    let mut quadric = vec![Quadric::default(); n];
    let mut of: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (i, c) in corners.iter().enumerate() {
        let (a, b, d) = (p[c[0] as usize], p[c[1] as usize], p[c[2] as usize]);
        let cross = (b - a).cross(d - a);
        let area = cross.length() * 0.5;
        if area > 0.0 {
            let normal = cross / (2.0 * area);
            let q = Quadric::plane(normal, -normal.dot(a), area);
            for &l in c {
                quadric[l as usize].add(&q);
            }
        }
        for &l in c {
            of[l as usize].push(i as u32);
        }
    }
    let mut alive_tri = vec![true; corners.len()];
    let mut alive = vec![true; n];
    let mut stamp = vec![0u32; n];
    let mut heap = BinaryHeap::new();
    let cost = |q: &[Quadric], from: usize, to: usize, p: &[DVec3]| {
        let mut sum = q[from];
        sum.add(&q[to]);
        sum.eval(p[to])
    };
    for c in &corners {
        for k in 0..3 {
            let (a, b) = (c[k] as usize, c[(k + 1) % 3] as usize);
            for (from, to) in [(a, b), (b, a)] {
                if !lock[from] {
                    heap.push(Candidate {
                        cost: cost(&quadric, from, to, &p),
                        from: from as u32,
                        to: to as u32,
                        stamp: (0, 0),
                    });
                }
            }
        }
    }

    let mut count = corners.len();
    let mut error: f64 = 0.0;
    let mut ring_a: Vec<u32> = Vec::new();
    let mut ring_b: Vec<u32> = Vec::new();
    while count > target {
        let Some(cand) = heap.pop() else {
            break;
        };
        let (a, b) = (cand.from as usize, cand.to as usize);
        if !alive[a] || !alive[b] {
            continue;
        }
        if cand.stamp != (stamp[a], stamp[b]) {
            // Stale: re-queue with the current cost if the edge still exists.
            if of[a]
                .iter()
                .any(|&t| corners[t as usize].contains(&(b as u32)))
            {
                heap.push(Candidate {
                    cost: cost(&quadric, a, b, &p),
                    stamp: (stamp[a], stamp[b]),
                    ..cand
                });
            }
            continue;
        }
        // Triangles on the edge, and the vertex `b` has in them.
        of[a].retain(|&t| alive_tri[t as usize]);
        of[b].retain(|&t| alive_tri[t as usize]);
        let shared: Vec<u32> = of[a]
            .iter()
            .copied()
            .filter(|&t| corners[t as usize].contains(&(b as u32)))
            .collect();
        if shared.is_empty() {
            continue;
        }
        // Link condition: the edge's two rings may only meet at the triangles on the edge.
        ring_a.clear();
        ring_b.clear();
        for (l, ring) in [(a, &mut ring_a), (b, &mut ring_b)] {
            for &t in &of[l] {
                for &m in &corners[t as usize] {
                    if m as usize != l && !ring.contains(&m) {
                        ring.push(m);
                    }
                }
            }
        }
        let common = ring_a.iter().filter(|m| ring_b.contains(m)).count();
        if common != shared.len() {
            continue;
        }
        // Two border vertices may already be joined by an edge outside this set of triangles
        // (in the neighbouring group): never join them here, or that edge gets four triangles.
        if lock[b]
            && ring_a
                .iter()
                .any(|&m| m as usize != b && lock[m as usize] && !ring_b.contains(&m))
        {
            continue;
        }
        // No folds: the triangles that stay must keep facing the same way.
        let folds = of[a].iter().any(|&t| {
            let c = corners[t as usize];
            if c.contains(&(b as u32)) {
                return false;
            }
            let q = |l: u32| p[l as usize];
            let before = (q(c[1]) - q(c[0])).cross(q(c[2]) - q(c[0]));
            let moved = |l: u32| if l as usize == a { p[b] } else { q(l) };
            let after = (moved(c[1]) - moved(c[0])).cross(moved(c[2]) - moved(c[0]));
            let (lb, la) = (before.length(), after.length());
            la <= 1e-12 * (1.0 + lb) || before.dot(after) < 0.2 * lb * la
        });
        if folds {
            continue;
        }
        error = error.max(cand.cost);
        // The vertex `b` has on this side (`a` is no seam, so all its triangles are one
        // wedge and both triangles on the edge see the same vertex of `b`).
        let to_vertex = {
            let t = shared[0] as usize;
            let k = corners[t]
                .iter()
                .position(|&m| m as usize == b)
                .expect("on edge");
            cv[t][k]
        };
        let moved: Vec<u32> = std::mem::take(&mut of[a]);
        for t in moved {
            let ti = t as usize;
            if !alive_tri[ti] {
                continue;
            }
            if corners[ti].contains(&(b as u32)) {
                alive_tri[ti] = false;
                count -= 1;
                continue;
            }
            for k in 0..3 {
                if corners[ti][k] as usize == a {
                    corners[ti][k] = b as u32;
                    cv[ti][k] = to_vertex;
                }
            }
            of[b].push(t);
        }
        alive[a] = false;
        let qa = quadric[a];
        quadric[b].add(&qa);
        stamp[b] += 1;
        of[b].retain(|&t| alive_tri[t as usize]);
        // New candidates around `b`.
        let around: Vec<u32> = of[b]
            .iter()
            .flat_map(|&t| corners[t as usize])
            .filter(|&m| m as usize != b)
            .collect();
        for m in around {
            let m = m as usize;
            of[m].retain(|&t| alive_tri[t as usize]);
            for (from, to) in [(b, m), (m, b)] {
                if !lock[from] {
                    heap.push(Candidate {
                        cost: cost(&quadric, from, to, &p),
                        from: from as u32,
                        to: to as u32,
                        stamp: (stamp[from], stamp[to]),
                    });
                }
            }
        }
    }

    let out = cv
        .iter()
        .zip(&alive_tri)
        .filter(|(_, a)| **a)
        .map(|(c, _)| *c)
        .collect();
    Simplified {
        tris: out,
        error: error.sqrt() as f32,
    }
}
