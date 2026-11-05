//! Timeline asset (`.timeline`, JSON): tracks of clips and markers laid out in time.
//!
//! A track's kind decides what its clips do (see [`TrackKind`]). Tracks keep a stable `id` that
//! Playable Directors use to bind them to scene objects; group tracks only hold other tracks
//! (by `group`), so muting or folding a group covers everything in it.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::animation::AnimationClip;

/// File extension of timeline assets.
pub const EXTENSION: &str = "timeline";

pub fn is_timeline(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

/// What a track does. New kinds (a camera track, say) get a variant here and a module in
/// `timeline::tracks`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrackKind {
    /// Holds other tracks; muting it mutes them.
    Group,
    /// The bound object is active only while a clip plays.
    #[default]
    Activation,
    /// Plays `.anim` clips on the bound object, blending where clips overlap.
    Animation,
    /// Plays audio clips through the bound Audio Source.
    Audio,
    /// Markers that call a script method on the bound object.
    Signal,
}

impl TrackKind {
    /// Add Track menu order.
    pub const ALL: [TrackKind; 5] = [
        Self::Activation,
        Self::Animation,
        Self::Audio,
        Self::Signal,
        Self::Group,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Group => "Track Group",
            Self::Activation => "Activation Track",
            Self::Animation => "Animation Track",
            Self::Audio => "Audio Track",
            Self::Signal => "Signal Track",
        }
    }

    /// Whether the track is bound to a scene object.
    pub fn bindable(self) -> bool {
        self != Self::Group
    }

    /// Whether the track holds clips (signal tracks hold markers instead).
    pub fn has_clips(self) -> bool {
        matches!(self, Self::Activation | Self::Animation | Self::Audio)
    }

    /// Component the bound object needs ("" = any object).
    pub fn binding_component(self) -> &'static str {
        match self {
            Self::Audio => "Audio Source",
            _ => "",
        }
    }
}

/// What an Activation track leaves when the director stops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PostPlayback {
    Active,
    Inactive,
    /// Back to the state it had before the timeline started.
    Revert,
    #[default]
    LeaveAsIs,
}

impl PostPlayback {
    pub const ALL: [PostPlayback; 4] =
        [Self::Active, Self::Inactive, Self::Revert, Self::LeaveAsIs];

    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Inactive => "Inactive",
            Self::Revert => "Revert",
            Self::LeaveAsIs => "Leave As Is",
        }
    }
}

/// What a clip does in the gap after it (until the next clip, or the end).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Extrapolation {
    #[default]
    None,
    /// Keep the clip's last frame.
    Hold,
}

/// Timeline length: from the last clip or marker, or set by hand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurationMode {
    #[default]
    BasedOnClips,
    FixedLength,
}

/// A block of time on a track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimelineClip {
    /// Display name ("Active", the clip's file name, …).
    pub name: String,
    /// Seconds on the timeline.
    pub start: f32,
    pub duration: f32,
    /// `.anim` (Animation tracks) or audio clip (Audio tracks); unused by Activation clips.
    pub asset: Option<String>,
    /// Seconds into the source asset at the clip's start.
    pub clip_in: f32,
    /// Playback speed of the source asset.
    pub speed: f32,
    /// Seconds the clip fades in from nothing (ignored where it overlaps the previous clip).
    pub ease_in: f32,
    /// Seconds the clip fades out (ignored where the next clip overlaps it).
    pub ease_out: f32,
    pub post_extrapolation: Extrapolation,
    /// Audio clips: volume of the voice (0–1).
    pub volume: f32,
}

impl Default for TimelineClip {
    fn default() -> Self {
        Self {
            name: String::new(),
            start: 0.0,
            duration: 1.0,
            asset: None,
            clip_in: 0.0,
            speed: 1.0,
            ease_in: 0.0,
            ease_out: 0.0,
            post_extrapolation: Extrapolation::None,
            volume: 1.0,
        }
    }
}

impl TimelineClip {
    pub fn end(&self) -> f32 {
        self.start + self.duration
    }
}

/// A point in time that calls `method` on the scripts of the track's bound object (or the
/// director's own object for timeline markers and unbound tracks).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Marker {
    pub time: f32,
    /// Script method to call (`SendMessage`).
    pub method: String,
    /// Optional text passed to the method.
    pub parameter: Option<String>,
    /// Also fires when playback starts after the marker's time.
    pub retroactive: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    /// Stable id the director's bindings refer to ("t1", "t2", …).
    pub id: String,
    pub name: String,
    pub kind: TrackKind,
    /// Id of the group track this track sits in.
    pub group: Option<String>,
    pub muted: bool,
    /// Locked tracks can't be edited in the Timeline window.
    pub locked: bool,
    /// Group tracks: children hidden in the Timeline window.
    pub collapsed: bool,
    pub clips: Vec<TimelineClip>,
    /// Signal tracks.
    pub markers: Vec<Marker>,
    /// Activation tracks.
    pub post_playback: PostPlayback,
    /// Animation tracks: keys recorded straight onto the track, played when it has no clips.
    pub recorded: Option<AnimationClip>,
}

impl Default for Track {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            kind: TrackKind::Activation,
            group: None,
            muted: false,
            locked: false,
            collapsed: false,
            clips: Vec::new(),
            markers: Vec::new(),
            post_playback: PostPlayback::LeaveAsIs,
            recorded: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimelineAsset {
    /// Frames per second for snapping and the frame counter.
    pub frame_rate: f32,
    pub duration_mode: DurationMode,
    /// Length with Fixed Length.
    pub fixed_duration: f32,
    pub tracks: Vec<Track>,
    /// The markers row above the tracks (sent to the director's object).
    pub markers: Vec<Marker>,
}

impl Default for TimelineAsset {
    fn default() -> Self {
        Self {
            frame_rate: 60.0,
            duration_mode: DurationMode::BasedOnClips,
            fixed_duration: 5.0,
            tracks: Vec::new(),
            markers: Vec::new(),
        }
    }
}

impl TimelineAsset {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut t: Self = serde_json::from_str(&crate::asset_db::resolve_json(text))
            .map_err(|e| e.to_string())?;
        t.sanitize();
        Ok(t)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, crate::asset_db::embed_json(&self.to_json()))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Fix what a hand-edited file may get wrong: missing ids, bad numbers, unsorted clips.
    fn sanitize(&mut self) {
        if !(self.frame_rate.is_finite() && self.frame_rate > 0.0) {
            self.frame_rate = 60.0;
        }
        let mut seen = std::collections::HashSet::new();
        for i in 0..self.tracks.len() {
            if self.tracks[i].id.is_empty() || !seen.insert(self.tracks[i].id.clone()) {
                let id = self.next_track_id();
                seen.insert(id.clone());
                self.tracks[i].id = id;
            }
        }
        for t in &mut self.tracks {
            for c in &mut t.clips {
                c.duration = finite(c.duration, 1.0).max(0.0);
                c.start = finite(c.start, 0.0).max(0.0);
                c.speed = finite(c.speed, 1.0);
                if c.speed.abs() < 1e-3 {
                    c.speed = 1.0;
                }
                c.ease_in = finite(c.ease_in, 0.0).clamp(0.0, c.duration);
                c.ease_out = finite(c.ease_out, 0.0).clamp(0.0, c.duration);
            }
            t.sort_clips();
        }
    }

    /// Seconds the timeline lasts.
    pub fn duration(&self) -> f32 {
        if self.duration_mode == DurationMode::FixedLength {
            return self.fixed_duration.max(0.0);
        }
        let clips = self
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.end()));
        let markers = self
            .tracks
            .iter()
            .flat_map(|t| t.markers.iter())
            .chain(&self.markers)
            .map(|m| m.time);
        clips.chain(markers).fold(0.0, f32::max)
    }

    /// An id no track uses yet.
    pub fn next_track_id(&self) -> String {
        let n = self
            .tracks
            .iter()
            .filter_map(|t| t.id.strip_prefix('t')?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("t{}", n + 1)
    }

    pub fn track(&self, id: &str) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn track_mut(&mut self, id: &str) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    /// Add a track of `kind` (inside `group`, after its last child) and return its id.
    pub fn add_track(&mut self, kind: TrackKind, group: Option<&str>) -> String {
        let id = self.next_track_id();
        let base = match kind {
            TrackKind::Group => "Track Group",
            TrackKind::Activation => "Activation Track",
            TrackKind::Animation => "Animation Track",
            TrackKind::Audio => "Audio Track",
            TrackKind::Signal => "Signal Track",
        };
        let taken = |n: &str| self.tracks.iter().any(|t| t.name == n);
        let name = (0..)
            .map(|i| match i {
                0 => base.to_owned(),
                i => format!("{base} ({i})"),
            })
            .find(|n| !taken(n))
            .unwrap_or_default();
        let group = group
            .filter(|g| self.track(g).is_some_and(|t| t.kind == TrackKind::Group))
            .map(str::to_owned);
        let track = Track {
            id: id.clone(),
            name,
            kind,
            group: if kind == TrackKind::Group {
                None
            } else {
                group.clone()
            },
            ..Default::default()
        };
        let at = match &group {
            Some(g) => self
                .tracks
                .iter()
                .rposition(|t| t.id == *g || t.group.as_deref() == Some(g))
                .map_or(self.tracks.len(), |i| i + 1),
            None => self.tracks.len(),
        };
        self.tracks.insert(at, track);
        id
    }

    /// Remove a track; a group takes its tracks with it.
    pub fn remove_track(&mut self, id: &str) {
        self.tracks
            .retain(|t| t.id != id && t.group.as_deref() != Some(id));
    }

    /// Muted itself or through its group.
    pub fn is_muted(&self, track: &Track) -> bool {
        track.muted
            || track
                .group
                .as_deref()
                .and_then(|g| self.track(g))
                .is_some_and(|g| g.muted)
    }

    /// Locked itself or through its group.
    pub fn is_locked(&self, track: &Track) -> bool {
        track.locked
            || track
                .group
                .as_deref()
                .and_then(|g| self.track(g))
                .is_some_and(|g| g.locked)
    }

    /// Snap a time to the nearest frame.
    pub fn snap(&self, time: f32) -> f32 {
        (time * self.frame_rate).round() / self.frame_rate
    }
}

impl Track {
    /// Keep clips in start order (blending looks at neighbours).
    pub fn sort_clips(&mut self) {
        self.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    }
}

fn finite(v: f32, fallback: f32) -> f32 {
    if v.is_finite() { v } else { fallback }
}
