//! The User Guide's four Script Graph examples, in `SampleProject/Assets/Guides/`. Each guide
//! chapter (`docs/guide/*/05…08-guide-*.md`) walks through one of these graphs node by node,
//! so the text and the graphs must stay the same: change both together.
//!
//! - **FirstPerson**: WASD moves a Character Controller relative to where it faces, the mouse
//!   turns the body and tilts the camera, Space jumps, gravity pulls down.
//! - **Door**: two doors on hinges. Stand in a door's zone and press E to open or close it, or
//!   (Auto Open on) it opens when you walk in and closes when you leave. It turns smoothly.
//! - **Saving**: F adds points, F5 saves the score and the player's position with PlayerPrefs,
//!   and the next Play loads them on start.
//! - **Animation**: the input speed drives an Animator (Idle → Walk → Run on `Speed`, holding
//!   Shift runs) and Space plays a Jump through a trigger.
//!
//! Regenerate with `cargo test -p misty_editor -- --ignored write_guide_examples`; the other
//! tests check the committed files still match and play each scene headlessly.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::animation::controller::{
    Condition, ConditionMode, Node as ANode, NodePositions, ParamKind, Parameter, State,
    StateMachine, Transition,
};
use misty_engine::animation::{AnimationClip, AnimatorController, Property};
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    Animator, AudioListener, BoxCollider, Camera, CharacterController, MeshFilter, MeshRenderer,
    PrimitiveMesh, RectTransform, Rigidbody, Scene, ScriptRef, ScriptValue, Scripts, TextAnchor,
    Transform, UiText,
};
use misty_engine::ui::presets;
use misty_engine::visual_script::{Literal, NoFiles, PinType, ScriptGraph, StickyNote, Variable};

pub const FIRST_PERSON_SCENE: &str = "Assets/Guides/FirstPerson/FirstPerson.scene";
pub const FIRST_PERSON_GRAPH: &str = "Assets/Guides/FirstPerson/FirstPersonPlayer.vgraph";
pub const DOOR_SCENE: &str = "Assets/Guides/Door/Door.scene";
pub const DOOR_GRAPH: &str = "Assets/Guides/Door/Door.vgraph";
pub const SAVING_SCENE: &str = "Assets/Guides/Saving/Saving.scene";
pub const SAVING_GRAPH: &str = "Assets/Guides/Saving/SaveLoad.vgraph";
pub const ANIMATION_SCENE: &str = "Assets/Guides/Animation/Animation.scene";
pub const ANIMATION_GRAPH: &str = "Assets/Guides/Animation/CharacterAnimation.vgraph";
const CONTROLLER: &str = "Assets/Guides/Animation/Character.controller";
const IDLE: &str = "Assets/Guides/Animation/Idle.anim";
const WALK: &str = "Assets/Guides/Animation/Walk.anim";
const RUN: &str = "Assets/Guides/Animation/Run.anim";
const JUMP: &str = "Assets/Guides/Animation/Jump.anim";

const CHECKER: &str = "Assets/Materials/Checker.mat";
const WOOD: &str = "Assets/Materials/Wood.mat";
const STEEL: &str = "Assets/Materials/Blue Steel.mat";
const CRATE: &str = "Assets/Materials/AO Crate.mat";
const RED: &str = "Assets/Materials/Red.mat";

/// PlayerPrefs keys the Saving guide uses.
pub const SCORE_KEY: &str = "Guide.Score";
pub const X_KEY: &str = "Guide.PlayerX";
pub const Y_KEY: &str = "Guide.PlayerY";
pub const Z_KEY: &str = "Guide.PlayerZ";

/// Capsule of the guide player (feet at its origin).
const PLAYER_HEIGHT: f32 = 1.8;
const PLAYER_RADIUS: f32 = 0.35;
const EYE_HEIGHT: f32 = 1.6;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

// ---- graph building ---------------------------------------------------------------------------

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

    /// `Input: GetKeyDown(key)`.
    fn key_down(&mut self, key: &str, col: f32, row: f32) -> u32 {
        let k = self.api("Input", "GetKeyDown", "call", col, row);
        self.v(k, "Key", s(key));
        k
    }

    /// A Branch on `Other` having the Player tag, entered from `from.out`; returns the Branch.
    fn if_player(&mut self, from: u32, out: &str, col: f32, row: f32) -> u32 {
        let tag = self.api("#GameObject", "CompareTag", "call", col, row + 1.2);
        self.w(from, "Other", tag, "Target");
        self.v(tag, "Tag", s("Player"));
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

fn yes(b: bool) -> Literal {
    Literal::Bool(b)
}

// ---- a) first-person player ---------------------------------------------------------------

/// WASD + mouse look + jump on a Character Controller, the camera a child at eye height.
pub fn first_person_graph() -> ScriptGraph {
    let mut b = B::new();
    b.var(Variable::new("Walk Speed", PinType::Float, f(5.0)).object());
    b.var(Variable::new("Jump Speed", PinType::Float, f(5.0)).object());
    b.var(Variable::new("Mouse Sensitivity", PinType::Float, f(2.0)).object());
    b.var(Variable::new("Gravity", PinType::Float, f(-15.0)));
    b.var(Variable::new("Velocity Y", PinType::Float, f(0.0)));
    b.var(Variable::new("Pitch", PinType::Float, f(0.0)));
    b.note(
        0.0,
        -1.4,
        460.0,
        "First-person player: the mouse turns the body (left/right) and tilts the camera \
         (up/down); WASD walks where you look; Space jumps; gravity pulls down.",
    );
    // 1. Start: hide and lock the mouse cursor.
    let start = b.n("OnStart", 0.0, 0.0);
    let lock = b.api("Cursor", "lockState", "set", 1.0, 0.0);
    b.v(lock, "Value", s("Locked"));
    b.w(start, "Out", lock, "In");

    // Every frame, five steps in order.
    let update = b.n("OnUpdate", 0.0, 2.0);
    let seq = b.np("Sequence", &[("count", "5")], 1.0, 2.0);
    b.w(update, "Out", seq, "In");
    let sens = b.get("Mouse Sensitivity", 1.0, 4.0);

    // 2. Look left / right: turn the whole body.
    let mouse_x = b.api("Input", "GetAxis", "call", 2.0, 3.6);
    b.v(mouse_x, "Axis Name", s("Mouse X"));
    let yaw = b.n("MultiplyFloat", 3.0, 3.6);
    b.w(mouse_x, "Result", yaw, "A");
    b.w(sens, "Value", yaw, "B");
    let turn = b.n("MakeVector3", 4.0, 3.6);
    b.w(yaw, "Result", turn, "Y");
    let rotate = b.api("#Transform", "Rotate", "call", 5.0, 2.0);
    b.w(seq, "Then 0", rotate, "In");
    b.w(turn, "Vector", rotate, "Euler Degrees");

    // 3. Look up / down: tilt only the camera, and never past straight up or down.
    let mouse_y = b.api("Input", "GetAxis", "call", 2.0, 5.6);
    b.v(mouse_y, "Axis Name", s("Mouse Y"));
    let tilt = b.n("MultiplyFloat", 3.0, 5.6);
    b.w(mouse_y, "Result", tilt, "A");
    b.w(sens, "Value", tilt, "B");
    let pitch = b.get("Pitch", 3.0, 6.8);
    let up = b.n("SubtractFloat", 4.0, 6.0);
    b.w(pitch, "Value", up, "A");
    b.w(tilt, "Result", up, "B");
    let clamp = b.n("ClampFloat", 5.0, 6.0);
    b.w(up, "Result", clamp, "Value");
    b.v(clamp, "Min", f(-85.0));
    b.v(clamp, "Max", f(85.0));
    let set_pitch = b.set("Pitch", 6.0, 4.6);
    b.w(seq, "Then 1", set_pitch, "In");
    b.w(clamp, "Result", set_pitch, "Value");
    let camera = b.call("GameObject", "Find", "name", 6.0, 7.2);
    b.v(camera, "Name", s("Main Camera"));
    let pitch2 = b.get("Pitch", 6.0, 6.2);
    let look = b.n("MakeVector3", 7.0, 6.2);
    b.w(pitch2, "Value", look, "X");
    let tilt_cam = b.api("#Transform", "localEulerAngles", "set", 8.0, 4.6);
    b.w(set_pitch, "Out", tilt_cam, "In");
    b.w(camera, "Result", tilt_cam, "Target");
    b.w(look, "Vector", tilt_cam, "Value");

    // 4. Jump: only when standing on something.
    let grounded = b.api("#CharacterController", "isGrounded", "get", 2.0, 9.0);
    let space = b.key_down("space", 2.0, 10.0);
    let both = b.n("And", 3.0, 9.4);
    b.w(grounded, "Value", both, "A");
    b.w(space, "Result", both, "B");
    let can_jump = b.n("Branch", 4.0, 8.4);
    b.w(seq, "Then 2", can_jump, "In");
    b.w(both, "Result", can_jump, "Condition");
    let jump_speed = b.get("Jump Speed", 4.0, 10.0);
    let set_jump = b.set("Velocity Y", 5.0, 8.4);
    b.w(can_jump, "True", set_jump, "In");
    b.w(jump_speed, "Value", set_jump, "Value");

    // 5. Gravity: in the air the fall speeds up; on the ground it rests at -1 (keeps contact).
    let vy = b.get("Velocity Y", 2.0, 12.0);
    let gravity = b.get("Gravity", 2.0, 13.0);
    let g_dt = b.n("MultiplyFloat", 3.0, 13.0);
    b.w(gravity, "Value", g_dt, "A");
    b.w(update, "Delta Time", g_dt, "B");
    let fall = b.n("AddFloat", 4.0, 12.4);
    b.w(vy, "Value", fall, "A");
    b.w(g_dt, "Result", fall, "B");
    let rest = b.n("MaxFloat", 5.0, 12.0);
    b.w(fall, "Result", rest, "A");
    b.v(rest, "B", f(-1.0));
    let pick = b.n("Select", 6.0, 11.6);
    b.w(grounded, "Value", pick, "Condition");
    b.w(rest, "Result", pick, "True");
    b.w(fall, "Result", pick, "False");
    let set_fall = b.set("Velocity Y", 7.0, 10.6);
    b.w(seq, "Then 3", set_fall, "In");
    b.w(pick, "Result", set_fall, "Value");

    // 6. Move: forward/back and left/right along the way the body faces, plus the fall.
    let h = b.api("Input", "GetAxis", "call", 2.0, 15.0);
    b.v(h, "Axis Name", s("Horizontal"));
    let v = b.api("Input", "GetAxis", "call", 2.0, 17.0);
    b.v(v, "Axis Name", s("Vertical"));
    let right = b.api("#Transform", "right", "get", 2.0, 16.0);
    let forward = b.api("#Transform", "forward", "get", 2.0, 18.0);
    let side = b.n("ScaleVector3", 3.0, 15.4);
    b.w(right, "Value", side, "Vector");
    b.w(h, "Result", side, "Scale");
    let ahead = b.n("ScaleVector3", 3.0, 17.4);
    b.w(forward, "Value", ahead, "Vector");
    b.w(v, "Result", ahead, "Scale");
    let walk = b.n("AddVector3", 4.0, 16.4);
    b.w(side, "Result", walk, "A");
    b.w(ahead, "Result", walk, "B");
    let speed = b.get("Walk Speed", 4.0, 17.6);
    let walk_fast = b.n("ScaleVector3", 5.0, 16.4);
    b.w(walk, "Result", walk_fast, "Vector");
    b.w(speed, "Value", walk_fast, "Scale");
    let vy2 = b.get("Velocity Y", 5.0, 14.6);
    let drop = b.n("MakeVector3", 6.0, 14.6);
    b.w(vy2, "Value", drop, "Y");
    let velocity = b.n("AddVector3", 7.0, 15.6);
    b.w(walk_fast, "Result", velocity, "A");
    b.w(drop, "Vector", velocity, "B");
    let step = b.n("ScaleVector3", 8.0, 15.6);
    b.w(velocity, "Result", step, "Vector");
    b.w(update, "Delta Time", step, "Scale");
    let mv = b.call("#CharacterController", "Move", "motion", 9.0, 13.0);
    b.w(seq, "Then 4", mv, "In");
    b.w(step, "Result", mv, "motion");
    b.g
}

// ---- b) door ------------------------------------------------------------------------------

/// A door zone: E toggles the door while the player is inside (or Auto Open opens it on
/// entering and closes it on leaving); the hinge turns smoothly towards open or closed.
pub fn door_graph() -> ScriptGraph {
    let mut b = B::new();
    b.var(Variable::new("Door Name", PinType::String, s("Door Hinge")).object());
    b.var(Variable::new("Open Angle", PinType::Float, f(90.0)).object());
    b.var(Variable::new("Turn Speed", PinType::Float, f(180.0)).object());
    b.var(Variable::new("Auto Open", PinType::Bool, yes(false)).object());
    b.var(Variable::new("Is Open", PinType::Bool, yes(false)));
    b.var(Variable::new("Player Inside", PinType::Bool, yes(false)));
    b.var(Variable::new("Angle", PinType::Float, f(0.0)));
    b.note(
        0.0,
        -1.4,
        460.0,
        "Door zone: remember whether the player is inside; E (or walking in, with Auto Open) \
         opens and closes the door; every frame the hinge turns towards its target angle.",
    );
    // 1. The player walks in.
    let enter = b.n("OnTriggerEnter", 0.0, 0.0);
    let is_player = b.if_player(enter, "Out", 1.0, 0.0);
    let inside = b.set("Player Inside", 3.0, 0.0);
    b.w(is_player, "True", inside, "In");
    b.v(inside, "Value", yes(true));
    let auto = b.get("Auto Open", 3.0, 1.4);
    let auto_open = b.n("Branch", 4.0, 0.0);
    b.w(inside, "Out", auto_open, "In");
    b.w(auto, "Value", auto_open, "Condition");
    let open = b.set("Is Open", 5.0, 0.0);
    b.w(auto_open, "True", open, "In");
    b.v(open, "Value", yes(true));

    // 2. The player walks out.
    let exit = b.n("OnTriggerExit", 0.0, 3.0);
    let was_player = b.if_player(exit, "Out", 1.0, 3.0);
    let outside = b.set("Player Inside", 3.0, 3.0);
    b.w(was_player, "True", outside, "In");
    b.v(outside, "Value", yes(false));
    let auto2 = b.get("Auto Open", 3.0, 4.4);
    let auto_close = b.n("Branch", 4.0, 3.0);
    b.w(outside, "Out", auto_close, "In");
    b.w(auto2, "Value", auto_close, "Condition");
    let close = b.set("Is Open", 5.0, 3.0);
    b.w(auto_close, "True", close, "In");
    b.v(close, "Value", yes(false));

    // Every frame: the key, then the turning.
    let update = b.n("OnUpdate", 0.0, 6.0);
    let seq = b.np("Sequence", &[("count", "2")], 1.0, 6.0);
    b.w(update, "Out", seq, "In");

    // 3. E toggles the door, but only from inside the zone.
    let here = b.get("Player Inside", 1.0, 7.4);
    let e_key = b.key_down("e", 1.0, 8.4);
    let both = b.n("And", 2.0, 7.8);
    b.w(here, "Value", both, "A");
    b.w(e_key, "Result", both, "B");
    let pressed = b.n("Branch", 3.0, 6.0);
    b.w(seq, "Then 0", pressed, "In");
    b.w(both, "Result", pressed, "Condition");
    let state = b.get("Is Open", 3.0, 7.6);
    let flip = b.n("Not", 4.0, 7.6);
    b.w(state, "Value", flip, "Value");
    let toggle = b.set("Is Open", 5.0, 6.0);
    b.w(pressed, "True", toggle, "In");
    b.w(flip, "Result", toggle, "Value");
    let state2 = b.get("Is Open", 5.0, 7.6);
    let words = b.n("Select", 6.0, 7.4);
    b.w(state2, "Value", words, "Condition");
    b.v(words, "True", s("Door opened"));
    b.v(words, "False", s("Door closed"));
    let log = b.api("Debug", "Log", "call", 7.0, 6.0);
    b.w(toggle, "Out", log, "In");
    b.w(words, "Result", log, "Message");

    // 4. Turn smoothly towards the target angle.
    let is_open = b.get("Is Open", 1.0, 11.0);
    let open_angle = b.get("Open Angle", 1.0, 12.0);
    let target = b.n("Select", 2.0, 11.0);
    b.w(is_open, "Value", target, "Condition");
    b.w(open_angle, "Value", target, "True");
    b.v(target, "False", f(0.0));
    let turn_speed = b.get("Turn Speed", 1.0, 13.4);
    let max_step = b.n("MultiplyFloat", 2.0, 13.4);
    b.w(turn_speed, "Value", max_step, "A");
    b.w(update, "Delta Time", max_step, "B");
    let angle = b.get("Angle", 2.0, 10.0);
    let towards = b.n("MoveTowards", 3.0, 11.2);
    b.w(angle, "Value", towards, "Current");
    b.w(target, "Result", towards, "Target");
    b.w(max_step, "Result", towards, "Max Delta");
    let set_angle = b.set("Angle", 4.0, 9.4);
    b.w(seq, "Then 1", set_angle, "In");
    b.w(towards, "Result", set_angle, "Value");
    let name = b.get("Door Name", 4.0, 11.6);
    let door = b.call("GameObject", "Find", "name", 5.0, 11.6);
    b.w(name, "Value", door, "Name");
    let angle2 = b.get("Angle", 5.0, 10.6);
    let rotation = b.n("MakeVector3", 6.0, 10.6);
    b.w(angle2, "Value", rotation, "Y");
    let hinge = b.api("#Transform", "localEulerAngles", "set", 7.0, 9.4);
    b.w(set_angle, "Out", hinge, "In");
    b.w(door, "Result", hinge, "Target");
    b.w(rotation, "Vector", hinge, "Value");
    b.g
}

// ---- c) saving ----------------------------------------------------------------------------

/// Score and position: load on start, F adds points, F5 saves.
pub fn saving_graph() -> ScriptGraph {
    let mut b = B::new();
    b.var(Variable::new("Score", PinType::Int, Literal::Int(0)));
    b.note(
        0.0,
        -1.4,
        460.0,
        "Save and load: on start, load the saved score and position if there is a save; \
         F adds 10 points; F5 saves the score and where the player stands.",
    );
    let score_text = b.call("GameObject", "Find", "name", 7.0, 2.4);
    b.v(score_text, "Name", s("Score Text"));
    let label = |b: &mut B, col: f32, row: f32| {
        let score = b.get("Score", col, row + 1.0);
        let text = b.n("Append", col + 1.0, row + 1.0);
        b.v(text, "A", s("Score: "));
        b.w(score, "Value", text, "B");
        text
    };

    // 1. Start: load, then show the score.
    let start = b.n("OnStart", 0.0, 0.0);
    let seq = b.np("Sequence", &[("count", "2")], 1.0, 0.0);
    b.w(start, "Out", seq, "In");
    let has = b.api("PlayerPrefs", "HasKey", "call", 1.0, 1.4);
    b.v(has, "Key", s(SCORE_KEY));
    let saved = b.n("Branch", 2.0, 0.0);
    b.w(seq, "Then 0", saved, "In");
    b.w(has, "Result", saved, "Condition");
    let load_score = b.api("PlayerPrefs", "GetInt", "call", 2.0, 1.4);
    b.v(load_score, "Key", s(SCORE_KEY));
    let set_score = b.set("Score", 3.0, 0.0);
    b.w(saved, "True", set_score, "In");
    b.w(load_score, "Result", set_score, "Value");
    let mut xyz = Vec::new();
    for (i, key) in [X_KEY, Y_KEY, Z_KEY].into_iter().enumerate() {
        let get = b.api("PlayerPrefs", "GetFloat", "call", 3.0, 1.4 + i as f32);
        b.v(get, "Key", s(key));
        xyz.push(get);
    }
    let pos = b.n("MakeVector3", 4.0, 2.0);
    for (get, pin) in xyz.iter().zip(["X", "Y", "Z"]) {
        b.w(*get, "Result", pos, pin);
    }
    let place = b.api("#Transform", "position", "set", 5.0, 0.0);
    b.w(set_score, "Out", place, "In");
    b.w(pos, "Vector", place, "Value");
    let loaded = b.api("Debug", "Log", "call", 6.0, 0.0);
    b.v(loaded, "Message", s("Loaded the saved game"));
    b.w(place, "Out", loaded, "In");
    let text = label(&mut b, 5.0, 3.0);
    let show = b.api("#Text", "text", "set", 8.0, 0.0);
    b.w(seq, "Then 1", show, "In");
    b.w(score_text, "Result", show, "Target");
    b.w(text, "Result", show, "Value");

    // Every frame: the two keys.
    let update = b.n("OnUpdate", 0.0, 6.0);
    let seq2 = b.np("Sequence", &[("count", "2")], 1.0, 6.0);
    b.w(update, "Out", seq2, "In");

    // 2. F: +10 points, and show them.
    let f_key = b.key_down("f", 1.0, 7.4);
    let add_points = b.n("Branch", 2.0, 6.0);
    b.w(seq2, "Then 0", add_points, "In");
    b.w(f_key, "Result", add_points, "Condition");
    let current = b.get("Score", 2.0, 7.4);
    let plus = b.n("AddInt", 3.0, 7.4);
    b.w(current, "Value", plus, "A");
    b.v(plus, "B", Literal::Int(10));
    let set_plus = b.set("Score", 4.0, 6.0);
    b.w(add_points, "True", set_plus, "In");
    b.w(plus, "Result", set_plus, "Value");
    let text2 = label(&mut b, 5.0, 6.6);
    let score_text2 = b.call("GameObject", "Find", "name", 6.0, 9.0);
    b.v(score_text2, "Name", s("Score Text"));
    let show2 = b.api("#Text", "text", "set", 8.0, 6.0);
    b.w(set_plus, "Out", show2, "In");
    b.w(score_text2, "Result", show2, "Target");
    b.w(text2, "Result", show2, "Value");

    // 3. F5: save the score and the position, then write the file.
    let f5 = b.key_down("f5", 1.0, 11.4);
    let save_now = b.n("Branch", 2.0, 10.4);
    b.w(seq2, "Then 1", save_now, "In");
    b.w(f5, "Result", save_now, "Condition");
    let score = b.get("Score", 2.0, 11.8);
    let save_score = b.api("PlayerPrefs", "SetInt", "call", 3.0, 10.4);
    b.v(save_score, "Key", s(SCORE_KEY));
    b.w(save_now, "True", save_score, "In");
    b.w(score, "Value", save_score, "Value");
    let here = b.api("#Transform", "position", "get", 3.0, 12.4);
    let parts = b.n("BreakVector3", 4.0, 12.4);
    b.w(here, "Value", parts, "Vector");
    let mut prev = save_score;
    for (i, (key, pin)) in [(X_KEY, "X"), (Y_KEY, "Y"), (Z_KEY, "Z")]
        .into_iter()
        .enumerate()
    {
        let set = b.api("PlayerPrefs", "SetFloat", "call", 4.0 + i as f32, 10.4);
        b.v(set, "Key", s(key));
        b.w(prev, "Out", set, "In");
        b.w(parts, pin, set, "Value");
        prev = set;
    }
    let write = b.api("PlayerPrefs", "Save", "call", 7.0, 10.4);
    b.w(prev, "Out", write, "In");
    let done = b.api("Debug", "Log", "call", 8.0, 10.4);
    b.v(done, "Message", s("Game saved"));
    b.w(write, "Out", done, "In");
    b.g
}

// ---- d) animation -------------------------------------------------------------------------

/// The input's speed drives the Animator: Speed for Idle / Walk / Run, a trigger for Jump.
pub fn animation_graph() -> ScriptGraph {
    let mut b = B::new();
    b.var(Variable::new("Walk Speed", PinType::Float, f(2.0)).object());
    b.var(Variable::new("Run Speed", PinType::Float, f(6.0)).object());
    b.var(Variable::new("Speed", PinType::Float, f(0.0)));
    b.note(
        0.0,
        -1.4,
        460.0,
        "Animation: work out how fast the character moves (Shift runs), move it, and hand \
         that speed to the Animator, whose transitions pick Idle, Walk or Run. Space jumps.",
    );
    let update = b.n("OnUpdate", 0.0, 0.0);
    let seq = b.np("Sequence", &[("count", "4")], 1.0, 0.0);
    b.w(update, "Out", seq, "In");

    // 1. How fast: stick or keys (0…1) × walk or run speed.
    let h = b.api("Input", "GetAxis", "call", 1.0, 2.0);
    b.v(h, "Axis Name", s("Horizontal"));
    let v = b.api("Input", "GetAxis", "call", 1.0, 3.0);
    b.v(v, "Axis Name", s("Vertical"));
    let dir = b.n("MakeVector3", 2.0, 2.4);
    b.w(h, "Result", dir, "X");
    b.w(v, "Result", dir, "Z");
    let length = b.n("Magnitude", 3.0, 2.4);
    b.w(dir, "Vector", length, "Vector");
    let amount = b.n("Clamp01", 4.0, 2.4);
    b.w(length, "Result", amount, "Value");
    let shift = b.api("Input", "GetKey", "call", 2.0, 4.2);
    b.v(shift, "Key", s("left shift"));
    let run = b.get("Run Speed", 2.0, 5.2);
    let walk = b.get("Walk Speed", 2.0, 6.2);
    let top = b.n("Select", 3.0, 4.8);
    b.w(shift, "Result", top, "Condition");
    b.w(run, "Value", top, "True");
    b.w(walk, "Value", top, "False");
    let speed = b.n("MultiplyFloat", 5.0, 3.0);
    b.w(amount, "Result", speed, "A");
    b.w(top, "Result", speed, "B");
    let set_speed = b.set("Speed", 6.0, 0.0);
    b.w(seq, "Then 0", set_speed, "In");
    b.w(speed, "Result", set_speed, "Value");

    // 2. Move: the direction at that speed, this frame.
    let norm = b.n("Normalize", 3.0, 7.6);
    b.w(dir, "Vector", norm, "Vector");
    let current = b.get("Speed", 3.0, 8.6);
    let velocity = b.n("ScaleVector3", 4.0, 7.6);
    b.w(norm, "Result", velocity, "Vector");
    b.w(current, "Value", velocity, "Scale");
    let step = b.n("ScaleVector3", 5.0, 7.6);
    b.w(velocity, "Result", step, "Vector");
    b.w(update, "Delta Time", step, "Scale");
    let mv = b.call("#CharacterController", "Move", "motion", 7.0, 1.6);
    b.w(seq, "Then 1", mv, "In");
    b.w(step, "Result", mv, "motion");

    // 3. Tell the Animator (eased a little so the change blends).
    let current2 = b.get("Speed", 7.0, 4.4);
    let animate = b.api("#Animator", "SetFloat", "call", 8.0, 3.2);
    b.v(animate, "Name", s("Speed"));
    b.v(animate, "Damp Time", f(0.1));
    b.w(seq, "Then 2", animate, "In");
    b.w(current2, "Value", animate, "Value");

    // 4. Space: jump.
    let space = b.key_down("space", 7.0, 6.4);
    let jump = b.n("Branch", 8.0, 5.4);
    b.w(seq, "Then 3", jump, "In");
    b.w(space, "Result", jump, "Condition");
    let trigger = b.api("#Animator", "SetTrigger", "call", 9.0, 5.4);
    b.v(trigger, "Name", s("Jump"));
    b.w(jump, "True", trigger, "In");
    b.g
}

/// A clip from (property, [(time, value)]) curves on the child "Body".
fn clip(length: f32, looping: bool, curves: &[(Property, &[(f32, f32)])]) -> AnimationClip {
    let mut c = AnimationClip {
        length,
        looping,
        ..Default::default()
    };
    for (p, keys) in curves {
        for (t, v) in *keys {
            c.set_key("Body", *p, *t, *v);
        }
    }
    c
}

fn clips() -> [(&'static str, AnimationClip); 4] {
    let idle = clip(
        2.0,
        true,
        &[
            (
                Property::Position(1),
                &[(0.0, 0.9), (1.0, 0.94), (2.0, 0.9)],
            ),
            (Property::Rotation(0), &[(0.0, 0.0), (2.0, 0.0)]),
        ],
    );
    let walk = clip(
        0.8,
        true,
        &[
            (
                Property::Position(1),
                &[(0.0, 0.9), (0.2, 1.0), (0.4, 0.9), (0.6, 1.0), (0.8, 0.9)],
            ),
            (Property::Rotation(0), &[(0.0, 5.0), (0.8, 5.0)]),
        ],
    );
    let run = clip(
        0.5,
        true,
        &[
            (
                Property::Position(1),
                &[
                    (0.0, 0.9),
                    (0.125, 1.1),
                    (0.25, 0.9),
                    (0.375, 1.1),
                    (0.5, 0.9),
                ],
            ),
            (Property::Rotation(0), &[(0.0, 15.0), (0.5, 15.0)]),
        ],
    );
    let jump = clip(
        0.7,
        false,
        &[
            (Property::Position(1), &[(0.0, 0.9), (0.35, 2.0), (0.7, 0.9)]),
            (Property::Rotation(0), &[(0.0, 0.0), (0.7, 0.0)]),
        ],
    );
    [(IDLE, idle), (WALK, walk), (RUN, run), (JUMP, jump)]
}

/// Idle ⇄ Walk at Speed 0.5, Walk ⇄ Run at Speed 4, Jump from Any State on its trigger.
pub fn controller() -> AnimatorController {
    let param = |name: &str, kind| Parameter {
        name: name.into(),
        kind,
        default: 0.0,
    };
    let when = |mode, threshold| Condition {
        parameter: "Speed".into(),
        mode,
        threshold,
    };
    let go = |from: &str, to: &str, c: Condition| Transition {
        has_exit_time: false,
        duration: 0.15,
        conditions: vec![c],
        ..Transition::new(ANode::state(from), ANode::state(to))
    };
    let state = |name: &str, motion: &str, x: f32, y: f32| State {
        name: name.into(),
        motion: Some(motion.into()),
        position: [x, y],
        ..Default::default()
    };
    AnimatorController {
        parameters: vec![
            param("Speed", ParamKind::Float),
            param("Jump", ParamKind::Trigger),
        ],
        base: StateMachine {
            states: vec![
                state("Idle", IDLE, 0.0, 0.0),
                state("Walk", WALK, 260.0, 0.0),
                state("Run", RUN, 520.0, 0.0),
                State {
                    looping: false,
                    ..state("Jump", JUMP, 260.0, -150.0)
                },
            ],
            default_state: Some("Idle".into()),
            transitions: vec![
                go("Idle", "Walk", when(ConditionMode::Greater, 0.5)),
                go("Walk", "Idle", when(ConditionMode::Less, 0.5)),
                go("Walk", "Run", when(ConditionMode::Greater, 4.0)),
                go("Run", "Walk", when(ConditionMode::Less, 4.0)),
                Transition {
                    has_exit_time: false,
                    duration: 0.1,
                    conditions: vec![Condition {
                        parameter: "Jump".into(),
                        mode: ConditionMode::If,
                        threshold: 0.0,
                    }],
                    ..Transition::new(ANode::AnyState, ANode::state("Jump"))
                },
                Transition {
                    has_exit_time: true,
                    exit_time: 0.9,
                    duration: 0.1,
                    ..Transition::new(ANode::state("Jump"), ANode::Exit)
                },
            ],
            nodes: NodePositions {
                entry: [-260.0, 0.0],
                any_state: [-260.0, -150.0],
                exit: [520.0, -150.0],
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

// ---- scenes -------------------------------------------------------------------------------

fn graph_ref(path: &str, fields: &[(&str, ScriptValue)]) -> ScriptRef {
    ScriptRef {
        path: path.into(),
        enabled: true,
        fields: fields
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
    }
}

fn shape(s: &mut Scene, name: &str, mesh: PrimitiveMesh, t: Transform, mat: &str) -> Entity {
    shape_in(s, name, None, mesh, t, mat)
}

fn shape_in(
    s: &mut Scene,
    name: &str,
    parent: Option<Entity>,
    mesh: PrimitiveMesh,
    t: Transform,
    mat: &str,
) -> Entity {
    let e = s.spawn(name, parent);
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

/// A guide scene: sun, ground; the default camera is left out (the player brings one).
fn base(name: &str, size: f32) -> Scene {
    let mut s = Scene::default_3d(name);
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.despawn(cam);
    }
    block(
        &mut s,
        "Ground",
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(size, 1.0, size),
        CHECKER,
    );
    s
}

/// The first-person player (feet at `at`) with its camera at eye height and `graphs`.
fn player(s: &mut Scene, at: Vec3, graphs: Vec<ScriptRef>) -> Entity {
    let p = s.spawn("Player", None);
    s.set_transform(p, Transform::from_position(at));
    s.game_object_mut(p).tag = "Player".into();
    s.add(
        p,
        CharacterController {
            height: PLAYER_HEIGHT,
            radius: PLAYER_RADIUS,
            center: Vec3::new(0.0, PLAYER_HEIGHT * 0.5, 0.0),
            ..Default::default()
        },
    );
    s.add(p, Scripts { list: graphs });
    let body = shape_in(
        s,
        "Body",
        Some(p),
        PrimitiveMesh::Capsule,
        Transform::from_position(Vec3::new(0.0, PLAYER_HEIGHT * 0.5, 0.0)),
        STEEL,
    );
    let mut t = s.transform(body);
    t.scale = Vec3::new(0.7, 0.9, 0.7);
    s.set_transform(body, t);
    let cam = s.spawn("Main Camera", Some(p));
    s.set_transform(
        cam,
        Transform::from_position(Vec3::new(0.0, EYE_HEIGHT, 0.0)),
    );
    s.game_object_mut(cam).tag = "MainCamera".into();
    s.add(
        cam,
        Camera {
            near: 0.1,
            ..Default::default()
        },
    );
    s.add(cam, AudioListener::default());
    p
}

/// A line of white UI text in the top-left corner, `y` pixels down.
fn hint(s: &mut Scene, canvas: Entity, name: &str, y: f32, size: f32, body: &str) -> Entity {
    let corner = Vec2::new(0.0, 1.0);
    let e = presets::text(s, name, canvas);
    *s.world.get::<&mut RectTransform>(e).unwrap() = RectTransform {
        anchor_min: corner,
        anchor_max: corner,
        pivot: corner,
        anchored_position: Vec2::new(20.0, -y),
        size_delta: Vec2::new(1100.0, size * 1.8),
    };
    let mut t = s.world.get::<&mut UiText>(e).unwrap();
    t.text = body.into();
    t.font_size = size;
    t.color = [1.0; 4];
    t.alignment = TextAnchor::UpperLeft;
    drop(t);
    e
}

fn canvas(s: &mut Scene) -> Entity {
    let c = presets::spawn(s, "Canvas", None, RectTransform::stretched());
    s.add(c, misty_engine::scene::Canvas::default());
    s.add(c, misty_engine::scene::CanvasScaler::default());
    c
}

fn kinematic() -> Rigidbody {
    Rigidbody {
        is_kinematic: true,
        use_gravity: false,
        ..Default::default()
    }
}

pub const FIRST_PERSON_START: Vec3 = Vec3::new(0.0, 0.05, -4.0);

pub fn first_person_scene() -> Scene {
    let mut s = base("FirstPerson", 30.0);
    player(
        &mut s,
        FIRST_PERSON_START,
        vec![graph_ref(FIRST_PERSON_GRAPH, &[])],
    );
    // Things to look at and jump onto.
    block(
        &mut s,
        "Crate",
        Vec3::new(2.0, 0.5, 2.0),
        Vec3::ONE,
        CRATE,
    );
    block(
        &mut s,
        "Low Wall",
        Vec3::new(-3.0, 0.4, 4.0),
        Vec3::new(4.0, 0.8, 0.5),
        WOOD,
    );
    block(
        &mut s,
        "Pillar",
        Vec3::new(0.0, 2.0, 8.0),
        Vec3::new(1.0, 4.0, 1.0),
        RED,
    );
    let c = canvas(&mut s);
    hint(
        &mut s,
        c,
        "Help Text",
        16.0,
        18.0,
        "WASD: walk   Mouse: look   Space: jump   (Script Graph: FirstPersonPlayer)",
    );
    s
}

/// Hinge positions of the two doors and their zones.
pub const KEY_DOOR_AT: Vec3 = Vec3::new(-4.0, 0.0, 6.0);
pub const AUTO_DOOR_AT: Vec3 = Vec3::new(4.0, 0.0, 6.0);

/// A door on a hinge at `at` (the panel to its right) and its zone in front, with the Door
/// graph; returns (hinge, zone).
fn door(s: &mut Scene, name: &str, at: Vec3, auto: bool) -> (Entity, Entity) {
    let hinge = s.spawn(&format!("{name} Hinge"), None);
    s.set_transform(hinge, Transform::from_position(at));
    let mut t = Transform::from_position(Vec3::new(1.0, 1.25, 0.0));
    t.scale = Vec3::new(2.0, 2.5, 0.15);
    let panel = shape_in(s, name, Some(hinge), PrimitiveMesh::Cube, t, CRATE);
    s.add(panel, BoxCollider::default());
    s.add(panel, kinematic());
    // Walls either side of the doorway.
    block(
        s,
        &format!("{name} Wall Left"),
        at + Vec3::new(-1.0, 1.5, 0.0),
        Vec3::new(2.0, 3.0, 0.4),
        WOOD,
    );
    block(
        s,
        &format!("{name} Wall Right"),
        at + Vec3::new(3.0, 1.5, 0.0),
        Vec3::new(2.0, 3.0, 0.4),
        WOOD,
    );
    let zone = s.spawn(&format!("{name} Zone"), None);
    s.set_transform(
        zone,
        Transform::from_position(at + Vec3::new(1.0, 1.0, -1.5)),
    );
    s.add(
        zone,
        BoxCollider {
            is_trigger: true,
            size: Vec3::new(3.0, 2.0, 3.0),
            ..Default::default()
        },
    );
    s.add(
        zone,
        Scripts {
            list: vec![graph_ref(
                DOOR_GRAPH,
                &[
                    (
                        "Door Name",
                        ScriptValue::Text(format!("{name} Hinge")),
                    ),
                    ("Auto Open", ScriptValue::Bool(auto)),
                ],
            )],
        },
    );
    (hinge, zone)
}

pub fn door_scene() -> Scene {
    let mut s = base("Door", 30.0);
    player(
        &mut s,
        FIRST_PERSON_START,
        vec![graph_ref(FIRST_PERSON_GRAPH, &[])],
    );
    door(&mut s, "Key Door", KEY_DOOR_AT, false);
    door(&mut s, "Auto Door", AUTO_DOOR_AT, true);
    let c = canvas(&mut s);
    hint(
        &mut s,
        c,
        "Help Text",
        16.0,
        18.0,
        "Left door: stand in front and press E.   Right door: opens by itself (Auto Open).",
    );
    s
}

pub const SAVING_START: Vec3 = Vec3::new(0.0, 0.05, -4.0);

pub fn saving_scene() -> Scene {
    let mut s = base("Saving", 30.0);
    player(
        &mut s,
        SAVING_START,
        vec![
            graph_ref(FIRST_PERSON_GRAPH, &[]),
            graph_ref(SAVING_GRAPH, &[]),
        ],
    );
    for (i, x) in [-6.0f32, -2.0, 2.0, 6.0].into_iter().enumerate() {
        block(
            &mut s,
            &format!("Marker {}", i + 1),
            Vec3::new(x, 0.25, 6.0),
            Vec3::new(1.0, 0.5, 1.0),
            if i % 2 == 0 { RED } else { WOOD },
        );
    }
    let c = canvas(&mut s);
    let score = hint(&mut s, c, "Score Text", 16.0, 26.0, "Score: 0");
    s.world.get::<&mut UiText>(score).unwrap().color = [1.0, 0.86, 0.3, 1.0];
    hint(
        &mut s,
        c,
        "Help Text",
        56.0,
        18.0,
        "F: +10 points   F5: save   Stop and Play again: the score and your position come back",
    );
    s
}

pub const CHARACTER_START: Vec3 = Vec3::new(0.0, 0.05, 0.0);

pub fn animation_scene() -> Scene {
    let mut s = base("Animation", 30.0);
    let cam = s.spawn("Main Camera", None);
    s.set_transform(
        cam,
        Transform::from_position(Vec3::new(0.0, 4.0, -9.0)).with_euler(Vec3::new(18.0, 0.0, 0.0)),
    );
    s.game_object_mut(cam).tag = "MainCamera".into();
    s.add(cam, Camera::default());
    s.add(cam, AudioListener::default());
    let ch = s.spawn("Character", None);
    s.set_transform(ch, Transform::from_position(CHARACTER_START));
    s.add(
        ch,
        CharacterController {
            height: PLAYER_HEIGHT,
            radius: PLAYER_RADIUS,
            center: Vec3::new(0.0, PLAYER_HEIGHT * 0.5, 0.0),
            ..Default::default()
        },
    );
    s.add(
        ch,
        Animator {
            controller: Some(CONTROLLER.into()),
            ..Default::default()
        },
    );
    s.add(
        ch,
        Scripts {
            list: vec![graph_ref(ANIMATION_GRAPH, &[])],
        },
    );
    let mut t = Transform::from_position(Vec3::new(0.0, 0.9, 0.0));
    t.scale = Vec3::new(0.8, 1.8, 0.8);
    shape_in(&mut s, "Body", Some(ch), PrimitiveMesh::Cube, t, STEEL);
    let c = canvas(&mut s);
    hint(
        &mut s,
        c,
        "Help Text",
        16.0,
        18.0,
        "WASD: walk   Hold Shift: run   Space: jump   (watch the Animator window while playing)",
    );
    s
}

fn graphs() -> [(&'static str, ScriptGraph); 4] {
    [
        (FIRST_PERSON_GRAPH, first_person_graph()),
        (DOOR_GRAPH, door_graph()),
        (SAVING_GRAPH, saving_graph()),
        (ANIMATION_GRAPH, animation_graph()),
    ]
}

fn scenes() -> [(&'static str, Scene); 4] {
    [
        (FIRST_PERSON_SCENE, first_person_scene()),
        (DOOR_SCENE, door_scene()),
        (SAVING_SCENE, saving_scene()),
        (ANIMATION_SCENE, animation_scene()),
    ]
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_guide_examples() {
    let root = project();
    for (rel, g) in graphs() {
        g.save(&root.join(rel)).unwrap();
    }
    for (rel, c) in clips() {
        c.save(&root.join(rel)).unwrap();
    }
    controller().save(&root.join(CONTROLLER)).unwrap();
    for (rel, scene) in scenes() {
        scene.save(&root.join(rel)).unwrap();
    }
}

#[test]
fn guide_examples_match_their_generator() {
    let root = project();
    for (rel, g) in graphs() {
        let on_disk = std::fs::read_to_string(root.join(rel)).unwrap();
        assert_eq!(
            on_disk.replace("\r\n", "\n"),
            g.to_json() + "\n",
            "{rel}: regenerate with write_guide_examples"
        );
        let errors = misty_engine::visual_script::compile::check(&g, &NoFiles);
        assert!(errors.is_empty(), "{rel}: {errors:?}");
    }
    for (rel, c) in clips() {
        crate::test_util::assert_close(&AnimationClip::load(&root.join(rel)).unwrap(), &c, rel);
    }
    crate::test_util::assert_close(
        &AnimatorController::load(&root.join(CONTROLLER)).unwrap(),
        &controller(),
        CONTROLLER,
    );
    for (rel, scene) in scenes() {
        let text = std::fs::read_to_string(root.join(rel)).unwrap();
        crate::test_util::assert_json_close(
            &text,
            &scene.to_json(),
            format!("{rel}: regenerate with write_guide_examples"),
        );
        let (_, report) = Scene::from_json("guide", &text).unwrap();
        assert!(report.warnings.is_empty(), "{rel}: {:?}", report.warnings);
    }
}

#[cfg(test)]
mod play {
    use super::*;
    use misty_engine::audio::Audio;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::{Input, LogKind};
    use misty_engine::settings::ProjectSettings;

    /// A guide scene playing headlessly.
    pub struct Game {
        pub scene: Scene,
        pub rt: Runtime,
        time: Time,
        pub logs: Vec<String>,
    }

    impl Game {
        pub fn start(scene_path: &str) -> Self {
            let root = project();
            let (mut scene, _) = Scene::load(&root.join(scene_path)).unwrap();
            let settings = ProjectSettings::load(&root).unwrap();
            let (audio, _) = Audio::null(root.clone());
            let rt = Runtime::start_with_audio(&mut scene, root, &settings, audio);
            let mut g = Self {
                scene,
                rt,
                time: Time::new(&settings.time),
                logs: Vec::new(),
            };
            g.frames(1, &Input::default());
            g
        }

        pub fn frame(&mut self, input: &Input) {
            self.time.advance(1.0 / 60.0);
            self.rt.update(&mut self.scene, &self.time, input.clone());
            for (k, t) in self.rt.take_logs() {
                assert!(k != LogKind::Error, "{t}");
                self.logs.push(t);
            }
        }

        pub fn frames(&mut self, n: usize, input: &Input) {
            for _ in 0..n {
                self.frame(input);
            }
        }

        pub fn idle(&mut self, n: usize) {
            self.frames(n, &Input::default());
        }

        pub fn find(&self, name: &str) -> Entity {
            self.scene
                .find_by_name(name)
                .unwrap_or_else(|| panic!("no {name}"))
        }

        /// Put `e` at `p` (as the Move tool would), physics too.
        pub fn place(&mut self, e: Entity, p: Vec3) {
            self.scene.set_world_position(e, p);
            self.rt.with_physics_scene(&mut self.scene, |physics, scene| {
                let (_, rot, pos) = scene.world_matrix(e).to_scale_rotation_translation();
                physics.teleport(e, pos, rot);
            });
        }
    }

    pub fn keys(held: &[&str], pressed: &[&str]) -> Input {
        let mut i = Input::default();
        i.held.extend(held.iter().map(|k| (*k).to_owned()));
        i.pressed.extend(pressed.iter().map(|k| (*k).to_owned()));
        i
    }

    /// A press on one frame (held and pressed).
    pub fn press(key: &str) -> Input {
        keys(&[key], &[key])
    }
}

/// a) W walks forward, the mouse turns the body and tilts the camera (clamped), Space jumps
/// and gravity brings the player back down.
#[test]
fn guide_first_person_moves_looks_and_jumps() {
    use play::*;
    let mut g = Game::start(FIRST_PERSON_SCENE);
    let player = g.find("Player");
    let camera = g.find("Main Camera");
    g.idle(30);
    let ground = g.scene.world_position(player).y;
    assert!(ground.abs() < 0.2, "standing on the ground: {ground}");
    assert_eq!(
        g.rt.cursor().lock,
        misty_engine::scripting::CursorLockMode::Locked
    );
    // Walk forward one second.
    let z0 = g.scene.world_position(player).z;
    g.frames(60, &keys(&["w"], &[]));
    let walked = g.scene.world_position(player).z - z0;
    assert!(walked > 3.0 && walked < 5.5, "walked {walked} m in 1 s");
    // Mouse: 100 px right turns the body 20° (Mouse X 0.1 × sensitivity 2); 50 px up tilts
    // the camera up (negative pitch).
    let look = misty_engine::scripting::Input {
        mouse_delta: Vec2::new(100.0, -50.0),
        ..Default::default()
    };
    g.frame(&look);
    g.idle(2);
    let yaw = g.scene.transform(player).euler().y;
    assert!((yaw - 20.0).abs() < 1.0, "yaw {yaw}");
    let pitch = g.scene.transform(camera).euler().x;
    assert!((pitch + 10.0).abs() < 1.0, "pitch {pitch}");
    // Walking now follows the new facing: mostly +z, a little +x.
    let p0 = g.scene.world_position(player);
    g.frames(30, &keys(&["w"], &[]));
    let d = g.scene.world_position(player) - p0;
    assert!(d.x > 0.3 && d.z > 1.0, "walked along the facing: {d}");
    // Looking far up stops at the clamp.
    let far_up = misty_engine::scripting::Input {
        mouse_delta: Vec2::new(0.0, -2000.0),
        ..Default::default()
    };
    g.frame(&far_up);
    let pitch = g.scene.transform(camera).euler().x;
    assert!((pitch + 85.0).abs() < 1.0, "clamped pitch {pitch}");
    // Jump: up, then back down.
    let ground = g.scene.world_position(player).y;
    g.frame(&press("space"));
    let mut top = ground;
    for _ in 0..90 {
        g.idle(1);
        top = top.max(g.scene.world_position(player).y);
    }
    assert!(top > ground + 0.6, "jumped {}", top - ground);
    let landed = g.scene.world_position(player).y;
    assert!((landed - ground).abs() < 0.1, "landed at {landed}");
}

/// b) The key door opens with E only from inside its zone and closes with E again; the auto
/// door opens on entering and closes on leaving; both turn smoothly to 90° and back.
#[test]
fn guide_door_opens_and_closes() {
    use play::*;
    let mut g = Game::start(DOOR_SCENE);
    let player = g.find("Player");
    let key_hinge = g.find("Key Door Hinge");
    let auto_hinge = g.find("Auto Door Hinge");
    let angle = |g: &play::Game, e: Entity| g.scene.transform(e).euler().y;
    g.idle(10);
    // E outside the zone does nothing.
    g.frame(&press("e"));
    g.idle(30);
    assert!(angle(&g, key_hinge).abs() < 0.5);
    // Into the key door's zone: nothing yet (Auto Open is off), then E opens it smoothly.
    g.place(player, KEY_DOOR_AT + Vec3::new(1.0, 0.05, -1.5));
    g.idle(10);
    assert!(angle(&g, key_hinge).abs() < 0.5, "waits for E");
    g.frame(&press("e"));
    g.idle(15);
    let halfway = angle(&g, key_hinge);
    assert!(halfway > 20.0 && halfway < 85.0, "turning: {halfway}");
    g.idle(40);
    assert!((angle(&g, key_hinge) - 90.0).abs() < 0.5, "open");
    assert!(g.logs.iter().any(|l| l == "Door opened"), "{:?}", g.logs);
    // The panel turned with its hinge (it swung away from the doorway).
    let panel = g.find("Key Door");
    let p = g.scene.world_position(panel);
    assert!((p.x - KEY_DOOR_AT.x).abs() < 0.2, "panel swung: {p}");
    // E again closes it.
    g.frame(&press("e"));
    g.idle(60);
    assert!(angle(&g, key_hinge).abs() < 0.5, "closed");
    assert!(g.logs.iter().any(|l| l == "Door closed"), "{:?}", g.logs);
    // The auto door opens when the player walks in and closes when they leave.
    g.place(player, AUTO_DOOR_AT + Vec3::new(1.0, 0.05, -1.5));
    g.idle(60);
    assert!((angle(&g, auto_hinge) - 90.0).abs() < 0.5, "auto opened");
    g.place(player, Vec3::new(0.0, 0.05, -6.0));
    g.idle(60);
    assert!(angle(&g, auto_hinge).abs() < 0.5, "auto closed");
    assert!(angle(&g, key_hinge).abs() < 0.5, "the key door stayed shut");
}

/// c) F adds points, F5 saves; a new run (the prefs file opened again, the scene loaded
/// fresh) starts with the saved score and the player where it was saved.
#[test]
fn guide_saving_survives_a_save_and_load() {
    use play::*;
    let _lock = crate::test_util::PREFS_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let file = std::env::temp_dir().join(format!(
        "misty_guide_saving_prefs_{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&file);
    misty_engine::prefs::open(&file);
    let score_of = |g: &play::Game| {
        let t = g.find("Score Text");
        g.scene.world.get::<&UiText>(t).unwrap().text.clone()
    };
    // First run: nothing saved yet.
    let mut g = Game::start(SAVING_SCENE);
    g.idle(5);
    assert_eq!(score_of(&g), "Score: 0");
    assert!(!g.logs.iter().any(|l| l == "Loaded the saved game"));
    g.frame(&press("f"));
    g.idle(1);
    g.frame(&press("f"));
    g.idle(1);
    assert_eq!(score_of(&g), "Score: 20");
    let player = g.find("Player");
    let spot = Vec3::new(3.0, 0.05, 5.0);
    g.place(player, spot);
    g.idle(20);
    let saved_at = g.scene.world_position(player);
    g.frame(&press("f5"));
    assert!(g.logs.iter().any(|l| l == "Game saved"), "{:?}", g.logs);
    let text = std::fs::read_to_string(&file).expect("F5 wrote the prefs file");
    assert!(text.contains(SCORE_KEY), "{text}");
    drop(g);

    // Second run: the prefs file is read again, as when the game starts next time.
    misty_engine::prefs::open(&file);
    let mut g = Game::start(SAVING_SCENE);
    g.idle(3);
    assert_eq!(score_of(&g), "Score: 20");
    assert!(g.logs.iter().any(|l| l == "Loaded the saved game"));
    let player = g.find("Player");
    let back = g.scene.world_position(player);
    assert!(
        (back - saved_at).length() < 0.2,
        "loaded at {back}, saved at {saved_at}"
    );
    for key in [SCORE_KEY, X_KEY, Y_KEY, Z_KEY] {
        misty_engine::prefs::delete_key(key);
    }
    let _ = std::fs::remove_file(&file);
}

/// d) Standing still is Idle; W walks (Walk), Shift + W runs (Run), letting go goes back to
/// Idle through Walk, and Space plays the Jump.
#[test]
fn guide_animation_follows_the_speed() {
    use play::*;
    let mut g = Game::start(ANIMATION_SCENE);
    let ch = g.find("Character");
    let state = |g: &play::Game| {
        g.rt.animator_status(&g.scene, ch)
            .unwrap()
            .current
            .unwrap()
            .name
    };
    g.idle(20);
    assert_eq!(state(&g), "Idle");
    let z0 = g.scene.world_position(ch).z;
    g.frames(40, &keys(&["w"], &[]));
    assert_eq!(state(&g), "Walk");
    let walked = g.scene.world_position(ch).z - z0;
    assert!(walked > 1.0, "moved {walked}");
    g.frames(40, &keys(&["w", "left shift"], &[]));
    assert_eq!(state(&g), "Run");
    let speed = g.rt.animator_status(&g.scene, ch).unwrap().params[0].2;
    assert!(speed > 5.0, "Speed {speed}");
    g.idle(60);
    assert_eq!(state(&g), "Idle");
    g.frame(&press("space"));
    g.idle(10);
    assert_eq!(state(&g), "Jump");
    g.idle(60);
    assert_eq!(state(&g), "Idle");
}
