//! Smooth remote motion: a playback clock per sender, a buffer of received transform states
//! played a little in the past, and the owner's prediction history for reconciliation.

use std::collections::VecDeque;

use glam::{Quat, Vec3};

/// Follows a sender's timestamps on the local timeline: it runs at local speed and is nudged
/// towards each sample, so jitter averages out.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlaybackClock {
    time: f64,
    started: bool,
}

impl PlaybackClock {
    pub fn sample(&mut self, sender_time: f64) {
        if !self.started || (sender_time - self.time).abs() > 0.5 {
            self.time = sender_time;
            self.started = true;
        } else {
            self.time += (sender_time - self.time) * 0.1;
        }
    }
    pub fn advance(&mut self, dt: f64) {
        if self.started {
            self.time += dt;
        }
    }
    pub fn now(&self) -> f64 {
        self.time
    }
    pub fn started(&self) -> bool {
        self.started
    }
}

/// A full transform state at a sender time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot {
    pub time: f64,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Snapshot {
    fn lerp(&self, b: &Snapshot, t: f32) -> Snapshot {
        Snapshot {
            time: self.time + (b.time - self.time) * t as f64,
            position: self.position.lerp(b.position, t),
            rotation: self.rotation.slerp(b.rotation, t),
            scale: self.scale.lerp(b.scale, t),
        }
    }
}

/// Received states of one remote object, oldest first.
#[derive(Clone, Debug, Default)]
pub struct SnapshotBuffer {
    states: VecDeque<Snapshot>,
    /// The next sample should jump there (a teleport or the first state).
    pub snap: bool,
}

impl SnapshotBuffer {
    /// The newest state (partial updates fill in from it).
    pub fn latest(&self) -> Option<&Snapshot> {
        self.states.back()
    }

    /// Add a state; a jump longer than `snap_distance` from the previous one clears the
    /// buffer, so the object teleports instead of sliding across.
    pub fn push(&mut self, s: Snapshot, snap_distance: f32) {
        if let Some(last) = self.states.back() {
            if s.time <= last.time {
                return; // late or duplicate (unreliable channel)
            }
            if last.position.distance(s.position) > snap_distance {
                self.states.clear();
                self.snap = true;
            }
        } else {
            self.snap = true;
        }
        self.states.push_back(s);
        while self.states.len() > 64 {
            self.states.pop_front();
        }
    }

    /// The state at `time`, between the two received around it (held at the ends).
    pub fn sample(&mut self, time: f64) -> Option<Snapshot> {
        let first = *self.states.front()?;
        if self.snap {
            self.snap = false;
            let last = *self.states.back()?;
            // Teleports show at once; play on from there.
            self.states.retain(|s| s.time >= last.time);
            return Some(last);
        }
        if time <= first.time {
            return Some(first);
        }
        // Drop states that are fully in the past (keep one before `time`).
        while self.states.len() >= 2 && self.states[1].time <= time {
            self.states.pop_front();
        }
        let a = self.states[0];
        match self.states.get(1) {
            Some(b) => {
                let t = ((time - a.time) / (b.time - a.time)).clamp(0.0, 1.0) as f32;
                Some(a.lerp(b, t))
            }
            None => Some(a),
        }
    }

    pub fn len(&self) -> usize {
        self.states.len()
    }
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
    pub fn clear(&mut self) {
        self.states.clear();
    }
}

/// The owner's predicted poses by local frame, for reconciliation with server states.
#[derive(Clone, Debug, Default)]
pub struct PredictionHistory {
    entries: VecDeque<(u32, Vec3, Quat)>,
}

impl PredictionHistory {
    pub fn record(&mut self, frame: u32, position: Vec3, rotation: Quat) {
        self.entries.push_back((frame, position, rotation));
        while self.entries.len() > 240 {
            self.entries.pop_front();
        }
    }

    /// The server says the pose after input `frame` is `server`: returns how far the
    /// prediction was off (add it to the current pose), and forgets frames up to `frame`.
    /// `None` when that frame is no longer known (snap to the server instead).
    pub fn reconcile(
        &mut self,
        frame: u32,
        server_pos: Vec3,
        server_rot: Quat,
    ) -> Option<(Vec3, Quat)> {
        let i = self.entries.iter().position(|(f, _, _)| *f == frame)?;
        let (_, p, r) = self.entries[i];
        let dp = server_pos - p;
        let dr = server_rot * r.inverse();
        self.entries.drain(..=i);
        // Later predictions were built on the wrong pose too.
        for e in &mut self.entries {
            e.1 += dp;
            e.2 = (dr * e.2).normalize();
        }
        Some((dp, dr))
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t: f64, x: f32) -> Snapshot {
        Snapshot {
            time: t,
            position: Vec3::new(x, 0.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    #[test]
    fn buffer_interpolates_holds_and_snaps() {
        let mut b = SnapshotBuffer::default();
        b.push(at(1.0, 0.0), 3.0);
        assert_eq!(b.sample(0.5).unwrap().position.x, 0.0, "first state jumps");
        b.push(at(1.1, 1.0), 3.0);
        b.push(at(1.2, 2.0), 3.0);
        b.push(at(1.15, 9.0), 3.0); // late: ignored
        assert!((b.sample(1.05).unwrap().position.x - 0.5).abs() < 1e-5);
        assert!((b.sample(1.15).unwrap().position.x - 1.5).abs() < 1e-5);
        assert_eq!(b.sample(5.0).unwrap().position.x, 2.0, "held at the newest");
        // A long jump teleports.
        b.push(at(1.3, 50.0), 3.0);
        assert_eq!(b.sample(1.21).unwrap().position.x, 50.0);
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn playback_clock_follows_jittery_samples() {
        let mut c = PlaybackClock::default();
        let mut sender = 10.0;
        for i in 0..300 {
            c.advance(1.0 / 60.0);
            sender += 1.0 / 60.0;
            if i % 2 == 0 {
                let jitter = if i % 4 == 0 { 0.02 } else { -0.02 };
                c.sample(sender + jitter);
            }
        }
        assert!((c.now() - sender).abs() < 0.03, "{} vs {sender}", c.now());
    }

    #[test]
    fn reconciliation_shifts_later_predictions() {
        let mut h = PredictionHistory::default();
        for f in 1..=5 {
            h.record(f, Vec3::new(f as f32, 0.0, 0.0), Quat::IDENTITY);
        }
        // The server put frame 3 at x = 2.5 (half a metre short).
        let (dp, _) = h
            .reconcile(3, Vec3::new(2.5, 0.0, 0.0), Quat::IDENTITY)
            .unwrap();
        assert!((dp.x + 0.5).abs() < 1e-6);
        let (dp, _) = h
            .reconcile(5, Vec3::new(4.5, 0.0, 0.0), Quat::IDENTITY)
            .unwrap();
        assert!(dp.length() < 1e-6, "already corrected");
        assert!(
            h.reconcile(1, Vec3::ZERO, Quat::IDENTITY).is_none(),
            "forgotten"
        );
    }
}
