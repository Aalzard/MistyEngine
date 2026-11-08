//! Cloth meshes made in code and vertex colours read from glTF files.
//!
//! [`flag_glb`] is a subdivided flag (no downloaded assets) whose vertex colour red channel holds
//! the max distance: 0 along the pole edge (pinned), 1 elsewhere. [`max_distances_from_colors`]
//! turns a mesh's vertex colour channel into a Cloth's max distances.

use serde_json::{Value, json};

use crate::scene::{ColorChannel, MeshRef};

/// Flag size (metres) and grid (quads across, quads down).
pub const FLAG_WIDTH: f32 = 2.4;
pub const FLAG_HEIGHT: f32 = 1.5;
pub const FLAG_COLUMNS: usize = 24;
pub const FLAG_ROWS: usize = 15;

const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;

/// Positions, uvs, colours and triangle indices of a mesh.
pub type Grid = (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[f32; 4]>, Vec<u32>);

/// Flag vertices: positions (pole along +Y at x = 0, cloth towards -X in the file, which the
/// importer's right- to left-handed mirror turns into +X), uvs, colours.
pub fn flag_grid() -> Grid {
    let (cols, rows) = (FLAG_COLUMNS, FLAG_ROWS);
    let mut pos = Vec::new();
    let mut uv = Vec::new();
    let mut col = Vec::new();
    for r in 0..=rows {
        for c in 0..=cols {
            let u = c as f32 / cols as f32;
            let v = r as f32 / rows as f32;
            pos.push([-u * FLAG_WIDTH, -v * FLAG_HEIGHT, 0.0]);
            uv.push([u, v]);
            // Pinned along the pole, free (1 = up to a metre from the mesh) elsewhere.
            let free = if c == 0 { 0.0 } else { 1.0 };
            col.push([free, free, free, 1.0]);
        }
    }
    let mut idx = Vec::new();
    let w = (cols + 1) as u32;
    for r in 0..rows as u32 {
        for c in 0..cols as u32 {
            let a = r * w + c;
            let (b, d, e) = (a + 1, a + w, a + w + 1);
            // Counter-clockwise seen from +Z in the (mirrored) file.
            idx.extend_from_slice(&[a, b, d, b, e, d]);
        }
    }
    (pos, uv, col, idx)
}

/// The flag as `.glb` bytes (one mesh "Flag" with POSITION, NORMAL, TEXCOORD_0, COLOR_0).
pub fn flag_glb() -> Vec<u8> {
    let (pos, uv, col, idx) = flag_grid();
    let normals = vec![[0.0f32, 0.0, 1.0]; pos.len()];
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut add = |bytes: Vec<u8>, component: u32, count: usize, kind: &str, mm: Option<Value>| {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        views.push(json!({"buffer": 0, "byteOffset": bin.len(), "byteLength": bytes.len()}));
        bin.extend_from_slice(&bytes);
        let mut acc = json!({
            "bufferView": views.len() - 1,
            "componentType": component,
            "count": count,
            "type": kind,
        });
        if let Some(mm) = mm {
            acc["min"] = mm["min"].clone();
            acc["max"] = mm["max"].clone();
        }
        accessors.push(acc);
        accessors.len() - 1
    };
    let floats = |v: &[f32]| v.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>();
    let flat3: Vec<f32> = pos.iter().flatten().copied().collect();
    let p = add(
        floats(&flat3),
        FLOAT,
        pos.len(),
        "VEC3",
        Some(json!({"min": [-FLAG_WIDTH, -FLAG_HEIGHT, 0.0], "max": [0.0, 0.0, 0.0]})),
    );
    let nflat: Vec<f32> = normals.iter().flatten().copied().collect();
    let n = add(floats(&nflat), FLOAT, normals.len(), "VEC3", None);
    let uflat: Vec<f32> = uv.iter().flatten().copied().collect();
    let t = add(floats(&uflat), FLOAT, uv.len(), "VEC2", None);
    let cflat: Vec<f32> = col.iter().flatten().copied().collect();
    let c = add(floats(&cflat), FLOAT, col.len(), "VEC4", None);
    let ibytes: Vec<u8> = idx.iter().flat_map(|i| i.to_le_bytes()).collect();
    let i = add(ibytes, UNSIGNED_INT, idx.len(), "SCALAR", None);
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let doc = json!({
        "asset": {"version": "2.0", "generator": "Misty Engine cloth sample"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": "Flag", "mesh": 0}],
        "meshes": [{
            "name": "Flag",
            "primitives": [{
                "attributes": {"POSITION": p, "NORMAL": n, "TEXCOORD_0": t, "COLOR_0": c},
                "indices": i,
            }],
        }],
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": views,
        "accessors": accessors,
    });
    glb(&doc, &bin)
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

/// Vertex colours (COLOR_0, RGBA) of mesh `mesh` of the glTF file at `path`, counting meshes the
/// way the importer does (each triangle primitive with positions is one mesh).
pub fn gltf_colors(path: &std::path::Path, mesh: usize) -> Result<Option<Vec<[f32; 4]>>, String> {
    let bytes = crate::vfs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (doc, buffers, _) =
        gltf::import_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut k = 0;
    for m in doc.meshes() {
        for prim in m.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = prim.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
            if reader.read_positions().is_none() {
                continue;
            }
            if k == mesh {
                return Ok(reader.read_colors(0).map(|c| c.into_rgba_f32().collect()));
            }
            k += 1;
        }
    }
    Err(format!("{}: no mesh {mesh}", path.display()))
}

/// Max distances from a vertex colour channel: channel value × `scale` (0 pins the vertex).
pub fn max_distances_from_colors(
    mesh: &MeshRef,
    channel: ColorChannel,
    scale: f32,
) -> Result<Vec<f32>, String> {
    let (path, index) = mesh
        .split_asset()
        .ok_or("Vertex colours need an imported glTF mesh.")?;
    if !path.to_lowercase().ends_with(".glb") && !path.to_lowercase().ends_with(".gltf") {
        return Err("Vertex colours are read from glTF files only.".into());
    }
    let colors = gltf_colors(&crate::assets::absolute(path), index)?
        .ok_or("The mesh has no vertex colours.")?;
    let expected = crate::models::mesh(mesh).map(|m| m.vertices.len());
    if expected.is_some_and(|n| n != colors.len()) {
        return Err("The vertex colours don't match the imported mesh's vertices.".into());
    }
    Ok(colors
        .iter()
        .map(|c| c[channel.index()].clamp(0.0, 1.0) * scale.max(0.0))
        .collect())
}
