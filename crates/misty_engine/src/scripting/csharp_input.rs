//! C# input: `EventSystem.current`, touches (`Input.touches`, `Touchscreen.current`),
//! `Camera.rect`, the Player Input Manager and Player Input (split screen, local multiplayer),
//! Input Actions (`InputAction.ReadValue`, `performed` events, `InputValue` messages),
//! `Gamepad`, `Keyboard` and `Mouse` (`Misty` classes in `dotnet/MistyEngine.cs`).
//!
//! Everything goes through one native function ([`input_system`], the `inputSystem` field at
//! the end of `NativeApi`) with an op code. Calls the engine makes into C# with an argument
//! C# can't get as a bool, number or text (`OnPlayerJoined(PlayerInput)`,
//! `OnMove(InputValue)`, `OnSelect(BaseEventData)`, an action's `CallbackContext`) leave that
//! argument here ([`PendingArg`]) and pass [`CsArg::Engine`]; C# reads it back (op 120).
//! C# events (`action.performed += …`, `playerInput.onDeviceLost`, `manager.onPlayerJoined`)
//! reach C# through the UI event entry point with event kinds from 110 up.

use std::cell::RefCell;

use glam::Vec2;
use hecs::Entity;

use super::{CsArg, CsHost, CsRef};
use crate::input::actions::path::{Control, ControlPath, DeviceFilter, DeviceKind};
use crate::input::actions::system::{DeviceId, PlayerState};
use crate::input::actions::{ActionEvent, ActionType, Phase, PlayerInput, PlayerInputManager};
use crate::scripting::csharp_native::{id_of, text, with_world};
use crate::scripting::{ScriptHost, World};

// ---- arguments handed to C# ------------------------------------------------------------------

/// An argument of a call into C# that C# reads back with op 120.
#[derive(Clone, Debug)]
pub(crate) enum PendingArg {
    /// A `PlayerInput` (`OnPlayerJoined`, `OnDeviceLost`, …).
    Player(Entity),
    /// An `InputValue` (Player Input's `On<Action>` messages).
    Value(crate::input::actions::Value),
    /// An `InputAction.CallbackContext`.
    Context { set: usize, event: ActionEvent },
    /// A `BaseEventData` (`OnSelect`, `OnDeselect`, `OnCancel`).
    EventData(Option<Entity>),
}

thread_local! {
    static PENDING: RefCell<Option<PendingArg>> = const { RefCell::new(None) };
    /// The last failure, for C# to put in its exception (op 199).
    static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Holds `arg` for C# until dropped.
struct Pending(Option<PendingArg>);

impl Pending {
    fn set(arg: PendingArg) -> Self {
        Self(PENDING.with(|p| p.replace(Some(arg))))
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        let previous = self.0.take();
        PENDING.with(|p| *p.borrow_mut() = previous);
    }
}

/// The C# event kinds (`Misty.InputEvents` in MistyEngine.cs).
const EV_STARTED: i32 = 110;
const EV_PERFORMED: i32 = 111;
const EV_CANCELED: i32 = 112;
const EV_ACTION_TRIGGERED: i32 = 120;
const EV_CONTROLS_CHANGED: i32 = 121;
const EV_DEVICE_LOST: i32 = 122;
const EV_DEVICE_REGAINED: i32 = 123;
const EV_PLAYER_JOINED: i32 = 130;
const EV_PLAYER_LEFT: i32 = 131;

/// The key C# keeps an action's listeners under (`InputAction.Key` in MistyEngine.cs).
fn action_key(set: usize, map: usize, action: usize) -> u64 {
    (1 << 63) | ((set as u64) << 40) | ((map as u64) << 20) | action as u64
}

impl CsHost {
    /// [`CsHost::message`] with an argument C# reads back (a `PlayerInput`, an `InputValue`…).
    pub(crate) fn message_with(
        &self,
        w: &mut World,
        r: CsRef,
        name: &str,
        arg: Option<&PendingArg>,
    ) -> bool {
        match arg {
            Some(a) => {
                let _p = Pending::set(a.clone());
                self.message(w, r, name, Some(&CsArg::Engine))
            }
            None => self.message(w, r, name, None),
        }
    }

    /// Run the C# listeners of input event `kind` under `key`. Whether any ran.
    fn input_event(&self, w: &mut World, key: u64, kind: i32, arg: PendingArg) -> bool {
        let Some(entry) = self.entry else {
            return false;
        };
        let _p = Pending::set(arg);
        let _enter = super::Enter::new(w);
        // SAFETY: entry point of the loaded assembly; no string is passed.
        let ran = unsafe { (entry.ui_event)(key, kind, 40, 0.0, std::ptr::null(), 0) };
        ran == 1
    }

    fn loaded(&self) -> bool {
        self.entry.is_some()
    }
}

impl ScriptHost {
    /// Like `call_if_defined`, with `cs_arg` for the C# scripts (`OnPlayerJoined(PlayerInput)`,
    /// `OnSelect(BaseEventData)`): methods without parameters are called too.
    pub(crate) fn call_if_defined_with(
        &mut self,
        target: Entity,
        method: &str,
        arg: Option<mlua::Value>,
        cs_arg: PendingArg,
    ) {
        for i in 0..self.instances.len() {
            let inst = &self.instances[i];
            if inst.entity != target || !self.runnable(inst) {
                continue;
            }
            if let Some(cs) = inst.cs {
                if self.cs_message_with(cs, method, Some(&cs_arg)) {
                    self.flush_spawned();
                }
                continue;
            }
            let defined = self
                .lua
                .registry_value::<mlua::Table>(&inst.table)
                .and_then(|t| t.get::<Option<mlua::Function>>(method))
                .is_ok_and(|f| f.is_some());
            if defined {
                self.call(i, method, arg.clone());
                self.flush_spawned();
            }
        }
    }

    /// Call `method` by name on C# instance `cs` with `arg`.
    pub(crate) fn cs_message_with(
        &self,
        cs: CsRef,
        method: &str,
        arg: Option<&PendingArg>,
    ) -> bool {
        let mut w = self
            .lua
            .app_data_mut::<World>()
            .expect("world is installed");
        self.csharp.message_with(&mut w, cs, method, arg)
    }

    fn cs_event(&mut self, key: u64, kind: i32, arg: PendingArg) {
        if !self.csharp.loaded() {
            return;
        }
        let ran = {
            let mut w = self
                .lua
                .app_data_mut::<World>()
                .expect("world is installed");
            self.csharp.input_event(&mut w, key, kind, arg)
        };
        if ran {
            self.flush_spawned();
        }
    }

    /// C# `action.started / performed / canceled` listeners and the `onActionTriggered`
    /// listeners of the Player Inputs playing `set`.
    pub(crate) fn cs_action_event(&mut self, set: usize, e: &ActionEvent, players: &[Entity]) {
        if !self.csharp.loaded() {
            return;
        }
        let kind = match e.phase {
            Phase::Started => EV_STARTED,
            Phase::Performed => EV_PERFORMED,
            _ => EV_CANCELED,
        };
        let arg = PendingArg::Context {
            set,
            event: e.clone(),
        };
        self.cs_event(action_key(set, e.map, e.action), kind, arg.clone());
        for p in players {
            self.cs_event(id_of(*p), EV_ACTION_TRIGGERED, arg.clone());
        }
    }

    /// C# `playerInput.onControlsChanged / onDeviceLost / onDeviceRegained` listeners.
    pub(crate) fn cs_player_event(&mut self, e: Entity, listener: &str) {
        let kind = match listener {
            "onControlsChanged" => EV_CONTROLS_CHANGED,
            "onDeviceLost" => EV_DEVICE_LOST,
            "onDeviceRegained" => EV_DEVICE_REGAINED,
            _ => return,
        };
        self.cs_event(id_of(e), kind, PendingArg::Player(e));
    }

    /// C# `manager.onPlayerJoined / onPlayerLeft` listeners.
    pub(crate) fn cs_manager_event(&mut self, manager: Entity, player: Entity, joined: bool) {
        let kind = if joined {
            EV_PLAYER_JOINED
        } else {
            EV_PLAYER_LEFT
        };
        self.cs_event(id_of(manager), kind, PendingArg::Player(player));
    }
}

// ---- the native function ---------------------------------------------------------------------

fn fail(msg: impl Into<String>) -> i64 {
    LAST_ERROR.with(|e| *e.borrow_mut() = msg.into());
    -1
}

/// Copy `s` into C#'s buffer (as much as fits); its full length in UTF-16 units.
fn write_text(buf: *mut u16, cap: i32, s: &str) -> i64 {
    let units: Vec<u16> = s.encode_utf16().collect();
    if !buf.is_null() && cap > 0 {
        let n = units.len().min(cap as usize);
        // SAFETY: C# passes a buffer of `cap` chars.
        unsafe { std::ptr::copy_nonoverlapping(units.as_ptr(), buf, n) };
    }
    units.len() as i64
}

fn entity(id: u64) -> Option<Entity> {
    Entity::from_bits(id)
}

fn live(w: &World, id: u64) -> Option<Entity> {
    entity(id).filter(|e| w.scene.contains(*e))
}

fn bits(e: Option<Entity>) -> i64 {
    e.map_or(0, id_of) as i64
}

fn flag(b: bool) -> i64 {
    i64::from(b)
}

/// An entity's bits in a double slot (C# reads them back as a `ulong`).
fn entity_slot(e: Option<Entity>) -> f64 {
    f64::from_bits(e.map_or(0, id_of))
}

/// A playing Player Input's state (starting the players first, like Lua's `player`).
fn player_state(w: &mut World, e: Entity) -> Option<PlayerState> {
    if w.input_system.player(e).is_none() {
        let World {
            input_system,
            scene,
            input,
            ..
        } = w;
        input_system.sync_players(scene, input);
    }
    w.input_system.player(e).cloned()
}

/// (map, action) from a packed handle (`map << 32 | action`).
fn unpack(arg: i64) -> (usize, usize) {
    (
        ((arg as u64) >> 32) as usize,
        (arg as u64 & 0xffff_ffff) as usize,
    )
}

/// Bits of a button-like control: 1 held, 2 pressed this frame, 4 released this frame.
fn button_bits(held: bool, pressed: bool, released: bool) -> i64 {
    flag(held) | flag(pressed) << 1 | flag(released) << 2
}

/// The engine side of the C# input classes. `op` picks what to do; `id` is an object (or an
/// action set, a gamepad slot), `arg` a number (a flag, an index, a packed action), `v` 16
/// doubles in and out, `buf`/`len` a text in (setters, names) or out (getters: the result is
/// the full length). -1 is a failure whose message op 199 returns.
///
/// Ops: EventSystem 0 selected, 1 select `id`, 2 / 3 first selected, 4 / 5 send navigation
/// events, 6 its object, 7 pointer `arg` (-1 mouse, else a finger) over UI; touches 10 count,
/// 11 touch `arg`, 12 supported; Camera 15 / 16 rect, 17 pixel rect; Player Input Manager 20
/// instance, 21 player count, 22 max players, 23 / 24 joining, 25 / 26 split screen,
/// 27 / 28 enabled, 29 / 30 join behaviour, 31 fixed screens, 32 join a player, 33 has one;
/// Player Input 40 has one, 41 index, 42 camera, 43 devices, 44 control scheme, 45 action set,
/// 46 current map, 47 active, 48 switch map, 49 switch scheme, 50 / 51 activate / deactivate,
/// 52 count, 53 `arg`-th, 54 by index, 55 behaviour, 56 gamepad slot; Input Actions (`id` a
/// set) 60 load, 61 instantiate, 62 name, 63 find action, 64 find map, 65 enable all, 66 map
/// name, 67 map enabled, 68 enable map, 69 find in map, 70 action name, 71 read value,
/// 72 action state bits, 73 enable action, 74 action type, 75 map count, 76 action count,
/// 77 binding display string, 78 active control, 79 enabled, 80 control schemes, 81 / 82 save /
/// load binding overrides, 83 remove overrides, 84 apply override; Gamepad 90 current, 91 count,
/// 92 `arg`-th slot, 93 sticks and triggers, 94 button, 95 name, 96 motor speeds; 100 a
/// Keyboard (`arg` 0) or Mouse (1) control; 120 the pending argument; 199 the last error.
pub(crate) extern "C" fn input_system(
    op: i32,
    id: u64,
    arg: i64,
    v: *mut f64,
    buf: *mut u16,
    len: i32,
) -> i64 {
    let mut vals = [0f64; 16];
    if !v.is_null() {
        // SAFETY: C# passes 16 doubles on its stack.
        unsafe { std::ptr::copy_nonoverlapping(v, vals.as_mut_ptr(), 16) };
    }
    let result = if op == 199 {
        LAST_ERROR.with(|e| write_text(buf, len, &e.borrow()))
    } else {
        with_world(|w| run(w, op, id, arg, &mut vals, buf, len))
            .unwrap_or_else(|| fail("no game is running"))
    };
    if !v.is_null() {
        // SAFETY: as above.
        unsafe { std::ptr::copy_nonoverlapping(vals.as_ptr(), v, 16) };
    }
    result
}

fn run(
    w: &mut World,
    op: i32,
    id: u64,
    arg: i64,
    v: &mut [f64; 16],
    buf: *mut u16,
    len: i32,
) -> i64 {
    match op {
        0..=9 => event_system(w, op, id, arg),
        10..=12 => touch(w, op, arg, v),
        15..=17 => camera_rect(w, op, id, v),
        20..=33 => manager(w, op, id, arg),
        40..=56 => player(w, op, id, arg, buf, len),
        60..=84 => actions(w, op, id, arg, v, buf, len),
        90..=96 => gamepad(w, op, id, arg, v, buf, len),
        100 => device_control(w, arg, v, &text(buf, len)),
        120 => pending(v, buf, len),
        _ => fail(format!("unknown input op {op}")),
    }
}

// ---- EventSystem -----------------------------------------------------------------------------

fn event_system(w: &mut World, op: i32, id: u64, arg: i64) -> i64 {
    use crate::scripting::event_system_api::{
        current_selected, event_system_object, request_select, with_event_system,
    };
    match op {
        0 => bits(current_selected(w)),
        1 => {
            let e = live(w, id);
            request_select(w, e);
            1
        }
        2 => bits(
            crate::ui::events::settings(&w.scene)
                .first_selected
                .filter(|e| w.scene.contains(*e)),
        ),
        3 => {
            let e = live(w, id);
            with_event_system(w, |es| es.first_selected = e);
            1
        }
        4 => flag(crate::ui::events::settings(&w.scene).send_navigation_events),
        5 => {
            with_event_system(w, |es| es.send_navigation_events = arg != 0);
            1
        }
        6 => bits(event_system_object(w)),
        7 => {
            let p = if arg < 0 {
                w.input.mouse_position
            } else {
                w.input
                    .touches
                    .iter()
                    .find(|t| t.finger_id == arg as u64)
                    .map(|t| t.position)
            };
            let Some(p) = p else { return 0 };
            let layout = crate::ui::layout(&w.scene, w.input.screen_size);
            flag(crate::ui::input::raycast_hit(&layout, &w.scene, p).is_some())
        }
        _ => fail("unknown EventSystem op"),
    }
}

// ---- touches ---------------------------------------------------------------------------------

/// `v`: finger id, position x, y, delta x, y (pixels, from the bottom-left, y up), phase.
fn touch(w: &mut World, op: i32, arg: i64, v: &mut [f64; 16]) -> i64 {
    match op {
        10 => w.input.touches.len() as i64,
        11 => {
            let h = w.input.screen_size.y;
            let Some(t) = usize::try_from(arg)
                .ok()
                .and_then(|i| w.input.touches.get(i))
            else {
                return fail(format!("Input.GetTouch: no touch {arg}"));
            };
            let phase = match t.phase {
                crate::input::TouchPhase::Began => 0.0,
                crate::input::TouchPhase::Moved => 1.0,
                crate::input::TouchPhase::Stationary => 2.0,
                crate::input::TouchPhase::Ended => 3.0,
                crate::input::TouchPhase::Canceled => 4.0,
            };
            v[..6].copy_from_slice(&[
                t.finger_id as f64,
                f64::from(t.position.x),
                f64::from(h - t.position.y),
                f64::from(t.delta.x) + 0.0,
                // `+ 0.0` turns -0 into 0.
                f64::from(-t.delta.y) + 0.0,
                phase,
            ]);
            1
        }
        12 => flag(w.input_system.touch_seen || !w.input.touches.is_empty()),
        _ => fail("unknown touch op"),
    }
}

// ---- Camera.rect -----------------------------------------------------------------------------

fn camera_rect(w: &mut World, op: i32, id: u64, v: &mut [f64; 16]) -> i64 {
    let screen = crate::scripting::camera_api::screen(w);
    let Some(e) = live(w, id) else {
        return fail("the Camera's object was destroyed");
    };
    let Ok(mut c) = w.scene.world.get::<&mut crate::scene::Camera>(e) else {
        return fail("There is no 'Camera' attached to the object");
    };
    match op {
        15 | 17 => {
            let mut r = c.viewport.map(f64::from);
            if op == 17 {
                let (sw, sh) = (f64::from(screen.x), f64::from(screen.y));
                r = [r[0] * sw, r[1] * sh, r[2] * sw, r[3] * sh];
            }
            v[..4].copy_from_slice(&r);
            1
        }
        16 => {
            let r = [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
            if r.iter().all(|x| x.is_finite()) {
                c.viewport = r;
            }
            1
        }
        _ => fail("unknown Camera op"),
    }
}

// ---- Player Input Manager --------------------------------------------------------------------

fn manager(w: &mut World, op: i32, id: u64, arg: i64) -> i64 {
    use crate::input::actions::PlayerJoinBehavior;
    use crate::scripting::player_manager_api as pm;
    if op == 20 {
        return bits(pm::manager(&w.scene).map(|(e, _)| e));
    }
    if op == 21 {
        return pm::player_count(w) as i64;
    }
    let Some(e) = live(w, id) else {
        return fail("the Player Input Manager's object was destroyed");
    };
    if op == 33 {
        return flag(w.scene.world.get::<&PlayerInputManager>(e).is_ok());
    }
    let Some(m) = w
        .scene
        .world
        .get::<&PlayerInputManager>(e)
        .ok()
        .map(|m| (*m).clone())
    else {
        return fail("There is no 'PlayerInputManager' attached to the object");
    };
    let set = |f: &dyn Fn(&mut PlayerInputManager)| {
        if let Ok(mut m) = w.scene.world.get::<&mut PlayerInputManager>(e) {
            f(&mut m);
        }
        1
    };
    match op {
        22 => i64::from(m.max_player_count),
        23 => flag(m.joining_enabled),
        24 => set(&|m| m.joining_enabled = arg != 0),
        25 => flag(m.split_screen),
        26 => set(&|m| m.split_screen = arg != 0),
        27 => flag(m.enabled),
        28 => set(&|m| m.enabled = arg != 0),
        29 => i64::from(m.join_behavior == PlayerJoinBehavior::JoinPlayersManually),
        30 => set(&|m| {
            m.join_behavior = if arg != 0 {
                PlayerJoinBehavior::JoinPlayersManually
            } else {
                PlayerJoinBehavior::JoinPlayersWhenButtonIsPressed
            }
        }),
        31 => i64::from(m.fixed_number_of_screens),
        32 => {
            let n = pm::player_objects(&w.scene).len();
            if m.max_player_count > 0 && n >= m.max_player_count as usize {
                return 0;
            }
            match pm::join(w, &m, None) {
                Ok(p) => id_of(p) as i64,
                Err(err) => fail(err.to_string()),
            }
        }
        _ => fail("unknown Player Input Manager op"),
    }
}

// ---- Player Input ----------------------------------------------------------------------------

fn player(w: &mut World, op: i32, id: u64, arg: i64, buf: *mut u16, len: i32) -> i64 {
    match op {
        52 | 53 => {
            let World {
                input_system,
                scene,
                input,
                ..
            } = w;
            input_system.sync_players(scene, input);
            return if op == 52 {
                w.input_system.players.len() as i64
            } else {
                bits(
                    usize::try_from(arg)
                        .ok()
                        .and_then(|i| w.input_system.players.get(i))
                        .map(|p| p.entity),
                )
            };
        }
        54 => {
            return bits(
                w.input_system
                    .players
                    .iter()
                    .find(|p| p.index as i64 == arg)
                    .map(|p| p.entity),
            );
        }
        _ => {}
    }
    let Some(e) = live(w, id) else {
        return fail("the Player Input's object was destroyed");
    };
    if op == 40 {
        return flag(w.scene.world.get::<&PlayerInput>(e).is_ok());
    }
    if op == 42 {
        return bits(crate::scripting::player_manager_api::player_camera(
            &w.scene, e,
        ));
    }
    let not_playing = || fail("this Player Input isn't playing (no Actions asset, or inactive)");
    match op {
        48 => {
            let name = text(buf, len);
            return match w.input_system.switch_map(e, &name) {
                Ok(()) => 1,
                Err(err) => fail(err),
            };
        }
        49 => {
            let name = text(buf, len);
            let World {
                input_system,
                input,
                ..
            } = w;
            return match input_system.switch_scheme(e, &name, input) {
                Ok(()) => 1,
                Err(err) => fail(err),
            };
        }
        50 | 51 => {
            w.input_system.set_player_active(e, op == 50);
            return 1;
        }
        _ => {}
    }
    let Some(s) = player_state(w, e) else {
        // A Player Input that isn't playing has no index (like a disabled one in Unity).
        return if op == 41 { -1 } else { not_playing() };
    };
    match op {
        41 => s.index as i64,
        43 => {
            let names: Vec<String> = s.devices.iter().map(|d| d.name(&w.input)).collect();
            write_text(buf, len, &names.join("\n"))
        }
        44 => write_text(buf, len, s.scheme.as_deref().unwrap_or("")),
        45 => s.set as i64,
        46 => s.map.map_or(-1, |m| m as i64),
        47 => flag(s.active),
        55 => s.behavior as i64,
        56 => s
            .devices
            .iter()
            .find_map(|d| match d {
                DeviceId::Gamepad(i)
                    if w.input
                        .joysticks
                        .get(*i)
                        .is_some_and(|j| !j.name.is_empty()) =>
                {
                    Some(*i as i64)
                }
                _ => None,
            })
            .unwrap_or(-1),
        _ => fail("unknown Player Input op"),
    }
}

// ---- Input Actions ---------------------------------------------------------------------------

fn actions(
    w: &mut World,
    op: i32,
    id: u64,
    arg: i64,
    v: &mut [f64; 16],
    buf: *mut u16,
    len: i32,
) -> i64 {
    if op == 60 || op == 61 {
        let path = text(buf, len);
        let r = if op == 60 {
            w.input_system.load_shared(&path)
        } else {
            w.input_system.instantiate(&path)
        };
        return match r {
            Ok(set) => set as i64,
            Err(e) => fail(e),
        };
    }
    let input_text = text(buf, len);
    let Some(s) = w.input_system.set_mut(id as usize) else {
        return fail("these input actions are gone");
    };
    let (map, action) = unpack(arg);
    let maps = s.asset.maps.len();
    let map_ok = map < maps;
    let action_ok = map_ok && action < s.asset.maps[map].actions.len();
    let need_map = matches!(op, 66..=69 | 76);
    let need_action = matches!(op, 70..=74 | 77 | 78 | 84);
    if (need_map && !map_ok) || (need_action && !action_ok) {
        return fail("no such action");
    }
    match op {
        62 => write_text(buf, len, &s.asset.name),
        63 => s
            .asset
            .find_action(&input_text)
            .map_or(-1, |(m, a)| ((m as i64) << 32) | a as i64),
        64 => s.asset.find_map(&input_text).map_or(-1, |m| m as i64),
        65 => {
            s.set_all_enabled(arg != 0);
            1
        }
        66 => write_text(buf, len, &s.asset.maps[map].name),
        67 => flag(s.map_enabled(map)),
        68 => {
            s.set_map_enabled(map, v[0] != 0.0);
            1
        }
        69 => s.asset.maps[map]
            .actions
            .iter()
            .position(|a| a.name.eq_ignore_ascii_case(&input_text))
            .map_or(-1, |a| a as i64),
        70 => write_text(buf, len, &s.asset.maps[map].actions[action].name),
        71 => {
            let Some(st) = s.state(map, action) else {
                return fail("no such action");
            };
            let val = st.value.as_vec2();
            v[0] = f64::from(val.x);
            v[1] = f64::from(val.y);
            v[2] = f64::from(st.magnitude);
            flag(st.value.is_vector())
        }
        72 => {
            let Some(st) = s.state(map, action) else {
                return fail("no such action");
            };
            let phase = match st.phase {
                Phase::Disabled => 0,
                Phase::Waiting => 1,
                Phase::Started => 2,
                Phase::Performed => 3,
                Phase::Canceled => 4,
            };
            flag(st.pressed)
                | flag(st.pressed_this_frame) << 1
                | flag(st.released_this_frame) << 2
                | flag(st.performed_this_frame) << 3
                | flag(st.enabled) << 4
                | flag(matches!(st.phase, Phase::Started | Phase::Performed)) << 5
                | flag(st.started_this_frame) << 6
                | flag(st.canceled_this_frame) << 7
                | phase << 8
        }
        73 => {
            s.set_action_enabled(map, action, v[0] != 0.0);
            1
        }
        74 => match s.asset.maps[map].actions[action].kind {
            ActionType::Value => 0,
            ActionType::Button => 1,
            ActionType::PassThrough => 2,
        },
        75 => maps as i64,
        76 => s.asset.maps[map].actions.len() as i64,
        77 => {
            let index = (v[0] >= 0.0).then_some(v[0] as usize);
            let shown = s.display_string(map, action, index);
            write_text(buf, len, &shown)
        }
        78 => {
            let control = s
                .state(map, action)
                .and_then(|st| st.active_control.clone())
                .unwrap_or_default();
            write_text(buf, len, &control)
        }
        79 => flag(s.any_enabled()),
        80 => {
            let names: Vec<String> = s
                .asset
                .control_schemes
                .iter()
                .map(|c| c.name.clone())
                .collect();
            write_text(buf, len, &names.join("\n"))
        }
        81 => {
            let json = s.overrides_json();
            write_text(buf, len, &json)
        }
        82 => match s.load_overrides_json(&input_text) {
            Ok(_) => 1,
            Err(e) => fail(format!("LoadBindingOverridesFromJson: {e}")),
        },
        83 => {
            let only = (arg >= 0 && v[0] != 0.0).then_some((map, action));
            s.clear_overrides(only);
            1
        }
        84 => {
            let index = v[0].max(0.0) as usize;
            let count = s.asset.maps[map].actions[action].flat_bindings().len();
            if index >= count {
                return fail(format!("binding index {index} is out of range"));
            }
            s.set_override(map, action, index, &input_text);
            1
        }
        _ => fail("unknown Input Actions op"),
    }
}

// ---- devices ---------------------------------------------------------------------------------

fn gamepad(
    w: &mut World,
    op: i32,
    id: u64,
    arg: i64,
    v: &mut [f64; 16],
    buf: *mut u16,
    len: i32,
) -> i64 {
    let connected =
        |w: &World, i: usize| w.input.joysticks.get(i).is_some_and(|j| !j.name.is_empty());
    let pads: Vec<usize> = (0..w.input.joysticks.len())
        .filter(|i| connected(w, *i))
        .collect();
    match op {
        90 => w
            .input_system
            .last_pad
            .filter(|i| connected(w, *i))
            .map_or(-1, |i| i as i64),
        91 => pads.len() as i64,
        92 => usize::try_from(arg)
            .ok()
            .and_then(|i| pads.get(i))
            .map_or(-1, |i| *i as i64),
        96 => {
            let slot = (id != u64::MAX).then_some(id as usize);
            w.input_system
                .set_motor_speeds(slot, v[0] as f32, v[1] as f32);
            1
        }
        _ => {
            let Some(j) = w
                .input
                .joysticks
                .get(id as usize)
                .filter(|j| !j.name.is_empty())
            else {
                return fail("the gamepad is gone");
            };
            match op {
                // Left stick, right stick (y up), d-pad, left and right trigger.
                93 => {
                    let a = &j.axes;
                    // The d-pad from its buttons (up 11, down 12, left 13, right 14), else
                    // from the hat axes, like `<Gamepad>/dpad`.
                    let b = |i: usize| if j.held[i] { 1.0 } else { 0.0 };
                    let mut dpad = (b(14) - b(13), b(11) - b(12));
                    if dpad == (0.0, 0.0) {
                        dpad = (a[5], a[6]);
                    }
                    let vals = [a[0], -a[1], a[3], -a[4], dpad.0, dpad.1, a[8], a[9]];
                    for (slot, x) in v.iter_mut().zip(vals) {
                        *slot = f64::from(x);
                    }
                    1
                }
                94 => {
                    let Some(i) = usize::try_from(arg).ok().filter(|i| *i < j.held.len()) else {
                        return fail("no such gamepad button");
                    };
                    button_bits(j.held[i], j.pressed[i], j.released[i])
                }
                95 => write_text(buf, len, &j.name),
                _ => fail("unknown Gamepad op"),
            }
        }
    }
}

/// A Keyboard (`device` 0) or Mouse (1) control by its Input System name (`space`, `w`,
/// `leftButton`, `position`): its value in `v` (x, y) and [`button_bits`] plus 8 for a 2D one.
fn device_control(w: &mut World, device: i64, v: &mut [f64; 16], name: &str) -> i64 {
    let kind = if device == 1 {
        DeviceKind::Mouse
    } else {
        DeviceKind::Keyboard
    };
    let Ok(c) = ControlPath::parse(&format!("{}/{name}", kind.layout())) else {
        return fail(format!("{} has no control '{name}'", kind.name()));
    };
    let value = c.read(&w.input, &DeviceFilter::all());
    let edge = |set: &std::collections::HashSet<String>| match &c.control {
        Control::Keys(k) => k.iter().any(|k| set.contains(*k)),
        _ => false,
    };
    let (pressed, released) = match (&c.control, kind) {
        (Control::MouseButton(i), DeviceKind::Mouse) => {
            (w.input.mouse_pressed[*i], w.input.mouse_released[*i])
        }
        _ => (edge(&w.input.pressed), edge(&w.input.released)),
    };
    let xy: Vec2 = value.as_vec2();
    v[0] = f64::from(xy.x);
    v[1] = f64::from(xy.y);
    let f = value.as_float();
    button_bits(f >= 0.5, pressed, released) | flag(c.is_vector()) << 3
}

// ---- the pending argument --------------------------------------------------------------------

/// The argument of the running call into C#: its kind (0 none, 1 PlayerInput, 2 InputValue,
/// 3 CallbackContext, 4 BaseEventData) with its parts in `v` (objects as raw bits) and `buf`.
fn pending(v: &mut [f64; 16], buf: *mut u16, len: i32) -> i64 {
    let arg = PENDING.with(|p| p.borrow().clone());
    match arg {
        None => 0,
        Some(PendingArg::Player(e)) => {
            v[0] = entity_slot(Some(e));
            1
        }
        Some(PendingArg::Value(val)) => {
            let xy = val.as_vec2();
            v[0] = f64::from(xy.x);
            v[1] = f64::from(xy.y);
            v[2] = f64::from(u8::from(val.is_vector()));
            2
        }
        Some(PendingArg::Context { set, event }) => {
            let xy = event.value.as_vec2();
            let phase = match event.phase {
                Phase::Started => 2.0,
                Phase::Performed => 3.0,
                _ => 4.0,
            };
            v[..10].copy_from_slice(&[
                set as f64,
                event.map as f64,
                event.action as f64,
                phase,
                f64::from(xy.x),
                f64::from(xy.y),
                f64::from(u8::from(event.value.is_vector())),
                event.time,
                event.start_time,
                f64::from(event.value.magnitude()),
            ]);
            let text = format!("{}\n{}", event.control, event.interaction.unwrap_or(""));
            v[15] = write_text(buf, len, &text).min(i64::from(len.max(0))) as f64;
            3
        }
        Some(PendingArg::EventData(e)) => {
            v[0] = entity_slot(e);
            4
        }
    }
}

#[cfg(test)]
#[path = "csharp_input_tests.rs"]
mod tests;
