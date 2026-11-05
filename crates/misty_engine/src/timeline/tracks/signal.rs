//! Signal tracks and the timeline's markers row: a marker calls its method once when
//! playback passes it.

use crate::timeline::asset::Marker;

/// Whether playback moving from `prev` to `cur` passes `m`. On the first frame of a play
/// (`fresh`), retroactive markers before the start fire too.
pub fn fires(m: &Marker, prev: f32, cur: f32, fresh: bool) -> bool {
    (prev < m.time && m.time <= cur) || (fresh && m.retroactive && m.time <= prev)
}
