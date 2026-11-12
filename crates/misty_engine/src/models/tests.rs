use std::path::PathBuf;

use glam::Vec3;

use super::*;
use crate::scene::MeshRef;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("misty_models_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Every triangle's face normal agrees with its vertex normals.
fn assert_outward(data: &MeshData) {
    for t in data.indices.chunks(3) {
        let v = |i: u32| data.vertices[i as usize];
        let p = |i: u32| Vec3::from(v(i).position);
        let face = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
        let n: Vec3 = t.iter().map(|i| Vec3::from(v(*i).normal)).sum();
        assert!(face.dot(n) > 0.0, "triangle {t:?} faces inward");
    }
}

const RAMP_OBJ: &str = "\
o Ramp
v 0 0 0
v 1 0 0
v 1 0 2
v 0 0 2
v 1 1 2
v 0 1 2
vt 0 0
vt 1 0
vt 1 1
vt 0 1
f 1/1 4/4 3/3 2/2
f 4/1 6/4 5/3 3/2
f 1/1 2/2 5/3 6/4
";

#[test]
fn obj_imports_mirrored_with_outward_faces_and_tangents() {
    let dir = temp_dir("obj");
    let path = dir.join("Ramp.obj");
    std::fs::write(&path, RAMP_OBJ).unwrap();
    let model = load(&path).unwrap();
    assert_eq!(model.meshes.len(), 1);
    assert_eq!(model.nodes.len(), 1);
    assert_eq!(model.nodes[0].name, "Ramp");
    let mesh = &model.meshes[0];
    assert_eq!(mesh.data.indices.len(), 3 * 6, "three quads, triangulated");
    // X is mirrored: the file's 0..1 range becomes -1..0.
    assert_eq!(
        mesh.bounds,
        (Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 2.0))
    );
    assert_outward(&mesh.data);
    for v in &mesh.data.vertices {
        let t = Vec3::from_slice(&v.tangent[..3]);
        assert!((t.length() - 1.0).abs() < 1e-3);
        assert!(t.dot(Vec3::from(v.normal)).abs() < 1e-3);
    }
    std::fs::remove_dir_all(dir).ok();
}

fn pad(mut v: Vec<u8>, with: u8) -> Vec<u8> {
    while !v.len().is_multiple_of(4) {
        v.push(with);
    }
    v
}

/// 2×2 PNG for the embedded-texture test.
fn tiny_png() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(2, 2, |x, y| {
        image::Rgba([x as u8 * 255, y as u8 * 255, 0, 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// A GLB with one quad mesh on a child node, a red material and an embedded PNG.
fn build_glb() -> Vec<u8> {
    let positions: [[f32; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let normals = [[0.0f32, 0.0, 1.0]; 4];
    let uvs: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    // Counter-clockwise seen from +Z (glTF front faces).
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
    let mut bin = Vec::new();
    bin.extend_from_slice(bytemuck::cast_slice(&positions)); // 0..48
    bin.extend_from_slice(bytemuck::cast_slice(&normals)); // 48..96
    bin.extend_from_slice(bytemuck::cast_slice(&uvs)); // 96..128
    bin.extend_from_slice(bytemuck::cast_slice(&indices)); // 128..140
    bin = pad(bin, 0);
    let png = tiny_png();
    let png_offset = bin.len();
    bin.extend_from_slice(&png);
    let bin = pad(bin, 0);

    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "scene": 0,
  "scenes": [{{ "nodes": [0] }}],
  "nodes": [
    {{ "name": "Ship", "translation": [1, 2, 3], "children": [1] }},
    {{ "name": "Hull", "mesh": 0, "scale": [2, 2, 2] }}
  ],
  "meshes": [{{ "name": "HullMesh", "primitives": [{{
    "attributes": {{ "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }},
    "indices": 3, "material": 0 }}] }}],
  "materials": [{{ "name": "Paint",
    "pbrMetallicRoughness": {{ "baseColorFactor": [1, 0, 0, 1], "metallicFactor": 0.5,
      "roughnessFactor": 0.25, "baseColorTexture": {{ "index": 0 }} }} }}],
  "textures": [{{ "source": 0 }}],
  "images": [{{ "bufferView": 4, "mimeType": "image/png" }}],
  "buffers": [{{ "byteLength": {len} }}],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 48, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 96, "byteLength": 32 }},
    {{ "buffer": 0, "byteOffset": 128, "byteLength": 12 }},
    {{ "buffer": 0, "byteOffset": {png_offset}, "byteLength": {png_len} }}
  ],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
       "min": [0, 0, 0], "max": [1, 1, 0] }},
    {{ "bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3" }},
    {{ "bufferView": 2, "componentType": 5126, "count": 4, "type": "VEC2" }},
    {{ "bufferView": 3, "componentType": 5123, "count": 6, "type": "SCALAR" }}
  ]
}}"#,
        len = bin.len(),
        png_len = png.len(),
    );
    let json = pad(json.into_bytes(), b' ');
    let mut glb = Vec::new();
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&((12 + 8 + json.len() + 8 + bin.len()) as u32).to_le_bytes());
    glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&json);
    glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&bin);
    glb
}

#[test]
fn glb_imports_hierarchy_meshes_and_materials() {
    let dir = temp_dir("glb");
    let path = dir.join("Ship.glb");
    std::fs::write(&path, build_glb()).unwrap();
    let model = load(&path).unwrap();

    assert_eq!(model.nodes.len(), 2);
    let (ship, hull) = (&model.nodes[0], &model.nodes[1]);
    assert_eq!((ship.name.as_str(), ship.parent), ("Ship", None));
    assert_eq!((hull.name.as_str(), hull.parent), ("Hull", Some(0)));
    assert_eq!(ship.position, Vec3::new(-1.0, 2.0, 3.0), "X mirrored");
    assert_eq!(hull.scale, Vec3::splat(2.0));
    assert_eq!(hull.meshes, vec![0]);

    let mesh = &model.meshes[0];
    assert_eq!(mesh.name, "HullMesh");
    assert_eq!(mesh.material, Some(0));
    assert_eq!(
        mesh.bounds,
        (Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0))
    );
    assert_outward(&mesh.data);
    // The quad faced +Z in the file and still does after mirroring X.
    assert!(
        mesh.data
            .vertices
            .iter()
            .all(|v| v.normal == [0.0, 0.0, 1.0])
    );

    let mat = &model.materials[0];
    assert_eq!(mat.name, "Paint");
    assert_eq!(mat.material.color, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(mat.material.metallic, 0.5);
    assert_eq!(mat.material.smoothness, 0.75);
    assert!(matches!(mat.albedo, Some(TextureSource::Embedded(_))));

    // Extraction writes the .mat and the embedded PNG next to the model, once.
    let model_path = path.to_string_lossy().replace('\\', "/");
    let created = extract_materials(&model_path, &model).unwrap();
    assert_eq!(created.len(), 2, "{created:?}");
    let mat_rel = material_asset_path(&model_path, &model, 0).unwrap();
    assert!(mat_rel.ends_with("Ship_Paint.mat"));
    let saved = Material::load(Path::new(&mat_rel)).unwrap();
    let albedo = saved.albedo_map.unwrap();
    assert!(albedo.ends_with("Ship_Paint_Albedo.png"));
    assert_eq!(image::open(&albedo).unwrap().width(), 2);
    assert!(extract_materials(&model_path, &model).unwrap().is_empty());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn cached_meshes_resolve_through_mesh_refs() {
    let dir = temp_dir("cache");
    let path = dir.join("Ramp.obj");
    std::fs::write(&path, RAMP_OBJ).unwrap();
    let rel = path.to_string_lossy().replace('\\', "/");
    let r = MeshRef::asset(&rel, 0);
    assert_eq!(r.split_asset(), Some((rel.as_str(), 0)));
    assert_eq!(mesh(&r).unwrap().indices.len(), 18);
    assert_eq!(bounds(&r).unwrap().1, Vec3::new(0.0, 1.0, 2.0));
    assert!(display_name(&r).ends_with("Ramp.obj ▸ Ramp"));
    assert!(with_edges(&r, |e| e.len()).unwrap() > 0);
    assert!(
        mesh(&MeshRef::asset(&rel, 5)).is_none(),
        "index out of range"
    );
    let cube = MeshRef::Primitive(crate::scene::PrimitiveMesh::Cube);
    assert_eq!(bounds(&cube), Some((Vec3::splat(-0.5), Vec3::splat(0.5))));
    std::fs::remove_dir_all(dir).ok();
}

/// A `.gltf` with one node `nodes` (JSON array) and, when `times` is not empty, one
/// translation animation on node 0 with those key times and `values` (VEC3 per key, or three
/// per key for CUBICSPLINE).
fn write_anim_gltf(
    dir: &Path,
    nodes: &str,
    times: &[f32],
    values: &[f32],
    interp: &str,
) -> PathBuf {
    let mut bin: Vec<u8> = Vec::new();
    for f in times.iter().chain(values) {
        bin.extend_from_slice(&f.to_le_bytes());
    }
    std::fs::write(dir.join("anim.bin"), &bin).unwrap();
    let (tl, vl) = (times.len() * 4, values.len() * 4);
    let animations = if times.is_empty() {
        String::new()
    } else {
        format!(
            r#","buffers": [{{ "uri": "anim.bin", "byteLength": {len} }}],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": {tl} }},
    {{ "buffer": 0, "byteOffset": {tl}, "byteLength": {vl} }}
  ],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": {tc}, "type": "SCALAR",
       "min": [{tmin}], "max": [{tmax}] }},
    {{ "bufferView": 1, "componentType": 5126, "count": {vc}, "type": "VEC3" }}
  ],
  "animations": [{{
    "channels": [{{ "sampler": 0, "target": {{ "node": 0, "path": "translation" }} }}],
    "samplers": [{{ "input": 0, "output": 1, "interpolation": "{interp}" }}]
  }}]"#,
            len = bin.len(),
            tc = times.len(),
            vc = values.len() / 3,
            tmin = times[0],
            tmax = times[times.len() - 1],
        )
    };
    let json = format!(
        r#"{{ "asset": {{ "version": "2.0" }}, "scene": 0, "scenes": [{{ "nodes": [0] }}],
  "nodes": {nodes}{animations} }}"#
    );
    let path = dir.join("Anim.gltf");
    std::fs::write(&path, json).unwrap();
    path
}

#[test]
fn gltf_animations_with_too_few_values_import_without_panicking() {
    let dir = temp_dir("gltf_short_anim");
    let nodes = r#"[{ "name": "A" }]"#;
    // Two keys, one translation.
    let path = write_anim_gltf(&dir, nodes, &[0.0, 1.0], &[1.0, 2.0, 3.0], "LINEAR");
    let model = load(&path).unwrap();
    assert!(model.animations.iter().all(|a| a.clip.curves.is_empty()));
    // Cubic splines need three values per key: two keys with two values each are too few.
    let path = write_anim_gltf(&dir, nodes, &[0.0, 1.0], &[0.0; 6], "CUBICSPLINE");
    assert!(load(&path).is_ok());
    // Well-formed: the track imports.
    let path = write_anim_gltf(
        &dir,
        nodes,
        &[0.0, 1.0],
        &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        "LINEAR",
    );
    let model = load(&path).unwrap();
    assert!(!model.animations[0].clip.curves.is_empty());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn gltf_node_cycles_import_each_node_once() {
    let dir = temp_dir("gltf_cycle");
    let path = write_anim_gltf(
        &dir,
        r#"[{ "name": "A", "children": [1] }, { "name": "B", "children": [0, 1] }]"#,
        &[],
        &[],
        "LINEAR",
    );
    let model = load(&path).unwrap();
    assert_eq!(model.nodes.len(), 2);
    std::fs::remove_dir_all(dir).ok();
}
