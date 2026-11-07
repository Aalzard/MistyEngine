//! Frame-rate independent critically damped smoothing for the Scene camera.
//!
//! Both springs use the exact solution of a critically damped spring over a step (the target
//! held still during the step), so a step of 1/30 s lands where two steps of 1/60 s do. A
//! smooth time `T` means: a target moving at a steady rate is followed `T` seconds behind, and
//! a jump is 90 % covered after about `2T`. `T = 0` follows the target exactly.

use glam::Vec3;

/// Angular frequency of a critically damped spring with smooth time `t` (SmoothDamp's).
fn omega(t: f32) -> f32 {
    2.0 / t
}

/// Chases a goal that input moves: [`Self::push`] moves the goal, [`Self::step`] returns how
/// far the smoothed value moves this frame. Only the offset to the goal is stored, so it
/// works for amounts applied as increments (look angles, pan offsets, log zoom).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Chase {
    /// Smoothed value minus goal (minus what is still to be applied).
    offset: Vec3,
    /// Velocity of the smoothed value.
    vel: Vec3,
}

impl Chase {
    /// Move the goal by `d`.
    pub fn push(&mut self, d: Vec3) {
        self.offset -= d;
    }

    /// Still to be applied (goal minus smoothed value).
    #[cfg(test)]
    pub fn remaining(&self) -> Vec3 {
        -self.offset
    }

    /// Advance by `dt` with smooth time `smooth`; returns the increment to apply. The rest
    /// is applied at once when it falls under `eps` (and the velocity is that small too).
    pub fn step(&mut self, smooth: f32, dt: f32, eps: f32) -> Vec3 {
        if smooth <= 0.0 {
            return self.finish();
        }
        if dt <= 0.0 {
            return Vec3::ZERO;
        }
        let w = omega(smooth);
        let y = self.offset;
        let j = self.vel + w * y;
        let e = (-w * dt).exp();
        self.offset = (y + j * dt) * e;
        self.vel = (self.vel - w * j * dt) * e;
        let mut inc = self.offset - y;
        if self.offset.abs().max_element() < eps && self.vel.abs().max_element() * smooth < eps {
            inc += self.finish();
        }
        inc
    }

    /// Apply everything that is left now.
    pub fn finish(&mut self) -> Vec3 {
        let rest = -self.offset;
        *self = Self::default();
        rest
    }

    /// Drop what is left (e.g. pitch pushed past the pole).
    pub fn cancel_axis(&mut self, axis: usize) {
        self.offset[axis] = 0.0;
        self.vel[axis] = 0.0;
    }

    pub fn is_settled(&self) -> bool {
        self.offset == Vec3::ZERO && self.vel == Vec3::ZERO
    }
}

/// A velocity easing towards a target velocity, and the distance it covers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Velocity {
    pub vel: Vec3,
    /// Rate of change of `vel`.
    acc: Vec3,
}

impl Velocity {
    /// Ease `vel` towards `target` (held for the step) and return the distance travelled
    /// during the step, integrated exactly.
    pub fn step(&mut self, target: Vec3, smooth: f32, dt: f32) -> Vec3 {
        if dt <= 0.0 {
            return Vec3::ZERO;
        }
        if smooth <= 0.0 {
            self.vel = target;
            self.acc = Vec3::ZERO;
            return target * dt;
        }
        let w = omega(smooth);
        let y = self.vel - target;
        let j = self.acc + w * y;
        let e = (-w * dt).exp();
        let travelled =
            target * dt + y * ((1.0 - e) / w) + j * ((1.0 - e * (1.0 + w * dt)) / (w * w));
        self.vel = target + (y + j * dt) * e;
        self.acc = (self.acc - w * j * dt) * e;
        // Snap to rest once there is nothing left to see.
        if target == Vec3::ZERO && self.vel.length() < 1e-4 && self.acc.length() * smooth < 1e-4 {
            *self = Self::default();
        }
        travelled
    }

    pub fn is_settled(&self) -> bool {
        self.vel == Vec3::ZERO && self.acc == Vec3::ZERO
    }

    pub fn stop(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATES: [f32; 3] = [30.0, 60.0, 144.0];

    /// Run `seconds` at `fps` (the last step is shortened to end exactly on time), calling
    /// `f(t, dt)` per frame with the frame's start time. Returns the samples at each 0.1 s.
    fn run(fps: f32, seconds: f32, mut f: impl FnMut(f32, f32) -> f32) -> Vec<f32> {
        let mut t = 0.0f32;
        let mut out = Vec::new();
        let mut next = 0.1f32;
        let dt = 1.0 / fps;
        let mut value = 0.0;
        while t < seconds - 1e-6 {
            // Stop on each sample time so every rate is compared at the same instants.
            let step = dt.min(next - t).min(seconds - t);
            value = f(t, step);
            t += step;
            if (t - next).abs() < 1e-5 {
                out.push(value);
                next += 0.1;
            }
        }
        out.push(value);
        out
    }

    fn assert_close(paths: &[Vec<f32>], tol: f32) {
        for p in &paths[1..] {
            assert_eq!(p.len(), paths[0].len());
            for (i, (a, b)) in paths[0].iter().zip(p).enumerate() {
                assert!((a - b).abs() <= tol, "sample {i}: {a} vs {b} ({paths:?})");
            }
        }
    }

    /// A jump of the goal: every frame rate follows the same curve exactly.
    #[test]
    fn chase_step_response_is_frame_rate_independent() {
        let paths: Vec<_> = RATES
            .iter()
            .map(|&fps| {
                let mut c = Chase::default();
                c.push(Vec3::X * 10.0);
                let mut applied = 0.0;
                run(fps, 0.6, |_, dt| {
                    applied += c.step(0.08, dt, 1e-6).x;
                    applied
                })
            })
            .collect();
        assert_close(&paths, 1e-3);
        // Critically damped: never past the goal, and settled after a few smooth times.
        for p in &paths {
            assert!(p.iter().all(|v| *v <= 10.0 + 1e-4), "{p:?}");
        }
        assert!((paths[0].last().unwrap() - 10.0).abs() < 1e-3);
    }

    /// A mouse moving steadily for 0.3 s then stopping: input arrives per frame (coarser at
    /// 30 FPS), the smoothed path agrees within a fraction of a degree.
    #[test]
    fn chase_of_streamed_input_matches_across_frame_rates() {
        let speed = 200.0; // degrees per second
        let paths: Vec<_> = RATES
            .iter()
            .map(|&fps| {
                let mut c = Chase::default();
                let mut applied = 0.0;
                run(fps, 0.6, |t, dt| {
                    // The input of this frame happened during it (as with mouse deltas).
                    let moving = (t + dt).min(0.3) - t.min(0.3);
                    c.push(Vec3::X * speed * moving.max(0.0));
                    applied += c.step(0.012, dt, 1e-6).x;
                    applied
                })
            })
            .collect();
        // 30 FPS input lands in 33 ms lumps; with a 12 ms smooth time that is at most about
        // half a lump (3 degrees) during the motion, and exact once it settles.
        assert_close(&paths, 3.5);
        for p in &paths {
            assert!((p.last().unwrap() - 60.0).abs() < 1e-3, "{p:?}");
        }
    }

    /// Smoothing adds a lag of about its smooth time to a steady motion, not more.
    #[test]
    fn steady_motion_lags_by_the_smooth_time() {
        let mut c = Chase::default();
        let (speed, smooth, dt) = (100.0, 0.012, 1.0 / 240.0);
        let mut applied = 0.0;
        for _ in 0..240 {
            c.push(Vec3::X * speed * dt);
            applied += c.step(smooth, dt, 1e-6).x;
        }
        // Input lands at the start of each frame, so half a frame less than the smooth time.
        let lag = c.remaining().x / speed;
        assert!((lag - (smooth - dt * 0.5)).abs() < 0.001, "lag {lag}");
        assert!(applied > 0.0);
    }

    #[test]
    fn zero_smooth_time_applies_input_at_once() {
        let mut c = Chase::default();
        c.push(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(c.step(0.0, 1.0 / 60.0, 1e-6), Vec3::new(1.0, 2.0, 3.0));
        assert!(c.is_settled());
        let mut v = Velocity::default();
        assert_eq!(v.step(Vec3::X * 4.0, 0.0, 0.5), Vec3::X * 2.0);
    }

    /// Hold a key for 0.5 s, release: speed eases in and out, and the distance flown is the
    /// same at every frame rate.
    #[test]
    fn velocity_path_is_frame_rate_independent() {
        let paths: Vec<_> = RATES
            .iter()
            .map(|&fps| {
                let mut v = Velocity::default();
                let mut pos = 0.0;
                run(fps, 1.0, |t, dt| {
                    let target = if t < 0.5 { Vec3::Z * 10.0 } else { Vec3::ZERO };
                    pos += v.step(target, 0.08, dt).z;
                    pos
                })
            })
            .collect();
        assert_close(&paths, 1e-3);
        // Easing in loses speed × smooth time (10 m/s × 0.08 s) and the glide after the
        // release wins it back: the same distance as instant start and stop.
        let end = *paths[0].last().unwrap();
        assert!((end - 5.0).abs() < 0.01, "{end}");
    }

    #[test]
    fn velocity_comes_to_rest() {
        let mut v = Velocity::default();
        for _ in 0..30 {
            v.step(Vec3::X, 0.08, 1.0 / 60.0);
        }
        for _ in 0..120 {
            v.step(Vec3::ZERO, 0.08, 1.0 / 60.0);
        }
        assert!(v.is_settled());
    }
}
