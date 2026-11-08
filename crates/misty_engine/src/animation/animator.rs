//! Play-mode evaluation of Animator components: parameters, one state machine per layer
//! (transitions, interruptions, sub-state machines, see `fsm`), blend trees, layers combined by
//! override or addition through Avatar Masks, root motion and target matching.
//!
//! Each frame every layer's playing states are sampled into poses (animated property values),
//! blended within the layer (cross-fades, frozen interrupted blends), then layers are applied
//! bottom to top by their weights and the result is written to the objects.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use glam::{Quat, Vec3};
use hecs::Entity;

use super::binding;
use super::clip::{AnimationClip, Property};
use super::controller::{
    AnimatorController, BlendTree, BlendType, LayerBlending, ParamKind, State,
};
use super::fsm::{self, LayerRun, Lengths, Params, Run};
use super::mask::AvatarMask;
use super::root_motion::{self, Delta};
use crate::scene::{Animator, Scene};

pub use super::fsm::exit_time_reached;

type Key = (Entity, Property);
/// Sampled property values.
type Pose = HashMap<Key, f32>;

/// Length used for states without a clip (seconds).
const EMPTY_LENGTH: f32 = 1.0;

/// Target matching in progress (`MatchTarget`).
#[derive(Clone, Copy, Debug)]
struct MatchTarget {
    position: Vec3,
    rotation: Quat,
    position_weight: Vec3,
    rotation_weight: f32,
    start: f32,
    end: f32,
    /// Base layer state the match belongs to (a state change cancels it).
    state: usize,
    /// Normalised time when the match last moved the object.
    last: f32,
}

/// One Animator's live state machines.
struct Instance {
    path: String,
    ctrl: Arc<AnimatorController>,
    /// Parameter values, in the controller's order (bools and triggers are 0 or 1).
    params: Vec<f32>,
    layers: Vec<LayerRun>,
    /// Root motion of the last update: world-space translation and turn.
    root_delta: (Vec3, Quat),
    /// Pose of an interrupted blend per layer, sampled once when it froze.
    frozen_poses: Vec<Option<Pose>>,
    matching: Option<MatchTarget>,
}

/// What the Animator window and `GetCurrentAnimatorStateInfo` show.
#[derive(Clone, Debug, PartialEq)]
pub struct StateInfo {
    pub name: String,
    /// "Base Layer.Combat.Attack": layer, sub-state machines and state joined by dots.
    pub full_path: String,
    pub tag: String,
    pub normalized_time: f32,
    /// Seconds of one loop at speed 1.
    pub length: f32,
    pub speed: f32,
    pub looping: bool,
}

/// One layer's live state.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerStatus {
    pub name: String,
    pub weight: f32,
    pub current: Option<StateInfo>,
    /// Destination of the cross-fade in progress and its progress (0..1).
    pub next: Option<(StateInfo, f32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimatorStatus {
    /// Controller asset path.
    pub controller: String,
    /// Base layer.
    pub current: Option<StateInfo>,
    pub next: Option<(StateInfo, f32)>,
    pub params: Vec<(String, ParamKind, f32)>,
    /// Every layer, base layer first.
    pub layers: Vec<LayerStatus>,
}

/// Root motion produced by one update, for the scripting host to apply (or hand to
/// `OnAnimatorMove`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RootMove {
    pub entity: Entity,
    /// World-space translation.
    pub delta_position: Vec3,
    /// Turn, applied as `rotation * delta_rotation`.
    pub delta_rotation: Quat,
    /// A script on the object handles it (`OnAnimatorMove`).
    pub by_script: bool,
}

/// Controllers, clips and masks read from the project on first use.
struct Assets {
    root: PathBuf,
    controllers: HashMap<String, Option<Arc<AnimatorController>>>,
    clips: HashMap<String, Option<Arc<AnimationClip>>>,
    masks: HashMap<String, Option<Arc<AvatarMask>>>,
    logs: Vec<String>,
}

impl Assets {
    fn controller(&mut self, path: &str) -> Option<Arc<AnimatorController>> {
        if !self.controllers.contains_key(path) {
            let loaded = match AnimatorController::load(&self.root.join(path)) {
                Ok(c) => Some(Arc::new(c)),
                Err(e) => {
                    self.logs
                        .push(format!("Animator Controller failed to load: {e}"));
                    None
                }
            };
            self.controllers.insert(path.to_owned(), loaded);
        }
        self.controllers.get(path).cloned().flatten()
    }

    fn clip(&mut self, path: &str) -> Option<Arc<AnimationClip>> {
        if !self.clips.contains_key(path) {
            let loaded = match AnimationClip::load(&self.root.join(path)) {
                Ok(c) => Some(Arc::new(c)),
                Err(e) => {
                    self.logs
                        .push(format!("Animation clip failed to load: {e}"));
                    None
                }
            };
            self.clips.insert(path.to_owned(), loaded);
        }
        self.clips.get(path).cloned().flatten()
    }

    fn mask(&mut self, path: &str) -> Option<Arc<AvatarMask>> {
        if !self.masks.contains_key(path) {
            let loaded = match AvatarMask::load(&self.root.join(path)) {
                Ok(m) => Some(Arc::new(m)),
                Err(e) => {
                    self.logs.push(format!("Avatar Mask failed to load: {e}"));
                    None
                }
            };
            self.masks.insert(path.to_owned(), loaded);
        }
        self.masks.get(path).cloned().flatten()
    }
}

/// What a state plays on a layer.
enum Motion<'a> {
    None,
    Clip(&'a str),
    Tree(&'a BlendTree),
}

/// The motion of state `state` of the machine running `layer` (synced layers use their own
/// clip per state).
fn motion(ctrl: &AnimatorController, layer: usize, state: usize) -> Motion<'_> {
    let ml = ctrl.machine_layer(layer);
    let Some(s) = ctrl.machine(ml).and_then(|m| m.states.get(state)) else {
        return Motion::None;
    };
    if ml != layer {
        return ctrl
            .layer(layer)
            .and_then(|l| l.sync_motions.get(&s.name))
            .map_or(Motion::None, |m| Motion::Clip(m));
    }
    match (&s.blend_tree, &s.motion) {
        (Some(t), _) => Motion::Tree(t),
        (None, Some(m)) => Motion::Clip(m),
        _ => Motion::None,
    }
}

fn state_of(ctrl: &AnimatorController, layer: usize, state: usize) -> Option<&State> {
    ctrl.machine(ctrl.machine_layer(layer))
        .and_then(|m| m.states.get(state))
}

fn param_value(ctrl: &AnimatorController, params: &[f32], name: &str) -> f32 {
    ctrl.parameters
        .iter()
        .position(|p| p.name == name)
        .and_then(|i| params.get(i).copied())
        .unwrap_or(0.0)
}

/// Clips of a motion with their weights and time scales.
fn clips<'a>(
    ctrl: &AnimatorController,
    params: &[f32],
    m: &Motion<'a>,
) -> Vec<(&'a str, f32, f32)> {
    match m {
        Motion::None => Vec::new(),
        Motion::Clip(c) => vec![(c, 1.0, 1.0)],
        Motion::Tree(t) => t
            .weights(&|n| param_value(ctrl, params, n))
            .into_iter()
            .filter_map(|(i, w)| {
                let c = &t.children[i];
                c.motion
                    .as_deref()
                    .map(|m| (m, w, c.time_scale.abs().max(1e-3)))
            })
            .collect(),
    }
}

/// Clip length of a state (blend trees: weighted by the parameters, divided by time scales).
fn state_length(
    assets: &mut Assets,
    ctrl: &AnimatorController,
    params: &[f32],
    layer: usize,
    state: usize,
) -> f32 {
    let m = motion(ctrl, layer, state);
    let parts = clips(ctrl, params, &m);
    let (mut len, mut wsum) = (0.0, 0.0);
    for (c, w, ts) in parts {
        len += w * assets.clip(c).map_or(EMPTY_LENGTH, |c| c.length) / ts;
        wsum += w;
    }
    let len = if wsum > 0.0 { len / wsum } else { 0.0 };
    if len > 1e-4 { len } else { EMPTY_LENGTH }
}

/// Playback speed of a state (speed × its multiplier parameter).
fn state_speed(ctrl: &AnimatorController, params: &[f32], layer: usize, state: usize) -> f32 {
    state_of(ctrl, layer, state).map_or(1.0, |s| {
        let k = if s.speed_parameter.is_empty() {
            1.0
        } else {
            param_value(ctrl, params, &s.speed_parameter)
        };
        s.speed * k
    })
}

/// Clip time for a normalised time: wraps when looping, otherwise holds the ends.
fn clip_time(norm: f32, length: f32, looping: bool) -> f32 {
    if looping {
        norm.rem_euclid(1.0) * length
    } else {
        norm.clamp(0.0, 1.0) * length
    }
}

/// How root keys are handled while sampling.
#[derive(Clone, Copy)]
struct RootSpec<'a> {
    path: &'a str,
}

/// What a sample reads from.
struct SampleCtx<'a> {
    scene: &'a Scene,
    root: Entity,
    ctrl: &'a AnimatorController,
    params: &'a [f32],
    mask: Option<&'a AvatarMask>,
    root_motion: Option<RootSpec<'a>>,
}

/// Property values of one run of a layer (blend trees mix their children; a value missing in
/// one child keeps the others' weighted mean). `at_start` samples the first frame instead
/// (the reference pose of additive layers).
fn sample_run(
    assets: &mut Assets,
    cx: &SampleCtx,
    layer: usize,
    run: Run,
    at_start: bool,
    rest: &mut dyn FnMut(Key) -> f32,
) -> Pose {
    let looping = state_of(cx.ctrl, layer, run.state).is_none_or(|s| s.looping);
    let m = motion(cx.ctrl, layer, run.state);
    let direct_unnormalized =
        matches!(m, Motion::Tree(t) if t.blend_type == BlendType::Direct && !t.normalize);
    let parts = clips(cx.ctrl, cx.params, &m);
    let mut seen: Vec<(Pose, f32)> = Vec::new();
    for (path, w, _) in parts {
        let Some(clip) = assets.clip(path) else {
            continue;
        };
        let t = if at_start {
            0.0
        } else {
            clip_time(run.norm, clip.length, looping)
        };
        let mut pose = HashMap::new();
        for curve in clip.curves.iter().filter(|c| !c.keys.is_empty()) {
            if cx
                .mask
                .is_some_and(|m| !m.allows(&curve.path, curve.property))
            {
                continue;
            }
            let mut value = None;
            if let Some(rs) = cx.root_motion
                && root_motion::is_root_key(rs.path, &curve.path, curve.property)
            {
                match root_motion::pinned_value(&clip, rs.path, curve.property) {
                    Some(v) => value = Some(v),
                    None => continue,
                }
            }
            if let Some(target) = binding::resolve(cx.scene, cx.root, &curve.path) {
                pose.insert(
                    (target, curve.property),
                    value.unwrap_or_else(|| curve.evaluate(t)),
                );
            }
        }
        seen.push((pose, w));
    }
    let mut out = HashMap::new();
    let total: f32 = seen.iter().map(|(_, w)| w).sum();
    for (pose, _) in &seen {
        for k in pose.keys() {
            if out.contains_key(k) {
                continue;
            }
            let (sum, wsum) = seen
                .iter()
                .filter_map(|(p, w)| p.get(k).map(|v| (v * w, *w)))
                .fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
            if wsum <= 0.0 {
                continue;
            }
            let v = if direct_unnormalized {
                // Direct weights that don't add up to 1 leave the rest to the rest pose; above 1
                // they are scaled down.
                if total < 1.0 {
                    sum + (1.0 - total) * rest(*k)
                } else {
                    sum / total
                }
            } else {
                sum / wsum
            };
            out.insert(*k, v);
        }
    }
    out
}

/// Weighted blend of poses; a value a pose lacks counts as its rest value.
fn blend(poses: &[(&Pose, f32)], rest: &mut dyn FnMut(Key) -> f32) -> Pose {
    let mut keys: Vec<Key> = poses.iter().flat_map(|(p, _)| p.keys().copied()).collect();
    keys.sort_by_key(|(e, p)| (e.to_bits(), *p));
    keys.dedup();
    let mut out = HashMap::new();
    for k in keys {
        let r = rest(k);
        let v: f32 = poses
            .iter()
            .map(|(p, w)| w * p.get(&k).copied().unwrap_or(r))
            .sum();
        out.insert(k, v);
    }
    out
}

/// A layer's blend sources this frame: (run, weight) pairs, plus the frozen side's weight.
fn sources(lr: &LayerRun) -> (Vec<(Run, f32)>, f32) {
    match (lr.current, lr.next) {
        (Some(c), Some(n)) => {
            let w = n.progress();
            if lr.frozen.is_empty() {
                (vec![(c, 1.0 - w), (n.to, w)], 0.0)
            } else {
                (vec![(n.to, w)], 1.0 - w)
            }
        }
        (Some(c), None) => (vec![(c, 1.0)], 0.0),
        _ => (Vec::new(), 0.0),
    }
}

pub struct AnimatorPlayer {
    assets: Assets,
    instances: HashMap<Entity, Instance>,
    /// Values animated properties had before the Animator first wrote them ("write defaults").
    defaults: HashMap<Key, f32>,
    /// Objects whose scripts define `OnAnimatorMove` (set by the scripting host each frame).
    script_moves: HashSet<Entity>,
    /// Root motion of the last update.
    moves: Vec<RootMove>,
}

impl AnimatorPlayer {
    /// Controllers, clips and masks are read from `root` (the project folder) on first use.
    pub fn new(root: PathBuf) -> Self {
        Self {
            assets: Assets {
                root,
                controllers: HashMap::new(),
                clips: HashMap::new(),
                masks: HashMap::new(),
                logs: Vec::new(),
            },
            instances: HashMap::new(),
            defaults: HashMap::new(),
            script_moves: HashSet::new(),
            moves: Vec::new(),
        }
    }

    /// Use an in-memory controller for `path` instead of reading the file (tests, previews).
    pub fn insert_controller(&mut self, path: &str, c: AnimatorController) {
        self.assets
            .controllers
            .insert(path.to_owned(), Some(Arc::new(c)));
    }

    /// Use an in-memory clip for `path` instead of reading the file.
    pub fn insert_clip(&mut self, path: &str, c: AnimationClip) {
        self.assets.clips.insert(path.to_owned(), Some(Arc::new(c)));
    }

    /// Use an in-memory avatar mask for `path` instead of reading the file.
    pub fn insert_mask(&mut self, path: &str, m: AvatarMask) {
        self.assets.masks.insert(path.to_owned(), Some(Arc::new(m)));
    }

    /// The live instance of `e`'s Animator, created in its default states on first use.
    fn instance(&mut self, scene: &Scene, e: Entity) -> Option<&mut Instance> {
        let path = scene
            .world
            .get::<&Animator>(e)
            .ok()
            .and_then(|a| a.controller.clone())?;
        if self.instances.get(&e).is_some_and(|i| i.path != path) {
            self.instances.remove(&e);
        }
        if !self.instances.contains_key(&e) {
            let ctrl = self.assets.controller(&path)?;
            let mut params: Vec<f32> = ctrl.parameters.iter().map(|p| p.default).collect();
            let n = ctrl.layer_count();
            let layers = (0..n)
                .map(|i| {
                    let synced = ctrl.machine_layer(i) != i;
                    let current = ctrl.machine(i).filter(|_| !synced).and_then(|m| {
                        let p = Params {
                            ctrl: &ctrl,
                            values: &mut params,
                        };
                        fsm::enter(m, "", &p, &mut Vec::new(), 0)
                    });
                    LayerRun {
                        current: current.map(|state| Run { state, norm: 0.0 }),
                        fresh: true,
                        weight: ctrl.layer(i).map_or(1.0, |l| l.weight),
                        ..Default::default()
                    }
                })
                .collect();
            self.instances.insert(
                e,
                Instance {
                    path,
                    ctrl,
                    params,
                    layers,
                    root_delta: (Vec3::ZERO, Quat::IDENTITY),
                    frozen_poses: vec![None; n],
                    matching: None,
                },
            );
        }
        self.instances.get_mut(&e)
    }

    /// Run `f` on `e`'s instance, or warn that it has no controller.
    fn with<R>(
        &mut self,
        scene: &Scene,
        e: Entity,
        f: impl FnOnce(&mut Instance, &mut Vec<String>) -> R,
    ) -> Option<R> {
        let mut logs = Vec::new();
        let r = match self.instance(scene, e) {
            Some(inst) => Some(f(inst, &mut logs)),
            None => {
                logs.push(format!(
                    "Animator on '{}' is not playing an Animator Controller.",
                    if scene.contains(e) {
                        scene.name(e)
                    } else {
                        "null".into()
                    }
                ));
                None
            }
        };
        self.assets.logs.extend(logs);
        r
    }

    // ---- parameters ------------------------------------------------------------------------

    /// Set a parameter, checking it exists and has one of the `kinds`.
    pub fn set_param(
        &mut self,
        scene: &Scene,
        e: Entity,
        name: &str,
        kinds: &[ParamKind],
        value: f32,
    ) {
        self.with(scene, e, |inst, logs| {
            if let Some(i) = param_index(inst, name, kinds, logs) {
                inst.params[i] = value;
            }
        });
    }

    /// `SetFloat(name, value, dampTime, deltaTime)`: move the float `dt / (damp + dt)` of the
    /// way to `value`, so repeated calls ease into it over about `damp` seconds.
    pub fn set_float_damped(
        &mut self,
        scene: &Scene,
        e: Entity,
        name: &str,
        value: f32,
        damp: f32,
        dt: f32,
    ) {
        self.with(scene, e, |inst, logs| {
            if let Some(i) = param_index(inst, name, FLOAT, logs) {
                inst.params[i] = damped(inst.params[i], value, damp, dt);
            }
        });
    }

    /// A parameter's value (0 and a warning if it does not exist).
    pub fn param(&mut self, scene: &Scene, e: Entity, name: &str, kinds: &[ParamKind]) -> f32 {
        self.with(scene, e, |inst, logs| {
            param_index(inst, name, kinds, logs).map_or(0.0, |i| inst.params[i])
        })
        .unwrap_or(0.0)
    }

    // ---- layers ----------------------------------------------------------------------------

    pub fn layer_count(&mut self, scene: &Scene, e: Entity) -> usize {
        self.instance(scene, e).map_or(0, |i| i.layers.len())
    }

    /// Layer name, or "" when out of range.
    pub fn layer_name(&mut self, scene: &Scene, e: Entity, layer: usize) -> String {
        self.instance(scene, e)
            .map(|i| i.ctrl.layer_name(layer).to_owned())
            .unwrap_or_default()
    }

    /// Index of the layer called `name` (-1 when none).
    pub fn layer_index(&mut self, scene: &Scene, e: Entity, name: &str) -> i64 {
        self.instance(scene, e)
            .and_then(|i| i.ctrl.layer_index(name))
            .map_or(-1, |i| i as i64)
    }

    /// `SetLayerWeight`: the base layer always stays at 1.
    pub fn set_layer_weight(&mut self, scene: &Scene, e: Entity, layer: usize, w: f32) -> bool {
        self.with(scene, e, |inst, logs| match inst.layers.get_mut(layer) {
            Some(_) if layer == 0 => true,
            Some(l) => {
                l.weight = w.clamp(0.0, 1.0);
                true
            }
            None => {
                logs.push(format!("Animator layer index {layer} is out of bounds."));
                false
            }
        })
        .unwrap_or(false)
    }

    pub fn layer_weight(&mut self, scene: &Scene, e: Entity, layer: usize) -> f32 {
        self.instance(scene, e)
            .and_then(|i| i.layers.get(layer).map(|l| l.weight))
            .unwrap_or(0.0)
    }

    // ---- playback --------------------------------------------------------------------------

    /// `Play(state, layer, normalizedTime)`: jump to a state (cancelling any cross-fade).
    /// Without `norm`, a state that is already playing continues; otherwise it starts at `norm`.
    /// `layer` -1 searches every layer.
    pub fn play(
        &mut self,
        scene: &Scene,
        e: Entity,
        state: &str,
        layer: i64,
        norm: Option<f32>,
    ) -> bool {
        self.with(scene, e, |inst, logs| {
            let Some((l, i)) = find_state(&inst.ctrl, state, layer, logs) else {
                return false;
            };
            let lr = &mut inst.layers[inst.ctrl.machine_layer(l)];
            lr.next = None;
            lr.frozen.clear();
            let keep = norm.is_none() && lr.current.is_some_and(|r| r.state == i);
            if !keep {
                lr.current = Some(Run {
                    state: i,
                    norm: norm.unwrap_or(0.0),
                });
            }
            true
        })
        .unwrap_or(false)
    }

    /// `CrossFade` / `CrossFadeInFixedTime`: blend to a state over `duration` (seconds when
    /// `fixed`, else lengths of the current state), starting it at `offset`. A blend already
    /// in progress is frozen and blended out.
    #[allow(clippy::too_many_arguments)]
    pub fn cross_fade(
        &mut self,
        scene: &Scene,
        e: Entity,
        state: &str,
        duration: f32,
        fixed: bool,
        layer: i64,
        offset: f32,
    ) -> bool {
        let Some(inst) = self.instance(scene, e) else {
            self.with(scene, e, |_, _| ());
            return false;
        };
        let ctrl = inst.ctrl.clone();
        let mut logs = Vec::new();
        let Some((l, i)) = find_state(&ctrl, state, layer, &mut logs) else {
            self.assets.logs.extend(logs);
            return false;
        };
        let ml = ctrl.machine_layer(l);
        let (params, cur) = (inst.params.clone(), inst.layers[ml].current);
        let src_len = cur.map_or(EMPTY_LENGTH, |r| {
            state_length(&mut self.assets, &ctrl, &params, ml, r.state)
        });
        let seconds = if fixed { duration } else { duration * src_len };
        let inst = self.instances.get_mut(&e).expect("instance exists");
        let lr = &mut inst.layers[ml];
        if lr.next.is_some() {
            fsm::interrupt(lr, i, seconds, offset, None, true);
        } else {
            fsm::start(lr, i, seconds, offset, None);
        }
        inst.frozen_poses[ml] = None;
        true
    }

    /// Current state, cross-fade and parameters of every layer (scripts, the Animator window).
    pub fn status(&mut self, scene: &Scene, e: Entity) -> Option<AnimatorStatus> {
        let inst = self.instance(scene, e)?;
        let (ctrl, params) = (inst.ctrl.clone(), inst.params.clone());
        let runs: Vec<(LayerRun, f32)> = (0..ctrl.layer_count())
            .map(|l| {
                (
                    inst.layers[ctrl.machine_layer(l)].clone(),
                    inst.layers[l].weight,
                )
            })
            .collect();
        let path = inst.path.clone();
        let mut info = |l: usize, r: Run| {
            let ml = ctrl.machine_layer(l);
            let s = &ctrl.machine(ml).expect("layer exists").states[r.state];
            let mut full = vec![ctrl.layer_name(l).to_owned()];
            full.extend(
                s.machine
                    .split('/')
                    .filter(|p| !p.is_empty())
                    .map(str::to_owned),
            );
            full.push(s.name.clone());
            StateInfo {
                name: s.name.clone(),
                full_path: full.join("."),
                tag: s.tag.clone(),
                normalized_time: r.norm,
                length: state_length(&mut self.assets, &ctrl, &params, l, r.state),
                speed: state_speed(&ctrl, &params, l, r.state),
                looping: s.looping,
            }
        };
        let layers: Vec<LayerStatus> = runs
            .iter()
            .enumerate()
            .map(|(l, (lr, w))| LayerStatus {
                name: ctrl.layer_name(l).to_owned(),
                weight: *w,
                current: lr.current.map(|r| info(l, r)),
                next: lr.next.map(|c| (info(l, c.to), c.progress())),
            })
            .collect();
        Some(AnimatorStatus {
            controller: path,
            current: layers[0].current.clone(),
            next: layers[0].next.clone(),
            params: ctrl
                .parameters
                .iter()
                .zip(&params)
                .map(|(p, v)| (p.name.clone(), p.kind, *v))
                .collect(),
            layers,
        })
    }

    // ---- root motion & target matching -------------------------------------------------------

    /// Objects whose scripts handle root motion themselves (`OnAnimatorMove`).
    pub fn set_script_moves(&mut self, entities: HashSet<Entity>) {
        self.script_moves = entities;
    }

    /// Root motion of the last update (world translation, turn).
    pub fn root_delta(&self, e: Entity) -> (Vec3, Quat) {
        self.instances
            .get(&e)
            .map_or((Vec3::ZERO, Quat::IDENTITY), |i| i.root_delta)
    }

    /// Root motion produced by the last update.
    pub fn take_moves(&mut self) -> Vec<RootMove> {
        std::mem::take(&mut self.moves)
    }

    /// `MatchTarget` for the root: between normalised times `start` and `end` of the current
    /// base layer state, move the object so it reaches `position` / `rotation` at `end`
    /// (per-axis position weights, rotation weight). Returns false if nothing plays.
    #[allow(clippy::too_many_arguments)]
    pub fn match_target(
        &mut self,
        scene: &Scene,
        e: Entity,
        position: Vec3,
        rotation: Quat,
        position_weight: Vec3,
        rotation_weight: f32,
        start: f32,
        end: f32,
    ) -> bool {
        self.with(scene, e, |inst, logs| {
            let Some(cur) = inst.layers[0].current else {
                return false;
            };
            if inst.layers[0].next.is_some() {
                logs.push("MatchTarget: Calling match target during a transition.".into());
                return false;
            }
            let norm = cur.norm.fract();
            inst.matching = Some(MatchTarget {
                position,
                rotation,
                position_weight,
                rotation_weight,
                start,
                end: end.max(start + 1e-4),
                state: cur.state,
                last: norm.max(start),
            });
            true
        })
        .unwrap_or(false)
    }

    pub fn is_matching_target(&self, e: Entity) -> bool {
        self.instances.get(&e).is_some_and(|i| i.matching.is_some())
    }

    /// `InterruptMatchTarget(completeMatch)`: stop matching, optionally snapping to the target.
    pub fn interrupt_match_target(&mut self, scene: &mut Scene, e: Entity, complete: bool) {
        let Some(m) = self.instances.get_mut(&e).and_then(|i| i.matching.take()) else {
            return;
        };
        if complete {
            move_towards(scene, e, &m, 1.0);
        }
    }

    /// Move objects that are matching a target (after root motion was applied).
    pub fn apply_match_targets(&mut self, scene: &mut Scene) {
        let mut done = Vec::new();
        for (e, inst) in &mut self.instances {
            let Some(m) = inst.matching.as_mut() else {
                continue;
            };
            let Some(cur) = inst.layers[0].current else {
                continue;
            };
            if cur.state != m.state || !scene.contains(*e) {
                done.push(*e);
                continue;
            }
            let norm = cur.norm.fract().max(cur.norm.min(1.0));
            let norm = if cur.norm >= 1.0 && norm < m.last {
                m.end
            } else {
                norm
            };
            if norm < m.start {
                continue;
            }
            let from = m.last.max(m.start);
            let f = if norm >= m.end {
                1.0
            } else {
                ((norm - from) / (m.end - from)).clamp(0.0, 1.0)
            };
            let snapshot = *m;
            m.last = norm;
            move_towards(scene, *e, &snapshot, f);
            if norm >= m.end {
                done.push(*e);
            }
        }
        for e in done {
            if let Some(i) = self.instances.get_mut(&e) {
                i.matching = None;
            }
        }
    }

    // ---- frame update ----------------------------------------------------------------------

    /// Advance every enabled Animator by `dt`: time, transitions, then the blended pose.
    /// Returns the animation events passed this frame as (object, function name); root motion
    /// is collected for [`take_moves`](Self::take_moves).
    pub fn update(&mut self, scene: &mut Scene, dt: f32) -> Vec<(Entity, String)> {
        self.instances.retain(|e, _| scene.contains(*e));
        self.defaults.retain(|(e, _), _| scene.contains(*e));
        self.moves.clear();
        let animators: Vec<(Entity, Animator)> = scene
            .world
            .query::<(Entity, &Animator)>()
            .iter()
            .map(|(e, a)| (e, a.clone()))
            .collect();
        let mut events = Vec::new();
        for (e, a) in animators {
            if !a.enabled || !scene.active_in_hierarchy(e) {
                if !a.keep_state_on_disable {
                    self.instances.remove(&e);
                }
                continue;
            }
            self.step(scene, e, &a, dt * a.speed, &mut events);
        }
        events
    }

    fn step(
        &mut self,
        scene: &mut Scene,
        e: Entity,
        a: &Animator,
        dt: f32,
        events: &mut Vec<(Entity, String)>,
    ) {
        let Some(inst) = self.instance(scene, e) else {
            return;
        };
        let ctrl = inst.ctrl.clone();
        let n = ctrl.layer_count();
        let script = self.script_moves.contains(&e);
        let root_spec = (a.apply_root_motion || script).then_some(RootSpec {
            path: a.root_motion_bone.as_str(),
        });
        // (layer, run, previous time, weight in the layer's blend) of the clips that played.
        let mut passed: Vec<(usize, Run, f32, f32)> = Vec::new();
        for l in 0..n {
            if ctrl.machine_layer(l) != l {
                continue;
            }
            let params = self.instances[&e].params.clone();
            let lr = &self.instances[&e].layers[l];
            let (Some(cur), next) = (lr.current, lr.next) else {
                continue;
            };
            let len_cur = state_length(&mut self.assets, &ctrl, &params, l, cur.state);
            let len_next = next.map_or(EMPTY_LENGTH, |c| {
                state_length(&mut self.assets, &ctrl, &params, l, c.to.state)
            });
            let speed_cur = state_speed(&ctrl, &params, l, cur.state);
            let speed_next = next.map_or(1.0, |c| state_speed(&ctrl, &params, l, c.to.state));
            let inst = self.instances.get_mut(&e).expect("instance exists");
            let lr = &mut inst.layers[l];
            let mut cur = cur;
            let (prev_cur, mut prev_next) = (cur.norm, 0.0);
            if lr.fresh {
                lr.fresh = false;
                passed.push((l, cur, cur.norm - f32::EPSILON, 1.0));
            } else {
                cur.norm += dt * speed_cur / len_cur;
                let w = lr.next.map_or(1.0, |c| 1.0 - c.progress());
                if lr.frozen.is_empty() {
                    passed.push((l, cur, prev_cur, w));
                }
                if let Some(c) = lr.next.as_mut() {
                    prev_next = c.to.norm;
                    c.to.norm += dt * speed_next / len_next;
                    c.elapsed += dt.abs();
                    passed.push((l, c.to, prev_next, 1.0 - w));
                }
            }
            lr.current = Some(cur);
            let (mut prev, mut lengths) = (
                prev_cur,
                Lengths {
                    current: len_cur,
                    next: len_next,
                },
            );
            // A finished cross-fade hands over to its destination.
            if let Some(c) = lr.next
                && c.elapsed >= c.duration
            {
                lr.current = Some(c.to);
                lr.next = None;
                lr.frozen.clear();
                inst.frozen_poses[l] = None;
                prev = prev_next;
                lengths.current = len_next;
            }
            let mut p = Params {
                ctrl: &ctrl,
                values: &mut inst.params,
            };
            let m = ctrl.machine(l).expect("layer exists");
            if fsm::evaluate(m, lr, prev, prev_next, lengths, &mut p) {
                inst.frozen_poses[l] = None;
            }
        }

        // Root motion of the base layer's clips.
        if let Some(rs) = root_spec {
            let params = self.instances[&e].params.clone();
            let mut delta = Delta::default();
            for &(l, run, prev, w) in passed.iter().filter(|p| p.0 == 0) {
                let looping = state_of(&ctrl, l, run.state).is_none_or(|s| s.looping);
                let m = motion(&ctrl, l, run.state);
                for (path, cw, _) in clips(&ctrl, &params, &m) {
                    if let Some(clip) = self.assets.clip(path) {
                        let d = root_motion::clip_delta(&clip, rs.path, prev, run.norm, looping);
                        delta.position += d.position * (w * cw);
                        delta.yaw += d.yaw * (w * cw);
                    }
                }
            }
            let (world_pos, turn) = world_root_delta(scene, e, rs.path, delta);
            let inst = self.instances.get_mut(&e).expect("instance exists");
            inst.root_delta = (world_pos, turn);
            self.moves.push(RootMove {
                entity: e,
                delta_position: world_pos,
                delta_rotation: turn,
                by_script: script,
            });
        } else if let Some(inst) = self.instances.get_mut(&e) {
            inst.root_delta = (Vec3::ZERO, Quat::IDENTITY);
        }

        self.apply(scene, e, root_spec);

        // Events of the clips that played this frame (layers with weight; a blend tree fires
        // the events of its heaviest child).
        let params = self.instances[&e].params.clone();
        let weights: Vec<f32> = self.instances[&e].layers.iter().map(|l| l.weight).collect();
        for (l, run, prev, _) in passed {
            if l > 0 && weights[l] <= 0.0 {
                continue;
            }
            let looping = state_of(&ctrl, l, run.state).is_none_or(|s| s.looping);
            let m = motion(&ctrl, l, run.state);
            let Some((path, _, _)) = clips(&ctrl, &params, &m)
                .into_iter()
                .max_by(|a, b| a.1.total_cmp(&b.1))
            else {
                continue;
            };
            let Some(clip) = self.assets.clip(path) else {
                continue;
            };
            let len = clip.length.max(1e-4);
            for ev in &clip.events {
                let crossed = if looping {
                    super::player::crossed(prev * len, run.norm * len, ev.time, len, true)
                } else {
                    let (p, c) = (prev.min(1.0) * len, run.norm.min(1.0) * len);
                    c > p && super::player::crossed(p, c, ev.time, len, false)
                };
                if crossed {
                    events.push((e, ev.function.clone()));
                }
            }
        }
    }

    /// Sample every layer, combine them and write the result. Properties the Animator animated
    /// before but not now go back to their rest values.
    fn apply(&mut self, scene: &mut Scene, e: Entity, root_spec: Option<RootSpec>) {
        let Some(inst) = self.instances.get(&e) else {
            return;
        };
        let ctrl = inst.ctrl.clone();
        let params = inst.params.clone();
        let runs: Vec<LayerRun> = inst.layers.clone();
        let mut frozen_poses = inst.frozen_poses.clone();
        let Self {
            assets, defaults, ..
        } = self;
        let mut rest = |k: Key| -> f32 {
            if let Some(v) = defaults.get(&k) {
                return *v;
            }
            let v = binding::read(scene, k.0, k.1).unwrap_or(0.0);
            defaults.insert(k, v);
            v
        };
        let mut result: Option<Pose> = None;
        for l in 0..ctrl.layer_count() {
            let weight = if l == 0 { 1.0 } else { runs[l].weight };
            if weight <= 0.0 {
                continue;
            }
            let mask = ctrl
                .layer(l)
                .and_then(|x| x.mask.clone())
                .and_then(|m| assets.mask(&m));
            let ml = ctrl.machine_layer(l);
            let lr = &runs[ml];
            let cx = SampleCtx {
                scene,
                root: e,
                ctrl: &ctrl,
                params: &params,
                mask: mask.as_deref(),
                root_motion: root_spec,
            };
            let (live, frozen_w) = sources(lr);
            let mut poses: Vec<(Pose, f32)> = live
                .iter()
                .map(|&(r, w)| (sample_run(assets, &cx, l, r, false, &mut rest), w))
                .collect();
            if frozen_w > 0.0 {
                let fp = match (ml == l, frozen_poses[l].clone()) {
                    (true, Some(p)) => p,
                    _ => {
                        let parts: Vec<(Pose, f32)> = lr
                            .frozen
                            .iter()
                            .map(|&(r, w)| (sample_run(assets, &cx, l, r, false, &mut rest), w))
                            .collect();
                        let refs: Vec<(&Pose, f32)> = parts.iter().map(|(p, w)| (p, *w)).collect();
                        let p = blend(&refs, &mut rest);
                        if ml == l {
                            frozen_poses[l] = Some(p.clone());
                        }
                        p
                    }
                };
                poses.push((fp, frozen_w));
            }
            let refs: Vec<(&Pose, f32)> = poses.iter().map(|(p, w)| (p, *w)).collect();
            let pose = blend(&refs, &mut rest);
            let Some(below) = result.as_mut() else {
                result = Some(pose);
                continue;
            };
            let additive = ctrl
                .layer(l)
                .is_some_and(|x| x.blending == LayerBlending::Additive);
            if additive {
                let mut parts: Vec<(Run, f32)> = live.clone();
                parts.extend(lr.frozen.iter().map(|&(r, w)| (r, w * frozen_w)));
                let reference: Vec<(Pose, f32)> = parts
                    .iter()
                    .map(|&(r, w)| (sample_run(assets, &cx, l, r, true, &mut rest), w))
                    .collect();
                let rrefs: Vec<(&Pose, f32)> = reference.iter().map(|(p, w)| (p, *w)).collect();
                let reference = blend(&rrefs, &mut rest);
                for (k, v) in pose {
                    let base = match below.get(&k) {
                        Some(b) => *b,
                        None => rest(k),
                    };
                    let r = reference.get(&k).copied().unwrap_or(v);
                    below.insert(k, base + weight * (v - r));
                }
            } else {
                for (k, v) in pose {
                    let base = match below.get(&k) {
                        Some(b) => *b,
                        None => rest(k),
                    };
                    below.insert(k, base + (v - base) * weight);
                }
            }
        }
        let result = result.unwrap_or_default();
        if let Some(inst) = self.instances.get_mut(&e) {
            inst.frozen_poses = frozen_poses;
        }
        let mut keys: Vec<Key> = result.keys().copied().collect();
        keys.sort_by_key(|(e, p)| (e.to_bits(), *p));
        for k in &keys {
            binding::write(scene, k.0, k.1, result[k]);
        }
        // The object's own root keys move it now: never put them back to rest values.
        if root_spec.is_some_and(|rs| rs.path.is_empty()) {
            self.defaults
                .retain(|(t, p), _| !(*t == e && root_motion::is_root_key("", "", *p)));
        }
        // Write defaults: what this Animator touched on its objects but no longer animates.
        let below: Vec<(Key, f32)> = self
            .defaults
            .iter()
            .filter(|(k, _)| !result.contains_key(k) && scene.is_ancestor(e, k.0))
            .map(|(k, v)| (*k, *v))
            .collect();
        for ((t, p), v) in below {
            binding::write(scene, t, p, v);
        }
    }

    pub fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.assets.logs)
    }
}

/// First-order damping used by `SetFloat` with a damp time.
pub fn damped(current: f32, target: f32, damp: f32, dt: f32) -> f32 {
    if damp <= 0.0 || dt <= 0.0 {
        return if dt > 0.0 || damp <= 0.0 {
            target
        } else {
            current
        };
    }
    current + (target - current) * (dt / (damp + dt))
}

/// Turn a root movement measured in clip space into a world translation and turn for `e`.
fn world_root_delta(scene: &Scene, e: Entity, root: &str, d: Delta) -> (Vec3, Quat) {
    let rot = scene.world_rotation(e);
    let scale = scene.world_matrix(e).to_scale_rotation_translation().0;
    let mut local = d.position;
    if !root.is_empty()
        && let Some(bone) = binding::resolve(scene, e, root)
        && let Some(parent) = scene.parent(bone)
        && parent != e
    {
        // A bone's curves are in its parent's space: bring them into the object's space.
        let rel = rot.inverse() * scene.world_rotation(parent);
        let rel_scale = scene.world_matrix(parent).to_scale_rotation_translation().0 / scale;
        local = rel * (local * rel_scale);
    }
    (rot * (local * scale), root_motion::yaw_quat(d.yaw))
}

/// Move `e` part of the way (`f`) to a match target.
fn move_towards(scene: &mut Scene, e: Entity, m: &MatchTarget, f: f32) {
    let pos = scene.world_position(e);
    let to = pos + (m.position - pos) * m.position_weight * f;
    scene.set_world_position(e, to);
    if m.rotation_weight > 0.0 {
        let rot = scene.world_rotation(e);
        scene.set_world_rotation(e, rot.slerp(m.rotation, (m.rotation_weight * f).min(1.0)));
    }
}

fn param_index(
    inst: &Instance,
    name: &str,
    kinds: &[ParamKind],
    logs: &mut Vec<String>,
) -> Option<usize> {
    let Some(i) = inst.ctrl.parameters.iter().position(|p| p.name == name) else {
        logs.push(format!("Parameter '{name}' does not exist."));
        return None;
    };
    let kind = inst.ctrl.parameters[i].kind;
    if !kinds.contains(&kind) {
        logs.push(format!("Parameter '{name}' is a {}.", kind.label()));
        return None;
    }
    Some(i)
}

/// A state by name on `layer` (-1 = the first layer that has it). Dotted paths also work:
/// "Base Layer.Idle", "Upper Body.Combat.Wave" (layer name first, state name last).
fn find_state(
    ctrl: &AnimatorController,
    name: &str,
    layer: i64,
    logs: &mut Vec<String>,
) -> Option<(usize, usize)> {
    let (mut layer, short) = (layer, name);
    if let Some((first, _)) = name.split_once('.')
        && let Some(l) = ctrl.layer_index(first)
    {
        layer = l as i64;
    }
    let short = short.rsplit('.').next().unwrap_or(short);
    let layers: Vec<usize> = if layer < 0 {
        (0..ctrl.layer_count()).collect()
    } else {
        vec![layer as usize]
    };
    let found = layers.into_iter().find_map(|l| {
        let ml = ctrl.machine_layer(l);
        ctrl.machine(ml)
            .and_then(|m| m.state_index(short))
            .map(|i| (l, i))
    });
    if found.is_none() {
        logs.push(format!("Animator.Play: State '{name}' could not be found."));
    }
    found
}

/// Lua `SetTrigger` / `ResetTrigger` and friends accept these kinds.
pub const FLOAT: &[ParamKind] = &[ParamKind::Float];
pub const INT: &[ParamKind] = &[ParamKind::Int];
pub const BOOL: &[ParamKind] = &[ParamKind::Bool];
pub const TRIGGER: &[ParamKind] = &[ParamKind::Trigger];
