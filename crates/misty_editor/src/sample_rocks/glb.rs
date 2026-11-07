//! A minimal binary glTF writer for the rock meshes: one node per mesh (named), positions,
//! normals and UVs, 16- or 32-bit indices, no materials (the prefabs assign `.mat` assets).
//!
//! The engine is left-handed and glTF right-handed: import mirrors X and flips the winding,
//! so the writer does the same and the imported mesh equals the generated one exactly.

use misty_engine::render::mesh::MeshData;
use serde_json::{Value, json};

const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

#[derive(Default)]
struct Bin {
    data: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Bin {
    fn add(&mut self, bytes: &[u8], target: u32, accessor: Value) -> usize {
        while !self.data.len().is_multiple_of(4) {
            self.data.push(0);
        }
        self.views.push(json!({
            "buffer": 0,
            "byteOffset": self.data.len(),
            "byteLength": bytes.len(),
            "target": target,
        }));
        self.data.extend_from_slice(bytes);
        let mut acc = accessor;
        acc["bufferView"] = json!(self.views.len() - 1);
        self.accessors.push(acc);
        self.accessors.len() - 1
    }

    fn floats(&mut self, values: &[[f32; 3]]) -> usize {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for v in values {
            for i in 0..3 {
                lo[i] = lo[i].min(v[i]);
                hi[i] = hi[i].max(v[i]);
            }
        }
        let bytes: Vec<u8> = values
            .iter()
            .flatten()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        self.add(
            &bytes,
            ARRAY_BUFFER,
            json!({"componentType": FLOAT, "count": values.len(), "type": "VEC3",
                   "min": lo, "max": hi}),
        )
    }
}

/// `meshes` (name, data) as `.glb` bytes.
pub fn write(meshes: &[(&str, &MeshData)]) -> Vec<u8> {
    let mut bin = Bin::default();
    let mut gltf_meshes = Vec::new();
    for (name, data) in meshes {
        let positions: Vec<[f32; 3]> = data
            .vertices
            .iter()
            .map(|v| [-v.position[0], v.position[1], v.position[2]])
            .collect();
        let normals: Vec<[f32; 3]> = data
            .vertices
            .iter()
            .map(|v| [-v.normal[0], v.normal[1], v.normal[2]])
            .collect();
        let uvs: Vec<u8> = data
            .vertices
            .iter()
            .flat_map(|v| v.uv)
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let p = bin.floats(&positions);
        let n = bin.floats(&normals);
        let t = bin.add(
            &uvs,
            ARRAY_BUFFER,
            json!({"componentType": FLOAT, "count": data.vertices.len(), "type": "VEC2"}),
        );
        // Mirrored X: flip every triangle back to the same facing.
        let flipped: Vec<u32> = data
            .indices
            .chunks(3)
            .flat_map(|t| [t[0], t[2], t[1]])
            .collect();
        let small = data.vertices.len() <= u16::MAX as usize;
        let bytes: Vec<u8> = if small {
            flipped
                .iter()
                .flat_map(|&i| (i as u16).to_le_bytes())
                .collect()
        } else {
            flipped.iter().flat_map(|&i| i.to_le_bytes()).collect()
        };
        let i = bin.add(
            &bytes,
            ELEMENT_ARRAY_BUFFER,
            json!({"componentType": if small { UNSIGNED_SHORT } else { UNSIGNED_INT },
                   "count": flipped.len(), "type": "SCALAR"}),
        );
        gltf_meshes.push(json!({
            "name": name,
            "primitives": [{
                "attributes": {"POSITION": p, "NORMAL": n, "TEXCOORD_0": t},
                "indices": i,
            }],
        }));
    }
    while !bin.data.len().is_multiple_of(4) {
        bin.data.push(0);
    }
    let nodes: Vec<Value> = meshes
        .iter()
        .enumerate()
        .map(|(k, (name, _))| json!({"name": name, "mesh": k}))
        .collect();
    let doc = json!({
        "asset": {"version": "2.0", "generator": "Misty Engine sample rocks"},
        "scene": 0,
        "scenes": [{"nodes": (0..meshes.len()).collect::<Vec<_>>()}],
        "nodes": nodes,
        "meshes": gltf_meshes,
        "buffers": [{"byteLength": bin.data.len()}],
        "bufferViews": bin.views,
        "accessors": bin.accessors,
    });
    let mut text = serde_json::to_vec(&doc).unwrap_or_default();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let total = 12 + 8 + text.len() + 8 + bin.data.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(&(bin.data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin.data);
    out
}
