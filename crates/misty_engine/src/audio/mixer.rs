//! Audio Mixer assets (`.mixer` JSON): a tree of groups under Master, each with an effect
//! chain; snapshots holding every group's volume, pitch and effect values; and exposed
//! parameters scripts can set by name.
//!
//! Values are addressed by string keys: `"<group id>/Volume"`, `"<group id>/Pitch"` and
//! `"<group id>/<effect id>/<parameter>"` (including `"<group>/<effect>/Wet"`, the wet mix level).
//! Ids never change, so renaming keeps snapshots valid.
//!
//! Send units name the Receive or Duck Volume unit they feed by effect id. A mixer's Output can
//! be a group of another mixer: its Master then plays into that group (nested mixers).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::effects::{EffectKind, MAX_DB, MIN_DB};

pub const MIXER_EXTENSION: &str = "mixer";

pub fn is_mixer(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(MIXER_EXTENSION))
}

/// Id of the Master group (always the first group).
pub const MASTER: u32 = 0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioMixer {
    pub groups: Vec<MixerGroup>,
    pub snapshots: Vec<MixerSnapshot>,
    /// Snapshot in effect when Play starts.
    pub start_snapshot: usize,
    pub exposed: Vec<ExposedParam>,
    /// Next free group / effect id.
    pub next_id: u32,
    /// Group of another mixer this mixer's Master plays into (`"Assets/Audio/Main.mixer#Music"`);
    /// `None` = straight to the listener.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MixerGroup {
    pub id: u32,
    pub name: String,
    /// `None` only for Master.
    pub parent: Option<u32>,
    pub mute: bool,
    pub solo: bool,
    pub bypass_effects: bool,
    pub effects: Vec<MixerEffect>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MixerEffect {
    pub id: u32,
    pub kind: EffectKind,
    #[serde(default)]
    pub bypass: bool,
    /// Send: id of the Receive / Duck Volume unit it feeds (in this mixer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MixerSnapshot {
    pub name: String,
    /// Values that differ from the defaults, by key.
    pub values: BTreeMap<String, f32>,
}

/// A value scripts reach with `mixer:SetFloat(name, v)`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExposedParam {
    pub name: String,
    pub key: String,
}

impl Default for AudioMixer {
    fn default() -> Self {
        Self {
            groups: vec![MixerGroup {
                id: MASTER,
                name: "Master".into(),
                ..Default::default()
            }],
            snapshots: vec![MixerSnapshot {
                name: "Snapshot".into(),
                ..Default::default()
            }],
            start_snapshot: 0,
            exposed: Vec::new(),
            next_id: 1,
            output: None,
        }
    }
}

// ---- keys ----------------------------------------------------------------------------------

pub fn volume_key(group: u32) -> String {
    format!("{group}/Volume")
}

pub fn pitch_key(group: u32) -> String {
    format!("{group}/Pitch")
}

pub fn effect_key(group: u32, effect: u32, param: &str) -> String {
    format!("{group}/{effect}/{param}")
}

/// What a key points at.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyTarget<'a> {
    Volume(u32),
    Pitch(u32),
    Effect(u32, u32, &'a str),
}

pub fn parse_key(key: &str) -> Option<KeyTarget<'_>> {
    let mut parts = key.splitn(3, '/');
    let group: u32 = parts.next()?.parse().ok()?;
    match (parts.next()?, parts.next()) {
        ("Volume", None) => Some(KeyTarget::Volume(group)),
        ("Pitch", None) => Some(KeyTarget::Pitch(group)),
        (effect, Some(param)) => Some(KeyTarget::Effect(group, effect.parse().ok()?, param)),
        _ => None,
    }
}

impl AudioMixer {
    // ---- file ----------------------------------------------------------------------------------

    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut m: Self = serde_json::from_str(&crate::asset_db::resolve_json(text))
            .map_err(|e| e.to_string())?;
        m.sanitize();
        Ok(m)
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

    /// Repair hand-edited files: Master first, valid parents, at least one snapshot.
    pub fn sanitize(&mut self) {
        if self.groups.first().is_none_or(|g| g.id != MASTER) {
            self.groups.retain(|g| g.id != MASTER);
            self.groups
                .insert(0, AudioMixer::default().groups.remove(0));
        }
        self.groups[0].parent = None;
        let ids: Vec<u32> = self.groups.iter().map(|g| g.id).collect();
        for g in self.groups.iter_mut().skip(1) {
            if g.parent.is_none_or(|p| !ids.contains(&p) || p == g.id) {
                g.parent = Some(MASTER);
            }
        }
        // Parent loops (A under B under A) never reach Master: such groups vanished from the
        // Mixer window's tree. Hang each loop's group under Master.
        for i in 1..self.groups.len() {
            let mut seen = vec![self.groups[i].id];
            let mut cur = self.groups[i].parent;
            while let Some(p) = cur
                && p != MASTER
            {
                if seen.contains(&p) {
                    self.groups[i].parent = Some(MASTER);
                    break;
                }
                seen.push(p);
                cur = self.groups.iter().find(|g| g.id == p).and_then(|g| g.parent);
            }
        }
        if self.snapshots.is_empty() {
            self.snapshots = AudioMixer::default().snapshots;
        }
        self.start_snapshot = self.start_snapshot.min(self.snapshots.len() - 1);
        let max_id = self
            .groups
            .iter()
            .flat_map(|g| std::iter::once(g.id).chain(g.effects.iter().map(|e| e.id)))
            .max()
            .unwrap_or(0);
        self.next_id = self.next_id.max(max_id + 1);
        // Sends feed receiving units on other groups only.
        let receivers = self.receivers();
        for g in &mut self.groups {
            let gid = g.id;
            for fx in &mut g.effects {
                let ok = fx.kind == EffectKind::Send
                    && fx
                        .target
                        .is_some_and(|t| receivers.iter().any(|(rg, re)| *re == t && *rg != gid));
                if !ok {
                    fx.target = None;
                }
            }
        }
    }

    fn fresh_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id - 1
    }

    // ---- groups --------------------------------------------------------------------------------

    pub fn group(&self, id: u32) -> Option<&MixerGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn group_mut(&mut self, id: u32) -> Option<&mut MixerGroup> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    pub fn group_by_name(&self, name: &str) -> Option<&MixerGroup> {
        self.groups.iter().find(|g| g.name == name)
    }

    pub fn children(&self, id: u32) -> impl Iterator<Item = &MixerGroup> {
        self.groups.iter().filter(move |g| g.parent == Some(id))
    }

    /// `id` and its parents up to Master.
    pub fn ancestors(&self, id: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut cur = self.group(id).map(|g| g.id);
        while let Some(g) = cur {
            if out.contains(&g) {
                break;
            }
            out.push(g);
            cur = self.group(g).and_then(|g| g.parent);
        }
        out
    }

    /// Whether `id` is `ancestor` or below it.
    pub fn is_under(&self, id: u32, ancestor: u32) -> bool {
        self.ancestors(id).contains(&ancestor)
    }

    /// "Master/Music/Drums".
    pub fn path(&self, id: u32) -> String {
        let mut names: Vec<&str> = self
            .ancestors(id)
            .iter()
            .filter_map(|g| self.group(*g).map(|g| g.name.as_str()))
            .collect();
        names.reverse();
        names.join("/")
    }

    /// Groups depth first (Master, then each child's subtree), with their depth.
    pub fn tree(&self) -> Vec<(u32, usize)> {
        let mut out = Vec::new();
        let mut stack = vec![(MASTER, 0)];
        while let Some((id, depth)) = stack.pop() {
            out.push((id, depth));
            let kids: Vec<u32> = self.children(id).map(|g| g.id).collect();
            stack.extend(kids.into_iter().rev().map(|k| (k, depth + 1)));
        }
        out
    }

    /// A name not used by any group ("SFX", "SFX 1", …).
    pub fn unique_group_name(&self, base: &str) -> String {
        let mut name = base.to_owned();
        let mut i = 1;
        while self.group_by_name(&name).is_some() {
            name = format!("{base} {i}");
            i += 1;
        }
        name
    }

    /// Add a child group under `parent`; returns its id.
    pub fn add_group(&mut self, parent: u32, name: &str) -> u32 {
        let parent = if self.group(parent).is_some() {
            parent
        } else {
            MASTER
        };
        let id = self.fresh_id();
        let name = self.unique_group_name(name);
        self.groups.push(MixerGroup {
            id,
            name,
            parent: Some(parent),
            ..Default::default()
        });
        id
    }

    /// Remove a group with everything below it (not Master), and the values pointing at them.
    pub fn remove_group(&mut self, id: u32) {
        if id == MASTER {
            return;
        }
        let gone: Vec<u32> = self
            .groups
            .iter()
            .filter(|g| self.is_under(g.id, id))
            .map(|g| g.id)
            .collect();
        self.groups.retain(|g| !gone.contains(&g.id));
        self.prune(|t| match t {
            KeyTarget::Volume(g) | KeyTarget::Pitch(g) | KeyTarget::Effect(g, _, _) => {
                gone.contains(&g)
            }
        });
        self.sanitize();
    }

    /// Rename a group; false if the name is empty or taken.
    pub fn rename_group(&mut self, id: u32, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || self.group_by_name(name).is_some_and(|g| g.id != id) {
            return false;
        }
        match self.group_mut(id) {
            Some(g) => {
                g.name = name.to_owned();
                true
            }
            None => false,
        }
    }

    /// Move a group under another one (not into its own subtree).
    pub fn reparent(&mut self, id: u32, parent: u32) -> bool {
        if id == MASTER || self.group(parent).is_none() || self.is_under(parent, id) {
            return false;
        }
        match self.group_mut(id) {
            Some(g) => {
                g.parent = Some(parent);
                true
            }
            None => false,
        }
    }

    // ---- effects -------------------------------------------------------------------------------

    pub fn add_effect(&mut self, group: u32, kind: EffectKind) -> Option<u32> {
        self.group(group)?;
        let id = self.fresh_id();
        self.group_mut(group)?.effects.push(MixerEffect {
            id,
            kind,
            bypass: false,
            target: None,
        });
        Some(id)
    }

    pub fn remove_effect(&mut self, group: u32, effect: u32) {
        if let Some(g) = self.group_mut(group) {
            g.effects.retain(|e| e.id != effect);
        }
        self.prune(|t| matches!(t, KeyTarget::Effect(g, e, _) if g == group && e == effect));
        self.sanitize();
    }

    /// Move an effect to position `to` of its chain (drag and drop).
    pub fn move_effect_to(&mut self, group: u32, effect: u32, to: usize) {
        if let Some(g) = self.group_mut(group)
            && let Some(i) = g.effects.iter().position(|e| e.id == effect)
        {
            let fx = g.effects.remove(i);
            let to = to.min(g.effects.len());
            g.effects.insert(to, fx);
        }
    }

    /// The group holding effect `effect`.
    pub fn group_of_effect(&self, effect: u32) -> Option<u32> {
        self.groups
            .iter()
            .find(|g| g.effects.iter().any(|e| e.id == effect))
            .map(|g| g.id)
    }

    /// Every Receive and Duck Volume unit: (group, effect).
    pub fn receivers(&self) -> Vec<(u32, u32)> {
        self.groups
            .iter()
            .flat_map(|g| {
                g.effects
                    .iter()
                    .filter(|e| e.kind.receives_sends())
                    .map(move |e| (g.id, e.id))
            })
            .collect()
    }

    /// Point a Send at a Receive / Duck Volume unit (`None` = nowhere); false if not allowed.
    pub fn set_send_target(&mut self, group: u32, send: u32, target: Option<u32>) -> bool {
        let allowed =
            target.is_none_or(|t| self.receivers().iter().any(|(g, e)| *e == t && *g != group));
        match self
            .group_mut(group)
            .and_then(|g| g.effects.iter_mut().find(|e| e.id == send))
        {
            Some(fx) if allowed && fx.kind == EffectKind::Send => {
                fx.target = target;
                true
            }
            _ => false,
        }
    }

    /// The Sends feeding receiving unit `effect`: (group, send effect).
    pub fn sends_to(&self, effect: u32) -> Vec<(u32, u32)> {
        self.groups
            .iter()
            .flat_map(|g| {
                g.effects
                    .iter()
                    .filter(move |e| e.target == Some(effect))
                    .map(move |e| (g.id, e.id))
            })
            .collect()
    }

    /// "Duck Volume on Music".
    pub fn effect_label(&self, effect: u32) -> String {
        match self.group_of_effect(effect) {
            Some(g) => {
                let kind = self.effect(g, effect).map_or("?", |e| e.kind.label());
                let name = self.group(g).map_or("?", |g| g.name.as_str());
                format!("{kind} on {name}")
            }
            None => "None".into(),
        }
    }

    /// Move an effect one slot earlier (`-1`) or later (`1`) in its chain.
    pub fn move_effect(&mut self, group: u32, effect: u32, by: isize) {
        if let Some(g) = self.group_mut(group)
            && let Some(i) = g.effects.iter().position(|e| e.id == effect)
        {
            let j = (i as isize + by).clamp(0, g.effects.len() as isize - 1) as usize;
            g.effects.swap(i, j);
        }
    }

    pub fn effect(&self, group: u32, effect: u32) -> Option<&MixerEffect> {
        self.group(group)?.effects.iter().find(|e| e.id == effect)
    }

    // ---- values --------------------------------------------------------------------------------

    /// Value a key has when no snapshot sets it; `None` for keys pointing at nothing.
    pub fn default_value(&self, key: &str) -> Option<f32> {
        match parse_key(key)? {
            KeyTarget::Volume(g) => self.group(g).map(|_| 0.0),
            KeyTarget::Pitch(g) => self.group(g).map(|_| 1.0),
            KeyTarget::Effect(g, e, param) => Some(self.effect(g, e)?.kind.param(param)?.default),
        }
    }

    /// Allowed range of a key's value.
    pub fn range(&self, key: &str) -> (f32, f32) {
        match parse_key(key) {
            Some(KeyTarget::Volume(_)) => (MIN_DB, MAX_DB),
            Some(KeyTarget::Pitch(_)) => (0.01, 10.0),
            Some(KeyTarget::Effect(g, e, param)) => self
                .effect(g, e)
                .and_then(|fx| fx.kind.param(param))
                .map_or((f32::MIN, f32::MAX), |d| (d.min, d.max)),
            None => (f32::MIN, f32::MAX),
        }
    }

    /// `key` in snapshot `snap` (its default when the snapshot doesn't store it).
    pub fn value(&self, snap: usize, key: &str) -> f32 {
        self.snapshots
            .get(snap)
            .and_then(|s| s.values.get(key))
            .copied()
            .or_else(|| self.default_value(key))
            .unwrap_or(0.0)
    }

    /// Store `key` in snapshot `snap` (clamped to its range).
    pub fn set_value(&mut self, snap: usize, key: &str, v: f32) {
        let (lo, hi) = self.range(key);
        if let Some(s) = self.snapshots.get_mut(snap) {
            s.values.insert(key.to_owned(), v.clamp(lo, hi));
        }
    }

    /// Every key the mixer has right now (volumes, pitches, effect parameters).
    pub fn keys(&self) -> Vec<String> {
        let mut out = Vec::new();
        for g in &self.groups {
            out.push(volume_key(g.id));
            out.push(pitch_key(g.id));
            for fx in &g.effects {
                out.extend(
                    fx.kind
                        .keyed_params()
                        .map(|d| effect_key(g.id, fx.id, d.name)),
                );
            }
        }
        out
    }

    /// Readable name of a key ("Volume of SFX", "Cutoff freq of Lowpass on Music").
    pub fn key_label(&self, key: &str) -> String {
        let name = |g: u32| self.group(g).map_or("?", |g| g.name.as_str()).to_owned();
        match parse_key(key) {
            Some(KeyTarget::Volume(g)) => format!("Volume of {}", name(g)),
            Some(KeyTarget::Pitch(g)) => format!("Pitch of {}", name(g)),
            Some(KeyTarget::Effect(g, e, p)) => {
                let fx = self.effect(g, e).map_or("?", |fx| fx.kind.label());
                let p = if p == super::effects::WET {
                    "Wet mix level"
                } else {
                    p
                };
                format!("{p} of {fx} on {}", name(g))
            }
            None => key.to_owned(),
        }
    }

    /// Drop stored values and exposed parameters whose key matches `gone`.
    fn prune(&mut self, gone: impl Fn(KeyTarget) -> bool) {
        let dead = |k: &str| parse_key(k).is_none_or(&gone);
        for s in &mut self.snapshots {
            s.values.retain(|k, _| !dead(k));
        }
        self.exposed.retain(|p| !dead(&p.key));
    }

    // ---- snapshots -----------------------------------------------------------------------------

    pub fn snapshot_index(&self, name: &str) -> Option<usize> {
        self.snapshots.iter().position(|s| s.name == name)
    }

    /// Add a copy of snapshot `from` under a unique name; returns its index.
    pub fn add_snapshot(&mut self, from: usize, name: &str) -> usize {
        let mut unique = name.to_owned();
        let mut i = 1;
        while self.snapshot_index(&unique).is_some() {
            unique = format!("{name} {i}");
            i += 1;
        }
        let values = self
            .snapshots
            .get(from)
            .map(|s| s.values.clone())
            .unwrap_or_default();
        self.snapshots.push(MixerSnapshot {
            name: unique,
            values,
        });
        self.snapshots.len() - 1
    }

    /// Remove a snapshot (the last one stays).
    pub fn remove_snapshot(&mut self, i: usize) {
        if self.snapshots.len() > 1 && i < self.snapshots.len() {
            self.snapshots.remove(i);
            if self.start_snapshot > i || self.start_snapshot >= self.snapshots.len() {
                self.start_snapshot = self.start_snapshot.saturating_sub(1);
            }
        }
    }

    // ---- exposed parameters --------------------------------------------------------------------

    pub fn exposed_key(&self, name: &str) -> Option<&str> {
        self.exposed
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.key.as_str())
    }

    /// Expose `key` to scripts ("MyExposedParam", "MyExposedParam 1", …); returns its name.
    pub fn expose(&mut self, key: &str) -> String {
        if let Some(p) = self.exposed.iter().find(|p| p.key == key) {
            return p.name.clone();
        }
        let mut name = "MyExposedParam".to_owned();
        let mut i = 1;
        while self.exposed_key(&name).is_some() {
            name = format!("MyExposedParam {i}");
            i += 1;
        }
        self.exposed.push(ExposedParam {
            name: name.clone(),
            key: key.to_owned(),
        });
        name
    }

    pub fn unexpose(&mut self, key: &str) {
        self.exposed.retain(|p| p.key != key);
    }

    /// Rename an exposed parameter; false if the name is empty or taken.
    pub fn rename_exposed(&mut self, old: &str, new: &str) -> bool {
        let new = new.trim();
        if new.is_empty() || (new != old && self.exposed_key(new).is_some()) {
            return false;
        }
        match self.exposed.iter_mut().find(|p| p.name == old) {
            Some(p) => {
                p.name = new.to_owned();
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Master ▸ Music, SFX ▸ Footsteps, with a lowpass on SFX.
    fn mixer() -> (AudioMixer, u32, u32, u32, u32) {
        let mut m = AudioMixer::default();
        let music = m.add_group(MASTER, "Music");
        let sfx = m.add_group(MASTER, "SFX");
        let steps = m.add_group(sfx, "Footsteps");
        let lp = m.add_effect(sfx, EffectKind::Lowpass).unwrap();
        (m, music, sfx, steps, lp)
    }

    #[test]
    fn keys_parse_back() {
        assert_eq!(parse_key("3/Volume"), Some(KeyTarget::Volume(3)));
        assert_eq!(parse_key("3/Pitch"), Some(KeyTarget::Pitch(3)));
        assert_eq!(
            parse_key("3/7/Cutoff freq"),
            Some(KeyTarget::Effect(3, 7, "Cutoff freq"))
        );
        assert_eq!(parse_key("x/Volume"), None);
        assert_eq!(parse_key("3"), None);
    }

    #[test]
    fn group_tree_paths_and_unique_names() {
        let (mut m, music, sfx, steps, _) = mixer();
        assert_eq!(m.path(steps), "Master/SFX/Footsteps");
        let order: Vec<u32> = m.tree().iter().map(|(g, _)| *g).collect();
        assert_eq!(order, [MASTER, music, sfx, steps]);
        assert_eq!(m.tree()[3].1, 2, "depth");
        let again = m.add_group(MASTER, "Music");
        assert_eq!(m.group(again).unwrap().name, "Music 1");
        assert!(!m.rename_group(again, "SFX"), "names stay unique");
        assert!(m.rename_group(again, "Voice"));
        assert!(!m.reparent(sfx, steps), "no cycles");
        assert!(m.reparent(steps, music));
        assert_eq!(m.path(steps), "Master/Music/Footsteps");
    }

    #[test]
    fn values_default_clamp_and_prune() {
        let (mut m, music, sfx, steps, lp) = mixer();
        let cutoff = effect_key(sfx, lp, "Cutoff freq");
        assert_eq!(m.value(0, &volume_key(music)), 0.0);
        assert_eq!(m.value(0, &pitch_key(music)), 1.0);
        assert_eq!(m.value(0, &cutoff), 5000.0);
        m.set_value(0, &volume_key(music), -200.0);
        assert_eq!(m.value(0, &volume_key(music)), MIN_DB, "clamped");
        m.set_value(0, &cutoff, 800.0);
        m.set_value(0, &volume_key(steps), -6.0);
        m.expose(&cutoff);
        m.remove_group(sfx);
        assert!(m.group(steps).is_none(), "children go too");
        assert_eq!(
            m.snapshots[0].values.len(),
            1,
            "only Music's volume is left"
        );
        assert!(m.exposed.is_empty());
        m.remove_group(MASTER);
        assert_eq!(m.groups[0].id, MASTER, "Master stays");
    }

    #[test]
    fn saves_and_loads() {
        let (mut m, music, sfx, _, lp) = mixer();
        m.set_value(0, &volume_key(music), -10.0);
        let muffled = m.add_snapshot(0, "Muffled");
        m.set_value(muffled, &effect_key(sfx, lp, "Cutoff freq"), 400.0);
        assert_eq!(m.expose(&volume_key(music)), "MyExposedParam");
        assert!(m.rename_exposed("MyExposedParam", "MusicVol"));
        m.group_mut(sfx).unwrap().solo = true;
        let dir = std::env::temp_dir().join(format!("misty_mixer_{}", std::process::id()));
        let path = dir.join("Main.mixer");
        m.save(&path).unwrap();
        let loaded = AudioMixer::load(&path).unwrap();
        assert_eq!(loaded, m);
        assert_eq!(loaded.value(muffled, &volume_key(music)), -10.0, "copied");
        assert_eq!(
            loaded.exposed_key("MusicVol"),
            Some(volume_key(music).as_str())
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn broken_files_are_repaired() {
        let m = AudioMixer::from_json(
            r#"{"groups":[{"id":4,"name":"Orphan","parent":99}],"snapshots":[],"start_snapshot":5}"#,
        )
        .unwrap();
        assert_eq!(m.groups[0].name, "Master");
        assert_eq!(m.group(4).unwrap().parent, Some(MASTER));
        assert_eq!((m.snapshots.len(), m.start_snapshot), (1, 0));
        assert_eq!(m.next_id, 5);
        assert!(is_mixer("Assets/Audio/Main.MIXER") && !is_mixer("a.wav"));
    }

    /// Groups whose parents loop (hand-edited file) hang under Master after loading, so the
    /// Mixer window's tree shows them.
    #[test]
    fn parent_loops_are_broken() {
        let mut m = AudioMixer::default();
        let a = m.add_group(MASTER, "A");
        let b = m.add_group(a, "B");
        m.groups.iter_mut().find(|g| g.id == a).unwrap().parent = Some(b);
        m.sanitize();
        let ids: Vec<u32> = m.tree().into_iter().map(|(g, _)| g).collect();
        assert!(ids.contains(&a) && ids.contains(&b), "{ids:?}");
        assert!(m.path(b).starts_with(&m.path(MASTER)));
    }

}
