//! Pin types, the literal values saved in graph files and the values graphs compute with.

use std::rc::Rc;

use glam::{Quat, Vec3};
use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::camera::{euler_to_quat, quat_to_euler};
use crate::scene::ScriptValue;

/// Type of a pin. `Exec` pins carry the flow; the others carry values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PinType {
    Exec,
    /// Anything (Lua results, custom event arguments, list items).
    Any,
    Bool,
    Int,
    Float,
    String,
    Vector3,
    Quaternion,
    GameObject,
    List,
}

impl PinType {
    /// Types a variable can have (the Blackboard's menu).
    pub const VARIABLE_TYPES: [PinType; 9] = [
        PinType::Bool,
        PinType::Int,
        PinType::Float,
        PinType::String,
        PinType::Vector3,
        PinType::Quaternion,
        PinType::GameObject,
        PinType::List,
        PinType::Any,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Exec => "Exec",
            Self::Any => "Any",
            Self::Bool => "Bool",
            Self::Int => "Int",
            Self::Float => "Float",
            Self::String => "String",
            Self::Vector3 => "Vector3",
            Self::Quaternion => "Quaternion",
            Self::GameObject => "GameObject",
            Self::List => "List",
        }
    }

    /// The lowercase name used in pin lists (`Speed:float`).
    pub fn key(self) -> &'static str {
        match self {
            Self::Exec => "exec",
            Self::Any => "any",
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::String => "string",
            Self::Vector3 => "vector3",
            Self::Quaternion => "quaternion",
            Self::GameObject => "gameobject",
            Self::List => "list",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        Some(match s.as_str() {
            "exec" => Self::Exec,
            "any" => Self::Any,
            "bool" | "boolean" => Self::Bool,
            "int" | "integer" => Self::Int,
            "float" | "number" => Self::Float,
            "string" | "text" => Self::String,
            "vector3" | "vec3" => Self::Vector3,
            "quaternion" | "quat" => Self::Quaternion,
            "gameobject" | "object" => Self::GameObject,
            "list" => Self::List,
            _ => return None,
        })
    }

    pub fn is_exec(self) -> bool {
        self == Self::Exec
    }

    /// Can an output of this type be wired into an input of type `to`? Ints widen to floats,
    /// anything converts to a string, and `Any` meets every value type.
    pub fn connects_to(self, to: PinType) -> bool {
        match (self, to) {
            (a, b) if a == b => true,
            (Self::Exec, _) | (_, Self::Exec) => false,
            (_, Self::Any) | (Self::Any, _) => true,
            (Self::Int, Self::Float) => true,
            (_, Self::String) => true,
            _ => false,
        }
    }

    /// The value an output holds before its node first runs.
    pub fn default_value(self) -> Value {
        match self {
            Self::Bool => Value::Bool(false),
            Self::Int => Value::Int(0),
            Self::Float => Value::Float(0.0),
            Self::String => Value::Str(Rc::from("")),
            Self::Vector3 => Value::Vec3(Vec3::ZERO),
            Self::Quaternion => Value::Quat(Quat::IDENTITY),
            Self::List => Value::List(Rc::new(Vec::new())),
            Self::Exec | Self::Any | Self::GameObject => Value::Nil,
        }
    }

    /// The literal an unconnected input of this type starts with.
    pub fn default_literal(self) -> Literal {
        match self {
            Self::Bool => Literal::Bool(false),
            Self::Int => Literal::Int(0),
            Self::Float => Literal::Float(0.0),
            Self::String => Literal::String(String::new()),
            Self::Vector3 => Literal::Vector3([0.0; 3]),
            Self::Quaternion => Literal::Quaternion([0.0; 3]),
            _ => Literal::None,
        }
    }
}

/// A value typed into a graph file (unconnected inputs, variable defaults).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    /// Nothing (an unconnected GameObject input means the graph's own object).
    #[default]
    None,
    Bool(bool),
    Int(i64),
    Float(f32),
    String(String),
    Vector3([f32; 3]),
    /// Euler angles in degrees.
    Quaternion([f32; 3]),
}

impl Literal {
    pub fn to_value(&self) -> Value {
        match self {
            Self::None => Value::Nil,
            Self::Bool(b) => Value::Bool(*b),
            Self::Int(i) => Value::Int(*i),
            Self::Float(f) => Value::Float(*f),
            Self::String(s) => Value::Str(Rc::from(s.as_str())),
            Self::Vector3(v) => Value::Vec3(Vec3::from(*v)),
            Self::Quaternion(e) => Value::Quat(euler_to_quat(Vec3::from(*e))),
        }
    }

    /// This literal as a value of type `ty` (numbers widen or truncate, others keep or reset).
    pub fn coerce(&self, ty: PinType) -> Literal {
        match (ty, self) {
            (PinType::Float, Self::Int(i)) => Self::Float(*i as f32),
            (PinType::Int, Self::Float(f)) => Self::Int(*f as i64),
            (PinType::Float, Self::Float(_))
            | (PinType::Int, Self::Int(_))
            | (PinType::Bool, Self::Bool(_))
            | (PinType::String, Self::String(_))
            | (PinType::Vector3, Self::Vector3(_))
            | (PinType::Quaternion, Self::Quaternion(_)) => self.clone(),
            (PinType::Any, l) => l.clone(),
            (t, _) => t.default_literal(),
        }
    }

    /// As an Inspector field value (Object variables), when the type has one.
    pub fn to_script_value(&self, ty: PinType) -> Option<ScriptValue> {
        Some(match self.coerce(ty) {
            Self::Bool(b) => ScriptValue::Bool(b),
            Self::Int(i) => ScriptValue::Number(i as f64),
            Self::Float(f) => ScriptValue::Number(f64::from(f)),
            Self::String(s) => ScriptValue::Text(s),
            Self::Vector3(v) => ScriptValue::Vector3(v),
            _ => return None,
        })
    }

    /// Short text for node bodies and tooltips.
    pub fn display(&self) -> String {
        match self {
            Self::None => "None".into(),
            Self::Bool(b) => b.to_string(),
            Self::Int(i) => i.to_string(),
            Self::Float(f) => format_float(*f),
            Self::String(s) => format!("\"{s}\""),
            Self::Vector3(v) | Self::Quaternion(v) => format!(
                "({}, {}, {})",
                format_float(v[0]),
                format_float(v[1]),
                format_float(v[2])
            ),
        }
    }
}

fn format_float(f: f32) -> String {
    if f == f.trunc() && f.abs() < 1e7 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

/// A Lua value held by a graph (results of Lua calls the graph doesn't understand).
pub struct LuaRef(pub mlua::RegistryKey);

impl std::fmt::Debug for LuaRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<lua value>")
    }
}

/// A value while a graph runs.
#[derive(Clone, Debug, Default)]
pub enum Value {
    #[default]
    Nil,
    Bool(bool),
    Int(i64),
    Float(f32),
    Str(Rc<str>),
    Vec3(Vec3),
    Quat(Quat),
    Object(Entity),
    List(Rc<Vec<Value>>),
    Lua(Rc<LuaRef>),
}

impl Value {
    pub fn str(s: &str) -> Self {
        Self::Str(Rc::from(s))
    }

    /// Truthiness: false, nil, 0 and "" are false.
    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::Int(i) => *i != 0,
            Self::Float(f) => *f != 0.0,
            Self::Nil => false,
            Self::Str(s) => !s.is_empty(),
            _ => true,
        }
    }

    #[inline]
    pub fn as_float(&self) -> f32 {
        match self {
            Self::Float(f) => *f,
            Self::Int(i) => *i as f32,
            Self::Bool(b) => f32::from(u8::from(*b)),
            Self::Str(s) => s.trim().parse().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    pub fn as_int(&self) -> i64 {
        match self {
            Self::Int(i) => *i,
            Self::Float(f) => *f as i64,
            Self::Bool(b) => i64::from(*b),
            Self::Str(s) => s.trim().parse().unwrap_or(0),
            _ => 0,
        }
    }

    pub fn as_vec3(&self) -> Vec3 {
        match self {
            Self::Vec3(v) => *v,
            Self::Quat(q) => quat_to_euler(*q),
            Self::Float(f) => Vec3::splat(*f),
            Self::Int(i) => Vec3::splat(*i as f32),
            _ => Vec3::ZERO,
        }
    }

    pub fn as_quat(&self) -> Quat {
        match self {
            Self::Quat(q) => *q,
            Self::Vec3(e) => euler_to_quat(*e),
            _ => Quat::IDENTITY,
        }
    }

    pub fn as_object(&self) -> Option<Entity> {
        match self {
            Self::Object(e) => Some(*e),
            _ => None,
        }
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Self::Nil)
    }

    /// Type name for messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Nil => "nil",
            Self::Bool(_) => "Bool",
            Self::Int(_) => "Int",
            Self::Float(_) => "Float",
            Self::Str(_) => "String",
            Self::Vec3(_) => "Vector3",
            Self::Quat(_) => "Quaternion",
            Self::Object(_) => "GameObject",
            Self::List(_) => "List",
            Self::Lua(_) => "Lua value",
        }
    }

    /// This value converted for a pin of type `ty` (what an `Any` output becomes when it
    /// flows into a typed input).
    pub fn convert(&self, ty: PinType) -> Value {
        match ty {
            PinType::Bool => Value::Bool(self.as_bool()),
            PinType::Int => Value::Int(self.as_int()),
            PinType::Float => Value::Float(self.as_float()),
            PinType::Vector3 => Value::Vec3(self.as_vec3()),
            PinType::Quaternion => Value::Quat(self.as_quat()),
            PinType::String => match self {
                Value::Str(_) => self.clone(),
                other => Value::str(&other.to_text(None)),
            },
            _ => self.clone(),
        }
    }

    /// Values equal as the Equal node sees them (numbers compare across Int and Float).
    pub fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Self::Nil, Self::Nil) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Int(_) | Self::Float(_), Self::Int(_) | Self::Float(_)) => {
                self.as_float() == other.as_float()
            }
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::Vec3(a), Self::Vec3(b)) => a == b,
            (Self::Quat(a), Self::Quat(b)) => a == b,
            (Self::Object(a), Self::Object(b)) => a == b,
            (Self::List(a), Self::List(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.equals(y))
            }
            (Self::Lua(a), Self::Lua(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// Text for Debug Log, To String and the debugger (objects by name when a scene is given).
    pub fn to_text(&self, scene: Option<&crate::scene::Scene>) -> String {
        match self {
            Self::Nil => "nil".into(),
            Self::Bool(b) => b.to_string(),
            Self::Int(i) => i.to_string(),
            Self::Float(f) => {
                if f.is_finite() && *f == f.trunc() && f.abs() < 1e15 {
                    format!("{}", *f as i64)
                } else {
                    format!("{f}")
                }
            }
            Self::Str(s) => s.to_string(),
            Self::Vec3(v) => format!("({:.2}, {:.2}, {:.2})", v.x, v.y, v.z),
            Self::Quat(q) => {
                let e = quat_to_euler(*q);
                format!("Euler({:.1}, {:.1}, {:.1})", e.x, e.y, e.z)
            }
            Self::Object(e) => match scene {
                Some(s) if s.contains(*e) => s.name(*e),
                Some(_) => "null".into(),
                None => format!("GameObject({})", e.id()),
            },
            Self::List(l) => {
                let items: Vec<String> = l.iter().take(8).map(|v| v.to_text(scene)).collect();
                let more = if l.len() > 8 { ", …" } else { "" };
                format!("[{}{more}]", items.join(", "))
            }
            Self::Lua(_) => "<lua value>".into(),
        }
    }

    /// Back to a literal (Object variables written to a save, tests).
    pub fn to_literal(&self) -> Literal {
        match self {
            Self::Bool(b) => Literal::Bool(*b),
            Self::Int(i) => Literal::Int(*i),
            Self::Float(f) => Literal::Float(*f),
            Self::Str(s) => Literal::String(s.to_string()),
            Self::Vec3(v) => Literal::Vector3(v.to_array()),
            Self::Quat(q) => Literal::Quaternion(quat_to_euler(*q).to_array()),
            _ => Literal::None,
        }
    }

    /// From an Inspector field value.
    pub fn from_script_value(v: &ScriptValue, ty: PinType) -> Value {
        let raw = match v {
            ScriptValue::Bool(b) => Value::Bool(*b),
            ScriptValue::Number(n) => Value::Float(*n as f32),
            ScriptValue::Text(t) => Value::str(t),
            ScriptValue::Vector3(a) => Value::Vec3(Vec3::from(*a)),
        };
        raw.convert(ty)
    }
}
