use glam::Vec3;
use misty_engine::scene::{BoxCollider, Light, Transform};

use super::*;
use crate::project::ProjectBrowser;

fn state(tag: &str) -> EditorState {
    let root =
        std::env::temp_dir().join(format!("misty_prefab_editor_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    EditorState::new(ProjectBrowser::open(root))
}

fn folder(s: &EditorState) -> PathBuf {
    s.project.assets_dir()
}

/// Turn `e` into a new prefab asset in Assets; returns its path.
fn make_prefab(s: &mut EditorState, e: Entity) -> String {
    let dir = folder(s);
    apply(
        s,
        Action::CreatePrefab {
            entity: e,
            folder: dir,
        },
    );
    s.scene.prefab_link(e).expect("linked")
}

fn child(s: &EditorState, e: Entity, name: &str) -> Entity {
    s.scene
        .children(e)
        .into_iter()
        .find(|c| s.scene.name(*c) == name)
        .unwrap_or_else(|| panic!("no child {name}"))
}

fn set_scale(s: &mut EditorState, e: Entity, v: f32) {
    let mut t = s.scene.transform(e);
    t.scale = Vec3::splat(v);
    s.scene.set_transform(e, t);
}

fn keys(s: &mut EditorState, root: Entity) -> Vec<Override> {
    let items = overrides_of(s, root).expect("asset exists");
    prefab::diff::record(items.into_iter().map(|o| o.key))
}

#[test]
fn create_links_and_instantiate_is_undoable() {
    let mut s = state("create");
    let cube = s.scene.spawn("Cube", None);
    s.scene.spawn("Child", Some(cube));
    let dir = folder(&s);
    apply(
        &mut s,
        Action::CreatePrefab {
            entity: cube,
            folder: dir.clone(),
        },
    );
    assert!(dir.join("Cube.prefab").exists());
    assert_eq!(
        s.scene.prefab_link(cube).as_deref(),
        Some("Assets/Cube.prefab")
    );
    assert!(
        s.project.entries.iter().any(|e| e.name == "Cube.prefab"),
        "Asset Files shows the new asset"
    );
    assert!(
        !s.scene
            .has_prefab_overrides(cube, &s.assets.prefab("Assets/Cube.prefab").unwrap()),
        "the object matches its new asset"
    );

    // A second prefab from the same name gets a free file name.
    apply(
        &mut s,
        Action::CreatePrefab {
            entity: cube,
            folder: dir.clone(),
        },
    );
    assert!(dir.join("Cube 1.prefab").exists());

    let before = s.scene.len();
    let e = instantiate(&mut s, "Assets/Cube.prefab", None).unwrap();
    assert_eq!(s.scene.len(), before + 2);
    assert_eq!(
        s.scene.world_position(e),
        s.scene_view.camera.pivot().round(),
        "placed at the Editor Scene pivot"
    );
    assert_eq!(s.selection.active(), Some(e));
    s.undo_step();
    assert_eq!(s.scene.len(), before);
    assert!(instantiate(&mut s, "Assets/Nope.prefab", None).is_none());
}

#[test]
fn apply_updates_asset_and_other_instances_revert_restores() {
    let mut s = state("apply");
    let crate_e = s.scene.spawn("Crate", None);
    let rel = make_prefab(&mut s, crate_e);
    instantiate(&mut s, &rel, None).unwrap();

    s.scene.add(crate_e, BoxCollider::default());
    let prefab = s.assets.prefab(&rel).unwrap();
    assert!(s.scene.has_prefab_overrides(crate_e, &prefab));
    apply(&mut s, Action::ApplyPrefab(crate_e));
    let prefab = s.assets.prefab(&rel).unwrap();
    assert!(!s.scene.has_prefab_overrides(crate_e, &prefab));
    let other = s.scene.prefab_instances(&rel)[1];
    assert!(
        s.scene
            .component_names(other)
            .contains(&"Box Collider".into()),
        "other instances pick up the change"
    );

    s.scene.remove_by_name(other, "Box Collider");
    apply(&mut s, Action::RevertPrefab(other));
    let other = s.selection.active().unwrap();
    assert!(
        s.scene
            .component_names(other)
            .contains(&"Box Collider".into())
    );

    apply(&mut s, Action::UnpackPrefab(other));
    assert_eq!(s.scene.prefab_link(other), None);
    s.undo_step();
    assert!(s.scene.prefab_link(other).is_some(), "unpack is undoable");
}

/// An instance dragged inside another instance of the same prefab was applied into the
/// asset: the prefab contained itself, and each Apply nested it one level deeper.
#[test]
fn a_prefab_cannot_be_applied_into_itself() {
    let mut s = state("self_nest");
    let crate_e = s.scene.spawn("Crate", None);
    let rel = make_prefab(&mut s, crate_e);
    let other = instantiate(&mut s, &rel, None).unwrap();
    let steps = s.undo.steps().len();
    s.scene.set_parent(other, Some(crate_e), None, true);
    apply(&mut s, Action::ApplyPrefab(crate_e));
    let prefab = s.assets.prefab(&rel).unwrap();
    assert!(
        !prefab.nested_paths().contains(&rel),
        "{:?}",
        prefab.nested_paths()
    );
    assert_eq!(s.undo.steps().len(), steps, "nothing applied, no step");
    assert_eq!(
        s.scene.parent(other),
        Some(crate_e),
        "the scene is as it was"
    );
}

/// A duplicate of an instance's child kept the original's prefab id: moved above the
/// original it took the original's place in the instance, and Revert deleted the original.
#[test]
fn a_duplicate_inside_an_instance_never_takes_the_originals_place() {
    let mut s = state("dup_id");
    let cube = s.scene.spawn("Cube", None);
    let lid = s.scene.spawn("Lid", Some(cube));
    make_prefab(&mut s, cube);
    s.selection.set(lid);
    crate::commands::apply(&mut s, Action::DuplicateSelected);
    let copy = s.selection.active().unwrap();
    assert_ne!(copy, lid);
    s.scene.set_parent(copy, Some(cube), Some(0), false);
    apply(&mut s, Action::RevertPrefab(cube));
    assert!(s.scene.contains(lid), "the original stays");
    assert!(!s.scene.contains(copy), "the copy was the added object");
}

#[test]
fn per_item_apply_and_revert_keep_other_instances_overrides() {
    let mut s = state("items");
    let crate_e = s.scene.spawn("Crate", None);
    let lid = s.scene.spawn("Lid", Some(crate_e));
    let rel = make_prefab(&mut s, crate_e);
    let other = instantiate(&mut s, &rel, None).unwrap();
    let other_lid = child(&s, other, "Lid");
    // The other instance overrides its lid's scale and adds a light.
    set_scale(&mut s, other_lid, 4.0);
    s.scene.add(other, Light::default());

    // This one changes its lid's position and scale; only the position is applied.
    let mut t = Transform::from_position(Vec3::new(0.0, 2.0, 0.0));
    t.scale = Vec3::splat(2.0);
    s.scene.set_transform(lid, t);
    let items = overrides_of(&mut s, crate_e).unwrap();
    let position = items
        .iter()
        .find(|o| matches!(&o.key, Override::Property { property, .. } if property == "position"))
        .unwrap()
        .key
        .clone();
    apply(
        &mut s,
        Action::ApplyPrefabOverrides {
            root: crate_e,
            keys: vec![position],
        },
    );
    assert_eq!(
        keys(&mut s, crate_e).len(),
        1,
        "the scale is still an override"
    );
    assert_eq!(
        s.scene.transform(other_lid).position,
        Vec3::new(0.0, 2.0, 0.0),
        "the other instance took the applied position"
    );
    assert_eq!(
        s.scene.transform(other_lid).scale,
        Vec3::splat(4.0),
        "and kept its own scale"
    );
    assert!(s.scene.world.get::<&Light>(other).is_ok());
    assert_eq!(s.undo.undo_label(), Some("Apply Prefab"));

    // Apply All on the other instance: this one keeps its scale override.
    apply(&mut s, Action::ApplyPrefab(other));
    assert!(
        s.scene.world.get::<&Light>(crate_e).is_ok(),
        "light arrived"
    );
    assert_eq!(s.scene.transform(lid).scale, Vec3::splat(2.0));

    // Revert just the scale.
    let scale = keys(&mut s, crate_e)
        .into_iter()
        .find(|k| matches!(k, Override::Property { property, .. } if property == "scale"))
        .unwrap();
    apply(
        &mut s,
        Action::RevertPrefabOverrides {
            root: crate_e,
            keys: vec![scale],
        },
    );
    assert_eq!(
        s.scene.transform(lid).scale,
        Vec3::splat(4.0),
        "the asset's value"
    );
    assert!(keys(&mut s, crate_e).is_empty());
    s.undo_step();
    assert_eq!(s.scene.transform(lid).scale, Vec3::splat(2.0), "undoable");
}

#[test]
fn variants_follow_their_base() {
    let mut s = state("variant");
    let crate_e = s.scene.spawn("Crate", None);
    s.scene.spawn("Lid", Some(crate_e));
    let base = make_prefab(&mut s, crate_e);
    let variant = create_variant(&mut s, &base).expect("variant created");
    assert_eq!(variant, "Assets/Crate Variant.prefab");
    assert_eq!(
        s.assets.prefab(&variant).unwrap().base(),
        Some(base.as_str())
    );

    // An instance of the variant overrides nothing yet.
    let v = instantiate(&mut s, &variant, None).unwrap();
    assert!(keys(&mut s, v).is_empty());
    assert_eq!(s.scene.name(v), "Crate Variant");

    // The variant's own change: its lid is bigger (made through an instance + Apply).
    let v_lid = child(&s, v, "Lid");
    set_scale(&mut s, v_lid, 3.0);
    apply(&mut s, Action::ApplyPrefab(v));

    // The base gains a collider: the variant file and its instance follow; the lid stays big.
    s.scene.add(crate_e, BoxCollider::default());
    apply(&mut s, Action::ApplyPrefab(crate_e));
    let vp = s.assets.prefab(&variant).unwrap();
    assert!(vp.root_components().contains(&"Box Collider".to_owned()));
    assert!(s.scene.world.get::<&BoxCollider>(v).is_ok());
    assert_eq!(s.scene.transform(v_lid).scale, Vec3::splat(3.0));
    assert_eq!(
        vp.base_record().map(<[Override]>::len),
        Some(1),
        "the variant records its one change"
    );
}

#[test]
fn nested_prefab_changes_update_the_outer_prefab_file() {
    let mut s = state("nested");
    let wheel = s.scene.spawn("Wheel", None);
    s.scene.spawn("Hub", Some(wheel));
    let wheel_rel = make_prefab(&mut s, wheel);
    // A cart holding a wheel instance becomes a prefab: the wheel stays linked inside it.
    let cart = s.scene.spawn("Cart", None);
    let inner = instantiate(&mut s, &wheel_rel, Some(cart)).unwrap();
    let cart_rel = make_prefab(&mut s, cart);
    assert_eq!(
        s.scene.prefab_link(inner).as_deref(),
        Some(wheel_rel.as_str())
    );
    assert_eq!(
        s.assets.prefab(&cart_rel).unwrap().nested_paths(),
        std::slice::from_ref(&wheel_rel)
    );
    // A prefab can't go inside itself.
    assert!(instantiate(&mut s, &cart_rel, Some(inner)).is_none());

    // Change the wheel through the loose instance and apply.
    s.scene.add(wheel, BoxCollider::default());
    apply(&mut s, Action::ApplyPrefab(wheel));
    let text = std::fs::read_to_string(s.assets.absolute(&cart_rel)).unwrap();
    assert!(
        text.contains("Box Collider"),
        "Cart.prefab was updated: {text}"
    );
    assert!(
        s.scene.world.get::<&BoxCollider>(inner).is_ok(),
        "the wheel inside the cart instance too"
    );
    assert!(
        keys(&mut s, cart).is_empty(),
        "no stray overrides on the cart"
    );
}

#[test]
fn saved_scenes_record_overrides_and_catch_up_when_opened() {
    let mut s = state("record");
    let crate_e = s.scene.spawn("Crate", None);
    let lid = s.scene.spawn("Lid", Some(crate_e));
    let rel = make_prefab(&mut s, crate_e);
    set_scale(&mut s, lid, 3.0);
    let scene_rel = "Assets/Level.scene";
    let id = s.scene.active_scene();
    assert!(crate::scene_files::write(&mut s, id, scene_rel));
    let text = std::fs::read_to_string(s.project.root.join(scene_rel)).unwrap();
    assert!(text.contains("\"overrides\""), "{text}");

    // Someone edits the prefab while the scene is closed: a collider, and a new lid scale.
    let (mut src, root) = s.assets.prefab(&rel).unwrap().to_scene("src");
    src.add(root, BoxCollider::default());
    let src_lid = src.children(root)[0];
    let mut t = src.transform(src_lid);
    t.scale = Vec3::splat(9.0);
    src.set_transform(src_lid, t);
    Prefab::from_scene(&src, root)
        .with_ids()
        .save(&s.assets.absolute(&rel))
        .unwrap();
    s.assets.forget_prefabs();

    assert!(crate::scene_files::load(&mut s, scene_rel));
    let crate_e = s.scene.find_by_name("Crate").unwrap();
    let lid = child(&s, crate_e, "Lid");
    assert!(
        s.scene.world.get::<&BoxCollider>(crate_e).is_ok(),
        "caught up"
    );
    assert_eq!(
        s.scene.transform(lid).scale,
        Vec3::splat(3.0),
        "override kept"
    );
    assert!(s.scene_dirty, "the file holds the old copy until saved");
}

#[test]
fn unpack_completely_unlinks_nested_instances() {
    let mut s = state("unpack");
    let wheel = s.scene.spawn("Wheel", None);
    let wheel_rel = make_prefab(&mut s, wheel);
    let cart = s.scene.spawn("Cart", None);
    let inner = instantiate(&mut s, &wheel_rel, Some(cart)).unwrap();
    make_prefab(&mut s, cart);
    apply(&mut s, Action::UnpackPrefab(cart));
    assert_eq!(s.scene.prefab_link(cart), None);
    assert_eq!(
        s.scene.prefab_link(inner).as_deref(),
        Some(wheel_rel.as_str())
    );
    s.undo_step();
    apply(&mut s, Action::UnpackPrefabCompletely(cart));
    assert_eq!(s.scene.prefab_link(cart), None);
    assert_eq!(s.scene.prefab_link(inner), None);
}

#[test]
fn missing_prefabs_report_no_overrides_and_can_be_unpacked() {
    let mut s = state("missing");
    let e = s.scene.spawn("Ghost", None);
    s.scene.link_prefab(e, "Assets/Gone.prefab");
    assert!(overrides_of(&mut s, e).is_none());
    apply(&mut s, Action::RevertPrefab(e));
    assert!(s.scene.contains(e), "revert leaves it alone");
    apply(&mut s, Action::UnpackPrefab(e));
    assert_eq!(s.scene.prefab_link(e), None);
}

#[test]
fn select_shows_the_asset() {
    let mut s = state("select");
    let e = s.scene.spawn("Thing", None);
    let sub = folder(&s).join("Prefabs");
    std::fs::create_dir_all(&sub).unwrap();
    apply(
        &mut s,
        Action::CreatePrefab {
            entity: e,
            folder: sub.clone(),
        },
    );
    apply(&mut s, Action::SelectPrefabAsset(e));
    assert_eq!(s.project.current, sub);
    assert_eq!(s.project.selected, Some(sub.join("Thing.prefab")));
    assert_eq!(
        s.inspector_asset.as_deref(),
        Some("Assets/Prefabs/Thing.prefab")
    );
    assert!(s.selection.is_empty());
}

#[test]
fn prefab_mode_edits_saves_and_returns_to_the_scene() {
    let mut s = state("stage");
    let crate_e = s.scene.spawn("Crate", None);
    s.scene.spawn("Lid", Some(crate_e));
    let rel = make_prefab(&mut s, crate_e);
    let other = instantiate(&mut s, &rel, None).unwrap();
    s.scene.add(other, Light::default());
    s.selection.set(other);
    let scene_objects = s.scene.len();

    assert!(crate::prefab_mode::open(&mut s, &rel, Some(other)));
    assert!(crate::prefab_mode::active(&s));
    assert_eq!(s.scene.len(), 2, "the stage holds the prefab alone");
    let root = s.selection.active().unwrap();
    assert_eq!(s.scene.name(root), "Crate");
    // Edit in the stage: a collider on the root.
    s.record("Add Archetype");
    s.scene.add(root, BoxCollider::default());
    assert!(s.scene_dirty);
    // Ctrl+S saves the prefab, not the scene.
    crate::commands::apply(&mut s, Action::SaveScene);
    assert!(!s.scene_dirty);
    assert!(
        s.assets
            .prefab(&rel)
            .unwrap()
            .root_components()
            .contains(&"Box Collider".to_owned())
    );

    // Leaving with unsaved changes asks first.
    s.record("Rename");
    s.scene.game_object_mut(root).name = "Box".into();
    apply(&mut s, Action::ClosePrefab { levels: 1 });
    assert!(crate::prefab_mode::active(&s), "waiting for an answer");
    apply(&mut s, Action::PrefabModeAnswer(false));
    assert!(!crate::prefab_mode::active(&s));

    // Back in the scene: both instances have the collider, the other keeps its light.
    assert_eq!(s.scene.len(), scene_objects);
    assert_eq!(s.selection.active(), Some(other), "selection restored");
    for e in s.scene.prefab_instances(&rel) {
        assert!(s.scene.world.get::<&BoxCollider>(e).is_ok());
    }
    assert!(s.scene.world.get::<&Light>(other).is_ok());
    assert_eq!(
        s.assets.prefab(&rel).unwrap().outline()[0].0,
        "Crate",
        "rename discarded"
    );
}

#[test]
fn prefab_mode_opens_nested_levels_and_variants_show_base_overrides() {
    let mut s = state("stage_nested");
    let wheel = s.scene.spawn("Wheel", None);
    let wheel_rel = make_prefab(&mut s, wheel);
    let cart = s.scene.spawn("Cart", None);
    instantiate(&mut s, &wheel_rel, Some(cart)).unwrap();
    let cart_rel = make_prefab(&mut s, cart);
    let variant = create_variant(&mut s, &cart_rel).unwrap();

    assert!(crate::prefab_mode::open(&mut s, &variant, None));
    let root = s.selection.active().unwrap();
    assert_eq!(
        s.scene.prefab_link(root).as_deref(),
        Some(cart_rel.as_str()),
        "a variant's root is an instance of its base"
    );
    // An override of the base, saved into the variant.
    s.record("Add Light");
    s.scene.add(root, Light::default());
    assert_eq!(keys(&mut s, root).len(), 1);
    assert!(crate::prefab_mode::save(&mut s));
    // One level deeper: the wheel inside.
    assert!(crate::prefab_mode::open(&mut s, &wheel_rel, None));
    assert_eq!(s.prefab_stages.len(), 2);
    assert!(
        !crate::prefab_mode::open(&mut s, &wheel_rel, None),
        "already open"
    );
    apply(&mut s, Action::ClosePrefab { levels: usize::MAX });
    assert!(!crate::prefab_mode::active(&s));
    let vp = s.assets.prefab(&variant).unwrap();
    assert_eq!(vp.base(), Some(cart_rel.as_str()));
    assert!(vp.root_components().contains(&"Light".to_owned()));
}

/// Deleting a prefab that is open in Prefab Mode closes it: Save used to write the deleted
/// prefab back as a new file.
#[test]
fn deleting_a_prefab_open_in_prefab_mode_closes_it() {
    let mut s = state("delete_open");
    let cube = s.scene.spawn("Cube", None);
    let rel = make_prefab(&mut s, cube);
    apply(
        &mut s,
        Action::OpenPrefab {
            path: rel.clone(),
            from: None,
        },
    );
    assert!(crate::prefab_mode::active(&s));
    let abs = s.assets.absolute(&rel);
    crate::commands::apply(&mut s, Action::DeleteAsset(abs.clone()));
    assert!(!abs.exists());
    assert!(!crate::prefab_mode::active(&s), "Prefab Mode closed");
    assert!(s.scene.contains(cube), "back in the scene");
    apply(&mut s, Action::SavePrefabStage);
    assert!(!abs.exists(), "not written back");
}

#[test]
fn prefab_mode_auto_save_writes_each_change() {
    let mut s = state("autosave");
    let e = s.scene.spawn("Lamp", None);
    let rel = make_prefab(&mut s, e);
    assert!(crate::prefab_mode::open(&mut s, &rel, None));
    s.prefab_stages.last_mut().unwrap().auto_save = true;
    let root = s.selection.active().unwrap();
    s.record("Add Light");
    s.scene.add(root, Light::default());
    // Leaving saves instead of asking.
    apply(&mut s, Action::ClosePrefab { levels: 1 });
    assert!(!crate::prefab_mode::active(&s));
    assert!(
        s.scene.world.get::<&Light>(e).is_ok(),
        "the instance followed"
    );
}

/// Renaming the prefab being edited while Prefab Mode is open: the stage, the stashed scene
/// and its undo history follow the new path, so saving doesn't recreate the old file and
/// leaving returns a scene whose instance links to the new file.
#[test]
fn renaming_the_prefab_in_prefab_mode_follows_the_stage_and_the_stashed_scene() {
    let mut s = state("rename_stage");
    let e = s.scene.spawn("Lamp", None);
    let rel = make_prefab(&mut s, e);
    assert!(crate::prefab_mode::open(&mut s, &rel, None));
    let from = s.project.root.join(&rel);
    crate::commands::apply(
        &mut s,
        Action::RenameAsset {
            path: from.clone(),
            name: "Torch".into(),
        },
    );
    let new_rel = rel.replace("Lamp.prefab", "Torch.prefab");
    assert_ne!(new_rel, rel);
    assert!(!from.exists());
    assert_eq!(
        crate::prefab_mode::open_path(&s).as_deref(),
        Some(new_rel.as_str())
    );
    let root = s.selection.active().unwrap();
    s.record("Add Light");
    s.scene.add(root, Light::default());
    assert!(crate::prefab_mode::save(&mut s));
    assert!(!from.exists(), "the old file is not written again");
    apply(&mut s, Action::ClosePrefab { levels: usize::MAX });
    assert!(!crate::prefab_mode::active(&s));
    assert_eq!(s.scene.prefab_link(e).as_deref(), Some(new_rel.as_str()));
    assert!(
        s.scene.world.get::<&Light>(e).is_ok(),
        "the instance followed"
    );
}

/// A prefab with a component this version doesn't know (a newer file, a removed package):
/// the warning reaches the Console and the prefab is not written back, which would delete
/// that component from the file (Prefab Mode save, Apply, updates from nested prefabs).
#[test]
fn prefabs_with_unknown_components_are_not_written_back() {
    let mut s = state("unknown_comp");
    let e = s.scene.spawn("Lamp", None);
    let rel = make_prefab(&mut s, e);
    let abs = s.project.root.join(&rel);
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&abs).unwrap()).unwrap();
    json["objects"][0]["components"]["Teleporter"] = serde_json::json!({ "range": 5 });
    std::fs::write(&abs, serde_json::to_string_pretty(&json).unwrap()).unwrap();
    s.assets.forget_prefabs();
    s.assets.messages.clear();
    assert!(crate::prefab_mode::open(&mut s, &rel, None));
    assert!(
        s.assets
            .messages
            .iter()
            .any(|m| m.contains("unknown component 'Teleporter'")),
        "{:?}",
        s.assets.messages
    );
    let root = s.selection.active().unwrap();
    s.record("Add Light");
    s.scene.add(root, Light::default());
    assert!(!crate::prefab_mode::save(&mut s), "refused");
    assert!(
        std::fs::read_to_string(&abs)
            .unwrap()
            .contains("Teleporter"),
        "the file keeps the component"
    );
}
