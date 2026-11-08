//! Touches: the fingers on a touch screen this frame (`Input.touchCount`, `Input.GetTouch`,
//! `<Touchscreen>/primaryTouch/…` bindings), built from the window's touch events.

use std::collections::BTreeMap;

use glam::Vec2;

/// Where a finger is in its gesture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TouchPhase {
    /// Touched down this frame.
    #[default]
    Began,
    Moved,
    Stationary,
    /// Lifted this frame.
    Ended,
    /// The system took the touch away.
    Canceled,
}

impl TouchPhase {
    pub fn name(self) -> &'static str {
        match self {
            Self::Began => "Began",
            Self::Moved => "Moved",
            Self::Stationary => "Stationary",
            Self::Ended => "Ended",
            Self::Canceled => "Canceled",
        }
    }

    /// Still on the screen (Ended / Canceled touches are reported for one frame).
    pub fn is_down(self) -> bool {
        !matches!(self, Self::Ended | Self::Canceled)
    }
}

/// One finger this frame. Positions are game-image pixels from the top-left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Touch {
    pub finger_id: u64,
    pub position: Vec2,
    /// Movement since the previous frame.
    pub delta: Vec2,
    pub phase: TouchPhase,
}

/// What a window reported for one finger.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TouchEvent {
    Start(u64, Vec2),
    Move(u64, Vec2),
    End(u64, Vec2),
    Cancel(u64, Vec2),
}

/// Turns touch events into per-frame touches with phases (kept by the editor's Game view and
/// the player between frames).
#[derive(Clone, Debug, Default)]
pub struct TouchTracker {
    /// Fingers down: id → (position at the end of the last frame, position now, began this
    /// frame).
    active: BTreeMap<u64, (Vec2, Vec2, bool)>,
    /// Fingers lifted this frame.
    ended: Vec<Touch>,
}

impl TouchTracker {
    pub fn event(&mut self, e: TouchEvent) {
        match e {
            TouchEvent::Start(id, p) => {
                self.active.insert(id, (p, p, true));
            }
            TouchEvent::Move(id, p) => {
                if let Some(t) = self.active.get_mut(&id) {
                    t.1 = p;
                }
            }
            TouchEvent::End(id, p) | TouchEvent::Cancel(id, p) => {
                let (last, _, _) = self.active.remove(&id).unwrap_or((p, p, true));
                let phase = if matches!(e, TouchEvent::End(..)) {
                    TouchPhase::Ended
                } else {
                    TouchPhase::Canceled
                };
                self.ended.push(Touch {
                    finger_id: id,
                    position: p,
                    delta: p - last,
                    phase,
                });
            }
        }
    }

    /// This frame's touches (fingers down in id order, then the lifted ones); starts the next
    /// frame.
    pub fn frame(&mut self) -> Vec<Touch> {
        let mut out: Vec<Touch> = self
            .active
            .iter_mut()
            .map(|(id, (last, now, began))| {
                let delta = *now - *last;
                let phase = if *began {
                    TouchPhase::Began
                } else if delta.length_squared() > 0.0 {
                    TouchPhase::Moved
                } else {
                    TouchPhase::Stationary
                };
                *last = *now;
                *began = false;
                Touch {
                    finger_id: *id,
                    position: *now,
                    delta,
                    phase,
                }
            })
            .collect();
        out.append(&mut self.ended);
        out
    }

    /// Any finger down?
    pub fn any(&self) -> bool {
        !self.active.is_empty()
    }
}
