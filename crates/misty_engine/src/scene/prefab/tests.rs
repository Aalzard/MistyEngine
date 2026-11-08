use glam::Vec3;

use super::*;
use crate::scene::{BoxCollider, Light, MeshFilter, PrimitiveMesh, Transform};

const PATH: &str = "Assets/Crate.prefab";

/// A "Crate" with a child "Lid" and a Box Collider.
fn source() -> (Scene, Entity) {
    let mut s = Scene::new("t");
    let root = s.spawn("Crate", None);
    s.set_transform(root, Transform::from_position(Vec3::new(4.0, 0.0, 0.0)));
    s.add(root, BoxCollider::default());
    let lid = s.spawn("Lid", Some(root));
    s.add(
        lid,
        MeshFilter {
            mesh: Some(PrimitiveMesh::Cube.into()),
        },
    );
    (s, root)
}

/// The crate as the editor saves it (objects carry ids).
fn crate_prefab() -> Prefab {
    let (s, root) = source();
    Prefab::from_scene(&s, root).with_ids()
}

fn names(s: &Scene) -> Vec<(String, usize)> {
    s.walk().into_iter().map(|(e, d)| (s.name(e), d)).collect()
}

fn keys(s: &Scene, root: Entity, p: &Prefab) -> Vec<Override> {
    diff::record(s.prefab_overrides(root, p).into_iter().map(|o| o.key))
}

fn child(s: &Scene, e: Entity, name: &str) -> Entity {
    s.children(e)
        .into_iter()
        .find(|c| s.name(*c) == name)
        .unwrap_or_else(|| panic!("no child {name}"))
}

fn scale(s: &Scene, e: Entity) -> Vec3 {
    s.transform(e).scale
}

fn set_scale(s: &mut Scene, e: Entity, v: f32) {
    let mut t = s.transform(e);
    t.scale = Vec3::splat(v);
    s.set_transform(e, t);
}

fn prop(object: u64, component: &str, property: &str) -> Override {
    Override::Property {
        object,
        component: component.into(),
        property: property.into(),
    }
}

/// Level id of `e` in the instance at `root`.
fn id_of(s: &Scene, root: Entity, e: Entity) -> u64 {
    s.prefab_object_ids(root)
        .into_iter()
        .find(|(x, _)| *x == e)
        .and_then(|(_, id)| id)
        .unwrap()
}

#[test]
fn round_trip_and_instantiate() {
    let (s, root) = source();
    let prefab = Prefab::from_scene(&s, root);
    let (loaded, report) = Prefab::from_json(&prefab.to_json()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(loaded, prefab);
    assert_eq!(
        loaded.outline(),
        [("Crate".to_owned(), 0), ("Lid".to_owned(), 1)]
    );
    assert_eq!(loaded.root_components(), ["Transform", "Box Collider"]);

    let mut scene = Scene::new("level");
    let a = scene.instantiate_prefab(&loaded, Some(PATH), None);
    let b = scene.instantiate_prefab(&loaded, Some(PATH), None);
    assert_eq!(
        names(&scene),
        [
            ("Crate".to_owned(), 0),
            ("Lid".to_owned(), 1),
            ("Crate (1)".to_owned(), 0),
            ("Lid".to_owned(), 1)
        ]
    );
    assert_eq!(scene.prefab_link(a).as_deref(), Some(PATH));
    assert_eq!(scene.prefab_instances(PATH), [a, b]);
    let lid = scene.children(a)[0];
    assert_eq!(scene.prefab_root(lid), Some(a));
    assert_eq!(scene.prefab_link(lid), None, "only the root is linked");
    assert_eq!(scene.transform(a).position, Vec3::new(4.0, 0.0, 0.0));
    assert!(
        !scene.component_names(a).iter().any(|n| n == "Prefab"),
        "the link is not an Inspector component"
    );
}

#[test]
fn files_with_ids_and_records_round_trip() {
    let prefab = crate_prefab();
    let json = prefab.to_json();
    assert!(json.contains("\"Prefab Source\""), "{json}");
    let (loaded, report) = Prefab::from_json(&json).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(loaded, prefab);

    // A scene instance saves its record; files without one load as before.
    let mut scene = Scene::new("level");
    let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
    set_scale(&mut scene, a, 2.0);
    let record = keys(&scene, a, &prefab);
    scene.set_prefab_record(a, Some(record.clone()));
    let text = scene.to_json();
    assert!(text.contains("\"kind\": \"property\""), "{text}");
    let (back, _) = Scene::from_json("level", &text).unwrap();
    assert_eq!(back.prefab_record(back.roots()[0]), Some(record));
    let old = text.replace("\"overrides\"", "\"overrides_unknown\"");
    let (old, _) = Scene::from_json("level", &old).unwrap();
    assert_eq!(old.prefab_record(old.roots()[0]), None);
}

#[test]
fn link_survives_scene_save() {
    let (s, root) = source();
    let prefab = Prefab::from_scene(&s, root);
    let mut scene = Scene::new("level");
    let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
    let (loaded, _) = Scene::from_json("level", &scene.to_json()).unwrap();
    let root = loaded.roots()[0];
    assert_eq!(loaded.prefab_link(root).as_deref(), Some(PATH));
    assert!(!loaded.has_prefab_overrides(root, &prefab));
    assert!(scene.unlink_prefab(a));
    assert_eq!(scene.prefab_root(a), None);
}

#[test]
fn root_name_and_position_are_not_overrides() {
    let (s, root) = source();
    let prefab = Prefab::from_scene(&s, root);
    let mut scene = Scene::new("level");
    let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
    scene.game_object_mut(a).name = "Renamed".into();
    scene.set_transform(
        a,
        Transform::from_position(Vec3::ONE).with_euler(Vec3::new(0.0, 90.0, 0.0)),
    );
    assert!(!scene.has_prefab_overrides(a, &prefab));

    set_scale(&mut scene, a, 2.0);
    assert_eq!(keys(&scene, a, &prefab), [prop(0, "Transform", "scale")]);
}

#[test]
fn diff_lists_each_property_component_and_object() {
    let prefab = crate_prefab();
    let mut scene = Scene::new("level");
    let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
    let lid = child(&scene, a, "Lid");
    let lid_id = id_of(&scene, a, lid);
    // A renamed child stays the same object (it carries its id).
    scene.game_object_mut(lid).name = "Top".into();
    set_scale(&mut scene, lid, 3.0);
    scene.add(lid, Light::default());
    scene.remove_by_name(a, "Box Collider");
    let extra = scene.spawn("Handle", Some(a));
    let items = scene.prefab_overrides(a, &prefab);
    let got = diff::record(items.iter().map(|o| o.key.clone()));
    assert_eq!(
        got,
        diff::record([
            prop(lid_id, "GameObject", "name"),
            prop(lid_id, "Transform", "scale"),
            Override::AddedComponent {
                object: lid_id,
                component: "Light".into()
            },
            Override::RemovedComponent {
                object: 0,
                component: "Box Collider".into()
            },
            Override::AddedObject {
                parent: 0,
                name: "Handle".into()
            },
        ])
    );
    let added = items
        .iter()
        .find(|o| matches!(o.key, Override::AddedObject { .. }))
        .unwrap();
    assert_eq!(added.entity, Some(extra));

    // Deleting the child is one removed object, whatever it had.
    scene.despawn(lid);
    assert!(
        keys(&scene, a, &prefab).contains(&Override::RemovedObject { object: lid_id }),
        "removed objects are listed"
    );
}

#[test]
fn revert_one_override_keeps_the_others() {
    let prefab = crate_prefab();
    let mut scene = Scene::new("level");
    let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
    let lid = child(&scene, a, "Lid");
    let lid_id = id_of(&scene, a, lid);
    set_scale(&mut scene, lid, 3.0);
    set_scale(&mut scene, a, 2.0);
    scene.spawn("Handle", Some(a));

    scene.revert_prefab_overrides(a, &prefab, &[prop(lid_id, "Transform", "scale")]);
    assert_eq!(scale(&scene, lid), Vec3::ONE, "reverted in place");
    assert!(scene.contains(lid), "same object");
    assert_eq!(scale(&scene, a), Vec3::splat(2.0), "other override kept");
    assert!(scene.children(a).iter().any(|c| scene.name(*c) == "Handle"));

    let handle = Override::AddedObject {
        parent: 0,
        name: "Handle".into(),
    };
    scene.revert_prefab_overrides(a, &prefab, &[handle]);
    assert_eq!(keys(&scene, a, &prefab), [prop(0, "Transform", "scale")]);
}

#[test]
fn revert_restores_contents_but_keeps_placement() {
    let (s, root) = source();
    let prefab = Prefab::from_scene(&s, root);
    let mut scene = Scene::new("level");
    scene.spawn("Before", None);
    let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
    scene.spawn("After", None);
    scene.set_transform(a, Transform::from_position(Vec3::new(0.0, 5.0, 0.0)));
    let lid = scene.children(a)[0];
    scene.despawn(lid);
    scene.spawn("Extra", Some(a));
    scene.remove_by_name(a, "Box Collider");
    assert!(scene.has_prefab_overrides(a, &prefab));

    let back = scene.revert_prefab(a, &prefab);
    assert_eq!(back, a, "reverted in place");
    assert!(!scene.has_prefab_overrides(a, &prefab));
    assert_eq!(scene.sibling_index(a), 1, "same place in the Hierarchy");
    assert_eq!(scene.transform(a).position, Vec3::new(0.0, 5.0, 0.0));
    assert_eq!(scene.prefab_link(a).as_deref(), Some(PATH));
    assert_eq!(scene.name(scene.children(a)[0]), "Lid");
    assert_eq!(scene.children(a).len(), 1);
    assert_eq!(scene.component_names(a), ["Transform", "Box Collider"]);
}

#[test]
fn apply_keeps_the_asset_root_placement() {
    let (s, root) = source();
    let prefab = Prefab::from_scene(&s, root);
    let mut scene = Scene::new("level");
    let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
    scene.set_transform(a, Transform::from_position(Vec3::new(9.0, 9.0, 9.0)));
    scene.game_object_mut(a).name = "Moved".into();
    scene.spawn("Handle", Some(a));

    let applied = scene.prefab_to_apply(a, &prefab);
    assert_eq!(applied.outline()[0].0, "Crate");
    assert_eq!(applied.outline().len(), 3);
    let mut other = Scene::new("o");
    let b = other.instantiate_prefab(&applied, None, None);
    assert_eq!(other.transform(b).position, Vec3::new(4.0, 0.0, 0.0));
    assert!(!scene.has_prefab_overrides(a, &applied));
}

#[test]
fn apply_one_override_and_other_instances_keep_theirs() {
    let prefab = crate_prefab();
    let mut scene = Scene::new("level");
    let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
    let b = scene.instantiate_prefab(&prefab, Some(PATH), None);
    let lid_a = child(&scene, a, "Lid");
    let lid_b = child(&scene, b, "Lid");
    let lid_id = id_of(&scene, a, lid_a);
    // a: lid scale 3 and the root's collider removed; b: its own lid name and an extra light.
    set_scale(&mut scene, lid_a, 3.0);
    scene.remove_by_name(a, "Box Collider");
    scene.game_object_mut(lid_b).name = "Top".into();
    scene.add(b, Light::default());

    // Apply only a's lid scale.
    let new = scene.apply_prefab_overrides(a, &prefab, &[prop(lid_id, "Transform", "scale")]);
    assert_eq!(
        keys(&scene, a, &new),
        [Override::RemovedComponent {
            object: 0,
            component: "Box Collider".into()
        }],
        "the applied override is gone, the other stays"
    );
    // b follows the asset and keeps its own overrides.
    assert!(scene.resync_prefab(b, &prefab, &new));
    assert_eq!(scale(&scene, lid_b), Vec3::splat(3.0));
    assert_eq!(scene.name(lid_b), "Top");
    assert!(scene.world.get::<&Light>(b).is_ok());
    assert_eq!(
        keys(&scene, b, &new),
        diff::record([
            prop(lid_id, "GameObject", "name"),
            Override::AddedComponent {
                object: 0,
                component: "Light".into()
            }
        ])
    );

    // Apply All on b: a (which removed the collider) keeps that.
    let all = scene.prefab_to_apply(b, &new);
    assert!(scene.resync_prefab(a, &new, &all));
    assert!(scene.world.get::<&Light>(a).is_ok(), "picked up b's light");
    assert!(
        scene.world.get::<&BoxCollider>(a).is_err(),
        "kept its removal"
    );
    assert_eq!(scene.name(lid_a), "Top");
    assert!(!scene.has_prefab_overrides(b, &all));
}

#[test]
fn applying_added_objects_and_components() {
    let prefab = crate_prefab();
    let mut scene = Scene::new("level");
    let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
    let b = scene.instantiate_prefab(&prefab, Some(PATH), None);
    let handle = scene.spawn("Handle", Some(a));
    scene.add(handle, BoxCollider::default());
    let lid = child(&scene, a, "Lid");
    scene.add(lid, Light::default());
    let new = scene.prefab_to_apply(a, &prefab);
    assert!(!scene.has_prefab_overrides(a, &new));
    assert!(
        scene.world.get::<&PrefabSource>(handle).is_ok(),
        "the added object got an id"
    );
    assert!(scene.resync_prefab(b, &prefab, &new));
    let handle_b = child(&scene, b, "Handle");
    assert!(scene.world.get::<&BoxCollider>(handle_b).is_ok());
    assert!(scene.world.get::<&Light>(child(&scene, b, "Lid")).is_ok());
    assert!(!scene.has_prefab_overrides(b, &new));
    assert_eq!(
        id_of(&scene, a, handle),
        id_of(&scene, b, handle_b),
        "both instances share the new object's id"
    );
}

#[test]
fn removed_objects_stay_removed_when_the_asset_changes() {
    let prefab = crate_prefab();
    let mut scene = Scene::new("level");
    let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
    let lid = child(&scene, a, "Lid");
    scene.despawn(lid);
    // The asset gains a second child and changes its collider.
    let (mut src, root) = prefab.to_scene("src");
    src.spawn("Hinge", Some(root));
    src.world.get::<&mut BoxCollider>(root).unwrap().size = Vec3::splat(5.0);
    let new = Prefab::from_scene(&src, root).with_ids();
    assert!(scene.resync_prefab(a, &prefab, &new));
    assert_eq!(
        scene
            .children(a)
            .iter()
            .map(|c| scene.name(*c))
            .collect::<Vec<_>>(),
        ["Hinge"]
    );
    assert_eq!(
        scene.world.get::<&BoxCollider>(a).unwrap().size,
        Vec3::splat(5.0)
    );
}

#[test]
fn references_inside_the_prefab_survive_following_it() {
    use crate::vcam::VirtualCamera;
    let (mut src, root) = source();
    let lid = src.children(root)[0];
    let cam = src.spawn("Cam", Some(root));
    src.add(
        cam,
        VirtualCamera {
            follow: Some(lid),
            ..Default::default()
        },
    );
    let prefab = Prefab::from_scene(&src, root).with_ids();
    let mut scene = Scene::new("level");
    let target = scene.spawn("Player", None);
    let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
    let cam_a = child(&scene, a, "Cam");
    let lid_a = child(&scene, a, "Lid");
    assert_eq!(
        scene.world.get::<&VirtualCamera>(cam_a).unwrap().follow,
        Some(lid_a)
    );
    // An instance aiming at a scene object keeps it when the asset changes.
    scene
        .world
        .get::<&mut VirtualCamera>(cam_a)
        .unwrap()
        .look_at = Some(target);
    src.world.get::<&mut VirtualCamera>(cam).unwrap().priority = 20;
    let new = Prefab::from_scene(&src, root).with_ids();
    assert!(scene.resync_prefab(a, &prefab, &new));
    let v = scene.world.get::<&VirtualCamera>(cam_a).unwrap();
    assert_eq!(v.priority, 20);
    assert_eq!(v.follow, Some(lid_a));
    assert_eq!(v.look_at, Some(target));
}

#[test]
fn variants_inherit_through_a_chain() {
    let base = crate_prefab();
    const BASE: &str = "Assets/Crate.prefab";
    const V1: &str = "Assets/Crate Big.prefab";
    // Variant 1 scales the lid; variant 2 (of variant 1) adds a light.
    let v1 = Prefab::variant_of(BASE, &base, "Crate Big");
    assert_eq!(v1.base(), Some(BASE));
    let (mut s1, r1) = v1.to_scene("v1");
    assert!(
        !s1.has_prefab_overrides(r1, &base),
        "a new variant changes nothing"
    );
    let lid1 = child(&s1, r1, "Lid");
    set_scale(&mut s1, lid1, 2.0);
    let v1 = Prefab::from_scene_keep_base(&s1, r1);
    let (loaded, _) = Prefab::from_json(&v1.to_json()).unwrap();
    assert_eq!(loaded.base(), Some(BASE), "the base survives the file");
    let v1 = loaded;

    let v2 = Prefab::variant_of(V1, &v1, "Crate Big Lit");
    let (mut s2, r2) = v2.to_scene("v2");
    s2.add(r2, Light::default());
    let v2 = Prefab::from_scene_keep_base(&s2, r2);

    // The base changes: its collider grows and the lid is renamed.
    let (mut src, root) = base.to_scene("src");
    src.world.get::<&mut BoxCollider>(root).unwrap().size = Vec3::splat(4.0);
    let lid = child(&src, root, "Lid");
    src.game_object_mut(lid).name = "Cover".into();
    set_scale(&mut src, lid, 9.0);
    let base2 = Prefab::from_scene(&src, root).with_ids();

    let v1b = v1
        .follow(BASE, &base, &base2)
        .expect("variant follows its base");
    assert!(
        v2.follow(BASE, &base, &base2).is_none(),
        "v2 uses v1, not the base"
    );
    let v2b = v2.follow(V1, &v1, &v1b).expect("follows variant 1");
    let (s, r) = v2b.to_scene("check");
    let lid = child(&s, r, "Cover");
    assert_eq!(
        scale(&s, lid),
        Vec3::splat(2.0),
        "variant 1's override wins"
    );
    assert_eq!(
        s.world.get::<&BoxCollider>(r).unwrap().size,
        Vec3::splat(4.0),
        "base change flows through"
    );
    assert!(s.world.get::<&Light>(r).is_ok(), "variant 2's own change");
    assert_eq!(s.name(r), "Crate Big Lit");
    assert_eq!(v2b.base(), Some(V1));
    assert_eq!(
        v1b.base_record().map(|r| r.len()),
        Some(1),
        "the variant's record lists its one override"
    );
}

#[test]
fn nested_prefabs_propagate_and_keep_outer_overrides() {
    const INNER: &str = "Assets/Wheel.prefab";
    const OUTER: &str = "Assets/Cart.prefab";
    let mut w = Scene::new("w");
    let wheel = w.spawn("Wheel", None);
    let hub = w.spawn("Hub", Some(wheel));
    w.add(hub, BoxCollider::default());
    let wheel_p = Prefab::from_scene(&w, wheel).with_ids();

    // The cart holds two wheels; the cart overrides the second wheel's hub scale.
    let mut c = Scene::new("c");
    let cart = c.spawn("Cart", None);
    c.instantiate_prefab(&wheel_p, Some(INNER), Some(cart));
    let w2 = c.instantiate_prefab(&wheel_p, Some(INNER), Some(cart));
    let hub2 = child(&c, w2, "Hub");
    set_scale(&mut c, hub2, 2.0);
    let cart_p = Prefab::from_scene(&c, cart).with_ids();
    assert_eq!(cart_p.nested_paths(), [INNER]);

    // A scene instance of the cart scales the first wheel's hub.
    let mut scene = Scene::new("level");
    let a = scene.instantiate_prefab(&cart_p, Some(OUTER), None);
    let wheels = scene.children(a);
    let hub1 = child(&scene, wheels[0], "Hub");
    let hub2 = child(&scene, wheels[1], "Hub");
    assert_ne!(
        id_of(&scene, a, hub1),
        id_of(&scene, a, hub2),
        "nested objects have distinct ids"
    );
    set_scale(&mut scene, hub1, 5.0);
    assert_eq!(
        keys(&scene, a, &cart_p),
        [prop(id_of(&scene, a, hub1), "Transform", "scale")]
    );

    // The wheel prefab changes its hub collider: the cart follows, then the scene.
    let (src, root) = wheel_p.to_scene("src");
    let h = child(&src, root, "Hub");
    src.world.get::<&mut BoxCollider>(h).unwrap().size = Vec3::splat(7.0);
    let wheel_p2 = Prefab::from_scene(&src, root).with_ids();
    let cart_p2 = cart_p.follow(INNER, &wheel_p, &wheel_p2).unwrap();
    assert!(scene.resync_prefab(a, &cart_p, &cart_p2));
    for hub in [hub1, hub2] {
        assert_eq!(
            scene.world.get::<&BoxCollider>(hub).unwrap().size,
            Vec3::splat(7.0)
        );
    }
    assert_eq!(scale(&scene, hub1), Vec3::splat(5.0), "scene override kept");
    assert_eq!(scale(&scene, hub2), Vec3::splat(2.0), "cart override kept");
    // Links inside the instance are preserved.
    assert_eq!(scene.prefab_link(wheels[0]).as_deref(), Some(INNER));
    assert_eq!(scene.prefab_roots(hub1), [wheels[0], a]);
}

#[test]
fn record_catches_up_with_an_asset_changed_while_closed() {
    let prefab = crate_prefab();
    let mut scene = Scene::new("level");
    let a = scene.instantiate_prefab(&prefab, Some(PATH), None);
    let lid = child(&scene, a, "Lid");
    set_scale(&mut scene, lid, 3.0);
    scene.spawn("Handle", Some(a));
    let record = keys(&scene, a, &prefab);
    scene.set_prefab_record(a, Some(record));
    let (mut loaded, _) = Scene::from_json("level", &scene.to_json()).unwrap();
    let a = loaded.roots()[0];
    assert!(
        !loaded.sync_prefab_to_record(a, &prefab),
        "nothing to do while the asset is the same"
    );

    // The asset changed on disk: the lid got a light, the collider a new size.
    let (mut src, root) = prefab.to_scene("src");
    let src_lid = child(&src, root, "Lid");
    src.add(src_lid, Light::default());
    set_scale(&mut src, src_lid, 8.0);
    src.world.get::<&mut BoxCollider>(root).unwrap().size = Vec3::splat(6.0);
    let new = Prefab::from_scene(&src, root).with_ids();
    assert!(loaded.sync_prefab_to_record(a, &new));
    let lid = child(&loaded, a, "Lid");
    assert!(loaded.world.get::<&Light>(lid).is_ok());
    assert_eq!(scale(&loaded, lid), Vec3::splat(3.0), "override kept");
    assert_eq!(
        loaded.world.get::<&BoxCollider>(a).unwrap().size,
        Vec3::splat(6.0)
    );
    assert!(
        loaded
            .children(a)
            .iter()
            .any(|c| loaded.name(*c) == "Handle")
    );

    // Older scenes (no record) are left as they are.
    loaded.set_prefab_record(a, None);
    assert!(!loaded.sync_prefab_to_record(a, &prefab));
}

#[test]
fn unpack_keeps_nested_instances_unless_completely() {
    const INNER: &str = "Assets/Wheel.prefab";
    let mut w = Scene::new("w");
    let wheel = w.spawn("Wheel", None);
    w.spawn("Hub", Some(wheel));
    let wheel_p = Prefab::from_scene(&w, wheel).with_ids();
    let mut c = Scene::new("c");
    let cart = c.spawn("Cart", None);
    c.instantiate_prefab(&wheel_p, Some(INNER), Some(cart));
    let cart_p = Prefab::from_scene(&c, cart).with_ids();

    let mut scene = Scene::new("level");
    let a = scene.instantiate_prefab(&cart_p, Some(PATH), None);
    let wheel = scene.children(a)[0];
    let b = scene.instantiate_prefab(&cart_p, Some(PATH), None);
    assert!(scene.unpack_prefab(a, false));
    assert_eq!(scene.prefab_link(a), None);
    assert_eq!(scene.prefab_link(wheel).as_deref(), Some(INNER));
    assert!(
        !scene.has_prefab_overrides(wheel, &wheel_p),
        "still a clean wheel"
    );
    assert!(scene.world.get::<&PrefabSource>(wheel).is_err());

    assert!(scene.unpack_prefab(b, true));
    assert!(
        scene
            .subtree(b)
            .iter()
            .all(|e| scene.prefab_link(*e).is_none())
    );
    assert!(
        scene
            .subtree(b)
            .iter()
            .all(|e| scene.world.get::<&PrefabSource>(*e).is_err())
    );
    assert!(!scene.unpack_prefab(b, false), "nothing left to unpack");
}

#[test]
fn invalid_prefabs_are_rejected() {
    assert!(Prefab::from_json(r#"{"version": 1, "objects": []}"#).is_err());
    let two_roots = r#"{"version": 1, "objects": [
        {"parent": null, "components": {}}, {"parent": null, "components": {}}]}"#;
    assert!(Prefab::from_json(two_roots).is_err());
}
