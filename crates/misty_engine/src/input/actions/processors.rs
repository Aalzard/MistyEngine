//! Processors: value filters on a binding or an action (`invert`, `scale(factor=2)`,
//! `normalize(min=0,max=1)`, `axisDeadzone`, `stickDeadzone`, `clamp`, and their Vector 2
//! forms), applied left to right.

use glam::Vec2;

use super::value::{NamedParams, Value, parse_list};

/// Default dead zone bounds (`axisDeadzone`, `stickDeadzone`).
pub const DEADZONE_MIN: f32 = 0.125;
pub const DEADZONE_MAX: f32 = 0.925;

/// Processors the editor offers: (name, default parameters, applies to vectors).
pub const KINDS: &[(&str, &str, bool)] = &[
    ("invert", "", false),
    ("invertVector2", "invertX=true,invertY=true", true),
    ("normalize", "min=0,max=1,zero=0", false),
    ("normalizeVector2", "", true),
    ("scale", "factor=1", false),
    ("scaleVector2", "x=1,y=1", true),
    ("clamp", "min=0,max=1", false),
    ("axisDeadzone", "min=0.125,max=0.925", false),
    ("stickDeadzone", "min=0.125,max=0.925", true),
];

/// Apply a processor list (`"invert,scale(factor=2)"`).
pub fn apply(text: &str, value: Value) -> Value {
    if text.trim().is_empty() {
        return value;
    }
    parse_list(text).iter().fold(value, |v, p| apply_one(p, v))
}

/// Remap |v| from [min, max] to [0, 1] keeping the sign (0 below min, 1 above max).
fn deadzone(v: f32, min: f32, max: f32) -> f32 {
    let a = v.abs();
    if a < min {
        0.0
    } else if a > max || max <= min {
        v.signum()
    } else {
        v.signum() * (a - min) / (max - min)
    }
}

pub fn apply_one(p: &NamedParams, value: Value) -> Value {
    let name = p.name.to_ascii_lowercase();
    match (name.as_str(), value) {
        ("invert", Value::Float(v)) => Value::Float(-v),
        ("invert", Value::Vector2(v)) => Value::Vector2(-v),
        ("invertvector2", v) => {
            let mut v = v.as_vec2();
            if p.num("invertX", 1.0) != 0.0 {
                v.x = -v.x;
            }
            if p.num("invertY", 1.0) != 0.0 {
                v.y = -v.y;
            }
            Value::Vector2(v)
        }
        ("normalize", Value::Float(v)) => {
            let (min, max) = (p.num("min", 0.0), p.num("max", 1.0));
            let zero = p.num("zero", min);
            if max <= min {
                return Value::Float(0.0);
            }
            if zero > min {
                // Two-sided: [min, zero] → [-1, 0], [zero, max] → [0, 1].
                let n = if v >= zero {
                    (v - zero) / (max - zero).max(f32::EPSILON)
                } else {
                    -(zero - v) / (zero - min).max(f32::EPSILON)
                };
                Value::Float(n.clamp(-1.0, 1.0))
            } else {
                Value::Float(((v - min) / (max - min)).clamp(0.0, 1.0))
            }
        }
        ("normalizevector2", v) => Value::Vector2(v.as_vec2().normalize_or_zero()),
        ("scale", Value::Float(v)) => Value::Float(v * p.num("factor", 1.0)),
        ("scale", Value::Vector2(v)) => Value::Vector2(v * p.num("factor", 1.0)),
        ("scalevector2", v) => {
            let v = v.as_vec2();
            Value::Vector2(Vec2::new(v.x * p.num("x", 1.0), v.y * p.num("y", 1.0)))
        }
        ("clamp", Value::Float(v)) => {
            let (min, max) = (p.num("min", 0.0), p.num("max", 1.0));
            Value::Float(v.clamp(min.min(max), max.max(min)))
        }
        ("clamp", Value::Vector2(v)) => {
            let (min, max) = (p.num("min", 0.0), p.num("max", 1.0));
            Value::Vector2(v.clamp(Vec2::splat(min.min(max)), Vec2::splat(max.max(min))))
        }
        ("axisdeadzone", Value::Float(v)) => Value::Float(deadzone(
            v,
            p.num("min", DEADZONE_MIN),
            p.num("max", DEADZONE_MAX),
        )),
        ("axisdeadzone", Value::Vector2(v)) => {
            let (min, max) = (p.num("min", DEADZONE_MIN), p.num("max", DEADZONE_MAX));
            Value::Vector2(Vec2::new(deadzone(v.x, min, max), deadzone(v.y, min, max)))
        }
        ("stickdeadzone", v) => {
            let v = v.as_vec2();
            let len = v.length();
            let (min, max) = (p.num("min", DEADZONE_MIN), p.num("max", DEADZONE_MAX));
            let scaled = deadzone(len, min, max);
            Value::Vector2(if len > 0.0 {
                v / len * scaled
            } else {
                Vec2::ZERO
            })
        }
        // Unknown processors (or a vector processor on a number) leave the value alone.
        _ => value,
    }
}
