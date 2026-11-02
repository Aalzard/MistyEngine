//! Game loop time and the per-frame update that runs while playing.

use crate::scene::Scene;

/// Like the familiar `Time` class: elapsed time, frame delta, frame count, time scale.
#[derive(Clone, Copy, Debug)]
pub struct Time {
    /// Seconds since Play started (scaled).
    pub time: f64,
    /// Scaled seconds since the previous frame.
    pub delta: f32,
    /// Real seconds since the previous frame.
    pub unscaled_delta: f32,
    pub frame: u64,
    pub time_scale: f32,
}

impl Default for Time {
    fn default() -> Self {
        Self {
            time: 0.0,
            delta: 0.0,
            unscaled_delta: 0.0,
            frame: 0,
            time_scale: 1.0,
        }
    }
}

impl Time {
    /// Advance by `real_dt` seconds of wall-clock time (capped like the maximum allowed timestep).
    pub fn advance(&mut self, real_dt: f32) {
        let dt = real_dt.clamp(0.0, 1.0 / 3.0);
        self.unscaled_delta = dt;
        self.delta = dt * self.time_scale;
        self.time += self.delta as f64;
        self.frame += 1;
    }
}

/// One game frame. Physics (step 7) and scripts (step 8) hook in here.
pub fn update(_scene: &mut Scene, _time: &Time) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_scales_and_caps_delta() {
        let mut t = Time::default();
        t.advance(0.016);
        assert_eq!(t.frame, 1);
        t.time_scale = 0.5;
        t.advance(0.02);
        assert!((t.delta - 0.01).abs() < 1e-6);
        t.advance(5.0);
        assert!(
            (t.unscaled_delta - 1.0 / 3.0).abs() < 1e-6,
            "long hitches are capped"
        );
    }
}
