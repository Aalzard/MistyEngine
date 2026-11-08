//! Where voices go: Audio Source Output → mixer group tracks (loaded on first use), each voice's
//! own filter / reverb-zone track, and the per-frame mixing context (listener, Doppler, zones).

use std::collections::HashMap;
use std::path::Path;

use glam::{Mat4, Vec3};
use hecs::Entity;

use super::effects::{EffectKind, EffectSpec};
use super::mixer::AudioMixer;
use super::{Audio, AudioBackend, MixerState, TrackId, VoiceParams, listener, spatial};
use crate::scene::{
    AudioEchoFilter, AudioHighPassFilter, AudioLowPassFilter, AudioReverbZone, AudioSource,
    ReverbPreset, Scene,
};

/// `"Assets/Audio/Main.mixer#SFX"`: how an Audio Source names its Output group.
pub fn output_ref(mixer: &str, group: &str) -> String {
    format!("{mixer}#{group}")
}

/// Mixer path and group name of an Output (no `#` = the mixer's Master).
pub fn split_output(output: &str) -> (&str, &str) {
    output.split_once('#').unwrap_or((output, "Master"))
}

/// An effect on a voice's own track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Slot {
    LowPass,
    HighPass,
    Echo,
    /// Reverb of the zone the listener is in.
    Zone,
}

/// A voice's way out: its mixer group and its own effect track (if it has filters).
#[derive(Default)]
pub(super) struct Route {
    group: Option<(usize, u32)>,
    own: Option<TrackId>,
    slots: Vec<Slot>,
}

impl Route {
    /// Track the voice plays on.
    pub fn target(&self, mixers: &[MixerState]) -> Option<TrackId> {
        self.own.or_else(|| {
            let (m, g) = self.group?;
            mixers.get(m)?.tracks.get(&g).copied()
        })
    }
}

/// What mixing a frame needs besides the voices themselves.
pub(super) struct Ctx<'a> {
    pub ear: Option<Mat4>,
    ear_pos: Option<Vec3>,
    ear_vel: Vec3,
    dt: f32,
    pub muted: bool,
    pub mixers: &'a [MixerState],
    last_pos: Option<&'a HashMap<Entity, Vec3>>,
    /// Strongest reverb zone at the listener: weight and preset.
    zone: Option<(f32, ReverbPreset)>,
}

impl<'a> Ctx<'a> {
    pub fn new(scene: &Scene, mixers: &'a [MixerState], muted: bool) -> Self {
        let ear = listener(scene);
        let ear_pos = ear.map(|m| m.w_axis.truncate());
        Self {
            ear,
            ear_pos,
            ear_vel: Vec3::ZERO,
            dt: 0.0,
            muted,
            mixers,
            last_pos: None,
            zone: ear_pos.and_then(|p| strongest_zone(scene, p)),
        }
    }

    /// With velocities from last frame's positions (Doppler).
    pub fn moving(
        scene: &Scene,
        mixers: &'a [MixerState],
        muted: bool,
        dt: f32,
        last_ear: Option<Vec3>,
        last_pos: &'a HashMap<Entity, Vec3>,
    ) -> Self {
        let mut c = Self::new(scene, mixers, muted);
        if dt > 0.0 {
            c.dt = dt;
            c.last_pos = Some(last_pos);
            if let (Some(now), Some(before)) = (c.ear_pos, last_ear) {
                c.ear_vel = (now - before) / dt;
            }
        }
        c
    }

    /// Volume, pitch and pan of a source voice: spatial mix, mixer pitch and solo, Doppler.
    pub fn source_params(
        &self,
        scene: &Scene,
        e: Entity,
        src: &AudioSource,
        route: &Route,
    ) -> VoiceParams {
        let at = scene.world_position(e);
        let mut p = spatial::mix(src, at, self.ear, self.muted);
        if let Some((m, g)) = route.group
            && let Some(mixer) = self.mixers.get(m)
        {
            p.pitch *= nested_pitch(self.mixers, m, g);
            if !mixer.solo_passes(g) {
                p.volume = 0.0;
            }
        }
        if let (Some(ear), Some(last)) = (self.ear_pos, self.last_pos.and_then(|l| l.get(&e))) {
            let vel = (at - *last) / self.dt;
            let level = src.doppler_level.clamp(0.0, 5.0) * src.spatial_blend.clamp(0.0, 1.0);
            p.pitch *= spatial::doppler(ear, self.ear_vel, at, vel, level);
        }
        p
    }

    /// Current values of a voice's own effects, in slot order.
    pub fn own_specs(
        &self,
        scene: &Scene,
        e: Entity,
        src: &AudioSource,
        slots: &[Slot],
    ) -> Vec<EffectSpec> {
        let w = &scene.world;
        slots
            .iter()
            .map(|slot| {
                let mut spec = match slot {
                    Slot::LowPass => w.get::<&AudioLowPassFilter>(e).ok().map(|f| f.spec()),
                    Slot::HighPass => w.get::<&AudioHighPassFilter>(e).ok().map(|f| f.spec()),
                    Slot::Echo => w.get::<&AudioEchoFilter>(e).ok().map(|f| f.spec()),
                    Slot::Zone => Some(self.zone_spec(src)),
                }
                .unwrap_or_else(|| removed(*slot));
                if *slot != Slot::Zone {
                    spec.bypass |= src.bypass_effects;
                }
                spec
            })
            .collect()
    }

    fn zone_spec(&self, src: &AudioSource) -> EffectSpec {
        let (weight, preset) = self.zone.unwrap_or((0.0, ReverbPreset::Generic));
        let mut v = preset.values().to_vec();
        v[3] *= weight * src.reverb_zone_mix.clamp(0.0, 1.1);
        let mut spec = EffectSpec::new(EffectKind::Reverb, v);
        spec.bypass = src.bypass_reverb_zones;
        spec
    }

    /// Update a voice's own track.
    pub fn push_own(
        &self,
        b: &mut dyn AudioBackend,
        scene: &Scene,
        e: Entity,
        src: &AudioSource,
        route: &Route,
    ) {
        if let Some(t) = route.own {
            b.set_track(t, 1.0, &self.own_specs(scene, e, src, &route.slots));
        }
    }

    /// Listener and source positions to remember for next frame's velocities.
    pub fn positions(
        &self,
        scene: &Scene,
        sources: impl Iterator<Item = Entity>,
    ) -> (Option<Vec3>, HashMap<Entity, Vec3>) {
        let pos = sources.map(|e| (e, scene.world_position(e))).collect();
        (self.ear_pos, pos)
    }
}

/// Pitch of group `g` of mixer `m`, times the pitch of the groups a nested mixer plays into.
fn nested_pitch(mixers: &[MixerState], m: usize, g: u32) -> f32 {
    let mut pitch = 1.0;
    let mut at = Some((m, g));
    for _ in 0..8 {
        let Some((m, g)) = at else { break };
        let Some(mixer) = mixers.get(m) else { break };
        pitch *= mixer.pitch(g);
        at = mixer.output_group;
    }
    pitch
}

/// A slot whose component was removed while playing: passes the sound through.
fn removed(slot: Slot) -> EffectSpec {
    let kind = match slot {
        Slot::LowPass => EffectKind::Lowpass,
        Slot::HighPass => EffectKind::Highpass,
        Slot::Echo => EffectKind::Echo,
        Slot::Zone => EffectKind::Reverb,
    };
    let mut spec = EffectSpec::new(kind, kind.defaults());
    spec.bypass = true;
    spec
}

/// The enabled zone the listener at `ear` hears most of.
fn strongest_zone(scene: &Scene, ear: Vec3) -> Option<(f32, ReverbPreset)> {
    scene
        .world
        .query::<(Entity, &AudioReverbZone)>()
        .iter()
        .filter(|(e, z)| z.enabled && scene.active_in_hierarchy(*e))
        .map(|(e, z)| (z.weight(scene.world_position(e).distance(ear)), z.preset))
        .max_by(|a, b| a.0.total_cmp(&b.0))
}

/// Effects a source needs its own track for (filters on its object, reverb zones in the scene).
fn own_slots(scene: &Scene, e: Entity, src: &AudioSource) -> Vec<Slot> {
    let w = &scene.world;
    let mut slots = Vec::new();
    if w.get::<&AudioLowPassFilter>(e).is_ok() {
        slots.push(Slot::LowPass);
    }
    if w.get::<&AudioHighPassFilter>(e).is_ok() {
        slots.push(Slot::HighPass);
    }
    if w.get::<&AudioEchoFilter>(e).is_ok() {
        slots.push(Slot::Echo);
    }
    let zones = w.query::<&AudioReverbZone>().iter().next().is_some();
    if zones && !src.bypass_reverb_zones {
        slots.push(Slot::Zone);
    }
    slots
}

/// Project-relative paths of every `.mixer` under `Assets/`.
pub fn project_mixers(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for p in crate::vfs::files_under(&root.join("Assets")) {
        if let Ok(rel) = p.strip_prefix(root) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            if super::is_mixer(&rel) {
                out.push(rel);
            }
        }
    }
    out
}

impl Audio {
    /// Route for a new voice of `src`: its Output group and, if needed, its own effect track.
    pub(super) fn route(&mut self, scene: &Scene, e: Entity, src: &AudioSource) -> Route {
        let group = src.output.as_deref().and_then(|o| self.resolve_output(o));
        let slots = own_slots(scene, e, src);
        let mut route = Route {
            group,
            own: None,
            slots,
        };
        if !route.slots.is_empty() {
            let parent = route.target(&self.mixers);
            let specs = self.ctx(scene).own_specs(scene, e, src, &route.slots);
            if let Some(b) = self.backend.as_mut() {
                route.own = b.add_track(parent, &specs).ok();
            }
        }
        route
    }

    pub(super) fn drop_route(&mut self, route: Route) {
        if let (Some(t), Some(b)) = (route.own, self.backend.as_mut()) {
            b.remove_track(t);
        }
    }

    /// Mixer and group id of an Output; warns once when it can't be found.
    fn resolve_output(&mut self, output: &str) -> Option<(usize, u32)> {
        let (path, group) = split_output(output);
        let found = self.load_mixer(path).and_then(|m| {
            let g = self.mixers[m].asset.group_by_name(group)?.id;
            Some((m, g))
        });
        if found.is_none() && self.bad_clips.insert(output.to_owned()) {
            self.logs.push(format!(
                "Audio Source output {output} not found; playing without a mixer"
            ));
        }
        if let Some((m, _)) = found {
            self.ensure_tracks(m);
        }
        found
    }

    /// Build mixer `m`'s tracks, first those of the mixer its Output plays into.
    fn ensure_tracks(&mut self, m: usize) {
        self.ensure_tracks_nested(m, 0);
    }

    fn ensure_tracks_nested(&mut self, m: usize, depth: usize) {
        if !self.mixers[m].tracks.is_empty() {
            return;
        }
        let output = self.resolve_nested_output(m, depth);
        self.mixers[m].output_group = output;
        self.mixers[m].output_track =
            output.and_then(|(o, g)| self.mixers[o].tracks.get(&g).copied());
        if let Some(b) = self.backend.as_mut() {
            self.mixers[m].ensure_tracks(b.as_mut(), &mut self.logs);
        }
    }

    /// Mixer index and group the Output of mixer `m` names (its tracks built), or `None`
    /// (no output, not found, or a loop of mixers: warned once).
    fn resolve_nested_output(&mut self, m: usize, depth: usize) -> Option<(usize, u32)> {
        let output = self.mixers[m].asset.output.clone()?;
        let (path, group) = split_output(&output);
        let path = path.to_owned();
        let found = self.load_mixer(&path).and_then(|o| {
            let g = self.mixers[o].asset.group_by_name(group)?.id;
            Some((o, g))
        });
        let looped = found.is_some_and(|(o, _)| o == m || self.feeds_into(o, m)) || depth > 8;
        match found {
            Some((o, g)) if !looped => {
                self.ensure_tracks_nested(o, depth + 1);
                Some((o, g))
            }
            _ => {
                if self.bad_clips.insert(output.clone()) {
                    let why = if looped { "would loop" } else { "not found" };
                    self.logs.push(format!(
                        "Audio Mixer {} output {output} {why}; playing straight out",
                        self.mixers[m].path
                    ));
                }
                None
            }
        }
    }

    /// Whether mixer `a` plays (through nested outputs) into mixer `b`.
    fn feeds_into(&self, a: usize, b: usize) -> bool {
        let mut at = self.mixers.get(a).and_then(|x| x.output_group);
        for _ in 0..16 {
            match at {
                Some((o, _)) if o == b => return true,
                Some((o, _)) => at = self.mixers.get(o).and_then(|x| x.output_group),
                None => return false,
            }
        }
        true
    }

    /// Index of the mixer at `path` (project-relative), loading it on first use.
    pub fn load_mixer(&mut self, path: &str) -> Option<usize> {
        if let Some(i) = self.mixers.iter().position(|m| m.path == path) {
            return Some(i);
        }
        match AudioMixer::load(&self.root.join(path)) {
            Ok(asset) => {
                self.mixers.push(MixerState::new(path.to_owned(), asset));
                Some(self.mixers.len() - 1)
            }
            Err(e) => {
                if self.bad_clips.insert(path.to_owned()) {
                    self.logs.push(format!("Audio Mixer {path}: {e}"));
                }
                None
            }
        }
    }

    /// `AudioMixer.Find(name)`: a loaded mixer with that file name, else the first `.mixer`
    /// under `Assets/` called `name`.
    pub fn find_mixer(&mut self, name: &str) -> Option<usize> {
        if let Some(i) = self.mixers.iter().position(|m| m.name() == name) {
            return Some(i);
        }
        let path = project_mixers(&self.root)
            .into_iter()
            .find(|p| crate::scene::clip_name(p) == name)?;
        self.load_mixer(&path)
    }

    /// Output reference of the group called `name`: in mixer `prefer` first, then in loaded
    /// mixers, then in every project mixer.
    pub fn find_group(&mut self, name: &str, prefer: Option<&str>) -> Option<String> {
        let mut candidates: Vec<String> = prefer.map(str::to_owned).into_iter().collect();
        candidates.extend(self.mixers.iter().map(|m| m.path.clone()));
        candidates.extend(project_mixers(&self.root));
        for path in candidates {
            if let Some(m) = self.load_mixer(&path)
                && self.mixers[m].asset.group_by_name(name).is_some()
            {
                return Some(output_ref(&path, name));
            }
        }
        None
    }

    pub fn mixer(&self, i: usize) -> Option<&MixerState> {
        self.mixers.get(i)
    }

    pub fn mixer_mut(&mut self, i: usize) -> Option<&mut MixerState> {
        self.mixers.get_mut(i)
    }

    /// The loaded mixer at `path`, if any.
    pub fn mixer_at(&self, path: &str) -> Option<&MixerState> {
        self.mixers.iter().find(|m| m.path == path)
    }

    /// Peak level (linear) of each group of the mixer at `path` since the last call.
    pub fn mixer_peaks(&mut self, path: &str) -> HashMap<u32, f32> {
        let Some(m) = self.mixers.iter().find(|m| m.path == path) else {
            return HashMap::new();
        };
        let Some(b) = self.backend.as_mut() else {
            return HashMap::new();
        };
        m.tracks
            .iter()
            .map(|(g, t)| (*g, b.track_peak(*t)))
            .collect()
    }

    /// Strongest gain reduction (dB, ≤ 0) of each group's Compressor / Duck Volume since the
    /// last call (for the Mixer window's ducking readout).
    pub fn mixer_reductions(&mut self, path: &str) -> HashMap<u32, f32> {
        let Some(m) = self.mixers.iter().find(|m| m.path == path) else {
            return HashMap::new();
        };
        let Some(b) = self.backend.as_mut() else {
            return HashMap::new();
        };
        m.tracks
            .iter()
            .map(|(g, t)| (*g, b.track_reduction(*t)))
            .collect()
    }

    /// The Mixer window edited a loaded mixer while playing: use the new values now. Groups and
    /// effects added, removed, moved or rerouted change the running graph in place (crossfaded);
    /// nothing routed there restarts. Voices of a removed group move to its nearest surviving
    /// parent.
    pub fn replace_mixer(&mut self, path: &str, asset: AudioMixer) {
        let Some(i) = self.mixers.iter().position(|m| m.path == path) else {
            return;
        };
        let old = self.mixers[i].asset.clone();
        let changed = self.mixers[i].replace_asset(asset);
        if self.mixers[i].tracks.is_empty() {
            return;
        }
        if changed {
            self.move_orphans(i, &old);
            if let Some(b) = self.backend.as_mut() {
                self.mixers[i].sync_layout(b.as_mut(), &old);
            }
        }
        if old.output != self.mixers[i].asset.output {
            self.mixers[i].output_group = None;
            let output = self.resolve_nested_output(i, 0);
            let track = output.and_then(|(o, g)| self.mixers[o].tracks.get(&g).copied());
            self.mixers[i].output_group = output;
            if let Some(b) = self.backend.as_mut() {
                self.mixers[i].set_output(b.as_mut(), track);
            }
        }
    }

    /// Voices on groups of mixer `i` that the edit removed: onto the nearest surviving parent.
    fn move_orphans(&mut self, i: usize, old: &AudioMixer) {
        let m = &self.mixers[i];
        let mut moves: Vec<(VoiceIdOrTrack, u32)> = Vec::new();
        let routes = self
            .sources
            .values_mut()
            .map(|v| (v.id, &mut v.route))
            .chain(self.one_shots.iter_mut().map(|o| (o.id, &mut o.route)));
        for (voice, route) in routes {
            if let Some((mi, g)) = route.group
                && mi == i
                && m.asset.group(g).is_none()
            {
                let to = m.survivor(old, g);
                route.group = Some((i, to));
                let what = match route.own {
                    Some(own) => VoiceIdOrTrack::Track(own),
                    None => VoiceIdOrTrack::Voice(voice),
                };
                moves.push((what, to));
            }
        }
        let Some(b) = self.backend.as_mut() else {
            return;
        };
        for (what, to) in moves {
            let track = self.mixers[i].tracks.get(&to).copied();
            match what {
                VoiceIdOrTrack::Voice(v) => b.move_voice(v, track),
                VoiceIdOrTrack::Track(t) => b.set_parent(t, track),
            }
        }
    }
}

/// What to reroute when a voice's group goes: the voice, or its own filter track.
enum VoiceIdOrTrack {
    Voice(super::VoiceId),
    Track(TrackId),
}
