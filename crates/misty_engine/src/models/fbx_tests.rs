//! FBX import (static, skinned, blend shapes, animation, materials), the model import settings
//! (axes, units, scale, normals, clip splitting, round trip) and Generate LODs.

use std::path::{Path, PathBuf};

use glam::{Mat4, Quat, Vec3};

use super::fbx_sample::{crate_fbx, normal_png, tentacle_fbx};
use super::settings::{AxisConversion, ClipSettings, ModelImporter, VectorSource};
use super::*;
use crate::animation::Property;
use crate::scene::Transform;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("misty_fbx_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

fn close(a: Vec3, b: Vec3, eps: f32) -> bool {
    (a - b).abs().max_element() <= eps
}

/// Every triangle's face normal agrees with its vertex normals (outward after mirroring).
fn assert_outward(data: &MeshData) {
    for t in data.indices.chunks(3) {
        let v = |i: u32| data.vertices[i as usize];
        let p = |i: u32| Vec3::from(v(i).position);
        let face = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
        let n: Vec3 = t.iter().map(|i| Vec3::from(v(*i).normal)).sum();
        assert!(face.dot(n) > 0.0, "triangle {t:?} faces inward");
    }
}

/// Model-space matrix of node `i`, with the clip's values at `t` where it animates them.
fn world(model: &Model, i: usize, clip: Option<(&crate::animation::AnimationClip, f32)>) -> Mat4 {
    let n = &model.nodes[i];
    let path = model.node_path(i);
    let mut pos = n.position;
    let mut rot = n.rotation;
    let mut scale = n.scale;
    if let Some((clip, t)) = clip {
        let get = |p: Property, rest: f32| clip.curve(&path, p).map_or(rest, |c| c.evaluate(t));
        let mut tr = Transform::default();
        tr.set_rotation(rot);
        let e = tr.euler();
        let e = Vec3::new(
            get(Property::Rotation(0), e.x),
            get(Property::Rotation(1), e.y),
            get(Property::Rotation(2), e.z),
        );
        tr.set_euler(e);
        rot = tr.rotation;
        pos = Vec3::new(
            get(Property::Position(0), pos.x),
            get(Property::Position(1), pos.y),
            get(Property::Position(2), pos.z),
        );
        scale = Vec3::new(
            get(Property::Scale(0), scale.x),
            get(Property::Scale(1), scale.y),
            get(Property::Scale(2), scale.z),
        );
    }
    let local = Mat4::from_scale_rotation_translation(scale, rot, pos);
    match n.parent {
        Some(p) => world(model, p, clip) * local,
        None => local,
    }
}

/// Model-space bounds of every node's meshes (Z-up files carry their axis conversion on the
/// root nodes, like Unity's import).
fn world_bounds(model: &Model) -> (Vec3, Vec3) {
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for (i, n) in model.nodes.iter().enumerate() {
        let m = world(model, i, None);
        for &k in &n.meshes {
            for v in &model.meshes[k].data.vertices {
                let p = m.transform_point3(Vec3::from(v.position));
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
    }
    (lo, hi)
}

fn node(model: &Model, name: &str) -> usize {
    model
        .nodes
        .iter()
        .position(|n| n.name == name)
        .unwrap_or_else(|| panic!("no node {name}"))
}

/// Skinned position of vertex `v` of mesh `m` (on node `n`) with the bones at `clip`/`t`.
fn skinned(
    model: &Model,
    n: usize,
    m: usize,
    v: usize,
    clip: Option<(&crate::animation::AnimationClip, f32)>,
) -> Vec3 {
    let mesh = &model.meshes[m];
    let skin = mesh.skin.as_ref().expect("skinned");
    let joints = &model.skins[model.nodes[n].skin.expect("skin")].joints;
    let p = Vec3::from(mesh.data.vertices[v].position);
    let mut out = Vec3::ZERO;
    for k in 0..4 {
        let w = skin.weights[v][k];
        if w > 0.0 {
            let j = skin.joints[v][k] as usize;
            out += w * (world(model, joints[j], clip) * skin.bind_poses[j]).transform_point3(p);
        }
    }
    out
}

fn tentacles(dir: &Path) -> (Model, Model) {
    let fbx = write(dir, "Tentacle.fbx", &tentacle_fbx());
    let glb = write(
        dir,
        "Tentacle.glb",
        &crate::skinning::sample::tentacle_glb(),
    );
    (load(&fbx).unwrap(), load(&glb).unwrap())
}

#[test]
fn fbx_static_model_has_parts_materials_textures_and_hierarchy() {
    let dir = temp_dir("crate");
    let wood = image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 150, 100, 255]));
    wood.save(dir.join("Wood.png")).unwrap();
    let path = write(&dir, "Crate.fbx", &crate_fbx());
    let model = load(&path).unwrap();

    let names: Vec<&str> = model.nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["Crate", "Handle"]);
    let handle = &model.nodes[1];
    assert_eq!(handle.parent, Some(0));
    assert!(close(handle.position, Vec3::new(0.0, 1.25, 0.0), 1e-5));
    // 45° about Y in the right-handed file is -45° in engine space (X mirrored).
    let expected = Quat::from_rotation_y(-45f32.to_radians());
    assert!(
        handle.rotation.dot(expected).abs() > 0.9999,
        "{:?}",
        handle.rotation
    );

    // One mesh per material part, like glTF primitives.
    assert_eq!(model.meshes.len(), 2);
    assert_eq!(model.nodes[0].meshes, [0, 1]);
    let tris: Vec<usize> = model
        .meshes
        .iter()
        .map(|m| m.data.indices.len() / 3)
        .collect();
    assert_eq!(
        tris,
        [8, 4],
        "four wood quads, two metal quads, triangulated"
    );
    assert_eq!(model.meshes[0].material, Some(0));
    assert_eq!(model.meshes[1].material, Some(1));
    for m in &model.meshes {
        assert_outward(&m.data);
        assert!(m.file_normals);
    }
    let (lo, hi) = model.meshes[0].bounds;
    assert!(close(lo, Vec3::new(-0.5, 0.0, -0.5), 1e-5), "{lo:?}");
    assert!(close(hi, Vec3::new(0.5, 1.0, 0.5), 1e-5), "{hi:?}");
    // FBX v grows upwards: flipped like OBJ.
    assert!(
        model.meshes[0]
            .data
            .vertices
            .iter()
            .any(|v| v.uv == [0.0, 1.0])
    );

    let wood_m = &model.materials[0];
    assert_eq!(wood_m.name, "Wood");
    let c = wood_m.material.color;
    assert!((c[0] - 0.8).abs() < 1e-4 && (c[1] - 0.6).abs() < 1e-4 && (c[2] - 0.4).abs() < 1e-4);
    // Outside a project the texture is read so extraction copies it in.
    match &wood_m.albedo {
        Some(TextureSource::Embedded(b)) => {
            assert_eq!(*b, std::fs::read(dir.join("Wood.png")).unwrap())
        }
        Some(TextureSource::File(p)) => assert!(p.ends_with("Wood.png")),
        None => panic!("wood texture not found"),
    }
    let metal = &model.materials[1];
    assert_eq!(metal.name, "Metal");
    assert!(metal.material.emission_enabled);
    match &metal.normal {
        Some(TextureSource::Embedded(b)) => assert_eq!(*b, normal_png()),
        other => panic!("embedded normal map expected, got {other:?}"),
    }
    assert!(model.skins.is_empty() && model.animations.is_empty());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn fbx_skinned_tentacle_matches_the_gltf_one() {
    let dir = temp_dir("tentacle");
    let (fbx, glb) = tentacles(&dir);
    // Z-up centimetres came in as Y-up metres: same shape as the glTF.
    assert_eq!(fbx.meshes.len(), 1);
    assert_eq!(
        fbx.meshes[0].data.vertices.len(),
        glb.meshes[0].data.vertices.len()
    );
    assert_eq!(
        fbx.meshes[0].data.indices.len(),
        glb.meshes[0].data.indices.len(),
        "quads triangulated"
    );
    let (a, b) = (world_bounds(&fbx), world_bounds(&glb));
    assert!(
        close(a.0, b.0, 1e-4) && close(a.1, b.1, 1e-4),
        "{a:?} vs {b:?}"
    );
    assert_outward(&fbx.meshes[0].data);

    // Skeleton: same bones, a skin whose root is Bone0.
    for j in 0..4 {
        let name = format!("Bone{j}");
        let (f, g) = (node(&fbx, &name), node(&glb, &name));
        assert_eq!(fbx.node_path(f), glb.node_path(g));
        let (pf, pg) = (
            world(&fbx, f, None).transform_point3(Vec3::ZERO),
            world(&glb, g, None).transform_point3(Vec3::ZERO),
        );
        assert!(close(pf, pg, 1e-4), "{name}: {pf:?} vs {pg:?}");
    }
    assert_eq!(fbx.skins.len(), 1);
    assert_eq!(fbx.skins[0].joints.len(), 4);
    assert_eq!(fbx.skins[0].root, Some(node(&fbx, "Bone0")));
    let mesh_node = node(&fbx, "TentacleMesh");
    assert_eq!(fbx.nodes[mesh_node].skin, Some(0));
    let skin = fbx.meshes[0].skin.as_ref().unwrap();
    assert_eq!(skin.bind_poses.len(), 4);
    for w in &skin.weights {
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-4);
    }
    // At rest the skin reproduces the mesh where its node puts it.
    for v in (0..fbx.meshes[0].data.vertices.len()).step_by(7) {
        let p = Vec3::from(fbx.meshes[0].data.vertices[v].position);
        let rest = world(&fbx, mesh_node, None).transform_point3(p);
        assert!(close(skinned(&fbx, mesh_node, 0, v, None), rest, 1e-4));
    }

    // Blend shapes: names, sizes and default weights like the glTF.
    let names: Vec<&str> = fbx.meshes[0]
        .blend_shapes
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["Bulge", "Pinch"]);
    for (sf, sg) in fbx.meshes[0]
        .blend_shapes
        .iter()
        .zip(&glb.meshes[0].blend_shapes)
    {
        assert!(
            (sf.max_offset() - sg.max_offset()).abs() < 1e-4,
            "{}",
            sf.name
        );
    }
    assert_eq!(fbx.meshes[0].default_weights, [0.0, 0.0]);

    // The Bend take: bones sway and the bulge pulses, like the glTF clip.
    assert_eq!(fbx.animations.len(), 1);
    let (af, ag) = (&fbx.animations[0], &glb.animations[0]);
    assert_eq!(af.name, "Bend");
    assert!((af.clip.length - 2.0).abs() < 1e-3, "{}", af.clip.length);
    assert_eq!(
        fbx.takes,
        [Take {
            name: "Bend".into(),
            length: af.clip.length
        }]
    );
    for t in [0.25, 0.5, 1.3] {
        let tip_f = world(&fbx, node(&fbx, "Bone3"), Some((&af.clip, t)))
            .transform_point3(Vec3::new(0.0, 0.0, 0.0));
        let tip_g =
            world(&glb, node(&glb, "Bone3"), Some((&ag.clip, t))).transform_point3(Vec3::ZERO);
        assert!(close(tip_f, tip_g, 2e-3), "t={t}: {tip_f:?} vs {tip_g:?}");
        // The animated skin follows the bones the same way.
        let v = 300;
        let (sf, sg) = (
            skinned(&fbx, mesh_node, 0, v, Some((&af.clip, t))),
            skinned(&glb, node(&glb, "TentacleMesh"), 0, v, Some((&ag.clip, t))),
        );
        assert!(close(sf, sg, 3e-3), "t={t}: {sf:?} vs {sg:?}");
    }
    let bulge = af
        .clip
        .curve("TentacleMesh", Property::BlendShape(0))
        .expect("bulge curve");
    let w = crate::skinning::sample::bulge_weight(1.0) * 100.0;
    assert!(
        (bulge.evaluate(1.0) - w).abs() < 0.5,
        "{}",
        bulge.evaluate(1.0)
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn convert_units_axis_conversion_and_scale_factor() {
    let dir = temp_dir("units");
    let path = write(&dir, "Tentacle.fbx", &tentacle_fbx());
    let import =
        |s: &ModelImporter| process::apply(load_with(&path, s).unwrap(), s, "Tentacle.fbx");
    let height = |m: &Model| {
        let (lo, hi) = world_bounds(m);
        hi - lo
    };

    let auto = import(&ModelImporter::default());
    assert!((height(&auto).y - 2.04).abs() < 1e-3, "{:?}", height(&auto));

    // Convert Units off: one file unit (a centimetre) is one unit.
    let cm = import(&ModelImporter {
        convert_units: false,
        ..Default::default()
    });
    assert!((height(&cm).y - 204.0).abs() < 0.1, "{:?}", height(&cm));

    // Y Up forced on a Z-up file: read as is, the tentacle lies along Z.
    let y_up = import(&ModelImporter {
        axis_conversion: AxisConversion::YUp,
        ..Default::default()
    });
    assert!((height(&y_up).z - 2.04).abs() < 1e-3, "{:?}", height(&y_up));
    // Z Up forced: the root nodes turn -90° about X; the bones end where Automatic puts them.
    let z_up = import(&ModelImporter {
        axis_conversion: AxisConversion::ZUp,
        ..Default::default()
    });
    let tip = |m: &Model, t: f32| {
        world(m, node(m, "Bone3"), Some((&m.animations[0].clip, t))).transform_point3(Vec3::ZERO)
    };
    for t in [0.0, 0.6] {
        assert!(close(tip(&z_up, t), tip(&auto, t), 2e-3), "t={t}");
    }

    // Scale Factor: everything twice as big, the skin still fits the bones.
    let big = import(&ModelImporter {
        scale_factor: 2.0,
        ..Default::default()
    });
    assert!((height(&big).y - 4.08).abs() < 2e-3);
    assert!(close(tip(&big, 0.6), tip(&auto, 0.6) * 2.0, 4e-3));
    let n = node(&big, "TentacleMesh");
    for v in [0, 100, 300] {
        let p = Vec3::from(big.meshes[0].data.vertices[v].position);
        let rest = world(&big, n, None).transform_point3(p);
        assert!(close(skinned(&big, n, 0, v, None), rest, 1e-3));
    }
    assert!(
        (big.meshes[0].blend_shapes[0].max_offset()
            - 2.0 * auto.meshes[0].blend_shapes[0].max_offset())
        .abs()
            < 1e-4
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn normals_calculated_with_the_smoothing_angle() {
    let dir = temp_dir("normals");
    std::fs::write(dir.join("Wood.png"), b"not an image").unwrap();
    let path = write(&dir, "Crate.fbx", &crate_fbx());
    let import = |angle: f32, normals| {
        let s = ModelImporter {
            normals,
            smoothing_angle: angle,
            generate_lightmap_uvs: false,
            ..Default::default()
        };
        process::apply(load_with(&path, &s).unwrap(), &s, "Crate.fbx")
    };
    // Sharp: every corner keeps its face's normal.
    let sharp = import(30.0, VectorSource::Calculate);
    for m in &sharp.meshes {
        assert_outward(&m.data);
        for v in &m.data.vertices {
            let n = Vec3::from(v.normal);
            assert!((n.abs().max_element() - 1.0).abs() < 1e-4, "{n:?}");
        }
    }
    // Fully smooth: corners average the faces of the mesh meeting there, weighted by their
    // angle at the corner (the four wood sides: horizontal diagonals; the metal top and
    // bottom meet nothing).
    let smooth = import(180.0, VectorSource::Calculate);
    let diag = 1.0 / 2f32.sqrt();
    for v in &smooth.meshes[0].data.vertices {
        let n = Vec3::from(v.normal).abs();
        assert!(close(n, Vec3::new(diag, 0.0, diag), 1e-3), "{n:?}");
    }
    for v in &smooth.meshes[1].data.vertices {
        let n = Vec3::from(v.normal).abs();
        assert!(close(n, Vec3::Y, 1e-3), "{n:?}");
    }
    assert!(smooth.meshes.iter().all(|m| !m.file_normals));
    // None: up everywhere; tangents None: +X.
    let none = process::apply(
        load_with(&path, &ModelImporter::default()).unwrap(),
        &ModelImporter {
            normals: VectorSource::None,
            tangents: VectorSource::None,
            ..Default::default()
        },
        "Crate.fbx",
    );
    assert!(
        none.meshes[0]
            .data
            .vertices
            .iter()
            .all(|v| v.normal == [0.0, 1.0, 0.0] && v.tangent == [1.0, 0.0, 0.0, 1.0])
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn clips_split_from_a_take() {
    let dir = temp_dir("clips");
    let path = write(&dir, "Tentacle.fbx", &tentacle_fbx());
    let s = ModelImporter {
        clips: vec![
            ClipSettings {
                name: "SwayOut".into(),
                take: "Bend".into(),
                first_frame: 0.0,
                last_frame: 30.0,
                looping: true,
            },
            ClipSettings {
                name: "SwayBack".into(),
                take: String::new(),
                first_frame: 30.0,
                last_frame: 60.0,
                looping: false,
            },
        ],
        ..Default::default()
    };
    let full = load(&path).unwrap();
    let model = process::apply(load_with(&path, &s).unwrap(), &s, "Tentacle.fbx");
    let names: Vec<&str> = model.animations.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["SwayOut", "SwayBack"]);
    assert_eq!(
        model.takes.len(),
        1,
        "the take list stays for the Inspector"
    );
    let back = &model.animations[1].clip;
    assert!((back.length - 1.0).abs() < 1e-4);
    assert!(!back.looping && model.animations[0].clip.looping);
    let path = "Bone0/Bone1";
    let original = full.animations[0]
        .clip
        .curve(path, Property::Rotation(2))
        .unwrap();
    let split = back.curve(path, Property::Rotation(2)).unwrap();
    for t in [0.0, 0.3, 1.0] {
        assert!(
            (split.evaluate(t) - original.evaluate(1.0 + t)).abs() < 1e-3,
            "t={t}"
        );
    }
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn import_settings_round_trip_and_old_meta_files_stay_unchanged() {
    // Defaults write only what existed before the new settings.
    let json = serde_json::to_value(ModelImporter::default()).unwrap();
    assert_eq!(json, serde_json::json!({ "generate_lightmap_uvs": true }));
    let old: ModelImporter = serde_json::from_str(r#"{"generate_lightmap_uvs":false}"#).unwrap();
    assert_eq!(
        old,
        ModelImporter {
            generate_lightmap_uvs: false,
            ..Default::default()
        }
    );
    let s = ModelImporter {
        scale_factor: 0.01,
        convert_units: false,
        axis_conversion: AxisConversion::ZUp,
        normals: VectorSource::Calculate,
        smoothing_angle: 45.0,
        tangents: VectorSource::Import,
        material_folder: "Assets/Materials".into(),
        clips: vec![ClipSettings::default()],
        read_write: false,
        generate_lightmap_uvs: false,
        virtual_geometry: true,
        generate_lods: true,
        lods: vec![
            LodSetting {
                quality: 1.0,
                screen_height: 0.4,
            },
            LodSetting {
                quality: 0.3,
                screen_height: 0.05,
            },
        ],
    };
    let dir = temp_dir("roundtrip");
    let model = dir.join("Thing.fbx");
    std::fs::write(&model, b"").unwrap();
    s.save(&model).unwrap();
    assert_eq!(ModelImporter::load(&model), s);
    // Out-of-range values come back usable.
    let mut bad = s.clone();
    bad.scale_factor = -3.0;
    bad.smoothing_angle = 500.0;
    bad.lods[1].quality = 7.0;
    bad.sanitize();
    assert_eq!(bad.scale_factor, 1.0);
    assert_eq!(bad.smoothing_angle, 180.0);
    assert_eq!(bad.lods[1].quality, 1.0);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn generate_lods_simplifies_static_meshes() {
    let data = crate::vgeo::rock::rock(4, 7);
    let tris = data.indices.len() / 3;
    let mut model = Model::default();
    model.meshes.push(ModelMesh::new("Rock".into(), data, None));
    model.nodes.push(ModelNode {
        name: "Rock".into(),
        parent: None,
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
        meshes: vec![0],
        skin: None,
        lods: Vec::new(),
    });
    let s = ModelImporter {
        generate_lods: true,
        generate_lightmap_uvs: false,
        ..Default::default()
    };
    let model = process::apply(model, &s, "Rock.fbx");
    assert_eq!(model.lod_heights, [0.5, 0.2, 0.03]);
    let lods = &model.nodes[0].lods;
    assert_eq!(lods.len(), 2);
    let count = |i: usize| model.meshes[i].data.indices.len() / 3;
    let (l1, l2) = (count(lods[0][0]), count(lods[1][0]));
    assert!(l1 < tris && l2 < l1, "{tris} → {l1} → {l2}");
    assert!((l1 as f32) < tris as f32 * 0.6, "{tris} → {l1}");
    assert!((l2 as f32) < tris as f32 * 0.3, "{tris} → {l2}");
    assert_eq!(model.meshes[lods[0][0]].name, "Rock LOD1");
    // The simplified surface stays close to the original.
    let (a, b) = (model.meshes[0].bounds, model.meshes[lods[1][0]].bounds);
    assert!(
        close(a.0, b.0, 0.1) && close(a.1, b.1, 0.1),
        "{a:?} vs {b:?}"
    );
}

/// The committed sample files are what the generator makes, and they import.
#[test]
fn sample_fbx_assets_are_up_to_date() {
    let models =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject/Assets/Models");
    for (name, bytes) in [("Tentacle.fbx", tentacle_fbx()), ("Crate.fbx", crate_fbx())] {
        let path = models.join(name);
        let on_disk = std::fs::read(&path).unwrap_or_default();
        assert!(
            on_disk == bytes,
            "{name} is stale: run the ignored regenerate_sample_fbx test"
        );
        assert!(!load(&path).unwrap().meshes.is_empty());
    }
}

/// Writes the sample FBX files: `cargo test -p misty_engine --release regenerate_sample_fbx
/// -- --ignored`.
#[test]
#[ignore]
fn regenerate_sample_fbx() {
    let models =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject/Assets/Models");
    std::fs::write(models.join("Tentacle.fbx"), tentacle_fbx()).unwrap();
    std::fs::write(models.join("Crate.fbx"), crate_fbx()).unwrap();
    let wood = image::RgbaImage::from_fn(64, 64, |x, y| {
        let grain = ((x as f32 * 0.35 + (y as f32 * 0.08).sin() * 3.0).sin() * 18.0) as i32;
        let c = |b: i32| (b + grain).clamp(0, 255) as u8;
        image::Rgba([c(170), c(120), c(70), 255])
    });
    wood.save(models.join("Wood.png")).unwrap();
}
