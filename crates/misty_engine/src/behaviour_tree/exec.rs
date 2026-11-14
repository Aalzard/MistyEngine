//! Ticking a compiled tree for one agent.
//!
//! Each agent keeps an [`ExecState`]: one small [`NodeState`] per node (status, current child,
//! counters, timers), the order Random Selectors picked, a random generator and its clock. All
//! of it is sized when the agent starts, so a tick allocates nothing (Log messages and script
//! calls aside).
//!
//! A tick first lets the **observers** react: conditionals with an abort mode whose key changed
//! since they last looked are evaluated. *Self* stops the conditional's own running branch when
//! the condition turned false; *Lower Priority* stops the branch running to its right under the
//! same composite when the condition turned true, and points the composite back at the
//! conditional. Then the running path is walked from the Root: composites resume their current
//! child, finished leaves report back, and new leaves start.
//!
//! Everything the tree does to the world goes through [`Host`], so the same code runs in the
//! game (scripting/behaviour_tree_api.rs) and in tests with a fake world.

use glam::Vec3;
use hecs::Entity;

use super::asset::{LogLevel, ParallelPolicy};
use super::blackboard::{BbValue, Blackboard, SharedBlackboards};
use super::compile::{CKind, Compiled, NO_PARENT, NO_SLOT, Slot};

/// A node's state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Status {
    /// Not started (or reset).
    #[default]
    Idle,
    Running,
    Success,
    Failure,
}

impl Status {
    pub fn finished(self) -> bool {
        matches!(self, Status::Success | Status::Failure)
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Idle => "Idle",
            Status::Running => "Running",
            Status::Success => "Success",
            Status::Failure => "Failure",
        }
    }

    fn from_bool(ok: bool) -> Status {
        if ok { Status::Success } else { Status::Failure }
    }
}

/// When a custom Lua leaf is called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeafPhase {
    Start,
    Tick,
    Abort,
}

/// What the tree can do to the world.
pub trait Host {
    /// Run `f` on the agent's blackboard and the shared ones.
    fn with_bb<R>(&mut self, f: impl FnOnce(&mut Blackboard, &mut SharedBlackboards) -> R) -> R;
    /// The agent's position.
    fn position(&mut self) -> Vec3;
    /// The direction the agent faces (horizontal or not).
    fn forward(&mut self) -> Vec3;
    /// Where `e` is; None when it is gone or inactive.
    fn object_position(&mut self, e: Entity) -> Option<Vec3>;
    /// Start walking (or re-plan) to `dest`; false when no path can be planned.
    fn move_start(&mut self, dest: Vec3) -> bool;
    /// Distance left before the agent stops (its stopping distance taken off; infinite while a
    /// path is being planned) and whether it still has a path.
    fn move_progress(&mut self) -> (f32, bool);
    /// Stop walking.
    fn move_stop(&mut self);
    /// Turn towards `point` by at most `max_degrees` (infinite = at once); returns the angle
    /// left (degrees).
    fn turn_towards(&mut self, point: Vec3, max_degrees: f32) -> f32;
    /// Nothing blocks the line from `from` to `to` (colliders of the agent and of `target` don't
    /// count).
    fn line_clear(&mut self, from: Vec3, to: Vec3, target: Option<Entity>) -> bool;
    fn play_animation(&mut self, state: &str, crossfade: f32) -> bool;
    fn set_animator_parameter(&mut self, name: &str, value: &BbValue) -> bool;
    /// Send the custom event `event` for node `node` (a Run Script Graph).
    fn send_event(&mut self, node: u32, graph: &str, event: &str);
    /// Result reported by `BehaviourTree.Finish` for the running script node, taken.
    fn take_finish(&mut self, node: u32) -> Option<bool>;
    /// Call a Run Lua function.
    fn call_lua(&mut self, node: u32, function: &str) -> Status;
    /// Call a custom Lua leaf.
    fn custom_lua(
        &mut self,
        node: u32,
        phase: LeafPhase,
        script: &str,
        params: &std::collections::BTreeMap<String, String>,
        dt: f32,
    ) -> Status;
    fn log(&mut self, level: LogLevel, text: &str);
}

/// Per-node memory.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NodeState {
    pub status: Status,
    /// Composite: position of the running child. Repeat: rounds done.
    pub child: u32,
    /// Wait / Time Limit: when it ends. Cooldown: until when it refuses.
    pub until: f64,
    /// Conditional: key version last looked at.
    pub seen: u32,
    /// Move To: last destination asked for.
    pub dest: Vec3,
    /// Last finished result and the tick it happened on (debugging).
    pub last: Status,
    pub last_tick: u64,
}

/// One agent's tree memory.
#[derive(Clone, Debug, Default)]
pub struct ExecState {
    pub nodes: Vec<NodeState>,
    /// Random Selector child orders (same layout as [`Compiled::children`]).
    order: Vec<u16>,
    rng: u64,
    /// Seconds the tree has run.
    pub time: f64,
    /// `time` when the current tick began (timers started during a tick count from here).
    start: f64,
    pub ticks: u64,
    /// The Root's last result.
    pub result: Status,
    /// Reused for Log messages.
    text: String,
}

/// Re-plan Move To when an object target moved this far (metres).
const REPLAN_DISTANCE: f32 = 0.5;
/// Look At succeeds within this angle (degrees).
const FACING_DEGREES: f32 = 3.0;

impl ExecState {
    /// Fresh memory for `tree`; `seed` picks the random sequence (same seed, same choices).
    pub fn new(tree: &Compiled, seed: u64) -> Self {
        let mut s = ExecState {
            nodes: vec![NodeState::default(); tree.nodes.len()],
            order: vec![0; tree.children.len()],
            rng: 0,
            time: 0.0,
            start: 0.0,
            ticks: 0,
            result: Status::Idle,
            text: String::with_capacity(64),
        };
        s.reseed(seed);
        s
    }

    pub fn reseed(&mut self, seed: u64) {
        // SplitMix64 of the seed, never zero (xorshift needs a non-zero state).
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        self.rng = (z ^ (z >> 31)) | 1;
    }

    fn random(&mut self) -> f32 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    /// Status of node `i` (Idle when out of range).
    pub fn status(&self, i: usize) -> Status {
        self.nodes.get(i).map_or(Status::Idle, |n| n.status)
    }

    /// Forget everything (the tree starts over on the next tick). Running leaves are stopped.
    pub fn reset<H: Host>(&mut self, tree: &Compiled, h: &mut H) {
        if !tree.nodes.is_empty() {
            self.abort(tree, 0, h);
        }
        for n in &mut self.nodes {
            *n = NodeState {
                last: n.last,
                last_tick: n.last_tick,
                ..Default::default()
            };
        }
        self.result = Status::Idle;
    }

    /// One tick of `dt` seconds; returns the Root's status.
    pub fn tick<H: Host>(&mut self, tree: &Compiled, h: &mut H, dt: f32) -> Status {
        if tree.nodes.is_empty() {
            return Status::Failure;
        }
        self.start = self.time;
        self.time += f64::from(dt.max(0.0));
        self.ticks += 1;
        self.observe(tree, h);
        let r = self.tick_node(tree, 0, h, dt);
        self.result = r;
        r
    }

    /// Event-driven aborts: conditionals whose key changed.
    fn observe<H: Host>(&mut self, tree: &Compiled, h: &mut H) {
        for &o in &tree.observers {
            let o = o as usize;
            let CKind::Conditional {
                key,
                op,
                ref value,
                abort,
            } = tree.nodes[o].kind
            else {
                continue;
            };
            let (version, holds) = h.with_bb(|bb, sh| {
                let v = bb.version(sh, key as usize);
                let holds = bb.get(sh, key as usize).is_some_and(|k| op.eval(k, value));
                (v, holds)
            });
            if version == self.nodes[o].seen {
                continue;
            }
            self.nodes[o].seen = version;
            if abort.aborts_self() && !holds && self.nodes[o].status == Status::Running {
                // Its parent ticks it again below; re-entering, it finds the condition false.
                self.abort(tree, o, h);
            }
            if abort.aborts_lower() && holds && self.nodes[o].status != Status::Running {
                // Climb through decorators to the branch directly under a composite.
                let mut branch = o;
                while tree.nodes[branch].parent != NO_PARENT {
                    let p = tree.nodes[branch].parent as usize;
                    if is_composite(&tree.nodes[p].kind) {
                        break;
                    }
                    branch = p;
                }
                let p = tree.nodes[branch].parent;
                if p == NO_PARENT {
                    continue;
                }
                let p = p as usize;
                if !matches!(
                    tree.nodes[p].kind,
                    CKind::Selector | CKind::Sequence | CKind::RandomSelector
                ) || self.nodes[p].status != Status::Running
                {
                    continue;
                }
                let mine = self.position_in(tree, p, branch);
                let current = self.nodes[p].child as usize;
                if current > mine && current < tree.nodes[p].child_count as usize {
                    let running = self.child_at(tree, p, current);
                    self.abort(tree, running, h);
                    self.nodes[p].child = mine as u32;
                }
            }
        }
    }

    /// Position of child node `c` in `p`'s running order.
    fn position_in(&self, tree: &Compiled, p: usize, c: usize) -> usize {
        let k = tree.nodes[c].index_in_parent as usize;
        if matches!(tree.nodes[p].kind, CKind::RandomSelector) {
            let first = tree.nodes[p].first_child as usize;
            let n = tree.nodes[p].child_count as usize;
            return self.order[first..first + n]
                .iter()
                .position(|x| *x as usize == k)
                .unwrap_or(k);
        }
        k
    }

    /// The child node at running position `k` of `p`.
    fn child_at(&self, tree: &Compiled, p: usize, k: usize) -> usize {
        let k = if matches!(tree.nodes[p].kind, CKind::RandomSelector) {
            self.order[tree.nodes[p].first_child as usize + k] as usize
        } else {
            k
        };
        tree.child(p, k)
    }

    /// Stop node `i` and everything running under it (leaves get their stop calls).
    pub fn abort<H: Host>(&mut self, tree: &Compiled, i: usize, h: &mut H) {
        let end = tree.nodes[i].end as usize;
        for j in i..end {
            if self.nodes[j].status != Status::Running {
                continue;
            }
            let id = tree.nodes[j].id;
            match &tree.nodes[j].kind {
                CKind::MoveTo { .. } => h.move_stop(),
                CKind::CustomLua { script, params } => {
                    h.custom_lua(id, LeafPhase::Abort, script, params, 0.0);
                }
                CKind::RunScriptGraph { .. } => {
                    let _ = h.take_finish(id);
                }
                _ => {}
            }
            self.nodes[j].status = Status::Idle;
            self.nodes[j].last = Status::Failure;
            self.nodes[j].last_tick = self.ticks;
        }
    }

    fn tick_node<H: Host>(&mut self, tree: &Compiled, i: usize, h: &mut H, dt: f32) -> Status {
        let entering = self.nodes[i].status != Status::Running;
        if entering {
            self.enter(tree, i, h);
        }
        let r = self.run(tree, i, h, dt, entering);
        self.nodes[i].status = r;
        if r.finished() {
            self.nodes[i].last = r;
            self.nodes[i].last_tick = self.ticks;
        }
        r
    }

    /// A node starts (again).
    fn enter<H: Host>(&mut self, tree: &Compiled, i: usize, h: &mut H) {
        let n = &tree.nodes[i];
        let st = &mut self.nodes[i];
        st.child = 0;
        match &n.kind {
            CKind::RandomSelector => {
                let first = n.first_child as usize;
                let count = n.child_count as usize;
                for k in 0..count {
                    self.order[first + k] = k as u16;
                }
                // Fisher–Yates.
                for k in (1..count).rev() {
                    let j = ((self.random() * (k + 1) as f32) as usize).min(k);
                    self.order.swap(first + k, first + j);
                }
            }
            CKind::Parallel(_) => {
                for k in 0..n.child_count as usize {
                    let c = tree.child(i, k);
                    self.nodes[c].status = Status::Idle;
                }
            }
            CKind::Wait { seconds, random } => {
                let jitter = if *random > 0.0 {
                    (self.random() * 2.0 - 1.0) * random
                } else {
                    0.0
                };
                self.nodes[i].until = self.start + f64::from((seconds + jitter).max(0.0));
            }
            CKind::TimeLimit(s) => self.nodes[i].until = self.start + f64::from(*s),
            CKind::Conditional { key, .. } => {
                let v = h.with_bb(|bb, sh| bb.version(sh, *key as usize));
                self.nodes[i].seen = v;
            }
            CKind::MoveTo { .. } => self.nodes[i].dest = Vec3::splat(f32::NAN),
            _ => {}
        }
    }

    /// Where a Vector or Object key points (None when unset or gone).
    fn target<H: Host>(h: &mut H, slot: Slot) -> (Option<Vec3>, Option<Entity>) {
        if slot == NO_SLOT {
            return (None, None);
        }
        let v = h.with_bb(|bb, sh| match bb.get(sh, slot as usize) {
            Some(BbValue::Vector(v)) => (Some(*v), None),
            Some(BbValue::Object(o)) => (None, o.0),
            _ => (None, None),
        });
        match v {
            (Some(p), _) => (Some(p), None),
            (None, Some(e)) => (h.object_position(e), Some(e)),
            _ => (None, None),
        }
    }

    fn run<H: Host>(
        &mut self,
        tree: &Compiled,
        i: usize,
        h: &mut H,
        dt: f32,
        entering: bool,
    ) -> Status {
        let count = tree.nodes[i].child_count as usize;
        let id = tree.nodes[i].id;
        match &tree.nodes[i].kind {
            CKind::Root => {
                if count == 0 {
                    return Status::Failure;
                }
                self.tick_node(tree, tree.child(i, 0), h, dt)
            }
            CKind::Sequence | CKind::Selector | CKind::RandomSelector => {
                let stop_on = if matches!(tree.nodes[i].kind, CKind::Sequence) {
                    Status::Failure
                } else {
                    Status::Success
                };
                loop {
                    let k = self.nodes[i].child as usize;
                    if k >= count {
                        // All ran without stopping: a Sequence succeeded, a Selector failed.
                        return if stop_on == Status::Failure {
                            Status::Success
                        } else {
                            Status::Failure
                        };
                    }
                    let c = self.child_at(tree, i, k);
                    match self.tick_node(tree, c, h, dt) {
                        Status::Running => return Status::Running,
                        r if r == stop_on => return r,
                        _ => self.nodes[i].child += 1,
                    }
                }
            }
            CKind::Parallel(policy) => {
                let (mut ok, mut failed) = (0, 0);
                for k in 0..count {
                    let c = tree.child(i, k);
                    let r = match self.nodes[c].status {
                        s @ (Status::Success | Status::Failure) => s,
                        _ => self.tick_node(tree, c, h, dt),
                    };
                    match r {
                        Status::Success => ok += 1,
                        Status::Failure => failed += 1,
                        _ => {}
                    }
                }
                let done = match policy {
                    ParallelPolicy::RequireAll if failed > 0 => Some(Status::Failure),
                    ParallelPolicy::RequireAll if ok == count => Some(Status::Success),
                    ParallelPolicy::RequireOne if ok > 0 => Some(Status::Success),
                    ParallelPolicy::RequireOne if failed == count => Some(Status::Failure),
                    _ => None,
                };
                match done {
                    Some(r) => {
                        for k in 0..count {
                            let c = tree.child(i, k);
                            self.abort(tree, c, h);
                        }
                        r
                    }
                    None => Status::Running,
                }
            }
            CKind::Inverter => match self.only_child(tree, i, h, dt) {
                Status::Success => Status::Failure,
                Status::Failure => Status::Success,
                r => r,
            },
            CKind::ForceSuccess => match self.only_child(tree, i, h, dt) {
                Status::Running => Status::Running,
                _ => Status::Success,
            },
            CKind::ForceFailure => match self.only_child(tree, i, h, dt) {
                Status::Running => Status::Running,
                _ => Status::Failure,
            },
            CKind::Repeat {
                count: n,
                until_fail,
            } => {
                let (n, until_fail) = (*n, *until_fail);
                // At most one round finishes per tick, so a child that finishes at once can't
                // spin forever inside one frame.
                match self.only_child(tree, i, h, dt) {
                    Status::Running => Status::Running,
                    Status::Failure if until_fail => Status::Success,
                    Status::Failure => Status::Failure,
                    _ => {
                        self.nodes[i].child += 1;
                        if !until_fail && n > 0 && self.nodes[i].child >= n {
                            Status::Success
                        } else {
                            Status::Running
                        }
                    }
                }
            }
            CKind::Cooldown(s) => {
                let s = *s;
                if entering && self.time + 1e-6 < self.nodes[i].until {
                    return Status::Failure;
                }
                let r = self.only_child(tree, i, h, dt);
                if r.finished() {
                    self.nodes[i].until = self.time + f64::from(s);
                }
                r
            }
            CKind::TimeLimit(_) => {
                if self.time + 1e-6 >= self.nodes[i].until {
                    if count > 0 {
                        let c = tree.child(i, 0);
                        self.abort(tree, c, h);
                    }
                    return Status::Failure;
                }
                self.only_child(tree, i, h, dt)
            }
            CKind::Conditional {
                key,
                op,
                value,
                abort,
            } => {
                if entering || abort.aborts_self() {
                    let holds = h.with_bb(|bb, sh| {
                        bb.get(sh, *key as usize).is_some_and(|k| op.eval(k, value))
                    });
                    if !holds {
                        if count > 0 {
                            let c = tree.child(i, 0);
                            self.abort(tree, c, h);
                        }
                        return Status::Failure;
                    }
                }
                self.only_child(tree, i, h, dt)
            }
            CKind::MoveTo { target, acceptance } => {
                let (Some(dest), _) = Self::target(h, *target) else {
                    return Status::Failure;
                };
                let here = h.position();
                if flat_distance(here, dest) <= *acceptance {
                    if !entering {
                        h.move_stop();
                    }
                    return Status::Success;
                }
                let last = self.nodes[i].dest;
                if entering || last.is_nan() || last.distance(dest) > REPLAN_DISTANCE {
                    self.nodes[i].dest = dest;
                    if !h.move_start(dest) {
                        return Status::Failure;
                    }
                    return Status::Running;
                }
                let (left, has_path) = h.move_progress();
                if left <= 0.05 {
                    // Stopped at the destination (within the agent's stopping distance), or at
                    // the closest point it can reach.
                    return Status::Success;
                }
                if !has_path {
                    // The path was lost (or never found): ask again next tick.
                    self.nodes[i].dest = Vec3::splat(f32::NAN);
                }
                Status::Running
            }
            CKind::Wait { .. } => {
                if self.time + 1e-6 >= self.nodes[i].until {
                    Status::Success
                } else {
                    Status::Running
                }
            }
            CKind::PlayAnimation { state, crossfade } => {
                Status::from_bool(h.play_animation(state, *crossfade))
            }
            CKind::SetAnimatorParameter {
                parameter,
                value,
                from,
            } => {
                let ok = match from {
                    Some(slot) => {
                        let v = h.with_bb(|bb, sh| bb.get(sh, *slot as usize).cloned());
                        match v {
                            Some(v) => h.set_animator_parameter(parameter, &v),
                            None => false,
                        }
                    }
                    None => h.set_animator_parameter(parameter, value),
                };
                Status::from_bool(ok)
            }
            CKind::LookAt { target, turn_speed } => {
                let (Some(p), _) = Self::target(h, *target) else {
                    return Status::Failure;
                };
                let max = if *turn_speed > 0.0 {
                    turn_speed * dt
                } else {
                    f32::INFINITY
                };
                if h.turn_towards(p, max) <= FACING_DEGREES {
                    Status::Success
                } else {
                    Status::Running
                }
            }
            CKind::SetBlackboard { key, value, from } => {
                let (key, from) = (*key, *from);
                let ok = h.with_bb(|bb, sh| {
                    if key == NO_SLOT {
                        return false;
                    }
                    match from {
                        Some(f) if f != NO_SLOT => match bb.get(sh, f as usize).cloned() {
                            Some(v) => bb.set(sh, key as usize, &v).is_ok(),
                            None => false,
                        },
                        Some(_) => false,
                        None => bb.set(sh, key as usize, value).is_ok(),
                    }
                });
                Status::from_bool(ok)
            }
            CKind::IsInRange { target, range } => {
                let (Some(p), _) = Self::target(h, *target) else {
                    return Status::Failure;
                };
                Status::from_bool(h.position().distance(p) <= *range)
            }
            CKind::CanSeeTarget {
                target,
                range,
                cos_half_fov,
                eye_height,
                store,
            } => {
                let (p, e) = Self::target(h, *target);
                let seen = p.is_some_and(|p| {
                    let here = h.position();
                    let eye = here + Vec3::Y * *eye_height;
                    let to = p - here;
                    let dist = to.length();
                    if dist > *range {
                        return false;
                    }
                    let fwd = h.forward();
                    let flat = Vec3::new(to.x, 0.0, to.z).normalize_or_zero();
                    let fwd_flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or_zero();
                    if dist > 0.01 && flat != Vec3::ZERO && fwd_flat.dot(flat) < *cos_half_fov {
                        return false;
                    }
                    h.line_clear(eye, p + Vec3::Y * (*eye_height * 0.75), e)
                });
                if let Some(s) = store
                    && *s != NO_SLOT
                {
                    let s = *s as usize;
                    h.with_bb(|bb, sh| {
                        let _ = bb.set(sh, s, &BbValue::Bool(seen));
                    });
                }
                Status::from_bool(seen)
            }
            CKind::HasLineOfSight { target, eye_height } => {
                let (p, e) = Self::target(h, *target);
                let Some(p) = p else {
                    return Status::Failure;
                };
                let eye = h.position() + Vec3::Y * *eye_height;
                Status::from_bool(h.line_clear(eye, p + Vec3::Y * (*eye_height * 0.75), e))
            }
            CKind::RunScriptGraph { graph, event, wait } => {
                if entering {
                    let _ = h.take_finish(id);
                    h.send_event(id, graph, event);
                    if !*wait {
                        return Status::Success;
                    }
                }
                match h.take_finish(id) {
                    Some(ok) => Status::from_bool(ok),
                    None => Status::Running,
                }
            }
            CKind::RunLua { function } => h.call_lua(id, function),
            CKind::CustomLua { script, params } => {
                if entering {
                    let r = h.custom_lua(id, LeafPhase::Start, script, params, dt);
                    if r.finished() {
                        return r;
                    }
                }
                h.custom_lua(id, LeafPhase::Tick, script, params, dt)
            }
            CKind::Log { message, level } => {
                let mut text = std::mem::take(&mut self.text);
                text.clear();
                fill_message(&mut text, message, h);
                h.log(*level, &text);
                self.text = text;
                Status::Success
            }
        }
    }

    fn only_child<H: Host>(&mut self, tree: &Compiled, i: usize, h: &mut H, dt: f32) -> Status {
        if tree.nodes[i].child_count == 0 {
            return Status::Failure;
        }
        let c = tree.child(i, 0);
        self.tick_node(tree, c, h, dt)
    }
}

fn is_composite(k: &CKind) -> bool {
    matches!(
        k,
        CKind::Sequence | CKind::Selector | CKind::RandomSelector | CKind::Parallel(_)
    )
}

fn flat_distance(a: Vec3, b: Vec3) -> f32 {
    let d = a - b;
    (d.x * d.x + d.z * d.z).sqrt()
}

/// `message` with `{key}` replaced by the key's value.
fn fill_message<H: Host>(out: &mut String, message: &str, h: &mut H) {
    use std::fmt::Write;
    let mut rest = message;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                let found = h.with_bb(|bb, sh| match bb.get_named(sh, name) {
                    Some(v) => {
                        let _ = write!(out, "{v}");
                        true
                    }
                    None => false,
                });
                if !found {
                    out.push('{');
                    out.push_str(name);
                    out.push('}');
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
}
