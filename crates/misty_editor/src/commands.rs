//! Scene-editing commands (create, delete, duplicate, parent, align, undo).

use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    BoxCollider, Camera, CapsuleCollider, ColliderKind, Light, LightType, MeshCollider, MeshFilter,
    MeshRenderer, ScriptRef, Scripts, SphereCollider, Transform,
};

use crate::actions::{Action, Create};
use crate::state::EditorState;

/// Apply a scene command. Returns the action back if it is not a scene command.
pub fn apply(s: &mut EditorState, action: Action) -> Option<Action> {
    match action {
        Action::Create {
            what: Create::Ui(el),
            parent,
        } => crate::ui_create::create(s, el, parent),
        Action::Create {
            what: Create::Terrain,
            parent,
        } => crate::terrain_tools::create(s, parent),
        Action::Create { what, parent } => create(s, what, parent),
        Action::CreateEmptyParent => create_empty_parent(s),
        // The open set of scenes (see `open_scenes`).
        Action::OpenSceneAdditive(rel) => {
            crate::open_scenes::open_additive(s, &rel);
        }
        Action::Scene(id, op) => crate::open_scenes::apply_op(s, id, op),
        Action::MoveToScene { objects, scene } => {
            crate::open_scenes::move_objects(s, &objects, scene)
        }
        Action::DeleteSelected => {
            let targets = s.selection.top_level(&s.scene);
            if !targets.is_empty() {
                s.record(&format!("Delete {}", s.selection_label()));
                for e in targets {
                    s.scene.despawn(e);
                }
                s.selection.clear();
            }
        }
        Action::DuplicateSelected => {
            let targets = s.selection.top_level(&s.scene);
            if !targets.is_empty() {
                s.record(&format!("Duplicate {}", s.selection_label()));
                let copies: Vec<Entity> =
                    targets.into_iter().map(|e| s.scene.duplicate(e)).collect();
                s.selection.set_many(copies);
            }
        }
        Action::RenameSelected => {
            if let Some(e) = s.selection.active() {
                s.hierarchy.renaming = Some((e, s.scene.name(e)));
            }
        }
        Action::SelectAll => {
            let all = s.scene.walk().into_iter().map(|(e, _)| e).collect();
            s.selection.set_many(all);
        }
        Action::DeselectAll => s.selection.clear(),
        Action::FrameSelected => {
            if let Some((center, radius)) = selection_bounds(s) {
                s.scene_view.camera.frame(center, radius);
            }
        }
        Action::ToggleActive => {
            let targets = s.selection.list().to_vec();
            if !targets.is_empty() {
                s.record(&format!("Toggle Active State of {}", s.selection_label()));
                for e in targets {
                    let mut go = s.scene.game_object_mut(e);
                    go.active = !go.active;
                }
            }
        }
        Action::ClearParent => {
            let targets = s.selection.top_level(&s.scene);
            if targets.iter().any(|e| s.scene.parent(*e).is_some()) {
                s.record(&format!("Clear Parent of {}", s.selection_label()));
                for e in targets {
                    s.scene.set_parent(e, None, None, true);
                }
            }
        }
        Action::SetSibling { first } => {
            if let Some(e) = s.selection.active() {
                s.record(&format!("Reorder {}", s.scene.name(e)));
                let parent = s.scene.parent(e);
                let index = if first { 0 } else { usize::MAX };
                s.scene.set_parent(e, parent, Some(index), false);
            }
        }
        Action::MoveToView => {
            let pivot = s.scene_view.camera.pivot();
            let targets = s.selection.top_level(&s.scene);
            if !targets.is_empty() {
                s.record("Move To View");
                for e in targets {
                    s.scene.set_world_position(e, pivot);
                }
            }
        }
        Action::OpenGridAndSnap => s.snap_window_open = true,
        Action::AlignToGrid => {
            let targets = s.selection.top_level(&s.scene);
            if !targets.is_empty() {
                s.record("Align To Grid");
                for e in targets {
                    let p = s.scene.world_position(e);
                    let snapped = crate::snap::snap_to_grid(p, s.snap.grid_size, [true; 3]);
                    s.scene.set_world_position(e, snapped);
                }
            }
        }
        Action::ToggleLockView => {
            let sv = &mut s.scene_view;
            sv.lock_to_selection = !sv.lock_to_selection;
        }
        Action::AlignWithView => {
            if let Some(e) = s.selection.active() {
                s.record("Align With View");
                let cam = s.scene_view.camera.camera();
                s.scene
                    .set_world_position(e, s.scene_view.camera.position());
                let parent_rot = s
                    .scene
                    .parent(e)
                    .map(|p| s.scene.world_matrix(p).to_scale_rotation_translation().1)
                    .unwrap_or_default();
                let mut t = s.scene.transform(e);
                t.set_rotation(parent_rot.inverse() * cam.rotation);
                s.scene.set_transform(e, t);
            }
        }
        Action::AlignViewToSelected => {
            if let Some(e) = s.selection.active() {
                let (_, rot, pos) = s.scene.world_matrix(e).to_scale_rotation_translation();
                s.scene_view.camera.align_to(pos, rot);
            }
        }
        Action::AddComponent(name) => {
            let targets = s.selection.list().to_vec();
            if !targets.is_empty() {
                s.record(&format!("Add {name}"));
                for e in targets {
                    if s.scene.add_by_name(e, name) {
                        crate::presets::apply_default(s, e, name);
                    } else {
                        s.console
                            .warn(format!("'{}' already has a {name}.", s.scene.name(e)));
                    }
                }
            }
        }
        a @ (Action::ApplyPreset { .. }
        | Action::RestoreComponentValues { .. }
        | Action::SavePreset { .. }) => {
            if let Some(other) = crate::presets::apply(s, a) {
                return Some(other);
            }
        }
        a @ (Action::RemoveComponent { .. }
        | Action::ResetComponent { .. }
        | Action::MoveComponent { .. }
        | Action::CopyComponent { .. }
        | Action::PasteComponentValues { .. }
        | Action::PasteComponentAsNew { .. }) => {
            if let Some(other) = crate::component_ops::apply(s, a) {
                return Some(other);
            }
        }
        Action::AddScript { entity, path } => {
            if s.scene.contains(entity) {
                s.record("Add Script");
                let mut scripts = s
                    .scene
                    .world
                    .get::<&Scripts>(entity)
                    .map(|x| (*x).clone())
                    .unwrap_or_default();
                scripts.list.push(ScriptRef {
                    path,
                    ..Default::default()
                });
                s.scene.add(entity, scripts);
            }
        }
        Action::RemoveScript { entity, slot } => {
            s.record("Remove Script");
            let empty = match s.scene.world.get::<&mut Scripts>(entity) {
                Ok(mut scripts) => {
                    if slot < scripts.list.len() {
                        scripts.list.remove(slot);
                    }
                    scripts.list.is_empty()
                }
                Err(_) => false,
            };
            if empty {
                s.scene.remove_by_name(entity, "Scripts");
            }
        }
        Action::ResetScript { entity, slot } => {
            s.record("Reset Script");
            if let Ok(mut scripts) = s.scene.world.get::<&mut Scripts>(entity)
                && let Some(r) = scripts.list.get_mut(slot)
            {
                r.fields.clear();
            }
        }
        Action::OpenAddComponent => s.inspector_add_open = true,
        Action::OpenProjectSettings => s.project_settings_window.open = true,
        Action::OpenBuildSettings => s.build_window.open = true,
        Action::Undo => {
            // Undone values are not new edits for Record mode.
            s.animation.skip_record = true;
            match s.undo_step() {
                Some(label) => s.console.info(format!("Undo {label}")),
                None => s.console.info("Nothing to undo."),
            }
        }
        Action::Redo => {
            s.animation.skip_record = true;
            match s.redo_step() {
                Some(label) => s.console.info(format!("Redo {label}")),
                None => s.console.info("Nothing to redo."),
            }
        }
        Action::UndoJump(position) => {
            s.animation.skip_record = true;
            s.undo_jump(position);
        }
        Action::ClearUndoHistory => {
            s.undo.clear();
            s.console.info("Undo history cleared.");
        }
        Action::InstantiateModel { path, parent } => {
            crate::model_import::instantiate(s, &path, parent);
        }
        a @ (Action::CreateAsset(_)
        | Action::RenameAsset { .. }
        | Action::MoveAsset { .. }
        | Action::DuplicateAsset(_)
        | Action::DeleteAsset(_)
        | Action::ShowInExplorer(_)
        | Action::ReimportAsset(_)
        | Action::FindReferencesInScene(_)
        | Action::Refresh) => {
            if let Some(other) = crate::asset_actions::apply(s, a) {
                return Some(other);
            }
        }
        a @ (Action::CopyObjects | Action::CutObjects | Action::PasteObjects { .. }) => {
            if let Some(other) = crate::object_clipboard::apply(s, a) {
                return Some(other);
            }
        }
        Action::EditorMenu(path) => {
            crate::editor_scripts::menus::run(s, &path);
        }
        other => return crate::prefabs::apply(s, other),
    }
    s.selection.retain_existing(&s.scene);
    None
}

/// GameObject ▸ Create…: at the Scene view pivot, or under `parent` at its origin.
fn create(s: &mut EditorState, what: Create, parent: Option<Entity>) {
    let parent = parent.filter(|p| s.scene.contains(*p));
    s.record(&format!("Create {}", what.default_name()));
    let e = s.scene.spawn(what.default_name(), parent);
    let mut t = Transform::default();
    if parent.is_none() {
        t.position = s.scene_view.camera.pivot().round();
    }
    match what {
        Create::Empty => {}
        Create::Folder => {
            // Not in the component order: the Inspector does not list it.
            let folder = misty_engine::scene::HierarchyFolder::default();
            let _ = s.scene.world.insert_one(e, folder);
        }
        Create::Primitive(mesh) => {
            s.scene.add(e, MeshFilter::primitive(mesh));
            s.scene.add(e, MeshRenderer::default());
            // Primitives come with a matching collider.
            match mesh.default_collider() {
                ColliderKind::Box => s.scene.add(e, BoxCollider::default()),
                ColliderKind::Sphere => s.scene.add(e, SphereCollider::default()),
                ColliderKind::Capsule => s.scene.add(e, CapsuleCollider::default()),
                ColliderKind::Mesh => s.scene.add(e, MeshCollider::default()),
            }
        }
        Create::Light(kind) => {
            let light = match kind {
                LightType::Directional => Light::directional(),
                LightType::Rectangle => Light::rectangle(),
                _ => Light {
                    kind,
                    ..Default::default()
                },
            };
            match kind {
                LightType::Point => {}
                // Rectangle lights start as a ceiling panel shining down.
                LightType::Rectangle => t.set_euler(Vec3::new(90.0, 0.0, 0.0)),
                _ => t.set_euler(Vec3::new(50.0, -30.0, 0.0)),
            }
            s.scene.add(e, light);
        }
        Create::Camera => s.scene.add(e, Camera::default()),
        Create::FogVolume => s.scene.add(e, misty_engine::scene::FogVolume::default()),
        Create::Water(kind) => {
            // The ocean sits at sea level; lakes and rivers where the view is looking.
            if kind == misty_engine::water::WaterKind::Ocean && parent.is_none() {
                t.position = Vec3::ZERO;
            }
            s.scene.add(e, misty_engine::water::WaterSurface::new(kind));
        }
        Create::AudioSource => s.scene.add(e, misty_engine::scene::AudioSource::default()),
        Create::Ui(_) | Create::Terrain => {} // handled by ui_create / terrain_tools
        Create::Sprite(shape) => {
            // On the XY plane, where the 2D view looks.
            t.position.z = 0.0;
            s.scene.add(
                e,
                misty_engine::scene::SpriteRenderer {
                    sprite: shape.map(misty_engine::sprites::builtin::path),
                    ..Default::default()
                },
            );
        }
        Create::ParticleSystem => {
            // Rotated so the cone emits upwards, like a new system in the familiar editor.
            t.set_euler(Vec3::new(-90.0, 0.0, 0.0));
            s.scene
                .add(e, misty_engine::scene::ParticleSystem::default());
        }
        Create::VisualEffect => {
            // Plays the project's first Effect Graph, if any (like the default asset the
            // reference editor assigns).
            let first = s.assets.effects.iter().next().cloned();
            s.scene.add(
                e,
                misty_engine::vfx::VisualEffect {
                    effect: first,
                    ..Default::default()
                },
            );
        }
    }
    crate::presets::apply_defaults(s, e);
    s.scene.set_transform(e, t);
    s.select(e);
    s.console.info(format!("Created '{}'.", s.scene.name(e)));
}

/// Wrap the selection in a new empty parent at their centre.
fn create_empty_parent(s: &mut EditorState) {
    let targets = s.selection.top_level(&s.scene);
    let Some(&first) = targets.first() else {
        return;
    };
    s.record("Create Empty Parent");
    let old_parent = s.scene.parent(first);
    let index = s.scene.sibling_index(first);
    let center = targets
        .iter()
        .map(|e| s.scene.world_position(*e))
        .sum::<Vec3>()
        / targets.len() as f32;
    let p = s.scene.spawn("GameObject", old_parent);
    s.scene.set_parent(p, old_parent, Some(index), false);
    s.scene.set_world_position(p, center);
    for e in targets {
        s.scene.set_parent(e, Some(p), None, true);
    }
    s.hierarchy.expanded.insert(p);
    s.select(p);
}

/// Centre and radius around the selection (for Frame Selected).
/// A Sprite Renderer's local bounds as a thin box.
fn sprite_box(scene: &misty_engine::scene::Scene, e: Entity) -> Option<(Vec3, Vec3)> {
    let (min, max) = misty_engine::sprites::local_bounds(scene, e)?;
    Some((min.extend(-0.01), max.extend(0.01)))
}

pub fn selection_bounds(s: &EditorState) -> Option<(Vec3, f32)> {
    let list = s.selection.list();
    if list.is_empty() {
        return None;
    }
    let points: Vec<(Vec3, f32)> = list
        .iter()
        .map(|e| {
            // UI objects frame their on-canvas rect.
            if let Some(b) = crate::ui_gizmos::ui_bounds(&s.scene, *e, s.game_view.screen_px) {
                return b;
            }
            let m = s.scene.world_matrix(*e);
            let bounds = s
                .scene
                .world
                .get::<&MeshFilter>(*e)
                .ok()
                .and_then(|f| f.mesh.clone())
                .and_then(|mesh| misty_engine::models::bounds(&mesh))
                .or_else(|| sprite_box(&s.scene, *e));
            match bounds {
                Some((min, max)) => (
                    m.transform_point3((min + max) * 0.5),
                    m.transform_vector3((max - min) * 0.5).length(),
                ),
                None => (m.w_axis.truncate(), 0.5),
            }
        })
        .collect();
    let center = points.iter().map(|(p, _)| *p).sum::<Vec3>() / points.len() as f32;
    let radius = points
        .iter()
        .map(|(p, r)| p.distance(center) + r)
        .fold(0.5_f32, f32::max);
    Some((center, radius))
}
