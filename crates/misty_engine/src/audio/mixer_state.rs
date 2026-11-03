//! A mixer while playing: current snapshot values (with transitions), script overrides of
//! exposed parameters, and the backend tracks its groups play on.

use std::collections::HashMap;

use super::backend::{AudioBackend, TrackId};
use super::effects::{EffectSpec, db_to_amplitude};
use super::mixer::{AudioMixer, MASTER, effect_key, pitch_key, volume_key};

/// A group's id, parent and effect chain (ids and kinds).
pub type GroupLayout = (u32, Option<u32>, Vec<(u32, super::EffectKind)>);

struct Transition {
    from: HashMap<String, f32>,
    to: usize,
    elapsed: f32,
    duration: f32,
}

pub struct MixerState {
    /// Project-relative `.mixer` path.
    pub path: String,
    pub asset: AudioMixer,
    /// Snapshot most recently transitioned to.
    pub snapshot: usize,
    /// Values right now, by key.
    current: HashMap<String, f32>,
    transition: Option<Transition>,
    /// `SetFloat` values by exposed parameter name.
    overrides: HashMap<String, f32>,
    /// Backend track of each group (built on first use).
    pub tracks: HashMap<u32, TrackId>,
}

impl MixerState {
    pub fn new(path: String, asset: AudioMixer) -> Self {
        let snapshot = asset.start_snapshot;
        let mut s = Self {
            path,
            asset,
            snapshot,
            current: HashMap::new(),
            transition: None,
            overrides: HashMap::new(),
            tracks: HashMap::new(),
        };
        s.current = s.snapshot_values(snapshot);
        s
    }

    /// File stem ("Main" for `Assets/Audio/Main.mixer`).
    pub fn name(&self) -> String {
        crate::scene::clip_name(&self.path)
    }

    fn snapshot_values(&self, snap: usize) -> HashMap<String, f32> {
        self.asset
            .keys()
            .into_iter()
            .map(|k| {
                let v = self.asset.value(snap, &k);
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
        if snap >= self.asset.snapshots.len() {
            return;
        }
        self.snapshot = snap;
        if time <= 0.0 {
            self.transition = None;
            self.current = self.snapshot_values(snap);
        } else {
            self.transition = Some(Transition {
                from: self.current.clone(),
                to: snap,
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
        let (to, done) = (t.to, k >= 1.0);
        let target = self.snapshot_values(to);
        let t = self.transition.as_ref().expect("checked");
        self.current = target
            .into_iter()
            .map(|(key, b)| {
                let a = t.from.get(&key).copied().unwrap_or(b);
                (key, a + (b - a) * k)
            })
            .collect();
        if done {
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

    /// The group's effect chain with current values.
    pub fn effects(&self, group: u32) -> Vec<EffectSpec> {
        let Some(g) = self.asset.group(group) else {
            return Vec::new();
        };
        g.effects
            .iter()
            .map(|fx| EffectSpec {
                kind: fx.kind,
                values: fx
                    .kind
                    .params()
                    .iter()
                    .map(|d| self.value(&effect_key(g.id, fx.id, d.name)))
                    .collect(),
                bypass: fx.bypass || g.bypass_effects,
            })
            .collect()
    }

    /// Build the group tracks (parents first) if they aren't yet.
    pub fn ensure_tracks(&mut self, backend: &mut dyn AudioBackend, logs: &mut Vec<String>) {
        if !self.tracks.is_empty() {
            return;
        }
        for (g, _) in self.asset.tree() {
            let parent = self
                .asset
                .group(g)
                .and_then(|g| g.parent)
                .and_then(|p| self.tracks.get(&p).copied());
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

    /// Groups, parents and effect kinds: what can't change without rebuilding the tracks.
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
    /// group layout changed and the tracks must be rebuilt.
    pub fn replace_asset(&mut self, asset: AudioMixer) -> bool {
        let rebuild = Self::layout(&asset) != Self::layout(&self.asset);
        self.asset = asset;
        self.snapshot = self.snapshot.min(self.asset.snapshots.len() - 1);
        self.transition = None;
        self.current = self.snapshot_values(self.snapshot);
        rebuild
    }
}

#[cfg(test)]
mod tests {
    use super::super::EffectKind;
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
}
