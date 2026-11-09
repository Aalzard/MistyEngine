//! Engine API nodes. An `Api` node names one member of the Lua API: settings `owner` (a global
//! table such as `Input`, `#Transform` for an object type, or empty for a global function),
//! `member`, `mode` (`get`, `set` or `call`) and `args` (the Lua parameter names). The editor
//! lists one node per member of the Lua binding table, so most of the Lua API is available as
//! nodes; this module gives the common members native implementations with typed pins
//! ([`NATIVE`]); the others call the Lua binding through [`super::lua_bridge`].
//!
//! Object members get a `Target` input (unconnected = the graph's own object).

use glam::Vec3;

use super::Node;
use super::natives::{Cx, D, NativeFn, Slot};
use super::nodes::{Pin, Ports, names};
use super::value::{PinType, Value};
use crate::camera::{euler_to_quat, quat_to_euler};
use crate::scripting::{LogKind, World};

/// What an `Api` node does with its member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Get,
    Set,
    Call,
}

impl Mode {
    pub fn parse(s: &str) -> Self {
        match s {
            "get" => Self::Get,
            "set" => Self::Set,
            _ => Self::Call,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Set => "set",
            Self::Call => "call",
        }
    }
}

/// A natively implemented API member.
pub struct NativeApi {
    pub owner: &'static str,
    pub member: &'static str,
    pub mode: Mode,
    /// Inputs after `Target` (object members).
    pub inputs: &'static [(&'static str, PinType, D)],
    pub outputs: &'static [(&'static str, PinType)],
    /// No exec pins (reads only).
    pub pure: bool,
    pub f: NativeFn,
    /// Not in the Lua API (graph-only members such as `transform.rotation`).
    pub graph_only: bool,
}

use PinType::{Bool, Float, GameObject, Int, Quaternion, String as Str, Vector3};

macro_rules! get {
    ($owner:literal, $member:literal, $ty:ident, $f:expr) => {
        NativeApi {
            owner: $owner,
            member: $member,
            mode: Mode::Get,
            inputs: &[],
            outputs: &[("Value", $ty)],
            pure: true,
            f: $f,
            graph_only: false,
        }
    };
}

macro_rules! set {
    ($owner:literal, $member:literal, $ty:ident, $d:expr, $f:expr) => {
        NativeApi {
            owner: $owner,
            member: $member,
            mode: Mode::Set,
            inputs: &[("Value", $ty, $d)],
            outputs: &[],
            pure: false,
            f: $f,
            graph_only: false,
        }
    };
}

macro_rules! call {
    ($owner:literal, $member:literal, $ins:expr, $outs:expr, $pure:expr, $f:expr) => {
        NativeApi {
            owner: $owner,
            member: $member,
            mode: Mode::Call,
            inputs: $ins,
            outputs: $outs,
            pure: $pure,
            f: $f,
            graph_only: false,
        }
    };
}

/// Read a transform value of the target (`i[0]`).
fn with_target<R>(
    cx: &Cx<'_>,
    i: &[Slot],
    f: impl FnOnce(&World, hecs::Entity) -> R,
) -> Result<R, String> {
    let e = cx.obj(i[0])?;
    Ok(f(cx.world, e))
}

fn position_get(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = with_target(cx, i, |w, e| w.scene.world_position(e))?;
    cx.set(o[0], Value::Vec3(v));
    Ok(())
}

fn position_set(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    let e = cx.obj(i[0])?;
    let v = cx.v3(i[1]);
    cx.world.scene.set_world_position(e, v);
    crate::scripting::api::sync_body(cx.world, e);
    Ok(())
}

fn local_position_get(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = with_target(cx, i, |w, e| w.scene.transform(e).position)?;
    cx.set(o[0], Value::Vec3(v));
    Ok(())
}

fn local_position_set(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    let e = cx.obj(i[0])?;
    let v = cx.v3(i[1]);
    let mut t = cx.world.scene.transform(e);
    t.position = v;
    cx.world.scene.set_transform(e, t);
    crate::scripting::api::sync_body(cx.world, e);
    Ok(())
}

fn rotation_get(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let q = with_target(cx, i, |w, e| w.scene.world_rotation(e))?;
    cx.set(o[0], Value::Quat(q));
    Ok(())
}

fn rotation_set(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    let e = cx.obj(i[0])?;
    let q = cx.q(i[1]);
    cx.world.scene.set_world_rotation(e, q.normalize());
    crate::scripting::api::sync_body(cx.world, e);
    Ok(())
}

fn euler_get(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = with_target(cx, i, |w, e| quat_to_euler(w.scene.world_rotation(e)))?;
    cx.set(o[0], Value::Vec3(v));
    Ok(())
}

fn euler_set(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    let e = cx.obj(i[0])?;
    let v = cx.v3(i[1]);
    cx.world.scene.set_world_rotation(e, euler_to_quat(v));
    crate::scripting::api::sync_body(cx.world, e);
    Ok(())
}

fn local_euler_get(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = with_target(cx, i, |w, e| w.scene.transform(e).euler())?;
    cx.set(o[0], Value::Vec3(v));
    Ok(())
}

fn local_euler_set(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    let e = cx.obj(i[0])?;
    let v = cx.v3(i[1]);
    let mut t = cx.world.scene.transform(e);
    t.set_euler(v);
    cx.world.scene.set_transform(e, t);
    crate::scripting::api::sync_body(cx.world, e);
    Ok(())
}

fn scale_get(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = with_target(cx, i, |w, e| w.scene.transform(e).scale)?;
    cx.set(o[0], Value::Vec3(v));
    Ok(())
}

fn scale_set(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    let e = cx.obj(i[0])?;
    let v = cx.v3(i[1]);
    let mut t = cx.world.scene.transform(e);
    t.scale = v;
    cx.world.scene.set_transform(e, t);
    Ok(())
}

fn axis_get(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot], axis: Vec3) -> Result<(), String> {
    let v = with_target(cx, i, |w, e| w.scene.world_rotation(e) * axis)?;
    cx.set(o[0], Value::Vec3(v));
    Ok(())
}

fn translate(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    // Like the Lua binding: along the object's own axes.
    let e = cx.obj(i[0])?;
    let d = cx.v3(i[1]);
    let w = &mut *cx.world;
    let p = w.scene.world_position(e) + w.scene.world_rotation(e) * d;
    w.scene.set_world_position(e, p);
    crate::scripting::api::sync_body(w, e);
    Ok(())
}

fn rotate(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    // Local axes, like `transform:Rotate(euler)`.
    let e = cx.obj(i[0])?;
    let d = cx.v3(i[1]);
    let w = &mut *cx.world;
    let q = w.scene.world_rotation(e) * euler_to_quat(d);
    w.scene.set_world_rotation(e, q.normalize());
    crate::scripting::api::sync_body(w, e);
    Ok(())
}

fn look_at(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    let e = cx.obj(i[0])?;
    let target = cx.v3(i[1]);
    let w = &mut *cx.world;
    let from = w.scene.world_position(e);
    let dir = target - from;
    if dir.length_squared() > 1e-10 {
        let q = super::natives::look_rotation(dir, Vec3::Y);
        w.scene.set_world_rotation(e, q);
        crate::scripting::api::sync_body(w, e);
    }
    Ok(())
}

fn go_name(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let n = with_target(cx, i, |w, e| w.scene.name(e))?;
    cx.set(o[0], Value::str(&n));
    Ok(())
}

fn go_tag(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let n = with_target(cx, i, |w, e| w.scene.game_object(e).tag.clone())?;
    cx.set(o[0], Value::str(&n));
    Ok(())
}

fn go_active(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let on = with_target(cx, i, |w, e| w.scene.game_object(e).active)?;
    cx.set(o[0], Value::Bool(on));
    Ok(())
}

fn go_set_active(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    let e = cx.obj(i[0])?;
    let on = cx.b(i[1]);
    cx.world.scene.game_object_mut(e).active = on;
    Ok(())
}

fn go_compare_tag(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let tag = cx.s(i[1]);
    let same = with_target(cx, i, |w, e| *w.scene.game_object(e).tag == *tag)?;
    cx.set(o[0], Value::Bool(same));
    Ok(())
}

fn destroy(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    let e = cx.obj(i[0])?;
    cx.world.destroy.push(e);
    Ok(())
}

fn find(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let name = cx.s(i[0]);
    let found = cx.world.scene.find_by_name(&name);
    cx.set(o[0], found.map_or(Value::Nil, Value::Object));
    Ok(())
}

fn log_with(cx: &mut Cx<'_>, i: &[Slot], kind: LogKind) -> Result<(), String> {
    let t = cx.text(i[0]);
    cx.world.logs.push((kind, t));
    Ok(())
}

fn velocity_get(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = with_target(cx, i, |w, e| w.physics.velocity(e).unwrap_or_default())?;
    cx.set(o[0], Value::Vec3(v));
    Ok(())
}

fn velocity_set(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    let e = cx.obj(i[0])?;
    let v = cx.v3(i[1]);
    cx.world.physics.set_velocity(e, v);
    Ok(())
}

fn add_force(cx: &mut Cx<'_>, i: &[Slot], _: &[Slot]) -> Result<(), String> {
    let e = cx.obj(i[0])?;
    let f = cx.v3(i[1]);
    let mode = cx.s(i[2]);
    let mode = if mode.is_empty() { "Force" } else { &mode };
    let w = &mut *cx.world;
    let dv = crate::scripting::api::delta_velocity(w, e, f, mode);
    let vel = w.physics.velocity(e).unwrap_or_default();
    w.physics.set_velocity(e, vel + dv);
    Ok(())
}

fn raycast(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let origin = cx.v3(i[0]);
    let dir = cx.v3(i[1]);
    let max = cx.f(i[2]);
    let max = if max <= 0.0 { f32::INFINITY } else { max };
    let hit = cx
        .world
        .physics
        .raycast(origin, dir, max, crate::physics::DEFAULT_RAYCAST_LAYERS);
    match hit {
        Some(h) => {
            cx.set(o[0], Value::Bool(true));
            cx.set(o[1], Value::Vec3(h.point));
            cx.set(o[2], Value::Vec3(h.normal));
            cx.set(o[3], Value::Float(h.distance));
            cx.set(o[4], Value::Object(h.entity));
        }
        None => {
            cx.set(o[0], Value::Bool(false));
            cx.set(o[1], Value::Vec3(Vec3::ZERO));
            cx.set(o[2], Value::Vec3(Vec3::ZERO));
            cx.set(o[3], Value::Float(0.0));
            cx.set(o[4], Value::Nil);
        }
    }
    Ok(())
}

fn axis_value(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let name = cx.s(i[0]);
    let v =
        cx.world.axes.axis(&name).ok_or_else(|| {
            format!("Input axis '{name}' is not set up (Project Settings ▸ Input)")
        })?;
    cx.set(o[0], Value::Float(v));
    Ok(())
}

const V0: D = D::V(0.0, 0.0, 0.0);

/// The natively implemented members.
pub static NATIVE: &[NativeApi] = &[
    // Transform
    get!("#Transform", "position", Vector3, position_get),
    set!("#Transform", "position", Vector3, V0, position_set),
    get!("#Transform", "localPosition", Vector3, local_position_get),
    set!(
        "#Transform",
        "localPosition",
        Vector3,
        V0,
        local_position_set
    ),
    get!("#Transform", "eulerAngles", Vector3, euler_get),
    set!("#Transform", "eulerAngles", Vector3, V0, euler_set),
    get!("#Transform", "localEulerAngles", Vector3, local_euler_get),
    set!(
        "#Transform",
        "localEulerAngles",
        Vector3,
        V0,
        local_euler_set
    ),
    get!("#Transform", "localScale", Vector3, scale_get),
    set!(
        "#Transform",
        "localScale",
        Vector3,
        D::V(1.0, 1.0, 1.0),
        scale_set
    ),
    get!("#Transform", "forward", Vector3, |cx, i, o| axis_get(
        cx,
        i,
        o,
        Vec3::Z
    )),
    get!("#Transform", "right", Vector3, |cx, i, o| axis_get(
        cx,
        i,
        o,
        Vec3::X
    )),
    get!("#Transform", "up", Vector3, |cx, i, o| axis_get(
        cx,
        i,
        o,
        Vec3::Y
    )),
    NativeApi {
        graph_only: true,
        ..get!("#Transform", "rotation", Quaternion, rotation_get)
    },
    NativeApi {
        graph_only: true,
        ..set!("#Transform", "rotation", Quaternion, D::N, rotation_set)
    },
    call!(
        "#Transform",
        "Translate",
        &[("Translation", Vector3, V0)],
        &[],
        false,
        translate
    ),
    call!(
        "#Transform",
        "Rotate",
        &[("Euler Degrees", Vector3, V0)],
        &[],
        false,
        rotate
    ),
    call!(
        "#Transform",
        "LookAt",
        &[("Point", Vector3, V0)],
        &[],
        false,
        look_at
    ),
    // GameObject
    get!("#GameObject", "name", Str, go_name),
    get!("#GameObject", "tag", Str, go_tag),
    get!("#GameObject", "activeSelf", Bool, go_active),
    call!(
        "#GameObject",
        "SetActive",
        &[("Value", Bool, D::B(true))],
        &[],
        false,
        go_set_active
    ),
    call!(
        "#GameObject",
        "CompareTag",
        &[("Tag", Str, D::S("Player"))],
        &[("Result", Bool)],
        true,
        go_compare_tag
    ),
    call!(
        "GameObject",
        "Find",
        &[("Name", Str, D::S(""))],
        &[("Result", GameObject)],
        true,
        find
    ),
    call!(
        "",
        "Destroy",
        &[("Object", GameObject, D::N)],
        &[],
        false,
        destroy
    ),
    // Rigidbody
    get!("#Rigidbody", "velocity", Vector3, velocity_get),
    set!("#Rigidbody", "velocity", Vector3, V0, velocity_set),
    call!(
        "#Rigidbody",
        "AddForce",
        &[("Force", Vector3, V0), ("Mode", Str, D::S("Force"))],
        &[],
        false,
        add_force
    ),
    // Physics
    call!(
        "Physics",
        "Raycast",
        &[
            ("Origin", Vector3, V0),
            ("Direction", Vector3, D::V(0.0, 0.0, 1.0)),
            ("Max Distance", Float, D::F(1000.0))
        ],
        &[
            ("Hit", Bool),
            ("Point", Vector3),
            ("Normal", Vector3),
            ("Distance", Float),
            ("Collider", GameObject)
        ],
        true,
        raycast
    ),
    // Time
    get!("Time", "deltaTime", Float, time_delta),
    get!("Time", "time", Float, time_time),
    get!("Time", "unscaledDeltaTime", Float, time_unscaled),
    get!("Time", "fixedDeltaTime", Float, time_fixed),
    get!("Time", "timeScale", Float, time_scale),
    get!("Time", "frameCount", Int, time_frame),
    // Input
    call!(
        "Input",
        "GetKey",
        &[("Key", Str, D::S("space"))],
        &[("Result", Bool)],
        true,
        |cx, i, o| {
            let k = cx.s(i[0]);
            let r = cx.world.input.key(&k);
            cx.set(o[0], Value::Bool(r));
            Ok(())
        }
    ),
    call!(
        "Input",
        "GetKeyDown",
        &[("Key", Str, D::S("space"))],
        &[("Result", Bool)],
        true,
        |cx, i, o| {
            let k = cx.s(i[0]);
            let r = cx.world.input.key_down(&k);
            cx.set(o[0], Value::Bool(r));
            Ok(())
        }
    ),
    call!(
        "Input",
        "GetKeyUp",
        &[("Key", Str, D::S("space"))],
        &[("Result", Bool)],
        true,
        |cx, i, o| {
            let k = cx.s(i[0]);
            let r = cx.world.input.key_up(&k);
            cx.set(o[0], Value::Bool(r));
            Ok(())
        }
    ),
    call!(
        "Input",
        "GetMouseButton",
        &[("Button", Int, D::I(0))],
        &[("Result", Bool)],
        true,
        |cx, i, o| {
            let b = cx.i(i[0]).max(0) as usize;
            let r = cx.world.input.mouse(b);
            cx.set(o[0], Value::Bool(r));
            Ok(())
        }
    ),
    call!(
        "Input",
        "GetMouseButtonDown",
        &[("Button", Int, D::I(0))],
        &[("Result", Bool)],
        true,
        |cx, i, o| {
            let b = cx.i(i[0]).max(0) as usize;
            let r = cx.world.input.mouse_down(b);
            cx.set(o[0], Value::Bool(r));
            Ok(())
        }
    ),
    call!(
        "Input",
        "GetAxis",
        &[("Axis Name", Str, D::S("Horizontal"))],
        &[("Result", Float)],
        true,
        axis_value
    ),
    // Debug
    call!(
        "Debug",
        "Log",
        &[("Message", Str, D::S("Hello"))],
        &[],
        false,
        |cx, i, _| log_with(cx, i, LogKind::Info)
    ),
    call!(
        "Debug",
        "LogWarning",
        &[("Message", Str, D::S(""))],
        &[],
        false,
        |cx, i, _| log_with(cx, i, LogKind::Warning)
    ),
    call!(
        "Debug",
        "LogError",
        &[("Message", Str, D::S(""))],
        &[],
        false,
        |cx, i, _| log_with(cx, i, LogKind::Error)
    ),
];

/// Time members read the frame clock.
fn time_value(w: &World, member: &str) -> Value {
    match member {
        "deltaTime" => Value::Float(w.time.delta),
        "time" => Value::Float(w.time.time as f32),
        "unscaledDeltaTime" => Value::Float(w.time.unscaled_delta),
        "fixedDeltaTime" => Value::Float(w.physics.fixed_dt),
        "timeScale" => Value::Float(w.time.time_scale),
        "frameCount" => Value::Int(w.time.frame as i64),
        _ => Value::Nil,
    }
}

fn time_delta(cx: &mut Cx<'_>, _: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = cx.world.time.delta;
    cx.set(o[0], Value::Float(v));
    Ok(())
}
fn time_time(cx: &mut Cx<'_>, _: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = cx.world.time.time as f32;
    cx.set(o[0], Value::Float(v));
    Ok(())
}
fn time_unscaled(cx: &mut Cx<'_>, _: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = time_value(cx.world, "unscaledDeltaTime");
    cx.set(o[0], v);
    Ok(())
}
fn time_fixed(cx: &mut Cx<'_>, _: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = time_value(cx.world, "fixedDeltaTime");
    cx.set(o[0], v);
    Ok(())
}
fn time_scale(cx: &mut Cx<'_>, _: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = time_value(cx.world, "timeScale");
    cx.set(o[0], v);
    Ok(())
}
fn time_frame(cx: &mut Cx<'_>, _: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = time_value(cx.world, "frameCount");
    cx.set(o[0], v);
    Ok(())
}

/// The native implementation of a member, if any.
pub fn native(owner: &str, member: &str, mode: Mode) -> Option<&'static NativeApi> {
    NATIVE
        .iter()
        .find(|n| n.owner == owner && n.member == member && n.mode == mode)
}

/// Lua calls that only read (listed as pure nodes without exec pins).
pub fn pure_call(owner: &str, member: &str) -> bool {
    matches!(owner, "Mathf" | "Vector3" | "Color" | "LayerMask")
        || ["Get", "Is", "Has", "Compare", "Contains", "Find"]
            .iter()
            .any(|p| member.starts_with(p))
}

/// Node settings of an `Api` node.
pub struct ApiRef<'a> {
    pub owner: &'a str,
    pub member: &'a str,
    pub mode: Mode,
}

pub fn api_ref(node: &Node) -> ApiRef<'_> {
    ApiRef {
        owner: node.param("owner"),
        member: node.param("member"),
        mode: Mode::parse(node.param("mode")),
    }
}

/// Does the node run without exec pins?
pub fn is_pure(node: &Node) -> bool {
    let a = api_ref(node);
    match native(a.owner, a.member, a.mode) {
        Some(n) => n.pure,
        None => match a.mode {
            Mode::Get => true,
            Mode::Set => false,
            Mode::Call => pure_call(a.owner, a.member),
        },
    }
}

pub fn ports(node: &Node) -> Ports {
    let a = api_ref(node);
    let mut p = Ports::default();
    if a.member.is_empty() {
        p.problem = Some("no API member chosen".into());
        return p;
    }
    let pure = is_pure(node);
    if !pure {
        p.inputs.push(Pin::exec("In"));
        p.outputs.push(Pin::exec("Out"));
    }
    if a.owner.starts_with('#') {
        p.inputs.push(Pin::data("Target", GameObject));
    }
    match native(a.owner, a.member, a.mode) {
        Some(n) => {
            p.inputs.extend(
                n.inputs
                    .iter()
                    .map(|(name, t, d)| Pin::with(name, *t, d.literal())),
            );
            p.outputs
                .extend(n.outputs.iter().map(|(name, t)| Pin::data(name, *t)));
        }
        None => match a.mode {
            Mode::Get => p.outputs.push(Pin::data("Value", PinType::Any)),
            Mode::Set => p.inputs.push(Pin::data("Value", PinType::Any)),
            Mode::Call => {
                for arg in names(node.param("args")) {
                    if arg == "..." {
                        continue;
                    }
                    p.inputs.push(Pin::data(&arg, PinType::Any));
                }
                p.outputs.push(Pin::data("Result", PinType::Any));
            }
        },
    }
    p
}

/// `Transform`, `Input`, or `Globals` for global functions.
pub fn owner_label(owner: &str) -> &str {
    match owner.trim_start_matches('#') {
        "" => "Globals",
        o => o,
    }
}

pub fn title(node: &Node) -> String {
    let a = api_ref(node);
    let owner = owner_label(a.owner);
    let member = a.member;
    match a.mode {
        Mode::Get => format!("{owner}: Get {member}"),
        Mode::Set => format!("{owner}: Set {member}"),
        Mode::Call if a.owner.is_empty() => member.to_owned(),
        Mode::Call => format!("{owner}: {member}"),
    }
}

pub fn category(node: &Node) -> String {
    format!("Engine/{}", owner_label(api_ref(node).owner))
}

/// Settings of an `Api` node for a member.
pub fn params(owner: &str, member: &str, mode: Mode, args: &[String]) -> Vec<(String, String)> {
    let mut out = vec![
        ("owner".to_owned(), owner.to_owned()),
        ("member".to_owned(), member.to_owned()),
        ("mode".to_owned(), mode.key().to_owned()),
    ];
    if !args.is_empty() {
        out.push(("args".to_owned(), args.join(", ")));
    }
    out
}
