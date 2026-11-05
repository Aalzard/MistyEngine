//! Objects named by where they are rather than by entity, so a selection survives its scene
//! being reloaded (new entities) or the editor restarting: the scene's file and, from the
//! scene's top level down, each object's name and which of the same-named siblings it is.

use misty_engine::hecs::Entity;
use misty_engine::scene::{Scene, SceneId};
use serde::{Deserialize, Serialize};

/// One step: an object's name and its index among siblings with that name.
pub type Step = (String, usize);

/// An object by place: scene file (or `None` for an untitled scene) and the steps down to it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRef {
    pub scene: Option<String>,
    pub path: Vec<Step>,
}

/// The steps from `e`'s scene's top level down to `e`.
pub fn steps(scene: &Scene, e: Entity) -> Vec<Step> {
    let mut chain = Vec::new();
    let mut cur = Some(e);
    while let Some(c) = cur {
        let parent = scene.parent(c);
        let siblings = match parent {
            Some(p) => scene.children(p),
            None => scene.roots_in(scene.scene_of(c)),
        };
        let name = scene.name(c);
        let nth = siblings
            .iter()
            .take_while(|x| **x != c)
            .filter(|x| scene.name(**x) == name)
            .count();
        chain.push((name, nth));
        cur = parent;
    }
    chain.reverse();
    chain
}

/// The object at `steps` inside scene `id`, if it is still there.
pub fn resolve(scene: &Scene, id: SceneId, steps: &[Step]) -> Option<Entity> {
    let mut level = scene.roots_in(id);
    let mut found = None;
    for (name, nth) in steps {
        let e = *level
            .iter()
            .filter(|x| scene.name(**x) == *name)
            .nth(*nth)?;
        found = Some(e);
        level = scene.children(e);
    }
    found
}

pub fn object_ref(scene: &Scene, e: Entity) -> ObjectRef {
    ObjectRef {
        scene: scene.scene_path(scene.scene_of(e)),
        path: steps(scene, e),
    }
}

/// The object `r` names, in the loaded scene saved at its path (or the active scene when it
/// has none).
pub fn find(scene: &Scene, r: &ObjectRef) -> Option<Entity> {
    let id = match &r.scene {
        Some(p) => scene.find_loaded(p)?,
        None => scene.active_scene(),
    };
    resolve(scene, id, &r.path)
}
