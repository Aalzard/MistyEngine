//! The SkinningDemo sample in `SampleProject`: three copies of a rigged, morphing tentacle made
//! in code (`Assets/Models/Tentacle.glb`): one playing the imported "Bend" clip (bones sway, the
//! Bulge blend shape pulses), one whose blend shapes a Lua script drives with
//! `SetBlendShapeWeight`, and one posed by hand with fixed weights. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_skinning_sample`; the other test checks the
//! committed files still match and that the scene plays.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::animation::AnimationClip;
use misty_engine::assets::Material;
use misty_engine::models;
use misty_engine::scene::{
    MeshFilter, MeshRenderer, PrimitiveMesh, Scene, ScriptRef, Scripts, SkinnedMeshRenderer,
    Transform,
};
use misty_engine::skinning;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

const MODEL: &str = "Assets/Models/Tentacle.glb";
const MATERIAL: &str = "Assets/Models/Tentacle_TentacleSkin.mat";
const CLIP: &str = "Assets/Models/Tentacle_Bend.anim";
const FLOOR: &str = "Assets/Materials/SkinningFloor.mat";
const SCRIPT: &str = "Assets/Scripts/TentacleShapes.lua";
const SCENE: &str = "Assets/Scenes/SkinningDemo.scene";

const LUA: &str = r#"-- TentacleShapes: drives the Skinned Mesh Renderer's blend shapes from code.
-- Bulge and Pinch pulse out of step (weights are 0-100, like the Inspector sliders).
local TentacleShapes = {
    speed = 1.6,
}

function TentacleShapes:Start()
    self.renderer = self.gameObject:GetComponent("SkinnedMeshRenderer")
    self.bulge = self.renderer:GetBlendShapeIndex("Bulge")
    self.pinch = self.renderer:GetBlendShapeIndex("Pinch")
end

function TentacleShapes:Update()
    local t = Time.time * self.speed
    self.renderer:SetBlendShapeWeight(self.bulge, 50 + 50 * math.sin(t))
    self.renderer:SetBlendShapeWeight(self.pinch, 50 + 50 * math.sin(t * 0.7 + 2.0))
end

return TentacleShapes
"#;

/// The model as the importer reads it (from bytes written to a temp file).
fn model() -> std::sync::Arc<models::Model> {
    let dir = std::env::temp_dir().join(format!("misty_skin_sample_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Tentacle.glb");
    std::fs::write(&file, skinning::sample::tentacle_glb()).unwrap();
    models::model(&file.to_string_lossy().replace('\\', "/")).unwrap()
}

/// The material the importer extracts for the tentacle.
fn skin_material(model: &models::Model) -> Material {
    model.materials[0].material.clone()
}

fn floor_material() -> Material {
    Material {
        color: [0.55, 0.58, 0.62, 1.0],
        smoothness: 0.25,
        ..Default::default()
    }
}

fn clip(model: &models::Model) -> AnimationClip {
    model.animations[0].clip.clone()
}

fn demo_scene(model: &models::Model) -> Scene {
    let mut s = Scene::default_3d("SkinningDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 1.9, -5.2))
                .with_euler(Vec3::new(10.0, 0.0, 0.0)),
        );
    }
    let floor = s.spawn("Floor", None);
    s.set_transform(floor, Transform::from_position(Vec3::ZERO));
    s.add(floor, MeshFilter::primitive(PrimitiveMesh::Plane));
    s.add(
        floor,
        MeshRenderer {
            material: Some(FLOOR.into()),
            ..Default::default()
        },
    );
    let materials = [Some(MATERIAL.to_owned())];

    // 1. Plays the imported clip.
    let animated = crate::model_import::spawn_with_clips(
        &mut s,
        MODEL,
        model,
        &materials,
        &[CLIP.to_owned()],
        None,
    );
    s.game_object_mut(animated).name = "Tentacle (Animated)".into();
    s.set_transform(
        animated,
        Transform::from_position(Vec3::new(-1.6, 0.0, 0.0)),
    );

    // 2. Blend shapes driven by Lua.
    let scripted = crate::model_import::spawn(&mut s, MODEL, model, &materials, None);
    s.game_object_mut(scripted).name = "Tentacle (Lua)".into();
    s.set_transform(scripted, Transform::from_position(Vec3::ZERO));
    let mesh = child(&s, scripted, "TentacleMesh");
    s.add(
        mesh,
        Scripts {
            list: vec![ScriptRef {
                path: SCRIPT.into(),
                enabled: true,
                fields: Default::default(),
            }],
        },
    );

    // 3. Posed by hand: bones turned in the Scene, weights set in the Inspector.
    let posed = crate::model_import::spawn(&mut s, MODEL, model, &materials, None);
    s.game_object_mut(posed).name = "Tentacle (Posed)".into();
    s.set_transform(posed, Transform::from_position(Vec3::new(1.6, 0.0, 0.0)));
    for (bone, z) in [("Bone1", -25.0), ("Bone2", -30.0), ("Bone3", -35.0)] {
        let b = child(&s, posed, bone);
        let mut t = s.transform(b);
        t.set_euler(Vec3::new(0.0, 0.0, z));
        s.set_transform(b, t);
    }
    let mesh = child(&s, posed, "TentacleMesh");
    if let Ok(mut r) = s.world.get::<&mut SkinnedMeshRenderer>(mesh) {
        r.blend_shape_weights = vec![70.0, 35.0];
    }
    s
}

/// The object named `name` below `root`.
fn child(s: &Scene, root: misty_engine::hecs::Entity, name: &str) -> misty_engine::hecs::Entity {
    s.walk()
        .into_iter()
        .map(|(e, _)| e)
        .find(|e| s.is_ancestor(root, *e) && s.name(*e) == name)
        .unwrap_or_else(|| panic!("{name} below the tentacle"))
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_skinning_sample() {
    let root = project();
    let model = model();
    std::fs::create_dir_all(root.join("Assets/Models")).unwrap();
    std::fs::write(root.join(MODEL), skinning::sample::tentacle_glb()).unwrap();
    skin_material(&model).save(&root.join(MATERIAL)).unwrap();
    floor_material().save(&root.join(FLOOR)).unwrap();
    clip(&model).save(&root.join(CLIP)).unwrap();
    std::fs::write(root.join(SCRIPT), LUA).unwrap();
    demo_scene(&model).save(&root.join(SCENE)).unwrap();
}

#[test]
fn skinning_sample_matches_its_generator_and_plays() {
    let root = project();
    let model = model();
    crate::test_util::assert_glb_close(
        &std::fs::read(root.join(MODEL)).unwrap(),
        &skinning::sample::tentacle_glb(),
        MODEL,
    );
    crate::test_util::assert_close(
        &Material::load(&root.join(MATERIAL)).unwrap(),
        &skin_material(&model),
        MATERIAL,
    );
    crate::test_util::assert_close(
        &Material::load(&root.join(FLOOR)).unwrap(),
        &floor_material(),
        FLOOR,
    );
    crate::test_util::assert_close(
        &AnimationClip::load(&root.join(CLIP)).unwrap(),
        &clip(&model),
        CLIP,
    );
    assert_eq!(
        std::fs::read_to_string(root.join(SCRIPT))
            .unwrap()
            .replace("\r\n", "\n"),
        LUA
    );
    let (mut scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let expected = demo_scene(&model);
    crate::test_util::assert_json_close(
        &scene.to_json(),
        &expected.to_json(),
        "scene matches its generator",
    );

    // Play: the animated tentacle's bones move and its Bulge pulses; Lua drives the other.
    let mut rt = misty_engine::runtime::Runtime::start_with_audio(
        &mut scene,
        root.clone(),
        &Default::default(),
        misty_engine::audio::Audio::null(root.clone()).0,
    );
    let animated = scene.find_by_name("Tentacle (Animated)").unwrap();
    let lua = scene.find_by_name("Tentacle (Lua)").unwrap();
    let bone2 = child(&scene, animated, "Bone2");
    let start = scene.transform(bone2).rotation;
    let mut time = misty_engine::runtime::Time::default();
    for _ in 0..30 {
        time.advance(1.0 / 60.0);
        rt.update(&mut scene, &time, Default::default());
    }
    let logs = rt.take_logs();
    assert!(logs.is_empty(), "{logs:?}");
    assert!(
        scene.transform(bone2).rotation.dot(start).abs() < 0.9999,
        "bone moved"
    );
    let weight = |root, i| {
        let m = child(&scene, root, "TentacleMesh");
        scene
            .world
            .get::<&SkinnedMeshRenderer>(m)
            .unwrap()
            .blend_shape_weight(i)
    };
    let bulge = weight(animated, 0);
    let want = skinning::sample::bulge_weight(0.5) * 100.0;
    assert!(
        (bulge - want).abs() < 8.0,
        "clip bulge {bulge}, want ~{want}"
    );
    assert!(
        weight(lua, 0) > 1.0 && weight(lua, 1) > 1.0,
        "Lua drove the shapes"
    );
}
