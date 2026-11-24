//! Tap counting (`Touch.tapCount`): a tap is a touch lifted soon after it went down without
//! moving far; taps that follow each other quickly near the same spot count up (2 on the
//! second tap of a double tap), like Unity.
//!
//! The scripting world runs one [`TapCounter`] over each frame's touches before scripts read
//! them, so every front end (editor Game view, player, tests) gets the same counts.

use std::collections::HashMap;

use glam::Vec2;

use super::touch::{Touch, TouchPhase};

/// A finger that is down.
#[derive(Clone, Copy, Debug)]
struct Finger {
    start: Vec2,
    began: f64,
    /// The count this touch makes if it turns out to be a tap.
    count: u32,
    /// Still short and still enough to be a tap.
    tap: bool,
}

/// A finished tap that the next one can follow.
#[derive(Clone, Copy, Debug)]
struct RecentTap {
    position: Vec2,
    at: f64,
    count: u32,
}

/// Fills [`Touch::tap_count`] frame by frame.
#[derive(Clone, Debug)]
pub struct TapCounter {
    /// Longest press (seconds) that is still a tap.
    pub max_tap_time: f64,
    /// Longest gap (seconds) between lifting one tap and touching down for the next one for
    /// them to count together.
    pub multi_tap_delay: f64,
    /// Farthest (pixels) a tap may move, and the next tap may land from the last one.
    pub radius: f32,
    clock: f64,
    fingers: HashMap<u64, Finger>,
    recent: Vec<RecentTap>,
}

impl Default for TapCounter {
    fn default() -> Self {
        Self {
            max_tap_time: 0.5,
            multi_tap_delay: 0.5,
            radius: 25.0,
            clock: 0.0,
            fingers: HashMap::new(),
            recent: Vec::new(),
        }
    }
}

impl TapCounter {
    /// Count this frame's touches, `dt` real seconds after the last frame.
    pub fn apply(&mut self, touches: &mut [Touch], dt: f32) {
        self.clock += f64::from(dt.max(0.0));
        let now = self.clock;
        self.apply_at(touches, now);
    }

    /// Count this frame's touches at `now` seconds (a steadily growing clock).
    pub fn apply_at(&mut self, touches: &mut [Touch], now: f64) {
        let delay = self.multi_tap_delay;
        self.recent.retain(|r| now - r.at <= delay);
        for t in touches.iter_mut() {
            // A finger that went down and up within one frame is only seen lifted.
            if t.phase == TouchPhase::Began || !self.fingers.contains_key(&t.finger_id) {
                let count = self.follow(t.position) + 1;
                self.fingers.insert(
                    t.finger_id,
                    Finger {
                        start: t.position,
                        began: now,
                        count,
                        tap: true,
                    },
                );
            }
            let (radius, max_time) = (self.radius, self.max_tap_time);
            let Some(f) = self.fingers.get_mut(&t.finger_id) else {
                continue;
            };
            if (t.position - f.start).length() > radius || now - f.began > max_time {
                f.tap = false;
            }
            let f = *f;
            match t.phase {
                TouchPhase::Ended => {
                    self.fingers.remove(&t.finger_id);
                    t.tap_count = if f.tap {
                        self.recent.push(RecentTap {
                            position: t.position,
                            at: now,
                            count: f.count,
                        });
                        f.count
                    } else {
                        0
                    };
                }
                TouchPhase::Canceled => {
                    self.fingers.remove(&t.finger_id);
                    t.tap_count = 0;
                }
                _ => t.tap_count = f.count,
            }
        }
        // Fingers the window stopped reporting without an end.
        self.fingers
            .retain(|id, _| touches.iter().any(|t| t.finger_id == *id));
    }

    /// The count of the recent tap nearest `p` within reach (taken: it continues here), or 0.
    fn follow(&mut self, p: Vec2) -> u32 {
        let radius = self.radius;
        let best = self
            .recent
            .iter()
            .enumerate()
            .filter(|(_, r)| (r.position - p).length() <= radius)
            .min_by(|a, b| {
                let da = (a.1.position - p).length_squared();
                let db = (b.1.position - p).length_squared();
                da.total_cmp(&db)
            })
            .map(|(i, _)| i);
        match best {
            Some(i) => self.recent.remove(i).count,
            None => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(id: u64, x: f32, y: f32, phase: TouchPhase) -> Touch {
        Touch {
            finger_id: id,
            position: Vec2::new(x, y),
            phase,
            ..Default::default()
        }
    }

    /// Run one frame with one finger; its tap count.
    fn step(c: &mut TapCounter, now: f64, t: Touch) -> u32 {
        let mut list = [t];
        c.apply_at(&mut list, now);
        list[0].tap_count
    }

    /// A quick tap at (x, y) from `t0`; the counts seen when it went down and when it lifted.
    fn tap(c: &mut TapCounter, id: u64, t0: f64, x: f32, y: f32) -> (u32, u32) {
        let down = step(c, t0, touch(id, x, y, TouchPhase::Began));
        step(c, t0 + 0.05, touch(id, x, y, TouchPhase::Stationary));
        let up = step(c, t0 + 0.1, touch(id, x + 2.0, y, TouchPhase::Ended));
        (down, up)
    }

    #[test]
    fn taps_in_a_row_near_the_same_spot_count_up() {
        let mut c = TapCounter::default();
        assert_eq!(tap(&mut c, 1, 0.0, 100.0, 100.0), (1, 1));
        assert_eq!(tap(&mut c, 2, 0.3, 105.0, 98.0), (2, 2));
        assert_eq!(tap(&mut c, 3, 0.6, 101.0, 103.0), (3, 3));
        // Too long after the last one: a new series.
        assert_eq!(tap(&mut c, 4, 2.0, 100.0, 100.0), (1, 1));
        // Far away: a new series too.
        assert_eq!(tap(&mut c, 5, 2.3, 300.0, 100.0), (1, 1));
    }

    #[test]
    fn a_long_press_or_a_drag_is_not_a_tap_and_breaks_the_series() {
        let mut c = TapCounter::default();
        tap(&mut c, 1, 0.0, 50.0, 50.0);
        // Held 0.8 s: reported while down, 0 when lifted.
        assert_eq!(
            step(&mut c, 0.3, touch(2, 50.0, 50.0, TouchPhase::Began)),
            2
        );
        assert_eq!(
            step(&mut c, 0.7, touch(2, 50.0, 50.0, TouchPhase::Stationary)),
            2
        );
        assert_eq!(
            step(&mut c, 1.1, touch(2, 50.0, 50.0, TouchPhase::Ended)),
            0
        );
        assert_eq!(tap(&mut c, 3, 1.3, 50.0, 50.0), (1, 1));
        // A drag.
        step(&mut c, 3.0, touch(4, 50.0, 50.0, TouchPhase::Began));
        step(&mut c, 3.05, touch(4, 120.0, 50.0, TouchPhase::Moved));
        assert_eq!(
            step(&mut c, 3.1, touch(4, 120.0, 50.0, TouchPhase::Ended)),
            0
        );
        assert_eq!(tap(&mut c, 5, 3.3, 120.0, 50.0), (1, 1));
    }

    #[test]
    fn a_tap_within_one_frame_and_canceled_touches() {
        let mut c = TapCounter::default();
        // Down and up between two frames: only the Ended touch is seen.
        assert_eq!(
            step(&mut c, 0.0, touch(7, 10.0, 10.0, TouchPhase::Ended)),
            1
        );
        assert_eq!(
            step(&mut c, 0.2, touch(8, 10.0, 10.0, TouchPhase::Ended)),
            2
        );
        step(&mut c, 0.4, touch(9, 10.0, 10.0, TouchPhase::Began));
        assert_eq!(
            step(&mut c, 0.45, touch(9, 10.0, 10.0, TouchPhase::Canceled)),
            0
        );
        assert_eq!(tap(&mut c, 10, 0.6, 10.0, 10.0), (1, 1));
    }

    #[test]
    fn two_fingers_count_on_their_own_spots() {
        let mut c = TapCounter::default();
        let mut frame = |now: f64, list: &mut [Touch]| {
            c.apply_at(list, now);
            list.iter().map(|t| t.tap_count).collect::<Vec<_>>()
        };
        let a = |phase| touch(1, 0.0, 0.0, phase);
        let b = |phase| touch(2, 500.0, 0.0, phase);
        assert_eq!(
            frame(0.0, &mut [a(TouchPhase::Began), b(TouchPhase::Began)]),
            [1, 1]
        );
        assert_eq!(
            frame(0.1, &mut [a(TouchPhase::Ended), b(TouchPhase::Ended)]),
            [1, 1]
        );
        assert_eq!(frame(0.3, &mut [b(TouchPhase::Began)]), [2]);
        assert_eq!(frame(0.35, &mut [b(TouchPhase::Ended)]), [2]);
        // Finger 1's spot still remembers its single tap.
        assert_eq!(frame(0.4, &mut [a(TouchPhase::Began)]), [2]);
    }

    #[test]
    fn the_clock_advances_with_dt() {
        let mut c = TapCounter::default();
        let mut t = [touch(1, 0.0, 0.0, TouchPhase::Began)];
        c.apply(&mut t, 0.016);
        t[0].phase = TouchPhase::Ended;
        c.apply(&mut t, 0.1);
        assert_eq!(t[0].tap_count, 1);
        let mut t = [touch(2, 0.0, 0.0, TouchPhase::Ended)];
        c.apply(&mut t, 1.0);
        assert_eq!(t[0].tap_count, 1, "a second later starts over");
    }
}
