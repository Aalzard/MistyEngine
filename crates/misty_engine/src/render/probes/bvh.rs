//! Bounding volume hierarchy over world triangles for the placement raycasts (median split on
//! the longest axis, up to four triangles per leaf, two-sided hits).

use glam::Vec3;

use crate::navigation::geometry::BakeGeometry;

struct Node {
    min: Vec3,
    max: Vec3,
    /// Leaf: first triangle index in `order`; inner: index of the left child.
    start: u32,
    /// Triangles in a leaf (0 = inner node; the right child follows the left subtree).
    count: u32,
    /// Inner nodes: index of the right child.
    right: u32,
}

pub struct Bvh {
    nodes: Vec<Node>,
    order: Vec<u32>,
    triangles: Vec<[Vec3; 3]>,
    normals: Vec<Vec3>,
}

/// Closest hit along a ray.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub t: f32,
    /// Face normal (on the side the mesh's normals point to).
    pub normal: Vec3,
}

const LEAF: usize = 4;

impl Bvh {
    pub fn new(geo: &BakeGeometry) -> Self {
        let mut b = Self {
            nodes: Vec::new(),
            order: (0..geo.triangles.len() as u32).collect(),
            triangles: geo.triangles.clone(),
            normals: geo.normals.clone(),
        };
        if !b.order.is_empty() {
            let n = b.order.len();
            b.build(0, n);
        }
        b
    }

    fn bounds(&self, range: &[u32]) -> (Vec3, Vec3) {
        range.iter().flat_map(|&i| self.triangles[i as usize]).fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(lo, hi), p| (lo.min(p), hi.max(p)),
        )
    }

    fn centroid(&self, i: u32) -> Vec3 {
        let t = &self.triangles[i as usize];
        (t[0] + t[1] + t[2]) / 3.0
    }

    /// Build the subtree over `order[start..end]`; returns its node index.
    fn build(&mut self, start: usize, end: usize) -> u32 {
        let (min, max) = self.bounds(&self.order[start..end]);
        let index = self.nodes.len() as u32;
        self.nodes.push(Node {
            min,
            max,
            start: start as u32,
            count: (end - start) as u32,
            right: 0,
        });
        if end - start <= LEAF {
            return index;
        }
        let axis = (max - min).max_position();
        let mid = (start + end) / 2;
        let mut slice = std::mem::take(&mut self.order);
        slice[start..end].select_nth_unstable_by(mid - start, |&a, &b| {
            self.centroid(a)[axis].total_cmp(&self.centroid(b)[axis])
        });
        self.order = slice;
        let left = self.build(start, mid);
        let right = self.build(mid, end);
        let node = &mut self.nodes[index as usize];
        node.count = 0;
        node.start = left;
        node.right = right;
        index
    }

    /// Nearest triangle hit by the ray `origin + t·dir` with `0 < t <= max_t` (`dir` unit).
    pub fn cast(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<Hit> {
        if self.nodes.is_empty() {
            return None;
        }
        let inv = dir.recip();
        let mut best: Option<Hit> = None;
        let mut limit = max_t;
        let mut stack = vec![0u32];
        while let Some(i) = stack.pop() {
            let n = &self.nodes[i as usize];
            if !slab(origin, inv, n.min, n.max, limit) {
                continue;
            }
            if n.count == 0 {
                stack.push(n.start);
                stack.push(n.right);
                continue;
            }
            for &t in &self.order[n.start as usize..(n.start + n.count) as usize] {
                if let Some(d) = intersect(origin, dir, &self.triangles[t as usize])
                    && d <= limit
                {
                    limit = d;
                    best = Some(Hit {
                        t: d,
                        normal: self.normals[t as usize],
                    });
                }
            }
        }
        best
    }
}

/// Ray / box overlap within `limit`.
fn slab(o: Vec3, inv: Vec3, min: Vec3, max: Vec3, limit: f32) -> bool {
    let a = (min - o) * inv;
    let b = (max - o) * inv;
    let near = a.min(b).max_element();
    let far = a.max(b).min_element();
    // NaN (ray in a slab plane) counts as a hit: the triangle test decides.
    !(near > far || far < 0.0 || near > limit)
}

/// Möller–Trumbore, both sides; distance of the hit.
fn intersect(o: Vec3, d: Vec3, t: &[Vec3; 3]) -> Option<f32> {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    // Touching counts (distance 0): a point on a wall is not in open space.
    let dist = e2.dot(q) * inv;
    (dist >= 0.0).then_some(dist)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_the_nearest_of_many_triangles() {
        let mut g = BakeGeometry::default();
        for i in 0..200 {
            let z = i as f32;
            let t = [
                Vec3::new(-1.0, -1.0, z),
                Vec3::new(1.0, -1.0, z),
                Vec3::new(0.0, 1.0, z),
            ];
            g.push(t, Vec3::NEG_Z);
        }
        let bvh = Bvh::new(&g);
        let hit = bvh.cast(Vec3::new(0.0, 0.0, 10.5), Vec3::Z, 100.0).unwrap();
        assert!((hit.t - 0.5).abs() < 1e-4);
        assert!(hit.normal.z < 0.0);
        assert!(bvh.cast(Vec3::new(0.0, 0.0, 10.5), Vec3::Z, 0.4).is_none());
        assert!(
            bvh.cast(Vec3::new(5.0, 0.0, -1.0), Vec3::Z, 100.0)
                .is_none()
        );
        let back = bvh
            .cast(Vec3::new(0.0, 0.0, 10.5), Vec3::NEG_Z, 100.0)
            .unwrap();
        assert!((back.t - 0.5).abs() < 1e-4);
    }
}
