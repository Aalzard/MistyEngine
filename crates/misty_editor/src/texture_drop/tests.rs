//! A texture dropped on an object: the material that is made or reused, the assignment, undo,
//! normal maps, the Scene Tree, objects without a renderer and prefab instances.
//!
//! Every test works in its own temp project.

use std::path::{Path, PathBuf};

use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::assets::Material;
use misty_engine::glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::scene::prefab::diff::Override;
use misty_engine::scene::{MeshFilter, MeshRenderer, PrimitiveMesh, SpriteRenderer};
use misty_engine::sprites::{TextureImporter, TextureType};

use super::*;
use crate::actions::Action;
use crate::panels::AssetPayload;
use crate::project::ProjectBrowser;

const ROCK: &str = "Assets/Textures/Rock.png";
const ROCK_MAT: &str = "Assets/Materials/Rock.mat";

fn temp(name: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("misty_texture_drop_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    root
}

/// A small image file at `rel` in the project at `root`.
fn texture(root: &Path, rel: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    misty_engine::image::RgbaImage::from_pixel(
        4,
        4,
        misty_engine::image::Rgba([200, 120, 60, 255]),
    )
    .save(path)
    .unwrap();
}

fn cube(s: &mut EditorState, name: &str) -> Entity {
    let e = s.scene.spawn(name, None);
    s.scene.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.scene.add(e, MeshRenderer::default());
    e
}

/// A project with Rock.png and a "Crate" cube at the origin.
fn state(name: &str) -> (EditorState, Entity) {
    let root = temp(name);
    texture(&root, ROCK);
    let mut s = EditorState::new(ProjectBrowser::open(root));
    let e = cube(&mut s, "Crate");
    (s, e)
}

fn material_of_cube(s: &EditorState, e: Entity) -> Option<String> {
    s.scene
        .world
        .get::<&MeshRenderer>(e)
        .unwrap()
        .material
        .clone()
}

fn on_disk(s: &EditorState, rel: &str) -> Material {
    Material::load(&s.project.root.join(rel)).unwrap()
}

fn materials_folder(s: &EditorState) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(s.project.root.join(FOLDER))
        .into_iter()
        .flatten()
        .flatten()
        .map(|f| f.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".mat"))
        .collect();
    names.sort();
    names
}

/// Let go of asset `rel` (dragged from Asset Files) at `at`.
fn drop_at(h: &mut Harness<'_, EditorState>, at: Pos2, rel: &str) {
    h.event(Event::PointerMoved(at));
    h.run_steps(2);
    egui::DragAndDrop::set_payload(&h.ctx, AssetPayload(rel.to_owned()));
    h.run_steps(2);
    h.event(Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.run_steps(2);
}

#[test]
fn a_drop_on_a_cube_in_the_editor_scene_makes_the_material_and_assigns_it_in_one_undo_step() {
    let (s, e) = state("scene");
    let mut h = Harness::builder().with_size([800.0, 600.0]).build_ui_state(
        |ui, s: &mut EditorState| crate::panels::scene::show(ui, s, None),
        s,
    );
    h.state_mut().scene_view.camera.align_to(
        Vec3::new(1.6, 2.2, -3.0),
        misty_engine::camera::euler_to_quat(Vec3::new(30.0, -25.0, 0.0)),
    );
    h.run_steps(2);
    let cam = h.state().scene_view.camera.camera();
    let rect = h.state().scene_view.view_rect;
    let on_cube = crate::picking::to_screen(&cam, rect, Vec3::ZERO).unwrap();

    // Empty space: nothing is made.
    let steps = h.state().undo.steps().len();
    drop_at(&mut h, rect.left_bottom() + egui::vec2(12.0, -12.0), ROCK);
    assert_eq!(material_of_cube(h.state(), e), None);
    assert!(!h.state().project.root.join(ROCK_MAT).exists());

    drop_at(&mut h, on_cube, ROCK);
    let s = h.state_mut();
    assert_eq!(material_of_cube(s, e).as_deref(), Some(ROCK_MAT));
    let made = on_disk(s, ROCK_MAT);
    let expected = Material {
        albedo_map: Some(ROCK.into()),
        ..Material::default()
    };
    assert_eq!(
        made, expected,
        "default lit settings, the texture as albedo"
    );
    assert_eq!(s.assets.materials.get(ROCK_MAT), Some(&expected));
    assert_eq!(s.live.current_toast(), Some("Created material Rock.mat"));
    // Shown in Asset Files; the scene selection is left alone.
    assert_eq!(s.project.selected, Some(s.project.root.join(ROCK_MAT)));
    assert_eq!(s.project.current, s.project.root.join(FOLDER));

    // One step; undo takes the assignment back and leaves the file.
    assert_eq!(s.undo.steps().len(), steps + 1);
    assert_eq!(s.undo_step().as_deref(), Some("Assign Material"));
    assert_eq!(material_of_cube(s, e), None);
    assert!(s.project.root.join(ROCK_MAT).exists());
    assert_eq!(s.redo_step().as_deref(), Some("Assign Material"));
    assert_eq!(material_of_cube(s, e).as_deref(), Some(ROCK_MAT));
}

#[test]
fn a_drop_on_a_scene_tree_row_does_the_same() {
    let (s, e) = state("tree");
    let mut h = Harness::builder().with_size([400.0, 500.0]).build_ui_state(
        |ui, s: &mut EditorState| crate::panels::hierarchy::show(ui, s),
        s,
    );
    h.run_steps(3);
    let row = h.get_by_label("Crate").rect().center();
    drop_at(&mut h, row, ROCK);
    let s = h.state_mut();
    assert_eq!(material_of_cube(s, e).as_deref(), Some(ROCK_MAT));
    assert_eq!(on_disk(s, ROCK_MAT).albedo_map.as_deref(), Some(ROCK));
    assert_eq!(s.undo.undo_label(), Some("Assign Material"));
    assert_eq!(s.live.current_toast(), Some("Created material Rock.mat"));
}

#[test]
fn the_same_texture_again_reuses_its_material() {
    let (mut s, e) = state("reuse");
    let other = cube(&mut s, "Barrel");
    assert_eq!(
        drop_on_object(&mut s, e, ROCK),
        Outcome::Created(ROCK_MAT.into())
    );
    assert_eq!(
        drop_on_object(&mut s, other, ROCK),
        Outcome::Reused(ROCK_MAT.into())
    );
    assert_eq!(material_of_cube(&s, other).as_deref(), Some(ROCK_MAT));
    assert_eq!(materials_folder(&s), ["Rock.mat"]);
    assert_eq!(s.live.current_toast(), Some("Assigned material Rock.mat"));
    assert_eq!(s.undo.steps(), ["Assign Material", "Assign Material"]);
    // On the object that has it already: nothing to undo.
    assert_eq!(
        drop_on_object(&mut s, e, ROCK),
        Outcome::Reused(ROCK_MAT.into())
    );
    assert_eq!(s.undo.steps().len(), 2);
}

#[test]
fn another_material_of_that_name_is_kept_and_a_numbered_one_is_made() {
    let (mut s, e) = state("collision");
    // Somebody's own Rock.mat: no albedo map.
    let folder = s.project.root.join(FOLDER);
    std::fs::create_dir_all(&folder).unwrap();
    let theirs = Material {
        color: [1.0, 0.0, 0.0, 1.0],
        ..Material::default()
    };
    theirs.save(&folder.join("Rock.mat")).unwrap();
    s.refresh_assets();

    let numbered = "Assets/Materials/Rock 1.mat";
    assert_eq!(
        drop_on_object(&mut s, e, ROCK),
        Outcome::Created(numbered.into())
    );
    assert_eq!(material_of_cube(&s, e).as_deref(), Some(numbered));
    assert_eq!(on_disk(&s, ROCK_MAT), theirs, "left as it was");
    assert_eq!(on_disk(&s, numbered).albedo_map.as_deref(), Some(ROCK));
    assert_eq!(s.live.current_toast(), Some("Created material Rock 1.mat"));
    // The numbered one is found the next time.
    let other = cube(&mut s, "Barrel");
    assert_eq!(
        drop_on_object(&mut s, other, ROCK),
        Outcome::Reused(numbered.into())
    );
    assert_eq!(materials_folder(&s), ["Rock 1.mat", "Rock.mat"]);

    // A texture of the same name from another folder is another texture.
    let moss = "Assets/Other/Rock.png";
    texture(&s.project.root, moss);
    let third = "Assets/Materials/Rock 2.mat";
    assert_eq!(
        drop_on_object(&mut s, other, moss),
        Outcome::Created(third.into())
    );
    assert_eq!(on_disk(&s, third).albedo_map.as_deref(), Some(moss));
}

#[test]
fn normal_maps_are_told_by_name_or_by_import_settings() {
    let (s, _) = state("detect");
    for (name, normal) in [
        ("Rock_Normal", true),
        ("Rock_normal", true),
        ("Rock_N", true),
        ("Rock_n", true),
        ("Rock_nrm", true),
        ("Rock-NormalMap", true),
        ("Rock Normals", true),
        ("Rock.nor", true),
        ("Rock", false),
        ("Rock_Albedo", false),
        ("Lantern", false),
        ("Cabin", false),
        ("N", false),
        ("_N", false),
    ] {
        let rel = format!("Assets/Textures/{name}.png");
        assert_eq!(is_normal_map(&s, &rel), normal, "{name}");
    }
    // Texture Type: Normal Map in the import settings, whatever the name.
    let bumps = "Assets/Textures/Bumps.png";
    texture(&s.project.root, bumps);
    assert!(!is_normal_map(&s, bumps));
    let settings = TextureImporter {
        texture_type: TextureType::NormalMap,
        ..TextureImporter::default()
    };
    settings.save(&s.project.root.join(bumps)).unwrap();
    assert!(is_normal_map(&s, bumps));
}

#[test]
fn a_normal_map_goes_into_the_objects_material() {
    let (mut s, e) = state("normal");
    let normal = "Assets/Textures/Rock_Normal.png";
    texture(&s.project.root, normal);
    drop_on_object(&mut s, e, ROCK);
    let steps = s.undo.steps().len();

    // The object's own material takes it: no new file, the albedo stays.
    assert_eq!(
        drop_on_object(&mut s, e, normal),
        Outcome::NormalMapSet(ROCK_MAT.into())
    );
    assert_eq!(materials_folder(&s), ["Rock.mat"]);
    let m = on_disk(&s, ROCK_MAT);
    assert_eq!(m.albedo_map.as_deref(), Some(ROCK));
    assert_eq!(m.normal_map.as_deref(), Some(normal));
    assert_eq!(material_of_cube(&s, e).as_deref(), Some(ROCK_MAT));
    assert_eq!(
        s.live.current_toast(),
        Some("Set the normal map of Rock.mat")
    );
    // One step, which puts the material back as it was (in memory and on disk).
    assert_eq!(s.undo.steps().len(), steps + 1);
    assert_eq!(s.undo_step().as_deref(), Some("Assign Normal Map"));
    assert_eq!(on_disk(&s, ROCK_MAT).normal_map, None);
    assert_eq!(s.assets.materials[ROCK_MAT].normal_map, None);
    assert_eq!(material_of_cube(&s, e).as_deref(), Some(ROCK_MAT));
    s.redo_step();
    assert_eq!(on_disk(&s, ROCK_MAT).normal_map.as_deref(), Some(normal));
    // Again: nothing more to undo.
    let steps = s.undo.steps().len();
    drop_on_object(&mut s, e, normal);
    assert_eq!(s.undo.steps().len(), steps);
}

#[test]
fn a_normal_map_on_default_material_makes_a_copy_named_after_the_texture() {
    let (mut s, e) = state("normal_copy");
    let normal = "Assets/Textures/Wall_N.png";
    texture(&s.project.root, normal);
    let wall = "Assets/Materials/Wall.mat";
    assert_eq!(
        drop_on_object(&mut s, e, normal),
        Outcome::Created(wall.into())
    );
    let expected = Material {
        normal_map: Some(normal.into()),
        ..Material::default()
    };
    assert_eq!(on_disk(&s, wall), expected);
    assert_eq!(material_of_cube(&s, e).as_deref(), Some(wall));
    assert_eq!(s.undo.steps(), ["Assign Material"]);
    // Another object on Default-Material gets the same copy.
    let other = cube(&mut s, "Barrel");
    assert_eq!(
        drop_on_object(&mut s, other, normal),
        Outcome::Reused(wall.into())
    );
    assert_eq!(materials_folder(&s), ["Wall.mat"]);
}

#[test]
fn an_object_without_a_renderer_gets_a_toast_and_no_change() {
    let (mut s, _) = state("no_renderer");
    let empty = s.scene.spawn("Spawn Point", None);
    let before = s.scene.clone();
    assert_eq!(drop_on_object(&mut s, empty, ROCK), Outcome::Nothing);
    assert_eq!(
        s.live.current_toast(),
        Some("Spawn Point has no renderer to put a texture on")
    );
    assert!(!s.undo.can_undo());
    assert!(!s.project.root.join(FOLDER).exists(), "nothing was made");
    assert_eq!(s.scene.len(), before.len());
    assert!(!s.scene_dirty);
}

#[test]
fn a_sprite_renderer_takes_the_texture_as_its_sprite() {
    let (mut s, _) = state("sprite");
    let e = s.scene.spawn("Player", None);
    s.scene.add(e, SpriteRenderer::default());
    assert_eq!(drop_on_object(&mut s, e, ROCK), Outcome::Sprite);
    let sprite = |s: &EditorState| {
        s.scene
            .world
            .get::<&SpriteRenderer>(e)
            .unwrap()
            .sprite
            .clone()
    };
    assert_eq!(sprite(&s).as_deref(), Some(ROCK));
    assert!(
        !s.project.root.join(FOLDER).exists(),
        "no material for sprites"
    );
    assert_eq!(s.undo_step().as_deref(), Some("Assign Sprite"));
    assert_eq!(sprite(&s), None);
}

#[test]
fn on_a_prefab_instance_the_material_is_an_override() {
    let (mut s, e) = state("prefab");
    let folder = s.project.assets_dir();
    crate::prefabs::apply(&mut s, Action::CreatePrefab { entity: e, folder });
    let prefab = s.scene.prefab_link(e).expect("linked");
    assert_eq!(crate::prefabs::overrides_of(&mut s, e).unwrap(), []);

    assert_eq!(
        drop_on_object(&mut s, e, ROCK),
        Outcome::Created(ROCK_MAT.into())
    );
    let overrides = crate::prefabs::overrides_of(&mut s, e).unwrap();
    assert_eq!(overrides.len(), 1, "{overrides:?}");
    assert!(
        matches!(
            &overrides[0].key,
            Override::Property { component, property, .. }
                if component == "Mesh Renderer" && property == "material"
        ),
        "{overrides:?}"
    );
    // The prefab asset itself is untouched.
    let asset = crate::prefabs::asset(&mut s, &prefab).unwrap();
    let mut fresh = misty_engine::scene::Scene::new("Fresh");
    let copy = fresh.instantiate_prefab(&asset, None, None);
    assert_eq!(
        fresh.world.get::<&MeshRenderer>(copy).unwrap().material,
        None
    );
}

/// The project's GUID database is one per process, and a few other tests install their own
/// for a moment. The drop is repeated in a fresh project until ours stayed open throughout.
#[test]
fn the_material_file_holds_the_texture_guid_and_gets_its_own_meta() {
    use misty_engine::asset_db::{self, meta};
    let texture_rel = "Assets/Textures/R41Granite.png";
    let material_rel = "Assets/Materials/R41Granite.mat";
    for attempt in 0..50 {
        let root = temp(&format!("guid_{attempt}"));
        texture(&root, texture_rel);
        let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
        let e = cube(&mut s, "Crate");
        asset_db::open(&root);
        let outcome = drop_on_object(&mut s, e, texture_rel);
        let text = std::fs::read_to_string(root.join(material_rel)).unwrap_or_default();
        let ours = asset_db::with(|db| db.root() == root).unwrap_or(false);
        if ours {
            asset_db::close();
        }
        let texture_guid = meta::read_guid(&root.join(texture_rel));
        let material_guid = meta::read_guid(&root.join(material_rel));
        let complete = ours
            && texture_guid.is_some_and(|g| text.contains(&g.to_string()))
            && material_guid.is_some();
        if !complete {
            continue;
        }
        assert_eq!(outcome, Outcome::Created(material_rel.into()));
        assert!(text.contains(texture_rel), "the path is there too");
        assert_ne!(texture_guid, material_guid);
        return;
    }
    panic!("the material never got the texture's GUID and a meta file");
}
