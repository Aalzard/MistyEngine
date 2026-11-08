//! Mesh distance fields: one 32³ signed distance volume per mesh (in mesh space), baked on the
//! CPU the first time a mesh is traced and kept in a 3D atlas shared by every view and every
//! instance. Distance = exact distance to the closest triangle; sign from the generalised
//! winding number (inside where the mesh's triangles wrap the point more than halfway), which
//! also gives open and badly closed meshes a sensible inside.
//!
//! Small meshes bake on the render thread (a few milliseconds). Bigger ones (a dense rock:
//! 82k triangles, ~80 s of CPU time) bake on the [`crate::background`] pool, never rayon's
//! global pool (the frame's own parallel loops wait behind anything queued there), and are
//! left out of the global SDF until they are ready. A finished bake is shared by every renderer
//! of the process and, in the editor, saved in `Library/MeshSdf/<content hash>.sdf`: the same
//! mesh reads back in a fraction of a millisecond in any later session, from any project path.
//! Without streaming (tests, screenshots) the frame that first needs a bake waits for it.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use glam::Vec3;
use rayon::prelude::*;

use super::scene::Brick;
use crate::scene::MeshRef;

/// Voxels per side of one mesh SDF.
pub const BRICK: u32 = 32;
/// Atlas size in bricks (x, y, z).
const BRICKS: [u32; 3] = [4, 4, 8];
/// Meshes up to this many triangles bake inline.
const INLINE_TRIANGLES: usize = 512;
/// Meshes above this many triangles are not traced (the bake would take too long).
const MAX_TRIANGLES: usize = 200_000;

pub fn atlas_size() -> [u32; 3] {
    BRICKS.map(|b| b * BRICK)
}

/// A finished bake: distances (mesh units) and the box they cover.
#[derive(Debug, PartialEq)]
pub struct Baked {
    pub min: Vec3,
    pub max: Vec3,
    pub distances: Vec<f32>,
}

/// A bake shared by every renderer: set once it is ready.
type Shared = Arc<OnceLock<Arc<Baked>>>;

enum Entry {
    Baking(Shared),
    Ready(Brick),
    /// No data or too big: never traced.
    Skipped,
}

/// The atlas and what is in it.
pub struct MeshSdfs {
    pub atlas: wgpu::Texture,
    pub view: wgpu::TextureView,
    entries: HashMap<String, Entry>,
    next: u32,
}

fn key(r: &MeshRef) -> String {
    match r {
        MeshRef::Primitive(p) => format!("primitive:{p:?}"),
        MeshRef::Asset(k) => k.clone(),
    }
}

impl MeshSdfs {
    pub fn new(device: &wgpu::Device) -> Self {
        let [w, h, d] = atlas_size();
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("misty gi mesh sdf atlas"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: d,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::R16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = atlas.create_view(&Default::default());
        Self {
            atlas,
            view,
            entries: HashMap::new(),
            next: 0,
        }
    }

    /// The brick of a mesh, baking it if it was never seen (None while it bakes or when it
    /// can't be traced).
    pub fn brick(&mut self, queue: &wgpu::Queue, r: &MeshRef) -> Option<Brick> {
        let k = key(r);
        if !self.entries.contains_key(&k) {
            let entry = match triangles(r) {
                None => Entry::Skipped,
                Some(tris) if tris.len() > MAX_TRIANGLES => Entry::Skipped,
                Some(tris) if tris.len() <= INLINE_TRIANGLES => {
                    Entry::Baking(Arc::new(OnceLock::from(Arc::new(bake(&tris)))))
                }
                Some(tris) => Entry::Baking(request(
                    tris,
                    cache_dir(r),
                    !crate::render::grass_streaming(),
                )),
            };
            self.entries.insert(k.clone(), entry);
        }
        let done = match self.entries.get(&k)? {
            Entry::Ready(b) => return Some(*b),
            Entry::Skipped => return None,
            Entry::Baking(slot) => slot.get()?.clone(),
        };
        let entry = match self.upload(queue, &done) {
            Some(b) => Entry::Ready(b),
            None => Entry::Skipped,
        };
        self.entries.insert(k, entry);
        match self.entries.get(&key(r))? {
            Entry::Ready(b) => Some(*b),
            _ => None,
        }
    }

    fn upload(&mut self, queue: &wgpu::Queue, b: &Baked) -> Option<Brick> {
        let n = self.next;
        if n >= BRICKS[0] * BRICKS[1] * BRICKS[2] {
            return None;
        }
        self.next += 1;
        let origin = [
            (n % BRICKS[0]) * BRICK,
            (n / BRICKS[0] % BRICKS[1]) * BRICK,
            (n / (BRICKS[0] * BRICKS[1])) * BRICK,
        ];
        let halves: Vec<u16> = b
            .distances
            .iter()
            .map(|&d| half::f16::from_f32(d).to_bits())
            .collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.atlas,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin[0],
                    y: origin[1],
                    z: origin[2],
                },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&halves),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(BRICK * 2),
                rows_per_image: Some(BRICK),
            },
            wgpu::Extent3d {
                width: BRICK,
                height: BRICK,
                depth_or_array_layers: BRICK,
            },
        );
        Some(Brick {
            origin,
            min: b.min,
            max: b.max,
        })
    }
}

/// Mesh-space triangles of a mesh.
fn triangles(r: &MeshRef) -> Option<Vec<[Vec3; 3]>> {
    let data = crate::models::mesh(r)?;
    let p = |i: u32| {
        data.vertices
            .get(i as usize)
            .map(|v| Vec3::from(v.position))
    };
    let tris: Vec<[Vec3; 3]> = data
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|t| Some([p(t[0])?, p(t[1])?, p(t[2])?]))
        .collect();
    (!tris.is_empty()).then_some(tris)
}

/// Bakes of this process by content hash: in flight or done.
fn shared() -> &'static Mutex<HashMap<u64, Shared>> {
    static SHARED: OnceLock<Mutex<HashMap<u64, Shared>>> = OnceLock::new();
    SHARED.get_or_init(Default::default)
}

/// Background bakes started and not finished yet.
static PENDING: AtomicUsize = AtomicUsize::new(0);

/// Mesh SDF bakes running in the background (the editor's status bar says so).
pub fn pending() -> usize {
    PENDING.load(Ordering::Relaxed)
}

/// Bumped when the file layout or the bake changes.
const FORMAT: u32 = 1;
const MAGIC: &[u8; 4] = b"MSDF";
/// Cache folder, relative to the project.
pub const LIBRARY_DIR: &str = "Library/MeshSdf";
/// Bytes before the distances: magic, format, hash, box.
const HEADER: usize = 4 + 4 + 8 + 24;

/// The editor's cache folder for mesh `r` (streaming, files on disk): the `Library` of the
/// project whose `Assets` hold the mesh. Tests without streaming and built games keep bakes in
/// memory only.
fn cache_dir(r: &MeshRef) -> Option<PathBuf> {
    if !crate::render::grass_streaming() || crate::vfs::is_mounted() {
        return None;
    }
    let (path, _) = r.split_asset()?;
    let file = crate::assets::absolute(path);
    let project = file
        .ancestors()
        .find(|a| a.file_name().is_some_and(|n| n == "Assets"))
        .and_then(Path::parent);
    Some(match project {
        Some(p) => p.join(LIBRARY_DIR),
        None => crate::assets::absolute(LIBRARY_DIR),
    })
}

/// Content hash of a bake's input: the triangles themselves (not the asset path, which differs
/// between copies of a project) and the bake's size and format.
pub fn content_hash(tris: &[[Vec3; 3]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |w: u32| {
        h ^= u64::from(w);
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    eat(FORMAT);
    eat(BRICK);
    eat(tris.len() as u32);
    for p in tris.iter().flatten() {
        eat(p.x.to_bits());
        eat(p.y.to_bits());
        eat(p.z.to_bits());
    }
    h
}

fn cache_file(dir: &Path, hash: u64) -> PathBuf {
    dir.join(format!("{hash:016x}.sdf"))
}

/// The bake cached for `hash` in `dir`, if it is there and whole.
pub fn load(dir: &Path, hash: u64) -> Option<Baked> {
    let bytes = std::fs::read(cache_file(dir, hash)).ok()?;
    let n = (BRICK * BRICK * BRICK) as usize;
    if bytes.len() != HEADER + n * 4 || &bytes[..4] != MAGIC {
        return None;
    }
    let word = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let f = |i: usize| f32::from_bits(word(i));
    let stored = u64::from(word(8)) | (u64::from(word(12)) << 32);
    if word(4) != FORMAT || stored != hash {
        return None;
    }
    let v = |i: usize| Vec3::new(f(i), f(i + 4), f(i + 8));
    Some(Baked {
        min: v(16),
        max: v(28),
        distances: (0..n).map(|i| f(HEADER + i * 4)).collect(),
    })
}

/// Write `baked` for `hash` into `dir` (through a temporary file, so a reader never sees half
/// of it).
pub fn save(dir: &Path, hash: u64, baked: &Baked) -> std::io::Result<()> {
    let mut out = Vec::with_capacity(HEADER + baked.distances.len() * 4);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT.to_le_bytes());
    out.extend_from_slice(&hash.to_le_bytes());
    for c in baked.min.to_array().into_iter().chain(baked.max.to_array()) {
        out.extend_from_slice(&c.to_le_bytes());
    }
    for d in &baked.distances {
        out.extend_from_slice(&d.to_le_bytes());
    }
    std::fs::create_dir_all(dir)?;
    let path = cache_file(dir, hash);
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::File::create(&tmp)?.write_all(&out)?;
    std::fs::rename(&tmp, path)
}

/// The bake of `tris` cached in `dir`, or a fresh one (then saved there).
fn load_or_bake(tris: &[[Vec3; 3]], hash: u64, dir: Option<&Path>) -> Baked {
    if let Some(b) = dir.and_then(|d| load(d, hash)) {
        return b;
    }
    let baked = bake(tris);
    if let Some(d) = dir
        && let Err(e) = save(d, hash, &baked)
    {
        eprintln!("{}: {e}", d.display());
    }
    baked
}

/// The shared bake of `tris`: started on the background pool unless this process already has
/// it (done or in flight). `wait`: return only once it is ready.
fn request(tris: Vec<[Vec3; 3]>, dir: Option<PathBuf>, wait: bool) -> Shared {
    let hash = content_hash(&tris);
    let (slot, start) = {
        let mut map = shared().lock().unwrap_or_else(|e| e.into_inner());
        match map.get(&hash) {
            Some(s) => (s.clone(), false),
            None => {
                let s = Shared::default();
                map.insert(hash, s.clone());
                (s, true)
            }
        }
    };
    if start {
        let out = slot.clone();
        let job = move || {
            let baked = load_or_bake(&tris, hash, dir.as_deref());
            let _ = out.set(Arc::new(baked));
        };
        if wait {
            crate::background::pool().install(job);
        } else {
            PENDING.fetch_add(1, Ordering::Relaxed);
            crate::background::pool().spawn(move || {
                job();
                PENDING.fetch_sub(1, Ordering::Relaxed);
            });
        }
    }
    if wait {
        // Started earlier without waiting (or by another thread): it lands soon.
        while slot.get().is_none() {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    slot
}

/// Closest point of triangle `t` to `p` (Ericson, Real-Time Collision Detection 5.1.5).
fn closest_on_triangle(p: Vec3, [a, b, c]: [Vec3; 3]) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// Signed solid angle of triangle `t` seen from `p` (Van Oosterom & Strackee).
fn solid_angle(p: Vec3, [a, b, c]: [Vec3; 3]) -> f32 {
    let (a, b, c) = (a - p, b - p, c - p);
    let (la, lb, lc) = (a.length(), b.length(), c.length());
    let num = a.dot(b.cross(c));
    let den = la * lb * lc + a.dot(b) * lc + b.dot(c) * la + c.dot(a) * lb;
    2.0 * num.atan2(den)
}

/// Bake a 32³ SDF over the triangles' bounds (grown by two voxels so the outside is covered).
pub fn bake(tris: &[[Vec3; 3]]) -> Baked {
    let (mut lo, mut hi) = tris.iter().flatten().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(a, b), p| (a.min(*p), b.max(*p)),
    );
    // Flat meshes still get a volume.
    let extent = (hi - lo).max_element().max(1e-3);
    let pad = Vec3::splat(extent / (BRICK as f32 - 4.0) * 2.0);
    lo -= pad;
    hi += pad;
    let n = BRICK as usize;
    let step = (hi - lo) / (BRICK - 1) as f32;
    let distances: Vec<f32> = (0..n * n * n)
        .into_par_iter()
        .map(|i| {
            let p = lo + Vec3::new((i % n) as f32, (i / n % n) as f32, (i / (n * n)) as f32) * step;
            let mut best = f32::MAX;
            let mut winding = 0.0;
            for t in tris {
                best = best.min(closest_on_triangle(p, *t).distance_squared(p));
                winding += solid_angle(p, *t);
            }
            let inside = (winding / (4.0 * std::f32::consts::PI)).abs() > 0.5;
            let d = best.sqrt();
            if inside { -d } else { d }
        })
        .collect();
    Baked {
        min: lo,
        max: hi,
        distances,
    }
}

/// Forget every bake of the process (tests: the next request bakes or reads the disk again).
/// Waits for the background bakes first, so none lands in a forgotten slot.
#[cfg(test)]
pub(crate) fn forget_all() {
    let t = std::time::Instant::now();
    while pending() > 0 && t.elapsed().as_secs() < 600 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    shared().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rock_triangles_at(detail: u32, seed: u32) -> Vec<[Vec3; 3]> {
        let data = crate::vgeo::rock::rock(detail, seed);
        let p = |i: u32| Vec3::from(data.vertices[i as usize].position);
        let tris: Vec<[Vec3; 3]> = (data.indices.as_chunks::<3>().0.iter())
            .map(|t| [p(t[0]), p(t[1]), p(t[2])])
            .collect();
        assert!(tris.len() > INLINE_TRIANGLES);
        tris
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("misty-mesh-sdf-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn wait(slot: &Shared) -> Arc<Baked> {
        let t = std::time::Instant::now();
        loop {
            if let Some(b) = slot.get() {
                return b.clone();
            }
            // Generous: in the full suite the background pool can be busy with other bakes.
            assert!(t.elapsed().as_secs() < 900, "bake never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// The MeadowDemo freeze: a dense mesh's bake (seconds) must not run on the calling (render)
    /// thread or rayon's global pool, and a later session reads it back from `Library` instead
    /// of baking again, keyed by content (not by path).
    #[test]
    fn big_meshes_bake_in_the_background_then_read_back_from_the_cache() {
        let dir = temp_dir("cache");
        // Dense enough that the bake takes far longer than the request, even on a busy
        // machine (a small mesh could finish in the background before the check below).
        let tris = rock_triangles_at(5, 11);
        let hash = content_hash(&tris);
        let t = std::time::Instant::now();
        let slot = request(tris.clone(), Some(dir.clone()), false);
        assert!(
            t.elapsed().as_millis() < 500,
            "request waited {:?}",
            t.elapsed()
        );
        assert!(slot.get().is_none(), "baked on the calling thread");
        let first = wait(&slot);
        assert_eq!(*first, bake(&tris), "the background bake is the same bake");
        let t = std::time::Instant::now();
        while load(&dir, hash).is_none() {
            assert!(t.elapsed().as_secs() < 900, "cache file never written");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        // Another renderer of the same process shares the bake.
        assert!(Arc::ptr_eq(&request(tris.clone(), None, true), &slot));

        // A new session (nothing in memory) with a copy of the project somewhere else: the
        // cache file is read, not baked again (a marked file proves which).
        shared().lock().unwrap().remove(&hash);
        let marked = Baked {
            distances: vec![7.0; first.distances.len()],
            min: first.min,
            max: first.max,
        };
        let copy = temp_dir("copy");
        save(&copy, hash, &marked).unwrap();
        let read = request(tris.clone(), Some(copy.clone()), true);
        assert_eq!(**read.get().unwrap(), marked);

        // An edited mesh is a different key; a damaged file is ignored.
        let mut edited = tris.clone();
        edited[0][0].x += 0.01;
        assert_ne!(content_hash(&edited), hash);
        std::fs::write(cache_file(&copy, hash), b"MSDF").unwrap();
        assert!(load(&copy, hash).is_none());
        shared().lock().unwrap().remove(&hash);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&copy);
    }

    #[test]
    fn a_cube_bakes_to_a_box_distance() {
        let data = crate::render::mesh::generate(crate::scene::PrimitiveMesh::Cube);
        let p = |i: u32| Vec3::from(data.vertices[i as usize].position);
        let tris: Vec<[Vec3; 3]> = data
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| [p(t[0]), p(t[1]), p(t[2])])
            .collect();
        let b = bake(&tris);
        let n = BRICK as usize;
        let step = (b.max - b.min) / (BRICK - 1) as f32;
        for (i, d) in b.distances.iter().enumerate() {
            let q =
                b.min + Vec3::new((i % n) as f32, (i / n % n) as f32, (i / (n * n)) as f32) * step;
            let e = q.abs() - Vec3::splat(0.5);
            let exact = e.max(Vec3::ZERO).length() + e.max_element().min(0.0);
            assert!((d - exact).abs() < 1e-3, "{q} {d} {exact}");
        }
    }
}
