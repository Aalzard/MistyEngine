//! Lua `MeshRenderer` / `Material` tests: instance copies, shared edits, property names.

use std::path::{Path, PathBuf};

use glam::Vec3;

use super::*;
use crate::assets::{AssetCache, Material};
use crate::physics::Physics;
use crate::render::MaterialParams;
use crate::render_scene;
use crate::scene::{MeshFilter, MeshRenderer, PrimitiveMesh, ScriptRef};

const MAT: &str = "Assets/Materials/Paint.mat";

/// Temp project with one material and the given scripts.
fn project(scripts: &[(&str, &str)]) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_material_api_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::create_dir_all(root.join("Assets/Materials")).unwrap();
    let paint = Material {
        color: [0.5, 0.5, 0.5, 1.0],
        ..Default::default()
    };
    paint.save(&root.join(MAT)).unwrap();
    for (name, src) in scripts {
        std::fs::write(root.join("Assets/Scripts").join(name), src).unwrap();
    }
    root
}

/// A sphere with the test material and optionally a script.
fn ball(scene: &mut Scene, name: &str, script: Option<&str>) -> Entity {
    let e = scene.spawn(name, None);
    scene.add(e, MeshFilter::primitive(PrimitiveMesh::Sphere));
    scene.add(
        e,
        MeshRenderer {
            material: Some(MAT.into()),
            ..Default::default()
        },
    );
    if let Some(file) = script {
        scene.add(
            e,
            Scripts {
                list: vec![ScriptRef {
                    path: format!("Assets/Scripts/{file}"),
                    enabled: true,
                    fields: Default::default(),
                }],
            },
        );
    }
    e
}

fn run(scene: &mut Scene, root: &Path, frames: u32) -> Vec<(LogKind, String)> {
    let physics = Physics::build(scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(scene, physics, root.to_path_buf());
    let mut time = Time::default();
    for _ in 0..frames {
        time.advance(1.0 / 60.0);
        host.update(scene, &time, Input::default());
    }
    host.take_logs()
}

/// What each object renders with (by Hierarchy order).
fn rendered(scene: &Scene, root: &Path) -> Vec<MaterialParams> {
    let cache = AssetCache::open(root);
    render_scene::mesh_draws(scene, &cache, |_| false)
        .into_iter()
        .map(|d| d.material)
        .collect()
}

const OVERRIDES: &str = r#"
local T = {}
function T:Start()
    local r = self.gameObject:GetComponent("MeshRenderer")
    Debug.Log(tostring(r:GetOverride("tiling")))
    r:SetOverride("tiling", Vector2.new(2, 3))
    r:SetOverride("color", Color.new(1, 0, 0, 1))
    r:SetOverride("smoothness", 0.5)
    local t = r:GetOverride("tiling")
    Debug.Log(t.x .. "," .. t.y .. " " .. r:GetOverride("smoothness"))
    r:ClearOverride("smoothness")
    Debug.Log(tostring(r:GetOverride("smoothness")))
    local ok = pcall(function() r:SetOverride("nope", 1) end)
    Debug.Log(tostring(ok))
end
return T
"#;

#[test]
fn overrides_change_one_object_and_read_back() {
    let root = project(&[("Over.lua", OVERRIDES)]);
    let mut scene = Scene::new("t");
    ball(&mut scene, "A", Some("Over.lua"));
    ball(&mut scene, "B", None);
    let logs: Vec<String> = run(&mut scene, &root, 1).into_iter().map(|l| l.1).collect();
    assert_eq!(logs, ["nil", "2.0,3.0 0.5", "nil", "false"]);
    let r = rendered(&scene, &root);
    assert_eq!(
        (r[0].tiling, r[0].color),
        ([2.0, 3.0], [0.5, 0.0, 0.0, 1.0])
    );
    assert_eq!(
        (r[1].tiling, r[1].color),
        ([1.0, 1.0], [0.5, 0.5, 0.5, 1.0])
    );
}

const INSTANCE: &str = r#"
local Tint = {}
function Tint:Start()
    local r = self.gameObject:GetComponent("MeshRenderer")
    r.material.color = Color.new(0, 1, 0, 1)
    local m = r.material
    m:SetFloat("_Metallic", 0.75)
    m:SetFloat("_Glossiness", 0.9)
    m:SetColor("_EmissionColor", Color.new(2, 1, 0, 1))
    m:EnableKeyword("_EMISSION")
    m.mainTextureOffset = Vector3.new(0.5, 0.25, 0)
    m:SetTextureScale("_MainTex", { x = 2, y = 3 })
    m:SetFloat("_Mode", 2)
    Debug.Log(m:GetFloat("_Metallic"), m:GetColor("_Color").g, r.sharedMaterial.name,
        tostring(r.material == r.sharedMaterial), tostring(m:HasProperty("_Cutoff")),
        tostring(m:HasProperty("_Nope")), tostring(m:IsKeywordEnabled("_ALPHABLEND_ON")),
        m:GetColor("_EmissionColor").r)
end
return Tint
"#;

#[test]
fn renderer_material_is_a_copy_for_this_object_only() {
    let root = project(&[("Tint.lua", INSTANCE)]);
    let mut scene = Scene::new("t");
    let a = ball(&mut scene, "A", Some("Tint.lua"));
    ball(&mut scene, "B", None);
    let logs = run(&mut scene, &root, 2);
    let line = &logs
        .iter()
        .find(|l| l.0 == LogKind::Info)
        .expect("logged")
        .1;
    assert_eq!(
        line, "0.75 1 Paint (Instance) true true false true 2",
        "{logs:?}"
    );

    let draws = rendered(&scene, &root);
    let (tinted, other) = (&draws[0], &draws[1]);
    assert_eq!(tinted.color, [0.0, 1.0, 0.0, 1.0]);
    assert_eq!((tinted.metallic, tinted.smoothness), (0.75, 0.9));
    assert_eq!(tinted.emission, [1.0, 0.5, 0.0]);
    assert_eq!(
        tinted.emission_intensity, 2.0,
        "HDR colour = colour × intensity"
    );
    assert_eq!(tinted.offset, [0.5, 0.25]);
    assert_eq!(tinted.tiling, [2.0, 3.0]);
    assert_eq!(tinted.mode, crate::render::RenderingMode::Fade);
    assert_eq!(other.color, [0.5, 0.5, 0.5, 1.0], "B still uses the asset");
    assert_eq!(
        Material::load(&root.join(MAT)).unwrap().color,
        [0.5, 0.5, 0.5, 1.0],
        "the file is untouched"
    );

    // Instances are Play-mode only: copies and saved scenes don't carry them.
    let saved = scene.to_json();
    assert!(!saved.contains("instances"), "{saved}");
    let (loaded, _) = Scene::from_json("t", &saved).unwrap();
    let r = loaded.world.get::<&MeshRenderer>(a).unwrap();
    assert!(r.instance(0).is_none());
}

const SHARED: &str = r#"
local Shared = {}
function Shared:Start()
    local r = self.gameObject:GetComponent("Renderer")
    r.sharedMaterial.color = Color.new(1, 0, 0, 1)
    r.sharedMaterial:SetFloat("_Cutoff", 0.2)
end
return Shared
"#;

const INSTANCE_ONLY: &str = r#"
local Own = {}
function Own:Start()
    local m = self.gameObject:GetComponent("MeshRenderer").material
    m:SetColor("_Color", Color.new(0, 0, 1, 1))
end
return Own
"#;

#[test]
fn shared_material_changes_every_user_but_not_instances() {
    let root = project(&[("Shared.lua", SHARED), ("Own.lua", INSTANCE_ONLY)]);
    let mut scene = Scene::new("t");
    ball(&mut scene, "Own", Some("Own.lua"));
    ball(&mut scene, "Changer", Some("Shared.lua"));
    ball(&mut scene, "Plain", None);
    run(&mut scene, &root, 2);
    let draws = rendered(&scene, &root);
    let colors: Vec<[f32; 4]> = draws.iter().map(|d| d.color).collect();
    assert_eq!(colors[1], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(colors[2], [1.0, 0.0, 0.0, 1.0], "same asset → same change");
    assert_eq!(draws[2].alpha_cutoff, 0.2);
    // `Own` made its copy before (or after) the shared change; either way it stays blue.
    assert_eq!(colors[0], [0.0, 0.0, 1.0, 1.0]);
    assert!(scene.shared_materials.contains_key(MAT));
}

const LISTS: &str = r#"
local Lists = {}
function Lists:Start()
    local r = self.gameObject:GetComponent("MeshRenderer")
    local mats = r.sharedMaterials
    Debug.Log(#mats, mats[1].name)
    r.sharedMaterials = { mats[1], mats[1] }
    local inst = r.materials
    inst[2].color = Color.new(1, 1, 0, 0.5)
    Debug.Log(#r.materials, r.material.shader)
    r.material.shader = "Unlit/Color"
end
return Lists
"#;

#[test]
fn material_lists_and_shader_switch() {
    let root = project(&[("Lists.lua", LISTS)]);
    let mut scene = Scene::new("t");
    let e = ball(&mut scene, "Ball", Some("Lists.lua"));
    let logs = run(&mut scene, &root, 1);
    let infos: Vec<&str> = logs
        .iter()
        .filter(|l| l.0 == LogKind::Info)
        .map(|l| l.1.as_str())
        .collect();
    assert_eq!(infos, ["1 Paint", "2 Standard"], "{logs:?}");
    assert_eq!(
        scene
            .world
            .get::<&MeshRenderer>(e)
            .unwrap()
            .material_count(),
        2
    );
    let draws = rendered(&scene, &root);
    assert_eq!(draws.len(), 2, "one draw per element");
    assert!(draws[0].unlit);
    assert_eq!(draws[1].color, [1.0, 1.0, 0.0, 0.5]);
}

const FEATURES: &str = r#"
local F = {}
function F:Start()
    local m = self.gameObject:GetComponent("MeshRenderer").material
    m:EnableKeyword("_CLEARCOAT")
    m:SetFloat("_CoatMask", 0.75)
    m:SetFloat("_CoatSmoothness", 0.9)
    m:EnableKeyword("_ANISOTROPY")
    m:SetFloat("_Anisotropy", -0.4)
    m:SetTexture("_ParallaxMap", "Assets/Textures/Height.png")
    Debug.Log(tostring(m:IsKeywordEnabled("_CLEARCOAT")), m:GetFloat("_CoatMask"),
        tostring(m:IsKeywordEnabled("_PARALLAXMAP")), m:GetTexture("_ParallaxMap"),
        tostring(m:HasProperty("_DetailAlbedoMap")))
end
return F
"#;

/// The Standard Lit v2 features through Unity's property and keyword names.
#[test]
fn lua_switches_material_features() {
    let root = project(&[("F.lua", FEATURES)]);
    let mut scene = Scene::new("t");
    ball(&mut scene, "A", Some("F.lua"));
    let logs = run(&mut scene, &root, 1);
    let line = &logs.iter().find(|l| l.0 == LogKind::Info).expect("logged").1;
    // The height map alone doesn't switch parallax on.
    assert_eq!(
        line, "true 0.75 false Assets/Textures/Height.png true",
        "{logs:?}"
    );
    let draws = rendered(&scene, &root);
    use crate::render::material_v2::feature;
    assert_eq!(draws[0].features(), feature::CLEARCOAT | feature::ANISOTROPY);
    let e = draws[0].extras.as_ref().unwrap();
    assert!((e.values.coat[0] - 0.75).abs() < 1e-6);
    assert_eq!(e.values.aniso[0], -0.4);
}
