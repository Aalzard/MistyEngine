//! The Animator side of the IK pass: which Animators run one and the Avatar they use
//! (the goals and solver live in `rigging::animator_ik`).

use std::sync::Arc;

use hecs::Entity;

use super::AnimatorPlayer;
use crate::animation::humanoid::Avatar;
use crate::scene::{Animator, Scene};

impl AnimatorPlayer {
    /// Playing Animators whose controller has layers with IK Pass on, with those layers.
    pub fn ik_passes(&mut self, scene: &Scene) -> Vec<(Entity, Vec<usize>)> {
        let mut out: Vec<(Entity, Vec<usize>)> = self
            .instances
            .iter()
            .filter(|(_, inst)| {
                inst.ctrl.base_ik_pass || inst.ctrl.layers.iter().any(|l| l.ik_pass)
            })
            .filter(|(e, _)| {
                scene.contains(**e)
                    && scene.active_in_hierarchy(**e)
                    && scene.world.get::<&Animator>(**e).is_ok_and(|a| a.enabled)
            })
            .map(|(e, inst)| {
                let layers = (0..inst.ctrl.layer_count())
                    .filter(|l| inst.ctrl.ik_pass(*l))
                    .collect();
                (*e, layers)
            })
            .collect();
        out.sort_by_key(|(e, _)| e.to_bits());
        if !out.is_empty() {
            self.ik.retain(scene);
        }
        out
    }

    /// The Avatar IK of `e` works with: its Animator's, or one found by bone names.
    pub fn ik_avatar(&mut self, scene: &Scene, e: Entity) -> Arc<Avatar> {
        let own = scene
            .world
            .get::<&Animator>(e)
            .ok()
            .and_then(|a| a.avatar.clone())
            .and_then(|p| self.assets.avatar(&p));
        self.ik.avatar_for(scene, e, own)
    }

    /// Start `e`'s IK pass (goals cleared, last frame's IK undone where nothing else moved).
    pub fn ik_begin(&mut self, scene: &mut Scene, e: Entity) {
        self.ik.begin(scene, e);
    }

    /// Solve the goals set in `e`'s `OnAnimatorIK`.
    pub fn ik_solve(&mut self, scene: &mut Scene, e: Entity) {
        let avatar = self.ik_avatar(scene, e);
        self.ik.solve(scene, e, &avatar);
    }
}
