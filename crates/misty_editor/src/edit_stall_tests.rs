//! Selecting an object and moving it must not stall the main thread.
//!
//! The collider Inspector's Material slot used to list the project's Physics Materials by
//! walking `Assets/` on the main thread, again every two seconds: 100 ms for the sample
//! project's 1800 files on an SSD, and up to a minute on a busy hard disk, where the editor
//! showed "Not Responding" whenever an object with a collider was selected. The test times the
//! frames of a selection and a move in a scene with a terrain, a plane and a project full of
//! files (`cargo test --release -p misty_editor edit_stall -- --nocapture`).

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use misty_engine::asset_db::{Guid, GuidDatabase};
use misty_engine::hecs::Entity;
use misty_engine::scene::PrimitiveMesh;

use crate::actions::{Action, Create};
use crate::project::ProjectBrowser;
use crate::state::EditorState;

/// Folders and files per folder of the test project (3000 files, more than the sample project).
const FOLDERS: usize = 60;
const FILES: usize = 50;

/// Main-thread time allowed for the selection frame plus the move frame. They take 1-3 ms in a
/// release build; walking the 3000 files took 70 ms or more.
const BUDGET: Duration = Duration::from_millis(if cfg!(debug_assertions) { 60 } else { 25 });

/// A project with many asset files and its GUID database (as the editor keeps it), a scene
/// with a terrain, a plane (Mesh Collider) and a cube (Box Collider).
fn setup() -> (EditorState, GuidDatabase, Entity, Entity) {
    let root = std::env::temp_dir().join(format!("misty_edit_stall_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut db = GuidDatabase::new(&root);
    for d in 0..FOLDERS {
        let dir = root.join(format!("Assets/Bulk/Folder {d}"));
        std::fs::create_dir_all(&dir).unwrap();
        for f in 0..FILES {
            std::fs::write(dir.join(format!("File {f}.txt")), b"").unwrap();
            db.insert(&format!("Assets/Bulk/Folder {d}/File {f}.txt"), Guid::new());
        }
    }
    std::fs::write(root.join("Assets/Ice.physicmat"), b"{}").unwrap();
    db.insert("Assets/Ice.physicmat", Guid::new());
    let mut s = EditorState::new(ProjectBrowser::open(root));
    let create = |s: &mut EditorState, what| {
        crate::commands::apply(s, Action::Create { what, parent: None });
        s.selection.active().unwrap()
    };
    create(&mut s, Create::Terrain);
    let plane = create(&mut s, Create::Primitive(PrimitiveMesh::Plane));
    let cube = create(&mut s, Create::Primitive(PrimitiveMesh::Cube));
    (s, db, plane, cube)
}

#[test]
fn selecting_and_moving_an_object_stays_within_a_frame() {
    let (s, db, plane, cube) = setup();
    let root = s.project.root.clone();
    // Time spent drawing the Inspector (the panel that stalled), added up per frame.
    let spent = Rc::new(Cell::new(Duration::ZERO));
    let spent2 = spent.clone();
    let mut h = Harness::builder()
        .with_size([420.0, 1400.0])
        .build_ui_state(
            move |ui, s: &mut EditorState| {
                let t = Instant::now();
                crate::panels::inspector::show(ui, s);
                spent2.set(spent2.get() + t.elapsed());
            },
            s,
        );
    // Warm up both Inspectors (fonts, layout).
    for e in [plane, cube, plane] {
        h.state_mut().selection.set(e);
        h.run();
    }
    let mut best = Duration::MAX;
    let mut rounds = Vec::new();
    for round in 0..5 {
        // The editor's database of this project (other tests install theirs), and the state
        // the old code was in whenever its two-second list had expired.
        misty_engine::asset_db::install(db.clone());
        crate::panels::inspector::physics_material_editor::refresh_list();
        h.state_mut().selection.set(cube);
        h.step();
        spent.set(Duration::ZERO);
        let mut took = Duration::ZERO;
        // Select the plane: the frame that draws its Inspector.
        h.state_mut().selection.set(plane);
        h.step();
        // Move it, as a gizmo drag or a typed position does: one undo step, then the frame.
        let t = Instant::now();
        let s = h.state_mut();
        s.undo.begin_frame(true, &s.scene, &s.selection, || None);
        let mut tr = s.scene.transform(plane);
        tr.position.x += 0.5 + round as f32;
        s.scene.set_transform(plane, tr);
        s.undo.mark_edited("Move");
        s.scene_dirty = true;
        s.undo.end_frame(false);
        took += t.elapsed();
        h.step();
        took += spent.get();
        best = best.min(took);
        rounds.push(took);
    }
    eprintln!("select + move, main-thread time per round: {rounds:.2?} (budget {BUDGET:?})");
    assert!(h.state().undo.can_undo());
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        best <= BUDGET,
        "selecting and moving an object took {best:.2?} of main-thread time at best \
         (budget {BUDGET:?}); rounds: {rounds:.2?}"
    );
}

#[test]
fn the_collider_material_list_comes_from_the_asset_database() {
    // No project folder at all: the database is the only place the material can come from.
    let root = std::env::temp_dir().join(format!("misty_edit_stall_db_{}", std::process::id()));
    let mut db = GuidDatabase::new(&root);
    db.insert("Assets/Physics/Rubber r40.physicmat", Guid::new());
    db.insert("Assets/Physics/Rubber r40.mat", Guid::new());
    // Other tests install their own databases: try again when one got in between.
    let listed = (0..50).any(|_| {
        misty_engine::asset_db::install(db.clone());
        let list = crate::panels::inspector::physics_material_editor::project_materials();
        list == ["Assets/Physics/Rubber r40.physicmat"]
    });
    assert!(listed);
}
