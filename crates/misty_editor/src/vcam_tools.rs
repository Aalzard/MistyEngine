//! GameObject ▸ Camera ▸ Virtual Camera / Free Look Camera: a new virtual camera where the
//! Scene view looks, and a Camera Brain on the main camera if it has none yet.

use misty_engine::hecs::Entity;
use misty_engine::scene::{Camera, Scene, Transform};
use misty_engine::vcam::{CameraBrain, VirtualCamera};

use crate::state::EditorState;

/// The camera a brain belongs on: the enabled one tagged MainCamera, else the first camera.
pub fn main_camera(scene: &Scene) -> Option<Entity> {
    let cams: Vec<Entity> = scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|&e| scene.world.get::<&Camera>(e).is_ok())
        .collect();
    cams.iter()
        .copied()
        .find(|&e| scene.game_object(e).tag == "MainCamera")
        .or_else(|| cams.first().copied())
}

/// Add a Camera Brain to the main camera when it has none; returns that camera.
pub fn ensure_brain(scene: &mut Scene) -> Option<Entity> {
    let cam = main_camera(scene)?;
    if scene.world.get::<&CameraBrain>(cam).is_err() {
        scene.add(cam, CameraBrain::default());
    }
    Some(cam)
}

pub fn create(state: &mut EditorState, parent: Option<Entity>, free_look: bool) {
    let parent = parent.filter(|p| state.scene.contains(*p));
    let base = if free_look {
        "Free Look Camera"
    } else {
        "Virtual Camera"
    };
    state.record(&format!("Create {base}"));
    let name = state.scene.unique_name(base, parent);
    let e = state.scene.spawn(&name, parent);
    let mut t = Transform::default();
    if parent.is_none() {
        let view = &state.scene_view.camera;
        t.position = view.position();
        t.rotation = view.rotation();
    }
    state.scene.set_transform(e, t);
    let vc = if free_look {
        VirtualCamera::free_look()
    } else {
        VirtualCamera::default()
    };
    state.scene.add(e, vc);
    let brain = ensure_brain(&mut state.scene);
    state.select(e);
    match brain {
        Some(cam) => state.console.info(format!(
            "Created '{name}'. '{}' shows it through its Camera Brain.",
            state.scene.name(cam)
        )),
        None => state.console.warn(format!(
            "Created '{name}'. Add a Camera with a Camera Brain to see it."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creating_adds_one_brain_to_the_main_camera() {
        let mut s = Scene::default_3d("t");
        let cam = ensure_brain(&mut s).unwrap();
        assert_eq!(s.name(cam), "Main Camera");
        ensure_brain(&mut s);
        assert_eq!(
            s.component_names(cam)
                .iter()
                .filter(|n| *n == "Camera Brain")
                .count(),
            1
        );
        assert!(ensure_brain(&mut Scene::new("empty")).is_none());
    }
}
