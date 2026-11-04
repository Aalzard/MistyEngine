//! Animation clip asset (`.anim`, JSON): length, loop flag, sample rate, curves and events.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::curve::Keys;

/// An animatable value. Stored in files as e.g. `"Transform.position.x"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Property {
    /// Local position axis (0 = x, 1 = y, 2 = z).
    Position(u8),
    /// Local Euler angle in degrees.
    Rotation(u8),
    Scale(u8),
    LightIntensity,
    LightRange,
    /// sRGB colour channel (0 = r, 1 = g, 2 = b).
    LightColor(u8),
    /// GameObject active flag (on when the value is ≥ 0.5).
    Active,
}

const AXES: [&str; 3] = ["x", "y", "z"];
const CHANNELS: [&str; 3] = ["r", "g", "b"];

impl Property {
    /// Every property, in Add Property menu order.
    pub const ALL: [Property; 15] = [
        Self::Position(0),
        Self::Position(1),
        Self::Position(2),
        Self::Rotation(0),
        Self::Rotation(1),
        Self::Rotation(2),
        Self::Scale(0),
        Self::Scale(1),
        Self::Scale(2),
        Self::LightIntensity,
        Self::LightRange,
        Self::LightColor(0),
        Self::LightColor(1),
        Self::LightColor(2),
        Self::Active,
    ];

    /// Component the value lives on ("Transform", "Light", "GameObject").
    pub fn component(self) -> &'static str {
        match self {
            Self::Position(_) | Self::Rotation(_) | Self::Scale(_) => "Transform",
            Self::LightIntensity | Self::LightRange | Self::LightColor(_) => "Light",
            Self::Active => "GameObject",
        }
    }

    /// Group label in the Animation window ("Position", "Color", "Is Active").
    pub fn group(self) -> &'static str {
        match self {
            Self::Position(_) => "Position",
            Self::Rotation(_) => "Rotation",
            Self::Scale(_) => "Scale",
            Self::LightIntensity => "Intensity",
            Self::LightRange => "Range",
            Self::LightColor(_) => "Color",
            Self::Active => "Is Active",
        }
    }

    /// Short label: "Position.x", "Color.r", "Intensity".
    pub fn label(self) -> String {
        match self.channel() {
            Some(c) => format!("{}.{c}", self.group()),
            None => self.group().to_owned(),
        }
    }

    fn channel(self) -> Option<&'static str> {
        match self {
            Self::Position(i) | Self::Rotation(i) | Self::Scale(i) => AXES.get(i as usize).copied(),
            Self::LightColor(i) => CHANNELS.get(i as usize).copied(),
            _ => None,
        }
    }

    /// File name: "Transform.position.x".
    pub fn name(self) -> String {
        let field = match self {
            Self::Position(_) => "position",
            Self::Rotation(_) => "rotation",
            Self::Scale(_) => "scale",
            Self::LightIntensity => "intensity",
            Self::LightRange => "range",
            Self::LightColor(_) => "color",
            Self::Active => "active",
        };
        match self.channel() {
            Some(c) => format!("{}.{field}.{c}", self.component()),
            None => format!("{}.{field}", self.component()),
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// Properties shown as one row group (Position.x/y/z share "Position").
    pub fn siblings(self) -> Vec<Property> {
        Self::ALL
            .into_iter()
            .filter(|p| p.group() == self.group() && p.component() == self.component())
            .collect()
    }

    /// Steps between values instead of blending (booleans).
    pub fn is_discrete(self) -> bool {
        self == Self::Active
    }
}

impl Serialize for Property {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.name())
    }
}

impl<'de> Deserialize<'de> for Property {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        Self::from_name(&name)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown property '{name}'")))
    }
}

/// One animated value: which object (path below the animated object) and property, and its keys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    /// Child names joined by '/', relative to the object with the Animation; "" = that object.
    #[serde(default)]
    pub path: String,
    pub property: Property,
    pub keys: Keys,
}

impl Curve {
    pub fn new(path: &str, property: Property) -> Self {
        Self {
            path: path.to_owned(),
            property,
            keys: Keys::default(),
        }
    }

    /// Value at `time` (booleans step instead of blending).
    pub fn evaluate(&self, time: f32) -> f32 {
        let v = self.keys.evaluate(time);
        if self.property.is_discrete() {
            // Hold each key's state until the next key.
            let k = self.keys.as_slice();
            let i = k.partition_point(|x| x.time <= time);
            return k.get(i.saturating_sub(1)).map_or(v, |x| x.value);
        }
        v
    }
}

/// Calls a script function on the animated object when playback passes `time`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimationEvent {
    pub time: f32,
    pub function: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimationClip {
    /// Seconds; at least the time of the last key.
    pub length: f32,
    /// Loop Time: playback wraps instead of stopping (unless the Animation overrides it).
    #[serde(rename = "loop")]
    pub looping: bool,
    /// Frames per second used to snap keys in the editor.
    pub sample_rate: f32,
    pub curves: Vec<Curve>,
    pub events: Vec<AnimationEvent>,
}

impl Default for AnimationClip {
    fn default() -> Self {
        Self {
            length: 1.0,
            looping: true,
            sample_rate: 60.0,
            curves: Vec::new(),
            events: Vec::new(),
        }
    }
}

/// File extension of animation clips.
pub const EXTENSION: &str = "anim";

pub fn is_clip(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

impl AnimationClip {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut clip: Self = serde_json::from_str(&crate::asset_db::resolve_json(text))
            .map_err(|e| e.to_string())?;
        clip.sanitize();
        Ok(clip)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, crate::asset_db::embed_json(&self.to_json()))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Fix values a hand-edited file may get wrong (unsorted keys, zero sample rate).
    fn sanitize(&mut self) {
        if !(self.sample_rate.is_finite() && self.sample_rate > 0.0) {
            self.sample_rate = 60.0;
        }
        for c in &mut self.curves {
            c.keys = Keys::new(c.keys.as_slice().to_vec());
        }
        self.fit_length();
    }

    /// Grow `length` to cover the last key (never shrinks it).
    pub fn fit_length(&mut self) {
        let last = self
            .curves
            .iter()
            .map(|c| c.keys.last_time())
            .fold(0.0, f32::max);
        if !self.length.is_finite() || self.length < last {
            self.length = last;
        }
    }

    pub fn curve(&self, path: &str, property: Property) -> Option<&Curve> {
        self.curves
            .iter()
            .find(|c| c.path == path && c.property == property)
    }

    /// The curve for (path, property), created empty if missing. Returns its index.
    pub fn curve_index_or_add(&mut self, path: &str, property: Property) -> usize {
        if let Some(i) = self
            .curves
            .iter()
            .position(|c| c.path == path && c.property == property)
        {
            return i;
        }
        self.curves.push(Curve::new(path, property));
        self.curves.len() - 1
    }

    /// Set a key (creating the curve) and grow the clip if needed.
    pub fn set_key(&mut self, path: &str, property: Property, time: f32, value: f32) {
        let i = self.curve_index_or_add(path, property);
        self.curves[i].keys.set(time, value);
        self.fit_length();
    }

    /// Distinct key times over all curves, sorted.
    pub fn key_times(&self) -> Vec<f32> {
        let mut times: Vec<f32> = self
            .curves
            .iter()
            .flat_map(|c| c.keys.as_slice().iter().map(|k| k.time))
            .collect();
        times.sort_by(f32::total_cmp);
        times.dedup_by(|b, a| (*a - *b).abs() < super::curve::TIME_EPSILON);
        times
    }

    /// Snap a time to the nearest frame of the sample rate.
    pub fn snap(&self, time: f32) -> f32 {
        (time * self.sample_rate).round() / self.sample_rate
    }
}
