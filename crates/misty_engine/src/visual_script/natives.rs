//! Native node functions: the context they run in and the table of simple nodes (math, logic,
//! comparisons, vectors, quaternions, random, strings, lists). A simple node is one function
//! from its input registers to its output registers; flow control, variables, events and engine
//! calls are compiled specially (see [`super::compile`] and [`super::api`]).

use std::rc::Rc;

use glam::{Quat, Vec3};
use hecs::Entity;

use super::value::{PinType, Value};
use crate::scripting::World;

/// Index of a register.
pub type Slot = u32;

/// A native node: reads its input registers, writes its output registers.
pub type NativeFn = fn(&mut Cx<'_>, &[Slot], &[Slot]) -> Result<(), String>;

/// What a native function can reach while a graph runs.
pub struct Cx<'a> {
    pub regs: &'a mut [Value],
    pub world: &'a mut World,
    /// The graph's GameObject.
    pub this: Entity,
    /// Random number state (xorshift).
    pub rng: &'a mut u64,
}

impl Cx<'_> {
    #[inline]
    pub fn get(&self, s: Slot) -> &Value {
        &self.regs[s as usize]
    }

    #[inline]
    pub fn set(&mut self, s: Slot, v: Value) {
        self.regs[s as usize] = v;
    }

    #[inline]
    pub fn f(&self, s: Slot) -> f32 {
        match &self.regs[s as usize] {
            Value::Float(f) => *f,
            v => v.as_float(),
        }
    }

    #[inline]
    pub fn i(&self, s: Slot) -> i64 {
        match &self.regs[s as usize] {
            Value::Int(i) => *i,
            v => v.as_int(),
        }
    }

    #[inline]
    pub fn b(&self, s: Slot) -> bool {
        match &self.regs[s as usize] {
            Value::Bool(b) => *b,
            v => v.as_bool(),
        }
    }

    #[inline]
    pub fn v3(&self, s: Slot) -> Vec3 {
        match &self.regs[s as usize] {
            Value::Vec3(v) => *v,
            v => v.as_vec3(),
        }
    }

    #[inline]
    pub fn q(&self, s: Slot) -> Quat {
        match &self.regs[s as usize] {
            Value::Quat(q) => *q,
            v => v.as_quat(),
        }
    }

    /// A string input (other values as text).
    pub fn s(&self, s: Slot) -> Rc<str> {
        match &self.regs[s as usize] {
            Value::Str(t) => t.clone(),
            v => Rc::from(v.to_text(Some(&self.world.scene))),
        }
    }

    /// A GameObject input that must name a live object.
    pub fn obj(&self, s: Slot) -> Result<Entity, String> {
        match &self.regs[s as usize] {
            Value::Object(e) if self.world.scene.contains(*e) => Ok(*e),
            Value::Object(_) => Err("the GameObject was destroyed".into()),
            Value::Nil => Err("no GameObject (the input is nil)".into()),
            v => Err(format!("expected a GameObject, got {}", v.type_name())),
        }
    }

    /// A value as text, objects by name.
    pub fn text(&self, s: Slot) -> String {
        self.regs[s as usize].to_text(Some(&self.world.scene))
    }

    /// Uniform in [0, 1).
    pub fn random(&mut self) -> f32 {
        let mut x = *self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *self.rng = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Default of a simple node's input (const-friendly [`super::Literal`]).
#[derive(Clone, Copy, Debug)]
pub enum D {
    N,
    B(bool),
    I(i64),
    F(f32),
    S(&'static str),
    V(f32, f32, f32),
}

impl D {
    pub fn literal(self) -> super::Literal {
        use super::Literal as L;
        match self {
            D::N => L::None,
            D::B(b) => L::Bool(b),
            D::I(i) => L::Int(i),
            D::F(f) => L::Float(f),
            D::S(s) => L::String(s.to_owned()),
            D::V(x, y, z) => L::Vector3([x, y, z]),
        }
    }
}

/// A node that is one native function.
pub struct Simple {
    /// Stable type name saved in graph files.
    pub kind: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub inputs: &'static [(&'static str, PinType, D)],
    pub outputs: &'static [(&'static str, PinType)],
    /// Has exec pins In → Out (an action) instead of being pure.
    pub exec: bool,
    pub f: NativeFn,
    /// Extra search words.
    pub keywords: &'static str,
}

/// `native!((a: f, b: f) -> Float { a + b })`: a function reading inputs with the [`Cx`]
/// getters in order and writing one output.
macro_rules! native {
    (($($arg:ident : $get:ident),*) -> $out:ident $body:block) => {
        |cx: &mut Cx<'_>, _i: &[Slot], o: &[Slot]| -> Result<(), String> {
            #[allow(unused_mut, unused_variables)]
            let mut n = 0usize;
            $(
                let $arg = cx.$get(_i[n]);
                #[allow(unused_assignments)]
                {
                    n += 1;
                }
            )*
            let r = $body;
            cx.set(o[0], Value::$out(r));
            Ok(())
        }
    };
}

use PinType::{Any, Bool, Float, GameObject, Int, List, Quaternion, String as Str, Vector3};

const AB_F: &[(&str, PinType, D)] = &[("A", Float, D::F(0.0)), ("B", Float, D::F(0.0))];
const AB_I: &[(&str, PinType, D)] = &[("A", Int, D::I(0)), ("B", Int, D::I(0))];
const AB_V: &[(&str, PinType, D)] = &[
    ("A", Vector3, D::V(0.0, 0.0, 0.0)),
    ("B", Vector3, D::V(0.0, 0.0, 0.0)),
];
const AB_B: &[(&str, PinType, D)] = &[("A", Bool, D::B(false)), ("B", Bool, D::B(false))];
const X_F: &[(&str, PinType, D)] = &[("Value", Float, D::F(0.0))];
const R_F: &[(&str, PinType)] = &[("Result", Float)];
const R_I: &[(&str, PinType)] = &[("Result", Int)];
const R_B: &[(&str, PinType)] = &[("Result", Bool)];
const R_V: &[(&str, PinType)] = &[("Result", Vector3)];
const R_Q: &[(&str, PinType)] = &[("Result", Quaternion)];

macro_rules! simple {
    ($kind:literal, $title:literal, $cat:literal, $ins:expr, $outs:expr, $f:expr) => {
        Simple {
            kind: $kind,
            title: $title,
            category: $cat,
            inputs: $ins,
            outputs: $outs,
            exec: false,
            f: $f,
            keywords: "",
        }
    };
    ($kind:literal, $title:literal, $cat:literal, $ins:expr, $outs:expr, $f:expr, $kw:literal) => {
        Simple {
            kind: $kind,
            title: $title,
            category: $cat,
            inputs: $ins,
            outputs: $outs,
            exec: false,
            f: $f,
            keywords: $kw,
        }
    };
}

fn break_vector3(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = cx.v3(i[0]);
    cx.set(o[0], Value::Float(v.x));
    cx.set(o[1], Value::Float(v.y));
    cx.set(o[2], Value::Float(v.z));
    Ok(())
}

fn select(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let v = if cx.b(i[0]) {
        cx.get(i[1]).clone()
    } else {
        cx.get(i[2]).clone()
    };
    cx.set(o[0], v);
    Ok(())
}

fn equal(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let r = cx.get(i[0]).equals(cx.get(i[1]));
    cx.set(o[0], Value::Bool(r));
    Ok(())
}

fn not_equal(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let r = !cx.get(i[0]).equals(cx.get(i[1]));
    cx.set(o[0], Value::Bool(r));
    Ok(())
}

fn to_string(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let t = cx.text(i[0]);
    cx.set(o[0], Value::str(&t));
    Ok(())
}

fn append(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let t = format!("{}{}", cx.text(i[0]), cx.text(i[1]));
    cx.set(o[0], Value::str(&t));
    Ok(())
}

fn list_length(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let n = match cx.get(i[0]) {
        Value::List(l) => l.len() as i64,
        Value::Nil => 0,
        v => return Err(format!("expected a List, got {}", v.type_name())),
    };
    cx.set(o[0], Value::Int(n));
    Ok(())
}

fn list_get(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let index = cx.i(i[1]);
    let item = match cx.get(i[0]) {
        Value::List(l) => usize::try_from(index)
            .ok()
            .and_then(|k| l.get(k).cloned())
            .ok_or_else(|| format!("index {index} is outside the list (length {})", l.len()))?,
        v => return Err(format!("expected a List, got {}", v.type_name())),
    };
    cx.set(o[0], item);
    Ok(())
}

fn random_float(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let (lo, hi) = (cx.f(i[0]), cx.f(i[1]));
    let r = lo + (hi - lo) * cx.random();
    cx.set(o[0], Value::Float(r));
    Ok(())
}

fn random_int(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let (lo, hi) = (cx.i(i[0]), cx.i(i[1]));
    let r = if hi > lo {
        lo + ((cx.random() as f64 * (hi - lo) as f64) as i64).min(hi - lo - 1)
    } else {
        lo
    };
    cx.set(o[0], Value::Int(r));
    Ok(())
}

fn is_valid(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let ok = matches!(cx.get(i[0]), Value::Object(e) if cx.world.scene.contains(*e));
    cx.set(o[0], Value::Bool(ok));
    Ok(())
}

fn divide_int(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let (a, b) = (cx.i(i[0]), cx.i(i[1]));
    if b == 0 {
        return Err("integer division by zero".into());
    }
    cx.set(o[0], Value::Int(a.wrapping_div(b)));
    Ok(())
}

fn modulo_int(cx: &mut Cx<'_>, i: &[Slot], o: &[Slot]) -> Result<(), String> {
    let (a, b) = (cx.i(i[0]), cx.i(i[1]));
    if b == 0 {
        return Err("integer modulo by zero".into());
    }
    cx.set(o[0], Value::Int(a.wrapping_rem(b)));
    Ok(())
}

fn move_towards(cur: f32, target: f32, max: f32) -> f32 {
    if (target - cur).abs() <= max {
        target
    } else {
        cur + (target - cur).signum() * max
    }
}

fn ping_pong(t: f32, len: f32) -> f32 {
    if len <= 0.0 {
        return 0.0;
    }
    let m = t.rem_euclid(len * 2.0);
    len - (m - len).abs()
}

fn inverse_lerp(a: f32, b: f32, v: f32) -> f32 {
    if (b - a).abs() < f32::EPSILON {
        0.0
    } else {
        ((v - a) / (b - a)).clamp(0.0, 1.0)
    }
}

/// The rotation whose +Z faces `forward` with +Y towards `up`.
pub fn look_rotation(forward: Vec3, up: Vec3) -> Quat {
    let f = forward.normalize_or_zero();
    if f == Vec3::ZERO {
        return Quat::IDENTITY;
    }
    let r = up.cross(f).normalize_or_zero();
    let r = if r == Vec3::ZERO { Vec3::X } else { r };
    let u = f.cross(r);
    Quat::from_mat3(&glam::Mat3::from_cols(r, u, f)).normalize()
}

/// Every simple node.
pub static SIMPLE: &[Simple] = &[
    // ---- Float
    simple!("AddFloat", "Add", "Math/Float", AB_F, R_F, native!((a: f, b: f) -> Float { a + b }), "plus sum +"),
    simple!("SubtractFloat", "Subtract", "Math/Float", AB_F, R_F, native!((a: f, b: f) -> Float { a - b }), "minus -"),
    simple!("MultiplyFloat", "Multiply", "Math/Float", AB_F, R_F, native!((a: f, b: f) -> Float { a * b }), "times *"),
    simple!("DivideFloat", "Divide", "Math/Float", AB_F, R_F, native!((a: f, b: f) -> Float { a / b }), "/"),
    simple!("ModuloFloat", "Modulo", "Math/Float", AB_F, R_F, native!((a: f, b: f) -> Float { if b == 0.0 { 0.0 } else { a.rem_euclid(b) } }), "remainder %"),
    simple!("NegateFloat", "Negate", "Math/Float", X_F, R_F, native!((a: f) -> Float { -a })),
    simple!("AbsFloat", "Abs", "Math/Float", X_F, R_F, native!((a: f) -> Float { a.abs() }), "absolute"),
    simple!("MinFloat", "Min", "Math/Float", AB_F, R_F, native!((a: f, b: f) -> Float { a.min(b) }), "minimum"),
    simple!("MaxFloat", "Max", "Math/Float", AB_F, R_F, native!((a: f, b: f) -> Float { a.max(b) }), "maximum"),
    simple!("ClampFloat", "Clamp", "Math/Float",
        &[("Value", Float, D::F(0.0)), ("Min", Float, D::F(0.0)), ("Max", Float, D::F(1.0))], R_F,
        native!((v: f, lo: f, hi: f) -> Float { v.clamp(lo.min(hi), hi.max(lo)) })),
    simple!("Clamp01", "Clamp 01", "Math/Float", X_F, R_F, native!((v: f) -> Float { v.clamp(0.0, 1.0) })),
    simple!("Lerp", "Lerp", "Math/Float",
        &[("A", Float, D::F(0.0)), ("B", Float, D::F(1.0)), ("T", Float, D::F(0.5))], R_F,
        native!((a: f, b: f, t: f) -> Float { a + (b - a) * t.clamp(0.0, 1.0) }), "interpolate mix"),
    simple!("InverseLerp", "Inverse Lerp", "Math/Float",
        &[("A", Float, D::F(0.0)), ("B", Float, D::F(1.0)), ("Value", Float, D::F(0.5))], R_F,
        native!((a: f, b: f, v: f) -> Float { inverse_lerp(a, b, v) })),
    simple!("Sin", "Sin", "Math/Float", &[("Radians", Float, D::F(0.0))], R_F, native!((a: f) -> Float { a.sin() }), "sine"),
    simple!("Cos", "Cos", "Math/Float", &[("Radians", Float, D::F(0.0))], R_F, native!((a: f) -> Float { a.cos() }), "cosine"),
    simple!("Tan", "Tan", "Math/Float", &[("Radians", Float, D::F(0.0))], R_F, native!((a: f) -> Float { a.tan() }), "tangent"),
    simple!("Atan2", "Atan2", "Math/Float", &[("Y", Float, D::F(0.0)), ("X", Float, D::F(1.0))], R_F, native!((y: f, x: f) -> Float { y.atan2(x) }), "angle"),
    simple!("Sqrt", "Square Root", "Math/Float", X_F, R_F, native!((a: f) -> Float { a.max(0.0).sqrt() }), "sqrt"),
    simple!("Pow", "Power", "Math/Float", &[("Base", Float, D::F(2.0)), ("Exponent", Float, D::F(2.0))], R_F, native!((a: f, b: f) -> Float { a.powf(b) }), "pow exponent"),
    simple!("Floor", "Floor", "Math/Float", X_F, R_F, native!((a: f) -> Float { a.floor() })),
    simple!("Ceil", "Ceil", "Math/Float", X_F, R_F, native!((a: f) -> Float { a.ceil() }), "ceiling"),
    simple!("Round", "Round", "Math/Float", X_F, R_F, native!((a: f) -> Float { a.round() })),
    simple!("Sign", "Sign", "Math/Float", X_F, R_F, native!((a: f) -> Float { if a == 0.0 { 0.0 } else { a.signum() } })),
    simple!("MoveTowards", "Move Towards", "Math/Float",
        &[("Current", Float, D::F(0.0)), ("Target", Float, D::F(1.0)), ("Max Delta", Float, D::F(0.1))], R_F,
        native!((c: f, t: f, m: f) -> Float { move_towards(c, t, m) })),
    simple!("PingPong", "Ping Pong", "Math/Float",
        &[("T", Float, D::F(0.0)), ("Length", Float, D::F(1.0))], R_F,
        native!((t: f, l: f) -> Float { ping_pong(t, l) }), "bounce back forth patrol"),
    simple!("DegToRad", "Degrees To Radians", "Math/Float", &[("Degrees", Float, D::F(0.0))], R_F, native!((a: f) -> Float { a.to_radians() })),
    simple!("Pi", "Pi", "Math/Float", &[], R_F, native!(() -> Float { std::f32::consts::PI }), "constant 3.14"),
    // ---- Int
    simple!("AddInt", "Add", "Math/Int", AB_I, R_I, native!((a: i, b: i) -> Int { a.wrapping_add(b) }), "plus sum +"),
    simple!("SubtractInt", "Subtract", "Math/Int", AB_I, R_I, native!((a: i, b: i) -> Int { a.wrapping_sub(b) }), "minus -"),
    simple!("MultiplyInt", "Multiply", "Math/Int", AB_I, R_I, native!((a: i, b: i) -> Int { a.wrapping_mul(b) }), "times *"),
    simple!("DivideInt", "Divide", "Math/Int", AB_I, R_I, divide_int, "/"),
    simple!("ModuloInt", "Modulo", "Math/Int", AB_I, R_I, modulo_int, "remainder %"),
    simple!("MinInt", "Min", "Math/Int", AB_I, R_I, native!((a: i, b: i) -> Int { a.min(b) })),
    simple!("MaxInt", "Max", "Math/Int", AB_I, R_I, native!((a: i, b: i) -> Int { a.max(b) })),
    simple!("AbsInt", "Abs", "Math/Int", &[("Value", Int, D::I(0))], R_I, native!((a: i) -> Int { a.wrapping_abs() })),
    simple!("ClampInt", "Clamp", "Math/Int",
        &[("Value", Int, D::I(0)), ("Min", Int, D::I(0)), ("Max", Int, D::I(10))], R_I,
        native!((v: i, lo: i, hi: i) -> Int { v.clamp(lo.min(hi), hi.max(lo)) })),
    simple!("IntToFloat", "To Float", "Math/Int", &[("Value", Int, D::I(0))], R_F, native!((a: i) -> Float { a as f32 }), "convert"),
    simple!("FloatToInt", "Truncate To Int", "Math/Float", X_F, R_I, native!((a: f) -> Int { a as i64 }), "convert int"),
    simple!("RoundToInt", "Round To Int", "Math/Float", X_F, R_I, native!((a: f) -> Int { a.round() as i64 }), "convert int"),
    // ---- Comparison
    simple!("Less", "Less", "Comparison", AB_F, R_B, native!((a: f, b: f) -> Bool { a < b }), "<"),
    simple!("LessOrEqual", "Less Or Equal", "Comparison", AB_F, R_B, native!((a: f, b: f) -> Bool { a <= b }), "<="),
    simple!("Greater", "Greater", "Comparison", AB_F, R_B, native!((a: f, b: f) -> Bool { a > b }), ">"),
    simple!("GreaterOrEqual", "Greater Or Equal", "Comparison", AB_F, R_B, native!((a: f, b: f) -> Bool { a >= b }), ">="),
    simple!("Equal", "Equal", "Comparison", &[("A", Any, D::N), ("B", Any, D::N)], R_B, equal, "== same"),
    simple!("NotEqual", "Not Equal", "Comparison", &[("A", Any, D::N), ("B", Any, D::N)], R_B, not_equal, "!= different"),
    simple!("ApproximatelyEqual", "Approximately Equal", "Comparison",
        &[("A", Float, D::F(0.0)), ("B", Float, D::F(0.0)), ("Tolerance", Float, D::F(0.0001))], R_B,
        native!((a: f, b: f, t: f) -> Bool { (a - b).abs() <= t }), "nearly"),
    // ---- Logic
    simple!("And", "And", "Logic", AB_B, R_B, native!((a: b, b: b) -> Bool { a && b }), "&&"),
    simple!("Or", "Or", "Logic", AB_B, R_B, native!((a: b, b: b) -> Bool { a || b }), "||"),
    simple!("Xor", "Xor", "Logic", AB_B, R_B, native!((a: b, b: b) -> Bool { a != b })),
    simple!("Not", "Not", "Logic", &[("Value", Bool, D::B(false))], R_B, native!((a: b) -> Bool { !a }), "! invert"),
    simple!("Select", "Select", "Logic",
        &[("Condition", Bool, D::B(true)), ("True", Any, D::N), ("False", Any, D::N)], &[("Result", Any)],
        select, "if ternary choose"),
    simple!("IsValid", "Is Valid", "Logic", &[("Object", GameObject, D::N)], R_B, is_valid, "null exists alive"),
    // ---- Vector3
    simple!("MakeVector3", "Make Vector3", "Math/Vector3",
        &[("X", Float, D::F(0.0)), ("Y", Float, D::F(0.0)), ("Z", Float, D::F(0.0))], &[("Vector", Vector3)],
        native!((x: f, y: f, z: f) -> Vec3 { Vec3::new(x, y, z) }), "new construct"),
    simple!("BreakVector3", "Break Vector3", "Math/Vector3",
        &[("Vector", Vector3, D::V(0.0, 0.0, 0.0))], &[("X", Float), ("Y", Float), ("Z", Float)],
        break_vector3, "split components xyz"),
    simple!("AddVector3", "Add", "Math/Vector3", AB_V, R_V, native!((a: v3, b: v3) -> Vec3 { a + b }), "plus +"),
    simple!("SubtractVector3", "Subtract", "Math/Vector3", AB_V, R_V, native!((a: v3, b: v3) -> Vec3 { a - b }), "minus -"),
    simple!("ScaleVector3", "Scale", "Math/Vector3",
        &[("Vector", Vector3, D::V(0.0, 0.0, 0.0)), ("Scale", Float, D::F(1.0))], R_V,
        native!((v: v3, s: f) -> Vec3 { v * s }), "multiply times *"),
    simple!("NegateVector3", "Negate", "Math/Vector3", &[("Vector", Vector3, D::V(0.0, 0.0, 0.0))], R_V, native!((v: v3) -> Vec3 { -v })),
    simple!("Dot", "Dot", "Math/Vector3", AB_V, R_F, native!((a: v3, b: v3) -> Float { a.dot(b) }), "dot product"),
    simple!("Cross", "Cross", "Math/Vector3", AB_V, R_V, native!((a: v3, b: v3) -> Vec3 { a.cross(b) }), "cross product"),
    simple!("Magnitude", "Length", "Math/Vector3", &[("Vector", Vector3, D::V(0.0, 0.0, 0.0))], R_F, native!((v: v3) -> Float { v.length() }), "magnitude"),
    simple!("Normalize", "Normalize", "Math/Vector3", &[("Vector", Vector3, D::V(0.0, 0.0, 1.0))], R_V, native!((v: v3) -> Vec3 { v.normalize_or_zero() }), "normalized unit direction"),
    simple!("Distance", "Distance", "Math/Vector3", AB_V, R_F, native!((a: v3, b: v3) -> Float { a.distance(b) })),
    simple!("LerpVector3", "Lerp", "Math/Vector3",
        &[("A", Vector3, D::V(0.0, 0.0, 0.0)), ("B", Vector3, D::V(1.0, 1.0, 1.0)), ("T", Float, D::F(0.5))], R_V,
        native!((a: v3, b: v3, t: f) -> Vec3 { a.lerp(b, t.clamp(0.0, 1.0)) }), "interpolate"),
    simple!("MoveTowardsVector3", "Move Towards", "Math/Vector3",
        &[("Current", Vector3, D::V(0.0, 0.0, 0.0)), ("Target", Vector3, D::V(0.0, 0.0, 0.0)), ("Max Delta", Float, D::F(0.1))], R_V,
        native!((c: v3, t: v3, m: f) -> Vec3 {
            let d = t - c;
            if d.length() <= m { t } else { c + d.normalize_or_zero() * m }
        })),
    // ---- Quaternion
    simple!("Euler", "Euler", "Math/Quaternion", &[("Degrees", Vector3, D::V(0.0, 0.0, 0.0))], R_Q,
        native!((e: v3) -> Quat { crate::camera::euler_to_quat(e) }), "from euler angles rotation"),
    simple!("ToEuler", "To Euler", "Math/Quaternion", &[("Rotation", Quaternion, D::N)], R_V,
        native!((q: q) -> Vec3 { crate::camera::quat_to_euler(q) }), "euler angles degrees"),
    simple!("MultiplyQuaternion", "Combine", "Math/Quaternion",
        &[("A", Quaternion, D::N), ("B", Quaternion, D::N)], R_Q, native!((a: q, b: q) -> Quat { (a * b).normalize() }), "multiply *"),
    simple!("RotateVector", "Rotate Vector", "Math/Quaternion",
        &[("Rotation", Quaternion, D::N), ("Vector", Vector3, D::V(0.0, 0.0, 1.0))], R_V,
        native!((q: q, v: v3) -> Vec3 { q * v }), "multiply direction"),
    simple!("LookRotation", "Look Rotation", "Math/Quaternion",
        &[("Forward", Vector3, D::V(0.0, 0.0, 1.0)), ("Up", Vector3, D::V(0.0, 1.0, 0.0))], R_Q,
        native!((f: v3, u: v3) -> Quat { look_rotation(f, u) }), "face direction"),
    simple!("Slerp", "Slerp", "Math/Quaternion",
        &[("A", Quaternion, D::N), ("B", Quaternion, D::N), ("T", Float, D::F(0.5))], R_Q,
        native!((a: q, b: q, t: f) -> Quat { a.slerp(b, t.clamp(0.0, 1.0)) }), "interpolate lerp"),
    simple!("AngleAxis", "Angle Axis", "Math/Quaternion",
        &[("Degrees", Float, D::F(0.0)), ("Axis", Vector3, D::V(0.0, 1.0, 0.0))], R_Q,
        native!((a: f, axis: v3) -> Quat { Quat::from_axis_angle(axis.normalize_or(Vec3::Y), a.to_radians()) })),
    simple!("InverseQuaternion", "Inverse", "Math/Quaternion", &[("Rotation", Quaternion, D::N)], R_Q, native!((q: q) -> Quat { q.inverse() })),
    // ---- Random
    simple!("RandomFloat", "Random Float", "Random",
        &[("Min", Float, D::F(0.0)), ("Max", Float, D::F(1.0))], R_F, random_float, "range value"),
    simple!("RandomInt", "Random Int", "Random",
        &[("Min", Int, D::I(0)), ("Max", Int, D::I(10))], R_I, random_int, "range integer"),
    // ---- String
    simple!("Append", "Append", "String",
        &[("A", Str, D::S("")), ("B", Str, D::S(""))], &[("Result", Str)], append, "concat join text +"),
    simple!("ToString", "To String", "String", &[("Value", Any, D::N)], &[("Result", Str)], to_string, "text format convert"),
    simple!("StringLength", "Length", "String", &[("Text", Str, D::S(""))], R_I,
        native!((t: s) -> Int { t.chars().count() as i64 })),
    // ---- List
    simple!("ListLength", "Count", "List", &[("List", List, D::N)], R_I, list_length, "length size"),
    simple!("ListGet", "Get Item", "List", &[("List", List, D::N), ("Index", Int, D::I(0))], &[("Item", Any)], list_get, "index element"),
];

/// One-input float functions run inline by the VM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum F1 {
    Neg,
    Abs,
    Sin,
    Cos,
    Tan,
    Floor,
    Ceil,
    Round,
    Clamp01,
}

impl F1 {
    #[inline(always)]
    pub fn eval(self, a: f32) -> f32 {
        match self {
            F1::Neg => -a,
            F1::Abs => a.abs(),
            F1::Sin => a.sin(),
            F1::Cos => a.cos(),
            F1::Tan => a.tan(),
            F1::Floor => a.floor(),
            F1::Ceil => a.ceil(),
            F1::Round => a.round(),
            F1::Clamp01 => a.clamp(0.0, 1.0),
        }
    }
}

/// Two-input float functions run inline by the VM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum F2 {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    Pow,
    Atan2,
}

impl F2 {
    #[inline(always)]
    pub fn eval(self, a: f32, b: f32) -> f32 {
        match self {
            F2::Add => a + b,
            F2::Sub => a - b,
            F2::Mul => a * b,
            F2::Div => a / b,
            F2::Min => a.min(b),
            F2::Max => a.max(b),
            F2::Pow => a.powf(b),
            F2::Atan2 => a.atan2(b),
        }
    }
}

/// Three-input float functions run inline by the VM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum F3 {
    Clamp,
    Lerp,
    InverseLerp,
    MoveTowards,
}

impl F3 {
    #[inline(always)]
    pub fn eval(self, a: f32, b: f32, c: f32) -> f32 {
        match self {
            F3::Clamp => a.clamp(b.min(c), c.max(b)),
            F3::Lerp => a + (b - a) * c.clamp(0.0, 1.0),
            F3::InverseLerp => inverse_lerp(a, b, c),
            F3::MoveTowards => move_towards(a, b, c),
        }
    }
}

/// A simple node the VM runs inline (same results as its [`SIMPLE`] function).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fast {
    F1(F1),
    F2(F2),
    F3(F3),
}

/// The inline form of the common float nodes.
pub fn fast(kind: &str) -> Option<Fast> {
    Some(match kind {
        "AddFloat" => Fast::F2(F2::Add),
        "SubtractFloat" => Fast::F2(F2::Sub),
        "MultiplyFloat" => Fast::F2(F2::Mul),
        "DivideFloat" => Fast::F2(F2::Div),
        "MinFloat" => Fast::F2(F2::Min),
        "MaxFloat" => Fast::F2(F2::Max),
        "Pow" => Fast::F2(F2::Pow),
        "Atan2" => Fast::F2(F2::Atan2),
        "NegateFloat" => Fast::F1(F1::Neg),
        "AbsFloat" => Fast::F1(F1::Abs),
        "Sin" => Fast::F1(F1::Sin),
        "Cos" => Fast::F1(F1::Cos),
        "Tan" => Fast::F1(F1::Tan),
        "Floor" => Fast::F1(F1::Floor),
        "Ceil" => Fast::F1(F1::Ceil),
        "Round" => Fast::F1(F1::Round),
        "Clamp01" => Fast::F1(F1::Clamp01),
        "ClampFloat" => Fast::F3(F3::Clamp),
        "Lerp" => Fast::F3(F3::Lerp),
        "InverseLerp" => Fast::F3(F3::InverseLerp),
        "MoveTowards" => Fast::F3(F3::MoveTowards),
        _ => return None,
    })
}

/// The simple node of a type name.
pub fn simple(kind: &str) -> Option<&'static Simple> {
    SIMPLE.iter().find(|s| s.kind == kind)
}
