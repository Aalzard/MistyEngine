//! Behaviour tree semantics with a fake world: composites and decorators (a table of cases),
//! aborts, blackboard types, save/load.

use std::collections::{BTreeMap, HashMap};

use glam::Vec3;
use hecs::Entity;

use super::asset::*;
use super::blackboard::*;
use super::compile::{Compiled, compile};
use super::exec::{ExecState, Host, LeafPhase, Status};

/// A fake world. `RunLua` leaves are scripted by their function name: "S" succeeds, "F" fails,
/// "R" runs forever, and a list like "R,R,S" gives one result per call (the last repeats).
#[derive(Default)]
struct Fake {
    bb: Blackboard,
    shared: SharedBlackboards,
    calls: Vec<String>,
    counters: HashMap<u32, usize>,
    pos: Vec3,
    fwd: Vec3,
    objects: HashMap<Entity, Vec3>,
    walls: bool,
    moving_to: Option<Vec3>,
    finish: HashMap<u32, bool>,
    logs: Vec<String>,
}

impl Fake {
    fn new(keys: &[KeyDef]) -> Self {
        let mut shared = SharedBlackboards::default();
        let bb = Blackboard::new(keys, "group", &mut shared);
        Fake {
            bb,
            shared,
            fwd: Vec3::Z,
            ..Default::default()
        }
    }

    fn set(&mut self, key: &str, v: BbValue) {
        self.bb.set_named(&mut self.shared, key, &v).unwrap();
    }
}

fn parse(s: &str) -> Status {
    match s {
        "S" => Status::Success,
        "F" => Status::Failure,
        _ => Status::Running,
    }
}

impl Host for Fake {
    fn with_bb<R>(&mut self, f: impl FnOnce(&mut Blackboard, &mut SharedBlackboards) -> R) -> R {
        f(&mut self.bb, &mut self.shared)
    }
    fn position(&mut self) -> Vec3 {
        self.pos
    }
    fn forward(&mut self) -> Vec3 {
        self.fwd
    }
    fn object_position(&mut self, e: Entity) -> Option<Vec3> {
        self.objects.get(&e).copied()
    }
    fn move_start(&mut self, dest: Vec3) -> bool {
        self.calls.push("move".into());
        self.moving_to = Some(dest);
        true
    }
    fn move_progress(&mut self) -> (f32, bool) {
        // Walk 1 m per call.
        if let Some(d) = self.moving_to {
            let to = d - self.pos;
            let step = to.clamp_length_max(1.0);
            self.pos += step;
            let left = (d - self.pos).length();
            if left < 1e-4 {
                self.moving_to = None;
            }
            (left, self.moving_to.is_some())
        } else {
            (0.0, false)
        }
    }
    fn move_stop(&mut self) {
        self.calls.push("stop".into());
        self.moving_to = None;
    }
    fn turn_towards(&mut self, point: Vec3, _max: f32) -> f32 {
        self.fwd = (point - self.pos).normalize_or_zero();
        0.0
    }
    fn line_clear(&mut self, _from: Vec3, _to: Vec3, _target: Option<Entity>) -> bool {
        !self.walls
    }
    fn play_animation(&mut self, state: &str, _crossfade: f32) -> bool {
        self.calls.push(format!("anim {state}"));
        true
    }
    fn set_animator_parameter(&mut self, name: &str, value: &BbValue) -> bool {
        self.calls.push(format!("param {name}={value}"));
        true
    }
    fn send_event(&mut self, node: u32, _graph: &str, event: &str) {
        self.calls.push(format!("event {event} {node}"));
    }
    fn take_finish(&mut self, node: u32) -> Option<bool> {
        self.finish.remove(&node)
    }
    fn call_lua(&mut self, node: u32, function: &str) -> Status {
        self.calls.push(function.to_owned());
        let parts: Vec<&str> = function.split(',').collect();
        let n = self.counters.entry(node).or_default();
        let r = parse(parts[(*n).min(parts.len() - 1)]);
        *n += 1;
        r
    }
    fn custom_lua(
        &mut self,
        _node: u32,
        phase: LeafPhase,
        script: &str,
        _params: &BTreeMap<String, String>,
        _dt: f32,
    ) -> Status {
        self.calls.push(format!("{script}:{phase:?}"));
        match phase {
            LeafPhase::Start => Status::Running,
            _ => Status::Running,
        }
    }
    fn log(&mut self, _level: LogLevel, text: &str) {
        self.logs.push(text.to_owned());
    }
}

/// Builds trees: children are made first, in order (their height on the canvas follows).
struct Builder {
    tree: BehaviourTree,
}

impl Builder {
    fn new() -> Self {
        Builder {
            tree: BehaviourTree::default(),
        }
    }

    fn add(&mut self, kind: NodeKind, children: &[u32]) -> u32 {
        let id = self.tree.next_id();
        let mut n = BtNode::new(id, kind, [0.0, id as f32 * 10.0]);
        n.children = children.to_vec();
        self.tree.nodes.push(n);
        id
    }

    fn lua(&mut self, f: &str) -> u32 {
        self.add(NodeKind::RunLua { function: f.into() }, &[])
    }

    fn key(&mut self, name: &str, ty: KeyType) {
        self.tree.keys.push(KeyDef::new(name, ty));
    }

    fn root(mut self, child: u32) -> BehaviourTree {
        self.add(NodeKind::Root, &[child]);
        self.tree
    }
}

fn run(tree: &BehaviourTree, ticks: usize, dt: f32) -> (Vec<Status>, Fake) {
    let c = compile(tree);
    let mut h = Fake::new(&tree.keys);
    let mut st = ExecState::new(&c, 7);
    let out = (0..ticks).map(|_| st.tick(&c, &mut h, dt)).collect();
    (out, h)
}

fn composite(kind: NodeKind, leaves: &[&str]) -> BehaviourTree {
    let mut b = Builder::new();
    let kids: Vec<u32> = leaves.iter().map(|f| b.lua(f)).collect();
    let c = b.add(kind, &kids);
    b.root(c)
}

fn decorator(kind: NodeKind, leaf: &str) -> BehaviourTree {
    let mut b = Builder::new();
    let l = b.lua(leaf);
    let d = b.add(kind, &[l]);
    b.root(d)
}

use Status::{Failure as F, Running as R, Success as S};

#[test]
fn composite_semantics_table() {
    let par_all = NodeKind::Parallel {
        policy: ParallelPolicy::RequireAll,
    };
    let par_one = NodeKind::Parallel {
        policy: ParallelPolicy::RequireOne,
    };
    // (kind, leaves, results over the ticks, leaf calls of the first tick)
    let cases: Vec<(NodeKind, Vec<&str>, Vec<Status>, Vec<&str>)> = vec![
        (NodeKind::Sequence, vec!["S", "S"], vec![S], vec!["S", "S"]),
        (
            NodeKind::Sequence,
            vec!["S", "F", "S"],
            vec![F],
            vec!["S", "F"],
        ),
        (
            NodeKind::Sequence,
            vec!["S", "R"],
            vec![R, R],
            vec!["S", "R"],
        ),
        (
            NodeKind::Sequence,
            vec!["R,S", "S"],
            vec![R, S],
            vec!["R,S"],
        ),
        (
            NodeKind::Selector,
            vec!["F", "S", "F"],
            vec![S],
            vec!["F", "S"],
        ),
        (NodeKind::Selector, vec!["F", "F"], vec![F], vec!["F", "F"]),
        (
            NodeKind::Selector,
            vec!["R,F", "S"],
            vec![R, S],
            vec!["R,F"],
        ),
        (par_all.clone(), vec!["S", "S"], vec![S], vec!["S", "S"]),
        (par_all.clone(), vec!["S", "F"], vec![F], vec!["S", "F"]),
        (
            par_all.clone(),
            vec!["R,S", "S"],
            vec![R, S],
            vec!["R,S", "S"],
        ),
        (par_one.clone(), vec!["F", "S"], vec![S], vec!["F", "S"]),
        (par_one.clone(), vec!["F", "F"], vec![F], vec!["F", "F"]),
        (
            par_one.clone(),
            vec!["R", "R,S"],
            vec![R, S],
            vec!["R", "R,S"],
        ),
    ];
    for (kind, leaves, want, first_calls) in cases {
        let tree = composite(kind.clone(), &leaves);
        let (got, h) = run(&tree, want.len(), 0.1);
        assert_eq!(got, want, "{} {leaves:?}", kind.label());
        let first: Vec<&str> = h
            .calls
            .iter()
            .take(first_calls.len())
            .map(|s| s.as_str())
            .collect();
        assert_eq!(first, first_calls, "{} {leaves:?} calls", kind.label());
    }
}

#[test]
fn decorator_semantics_table() {
    let cases: Vec<(NodeKind, &str, Vec<Status>)> = vec![
        (NodeKind::Inverter, "S", vec![F]),
        (NodeKind::Inverter, "F", vec![S]),
        (NodeKind::Inverter, "R", vec![R]),
        (NodeKind::ForceSuccess, "F", vec![S]),
        (NodeKind::ForceSuccess, "R,F", vec![R, S]),
        (NodeKind::ForceFailure, "S", vec![F]),
        (
            NodeKind::Repeat {
                count: 3,
                until_fail: false,
            },
            "S",
            vec![R, R, S],
        ),
        (
            NodeKind::Repeat {
                count: 3,
                until_fail: false,
            },
            "S,F",
            vec![R, F],
        ),
        (
            NodeKind::Repeat {
                count: 0,
                until_fail: true,
            },
            "S,S,F",
            vec![R, R, S],
        ),
        (
            NodeKind::Repeat {
                count: 0,
                until_fail: false,
            },
            "S",
            vec![R, R, R, R],
        ),
        // Cooldown 0.25 s, ticks of 0.1 s: runs, refuses twice, runs again.
        (NodeKind::Cooldown { seconds: 0.25 }, "S", vec![S, F, F, S]),
        (NodeKind::Cooldown { seconds: 0.25 }, "F", vec![F, F, F, F]),
        // Time limit 0.25 s on a leaf that runs forever: fails on the third tick.
        (NodeKind::TimeLimit { seconds: 0.25 }, "R", vec![R, R, F]),
        (NodeKind::TimeLimit { seconds: 0.25 }, "R,S", vec![R, S]),
    ];
    for (kind, leaf, want) in cases {
        let tree = decorator(kind.clone(), leaf);
        let (got, h) = run(&tree, want.len(), 0.1);
        assert_eq!(
            got,
            want,
            "{} over {leaf}: calls {:?}",
            kind.label(),
            h.calls
        );
    }
}

#[test]
fn cooldown_skips_its_child_while_cooling() {
    let tree = decorator(NodeKind::Cooldown { seconds: 0.25 }, "S");
    let (_, h) = run(&tree, 4, 0.1);
    assert_eq!(h.calls.len(), 2, "ran on ticks 1 and 4: {:?}", h.calls);
}

#[test]
fn conditional_compares_blackboard_values() {
    let cases: Vec<(KeyType, BbValue, CompareOp, BbValue, Status)> = vec![
        (
            KeyType::Bool,
            BbValue::Bool(true),
            CompareOp::IsSet,
            BbValue::Bool(true),
            S,
        ),
        (
            KeyType::Bool,
            BbValue::Bool(false),
            CompareOp::IsSet,
            BbValue::Bool(true),
            F,
        ),
        (
            KeyType::Bool,
            BbValue::Bool(false),
            CompareOp::IsNotSet,
            BbValue::Bool(true),
            S,
        ),
        (
            KeyType::Int,
            BbValue::Int(3),
            CompareOp::Greater,
            BbValue::Int(2),
            S,
        ),
        (
            KeyType::Int,
            BbValue::Int(3),
            CompareOp::Less,
            BbValue::Int(2),
            F,
        ),
        (
            KeyType::Int,
            BbValue::Int(3),
            CompareOp::Equal,
            BbValue::Float(3.0),
            S,
        ),
        (
            KeyType::Float,
            BbValue::Float(0.5),
            CompareOp::LessOrEqual,
            BbValue::Float(0.5),
            S,
        ),
        (
            KeyType::Float,
            BbValue::Float(0.5),
            CompareOp::GreaterOrEqual,
            BbValue::Int(1),
            F,
        ),
        (
            KeyType::String,
            BbValue::String("idle".into()),
            CompareOp::Equal,
            BbValue::String("idle".into()),
            S,
        ),
        (
            KeyType::String,
            BbValue::String("idle".into()),
            CompareOp::NotEqual,
            BbValue::String("idle".into()),
            F,
        ),
        (
            KeyType::Vector,
            BbValue::Vector(Vec3::X),
            CompareOp::Equal,
            BbValue::Vector(Vec3::X),
            S,
        ),
        (
            KeyType::Vector,
            BbValue::Vector(Vec3::ZERO),
            CompareOp::IsSet,
            BbValue::Bool(true),
            F,
        ),
        (
            KeyType::Object,
            BbValue::Object(ObjRef(None)),
            CompareOp::IsNotSet,
            BbValue::Bool(true),
            S,
        ),
    ];
    for (ty, start, op, value, want) in cases {
        let mut b = Builder::new();
        b.key("K", ty);
        let l = b.lua("S");
        let d = b.add(
            NodeKind::Conditional {
                key: "K".into(),
                op,
                value: value.clone(),
                abort: AbortMode::None,
            },
            &[l],
        );
        let tree = b.root(d);
        let c = compile(&tree);
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        let mut h = Fake::new(&tree.keys);
        h.set("K", start.clone());
        let mut st = ExecState::new(&c, 1);
        assert_eq!(st.tick(&c, &mut h, 0.1), want, "{start:?} {op:?} {value:?}");
    }
}

#[test]
fn random_selector_is_deterministic_and_tries_every_child() {
    // Leaves named by their place, all failing: the call order is the shuffled order.
    let tree = composite(
        NodeKind::RandomSelector,
        &["F", "F,F", "F,F,F", "F,F,F,F", "F,F,F,F,F"],
    );
    let c = compile(&tree);
    let order = |seed: u64| {
        let mut h = Fake::new(&[]);
        let mut st = ExecState::new(&c, seed);
        assert_eq!(st.tick(&c, &mut h, 0.1), F);
        h.calls
    };
    let first = order(3);
    assert_eq!(first.len(), 5, "every child tried: {first:?}");
    let mut sorted = first.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 5, "each once");
    assert_eq!(order(3), first, "same seed, same order");
    assert!((0..20).any(|s| order(s) != first), "seeds change the order");
}

#[test]
fn wait_runs_for_its_time() {
    let mut b = Builder::new();
    let w = b.add(
        NodeKind::Wait {
            seconds: 0.3,
            random: 0.0,
        },
        &[],
    );
    let tree = b.root(w);
    let (got, _) = run(&tree, 4, 0.1);
    assert_eq!(
        got,
        vec![R, R, S, R],
        "0.1, 0.2 waiting; 0.3 done; then starts over"
    );
}

/// Selector [ Conditional(Alert, Lower Priority) → Chase, Patrol ].
fn guard_tree(abort: AbortMode) -> BehaviourTree {
    let mut b = Builder::new();
    b.key("Alert", KeyType::Bool);
    let chase = b.add(
        NodeKind::CustomLua {
            script: "Chase".into(),
            params: BTreeMap::new(),
        },
        &[],
    );
    let cond = b.add(
        NodeKind::Conditional {
            key: "Alert".into(),
            op: CompareOp::IsSet,
            value: BbValue::Bool(true),
            abort,
        },
        &[chase],
    );
    let patrol = b.add(
        NodeKind::CustomLua {
            script: "Patrol".into(),
            params: BTreeMap::new(),
        },
        &[],
    );
    let sel = b.add(NodeKind::Selector, &[cond, patrol]);
    b.root(sel)
}

#[test]
fn lower_priority_abort_switches_branches_when_the_key_changes() {
    let tree = guard_tree(AbortMode::LowerPriority);
    let c = compile(&tree);
    assert!(c.errors.is_empty(), "{:?}", c.errors);
    assert_eq!(c.observers.len(), 1);
    let mut h = Fake::new(&tree.keys);
    let mut st = ExecState::new(&c, 1);
    assert_eq!(st.tick(&c, &mut h, 0.1), R);
    assert_eq!(h.calls, ["Patrol:Start", "Patrol:Tick"]);
    st.tick(&c, &mut h, 0.1);
    h.calls.clear();
    h.set("Alert", BbValue::Bool(true));
    assert_eq!(st.tick(&c, &mut h, 0.1), R);
    assert_eq!(h.calls, ["Patrol:Abort", "Chase:Start", "Chase:Tick"]);
}

#[test]
fn without_an_abort_mode_the_running_branch_keeps_going() {
    let tree = guard_tree(AbortMode::None);
    let c = compile(&tree);
    let mut h = Fake::new(&tree.keys);
    let mut st = ExecState::new(&c, 1);
    st.tick(&c, &mut h, 0.1);
    h.set("Alert", BbValue::Bool(true));
    h.calls.clear();
    st.tick(&c, &mut h, 0.1);
    assert_eq!(h.calls, ["Patrol:Tick"]);
}

#[test]
fn self_abort_stops_its_branch_when_the_condition_turns_false() {
    let tree = guard_tree(AbortMode::SelfOnly);
    let c = compile(&tree);
    let mut h = Fake::new(&tree.keys);
    h.set("Alert", BbValue::Bool(true));
    let mut st = ExecState::new(&c, 1);
    st.tick(&c, &mut h, 0.1);
    assert_eq!(h.calls, ["Chase:Start", "Chase:Tick"]);
    h.calls.clear();
    h.set("Alert", BbValue::Bool(false));
    st.tick(&c, &mut h, 0.1);
    assert_eq!(h.calls, ["Chase:Abort", "Patrol:Start", "Patrol:Tick"]);
}

#[test]
fn both_aborts_go_back_and_forth() {
    let tree = guard_tree(AbortMode::Both);
    let c = compile(&tree);
    let mut h = Fake::new(&tree.keys);
    let mut st = ExecState::new(&c, 1);
    st.tick(&c, &mut h, 0.1);
    for round in 0..3 {
        h.calls.clear();
        h.set("Alert", BbValue::Bool(true));
        st.tick(&c, &mut h, 0.1);
        assert_eq!(
            h.calls,
            ["Patrol:Abort", "Chase:Start", "Chase:Tick"],
            "round {round}"
        );
        h.calls.clear();
        h.set("Alert", BbValue::Bool(false));
        st.tick(&c, &mut h, 0.1);
        assert_eq!(
            h.calls,
            ["Chase:Abort", "Patrol:Start", "Patrol:Tick"],
            "round {round}"
        );
    }
}

#[test]
fn time_limit_aborts_its_running_child() {
    let mut b = Builder::new();
    let leaf = b.add(
        NodeKind::CustomLua {
            script: "Long".into(),
            params: BTreeMap::new(),
        },
        &[],
    );
    let tl = b.add(NodeKind::TimeLimit { seconds: 0.15 }, &[leaf]);
    let tree = b.root(tl);
    let (got, h) = run(&tree, 2, 0.1);
    assert_eq!(got, vec![R, F]);
    assert_eq!(h.calls, ["Long:Start", "Long:Tick", "Long:Abort"]);
}

#[test]
fn parallel_stops_the_other_children_when_it_finishes() {
    let mut b = Builder::new();
    let long = b.add(
        NodeKind::CustomLua {
            script: "Long".into(),
            params: BTreeMap::new(),
        },
        &[],
    );
    let quick = b.lua("R,S");
    let p = b.add(
        NodeKind::Parallel {
            policy: ParallelPolicy::RequireOne,
        },
        &[long, quick],
    );
    let tree = b.root(p);
    let (got, h) = run(&tree, 2, 0.1);
    assert_eq!(got, vec![R, S]);
    assert_eq!(h.calls.last().map(String::as_str), Some("Long:Abort"));
}

#[test]
fn leaves_move_look_see_and_write_the_blackboard() {
    let mut b = Builder::new();
    b.key("Goal", KeyType::Vector);
    b.key("Player", KeyType::Object);
    b.key("Seen", KeyType::Bool);
    b.key("Speed", KeyType::Float);
    let see = b.add(
        NodeKind::CanSeeTarget {
            target: "Player".into(),
            range: 10.0,
            fov: 90.0,
            eye_height: 1.6,
            store: "Seen".into(),
        },
        &[],
    );
    let set = b.add(
        NodeKind::SetBlackboard {
            key: "Speed".into(),
            value: BbValue::Float(3.5),
            from_key: String::new(),
        },
        &[],
    );
    let param = b.add(
        NodeKind::SetAnimatorParameter {
            parameter: "Speed".into(),
            value: BbValue::Float(0.0),
            from_key: "Speed".into(),
        },
        &[],
    );
    let mv = b.add(
        NodeKind::MoveTo {
            target: "Goal".into(),
            acceptance: 0.5,
        },
        &[],
    );
    let look = b.add(
        NodeKind::LookAt {
            target: "Player".into(),
            turn_speed: 0.0,
        },
        &[],
    );
    let log = b.add(
        NodeKind::Log {
            message: "speed {Speed} seen {Seen} {Nope}".into(),
            level: LogLevel::Info,
        },
        &[],
    );
    let seq = b.add(NodeKind::Sequence, &[see, set, param, mv, look, log]);
    let tree = b.root(seq);
    let c = compile(&tree);
    assert!(c.errors.is_empty(), "{:?}", c.errors);
    let mut h = Fake::new(&tree.keys);
    let player = Entity::from_bits(1 << 32 | 5).unwrap();
    h.objects.insert(player, Vec3::new(0.0, 0.0, 5.0));
    h.set("Player", BbValue::Object(ObjRef(Some(player))));
    h.set("Goal", BbValue::Vector(Vec3::new(0.0, 0.0, 3.0)));
    let mut st = ExecState::new(&c, 1);
    let mut results = Vec::new();
    for _ in 0..6 {
        results.push(st.tick(&c, &mut h, 0.1));
        if results.last() == Some(&S) {
            break;
        }
    }
    assert_eq!(results.last(), Some(&S), "{results:?} {:?}", h.calls);
    assert!(
        h.calls.contains(&"param Speed=3.500".to_owned()),
        "{:?}",
        h.calls
    );
    assert!(h.pos.distance(Vec3::new(0.0, 0.0, 3.0)) <= 0.5);
    assert_eq!(h.logs, ["speed 3.500 seen true {Nope}"]);
    assert_eq!(
        h.bb.get_named(&h.shared, "Seen"),
        Some(&BbValue::Bool(true))
    );
    // Behind a wall: not seen, and the key says so.
    h.walls = true;
    let mut st = ExecState::new(&c, 1);
    assert_eq!(st.tick(&c, &mut h, 0.1), F);
    assert_eq!(
        h.bb.get_named(&h.shared, "Seen"),
        Some(&BbValue::Bool(false))
    );
    // Out of the field of view.
    h.walls = false;
    h.fwd = -Vec3::Z;
    h.pos = Vec3::ZERO;
    let mut st = ExecState::new(&c, 1);
    assert_eq!(st.tick(&c, &mut h, 0.1), F, "behind the agent");
}

#[test]
fn run_script_graph_waits_for_finish() {
    let mut b = Builder::new();
    let g = b.add(
        NodeKind::RunScriptGraph {
            graph: "Assets/Attack.vgraph".into(),
            event: "Attack".into(),
            wait: true,
        },
        &[],
    );
    let tree = b.root(g);
    let c = compile(&tree);
    assert_eq!(c.graphs.len(), 1);
    let mut h = Fake::new(&[]);
    let mut st = ExecState::new(&c, 1);
    assert_eq!(st.tick(&c, &mut h, 0.1), R);
    assert_eq!(st.tick(&c, &mut h, 0.1), R);
    h.finish.insert(g, true);
    assert_eq!(st.tick(&c, &mut h, 0.1), S);
    assert_eq!(h.calls, [format!("event Attack {g}")]);
}

#[test]
fn compile_reports_problems() {
    let mut b = Builder::new();
    b.key("Flag", KeyType::Bool);
    let m = b.add(
        NodeKind::MoveTo {
            target: "Flag".into(),
            acceptance: 1.0,
        },
        &[],
    );
    let missing = b.add(
        NodeKind::IsInRange {
            target: "Nope".into(),
            range: 1.0,
        },
        &[],
    );
    let inv = b.add(NodeKind::Inverter, &[]);
    let seq = b.add(NodeKind::Sequence, &[m, missing, inv]);
    let _stray = b.lua("S");
    let tree = b.root(seq);
    let c = compile(&tree);
    let text: Vec<String> = c.errors.iter().map(|e| e.message.clone()).collect();
    let has = |s: &str| text.iter().any(|t| t.contains(s));
    assert!(has("key 'Flag' is Bool"), "{text:?}");
    assert!(has("no blackboard key 'Nope'"), "{text:?}");
    assert!(has("Inverter needs a child"), "{text:?}");
    assert!(has("never run"), "{text:?}");
    assert!(
        compile(&BehaviourTree::default()).errors[0]
            .message
            .contains("no Root")
    );
}

#[test]
fn blackboard_types_convert_share_and_count_versions() {
    let keys = vec![
        KeyDef::new("b", KeyType::Bool),
        KeyDef::new("i", KeyType::Int),
        KeyDef::new("f", KeyType::Float),
        KeyDef::new("v", KeyType::Vector),
        KeyDef::new("o", KeyType::Object),
        KeyDef::new("s", KeyType::String),
        KeyDef {
            shared: true,
            ..KeyDef::new("alarm", KeyType::Bool)
        },
    ];
    let mut shared = SharedBlackboards::default();
    let mut a = Blackboard::new(&keys, "guards", &mut shared);
    let mut b = Blackboard::new(&keys, "guards", &mut shared);
    let mut other = Blackboard::new(&keys, "other", &mut shared);
    // Defaults by type.
    assert_eq!(a.get_named(&shared, "i"), Some(&BbValue::Int(0)));
    assert_eq!(
        a.get_named(&shared, "v"),
        Some(&BbValue::Vector(Vec3::ZERO))
    );
    assert_eq!(
        a.get_named(&shared, "s"),
        Some(&BbValue::String(String::new()))
    );
    // Conversions between numbers and to text; vectors don't go into numbers.
    a.set_named(&mut shared, "i", &BbValue::Float(2.7)).unwrap();
    assert_eq!(a.get_named(&shared, "i"), Some(&BbValue::Int(2)));
    a.set_named(&mut shared, "f", &BbValue::Int(4)).unwrap();
    assert_eq!(a.get_named(&shared, "f"), Some(&BbValue::Float(4.0)));
    a.set_named(&mut shared, "s", &BbValue::Int(9)).unwrap();
    assert_eq!(
        a.get_named(&shared, "s"),
        Some(&BbValue::String("9".into()))
    );
    assert!(
        a.set_named(&mut shared, "f", &BbValue::Vector(Vec3::ONE))
            .is_err()
    );
    assert!(a.set_named(&mut shared, "o", &BbValue::Int(1)).is_err());
    assert!(
        a.set_named(&mut shared, "missing", &BbValue::Int(1))
            .is_err()
    );
    // Versions go up only on change.
    let slot = a.index("b").unwrap();
    let v0 = a.version(&shared, slot);
    assert!(a.set(&mut shared, slot, &BbValue::Bool(true)).unwrap());
    assert!(!a.set(&mut shared, slot, &BbValue::Bool(true)).unwrap());
    assert_eq!(a.version(&shared, slot), v0 + 1);
    // Per agent: b doesn't see a's own values.
    assert_eq!(b.get_named(&shared, "b"), Some(&BbValue::Bool(false)));
    // Shared keys: one value per group.
    b.set_named(&mut shared, "alarm", &BbValue::Bool(true))
        .unwrap();
    assert_eq!(a.get_named(&shared, "alarm"), Some(&BbValue::Bool(true)));
    assert!(a.is_shared(a.index("alarm").unwrap()));
    assert_eq!(
        other.get_named(&shared, "alarm"),
        Some(&BbValue::Bool(false))
    );
    let alarm = a.index("alarm").unwrap();
    assert_eq!(a.version(&shared, alarm), b.version(&shared, alarm));
    other
        .set_named(&mut shared, "alarm", &BbValue::Bool(true))
        .unwrap();
    // Objects: set, then forgotten when destroyed.
    let e = Entity::from_bits(1 << 32 | 3).unwrap();
    a.set_named(&mut shared, "o", &BbValue::Object(ObjRef(Some(e))))
        .unwrap();
    assert_eq!(
        a.get_named(&shared, "o").and_then(BbValue::as_object),
        Some(e)
    );
    a.forget_object(e);
    assert_eq!(a.get_named(&shared, "o").and_then(BbValue::as_object), None);
    // Text values reuse their buffer.
    a.set_named(&mut shared, "s", &BbValue::String("patrol".into()))
        .unwrap();
    assert_eq!(a.describe(&shared)[5].2, "patrol");
}

/// Every node kind, keys of every type, a comment and a group survive a save and a load.
#[test]
fn save_load_round_trip() {
    let mut t = BehaviourTree::new_default();
    for (i, kind) in NodeKind::palette().into_iter().enumerate() {
        let id = t.next_id();
        let mut n = BtNode::new(id, kind, [400.0, i as f32 * 60.0]);
        if i % 3 == 0 {
            n.name = format!("Node {i}");
        }
        t.nodes.push(n);
    }
    for ty in KeyType::ALL {
        let mut k = KeyDef::new(&format!("{}Key", ty.label()), ty);
        k.default = match ty {
            KeyType::Bool => BbValue::Bool(true),
            KeyType::Int => BbValue::Int(-4),
            KeyType::Float => BbValue::Float(2.5),
            KeyType::Vector => BbValue::Vector(Vec3::new(1.0, 2.0, 3.0)),
            KeyType::Object => BbValue::Object(ObjRef(None)),
            KeyType::String => BbValue::String("hi \"there\"".into()),
        };
        k.shared = ty == KeyType::Int;
        k.description = "about it".into();
        t.keys.push(k);
    }
    if let Some(NodeKind::CustomLua { params, .. }) = t
        .nodes
        .last_mut()
        .map(|n| &mut n.kind)
        .filter(|k| matches!(k, NodeKind::CustomLua { .. }))
    {
        params.insert("speed".into(), "2".into());
    }
    t.comments.push(Comment {
        text: "Guard brain".into(),
        position: [10.0, -80.0],
        size: [200.0, 60.0],
    });
    t.groups.push(Group {
        title: "Combat".into(),
        nodes: vec![2, 3],
    });
    t.link(2, 3).unwrap();
    let text = t.to_json();
    let back = BehaviourTree::from_json(&text).unwrap();
    assert_eq!(back, t);
    assert_eq!(back.to_json(), text, "stable output");
    // Files from a newer editor are refused, broken files explain why.
    let newer = text.replacen("\"version\": 1", "\"version\": 99", 1);
    assert!(
        BehaviourTree::from_json(&newer)
            .unwrap_err()
            .contains("newer")
    );
    assert!(BehaviourTree::from_json("{\"nodes\": 3}").is_err());
    // And through a file.
    let dir = std::env::temp_dir().join(format!("misty_bt_{}", std::process::id()));
    let path = dir.join("Guard.btree");
    t.save(&path).unwrap();
    assert_eq!(BehaviourTree::load(&path).unwrap(), t);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn editing_links_keeps_children_ordered_and_refuses_loops() {
    let mut t = BehaviourTree::new_default();
    let a = t.next_id();
    t.nodes.push(BtNode::new(
        a,
        NodeKind::Wait {
            seconds: 1.0,
            random: 0.0,
        },
        [400.0, 100.0],
    ));
    let b = t.next_id();
    t.nodes.push(BtNode::new(
        b,
        NodeKind::Wait {
            seconds: 1.0,
            random: 0.0,
        },
        [400.0, -100.0],
    ));
    t.link(2, a).unwrap();
    t.link(2, b).unwrap();
    assert_eq!(t.node(2).unwrap().children, vec![b, a], "top first");
    t.node_mut(b).unwrap().position[1] = 300.0;
    t.sort_all_children();
    assert_eq!(t.node(2).unwrap().children, vec![a, b]);
    assert!(t.link(a, 2).is_err(), "a leaf has no children");
    let s = t.next_id();
    t.nodes
        .push(BtNode::new(s, NodeKind::Sequence, [600.0, 0.0]));
    t.link(2, s).unwrap();
    assert!(t.link(s, 2).unwrap_err().contains("loop"));
    assert!(t.link(s, 1).is_err(), "nothing above the Root");
    // A decorator keeps one child: linking another replaces it.
    let inv = t.next_id();
    t.nodes
        .push(BtNode::new(inv, NodeKind::Inverter, [800.0, 0.0]));
    t.link(inv, a).unwrap();
    t.link(inv, b).unwrap();
    assert_eq!(t.node(inv).unwrap().children, vec![b]);
    assert!(
        !t.node(2).unwrap().children.contains(&b),
        "moved from its old parent"
    );
    // Renaming a key follows into the nodes.
    t.keys.push(KeyDef::new("Target", KeyType::Object));
    let m = t.next_id();
    t.nodes.push(BtNode::new(
        m,
        NodeKind::MoveTo {
            target: "Target".into(),
            acceptance: 1.0,
        },
        [0.0, 0.0],
    ));
    t.rename_key("Target", "Enemy");
    assert!(
        matches!(&t.node(m).unwrap().kind, NodeKind::MoveTo { target, .. } if target == "Enemy")
    );
    t.remove_node(a);
    assert!(t.node(inv).is_some() && !t.node(2).unwrap().children.contains(&a));
}

/// A tick allocates nothing: the memory is sized once, and ticking a busy tree many times
/// leaves every buffer where it was.
#[test]
fn ticking_reuses_its_memory() {
    let tree = guard_tree(AbortMode::Both);
    let c: Compiled = compile(&tree);
    let mut h = Fake::new(&tree.keys);
    let mut st = ExecState::new(&c, 1);
    let ptr = st.nodes.as_ptr();
    let cap = st.nodes.capacity();
    for i in 0..1000 {
        h.set("Alert", BbValue::Bool(i % 7 < 3));
        st.tick(&c, &mut h, 0.016);
        h.calls.clear();
    }
    assert_eq!(st.nodes.as_ptr(), ptr);
    assert_eq!(st.nodes.capacity(), cap);
    assert_eq!(st.ticks, 1000);
}
