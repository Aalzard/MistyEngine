//! Play-mode evaluation of Animator components: parameters, the current state, transitions with
//! exit times and conditions, and cross-fades that blend the sampled property values.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use hecs::Entity;

use super::binding;
use super::clip::{AnimationClip, Property};
use super::controller::{AnimatorController, Node, ParamKind, State, Transition};
use crate::scene::{Animator, Scene};

/// Sampled property values of one state.
type Pose = HashMap<(Entity, Property), f32>;

/// Length used for states without a clip (seconds).
const EMPTY_LENGTH: f32 = 1.0;

/// A state being played: its index and normalised time (1 = one clip length, keeps growing).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Run {
    state: usize,
    norm: f32,
}

#[derive(Clone, Copy, Debug)]
struct Crossfade {
    to: Run,
    elapsed: f32,
    duration: f32,
}

/// One Animator's live state machine.
struct Instance {
    path: String,
    ctrl: Arc<AnimatorController>,
    /// Parameter values, in the controller's order (bools and triggers are 0 or 1).
    params: Vec<f32>,
    current: Option<Run>,
    next: Option<Crossfade>,
    /// Just entered: the next update samples without advancing time.
    fresh: bool,
}

/// What the Animator window and `GetCurrentAnimatorStateInfo` show.
#[derive(Clone, Debug, PartialEq)]
pub struct StateInfo {
    pub name: String,
    pub normalized_time: f32,
    /// Seconds of one loop at speed 1.
    pub length: f32,
    pub speed: f32,
    pub looping: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimatorStatus {
    /// Controller asset path.
    pub controller: String,
    pub current: Option<StateInfo>,
    /// Destination of the cross-fade in progress and its progress (0..1).
    pub next: Option<(StateInfo, f32)>,
    pub params: Vec<(String, ParamKind, f32)>,
}

pub struct AnimatorPlayer {
    root: PathBuf,
    controllers: HashMap<String, Option<Arc<AnimatorController>>>,
    clips: HashMap<String, Option<Arc<AnimationClip>>>,
    instances: HashMap<Entity, Instance>,
    /// Values animated properties had before the Animator first wrote them ("write defaults").
    defaults: HashMap<(Entity, Property), f32>,
    logs: Vec<String>,
}

impl AnimatorPlayer {
    /// Controllers and clips are read from `root` (the project folder) on first use.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            controllers: HashMap::new(),
            clips: HashMap::new(),
            instances: HashMap::new(),
            defaults: HashMap::new(),
            logs: Vec::new(),
        }
    }

    /// Use an in-memory controller for `path` instead of reading the file (tests, previews).
    pub fn insert_controller(&mut self, path: &str, c: AnimatorController) {
        self.controllers.insert(path.to_owned(), Some(Arc::new(c)));
    }

    /// Use an in-memory clip for `path` instead of reading the file.
    pub fn insert_clip(&mut self, path: &str, c: AnimationClip) {
        self.clips.insert(path.to_owned(), Some(Arc::new(c)));
    }

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

    /// The live instance of `e`'s Animator, created in its default state on first use.
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
            let ctrl = self.controller(&path)?;
            let params = ctrl.parameters.iter().map(|p| p.default).collect();
            let current = ctrl
                .default_state
                .as_deref()
                .and_then(|d| ctrl.state_index(d))
                .map(|state| Run { state, norm: 0.0 });
            self.instances.insert(
                e,
                Instance {
                    path,
                    ctrl,
                    params,
                    current,
                    next: None,
                    fresh: true,
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
        self.logs.extend(logs);
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

    /// A parameter's value (0 and a warning if it does not exist).
    pub fn param(&mut self, scene: &Scene, e: Entity, name: &str, kinds: &[ParamKind]) -> f32 {
        self.with(scene, e, |inst, logs| {
            param_index(inst, name, kinds, logs).map_or(0.0, |i| inst.params[i])
        })
        .unwrap_or(0.0)
    }

    // ---- playback --------------------------------------------------------------------------

    /// `Play(state)`: jump to a state (cancelling any cross-fade). Without `norm`, a state that
    /// is already playing continues; otherwise it starts at `norm` (0 = the beginning).
    pub fn play(&mut self, scene: &Scene, e: Entity, state: &str, norm: Option<f32>) -> bool {
        self.with(scene, e, |inst, logs| {
            let Some(i) = find_state(inst, state, logs) else {
                return false;
            };
            inst.next = None;
            let keep = norm.is_none() && inst.current.is_some_and(|r| r.state == i);
            if !keep {
                inst.current = Some(Run {
                    state: i,
                    norm: norm.unwrap_or(0.0),
                });
            }
            true
        })
        .unwrap_or(false)
    }

    /// `CrossFade` / `CrossFadeInFixedTime`: blend to a state over `duration` (seconds when
    /// `fixed`, else lengths of the current state), starting it at `offset`.
    pub fn cross_fade(
        &mut self,
        scene: &Scene,
        e: Entity,
        state: &str,
        duration: f32,
        fixed: bool,
        offset: f32,
    ) -> bool {
        let lengths = self.lengths(scene, e);
        self.with(scene, e, |inst, logs| {
            let Some(i) = find_state(inst, state, logs) else {
                return false;
            };
            let src_len = inst.current.map_or(EMPTY_LENGTH, |r| lengths[r.state]);
            let seconds = if fixed { duration } else { duration * src_len };
            start(inst, i, seconds, offset);
            true
        })
        .unwrap_or(false)
    }

    /// Current state, cross-fade and parameters (for scripts and the Animator window).
    pub fn status(&mut self, scene: &Scene, e: Entity) -> Option<AnimatorStatus> {
        let lengths = self.lengths(scene, e);
        let inst = self.instance(scene, e)?;
        let info = |r: Run| {
            let s = &inst.ctrl.states[r.state];
            StateInfo {
                name: s.name.clone(),
                normalized_time: r.norm,
                length: lengths[r.state],
                speed: s.speed,
                looping: s.looping,
            }
        };
        Some(AnimatorStatus {
            controller: inst.path.clone(),
            current: inst.current.map(info),
            next: inst.next.map(|c| (info(c.to), progress(&c))),
            params: inst
                .ctrl
                .parameters
                .iter()
                .zip(&inst.params)
                .map(|(p, v)| (p.name.clone(), p.kind, *v))
                .collect(),
        })
    }

    /// Clip length of every state of `e`'s controller (blend trees: weighted by the parameter).
    fn lengths(&mut self, scene: &Scene, e: Entity) -> Vec<f32> {
        let Some(inst) = self.instance(scene, e) else {
            return Vec::new();
        };
        let (ctrl, params) = (inst.ctrl.clone(), inst.params.clone());
        ctrl.states
            .iter()
            .map(|s| self.state_length(&ctrl, &params, s))
            .collect()
    }

    fn state_length(&mut self, ctrl: &AnimatorController, params: &[f32], s: &State) -> f32 {
        let len = match &s.blend_tree {
            Some(tree) => {
                let v = blend_value(ctrl, params, &tree.parameter);
                tree.weights(v)
                    .into_iter()
                    .map(|(i, w)| {
                        let clip = tree.children[i]
                            .motion
                            .as_deref()
                            .and_then(|m| self.clip(m));
                        w * clip.map_or(EMPTY_LENGTH, |c| c.length)
                    })
                    .sum()
            }
            None => s
                .motion
                .as_deref()
                .and_then(|m| self.clip(m))
                .map_or(EMPTY_LENGTH, |c| c.length),
        };
        if len > 1e-4 { len } else { EMPTY_LENGTH }
    }

    // ---- frame update ----------------------------------------------------------------------

    /// Advance every enabled Animator by `dt`: time, transitions, then the blended pose.
    /// Returns the animation events passed this frame as (object, function name).
    pub fn update(&mut self, scene: &mut Scene, dt: f32) -> Vec<(Entity, String)> {
        self.instances.retain(|e, _| scene.contains(*e));
        self.defaults.retain(|(e, _), _| scene.contains(*e));
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
            self.step(scene, e, dt * a.speed, &mut events);
        }
        events
    }

    fn step(&mut self, scene: &mut Scene, e: Entity, dt: f32, events: &mut Vec<(Entity, String)>) {
        let lengths = self.lengths(scene, e);
        let Some(inst) = self.instance(scene, e) else {
            return;
        };
        let ctrl = inst.ctrl.clone();
        let Some(mut cur) = inst.current else {
            return;
        };
        let advance = |r: &mut Run| {
            let prev = r.norm;
            r.norm += dt * ctrl.states[r.state].speed / lengths[r.state];
            prev
        };
        let mut passed: Vec<(Run, f32)> = Vec::new();
        if inst.fresh {
            inst.fresh = false;
            passed.push((cur, -f32::EPSILON));
        } else {
            let prev = advance(&mut cur);
            passed.push((cur, prev));
            if let Some(c) = inst.next.as_mut() {
                let prev = advance(&mut c.to);
                passed.push((c.to, prev));
                c.elapsed += dt.abs();
            }
        }
        inst.current = Some(cur);
        // A finished cross-fade hands over to its destination.
        if let Some(c) = inst.next
            && c.elapsed >= c.duration
        {
            inst.current = Some(c.to);
            inst.next = None;
        }
        if inst.next.is_none() {
            let prev = passed[0].1;
            evaluate_transitions(inst, prev, &lengths);
        }
        let inst = &self.instances[&e];
        let mut sources: Vec<(Run, f32)> = Vec::new();
        match (inst.current, inst.next) {
            (Some(c), Some(n)) => {
                let w = progress(&n);
                sources.push((c, 1.0 - w));
                sources.push((n.to, w));
            }
            (Some(c), None) => sources.push((c, 1.0)),
            _ => {}
        }
        let params = inst.params.clone();
        self.apply(scene, e, &ctrl, &params, &sources);
        // Events of the clips that played this frame.
        for (run, prev) in passed {
            let s = &ctrl.states[run.state];
            if s.blend_tree.is_some() {
                continue;
            }
            let Some(clip) = s.motion.as_deref().and_then(|m| self.clip(m)) else {
                continue;
            };
            let len = clip.length.max(1e-4);
            for ev in &clip.events {
                let crossed = if s.looping {
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

    /// Sample every source state, blend by weight (missing values use the rest pose) and write.
    /// Properties the Animator animated before but not now go back to their rest values.
    fn apply(
        &mut self,
        scene: &mut Scene,
        e: Entity,
        ctrl: &AnimatorController,
        params: &[f32],
        sources: &[(Run, f32)],
    ) {
        let mut poses: Vec<(Pose, f32)> = Vec::new();
        for (run, w) in sources {
            let pose = self.sample_state(scene, e, ctrl, params, *run);
            poses.push((pose, *w));
        }
        let mut keys: Vec<(Entity, Property)> =
            poses.iter().flat_map(|(p, _)| p.keys().copied()).collect();
        keys.sort_by_key(|(e, p)| (e.to_bits(), *p));
        keys.dedup();
        for k in &keys {
            if !self.defaults.contains_key(k)
                && let Some(v) = binding::read(scene, k.0, k.1)
            {
                self.defaults.insert(*k, v);
            }
        }
        for k in &keys {
            let rest = self.defaults.get(k).copied().unwrap_or(0.0);
            let v: f32 = poses
                .iter()
                .map(|(p, w)| w * p.get(k).copied().unwrap_or(rest))
                .sum();
            binding::write(scene, k.0, k.1, v);
        }
        // Write defaults: what this Animator touched on its objects but no longer animates.
        let below: Vec<((Entity, Property), f32)> = self
            .defaults
            .iter()
            .filter(|(k, _)| !keys.contains(k) && scene.is_ancestor(e, k.0))
            .map(|(k, v)| (*k, *v))
            .collect();
        for ((t, p), v) in below {
            binding::write(scene, t, p, v);
        }
    }

    /// Property values of one state at its time (blend trees mix their children).
    fn sample_state(
        &mut self,
        scene: &Scene,
        root: Entity,
        ctrl: &AnimatorController,
        params: &[f32],
        run: Run,
    ) -> Pose {
        let s = &ctrl.states[run.state];
        let mut out = HashMap::new();
        let parts: Vec<(Option<String>, f32)> = match &s.blend_tree {
            Some(tree) => {
                let v = blend_value(ctrl, params, &tree.parameter);
                tree.weights(v)
                    .into_iter()
                    .map(|(i, w)| (tree.children[i].motion.clone(), w))
                    .collect()
            }
            None => vec![(s.motion.clone(), 1.0)],
        };
        let mut seen: Vec<Pose> = Vec::new();
        let mut weights = Vec::new();
        for (motion, w) in parts {
            let Some(clip) = motion.as_deref().and_then(|m| self.clip(m)) else {
                continue;
            };
            let t = clip_time(run.norm, clip.length, s.looping);
            let mut pose = HashMap::new();
            for curve in clip.curves.iter().filter(|c| !c.keys.is_empty()) {
                if let Some(target) = binding::resolve(scene, root, &curve.path) {
                    pose.insert((target, curve.property), curve.evaluate(t));
                }
            }
            seen.push(pose);
            weights.push(w);
        }
        // Blend tree children: a value missing in one child keeps the others' weighted mean.
        for (pose, _) in seen.iter().zip(&weights) {
            for k in pose.keys() {
                if out.contains_key(k) {
                    continue;
                }
                let (sum, wsum) = seen
                    .iter()
                    .zip(&weights)
                    .filter_map(|(p, w)| p.get(k).map(|v| (v * w, *w)))
                    .fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
                if wsum > 0.0 {
                    out.insert(*k, sum / wsum);
                }
            }
        }
        out
    }

    pub fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }
}

/// Clip time for a normalised time: wraps when looping, otherwise holds the ends.
fn clip_time(norm: f32, length: f32, looping: bool) -> f32 {
    if looping {
        norm.rem_euclid(1.0) * length
    } else {
        norm.clamp(0.0, 1.0) * length
    }
}

fn progress(c: &Crossfade) -> f32 {
    if c.duration <= 0.0 {
        1.0
    } else {
        (c.elapsed / c.duration).clamp(0.0, 1.0)
    }
}

fn blend_value(ctrl: &AnimatorController, params: &[f32], name: &str) -> f32 {
    ctrl.parameters
        .iter()
        .position(|p| p.name == name)
        .and_then(|i| params.get(i).copied())
        .unwrap_or(0.0)
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

/// A state by name; "Base Layer.Name" also works.
fn find_state(inst: &Instance, name: &str, logs: &mut Vec<String>) -> Option<usize> {
    let short = name.strip_prefix("Base Layer.").unwrap_or(name);
    let i = inst.ctrl.state_index(short);
    if i.is_none() {
        logs.push(format!("Animator.Play: State '{name}' could not be found."));
    }
    i
}

/// Start a cross-fade to `state` (an immediate switch when `seconds` is 0).
fn start(inst: &mut Instance, state: usize, seconds: f32, offset: f32) {
    let to = Run {
        state,
        norm: offset,
    };
    if seconds <= 0.0 || inst.current.is_none() {
        inst.current = Some(to);
        inst.next = None;
    } else {
        inst.next = Some(Crossfade {
            to,
            elapsed: 0.0,
            duration: seconds,
        });
    }
}

/// Whether a transition's exit time allows leaving now. Exit times below 1 repeat every loop:
/// the transition may fire from that point of each loop to its end.
pub fn exit_time_reached(prev: f32, cur: f32, exit: f32, looping: bool) -> bool {
    if cur < exit {
        return false;
    }
    if exit >= 1.0 || !looping {
        return true;
    }
    cur.fract() >= exit || (cur - exit).floor() > (prev - exit).floor()
}

/// Whether every condition holds for the current parameter values.
fn conditions_hold(inst: &Instance, t: &Transition) -> bool {
    t.conditions.iter().all(|c| {
        inst.ctrl
            .parameters
            .iter()
            .position(|p| p.name == c.parameter)
            .is_some_and(|i| c.mode.test(inst.params[i], c.threshold))
    })
}

/// Fire the first transition that can go (Any State ones first). Its triggers are consumed.
fn evaluate_transitions(inst: &mut Instance, prev: f32, lengths: &[f32]) {
    let Some(cur) = inst.current else { return };
    let ctrl = inst.ctrl.clone();
    let here = Node::State(ctrl.states[cur.state].name.clone());
    let candidates = ctrl
        .transitions_from(&Node::AnyState)
        .chain(ctrl.transitions_from(&here));
    for (_, t) in candidates {
        // A transition needs an exit time or a condition (otherwise it never fires).
        if !t.has_exit_time && t.conditions.is_empty() {
            continue;
        }
        let dest = match &t.to {
            Node::State(n) => ctrl.state_index(n),
            Node::Exit => ctrl
                .default_state
                .as_deref()
                .and_then(|d| ctrl.state_index(d)),
            _ => None,
        };
        let Some(dest) = dest else { continue };
        // Any State never re-enters the state that is already playing.
        if t.from == Node::AnyState && dest == cur.state {
            continue;
        }
        let looping = ctrl.states[cur.state].looping;
        if t.has_exit_time && !exit_time_reached(prev, cur.norm, t.exit_time, looping) {
            continue;
        }
        if !conditions_hold(inst, t) {
            continue;
        }
        for c in &t.conditions {
            if let Some(i) = ctrl.parameters.iter().position(|p| p.name == c.parameter)
                && ctrl.parameters[i].kind == ParamKind::Trigger
            {
                inst.params[i] = 0.0;
            }
        }
        let seconds = if t.fixed_duration {
            t.duration
        } else {
            t.duration * lengths[cur.state]
        };
        start(inst, dest, seconds, t.offset);
        return;
    }
}

/// Lua `SetTrigger` / `ResetTrigger` and friends accept these kinds.
pub const FLOAT: &[ParamKind] = &[ParamKind::Float];
pub const INT: &[ParamKind] = &[ParamKind::Int];
pub const BOOL: &[ParamKind] = &[ParamKind::Bool];
pub const TRIGGER: &[ParamKind] = &[ParamKind::Trigger];
