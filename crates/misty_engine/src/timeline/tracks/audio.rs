//! Audio tracks: each clip plays its audio file through the bound Audio Source (its volume,
//! 3D settings and mixer output), starting at the right offset when playback jumps in.

use crate::timeline::asset::Track;
use crate::timeline::evaluate::local_time;

/// The clip under `t` and the seconds into its audio file.
pub fn current(track: &Track, t: f32) -> Option<(usize, f32)> {
    let i = track
        .clips
        .iter()
        .position(|c| t >= c.start && t < c.end() && c.asset.is_some())?;
    Some((i, local_time(&track.clips[i], t).max(0.0)))
}
