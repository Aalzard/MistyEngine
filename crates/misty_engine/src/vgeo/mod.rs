//! Misty Virtual Geometry: dense static meshes split into small clusters of triangles with a
//! hierarchy of simplified clusters, so the GPU picks the level of detail per cluster (not per
//! object) and draws about as many triangles as there are pixels to cover.
//!
//! Offline (here, on the CPU):
//!
//! 1. The mesh is split into clusters of at most [`MAX_TRIANGLES`] connected triangles.
//! 2. Neighbouring clusters are grouped (about [`GROUP_SIZE`] per group). Each group's triangles
//!    are simplified to half with the group's outer border locked, then split into new
//!    clusters. Repeating on the new clusters builds a DAG of coarser and coarser clusters
//!    until one cluster is left or simplifying stops paying.
//! 3. Every cluster stores its own error (the group that made it) and its parent error (the
//!    group it was simplified into), each with a bounding sphere. A cluster is drawn when its
//!    own error projects to at most the pixel threshold and its parent's to more
//!    ([`VirtualMesh::select`]). Errors and spheres only grow towards the roots, so exactly one
//!    cluster covers each part of the surface for any camera, and because a group switches as
//!    a whole and its border never moved, neighbouring clusters always meet without cracks.
//!
//! Simplification only removes vertices (half-edge collapses), so every level indexes the
//! source mesh's own vertex buffer: no vertex data is added. See docs/pending/virtual-geometry.md.

mod cluster;
mod dag;
pub mod file;
pub mod rock;
mod simplify;
#[cfg(test)]
mod tests;

use glam::Vec3;

/// Most triangles in one cluster (the GPU draws 3 × this many vertices per cluster).
pub const MAX_TRIANGLES: usize = 128;
/// Clusters merged into one group before it is simplified.
pub const GROUP_SIZE: usize = 4;
/// Parent error of the roots: never replaced by a coarser level.
pub const ROOT_ERROR: f32 = 3.0e38;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sphere {
    pub center: Vec3,
    pub radius: f32,
}

impl Sphere {
    pub fn new(center: Vec3, radius: f32) -> Self {
        Self { center, radius }
    }

    /// A sphere around `points` (Ritter's: not the smallest, at most a few percent larger).
    pub fn around(points: &[Vec3]) -> Self {
        let Some(&first) = points.first() else {
            return Self::default();
        };
        let far = |from: Vec3| {
            points
                .iter()
                .copied()
                .max_by(|a, b| {
                    (*a - from)
                        .length_squared()
                        .total_cmp(&(*b - from).length_squared())
                })
                .unwrap_or(from)
        };
        let a = far(first);
        let b = far(a);
        let mut s = Self::new((a + b) * 0.5, (b - a).length() * 0.5);
        for &p in points {
            let d = (p - s.center).length();
            if d > s.radius {
                let r = (s.radius + d) * 0.5;
                s.center += (p - s.center) * ((r - s.radius) / d);
                s.radius = r;
            }
        }
        s
    }

    /// The smallest sphere holding both.
    pub fn union(self, o: Self) -> Self {
        let d = (o.center - self.center).length();
        if d + o.radius <= self.radius {
            return self;
        }
        if d + self.radius <= o.radius {
            return o;
        }
        let r = (d + self.radius + o.radius) * 0.5;
        let center = self.center + (o.center - self.center) * ((r - self.radius) / d.max(1e-12));
        Self::new(center, r)
    }

    /// A sphere holding all of `spheres` (each fully inside, up to float rounding).
    pub fn merge(spheres: &[Self]) -> Self {
        let mut it = spheres.iter().copied();
        let Some(first) = it.next() else {
            return Self::default();
        };
        it.fold(first, Self::union)
    }

    /// Does this sphere hold `o` (with a relative tolerance for rounding)?
    pub fn contains(&self, o: &Self) -> bool {
        let slack = 1e-4 * (1.0 + self.radius);
        (o.center - self.center).length() + o.radius <= self.radius + slack
    }
}

/// One cluster of a [`VirtualMesh`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cluster {
    /// First index of its triangles in [`VirtualMesh::indices`].
    pub first: u32,
    pub triangles: u32,
    /// Around its own triangles (frustum and occlusion culling).
    pub bounds: Sphere,
    /// Its own error (object units) and the sphere it is measured from: shared by every
    /// cluster the same group made (0 and its bounds at the finest level).
    pub lod: Sphere,
    pub error: f32,
    /// The error and sphere of the group it was simplified into ([`ROOT_ERROR`] at the roots).
    pub parent: Sphere,
    pub parent_error: f32,
    /// 0 = source triangles.
    pub level: u32,
    /// The group that made it (`u32::MAX` at level 0).
    pub group: u32,
    /// The group it was simplified into (`u32::MAX` at the roots).
    pub parent_group: u32,
    /// Normal cone of its triangles: the cluster faces away from every eye with
    /// `dot(bounds.center - eye, cone_axis) >= cone_cutoff * |bounds.center - eye| + radius`
    /// ([`Cluster::backfacing`]). `cone_cutoff` > 1 = never (normals spread past 90°).
    pub cone_axis: Vec3,
    pub cone_cutoff: f32,
}

impl Cluster {
    /// Does every triangle face away from `eye` (back-face culling of the whole cluster)?
    /// Must match the test in render/vgeo/vgeo_cull.wgsl.
    pub fn backfacing(&self, eye: Vec3) -> bool {
        let to = self.bounds.center - eye;
        to.dot(self.cone_axis) >= self.cone_cutoff * to.length() + self.bounds.radius
    }
}

/// Normal cone of triangles (`indices` into `positions`, front faces wound so that
/// `(b - a) × (c - a)` points out): the axis and the cutoff of [`Cluster::backfacing`].
pub fn normal_cone(tris: &[[u32; 3]], positions: &[Vec3]) -> (Vec3, f32) {
    let normals: Vec<Vec3> = tris
        .iter()
        .filter_map(|t| {
            let [a, b, c] = t.map(|v| positions[v as usize]);
            (b - a).cross(c - a).try_normalize()
        })
        .collect();
    let never = (Vec3::Z, 2.0);
    let Some(axis) = normals.iter().copied().sum::<Vec3>().try_normalize() else {
        return never;
    };
    let min_dot = normals.iter().map(|n| n.dot(axis)).fold(1.0f32, f32::min);
    if min_dot <= 0.0 {
        return never;
    }
    // The cluster faces away when the view direction is within 90° − (cone half angle) of
    // the axis: cos of that angle = sin(acos(min_dot)). A little slack for rounding.
    let cutoff = (1.0 - min_dot * min_dot).max(0.0).sqrt();
    (axis, (cutoff + 1e-3).min(1.0))
}

/// A mesh built for virtual geometry: every cluster of every level, ordered by parent error
/// (the GPU skips the finest clusters of far objects with one binary search).
#[derive(Clone, Debug, Default)]
pub struct VirtualMesh {
    pub clusters: Vec<Cluster>,
    /// Triangles of every cluster, indexing the source vertex buffer.
    pub indices: Vec<u32>,
    /// Holds every cluster's bounds and LOD spheres.
    pub bounds: Sphere,
    /// Levels in the hierarchy (1 = no simplified level).
    pub levels: u32,
    pub source_triangles: usize,
    /// Groups that simplified to less than 85 % of their triangles / all groups.
    pub groups: u32,
    pub stuck_groups: u32,
}

/// A camera for level-of-detail selection.
#[derive(Clone, Copy, Debug)]
pub struct LodView {
    pub camera: Vec3,
    /// Pixels covered by one unit at distance 1 (screen height / (2 tan(fov / 2))); for an
    /// orthographic camera, pixels per unit (and `orthographic` set).
    pub scale: f32,
    pub orthographic: bool,
    /// Largest error allowed on screen, in pixels.
    pub threshold: f32,
}

impl LodView {
    /// Error `error` measured from `sphere`, in pixels. Must match `projected_error` in
    /// render/vgeo/vgeo_cull.wgsl.
    pub fn projected(&self, sphere: Sphere, error: f32) -> f32 {
        if error >= ROOT_ERROR {
            return f32::INFINITY;
        }
        if self.orthographic {
            return error * self.scale;
        }
        let d = (sphere.center - self.camera).length() - sphere.radius;
        error * self.scale / d.max(1e-4)
    }
}

impl VirtualMesh {
    /// Build the clusters and hierarchy of a triangle mesh (`indices` into `positions`).
    pub fn build(positions: &[Vec3], indices: &[u32]) -> Self {
        dag::build(positions, indices)
    }

    /// Clusters `view` draws (object space). The reference for the GPU selection.
    pub fn select(&self, view: &LodView) -> Vec<usize> {
        (0..self.clusters.len())
            .filter(|&i| {
                let c = &self.clusters[i];
                view.projected(c.lod, c.error) <= view.threshold
                    && view.projected(c.parent, c.parent_error) > view.threshold
            })
            .collect()
    }

    /// Triangles of cluster `i`.
    pub fn triangles(&self, i: usize) -> &[u32] {
        let c = &self.clusters[i];
        &self.indices[c.first as usize..(c.first + 3 * c.triangles) as usize]
    }

    /// Clusters at level 0 (the source triangles).
    pub fn finest(&self) -> usize {
        self.clusters.iter().filter(|c| c.level == 0).count()
    }
}

/// Position id of every vertex: vertices at exactly the same place share one (UV and normal
/// seams split vertices, not the surface).
pub(crate) fn weld(positions: &[Vec3]) -> Vec<u32> {
    let mut ids = std::collections::HashMap::with_capacity(positions.len());
    positions
        .iter()
        .map(|p| {
            let key = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
            let next = ids.len() as u32;
            *ids.entry(key).or_insert(next)
        })
        .collect()
}
