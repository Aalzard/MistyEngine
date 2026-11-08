//! Sample FBX files made in code with [`super::fbx_write`] (no downloaded assets):
//!
//! - `Tentacle.fbx`: the SkinningDemo tentacle ([`crate::skinning::sample`]) as an FBX the way
//!   Blender or 3ds Max write it: Z up, centimetres, quads, four bones with a skin, the Bulge
//!   and Pinch blend shapes and the looping "Bend" take. Imported, it matches `Tentacle.glb`.
//! - `Crate.fbx`: a static box, Y up, metres, two materials (a wood texture next to the file
//!   and an embedded normal map) and a child node.

use std::f64::consts::TAU;

use glam::DMat4;

use super::fbx_write::{Axes, Document};
use crate::skinning::sample::{
    BEND_SECONDS, HEIGHT, JOINTS, SHAPES, bend_angle, bulge_weight, weights_at,
};

const RINGS: usize = 24;
const SIDES: usize = 16;

fn radius(y: f64) -> f64 {
    0.3 * (1.0 - 0.65 * y / HEIGHT as f64) + 0.02
}

/// glTF space (Y up, metres) → the file's (Z up, centimetres).
fn to_file(p: [f64; 3]) -> [f64; 3] {
    [p[0] * 100.0, -p[2] * 100.0, p[1] * 100.0]
}

/// A direction (no unit change).
fn dir_to_file(n: [f64; 3]) -> [f64; 3] {
    [n[0], -n[2], n[1]]
}

/// The tentacle as `.fbx` bytes.
pub fn tentacle_fbx() -> Vec<u8> {
    let mut positions = Vec::new();
    let mut normals_cp = Vec::new();
    let mut uvs_cp = Vec::new();
    let mut radial = Vec::new();
    for k in 0..=RINGS {
        let y = HEIGHT as f64 * k as f64 / RINGS as f64;
        let r = radius(y);
        for s in 0..=SIDES {
            let a = TAU * s as f64 / SIDES as f64;
            let (c, sn) = (a.cos(), a.sin());
            positions.push([r * c, y, r * sn]);
            let l = (c * c + 0.08 * 0.08 + sn * sn).sqrt();
            normals_cp.push([c / l, 0.08 / l, sn / l]);
            uvs_cp.push([s as f64 / SIDES as f64, y / HEIGHT as f64]);
            radial.push([c, sn]);
        }
    }
    let tip = positions.len() as i32;
    positions.push([0.0, HEIGHT as f64 + 0.04, 0.0]);
    normals_cp.push([0.0, 1.0, 0.0]);
    uvs_cp.push([0.5, 1.0]);
    radial.push([0.0, 0.0]);

    let row = SIDES as i32 + 1;
    let mut polygons: Vec<Vec<i32>> = Vec::new();
    for k in 0..RINGS as i32 {
        for s in 0..SIDES as i32 {
            let (v00, v01) = (k * row + s, k * row + s + 1);
            let (v10, v11) = (v00 + row, v01 + row);
            // Counter-clockwise seen from outside.
            polygons.push(vec![v00, v10, v11, v01]);
        }
    }
    let top = RINGS as i32 * row;
    for s in 0..SIDES as i32 {
        polygons.push(vec![top + s, tip, top + s + 1]);
    }
    let corners: Vec<i32> = polygons.iter().flatten().copied().collect();
    let normals: Vec<[f64; 3]> = corners
        .iter()
        .map(|&v| dir_to_file(normals_cp[v as usize]))
        .collect();
    let uvs: Vec<[f64; 2]> = corners.iter().map(|&v| uvs_cp[v as usize]).collect();
    let file_positions: Vec<[f64; 3]> = positions.iter().map(|p| to_file(*p)).collect();

    let mut doc = Document::new();
    let mesh = doc.model("TentacleMesh", "Mesh", 0, [0.0; 3], [0.0; 3], [1.0; 3]);
    let geometry = doc.geometry("Tentacle", &file_positions, &polygons, &normals, &uvs, &[]);
    doc.connect(geometry, mesh);
    let skin_color = [0.62f64, 0.16, 0.36].map(|c| c.powf(1.0 / 2.2));
    let material = doc.material("TentacleSkin", skin_color, 20.0, [0.0; 3]);
    doc.connect(material, mesh);

    let mut bones = Vec::new();
    let mut parent = 0;
    for (j, y) in JOINTS.iter().enumerate() {
        let local = if j == 0 { *y } else { y - JOINTS[j - 1] };
        let id = doc.model(
            &format!("Bone{j}"),
            "LimbNode",
            parent,
            [0.0, 0.0, local as f64 * 100.0],
            [0.0; 3],
            [1.0; 3],
        );
        bones.push(id);
        parent = id;
    }
    let mut clusters = Vec::new();
    for (j, bone) in bones.iter().enumerate() {
        let mut indexes = Vec::new();
        let mut weights = Vec::new();
        for (v, p) in positions.iter().enumerate() {
            let (js, ws) = weights_at(p[1] as f32);
            for (jj, w) in js.iter().zip(ws) {
                if *jj as usize == j && w > 0.0 {
                    indexes.push(v as i32);
                    weights.push(w as f64);
                }
            }
        }
        let world = DMat4::from_translation(glam::DVec3::new(0.0, 0.0, JOINTS[j] as f64 * 100.0));
        clusters.push((*bone, indexes, weights, world, DMat4::IDENTITY));
    }
    doc.skin(geometry, &clusters);

    let shape = |which: usize| {
        let mut indexes = Vec::new();
        let mut offsets = Vec::new();
        for (v, p) in positions.iter().enumerate() {
            let y = p[1];
            let [c, s] = radial[v];
            let k = if which == 0 {
                0.16 * (-((y - 1.0) / 0.28).powi(2)).exp()
            } else {
                let t = ((y - 1.0) / (HEIGHT as f64 - 1.0)).clamp(0.0, 1.0);
                -0.75 * radius(y) * t * t * (3.0 - 2.0 * t)
            };
            let o = [c * k, 0.0, s * k];
            if o.iter().any(|x| x.abs() > 1e-9) {
                indexes.push(v as i32);
                offsets.push(to_file(o));
            }
        }
        (SHAPES[which], indexes, offsets)
    };
    let channels = doc.blend_shapes(geometry, &[shape(0), shape(1)]);

    let layer = doc.anim_stack("Bend", BEND_SECONDS as f64);
    let times: Vec<f64> = (0..=16)
        .map(|i| i as f64 * BEND_SECONDS as f64 / 16.0)
        .collect();
    for (j, bone) in bones.iter().enumerate() {
        // About glTF's +Z, which is the file's -Y.
        let angles: Vec<f32> = times
            .iter()
            .map(|t| -bend_angle(j, *t as f32).to_degrees())
            .collect();
        let zeros = vec![0.0f32; times.len()];
        doc.anim_curves(
            layer,
            *bone,
            "Lcl Rotation",
            &[
                ("d|X", times.clone(), zeros.clone()),
                ("d|Y", times.clone(), angles),
                ("d|Z", times.clone(), zeros),
            ],
        );
    }
    let bulge: Vec<f32> = times
        .iter()
        .map(|t| bulge_weight(*t as f32) * 100.0)
        .collect();
    doc.anim_curves(
        layer,
        channels[0],
        "DeformPercent",
        &[("d|DeformPercent", times.clone(), bulge)],
    );
    doc.finish(Axes {
        up: 2,
        unit_scale: 1.0,
    })
}

/// A 4×4 tangent-space normal map (bumps), as PNG bytes.
pub fn normal_png() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(4, 4, |x, y| {
        let v = if (x + y) % 2 == 0 { 150 } else { 106 };
        image::Rgba([v, 128, 255, 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .expect("encode png");
    out.into_inner()
}

/// The crate as `.fbx` bytes; it references `Wood.png` next to the file.
pub fn crate_fbx() -> Vec<u8> {
    let p = [
        [-0.5, 0.0, -0.5],
        [0.5, 0.0, -0.5],
        [0.5, 0.0, 0.5],
        [-0.5, 0.0, 0.5],
        [-0.5, 1.0, -0.5],
        [0.5, 1.0, -0.5],
        [0.5, 1.0, 0.5],
        [-0.5, 1.0, 0.5],
    ];
    // Counter-clockwise seen from outside (right-handed, Y up); bottom and top last.
    let polygons = vec![
        vec![4, 5, 1, 0], // -Z
        vec![5, 6, 2, 1], // +X
        vec![6, 7, 3, 2], // +Z
        vec![7, 4, 0, 3], // -X
        vec![0, 1, 2, 3], // bottom
        vec![4, 7, 6, 5], // top
    ];
    let face_normals = [
        [0.0, 0.0, -1.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    for (f, poly) in polygons.iter().enumerate() {
        for k in 0..poly.len() {
            normals.push(face_normals[f]);
            uvs.push([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]][k]);
        }
    }
    let mut doc = Document::new();
    let root = doc.model("Crate", "Mesh", 0, [0.0, 0.0, 0.0], [0.0; 3], [1.0; 3]);
    doc.model(
        "Handle",
        "Null",
        root,
        [0.0, 1.25, 0.0],
        [0.0, 45.0, 0.0],
        [1.0; 3],
    );
    let geometry = doc.geometry(
        "CrateMesh",
        &p,
        &polygons,
        &normals,
        &uvs,
        &[0, 0, 0, 0, 1, 1],
    );
    doc.connect(geometry, root);
    let wood = doc.material("Wood", [0.8, 0.6, 0.4], 2.0, [0.0; 3]);
    let metal = doc.material("Metal", [0.5, 0.5, 0.55], 80.0, [0.0, 0.1, 0.2]);
    doc.connect(wood, root);
    doc.connect(metal, root);
    doc.texture(wood, "DiffuseColor", "Wood.png", None);
    doc.texture(metal, "NormalMap", "MetalNormal.png", Some(normal_png()));
    doc.finish(Axes {
        up: 1,
        unit_scale: 100.0,
    })
}
