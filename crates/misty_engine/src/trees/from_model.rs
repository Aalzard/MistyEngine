//! Tree meshes from a model file ([`TreeAsset::model`]): a tree modelled elsewhere (an asset
//! pack's FBX, a glTF) placed and drawn like a generated one — instanced, with the lighter
//! mesh further out, the baked impostor far away, wind, leaf cut-outs and translucency.
//!
//! The model's parts are sorted by name (case does not matter):
//! - leaves: parts whose material, mesh or node name holds one of [`LEAF_WORDS`]; everything
//!   else is bark;
//! - parts named `billboard` or `impostor` (a pack's own far card) are left out: the impostor
//!   is baked from the mesh, like every other tree's;
//! - levels of detail: parts under a name with `LOD<n>` belong to that level. The lowest
//!   level is the full mesh, the next one the lighter mesh (a model without levels uses its
//!   one mesh for both).
//!
//! The model keeps its own texture coordinates (the bark and leaf textures are the asset's),
//! its foot stands at the model's origin, and the wind weights come from the shape: height for
//! the trunk's bend, distance from the trunk's axis for the limbs' sway (one phase per
//! direction and height, so a branch moves as one), a flutter on the leaves. Each connected
//! piece of the leaves (a twig's card) gets its own colour variation.

use std::collections::HashMap;

use glam::{Mat4, Vec2, Vec3};

use super::TreeAsset;
use super::generate::{TreeMesh, TreeVertex};
use crate::models::Model;

/// Words that mark a part as leaves.
pub const LEAF_WORDS: [&str; 8] = [
    "leaf", "leave", "foliage", "frond", "needle", "blossom", "flower", "petal",
];

/// Words that mark a part as the pack's own far card (left out).
const CARD_WORDS: [&str; 2] = ["billboard", "impostor"];

fn has_word(name: &str, words: &[&str]) -> bool {
    let lower = name.to_ascii_lowercase();
    words.iter().any(|w| lower.contains(w))
}

/// The level in a name ("Tree_LOD2" → 2, "lod 1" → 1).
pub fn lod_level(name: &str) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("lod") {
        let rest = lower[from + i + 3..].trim_start_matches(['_', ' ', '-']);
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(n) = digits.parse() {
            return Some(n);
        }
        from += i + 3;
    }
    None
}

/// One part of the model in tree space.
struct Part {
    mesh: usize,
    matrix: Mat4,
    leaf: bool,
    level: Option<u32>,
}

fn parts(model: &Model) -> Vec<Part> {
    // Parents come before their children.
    let mut world: Vec<Mat4> = Vec::with_capacity(model.nodes.len());
    let mut levels: Vec<Option<u32>> = Vec::with_capacity(model.nodes.len());
    let mut cards: Vec<bool> = Vec::with_capacity(model.nodes.len());
    let mut out = Vec::new();
    for n in &model.nodes {
        let local = Mat4::from_scale_rotation_translation(n.scale, n.rotation, n.position);
        let parent = n.parent.filter(|p| *p < world.len());
        let m = parent.map_or(local, |p| world[p] * local);
        let level = lod_level(&n.name).or(parent.and_then(|p| levels[p]));
        let card = has_word(&n.name, &CARD_WORDS) || parent.is_some_and(|p| cards[p]);
        world.push(m);
        levels.push(level);
        cards.push(card);
        for &i in &n.meshes {
            let Some(mesh) = model.meshes.get(i) else {
                continue;
            };
            let material = mesh
                .material
                .and_then(|k| model.materials.get(k))
                .map_or("", |k| k.name.as_str());
            if card || has_word(&mesh.name, &CARD_WORDS) || has_word(material, &CARD_WORDS) {
                continue;
            }
            out.push(Part {
                mesh: i,
                matrix: m,
                leaf: has_word(material, &LEAF_WORDS)
                    || has_word(&mesh.name, &LEAF_WORDS)
                    || has_word(&n.name, &LEAF_WORDS),
                level: level.or_else(|| lod_level(&mesh.name)),
            });
        }
    }
    out
}

/// Disjoint sets over vertex positions (leaf cards sharing a corner are one piece).
fn find(parent: &mut [u32], mut i: u32) -> u32 {
    while parent[i as usize] != i {
        parent[i as usize] = parent[parent[i as usize] as usize];
        i = parent[i as usize];
    }
    i
}

fn hash01(a: u32, b: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9e37_79b9) ^ b.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    (h & 0xffff) as f32 / 65535.0
}

/// One level of the model as a tree mesh: bark first, then leaves.
fn level_mesh(model: &Model, parts: &[&Part], height: f32, trunk: f32) -> TreeMesh {
    let mut m = TreeMesh::default();
    for leaf in [false, true] {
        let start = m.indices.len() as u32;
        let first_vertex = m.vertices.len();
        for p in parts.iter().filter(|p| p.leaf == leaf) {
            let mesh = &model.meshes[p.mesh];
            let base = m.vertices.len() as u32;
            let normal_matrix = glam::Mat3::from_mat4(p.matrix).inverse().transpose();
            // A mirroring transform turns the triangles inside out.
            let flipped = glam::Mat3::from_mat4(p.matrix).determinant() < 0.0;
            for v in &mesh.data.vertices {
                let pos = p.matrix.transform_point3(Vec3::from(v.position));
                let n = (normal_matrix * Vec3::from(v.normal)).normalize_or(Vec3::Y);
                let out = Vec2::new(pos.x, pos.z).length();
                let h = (pos.y / height).clamp(0.0, 1.0);
                // Limbs: beyond the trunk, more towards the tips.
                let sway = ((out - trunk).max(0.0) * 0.12).min(0.6);
                let around = pos.z.atan2(pos.x);
                let phase = 0.5 + 0.5 * (around + h * 4.0).sin();
                m.vertices.push(TreeVertex {
                    position: pos.into(),
                    normal: n.into(),
                    uv: v.uv,
                    wind: [sway, phase, if leaf { 0.5 } else { 0.0 }, h],
                    extra: [255, 128, if leaf { 255 } else { 0 }, 0],
                });
            }
            for tri in mesh.data.indices.as_chunks::<3>().0 {
                let [a, b, c] = *tri;
                if flipped {
                    m.indices.extend([base + a, base + c, base + b]);
                } else {
                    m.indices.extend([base + a, base + b, base + c]);
                }
            }
        }
        let range = start..m.indices.len() as u32;
        if leaf {
            vary_pieces(&mut m, first_vertex, range.clone());
            m.leaves = range;
        } else {
            m.bark = range;
        }
    }
    m
}

/// Give every connected piece of the leaves (vertices from `first` on, triangles `range`)
/// one colour variation value, and shade pieces a little by how deep they sit in the crown.
fn vary_pieces(m: &mut TreeMesh, first: usize, range: std::ops::Range<u32>) {
    let count = m.vertices.len() - first;
    if count == 0 {
        return;
    }
    // Vertices at one position are one point (cards are split along UV seams).
    let mut parent: Vec<u32> = (0..count as u32).collect();
    let mut at: HashMap<[u32; 3], u32> = HashMap::new();
    for i in 0..count {
        let key = m.vertices[first + i].position.map(f32::to_bits);
        let j = *at.entry(key).or_insert(i as u32);
        if j != i as u32 {
            let (a, b) = (find(&mut parent, i as u32), find(&mut parent, j));
            parent[a as usize] = b;
        }
    }
    let tris = m.indices[range.start as usize..range.end as usize]
        .as_chunks::<3>()
        .0
        .to_vec();
    for t in &tris {
        let a = find(&mut parent, t[0] - first as u32);
        for k in 1..3 {
            let b = find(&mut parent, t[k] - first as u32);
            parent[b as usize] = a;
        }
    }
    for i in 0..count {
        let root = find(&mut parent, i as u32);
        // The piece's first vertex names it (stable for a given file).
        let p = m.vertices[first + root as usize].position;
        let v = hash01(p[0].to_bits() ^ p[2].to_bits().rotate_left(13), p[1].to_bits());
        m.vertices[first + i].extra[1] = (v * 255.0) as u8;
    }
}

/// The two mesh levels of the asset's model and its height, or why it can't be used.
pub fn build(t: &TreeAsset) -> Result<([TreeMesh; 2], f32), String> {
    let model = crate::models::model(&t.model)?;
    let all = parts(&model);
    if all.is_empty() {
        return Err(format!("{}: the model has no meshes", t.model));
    }
    // The levels the file has, lowest first; parts without a level belong to every level.
    let mut levels: Vec<u32> = all.iter().filter_map(|p| p.level).collect();
    levels.sort_unstable();
    levels.dedup();
    let pick = |level: Option<u32>| -> Vec<&Part> {
        all.iter()
            .filter(|p| p.level.is_none() || p.level == level)
            .collect()
    };
    let full = pick(levels.first().copied());
    let light = pick(levels.get(1).or(levels.first()).copied());
    let mut height = 0.0f32;
    let mut trunk = f32::MAX;
    for p in &full {
        let (lo, hi) = model.meshes[p.mesh].bounds;
        for k in 0..8 {
            let c = Vec3::new(
                if k & 1 == 0 { lo.x } else { hi.x },
                if k & 2 == 0 { lo.y } else { hi.y },
                if k & 4 == 0 { lo.z } else { hi.z },
            );
            height = height.max(p.matrix.transform_point3(c).y);
        }
    }
    if !height.is_finite() || height <= 0.01 {
        return Err(format!("{}: the model has no height", t.model));
    }
    // The trunk's radius: how far the bark reaches from the axis in the lowest tenth.
    for p in full.iter().filter(|p| !p.leaf) {
        let mut reach = 0.0f32;
        let mut any = false;
        for v in &model.meshes[p.mesh].data.vertices {
            let q = p.matrix.transform_point3(Vec3::from(v.position));
            if q.y < height * 0.1 {
                reach = reach.max(Vec2::new(q.x, q.z).length());
                any = true;
            }
        }
        if any {
            trunk = trunk.min(reach);
        }
    }
    if trunk == f32::MAX {
        trunk = t.trunk_radius;
    }
    let lod0 = level_mesh(&model, &full, height, trunk);
    let lod1 = level_mesh(&model, &light, height, trunk);
    if lod0.indices.is_empty() {
        return Err(format!("{}: the model has no triangles", t.model));
    }
    Ok(([lod0, lod1], height))
}
