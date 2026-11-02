//! Scene-editing commands (create, delete, duplicate, parent, align, undo).

use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    BoxCollider, Camera, CapsuleCollider, ColliderKind, Light, LightType, MeshCollider, MeshFilter,
    MeshRenderer, Scene, ScriptRef, Scripts, SphereCollider, Transform, registry,
};

use crate::actions::{Action, Create};
use crate::state::EditorState;

/// Apply a scene command. Returns the action back if it is not a scene command.
pub fn apply(s: &mut EditorState, action: Action) -> Option<Action> {
    match action {
        Action::NewScene => {
            s.record("New Scene");
            s.scene = Scene::default_3d("Untitled");
            s.selection.clear();
            s.scene_dirty = false;
            s.console.info("Created new scene.");
        }
        Action::Create { what, parent } => create(s, what, parent),
        Action::CreateEmptyParent => create_empty_parent(s),
        Action::DeleteSelected => {
            let targets = s.selection.top_level(&s.scene);
            if !targets.is_empty() {
                s.record("Delete");
                for e in targets {
                    s.scene.despawn(e);
                }
                s.selection.clear();
            }
        }
        Action::DuplicateSelected => {
            let targets = s.selection.top_level(&s.scene);
            if !targets.is_empty() {
                s.record("Duplicate");
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
                s.record("Toggle Active State");
                for e in targets {
                    let mut go = s.scene.game_object_mut(e);
                    go.active = !go.active;
                }
            }
        }
        Action::ClearParent => {
            let targets = s.selection.top_level(&s.scene);
            if targets.iter().any(|e| s.scene.parent(*e).is_some()) {
                s.record("Clear Parent");
                for e in targets {
                    s.scene.set_parent(e, None, None, true);
                }
            }
        }
        Action::SetSibling { first } => {
            if let Some(e) = s.selection.active() {
                s.record("Set Sibling Index");
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
                    if !s.scene.add_by_name(e, name) {
                        s.console
                            .warn(format!("'{}' already has a {name}.", s.scene.name(e)));
                    }
                }
            }
        }
        Action::RemoveComponent { entity, name } => {
            s.record(&format!("Remove {name}"));
            s.scene.remove_by_name(entity, &name);
        }
        Action::ResetComponent { entity, name } => {
            if let Some(info) = registry::find(&name) {
                s.record(&format!("Reset {name}"));
                (info.add_default)(&mut s.scene.world, entity);
            }
        }
        Action::MoveComponent { entity, name, dir } => {
            s.record("Move Component");
            s.scene.move_component(entity, &name, dir);
        }
        Action::PasteComponentValues { entity, name } => {
            if let (Some(info), Some((_, value))) =
                (registry::find(&name), s.component_clipboard.clone())
            {
                s.record(&format!("Paste {name} Values"));
                if let Err(err) = (info.from_json)(&mut s.scene.world, entity, value) {
                    s.console.warn(format!("Paste failed: {err}"));
                }
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
        Action::Undo => match s.undo.undo(&mut s.scene, &mut s.selection) {
            Some(label) => s.console.info(format!("Undo {label}")),
            None => s.console.info("Nothing to undo."),
        },
        Action::Redo => match s.undo.redo(&mut s.scene, &mut s.selection) {
            Some(label) => s.console.info(format!("Redo {label}")),
            None => s.console.info("Nothing to redo."),
        },
        other => return Some(other),
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
        Create::Primitive(mesh) => {
            s.scene.add(e, MeshFilter { mesh: Some(mesh) });
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
            let light = if kind == LightType::Directional {
                Light::directional()
            } else {
                Light {
                    kind,
                    ..Default::default()
                }
            };
            if kind != LightType::Point {
                t.set_euler(Vec3::new(50.0, -30.0, 0.0));
            }
            s.scene.add(e, light);
        }
        Create::Camera => s.scene.add(e, Camera::default()),
    }
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
pub fn selection_bounds(s: &EditorState) -> Option<(Vec3, f32)> {
    let list = s.selection.list();
    if list.is_empty() {
        return None;
    }
    let points: Vec<(Vec3, f32)> = list
        .iter()
        .map(|e| {
            let m = s.scene.world_matrix(*e);
            let extent = s
                .scene
                .world
                .get::<&MeshFilter>(*e)
                .ok()
                .and_then(|f| f.mesh)
                .map(|mesh| (m.transform_vector3(mesh.half_extents())).length())
                .unwrap_or(0.5);
            (m.w_axis.truncate(), extent)
        })
        .collect();
    let center = points.iter().map(|(p, _)| *p).sum::<Vec3>() / points.len() as f32;
    let radius = points
        .iter()
        .map(|(p, r)| p.distance(center) + r)
        .fold(0.5_f32, f32::max);
    Some((center, radius))
}
