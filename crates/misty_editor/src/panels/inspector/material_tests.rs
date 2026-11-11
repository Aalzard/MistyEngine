//! The material Properties view: square texture slots (drop, picker, clear, show in Asset
//! Files), rows that appear only when they apply, and the normal-map warning with its fix.

use std::path::PathBuf;

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::assets::Material;
use misty_engine::render::RenderingMode;
use misty_engine::sprites::{TextureImporter, TextureType};

use crate::panels::AssetPayload;
use crate::project::ProjectBrowser;
use crate::state::EditorState;

const MAT: &str = "Assets/Wall.mat";
const BRICK: &str = "Assets/Textures/Brick.png";
const BUMPS: &str = "Assets/Textures/Bumps.png";

/// A project with two textures and the material `m` shown in the Properties view.
fn project(tag: &str, m: Material) -> (Harness<'static, EditorState>, PathBuf) {
    let root = std::env::temp_dir().join(format!("misty_mat_ui_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Textures")).unwrap();
    for (rel, rgb) in [(BRICK, [180, 70, 50]), (BUMPS, [128, 128, 255])] {
        misty_engine::image::RgbaImage::from_pixel(
            8,
            8,
            misty_engine::image::Rgba([rgb[0], rgb[1], rgb[2], 255]),
        )
        .save(root.join(rel))
        .unwrap();
    }
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    // Set by hand: the scan resolves paths through the process-wide project root, which tests
    // running alongside move.
    s.assets.textures = vec![BRICK.to_owned(), BUMPS.to_owned()];
    s.assets.update_material(MAT, m).unwrap();
    s.selection.clear();
    s.inspector_asset = Some(MAT.to_owned());
    let mut h = Harness::builder()
        .with_size([420.0, 1100.0])
        .build_ui_state(|ui, s: &mut EditorState| super::show(ui, s), s);
    h.run();
    (h, root)
}

fn material(h: &Harness<'static, EditorState>) -> Material {
    h.state().assets.materials[MAT].clone()
}

fn has(h: &Harness<'static, EditorState>, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

#[test]
fn a_texture_dropped_on_a_slot_is_assigned() {
    let (mut h, root) = project("drop", Material::default());
    let slot = h.get_by_label("Albedo texture").rect().center();
    // A drag that started elsewhere (Asset Files) and ends over the slot.
    h.hover_at(egui::pos2(400.0, 1080.0));
    h.drag_at(egui::pos2(400.0, 1080.0));
    h.run();
    egui::DragAndDrop::set_payload(&h.ctx, AssetPayload(BRICK.to_owned()));
    h.hover_at(slot);
    h.run();
    assert_eq!(material(&h).albedo_map, None, "nothing until the drop");
    h.event(egui::Event::PointerButton {
        pos: slot,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert_eq!(material(&h).albedo_map.as_deref(), Some(BRICK));
    assert!(
        !has(&h, "None"),
        "a drop does not open the picker (its None row)"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_drop_on_the_name_beside_the_slot_counts_and_other_assets_are_ignored() {
    let (mut h, root) = project("drop_name", Material::default());
    let slot = h.get_by_label("Occlusion texture").rect();
    let on_name = egui::pos2(slot.right() + 30.0, slot.center().y);
    let release = |h: &mut Harness<'static, EditorState>, payload: &str| {
        h.hover_at(egui::pos2(400.0, 1080.0));
        h.drag_at(egui::pos2(400.0, 1080.0));
        h.run();
        egui::DragAndDrop::set_payload(&h.ctx, AssetPayload(payload.to_owned()));
        h.hover_at(on_name);
        h.run();
        h.event(egui::Event::PointerButton {
            pos: on_name,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    };
    release(&mut h, "Assets/Crate.prefab");
    assert_eq!(material(&h).occlusion_map, None, "only textures are taken");
    release(&mut h, BRICK);
    assert_eq!(material(&h).occlusion_map.as_deref(), Some(BRICK));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn right_click_clears_a_slot_as_one_undo_step() {
    let m = Material {
        albedo_map: Some(BRICK.to_owned()),
        ..Default::default()
    };
    let (mut h, root) = project("clear", m);
    {
        // As the app does at the start of a frame with a mouse button down.
        let s = h.state_mut();
        s.undo.begin_frame(true, &s.scene, &s.selection, || None);
    }
    h.get_by_label("Albedo texture").click_secondary();
    h.run();
    assert_eq!(material(&h).albedo_map, None);
    h.state_mut().undo.end_frame(false);
    crate::commands::apply(h.state_mut(), crate::actions::Action::Undo);
    assert_eq!(
        material(&h).albedo_map.as_deref(),
        Some(BRICK),
        "undo restores the texture"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn the_picker_searches_assigns_and_clears() {
    let (mut h, root) = project("picker", Material::default());
    h.get_by_label("Albedo texture").click();
    h.run();
    h.get_by_label("None");
    h.get_by_label("Brick.png");
    h.get_by_label("Bumps.png");
    // The search field has focus: typing narrows the list.
    h.event(egui::Event::Text("bum".into()));
    h.run();
    assert!(!has(&h, "Brick.png"), "filtered out by the search");
    h.get_by_label("Bumps.png").click();
    h.run();
    assert_eq!(material(&h).albedo_map.as_deref(), Some(BUMPS));
    assert!(!has(&h, "None"), "the picker closes after choosing");

    // Open it again (the search starts empty) and choose None.
    h.get_by_label("Albedo texture").click();
    h.run();
    h.get_by_label("Brick.png");
    h.get_by_label("None").click();
    h.run();
    assert_eq!(material(&h).albedo_map, None);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn ctrl_click_shows_the_texture_in_asset_files() {
    let m = Material {
        albedo_map: Some(BRICK.to_owned()),
        ..Default::default()
    };
    let (mut h, root) = project("show", m);
    h.get_by_label("Albedo texture")
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    let s = h.state();
    assert_eq!(s.project.selected, Some(s.assets.absolute(BRICK)));
    assert_eq!(s.project.current, root.join("Assets/Textures"));
    assert_eq!(
        s.inspector_asset.as_deref(),
        Some(MAT),
        "the Properties view stays on the material"
    );
    assert!(!has(&h, "None"), "the picker stays closed");
    assert_eq!(material(&h).albedo_map.as_deref(), Some(BRICK));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn the_cutoff_slider_shows_only_for_cutout() {
    let (mut h, root) = project("cutout", Material::default());
    h.get_by_label("Main Maps");
    assert!(!has(&h, "Alpha Cutoff"));
    let mut m = material(&h);
    m.rendering_mode = RenderingMode::Cutout;
    h.state_mut().assets.update_material(MAT, m).unwrap();
    h.run();
    h.get_by_label("Alpha Cutoff");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn emission_opens_its_colour_and_texture() {
    let (mut h, root) = project("emission", Material::default());
    assert!(!has(&h, "Intensity"));
    assert!(!has(&h, "Color texture"));
    h.get_by_role_and_label(Role::CheckBox, "Emission").click();
    h.run();
    assert!(material(&h).emission_enabled);
    h.get_by_label("Intensity");
    h.get_by_label("Color texture");
    h.get_by_role_and_label(Role::CheckBox, "Emission").click();
    h.run();
    assert!(!material(&h).emission_enabled);
    assert!(!has(&h, "Intensity"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn strength_fields_show_once_a_map_is_set() {
    let (mut h, root) = project("strength", Material::default());
    assert!(!has(&h, "Steps"), "no parallax rows without a height map");
    let mut m = material(&h);
    m.features.parallax.height_map = Some(BRICK.to_owned());
    m.features.parallax.enabled = true;
    h.state_mut().assets.update_material(MAT, m).unwrap();
    h.run();
    h.get_by_label("Steps");
    assert!(!has(&h, "Enabled"), "on with its map: no Enabled row");
    // Clearing the height map switches parallax off.
    h.get_by_label("Height Map texture").click_secondary();
    h.run();
    let p = material(&h).features.parallax;
    assert_eq!((p.height_map, p.enabled), (None, false));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_detail_map_from_the_picker_turns_the_detail_maps_on() {
    let (mut h, root) = project("detail", Material::default());
    h.get_by_label("Secondary Maps");
    h.get_by_label("Detail Albedo x2 texture").click();
    h.run();
    h.get_by_label("Brick.png").click();
    h.run();
    let d = material(&h).features.detail;
    assert_eq!(d.albedo_map.as_deref(), Some(BRICK));
    assert!(d.enabled);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_normal_map_slot_warns_and_fix_now_sets_the_import_type() {
    let (mut h, root) = project("normal", Material::default());
    assert!(!has(&h, "Fix Now"));
    let normal = h
        .get_all_by_label("Normal Map texture")
        .next()
        .expect("the Main Maps normal slot");
    normal.click();
    h.run();
    h.get_by_label("Bumps.png").click();
    h.run();
    assert_eq!(material(&h).normal_map.as_deref(), Some(BUMPS));
    let abs = h.state().assets.absolute(BUMPS);
    assert_eq!(
        TextureImporter::load(&abs).texture_type,
        TextureType::Default
    );
    h.get_by_label("⚠ Not a normal map");
    h.get_by_label("Fix Now").click();
    h.run();
    assert_eq!(
        TextureImporter::load(&abs).texture_type,
        TextureType::NormalMap
    );
    assert!(!has(&h, "Fix Now"), "the warning goes once it is fixed");
    assert_eq!(
        material(&h).normal_map.as_deref(),
        Some(BUMPS),
        "the material keeps its map"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn advanced_surface_features_open_under_their_checkbox() {
    let (mut h, root) = project("surface", Material::default());
    h.get_by_label("Advanced Surface");
    h.get_by_label("Rendering Options");
    h.get_by_label("Advanced Options");
    assert!(!has(&h, "Wrap"));
    h.get_by_role_and_label(Role::CheckBox, "Translucency")
        .click();
    h.run();
    assert!(material(&h).features.translucency.enabled);
    h.get_by_label("Wrap");
    h.get_by_label("Thickness texture");
    std::fs::remove_dir_all(root).ok();
}
