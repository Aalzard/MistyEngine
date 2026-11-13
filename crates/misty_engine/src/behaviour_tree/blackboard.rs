//! Blackboards: the typed memory a behaviour tree reads and writes.
//!
//! A tree asset declares its keys (name, type, default, shared). Every agent gets its own
//! [`Blackboard`] with one slot per key, filled with the defaults and the agent's overrides.
//! Keys marked **shared** live in a [`SharedBlackboards`] store instead: every agent of the same
//! share group (by default the tree's asset path) sees the same value, so one guard spotting the
//! player can alert the others.
//!
//! Slots are addressed by index (resolved when the tree compiles), so reading and writing during
//! a tick does no lookups and no allocation (string values aside). Each slot keeps a version
//! counter that goes up when its value changes: conditional decorators watch it to abort
//! branches only when something they depend on changed.

use std::collections::HashMap;

use glam::Vec3;
use hecs::Entity;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The type of a blackboard key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyType {
    #[default]
    Bool,
    Int,
    Float,
    Vector,
    Object,
    String,
}

impl KeyType {
    pub const ALL: [KeyType; 6] = [
        KeyType::Bool,
        KeyType::Int,
        KeyType::Float,
        KeyType::Vector,
        KeyType::Object,
        KeyType::String,
    ];

    pub fn label(self) -> &'static str {
        match self {
            KeyType::Bool => "Bool",
            KeyType::Int => "Int",
            KeyType::Float => "Float",
            KeyType::Vector => "Vector",
            KeyType::Object => "Object",
            KeyType::String => "String",
        }
    }

    /// The value a key of this type starts with when nothing else is given.
    pub fn zero(self) -> BbValue {
        match self {
            KeyType::Bool => BbValue::Bool(false),
            KeyType::Int => BbValue::Int(0),
            KeyType::Float => BbValue::Float(0.0),
            KeyType::Vector => BbValue::Vector(Vec3::ZERO),
            KeyType::Object => BbValue::Object(ObjRef(None)),
            KeyType::String => BbValue::String(String::new()),
        }
    }
}

/// An object reference held by a blackboard. Asset files can't name objects of a scene, so it is
/// always written as `null` (agents set objects through overrides, nodes and scripts).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjRef(pub Option<Entity>);

impl Serialize for ObjRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_unit()
    }
}

impl<'de> Deserialize<'de> for ObjRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let _ = serde::de::IgnoredAny::deserialize(d)?;
        Ok(ObjRef(None))
    }
}

/// A blackboard value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BbValue {
    Bool(bool),
    Int(i64),
    Float(f32),
    Vector(Vec3),
    Object(ObjRef),
    String(String),
}

impl Default for BbValue {
    fn default() -> Self {
        BbValue::Bool(false)
    }
}

impl BbValue {
    pub fn key_type(&self) -> KeyType {
        match self {
            BbValue::Bool(_) => KeyType::Bool,
            BbValue::Int(_) => KeyType::Int,
            BbValue::Float(_) => KeyType::Float,
            BbValue::Vector(_) => KeyType::Vector,
            BbValue::Object(_) => KeyType::Object,
            BbValue::String(_) => KeyType::String,
        }
    }

    /// "Set": true, non-zero, a live object, non-empty text.
    pub fn is_set(&self) -> bool {
        match self {
            BbValue::Bool(b) => *b,
            BbValue::Int(i) => *i != 0,
            BbValue::Float(f) => *f != 0.0,
            BbValue::Vector(v) => *v != Vec3::ZERO,
            BbValue::Object(o) => o.0.is_some(),
            BbValue::String(s) => !s.is_empty(),
        }
    }

    /// As a number (bools are 0/1), for comparisons and Animator parameters.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            BbValue::Bool(b) => Some(f64::from(u8::from(*b))),
            BbValue::Int(i) => Some(*i as f64),
            BbValue::Float(f) => Some(f64::from(*f)),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<Entity> {
        match self {
            BbValue::Object(o) => o.0,
            _ => None,
        }
    }

    pub fn as_vector(&self) -> Option<Vec3> {
        match self {
            BbValue::Vector(v) => Some(*v),
            _ => None,
        }
    }

    /// This value turned into type `ty` (numbers between each other, anything to its text);
    /// None when it can't be.
    pub fn convert(&self, ty: KeyType) -> Option<BbValue> {
        if self.key_type() == ty {
            return Some(self.clone());
        }
        Some(match (ty, self) {
            (KeyType::Bool, v) => BbValue::Bool(v.is_set()),
            (KeyType::Int, v) => BbValue::Int(v.as_f64()? as i64),
            (KeyType::Float, v) => BbValue::Float(v.as_f64()? as f32),
            (KeyType::String, v) => BbValue::String(v.to_string()),
            _ => return None,
        })
    }

    /// Replace `self` with `v` in place (reusing a string's buffer); true when it changed.
    pub fn assign(&mut self, v: &BbValue) -> bool {
        if self == v {
            return false;
        }
        match (&mut *self, v) {
            (BbValue::String(a), BbValue::String(b)) => {
                a.clear();
                a.push_str(b);
            }
            (a, b) => *a = b.clone(),
        }
        true
    }
}

impl std::fmt::Display for BbValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BbValue::Bool(b) => write!(f, "{b}"),
            BbValue::Int(i) => write!(f, "{i}"),
            BbValue::Float(x) => write!(f, "{x:.3}"),
            BbValue::Vector(v) => write!(f, "({:.2}, {:.2}, {:.2})", v.x, v.y, v.z),
            BbValue::Object(ObjRef(Some(e))) => write!(f, "object #{}", e.id()),
            BbValue::Object(ObjRef(None)) => write!(f, "none"),
            BbValue::String(s) => write!(f, "{s}"),
        }
    }
}

/// How a conditional compares a key with its value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompareOp {
    #[default]
    IsSet,
    IsNotSet,
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

impl CompareOp {
    pub const ALL: [CompareOp; 8] = [
        CompareOp::IsSet,
        CompareOp::IsNotSet,
        CompareOp::Equal,
        CompareOp::NotEqual,
        CompareOp::Less,
        CompareOp::LessOrEqual,
        CompareOp::Greater,
        CompareOp::GreaterOrEqual,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CompareOp::IsSet => "Is Set",
            CompareOp::IsNotSet => "Is Not Set",
            CompareOp::Equal => "==",
            CompareOp::NotEqual => "!=",
            CompareOp::Less => "<",
            CompareOp::LessOrEqual => "<=",
            CompareOp::Greater => ">",
            CompareOp::GreaterOrEqual => ">=",
        }
    }

    /// Whether the comparison uses the value at all.
    pub fn needs_value(self) -> bool {
        !matches!(self, CompareOp::IsSet | CompareOp::IsNotSet)
    }

    /// `key <op> value`. Numbers compare as numbers (an Int key with a Float value works),
    /// vectors and objects only by (in)equality, text by order.
    pub fn eval(self, key: &BbValue, value: &BbValue) -> bool {
        use std::cmp::Ordering;
        let ord: Option<Ordering> = match (key, value) {
            (BbValue::String(a), BbValue::String(b)) => Some(a.as_str().cmp(b.as_str())),
            (BbValue::Vector(a), BbValue::Vector(b)) => {
                if a.abs_diff_eq(*b, 1e-4) {
                    Some(Ordering::Equal)
                } else {
                    None
                }
            }
            (BbValue::Object(a), BbValue::Object(b)) => (a == b).then_some(Ordering::Equal),
            (a, b) => match (a.as_f64(), b.as_f64()) {
                (Some(x), Some(y)) => x.partial_cmp(&y),
                _ => None,
            },
        };
        match self {
            CompareOp::IsSet => key.is_set(),
            CompareOp::IsNotSet => !key.is_set(),
            CompareOp::Equal => ord == Some(Ordering::Equal),
            CompareOp::NotEqual => ord != Some(Ordering::Equal),
            CompareOp::Less => ord == Some(Ordering::Less),
            CompareOp::LessOrEqual => matches!(ord, Some(Ordering::Less | Ordering::Equal)),
            CompareOp::Greater => ord == Some(Ordering::Greater),
            CompareOp::GreaterOrEqual => {
                matches!(ord, Some(Ordering::Greater | Ordering::Equal))
            }
        }
    }
}

/// A key declared by a tree asset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyDef {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: KeyType,
    /// Starting value (of type `ty`).
    pub default: BbValue,
    /// Lives in the share group's blackboard: every agent of the group sees one value.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
    /// Shown in the editor only.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl Default for KeyDef {
    fn default() -> Self {
        Self {
            name: String::new(),
            ty: KeyType::Bool,
            default: BbValue::Bool(false),
            shared: false,
            description: String::new(),
        }
    }
}

impl KeyDef {
    pub fn new(name: &str, ty: KeyType) -> Self {
        Self {
            name: name.to_owned(),
            ty,
            default: ty.zero(),
            ..Default::default()
        }
    }

    /// The default, turned into the key's type when the file disagrees.
    pub fn initial(&self) -> BbValue {
        self.default.convert(self.ty).unwrap_or_else(|| self.ty.zero())
    }
}

/// Where a slot's value lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Home {
    /// Index into the agent's own values.
    Local(u16),
    /// Index into the share group's values.
    Shared(u16),
}

/// One share group's values.
#[derive(Clone, Debug, Default)]
pub struct SharedGroup {
    pub names: Vec<String>,
    pub values: Vec<BbValue>,
    pub versions: Vec<u32>,
}

impl SharedGroup {
    fn slot(&mut self, def: &KeyDef) -> u16 {
        if let Some(i) = self.names.iter().position(|n| *n == def.name) {
            // A key of another type under the same name: the first declaration wins, but the
            // value is converted so reads keep their type.
            return i as u16;
        }
        self.names.push(def.name.clone());
        self.values.push(def.initial());
        self.versions.push(0);
        (self.names.len() - 1) as u16
    }
}

/// Blackboards shared between agents, by share group name.
#[derive(Clone, Debug, Default)]
pub struct SharedBlackboards {
    pub groups: HashMap<String, SharedGroup>,
}

impl SharedBlackboards {
    pub fn group(&self, name: &str) -> Option<&SharedGroup> {
        self.groups.get(name)
    }

    pub fn group_mut(&mut self, name: &str) -> &mut SharedGroup {
        self.groups.entry(name.to_owned()).or_default()
    }
}

/// One agent's blackboard: its key list, its own values and the share group it reads shared
/// keys from.
#[derive(Clone, Debug, Default)]
pub struct Blackboard {
    pub names: Vec<String>,
    pub types: Vec<KeyType>,
    homes: Vec<Home>,
    values: Vec<BbValue>,
    versions: Vec<u32>,
    /// Share group (empty = no shared keys).
    pub group: String,
}

impl Blackboard {
    /// A blackboard for `keys`; shared keys get slots in `shared`'s group `group`.
    pub fn new(keys: &[KeyDef], group: &str, shared: &mut SharedBlackboards) -> Self {
        let mut bb = Blackboard {
            group: group.to_owned(),
            ..Default::default()
        };
        for k in keys {
            bb.names.push(k.name.clone());
            bb.types.push(k.ty);
            if k.shared {
                let slot = shared.group_mut(group).slot(k);
                bb.homes.push(Home::Shared(slot));
            } else {
                bb.homes.push(Home::Local(bb.values.len() as u16));
                bb.values.push(k.initial());
                bb.versions.push(0);
            }
        }
        bb
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Slot of the key called `name`.
    pub fn index(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }

    pub fn is_shared(&self, slot: usize) -> bool {
        matches!(self.homes.get(slot), Some(Home::Shared(_)))
    }

    /// The value in `slot` (shared keys read from `shared`).
    pub fn get<'a>(&'a self, shared: &'a SharedBlackboards, slot: usize) -> Option<&'a BbValue> {
        match *self.homes.get(slot)? {
            Home::Local(i) => self.values.get(i as usize),
            Home::Shared(i) => shared.group(&self.group)?.values.get(i as usize),
        }
    }

    /// Version of `slot` (goes up on every change).
    pub fn version(&self, shared: &SharedBlackboards, slot: usize) -> u32 {
        match self.homes.get(slot) {
            Some(Home::Local(i)) => self.versions.get(*i as usize).copied().unwrap_or(0),
            Some(Home::Shared(i)) => shared
                .group(&self.group)
                .and_then(|g| g.versions.get(*i as usize).copied())
                .unwrap_or(0),
            None => 0,
        }
    }

    /// Write `v` (converted to the key's type) into `slot`; Err when the types don't fit.
    pub fn set(
        &mut self,
        shared: &mut SharedBlackboards,
        slot: usize,
        v: &BbValue,
    ) -> Result<bool, String> {
        let Some(&ty) = self.types.get(slot) else {
            return Err(format!("no blackboard key #{slot}"));
        };
        let converted;
        let v = if v.key_type() == ty {
            v
        } else {
            converted = v.convert(ty).ok_or_else(|| {
                format!(
                    "blackboard key '{}' is {}, not {}",
                    self.names[slot],
                    ty.label(),
                    v.key_type().label()
                )
            })?;
            &converted
        };
        let (value, version) = match self.homes[slot] {
            Home::Local(i) => (
                &mut self.values[i as usize],
                &mut self.versions[i as usize],
            ),
            Home::Shared(i) => {
                let g = shared
                    .groups
                    .get_mut(&self.group)
                    .ok_or("the shared blackboard is gone")?;
                (&mut g.values[i as usize], &mut g.versions[i as usize])
            }
        };
        let changed = value.assign(v);
        if changed {
            *version = version.wrapping_add(1);
        }
        Ok(changed)
    }

    /// Write by key name.
    pub fn set_named(
        &mut self,
        shared: &mut SharedBlackboards,
        name: &str,
        v: &BbValue,
    ) -> Result<bool, String> {
        let slot = self
            .index(name)
            .ok_or_else(|| format!("no blackboard key '{name}'"))?;
        self.set(shared, slot, v)
    }

    /// Read by key name.
    pub fn get_named<'a>(
        &'a self,
        shared: &'a SharedBlackboards,
        name: &str,
    ) -> Option<&'a BbValue> {
        self.get(shared, self.index(name)?)
    }

    /// Every key as (name, type, value text, shared), for the editor's live view.
    pub fn describe(&self, shared: &SharedBlackboards) -> Vec<(String, KeyType, String, bool)> {
        (0..self.len())
            .map(|i| {
                (
                    self.names[i].clone(),
                    self.types[i],
                    self.get(shared, i).map(|v| v.to_string()).unwrap_or_default(),
                    self.is_shared(i),
                )
            })
            .collect()
    }

    /// Object references to `gone` become empty (destroyed objects).
    pub fn forget_object(&mut self, gone: Entity) {
        for (i, v) in self.values.iter_mut().enumerate() {
            if v.as_object() == Some(gone) {
                *v = BbValue::Object(ObjRef(None));
                self.versions[i] = self.versions[i].wrapping_add(1);
            }
        }
    }
}
