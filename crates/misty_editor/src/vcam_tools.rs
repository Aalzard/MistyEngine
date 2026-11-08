//! GameObject ▸ Camera ▸ Virtual Camera / Free Look Camera / Target Group Camera / Impulse
//! Source: a new virtual camera (or impulse source) where the
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

/// What GameObject ▸ Camera can make here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Virtual,
    FreeLook,
    /// A Target Group of the selected objects and a Group Composer camera framing it.
    TargetGroup,
    ImpulseSource,
}

impl Kind {
    pub const ALL: [Kind; 4] = [
        Self::Virtual,
        Self::FreeLook,
        Self::TargetGroup,
        Self::ImpulseSource,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Virtual => "Virtual Camera",
            Self::FreeLook => "Free Look Camera",
            Self::TargetGroup => "Target Group Camera",
            Self::ImpulseSource => "Impulse Source",
        }
    }
}

/// The virtual camera entries of GameObject ▸ Camera.
pub fn menu(ui: &mut egui::Ui, s: &mut EditorState, parent: Option<Entity>) {
    for k in Kind::ALL {
        if k == Kind::ImpulseSource {
            ui.separator();
        }
        if ui.button(k.label()).clicked() {
            make(s, parent, k);
            ui.close();
        }
    }
}

/// Make a `kind` object (where the Scene view looks, unless it has a parent).
pub fn make(state: &mut EditorState, parent: Option<Entity>, kind: Kind) {
    let parent = parent.filter(|p| state.scene.contains(*p));
    let base = kind.label();
    // The objects a group camera frames: the selection (before the new object is selected).
    let members: Vec<Entity> = state
        .selection
        .list()
        .iter()
        .copied()
        .filter(|e| state.scene.contains(*e))
        .collect();
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
    if kind == Kind::ImpulseSource {
        state
            .scene
            .add(e, misty_engine::vcam::ImpulseSource::default());
        state.select(e);
        state.console.info(format!(
            "Created '{name}'. Cameras with an Impulse Listener shake when it fires."
        ));
        return;
    }
    let vc = match kind {
        Kind::FreeLook => VirtualCamera::free_look(),
        Kind::TargetGroup => {
            let group = state.scene.unique_name("Target Group", parent);
            let g = state.scene.spawn(&group, parent);
            state.scene.add(
                g,
                misty_engine::vcam::TargetGroup {
                    members: members
                        .iter()
                        .map(|m| misty_engine::vcam::GroupMember {
                            target: Some(*m),
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                },
            );
            VirtualCamera {
                follow: Some(g),
                look_at: Some(g),
                ..VirtualCamera::group_framing()
            }
        }
        _ => VirtualCamera::default(),
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
