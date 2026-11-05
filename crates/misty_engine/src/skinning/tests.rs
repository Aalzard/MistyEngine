use std::sync::OnceLock;

use glam::{Mat4, Quat, Vec3};
use hecs::Entity;

use super::*;
use crate::animation::{Property, binding};
use crate::render::MaterialParams;
use crate::scene::{Animation, Scene, Transform};

struct NoMaterials;

impl MaterialSource for NoMaterials {
    fn params(&self, _: Option<&str>) -> MaterialParams {
        MaterialParams::default()
    }
}

/// The sample tentacle written once to a temp folder; its absolute path.
fn tentacle() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("misty_skinning_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Tentacle.glb");
        std::fs::write(&file, sample::tentacle_glb()).unwrap();
        file.to_string_lossy().replace('\\', "/")
    })
}

/// Build the model's objects the way the editor's import does: a root, one object per node,
/// Skinned Mesh Renderers on skinned nodes and an Animation with the file's clip (inserted
/// under `clip_path`). Returns (root, objects by node).
fn spawn(scene: &mut Scene, clip_path: Option<&str>) -> (Entity, Vec<Entity>) {
    let rel = tentacle();
    let model = models::model(rel).unwrap();
    let root = scene.spawn("Tentacle", None);
    let mut objects = Vec::new();
    for n in &model.nodes {
        let p = n.parent.map_or(root, |i| objects[i]);
        let e = scene.spawn(&n.name, Some(p));
        let mut t = Transform::from_position(n.position);
        t.set_rotation(n.rotation);
        t.scale = n.scale;
        scene.set_transform(e, t);
        objects.push(e);
    }
    for (i, n) in model.nodes.iter().enumerate() {
        if let Some(&m) = n.meshes.first()
            && is_skinned_node(&model, i)
        {
            let r = renderer_for(&model, rel, i, m, &objects, None);
            scene.add(objects[i], r);
        }
    }
    if let Some(path) = clip_path {
        scene.add(
            root,
            Animation {
                clip: Some(path.into()),
                clips: vec![path.into()],
                ..Default::default()
            },
        );
    }
    (root, objects)
}

fn mesh_node(scene: &Scene) -> Entity {
    scene.find_by_name("TentacleMesh").unwrap()
}

fn pose_of(scene: &Scene, e: Entity) -> SkinPose {
    let r = scene.world.get::<&SkinnedMeshRenderer>(e).unwrap().clone();
    compute_pose(scene, e, &r, &|x| scene.world_matrix(x)).unwrap()
}

#[test]
fn import_reads_bones_normalised_weights_shapes_and_the_clip() {
    let model = models::model(tentacle()).unwrap();
    assert_eq!(model.skins.len(), 1);
    let skin = &model.skins[0];
    assert_eq!(skin.joints.len(), 4, "four bones");
    let names: Vec<&str> = skin
        .joints
        .iter()
        .map(|j| model.nodes[*j].name.as_str())
        .collect();
    assert_eq!(names, ["Bone0", "Bone1", "Bone2", "Bone3"]);
    assert_eq!(
        skin.root.map(|r| model.nodes[r].name.as_str()),
        Some("Bone0")
    );
    assert_eq!(model.node_path(skin.joints[2]), "Bone0/Bone1/Bone2");

    let mesh = &model.meshes[0];
    let s = mesh.skin.as_ref().expect("skinned mesh");
    assert_eq!(s.joints.len(), mesh.data.vertices.len());
    for w in &s.weights {
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-5, "{w:?}");
    }
    assert_eq!(s.joint_count(), 4);
    assert_eq!(s.bind_poses.len(), 4);
    // Bone2 sits at y = 1: its inverse bind matrix moves y = 1 to the origin.
    let p = s.bind_poses[2].transform_point3(Vec3::new(0.0, 1.0, 0.0));
    assert!(p.length() < 1e-5, "{p}");

    let shapes: Vec<&str> = mesh.blend_shapes.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(shapes, sample::SHAPES);
    assert_eq!(mesh.default_weights, [0.0, 0.0]);

    assert_eq!(model.animations.len(), 1);
    let clip = &model.animations[0].clip;
    assert_eq!(model.animations[0].name, "Bend");
    assert!((clip.length - sample::BEND_SECONDS).abs() < 1e-4);
    assert!(clip.looping);
    assert!(clip.curve("Bone0/Bone1", Property::Rotation(2)).is_some());
    assert!(
        clip.curve("TentacleMesh", Property::BlendShape(0))
            .is_some()
    );
}

#[test]
fn bind_pose_deforms_to_the_mesh_and_a_bent_bone_moves_only_its_part() {
    let mut scene = Scene::default();
    let (_, objects) = spawn(&mut scene, None);
    let e = mesh_node(&scene);
    let model = models::model(tentacle()).unwrap();
    let m = &model.meshes[0];

    let pose = pose_of(&scene, e);
    let bind = deform(
        &m.data,
        m.skin.as_ref(),
        &m.blend_shapes,
        &pose.matrices,
        &pose.weights,
    );
    for (a, b) in bind.iter().zip(&m.data.vertices) {
        assert!((Vec3::from(a.position) - Vec3::from(b.position)).length() < 1e-4);
    }

    // Turn Bone3 (top joint, y = 1.5) by 90°: the tip swings, the base stays.
    let bone3 = objects[model.skins[0].joints[3]];
    let mut t = scene.transform(bone3);
    t.set_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
    scene.set_transform(bone3, t);
    let pose = pose_of(&scene, e);
    let bent = deform(
        &m.data,
        m.skin.as_ref(),
        &m.blend_shapes,
        &pose.matrices,
        &pose.weights,
    );
    let (base, tip) = (0, m.data.vertices.len() - 1);
    assert!(
        (Vec3::from(bent[base].position) - Vec3::from(m.data.vertices[base].position)).length()
            < 1e-4
    );
    let moved = Vec3::from(bent[tip].position) - Vec3::from(m.data.vertices[tip].position);
    assert!(moved.length() > 0.3, "tip moved {moved}");
    // Posed bounds hold every deformed vertex.
    let (lo, hi) = posed_bounds(m.skin.as_ref().unwrap(), &pose.matrices).unwrap();
    for v in &bent {
        let p = Vec3::from(v.position);
        assert!(
            p.cmpge(lo - 1e-4).all() && p.cmple(hi + 1e-4).all(),
            "{p} outside"
        );
    }
}

#[test]
fn blend_shape_weight_moves_vertices() {
    let mut scene = Scene::default();
    spawn(&mut scene, None);
    let e = mesh_node(&scene);
    let model = models::model(tentacle()).unwrap();
    let m = &model.meshes[0];
    // A vertex on the ring at y = 1 (the bulge's centre).
    let ring = 12 * 17;
    let before = Vec3::from(m.data.vertices[ring].position);
    assert!((before.y - 1.0).abs() < 1e-5);

    scene
        .world
        .get::<&mut SkinnedMeshRenderer>(e)
        .unwrap()
        .set_blend_shape_weight(0, 100.0);
    let pose = pose_of(&scene, e);
    assert_eq!(pose.weights, [1.0, 0.0]);
    let out = deform(
        &m.data,
        m.skin.as_ref(),
        &m.blend_shapes,
        &pose.matrices,
        &pose.weights,
    );
    let after = Vec3::from(out[ring].position);
    let grow =
        Vec3::new(after.x, 0.0, after.z).length() - Vec3::new(before.x, 0.0, before.z).length();
    assert!((grow - 0.16).abs() < 0.01, "bulged by {grow}");
    // Half weight, half the offset.
    scene
        .world
        .get::<&mut SkinnedMeshRenderer>(e)
        .unwrap()
        .set_blend_shape_weight(0, 50.0);
    let pose = pose_of(&scene, e);
    let half = deform(
        &m.data,
        m.skin.as_ref(),
        &m.blend_shapes,
        &pose.matrices,
        &pose.weights,
    );
    let h = Vec3::new(half[ring].position[0], 0.0, half[ring].position[2]).length()
        - Vec3::new(before.x, 0.0, before.z).length();
    assert!((h - 0.08).abs() < 0.01, "{h}");
}

#[test]
fn the_imported_clip_drives_bones_and_blend_shapes() {
    let mut scene = Scene::default();
    let (root, _) = spawn(&mut scene, None);
    let model = models::model(tentacle()).unwrap();
    let clip = &model.animations[0].clip;
    let t = 0.5;
    binding::sample(&mut scene, root, clip, t);
    let bone1 = scene.find_by_name("Bone1").unwrap();
    let got = scene.transform(bone1).rotation;
    // The file turns Bone1 about Z; mirroring X turns it the other way in engine space.
    let want = Quat::from_rotation_z(-sample::bend_angle(1, t));
    assert!(got.dot(want).abs() > 0.9999, "{got:?} vs {want:?}");
    let e = mesh_node(&scene);
    let w = scene
        .world
        .get::<&SkinnedMeshRenderer>(e)
        .unwrap()
        .blend_shape_weight(0);
    assert!((w - sample::bulge_weight(t) * 100.0).abs() < 0.5, "{w}");
    // The pose follows: Bone1's skin matrix is no longer the identity.
    let pose = pose_of(&scene, e);
    assert!(!pose.matrices[1].abs_diff_eq(Mat4::IDENTITY, 1e-3));
}

#[test]
fn animation_component_plays_the_clip_in_play_mode() {
    let dir = std::env::temp_dir().join(format!("misty_skin_play_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let model = models::model(tentacle()).unwrap();
    model.animations[0]
        .clip
        .save(&dir.join("Bend.anim"))
        .unwrap();
    let mut scene = Scene::default();
    spawn(&mut scene, Some("Bend.anim"));
    let mut player = crate::animation::AnimationPlayer::new(dir.clone());
    player.play_automatically(&scene, None);
    let bone2 = scene.find_by_name("Bone2").unwrap();
    let start = scene.transform(bone2).rotation;
    for _ in 0..20 {
        player.update(&mut scene, 1.0 / 60.0);
    }
    assert!(player.take_logs().is_empty());
    assert!(
        scene.transform(bone2).rotation.dot(start).abs() < 0.9999,
        "bone moved"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn skinned_renderer_round_trips_through_a_scene_file() {
    let mut scene = Scene::default();
    spawn(&mut scene, None);
    let e = mesh_node(&scene);
    scene
        .world
        .get::<&mut SkinnedMeshRenderer>(e)
        .unwrap()
        .set_blend_shape_weight(1, 42.0);
    let text = scene.to_json();
    let (back, report) = Scene::from_json("Test", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let e2 = back.find_by_name("TentacleMesh").unwrap();
    let r = back.world.get::<&SkinnedMeshRenderer>(e2).unwrap().clone();
    let names: Vec<String> = r.bones.iter().map(|b| back.name(b.unwrap())).collect();
    assert_eq!(names, ["Bone0", "Bone1", "Bone2", "Bone3"]);
    assert_eq!(r.root_bone.map(|b| back.name(b)), Some("Bone0".into()));
    assert_eq!(r.blend_shape_weight(1), 42.0);
    assert_eq!(r.mesh, Some(MeshRef::asset(tentacle(), 0)));
    // Copies point at their own bones.
    let mut copy = back.clone();
    let root = copy.find_by_name("Tentacle").unwrap();
    let dup = copy.copy_subtree(root, None);
    let mesh = copy
        .walk()
        .into_iter()
        .map(|(x, _)| x)
        .find(|x| copy.is_ancestor(dup, *x) && copy.name(*x) == "TentacleMesh")
        .unwrap();
    let r = copy
        .world
        .get::<&SkinnedMeshRenderer>(mesh)
        .unwrap()
        .clone();
    assert!(r.bones.iter().all(|b| copy.is_ancestor(dup, b.unwrap())));
}

#[test]
fn draws_publish_poses_and_unchanged_poses_keep_their_version() {
    let mut scene = Scene::default();
    spawn(&mut scene, None);
    let mut cache = crate::draw_cache::DrawCache::new();
    let draws = cache.update(&scene, &NoMaterials, u32::MAX, |_| false);
    let d = draws
        .iter()
        .find(|d| matches!(&d.mesh, MeshRef::Asset(k) if is_pose_key(k)))
        .expect("a skinned draw");
    let MeshRef::Asset(key) = &d.mesh else {
        unreachable!()
    };
    let v0 = pose(key).unwrap().version;
    assert!(models::bounds(&d.mesh).is_some());
    assert!(models::mesh(&d.mesh).is_some_and(|m| !m.indices.is_empty()));
    cache.update(&scene, &NoMaterials, u32::MAX, |_| false);
    assert_eq!(pose(key).unwrap().version, v0, "static pose");
    let bone = scene.find_by_name("Bone2").unwrap();
    let mut t = scene.transform(bone);
    t.set_euler(Vec3::new(0.0, 0.0, 30.0));
    scene.set_transform(bone, t);
    cache.update(&scene, &NoMaterials, u32::MAX, |_| false);
    assert_eq!(pose(key).unwrap().version, v0 + 1);
}

#[test]
fn lua_sets_and_reads_blend_shape_weights() {
    const SCRIPT: &str = r#"
local T = {}
function T:Start()
    local r = self.gameObject:GetComponent("SkinnedMeshRenderer")
    r:SetBlendShapeWeight(1, 75)
    Debug.Log(r:GetBlendShapeWeight(1) .. " " .. r.blendShapeCount .. " "
        .. r:GetBlendShapeIndex("Pinch") .. " " .. r:GetBlendShapeName(0))
    local ok = pcall(function() r:SetBlendShapeWeight(5, 1) end)
    Debug.Log(tostring(ok))
end
return T
"#;
    let root = std::env::temp_dir().join(format!("misty_skin_lua_{}", std::process::id()));
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    std::fs::write(root.join("Assets/Shape.lua"), SCRIPT).unwrap();
    let mut scene = Scene::default();
    spawn(&mut scene, None);
    let e = mesh_node(&scene);
    scene.add(
        e,
        crate::scene::Scripts {
            list: vec![crate::scene::ScriptRef {
                path: "Assets/Shape.lua".into(),
                enabled: true,
                fields: Default::default(),
            }],
        },
    );
    let physics = crate::physics::Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = crate::scripting::ScriptHost::start(&mut scene, physics, root.clone());
    let mut time = crate::runtime::Time::default();
    time.advance(1.0 / 60.0);
    host.update(&mut scene, &time, Default::default());
    let logs: Vec<String> = host.take_logs().into_iter().map(|l| l.1).collect();
    assert_eq!(logs, ["75.0 2 1 Bulge", "false"]);
    let w = scene
        .world
        .get::<&SkinnedMeshRenderer>(e)
        .unwrap()
        .blend_shape_weight(1);
    assert_eq!(w, 75.0);
    std::fs::remove_dir_all(root).ok();
}
