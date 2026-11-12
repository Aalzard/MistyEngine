//! Keyframes and curve evaluation: cubic Hermite segments with Auto, Linear or Constant tangents.

use serde::{Deserialize, Serialize};

/// How a key's tangents are chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TangentMode {
    /// Smooth (Catmull-Rom slope), flattened at extremes and end keys so curves don't overshoot.
    #[default]
    Auto,
    /// Straight lines to the neighbouring keys.
    Linear,
    /// Hold this key's value until the next key (a step).
    Constant,
}

impl TangentMode {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Linear, Self::Constant];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Linear => "Linear",
            Self::Constant => "Constant",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    /// Seconds from the clip start.
    pub time: f32,
    pub value: f32,
    #[serde(default, skip_serializing_if = "is_auto")]
    pub tangent: TangentMode,
}

fn is_auto(m: &TangentMode) -> bool {
    *m == TangentMode::Auto
}

impl Keyframe {
    pub fn new(time: f32, value: f32) -> Self {
        Self {
            time,
            value,
            tangent: TangentMode::Auto,
        }
    }
}

/// Keys sorted by time. Two keys never share a time (inserting at an existing time replaces it).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Keys(Vec<Keyframe>);

/// Keys closer than this (seconds) are at the same time.
pub const TIME_EPSILON: f32 = 1e-4;

impl Keys {
    pub fn new(mut keys: Vec<Keyframe>) -> Self {
        keys.sort_by(|a, b| a.time.total_cmp(&b.time));
        keys.dedup_by(|b, a| (a.time - b.time).abs() < TIME_EPSILON);
        Self(keys)
    }

    pub fn as_slice(&self) -> &[Keyframe] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&Keyframe> {
        self.0.get(i)
    }

    /// Index of the key at `time`, if any.
    pub fn find(&self, time: f32) -> Option<usize> {
        self.0
            .iter()
            .position(|k| (k.time - time).abs() < TIME_EPSILON)
    }

    /// Add a key, replacing the value of one at the same time (its tangent mode is kept).
    /// Returns the key's index.
    pub fn set(&mut self, time: f32, value: f32) -> usize {
        // Appending past the last key (importers resampling a clip) needs no search.
        if self.0.last().is_some_and(|k| time - k.time >= TIME_EPSILON) {
            self.0.push(Keyframe::new(time, value));
            return self.0.len() - 1;
        }
        if let Some(i) = self.find(time) {
            self.0[i].value = value;
            return i;
        }
        let i = self.0.partition_point(|k| k.time < time);
        self.0.insert(i, Keyframe::new(time, value));
        i
    }

    pub fn remove(&mut self, i: usize) -> Option<Keyframe> {
        (i < self.0.len()).then(|| self.0.remove(i))
    }

    pub fn set_tangent(&mut self, i: usize, mode: TangentMode) {
        if let Some(k) = self.0.get_mut(i) {
            k.tangent = mode;
        }
    }

    /// Move key `i` to a new time/value, replacing any key already there. Returns its new index.
    pub fn move_key(&mut self, i: usize, time: f32, value: f32) -> usize {
        let Some(mut k) = self.remove(i) else {
            return i;
        };
        if let Some(j) = self.find(time) {
            self.0.remove(j);
        }
        k.time = time;
        k.value = value;
        let j = self.0.partition_point(|x| x.time < time);
        self.0.insert(j, k);
        j
    }

    pub fn last_time(&self) -> f32 {
        self.0.last().map_or(0.0, |k| k.time)
    }

    /// Value at `time`: the first/last value outside the keys, Hermite in between.
    pub fn evaluate(&self, time: f32) -> f32 {
        let k = &self.0;
        match k.len() {
            0 => return 0.0,
            1 => return k[0].value,
            _ => {}
        }
        if time <= k[0].time {
            return k[0].value;
        }
        let last = k.len() - 1;
        if time >= k[last].time {
            return k[last].value;
        }
        let i = k.partition_point(|x| x.time <= time) - 1;
        let (a, b) = (k[i], k[i + 1]);
        if a.tangent == TangentMode::Constant {
            return a.value;
        }
        let dt = b.time - a.time;
        let s = (time - a.time) / dt;
        let (m0, m1) = (self.out_slope(i), self.in_slope(i + 1));
        hermite(s, a.value, b.value, m0 * dt, m1 * dt)
    }

    /// Slope leaving key `i` (towards key `i + 1`).
    pub fn out_slope(&self, i: usize) -> f32 {
        match self.0[i].tangent {
            TangentMode::Linear => self.segment_slope(i),
            TangentMode::Constant => 0.0,
            TangentMode::Auto => self.auto_slope(i),
        }
    }

    /// Slope arriving at key `i` (from key `i - 1`).
    pub fn in_slope(&self, i: usize) -> f32 {
        match self.0[i].tangent {
            TangentMode::Linear if i > 0 => self.segment_slope(i - 1),
            TangentMode::Linear | TangentMode::Constant => 0.0,
            TangentMode::Auto => self.auto_slope(i),
        }
    }

    /// Straight-line slope from key `i` to key `i + 1`.
    fn segment_slope(&self, i: usize) -> f32 {
        match (self.0.get(i), self.0.get(i + 1)) {
            (Some(a), Some(b)) => (b.value - a.value) / (b.time - a.time).max(TIME_EPSILON),
            _ => 0.0,
        }
    }

    /// Clamped auto tangent: flat at the ends and at local minima/maxima.
    fn auto_slope(&self, i: usize) -> f32 {
        let k = &self.0;
        if i == 0 || i + 1 >= k.len() {
            return 0.0;
        }
        let (p, c, n) = (k[i - 1], k[i], k[i + 1]);
        if (c.value - p.value) * (n.value - c.value) <= 0.0 {
            return 0.0;
        }
        (n.value - p.value) / (n.time - p.time)
    }
}

/// Cubic Hermite between `p0` and `p1` with tangents already scaled by the segment length.
fn hermite(s: f32, p0: f32, p1: f32, m0: f32, m1: f32) -> f32 {
    let (s2, s3) = (s * s, s * s * s);
    (2.0 * s3 - 3.0 * s2 + 1.0) * p0
        + (s3 - 2.0 * s2 + s) * m0
        + (-2.0 * s3 + 3.0 * s2) * p1
        + (s3 - s2) * m1
}
