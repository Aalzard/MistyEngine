//! Action values (a number or a 2D vector) and the `name(key=value,…)` lists that processors,
//! interactions and composite parameters are written in.

use glam::Vec2;

/// What a control, composite or action reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    /// Buttons (0..1) and axes (-1..1, or any range for deltas).
    Float(f32),
    Vector2(Vec2),
}

impl Default for Value {
    fn default() -> Self {
        Value::Float(0.0)
    }
}

impl Value {
    /// How far from its default the value is (|x| or the vector's length).
    pub fn magnitude(self) -> f32 {
        match self {
            Value::Float(v) => v.abs(),
            Value::Vector2(v) => v.length(),
        }
    }

    pub fn as_float(self) -> f32 {
        match self {
            Value::Float(v) => v,
            Value::Vector2(v) => v.length(),
        }
    }

    pub fn as_vec2(self) -> Vec2 {
        match self {
            Value::Float(v) => Vec2::new(v, 0.0),
            Value::Vector2(v) => v,
        }
    }

    /// The default (zero) value of the same kind.
    pub fn zero_like(self) -> Value {
        match self {
            Value::Float(_) => Value::Float(0.0),
            Value::Vector2(_) => Value::Vector2(Vec2::ZERO),
        }
    }

    pub fn is_vector(self) -> bool {
        matches!(self, Value::Vector2(_))
    }

    /// Converted to what an action's control type expects.
    pub fn convert(self, to: super::asset::ControlType) -> Value {
        use super::asset::ControlType;
        match to {
            ControlType::Any => self,
            ControlType::Button | ControlType::Axis => Value::Float(self.as_float()),
            ControlType::Vector2 => Value::Vector2(self.as_vec2()),
        }
    }
}

/// One `name(key=value,…)` entry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NamedParams {
    pub name: String,
    pub params: Vec<(String, String)>,
}

impl NamedParams {
    /// A number parameter (also `true`/`false` as 1/0).
    pub fn num(&self, key: &str, default: f32) -> f32 {
        self.get(key)
            .and_then(|v| match v.to_ascii_lowercase().as_str() {
                "true" => Some(1.0),
                "false" => Some(0.0),
                s => s.parse().ok(),
            })
            .unwrap_or(default)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    pub fn set(&mut self, key: &str, value: String) {
        match self
            .params
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
        {
            Some(p) => p.1 = value,
            None => self.params.push((key.into(), value)),
        }
    }

    pub fn remove(&mut self, key: &str) {
        self.params.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
    }

    /// Back to text (`hold(duration=0.5)`, or just `hold`).
    pub fn to_text(&self) -> String {
        if self.params.is_empty() {
            return self.name.clone();
        }
        let p: Vec<String> = self
            .params
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        format!("{}({})", self.name, p.join(","))
    }
}

/// Parse `a(x=1,y=2),b,c(z=true)`. Blank entries are skipped.
pub fn parse_list(text: &str) -> Vec<NamedParams> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let name_end = rest.find(['(', ',']).unwrap_or(rest.len());
        let name = rest[..name_end].trim().to_owned();
        rest = &rest[name_end..];
        let mut params = Vec::new();
        if let Some(inner) = rest.strip_prefix('(') {
            let close = inner.find(')').unwrap_or(inner.len());
            params = parse_params(&inner[..close]);
            rest = inner.get(close + 1..).unwrap_or("");
        }
        rest = rest.trim_start().trim_start_matches(',').trim_start();
        if !name.is_empty() {
            out.push(NamedParams { name, params });
        }
    }
    out
}

/// Parse `x=1,y=2` (composite parameters).
pub fn parse_params(text: &str) -> Vec<(String, String)> {
    text.split(',')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            let k = k.trim();
            (!k.is_empty()).then(|| (k.to_owned(), v.trim().to_owned()))
        })
        .collect()
}

/// Back to `a(x=1),b`.
pub fn list_to_text(list: &[NamedParams]) -> String {
    list.iter()
        .map(NamedParams::to_text)
        .collect::<Vec<_>>()
        .join(",")
}
