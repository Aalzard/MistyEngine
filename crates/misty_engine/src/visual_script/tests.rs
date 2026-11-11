//! Script Graph tests: each builds a graph in code, writes it into a temp project, attaches it
//! to objects and plays frames through the script host (the same path the editor and the
//! player use).

use std::path::PathBuf;

use glam::Vec3;
use hecs::Entity;

use super::*;
use crate::physics::Physics;
use crate::runtime::Time;
use crate::scene::{
    BoxCollider, MeshFilter, PrimitiveMesh, Rigidbody, Scene, ScriptRef, ScriptValue, Scripts,
    Transform,
};
use crate::scripting::{Input, LogKind, ScriptHost};

const GRAPH: &str = "Assets/G.vgraph";

/// A graph under construction.
struct G(ScriptGraph);

impl G {
    fn new() -> Self {
        Self(ScriptGraph::default())
    }

    fn n(&mut self, kind: &str) -> u32 {
        let x = self.0.nodes.len() as f32 * 40.0;
        self.0.add_node(kind, [x, 0.0])
    }

    fn np(&mut self, kind: &str, params: &[(&str, &str)]) -> u32 {
        let x = self.0.nodes.len() as f32 * 40.0;
        self.0.add_node_with(kind, params, [x, 0.0])
    }

    fn api(&mut self, owner: &str, member: &str, mode: &str) -> u32 {
        self.np(
            "Api",
            &[("owner", owner), ("member", member), ("mode", mode)],
        )
    }

    fn api_args(&mut self, owner: &str, member: &str, args: &str) -> u32 {
        self.np(
            "Api",
            &[
                ("owner", owner),
                ("member", member),
                ("mode", "call"),
                ("args", args),
            ],
        )
    }

    fn log(&mut self) -> u32 {
        self.api("Debug", "Log", "call")
    }

    /// A Debug.Log of a fixed text.
    fn log_text(&mut self, text: &str) -> u32 {
        let l = self.log();
        self.v(l, "Message", Literal::String(text.into()));
        l
    }

    fn w(&mut self, a: u32, out: &str, b: u32, inp: &str) {
        if let Err(e) = self.0.connect(a, out, b, inp, &NoFiles) {
            panic!("{a}.{out} -> {b}.{inp}: {e}");
        }
    }

    fn v(&mut self, n: u32, pin: &str, lit: Literal) {
        self.0.set_value(n, pin, lit);
    }

    fn f(&mut self, n: u32, pin: &str, x: f32) {
        self.v(n, pin, Literal::Float(x));
    }

    fn var(&mut self, name: &str, ty: PinType, lit: Literal) {
        self.0.variables.push(Variable::new(name, ty, lit));
    }

    fn ovar(&mut self, name: &str, ty: PinType, lit: Literal) {
        self.0.variables.push(Variable::new(name, ty, lit).object());
    }

    fn get(&mut self, name: &str) -> u32 {
        self.np("GetVariable", &[("name", name)])
    }

    fn set(&mut self, name: &str) -> u32 {
        self.np("SetVariable", &[("name", name)])
    }

    /// `name = name + 1` (Int), entered at the returned node's In.
    fn increment(&mut self, name: &str) -> u32 {
        let g = self.get(name);
        let add = self.n("AddInt");
        self.w(g, "Value", add, "A");
        self.v(add, "B", Literal::Int(1));
        let s = self.set(name);
        self.w(add, "Result", s, "Value");
        s
    }
}

fn project(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_vgraph_{tag}_{n}"));
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    root
}

fn attach(scene: &mut Scene, e: Entity, path: &str, fields: &[(&str, ScriptValue)]) {
    let mut list = scene
        .world
        .get::<&Scripts>(e)
        .map(|s| s.list.clone())
        .unwrap_or_default();
    list.push(ScriptRef {
        path: path.into(),
        enabled: true,
        fields: fields
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
    });
    scene.add(e, Scripts { list });
}

/// A playing game.
struct Play {
    host: ScriptHost,
    scene: Scene,
    time: Time,
    logs: Vec<(LogKind, String)>,
}

impl Play {
    fn start(root: PathBuf, mut scene: Scene) -> Self {
        let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
        let host = ScriptHost::start(&mut scene, physics, root);
        let mut p = Self {
            host,
            scene,
            time: Time::default(),
            logs: Vec::new(),
        };
        p.logs.extend(p.host.take_logs());
        p
    }

    fn frame_with(&mut self, input: Input) {
        self.time.advance(1.0 / 60.0);
        self.host.update(&mut self.scene, &self.time, input);
        self.logs.extend(self.host.take_logs());
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame_with(Input::default());
        }
    }

    fn infos(&self) -> Vec<String> {
        self.logs
            .iter()
            .filter(|(k, _)| *k == LogKind::Info)
            .map(|(_, t)| t.clone())
            .collect()
    }

    fn errors(&self) -> Vec<String> {
        self.logs
            .iter()
            .filter(|(k, _)| *k == LogKind::Error)
            .map(|(_, t)| t.clone())
            .collect()
    }

    fn no_errors(&self) {
        assert!(self.errors().is_empty(), "{:?}", self.errors());
    }

    fn ovar(&self, e: Entity, name: &str) -> Value {
        self.host
            .object_variables(e)
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .unwrap_or_else(|| panic!("no object variable {name}"))
    }
}

/// Play `g` on one object named "Thing".
fn play(tag: &str, g: &ScriptGraph) -> (Play, Entity) {
    let root = project(tag);
    g.save(&root.join(GRAPH)).unwrap();
    let mut scene = Scene::new("t");
    let e = scene.spawn("Thing", None);
    attach(&mut scene, e, GRAPH, &[]);
    (Play::start(root, scene), e)
}

/// Start → each node's In in order, through their Out pins.
fn chain(g: &mut G, from: u32, from_pin: &str, nodes: &[u32]) {
    let mut prev = (from, from_pin.to_owned());
    for n in nodes {
        g.w(prev.0, &prev.1, *n, "In");
        prev = (*n, "Out".to_owned());
    }
}

// ---- events -------------------------------------------------------------------------------------

#[test]
fn start_runs_once_and_update_every_frame() {
    let mut g = G::new();
    g.ovar("Count", PinType::Int, Literal::Int(0));
    let start = g.n("OnStart");
    let hello = g.log_text("started");
    g.w(start, "Out", hello, "In");
    let update = g.n("OnUpdate");
    let inc = g.increment("Count");
    g.w(update, "Out", inc, "In");
    let (mut p, e) = play("events", &g.0);
    p.frames(5);
    p.no_errors();
    assert_eq!(p.infos(), ["started"]);
    assert_eq!(p.ovar(e, "Count").as_int(), 5);
}

#[test]
fn update_reports_delta_time_and_late_and_fixed_update_run() {
    let mut g = G::new();
    g.ovar("Dt", PinType::Float, Literal::Float(0.0));
    g.ovar("Fixed", PinType::Int, Literal::Int(0));
    g.ovar("Late", PinType::Int, Literal::Int(0));
    let update = g.n("OnUpdate");
    let s = g.set("Dt");
    g.w(update, "Out", s, "In");
    g.w(update, "Delta Time", s, "Value");
    let fixed = g.n("OnFixedUpdate");
    let f = g.increment("Fixed");
    g.w(fixed, "Out", f, "In");
    let late = g.n("OnLateUpdate");
    let l = g.increment("Late");
    g.w(late, "Out", l, "In");
    let (mut p, e) = play("dt", &g.0);
    p.frames(30);
    p.no_errors();
    assert!((p.ovar(e, "Dt").as_float() - 1.0 / 60.0).abs() < 1e-4);
    assert_eq!(p.ovar(e, "Late").as_int(), 30);
    let fixed = p.ovar(e, "Fixed").as_int();
    assert!((20..=40).contains(&fixed), "fixed steps {fixed}");
}

#[test]
fn timers_fire_on_their_interval_and_once_when_not_repeating() {
    let mut g = G::new();
    g.ovar("Ticks", PinType::Int, Literal::Int(0));
    g.ovar("Once", PinType::Int, Literal::Int(0));
    let t = g.n("OnTimer");
    g.f(t, "Interval", 0.25);
    let inc = g.increment("Ticks");
    g.w(t, "Out", inc, "In");
    let t2 = g.n("OnTimer");
    g.f(t2, "Interval", 0.1);
    g.v(t2, "Repeat", Literal::Bool(false));
    let inc2 = g.increment("Once");
    g.w(t2, "Out", inc2, "In");
    let (mut p, e) = play("timer", &g.0);
    p.frames(61);
    p.no_errors();
    let ticks = p.ovar(e, "Ticks").as_int();
    assert!((3..=4).contains(&ticks), "ticks {ticks}");
    assert_eq!(p.ovar(e, "Once").as_int(), 1);
}

fn cube(scene: &mut Scene, name: &str, pos: Vec3, body: bool) -> Entity {
    let e = scene.spawn(name, None);
    scene.set_transform(e, Transform::from_position(pos));
    scene.add(
        e,
        MeshFilter {
            mesh: Some(PrimitiveMesh::Cube.into()),
        },
    );
    scene.add(e, BoxCollider::default());
    if body {
        scene.add(e, Rigidbody::default());
    }
    e
}

#[test]
fn trigger_and_collision_events_name_the_other_object() {
    let root = project("physics");
    // The zone logs what enters and leaves; the ground logs what lands on it.
    let mut zone = G::new();
    for (event, word) in [("OnTriggerEnter", "enter "), ("OnTriggerExit", "exit ")] {
        let ev = zone.n(event);
        let name = zone.api("#GameObject", "name", "get");
        zone.w(ev, "Other", name, "Target");
        let text = zone.n("Append");
        zone.v(text, "A", Literal::String(word.into()));
        zone.w(name, "Value", text, "B");
        let log = zone.log();
        zone.w(text, "Result", log, "Message");
        zone.w(ev, "Out", log, "In");
    }
    zone.0.save(&root.join("Assets/Zone.vgraph")).unwrap();
    let mut ground = G::new();
    let hit = ground.n("OnCollisionEnter");
    let log = ground.log_text("landed");
    ground.w(hit, "Out", log, "In");
    ground.0.save(&root.join("Assets/Ground.vgraph")).unwrap();

    let mut scene = Scene::new("t");
    let floor = cube(&mut scene, "Ground", Vec3::ZERO, false);
    attach(&mut scene, floor, "Assets/Ground.vgraph", &[]);
    cube(&mut scene, "Cube", Vec3::new(0.0, 4.0, 0.0), true);
    let z = cube(&mut scene, "Zone", Vec3::new(0.0, 2.2, 0.0), false);
    scene.world.get::<&mut BoxCollider>(z).unwrap().is_trigger = true;
    scene.world.get::<&mut BoxCollider>(z).unwrap().size = Vec3::new(1.0, 0.2, 1.0);
    attach(&mut scene, z, "Assets/Zone.vgraph", &[]);
    let mut p = Play::start(root, scene);
    p.frames(150);
    p.no_errors();
    let logs = p.infos();
    assert!(logs.contains(&"enter Cube".to_owned()), "{logs:?}");
    assert!(logs.contains(&"exit Cube".to_owned()), "{logs:?}");
    assert!(logs.contains(&"landed".to_owned()), "{logs:?}");
    let enter = logs.iter().position(|l| l == "enter Cube").unwrap();
    let exit = logs.iter().position(|l| l == "exit Cube").unwrap();
    assert!(enter < exit);
}

#[test]
fn input_actions_fire_performed_held_and_released() {
    use crate::input::actions::{InputActionAsset, PlayerInput};
    let root = project("input");
    InputActionAsset::starter("Player")
        .save(&root.join("Assets/Player.inputactions"))
        .unwrap();
    let mut g = G::new();
    g.ovar("Held", PinType::Int, Literal::Int(0));
    let jump = g.np(
        "OnInputAction",
        &[("action", "Jump"), ("mode", "performed")],
    );
    let log = g.log_text("jump");
    g.w(jump, "Out", log, "In");
    let held = g.np(
        "OnInputAction",
        &[("action", "Player/Jump"), ("mode", "held")],
    );
    let inc = g.increment("Held");
    g.w(held, "Out", inc, "In");
    let up = g.np("OnInputAction", &[("action", "Jump"), ("mode", "released")]);
    let log2 = g.log_text("released");
    g.w(up, "Out", log2, "In");
    g.0.save(&root.join(GRAPH)).unwrap();
    let mut scene = Scene::new("t");
    let e = scene.spawn("Player", None);
    scene.add(
        e,
        PlayerInput {
            actions: Some("Assets/Player.inputactions".into()),
            default_map: "Player".into(),
            ..Default::default()
        },
    );
    attach(&mut scene, e, GRAPH, &[]);
    let physics = Physics::build(&scene, Vec3::ZERO);
    let (audio, _) = crate::audio::Audio::null(root.clone());
    let settings = crate::settings::ProjectSettings::default();
    let host = ScriptHost::start_configured(&mut scene, physics, root, audio, &settings);
    let mut p = Play {
        host,
        scene,
        time: Time::default(),
        logs: Vec::new(),
    };
    let key = |pressed: bool, held: bool, released: bool| {
        let mut i = Input::default();
        if held {
            i.held.insert("space".into());
        }
        if pressed {
            i.pressed.insert("space".into());
        }
        if released {
            i.released.insert("space".into());
        }
        i
    };
    p.frame_with(Input::default());
    p.frame_with(key(true, true, false));
    p.frame_with(key(false, true, false));
    p.frame_with(key(false, true, false));
    p.frame_with(key(false, false, true));
    p.frame_with(Input::default());
    p.no_errors();
    assert_eq!(p.infos(), ["jump", "released"]);
    assert_eq!(p.ovar(e, "Held").as_int(), 3);
}

#[test]
fn custom_events_reach_graphs_and_lua_both_ways_and_on_destroy_runs() {
    let root = project("custom");
    // A sends Hit(5) to B and broadcasts Ping; then destroys C.
    let mut a = G::new();
    let start = a.n("OnStart");
    let find = a.api("GameObject", "Find", "call");
    a.v(find, "Name", Literal::String("B".into()));
    let send = a.np("SendCustomEvent", &[("name", "Hit"), ("args", "amount")]);
    a.w(find, "Result", send, "Target");
    a.v(send, "amount", Literal::Int(5));
    let ping = a.np("BroadcastCustomEvent", &[("name", "Ping")]);
    let find_c = a.api("GameObject", "Find", "call");
    a.v(find_c, "Name", Literal::String("C".into()));
    let destroy = a.api("", "Destroy", "call");
    a.w(find_c, "Result", destroy, "Object");
    chain(&mut a, start, "Out", &[send, ping, destroy]);
    a.0.save(&root.join("Assets/A.vgraph")).unwrap();
    // B logs Hit and Ping.
    let mut b = G::new();
    let hit = b.np("OnCustomEvent", &[("name", "Hit"), ("args", "amount")]);
    let text = b.n("Append");
    b.v(text, "A", Literal::String("graph hit ".into()));
    b.w(hit, "amount", text, "B");
    let log = b.log();
    b.w(text, "Result", log, "Message");
    b.w(hit, "Out", log, "In");
    let ping_in = b.np("OnCustomEvent", &[("name", "Ping")]);
    let log2 = b.log_text("graph ping");
    b.w(ping_in, "Out", log2, "In");
    b.0.save(&root.join("Assets/B.vgraph")).unwrap();
    // C says goodbye.
    let mut c = G::new();
    let bye = c.n("OnDestroy");
    let log3 = c.log_text("graph bye");
    c.w(bye, "Out", log3, "In");
    c.0.save(&root.join("Assets/C.vgraph")).unwrap();
    // Lua on B hears Hit; Lua on D sends Hit(2) to B from Lua.
    std::fs::write(
        root.join("Assets/Listener.lua"),
        "local L = {}\nfunction L:Hit(amount) Debug.Log('lua hit ' .. amount) end\nreturn L\n",
    )
    .unwrap();
    std::fs::write(
        root.join("Assets/Sender.lua"),
        "local S = {}\nfunction S:Update()\n  if Time.frameCount == 3 then GameObject.Find('B'):SendCustomEvent('Hit', 2) end\nend\nreturn S\n",
    )
    .unwrap();
    let mut scene = Scene::new("t");
    let ea = scene.spawn("A", None);
    attach(&mut scene, ea, "Assets/A.vgraph", &[]);
    let eb = scene.spawn("B", None);
    attach(&mut scene, eb, "Assets/B.vgraph", &[]);
    attach(&mut scene, eb, "Assets/Listener.lua", &[]);
    let ec = scene.spawn("C", None);
    attach(&mut scene, ec, "Assets/C.vgraph", &[]);
    let ed = scene.spawn("D", None);
    attach(&mut scene, ed, "Assets/Sender.lua", &[]);
    let mut p = Play::start(root, scene);
    p.frames(4);
    p.no_errors();
    assert_eq!(
        p.infos(),
        [
            "graph hit 5",
            "lua hit 5",
            "graph ping",
            "graph bye",
            "graph hit 2",
            "lua hit 2"
        ]
    );
    assert!(!p.scene.contains(ec));
}

// ---- math and logic -----------------------------------------------------------------------------

/// Start → Log(each value) in order.
fn log_values(g: &mut G, values: &[(u32, &str)]) {
    let start = g.n("OnStart");
    let mut logs = Vec::new();
    for (n, pin) in values {
        let l = g.log();
        g.w(*n, pin, l, "Message");
        logs.push(l);
    }
    chain(g, start, "Out", &logs);
}

#[test]
fn math_logic_vector_and_string_nodes_compute() {
    let mut g = G::new();
    let add = g.n("AddFloat");
    g.f(add, "A", 2.0);
    g.f(add, "B", 3.0);
    let mul = g.n("MultiplyFloat");
    g.w(add, "Result", mul, "A");
    g.f(mul, "B", 4.0);
    let lerp = g.n("Lerp");
    g.f(lerp, "B", 10.0);
    g.f(lerp, "T", 0.25);
    let clamp = g.n("ClampFloat");
    g.f(clamp, "Value", 15.0);
    g.f(clamp, "Max", 10.0);
    let div = g.n("DivideInt");
    g.v(div, "A", Literal::Int(7));
    g.v(div, "B", Literal::Int(2));
    let modulo = g.n("ModuloInt");
    g.v(modulo, "A", Literal::Int(7));
    g.v(modulo, "B", Literal::Int(3));
    let gt = g.n("Greater");
    g.f(gt, "A", 3.0);
    g.f(gt, "B", 2.0);
    let not = g.n("Not");
    g.w(gt, "Result", not, "Value");
    let sel = g.n("Select");
    g.w(not, "Result", sel, "Condition");
    g.v(sel, "True", Literal::String("a".into()));
    g.v(sel, "False", Literal::String("b".into()));
    let text = g.n("Append");
    g.v(text, "A", Literal::String("Score: ".into()));
    g.w(div, "Result", text, "B");
    let dist = g.n("Distance");
    g.v(dist, "B", Literal::Vector3([3.0, 4.0, 0.0]));
    let euler = g.n("Euler");
    g.v(euler, "Degrees", Literal::Vector3([0.0, 90.0, 0.0]));
    let rot = g.n("RotateVector");
    g.w(euler, "Result", rot, "Rotation");
    let len = g.n("Magnitude");
    g.w(rot, "Result", len, "Vector");
    let round = g.n("RoundToInt");
    g.w(len, "Result", round, "Value");
    let mk = g.n("MakeVector3");
    g.f(mk, "X", 1.0);
    g.f(mk, "Y", 2.0);
    let brk = g.n("BreakVector3");
    g.w(mk, "Vector", brk, "Vector");
    let eq = g.n("Equal");
    g.w(brk, "Y", eq, "A");
    g.v(eq, "B", Literal::Int(2));
    let pp = g.n("PingPong");
    g.f(pp, "T", 3.5);
    g.f(pp, "Length", 2.0);
    log_values(
        &mut g,
        &[
            (mul, "Result"),
            (lerp, "Result"),
            (clamp, "Result"),
            (div, "Result"),
            (modulo, "Result"),
            (gt, "Result"),
            (sel, "Result"),
            (text, "Result"),
            (dist, "Result"),
            (round, "Result"),
            (eq, "Result"),
            (pp, "Result"),
        ],
    );
    let (mut p, _) = play("math", &g.0);
    p.frames(1);
    p.no_errors();
    assert_eq!(
        p.infos(),
        [
            "20", "2.5", "10", "3", "1", "true", "b", "Score: 3", "5", "1", "true", "0.5"
        ]
    );
}

#[test]
fn random_quaternion_and_list_nodes() {
    let mut g = G::new();
    g.ovar("R", PinType::Float, Literal::Float(0.0));
    g.ovar("I", PinType::Int, Literal::Int(0));
    g.ovar("Back", PinType::Vector3, Literal::Vector3([0.0; 3]));
    g.ovar("N", PinType::Int, Literal::Int(0));
    g.ovar("Second", PinType::Any, Literal::None);
    let start = g.n("OnStart");
    let r = g.n("RandomFloat");
    g.f(r, "Min", 5.0);
    g.f(r, "Max", 6.0);
    let sr = g.set("R");
    g.w(r, "Result", sr, "Value");
    let ri = g.n("RandomInt");
    g.v(ri, "Min", Literal::Int(3));
    g.v(ri, "Max", Literal::Int(4));
    let si = g.set("I");
    g.w(ri, "Result", si, "Value");
    // Euler → quaternion → Euler.
    let q = g.n("Euler");
    g.v(q, "Degrees", Literal::Vector3([10.0, 20.0, 30.0]));
    let back = g.n("ToEuler");
    g.w(q, "Result", back, "Rotation");
    let sb = g.set("Back");
    g.w(back, "Result", sb, "Value");
    let list = g.np("MakeList", &[("count", "3")]);
    g.v(list, "Item 0", Literal::Int(1));
    g.v(list, "Item 1", Literal::String("two".into()));
    g.v(list, "Item 2", Literal::Float(3.0));
    let count = g.n("ListLength");
    g.w(list, "List", count, "List");
    let sn = g.set("N");
    g.w(count, "Result", sn, "Value");
    let item = g.n("ListGet");
    g.w(list, "List", item, "List");
    g.v(item, "Index", Literal::Int(1));
    let s2 = g.set("Second");
    g.w(item, "Item", s2, "Value");
    chain(&mut g, start, "Out", &[sr, si, sb, sn, s2]);
    let (mut p, e) = play("random", &g.0);
    p.frames(1);
    p.no_errors();
    let r = p.ovar(e, "R").as_float();
    assert!((5.0..6.0).contains(&r), "{r}");
    assert_eq!(p.ovar(e, "I").as_int(), 3);
    let b = p.ovar(e, "Back").as_vec3();
    assert!((b - Vec3::new(10.0, 20.0, 30.0)).length() < 0.01, "{b}");
    assert_eq!(p.ovar(e, "N").as_int(), 3);
    assert_eq!(p.ovar(e, "Second").to_text(None), "two");
}

// ---- flow control -------------------------------------------------------------------------------

#[test]
fn branch_sequence_and_for_loop_with_break() {
    let mut g = G::new();
    g.var("Sum", PinType::Int, Literal::Int(0));
    let start = g.n("OnStart");
    let seq = g.np("Sequence", &[("count", "3")]);
    g.w(start, "Out", seq, "In");
    // Then 0: branch on 5 > 3.
    let gt = g.n("Greater");
    g.f(gt, "A", 5.0);
    g.f(gt, "B", 3.0);
    let br = g.n("Branch");
    g.w(gt, "Result", br, "Condition");
    g.w(seq, "Then 0", br, "In");
    let yes = g.log_text("yes");
    let no = g.log_text("no");
    g.w(br, "True", yes, "In");
    g.w(br, "False", no, "In");
    // Then 1: for 0..9, sum the indexes; log the sum when completed.
    let lp = g.n("ForLoop");
    g.w(seq, "Then 1", lp, "In");
    let get = g.get("Sum");
    let add = g.n("AddInt");
    g.w(get, "Value", add, "A");
    g.w(lp, "Index", add, "B");
    let set = g.set("Sum");
    g.w(add, "Result", set, "Value");
    g.w(lp, "Loop Body", set, "In");
    let total = g.log();
    let get2 = g.get("Sum");
    g.w(get2, "Value", total, "Message");
    g.w(lp, "Completed", total, "In");
    // Then 2: loop from 0, log the index, break at 3.
    let lp2 = g.n("ForLoop");
    g.v(lp2, "Last", Literal::Int(100));
    g.w(seq, "Then 2", lp2, "In");
    let ge = g.n("GreaterOrEqual");
    g.w(lp2, "Index", ge, "A");
    g.f(ge, "B", 3.0);
    let br2 = g.n("Branch");
    g.w(ge, "Result", br2, "Condition");
    g.w(lp2, "Loop Body", br2, "In");
    g.w(br2, "True", lp2, "Break");
    let li = g.log();
    g.w(lp2, "Index", li, "Message");
    g.w(br2, "False", li, "In");
    let done = g.log_text("done");
    g.w(lp2, "Completed", done, "In");
    let (mut p, _) = play("flow", &g.0);
    p.frames(2);
    p.no_errors();
    assert_eq!(p.infos(), ["yes", "45", "0", "1", "2", "done"]);
}

#[test]
fn for_each_and_while_loops() {
    let mut g = G::new();
    g.var("Sum", PinType::Float, Literal::Float(0.0));
    g.var("Counter", PinType::Int, Literal::Int(0));
    let start = g.n("OnStart");
    let list = g.np("MakeList", &[("count", "3")]);
    for (i, x) in [1.0, 2.0, 3.5].iter().enumerate() {
        g.v(list, &format!("Item {i}"), Literal::Float(*x));
    }
    let each = g.n("ForEach");
    g.w(list, "List", each, "List");
    g.w(start, "Out", each, "In");
    let get = g.get("Sum");
    let add = g.n("AddFloat");
    g.w(get, "Value", add, "A");
    g.w(each, "Item", add, "B");
    let set = g.set("Sum");
    g.w(add, "Result", set, "Value");
    g.w(each, "Loop Body", set, "In");
    let wl = g.n("While");
    g.w(each, "Completed", wl, "In");
    let c = g.get("Counter");
    let less = g.n("Less");
    g.w(c, "Value", less, "A");
    g.f(less, "B", 5.0);
    g.w(less, "Result", wl, "Condition");
    let inc = g.increment("Counter");
    g.w(wl, "Loop Body", inc, "In");
    let l1 = g.log();
    let s = g.get("Sum");
    g.w(s, "Value", l1, "Message");
    let l2 = g.log();
    let c2 = g.get("Counter");
    g.w(c2, "Value", l2, "Message");
    g.w(wl, "Completed", l1, "In");
    g.w(l1, "Out", l2, "In");
    let (mut p, _) = play("loops", &g.0);
    p.frames(1);
    p.no_errors();
    assert_eq!(p.infos(), ["6.5", "5"]);
}

#[test]
fn while_guard_stops_a_loop_that_never_ends() {
    let mut g = G::new();
    let start = g.n("OnStart");
    let wl = g.n("While");
    g.v(wl, "Condition", Literal::Bool(true));
    g.v(wl, "Max Iterations", Literal::Int(100));
    g.w(start, "Out", wl, "In");
    let after = g.log_text("never");
    g.w(wl, "Completed", after, "In");
    let (mut p, _) = play("guard", &g.0);
    p.frames(3);
    let errors = p.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("While looped 100 times"), "{errors:?}");
    assert!(errors[0].contains("'While'"), "{errors:?}");
    assert!(p.infos().is_empty());
}

#[test]
fn do_once_gate_and_flip_flop_keep_their_state() {
    let mut g = G::new();
    let update = g.n("OnUpdate");
    let seq = g.np("Sequence", &[("count", "4")]);
    g.w(update, "Out", seq, "In");
    // Do Once, reset on frame 3.
    let once = g.n("DoOnce");
    g.w(seq, "Then 0", once, "In");
    let l_once = g.log_text("once");
    g.w(once, "Completed", l_once, "In");
    let frame = g.api("Time", "frameCount", "get");
    let eq = g.n("Equal");
    g.w(frame, "Value", eq, "A");
    g.v(eq, "B", Literal::Int(3));
    let br = g.n("Branch");
    g.w(eq, "Result", br, "Condition");
    g.w(seq, "Then 1", br, "In");
    g.w(br, "True", once, "Reset");
    // A gate that starts closed and toggles after each try.
    let gate = g.np("Gate", &[("start_closed", "true")]);
    g.w(seq, "Then 2", gate, "Enter");
    let l_gate = g.log_text("pass");
    g.w(gate, "Exit", l_gate, "In");
    let ff = g.n("FlipFlop");
    g.w(seq, "Then 3", ff, "In");
    let la = g.log_text("A");
    let lb = g.log_text("B");
    g.w(ff, "A", la, "In");
    g.w(ff, "B", lb, "In");
    g.w(la, "Out", gate, "Toggle");
    g.w(lb, "Out", gate, "Toggle");
    let (mut p, _) = play("state", &g.0);
    p.frames(4);
    p.no_errors();
    assert_eq!(p.infos(), expected_state_logs());
}

/// Frame by frame: Do Once fires on frames 1 and 4 (reset on frame 3); the gate starts closed
/// and is toggled after each try, so frames 2 and 4 pass; the flip flop alternates A, B.
fn expected_state_logs() -> Vec<String> {
    let mut out = Vec::new();
    for frame in 1..=4 {
        if frame == 1 || frame == 4 {
            out.push("once");
        }
        if frame % 2 == 0 {
            out.push("pass");
        }
        out.push(if frame % 2 == 1 { "A" } else { "B" });
    }
    out.into_iter().map(str::to_owned).collect()
}

#[test]
fn delay_resumes_later_and_ignores_triggers_while_pending() {
    let mut g = G::new();
    g.ovar("Ticks", PinType::Int, Literal::Int(0));
    let start = g.n("OnStart");
    let d = g.n("Delay");
    g.f(d, "Duration", 0.5);
    g.w(start, "Out", d, "In");
    let late = g.log_text("late");
    g.w(d, "Completed", late, "In");
    let update = g.n("OnUpdate");
    let d2 = g.n("Delay");
    g.f(d2, "Duration", 0.25);
    g.w(update, "Out", d2, "In");
    let inc = g.increment("Ticks");
    g.w(d2, "Completed", inc, "In");
    // A Delay inside a Sequence lets the next branch run right away.
    let seq = g.np("Sequence", &[("count", "2")]);
    let start2 = g.n("OnStart");
    g.w(start2, "Out", seq, "In");
    let d3 = g.n("Delay");
    g.f(d3, "Duration", 0.1);
    g.w(seq, "Then 0", d3, "In");
    let now = g.log_text("then 1");
    g.w(seq, "Then 1", now, "In");
    let (mut p, e) = play("delay", &g.0);
    p.frames(25);
    assert_eq!(p.infos(), ["then 1"]);
    p.frames(10);
    assert_eq!(p.infos(), ["then 1", "late"]);
    p.frames(25);
    p.no_errors();
    let ticks = p.ovar(e, "Ticks").as_int();
    assert!((3..=4).contains(&ticks), "ticks {ticks}");
}

// ---- variables ----------------------------------------------------------------------------------

#[test]
fn graph_and_object_variables_with_inspector_values_and_lua() {
    let root = project("vars");
    let mut g = G::new();
    g.var("Local", PinType::Int, Literal::Int(7));
    g.ovar("Health", PinType::Float, Literal::Float(100.0));
    let start = g.n("OnStart");
    let lh = g.log();
    let h = g.get("Health");
    g.w(h, "Value", lh, "Message");
    let ll = g.log();
    let l = g.get("Local");
    g.w(l, "Value", ll, "Message");
    // The Lua script's variable, read by name.
    let lo = g.log();
    let other = g.n("GetObjectVariable");
    g.v(other, "Name", Literal::String("FromLua".into()));
    g.w(other, "Value", lo, "Message");
    // Another object's variable.
    let lf = g.log();
    let find = g.api("GameObject", "Find", "call");
    g.v(find, "Name", Literal::String("Other".into()));
    let theirs = g.n("GetObjectVariable");
    g.w(find, "Result", theirs, "Target");
    g.v(theirs, "Name", Literal::String("Speed".into()));
    g.w(theirs, "Value", lf, "Message");
    chain(&mut g, start, "Out", &[lh, ll, lo, lf]);
    // Each frame Health drops by 1.
    let update = g.n("OnUpdate");
    let h2 = g.get("Health");
    let sub = g.n("SubtractFloat");
    g.w(h2, "Value", sub, "A");
    g.f(sub, "B", 1.0);
    let set = g.set("Health");
    g.w(sub, "Result", set, "Value");
    g.w(update, "Out", set, "In");
    g.0.save(&root.join(GRAPH)).unwrap();
    let mut other_g = G::new();
    other_g.ovar("Speed", PinType::Float, Literal::Float(7.5));
    other_g.0.save(&root.join("Assets/Other.vgraph")).unwrap();
    std::fs::write(
        root.join("Assets/Reader.lua"),
        "local R = {}\nfunction R:Awake() self.gameObject:SetVariable('FromLua', 'hello') end\n\
         function R:LateUpdate() if Time.frameCount == 2 then Debug.Log('lua sees ' .. self.gameObject:GetVariable('Health')) end end\nreturn R\n",
    )
    .unwrap();
    let mut scene = Scene::new("t");
    let e = scene.spawn("Thing", None);
    attach(
        &mut scene,
        e,
        GRAPH,
        &[("Health", ScriptValue::Number(50.0))],
    );
    attach(&mut scene, e, "Assets/Reader.lua", &[]);
    let o = scene.spawn("Other", None);
    attach(&mut scene, o, "Assets/Other.vgraph", &[]);
    let mut p = Play::start(root, scene);
    p.frames(2);
    p.no_errors();
    assert_eq!(p.infos(), ["50", "7", "hello", "7.5", "lua sees 48.0"]);
    assert_eq!(p.ovar(e, "Health").as_float(), 48.0);
}

// ---- engine API and Lua -------------------------------------------------------------------------

#[test]
fn engine_api_nodes_run_natively_or_through_lua() {
    let root = project("api");
    let mut g = G::new();
    let start = g.n("OnStart");
    let setp = g.api("#Transform", "position", "set");
    g.v(setp, "Value", Literal::Vector3([1.0, 2.0, 3.0]));
    let getp = g.api("#Transform", "position", "get");
    let l1 = g.log();
    g.w(getp, "Value", l1, "Message");
    // Not native: through the Lua binding.
    let layer = g.api("#GameObject", "layer", "get");
    let l2 = g.log();
    g.w(layer, "Value", l2, "Message");
    let mass = g.api("#Rigidbody", "mass", "set");
    g.f(mass, "Value", 3.0);
    let getm = g.api("#Rigidbody", "mass", "get");
    let l3 = g.log();
    g.w(getm, "Value", l3, "Message");
    let dist = g.api_args("Vector3", "Distance", "a, b");
    g.v(dist, "a", Literal::Vector3([0.0; 3]));
    g.v(dist, "b", Literal::Vector3([0.0, 0.0, 2.0]));
    let l4 = g.log();
    g.w(dist, "Result", l4, "Message");
    let field = g.n("GetLuaField");
    g.v(field, "Object", Literal::None);
    let me = g.n("Self");
    g.w(me, "Self", field, "Object");
    g.v(field, "Field", Literal::String("name".into()));
    let l5 = g.log();
    g.w(field, "Value", l5, "Message");
    // Rotate natively.
    let rot = g.api("#Transform", "Rotate", "call");
    g.v(rot, "Euler Degrees", Literal::Vector3([0.0, 90.0, 0.0]));
    chain(&mut g, start, "Out", &[setp, l1, l2, mass, l3, l4, l5, rot]);
    g.0.save(&root.join(GRAPH)).unwrap();
    let mut scene = Scene::new("t");
    let e = scene.spawn("Thing", None);
    scene.add(e, Rigidbody::default());
    scene.add(e, BoxCollider::default());
    attach(&mut scene, e, GRAPH, &[]);
    let mut p = Play::start(root, scene);
    p.frames(1);
    p.no_errors();
    assert_eq!(p.infos(), ["(1.00, 2.00, 3.00)", "0", "3", "2", "Thing"]);
    let yaw = p.scene.transform(e).euler().y;
    assert!((yaw - 90.0).abs() < 0.5, "{yaw}");
}

#[test]
fn call_lua_function_returns_its_result_and_reports_a_missing_script() {
    let root = project("calllua");
    std::fs::write(
        root.join("Assets/Calc.lua"),
        "local C = { base = 10 }\nfunction C:Add(a, b) return self.base + a + b end\nreturn C\n",
    )
    .unwrap();
    let mut g = G::new();
    let start = g.n("OnStart");
    let call = g.np(
        "CallLua",
        &[("script", "Calc"), ("function", "Add"), ("args", "a, b")],
    );
    g.v(call, "a", Literal::Int(2));
    g.v(call, "b", Literal::Float(3.5));
    let log = g.log();
    g.w(call, "Result", log, "Message");
    let missing = g.np("CallLua", &[("script", "Nope"), ("function", "Go")]);
    chain(&mut g, start, "Out", &[call, log, missing]);
    g.0.save(&root.join(GRAPH)).unwrap();
    let mut scene = Scene::new("t");
    let e = scene.spawn("Thing", None);
    attach(&mut scene, e, "Assets/Calc.lua", &[]);
    attach(&mut scene, e, GRAPH, &[]);
    let mut p = Play::start(root, scene);
    p.frames(2);
    assert_eq!(p.infos(), ["15.5"]);
    let errors = p.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("no Lua script 'Nope'"), "{errors:?}");
}

// ---- subgraphs ----------------------------------------------------------------------------------

/// A macro doubling X into Y, counting its uses in a private variable (Uses out).
fn doubler() -> ScriptGraph {
    let mut m = G::new();
    m.var("Count", PinType::Int, Literal::Int(0));
    let input = m.np("GraphInput", &[("pins", "In:exec, X:float")]);
    let output = m.np("GraphOutput", &[("pins", "Out:exec, Y:float, Uses:int")]);
    let mul = m.n("MultiplyFloat");
    m.w(input, "X", mul, "A");
    m.f(mul, "B", 2.0);
    m.w(mul, "Result", output, "Y");
    let inc = m.increment("Count");
    m.w(input, "In", inc, "In");
    m.w(inc, "Out", output, "Out");
    let c = m.get("Count");
    m.w(c, "Value", output, "Uses");
    m.0
}

#[test]
fn macros_and_subgraph_assets_inline_with_their_own_state() {
    let root = project("subgraphs");
    doubler().save(&root.join("Assets/Doubler.vgraph")).unwrap();
    let mut g = G::new();
    g.0.macros.push(Macro {
        name: "Double".into(),
        graph: doubler(),
    });
    let start = g.n("OnStart");
    let m1 = g.np("Subgraph", &[("macro", "Double")]);
    g.f(m1, "X", 21.0);
    let l1 = g.log();
    g.w(m1, "Y", l1, "Message");
    // The same macro again, fed by the first.
    let m2 = g.np("Subgraph", &[("macro", "Double")]);
    g.w(m1, "Y", m2, "X");
    let l2 = g.log();
    g.w(m2, "Y", l2, "Message");
    // The asset version.
    let a = g.np("Subgraph", &[("path", "Assets/Doubler.vgraph")]);
    g.f(a, "X", 1.5);
    let l3 = g.log();
    // (Wired by hand: the builder checks pins without the project's files.)
    g.0.connections.push(Connection {
        from: a,
        output: "Y".into(),
        to: l3,
        input: "Message".into(),
    });
    chain(&mut g, start, "Out", &[m1, l1, m2]);
    for (from, out, to, inp) in [
        (m2, "Out", l2, "In"),
        (l2, "Out", a, "In"),
        (a, "Out", l3, "In"),
    ] {
        g.0.connections.push(Connection {
            from,
            output: out.into(),
            to,
            input: inp.into(),
        });
    }
    // Every frame a third use counts its own runs.
    let update = g.n("OnUpdate");
    let m3 = g.np("Subgraph", &[("macro", "Double")]);
    let l4 = g.log();
    g.w(m3, "Uses", l4, "Message");
    chain(&mut g, update, "Out", &[m3, l4]);
    g.0.save(&root.join(GRAPH)).unwrap();
    let mut scene = Scene::new("t");
    let e = scene.spawn("Thing", None);
    attach(&mut scene, e, GRAPH, &[]);
    let mut p = Play::start(root, scene);
    p.frames(3);
    p.no_errors();
    assert_eq!(p.infos(), ["42", "84", "3", "1", "2", "3"]);
    // A subgraph using itself is refused.
    let mut rec = G::new();
    let mut inner = doubler();
    inner.add_node_with("Subgraph", &[("macro", "Loop")], [0.0, 0.0]);
    rec.0.macros.push(Macro {
        name: "Loop".into(),
        graph: inner.clone(),
    });
    // The macro list travels with the inner graph so it can name itself.
    rec.0.macros[0].graph.macros.push(Macro {
        name: "Loop".into(),
        graph: inner,
    });
    let s = rec.np("Subgraph", &[("macro", "Loop")]);
    let errs = compile::check(&rec.0, &NoFiles);
    assert!(
        errs.iter()
            .any(|e| e.node == Some(s) && e.message.contains("uses itself")),
        "{errs:?}"
    );
    // A missing asset is reported on the node.
    let mut missing = G::new();
    let m = missing.np("Subgraph", &[("path", "Assets/Gone.vgraph")]);
    let errs = compile::check(&missing.0, &FileResolver(project("gone")));
    assert!(errs.iter().any(|e| e.node == Some(m)), "{errs:?}");
}

// ---- errors -------------------------------------------------------------------------------------

#[test]
fn compile_errors_point_at_their_nodes() {
    let mut g = G::new();
    let start = g.n("OnStart");
    let missing = g.np("GetVariable", &[("name", "Nope")]);
    let unknown = g.n("Bogus");
    let br = g.n("Branch");
    g.w(start, "Out", br, "In");
    let v = g.n("MakeVector3");
    // A Vector3 into a Bool input: refused when wiring…
    assert!(g.0.connect(v, "Vector", br, "Condition", &NoFiles).is_err());
    // …and reported when a file has it anyway.
    g.0.connections.push(Connection {
        from: v,
        output: "Vector".into(),
        to: br,
        input: "Condition".into(),
    });
    // Two adds feeding each other.
    let a = g.n("AddFloat");
    let b = g.n("AddFloat");
    g.w(a, "Result", b, "A");
    g.w(b, "Result", a, "A");
    let log = g.log();
    g.w(a, "Result", log, "Message");
    g.w(br, "True", log, "In");
    let each = g.n("ForEach");
    g.w(br, "False", each, "In");
    let errs = compile::check(&g.0, &NoFiles);
    let on = |n: u32, text: &str| {
        errs.iter()
            .any(|e| e.node == Some(n) && e.message.contains(text))
    };
    assert!(on(missing, "no variable named 'Nope'"), "{errs:?}");
    assert!(on(unknown, "unknown node type 'Bogus'"), "{errs:?}");
    assert!(on(br, "cannot take"), "{errs:?}");
    assert!(on(a, "loop back") || on(b, "loop back"), "{errs:?}");
    assert!(on(each, "connect a List"), "{errs:?}");
    // A graph that doesn't compile logs its problems when played and runs nothing.
    let (mut p, _) = play("errors", &g.0);
    p.frames(2);
    let errors = p.errors();
    assert!(!errors.is_empty());
    assert!(
        errors.iter().all(|e| e.contains("does not compile")),
        "{errors:?}"
    );
}

/// Constant nodes (and pass-through subgraphs) wired into each other: the compiler reports
/// the loop instead of recursing until the stack overflows (the editor compiles on every
/// wire, so this crashed it the moment the second wire was made).
#[test]
fn constant_and_tunnel_loops_are_reported() {
    let mut g = G::new();
    let a = g.n("Float");
    let b = g.n("Float");
    g.w(a, "Value", b, "Value");
    g.w(b, "Value", a, "Value");
    let start = g.n("OnStart");
    let log = g.log();
    g.w(start, "Out", log, "In");
    g.w(a, "Value", log, "Message");
    let errs = compile::check(&g.0, &NoFiles);
    assert!(
        errs.iter()
            .any(|e| (e.node == Some(a) || e.node == Some(b)) && e.message.contains("loop back")),
        "{errs:?}"
    );
    // The same through two uses of a macro that passes its input straight out.
    let mut pass = G::new();
    let input = pass.np("GraphInput", &[("pins", "X:float")]);
    let output = pass.np("GraphOutput", &[("pins", "Y:float")]);
    pass.w(input, "X", output, "Y");
    let mut g = G::new();
    g.0.macros.push(Macro {
        name: "Pass".into(),
        graph: pass.0,
    });
    let m1 = g.np("Subgraph", &[("macro", "Pass")]);
    let m2 = g.np("Subgraph", &[("macro", "Pass")]);
    g.w(m1, "Y", m2, "X");
    g.w(m2, "Y", m1, "X");
    let start = g.n("OnStart");
    let log = g.log();
    g.w(start, "Out", log, "In");
    g.w(m1, "Y", log, "Message");
    let errs = compile::check(&g.0, &NoFiles);
    assert!(
        errs.iter().any(|e| e.message.contains("loop back")),
        "{errs:?}"
    );
}

/// A Lua value holding itself (`t[1] = t`) or a table shared many times per level reaches a
/// graph without recursing forever: the cycle (or what is past the item budget) stays an
/// opaque Lua value.
#[test]
fn lua_tables_that_hold_themselves_convert_without_overflowing() {
    let lua = mlua::Lua::new();
    let t: mlua::Table = lua
        .load("local t = {} t[1] = t t[2] = t return t")
        .eval()
        .unwrap();
    match lua_bridge::from_lua(&lua, mlua::Value::Table(t)) {
        Value::List(l) => {
            assert_eq!(l.len(), 2);
            assert!(matches!(l[0], Value::Lua(_)), "{:?}", l[0]);
        }
        other => panic!("{other:?}"),
    }
    // 4 references at each of 40 levels: 4^40 values if walked naively.
    let wide: mlua::Table = lua
        .load("local cur = {1, 2, 3} for _ = 1, 40 do cur = {cur, cur, cur, cur} end return cur")
        .eval()
        .unwrap();
    let start = std::time::Instant::now();
    assert!(matches!(
        lua_bridge::from_lua(&lua, mlua::Value::Table(wide)),
        Value::List(_) | Value::Lua(_)
    ));
    assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
}

#[test]
fn run_time_errors_are_logged_once_with_the_node() {
    let mut g = G::new();
    let update = g.n("OnUpdate");
    let div = g.n("DivideInt");
    g.v(div, "A", Literal::Int(1));
    let log = g.log();
    g.w(div, "Result", log, "Message");
    g.w(update, "Out", log, "In");
    let (mut p, _) = play("runtime_errors", &g.0);
    p.frames(10);
    let errors = p.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("Thing: Script Graph Assets/G.vgraph, node 'Divide'")
            && errors[0].contains("division by zero"),
        "{errors:?}"
    );
    assert!(p.infos().is_empty());
}

// ---- files --------------------------------------------------------------------------------------

#[test]
fn graphs_round_trip_through_json_and_files() {
    let mut g = G::new();
    g.var("B", PinType::Bool, Literal::Bool(true));
    g.var("I", PinType::Int, Literal::Int(-3));
    g.ovar("F", PinType::Float, Literal::Float(0.1));
    g.var("S", PinType::String, Literal::String("hi \"there\"".into()));
    g.var("V", PinType::Vector3, Literal::Vector3([1.0, -2.5, 3.25]));
    g.var(
        "Q",
        PinType::Quaternion,
        Literal::Quaternion([0.0, 45.0, 0.0]),
    );
    g.var("O", PinType::GameObject, Literal::None);
    let start = g.n("OnStart");
    let l = g.log_text("x");
    g.w(start, "Out", l, "In");
    g.0.groups.push(Group {
        title: "Start up".into(),
        nodes: vec![start, l],
    });
    g.0.notes.push(StickyNote {
        position: [10.0, 20.0],
        size: [200.0, 100.0],
        text: "A note".into(),
    });
    g.0.macros.push(Macro {
        name: "Double".into(),
        graph: doubler(),
    });
    let text = g.0.to_json();
    let back = ScriptGraph::from_json(&text).unwrap();
    assert_eq!(back, g.0);
    assert_eq!(back.to_json(), text);
    let root = project("roundtrip");
    let path = root.join("Assets/R.vgraph");
    g.0.save(&path).unwrap();
    assert_eq!(ScriptGraph::load(&path).unwrap(), g.0);
    // The Inspector fields are the Object variables.
    assert_eq!(
        inspect_fields(&g.0),
        [("F".to_owned(), ScriptValue::Number(f64::from(0.1f32)))]
    );
    let a = compile(&g.0, &NoFiles).unwrap();
    let b = compile(&back, &NoFiles).unwrap();
    assert_eq!(a.code.len(), b.code.len());
}

// ---- debugger -----------------------------------------------------------------------------------

#[test]
fn breakpoints_pause_a_flow_and_stepping_runs_it_node_by_node() {
    let mut g = G::new();
    let update = g.n("OnUpdate");
    let a = g.log_text("a");
    let add = g.n("AddFloat");
    g.f(add, "A", 2.0);
    g.f(add, "B", 3.0);
    let b = g.log();
    g.w(add, "Result", b, "Message");
    let c = g.log_text("c");
    chain(&mut g, update, "Out", &[a, b, c]);
    let (mut p, e) = play("debug", &g.0);
    {
        let d = p.host.graph_debugger();
        d.path = Some(GRAPH.into());
        d.breakpoints.insert(b);
        d.watch = Some(e);
    }
    p.frames(1);
    assert_eq!(p.infos(), ["a"]);
    let paused = p.host.graph_debugger().paused.as_ref().map(|x| x.node);
    assert_eq!(paused, Some(b));
    assert!(p.host.graph_debugger().hits.keys().any(|(n, _)| *n == a));
    // Step: B runs and the flow stops before C.
    p.host.graph_continue(&mut p.scene, true);
    p.logs.extend(p.host.take_logs());
    assert_eq!(p.infos(), ["a", "5"]);
    assert_eq!(
        p.host.graph_debugger().paused.as_ref().map(|x| x.node),
        Some(c)
    );
    // The values on the wires.
    assert_eq!(
        p.host.graph_value(add, "Result", true).as_deref(),
        Some("5")
    );
    assert_eq!(
        p.host.graph_value(b, "Message", false).as_deref(),
        Some("5")
    );
    // Continue: C runs, nothing is paused; the next frame stops at B again.
    p.host.graph_continue(&mut p.scene, false);
    p.logs.extend(p.host.take_logs());
    assert_eq!(p.infos(), ["a", "5", "c"]);
    assert!(p.host.graph_debugger().paused.is_none());
    p.frames(1);
    assert_eq!(p.infos(), ["a", "5", "c", "a"]);
    assert!(p.host.graph_debugger().paused.is_some());
    // Without breakpoints the paused flow finishes at the start of the next frame.
    p.host.graph_debugger().breakpoints.clear();
    p.frames(1);
    p.no_errors();
    assert_eq!(p.infos(), ["a", "5", "c", "a", "5", "c", "a", "5", "c"]);
}

// ---- performance --------------------------------------------------------------------------------

/// A 100-node graph: On Update runs ten stages, each a Set Variable fed by eight pure math
/// nodes, then moves the object.
fn busy_graph() -> ScriptGraph {
    let mut g = G::new();
    let update = g.n("OnUpdate");
    let mut prev = (update, "Out".to_owned());
    for k in 0..10 {
        let name = format!("Acc{k}");
        g.var(&name, PinType::Float, Literal::Float(k as f32));
        let get = g.get(&name);
        let add = g.n("AddFloat");
        g.w(get, "Value", add, "A");
        g.w(update, "Delta Time", add, "B");
        let mul = g.n("MultiplyFloat");
        g.w(add, "Result", mul, "A");
        g.f(mul, "B", 0.5);
        let sin = g.n("Sin");
        g.w(mul, "Result", sin, "Radians");
        let abs = g.n("AbsFloat");
        g.w(sin, "Result", abs, "Value");
        let max = g.n("MaxFloat");
        g.w(abs, "Result", max, "A");
        g.f(max, "B", 0.1);
        let lerp = g.n("Lerp");
        g.w(get, "Value", lerp, "A");
        g.w(max, "Result", lerp, "B");
        let clamp = g.n("ClampFloat");
        g.w(lerp, "Result", clamp, "Value");
        g.f(clamp, "Min", -100.0);
        g.f(clamp, "Max", 100.0);
        let set = g.set(&name);
        g.w(clamp, "Result", set, "Value");
        g.w(prev.0, &prev.1, set, "In");
        prev = (set, "Out".to_owned());
    }
    let last = g.get("Acc9");
    let mk = g.n("MakeVector3");
    g.w(last, "Value", mk, "X");
    let gt = g.n("Greater");
    g.w(last, "Value", gt, "A");
    let br = g.n("Branch");
    g.w(gt, "Result", br, "Condition");
    g.w(prev.0, &prev.1, br, "In");
    let t = g.api("Time", "time", "get");
    let wave = g.n("Sin");
    g.w(t, "Value", wave, "Radians");
    let scale = g.n("MultiplyFloat");
    g.w(wave, "Result", scale, "A");
    g.f(scale, "B", 0.25);
    let lift = g.n("AddFloat");
    g.w(scale, "Result", lift, "A");
    g.f(lift, "B", 1.0);
    g.w(lift, "Result", mk, "Y");
    let setp = g.api("#Transform", "position", "set");
    g.w(mk, "Vector", setp, "Value");
    g.w(br, "True", setp, "In");
    g.0
}

#[test]
fn a_hundred_node_graph_on_a_thousand_objects_runs_in_about_a_millisecond() {
    let g = busy_graph();
    assert!(g.nodes.len() >= 100, "{} nodes", g.nodes.len());
    let root = project("perf");
    g.save(&root.join(GRAPH)).unwrap();
    let make = |with_graph: bool| {
        let mut scene = Scene::new("t");
        for i in 0..1000 {
            let e = scene.spawn(&format!("Obj{i}"), None);
            if with_graph {
                attach(&mut scene, e, GRAPH, &[]);
            }
        }
        Play::start(root.clone(), scene)
    };
    let measure = |p: &mut Play| {
        p.frames(10);
        let n = 120;
        let mut best = f64::MAX;
        let mut total = 0.0;
        for _ in 0..n {
            let t = std::time::Instant::now();
            p.frames(1);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            best = best.min(ms);
            total += ms;
        }
        (total / n as f64, best)
    };
    let mut with = make(true);
    with.no_errors();
    assert_eq!(with.host.graph_instance_count(), 1000);
    let (avg, best) = measure(&mut with);
    // The graphs' Update pass alone (dispatch to 1000 instances and their flows).
    let mut pass = Vec::new();
    for _ in 0..200 {
        with.frames(1);
        pass.push(with.host.graph_update_time().as_secs_f64() * 1000.0);
    }
    pass.sort_by(f64::total_cmp);
    let (pass_best, pass_median) = (pass[0], pass[pass.len() / 2]);
    println!(
        "Script Graph Update pass, 1000 objects x {} nodes: median {pass_median:.3} ms, best {pass_best:.3} ms",
        g.nodes.len()
    );
    let mut without = make(false);
    let (base_avg, base_best) = measure(&mut without);
    // The cost of an instance with a one-node Update flow, and of one with no Update at all.
    for (label, tiny) in [("one Set Variable on Update", true), ("Start only", false)] {
        let mut t = G::new();
        t.var("X", PinType::Float, Literal::Float(0.0));
        if tiny {
            let u = t.n("OnUpdate");
            let s = t.set("X");
            t.w(u, "Out", s, "In");
        } else {
            t.n("OnStart");
        }
        t.0.save(&root.join("Assets/Tiny.vgraph")).unwrap();
        let mut scene = Scene::new("t");
        for i in 0..1000 {
            let e = scene.spawn(&format!("Obj{i}"), None);
            attach(&mut scene, e, "Assets/Tiny.vgraph", &[]);
        }
        let mut p = Play::start(root.clone(), scene);
        let (a, b) = measure(&mut p);
        println!("  1000 objects, {label}: frame {a:.3} ms (best {b:.3})");
    }
    let cost = (avg - base_avg).max(0.0);
    println!(
        "Script Graph benchmark: {} nodes x 1000 objects: frame {avg:.3} ms (best {best:.3}), \
         empty scene {base_avg:.3} ms (best {base_best:.3}); graphs cost {cost:.3} ms, best {:.3} ms",
        g.nodes.len(),
        (best - base_best).max(0.0)
    );
    with.no_errors();
    // The graphs really ran: the objects moved.
    let moved = with
        .scene
        .walk()
        .iter()
        .filter(|(e, _)| with.scene.world_position(*e).x != 0.0)
        .count();
    assert_eq!(moved, 1000);
    if !cfg!(debug_assertions) {
        // About 1 ms on a desktop; generous here because tests run side by side.
        assert!(pass_best < 4.0, "graph Update pass {pass_best:.3} ms");
        assert!(
            best - base_best < 6.0,
            "graphs cost {:.3} ms",
            best - base_best
        );
    }
}

// ---- saving, Animator parameters, cursor ---------------------------------------------------

/// Play `g` on one object named "Thing" in an existing project folder.
fn play_in(root: PathBuf, g: &ScriptGraph) -> (Play, Entity) {
    g.save(&root.join(GRAPH)).unwrap();
    let mut scene = Scene::new("t");
    let e = scene.spawn("Thing", None);
    attach(&mut scene, e, GRAPH, &[]);
    (Play::start(root, scene), e)
}

#[test]
fn player_prefs_nodes_save_and_read_typed_values() {
    let _lock = crate::prefs::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let root = project("prefs");
    let file = root.join("prefs.json");
    crate::prefs::open(&file);
    let mut g = G::new();
    let start = g.n("OnStart");
    let set_i = g.api("PlayerPrefs", "SetInt", "call");
    g.v(set_i, "Key", Literal::String("vs.score".into()));
    g.v(set_i, "Value", Literal::Int(42));
    let set_f = g.api("PlayerPrefs", "SetFloat", "call");
    g.v(set_f, "Key", Literal::String("vs.x".into()));
    g.f(set_f, "Value", 1.5);
    let set_s = g.api("PlayerPrefs", "SetString", "call");
    g.v(set_s, "Key", Literal::String("vs.name".into()));
    g.v(set_s, "Value", Literal::String("Ada".into()));
    let save = g.api("PlayerPrefs", "Save", "call");
    let mut logs = Vec::new();
    for (member, key, default) in [
        ("GetInt", "vs.score", Literal::Int(0)),
        ("GetFloat", "vs.x", Literal::Float(0.0)),
        ("GetString", "vs.name", Literal::String("?".into())),
        ("GetInt", "vs.missing", Literal::Int(3)),
    ] {
        let get = g.api("PlayerPrefs", member, "call");
        g.v(get, "Key", Literal::String(key.into()));
        g.v(get, "Default", default);
        let l = g.log();
        g.w(get, "Result", l, "Message");
        logs.push(l);
    }
    let has = g.api("PlayerPrefs", "HasKey", "call");
    g.v(has, "Key", Literal::String("vs.score".into()));
    let l_has = g.log();
    g.w(has, "Result", l_has, "Message");
    let del = g.api("PlayerPrefs", "DeleteKey", "call");
    g.v(del, "Key", Literal::String("vs.score".into()));
    let has2 = g.api("PlayerPrefs", "HasKey", "call");
    g.v(has2, "Key", Literal::String("vs.score".into()));
    let l_has2 = g.log();
    g.w(has2, "Result", l_has2, "Message");
    let mut order = vec![set_i, set_f, set_s, save];
    order.extend(&logs);
    order.extend([l_has, del, l_has2]);
    chain(&mut g, start, "Out", &order);
    let (mut p, _) = play_in(root, &g.0);
    p.frames(1);
    p.no_errors();
    assert_eq!(p.infos(), ["42", "1.5", "Ada", "3", "true", "false"]);
    // Save wrote the file before the key was deleted: a new run reads it back.
    crate::prefs::open(&file);
    assert_eq!(crate::prefs::get_int("vs.score", 0), 42);
    assert_eq!(crate::prefs::get_string("vs.name", ""), "Ada");
}

#[test]
fn animator_parameter_nodes_drive_the_state_machine() {
    use crate::animation::AnimatorController;
    use crate::animation::controller::{
        Condition, ConditionMode, Node as ANode, ParamKind, Parameter, State, StateMachine,
        Transition,
    };
    let root = project("animator");
    let param = |name: &str, kind| Parameter {
        name: name.into(),
        kind,
        default: 0.0,
    };
    let when = |p: &str, mode, threshold| Condition {
        parameter: p.into(),
        mode,
        threshold,
    };
    let ctrl = AnimatorController {
        parameters: vec![
            param("Speed", ParamKind::Float),
            param("Jump", ParamKind::Trigger),
            param("Crouch", ParamKind::Bool),
            param("Weapon", ParamKind::Int),
        ],
        base: StateMachine {
            states: ["Idle", "Walk", "Jump"]
                .iter()
                .map(|n| State {
                    name: (*n).into(),
                    ..Default::default()
                })
                .collect(),
            default_state: Some("Idle".into()),
            transitions: vec![
                Transition {
                    has_exit_time: false,
                    duration: 0.0,
                    conditions: vec![when("Speed", ConditionMode::Greater, 1.0)],
                    ..Transition::new(ANode::state("Idle"), ANode::state("Walk"))
                },
                Transition {
                    has_exit_time: false,
                    duration: 0.0,
                    conditions: vec![when("Jump", ConditionMode::If, 0.0)],
                    ..Transition::new(ANode::AnyState, ANode::state("Jump"))
                },
            ],
            ..Default::default()
        },
        ..Default::default()
    };
    ctrl.save(&root.join("Assets/Hero.controller")).unwrap();
    let mut g = G::new();
    let start = g.n("OnStart");
    let speed = g.api("#Animator", "SetFloat", "call");
    g.v(speed, "Name", Literal::String("Speed".into()));
    g.f(speed, "Value", 2.5);
    let crouch = g.api("#Animator", "SetBool", "call");
    g.v(crouch, "Name", Literal::String("Crouch".into()));
    let weapon = g.api("#Animator", "SetInteger", "call");
    g.v(weapon, "Name", Literal::String("Weapon".into()));
    g.v(weapon, "Value", Literal::Int(2));
    let mut logs = Vec::new();
    for (member, name) in [
        ("GetFloat", "Speed"),
        ("GetBool", "Crouch"),
        ("GetInteger", "Weapon"),
    ] {
        let get = g.api("#Animator", member, "call");
        g.v(get, "Name", Literal::String(name.into()));
        let l = g.log();
        g.w(get, "Result", l, "Message");
        logs.push(l);
    }
    // Lock the cursor too.
    let lock = g.api("Cursor", "lockState", "set");
    let mut order = vec![speed, crouch, weapon];
    order.extend(&logs);
    order.push(lock);
    chain(&mut g, start, "Out", &order);
    // Space sets the Jump trigger; a damped SetFloat eases Speed towards 0 every frame.
    let update = g.n("OnUpdate");
    let seq = g.np("Sequence", &[("count", "2")]);
    g.w(update, "Out", seq, "In");
    let key = g.api("Input", "GetKeyDown", "call");
    let br = g.n("Branch");
    g.w(key, "Result", br, "Condition");
    g.w(seq, "Then 0", br, "In");
    let jump = g.api("#Animator", "SetTrigger", "call");
    g.w(br, "True", jump, "In");
    let ease = g.api("#Animator", "SetFloat", "call");
    g.v(ease, "Name", Literal::String("Speed".into()));
    g.f(ease, "Damp Time", 0.2);
    g.w(seq, "Then 1", ease, "In");
    g.0.save(&root.join(GRAPH)).unwrap();
    let mut scene = Scene::new("t");
    let e = scene.spawn("Hero", None);
    scene.add(
        e,
        crate::scene::Animator {
            controller: Some("Assets/Hero.controller".into()),
            ..Default::default()
        },
    );
    attach(&mut scene, e, GRAPH, &[]);
    let mut p = Play::start(root, scene);
    p.frames(1);
    p.no_errors();
    assert_eq!(&p.infos()[..3], ["2.5", "true", "2"]);
    assert_eq!(
        p.host.cursor().lock,
        crate::scripting::CursorLockMode::Locked
    );
    assert!(!p.host.cursor().visible);
    p.frames(2);
    let state = |p: &Play| {
        p.host
            .with_animator(|a| a.status(&p.scene, e))
            .unwrap()
            .current
            .unwrap()
            .name
    };
    assert_eq!(state(&p), "Walk");
    let mut space = Input::default();
    space.pressed.insert("space".into());
    p.frame_with(space);
    p.frames(2);
    assert_eq!(state(&p), "Jump");
    // Damped: Speed eased down from 2.5 but has not reached 0 yet.
    let speed = p
        .host
        .with_animator(|a| a.param(&p.scene, e, "Speed", &[ParamKind::Float]));
    assert!(speed > 0.0 && speed < 2.5, "{speed}");
    p.no_errors();
}

#[test]
fn animator_nodes_say_when_the_target_has_no_animator() {
    let mut g = G::new();
    let start = g.n("OnStart");
    let set = g.api("#Animator", "SetFloat", "call");
    g.w(start, "Out", set, "In");
    let (mut p, _) = play("no_animator", &g.0);
    p.frames(1);
    let errors = p.errors();
    assert!(
        errors.iter().any(|e| e.contains("'Thing' has no Animator")),
        "{errors:?}"
    );
}
