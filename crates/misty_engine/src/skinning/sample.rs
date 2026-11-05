//! A small rigged, morphing glTF made in code (no downloaded assets): a tapered "tentacle" on a
//! chain of four bones, with "Bulge" and "Pinch" blend shapes and a looping "Bend" animation
//! that sways the bones and pulses the bulge. Used by the SkinningDemo sample and by tests.

use std::f32::consts::TAU;

use serde_json::{Value, json};

/// Rings along the height and vertices around each ring.
const RINGS: usize = 24;
const SIDES: usize = 16;
pub const HEIGHT: f32 = 2.0;
/// Joint heights (Bone0 … Bone3), in file space.
pub const JOINTS: [f32; 4] = [0.0, 0.5, 1.0, 1.5];
/// Length of the Bend animation, seconds.
pub const BEND_SECONDS: f32 = 2.0;

pub const SHAPES: [&str; 2] = ["Bulge", "Pinch"];

fn radius(y: f32) -> f32 {
    0.3 * (1.0 - 0.65 * y / HEIGHT) + 0.02
}

/// Two-bone linear blend between segment centres (weights of the joints at `y`).
pub fn weights_at(y: f32) -> ([u16; 4], [f32; 4]) {
    let centers: Vec<f32> = JOINTS.iter().map(|j| j + 0.25).collect();
    if y <= centers[0] {
        return ([0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
    }
    if y >= centers[3] {
        return ([3, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
    }
    let k = centers.iter().rposition(|c| *c <= y).unwrap_or(0).min(2);
    let t = (y - centers[k]) / (centers[k + 1] - centers[k]);
    ([k as u16, k as u16 + 1, 0, 0], [1.0 - t, t, 0.0, 0.0])
}

/// Accessors and buffer views appended to one binary buffer.
#[derive(Default)]
struct Bin {
    data: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Bin {
    /// Add `bytes` as a new view + accessor; returns the accessor index.
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        bytes: &[u8],
        component: u32,
        count: usize,
        kind: &str,
        minmax: Option<(Vec<f32>, Vec<f32>)>,
        target: Option<u32>,
    ) -> usize {
        while !self.data.len().is_multiple_of(4) {
            self.data.push(0);
        }
        let mut view = json!({
            "buffer": 0,
            "byteOffset": self.data.len(),
            "byteLength": bytes.len(),
        });
        if let Some(t) = target {
            view["target"] = json!(t);
        }
        self.data.extend_from_slice(bytes);
        self.views.push(view);
        let mut acc = json!({
            "bufferView": self.views.len() - 1,
            "componentType": component,
            "count": count,
            "type": kind,
        });
        if let Some((lo, hi)) = minmax {
            acc["min"] = json!(lo);
            acc["max"] = json!(hi);
        }
        self.accessors.push(acc);
        self.accessors.len() - 1
    }

    fn vec3(&mut self, v: &[[f32; 3]], target: Option<u32>) -> usize {
        let mut lo = vec![f32::MAX; 3];
        let mut hi = vec![f32::MIN; 3];
        for p in v {
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        let bytes: Vec<u8> = v.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
        self.add(&bytes, FLOAT, v.len(), "VEC3", Some((lo, hi)), target)
    }

    fn floats(&mut self, v: &[f32], kind: &str, per: usize, minmax: bool) -> usize {
        let bytes: Vec<u8> = v.iter().flat_map(|f| f.to_le_bytes()).collect();
        let mm = minmax.then(|| {
            let lo = v.iter().copied().fold(f32::MAX, f32::min);
            let hi = v.iter().copied().fold(f32::MIN, f32::max);
            (vec![lo], vec![hi])
        });
        self.add(&bytes, FLOAT, v.len() / per, kind, mm, None)
    }
}

const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

/// Angle (radians) of bone `j` about Z at time `t` of the Bend animation.
pub fn bend_angle(j: usize, t: f32) -> f32 {
    let phase = TAU * t / BEND_SECONDS - j as f32 * 0.7;
    match j {
        0 => 0.12 * phase.sin(),
        _ => 0.45 * phase.sin(),
    }
}

/// The Bulge weight (0–1) at time `t` of the Bend animation.
pub fn bulge_weight(t: f32) -> f32 {
    0.5 - 0.5 * (TAU * t / BEND_SECONDS).cos()
}

/// The tentacle as `.glb` bytes.
pub fn tentacle_glb() -> Vec<u8> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs: Vec<f32> = Vec::new();
    let mut joints: Vec<u8> = Vec::new();
    let mut weights: Vec<f32> = Vec::new();
    let mut bulge = Vec::new();
    let mut pinch = Vec::new();
    let mut push = |p: [f32; 3], n: [f32; 3], uv: [f32; 2], radial: [f32; 2]| {
        positions.push(p);
        normals.push(n);
        uvs.extend(uv);
        let (j, w) = weights_at(p[1]);
        joints.extend(j.iter().flat_map(|x| x.to_le_bytes()));
        weights.extend(w);
        let y = p[1];
        let b = 0.16 * (-((y - 1.0) / 0.28).powi(2)).exp();
        bulge.push([radial[0] * b, 0.0, radial[1] * b]);
        let s = ((y - 1.0) / (HEIGHT - 1.0)).clamp(0.0, 1.0);
        let k = -0.75 * radius(y) * s * s * (3.0 - 2.0 * s);
        pinch.push([radial[0] * k, 0.0, radial[1] * k]);
    };
    for k in 0..=RINGS {
        let y = HEIGHT * k as f32 / RINGS as f32;
        let r = radius(y);
        for s in 0..=SIDES {
            let a = TAU * s as f32 / SIDES as f32;
            let (c, sn) = (a.cos(), a.sin());
            push(
                [r * c, y, r * sn],
                [c, 0.08, sn],
                [s as f32 / SIDES as f32, 1.0 - y / HEIGHT],
                [c, sn],
            );
        }
    }
    let tip = ((RINGS + 1) * (SIDES + 1)) as u32;
    push(
        [0.0, HEIGHT + 0.04, 0.0],
        [0.0, 1.0, 0.0],
        [0.5, 0.0],
        [0.0, 0.0],
    );
    let row = SIDES as u32 + 1;
    let mut indices: Vec<u32> = Vec::new();
    for k in 0..RINGS as u32 {
        for s in 0..SIDES as u32 {
            let (v00, v01) = (k * row + s, k * row + s + 1);
            let (v10, v11) = (v00 + row, v01 + row);
            // Counter-clockwise seen from outside (glTF front faces).
            indices.extend([v00, v10, v01, v01, v10, v11]);
        }
    }
    let top = RINGS as u32 * row;
    for s in 0..SIDES as u32 {
        indices.extend([top + s, tip, top + s + 1]);
    }
    for n in &mut normals {
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        *n = n.map(|x| x / l);
    }

    let mut bin = Bin::default();
    let count = positions.len();
    let a_pos = bin.vec3(&positions, Some(ARRAY_BUFFER));
    let a_nrm = bin.vec3(&normals, Some(ARRAY_BUFFER));
    let uv_bytes: Vec<u8> = uvs.iter().flat_map(|f| f.to_le_bytes()).collect();
    let a_uv = bin.add(&uv_bytes, FLOAT, count, "VEC2", None, Some(ARRAY_BUFFER));
    let a_j = bin.add(
        &joints,
        UNSIGNED_SHORT,
        count,
        "VEC4",
        None,
        Some(ARRAY_BUFFER),
    );
    let w_bytes: Vec<u8> = weights.iter().flat_map(|f| f.to_le_bytes()).collect();
    let a_w = bin.add(&w_bytes, FLOAT, count, "VEC4", None, Some(ARRAY_BUFFER));
    let a_bulge = bin.vec3(&bulge, Some(ARRAY_BUFFER));
    let a_pinch = bin.vec3(&pinch, Some(ARRAY_BUFFER));
    let i_bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
    let a_idx = bin.add(
        &i_bytes,
        UNSIGNED_INT,
        indices.len(),
        "SCALAR",
        None,
        Some(ELEMENT_ARRAY_BUFFER),
    );
    // Inverse bind matrices: joints sit on the Y axis, unrotated.
    let ibm: Vec<f32> = JOINTS
        .iter()
        .flat_map(|y| {
            let mut m = [0.0f32; 16];
            m[0] = 1.0;
            m[5] = 1.0;
            m[10] = 1.0;
            m[15] = 1.0;
            m[13] = -y;
            m
        })
        .collect();
    let a_ibm = bin.floats(&ibm, "MAT4", 16, false);

    // Bend: rotation keys every 0.125 s for every bone, and the bulge pulse.
    let keys: Vec<f32> = (0..=16).map(|i| i as f32 * BEND_SECONDS / 16.0).collect();
    let a_time = bin.floats(&keys, "SCALAR", 1, true);
    let mut channels = Vec::new();
    let mut samplers = Vec::new();
    for j in 0..JOINTS.len() {
        let rot: Vec<f32> = keys
            .iter()
            .flat_map(|t| {
                let half = bend_angle(j, *t) * 0.5;
                [0.0, 0.0, half.sin(), half.cos()]
            })
            .collect();
        let a = bin.floats(&rot, "VEC4", 4, false);
        samplers.push(json!({"input": a_time, "output": a, "interpolation": "LINEAR"}));
        channels.push(json!({
            "sampler": samplers.len() - 1,
            "target": {"node": 1 + j, "path": "rotation"},
        }));
    }
    let w: Vec<f32> = keys.iter().flat_map(|t| [bulge_weight(*t), 0.0]).collect();
    let a = bin.floats(&w, "SCALAR", 1, false);
    samplers.push(json!({"input": a_time, "output": a, "interpolation": "LINEAR"}));
    channels.push(json!({
        "sampler": samplers.len() - 1,
        "target": {"node": 0, "path": "weights"},
    }));

    let bone = |j: usize| {
        let local_y = if j == 0 {
            JOINTS[0]
        } else {
            JOINTS[j] - JOINTS[j - 1]
        };
        let mut n = json!({"name": format!("Bone{j}"), "translation": [0.0, local_y, 0.0]});
        if j + 1 < JOINTS.len() {
            n["children"] = json!([2 + j]);
        }
        n
    };
    let doc = json!({
        "asset": {"version": "2.0", "generator": "Misty Engine sample generator"},
        "scene": 0,
        "scenes": [{"name": "Scene", "nodes": [0, 1]}],
        "nodes": [
            {"name": "TentacleMesh", "mesh": 0, "skin": 0},
            bone(0), bone(1), bone(2), bone(3),
        ],
        "meshes": [{
            "name": "Tentacle",
            "primitives": [{
                "attributes": {
                    "POSITION": a_pos, "NORMAL": a_nrm, "TEXCOORD_0": a_uv,
                    "JOINTS_0": a_j, "WEIGHTS_0": a_w,
                },
                "indices": a_idx,
                "material": 0,
                "targets": [{"POSITION": a_bulge}, {"POSITION": a_pinch}],
            }],
            "weights": [0.0, 0.0],
            "extras": {"targetNames": SHAPES},
        }],
        "skins": [{"name": "Armature", "joints": [1, 2, 3, 4], "skeleton": 1,
                   "inverseBindMatrices": a_ibm}],
        "animations": [{"name": "Bend", "channels": channels, "samplers": samplers}],
        "materials": [{
            "name": "TentacleSkin",
            "pbrMetallicRoughness": {
                "baseColorFactor": [0.62, 0.16, 0.36, 1.0],
                "metallicFactor": 0.0,
                "roughnessFactor": 0.45,
            },
        }],
        "buffers": [{"byteLength": 0}],
        "bufferViews": bin.views,
        "accessors": bin.accessors,
    });
    let mut doc = doc;
    while !bin.data.len().is_multiple_of(4) {
        bin.data.push(0);
    }
    doc["buffers"][0]["byteLength"] = json!(bin.data.len());
    glb(&doc, &bin.data)
}

/// Pack JSON and a binary chunk as a GLB file.
fn glb(doc: &Value, bin: &[u8]) -> Vec<u8> {
    let mut text = serde_json::to_vec(doc).unwrap_or_default();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let total = 12 + 8 + text.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(bin);
    out
}
