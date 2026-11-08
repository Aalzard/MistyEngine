//! A mixer while playing: current snapshot values (with transitions, also blends of several
//! weighted snapshots), script overrides of exposed parameters, and the backend tracks its
//! groups play on. Edits to the asset while playing are applied to the tracks in place.

use std::collections::HashMap;

use super::backend::{AudioBackend, TrackId};
use super::effects::{EffectKind, EffectSpec, WET, db_to_amplitude};
use super::mixer::{AudioMixer, MASTER, effect_key, pitch_key, volume_key};

/// A group's id, parent and effect chain (ids and kinds).
pub type GroupLayout = (u32, Option<u32>, Vec<(u32, EffectKind)>);

struct Transition {
    from: HashMap<String, f32>,
    to: HashMap<String, f32>,
    elapsed: f32,
    duration: f32,
}

pub struct MixerState {
    /// Project-relative `.mixer` path.
    pub path: String,
    pub asset: AudioMixer,
    /// Snapshot most recently transitioned to (the heaviest one of a blend).
    pub snapshot: usize,
    /// Weights of the blend most recently transitioned to (snapshot index, weight).
    pub weights: Vec<(usize, f32)>,
    /// Values right now, by key.
    current: HashMap<String, f32>,
    transition: Option<Transition>,
    /// `SetFloat` values by exposed parameter name.
    overrides: HashMap<String, f32>,
    /// Backend track of each group (built on first use).
    pub tracks: HashMap<u32, TrackId>,
    /// Nested mixer: the track (a group of another mixer) Master plays into.
    pub output_track: Option<TrackId>,
    /// Nested mixer: index of that mixer and the group.
    pub output_group: Option<(usize, u32)>,
}

impl MixerState {
    pub fn new(path: String, asset: AudioMixer) -> Self {
        let snapshot = asset.start_snapshot;
        let mut s = Self {
            path,
            asset,
            snapshot,
            weights: vec![(snapshot, 1.0)],
            current: HashMap::new(),
            transition: None,
            overrides: HashMap::new(),
            tracks: HashMap::new(),
            output_track: None,
            output_group: None,
        };
        s.current = s.blend(&[(snapshot, 1.0)]);
        s
    }

    /// File stem ("Main" for `Assets/Audio/Main.mixer`).
    pub fn name(&self) -> String {
        crate::scene::clip_name(&self.path)
    }

    /// Every key's value in a weighted blend of snapshots: `Σ wᵢ·vᵢ / Σ wᵢ` (weights below
    /// zero count as zero; all zero keeps the current values).
    pub fn blend(&self, weights: &[(usize, f32)]) -> HashMap<String, f32> {
        let valid: Vec<(usize, f32)> = weights
            .iter()
            .filter(|(i, w)| *i < self.asset.snapshots.len() && *w > 0.0)
            .copied()
            .collect();
        let total: f32 = valid.iter().map(|(_, w)| w).sum();
        self.asset
            .keys()
            .into_iter()
            .map(|k| {
                let v = if total > 0.0 {
                    valid
                        .iter()
                        .map(|(i, w)| self.asset.value(*i, &k) * w)
                        .sum::<f32>()
                        / total
                } else {
                    self.current
                        .get(&k)
                        .copied()
                        .unwrap_or_else(|| self.asset.value(self.snapshot, &k))
                };
                (k, v)
            })
            .collect()
    }

    /// Value of `key` now: a script override of an exposed parameter wins over the snapshot.
    pub fn value(&self, key: &str) -> f32 {
        self.asset
            .exposed
            .iter()
            .filter(|p| p.key == key)
            .find_map(|p| self.overrides.get(&p.name))
            .or_else(|| self.current.get(key))
            .copied()
            .or_else(|| self.asset.default_value(key))
            .unwrap_or(0.0)
    }

    /// Every current value (for the Mixer window's live view).
    pub fn values(&self) -> HashMap<String, f32> {
        self.asset
            .keys()
            .into_iter()
            .map(|k| {
                let v = self.value(&k);
                (k, v)
            })
            .collect()
    }

    /// `snapshot:TransitionTo(time)`: blend linearly to `snap` over `time` seconds.
    pub fn transition_to(&mut self, snap: usize, time: f32) {
        if snap < self.asset.snapshots.len() {
            self.transition_to_weighted(&[(snap, 1.0)], time);
        }
    }

    /// `mixer:TransitionToSnapshots(snapshots, weights, time)`: move linearly, over `time`
    /// seconds, to the weighted blend of several snapshots.
    pub fn transition_to_weighted(&mut self, weights: &[(usize, f32)], time: f32) {
        let valid: Vec<(usize, f32)> = weights
            .iter()
            .filter(|(i, _)| *i < self.asset.snapshots.len())
            .map(|(i, w)| (*i, w.max(0.0)))
            .collect();
        if valid.is_empty() {
            return;
        }
        if let Some((i, _)) = valid.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
            self.snapshot = *i;
        }
        let target = self.blend(&valid);
        self.weights = valid;
        if time <= 0.0 {
            self.transition = None;
            self.current = target;
        } else {
            self.transition = Some(Transition {
                from: self.current.clone(),
                to: target,
                elapsed: 0.0,
                duration: time,
            });
        }
    }

    /// Advance a running transition.
    pub fn tick(&mut self, dt: f32) {
        let Some(t) = self.transition.as_mut() else {
            return;
        };
        t.elapsed += dt.max(0.0);
        let k = (t.elapsed / t.duration).clamp(0.0, 1.0);
        self.current =
            t.to.iter()
                .map(|(key, b)| {
                    let a = t.from.get(key).copied().unwrap_or(*b);
                    (key.clone(), a + (b - a) * k)
                })
                .collect();
        if k >= 1.0 {
            self.transition = None;
        }
    }

    /// `mixer:SetFloat(name, value)`; false if no parameter has that name.
    pub fn set_float(&mut self, name: &str, value: f32) -> bool {
        let Some(key) = self.asset.exposed_key(name) else {
            return false;
        };
        let (lo, hi) = self.asset.range(key);
        self.overrides.insert(name.to_owned(), value.clamp(lo, hi));
        true
    }

    /// `mixer:GetFloat(name)`.
    pub fn get_float(&self, name: &str) -> Option<f32> {
        self.asset.exposed_key(name).map(|k| self.value(k))
    }

    /// `mixer:ClearFloat(name)`: back to the snapshot's value.
    pub fn clear_float(&mut self, name: &str) -> bool {
        self.overrides.remove(name).is_some()
    }

    /// Group pitch multiplied down from Master (applied to every voice routed there).
    pub fn pitch(&self, group: u32) -> f32 {
        self.asset
            .ancestors(group)
            .iter()
            .map(|g| self.value(&pitch_key(*g)))
            .product()
    }

    /// Whether voices routed to `group` are heard with the solo buttons as they are: when any
    /// group is soloed, only groups at or below a soloed one sound.
    pub fn solo_passes(&self, group: u32) -> bool {
        let soloed: Vec<u32> = self
            .asset
            .groups
            .iter()
            .filter(|g| g.solo)
            .map(|g| g.id)
            .collect();
        soloed.is_empty() || soloed.iter().any(|s| self.asset.is_under(group, *s))
    }

    /// Track volume (linear) of a group: its fader, or silence when muted.
    pub fn track_volume(&self, group: u32) -> f32 {
        match self.asset.group(group) {
            Some(g) if !g.mute => db_to_amplitude(self.value(&volume_key(group))),
            _ => 0.0,
        }
    }

    /// The group's effect chain with current values, wet levels and send targets.
    pub fn effects(&self, group: u32) -> Vec<EffectSpec> {
        let Some(g) = self.asset.group(group) else {
            return Vec::new();
        };
        g.effects
            .iter()
            .map(|fx| {
                let values = fx
                    .kind
                    .params()
                    .iter()
                    .map(|d| self.value(&effect_key(g.id, fx.id, d.name)))
                    .collect();
                let wet = if fx.kind.has_wet() {
                    (self.value(&effect_key(g.id, fx.id, WET)) * 0.01).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let target = fx.target.and_then(|e| {
                    let tg = self.asset.group_of_effect(e)?;
                    Some((*self.tracks.get(&tg)?, e))
                });
                EffectSpec {
                    kind: fx.kind,
                    values,
                    bypass: fx.bypass || g.bypass_effects,
                    id: fx.id,
                    wet,
                    target,
                }
            })
            .collect()
    }

    /// Track a group plays into.
    fn parent_track(&self, group: u32) -> Option<TrackId> {
        match self.asset.group(group).and_then(|g| g.parent) {
            Some(p) => self.tracks.get(&p).copied(),
            None => self.output_track,
        }
    }

    /// Build the group tracks (parents first) if they aren't yet.
    pub fn ensure_tracks(&mut self, backend: &mut dyn AudioBackend, logs: &mut Vec<String>) {
        if !self.tracks.is_empty() {
            return;
        }
        for (g, _) in self.asset.tree() {
            let parent = self.parent_track(g);
            match backend.add_track(parent, &self.effects(g)) {
                Ok(t) => {
                    self.tracks.insert(g, t);
                }
                Err(e) => {
                    logs.push(format!("Audio Mixer {}: {e}", self.path));
                    return;
                }
            }
        }
        // Sends can name their targets now that every track exists.
        self.push(backend);
    }

    /// Send current volumes and effect values to the tracks.
    pub fn push(&self, backend: &mut dyn AudioBackend) {
        for (g, t) in &self.tracks {
            backend.set_track(*t, self.track_volume(*g), &self.effects(*g));
        }
    }

    /// Remove the tracks, children before parents.
    pub fn remove_tracks(&mut self, backend: &mut dyn AudioBackend) {
        for (g, _) in self.asset.tree().into_iter().rev() {
            if let Some(t) = self.tracks.remove(&g) {
                backend.remove_track(t);
            }
        }
    }

    /// The Master group's track.
    pub fn master_track(&self) -> Option<TrackId> {
        self.tracks.get(&MASTER).copied()
    }

    /// Route Master into another track (nested mixer output changed).
    pub fn set_output(&mut self, backend: &mut dyn AudioBackend, track: Option<TrackId>) {
        self.output_track = track;
        if let Some(m) = self.master_track() {
            backend.set_parent(m, track);
        }
    }

    /// Groups, parents and effect kinds: what changes the shape of the graph.
    pub fn layout(asset: &AudioMixer) -> Vec<GroupLayout> {
        asset
            .groups
            .iter()
            .map(|g| {
                let fx = g.effects.iter().map(|e| (e.id, e.kind)).collect();
                (g.id, g.parent, fx)
            })
            .collect()
    }

    /// Take an edited copy of the asset (Mixer window in Play mode). Returns true when the
    /// layout changed (call [`MixerState::sync_layout`] with the old asset).
    pub fn replace_asset(&mut self, asset: AudioMixer) -> bool {
        let changed = Self::layout(&asset) != Self::layout(&self.asset);
        self.asset = asset;
        self.snapshot = self.snapshot.min(self.asset.snapshots.len() - 1);
        self.weights
            .retain(|(i, _)| *i < self.asset.snapshots.len());
        if self.weights.is_empty() {
            self.weights = vec![(self.snapshot, 1.0)];
        }
        self.transition = None;
        self.current = self.blend(&self.weights.clone());
        changed
    }

    /// The group voices of `group` (from the `old` layout) play on now: itself, or its nearest
    /// ancestor that still exists.
    pub fn survivor(&self, old: &AudioMixer, group: u32) -> u32 {
        old.ancestors(group)
            .into_iter()
            .find(|g| self.asset.group(*g).is_some())
            .unwrap_or(MASTER)
    }

    /// Bring the tracks in line with the asset after a layout edit, without restarting
    /// anything: new groups get tracks, moved groups are rerouted, changed chains are swapped
    /// (crossfaded), removed groups' tracks go (move their voices first).
    pub fn sync_layout(&mut self, backend: &mut dyn AudioBackend, old: &AudioMixer) {
        if self.tracks.is_empty() {
            return;
        }
        for (g, _) in self.asset.tree() {
            if !self.tracks.contains_key(&g) {
                let parent = self.parent_track(g);
                if let Ok(t) = backend.add_track(parent, &self.effects(g)) {
                    self.tracks.insert(g, t);
                }
            }
        }
        let layout_of = |a: &AudioMixer, g: u32| {
            a.group(g).map(|g| {
                let fx: Vec<(u32, EffectKind)> = g.effects.iter().map(|e| (e.id, e.kind)).collect();
                (g.parent, fx)
            })
        };
        for g in self.asset.groups.iter().map(|g| g.id).collect::<Vec<_>>() {
            let (Some(now), Some(t)) = (layout_of(&self.asset, g), self.tracks.get(&g).copied())
            else {
                continue;
            };
            let before = layout_of(old, g);
            if before.as_ref().map(|b| b.0) != Some(now.0) {
                backend.set_parent(t, self.parent_track(g));
            }
            if before.as_ref().map(|b| &b.1) != Some(&now.1) {
                backend.set_chain(t, &self.effects(g));
            }
        }
        for (g, _) in old.tree().into_iter().rev() {
            if self.asset.group(g).is_none()
                && let Some(t) = self.tracks.remove(&g)
            {
                backend.remove_track(t);
            }
        }
        self.push(backend);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> (MixerState, u32, u32, u32) {
        let mut m = AudioMixer::default();
        let music = m.add_group(MASTER, "Music");
        let sfx = m.add_group(MASTER, "SFX");
        let lp = m.add_effect(sfx, EffectKind::Lowpass).unwrap();
        m.set_value(0, &volume_key(music), -10.0);
        let muffled = m.add_snapshot(0, "Muffled");
        m.set_value(muffled, &volume_key(music), -30.0);
        m.set_value(muffled, &effect_key(sfx, lp, "Cutoff freq"), 1000.0);
        m.expose(&volume_key(music));
        m.rename_exposed("MyExposedParam", "MusicVol");
        (
            MixerState::new("Assets/Main.mixer".into(), m),
            music,
            sfx,
            lp,
        )
    }

    #[test]
    fn snapshot_transitions_interpolate() {
        let (mut s, music, sfx, lp) = state();
        let cutoff = effect_key(sfx, lp, "Cutoff freq");
        assert_eq!(s.value(&volume_key(music)), -10.0);
        assert_eq!(s.value(&cutoff), 5000.0);
        s.transition_to(1, 2.0);
        s.tick(0.5);
        assert!(
            (s.value(&volume_key(music)) + 15.0).abs() < 1e-4,
            "a quarter of the way"
        );
        assert!((s.value(&cutoff) - 4000.0).abs() < 1e-2);
        s.tick(1.5);
        assert_eq!(s.value(&volume_key(music)), -30.0);
        s.tick(1.0);
        assert_eq!(s.value(&cutoff), 1000.0, "stays at the target");
        s.transition_to(0, 0.0);
        assert_eq!(s.value(&cutoff), 5000.0, "zero time jumps");
        assert_eq!(s.name(), "Main");
    }

    #[test]
    fn snapshot_weights_blend_and_interpolate() {
        let (mut s, music, sfx, lp) = state();
        let cutoff = effect_key(sfx, lp, "Cutoff freq");
        // 3 parts Normal, 1 part Muffled: three quarters of the way from Muffled to Normal.
        s.transition_to_weighted(&[(0, 3.0), (1, 1.0)], 0.0);
        assert!((s.value(&volume_key(music)) - (-10.0 * 0.75 - 30.0 * 0.25)).abs() < 1e-4);
        assert!((s.value(&cutoff) - (5000.0 * 0.75 + 1000.0 * 0.25)).abs() < 1e-2);
        assert_eq!(s.snapshot, 0, "the heaviest snapshot is the current one");
        // Moving to an even blend over 1 s: halfway there after 0.5 s.
        s.transition_to_weighted(&[(0, 0.5), (1, 0.5)], 1.0);
        s.tick(0.5);
        let start = -15.0;
        let end = -20.0;
        assert!((s.value(&volume_key(music)) - (start + (end - start) * 0.5)).abs() < 1e-3);
        s.tick(0.5);
        assert!((s.value(&volume_key(music)) - end).abs() < 1e-4);
        // Weights need not add up to one; zero and negative ones count for nothing.
        s.transition_to_weighted(&[(0, 0.0), (1, 5.0), (0, -2.0)], 0.0);
        assert_eq!(s.value(&volume_key(music)), -30.0);
        assert_eq!(s.snapshot, 1);
        // All zero: nothing moves.
        s.transition_to_weighted(&[(0, 0.0)], 0.0);
        assert_eq!(s.value(&volume_key(music)), -30.0);
    }

    #[test]
    fn exposed_parameters_override_snapshots() {
        let (mut s, music, _, _) = state();
        assert_eq!(s.get_float("MusicVol"), Some(-10.0));
        assert!(s.set_float("MusicVol", -3.0));
        s.transition_to(1, 0.0);
        assert_eq!(s.value(&volume_key(music)), -3.0, "the override wins");
        assert!(s.set_float("MusicVol", 99.0));
        assert_eq!(
            s.get_float("MusicVol"),
            Some(20.0),
            "clamped to the fader range"
        );
        assert!(s.clear_float("MusicVol"));
        assert_eq!(s.get_float("MusicVol"), Some(-30.0));
        assert!(!s.set_float("Nope", 1.0));
        assert_eq!(s.get_float("Nope"), None);
    }

    #[test]
    fn mute_solo_bypass_and_pitch() {
        let (mut s, music, sfx, _) = state();
        assert!(s.solo_passes(music) && s.solo_passes(MASTER));
        s.asset.group_mut(sfx).unwrap().solo = true;
        assert!(!s.solo_passes(music) && s.solo_passes(sfx));
        assert!(
            !s.solo_passes(MASTER),
            "sounds routed to Master itself are cut too"
        );
        s.asset.group_mut(music).unwrap().mute = true;
        assert_eq!(s.track_volume(music), 0.0);
        assert!((s.track_volume(sfx) - 1.0).abs() < 1e-6);
        assert!(!s.effects(sfx)[0].bypass);
        s.asset.group_mut(sfx).unwrap().bypass_effects = true;
        assert!(s.effects(sfx)[0].bypass);
        s.asset.set_value(0, &pitch_key(MASTER), 0.5);
        s.asset.set_value(0, &pitch_key(sfx), 1.5);
        s.transition_to(0, 0.0);
        assert!(
            (s.pitch(sfx) - 0.75).abs() < 1e-6,
            "pitches multiply down the tree"
        );
    }

    #[test]
    fn effects_carry_ids_wet_levels_and_send_targets() {
        let (mut s, music, sfx, lp) = state();
        let duck = s.asset.add_effect(music, EffectKind::DuckVolume).unwrap();
        let send = s.asset.add_effect(sfx, EffectKind::Send).unwrap();
        assert!(s.asset.set_send_target(sfx, send, Some(duck)));
        assert!(
            !s.asset.set_send_target(sfx, send, Some(lp)),
            "not a receiver"
        );
        s.asset.set_value(0, &effect_key(sfx, lp, WET), 25.0);
        s.transition_to(0, 0.0);
        s.tracks.insert(music, 41);
        let fx = s.effects(sfx);
        assert_eq!((fx[0].id, fx[0].wet), (lp, 0.25));
        assert_eq!(fx[1].target, Some((41, duck)), "the Duck Volume's track");
        assert_eq!(
            s.effects(music)[0].wet,
            1.0,
            "routing units have no wet level"
        );
        assert_eq!(s.asset.sends_to(duck), vec![(sfx, send)]);
        assert_eq!(s.asset.effect_label(duck), "Duck Volume on Music");
        // Removing the receiver clears the send.
        s.asset.remove_effect(music, duck);
        assert_eq!(s.asset.effect(sfx, send).unwrap().target, None);
    }
}
