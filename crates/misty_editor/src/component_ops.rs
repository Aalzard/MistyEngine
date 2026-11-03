//! Inspector component context-menu commands: Reset, Remove, Move Up/Down, Copy and Paste.
//! Every command is one undo step and applies to all the objects the Inspector edits.

use misty_engine::hecs::Entity;
use misty_engine::scene::registry;

use crate::actions::Action;
use crate::state::EditorState;

/// Apply a component command. Returns the action back if it is not one.
pub fn apply(s: &mut EditorState, action: Action) -> Option<Action> {
    match action {
        Action::RemoveComponent { entities, name } => {
            let targets = with_component(s, &entities, &name);
            if !targets.is_empty() {
                s.record(&format!("Remove {name}"));
                for e in targets {
                    s.scene.remove_by_name(e, &name);
                }
            }
        }
        Action::ResetComponent { entities, name } => {
            let targets = with_component(s, &entities, &name);
            if let Some(info) = registry::find(&name)
                && !targets.is_empty()
            {
                s.record(&format!("Reset {name}"));
                for e in targets {
                    (info.add_default)(&mut s.scene.world, e);
                }
            }
        }
        Action::MoveComponent { entity, name, dir } => {
            if can_move(s, entity, &name, dir) {
                s.record(if dir < 0 {
                    "Move Component Up"
                } else {
                    "Move Component Down"
                });
                s.scene.move_component(entity, &name, dir);
            }
        }
        Action::CopyComponent { entity, name } => {
            s.component_clipboard = registry::find(&name)
                .and_then(|info| (info.to_json)(&s.scene.world, entity))
                .map(|v| (name, v));
        }
        Action::PasteComponentValues { entities, name } => {
            let (copied, value) = s.component_clipboard.clone()?;
            let targets = with_component(s, &entities, &name);
            if let Some(info) = registry::find(&name)
                && copied == name
                && !targets.is_empty()
            {
                s.record(&format!("Paste {name} Values"));
                for e in targets {
                    if let Err(err) = (info.from_json)(&mut s.scene.world, e, value.clone()) {
                        s.console.warn(format!("Paste failed: {err}"));
                    }
                }
            }
        }
        Action::PasteComponentAsNew { entities } => paste_as_new(s, &entities),
        other => return Some(other),
    }
    None
}

/// The objects in `entities` that have component `name`.
fn with_component(s: &EditorState, entities: &[Entity], name: &str) -> Vec<Entity> {
    let Some(info) = registry::find(name) else {
        return Vec::new();
    };
    entities
        .iter()
        .copied()
        .filter(|e| s.scene.contains(*e) && (info.has)(&s.scene.world, *e))
        .collect()
}

/// Move Up/Down is possible: Transform stays first, and the slot must exist.
pub fn can_move(s: &EditorState, e: Entity, name: &str, dir: i32) -> bool {
    if !s.scene.contains(e) || registry::find(name).is_none_or(|i| !i.removable) {
        return false;
    }
    let order = s.scene.component_names(e);
    let Some(i) = order.iter().position(|n| n == name) else {
        return false;
    };
    let j = i as i32 + dir;
    j >= 1 && (j as usize) < order.len()
}

/// The clipboard holds a component that `e` doesn't have yet (Paste Component As New).
pub fn can_paste_as_new(s: &EditorState, e: Entity) -> bool {
    s.component_clipboard.as_ref().is_some_and(|(name, _)| {
        registry::find(name).is_some_and(|i| i.removable && !(i.has)(&s.scene.world, e))
    })
}

/// Add the copied component, with its values, to every object that lacks it.
fn paste_as_new(s: &mut EditorState, entities: &[Entity]) {
    let Some((name, value)) = s.component_clipboard.clone() else {
        return;
    };
    let targets: Vec<Entity> = entities
        .iter()
        .copied()
        .filter(|e| s.scene.contains(*e) && can_paste_as_new(s, *e))
        .collect();
    let Some(info) = registry::find(&name) else {
        return;
    };
    if targets.is_empty() {
        s.console.warn(format!("The object already has a {name}."));
        return;
    }
    s.record(&format!("Paste {name} As New"));
    for e in targets {
        match (info.from_json)(&mut s.scene.world, e, value.clone()) {
            Ok(()) => s
                .scene
                .game_object_mut(e)
                .component_order
                .push(name.clone()),
            Err(err) => s.console.warn(format!("Paste failed: {err}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use misty_engine::scene::{BoxCollider, Camera, Light};

    use super::*;
    use crate::project::ProjectBrowser;

    fn state() -> EditorState {
        EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_component_ops_test"),
        ))
    }

    fn run(s: &mut EditorState, a: Action) {
        assert!(apply(s, a).is_none());
    }

    #[test]
    fn paste_values_applies_to_every_target_and_undoes_in_one_step() {
        let mut s = state();
        let (a, b, c) = (
            s.scene.spawn("A", None),
            s.scene.spawn("B", None),
            s.scene.spawn("C", None),
        );
        for e in [a, b, c] {
            s.scene.add(e, Light::default());
        }
        s.scene.world.get::<&mut Light>(a).unwrap().intensity = 7.0;
        run(
            &mut s,
            Action::CopyComponent {
                entity: a,
                name: "Light".into(),
            },
        );
        let name = "Light".to_owned();
        run(
            &mut s,
            Action::PasteComponentValues {
                entities: vec![b, c],
                name,
            },
        );
        for e in [b, c] {
            assert_eq!(s.scene.world.get::<&Light>(e).unwrap().intensity, 7.0);
        }
        crate::commands::apply(&mut s, Action::Undo);
        for e in [b, c] {
            assert_eq!(s.scene.world.get::<&Light>(e).unwrap().intensity, 1.0);
        }
    }

    #[test]
    fn paste_as_new_adds_the_component_once_in_inspector_order() {
        let mut s = state();
        let (a, b) = (s.scene.spawn("A", None), s.scene.spawn("B", None));
        s.scene.add(
            a,
            BoxCollider {
                is_trigger: true,
                ..Default::default()
            },
        );
        s.scene.add(b, Camera::default());
        run(
            &mut s,
            Action::CopyComponent {
                entity: a,
                name: "Box Collider".into(),
            },
        );
        assert!(can_paste_as_new(&s, b) && !can_paste_as_new(&s, a));
        run(
            &mut s,
            Action::PasteComponentAsNew {
                entities: vec![a, b],
            },
        );
        assert_eq!(
            s.scene.component_names(b),
            ["Transform", "Camera", "Box Collider"]
        );
        assert!(s.scene.world.get::<&BoxCollider>(b).unwrap().is_trigger);
        crate::commands::apply(&mut s, Action::Undo);
        assert_eq!(s.scene.component_names(b), ["Transform", "Camera"]);
    }

    #[test]
    fn move_reset_and_remove_respect_order_and_undo() {
        let mut s = state();
        let e = s.scene.spawn("E", None);
        s.scene.add(e, Camera::default());
        s.scene.add(e, Light::default());
        assert!(!can_move(&s, e, "Camera", -1), "Transform stays first");
        assert!(!can_move(&s, e, "Light", 1), "already last");
        assert!(!can_move(&s, e, "Transform", 1));
        run(
            &mut s,
            Action::MoveComponent {
                entity: e,
                name: "Light".into(),
                dir: -1,
            },
        );
        assert_eq!(s.scene.component_names(e), ["Transform", "Light", "Camera"]);

        s.scene.world.get::<&mut Camera>(e).unwrap().fov = 20.0;
        run(
            &mut s,
            Action::ResetComponent {
                entities: vec![e],
                name: "Camera".into(),
            },
        );
        assert_eq!(s.scene.world.get::<&Camera>(e).unwrap().fov, 60.0);
        run(
            &mut s,
            Action::RemoveComponent {
                entities: vec![e],
                name: "Camera".into(),
            },
        );
        assert_eq!(s.scene.component_names(e), ["Transform", "Light"]);

        for _ in 0..3 {
            crate::commands::apply(&mut s, Action::Undo);
        }
        assert_eq!(s.scene.component_names(e), ["Transform", "Camera", "Light"]);
        assert_eq!(s.scene.world.get::<&Camera>(e).unwrap().fov, 60.0);
    }
}
