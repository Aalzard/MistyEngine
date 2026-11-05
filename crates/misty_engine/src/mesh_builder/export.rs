//! Export to Model: write an Editable Mesh as a Wavefront `.obj` (one object per material, so
//! each imports as its own mesh). OBJ is right-handed, so X is mirrored and triangles are
//! written the other way round; the importer undoes both.

use std::fmt::Write as _;

use super::{EditableMesh, bake};

/// OBJ text of `mesh` named `name`, and the Materials element of each object in the file
/// (object `i` imports as mesh `i`; materials without faces are left out).
pub fn to_obj(mesh: &EditableMesh, name: &str) -> (String, Vec<u32>) {
    let parts = bake(mesh, None);
    let mut text = format!("# {name}, exported by Misty Engine's Mesh Builder\n");
    let mut materials = Vec::new();
    let mut base = 1usize;
    for (k, part) in parts.iter().enumerate().skip(1) {
        if part.indices.is_empty() {
            continue;
        }
        let material = (k - 1) as u32;
        materials.push(material);
        let _ = writeln!(
            text,
            "o {}_{}",
            name.replace(char::is_whitespace, "_"),
            material
        );
        for v in &part.vertices {
            let p = v.position;
            let _ = writeln!(text, "v {} {} {}", -p[0], p[1], p[2]);
        }
        for v in &part.vertices {
            let _ = writeln!(text, "vt {} {}", v.uv[0], 1.0 - v.uv[1]);
        }
        for v in &part.vertices {
            let n = v.normal;
            let _ = writeln!(text, "vn {} {} {}", -n[0], n[1], n[2]);
        }
        for t in part.indices.as_chunks::<3>().0 {
            let i = |k: u32| base + k as usize;
            let (a, b, c) = (i(t[0]), i(t[2]), i(t[1]));
            let _ = writeln!(text, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}");
        }
        base += part.vertices.len();
    }
    (text, materials)
}
