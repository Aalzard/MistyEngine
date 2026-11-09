use std::cell::RefCell;

use glam::Vec3;

use super::*;
use crate::render::overrides::OverrideValue;
use crate::scene::PrimitiveMesh;

/// Materials whose colour the test can change between frames.
#[derive(Default)]
struct Colors(RefCell<HashMap<String, [f32; 4]>>);

impl MaterialSource for Colors {
    fn params(&self, rel: Option<&str>) -> MaterialParams {
        let color = rel.and_then(|r| self.0.borrow().get(r).copied());
        MaterialParams {
            color: color.unwrap_or([1.0; 4]),
            ..Default::default()
        }
    }
}

fn cube(s: &mut Scene, name: &str, parent: Option<Entity>, x: f32, mat: &str) -> Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, Transform::from_position(Vec3::new(x, 0.0, 0.0)));
    s.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.add(
        e,
        MeshRenderer {
            material: Some(mat.into()),
            ..Default::default()
        },
    );
    e
}

/// The cached list must always equal a fresh gather.
fn check(cache: &mut DrawCache, s: &Scene, m: &Colors) -> CacheStats {
    let cached = cache.update(s, m, u32::MAX, |_| false);
    let fresh = DrawCache::new().update(s, m, u32::MAX, |_| false);
    assert_eq!(*cached, *fresh);
    cache.stats()
}

#[test]
fn incremental_updates_match_a_fresh_gather() {
    let mut s = Scene::new("t");
    let m = Colors::default();
    let root = cube(&mut s, "Root", None, 0.0, "A.mat");
    let kids: Vec<Entity> = (0..4)
        .map(|i| cube(&mut s, "Kid", Some(root), i as f32, "B.mat"))
        .collect();
    let other = cube(&mut s, "Other", None, 9.0, "A.mat");
    let mut cache = DrawCache::new();
    assert!(check(&mut cache, &s, &m).relaid);

    // Nothing changed: everything kept.
    let st = check(&mut cache, &s, &m);
    assert_eq!((st.rebuilt, st.kept, st.relaid), (0, 6, false));

    // Moving the parent rebuilds it and its children only.
    let mut t = s.transform(root);
    t.position.y = 2.0;
    s.set_transform(root, t);
    let st = check(&mut cache, &s, &m);
    assert_eq!((st.rebuilt, st.kept, st.relaid), (5, 1, false));

    // A material value change reaches every user of that material.
    m.0.borrow_mut()
        .insert("B.mat".into(), [1.0, 0.0, 0.0, 1.0]);
    let st = check(&mut cache, &s, &m);
    assert_eq!((st.rebuilt, st.kept), (4, 2));

    // A per-object override rebuilds that object alone.
    s.world
        .get::<&mut MeshRenderer>(kids[1])
        .unwrap()
        .edit_overrides(0, |o| o.set("tiling", OverrideValue::Vec2([3.0, 1.0])));
    let st = check(&mut cache, &s, &m);
    assert_eq!((st.rebuilt, st.kept), (1, 5));

    // Hiding, a second material element and new objects change the layout.
    s.game_object_mut(kids[2]).active = false;
    assert!(check(&mut cache, &s, &m).relaid);
    s.world
        .get::<&mut MeshRenderer>(other)
        .unwrap()
        .set_material_count(2);
    assert!(check(&mut cache, &s, &m).relaid);
    cube(&mut s, "New", Some(kids[0]), 1.0, "A.mat");
    assert!(check(&mut cache, &s, &m).relaid);
    // Showing it again reuses nothing stale.
    s.game_object_mut(kids[2]).active = true;
    s.world
        .get::<&mut MeshRenderer>(kids[2])
        .unwrap()
        .cast_shadows = false;
    assert!(check(&mut cache, &s, &m).relaid);
    let st = check(&mut cache, &s, &m);
    assert_eq!((st.rebuilt, st.relaid), (0, false));
    // Script-made shared material changes are seen too.
    let shared = crate::assets::Material {
        color: [0.0, 1.0, 0.0, 1.0],
        ..Default::default()
    };
    s.shared_materials.insert("A.mat".into(), shared);
    let st = check(&mut cache, &s, &m);
    assert!(st.rebuilt >= 3, "{st:?}");
}

#[test]
fn overrides_survive_prefab_apply_and_revert_and_old_files_load() {
    use crate::scene::prefab::Prefab;
    let mut s = Scene::new("t");
    let e = cube(&mut s, "Wall", None, 0.0, "A.mat");
    let edit = |s: &mut Scene, e: Entity, v: f32| {
        s.world
            .get::<&mut MeshRenderer>(e)
            .unwrap()
            .edit_overrides(0, |o| o.set("rotation", OverrideValue::Float(v)));
    };
    edit(&mut s, e, 45.0);
    // Apply: the asset keeps the override; a round trip through JSON too.
    let asset = s.prefab_to_apply(e, &Prefab::from_scene(&s, e));
    let (asset, _) = Prefab::from_json(&asset.to_json()).unwrap();
    let inst = s.instantiate_prefab(&asset, Some("Assets/Wall.prefab"), None);
    let rotation = |s: &Scene, e: Entity| {
        let r = s.world.get::<&MeshRenderer>(e).unwrap();
        r.overrides_at(0).and_then(|o| o.rotation)
    };
    assert_eq!(rotation(&s, inst), Some(45.0));
    // An instance edit is an override of the prefab; Revert brings the asset's back.
    edit(&mut s, inst, 10.0);
    assert!(s.has_prefab_overrides(inst, &asset));
    let back = s.revert_prefab(inst, &asset);
    assert_eq!(rotation(&s, back), Some(45.0));
    assert!(!s.has_prefab_overrides(back, &asset));
    // Renderers without overrides save exactly as before (no new key).
    let plain = serde_json::to_string(&MeshRenderer::default()).unwrap();
    assert!(!plain.contains("overrides"), "{plain}");
    let old: MeshRenderer = serde_json::from_str(r#"{"material":"A.mat"}"#).unwrap();
    assert!(old.overrides.is_empty());
}

#[test]
fn outlines_follow_the_selection_and_lod_levels_are_tagged() {
    let mut s = Scene::new("t");
    let m = Colors::default();
    let group = s.spawn("Group", None);
    let lod0 = cube(&mut s, "LOD0", Some(group), 0.0, "A.mat");
    let lod1 = cube(&mut s, "LOD1", Some(group), 0.0, "A.mat");
    let mut g = LodGroup::default();
    g.lods.truncate(2);
    g.lods[0].renderers = vec![lod0];
    g.lods[1].renderers = vec![lod1];
    s.add(group, g);
    let mut cache = DrawCache::new();
    let draws = cache.update(&s, &m, u32::MAX, |e| e == group);
    assert!(draws.iter().all(|d| d.outline), "children of the selection");
    let (a, b) = (draws[0].lod.unwrap(), draws[1].lod.unwrap());
    assert_eq!((a.min, a.max), (0.6, f32::INFINITY));
    assert_eq!((b.min, b.max), (0.3, 0.6));
    assert!((a.size - 1.0).abs() < 1e-5 && a.center == b.center);
    let draws = cache.update(&s, &m, u32::MAX, |_| false);
    assert!(draws.iter().all(|d| !d.outline));
}

/// Unchanged objects keep their cached entry from call to call (no copies); any change after
/// any number of unchanged calls still shows, and never a stale draw.
#[test]
fn kept_entries_never_hide_a_later_change() {
    let mut s = Scene::new("t");
    let m = Colors::default();
    let a = cube(&mut s, "A", None, 0.0, "A.mat");
    let b = cube(&mut s, "B", None, 2.0, "B.mat");
    let mut cache = DrawCache::new();
    check(&mut cache, &s, &m);
    for _ in 0..3 {
        let st = check(&mut cache, &s, &m);
        assert_eq!((st.rebuilt, st.kept), (0, 2));
    }
    // Another mesh, another material path, a renderer flag, a move: each seen at once.
    s.world.get::<&mut MeshFilter>(a).unwrap().mesh = Some(PrimitiveMesh::Sphere.into());
    assert_eq!(check(&mut cache, &s, &m).rebuilt, 1);
    check(&mut cache, &s, &m);
    s.world.get::<&mut MeshRenderer>(b).unwrap().material = Some("C.mat".into());
    assert_eq!(check(&mut cache, &s, &m).rebuilt, 1);
    check(&mut cache, &s, &m);
    s.world.get::<&mut MeshRenderer>(a).unwrap().receive_shadows = false;
    assert_eq!(check(&mut cache, &s, &m).rebuilt, 1);
    check(&mut cache, &s, &m);
    s.set_transform(b, Transform::from_position(Vec3::new(5.0, 1.0, 0.0)));
    assert_eq!(check(&mut cache, &s, &m).rebuilt, 1);
    // Disabled, then enabled again after unchanged calls: drawn with its current values.
    s.world.get::<&mut MeshRenderer>(a).unwrap().enabled = false;
    assert!(check(&mut cache, &s, &m).relaid);
    check(&mut cache, &s, &m);
    s.world.get::<&mut MeshRenderer>(a).unwrap().receive_shadows = true;
    s.world.get::<&mut MeshRenderer>(a).unwrap().enabled = true;
    assert!(check(&mut cache, &s, &m).relaid);
    let st = check(&mut cache, &s, &m);
    assert_eq!((st.rebuilt, st.kept), (0, 2));
}
