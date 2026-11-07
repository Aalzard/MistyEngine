//! Built tree models cached on disk in the project's `Library/TreeCache`, keyed by a hash of
//! everything the build reads: the recipe, its textures (path, size, modified time) and the
//! generator's own source. Building a tree (meshes and the eight-view impostor bake) takes a
//! few hundred milliseconds per prototype; reading one back takes a few.
//!
//! Only the editor uses it (streaming on, no pak mounted): tests build in place, built games
//! have no `Library`.

use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::PathBuf;

use glam::{Vec2, Vec3};
use image::RgbaImage;

use super::TreeAsset;
use super::generate::{TreeMesh, TreeModel, TreeVertex};
use super::impostor::Impostor;

/// Bumped when the file layout changes.
const FORMAT: u32 = 1;
const MAGIC: &[u8; 4] = b"MTRE";

/// The generator's source: a change to how trees are built is a different key.
const GENERATOR: [&str; 4] = [
    include_str!("generate.rs"),
    include_str!("impostor.rs"),
    include_str!("mod.rs"),
    include_str!("store.rs"),
];

/// Is the disk cache used here (the editor: streaming, files on disk)?
pub(super) fn enabled() -> bool {
    crate::render::grass_streaming() && !crate::vfs::is_mounted()
}

/// The cache file of `asset`.
pub(super) fn path(asset: &TreeAsset) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    FORMAT.hash(&mut h);
    GENERATOR.hash(&mut h);
    serde_json::to_string(asset)
        .unwrap_or_default()
        .hash(&mut h);
    for rel in asset.textures() {
        rel.hash(&mut h);
        let meta = std::fs::metadata(crate::assets::absolute(rel)).ok();
        meta.as_ref().map(|m| m.len()).hash(&mut h);
        meta.and_then(|m| m.modified().ok()).hash(&mut h);
    }
    crate::assets::absolute(&format!("Library/TreeCache/{:016x}.bin", h.finish()))
}

/// The model cached at `path`, if it is there and whole.
pub(super) fn load(path: &PathBuf) -> Option<TreeModel> {
    let bytes = std::fs::read(path).ok()?;
    let mut r = &bytes[..];
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic).ok()?;
    if &magic != MAGIC || u32_(&mut r)? != FORMAT {
        return None;
    }
    let lods = [mesh(&mut r)?, mesh(&mut r)?];
    let impostor = Impostor {
        albedo: image(&mut r)?,
        normal: image(&mut r)?,
        width: f32_(&mut r)?,
        height: f32_(&mut r)?,
        bottom: f32_(&mut r)?,
    };
    let model = TreeModel {
        lods,
        impostor,
        min: vec3(&mut r)?,
        max: vec3(&mut r)?,
        crown_center: vec3(&mut r)?,
        crown_radii: Vec2::new(f32_(&mut r)?, f32_(&mut r)?),
        height: f32_(&mut r)?,
    };
    r.is_empty().then_some(model)
}

/// Write `model` to `path` (through a temporary file, so a reader never sees half of it).
pub(super) fn save(path: &PathBuf, model: &TreeModel) -> std::io::Result<()> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT.to_le_bytes());
    for m in &model.lods {
        put_mesh(&mut out, m);
    }
    let i = &model.impostor;
    put_image(&mut out, &i.albedo);
    put_image(&mut out, &i.normal);
    for v in [i.width, i.height, i.bottom] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for v in [model.min, model.max, model.crown_center] {
        put_vec3(&mut out, v);
    }
    for v in [model.crown_radii.x, model.crown_radii.y, model.height] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::File::create(&tmp)?.write_all(&out)?;
    std::fs::rename(&tmp, path)
}

fn put_mesh(out: &mut Vec<u8>, m: &TreeMesh) {
    out.extend_from_slice(&(m.vertices.len() as u32).to_le_bytes());
    out.extend_from_slice(bytemuck::cast_slice(&m.vertices));
    out.extend_from_slice(&(m.indices.len() as u32).to_le_bytes());
    out.extend_from_slice(bytemuck::cast_slice(&m.indices));
    for v in [m.bark.start, m.bark.end, m.leaves.start, m.leaves.end] {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

fn put_image(out: &mut Vec<u8>, img: &RgbaImage) {
    out.extend_from_slice(&img.width().to_le_bytes());
    out.extend_from_slice(&img.height().to_le_bytes());
    out.extend_from_slice(img.as_raw());
}

fn put_vec3(out: &mut Vec<u8>, v: Vec3) {
    for c in v.to_array() {
        out.extend_from_slice(&c.to_le_bytes());
    }
}

fn take<'a>(r: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    let (head, rest) = r.split_at_checked(n)?;
    *r = rest;
    Some(head)
}

fn u32_(r: &mut &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(take(r, 4)?.try_into().ok()?))
}

fn f32_(r: &mut &[u8]) -> Option<f32> {
    Some(f32::from_le_bytes(take(r, 4)?.try_into().ok()?))
}

fn vec3(r: &mut &[u8]) -> Option<Vec3> {
    Some(Vec3::new(f32_(r)?, f32_(r)?, f32_(r)?))
}

fn mesh(r: &mut &[u8]) -> Option<TreeMesh> {
    let n = u32_(r)? as usize;
    let vertices: Vec<TreeVertex> =
        bytemuck::pod_collect_to_vec(take(r, n.checked_mul(std::mem::size_of::<TreeVertex>())?)?);
    let n = u32_(r)? as usize;
    let indices: Vec<u32> = bytemuck::pod_collect_to_vec(take(r, n.checked_mul(4)?)?);
    let (bs, be, ls, le) = (u32_(r)?, u32_(r)?, u32_(r)?, u32_(r)?);
    Some(TreeMesh {
        vertices,
        indices,
        bark: bs..be,
        leaves: ls..le,
    })
}

fn image(r: &mut &[u8]) -> Option<RgbaImage> {
    let (w, h) = (u32_(r)?, u32_(r)?);
    let len = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
    RgbaImage::from_raw(w, h, take(r, len)?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_model_reads_back_identical() {
        let model = super::super::generate::build(&TreeAsset::birch(), &Default::default());
        let dir = std::env::temp_dir().join(format!("misty-tree-cache-{}", std::process::id()));
        let file = dir.join("birch.bin");
        save(&file, &model).unwrap();
        let back = load(&file).expect("reads back");
        for (a, b) in model.lods.iter().zip(&back.lods) {
            assert_eq!(a.vertices, b.vertices);
            assert_eq!(a.indices, b.indices);
            assert_eq!((&a.bark, &a.leaves), (&b.bark, &b.leaves));
        }
        let (i, j) = (&model.impostor, &back.impostor);
        assert_eq!(i.albedo, j.albedo);
        assert_eq!(i.normal, j.normal);
        assert_eq!((i.width, i.height, i.bottom), (j.width, j.height, j.bottom));
        assert_eq!((model.min, model.max), (back.min, back.max));
        assert_eq!(model.crown_center, back.crown_center);
        assert_eq!(model.crown_radii, back.crown_radii);
        assert_eq!(model.height, back.height);
        // A cut-off file is not a model.
        let bytes = std::fs::read(&file).unwrap();
        std::fs::write(&file, &bytes[..bytes.len() - 7]).unwrap();
        assert!(load(&file).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn key_follows_the_recipe() {
        let (a, mut b) = (TreeAsset::oak(), TreeAsset::oak());
        assert_eq!(path(&a), path(&b));
        b.seed += 1;
        assert_ne!(path(&a), path(&b));
    }
}
