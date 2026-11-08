//! The running Input System of a game: every loaded action set (shared asset copies loaded by
//! scripts and one per Player Input), players with their control schemes and paired devices,
//! interactive rebinding, gamepad rumble requests, and the clock interactions time against.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use hecs::Entity;
use serde::{Deserialize, Serialize};

use super::asset::{ActionType, ControlType, InputActionAsset};
use super::path::{
    ControlPath, DeviceFilter, DeviceKind, keyboard_path, normalize, pad_button_path,
};
use super::player_input::{PlayerInput, PlayerInputBehavior};
use super::state::{ActionEvent, ActionSet};
use crate::scene::Scene;
use crate::scripting::Input;

/// Project Settings ▸ Input Manager ▸ Active Input Handling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActiveInputHandling {
    /// Only `Input.GetAxis` / `GetKey` …; actions never read devices.
    InputManager,
    /// Only actions; the Input Manager functions raise an error.
    InputSystem,
    /// Both work (the default).
    #[default]
    Both,
}

impl ActiveInputHandling {
    pub const ALL: [Self; 3] = [Self::InputManager, Self::InputSystem, Self::Both];

    pub fn label(self) -> &'static str {
        match self {
            Self::InputManager => "Input Manager (Old)",
            Self::InputSystem => "Input System Package (New)",
            Self::Both => "Both",
        }
    }

    pub fn is_both(&self) -> bool {
        *self == Self::Both
    }

    pub fn legacy_allowed(self) -> bool {
        self != Self::InputSystem
    }

    pub fn actions_allowed(self) -> bool {
        self != Self::InputManager
    }
}

/// A device a player can be paired with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeviceId {
    Keyboard,
    Mouse,
    Touchscreen,
    /// Joystick slot (0-based).
    Gamepad(usize),
}

impl DeviceId {
    pub fn kind(self) -> DeviceKind {
        match self {
            DeviceId::Keyboard => DeviceKind::Keyboard,
            DeviceId::Mouse => DeviceKind::Mouse,
            DeviceId::Touchscreen => DeviceKind::Touchscreen,
            DeviceId::Gamepad(_) => DeviceKind::Gamepad,
        }
    }

    /// "Keyboard", "Gamepad 1 (Xbox Controller)".
    pub fn name(self, input: &Input) -> String {
        match self {
            DeviceId::Gamepad(i) => match input.joysticks.get(i).map(|j| j.name.as_str()) {
                Some(n) if !n.is_empty() => format!("Gamepad {} ({n})", i + 1),
                _ => format!("Gamepad {}", i + 1),
            },
            other => other.kind().name().to_owned(),
        }
    }
}

/// A Player Input that is playing.
#[derive(Clone, Debug)]
pub struct PlayerState {
    pub entity: Entity,
    /// Its action set.
    pub set: usize,
    /// 0-based, in Hierarchy order.
    pub index: usize,
    pub scheme: Option<String>,
    pub devices: Vec<DeviceId>,
    /// Current action map.
    pub map: Option<usize>,
    /// `ActivateInput` / `DeactivateInput`.
    pub active: bool,
    pub behavior: PlayerInputBehavior,
    pub auto_switch: bool,
    default_scheme: String,
    lost: Vec<DeviceId>,
}

/// An interactive rebinding (`PerformInteractiveRebinding`).
#[derive(Clone, Debug)]
pub struct Rebind {
    pub id: u64,
    pub set: usize,
    pub map: usize,
    pub action: usize,
    pub flat: usize,
    /// Controls never taken (`WithControlsExcluding`): matched as path fragments.
    pub excluded: Vec<String>,
    /// Controls that cancel (`WithCancelingThrough`).
    pub cancel: Vec<String>,
    /// Seconds to wait for a better match (`OnMatchWaitForAnother`).
    pub wait: f64,
    pub started: bool,
    started_at: f64,
    candidate: Option<(String, f64)>,
    was_enabled: bool,
    pub expect_vector: bool,
}

/// Something scripts hear about after an Input System update.
#[derive(Clone, Debug, PartialEq)]
pub enum SystemEvent {
    Action { set: usize, event: ActionEvent },
    ControlsChanged(Entity),
    DeviceLost(Entity),
    DeviceRegained(Entity),
    RebindComplete { id: u64, path: String },
    RebindCanceled { id: u64 },
}

pub struct InputSystem {
    root: PathBuf,
    pub handling: ActiveInputHandling,
    sets: Vec<Option<ActionSet>>,
    shared: HashMap<String, usize>,
    pub players: Vec<PlayerState>,
    failed: HashSet<Entity>,
    /// Real seconds since Play (interactions time against it).
    pub now: f64,
    rebinds: Vec<Rebind>,
    next_rebind: u64,
    /// Actions waiting for the control that rebound them to be let go before they come back
    /// on (so the press that picked the binding doesn't also trigger it).
    reenable: Vec<(usize, usize, usize)>,
    /// Motor speeds (low, high) per gamepad slot as last asked.
    rumble: BTreeMap<usize, (f32, f32)>,
    rumble_changed: bool,
    /// Gamepad slot used most recently (`Gamepad.current`).
    pub last_pad: Option<usize>,
    /// A touch has been seen (`Touchscreen.current` exists).
    pub touch_seen: bool,
    connected: Vec<bool>,
    /// The previous frame's input (actions enabled since then start from it).
    prev_input: Input,
    /// Problems for the Console.
    pub logs: Vec<String>,
}

impl InputSystem {
    pub fn new(root: &Path, handling: ActiveInputHandling) -> Self {
        Self {
            root: root.to_path_buf(),
            handling,
            sets: Vec::new(),
            shared: HashMap::new(),
            players: Vec::new(),
            failed: HashSet::new(),
            now: 0.0,
            rebinds: Vec::new(),
            next_rebind: 1,
            reenable: Vec::new(),
            rumble: BTreeMap::new(),
            rumble_changed: false,
            last_pad: None,
            touch_seen: false,
            connected: Vec::new(),
            prev_input: Input::default(),
            logs: Vec::new(),
        }
    }

    pub fn set(&self, id: usize) -> Option<&ActionSet> {
        self.sets.get(id)?.as_ref()
    }

    pub fn set_mut(&mut self, id: usize) -> Option<&mut ActionSet> {
        self.sets.get_mut(id)?.as_mut()
    }

    /// Add a set made in code; returns its id.
    pub fn add_set(&mut self, set: ActionSet) -> usize {
        self.sets.push(Some(set));
        self.sets.len() - 1
    }

    fn load_asset(&self, path: &str) -> Result<InputActionAsset, String> {
        InputActionAsset::load(&self.root.join(path))
    }

    /// The shared copy of an asset (`InputActions.Load`): every caller gets the same set.
    pub fn load_shared(&mut self, path: &str) -> Result<usize, String> {
        let key = path.replace('\\', "/");
        if let Some(id) = self.shared.get(&key) {
            return Ok(*id);
        }
        let asset = self.load_asset(&key)?;
        let id = self.add_set(ActionSet::new(asset, &key));
        self.shared.insert(key, id);
        Ok(id)
    }

    /// A new, separate copy (`InputActions.Instantiate`).
    pub fn instantiate(&mut self, path: &str) -> Result<usize, String> {
        let key = path.replace('\\', "/");
        let asset = self.load_asset(&key)?;
        Ok(self.add_set(ActionSet::new(asset, &key)))
    }

    pub fn player(&self, e: Entity) -> Option<&PlayerState> {
        self.players.iter().find(|p| p.entity == e)
    }

    fn player_mut(&mut self, e: Entity) -> Option<&mut PlayerState> {
        self.players.iter_mut().find(|p| p.entity == e)
    }

    /// Start Player Inputs that appeared, stop the ones that went away (objects destroyed,
    /// deactivated, or the component removed) and pair devices when the players change.
    pub fn sync_players(&mut self, scene: &Scene, input: &Input) {
        let present: Vec<(Entity, PlayerInput)> = scene
            .walk()
            .into_iter()
            .filter(|(e, _)| scene.active_in_hierarchy(*e))
            .filter_map(|(e, _)| {
                scene
                    .world
                    .get::<&PlayerInput>(e)
                    .ok()
                    .map(|c| (e, (*c).clone()))
            })
            .collect();
        let before: Vec<Entity> = self.players.iter().map(|p| p.entity).collect();
        let gone: Vec<usize> = self
            .players
            .iter()
            .enumerate()
            .filter(|(_, p)| !present.iter().any(|(e, _)| *e == p.entity))
            .map(|(i, _)| i)
            .collect();
        for i in gone.into_iter().rev() {
            let p = self.players.remove(i);
            if let Some(set) = self.set_mut(p.set) {
                set.set_all_enabled(false);
            }
        }
        for (e, c) in &present {
            if before.contains(e) || self.failed.contains(e) {
                continue;
            }
            match self.start_player(*e, c) {
                Ok(p) => self.players.push(p),
                Err(err) => {
                    self.failed.insert(*e);
                    let name = scene.name(*e);
                    self.logs.push(format!("Player Input on '{name}': {err}"));
                }
            }
        }
        // Keep Hierarchy order and number the players.
        self.players.sort_by_key(|p| {
            present
                .iter()
                .position(|(e, _)| *e == p.entity)
                .unwrap_or(usize::MAX)
        });
        let after: Vec<Entity> = self.players.iter().map(|p| p.entity).collect();
        for (i, p) in self.players.iter_mut().enumerate() {
            p.index = i;
        }
        if after != before {
            self.pair_all(input);
        }
    }

    fn start_player(&mut self, e: Entity, c: &PlayerInput) -> Result<PlayerState, String> {
        let path = c
            .actions
            .clone()
            .filter(|p| !p.trim().is_empty())
            .ok_or("no Actions asset assigned")?;
        // The first player uses the shared copy; more players get their own.
        let shared_used = self.players.iter().any(|p| {
            self.set(p.set)
                .is_some_and(|s| s.path == path.replace('\\', "/"))
        });
        let set = if shared_used {
            self.instantiate(&path)?
        } else {
            self.load_shared(&path)?
        };
        let s = self.set_mut(set).expect("just loaded");
        let map = if c.default_map.trim().is_empty() {
            (!s.asset.maps.is_empty()).then_some(0)
        } else {
            s.asset.find_map(&c.default_map)
        };
        if let Some(m) = map {
            s.set_map_enabled(m, true);
        }
        Ok(PlayerState {
            entity: e,
            set,
            index: 0,
            scheme: None,
            devices: Vec::new(),
            map,
            active: true,
            behavior: c.behavior,
            auto_switch: c.auto_switch,
            default_scheme: c.default_control_scheme.clone(),
            lost: Vec::new(),
        })
    }

    /// Devices present this frame.
    fn available(&self, input: &Input) -> Vec<DeviceId> {
        let mut out = vec![DeviceId::Keyboard, DeviceId::Mouse];
        out.extend(
            input
                .joysticks
                .iter()
                .enumerate()
                .filter(|(_, j)| !j.name.is_empty())
                .map(|(i, _)| DeviceId::Gamepad(i)),
        );
        if self.touch_seen || !input.touches.is_empty() {
            out.push(DeviceId::Touchscreen);
        }
        out
    }

    /// Pick devices for a scheme from `free`; None when a required device is missing.
    fn devices_for(set: &ActionSet, scheme: usize, free: &[DeviceId]) -> Option<Vec<DeviceId>> {
        let s = &set.asset.control_schemes[scheme];
        let mut taken = Vec::new();
        for req in &s.devices {
            let kind = req.path.trim_matches(|c| c == '<' || c == '>');
            let found = free
                .iter()
                .find(|d| d.kind().name().eq_ignore_ascii_case(kind) && !taken.contains(*d));
            match found {
                Some(d) => taken.push(*d),
                None if req.optional => {}
                None => return None,
            }
        }
        Some(taken)
    }

    /// Pair every player: alone, a player may use every device (the binding mask follows the
    /// control scheme); with several players each gets its own devices, in player order.
    fn pair_all(&mut self, input: &Input) {
        let available = self.available(input);
        let single = self.players.len() == 1;
        let mut free = available.clone();
        for i in 0..self.players.len() {
            let (set_id, default_scheme) =
                (self.players[i].set, self.players[i].default_scheme.clone());
            let Some(set) = self.sets.get(set_id).and_then(Option::as_ref) else {
                continue;
            };
            let mut order: Vec<usize> = (0..set.asset.control_schemes.len()).collect();
            if let Some(d) = set.asset.find_scheme(&default_scheme) {
                order.retain(|s| *s != d);
                order.insert(0, d);
            }
            // Alone with a gamepad plugged in and no default: start on the scheme of whatever
            // comes first in the asset.
            let pool = if single {
                available.clone()
            } else {
                free.clone()
            };
            let pick = order
                .iter()
                .find_map(|s| Self::devices_for(set, *s, &pool).map(|d| (*s, d)));
            let (scheme, group, devices) = match pick {
                Some((s, d)) => {
                    let cs = &set.asset.control_schemes[s];
                    (Some(cs.name.clone()), Some(cs.group().to_owned()), d)
                }
                None if set.asset.control_schemes.is_empty() => (None, None, pool.clone()),
                None => (None, None, Vec::new()),
            };
            if !single {
                free.retain(|d| !devices.contains(d));
            }
            let filter = if single {
                DeviceFilter::all()
            } else {
                filter_of(&devices)
            };
            let set = self.sets[set_id].as_mut().expect("set exists");
            set.binding_mask = group;
            set.devices = filter;
            let p = &mut self.players[i];
            p.scheme = scheme;
            p.devices = if single { available.clone() } else { devices };
            p.lost.clear();
        }
        self.connected = connected(input);
    }

    /// `SwitchCurrentControlScheme`: the named scheme with the devices it needs (a player's own
    /// devices plus free ones).
    pub fn switch_scheme(&mut self, e: Entity, name: &str, input: &Input) -> Result<(), String> {
        let single = self.players.len() == 1;
        let Some(pi) = self.players.iter().position(|p| p.entity == e) else {
            return Err("not a playing Player Input".into());
        };
        let set_id = self.players[pi].set;
        let set = self.set(set_id).ok_or("no actions")?;
        let s = set
            .asset
            .find_scheme(name)
            .ok_or_else(|| format!("no control scheme '{name}'"))?;
        let others: Vec<DeviceId> = self
            .players
            .iter()
            .filter(|p| p.entity != e)
            .flat_map(|p| p.devices.clone())
            .collect();
        let pool: Vec<DeviceId> = self
            .available(input)
            .into_iter()
            .filter(|d| single || !others.contains(d))
            .collect();
        let devices = Self::devices_for(set, s, &pool)
            .ok_or_else(|| format!("the devices of '{name}' are not available"))?;
        let cs = &set.asset.control_schemes[s];
        let (scheme, group) = (cs.name.clone(), cs.group().to_owned());
        let set = self.set_mut(set_id).expect("set");
        set.binding_mask = Some(group);
        set.devices = if single {
            DeviceFilter::all()
        } else {
            filter_of(&devices)
        };
        let p = &mut self.players[pi];
        p.scheme = Some(scheme);
        if !single {
            p.devices = devices;
        }
        Ok(())
    }

    /// `SwitchCurrentActionMap`: disable the current map, enable `name`.
    pub fn switch_map(&mut self, e: Entity, name: &str) -> Result<(), String> {
        let p = self.player(e).ok_or("not a playing Player Input")?.clone();
        let set = self.set_mut(p.set).ok_or("no actions")?;
        let m = set
            .asset
            .find_map(name)
            .ok_or_else(|| format!("no action map '{name}'"))?;
        if let Some(old) = p.map {
            set.set_map_enabled(old, false);
        }
        if p.active {
            set.set_map_enabled(m, true);
        }
        self.player_mut(e).expect("player").map = Some(m);
        Ok(())
    }

    /// `ActivateInput` / `DeactivateInput`.
    pub fn set_player_active(&mut self, e: Entity, on: bool) {
        let Some(p) = self.player(e).cloned() else {
            return;
        };
        if let (Some(m), Some(set)) = (p.map, self.set_mut(p.set)) {
            set.set_map_enabled(m, on);
        }
        if let Some(p) = self.player_mut(e) {
            p.active = on;
        }
    }

    /// Which devices were used this frame.
    fn activity(input: &Input) -> Vec<DeviceId> {
        let mut out = Vec::new();
        if !input.pressed.is_empty() {
            out.push(DeviceId::Keyboard);
        }
        if input.mouse_pressed.iter().any(|b| *b)
            || input.scroll_delta != glam::Vec2::ZERO
            || input.mouse_delta.length() > 3.0
        {
            out.push(DeviceId::Mouse);
        }
        for (i, j) in input.joysticks.iter().enumerate() {
            if !j.name.is_empty()
                && (j.pressed.iter().any(|b| *b) || j.axes.iter().any(|a| a.abs() > 0.5))
            {
                out.push(DeviceId::Gamepad(i));
            }
        }
        if input
            .touches
            .iter()
            .any(|t| t.phase == crate::input::touch::TouchPhase::Began)
        {
            out.push(DeviceId::Touchscreen);
        }
        out
    }

    /// Step everything for one frame of `dt` real seconds; returns what scripts should hear.
    pub fn update(&mut self, scene: &Scene, input: &Input, dt: f32) -> Vec<SystemEvent> {
        self.now += f64::from(dt.max(0.0));
        let mut events = Vec::new();
        if !input.touches.is_empty() {
            self.touch_seen = true;
        }
        let used = Self::activity(input);
        if let Some(DeviceId::Gamepad(i)) = used.iter().find(|d| matches!(d, DeviceId::Gamepad(_)))
        {
            self.last_pad = Some(*i);
        }
        if self.last_pad.is_none() {
            self.last_pad = input.joysticks.iter().position(|j| !j.name.is_empty());
        }
        self.sync_players(scene, input);
        self.devices_frame(input, &used, &mut events);
        self.rebind_frame(input, &mut events);
        let mut waiting = std::mem::take(&mut self.reenable);
        waiting.retain(|&(set, map, action)| {
            let Some(s) = self.sets.get_mut(set).and_then(Option::as_mut) else {
                return false;
            };
            if s.actuation(map, action, input) > 0.0 {
                return true;
            }
            s.set_action_enabled(map, action, true);
            false
        });
        self.reenable = waiting;
        if self.handling.actions_allowed() {
            let now = self.now;
            let prev = &self.prev_input;
            for (id, set) in self.sets.iter_mut().enumerate() {
                let Some(set) = set else { continue };
                for event in set.update_from(input, Some(prev), now) {
                    events.push(SystemEvent::Action { set: id, event });
                }
            }
        }
        self.prev_input = input.clone();
        events
    }

    /// Lost / regained gamepads, pairing new pads, and control scheme auto-switching.
    fn devices_frame(&mut self, input: &Input, used: &[DeviceId], events: &mut Vec<SystemEvent>) {
        let now_connected = connected(input);
        let newly: Vec<usize> = now_connected
            .iter()
            .enumerate()
            .filter(|(i, c)| **c && !self.connected.get(*i).copied().unwrap_or(false))
            .map(|(i, _)| i)
            .collect();
        for p in &mut self.players {
            for d in p.devices.clone() {
                if let DeviceId::Gamepad(i) = d {
                    let here = now_connected.get(i).copied().unwrap_or(false);
                    if !here && !p.lost.contains(&d) {
                        p.lost.push(d);
                        events.push(SystemEvent::DeviceLost(p.entity));
                    } else if here && p.lost.contains(&d) {
                        p.lost.retain(|x| *x != d);
                        events.push(SystemEvent::DeviceRegained(p.entity));
                    }
                }
            }
        }
        self.connected = now_connected;
        let single = self.players.len() == 1;
        if !newly.is_empty() {
            if single {
                let available = self.available(input);
                self.players[0].devices = available;
            } else if self
                .players
                .iter()
                .any(|p| p.scheme.is_none() || p.devices.is_empty())
            {
                // A player was waiting for a device.
                self.pair_all(input);
                for p in &self.players {
                    events.push(SystemEvent::ControlsChanged(p.entity));
                }
            }
        }
        if !single || !self.players[0].auto_switch || used.is_empty() {
            return;
        }
        let set_id = self.players[0].set;
        let Some(set) = self.set(set_id) else { return };
        let current = self.players[0]
            .scheme
            .as_ref()
            .and_then(|s| set.asset.find_scheme(s));
        let fits = |s: usize, d: DeviceId| {
            set.asset.control_schemes[s].devices.iter().any(|r| {
                r.path
                    .trim_matches(|c| c == '<' || c == '>')
                    .eq_ignore_ascii_case(d.kind().name())
            })
        };
        if current.is_some_and(|c| used.iter().any(|d| fits(c, *d))) {
            return;
        }
        let pool = self.available(input);
        let switch = used.iter().find_map(|d| {
            (0..set.asset.control_schemes.len())
                .find(|s| fits(*s, *d) && Self::devices_for(set, *s, &pool).is_some())
        });
        if let Some(s) = switch {
            let cs = &set.asset.control_schemes[s];
            let (name, group) = (cs.name.clone(), cs.group().to_owned());
            if let Some(set) = self.set_mut(set_id) {
                set.binding_mask = Some(group);
            }
            self.players[0].scheme = Some(name);
            events.push(SystemEvent::ControlsChanged(self.players[0].entity));
        }
    }

    // ---- rebinding -----------------------------------------------------------------------

    /// A new rebinding operation for a flat binding of an action (not started yet).
    pub fn new_rebind(
        &mut self,
        set: usize,
        map: usize,
        action: usize,
        flat: Option<usize>,
    ) -> Result<u64, String> {
        let s = self.set(set).ok_or("no such actions")?;
        let def = s
            .asset
            .maps
            .get(map)
            .and_then(|m| m.actions.get(action))
            .ok_or("no such action")?;
        let flats = def.flat_bindings();
        // Default: the first binding in the binding mask that isn't a composite.
        let flat = match flat {
            Some(f) => f,
            None => (0..flats.len())
                .find(|&f| {
                    let b = &def.bindings[flats[f].binding];
                    let part_or_plain = flats[f].part.is_some() || !b.is_composite();
                    part_or_plain && s.binding_mask.as_ref().is_none_or(|m| b.in_group(m))
                })
                .ok_or("the action has no binding to rebind")?,
        };
        let fb = *flats.get(flat).ok_or("binding index out of range")?;
        if fb.part.is_none() && def.bindings[fb.binding].is_composite() {
            return Err("can't rebind a composite itself; rebind one of its parts".into());
        }
        let expect_vector = fb.part.is_none()
            && (def.control_type == ControlType::Vector2
                || (def.kind == ActionType::Value
                    && ControlPath::parse(&def.bindings[fb.binding].path)
                        .is_ok_and(|c| c.is_vector())));
        let id = self.next_rebind;
        self.next_rebind += 1;
        self.rebinds.push(Rebind {
            id,
            set,
            map,
            action,
            flat,
            excluded: vec!["<Mouse>/position".into(), "<Mouse>/delta".into()],
            cancel: vec!["<Keyboard>/escape".into()],
            wait: 0.0,
            started: false,
            started_at: 0.0,
            candidate: None,
            was_enabled: false,
            expect_vector,
        });
        Ok(id)
    }

    pub fn rebind_mut(&mut self, id: u64) -> Option<&mut Rebind> {
        self.rebinds.iter_mut().find(|r| r.id == id)
    }

    pub fn rebind(&self, id: u64) -> Option<&Rebind> {
        self.rebinds.iter().find(|r| r.id == id)
    }

    /// Start listening: the action is disabled until the rebind ends.
    pub fn start_rebind(&mut self, id: u64) {
        let now = self.now;
        let Some(r) = self.rebinds.iter_mut().find(|r| r.id == id) else {
            return;
        };
        if r.started {
            return;
        }
        r.started = true;
        r.started_at = now;
        let (set, map, action) = (r.set, r.map, r.action);
        let enabled = self
            .set(set)
            .and_then(|s| s.state(map, action))
            .is_some_and(|st| st.enabled);
        if let Some(r) = self.rebind_mut(id) {
            r.was_enabled = enabled;
        }
        if let Some(s) = self.set_mut(set) {
            s.set_action_enabled(map, action, false);
        }
    }

    /// End a rebind; `path` = the new binding (None = canceled).
    fn finish_rebind(&mut self, id: u64, path: Option<String>, events: &mut Vec<SystemEvent>) {
        let Some(i) = self.rebinds.iter().position(|r| r.id == id) else {
            return;
        };
        let r = self.rebinds.remove(i);
        if let Some(s) = self.set_mut(r.set)
            && let Some(p) = &path
        {
            s.set_override(r.map, r.action, r.flat, p);
        }
        if r.was_enabled {
            self.reenable.push((r.set, r.map, r.action));
        }
        events.push(match path {
            Some(path) => SystemEvent::RebindComplete { id, path },
            None => SystemEvent::RebindCanceled { id },
        });
    }

    /// `Cancel()` from a script.
    pub fn cancel_rebind(&mut self, id: u64) -> Vec<SystemEvent> {
        let mut events = Vec::new();
        let started = self.rebind(id).is_some_and(|r| r.started);
        if started {
            self.finish_rebind(id, None, &mut events);
        } else {
            self.rebinds.retain(|r| r.id != id);
        }
        events
    }

    fn rebind_frame(&mut self, input: &Input, events: &mut Vec<SystemEvent>) {
        let ids: Vec<u64> = self
            .rebinds
            .iter()
            .filter(|r| r.started && self.now > r.started_at)
            .map(|r| r.id)
            .collect();
        for id in ids {
            let Some(r) = self.rebind(id).cloned() else {
                continue;
            };
            let found = candidates(input, r.expect_vector);
            let lower = |s: &str| s.to_ascii_lowercase();
            if found
                .iter()
                .any(|c| r.cancel.iter().any(|x| lower(&normalize(x)) == lower(c)))
            {
                self.finish_rebind(id, None, events);
                continue;
            }
            let pick = found.into_iter().find(|c| {
                !r.excluded
                    .iter()
                    .any(|x| !x.is_empty() && lower(c).contains(&lower(x)))
            });
            let now = self.now;
            match (pick, r.candidate.clone()) {
                (Some(p), _) if r.wait <= 0.0 => self.finish_rebind(id, Some(p), events),
                (Some(p), _) => {
                    if let Some(rr) = self.rebind_mut(id) {
                        rr.candidate = Some((p, now));
                    }
                }
                (None, Some((p, at))) if now - at >= r.wait => {
                    self.finish_rebind(id, Some(p), events)
                }
                _ => {}
            }
        }
    }

    // ---- haptics -------------------------------------------------------------------------

    /// `SetMotorSpeeds(low, high)` on a gamepad slot (None = the current gamepad).
    pub fn set_motor_speeds(&mut self, pad: Option<usize>, low: f32, high: f32) {
        let Some(pad) = pad.or(self.last_pad) else {
            return;
        };
        let v = (low.clamp(0.0, 1.0), high.clamp(0.0, 1.0));
        if self.rumble.get(&pad) != Some(&v) {
            self.rumble.insert(pad, v);
            self.rumble_changed = true;
        }
    }

    /// `ResetHaptics` on every pad (also when Play stops).
    pub fn reset_haptics(&mut self) {
        for v in self.rumble.values_mut() {
            *v = (0.0, 0.0);
        }
        self.rumble_changed = true;
    }

    pub fn motor_speeds(&self, pad: usize) -> (f32, f32) {
        self.rumble.get(&pad).copied().unwrap_or((0.0, 0.0))
    }

    /// Motor speeds per pad when they changed since the last call (the editor and player
    /// send them to the devices).
    pub fn take_rumble(&mut self) -> Option<Vec<(usize, f32, f32)>> {
        if !self.rumble_changed {
            return None;
        }
        self.rumble_changed = false;
        Some(self.rumble.iter().map(|(p, (l, h))| (*p, *l, *h)).collect())
    }
}

fn connected(input: &Input) -> Vec<bool> {
    input.joysticks.iter().map(|j| !j.name.is_empty()).collect()
}

fn filter_of(devices: &[DeviceId]) -> DeviceFilter {
    DeviceFilter {
        keyboard: devices.contains(&DeviceId::Keyboard),
        mouse: devices.contains(&DeviceId::Mouse),
        touch: devices.contains(&DeviceId::Touchscreen),
        gamepads: Some(
            devices
                .iter()
                .filter_map(|d| match d {
                    DeviceId::Gamepad(i) => Some(*i),
                    _ => None,
                })
                .collect(),
        ),
    }
}

/// Controls actuated this frame, as paths, for rebinding and the editor's Listen button.
/// `vector` = the binding wants a whole stick / d-pad rather than a button.
pub fn candidates(input: &Input, vector: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut keys: Vec<&String> = input.pressed.iter().collect();
    keys.sort();
    if !vector {
        out.extend(keys.into_iter().filter_map(|k| keyboard_path(k)));
        for (i, name) in ["leftButton", "rightButton", "middleButton"]
            .iter()
            .enumerate()
        {
            if input.mouse_pressed[i] {
                out.push(format!("<Mouse>/{name}"));
            }
        }
    } else if keys.iter().any(|k| k.as_str() == "escape") {
        // Escape still cancels while waiting for a stick.
        out.push("<Keyboard>/escape".into());
    }
    for j in input.joysticks.iter().filter(|j| !j.name.is_empty()) {
        let sticks = [
            ("leftStick", glam::Vec2::new(j.axes[0], -j.axes[1])),
            ("rightStick", glam::Vec2::new(j.axes[3], -j.axes[4])),
        ];
        if vector {
            for (name, v) in sticks {
                if v.length() > 0.5 {
                    out.push(format!("<Gamepad>/{name}"));
                }
            }
            if (11..=14).any(|b| j.pressed[b]) {
                out.push("<Gamepad>/dpad".into());
            }
            continue;
        }
        for b in 0..j.pressed.len() {
            if j.pressed[b]
                && let Some(p) = pad_button_path(b)
            {
                out.push(p);
            }
        }
        for (axis, name) in [(8, "leftTrigger"), (9, "rightTrigger")] {
            if j.axes[axis] > 0.5 {
                out.push(format!("<Gamepad>/{name}"));
            }
        }
        for (name, v) in sticks {
            if v.length() > 0.5 {
                let dir = if v.x.abs() > v.y.abs() {
                    if v.x > 0.0 { "right" } else { "left" }
                } else if v.y > 0.0 {
                    "up"
                } else {
                    "down"
                };
                out.push(format!("<Gamepad>/{name}/{dir}"));
            }
        }
    }
    if !vector
        && input
            .touches
            .iter()
            .any(|t| t.phase == crate::input::touch::TouchPhase::Began)
    {
        out.push("<Touchscreen>/primaryTouch/press".into());
    }
    out
}
