//! Procedural VisualScriptingDemo scene in `SampleProject`: a small level whose logic is all
//! Script Graphs (`Assets/VisualScripting/*.vgraph`), no Lua:
//!
//! - **Player**: a Character Controller moved by the Horizontal / Vertical axes, jumping on
//!   Jump, with gravity; it listens for the custom events `Launch` (jump pads) and `Respawn`.
//! - **Door**: a trigger in front of the door opens it while the player stands there.
//! - **Coins**: spin; touched by the player they send `CoinCollected` to the score and destroy
//!   themselves. **Score Text** counts them on the UI (its text is built by a macro).
//! - **Patrol**: an enemy walking between two points, turning on a timer; catching the player
//!   sends it `Respawn`.
//! - **Jump Pad**: launches the player with its Force (an Object variable).
//!
//! Regenerate with `cargo test -p misty_editor -- --ignored write_visual_scripting_demo`; the
//! other tests check the committed files still match and play the scene headlessly.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    BoxCollider, CharacterController, MeshFilter, MeshRenderer, PrimitiveMesh, RectTransform,
    Rigidbody, Scene, ScriptRef, ScriptValue, Scripts, SphereCollider, TextAnchor, Transform,
    UiText,
};
use misty_engine::ui::presets;
use misty_engine::visual_script::{
    Literal, Macro, NoFiles, PinType, ScriptGraph, StickyNote, Variable,
};

const SCENE: &str = "Assets/Scenes/VisualScriptingDemo.scene";
const PLAYER: &str = "Assets/VisualScripting/Player.vgraph";
const DOOR: &str = "Assets/VisualScripting/Door.vgraph";
const COIN: &str = "Assets/VisualScripting/Coin.vgraph";
const SCORE: &str = "Assets/VisualScripting/Score.vgraph";
const PATROL: &str = "Assets/VisualScripting/Patrol.vgraph";
const JUMP_PAD: &str = "Assets/VisualScripting/JumpPad.vgraph";
const CHECKER: &str = "Assets/Materials/Checker.mat";
const WOOD: &str = "Assets/Materials/Wood.mat";
const STEEL: &str = "Assets/Materials/Blue Steel.mat";
const LASER: &str = "Assets/Materials/Laser.mat";
const CRATE: &str = "Assets/Materials/AO Crate.mat";
/// Coins in the level.
const COINS: [Vec3; 3] = [
    Vec3::new(-4.0, 1.0, 4.0),
    Vec3::new(4.0, 1.0, 4.0),
    Vec3::new(0.0, 1.0, 9.0),
];
const PLAYER_START: Vec3 = Vec3::new(0.0, 1.1, -6.0);
const DOOR_AT: Vec3 = Vec3::new(0.0, 1.5, 13.0);
const PAD_AT: Vec3 = Vec3::new(-7.0, 0.05, -2.0);
const PATROL_A: Vec3 = Vec3::new(5.0, 0.75, -1.0);
const PATROL_B: Vec3 = Vec3::new(9.0, 0.75, -1.0);

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// A graph laid out on a grid as it is built.
struct B {
    g: ScriptGraph,
}

/// Grid cell → canvas position.
fn at(col: f32, row: f32) -> [f32; 2] {
    [col * 250.0, row * 110.0]
}

impl B {
    fn new() -> Self {
        Self {
            g: ScriptGraph::default(),
        }
    }

    fn n(&mut self, kind: &str, col: f32, row: f32) -> u32 {
        self.g.add_node(kind, at(col, row))
    }

    fn np(&mut self, kind: &str, params: &[(&str, &str)], col: f32, row: f32) -> u32 {
        self.g.add_node_with(kind, params, at(col, row))
    }

    fn api(&mut self, owner: &str, member: &str, mode: &str, col: f32, row: f32) -> u32 {
        self.np(
            "Api",
            &[("owner", owner), ("member", member), ("mode", mode)],
            col,
            row,
        )
    }

    fn call(&mut self, owner: &str, member: &str, args: &str, col: f32, row: f32) -> u32 {
        self.np(
            "Api",
            &[
                ("owner", owner),
                ("member", member),
                ("mode", "call"),
                ("args", args),
            ],
            col,
            row,
        )
    }

    fn get(&mut self, name: &str, col: f32, row: f32) -> u32 {
        self.np("GetVariable", &[("name", name)], col, row)
    }

    fn set(&mut self, name: &str, col: f32, row: f32) -> u32 {
        self.np("SetVariable", &[("name", name)], col, row)
    }

    fn w(&mut self, a: u32, out: &str, b: u32, inp: &str) {
        if let Err(e) = self.g.connect(a, out, b, inp, &NoFiles) {
            panic!("{a}.{out} -> {b}.{inp}: {e}");
        }
    }

    fn v(&mut self, n: u32, pin: &str, lit: Literal) {
        self.g.set_value(n, pin, lit);
    }

    fn var(&mut self, v: Variable) {
        self.g.variables.push(v);
    }

    fn note(&mut self, col: f32, row: f32, w: f32, text: &str) {
        let [x, y] = at(col, row);
        self.g.notes.push(StickyNote {
            position: [x, y],
            size: [w, 70.0],
            text: text.into(),
        });
    }

    /// A Branch on `Other` having the Player tag, entered from `from.out`; returns the Branch.
    fn if_player(&mut self, from: u32, out: &str, other: (u32, &str), col: f32, row: f32) -> u32 {
        let tag = self.api("#GameObject", "CompareTag", "call", col, row + 1.2);
        self.w(other.0, other.1, tag, "Target");
        self.v(tag, "Tag", Literal::String("Player".into()));
        let br = self.n("Branch", col + 1.0, row);
        self.w(tag, "Result", br, "Condition");
        self.w(from, out, br, "In");
        br
    }
}

fn f(x: f32) -> Literal {
    Literal::Float(x)
}

fn s(t: &str) -> Literal {
    Literal::String(t.into())
}

/// The player: move with the axes, jump, gravity; `Launch(force)` and `Respawn` events.
pub fn player_graph() -> ScriptGraph {
    let mut b = B::new();
    b.var(Variable::new("Speed", PinType::Float, f(5.0)).object());
    b.var(Variable::new("Jump Speed", PinType::Float, f(6.0)).object());
    b.var(Variable::new("Gravity", PinType::Float, f(-15.0)));
    b.var(Variable::new("Velocity Y", PinType::Float, f(0.0)));
    b.var(Variable::new(
        "Spawn",
        PinType::Vector3,
        Literal::Vector3([0.0; 3]),
    ));
    b.note(0.0, -1.2, 420.0, "Player: axes move the Character Controller, Jump jumps.\nJump pads send Launch, the patrol sends Respawn.");
    // Start: remember where we began.
    let start = b.n("OnStart", 0.0, 0.0);
    let pos = b.api("#Transform", "position", "get", 1.0, 1.0);
    let spawn = b.set("Spawn", 2.0, 0.0);
    b.w(start, "Out", spawn, "In");
    b.w(pos, "Value", spawn, "Value");
    // Update: ground check and jump, gravity, then move.
    let update = b.n("OnUpdate", 0.0, 3.0);
    let seq = b.np("Sequence", &[("count", "3")], 1.0, 3.0);
    b.w(update, "Out", seq, "In");
    let grounded = b.api("#CharacterController", "isGrounded", "get", 1.0, 5.0);
    let jump = b.call("Input", "GetButtonDown", "buttonName", 1.0, 6.0);
    b.v(jump, "buttonName", s("Jump"));
    let both = b.n("And", 2.0, 5.5);
    b.w(grounded, "Value", both, "A");
    b.w(jump, "Result", both, "B");
    let br = b.n("Branch", 3.0, 3.0);
    b.w(seq, "Then 0", br, "In");
    b.w(both, "Result", br, "Condition");
    let js = b.get("Jump Speed", 3.0, 4.5);
    let set_jump = b.set("Velocity Y", 4.0, 3.0);
    b.w(br, "True", set_jump, "In");
    b.w(js, "Value", set_jump, "Value");
    // Falling speed: on the ground it rests at -1, in the air gravity adds up.
    let vy = b.get("Velocity Y", 2.0, 8.0);
    let grav = b.get("Gravity", 2.0, 9.0);
    let dt = b.api("Time", "deltaTime", "get", 2.0, 10.0);
    let g_dt = b.n("MultiplyFloat", 3.0, 9.0);
    b.w(grav, "Value", g_dt, "A");
    b.w(dt, "Value", g_dt, "B");
    let fall = b.n("AddFloat", 4.0, 8.5);
    b.w(vy, "Value", fall, "A");
    b.w(g_dt, "Result", fall, "B");
    let rest = b.n("MaxFloat", 5.0, 8.0);
    b.w(fall, "Result", rest, "A");
    b.v(rest, "B", f(-1.0));
    let pick = b.n("Select", 6.0, 7.5);
    b.w(grounded, "Value", pick, "Condition");
    b.w(rest, "Result", pick, "True");
    b.w(fall, "Result", pick, "False");
    let set_fall = b.set("Velocity Y", 7.0, 5.0);
    b.w(seq, "Then 1", set_fall, "In");
    b.w(pick, "Result", set_fall, "Value");
    // Move: (h, vy / speed, v) × speed × dt.
    let h = b.api("Input", "GetAxis", "call", 5.0, 10.5);
    b.v(h, "Axis Name", s("Horizontal"));
    let v = b.api("Input", "GetAxis", "call", 5.0, 11.5);
    b.v(v, "Axis Name", s("Vertical"));
    let speed = b.get("Speed", 6.0, 12.5);
    let hs = b.n("MultiplyFloat", 6.0, 10.5);
    b.w(h, "Result", hs, "A");
    b.w(speed, "Value", hs, "B");
    let vs = b.n("MultiplyFloat", 6.0, 11.5);
    b.w(v, "Result", vs, "A");
    b.w(speed, "Value", vs, "B");
    let vy2 = b.get("Velocity Y", 6.0, 9.5);
    let dir = b.n("MakeVector3", 7.0, 10.0);
    b.w(hs, "Result", dir, "X");
    b.w(vy2, "Value", dir, "Y");
    b.w(vs, "Result", dir, "Z");
    let step = b.n("ScaleVector3", 8.0, 10.0);
    b.w(dir, "Vector", step, "Vector");
    b.w(dt, "Value", step, "Scale");
    let mv = b.call("#CharacterController", "Move", "motion", 9.0, 5.0);
    b.w(seq, "Then 2", mv, "In");
    b.w(step, "Result", mv, "motion");
    // Launch(force): a jump pad throws us up.
    let launch = b.np(
        "OnCustomEvent",
        &[("name", "Launch"), ("args", "force")],
        0.0,
        14.0,
    );
    let set_launch = b.set("Velocity Y", 1.5, 14.0);
    b.w(launch, "Out", set_launch, "In");
    b.w(launch, "force", set_launch, "Value");
    // Respawn: back to the start, at rest.
    let respawn = b.np("OnCustomEvent", &[("name", "Respawn")], 0.0, 16.0);
    let back = b.get("Spawn", 1.0, 17.0);
    let place = b.api("#Transform", "position", "set", 2.0, 16.0);
    b.w(respawn, "Out", place, "In");
    b.w(back, "Value", place, "Value");
    let rest2 = b.set("Velocity Y", 3.0, 16.0);
    b.w(place, "Out", rest2, "In");
    b.v(rest2, "Value", f(0.0));
    b.g
}

/// The door trigger: opens the door while the player stands in it.
pub fn door_graph() -> ScriptGraph {
    let mut b = B::new();
    b.var(Variable::new("Open Height", PinType::Float, f(3.0)).object());
    b.var(Variable::new("Speed", PinType::Float, f(3.0)).object());
    b.var(Variable::new("Open", PinType::Bool, Literal::Bool(false)));
    b.var(Variable::new(
        "Closed",
        PinType::Vector3,
        Literal::Vector3([0.0; 3]),
    ));
    b.note(
        0.0,
        -1.2,
        380.0,
        "Door: stand in the trigger to open the door; leave to close it.",
    );
    let door = b.call("GameObject", "Find", "name", 0.0, 2.0);
    b.v(door, "Name", s("Door"));
    // Start: remember the closed position.
    let start = b.n("OnStart", 0.0, 0.0);
    let pos = b.api("#Transform", "position", "get", 1.0, 1.0);
    b.w(door, "Result", pos, "Target");
    let closed = b.set("Closed", 2.0, 0.0);
    b.w(start, "Out", closed, "In");
    b.w(pos, "Value", closed, "Value");
    // Enter / exit.
    let enter = b.n("OnTriggerEnter", 0.0, 4.0);
    let br = b.if_player(enter, "Out", (enter, "Other"), 1.0, 4.0);
    let open = b.set("Open", 3.0, 4.0);
    b.w(br, "True", open, "In");
    b.v(open, "Value", Literal::Bool(true));
    let log = b.api("Debug", "Log", "call", 4.0, 4.0);
    b.v(log, "Message", s("Door opening"));
    b.w(open, "Out", log, "In");
    let exit = b.n("OnTriggerExit", 0.0, 7.0);
    let br2 = b.if_player(exit, "Out", (exit, "Other"), 1.0, 7.0);
    let shut = b.set("Open", 3.0, 7.0);
    b.w(br2, "True", shut, "In");
    b.v(shut, "Value", Literal::Bool(false));
    // Update: slide towards open or closed.
    let update = b.n("OnUpdate", 0.0, 10.0);
    let c = b.get("Closed", 1.0, 11.0);
    let lift = b.get("Open Height", 1.0, 12.0);
    let up = b.n("MakeVector3", 2.0, 12.0);
    b.w(lift, "Value", up, "Y");
    let raised = b.n("AddVector3", 3.0, 11.5);
    b.w(c, "Value", raised, "A");
    b.w(up, "Vector", raised, "B");
    let is_open = b.get("Open", 3.0, 13.0);
    let target = b.n("Select", 4.0, 12.0);
    b.w(is_open, "Value", target, "Condition");
    b.w(raised, "Result", target, "True");
    b.w(c, "Value", target, "False");
    let now = b.api("#Transform", "position", "get", 4.0, 10.8);
    b.w(door, "Result", now, "Target");
    let sp = b.get("Speed", 4.0, 14.0);
    let dt = b.api("Time", "deltaTime", "get", 4.0, 15.0);
    let max = b.n("MultiplyFloat", 5.0, 14.0);
    b.w(sp, "Value", max, "A");
    b.w(dt, "Value", max, "B");
    let next = b.n("MoveTowardsVector3", 6.0, 11.5);
    b.w(now, "Value", next, "Current");
    b.w(target, "Result", next, "Target");
    b.w(max, "Result", next, "Max Delta");
    let set = b.api("#Transform", "position", "set", 7.0, 10.0);
    b.w(update, "Out", set, "In");
    b.w(door, "Result", set, "Target");
    b.w(next, "Result", set, "Value");
    b.g
}

/// A coin: spins; the player collects it.
pub fn coin_graph() -> ScriptGraph {
    let mut b = B::new();
    b.var(Variable::new("Value", PinType::Int, Literal::Int(1)).object());
    b.var(Variable::new("Spin", PinType::Float, f(120.0)).object());
    b.note(
        0.0,
        -1.2,
        380.0,
        "Coin: spins; the player collects it (CoinCollected goes to the score).",
    );
    let update = b.n("OnUpdate", 0.0, 0.0);
    let spin = b.get("Spin", 0.0, 1.2);
    let deg = b.n("MultiplyFloat", 1.0, 1.2);
    b.w(spin, "Value", deg, "A");
    b.w(update, "Delta Time", deg, "B");
    let euler = b.n("MakeVector3", 2.0, 1.2);
    b.w(deg, "Result", euler, "Y");
    let rot = b.api("#Transform", "Rotate", "call", 3.0, 0.0);
    b.w(update, "Out", rot, "In");
    b.w(euler, "Vector", rot, "Euler Degrees");
    let enter = b.n("OnTriggerEnter", 0.0, 3.0);
    let br = b.if_player(enter, "Out", (enter, "Other"), 1.0, 3.0);
    let score = b.call("GameObject", "Find", "name", 2.0, 5.0);
    b.v(score, "Name", s("Score Text"));
    let value = b.get("Value", 2.0, 6.0);
    let send = b.np(
        "SendCustomEvent",
        &[("name", "CoinCollected"), ("args", "value")],
        3.0,
        3.0,
    );
    b.w(br, "True", send, "In");
    b.w(score, "Result", send, "Target");
    b.w(value, "Value", send, "value");
    let me = b.n("Self", 3.0, 5.5);
    let destroy = b.api("", "Destroy", "call", 4.0, 3.0);
    b.w(send, "Out", destroy, "In");
    b.w(me, "Self", destroy, "Object");
    b.g
}

/// The score text: counts collected coins (the text comes from a macro).
pub fn score_graph() -> ScriptGraph {
    let mut b = B::new();
    b.var(Variable::new("Score", PinType::Int, Literal::Int(0)).object());
    b.var(Variable::new("Total", PinType::Int, Literal::Int(COINS.len() as i64)).object());
    b.note(
        0.0,
        -1.2,
        380.0,
        "Score: CoinCollected adds to Score; the Format macro writes the text.",
    );
    // The Format macro: "Coins: 2 / 3", or a cheer when all are found.
    let mut m = B::new();
    let input = m.np("GraphInput", &[("pins", "Score:int, Total:int")], 0.0, 0.0);
    let output = m.np("GraphOutput", &[("pins", "Text:string")], 4.0, 0.0);
    let a = m.n("Append", 1.0, 0.0);
    m.v(a, "A", s("Coins: "));
    m.w(input, "Score", a, "B");
    let b2 = m.n("Append", 2.0, 0.0);
    m.w(a, "Result", b2, "A");
    m.v(b2, "B", s(" / "));
    let c = m.n("Append", 3.0, 0.5);
    m.w(b2, "Result", c, "A");
    m.w(input, "Total", c, "B");
    let done = m.n("GreaterOrEqual", 2.0, 2.0);
    m.w(input, "Score", done, "A");
    m.w(input, "Total", done, "B");
    let pick = m.n("Select", 3.5, 2.0);
    m.w(done, "Result", pick, "Condition");
    m.v(pick, "True", s("All coins collected!"));
    m.w(c, "Result", pick, "False");
    m.w(pick, "Result", output, "Text");
    b.g.macros.push(Macro {
        name: "Format".into(),
        graph: m.g,
    });
    let fmt = b.np("Subgraph", &[("macro", "Format")], 2.0, 2.0);
    let score = b.get("Score", 1.0, 2.0);
    let total = b.get("Total", 1.0, 3.0);
    b.w(score, "Value", fmt, "Score");
    b.w(total, "Value", fmt, "Total");
    let start = b.n("OnStart", 0.0, 0.0);
    let show = b.api("#Text", "text", "set", 3.0, 0.0);
    b.w(start, "Out", show, "In");
    b.w(fmt, "Text", show, "Value");
    let got = b.np(
        "OnCustomEvent",
        &[("name", "CoinCollected"), ("args", "value")],
        0.0,
        5.0,
    );
    let add = b.n("AddInt", 1.0, 6.0);
    let current = b.get("Score", 0.0, 6.5);
    b.w(current, "Value", add, "A");
    b.w(got, "value", add, "B");
    let set = b.set("Score", 2.0, 5.0);
    b.w(got, "Out", set, "In");
    b.w(add, "Result", set, "Value");
    let show2 = b.api("#Text", "text", "set", 3.0, 5.0);
    b.w(set, "Out", show2, "In");
    b.w(fmt, "Text", show2, "Value");
    b.g
}

/// The patrol: walks between two points, turning on a timer; catches the player.
pub fn patrol_graph() -> ScriptGraph {
    let mut b = B::new();
    b.var(
        Variable::new(
            "Point A",
            PinType::Vector3,
            Literal::Vector3(PATROL_A.to_array()),
        )
        .object(),
    );
    b.var(
        Variable::new(
            "Point B",
            PinType::Vector3,
            Literal::Vector3(PATROL_B.to_array()),
        )
        .object(),
    );
    b.var(Variable::new("Speed", PinType::Float, f(2.5)).object());
    b.var(Variable::new("Turn Every", PinType::Float, f(2.0)).object());
    b.var(Variable::new(
        "Target",
        PinType::Vector3,
        Literal::Vector3([0.0; 3]),
    ));
    b.note(
        0.0,
        -1.2,
        400.0,
        "Patrol: a timer switches the target point; catching the player sends Respawn.",
    );
    let start = b.n("OnStart", 0.0, 0.0);
    let pb = b.get("Point B", 1.0, 1.0);
    let first = b.set("Target", 2.0, 0.0);
    b.w(start, "Out", first, "In");
    b.w(pb, "Value", first, "Value");
    // Every few seconds: flip between the points.
    let timer = b.n("OnTimer", 0.0, 3.0);
    let every = b.get("Turn Every", 0.0, 4.2);
    b.w(every, "Value", timer, "Interval");
    let flip = b.n("FlipFlop", 1.0, 3.0);
    b.w(timer, "Out", flip, "In");
    let pa = b.get("Point A", 1.5, 5.0);
    let to_a = b.set("Target", 2.5, 3.0);
    b.w(flip, "A", to_a, "In");
    b.w(pa, "Value", to_a, "Value");
    let pb2 = b.get("Point B", 2.5, 5.5);
    let to_b = b.set("Target", 3.5, 4.0);
    b.w(flip, "B", to_b, "In");
    b.w(pb2, "Value", to_b, "Value");
    // Update: walk.
    let update = b.n("OnUpdate", 0.0, 7.0);
    let now = b.api("#Transform", "position", "get", 1.0, 8.0);
    let target = b.get("Target", 1.0, 9.0);
    let sp = b.get("Speed", 1.0, 10.0);
    let step = b.n("MultiplyFloat", 2.0, 10.0);
    b.w(sp, "Value", step, "A");
    b.w(update, "Delta Time", step, "B");
    let next = b.n("MoveTowardsVector3", 3.0, 8.5);
    b.w(now, "Value", next, "Current");
    b.w(target, "Value", next, "Target");
    b.w(step, "Result", next, "Max Delta");
    let set = b.api("#Transform", "position", "set", 4.0, 7.0);
    b.w(update, "Out", set, "In");
    b.w(next, "Result", set, "Value");
    // Caught!
    let enter = b.n("OnTriggerEnter", 0.0, 12.0);
    let br = b.if_player(enter, "Out", (enter, "Other"), 1.0, 12.0);
    let send = b.np("SendCustomEvent", &[("name", "Respawn")], 3.0, 12.0);
    b.w(br, "True", send, "In");
    b.w(enter, "Other", send, "Target");
    let log = b.api("Debug", "Log", "call", 4.0, 12.0);
    b.v(log, "Message", s("Caught by the patrol!"));
    b.w(send, "Out", log, "In");
    b.g
}

/// The jump pad: launches the player.
pub fn jump_pad_graph() -> ScriptGraph {
    let mut b = B::new();
    b.var(Variable::new("Force", PinType::Float, f(12.0)).object());
    b.note(
        0.0,
        -1.2,
        360.0,
        "Jump Pad: sends Launch(Force) to the player.",
    );
    let enter = b.n("OnTriggerEnter", 0.0, 0.0);
    let br = b.if_player(enter, "Out", (enter, "Other"), 1.0, 0.0);
    let force = b.get("Force", 2.0, 2.0);
    let send = b.np(
        "SendCustomEvent",
        &[("name", "Launch"), ("args", "force")],
        3.0,
        0.0,
    );
    b.w(br, "True", send, "In");
    b.w(enter, "Other", send, "Target");
    b.w(force, "Value", send, "force");
    b.g
}

fn graphs() -> [(&'static str, ScriptGraph); 6] {
    [
        (PLAYER, player_graph()),
        (DOOR, door_graph()),
        (COIN, coin_graph()),
        (SCORE, score_graph()),
        (PATROL, patrol_graph()),
        (JUMP_PAD, jump_pad_graph()),
    ]
}

fn graph(path: &str, fields: &[(&str, ScriptValue)]) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            enabled: true,
            fields: fields
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
        }],
    }
}

fn shape(s: &mut Scene, name: &str, mesh: PrimitiveMesh, t: Transform, mat: &str) -> Entity {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(mesh));
    s.add(
        e,
        MeshRenderer {
            material: Some(mat.into()),
            ..Default::default()
        },
    );
    e
}

fn block(s: &mut Scene, name: &str, pos: Vec3, scale: Vec3, mat: &str) -> Entity {
    let mut t = Transform::from_position(pos);
    t.scale = scale;
    let e = shape(s, name, PrimitiveMesh::Cube, t, mat);
    s.add(e, BoxCollider::default());
    e
}

/// An invisible trigger box.
fn trigger(s: &mut Scene, name: &str, pos: Vec3, size: Vec3) -> Entity {
    let e = s.spawn(name, None);
    s.set_transform(e, Transform::from_position(pos));
    s.add(
        e,
        BoxCollider {
            is_trigger: true,
            size,
            ..Default::default()
        },
    );
    e
}

fn kinematic() -> Rigidbody {
    Rigidbody {
        is_kinematic: true,
        use_gravity: false,
        ..Default::default()
    }
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("VisualScriptingDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 19.0, -13.0))
                .with_euler(Vec3::new(52.0, 0.0, 0.0)),
        );
    }
    block(
        &mut s,
        "Ground",
        Vec3::new(0.0, -0.5, 3.0),
        Vec3::new(26.0, 1.0, 26.0),
        CHECKER,
    );
    // Walls with a doorway at the far end.
    block(
        &mut s,
        "Wall Left",
        Vec3::new(-7.5, 1.5, 13.5),
        Vec3::new(11.0, 3.0, 1.0),
        WOOD,
    );
    block(
        &mut s,
        "Wall Right",
        Vec3::new(7.5, 1.5, 13.5),
        Vec3::new(11.0, 3.0, 1.0),
        WOOD,
    );
    let door = block(
        &mut s,
        "Door",
        DOOR_AT + Vec3::new(0.0, 0.0, 0.5),
        Vec3::new(4.0, 3.0, 0.4),
        CRATE,
    );
    s.add(door, kinematic());
    let dt = trigger(
        &mut s,
        "Door Trigger",
        Vec3::new(0.0, 1.0, 11.0),
        Vec3::new(4.0, 2.0, 3.0),
    );
    s.add(dt, graph(DOOR, &[]));
    // The player.
    let player = shape(
        &mut s,
        "Player",
        PrimitiveMesh::Capsule,
        Transform::from_position(PLAYER_START),
        STEEL,
    );
    s.game_object_mut(player).tag = "Player".into();
    s.add(player, CharacterController::default());
    s.add(player, graph(PLAYER, &[]));
    // Coins.
    for (i, p) in COINS.iter().enumerate() {
        let mut t = Transform::from_position(*p);
        t.scale = Vec3::new(0.7, 0.1, 0.7);
        t.set_euler(Vec3::new(90.0, 0.0, 0.0));
        let c = shape(
            &mut s,
            &format!("Coin {}", i + 1),
            PrimitiveMesh::Cylinder,
            t,
            LASER,
        );
        s.add(
            c,
            SphereCollider {
                is_trigger: true,
                ..Default::default()
            },
        );
        s.add(c, graph(COIN, &[]));
    }
    // The patrol.
    let mut t = Transform::from_position(PATROL_A);
    t.scale = Vec3::new(1.0, 1.5, 1.0);
    let enemy = shape(&mut s, "Patrol", PrimitiveMesh::Cube, t, CRATE);
    s.add(
        enemy,
        BoxCollider {
            is_trigger: true,
            ..Default::default()
        },
    );
    s.add(enemy, kinematic());
    s.add(enemy, graph(PATROL, &[]));
    // The jump pad, with a ledge to jump onto.
    let mut t = Transform::from_position(PAD_AT);
    t.scale = Vec3::new(2.0, 0.1, 2.0);
    let pad = shape(&mut s, "Jump Pad", PrimitiveMesh::Cylinder, t, LASER);
    s.add(
        pad,
        BoxCollider {
            is_trigger: true,
            size: Vec3::new(1.0, 10.0, 1.0),
            ..Default::default()
        },
    );
    s.add(
        pad,
        graph(JUMP_PAD, &[("Force", ScriptValue::Number(12.0))]),
    );
    block(
        &mut s,
        "Ledge",
        Vec3::new(-9.0, 1.5, 3.0),
        Vec3::new(4.0, 3.0, 4.0),
        WOOD,
    );

    // UI: score and controls.
    let canvas = presets::spawn(&mut s, "Canvas", None, RectTransform::stretched());
    s.add(canvas, misty_engine::scene::Canvas::default());
    s.add(canvas, misty_engine::scene::CanvasScaler::default());
    let corner = Vec2::new(0.0, 1.0);
    let score = presets::text(&mut s, "Score Text", canvas);
    *s.world.get::<&mut RectTransform>(score).unwrap() = RectTransform {
        anchor_min: corner,
        anchor_max: corner,
        pivot: corner,
        anchored_position: Vec2::new(20.0, -20.0),
        size_delta: Vec2::new(600.0, 50.0),
    };
    {
        let mut t = s.world.get::<&mut UiText>(score).unwrap();
        t.text = "Coins: 0 / 3".into();
        t.font_size = 24.0;
        t.color = [1.0, 0.86, 0.3, 1.0];
        t.alignment = TextAnchor::UpperLeft;
    }
    s.add(score, graph(SCORE, &[]));
    let help = presets::text(&mut s, "Help Text", canvas);
    *s.world.get::<&mut RectTransform>(help).unwrap() = RectTransform {
        anchor_min: corner,
        anchor_max: corner,
        pivot: corner,
        anchored_position: Vec2::new(20.0, -56.0),
        size_delta: Vec2::new(1100.0, 40.0),
    };
    {
        let mut t = s.world.get::<&mut UiText>(help).unwrap();
        t.text = "WASD / arrows move, Space jumps. All the logic is Script Graphs.".into();
        t.font_size = 16.0;
        t.color = [1.0; 4];
        t.alignment = TextAnchor::UpperLeft;
    }
    s
}

fn files() -> Vec<(&'static str, String)> {
    graphs()
        .into_iter()
        .map(|(p, g)| (p, g.to_json() + "\n"))
        .collect()
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_visual_scripting_demo() {
    let root = project();
    for (rel, text) in files() {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn visual_scripting_demo_matches_its_generator() {
    let root = project();
    for (rel, text) in files() {
        let on_disk = std::fs::read_to_string(root.join(rel)).unwrap();
        assert_eq!(
            on_disk.replace("\r\n", "\n"),
            text,
            "{rel}: regenerate with write_visual_scripting_demo"
        );
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_visual_scripting_demo",
    );
    let (scene, report) = Scene::from_json("VisualScriptingDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // No Lua anywhere: every script entry is a Script Graph, and each compiles.
    let files = misty_engine::visual_script::FileResolver(root.clone());
    let mut graphs = 0;
    for (e, _) in scene.walk() {
        if let Ok(s) = scene.world.get::<&Scripts>(e) {
            for r in &s.list {
                assert!(
                    misty_engine::visual_script::is_graph_path(&r.path),
                    "{}",
                    r.path
                );
                let g = ScriptGraph::load(&root.join(&r.path)).unwrap();
                let errors = misty_engine::visual_script::compile::check(&g, &files);
                assert!(errors.is_empty(), "{}: {errors:?}", r.path);
                graphs += 1;
            }
        }
    }
    assert_eq!(graphs, 8);
}

/// Playing headlessly with injected input: the player walks (axes), jumps (Jump); the jump pad
/// launches it; the patrol walks and turns on its timer; collecting the coins updates the
/// score text; standing in the door trigger opens the door; the patrol catches the player and
/// sends it back to the start.
#[test]
fn visual_scripting_demo_plays() {
    use misty_engine::audio::Audio;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::{Input, LogKind};
    use misty_engine::settings::ProjectSettings;

    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let settings = ProjectSettings::load(&root).unwrap();
    let (audio, _) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root, &settings, audio);
    assert_eq!(rt.graph_instance_count(), 8);
    let mut time = Time::new(&settings.time);
    let player = scene.find_by_name("Player").unwrap();
    let door = scene.find_by_name("Door").unwrap();
    let patrol = scene.find_by_name("Patrol").unwrap();
    let score = scene.find_by_name("Score Text").unwrap();
    let mut logs: Vec<String> = Vec::new();
    let mut frame = |scene: &mut Scene, rt: &mut Runtime, input: &Input| {
        time.advance(1.0 / 60.0);
        rt.update(scene, &time, input.clone());
        for (k, t) in rt.take_logs() {
            assert!(k != LogKind::Error, "{t}");
            logs.push(t);
        }
    };
    let keys = |held: &[&str], pressed: &[&str]| {
        let mut i = Input::default();
        i.held.extend(held.iter().map(|k| (*k).to_owned()));
        i.pressed.extend(pressed.iter().map(|k| (*k).to_owned()));
        i
    };
    let idle = Input::default();
    for _ in 0..30 {
        frame(&mut scene, &mut rt, &idle);
    }
    // Settled on the ground.
    let y0 = scene.world_position(player).y;
    assert!((0.9..1.3).contains(&y0), "player rests on the ground: {y0}");
    // Walk forward (+z) with W.
    let z0 = scene.world_position(player).z;
    for _ in 0..30 {
        frame(&mut scene, &mut rt, &keys(&["w"], &[]));
    }
    let z1 = scene.world_position(player).z;
    assert!(z1 - z0 > 1.0, "walked {z0} -> {z1}");
    // Jump.
    frame(&mut scene, &mut rt, &keys(&["space"], &["space"]));
    let mut top = y0;
    for _ in 0..20 {
        frame(&mut scene, &mut rt, &idle);
        top = top.max(scene.world_position(player).y);
    }
    assert!(top > y0 + 0.5, "jumped to {top}");
    for _ in 0..40 {
        frame(&mut scene, &mut rt, &idle);
    }
    // The patrol walks between its points and turns.
    let px: Vec<f32> = (0..200)
        .map(|_| {
            frame(&mut scene, &mut rt, &idle);
            scene.world_position(patrol).x
        })
        .collect();
    let (lo, hi) = px
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
    assert!(hi - lo > 2.0, "patrol moved {lo}..{hi}");
    // It waits at a point until the timer turns it: look at the moves only.
    let moves: Vec<f32> = px
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|d| d.abs() > 1e-4)
        .collect();
    let turned = moves.windows(2).any(|w| w[0] * w[1] < 0.0);
    assert!(turned, "the timer turned the patrol");

    // Put the player into things (as the Move tool would) and let a few frames run.
    let place = |scene: &mut Scene, rt: &mut Runtime, p: Vec3| {
        scene.set_world_position(player, p);
        rt.with_physics_scene(scene, |physics, scene| {
            let (_, rot, pos) = scene.world_matrix(player).to_scale_rotation_translation();
            physics.teleport(player, pos, rot);
        });
    };
    macro_rules! run {
        ($n:expr) => {
            for _ in 0..$n {
                frame(&mut scene, &mut rt, &idle);
            }
        };
    }
    // Coins.
    for (i, c) in COINS.iter().enumerate() {
        place(&mut scene, &mut rt, *c + Vec3::new(0.0, 0.1, 0.0));
        run!(6);
        assert!(
            scene.find_by_name(&format!("Coin {}", i + 1)).is_none(),
            "coin {} collected",
            i + 1
        );
    }
    let text = scene.world.get::<&UiText>(score).unwrap().text.clone();
    assert_eq!(text, "All coins collected!");
    // The door opens while the player stands in its trigger.
    let door_y = scene.world_position(door).y;
    place(&mut scene, &mut rt, Vec3::new(0.0, 1.1, 11.0));
    run!(90);
    let open_y = scene.world_position(door).y;
    assert!(open_y > door_y + 2.0, "door {door_y} -> {open_y}");
    // Leaving closes it again.
    place(&mut scene, &mut rt, Vec3::new(0.0, 1.1, 4.0));
    run!(90);
    assert!(scene.world_position(door).y < open_y - 2.0);
    // The jump pad throws the player up.
    place(&mut scene, &mut rt, PAD_AT + Vec3::new(0.0, 1.1, 0.0));
    let mut top = 0.0f32;
    for _ in 0..30 {
        frame(&mut scene, &mut rt, &idle);
        top = top.max(scene.world_position(player).y);
    }
    assert!(top > 4.0, "launched to {top}");
    run!(90);
    // The patrol catches the player: back to the start.
    let at = scene.world_position(patrol);
    place(&mut scene, &mut rt, at);
    run!(3);
    let back = scene.world_position(player);
    assert!(back.distance(PLAYER_START) < 1.0, "respawned at {back}");
    assert!(logs.iter().any(|l| l == "Door opening"), "{logs:?}");
    assert!(
        logs.iter().any(|l| l == "Caught by the patrol!"),
        "{logs:?}"
    );
}
