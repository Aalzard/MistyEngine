use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use glam::Vec3;
use misty_engine::navigation::{NavMeshModifierVolume, NavMeshSurface};
use misty_engine::scene::{MeshFilter, MeshRenderer, PrimitiveMesh, Transform};

use super::*;
use crate::actions::Action;
use crate::layout::Tab;
use crate::project::ProjectBrowser;

fn state(name: &str) -> EditorState {
    let root = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Scenes")).unwrap();
    EditorState::new(ProjectBrowser::open(root))
}

/// A saved scene with a static 10 m floor.
fn floor_state(name: &str) -> EditorState {
    let mut s = state(name);
    let f = s.scene.spawn("Floor", None);
    let mut t = Transform::from_position(Vec3::new(0.0, -0.5, 0.0));
    t.scale = Vec3::new(10.0, 1.0, 10.0);
    s.scene.set_transform(f, t);
    s.scene.add(f, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.scene.add(f, MeshRenderer::default());
    s.scene.game_object_mut(f).is_static = true;
    s.scene.set_scene_path(0, "Assets/Scenes/Level.scene");
    s
}

#[test]
fn window_menu_opens_the_navigation_window() {
    let mut h = Harness::builder().with_size([900.0, 600.0]).build_ui_state(
        |ui, s: &mut EditorState| crate::menu_bar::show(ui, s),
        state("misty_navigation_menu_test"),
    );
    h.run();
    h.get_by_label("Window").click();
    h.run();
    h.get_by_label("AI ⏵").hover();
    h.run();
    h.get_by_label("Navigation").click();
    h.run();
    let actions = std::mem::take(&mut h.state_mut().actions);
    assert!(
        matches!(actions.as_slice(), [Action::FocusTab(Tab::Navigation)]),
        "{actions:?}"
    );
}

#[test]
fn bake_button_saves_the_nav_mesh_next_to_the_scene() {
    let mut h = Harness::builder().with_size([560.0, 700.0]).build_ui_state(
        |ui, s: &mut EditorState| show(ui, s),
        floor_state("misty_navigation_bake_test"),
    );
    h.run();
    h.get_by_label("Agent Radius");
    h.get_all_by_label("Bake").next().unwrap().click();
    h.run();
    h.get_by_label("Not baked yet.");
    h.get_all_by_label("Bake").last().unwrap().click();
    // The window repaints while baking.
    h.step();
    finish_bake(h.state_mut());
    h.run();
    let rel = "Assets/Scenes/Level/NavMesh.navmesh";
    {
        let s = h.state();
        assert_eq!(s.scene.navigation.nav_mesh.as_deref(), Some(rel));
        assert!(s.project.root.join(rel).is_file());
        assert!(s.scene_dirty);
        assert!(
            s.navigation.status.starts_with("Baked"),
            "{}",
            s.navigation.status
        );
        let mesh = current_mesh(s).unwrap();
        assert!((mesh.area() - 81.0).abs() < 4.0, "{}", mesh.area());
        // The overlay draws while the window is open.
        let mut lines = crate::gizmos::Lines::default();
        let tris = crate::gizmos::nav_overlay(s, &h.ctx, &mut lines);
        assert!(!tris.is_empty() && !lines.0.is_empty());
    }
    // Baking again reuses every tile.
    h.get_all_by_label("Bake").last().unwrap().click();
    h.step();
    finish_bake(h.state_mut());
    h.run();
    let status = h.state().navigation.status.clone();
    let tiles = current_mesh(h.state()).unwrap().tiles().len();
    assert!(
        status.contains(&format!("{tiles} reused")),
        "{status} ({tiles} tiles)"
    );
    h.get_by_label("Clear").click();
    h.run();
    let s = h.state();
    assert!(s.scene.navigation.nav_mesh.is_none());
    assert!(!s.project.root.join(rel).exists());
}

#[test]
fn baking_needs_a_saved_scene() {
    let mut s = floor_state("misty_navigation_unsaved_test");
    s.scene.forget_scene_path(0);
    start_bake(&mut s);
    assert!(!s.navigation.baking());
    assert!(s.navigation.status.contains("Save the scene"));
}

#[test]
fn agents_tab_adds_and_edits_agent_types() {
    let mut h = Harness::builder().with_size([640.0, 600.0]).build_ui_state(
        |ui, s: &mut EditorState| show(ui, s),
        state("misty_navigation_agents_test"),
    );
    h.run();
    // In the list and as the heading.
    assert_eq!(h.get_all_by_label("Humanoid").count(), 2);
    h.get_by_label("+").click();
    h.run();
    {
        let s = h.state();
        let types = s.scene.navigation.all_agent_types();
        assert_eq!(types.len(), 2, "{types:?}");
        assert_eq!(types[1].name, "New Agent");
        assert_eq!(s.navigation.selected_agent, types[1].id);
    }
    // The new type is selected: it has a name field (the Humanoid doesn't).
    h.get_by_label("Name");
    h.get_by_label("Agent Radius");
    h.get_by_label("−").click();
    h.run();
    assert!(h.state().scene.navigation.agent_types.is_empty());
    assert!(h.state().scene_dirty);
    // The edits are undoable (quick successive edits of the settings merge into one step).
    assert!(h.state_mut().undo_step().is_some());
    assert!(h.state().scene.navigation.agent_types.len() <= 1);
}

#[test]
fn areas_tab_names_areas_and_sets_costs() {
    let mut h = Harness::builder().with_size([640.0, 900.0]).build_ui_state(
        |ui, s: &mut EditorState| show(ui, s),
        state("misty_navigation_areas_test"),
    );
    h.run();
    h.get_by_label("Areas").click();
    h.run();
    h.get_by_label("Walkable");
    h.get_by_label("Not Walkable");
    h.get_by_label("Jump");
    h.get_by_label("User 3");
    h.get_by_label("Built-in 0");
    // Type a name into the first user area.
    let fields: Vec<_> = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .collect();
    assert_eq!(fields.len(), 29, "29 user areas");
    fields[0].click();
    h.run();
    h.get_all_by_role(egui::accesskit::Role::TextInput)
        .next()
        .unwrap()
        .type_text("Mud");
    h.run();
    let s = h.state();
    assert_eq!(s.scene.navigation.area(3).name, "Mud");
    assert_eq!(s.scene.navigation.area_from_name("Mud"), Some(3));
    assert!(s.scene_dirty);
}

#[test]
fn surface_inspector_bakes_its_own_nav_mesh() {
    let mut st = floor_state("misty_navigation_surface_test");
    // Not static: surfaces don't need the flag.
    let floor = st.scene.find_by_name("Floor").unwrap();
    st.scene.game_object_mut(floor).is_static = false;
    let e = st.scene.spawn("Level Surface", None);
    st.scene.add(e, NavMeshSurface::default());
    let v = st.scene.spawn("Mud", None);
    st.scene
        .set_transform(v, Transform::from_position(Vec3::new(2.0, 0.0, 0.0)));
    st.scene.add(
        v,
        NavMeshModifierVolume {
            size: Vec3::new(2.0, 2.0, 2.0),
            area: 3,
            ..Default::default()
        },
    );
    st.select(e);
    let mut h = Harness::builder().with_size([520.0, 900.0]).build_ui_state(
        |ui, s: &mut EditorState| crate::panels::inspector::show(ui, s),
        st,
    );
    h.run();
    h.get_by_label("Collect Objects");
    h.get_by_label("Override Voxel Size");
    h.get_by_label("Bake").click();
    h.step();
    finish_bake(h.state_mut());
    h.run();
    let s = h.state();
    let rel = "Assets/Scenes/Level/NavMesh-Level Surface.navmesh";
    let data = s
        .scene
        .world
        .get::<&NavMeshSurface>(e)
        .unwrap()
        .nav_mesh_data
        .clone();
    assert_eq!(data.as_deref(), Some(rel));
    let meshes = surface_meshes(s);
    assert_eq!(meshes.len(), 1);
    let m = &meshes[0].1;
    assert!((m.area() - 81.0).abs() < 4.0, "{}", m.area());
    // The modifier volume made a patch of area 3.
    assert!(m.polys().any(|(_, p)| p.area == 3));
    assert!(
        surface_status(s, e).is_some_and(|t| t.starts_with("Baked")),
        "{:?}",
        surface_status(s, e)
    );
    // Selecting the surface shows the overlay.
    let mut lines = crate::gizmos::Lines::default();
    let tris = crate::gizmos::nav_overlay(s, &h.ctx, &mut lines);
    assert!(!tris.is_empty());
}
