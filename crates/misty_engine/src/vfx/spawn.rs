//! The Spawn context runs on the CPU: how many particles each step emits from constant rates,
//! single bursts and periodic bursts. Deterministic for a given sequence of steps.

use super::blocks::{BlockKind, spawn_ports};
use super::codegen::{CpuExpr, SpawnBlock};
use super::graph::{EffectProperty, PropertyValue};

/// Spawn system state of one instance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpawnState {
    /// Seconds since Play.
    pub time: f32,
    /// Fractional particles carried to the next step, per block.
    pub carry: Vec<f32>,
}

/// Property values the spawn inputs read: overrides first, then the graph's defaults.
pub struct Props<'a> {
    pub defaults: &'a [EffectProperty],
    pub overrides: &'a [(String, PropertyValue)],
}

impl Props<'_> {
    fn float(&self, name: &str) -> f32 {
        let pick = |v: &PropertyValue| match v {
            PropertyValue::Float(f) => Some(*f),
            _ => None,
        };
        self.overrides
            .iter()
            .filter(|(n, _)| n == name)
            .find_map(|(_, v)| pick(v))
            .or_else(|| {
                self.defaults
                    .iter()
                    .filter(|p| p.name == name)
                    .find_map(|p| pick(&p.value))
            })
            .unwrap_or(0.0)
    }
}

pub fn eval(e: &CpuExpr, props: &Props) -> f32 {
    match e {
        CpuExpr::Const(v) => *v,
        CpuExpr::Property(name) => props.float(name),
        CpuExpr::Binary(op, a, b) => {
            let (a, b) = (eval(a, props), eval(b, props));
            match op {
                '+' => a + b,
                '-' => a - b,
                '*' => a * b,
                _ if b != 0.0 => a / b,
                _ => 0.0,
            }
        }
    }
}

/// Bursts scheduled at `delay + k × period` (k ≥ 0) inside `[from, to)`.
fn periodic_hits(delay: f32, period: f32, from: f32, to: f32) -> u32 {
    if to <= delay {
        return 0;
    }
    let period = period.max(0.001);
    // Number of burst times ≤ t, for t ≥ delay: floor((t - delay) / period) + 1.
    let upto = |t: f32| -> i64 {
        if t < delay {
            0
        } else {
            ((t - delay) / period).floor() as i64 + 1
        }
    };
    // Times in [from, to) = (count < to) - (count < from); "< t" = upto(t - eps).
    let below = |t: f32| upto(t - 1e-6);
    (below(to) - below(from)).max(0) as u32
}

/// Advance the spawn system by `dt` (while playing) and return the particles to emit.
pub fn step(blocks: &[SpawnBlock], state: &mut SpawnState, props: &Props, dt: f32) -> u32 {
    state.carry.resize(blocks.len(), 0.0);
    let (from, to) = (state.time, state.time + dt.max(0.0));
    let mut total = 0u32;
    for (i, b) in blocks.iter().enumerate() {
        let input = |name: &str| b.input(name).map_or(0.0, |e| eval(e, props));
        let n = match b.kind {
            BlockKind::ConstantRate => {
                let want = input(spawn_ports::RATE).max(0.0) * (to - from) + state.carry[i];
                // A little slack so 0.5 + 0.5 in floats still makes one particle.
                let n = (want + 1e-4).floor();
                state.carry[i] = (want - n).max(0.0);
                n as u32
            }
            BlockKind::SingleBurst => {
                let delay = input(spawn_ports::DELAY).max(0.0);
                let hit = delay >= from && delay < to;
                if hit {
                    input(spawn_ports::COUNT).max(0.0).round() as u32
                } else {
                    0
                }
            }
            BlockKind::PeriodicBurst => {
                let delay = input(spawn_ports::DELAY).max(0.0);
                let period = input(spawn_ports::PERIOD);
                let hits = periodic_hits(delay, period, from, to);
                // Saturating: 1000 hits (Period 0.001) × Count 5e6 wrapped to a small count.
                hits.saturating_mul(input(spawn_ports::COUNT).max(0.0).round() as u32)
            }
            _ => 0,
        };
        total = total.saturating_add(n);
    }
    state.time = to;
    total
}
