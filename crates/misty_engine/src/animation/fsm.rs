//! Animator state machine rules for one layer: which transition fires (Any State first, then
//! the current state's, in order), exit times, conditions and triggers, entering and leaving
//! sub-state machines, and interrupting a transition in progress.

use super::controller::{
    AnimatorController, Condition, Node, ParamKind, StateMachine, Transition, machine_parent,
};

/// Deepest chain of Entry / sub-machine hops followed while resolving one transition.
const MAX_HOPS: usize = 16;

/// A state being played: its index in the layer's machine and normalised time (1 = one length,
/// keeps growing while it loops).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Run {
    pub state: usize,
    pub norm: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crossfade {
    pub to: Run,
    pub elapsed: f32,
    pub duration: f32,
    /// Transition being taken (index into the machine's transitions); None for script fades.
    pub transition: Option<usize>,
}

impl Crossfade {
    /// 0 at the start of the blend, 1 when the destination has taken over.
    pub fn progress(&self) -> f32 {
        if self.duration <= 0.0 {
            1.0
        } else {
            (self.elapsed / self.duration).clamp(0.0, 1.0)
        }
    }
}

/// One layer's live state machine.
#[derive(Clone, Debug, Default)]
pub struct LayerRun {
    pub current: Option<Run>,
    pub next: Option<Crossfade>,
    /// An interrupted blend, held still at the moment of interruption: runs (with fixed times)
    /// and weights. While non-empty it replaces `current` as the side `next` blends away from.
    pub frozen: Vec<(Run, f32)>,
    /// Just entered: the next update samples without advancing time.
    pub fresh: bool,
    pub weight: f32,
}

/// Parameter values with the controller's parameter list.
pub struct Params<'a> {
    pub ctrl: &'a AnimatorController,
    pub values: &'a mut [f32],
}

impl Params<'_> {
    pub fn get(&self, name: &str) -> f32 {
        self.ctrl
            .parameters
            .iter()
            .position(|p| p.name == name)
            .and_then(|i| self.values.get(i).copied())
            .unwrap_or(0.0)
    }

    fn holds(&self, conditions: &[Condition]) -> bool {
        conditions.iter().all(|c| {
            self.ctrl
                .parameters
                .iter()
                .position(|p| p.name == c.parameter)
                .is_some_and(|i| c.mode.test(self.values[i], c.threshold))
        })
    }

    /// Switch off the triggers `conditions` used.
    fn consume(&mut self, conditions: &[Condition]) {
        for c in conditions {
            if let Some(i) = self
                .ctrl
                .parameters
                .iter()
                .position(|p| p.name == c.parameter)
                && self.ctrl.parameters[i].kind == ParamKind::Trigger
            {
                self.values[i] = 0.0;
            }
        }
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

/// The state a machine's Entry leads to: the first Entry transition whose conditions hold, or
/// the machine's default state. Conditions used are added to `used`.
pub fn enter(
    m: &StateMachine,
    machine: &str,
    p: &Params,
    used: &mut Vec<Condition>,
    hops: usize,
) -> Option<usize> {
    if hops > MAX_HOPS {
        return None;
    }
    let entry = m
        .transitions_from(&Node::Entry)
        .filter(|(_, t)| t.machine == machine)
        .find(|(_, t)| !t.conditions.is_empty() && p.holds(&t.conditions));
    if let Some((_, t)) = entry {
        used.extend(t.conditions.iter().cloned());
        return resolve(m, t, p, used, hops + 1);
    }
    m.default_of(machine).and_then(|d| m.state_index(d))
}

/// Leave machine `machine` through its Exit: the parent's transitions from this machine are
/// checked in order (no exit time; one without conditions always goes); otherwise the parent's
/// Entry decides. Leaving the top machine starts it again from its Entry.
fn exit(
    m: &StateMachine,
    machine: &str,
    p: &Params,
    used: &mut Vec<Condition>,
    hops: usize,
) -> Option<usize> {
    if hops > MAX_HOPS {
        return None;
    }
    if machine.is_empty() {
        return enter(m, "", p, used, hops + 1);
    }
    let node = Node::Machine(machine.to_owned());
    let out = m
        .transitions_from(&node)
        .find(|(_, t)| p.holds(&t.conditions));
    if let Some((_, t)) = out {
        used.extend(t.conditions.iter().cloned());
        return resolve(m, t, p, used, hops + 1);
    }
    enter(m, machine_parent(machine), p, used, hops + 1)
}

/// The state a transition finally lands in (through sub-machine Entry and Exit nodes).
pub fn resolve(
    m: &StateMachine,
    t: &Transition,
    p: &Params,
    used: &mut Vec<Condition>,
    hops: usize,
) -> Option<usize> {
    match &t.to {
        Node::State(n) => m.state_index(n),
        Node::Machine(path) => enter(m, path, p, used, hops + 1),
        Node::Exit => exit(m, &m.context(t), p, used, hops + 1),
        _ => None,
    }
}

/// What a firing transition asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fire {
    pub dest: usize,
    pub transition: usize,
    pub seconds: f32,
    pub offset: f32,
}

/// Seconds of a transition's blend (normalised durations count lengths of the state left).
pub fn seconds(t: &Transition, source_length: f32) -> f32 {
    if t.fixed_duration {
        t.duration
    } else {
        t.duration * source_length
    }
}

/// Check one transition from a state playing `run` (`prev` = its time last frame); when it can
/// go, consume its triggers and return where it lands. `entering` is the state already being
/// played or entered, which Any State only restarts when allowed.
#[allow(clippy::too_many_arguments)]
fn try_fire(
    m: &StateMachine,
    index: usize,
    run: Run,
    prev: f32,
    source_length: f32,
    entering: usize,
    p: &mut Params,
) -> Option<Fire> {
    let t = &m.transitions[index];
    // A transition needs an exit time or a condition (otherwise it never fires).
    if !t.has_exit_time && t.conditions.is_empty() {
        return None;
    }
    let looping = m.states.get(run.state).is_some_and(|s| s.looping);
    if t.has_exit_time && !exit_time_reached(prev, run.norm, t.exit_time, looping) {
        return None;
    }
    if !p.holds(&t.conditions) {
        return None;
    }
    let mut used = Vec::new();
    let dest = resolve(m, t, p, &mut used, 0)?;
    if t.from == Node::AnyState && dest == entering && !t.can_transition_to_self {
        return None;
    }
    p.consume(&t.conditions);
    p.consume(&used);
    Some(Fire {
        dest,
        transition: index,
        seconds: seconds(t, source_length),
        offset: t.offset,
    })
}

/// Lengths (seconds at speed 1) of the states a layer is playing.
#[derive(Clone, Copy, Debug)]
pub struct Lengths {
    pub current: f32,
    pub next: f32,
}

/// Check the layer's transitions after time advanced (`prev_*` = normalised times before).
/// Without a blend in progress: Any State transitions, then the current state's, in order.
/// During a blend: Any State transitions, then the groups the blend's Interruption Source
/// names (with Ordered Interruption, only the current state's transitions listed before the
/// one being taken). Returns whether something fired.
pub fn evaluate(
    m: &StateMachine,
    lr: &mut LayerRun,
    prev_current: f32,
    prev_next: f32,
    lengths: Lengths,
    p: &mut Params,
) -> bool {
    let Some(cur) = lr.current else {
        return false;
    };
    let Some(cf) = lr.next else {
        let here = Node::State(m.states[cur.state].name.clone());
        let order: Vec<usize> = m
            .transitions_from(&Node::AnyState)
            .chain(m.transitions_from(&here))
            .map(|(i, _)| i)
            .collect();
        for i in order {
            if let Some(f) = try_fire(m, i, cur, prev_current, lengths.current, cur.state, p) {
                start(lr, f.dest, f.seconds, f.offset, Some(f.transition));
                return true;
            }
        }
        return false;
    };
    let active = cf.transition.and_then(|i| m.transitions.get(i));
    // (transition, run it leaves, its previous time, its length, from the next state)
    let mut queue: Vec<(usize, Run, f32, f32, bool)> = m
        .transitions_from(&Node::AnyState)
        .filter(|(i, _)| Some(*i) != cf.transition)
        .map(|(i, _)| (i, cf.to, prev_next, lengths.next, true))
        .collect();
    if let Some(t) = active {
        for &from_next in t.interruption_source.order() {
            if from_next {
                let node = Node::State(m.states[cf.to.state].name.clone());
                queue.extend(
                    m.transitions_from(&node)
                        .map(|(i, _)| (i, cf.to, prev_next, lengths.next, true)),
                );
            } else {
                let node = match &t.from {
                    Node::State(_) => t.from.clone(),
                    _ => Node::State(m.states[cur.state].name.clone()),
                };
                let ordered = t.ordered_interruption && t.from == node;
                let limit = cf.transition.unwrap_or(usize::MAX);
                queue.extend(
                    m.transitions_from(&node)
                        .filter(|(i, _)| Some(*i) != cf.transition && (!ordered || *i < limit))
                        .map(|(i, _)| (i, cur, prev_current, lengths.current, false)),
                );
            }
        }
    }
    for (i, run, prev, len, from_next) in queue {
        // Any State transitions are checked against the state being entered.
        let entering = cf.to.state;
        if let Some(f) = try_fire(m, i, run, prev, len, entering, p) {
            interrupt(
                lr,
                f.dest,
                f.seconds,
                f.offset,
                Some(f.transition),
                from_next,
            );
            return true;
        }
    }
    false
}

/// Start a blend to `state` (an immediate switch when `seconds` is 0 or nothing plays).
pub fn start(
    lr: &mut LayerRun,
    state: usize,
    seconds: f32,
    offset: f32,
    transition: Option<usize>,
) {
    let to = Run {
        state,
        norm: offset,
    };
    lr.frozen.clear();
    if seconds <= 0.0 || lr.current.is_none() {
        lr.current = Some(to);
        lr.next = None;
    } else {
        lr.next = Some(Crossfade {
            to,
            elapsed: 0.0,
            duration: seconds,
            transition,
        });
    }
}

/// Interrupt the blend in progress: its pose is frozen where it is and blended out by the new
/// blend to `state`. `from_next` = the interrupting transition left the state being entered
/// (it becomes the current state).
pub fn interrupt(
    lr: &mut LayerRun,
    state: usize,
    seconds: f32,
    offset: f32,
    transition: Option<usize>,
    from_next: bool,
) {
    let (Some(cur), Some(cf)) = (lr.current, lr.next) else {
        start(lr, state, seconds, offset, transition);
        return;
    };
    if seconds <= 0.0 {
        lr.current = Some(Run {
            state,
            norm: offset,
        });
        lr.next = None;
        lr.frozen.clear();
        return;
    }
    let w = cf.progress();
    let side = if lr.frozen.is_empty() {
        vec![(cur, 1.0)]
    } else {
        std::mem::take(&mut lr.frozen)
    };
    let mut parts: Vec<(Run, f32)> = side.into_iter().map(|(r, x)| (r, x * (1.0 - w))).collect();
    parts.push((cf.to, w));
    parts.retain(|(_, x)| *x > 1e-4);
    lr.frozen = parts;
    lr.current = Some(if from_next { cf.to } else { cur });
    lr.next = Some(Crossfade {
        to: Run {
            state,
            norm: offset,
        },
        elapsed: 0.0,
        duration: seconds,
        transition,
    });
}
