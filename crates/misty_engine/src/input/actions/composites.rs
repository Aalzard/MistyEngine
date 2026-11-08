//! Composites: one value made from several part bindings. `2DVector` (up/down/left/right,
//! e.g. WASD), `1DAxis` (negative/positive), `ButtonWithOneModifier` (modifier + button, e.g.
//! Shift + B), `ButtonWithTwoModifiers` and `OneModifier` (modifier + any binding).

use glam::Vec2;

use super::value::{NamedParams, Value, parse_params};

/// Composite kinds the editor offers: (type name, editor label, parts, default parameters).
pub const KINDS: &[(&str, &str, &[&str], &str)] = &[
    (
        "2DVector",
        "Up/Down/Left/Right Composite",
        &["up", "down", "left", "right"],
        "mode=DigitalNormalized",
    ),
    (
        "1DAxis",
        "Positive/Negative Composite",
        &["negative", "positive"],
        "",
    ),
    (
        "ButtonWithOneModifier",
        "Binding With One Modifier",
        &["modifier", "button"],
        "",
    ),
    (
        "ButtonWithTwoModifiers",
        "Binding With Two Modifiers",
        &["modifier1", "modifier2", "button"],
        "",
    ),
    (
        "OneModifier",
        "One Modifier (any value)",
        &["modifier", "binding"],
        "",
    ),
];

/// Part names of a composite type.
pub fn parts_of(kind: &str) -> &'static [&'static str] {
    KINDS
        .iter()
        .find(|k| k.0.eq_ignore_ascii_case(kind))
        .map_or(&[], |k| k.2)
}

/// How a 2D Vector composite combines its parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorMode {
    /// Digital parts, diagonals normalised to length 1 (the default).
    DigitalNormalized,
    /// Digital parts, diagonals (1, 1).
    Digital,
    /// Analog part values as they are.
    Analog,
}

impl VectorMode {
    pub const ALL: [VectorMode; 3] = [Self::DigitalNormalized, Self::Digital, Self::Analog];

    pub fn name(self) -> &'static str {
        match self {
            Self::DigitalNormalized => "DigitalNormalized",
            Self::Digital => "Digital",
            Self::Analog => "Analog",
        }
    }

    /// Names or the numbers files may hold (0 normalised, 1 digital, 2 analog).
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "digital" | "1" => Self::Digital,
            "analog" | "2" => Self::Analog,
            _ => Self::DigitalNormalized,
        }
    }
}

/// Which side a 1D Axis takes when both parts are held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhichSideWins {
    Neither,
    Positive,
    Negative,
}

impl WhichSideWins {
    pub const ALL: [WhichSideWins; 3] = [Self::Neither, Self::Positive, Self::Negative];

    pub fn name(self) -> &'static str {
        match self {
            Self::Neither => "Neither",
            Self::Positive => "Positive",
            Self::Negative => "Negative",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "positive" | "1" => Self::Positive,
            "negative" | "2" => Self::Negative,
            _ => Self::Neither,
        }
    }
}

/// Press point for modifiers and digital parts.
pub const PRESS_POINT: f32 = 0.5;

/// Combine part values (by part name) into the composite's value. `part(name)` reads a part
/// (the default when it has no binding).
pub fn evaluate(kind: &str, parameters: &str, part: impl Fn(&str) -> Value) -> Value {
    let params = NamedParams {
        name: kind.into(),
        params: parse_params(parameters),
    };
    let f = |name: &str| part(name).as_float();
    let pressed = |name: &str| f(name) >= PRESS_POINT;
    match kind.to_ascii_lowercase().as_str() {
        "2dvector" | "dpad" => {
            let mode = VectorMode::parse(params.get("mode").unwrap_or(""));
            let (up, down, left, right) = (f("up"), f("down"), f("left"), f("right"));
            let v = match mode {
                VectorMode::Analog => Vec2::new(right - left, up - down),
                _ => {
                    let d = |a: f32| if a >= PRESS_POINT { 1.0 } else { 0.0 };
                    Vec2::new(d(right) - d(left), d(up) - d(down))
                }
            };
            Value::Vector2(if mode == VectorMode::DigitalNormalized {
                v.normalize_or_zero()
            } else {
                v
            })
        }
        "1daxis" | "axis" => {
            let min = params.num("minValue", -1.0);
            let max = params.num("maxValue", 1.0);
            let side = WhichSideWins::parse(params.get("whichSideWins").unwrap_or(""));
            let (neg, pos) = (f("negative"), f("positive"));
            let mid = (min + max) * 0.5;
            let positive = || mid + (max - mid) * pos.clamp(0.0, 1.0);
            let negative = || mid - (mid - min) * neg.clamp(0.0, 1.0);
            Value::Float(match (neg > 0.0, pos > 0.0) {
                (true, true) => match side {
                    WhichSideWins::Neither => mid,
                    WhichSideWins::Positive => positive(),
                    WhichSideWins::Negative => negative(),
                },
                (false, true) => positive(),
                (true, false) => negative(),
                (false, false) => mid,
            })
        }
        "buttonwithonemodifier" => Value::Float(if pressed("modifier") {
            f("button")
        } else {
            0.0
        }),
        "buttonwithtwomodifiers" => Value::Float(if pressed("modifier1") && pressed("modifier2") {
            f("button")
        } else {
            0.0
        }),
        "onemodifier" => {
            let v = part("binding");
            if pressed("modifier") {
                v
            } else {
                v.zero_like()
            }
        }
        _ => Value::Float(0.0),
    }
}
