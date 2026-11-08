//! Play-mode camera rig: solves every active Virtual Camera, picks each brain's live one and
//! blends the real Camera between them.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use hecs::Entity;

use super::components::{BlendDefinition, BlendList, BrainUpdate, CameraBrain, VirtualCamera};
use super::impulse::ImpulseManager;
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
    /// The camera a timeline override showed last frame (most weight), while one is on.
    overridden: Option<Entity>,
    /// Weight of the override's `to` camera last frame.
    override_weight: Option<f32>,
}

/// A timeline Camera track taking over a brain for this frame: it shows `from` blended
/// towards `to` by `weight` (0 = all `from`); `None` stands for the brain's own live camera,
/// so clips can ease in from and out to normal play.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraOverride {
    pub brain: Entity,
    pub from: Option<Entity>,
    pub to: Option<Entity>,
    pub weight: f32,
}

impl CameraOverride {
    /// The camera with the most weight (`None` = the brain's own live camera).
    pub fn dominant(&self) -> Option<Entity> {
        if self.weight >= 0.5 {
            self.to
        } else {
            self.from
        }
    }
}

/// Runtime state of every brain and virtual camera while playing.
#[derive(Default)]
pub struct CameraRig {
    vcams: HashMap<Entity, VcamRuntime>,
    /// When each active camera was enabled (larger = more recent; breaks priority ties).
    stamps: HashMap<Entity, u64>,
    counter: u64,
    brains: HashMap<Entity, BrainState>,
    /// Timeline overrides for the coming camera pass (replaced by the directors every frame).
    overrides: Vec<CameraOverride>,
    /// Live camera shakes.
    pub impulses: ImpulseManager,
    /// Project folder (blend list assets are read from it).
    root: Option<PathBuf>,
    blend_lists: HashMap<String, Arc<BlendList>>,
    logs: Vec<String>,
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
    /// A rig that reads blend list assets from the project folder `root`.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: Some(root),
            ..Default::default()
        }
    }

    /// Timeline Camera tracks' overrides for the next camera pass (replaces the last ones).
    pub fn set_overrides(&mut self, overrides: Vec<CameraOverride>) {
        self.overrides = overrides;
    }

    /// The override on brain `b` this frame, if a timeline has one (the last one wins).
    pub fn override_for(&self, b: Entity) -> Option<CameraOverride> {
        self.overrides.iter().rev().find(|o| o.brain == b).copied()
    }

    /// Problems for the Console (blend lists that failed to load).
    pub fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }

    /// The blend list asset at `path` (cached; a broken file warns once and counts as empty).
    fn blend_list(&mut self, path: &str) -> Arc<BlendList> {
        if let Some(l) = self.blend_lists.get(path) {
            return l.clone();
        }
        let loaded = match &self.root {
            Some(root) => BlendList::load(&root.join(path)).unwrap_or_else(|e| {
                self.logs.push(format!("Blend list failed to load: {e}"));
                BlendList::default()
            }),
            None => BlendList::default(),
        };
        let l = Arc::new(loaded);
        self.blend_lists.insert(path.to_owned(), l.clone());
        l
    }

    /// The blend `brain` uses from camera `from` to `to`.
    fn blend_between(
        &mut self,
        scene: &Scene,
        brain: &CameraBrain,
        from: Entity,
        to: Entity,
    ) -> BlendDefinition {
        let list = brain
            .blend_list
            .as_deref()
            .map(|p| self.blend_list(p))
            .unwrap_or_default();
        brain.blend_with(&list.blends, &scene.name(from), &scene.name(to))
    }

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
        super::group::place_groups(scene);
        self.impulses.advance(cx.dt);
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
            let mut out = solve(scene, e, &mut vc, st, cx);
            if vc.impulse.enabled {
                let (dp, dr) = self.impulses.offset(&vc.impulse, out.position);
                out.position += dp;
                out.rotation = (out.rotation * dr).normalize();
                st.output = Some(out);
            }
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
        let prev = self.brains.get(&b).copied().unwrap_or_default();
        let ov = self.override_for(b);
        // A timeline override just ended: blend from the camera it showed to the live one.
        let ended = match (ov, prev.overridden, live) {
            (None, Some(old), Some(new)) if old != new => {
                Some((old, self.blend_between(scene, &brain, old, new)))
            }
            _ => None,
        };
        let changed = match (prev.live, live) {
            (Some(old), Some(new)) if old != new && ended.is_none() => {
                Some(self.blend_between(scene, &brain, old, new))
            }
            _ => None,
        };
        let st = self.brains.entry(b).or_default();
        if let Some((old, def)) = ended {
            st.blend = match st.output {
                Some(shown) if !def.is_cut() => Some(ActiveBlend {
                    from: match outputs.get(&old) {
                        Some(o) => BlendFrom::Camera(old, *o),
                        None => BlendFrom::Snapshot(shown),
                    },
                    def,
                    elapsed: 0.0,
                }),
                _ => None,
            };
            st.live = live;
        } else if live != st.live {
            st.blend = match (st.live, changed, st.output) {
                (Some(old), Some(def), Some(shown)) => {
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
        st.overridden = None;
        st.override_weight = None;
        let normal =
            live.and_then(|l| outputs.get(&l))
                .copied()
                .map(|to| match st.blend.as_mut() {
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
                        let w = blend.def.weight(blend.progress());
                        if blend.progress() >= 1.0 {
                            st.blend = None;
                        }
                        from.lerp(&to, w)
                    }
                    None => to,
                });
        // A timeline Camera track shows its clips' cameras instead (`None` = normal play).
        let out = match ov {
            Some(o) => {
                let pose = |c: Option<Entity>| c.and_then(|c| outputs.get(&c)).copied().or(normal);
                let shown = match (pose(o.from), pose(o.to)) {
                    (Some(a), Some(z)) => Some(a.lerp(&z, o.weight.clamp(0.0, 1.0))),
                    (a, z) => a.or(z),
                };
                st.overridden = o.dominant().or(st.live);
                st.override_weight = Some(o.weight);
                shown
            }
            None => normal,
        };
        let Some(out) = out else {
            return;
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

    /// The virtual camera brain `b` shows (the one it blends towards during a blend; under a
    /// timeline override, the override's camera with the most weight).
    pub fn live_camera(&self, b: Entity) -> Option<Entity> {
        self.brains.get(&b).and_then(|s| s.overridden.or(s.live))
    }

    /// True while brain `b` is between two cameras.
    pub fn is_blending(&self, b: Entity) -> bool {
        self.blend_weight(b).is_some()
    }

    /// Weight of the incoming camera during a blend (eased), or `None` when not blending.
    pub fn blend_weight(&self, b: Entity) -> Option<f32> {
        let s = self.brains.get(&b)?;
        if let Some(w) = s.override_weight {
            return (w > 1e-4 && w < 1.0 - 1e-4).then_some(w);
        }
        let blend = s.blend?;
        Some(blend.def.weight(blend.progress()))
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
        let overridden = |b: Entity| self.overrides.iter().any(|o| o.brain == b);
        self.overrides.iter().any(|o| {
            (o.to == Some(e) && o.weight > 1e-4) || (o.from == Some(e) && o.weight < 1.0 - 1e-4)
        }) || self.brains.iter().any(|(b, s)| {
            !overridden(*b)
                && (s.live == Some(e)
                    || s.blend
                        .is_some_and(|b| matches!(b.from, BlendFrom::Camera(f, _) if f == e)))
        })
    }

    /// Forget camera `e`'s damping so it snaps next frame (after a teleport, say).
    pub fn reset_camera(&mut self, e: Entity) {
        self.vcams.remove(&e);
    }
}
