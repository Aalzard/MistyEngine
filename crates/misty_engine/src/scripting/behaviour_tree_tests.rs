//! Behaviour Tree Agents ticking in a scene with the script host: blackboard values from Lua,
//! Run Lua and custom Lua leaves, events answered with `BehaviourTree.Finish`, Look At, shared
//! keys, Stop / Start, the tick rate.

use std::collections::BTreeMap;
use std::path::PathBuf;

use glam::Vec3;

use super::*;
use crate::behaviour_tree::asset::{AbortMode, BtNode, LogLevel, NodeKind};
use crate::behaviour_tree::blackboard::{BbValue, CompareOp, KeyDef, KeyType};
use crate::behaviour_tree::component::BlackboardOverride;
use crate::behaviour_tree::{BehaviourTree, BehaviourTreeAgent};
use crate::physics::Physics;
use crate::scene::ScriptRef;

fn project(files: &[(&str, &str)]) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_bt_host_{n}"));
    for (name, text) in files {
        let p = root.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    root
}

/// A tree from (kind, children) pairs in creation order; the last one is under the Root.
fn tree(keys: Vec<KeyDef>, nodes: Vec<(NodeKind, Vec<u32>)>) -> BehaviourTree {
    let mut t = BehaviourTree {
        keys,
        ..Default::default()
    };
    let last = nodes.len() as u32;
    for (i, (kind, kids)) in nodes.into_iter().enumerate() {
        let id = i as u32 + 1;
        let mut n = BtNode::new(id, kind, [0.0, id as f32 * 10.0]);
        n.children = kids;
        t.nodes.push(n);
    }
    let mut root = BtNode::new(last + 1, NodeKind::Root, [-200.0, 0.0]);
    root.children = vec![last];
    t.nodes.push(root);
    t
}

fn agent(scene: &mut Scene, name: &str, tree: &str) -> Entity {
    let e = scene.spawn(name, None);
    scene.add(
        e,
        BehaviourTreeAgent {
            tree: Some(tree.into()),
            ..Default::default()
        },
    );
    e
}

fn script(scene: &mut Scene, e: Entity, path: &str) {
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: path.into(),
                ..Default::default()
            }],
        },
    );
}

fn frames(host: &mut ScriptHost, scene: &mut Scene, time: &mut Time, n: usize) {
    for _ in 0..n {
        time.advance(1.0 / 60.0);
        host.update(scene, time, Input::default());
    }
}

fn texts(logs: &[(LogKind, String)]) -> Vec<String> {
    logs.iter().map(|(_, t)| t.clone()).collect()
}

const BRAIN: &str = r#"
local Brain = {}
function Brain:Awake()
    BehaviourTree.SetValue(self.gameObject, "Mood", "grumpy")
    self.steps = 0
end
function Brain:Step()
    self.steps = self.steps + 1
    if self.steps < 3 then return "running" end
    return true
end
function Brain:Attack()
    -- Answers the Run Script Graph node's custom event.
    BehaviourTree.Finish(self.gameObject, true)
end
return Brain
"#;

const LEAF: &str = r#"
-- A custom leaf: counts ticks up to params.ticks, then succeeds.
local Leaf = {}
function Leaf:OnStart()
    self.n = 0
    BehaviourTree.SetValue(self.gameObject, "Started", true)
end
function Leaf:OnTick(dt)
    self.n = self.n + 1
    if self.n < self.params.ticks then return "running" end
    return "success"
end
return Leaf
"#;

#[test]
fn agent_runs_lua_leaves_and_reads_values_set_by_scripts() {
    let t = tree(
        vec![
            KeyDef::new("Mood", KeyType::String),
            KeyDef::new("Started", KeyType::Bool),
        ],
        vec![
            (
                NodeKind::Log {
                    message: "mood {Mood}".into(),
                    level: LogLevel::Info,
                },
                vec![],
            ),
            (
                NodeKind::RunLua {
                    function: "Step".into(),
                },
                vec![],
            ),
            (
                NodeKind::CustomLua {
                    script: "Assets/AI/CountLeaf.lua".into(),
                    params: BTreeMap::from([("ticks".to_owned(), "2".to_owned())]),
                },
                vec![],
            ),
            (
                NodeKind::RunScriptGraph {
                    graph: String::new(),
                    event: "Attack".into(),
                    wait: true,
                },
                vec![],
            ),
            (
                NodeKind::Log {
                    message: "done".into(),
                    level: LogLevel::Info,
                },
                vec![],
            ),
            (NodeKind::Sequence, vec![1, 2, 3, 4, 5]),
        ],
    );
    let root = project(&[
        ("Assets/AI/Guard.btree", &t.to_json()),
        ("Assets/AI/Brain.lua", BRAIN),
        ("Assets/AI/CountLeaf.lua", LEAF),
    ]);
    let mut scene = Scene::new("t");
    let e = agent(&mut scene, "Guard", "Assets/AI/Guard.btree");
    {
        let mut a = scene.world.get::<&mut BehaviourTreeAgent>(e).unwrap();
        a.restart_when_done = false;
    }
    script(&mut scene, e, "Assets/AI/Brain.lua");
    let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(&mut scene, physics, root.clone());
    let mut time = Time::default();
    frames(&mut host, &mut scene, &mut time, 12);
    let logs = texts(&host.take_logs());
    assert!(logs.contains(&"mood grumpy".to_owned()), "{logs:?}");
    assert_eq!(logs.iter().filter(|l| *l == "done").count(), 1, "{logs:?}");
    assert!(
        logs.iter().all(|l| !l.contains("rror")),
        "no errors: {logs:?}"
    );
    let status = host.with_behaviour(|b| {
        let a = b.agent(e).unwrap();
        (a.run, a.bb.get_named(&b.shared, "Started").cloned())
    });
    assert_eq!(
        status.0,
        crate::behaviour_tree::runtime::RunState::Done(crate::behaviour_tree::Status::Success)
    );
    assert_eq!(status.1, Some(BbValue::Bool(true)));
    let view = host.behaviour_debug("Assets/AI/Guard.btree", None).unwrap();
    assert_eq!(view.entity, Some(e));
    assert!(view.keys.iter().any(|k| k.0 == "Mood" && k.2 == "grumpy"));
    let _ = std::fs::remove_dir_all(root);
}

const CONTROL: &str = r#"
local Control = {}
function Control:Update()
    local f = Time.frameCount
    if f == 5 then BehaviourTree.Stop(self.gameObject) end
    if f == 10 then
        Debug.Log("status " .. BehaviourTree.GetStatus(self.gameObject))
        BehaviourTree.Start(self.gameObject)
    end
    if f == 12 then
        Debug.Log("running " .. tostring(BehaviourTree.IsRunning(self.gameObject)))
        Debug.Log("has " .. tostring(BehaviourTree.HasKey(self.gameObject, "Alarm")))
        Debug.Log("count " .. tostring(BehaviourTree.GetValue(self.gameObject, "Ticks")))
    end
end
return Control
"#;

#[test]
fn shared_keys_lookat_stop_start_and_tick_rate() {
    // Watcher: sets the shared Alarm. Listener: logs once it sees it. Both turn to Target.
    let mut alarm = KeyDef::new("Alarm", KeyType::Bool);
    alarm.shared = true;
    let keys = vec![
        alarm,
        KeyDef::new("Target", KeyType::Object),
        KeyDef::new("Ticks", KeyType::Int),
        KeyDef::new("Watcher", KeyType::Bool),
    ];
    let t = tree(
        keys,
        vec![
            (
                NodeKind::LookAt {
                    target: "Target".into(),
                    turn_speed: 0.0,
                },
                vec![],
            ),
            (
                NodeKind::SetBlackboard {
                    key: "Alarm".into(),
                    value: BbValue::Bool(true),
                    from_key: String::new(),
                },
                vec![],
            ),
            (
                NodeKind::Conditional {
                    key: "Watcher".into(),
                    op: CompareOp::IsSet,
                    value: BbValue::Bool(true),
                    abort: AbortMode::None,
                },
                vec![2],
            ),
            (
                NodeKind::Log {
                    message: "alarm heard".into(),
                    level: LogLevel::Warning,
                },
                vec![],
            ),
            (
                NodeKind::Conditional {
                    key: "Alarm".into(),
                    op: CompareOp::IsSet,
                    value: BbValue::Bool(true),
                    abort: AbortMode::None,
                },
                vec![4],
            ),
            (NodeKind::ForceSuccess, vec![5]),
            (NodeKind::Selector, vec![3, 6]),
            (NodeKind::Sequence, vec![1, 7]),
        ],
    );
    let root = project(&[
        ("Assets/AI/Pair.btree", &t.to_json()),
        ("Assets/AI/Control.lua", CONTROL),
    ]);
    let mut scene = Scene::new("t");
    let target = scene.spawn("Target", None);
    scene.set_transform(target, crate::scene::Transform::from_position(Vec3::new(10.0, 0.0, 0.0)));
    let watcher = agent(&mut scene, "Watcher", "Assets/AI/Pair.btree");
    let listener = agent(&mut scene, "Listener", "Assets/AI/Pair.btree");
    for (e, watching) in [(watcher, true), (listener, false)] {
        let mut a = scene.world.get::<&mut BehaviourTreeAgent>(e).unwrap();
        a.overrides = vec![
            BlackboardOverride {
                key: "Target".into(),
                object: Some(target),
                ..Default::default()
            },
            BlackboardOverride {
                key: "Watcher".into(),
                value: BbValue::Bool(watching),
                object: None,
            },
        ];
    }
    {
        // The listener ticks 10 times a second.
        let mut a = scene.world.get::<&mut BehaviourTreeAgent>(listener).unwrap();
        a.tick_rate = 10.0;
    }
    script(&mut scene, watcher, "Assets/AI/Control.lua");
    let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(&mut scene, physics, root.clone());
    let mut time = Time::default();
    frames(&mut host, &mut scene, &mut time, 3);
    // The watcher (every frame) already faces the target (+X); the listener (10 per second)
    // hasn't ticked yet.
    let facing = |scene: &Scene, e: Entity| (scene.world_rotation(e) * Vec3::Z).distance(Vec3::X) < 0.01;
    assert!(facing(&scene, watcher));
    assert!(!facing(&scene, listener));
    frames(&mut host, &mut scene, &mut time, 57);
    assert!(facing(&scene, listener), "the listener turned too");
    let logs = texts(&host.take_logs());
    assert!(logs.iter().any(|l| l == "alarm heard"), "{logs:?}");
    assert!(logs.contains(&"status Stopped".to_owned()), "{logs:?}");
    assert!(logs.contains(&"running true".to_owned()), "{logs:?}");
    assert!(logs.contains(&"has true".to_owned()), "{logs:?}");
    let ticks = host.with_behaviour(|b| {
        (
            b.agent(watcher).unwrap().exec.ticks,
            b.agent(listener).unwrap().exec.ticks,
        )
    });
    // 60 frames: the watcher ticks every frame but 5 stopped ones, the listener ~10 times.
    assert!((50..=56).contains(&ticks.0), "watcher ticks {}", ticks.0);
    assert!((9..=11).contains(&ticks.1), "listener ticks {}", ticks.1);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_missing_tree_or_key_warns_instead_of_failing() {
    let root = project(&[]);
    let mut scene = Scene::new("t");
    let e = agent(&mut scene, "Lost", "Assets/AI/Nope.btree");
    {
        let mut a = scene.world.get::<&mut BehaviourTreeAgent>(e).unwrap();
        a.overrides.push(BlackboardOverride {
            key: "Ghost".into(),
            value: BbValue::Int(1),
            object: None,
        });
    }
    let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(&mut scene, physics, root.clone());
    let mut time = Time::default();
    frames(&mut host, &mut scene, &mut time, 3);
    let logs = texts(&host.take_logs());
    assert!(logs.iter().any(|l| l.contains("Nope.btree")), "{logs:?}");
    assert!(logs.iter().any(|l| l.contains("'Ghost'")), "{logs:?}");
    let _ = std::fs::remove_dir_all(root);
}
