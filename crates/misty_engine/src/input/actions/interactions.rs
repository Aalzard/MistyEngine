//! Interactions: patterns of control input that drive an action's started / performed /
//! canceled. `press(pressPoint,behavior)`, `hold(duration,pressPoint)`, `tap(duration)`,
//! `slowTap(duration)` and `multiTap(tapTime,tapDelay,tapCount)`. Each one is a small state
//! machine stepped every frame with the control's magnitude and the time, so timeouts happen
//! even when the input doesn't change.

use super::value::{NamedParams, parse_list};

/// Default press point for buttons and interactions.
pub const DEFAULT_PRESS_POINT: f32 = 0.5;
/// A pressed control counts as released below this share of its press point.
pub const RELEASE_RATIO: f32 = 0.75;
/// Default `hold` duration.
pub const DEFAULT_HOLD: f64 = 0.4;
/// Default `tap` duration (longest press that still taps).
pub const DEFAULT_TAP: f64 = 0.2;
/// Default `slowTap` duration (shortest press that counts).
pub const DEFAULT_SLOW_TAP: f64 = 0.5;
/// Default `multiTap` gap allowed between taps.
pub const DEFAULT_TAP_DELAY: f64 = 0.75;
/// Slack on time comparisons (frame times add up with rounding errors).
const EPSILON: f64 = 1e-6;

/// Interactions the editor offers: (name, default parameters).
pub const KINDS: &[(&str, &str)] = &[
    ("press", "behavior=PressOnly"),
    ("hold", "duration=0.4"),
    ("tap", "duration=0.2"),
    ("slowTap", "duration=0.5"),
    ("multiTap", "tapCount=2,tapTime=0.2,tapDelay=0.75"),
];

/// When a `press` interaction performs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressBehavior {
    /// On press (canceled on release).
    PressOnly,
    /// On release.
    ReleaseOnly,
    /// On press and again on release.
    PressAndRelease,
}

impl PressBehavior {
    pub const ALL: [PressBehavior; 3] = [Self::PressOnly, Self::ReleaseOnly, Self::PressAndRelease];

    pub fn name(self) -> &'static str {
        match self {
            Self::PressOnly => "PressOnly",
            Self::ReleaseOnly => "ReleaseOnly",
            Self::PressAndRelease => "PressAndRelease",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "releaseonly" | "1" => Self::ReleaseOnly,
            "pressandrelease" | "2" => Self::PressAndRelease,
            _ => Self::PressOnly,
        }
    }
}

/// A parsed interaction with its parameters.
#[derive(Clone, Debug, PartialEq)]
pub enum Interaction {
    Press {
        press_point: f32,
        behavior: PressBehavior,
    },
    Hold {
        press_point: f32,
        duration: f64,
    },
    Tap {
        press_point: f32,
        duration: f64,
    },
    SlowTap {
        press_point: f32,
        duration: f64,
    },
    MultiTap {
        press_point: f32,
        tap_time: f64,
        tap_delay: f64,
        tap_count: u32,
    },
}

impl Interaction {
    /// Parse one entry; unknown names give None.
    pub fn from_params(p: &NamedParams) -> Option<Self> {
        let pp = p.num("pressPoint", DEFAULT_PRESS_POINT).clamp(0.01, 1.0);
        let secs = |key: &str, default: f64| f64::from(p.num(key, default as f32)).max(0.0);
        Some(match p.name.to_ascii_lowercase().as_str() {
            "press" => Interaction::Press {
                press_point: pp,
                behavior: PressBehavior::parse(p.get("behavior").unwrap_or("")),
            },
            "hold" => Interaction::Hold {
                press_point: pp,
                duration: secs("duration", DEFAULT_HOLD),
            },
            "tap" => Interaction::Tap {
                press_point: pp,
                duration: secs("duration", DEFAULT_TAP),
            },
            "slowtap" => Interaction::SlowTap {
                press_point: pp,
                duration: secs("duration", DEFAULT_SLOW_TAP),
            },
            "multitap" => Interaction::MultiTap {
                press_point: pp,
                tap_time: secs("tapTime", DEFAULT_TAP),
                tap_delay: secs("tapDelay", DEFAULT_TAP_DELAY),
                tap_count: p.num("tapCount", 2.0).max(1.0) as u32,
            },
            _ => return None,
        })
    }

    /// Parse a list (`"hold(duration=1),tap"`); unknown names are skipped.
    pub fn parse_list(text: &str) -> Vec<Interaction> {
        parse_list(text)
            .iter()
            .filter_map(Interaction::from_params)
            .collect()
    }

    pub fn name(&self) -> &'static str {
        match self {
            Interaction::Press { .. } => "Press",
            Interaction::Hold { .. } => "Hold",
            Interaction::Tap { .. } => "Tap",
            Interaction::SlowTap { .. } => "SlowTap",
            Interaction::MultiTap { .. } => "MultiTap",
        }
    }

    fn press_point(&self) -> f32 {
        match self {
            Interaction::Press { press_point, .. }
            | Interaction::Hold { press_point, .. }
            | Interaction::Tap { press_point, .. }
            | Interaction::SlowTap { press_point, .. }
            | Interaction::MultiTap { press_point, .. } => *press_point,
        }
    }
}

/// Phase of an interaction (and of an action).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    Disabled,
    #[default]
    Waiting,
    Started,
    Performed,
    Canceled,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::Disabled => "Disabled",
            Phase::Waiting => "Waiting",
            Phase::Started => "Started",
            Phase::Performed => "Performed",
            Phase::Canceled => "Canceled",
        }
    }
}

/// A transition an interaction reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    Started,
    Performed,
    Canceled,
}

/// Running state of one interaction on one binding.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InteractionState {
    /// Waiting, Started or Performed (performed-and-staying, e.g. a hold still held).
    pub phase: Phase,
    /// Control past the press point (with release hysteresis).
    pressed: bool,
    /// When the current press began.
    press_time: f64,
    /// When the gesture started.
    pub start_time: f64,
    /// Multi-tap: taps completed so far and when the last one ended.
    taps: u32,
    last_release: f64,
}

impl InteractionState {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Take a control already held as pressed, without starting anything.
    pub fn prime(&mut self, it: &Interaction, magnitude: f32) {
        self.pressed = magnitude >= it.press_point();
    }

    /// Step with the control's `magnitude` at time `now` (seconds); returns the transitions
    /// in order (e.g. `[Started, Performed]` for a press).
    pub fn step(&mut self, it: &Interaction, magnitude: f32, now: f64) -> Vec<Transition> {
        let pp = it.press_point();
        let was = self.pressed;
        let pressed = if was {
            magnitude >= pp * RELEASE_RATIO
        } else {
            magnitude >= pp
        };
        self.pressed = pressed;
        let down = pressed && !was;
        let up = !pressed && was;
        if down {
            self.press_time = now;
        }
        let mut out = Vec::new();
        use Transition::*;
        match *it {
            Interaction::Press { behavior, .. } => match behavior {
                PressBehavior::PressOnly => {
                    if down {
                        self.start(now, &mut out);
                        out.push(Performed);
                        self.phase = Phase::Performed;
                    } else if up && self.phase != Phase::Waiting {
                        out.push(Canceled);
                        self.phase = Phase::Waiting;
                    }
                }
                PressBehavior::ReleaseOnly => {
                    if down {
                        self.start(now, &mut out);
                    } else if up && self.phase == Phase::Started {
                        out.push(Performed);
                        self.phase = Phase::Waiting;
                    }
                }
                PressBehavior::PressAndRelease => {
                    if down {
                        self.start(now, &mut out);
                        out.push(Performed);
                        self.phase = Phase::Performed;
                    } else if up && self.phase == Phase::Performed {
                        out.push(Performed);
                        self.phase = Phase::Waiting;
                    }
                }
            },
            Interaction::Hold { duration, .. } => {
                if down {
                    self.start(now, &mut out);
                }
                if self.phase == Phase::Started
                    && pressed
                    && now - self.press_time + EPSILON >= duration
                {
                    out.push(Performed);
                    self.phase = Phase::Performed;
                }
                if up && self.phase != Phase::Waiting {
                    out.push(Canceled);
                    self.phase = Phase::Waiting;
                }
            }
            Interaction::Tap { duration, .. } => {
                if down {
                    self.start(now, &mut out);
                }
                if self.phase == Phase::Started {
                    if up {
                        out.push(if now - self.press_time <= duration + EPSILON {
                            Performed
                        } else {
                            Canceled
                        });
                        self.phase = Phase::Waiting;
                    } else if now - self.press_time > duration + EPSILON {
                        // Held too long: no tap.
                        out.push(Canceled);
                        self.phase = Phase::Waiting;
                    }
                }
            }
            Interaction::SlowTap { duration, .. } => {
                if down {
                    self.start(now, &mut out);
                }
                if up && self.phase == Phase::Started {
                    out.push(if now - self.press_time + EPSILON >= duration {
                        Performed
                    } else {
                        Canceled
                    });
                    self.phase = Phase::Waiting;
                }
            }
            Interaction::MultiTap {
                tap_time,
                tap_delay,
                tap_count,
                ..
            } => {
                if down {
                    if self.phase == Phase::Waiting {
                        self.taps = 0;
                        self.start(now, &mut out);
                    }
                } else if self.phase == Phase::Started {
                    if up {
                        if now - self.press_time <= tap_time + EPSILON {
                            self.taps += 1;
                            self.last_release = now;
                            if self.taps >= tap_count {
                                out.push(Performed);
                                self.phase = Phase::Waiting;
                            }
                        } else {
                            out.push(Canceled);
                            self.phase = Phase::Waiting;
                        }
                    } else if pressed && now - self.press_time > tap_time + EPSILON {
                        // A tap held too long.
                        out.push(Canceled);
                        self.phase = Phase::Waiting;
                    } else if !pressed
                        && self.taps > 0
                        && now - self.last_release > tap_delay + EPSILON
                    {
                        // The next tap came too late.
                        out.push(Canceled);
                        self.phase = Phase::Waiting;
                    }
                }
            }
        }
        out
    }

    fn start(&mut self, now: f64, out: &mut Vec<Transition>) {
        if self.phase == Phase::Waiting {
            self.phase = Phase::Started;
            self.start_time = now;
            out.push(Transition::Started);
        }
    }
}
