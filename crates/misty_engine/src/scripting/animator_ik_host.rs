//! The IK pass of the frame: `OnAnimatorIK(layerIndex)` on the scripts of Animators whose
//! controller has IK Pass layers, once per IK layer bottom to top, each followed by solving
//! the goals that call set on the pose blended up to that layer (`animation::animator_ik_pass`,
//! `rigging::animator_ik`).

use mlua::Value;

use super::{ScriptHost, csharp};

impl ScriptHost {
    /// After the Animators wrote their pose (and root motion moved them), before Timelines,
    /// rigs and LateUpdate. Nothing to do (one map walk) while no controller uses IK Pass.
    pub(super) fn animator_ik_pass(&mut self) {
        let passes = self.with_world(|w| w.animator.ik_passes(&w.scene));
        for (e, layers) in passes {
            self.with_world(|w| w.animator.ik_begin(&mut w.scene, e));
            for layer in layers {
                self.with_world(|w| {
                    if w.scene.contains(e) {
                        w.animator.ik_layer_begin(&mut w.scene, e, layer);
                    }
                });
                for i in 0..self.instances.len() {
                    if self.instances[i].entity != e || !self.runnable(&self.instances[i]) {
                        continue;
                    }
                    if let Some(cs) = self.instances[i].cs {
                        if cs.defines(csharp::Method::OnAnimatorIK) {
                            let mut w = self
                                .lua
                                .app_data_mut::<super::World>()
                                .expect("world is installed");
                            self.csharp.call_arg(
                                &mut w,
                                cs,
                                csharp::Method::OnAnimatorIK,
                                layer as u64,
                            );
                        }
                    } else {
                        self.call(i, "OnAnimatorIK", Some(Value::Integer(layer as i64)));
                    }
                    self.flush_spawned();
                }
                self.with_world(|w| {
                    if w.scene.contains(e) {
                        w.animator.ik_layer_solve(&mut w.scene, e, layer);
                    }
                });
            }
            self.with_world(|w| {
                if w.scene.contains(e) {
                    w.animator.ik_end(&mut w.scene, e);
                }
            });
        }
    }
}
