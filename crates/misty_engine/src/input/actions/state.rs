//! A running copy of an Input Actions asset: which maps and actions are enabled, binding
//! overrides, the binding mask (control scheme) and paired devices, and each action's phase
//! and value, stepped once per frame.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::asset::{ActionType, Binding, ControlType, InputActionAsset, has_group};
use super::composites;
use super::interactions::{
    DEFAULT_PRESS_POINT, Interaction, InteractionState, Phase, RELEASE_RATIO, Transition,
};
use super::path::{ControlPath, DeviceFilter};
use super::processors;
use super::value::Value;
use crate::scripting::Input;

/// A phase change of an action (what `started` / `performed` / `canceled` listeners get).
#[derive(Clone, Debug, PartialEq)]
pub struct ActionEvent {
    pub map: usize,
    pub action: usize,
    /// Started, Performed or Canceled.
    pub phase: Phase,
    pub value: Value,
    pub time: f64,
    pub start_time: f64,
    /// The interaction that drove it ("Hold"), None for the action type's default behaviour.
    pub interaction: Option<&'static str>,
    /// Path of the control that drove it.
    pub control: String,
}

/// A parsed plain binding or composite part.
#[derive(Clone, Debug)]
struct Leaf {
    name: String,
    control: Option<ControlPath>,
    path: String,
    groups: String,
    processors: String,
}

/// A parsed top-level binding.
#[derive(Clone, Debug)]
struct Unit {
    composite: String,
    parameters: String,
    /// One leaf for plain bindings, the parts for composites.
    leaves: Vec<Leaf>,
    processors: String,
    interactions: Vec<Interaction>,
}

/// Per-action running state.
#[derive(Clone, Debug, Default)]
pub struct ActionState {
    pub enabled: bool,
    pub phase: Phase,
    pub value: Value,
    pub magnitude: f32,
    /// Past the press point (button-style polling, independent of interactions).
    pub pressed: bool,
    pub pressed_this_frame: bool,
    pub released_this_frame: bool,
    pub performed_this_frame: bool,
    pub started_this_frame: bool,
    pub canceled_this_frame: bool,
    pub start_time: f64,
    /// The control driving the action (its path).
    pub active_control: Option<String>,
    /// Just enabled: the next update first takes the controls' state from the previous frame,
    /// so a button already held doesn't count as a new press.
    prime: bool,
    driver: Option<(usize, usize)>,
    states: Vec<Vec<InteractionState>>,
    last: Vec<Option<Value>>,
}

/// One binding override saved by `SaveBindingOverridesAsJson`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverrideEntry {
    /// `Map/Action`, for people reading the file.
    pub action: String,
    pub id: String,
    pub path: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OverrideFile {
    pub bindings: Vec<OverrideEntry>,
}

pub struct ActionSet {
    pub asset: InputActionAsset,
    /// Project path it was loaded from ("" for assets made in code).
    pub path: String,
    /// Binding (or part) id → override path.
    pub overrides: BTreeMap<String, String>,
    /// Only bindings in this control scheme group are used (None = all).
    pub binding_mask: Option<String>,
    /// Devices the bindings may read.
    pub devices: DeviceFilter,
    actions: Vec<Vec<ActionState>>,
    units: Vec<Vec<Vec<Unit>>>,
    /// Canceled events of actions disabled between frames, sent with the next update.
    pending: Vec<ActionEvent>,
    now: f64,
}

impl ActionSet {
    pub fn new(asset: InputActionAsset, path: &str) -> Self {
        let mut s = Self {
            asset,
            path: path.into(),
            overrides: BTreeMap::new(),
            binding_mask: None,
            devices: DeviceFilter::all(),
            actions: Vec::new(),
            units: Vec::new(),
            pending: Vec::new(),
            now: 0.0,
        };
        s.actions = s
            .asset
            .maps
            .iter()
            .map(|m| {
                m.actions
                    .iter()
                    .map(|a| ActionState {
                        phase: Phase::Disabled,
                        value: zero_for(a.control_type),
                        ..Default::default()
                    })
                    .collect()
            })
            .collect();
        s.rebuild();
        s
    }

    /// Re-parse bindings (after an override or asset change); interaction state restarts.
    pub fn rebuild(&mut self) {
        self.units = self
            .asset
            .maps
            .iter()
            .map(|m| {
                m.actions
                    .iter()
                    .map(|a| {
                        a.bindings
                            .iter()
                            .map(|b| self.unit(b, &a.interactions, &a.processors))
                            .collect()
                    })
                    .collect()
            })
            .collect();
        for (m, map) in self.units.iter().enumerate() {
            for (a, units) in map.iter().enumerate() {
                let st = &mut self.actions[m][a];
                st.states = units
                    .iter()
                    .map(|u| vec![InteractionState::default(); u.interactions.len()])
                    .collect();
                st.last = vec![None; units.len()];
                st.driver = None;
            }
        }
    }

    fn unit(&self, b: &Binding, action_interactions: &str, action_processors: &str) -> Unit {
        let leaf = |name: &str, path: &str, id: &str, groups: &str, processors: &str| {
            let path = self.overrides.get(id).map_or(path, String::as_str);
            Leaf {
                name: name.into(),
                control: ControlPath::parse(path).ok(),
                path: path.into(),
                groups: groups.into(),
                processors: processors.into(),
            }
        };
        let leaves = if b.is_composite() {
            b.parts
                .iter()
                .map(|p| leaf(&p.name, &p.path, &p.id, &p.groups, &p.processors))
                .collect()
        } else {
            vec![leaf("", &b.path, &b.id, &b.groups, "")]
        };
        let mut interactions = Interaction::parse_list(&b.interactions);
        interactions.extend(Interaction::parse_list(action_interactions));
        let mut processors = b.processors.clone();
        if !action_processors.trim().is_empty() {
            if !processors.is_empty() {
                processors.push(',');
            }
            processors.push_str(action_processors);
        }
        Unit {
            composite: b.composite.clone(),
            parameters: b.parameters.clone(),
            leaves,
            processors,
            interactions,
        }
    }

    pub fn state(&self, map: usize, action: usize) -> Option<&ActionState> {
        self.actions.get(map)?.get(action)
    }

    pub fn map_enabled(&self, map: usize) -> bool {
        self.actions
            .get(map)
            .is_some_and(|a| !a.is_empty() && a.iter().all(|s| s.enabled))
    }

    pub fn any_enabled(&self) -> bool {
        self.actions.iter().flatten().any(|s| s.enabled)
    }

    /// Enable or disable one action. Disabling an action in progress cancels it.
    pub fn set_action_enabled(&mut self, map: usize, action: usize, on: bool) {
        let now = self.now;
        let Some(st) = self.actions.get_mut(map).and_then(|m| m.get_mut(action)) else {
            return;
        };
        if st.enabled == on {
            return;
        }
        st.enabled = on;
        if !on {
            if matches!(st.phase, Phase::Started | Phase::Performed) {
                self.pending.push(ActionEvent {
                    map,
                    action,
                    phase: Phase::Canceled,
                    value: st.value.zero_like(),
                    time: now,
                    start_time: st.start_time,
                    interaction: None,
                    control: st.active_control.clone().unwrap_or_default(),
                });
            }
            let mut states = std::mem::take(&mut st.states);
            states
                .iter_mut()
                .flatten()
                .for_each(InteractionState::reset);
            let n = st.last.len();
            let value = zero_for(self.asset.maps[map].actions[action].control_type);
            *st = ActionState {
                phase: Phase::Disabled,
                value,
                states,
                last: vec![None; n],
                ..Default::default()
            };
        } else {
            st.phase = Phase::Waiting;
            st.prime = true;
        }
    }

    pub fn set_map_enabled(&mut self, map: usize, on: bool) {
        for a in 0..self.actions.get(map).map_or(0, Vec::len) {
            self.set_action_enabled(map, a, on);
        }
    }

    pub fn set_all_enabled(&mut self, on: bool) {
        for m in 0..self.actions.len() {
            self.set_map_enabled(m, on);
        }
    }

    /// Is this leaf used under the current binding mask?
    fn leaf_active(&self, leaf: &Leaf) -> bool {
        match &self.binding_mask {
            None => true,
            Some(mask) => leaf.groups.trim().is_empty() || has_group(&leaf.groups, mask),
        }
    }

    fn read_leaf(&self, leaf: &Leaf, input: &Input) -> Option<Value> {
        if !self.leaf_active(leaf) {
            return None;
        }
        let c = leaf.control.as_ref()?;
        Some(processors::apply(
            &leaf.processors,
            c.read(input, &self.devices),
        ))
    }

    /// The unit's value; None when no part of it is in use.
    fn read_unit(&self, unit: &Unit, input: &Input) -> Option<Value> {
        let raw = if unit.composite.is_empty() {
            self.read_leaf(&unit.leaves[0], input)?
        } else {
            let parts: Vec<(String, Option<Value>)> = unit
                .leaves
                .iter()
                .map(|l| (l.name.to_ascii_lowercase(), self.read_leaf(l, input)))
                .collect();
            if parts.iter().all(|(_, v)| v.is_none()) {
                return None;
            }
            composites::evaluate(&unit.composite, &unit.parameters, |name| {
                // Several bindings may share a part name: the strongest wins.
                parts
                    .iter()
                    .filter(|(n, _)| n == &name.to_ascii_lowercase())
                    .filter_map(|(_, v)| *v)
                    .fold(None::<Value>, |best, v| match best {
                        Some(b) if b.magnitude() >= v.magnitude() => Some(b),
                        _ => Some(v),
                    })
                    .unwrap_or_default()
            })
        };
        Some(processors::apply(&unit.processors, raw))
    }

    /// How far the most actuated binding of an action is pressed right now (enabled or not).
    pub fn actuation(&self, map: usize, action: usize, input: &Input) -> f32 {
        self.units
            .get(map)
            .and_then(|m| m.get(action))
            .map_or(0.0, |units| {
                units
                    .iter()
                    .filter_map(|u| self.read_unit(u, input))
                    .map(Value::magnitude)
                    .fold(0.0, f32::max)
            })
    }

    /// Step every enabled action with this frame's input at time `now` (seconds); returns the
    /// phase changes in order.
    pub fn update(&mut self, input: &Input, now: f64) -> Vec<ActionEvent> {
        self.update_from(input, None, now)
    }

    /// Like [`ActionSet::update`], with the previous frame's input: actions enabled since then
    /// take their pressed state from it (a key held while a map is switched on is not a press).
    pub fn update_from(
        &mut self,
        input: &Input,
        prev: Option<&Input>,
        now: f64,
    ) -> Vec<ActionEvent> {
        self.now = now;
        let mut events = std::mem::take(&mut self.pending);
        for m in 0..self.actions.len() {
            for a in 0..self.actions[m].len() {
                let st = &mut self.actions[m][a];
                st.pressed_this_frame = false;
                st.released_this_frame = false;
                st.performed_this_frame = false;
                st.started_this_frame = false;
                st.canceled_this_frame = false;
                if !st.enabled {
                    continue;
                }
                if std::mem::take(&mut st.prime)
                    && let Some(prev) = prev
                {
                    self.prime(m, a, prev);
                }
                self.step_action(m, a, input, now, &mut events);
            }
        }
        events
    }

    /// Take the pressed state of an action's controls (and interactions, and pass-through
    /// values) from `prev` without reporting anything.
    fn prime(&mut self, m: usize, a: usize, prev: &Input) {
        let control_type = self.asset.maps[m].actions[a].control_type;
        let values: Vec<Option<Value>> = self.units[m][a]
            .iter()
            .map(|u| self.read_unit(u, prev).map(|v| v.convert(control_type)))
            .collect();
        let magnitude = values
            .iter()
            .flatten()
            .map(|v| v.magnitude())
            .fold(0.0, f32::max);
        let units = &self.units[m][a];
        let st = &mut self.actions[m][a];
        st.pressed = magnitude >= DEFAULT_PRESS_POINT;
        for (u, unit) in units.iter().enumerate() {
            let mag = values[u].map_or(0.0, Value::magnitude);
            for (k, it) in unit.interactions.iter().enumerate() {
                st.states[u][k].prime(it, mag);
            }
        }
        if self.asset.maps[m].actions[a].kind == ActionType::PassThrough {
            st.last = values;
        }
    }

    fn step_action(
        &mut self,
        m: usize,
        a: usize,
        input: &Input,
        now: f64,
        events: &mut Vec<ActionEvent>,
    ) {
        let def = &self.asset.maps[m].actions[a];
        let kind = def.kind;
        let control_type = def.control_type;
        let values: Vec<Option<Value>> = self.units[m][a]
            .iter()
            .map(|u| self.read_unit(u, input).map(|v| v.convert(control_type)))
            .collect();
        let paths: Vec<String> = self.units[m][a]
            .iter()
            .map(|u| {
                if u.composite.is_empty() {
                    u.leaves[0].path.clone()
                } else {
                    u.composite.clone()
                }
            })
            .collect();
        // The most actuated binding drives the action.
        let mut best: Option<(usize, Value)> = None;
        for (u, v) in values.iter().enumerate() {
            if let Some(v) = v
                && best.is_none_or(|(_, b)| v.magnitude() > b.magnitude())
            {
                best = Some((u, *v));
            }
        }
        let zero = match control_type {
            ControlType::Vector2 => Value::Vector2(glam::Vec2::ZERO),
            ControlType::Any => best.map_or(Value::Float(0.0), |(_, v)| v.zero_like()),
            _ => Value::Float(0.0),
        };
        let (best_u, best_v) = best.unwrap_or((usize::MAX, zero));
        let magnitude = best_v.magnitude();
        let has_interactions = self.units[m][a].iter().any(|u| !u.interactions.is_empty());

        let st = &mut self.actions[m][a];
        let was = st.pressed;
        let pressed = if was {
            magnitude >= DEFAULT_PRESS_POINT * RELEASE_RATIO
        } else {
            magnitude >= DEFAULT_PRESS_POINT
        };
        st.pressed = pressed;
        st.pressed_this_frame = pressed && !was;
        st.released_this_frame = !pressed && was;
        if kind != ActionType::PassThrough {
            st.value = best_v;
            st.magnitude = magnitude;
            if best_u != usize::MAX && magnitude > 0.0 {
                st.active_control = Some(paths[best_u].clone());
            }
        }
        let mut emit = |st: &mut ActionState,
                        phase: Phase,
                        value: Value,
                        interaction: Option<&'static str>,
                        control: String| {
            match phase {
                Phase::Started => st.started_this_frame = true,
                Phase::Performed => st.performed_this_frame = true,
                Phase::Canceled => st.canceled_this_frame = true,
                _ => {}
            }
            events.push(ActionEvent {
                map: m,
                action: a,
                phase,
                value,
                time: now,
                start_time: st.start_time,
                interaction,
                control,
            });
        };

        if has_interactions {
            let units = &self.units[m][a];
            let st = &mut self.actions[m][a];
            for u in 0..units.len() {
                let mag = values[u].map_or(0.0, Value::magnitude);
                let value = values[u].unwrap_or(zero);
                for k in 0..units[u].interactions.len() {
                    let it = &units[u].interactions[k];
                    let transitions = st.states[u][k].step(it, mag, now);
                    if transitions.is_empty() {
                        continue;
                    }
                    if st.driver.is_some_and(|d| d != (u, k)) {
                        // Another interaction drives the action.
                        continue;
                    }
                    if st.driver.is_none() {
                        st.driver = Some((u, k));
                        st.start_time = st.states[u][k].start_time;
                    }
                    for t in transitions {
                        match t {
                            Transition::Started => {
                                st.phase = Phase::Started;
                                emit(st, Phase::Started, value, Some(it.name()), paths[u].clone());
                            }
                            Transition::Performed => {
                                st.phase = Phase::Performed;
                                emit(
                                    st,
                                    Phase::Performed,
                                    value,
                                    Some(it.name()),
                                    paths[u].clone(),
                                );
                            }
                            Transition::Canceled => {
                                // Another interaction that already started takes over.
                                let next = (0..units.len())
                                    .flat_map(|u2| {
                                        (0..units[u2].interactions.len()).map(move |k2| (u2, k2))
                                    })
                                    .find(|&(u2, k2)| {
                                        (u2, k2) != (u, k)
                                            && st.states[u2][k2].phase == Phase::Started
                                    });
                                if let Some(n) = next {
                                    st.driver = Some(n);
                                    st.phase = Phase::Started;
                                } else {
                                    st.phase = Phase::Waiting;
                                    emit(
                                        st,
                                        Phase::Canceled,
                                        value.zero_like(),
                                        Some(it.name()),
                                        paths[u].clone(),
                                    );
                                }
                            }
                        }
                    }
                    if st.driver == Some((u, k)) && st.states[u][k].phase == Phase::Waiting {
                        // Finished: the action waits again and the others start over.
                        st.driver = None;
                        st.phase = Phase::Waiting;
                        for (u2, list) in st.states.iter_mut().enumerate() {
                            for (k2, s) in list.iter_mut().enumerate() {
                                if (u2, k2) != (u, k) {
                                    s.reset();
                                }
                            }
                        }
                    }
                }
            }
            return;
        }

        let control = if best_u == usize::MAX {
            st.active_control.clone().unwrap_or_default()
        } else {
            paths[best_u].clone()
        };
        match kind {
            ActionType::Button => {
                if st.pressed_this_frame {
                    if st.phase == Phase::Waiting {
                        st.start_time = now;
                        st.phase = Phase::Started;
                        emit(st, Phase::Started, best_v, None, control.clone());
                    }
                    st.phase = Phase::Performed;
                    emit(st, Phase::Performed, best_v, None, control);
                } else if st.phase == Phase::Waiting && magnitude > 0.0 && !pressed {
                    // An analog control moving towards the press point.
                    st.start_time = now;
                    st.phase = Phase::Started;
                    emit(st, Phase::Started, best_v, None, control);
                } else if (st.phase == Phase::Performed && !pressed)
                    || (st.phase == Phase::Started && magnitude == 0.0)
                {
                    st.phase = Phase::Waiting;
                    emit(st, Phase::Canceled, best_v.zero_like(), None, control);
                }
            }
            ActionType::Value => {
                if magnitude > 0.0 {
                    if st.phase == Phase::Waiting {
                        st.start_time = now;
                        st.phase = Phase::Started;
                        st.last = vec![None; values.len()];
                        emit(st, Phase::Started, best_v, None, control.clone());
                        st.phase = Phase::Performed;
                        emit(st, Phase::Performed, best_v, None, control);
                        st.last[0] = Some(best_v);
                    } else if st.last.first().copied().flatten() != Some(best_v) {
                        st.phase = Phase::Performed;
                        emit(st, Phase::Performed, best_v, None, control);
                        if let Some(l) = st.last.first_mut() {
                            *l = Some(best_v);
                        }
                    }
                } else if st.phase != Phase::Waiting {
                    st.phase = Phase::Waiting;
                    if let Some(l) = st.last.first_mut() {
                        *l = None;
                    }
                    emit(st, Phase::Canceled, best_v.zero_like(), None, control);
                }
            }
            ActionType::PassThrough => {
                for (u, v) in values.iter().enumerate() {
                    let Some(v) = *v else { continue };
                    let before = st.last.get(u).copied().flatten();
                    // The first frame only reports controls that are actuated.
                    let changed = match before {
                        Some(b) => b != v,
                        None => v.magnitude() > 0.0,
                    };
                    if let Some(l) = st.last.get_mut(u) {
                        *l = Some(v);
                    }
                    if changed {
                        st.value = v;
                        st.magnitude = v.magnitude();
                        st.active_control = Some(paths[u].clone());
                        st.phase = Phase::Performed;
                        emit(st, Phase::Performed, v, None, paths[u].clone());
                    }
                }
            }
        }
    }

    /// The path a flat binding index uses (override first). Composites give "".
    pub fn effective_path(&self, map: usize, action: usize, flat: usize) -> Option<String> {
        let (id, path) = self.flat_entry(map, action, flat)?;
        Some(self.overrides.get(&id).cloned().unwrap_or(path))
    }

    /// (id, authored path) of a flat binding index.
    pub fn flat_entry(&self, map: usize, action: usize, flat: usize) -> Option<(String, String)> {
        let def = self.asset.maps.get(map)?.actions.get(action)?;
        let f = *def.flat_bindings().get(flat)?;
        let b = &def.bindings[f.binding];
        Some(match f.part {
            None => (b.id.clone(), b.path.clone()),
            Some(p) => (b.parts[p].id.clone(), b.parts[p].path.clone()),
        })
    }

    /// Override a flat binding's path (`ApplyBindingOverride`); "" or the authored path clears.
    pub fn set_override(&mut self, map: usize, action: usize, flat: usize, path: &str) -> bool {
        let Some((id, authored)) = self.flat_entry(map, action, flat) else {
            return false;
        };
        if path.is_empty() || path == authored {
            self.overrides.remove(&id);
        } else {
            self.overrides.insert(id, path.to_owned());
        }
        self.rebuild();
        true
    }

    /// Remove every override of one action (None = all actions).
    pub fn clear_overrides(&mut self, action: Option<(usize, usize)>) {
        match action {
            None => self.overrides.clear(),
            Some((m, a)) => {
                let n = self.asset.maps[m].actions[a].flat_bindings().len();
                for flat in 0..n {
                    if let Some((id, _)) = self.flat_entry(m, a, flat) {
                        self.overrides.remove(&id);
                    }
                }
            }
        }
        self.rebuild();
    }

    /// `SaveBindingOverridesAsJson`.
    pub fn overrides_json(&self) -> String {
        let mut file = OverrideFile::default();
        for map in &self.asset.maps {
            for action in &map.actions {
                for b in &action.bindings {
                    let ids = std::iter::once(&b.id).chain(b.parts.iter().map(|p| &p.id));
                    for id in ids {
                        if let Some(path) = self.overrides.get(id) {
                            file.bindings.push(OverrideEntry {
                                action: format!("{}/{}", map.name, action.name),
                                id: id.clone(),
                                path: path.clone(),
                            });
                        }
                    }
                }
            }
        }
        serde_json::to_string(&file).unwrap_or_default()
    }

    /// `LoadBindingOverridesFromJson`: replaces the overrides; unknown ids are skipped.
    pub fn load_overrides_json(&mut self, text: &str) -> Result<usize, String> {
        if text.trim().is_empty() {
            self.overrides.clear();
            self.rebuild();
            return Ok(0);
        }
        let file: OverrideFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let known: std::collections::HashSet<&String> = self
            .asset
            .maps
            .iter()
            .flat_map(|m| &m.actions)
            .flat_map(|a| &a.bindings)
            .flat_map(|b| std::iter::once(&b.id).chain(b.parts.iter().map(|p| &p.id)))
            .collect();
        let mut overrides = BTreeMap::new();
        for e in file.bindings {
            if known.contains(&e.id) {
                overrides.insert(e.id, e.path);
            }
        }
        let n = overrides.len();
        self.overrides = overrides;
        self.rebuild();
        Ok(n)
    }

    /// Display text of a flat binding (`GetBindingDisplayString`): "W", "W/A/S/D" for a 2D
    /// composite, "Shift+B" for a modifier composite.
    pub fn display_string(&self, map: usize, action: usize, flat: Option<usize>) -> String {
        let Some(def) = self.asset.maps.get(map).and_then(|m| m.actions.get(action)) else {
            return String::new();
        };
        let flats = def.flat_bindings();
        let pick: Vec<usize> = match flat {
            Some(f) => vec![f],
            None => (0..flats.len())
                .filter(|&f| flats[f].part.is_none())
                .filter(|&f| {
                    let b = &def.bindings[flats[f].binding];
                    let ungrouped = if b.is_composite() {
                        b.parts.iter().all(|p| p.groups.trim().is_empty())
                    } else {
                        b.groups.trim().is_empty()
                    };
                    self.binding_mask
                        .as_ref()
                        .is_none_or(|m| ungrouped || b.in_group(m))
                })
                .collect(),
        };
        let one = |f: usize| -> String {
            let Some(fb) = flats.get(f) else {
                return String::new();
            };
            let b = &def.bindings[fb.binding];
            if let Some(p) = fb.part {
                let part = &b.parts[p];
                let path = self.overrides.get(&part.id).unwrap_or(&part.path);
                return super::path::short_name(path);
            }
            if !b.is_composite() {
                let path = self.overrides.get(&b.id).unwrap_or(&b.path);
                return super::path::short_name(path);
            }
            let names: Vec<String> = b
                .parts
                .iter()
                .map(|p| super::path::short_name(self.overrides.get(&p.id).unwrap_or(&p.path)))
                .collect();
            let sep = if b.composite.to_ascii_lowercase().contains("modifier") {
                "+"
            } else {
                "/"
            };
            names.join(sep)
        };
        pick.into_iter()
            .map(one)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

/// The default value an action of this control type reads (a zero vector for Vector 2).
fn zero_for(ct: ControlType) -> Value {
    match ct {
        ControlType::Vector2 => Value::Vector2(glam::Vec2::ZERO),
        _ => Value::Float(0.0),
    }
}
