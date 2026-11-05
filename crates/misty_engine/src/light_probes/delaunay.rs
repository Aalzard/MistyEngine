//! 3D Delaunay tetrahedralization of probe positions (Bowyer–Watson, incremental).
//!
//! Points go into a huge enclosing tetrahedron one by one: the tetrahedra whose circumsphere
//! holds the new point (a connected cavity grown from the one containing it) are removed and the
//! cavity's boundary faces are joined to the point. Tetrahedra touching the enclosing vertices
//! are dropped at the end; the faces left without a neighbour form the hull.
//!
//! Probe layouts are usually grids, where many points share a sphere and the Delaunay
//! tetrahedralization is not unique. A tiny deterministic jitter (1e-5 of the layout size)
//! breaks those ties, so the topology is built on [`Tetrahedralization::jittered`] positions in
//! f64 while interpolation weights use the real positions (see `field`).

use std::collections::HashMap;

use glam::{DVec3, Vec3};

/// "No neighbour" (a hull face).
pub const NONE: u32 = u32::MAX;

/// The face opposite vertex `i` of a tetrahedron, as vertex slots.
pub const FACES: [[usize; 3]; 4] = [[1, 2, 3], [0, 3, 2], [0, 1, 3], [0, 2, 1]];

/// Relative size of the tie-breaking jitter.
const JITTER: f64 = 1e-5;
/// Enclosing tetrahedron size relative to the layout. Tetrahedra whose circumsphere reaches an
/// enclosing vertex are lost with it, which can leave the hull short of convex by about
/// size / (2·SUPER) (2000th of the size) along flat stretches; points there count as outside and use the nearest hull
/// face, which is continuous with the inside. Much larger values cost f64 precision.
const SUPER: f64 = 1e3;

/// Tetrahedra over a point set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tetrahedralization {
    /// Vertex indices, positively oriented on the jittered positions.
    pub tets: Vec<[u32; 4]>,
    /// Neighbour across the face opposite each vertex ([`NONE`] on the hull).
    pub neighbors: Vec<[u32; 4]>,
    /// Boundary triangles, wound so their normal points out.
    pub hull: Vec<[u32; 3]>,
    /// The positions the topology was built on.
    pub jittered: Vec<DVec3>,
}

/// Six times the signed volume of `abcd` (> 0 when `d` is on the side `(b-a)×(c-a)` points to).
pub fn orient(a: DVec3, b: DVec3, c: DVec3, d: DVec3) -> f64 {
    (b - a).cross(c - a).dot(d - a)
}

fn circumsphere(a: DVec3, b: DVec3, c: DVec3, d: DVec3) -> (DVec3, f64) {
    let (b, c, d) = (b - a, c - a, d - a);
    let den = 2.0 * b.dot(c.cross(d));
    if den.abs() < 1e-300 {
        return (a, f64::INFINITY);
    }
    let o = (c.cross(d) * b.length_squared()
        + d.cross(b) * c.length_squared()
        + b.cross(c) * d.length_squared())
        / den;
    (a + o, o.length_squared())
}

/// Deterministic offset in [-1, 1]³ per point.
fn jitter(i: usize) -> DVec3 {
    let h = |k: u64| {
        let mut x = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ k.wrapping_mul(0xBF58_476D);
        x ^= x >> 31;
        x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^= x >> 29;
        (x >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    };
    DVec3::new(h(1), h(2), h(3))
}

struct Builder {
    pts: Vec<DVec3>,
    tets: Vec<[u32; 4]>,
    nb: Vec<[u32; 4]>,
    sphere: Vec<(DVec3, f64)>,
    alive: Vec<bool>,
    mark: Vec<u32>,
    stamp: u32,
    last: u32,
}

impl Builder {
    fn p(&self, i: u32) -> DVec3 {
        self.pts[i as usize]
    }

    fn add(&mut self, mut v: [u32; 4]) -> u32 {
        let o = orient(self.p(v[0]), self.p(v[1]), self.p(v[2]), self.p(v[3]));
        if o < 0.0 {
            v.swap(0, 1);
        }
        let s = circumsphere(self.p(v[0]), self.p(v[1]), self.p(v[2]), self.p(v[3]));
        self.tets.push(v);
        self.nb.push([NONE; 4]);
        self.sphere.push(s);
        self.alive.push(true);
        self.mark.push(0);
        (self.tets.len() - 1) as u32
    }

    fn in_sphere(&self, t: u32, p: DVec3) -> bool {
        let (c, r2) = self.sphere[t as usize];
        c.distance_squared(p) < r2
    }

    /// A live tetrahedron containing `p` (walking from the last one made).
    fn locate(&self, p: DVec3) -> Option<u32> {
        let mut t = self.last;
        if !self.alive.get(t as usize).copied().unwrap_or(false) {
            t = self.alive.iter().position(|a| *a)? as u32;
        }
        let limit = 64 + self.tets.len() * 4;
        'walk: for step in 0..limit {
            let v = self.tets[t as usize];
            for k in 0..4 {
                // Rotate the first face tried so the walk can't cycle forever.
                let i = (k + step) % 4;
                let mut q = v.map(|x| self.p(x));
                q[i] = p;
                if orient(q[0], q[1], q[2], q[3]) < 0.0 {
                    let n = self.nb[t as usize][i];
                    if n == NONE {
                        break 'walk;
                    }
                    t = n;
                    continue 'walk;
                }
            }
            return Some(t);
        }
        (0..self.tets.len() as u32).find(|&t| self.alive[t as usize] && self.in_sphere(t, p))
    }

    fn insert(&mut self, pi: u32) {
        let p = self.p(pi);
        let Some(seed) = self.locate(p) else {
            return;
        };
        self.stamp += 1;
        let mut stamp = self.stamp;
        self.mark[seed as usize] = stamp;
        let mut bad = vec![seed];
        let mut stack = vec![seed];
        while let Some(t) = stack.pop() {
            for i in 0..4 {
                let n = self.nb[t as usize][i];
                if n != NONE && self.mark[n as usize] != stamp && self.in_sphere(n, p) {
                    self.mark[n as usize] = stamp;
                    bad.push(n);
                    stack.push(n);
                }
            }
        }
        // Inexact insphere tests can make a cavity the point doesn't see all of: drop the
        // tetrahedra behind boundary faces it is not strictly in front of, keep it connected.
        loop {
            let mut culprit = None;
            'scan: for &t in &bad {
                if t == seed {
                    continue;
                }
                let v = self.tets[t as usize];
                for (i, face) in FACES.iter().enumerate() {
                    let n = self.nb[t as usize][i];
                    if n != NONE && self.mark[n as usize] == stamp {
                        continue;
                    }
                    let f = face.map(|k| self.p(v[k]));
                    let own = orient(f[0], f[1], f[2], self.p(v[i]));
                    let new = orient(f[0], f[1], f[2], p);
                    if own * new <= 0.0 {
                        culprit = Some(t);
                        break 'scan;
                    }
                }
            }
            let Some(c) = culprit else { break };
            self.mark[c as usize] = 0;
            // Keep what is still reachable from the seed.
            let kept: Vec<u32> = bad.iter().copied().filter(|t| *t != c).collect();
            self.stamp += 1;
            let stamp2 = self.stamp;
            self.mark[seed as usize] = stamp2;
            let mut reach = vec![seed];
            let mut stack = vec![seed];
            while let Some(t) = stack.pop() {
                for i in 0..4 {
                    let n = self.nb[t as usize][i];
                    if n != NONE && self.mark[n as usize] == stamp && kept.contains(&n) {
                        self.mark[n as usize] = stamp2;
                        reach.push(n);
                        stack.push(n);
                    }
                }
            }
            for t in kept {
                if self.mark[t as usize] == stamp {
                    self.mark[t as usize] = 0;
                }
            }
            bad = reach;
            stamp = stamp2;
        }
        let mut edges: HashMap<(u32, u32), (u32, usize)> = HashMap::new();
        let mut made = NONE;
        for &t in &bad {
            for (i, face) in FACES.iter().enumerate() {
                let n = self.nb[t as usize][i];
                if n != NONE && self.mark[n as usize] == stamp {
                    continue;
                }
                let f = face.map(|k| self.tets[t as usize][k]);
                let new = self.add([f[0], f[1], f[2], pi]);
                made = new;
                // Slot 3 holds the new point (orientation only swaps 0 and 1).
                self.nb[new as usize][3] = n;
                if n != NONE
                    && let Some(j) = self.nb[n as usize].iter().position(|x| *x == t)
                {
                    self.nb[n as usize][j] = new;
                }
                let v = self.tets[new as usize];
                for k in 0..3 {
                    let others: Vec<u32> = (0..3).filter(|j| *j != k).map(|j| v[j]).collect();
                    let key = (others[0].min(others[1]), others[0].max(others[1]));
                    match edges.remove(&key) {
                        Some((o, ok)) => {
                            self.nb[new as usize][k] = o;
                            self.nb[o as usize][ok] = new;
                        }
                        None => {
                            edges.insert(key, (new, k));
                        }
                    }
                }
            }
        }
        for t in bad {
            self.alive[t as usize] = false;
        }
        if made != NONE {
            self.last = made;
        }
    }
}

/// Delaunay tetrahedralization of `points` (empty below four points or when they are all
/// coplanar to within the jitter).
pub fn tetrahedralize(points: &[Vec3]) -> Tetrahedralization {
    let n = points.len();
    let real: Vec<DVec3> = points.iter().map(|p| p.as_dvec3()).collect();
    let (lo, hi) = real.iter().fold(
        (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    let extent = if n > 0 {
        (hi - lo).length().max(1e-3)
    } else {
        1.0
    };
    let jittered: Vec<DVec3> = real
        .iter()
        .enumerate()
        .map(|(i, p)| *p + jitter(i) * (extent * JITTER))
        .collect();
    let mut out = Tetrahedralization {
        jittered: jittered.clone(),
        ..Default::default()
    };
    if n < 4 {
        return out;
    }
    let center = (lo + hi) * 0.5;
    let s = extent * SUPER;
    let mut pts = jittered;
    for c in [
        DVec3::new(1.0, 1.0, 1.0),
        DVec3::new(1.0, -1.0, -1.0),
        DVec3::new(-1.0, 1.0, -1.0),
        DVec3::new(-1.0, -1.0, 1.0),
    ] {
        pts.push(center + c * s);
    }
    let mut b = Builder {
        pts,
        tets: Vec::new(),
        nb: Vec::new(),
        sphere: Vec::new(),
        alive: Vec::new(),
        mark: Vec::new(),
        stamp: 0,
        last: 0,
    };
    let n32 = n as u32;
    b.add([n32, n32 + 1, n32 + 2, n32 + 3]);
    for i in 0..n32 {
        b.insert(i);
    }

    // Keep the tetrahedra made of real points only (and of non-zero volume).
    let mut remap = vec![NONE; b.tets.len()];
    for (t, v) in b.tets.iter().enumerate() {
        if b.alive[t] && v.iter().all(|x| *x < n32) {
            let q = v.map(|x| b.pts[x as usize]);
            if orient(q[0], q[1], q[2], q[3]) > 0.0 {
                remap[t] = out.tets.len() as u32;
                out.tets.push(*v);
            }
        }
    }
    for (t, nb) in b.nb.iter().enumerate() {
        if remap[t] != NONE {
            out.neighbors
                .push(nb.map(|x| if x == NONE { NONE } else { remap[x as usize] }));
        }
    }
    out.hull = boundary(&out).into_iter().map(|(f, _, _)| f).collect();
    out
}

/// Faces without a neighbour, wound outwards, with their tetrahedron and slot.
fn boundary(t: &Tetrahedralization) -> Vec<([u32; 3], u32, usize)> {
    let q = |x: u32| t.jittered[x as usize];
    let mut out = Vec::new();
    for (ti, v) in t.tets.iter().enumerate() {
        for (i, face) in FACES.iter().enumerate() {
            if t.neighbors[ti][i] != NONE {
                continue;
            }
            let mut f = face.map(|k| v[k]);
            if orient(q(f[0]), q(f[1]), q(f[2]), q(v[i])) > 0.0 {
                f.swap(0, 1);
            }
            out.push((f, ti as u32, i));
        }
    }
    out
}

impl Tetrahedralization {
    /// Barycentric coordinates of `p` in tetrahedron `t` on the jittered positions.
    pub fn barycentric(&self, t: u32, p: DVec3) -> [f64; 4] {
        let q = self.tets[t as usize].map(|x| self.jittered[x as usize]);
        let vol = orient(q[0], q[1], q[2], q[3]);
        let mut b = [0.0; 4];
        for (i, w) in b.iter_mut().enumerate() {
            let mut r = q;
            r[i] = p;
            *w = orient(r[0], r[1], r[2], r[3]) / vol;
        }
        b
    }

    /// Walk from tetrahedron `*hint` towards `p`; `Some(t)` when a tetrahedron contains it,
    /// `None` when the walk leaves through the hull. Updates the hint.
    pub fn locate(&self, p: DVec3, hint: &mut u32) -> Option<u32> {
        if self.tets.is_empty() {
            return None;
        }
        let mut t = if (*hint as usize) < self.tets.len() {
            *hint
        } else {
            0
        };
        let eps = -1e-9;
        for _ in 0..self.tets.len() + 16 {
            *hint = t;
            let b = self.barycentric(t, p);
            let (i, min) = b
                .iter()
                .enumerate()
                .fold(
                    (0, f64::INFINITY),
                    |acc, (i, w)| if *w < acc.1 { (i, *w) } else { acc },
                );
            if min >= eps {
                return Some(t);
            }
            let n = self.neighbors[t as usize][i];
            if n == NONE {
                return None;
            }
            t = n;
        }
        // Didn't settle (shouldn't happen on a Delaunay mesh): search them all.
        (0..self.tets.len() as u32).find(|&t| {
            self.barycentric(t, p).iter().all(|w| *w >= eps) && {
                *hint = t;
                true
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random_points(n: usize, seed: u64) -> Vec<Vec3> {
        let mut s = seed;
        let mut r = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f32 / (1u64 << 53) as f32
        };
        (0..n)
            .map(|_| Vec3::new(r() * 10.0 - 5.0, r() * 4.0, r() * 6.0 - 3.0))
            .collect()
    }

    fn grid(nx: usize, ny: usize, nz: usize) -> Vec<Vec3> {
        let mut v = Vec::new();
        for x in 0..nx {
            for y in 0..ny {
                for z in 0..nz {
                    v.push(Vec3::new(x as f32 * 2.0, y as f32 * 1.5, z as f32 * 2.0));
                }
            }
        }
        v
    }

    /// Positive volumes, symmetric adjacency, every point used, a convex hull, and random points
    /// inside the hull in exactly one tetrahedron (no overlaps, no gaps).
    fn check_valid(points: &[Vec3], delaunay: bool) {
        let t = tetrahedralize(points);
        assert!(!t.tets.is_empty());
        let q = |i: u32| t.jittered[i as usize];
        for (i, v) in t.tets.iter().enumerate() {
            assert!(orient(q(v[0]), q(v[1]), q(v[2]), q(v[3])) > 0.0);
            for (k, &n) in t.neighbors[i].iter().enumerate() {
                if n == NONE {
                    continue;
                }
                let back = t.neighbors[n as usize]
                    .iter()
                    .position(|x| *x == i as u32)
                    .expect("adjacency is symmetric");
                let mut a = FACES[k].map(|s| v[s]);
                let mut b = FACES[back].map(|s| t.tets[n as usize][s]);
                a.sort();
                b.sort();
                assert_eq!(a, b, "neighbours share the face");
            }
        }
        let mut used = vec![false; points.len()];
        for v in &t.tets {
            for x in v {
                used[*x as usize] = true;
            }
        }
        assert!(used.iter().all(|u| *u), "every point is a vertex");
        // Convex up to 1e-3 of the size (see SUPER): every point is behind every hull face.
        let tol = 1e-3 * 10.0;
        // Signed distance to a hull face; faces of collinear points (edges of slivers on a flat
        // stretch of a grid's hull) have no plane and are skipped.
        let dist = |f: &[u32; 3], p: DVec3| {
            let n = (q(f[1]) - q(f[0])).cross(q(f[2]) - q(f[0]));
            if n.length() < 1e-3 {
                return f64::NEG_INFINITY;
            }
            n.dot(p - q(f[0])) / n.length()
        };
        for f in &t.hull {
            for p in &t.jittered {
                assert!(dist(f, *p) <= tol, "{} face {f:?} p {p}", dist(f, *p));
            }
        }
        // Coverage and no overlaps: random points inside the hull.
        let inside_hull = |p: DVec3| t.hull.iter().all(|f| dist(f, p) < -tol);
        // Random points over the layout's bounds.
        let lo = points.iter().fold(Vec3::INFINITY, |a, p| a.min(*p));
        let hi = points.iter().fold(Vec3::NEG_INFINITY, |a, p| a.max(*p));
        let samples = random_points(400, 99).into_iter().map(|s| {
            let u = (s - Vec3::new(-5.0, 0.0, -3.0)) / Vec3::new(10.0, 4.0, 6.0);
            lo + u * (hi - lo)
        });
        let mut tested = 0;
        for s in samples {
            let p = s.as_dvec3();
            if !inside_hull(p) {
                continue;
            }
            tested += 1;
            let containing = (0..t.tets.len() as u32)
                .filter(|&i| t.barycentric(i, p).iter().all(|w| *w > -1e-9))
                .count();
            assert_eq!(containing, 1, "{p}");
            let mut hint = 0;
            assert!(t.locate(p, &mut hint).is_some());
        }
        assert!(tested > 20, "{tested}");
        if delaunay {
            // Empty circumspheres (on the positions the topology was built on).
            for v in &t.tets {
                let (c, r2) = circumsphere(q(v[0]), q(v[1]), q(v[2]), q(v[3]));
                for (i, p) in t.jittered.iter().enumerate() {
                    if v.contains(&(i as u32)) {
                        continue;
                    }
                    assert!(c.distance_squared(*p) >= r2 * (1.0 - 1e-9), "point {i}");
                }
            }
        }
    }

    fn t_volume(t: &Tetrahedralization) -> f64 {
        t.tets
            .iter()
            .map(|v| {
                let q = v.map(|i| t.jittered[i as usize]);
                orient(q[0], q[1], q[2], q[3]) / 6.0
            })
            .sum()
    }

    #[test]
    fn random_points_give_a_valid_delaunay_tetrahedralization() {
        for seed in [1, 7, 42] {
            check_valid(&random_points(120, seed), true);
        }
    }

    #[test]
    fn grids_tetrahedralize_despite_ties() {
        let g = grid(4, 3, 4);
        check_valid(&g, true);
        // The tetrahedra fill the grid's box.
        let volume: f64 = t_volume(&tetrahedralize(&g));
        assert!((volume - 6.0 * 3.0 * 6.0).abs() < 1e-3 * volume, "{volume}");
        // A single box: five or six real tetrahedra (plus flat slivers along its faces, where
        // the jitter decides) filling it.
        let t = tetrahedralize(&grid(2, 2, 2));
        let solid = t
            .tets
            .iter()
            .filter(|v| {
                let q = v.map(|i| t.jittered[i as usize]);
                orient(q[0], q[1], q[2], q[3]) > 1e-3
            })
            .count();
        assert!((5..=6).contains(&solid), "{solid}");
        assert!((t_volume(&t) - 6.0).abs() < 1e-3, "{}", t_volume(&t));
    }

    #[test]
    fn too_few_or_flat_points_give_nothing() {
        assert!(tetrahedralize(&[]).tets.is_empty());
        assert!(
            tetrahedralize(&[Vec3::ZERO, Vec3::X, Vec3::Y])
                .tets
                .is_empty()
        );
        let flat: Vec<Vec3> = (0..9)
            .map(|i| Vec3::new((i % 3) as f32, 0.0, (i / 3) as f32))
            .collect();
        // Coplanar points only make slivers of jitter thickness (or none).
        let t = tetrahedralize(&flat);
        let q = |i: u32| t.jittered[i as usize];
        for v in &t.tets {
            assert!(orient(q(v[0]), q(v[1]), q(v[2]), q(v[3])).abs() < 1e-3);
        }
    }
}
