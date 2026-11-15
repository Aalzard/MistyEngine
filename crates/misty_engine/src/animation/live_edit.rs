//! Animator Controllers edited while the game runs: a running Animator moves onto the new
//! version of its controller and keeps what still makes sense.
//!
//! - Each layer (matched by name, or by position when it was renamed) keeps its current state
//!   and normalised time. A state is matched by name, or by its slot in the machine when it was
//!   renamed in place; a state that no longer exists sends the layer to its default state.
//! - Parameters that still exist with the same type keep their values; the others start at
//!   their new defaults.
//! - A transition in progress continues if it still exists. If it was removed, the layer snaps
//!   to its destination (or stays on its source if the destination is gone too).

use super::controller::{AnimatorController, Node, StateMachine, Transition};
use super::fsm::{self, Crossfade, LayerRun, Params, Run};

/// A running Animator moved onto a new controller version.
pub(super) struct Rebound {
    pub params: Vec<f32>,
    pub layers: Vec<LayerRun>,
    /// New index of each old base layer state (`None` = gone).
    pub base_states: Vec<Option<usize>>,
}

/// Move parameter values and layer runs played with `old` onto `new`.
pub(super) fn rebind(
    old: &AnimatorController,
    new: &AnimatorController,
    params: &[f32],
    layers: &[LayerRun],
) -> Rebound {
    let mut values: Vec<f32> = new
        .parameters
        .iter()
        .map(|p| {
            old.parameters
                .iter()
                .position(|q| q.name == p.name && q.kind == p.kind)
                .and_then(|i| params.get(i).copied())
                .unwrap_or(p.default)
        })
        .collect();
    let mut out = Vec::with_capacity(new.layer_count());
    for l in 0..new.layer_count() {
        let from = matching_layer(old, new, l);
        let old_run = from.and_then(|o| layers.get(o).map(|r| (o, r)));
        let default_weight = new.layer(l).map_or(1.0, |x| x.weight);
        // A script's SetLayerWeight survives unless the edit changed the layer's weight.
        let weight = match old_run {
            Some((o, r)) if old.layer(o).map(|x| x.weight) == new.layer(l).map(|x| x.weight) => {
                r.weight
            }
            _ => default_weight,
        };
        let mut lr = LayerRun {
            weight,
            ..Default::default()
        };
        if new.machine_layer(l) == l {
            let m = new.machine(l).expect("layer exists");
            if let Some((o, r)) = old_run
                && old.machine_layer(o) == o
                && let Some(om) = old.machine(o)
            {
                remap_run(om, m, r, &mut lr);
            }
            if lr.current.is_none() {
                let p = Params {
                    ctrl: new,
                    values: &mut values,
                };
                lr.current =
                    fsm::enter(m, "", &p, &mut Vec::new(), 0).map(|state| Run { state, norm: 0.0 });
                lr.fresh = true;
            }
        }
        out.push(lr);
    }
    let base_states = (0..old.base.states.len())
        .map(|i| state_map(&old.base, &new.base, i))
        .collect();
    Rebound {
        params: values,
        layers: out,
        base_states,
    }
}

/// The old layer that new layer `l` continues: same name, or same position if that layer was
/// renamed.
fn matching_layer(old: &AnimatorController, new: &AnimatorController, l: usize) -> Option<usize> {
    if l == 0 {
        return Some(0);
    }
    old.layer_index(new.layer_name(l)).or_else(|| {
        (l < old.layer_count() && new.layer_index(old.layer_name(l)).is_none()).then_some(l)
    })
}

/// New index of old state `i`: same name, or the same slot if it was renamed in place (the new
/// name was not in the old machine).
fn state_map(old: &StateMachine, new: &StateMachine, i: usize) -> Option<usize> {
    let s = old.states.get(i)?;
    if let Some(j) = new.state_index(&s.name) {
        return Some(j);
    }
    new.states
        .get(i)
        .filter(|n| old.state_index(&n.name).is_none())
        .map(|_| i)
}

/// New path of old sub-state machine `path`: the same path, or the machine in the same slot if
/// it was renamed in place (its new path was not in the old machine). "" stays "".
fn machine_map(old: &StateMachine, new: &StateMachine, path: &str) -> Option<String> {
    if path.is_empty() || new.machine_exists(path) {
        return Some(path.to_owned());
    }
    let i = old.sub_machines.iter().position(|m| m.path == path)?;
    new.sub_machines
        .get(i)
        .filter(|n| !old.machine_exists(&n.path))
        .map(|n| n.path.clone())
}

/// New index of old transition `t`: the transition between the same (maybe renamed) ends,
/// preferring the same position. `None` when it was removed.
fn transition_map(old: &StateMachine, new: &StateMachine, t: usize) -> Option<usize> {
    let ot = old.transitions.get(t)?;
    let end = |n: &Node| match n {
        Node::State(name) => old
            .state_index(name)
            .and_then(|i| state_map(old, new, i))
            .map(|j| Node::State(new.states[j].name.clone())),
        Node::Machine(path) => machine_map(old, new, path).map(Node::Machine),
        other => Some(other.clone()),
    };
    let (from, to) = (end(&ot.from)?, end(&ot.to)?);
    let machine = machine_map(old, new, &ot.machine)?;
    let same = |x: &Transition| x.from == from && x.to == to && x.machine == machine;
    if new.transitions.get(t).is_some_and(same) {
        return Some(t);
    }
    new.transitions.iter().position(same)
}

/// One layer's run on the old machine `om`, carried over to the new machine `m`.
fn remap_run(om: &StateMachine, m: &StateMachine, r: &LayerRun, lr: &mut LayerRun) {
    let map = |run: Run| {
        state_map(om, m, run.state).map(|state| Run {
            state,
            norm: run.norm,
        })
    };
    lr.fresh = r.fresh;
    lr.current = r.current.and_then(map);
    lr.frozen = r
        .frozen
        .iter()
        .filter_map(|&(run, w)| map(run).map(|run| (run, w)))
        .collect();
    if let Some(c) = r.next {
        let to = map(c.to);
        let kept = match c.transition {
            // A script cross-fade has no transition to lose.
            None => Some(None),
            Some(t) => transition_map(om, m, t).map(Some),
        };
        match (to, kept) {
            (Some(to), Some(transition)) if lr.current.is_some() => {
                lr.next = Some(Crossfade {
                    to,
                    transition,
                    ..c
                });
            }
            // The transition (or the state it left) is gone: the destination takes over.
            (Some(to), _) => lr.current = Some(to),
            // The destination is gone: stay on the source.
            (None, _) => {}
        }
    }
    if lr.next.is_none() {
        lr.frozen.clear();
    }
}
