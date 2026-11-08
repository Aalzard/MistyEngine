//! Signal processing for the mixer graph, in plain Rust on stereo frame buffers: the effect
//! processors ([`fx`]), the binaural renderer ([`hrtf`]), smoothing ramps, and measurement
//! helpers used by tests and meters ([`analysis`]). Nothing here touches a sound device, so
//! every effect can be rendered offline and checked against the numbers it should produce.

pub mod analysis;
pub mod fx;
pub mod hrtf;
pub mod pitch;

pub use kira::Frame;

/// Frames processed at once by the graph (buffers are allocated for this many).
pub const BLOCK: usize = 256;

/// Seconds a click-free change (insert, remove, bypass, reroute) takes to fade.
pub const FADE_SECONDS: f32 = 0.02;

/// A value moving linearly to its target over a fixed time, sample by sample.
#[derive(Clone, Copy, Debug)]
pub struct Ramp {
    pub value: f32,
    pub target: f32,
    step: f32,
}

impl Ramp {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            target: value,
            step: 0.0,
        }
    }

    /// Head for `target`, arriving after `samples` samples (0 = jump).
    pub fn set(&mut self, target: f32, samples: f32) {
        if target == self.target {
            return;
        }
        self.target = target;
        if samples <= 1.0 {
            self.value = target;
            self.step = 0.0;
        } else {
            self.step = (target - self.value) / samples;
        }
    }

    /// Value for the next sample.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        if self.step != 0.0 {
            self.value += self.step;
            if (self.step > 0.0 && self.value >= self.target)
                || (self.step < 0.0 && self.value <= self.target)
            {
                self.value = self.target;
                self.step = 0.0;
            }
        }
        self.value
    }

    pub fn moving(&self) -> bool {
        self.step != 0.0
    }

    /// At rest at `v`.
    pub fn is_at(&self, v: f32) -> bool {
        !self.moving() && self.value == v
    }
}

/// Samples of a [`FADE_SECONDS`] fade at `sample_rate`.
pub fn fade_samples(sample_rate: f32) -> f32 {
    FADE_SECONDS * sample_rate
}

/// One-pole smoothing coefficient reaching ~63 % of a step after `seconds`.
pub fn one_pole(seconds: f32, sample_rate: f32) -> f32 {
    if seconds <= 0.0 {
        1.0
    } else {
        1.0 - (-1.0 / (seconds * sample_rate)).exp()
    }
}
