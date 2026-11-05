//! Play-mode camera rig: solves every active Virtual Camera, picks each brain's live one and
//! blends the real Camera between them.

use std::collections::HashMap;

use hecs::Entity;

use super::components::{BlendDefinition, BrainUpdate, CameraBrain, VirtualCamera};
use super::solve::{CamState, SolveContext, VcamRuntime, solve};
use crate::scene::{Camera, Scene};

/// Where a blend comes from.
#[derive(Clone, Copy, Debug)]
enum BlendFrom {
    /// A camera that keeps moving during the blend (its last pose if it stops).
    Camera(Entity, CamState),
    /// A frozen pose (a blend interrupted half way).
    Snapshot(CamState),
}

#[derive(Clone, Copy, Debug)]
struct ActiveBlend {
    from: BlendFrom,
    def: BlendDefinition,
    elapsed: f32,
}

impl ActiveBlend {
    fn progress(&self) -> f32 {
        (self.elapsed / self.def.duration.max(1e-4)).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct BrainState {
    live: Option<Entity>,
    blend: Option<ActiveBlend>,
    output: Option<CamState>,
}

/// Runtime state of every brain and virtual camera while playing.
#[derive(Default)]
pub struct CameraRig {
    vcams: HashMap<Entity, VcamRuntime>,
    /// When each active camera was enabled (larger = more recent; breaks priority ties).
    stamps: HashMap<Entity, u64>,
    counter: u64,
    brains: HashMap<Entity, BrainState>,
}

/// Active, enabled virtual cameras in Hierarchy order.
pub fn active_cameras(scene: &Scene) -> Vec<Entity> {
    scene
        .active_with::<VirtualCamera>()
        .into_iter()
        .filter(|&e| {
            scene
                .world
                .get::<&VirtualCamera>(e)
                .is_ok_and(|v| v.enabled)
        })
        .collect()
}

/// Active, enabled brains in Hierarchy order.
pub fn active_brains(scene: &Scene) -> Vec<Entity> {
    scene
        .active_with::<CameraBrain>()
        .into_iter()
        .filter(|&e| scene.world.get::<&CameraBrain>(e).is_ok_and(|b| b.enabled))
        .collect()
}

impl CameraRig {
    /// Move the cameras if this is the pass they run in (the first brain's Update Method;
    /// Late Update without a brain).
    pub fn update(&mut self, scene: &mut Scene, pass: BrainUpdate, cx: &SolveContext) {
        let brains = active_brains(scene);
        let method = brains
            .first()
            .and_then(|&b| {
                scene
                    .world
                    .get::<&CameraBrain>(b)
                    .ok()
                    .map(|b| b.update_method)
            })
            .unwrap_or_default();
        if method != pass {
            return;
        }
        let outputs = self.solve_cameras(scene, cx);
        for b in brains {
            self.update_brain(scene, b, &outputs, cx.dt);
        }
        self.brains.retain(|b, _| scene.contains(*b));
    }

    fn solve_cameras(&mut self, scene: &mut Scene, cx: &SolveContext) -> HashMap<Entity, CamState> {
        let active = active_cameras(scene);
        for &e in &active {
            if !self.stamps.contains_key(&e) {
                self.counter += 1;
                self.stamps.insert(e, self.counter);
            }
        }
        self.stamps.retain(|e, _| active.contains(e));
        self.vcams.retain(|e, _| active.contains(e));
        let mut outputs = HashMap::new();
        for e in active {
            let Some(mut vc) = scene
                .world
                .get::<&VirtualCamera>(e)
                .ok()
                .map(|v| (*v).clone())
            else {
                continue;
            };
            let st = self.vcams.entry(e).or_default();
            let out = solve(scene, e, &mut vc, st, cx);
            // The object itself carries the raw pose (no dutch, collider or noise).
            let (pos, rot) = (st.raw_position, st.raw_rotation);
            scene.set_world_position(e, pos);
            scene.set_world_rotation(e, rot);
            if let Ok(mut c) = scene.world.get::<&mut VirtualCamera>(e) {
                c.orbital = vc.orbital;
            }
            outputs.insert(e, out);
        }
        outputs
    }

    /// The camera a brain should show: highest priority, then most recently enabled.
    pub fn pick_live(&self, scene: &Scene) -> Option<Entity> {
        active_cameras(scene).into_iter().max_by_key(|e| {
            let priority = scene
                .world
                .get::<&VirtualCamera>(*e)
                .map_or(0, |v| v.priority);
            (priority, self.stamps.get(e).copied().unwrap_or(0))
        })
    }

    fn update_brain(
        &mut self,
        scene: &mut Scene,
        b: Entity,
        outputs: &HashMap<Entity, CamState>,
        dt: f32,
    ) {
        let Some(brain) = scene
            .world
            .get::<&CameraBrain>(b)
            .ok()
            .map(|c| (*c).clone())
        else {
            return;
        };
        let live = self.pick_live(scene);
        let st = self.brains.entry(b).or_default();
        if live != st.live {
            st.blend = match (st.live, live, st.output) {
                (Some(old), Some(new), Some(shown)) => {
                    let def = brain.blend_for(&scene.name(old), &scene.name(new));
                    if def.is_cut() {
                        None
                    } else if st.blend.is_some() || !outputs.contains_key(&old) {
                        Some(ActiveBlend {
                            from: BlendFrom::Snapshot(shown),
                            def,
                            elapsed: 0.0,
                        })
                    } else {
                        Some(ActiveBlend {
                            from: BlendFrom::Camera(old, outputs[&old]),
                            def,
                            elapsed: 0.0,
                        })
                    }
                }
                _ => None,
            };
            st.live = live;
        }
        let Some(to) = live.and_then(|l| outputs.get(&l)).copied() else {
            return;
        };
        let out = match st.blend.as_mut() {
            Some(blend) => {
                blend.elapsed += dt;
                if let BlendFrom::Camera(from, last) = &mut blend.from
                    && let Some(now) = outputs.get(from)
                {
                    *last = *now;
                }
                let from = match blend.from {
                    BlendFrom::Camera(_, s) | BlendFrom::Snapshot(s) => s,
                };
                let w = blend.def.style.ease(blend.progress());
                if blend.progress() >= 1.0 {
                    st.blend = None;
                }
                from.lerp(&to, w)
            }
            None => to,
        };
        st.output = Some(out);
        scene.set_world_position(b, out.position);
        scene.set_world_rotation(b, out.rotation);
        if let Ok(mut cam) = scene.world.get::<&mut Camera>(b) {
            cam.fov = out.fov;
            cam.near = out.near;
            cam.far = out.far;
        }
    }

    /// The virtual camera brain `b` shows (the one it blends towards during a blend).
    pub fn live_camera(&self, b: Entity) -> Option<Entity> {
        self.brains.get(&b).and_then(|s| s.live)
    }

    /// True while brain `b` is between two cameras.
    pub fn is_blending(&self, b: Entity) -> bool {
        self.brains.get(&b).is_some_and(|s| s.blend.is_some())
    }

    /// Weight of the incoming camera during a blend (eased), or `None` when not blending.
    pub fn blend_weight(&self, b: Entity) -> Option<f32> {
        let blend = self.brains.get(&b)?.blend?;
        Some(blend.def.style.ease(blend.progress()))
    }

    /// The pose brain `b` gave its camera last.
    pub fn brain_output(&self, b: Entity) -> Option<CamState> {
        self.brains.get(&b)?.output
    }

    /// The last pose solved for virtual camera `e` (with collider and noise).
    pub fn camera_output(&self, e: Entity) -> Option<CamState> {
        self.vcams.get(&e)?.output
    }

    /// True when some brain shows `e` (live, or blending to or from it).
    pub fn is_live(&self, e: Entity) -> bool {
        self.brains.values().any(|s| {
            s.live == Some(e)
                || s.blend
                    .is_some_and(|b| matches!(b.from, BlendFrom::Camera(f, _) if f == e))
        })
    }

    /// Forget camera `e`'s damping so it snaps next frame (after a teleport, say).
    pub fn reset_camera(&mut self, e: Entity) {
        self.vcams.remove(&e);
    }
}
