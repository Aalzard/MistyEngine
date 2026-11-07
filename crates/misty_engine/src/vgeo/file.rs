//! `.vgeo` files: a model's built virtual meshes, cached in `Library/VirtualGeometry/<guid>.vgeo`
//! (and packed into player builds) so the cluster build runs once per import, not per load.
//!
//! Little-endian binary: magic, [`VERSION`], mesh count, then per mesh a present flag, the
//! content hash of the source it was built from ([`content_hash`]), the build time and the
//! [`VirtualMesh`]. A file with another version or hash is ignored and rebuilt.

use glam::Vec3;

use super::{Cluster, Group, Sphere, VirtualMesh};

const MAGIC: &[u8; 8] = b"MISTYVG\0";
/// Bumped whenever the layout or the builder's output changes.
pub const VERSION: u32 = 3;

/// One mesh of a `.vgeo` file.
#[derive(Clone, Debug)]
pub struct Entry {
    /// [`content_hash`] of the source positions and indices.
    pub hash: u64,
    /// Time the build took, in milliseconds.
    pub build_ms: f32,
    pub mesh: VirtualMesh,
}

/// FNV-1a of every position and index: the cache key of a mesh's build.
pub fn content_hash(positions: &[Vec3], indices: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |w: u32| {
        for b in w.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    eat(positions.len() as u32);
    for p in positions {
        eat(p.x.to_bits());
        eat(p.y.to_bits());
        eat(p.z.to_bits());
    }
    eat(indices.len() as u32);
    for &i in indices {
        eat(i);
    }
    h
}

struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }
    fn sphere(&mut self, s: Sphere) {
        self.vec3(s.center);
        self.f32(s.radius);
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.0.split_first_chunk::<N>()?;
        self.0 = rest;
        Some(*head)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take::<1>()?[0])
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_bits(self.u32()?))
    }
    fn vec3(&mut self) -> Option<Vec3> {
        Some(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    fn sphere(&mut self) -> Option<Sphere> {
        Some(Sphere::new(self.vec3()?, self.f32()?))
    }
    /// A count of items of at least `size` bytes each that the rest of the file can hold.
    fn count(&mut self, size: usize) -> Option<usize> {
        let n = self.u32()? as usize;
        (n.checked_mul(size)? <= self.0.len()).then_some(n)
    }
}

/// The file bytes of `meshes`: per mesh its content hash, build time (ms) and build (`None` =
/// a mesh that isn't built, e.g. skinned).
pub fn write(meshes: &[Option<(u64, f32, &VirtualMesh)>]) -> Vec<u8> {
    let mut w = Writer(Vec::new());
    w.0.extend_from_slice(MAGIC);
    w.u32(VERSION);
    w.u32(meshes.len() as u32);
    for m in meshes {
        let Some((hash, build_ms, v)) = *m else {
            w.0.push(0);
            continue;
        };
        w.0.push(1);
        w.u64(hash);
        w.f32(build_ms);
        w.sphere(v.bounds);
        w.u32(v.levels);
        w.u64(v.source_triangles as u64);
        w.u32(v.stuck_groups);
        w.u32(v.materials);
        w.u32(v.groups.len() as u32);
        for g in &v.groups {
            w.sphere(g.sphere);
            w.f32(g.error);
        }
        w.u32(v.clusters.len() as u32);
        for c in &v.clusters {
            w.u32(c.first);
            w.u32(c.triangles);
            w.sphere(c.bounds);
            w.sphere(c.lod);
            w.f32(c.error);
            w.sphere(c.parent);
            w.f32(c.parent_error);
            w.u32(c.level);
            w.u32(c.group);
            w.u32(c.parent_group);
            w.u32(c.material);
            w.vec3(c.cone_axis);
            w.f32(c.cone_cutoff);
        }
        w.u32(v.indices.len() as u32);
        for &i in &v.indices {
            w.u32(i);
        }
    }
    w.0
}

/// Bytes of one cluster record.
const CLUSTER_BYTES: usize = 4 * 24;

/// Read a file written by [`write`] (`None` for another version or a damaged file).
pub fn read(bytes: &[u8]) -> Option<Vec<Option<Entry>>> {
    let mut r = Reader(bytes);
    if &r.take::<8>()? != MAGIC || r.u32()? != VERSION {
        return None;
    }
    let count = r.count(1)?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if r.u8()? == 0 {
            out.push(None);
            continue;
        }
        let hash = r.u64()?;
        let build_ms = r.f32()?;
        let bounds = r.sphere()?;
        let levels = r.u32()?;
        let source_triangles = r.u64()? as usize;
        let stuck_groups = r.u32()?;
        let materials = r.u32()?;
        let n = r.count(20)?;
        let mut groups = Vec::with_capacity(n);
        for _ in 0..n {
            groups.push(Group {
                sphere: r.sphere()?,
                error: r.f32()?,
            });
        }
        let n = r.count(CLUSTER_BYTES)?;
        let mut clusters = Vec::with_capacity(n);
        for _ in 0..n {
            clusters.push(Cluster {
                first: r.u32()?,
                triangles: r.u32()?,
                bounds: r.sphere()?,
                lod: r.sphere()?,
                error: r.f32()?,
                parent: r.sphere()?,
                parent_error: r.f32()?,
                level: r.u32()?,
                group: r.u32()?,
                parent_group: r.u32()?,
                material: r.u32()?,
                cone_axis: r.vec3()?,
                cone_cutoff: r.f32()?,
            });
        }
        let n = r.count(4)?;
        let mut indices = Vec::with_capacity(n);
        for _ in 0..n {
            indices.push(r.u32()?);
        }
        // Every cluster's triangles must be inside the index list, its groups in the group
        // list (or none) and its part among the parts.
        let group_ok = |g: u32| g == super::NO_GROUP || (g as usize) < groups.len();
        let fits = clusters.iter().all(|c| {
            (c.first as u64 + 3 * c.triangles as u64) <= indices.len() as u64
                && group_ok(c.group)
                && group_ok(c.parent_group)
                && c.material < materials
        });
        if !fits {
            return None;
        }
        out.push(Some(Entry {
            hash,
            build_ms,
            mesh: VirtualMesh {
                clusters,
                groups,
                indices,
                bounds,
                levels,
                source_triangles,
                stuck_groups,
                materials,
            },
        }));
    }
    Some(out)
}
