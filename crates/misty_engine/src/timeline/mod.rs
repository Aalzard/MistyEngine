//! Timeline: sequencing cutscenes. A `.timeline` asset lays out tracks of clips and markers in
//! time; a Playable Director component plays it in a scene, with each track bound to the object
//! it drives.
//!
//! - [`asset`]: the file format (tracks, clips, markers, groups, mute / lock).
//! - [`evaluate`]: clip weights (eases, blends between overlapping clips) and posing the scene
//!   at a time, shared by Play mode and the editor's preview.
//! - [`tracks`]: one module per track kind (Activation, Animation, Audio, Signal, Camera,
//!   Control).
//! - [`director`]: Play-mode playback (wrap modes, pause, signals, audio, sub-timelines,
//!   camera overrides, `played` / `stopped`).

pub mod asset;
pub mod director;
pub mod evaluate;
pub mod tracks;

pub use asset::{
    ControlOptions, DurationMode, EXTENSION, Extrapolation, Marker, PostPlayback, TimelineAsset,
    TimelineClip, Track, TrackKind, is_timeline,
};
pub use director::{DirectorEvent, DirectorPlayer, PlayState};
pub use evaluate::{Baseline, Bindings, ClipSource, Resolver};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_r19;
