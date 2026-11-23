//! Lua scripting with a MonoBehaviour-style lifecycle.
//!
//! A script file returns a class table. Plain values in it (numbers, booleans, strings, Vector3)
//! are public fields shown in the Inspector; functions are callbacks:
//! `Awake`, `OnEnable`, `Start`, `Update`, `FixedUpdate`, `LateUpdate`, `OnDisable`, `OnDestroy`
//! (see `lifecycle`), `OnCollisionEnter/Exit(other)`, `OnTriggerEnter/Exit(other)`, `OnControllerColliderHit(hit)`,
//! `OnJointBreak(breakForce)` (then the joint component is removed),
//! and the 2D `OnCollisionEnter2D/Stay2D/Exit2D(collision)`, `OnTriggerEnter2D/Stay2D/Exit2D(other)`.
//! `Instantiate(prefabPath or gameObject [, position])` creates objects while playing; their
//! scripts get `Awake` right after the callback that created them.
//! `SceneManager.LoadScene(name or index)` and `Application.Quit()` are handled after the frame
//! by whoever runs the game (player or editor).
//!
//! ```lua
//! local Rotator = { speed = 90 }
//! function Rotator:Update()
//!     self.transform:Rotate(0, self.speed * Time.deltaTime, 0)
//! end
//! return Rotator
//! ```

pub mod accessibility_api;
pub mod animator_api;
pub mod api;
pub mod app_api;
pub mod assets_api;
pub mod attributes;
pub mod audio_api;
pub mod behaviour_tree_api;
pub mod camera_api;
mod component_ops;
pub mod csharp;
pub mod csharp_fields;
mod csharp_native;
pub mod cursor_api;
mod deadline;
pub mod event_system_api;
pub mod input_actions_api;
pub mod input_api;
mod lifecycle;
#[cfg(test)]
mod lifecycle_tests;
pub mod light_probes_api;
pub mod localization_api;
pub mod material_api;
mod messages;
pub mod navigation_api;
pub mod network_api;
mod override_api;
pub mod particles_api;
pub mod physics2d_api;
pub mod physics_api;
pub mod player_manager_api;
pub mod recorder_api;
pub mod render_settings_api;
pub mod rigging_api;
pub mod save_api;
mod save_host;
pub mod scene_api;
mod scene_ops;
pub mod skinning_api;
pub mod spline_api;
pub mod syntax;
pub mod terrain_api;
pub mod tilemap_api;
pub mod timeline_api;
pub mod ui_api;
mod ui_dispatch;
pub mod ui_widgets_api;
pub mod vcam_api;
pub mod vehicle_api;
pub mod vfx_api;
pub mod video_api;
pub mod water_api;
pub mod weather_api;
pub mod world_partition_api;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use glam::{Vec2, Vec3};
use hecs::Entity;
use mlua::{Function, Lua, RegistryKey, Table, Value};

use crate::animation::{AnimationPlayer, AnimatorPlayer};
use crate::audio::Audio;
use crate::physics::{Contact, Physics};
use crate::physics2d::{Contact2D, Phase, Physics2D};
use crate::profile_scope;
use crate::profiler::Category;
use crate::runtime::Time;
use crate::scene::{Scene, ScriptValue, Scripts};
use crate::ui::UiInteraction;
use api::{LGameObject, LTransform, LVec3};
use deadline::{arm_deadline, arm_deadline_for, install_deadline};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogKind {
    Info,
    Warning,
    Error,
}

/// Keyboard/mouse state for one frame, with lowercase key names ("w", "space", "left shift", "up").
#[derive(Clone, Debug, Default)]
pub struct Input {
    pub held: HashSet<String>,
    pub pressed: HashSet<String>,
    pub released: HashSet<String>,
    pub mouse_held: [bool; 3],
    pub mouse_pressed: [bool; 3],
    pub mouse_released: [bool; 3],
    pub mouse_delta: Vec2,
    /// Pointer over the game image, in its pixels from the top-left (None = outside / unknown).
    pub mouse_position: Option<Vec2>,
    /// Game image size in pixels (UI layout and clicks use it).
    pub screen_size: Vec2,
    /// Characters typed this frame (Input Fields).
    pub text: String,
    /// Key presses this frame including auto-repeats (Input Field editing).
    pub typed: Vec<String>,
    /// Mouse wheel this frame in pixels (y > 0 = scrolled away from the user).
    pub scroll_delta: Vec2,
    /// Connected gamepads by joystick number - 1.
    pub joysticks: Vec<crate::input::Joystick>,
    /// Fingers on a touch screen this frame.
    pub touches: Vec<crate::input::Touch>,
}

impl Input {
    /// This frame's input as the UI event system sees it.
    pub fn ui_pointer(&self, dt: f32) -> crate::ui::PointerInput {
        crate::ui::PointerInput {
            position: self.mouse_position,
            down: self.mouse_pressed[0],
            held: self.mouse_held[0],
            up: self.mouse_released[0],
            scroll: self.scroll_delta,
            text: self.text.clone(),
            keys: self.typed.clone(),
            shift: self.key("left shift") || self.key("right shift"),
            ctrl: self.key("left ctrl") || self.key("right ctrl"),
            screen: self.screen_size,
            dt,
            ..Default::default()
        }
    }

    /// The first gamepad as the Input Manager sees it, for UI navigation without the Input
    /// System: (d-pad or left stick, y up; South pressed; East pressed).
    pub fn ui_legacy_pad(&self) -> (Vec2, bool, bool) {
        let Some(j) = self.joysticks.iter().find(|j| !j.name.is_empty()) else {
            return (Vec2::ZERO, false, false);
        };
        let dpad = Vec2::new(j.axes[5], j.axes[6]);
        let stick = Vec2::new(j.axes[0], -j.axes[1]);
        let v = if dpad.length() > stick.length() {
            dpad
        } else {
            stick
        };
        (v, j.pressed[0], j.pressed[1])
    }

    /// Key, mouse ("mouse 0") or joystick ("joystick button 0") button names.
    pub fn key(&self, k: &str) -> bool {
        self.button_held(k)
    }
    pub fn key_down(&self, k: &str) -> bool {
        self.button_pressed(k)
    }
    pub fn key_up(&self, k: &str) -> bool {
        self.button_released(k)
    }
    pub fn mouse(&self, b: usize) -> bool {
        self.mouse_held.get(b).copied().unwrap_or(false)
    }
    pub fn mouse_down(&self, b: usize) -> bool {
        self.mouse_pressed.get(b).copied().unwrap_or(false)
    }
    pub fn mouse_up(&self, b: usize) -> bool {
        self.mouse_released.get(b).copied().unwrap_or(false)
    }
}

/// `Cursor.lockState`: how the game wants the OS cursor handled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorLockMode {
    /// Free, as usual.
    #[default]
    None,
    /// Held at the window centre and hidden by default: mouse look reads raw motion.
    Locked,
    /// Kept inside the window.
    Confined,
}

impl CursorLockMode {
    pub const ALL: [Self; 3] = [Self::None, Self::Locked, Self::Confined];

    /// The `CursorLockMode.X` name scripts use.
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Locked => "Locked",
            Self::Confined => "Confined",
        }
    }
}

/// What scripts asked of the cursor (`Cursor.lockState`, `Cursor.visible`); the editor's Game
/// view and the player apply it to their window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorState {
    pub lock: CursorLockMode,
    pub visible: bool,
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            lock: CursorLockMode::None,
            visible: true,
        }
    }
}

/// Something a script asked the game (not the scene) to do; handled after the frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// `SceneManager.LoadScene(name or index)`.
    LoadScene(crate::build::SceneRef),
    /// `SceneManager.LoadScene(name or index, LoadSceneMode.Additive)`.
    LoadSceneAdditive(crate::build::SceneRef),
    /// `SceneManager.UnloadSceneAsync(name or index)`.
    UnloadScene(crate::build::SceneRef),
    /// `Application.Quit()`.
    Quit,
    /// Networking: load the server's scene (any project scene, not only the Build Settings
    /// list); the connection carries over to it.
    NetworkLoadScene(String),
}

/// What scripts can reach during a frame (stored in the Lua state's app data).
pub struct World {
    pub scene: Scene,
    pub physics: Physics,
    /// The 2D physics world (Rigidbody 2D, 2D colliders).
    pub physics2d: Physics2D,
    pub time: Time,
    pub input: Input,
    pub logs: Vec<(LogKind, String)>,
    pub destroy: Vec<Entity>,
    /// Objects scripts marked with `DontDestroyOnLoad`: a single scene load keeps them.
    pub dont_destroy: Vec<Entity>,
    /// `LoadSceneAsync` / `UnloadSceneAsync` operations (scene_ops.rs).
    pub scene_ops: Vec<scene_ops::SceneOp>,
    /// `SendMessage` calls waiting for the scripts of the other language (messages.rs).
    pub messages: Vec<messages::QueuedMessage>,
    /// Scripts added while playing, by object and slot, waiting for their instance.
    pub added_scripts: Vec<(Entity, usize)>,
    /// Scripts `Destroy(script)` removes at the end of the frame.
    pub destroy_scripts: Vec<(Entity, component_ops::ScriptSel)>,
    /// Project folder, for reading prefab files.
    pub root: PathBuf,
    /// Roots created by `Instantiate` that still need script instances.
    pub spawned: Vec<Entity>,
    /// Scene loads and quit requests made this frame.
    pub requests: Vec<Request>,
    /// Audio Source voices (`Play`, `PlayOneShot`, …).
    pub audio: Audio,
    /// Animation component playback (`Play`, `Stop`, …).
    pub animation: AnimationPlayer,
    /// Animator state machines (`SetFloat`, `Play`, …).
    pub animator: AnimatorPlayer,
    /// Animation Rigging memory (pre-rig poses, bind offsets, damping).
    pub rigging: crate::rigging::RigState,
    /// Playable Directors playing timelines (`Play`, `Pause`, `Stop`, …).
    pub timeline: crate::timeline::DirectorPlayer,
    /// Nav Mesh Agents and the scene's nav mesh.
    pub navigation: crate::navigation::NavRuntime,
    /// Input Manager axes (`Input.GetAxis`, `GetButton`).
    pub axes: crate::input::InputManager,
    /// The Input System: action sets, Player Inputs, rebinding, rumble.
    pub input_system: crate::input::actions::InputSystem,
    /// Project tags and layer names (`LayerMask.NameToLayer`, `FindWithTag`).
    pub tags_layers: crate::tags_layers::TagsAndLayers,
    /// Camera Brains and Virtual Cameras.
    pub cameras: crate::vcam::CameraRig,
    /// Save games: the scene's Save IDs at start, spawned prefabs, a pending `Load`.
    pub save: crate::save::SaveRuntime,
    /// String Tables and the current locale (`Localization`, Localized Text).
    pub localization: crate::localization::Localizer,
    /// Video Players (`Play`, `Pause`, `time`, …).
    pub video: crate::video::VideoPlayback,
    /// Video events raised by script calls, dispatched with the frame's own.
    pub video_events: Vec<crate::video::VideoEvent>,
    /// `Cursor.lockState` / `Cursor.visible` as last set by a script.
    pub cursor: CursorState,
    /// Multiplayer: mode, connections, spawned network objects.
    pub network: crate::network::NetSession,
    /// World Partition streaming as scripts see it (`WorldPartition`), and their requests.
    pub streaming: world_partition_api::ScriptStreaming,
    /// Object variables of Script Graphs (`gameObject:GetVariable`).
    pub graph_vars: crate::visual_script::ObjectVariables,
    /// Custom events sent this frame, delivered after the callback or flow that sent them.
    pub graph_events: Vec<crate::visual_script::runtime::CustomEvent>,
    /// Behaviour Tree Agents, their blackboards and the shared ones.
    pub behaviour: crate::behaviour_tree::BtRuntime,
    /// The UI selection as of the last UI pass (`EventSystem.currentSelectedGameObject`).
    pub ui_selected: Option<Entity>,
    /// A selection a script asked for (`SetSelectedGameObject`, `Selectable.Select`), applied
    /// at the start of the next UI pass; `Some(None)` clears it.
    pub ui_select_request: Option<Option<Entity>>,
    /// Fingers on On-Screen Sticks and Buttons (the virtual gamepad they drive).
    pub on_screen: crate::input::OnScreenControls,
    /// Players the Player Input Manager has reported (joined / left).
    pub player_manager: player_manager_api::PlayerManagerState,
}

impl World {
    /// A world around `scene` with nothing playing yet (the script host's starting state; the
    /// Test Runner gives EditMode tests one built from a scratch copy of the scene).
    pub fn new(
        scene: Scene,
        physics: Physics,
        physics2d: Physics2D,
        project_root: PathBuf,
        audio: Audio,
        settings: &crate::settings::ProjectSettings,
    ) -> Self {
        let navigation = crate::navigation::NavRuntime::load(&scene, &project_root);
        let project_root_for_video = project_root.clone();
        let project_root_for_input = project_root.clone();
        let project_root_for_cameras = project_root.clone();
        let project_root_for_bt = project_root.clone();
        let save = crate::save::SaveRuntime::begin(&scene);
        let (localization, table_warnings) =
            crate::localization::Localizer::load(&project_root, &settings.localization);
        let mut network = crate::network::NetSession::default();
        network.config.game_name = settings
            .player
            .clone()
            .unwrap_or_default()
            .product_name_or(&project_root);
        World {
            save,
            localization,
            navigation,
            scene,
            physics,
            physics2d,
            time: Time::default(),
            input: Input::default(),
            // String Table problems show in the Console at start.
            logs: table_warnings
                .into_iter()
                .map(|w| (LogKind::Warning, w))
                .collect(),
            destroy: Vec::new(),
            dont_destroy: Vec::new(),
            scene_ops: Vec::new(),
            messages: Vec::new(),
            added_scripts: Vec::new(),
            destroy_scripts: Vec::new(),
            root: project_root.clone(),
            spawned: Vec::new(),
            requests: Vec::new(),
            audio,
            animation: AnimationPlayer::new(project_root.clone()),
            animator: AnimatorPlayer::new(project_root.clone()),
            rigging: Default::default(),
            timeline: crate::timeline::DirectorPlayer::new(project_root),
            axes: crate::input::InputManager::new(&settings.input),
            input_system: crate::input::actions::InputSystem::new(
                &project_root_for_input,
                settings.input.active_handling,
            ),
            tags_layers: settings.tags_and_layers.clone(),
            cameras: crate::vcam::CameraRig::new(project_root_for_cameras),
            video: crate::video::VideoPlayback::new(project_root_for_video),
            video_events: Vec::new(),
            cursor: CursorState::default(),
            network,
            streaming: Default::default(),
            graph_vars: Default::default(),
            graph_events: Vec::new(),
            behaviour: crate::behaviour_tree::BtRuntime::new(&project_root_for_bt),
            ui_selected: None,
            ui_select_request: None,
            on_screen: Default::default(),
            player_manager: Default::default(),
        }
    }
}

/// Registry table holding every script instance table by object and script name, so
/// `gameObject:GetComponent("Rotator")` can reach another object's script.
const SCRIPT_INSTANCES: &str = "misty.scripts";

/// The script instance table of `name` on `e`, if that object runs such a script (only from
/// the Lua state the game scripts live in).
pub(crate) fn script_instance(lua: &Lua, e: Entity, name: &str) -> Option<Table> {
    let all: Table = lua.named_registry_value(SCRIPT_INSTANCES).ok()?;
    let mine: Table = all.get(e.to_bits().get() as i64).ok()?;
    mine.get::<Option<Table>>(name).ok().flatten()
}

fn register_script_instance(lua: &Lua, e: Entity, name: &str, t: &Table) -> mlua::Result<()> {
    let all: Table = match lua.named_registry_value::<Option<Table>>(SCRIPT_INSTANCES)? {
        Some(t) => t,
        None => {
            let t = lua.create_table()?;
            lua.set_named_registry_value(SCRIPT_INSTANCES, t.clone())?;
            t
        }
    };
    let key = e.to_bits().get() as i64;
    let mine: Table = match all.get::<Option<Table>>(key)? {
        Some(t) => t,
        None => {
            let t = lua.create_table()?;
            all.set(key, t.clone())?;
            t
        }
    };
    mine.set(name, t.clone())
}

/// The script name `t` is registered under on `e`, if it is one of its instance tables.
pub(crate) fn script_name_of(lua: &Lua, e: Entity, t: &Table) -> Option<String> {
    let all: Table = lua.named_registry_value(SCRIPT_INSTANCES).ok()?;
    let mine: Table = all.get(e.to_bits().get() as i64).ok()?;
    mine.pairs::<String, Table>()
        .flatten()
        .find(|(_, v)| v == t)
        .map(|(k, _)| k)
}

/// Forget the instance table of script `name` on `e` (the script was removed while playing).
fn unregister_script_instance(lua: &Lua, e: Entity, name: &str) {
    let Ok(all) = lua.named_registry_value::<Table>(SCRIPT_INSTANCES) else {
        return;
    };
    if let Ok(mine) = all.get::<Table>(e.to_bits().get() as i64) {
        let _ = mine.set(name, Value::Nil);
    }
}

fn unregister_script_instances(lua: &Lua, entities: &[Entity]) {
    let Ok(all) = lua.named_registry_value::<Table>(SCRIPT_INSTANCES) else {
        return;
    };
    for e in entities {
        let _ = all.set(e.to_bits().get() as i64, Value::Nil);
    }
}

struct Instance {
    entity: Entity,
    /// Index into the object's `Scripts` list.
    slot: usize,
    name: String,
    table: RegistryKey,
    /// The same table, ready to pass to a call (no registry lookup per callback).
    handle: Table,
    started: bool,
    /// `Awake` ran (the first time the instance could run); `OnDestroy` only follows it.
    awoken: bool,
    /// `OnEnable` ran and `OnDisable` has not since: the two always alternate.
    enabled: bool,
    /// `Scene::activity` when `enabled` last changed: a switch off and on after it counts.
    stamp: u64,
    /// A C# script (its `table` is then an empty stand-in).
    cs: Option<csharp::CsRef>,
    /// Per-frame callbacks the script lacks (see [`FRAME_CALLBACKS`]): passes skip it.
    lacks: u8,
}

/// Callbacks called on every script every frame or physics step: one without them is skipped
/// (a scene of a thousand scripts with only an Update spent most of FixedUpdate and
/// LateUpdate looking), and Stay contacts aren't sent at all while no script has them.
const FRAME_CALLBACKS: [&str; 5] = [
    "Update",
    "FixedUpdate",
    "LateUpdate",
    "OnCollisionStay",
    "OnTriggerStay",
];

/// Update the touching pairs (a, b, trigger) with a step's contacts; returns the pairs that
/// touched before the step and still do (the ones that get Stay).
///
/// One entry per collider pair: an object with several colliders (a Box and a Sphere, a
/// terrain's heightfield and trunks) meets another once per collider, so a Stop takes away
/// one entry, not every entry of the two objects (that ended Stay while another collider
/// still touched).
fn advance_touching(
    touching: &mut Vec<(Entity, Entity, bool)>,
    contacts: &[Contact],
) -> Vec<(Entity, Entity, bool)> {
    fn same(c: &Contact) -> impl Fn(&(Entity, Entity, bool)) -> bool + '_ {
        move |&(a, b, t)| t == c.trigger && ((a, b) == (c.a, c.b) || (a, b) == (c.b, c.a))
    }
    let mut staying = std::mem::take(touching);
    let mut met = Vec::new();
    for c in contacts {
        if c.started {
            met.push((c.a, c.b, c.trigger));
        } else if let Some(i) = met.iter().position(same(c)) {
            // Met and parted within this step.
            met.remove(i);
        } else if let Some(i) = staying.iter().position(same(c)) {
            staying.remove(i);
        }
    }
    *touching = staying.clone();
    touching.extend(met);
    staying
}

/// Bit of `Instance::lacks` for callback `name` (0 for the others: always called).
fn lacks_bit(name: &str) -> u8 {
    FRAME_CALLBACKS
        .iter()
        .position(|n| *n == name)
        .map_or(0, |i| 1 << i)
}

/// Calls `t[name](t)` / `t[name](t, arg)` when the script has such a function: the lookup
/// (through the instance's `__index`) and the call in one trip into Lua. From Rust they were
/// a protected table read plus a call, about twice the time of an empty `Update`.
const INVOKE: &str = r#"
local type = type
return function(t, name)
    local f = t[name]
    if type(f) == "function" then f(t) end
end, function(t, name, arg)
    local f = t[name]
    if type(f) == "function" then f(t, arg) end
end
"#;

/// The [`INVOKE`] functions and the names of the callbacks called most, as Lua strings (a
/// `&str` is hashed and looked up in Lua's string table on every call).
struct Invoke {
    plain: Function,
    with_arg: Function,
    names: Vec<(&'static str, mlua::String)>,
}

impl Invoke {
    fn new(lua: &Lua) -> mlua::Result<Self> {
        let (plain, with_arg) = lua.load(INVOKE).set_name("=script").eval()?;
        let names = FRAME_CALLBACKS
            .iter()
            .chain(&["Start"])
            .map(|n| Ok((*n, lua.create_string(n)?)))
            .collect::<mlua::Result<_>>()?;
        Ok(Self {
            plain,
            with_arg,
            names,
        })
    }

    fn name(&self, lua: &Lua, name: &str) -> mlua::Result<mlua::String> {
        match self.names.iter().find(|(n, _)| *n == name) {
            Some((_, s)) => Ok(s.clone()),
            None => lua.create_string(name),
        }
    }
}

pub struct ScriptHost {
    /// Before `lua`: the loop guard's watchdog lets go of the state before it closes.
    _watch: deadline::WatchGuard,
    lua: Lua,
    /// `None` only if the helper chunk failed to load (callbacks then go the slow way).
    invoke: Option<Invoke>,
    root: PathBuf,
    classes: HashMap<String, Option<RegistryKey>>,
    instances: Vec<Instance>,
    /// UI button hover/press state.
    ui: UiInteraction,
    /// Network class info cache and settle state.
    net: network_api::NetHostState,
    /// Script Graphs running on objects.
    graphs: crate::visual_script::GraphRuntime,
    /// How long the last frame's Script Graph Update pass took.
    graph_update_time: std::time::Duration,
    /// `Scene::activity` when OnEnable / OnDisable were last brought up to date.
    seen_activity: u64,
    /// A look gave up (scripts kept switching each other): `Instance::enabled` may not match
    /// the objects any more, so nothing takes it for the truth.
    flags_stale: bool,
    /// OnEnable / OnDisable are being delivered (callbacks they run don't start another pass).
    syncing: bool,
    /// While Play starts, the scripts the start pass has not reached yet: they get Awake and
    /// OnEnable in order, not when touched (the ones it went by are looked at as usual).
    start_pending: std::ops::Range<usize>,
    /// C# scripts (`.cs`), run on the .NET runtime next to the Lua ones.
    csharp: csharp::CsHost,
    /// 3D pairs touching now (a, b, trigger): OnCollisionStay / OnTriggerStay every physics
    /// step after the one they met in, until they part.
    touching: Vec<(Entity, Entity, bool)>,
}

impl ScriptHost {
    /// Bytes allocated by the Lua state.
    pub fn lua_memory(&self) -> usize {
        self.lua.used_memory()
    }

    /// Audio clips the mixer keeps in memory.
    pub fn audio_clip_memory(&self) -> Vec<(String, u64, &'static str)> {
        self.with_world(|w| w.audio.clip_memory())
    }

    /// Create script instances for every script in the scene and run `Awake` (silent audio).
    pub fn start(scene: &mut Scene, physics: Physics, project_root: PathBuf) -> Self {
        let (audio, _) = Audio::null(project_root.clone());
        Self::start_with_audio(scene, physics, project_root, audio)
    }

    /// Like [`ScriptHost::start`]; sources with Play On Awake start after the scripts' `Awake`.
    /// `scene` is moved into the host's world for the duration of the call.
    pub fn start_with_audio(
        scene: &mut Scene,
        physics: Physics,
        project_root: PathBuf,
        audio: Audio,
    ) -> Self {
        let settings = crate::settings::ProjectSettings::default();
        Self::start_configured(scene, physics, project_root, audio, &settings)
    }

    /// Like [`ScriptHost::start_with_audio`] with the project's input axes, tags and layers.
    pub fn start_configured(
        scene: &mut Scene,
        physics: Physics,
        project_root: PathBuf,
        audio: Audio,
        settings: &crate::settings::ProjectSettings,
    ) -> Self {
        // Tile colliders and other assets come from the game's own project.
        let _root = crate::assets::scoped_root(&project_root);
        let physics2d = Physics2D::build(scene, &settings.physics2d, physics.settings());
        Self::start_with_physics(scene, physics, physics2d, project_root, audio, settings)
    }

    /// Like [`ScriptHost::start_configured`] with a prepared 2D physics world.
    pub fn start_with_physics(
        scene: &mut Scene,
        physics: Physics,
        physics2d: Physics2D,
        project_root: PathBuf,
        audio: Audio,
        settings: &crate::settings::ProjectSettings,
    ) -> Self {
        // Assets the scripts load come from the game's own project, not the global root.
        let _root = crate::assets::scoped_root(&project_root);
        let lua = {
            crate::profile_scope!("Scripts.Start.Lua");
            game_lua()
        };
        let watch = deadline::install_watched_deadline(&lua);
        arm_deadline(&lua, true);
        let mut logs = Vec::new();
        if let Err(e) = {
            crate::profile_scope!("Scripts.Start.Api");
            api::register(&lua)
        } {
            logs.push((LogKind::Error, format!("Script API failed to load: {e}")));
        }
        lua.set_app_data(crate::asset_groups::AssetSystem::auto(&project_root));
        let world_scope = crate::profiler::Scope::begin_lazy(|| "Scripts.Start.World", None);
        let mut world = World::new(
            std::mem::take(scene),
            physics,
            physics2d,
            project_root.clone(),
            audio,
            settings,
        );
        drop(world_scope);
        logs.append(&mut world.logs);
        world.logs = logs;
        lua.set_app_data(world);
        let invoke = Invoke::new(&lua).ok();
        let mut host = Self {
            _watch: watch,
            lua,
            invoke,
            root: project_root,
            classes: HashMap::new(),
            instances: Vec::new(),
            ui: UiInteraction::default(),
            net: Default::default(),
            graphs: Default::default(),
            graph_update_time: Default::default(),
            seen_activity: 0,
            flags_stale: false,
            syncing: false,
            start_pending: 0..0,
            csharp: Default::default(),
            touching: Vec::new(),
        };
        // OnEnable / OnDisable look at changes from here on.
        host.seen_activity = host.with_world(|w| w.scene.activity());
        let scripted: Vec<(Entity, Vec<crate::scene::ScriptRef>)> = host.with_world(|w| {
            w.scene
                .walk()
                .into_iter()
                .filter_map(|(e, _)| {
                    w.scene
                        .world
                        .get::<&Scripts>(e)
                        .ok()
                        .map(|s| (e, s.list.clone()))
                })
                .collect()
        });
        let instantiate_scope =
            crate::profiler::Scope::begin_lazy(|| "Scripts.Start.Instantiate", None);
        for (e, list) in scripted {
            for (slot, script) in list.iter().enumerate() {
                host.instantiate(e, slot, script);
            }
        }
        drop(instantiate_scope);
        // C# sees the fixed step and frame 0 in Awake.
        {
            let mut w = host
                .lua
                .app_data_mut::<World>()
                .expect("world is installed");
            host.csharp.set_time(&mut w);
        }
        // Behaviour Tree Agents get their blackboards (and Script Graphs) before Awake, so
        // scripts can set blackboard values there.
        host.sync_behaviour_trees();
        // Player Inputs enable their default map before any Awake.
        host.input_sync();
        crate::profile_scope!("Scripts.Start.Awake");
        // Awake then OnEnable, script by script, for the ones that can run.
        let count = host.instances.len();
        for i in 0..count {
            host.start_pending = i + 1..count;
            host.reconcile(i, None, true);
        }
        host.start_pending = 0..0;
        host.flush_spawned();
        host.localization_frame();
        host.with_world(|w| {
            w.audio.play_on_awake(&w.scene, None);
            w.animation.play_automatically(&w.scene, None);
            w.timeline.play_on_awake(&w.scene, None);
            let ev = w.video.play_on_awake(&mut w.scene, &mut w.audio, None);
            w.video_events.extend(ev);
        });
        *scene = host.with_world(|w| std::mem::take(&mut w.scene));
        arm_deadline(&host.lua, false);
        host
    }

    /// Between frames, install this game's world (holding `scene`) in another Lua state and run
    /// `f`: the Test Runner resumes PlayMode tests this way, so test code uses the full game API
    /// on the running game. Objects the code instantiated get their scripts and `Awake` after.
    pub fn lend_world<R>(&mut self, scene: &mut Scene, other: &Lua, f: impl FnOnce() -> R) -> R {
        let mut world = self
            .lua
            .remove_app_data::<World>()
            .expect("world is installed");
        world.scene = std::mem::take(scene);
        if let Err(e) = api::refresh_time(other, &world) {
            world.logs.push((LogKind::Error, format!("Time: {e}")));
        }
        other.set_app_data(world);
        let result = f();
        let world = other
            .remove_app_data::<World>()
            .expect("the lent world comes back");
        self.lua.set_app_data(world);
        self.flush_spawned();
        *scene = self.with_world(|w| std::mem::take(&mut w.scene));
        result
    }

    /// Run `f` on the Animator state machines between frames (the Animator window's live view).
    pub fn with_animator<R>(&self, f: impl FnOnce(&mut AnimatorPlayer) -> R) -> R {
        self.with_world(|w| f(&mut w.animator))
    }

    /// A new version of the animation clip at `path` (changed while playing): Animators,
    /// Animation components and Timelines playing it use it from their next frame.
    pub fn reload_animation_clip(&self, path: &str, c: crate::animation::AnimationClip) {
        self.with_world(|w| {
            w.animator.insert_clip(path, c.clone());
            w.timeline.replace_clip(path, c.clone());
            w.animation.insert_clip(path, c);
        });
    }

    /// The Playable Directors of this game (between frames).
    pub fn with_timeline<R>(&self, f: impl FnOnce(&mut crate::timeline::DirectorPlayer) -> R) -> R {
        self.with_world(|w| f(&mut w.timeline))
    }

    /// Nav Mesh Agents of this game (between frames).
    pub fn with_navigation<R>(&self, f: impl FnOnce(&crate::navigation::NavRuntime) -> R) -> R {
        self.with_world(|w| f(&w.navigation))
    }

    /// The navigation runtime, to change (tests: `sync_rebuilds` for frame-exact carving).
    pub fn with_navigation_mut<R>(
        &self,
        f: impl FnOnce(&mut crate::navigation::NavRuntime) -> R,
    ) -> R {
        self.with_world(|w| f(&mut w.navigation))
    }

    /// The audio voices of this game (between frames).
    pub fn with_audio<R>(&self, f: impl FnOnce(&mut Audio) -> R) -> R {
        self.with_world(|w| f(&mut w.audio))
    }

    fn with_world<R>(&self, f: impl FnOnce(&mut World) -> R) -> R {
        let mut w = self
            .lua
            .app_data_mut::<World>()
            .expect("world is installed");
        f(&mut w)
    }

    fn log(&self, kind: LogKind, text: String) {
        self.with_world(|w| w.logs.push((kind, text)));
    }

    /// Load (once) and return the class table of a script file.
    fn class(&mut self, path: &str) -> Option<Table> {
        if !self.classes.contains_key(path) {
            let key = match load_class(&self.lua, &self.root, path) {
                Ok(t) => self.lua.create_registry_value(t).ok(),
                Err(e) => {
                    self.log(LogKind::Error, e);
                    None
                }
            };
            self.classes.insert(path.to_owned(), key);
        }
        let key = self.classes.get(path)?.as_ref()?;
        self.lua.registry_value(key).ok()
    }

    fn instantiate(&mut self, e: Entity, slot: usize, script: &crate::scene::ScriptRef) {
        if crate::visual_script::is_graph_path(&script.path) {
            self.graphs
                .instantiate(&self.lua, &self.root, e, slot, script);
            return;
        }
        if csharp::is_csharp_path(&script.path) {
            let root = self.root.clone();
            let cs = {
                let mut w = self
                    .lua
                    .app_data_mut::<World>()
                    .expect("world is installed");
                self.csharp.create(&mut w, &root, e, slot, script)
            };
            let Some(cs) = cs else { return };
            // Lua code looking at instances (UI events, input listeners) finds no functions in
            // the stand-in table; C# callbacks go through `cs`.
            match self
                .lua
                .create_table()
                .and_then(|t| Ok((self.lua.create_registry_value(&t)?, t)))
            {
                Ok((table, handle)) => self.instances.push(Instance {
                    entity: e,
                    slot,
                    name: script.name(),
                    table,
                    handle,
                    started: false,
                    awoken: false,
                    enabled: false,
                    stamp: 0,
                    cs: Some(cs),
                    lacks: FRAME_CALLBACKS
                        .iter()
                        .map(|n| {
                            let m = csharp::method_of(n);
                            if m.is_some_and(|m| cs.defines(m)) {
                                0
                            } else {
                                lacks_bit(n)
                            }
                        })
                        .fold(0, |a, b| a | b),
                }),
                Err(err) => {
                    self.csharp.release(cs);
                    self.log(LogKind::Error, format!("{}: {err}", script.name()));
                }
            }
            return;
        }
        let Some(class) = self.class(&script.path) else {
            return;
        };
        // How save games name this script on its object.
        let key = self.with_world(|w| {
            w.scene
                .world
                .get::<&Scripts>(e)
                .ok()
                .and_then(|s| crate::save::script_keys(&s).get(slot).cloned())
                .unwrap_or_else(|| script.path.clone())
        });
        let info = self.net_class_info(&script.path);
        let networked = self.with_world(|w| network_api::is_networked(&w.scene, e));
        let script_name = script.name();
        let build = || -> mlua::Result<Table> {
            let t = self.lua.create_table()?;
            for (k, v) in &script.fields {
                // `self.enabled` is the component's checkbox, never a field.
                if k != "enabled" {
                    t.set(k.as_str(), to_lua(&self.lua, v)?)?;
                }
            }
            t.set("gameObject", LGameObject(e))?;
            t.set("transform", LTransform(e))?;
            // Methods and defaults come from the class; `self.enabled` is the component's
            // checkbox (lifecycle.rs).
            let (index, newindex) = lifecycle::instance_meta(&self.lua, &class, e, slot)?;
            let meta = self.lua.create_table()?;
            meta.set("__index", index)?;
            meta.set("__newindex", newindex)?;
            t.set_metatable(Some(meta))?;
            Ok(t)
        };
        let tracked = build().and_then(|t| {
            network_api::prepare_instance(&self.lua, &t, &class, &script_name, &info, networked)?;
            save_api::track(&self.lua, e, &key, &t)?;
            register_script_instance(&self.lua, e, &script.name(), &t)?;
            Ok(t)
        });
        // Frame callbacks the class has (inherited ones count: `get` follows `__index`).
        let lacks = FRAME_CALLBACKS
            .iter()
            .filter(|n| !matches!(class.get::<Value>(**n), Ok(Value::Function(_))))
            .map(|n| lacks_bit(n))
            .fold(0, |a, b| a | b);
        match tracked.and_then(|t| Ok((self.lua.create_registry_value(&t)?, t))) {
            Ok((table, handle)) => self.instances.push(Instance {
                entity: e,
                slot,
                name: script.name(),
                table,
                handle,
                started: false,
                awoken: false,
                enabled: false,
                stamp: 0,
                cs: None,
                lacks,
            }),
            Err(err) => self.log(LogKind::Error, format!("{}: {err}", script.name())),
        }
    }

    /// Whether an instance should receive callbacks (object alive + active, script enabled).
    fn runnable(&self, i: &Instance) -> bool {
        self.with_world(|w| {
            // The script's checkbox first (one lookup, and it answers for missing objects).
            let enabled = w
                .scene
                .world
                .get::<&Scripts>(i.entity)
                .ok()
                .and_then(|s| s.list.get(i.slot).map(|r| r.enabled))
                .unwrap_or(false);
            enabled && w.scene.active_in_hierarchy(i.entity)
        })
    }

    /// [`Self::runnable`] for the frame passes, which ask it of every script: while no object
    /// was switched since the last look ([`Self::sync_enabled`]), the object's side of the
    /// answer is the instance's `enabled` flag (the look keeps the two the same), so the walk
    /// up the parents, a cache miss per level and script, is left out. The script's checkbox
    /// is still read: it also says no for a destroyed object or a removed script.
    fn runnable_now(&self, i: &Instance) -> bool {
        let seen = (!self.flags_stale).then_some(self.seen_activity);
        self.with_world(|w| {
            let enabled = w
                .scene
                .world
                .get::<&Scripts>(i.entity)
                .ok()
                .and_then(|s| s.list.get(i.slot).map(|r| r.enabled))
                .unwrap_or(false);
            if !enabled {
                return false;
            }
            if seen == Some(w.scene.activity()) {
                i.enabled
            } else {
                w.scene.active_in_hierarchy(i.entity)
            }
        })
    }

    fn call(&self, index: usize, name: &str, arg: Option<Value>) {
        let inst = &self.instances[index];
        if let Some(cs) = inst.cs {
            if let Some(m) = csharp::method_of(name).filter(|m| cs.defines(*m)) {
                profile_scope!(format!("{}.{name}()", inst.name));
                // Contact callbacks get the other object.
                let other = match &arg {
                    Some(Value::UserData(ud)) => ud.borrow::<LGameObject>().ok().map(|g| g.0),
                    _ => None,
                };
                let mut w = self
                    .lua
                    .app_data_mut::<World>()
                    .expect("world is installed");
                self.csharp.call(&mut w, cs, m, other);
            }
            return;
        }
        let t = &inst.handle;
        // Lookup and call in one trip into Lua, unless the Profiler records (it shows only
        // the scripts that have the function, each under its own name).
        let result = match &self.invoke {
            Some(invoke) if !crate::profiler::enabled() => {
                invoke.name(&self.lua, name).and_then(|name| match arg {
                    Some(a) => invoke.with_arg.call::<()>((t, name, a)),
                    None => invoke.plain.call::<()>((t, name)),
                })
            }
            _ => {
                let f = match t.get::<Option<Function>>(name) {
                    Ok(Some(f)) => f,
                    _ => return,
                };
                // Per script file and method, e.g. "Orbit.Update()".
                profile_scope!(format!("{}.{name}()", inst.name));
                match arg {
                    Some(a) => f.call::<()>((t, a)),
                    None => f.call::<()>(t),
                }
            }
        };
        if let Err(e) = result {
            self.log(LogKind::Error, format!("{} ({name}): {e}", inst.name));
        }
    }

    fn call_all(&mut self, name: &str) {
        // Objects switched on or off outside a callback (animation, Timeline) first.
        self.sync_enabled();
        // Objects created during this pass join from the next one (after their Awake).
        let bit = lacks_bit(name);
        for i in 0..self.instances.len() {
            // Started scripts without this callback have nothing to do here.
            if self.instances[i].lacks & bit != 0 && self.instances[i].started {
                continue;
            }
            if !self.runnable_now(&self.instances[i]) {
                continue;
            }
            // Enabled since this frame's Start pass (in a collision callback, by FixedUpdate):
            // Start still comes before its first FixedUpdate / Update / LateUpdate.
            if !self.instances[i].started && self.instances[i].enabled {
                self.instances[i].started = true;
                self.call(i, "Start", None);
                self.flush_spawned();
                if !self.runnable(&self.instances[i]) {
                    continue;
                }
            }
            self.call(i, name, None);
            self.flush_spawned();
        }
    }

    /// OnControllerColliderHit for the moves made by the last callback.
    fn controller_hits(&mut self) {
        let hits = self.with_world(|w| w.physics.take_controller_hits());
        for hit in hits {
            for i in 0..self.instances.len() {
                if self.instances[i].entity == hit.controller && self.runnable(&self.instances[i]) {
                    let arg = physics_api::controller_hit_table(&self.lua, &hit)
                        .map(Value::Table)
                        .ok();
                    self.call(i, "OnControllerColliderHit", arg);
                }
            }
        }
    }

    /// Run what the last callback queued: controller hits, then `Awake` of objects made by
    /// `Instantiate`.
    fn flush_spawned(&mut self) {
        // Most callbacks leave nothing behind: one look instead of the passes below.
        if !self.syncing {
            let seen = self.seen_activity;
            let quiet = self.with_world(|w| {
                w.spawned.is_empty()
                    && w.graph_events.is_empty()
                    && w.messages.is_empty()
                    && w.added_scripts.is_empty()
                    && !w.physics.has_controller_hits()
                    && w.scene.activity() == seen
                    && !w.network.settle_due
                    && w.network.events.is_empty()
                    && !w.network.shutdown_requested
                    && w.network.owner_changes.is_empty()
                    && w.network.despawns.is_empty()
            });
            if quiet {
                return;
            }
        }
        self.controller_hits();
        // OnDisable / OnEnable for what the callback switched off or on.
        self.sync_enabled();
        // Awake may instantiate more; stop runaway chains instead of hanging.
        for _ in 0..64 {
            let delivered =
                self.wake_added_scripts() | self.deliver_custom_events() | self.deliver_messages();
            let spawned = self.with_world(|w| std::mem::take(&mut w.spawned));
            if spawned.is_empty() {
                if delivered {
                    continue;
                }
                self.sync_enabled();
                // Network objects need their scripts' Awake before OnNetworkSpawn.
                self.network_settle();
                return;
            }
            for root in spawned {
                self.awake_spawned(root);
            }
        }
        self.with_world(|w| w.spawned.clear());
        self.log(
            LogKind::Error,
            "Instantiate: objects kept instantiating more objects in Awake; stopped".into(),
        );
    }

    fn awake_spawned(&mut self, root: Entity) {
        let scripted: Vec<(Entity, Vec<crate::scene::ScriptRef>)> = self.with_world(|w| {
            if !w.scene.contains(root) {
                return Vec::new();
            }
            w.scene
                .subtree(root)
                .into_iter()
                .filter_map(|e| {
                    w.scene
                        .world
                        .get::<&Scripts>(e)
                        .ok()
                        .map(|s| (e, s.list.clone()))
                })
                .collect()
        });
        let first = self.instances.len();
        for (e, list) in scripted {
            for (slot, script) in list.iter().enumerate() {
                self.instantiate(e, slot, script);
            }
        }
        // Awake then OnEnable; what they spawn waits for the caller's next round.
        for i in first..self.instances.len() {
            self.reconcile(i, None, false);
        }
        self.with_world(|w| {
            w.audio.play_on_awake(&w.scene, Some(root));
            w.animation.play_automatically(&w.scene, Some(root));
            w.timeline.play_on_awake(&w.scene, Some(root));
            let ev = w
                .video
                .play_on_awake(&mut w.scene, &mut w.audio, Some(root));
            w.video_events.extend(ev);
        });
    }

    /// Objects with component `T` and a runnable script instance defining `function`.
    fn entities_with<T: hecs::Component>(&self, function: &str) -> HashSet<Entity> {
        // Asked every frame: nothing to look at in a scene without the component, and only
        // the scripts next to one otherwise (it was a Lua lookup per script and frame).
        if !self.with_world(|w| w.scene.has_any::<T>()) {
            return HashSet::new();
        }
        self.instances
            .iter()
            .filter(|i| self.with_world(|w| w.scene.world.get::<&T>(i.entity).is_ok()))
            .filter(|i| match i.cs {
                Some(cs) => csharp::method_of(function).is_some_and(|m| cs.defines(m)),
                None => self
                    .lua
                    .registry_value::<Table>(&i.table)
                    .ok()
                    .and_then(|t| t.get::<Option<Function>>(function).ok().flatten())
                    .is_some(),
            })
            .filter(|i| self.runnable(i))
            .map(|i| i.entity)
            .collect()
    }

    /// Apply the Animators' root motion (or call `OnAnimatorMove` on objects whose scripts
    /// handle it), then move objects matching a target.
    fn root_motion(&mut self, dt: f32) {
        let moves = self.with_world(|w| w.animator.take_moves());
        for m in moves {
            if m.by_script {
                for i in 0..self.instances.len() {
                    if self.instances[i].entity == m.entity && self.runnable(&self.instances[i]) {
                        self.call(i, "OnAnimatorMove", None);
                        self.flush_spawned();
                    }
                }
            } else {
                self.with_world(|w| {
                    animator_api::apply_root_motion(
                        w,
                        m.entity,
                        m.delta_position,
                        m.delta_rotation,
                        dt,
                    )
                });
            }
        }
        self.with_world(|w| w.animator.apply_match_targets(&mut w.scene));
    }

    /// Advance animations (after Update, before LateUpdate) and call their events' functions on
    /// the animated object's scripts.
    fn animate(&mut self, dt: f32) {
        let script_moves = self.entities_with::<crate::scene::Animator>("OnAnimatorMove");
        let events = self.with_world(|w| {
            let World {
                animation,
                animator,
                scene,
                ..
            } = w;
            // An Animator on the same object runs last, so it wins for shared properties.
            let mut events = animation.update(scene, dt);
            animator.set_script_moves(script_moves);
            events.extend(animator.update(scene, dt));
            events
        });
        self.root_motion(dt);
        for (e, function) in events {
            for i in 0..self.instances.len() {
                if self.instances[i].entity == e && self.runnable(&self.instances[i]) {
                    self.call(i, &function, None);
                }
            }
            self.flush_spawned();
        }
        // Timelines run last: like the reference engine, they override other animation.
        let unscaled = self.with_world(|w| w.time.unscaled_delta);
        let events = self.with_world(|w| {
            let World {
                timeline,
                scene,
                audio,
                physics,
                cameras,
                ..
            } = w;
            timeline.gravity = physics.gravity();
            let events = timeline.update(scene, audio, dt, unscaled);
            // Camera tracks take their brains over for this frame's camera pass.
            cameras.set_overrides(timeline.camera_overrides());
            events
        });
        self.director_events(events);
        let logs = self.with_world(|w| {
            let mut l = w.animation.take_logs();
            l.extend(w.animator.take_logs());
            l.extend(w.timeline.take_logs());
            l
        });
        for l in logs {
            self.log(LogKind::Warning, l);
        }
    }

    /// Advance Video Players and run their event listeners (`started`, `loopPointReached`,
    /// `prepareCompleted`, `errorReceived`).
    fn update_video(&mut self, dt: f32) {
        let (events, logs) = self.with_world(|w| {
            let World {
                video,
                scene,
                audio,
                video_events,
                ..
            } = w;
            let mut events = std::mem::take(video_events);
            events.extend(video.update(scene, audio, dt));
            (events, video.take_logs())
        });
        for l in logs {
            self.log(LogKind::Warning, l);
        }
        use crate::video::VideoEvent;
        for ev in events {
            let (e, name, message) = match ev {
                VideoEvent::Started(e) => (e, "started", None),
                VideoEvent::LoopPointReached(e) => (e, "loopPointReached", None),
                VideoEvent::PrepareCompleted(e) => (e, "prepareCompleted", None),
                VideoEvent::Error(e, m) => (e, "errorReceived", Some(m)),
            };
            let vp = self
                .lua
                .create_userdata(video_api::LVideoPlayer(e))
                .map(Value::UserData)
                .unwrap_or(Value::Nil);
            let msg = message
                .and_then(|m| self.lua.create_string(&m).ok())
                .map_or(Value::Nil, Value::String);
            for f in ui_widgets_api::listeners(&self.lua, e, name) {
                if let Err(err) = f.call::<()>((vp.clone(), msg.clone())) {
                    self.log(LogKind::Error, format!("{name} listener: {err}"));
                }
                self.flush_spawned();
            }
        }
    }

    /// Signals call their method on the target's scripts; `stopped` runs the listeners added
    /// with `director.stopped:AddListener(f)`.
    fn director_events(&mut self, events: Vec<crate::timeline::DirectorEvent>) {
        use crate::timeline::DirectorEvent;
        for ev in events {
            match ev {
                DirectorEvent::Signal {
                    director,
                    target,
                    method,
                    parameter,
                } => {
                    let arg = parameter
                        .as_ref()
                        .and_then(|p| self.lua.create_string(p).ok())
                        .map(Value::String);
                    self.send_message_with(target, &method, arg.clone(), "Signal");
                    let d = self
                        .lua
                        .create_userdata(timeline_api::LDirector(director))
                        .map(Value::UserData)
                        .unwrap_or(Value::Nil);
                    let m = self
                        .lua
                        .create_string(&method)
                        .map(Value::String)
                        .unwrap_or(Value::Nil);
                    for f in ui_widgets_api::listeners(&self.lua, director, "signalReceived") {
                        let p = arg.clone().unwrap_or(Value::Nil);
                        if let Err(err) = f.call::<()>((d.clone(), m.clone(), p)) {
                            self.log(LogKind::Error, format!("signalReceived listener: {err}"));
                        }
                        self.flush_spawned();
                    }
                }
                DirectorEvent::Impulse(source) => {
                    self.with_world(|w| {
                        let World { cameras, scene, .. } = w;
                        cameras.impulses.generate(scene, source, None, 1.0);
                    });
                }
                DirectorEvent::Stopped(e) | DirectorEvent::Played(e) => {
                    let name = if matches!(ev, DirectorEvent::Stopped(_)) {
                        "stopped"
                    } else {
                        "played"
                    };
                    let arg = self
                        .lua
                        .create_userdata(timeline_api::LDirector(e))
                        .map(Value::UserData)
                        .ok();
                    for f in ui_widgets_api::listeners(&self.lua, e, name) {
                        let result = match &arg {
                            Some(a) => f.call::<()>(a.clone()),
                            None => f.call::<()>(()),
                        };
                        if let Err(err) = result {
                            self.log(LogKind::Error, format!("{name} listener: {err}"));
                        }
                        self.flush_spawned();
                    }
                }
            }
        }
    }

    fn dispatch(&mut self, contacts: &[Contact]) {
        use crate::visual_script::compile::EventKind;
        // Stay first, for the pairs that touched before this step and still do (only looked at
        // while some script or graph has the callback).
        let listens = |host: &Self, name: &str, kind: &EventKind| {
            let bit = lacks_bit(name);
            host.instances.iter().any(|i| i.lacks & bit == 0) || host.graphs.listens(kind)
        };
        let want_collision = listens(self, "OnCollisionStay", &EventKind::CollisionStay);
        let want_trigger = listens(self, "OnTriggerStay", &EventKind::TriggerStay);
        let alive = self.with_world(|w| {
            self.touching
                .iter()
                .map(|(a, b, _)| w.scene.contains(*a) && w.scene.contains(*b))
                .collect::<Vec<_>>()
        });
        let mut keep = alive.into_iter();
        self.touching.retain(|_| keep.next().unwrap_or(false));
        let staying = advance_touching(&mut self.touching, contacts);
        for (a, b, trigger) in staying {
            if !(if trigger {
                want_trigger
            } else {
                want_collision
            }) {
                continue;
            }
            let (event, kind) = if trigger {
                ("OnTriggerStay", EventKind::TriggerStay)
            } else {
                ("OnCollisionStay", EventKind::CollisionStay)
            };
            self.contact_event(a, b, event, &kind);
        }
        for c in contacts {
            let (event, kind) = match (c.trigger, c.started) {
                (true, true) => ("OnTriggerEnter", EventKind::TriggerEnter),
                (true, false) => ("OnTriggerExit", EventKind::TriggerExit),
                (false, true) => ("OnCollisionEnter", EventKind::CollisionEnter),
                (false, false) => ("OnCollisionExit", EventKind::CollisionExit),
            };
            self.contact_event(c.a, c.b, event, &kind);
        }
    }

    /// A 3D contact callback on both objects' scripts and graphs.
    fn contact_event(
        &mut self,
        a: Entity,
        b: Entity,
        event: &str,
        kind: &crate::visual_script::compile::EventKind,
    ) {
        for (me, other) in [(a, b), (b, a)] {
            self.graphs.contact(&self.lua, me, other, kind);
            for i in 0..self.instances.len() {
                // Disabled scripts hear contacts too (not Update), as in the familiar
                // engine; switched-off objects don't.
                if self.instances[i].entity == me && self.hears_contacts(&self.instances[i]) {
                    let arg = self
                        .lua
                        .create_userdata(LGameObject(other))
                        .map(Value::UserData)
                        .ok();
                    self.call(i, event, arg);
                }
            }
        }
    }

    /// `OnJointBreak(breakForce)` on the scripts of objects whose joint broke in the last step;
    /// then the joint component is removed (like the familiar engine does).
    fn joint_breaks(&mut self) {
        let breaks = self.with_world(|w| w.physics.take_joint_breaks());
        for b in breaks {
            for i in 0..self.instances.len() {
                if self.instances[i].entity == b.entity && self.runnable(&self.instances[i]) {
                    self.call(i, "OnJointBreak", Some(Value::Number(f64::from(b.force))));
                }
            }
            self.with_world(|w| {
                if w.scene.contains(b.entity) {
                    w.scene.remove_by_name(b.entity, b.joint);
                }
            });
            self.flush_spawned();
        }
    }

    /// `OnCollision*2D(collision)` / `OnTrigger*2D(other)` for 2D contacts.
    /// Script Graphs hear them on the matching On Trigger/Collision … 2D nodes (Other; plus
    /// Point, Normal and Relative Velocity for collisions).
    fn dispatch_2d(&mut self, contacts: &[Contact2D]) {
        use crate::visual_script::compile::EventKind;
        use crate::visual_script::value::Value as GValue;
        for c in contacts {
            let (base, kind) = match (c.trigger, c.phase) {
                (true, Phase::Enter) => ("OnTriggerEnter2D", EventKind::TriggerEnter2D),
                (true, Phase::Stay) => ("OnTriggerStay2D", EventKind::TriggerStay2D),
                (true, Phase::Exit) => ("OnTriggerExit2D", EventKind::TriggerExit2D),
                (false, Phase::Enter) => ("OnCollisionEnter2D", EventKind::CollisionEnter2D),
                (false, Phase::Stay) => ("OnCollisionStay2D", EventKind::CollisionStay2D),
                (false, Phase::Exit) => ("OnCollisionExit2D", EventKind::CollisionExit2D),
            };
            for (me, other) in [(c.a, c.b), (c.b, c.a)] {
                // Same frame as the Lua collision table: normals point away from `other`.
                let sign = if c.a == me { 1.0 } else { -1.0 };
                let v3 = |v: glam::Vec2| GValue::Vec3(glam::Vec3::new(v.x, v.y, 0.0));
                let args = [
                    GValue::Object(other),
                    v3(c.point),
                    v3(-c.normal * sign),
                    v3(c.relative_velocity * sign),
                ];
                self.graphs.contact_with(&self.lua, me, &kind, &args);
                for i in 0..self.instances.len() {
                    // Disabled scripts too, like 3D contacts.
                    if self.instances[i].entity != me || !self.hears_contacts(&self.instances[i]) {
                        continue;
                    }
                    // C# scripts get the other object (`Collision2D` / `Collider2D`).
                    let arg = if c.trigger || self.instances[i].cs.is_some() {
                        self.lua
                            .create_userdata(LGameObject(other))
                            .map(Value::UserData)
                    } else {
                        physics2d_api::collision_table(&self.lua, me, c).map(Value::Table)
                    };
                    self.call(i, base, arg.ok());
                }
            }
            self.flush_spawned();
        }
    }

    /// One game frame: Start (first frame), FixedUpdate + physics steps, Update, LateUpdate, Destroy.
    pub fn update(&mut self, scene: &mut Scene, time: &Time, input: Input) {
        let root = self.root.clone();
        let _root = crate::assets::scoped_root(&root);
        arm_deadline(&self.lua, true);
        self.update_frame(scene, time, input);
        arm_deadline(&self.lua, false);
    }

    fn update_frame(&mut self, scene: &mut Scene, time: &Time, input: Input) {
        let scene_in = std::mem::take(scene);
        self.with_world(|w| {
            w.scene = scene_in;
            // Weather events reach scripts and graphs like broadcast custom events
            // (OnLightning, OnThunder, OnWeatherChanged; scripting/weather_api.rs).
            for e in w.scene.weather.take_events() {
                w.graph_events.push(weather_api::custom_event(&e));
            }
            w.time = *time;
            w.save.mark_time(time.time);
            // On-screen sticks and buttons become a virtual gamepad / keys before anything
            // reads the input.
            let mut input = input;
            w.on_screen.apply(&mut w.scene, &mut input);
            for msg in std::mem::take(&mut w.on_screen.logs) {
                w.logs
                    .push((LogKind::Warning, format!("On-screen control {msg}")));
            }
            // Axis smoothing runs on real time, so it works while the game is paused.
            w.axes.update(&input, time.unscaled_delta);
            w.input = input;
        });
        // Scene operations carried out between frames finish (their `completed` callbacks).
        self.scene_ops_tick();
        let input_events = {
            profile_scope!("Input.Update", Category::Scripts);
            self.input_update(time.unscaled_delta)
        };
        {
            profile_scope!("PlayerInputManager", Category::Scripts);
            self.player_manager_frame();
        }
        if let Some(w) = self.lua.app_data_ref::<World>()
            && let Err(e) = api::refresh_time(&self.lua, &w)
        {
            drop(w);
            self.log(LogKind::Error, format!("Time: {e}"));
        }
        {
            let mut w = self
                .lua
                .app_data_mut::<World>()
                .expect("world is installed");
            self.csharp.set_time(&mut w);
        }

        {
            // Before Start: objects the server spawned this frame get Start like any other.
            profile_scope!("Network.Receive", Category::Scripts);
            self.network_begin(f64::from(time.unscaled_delta));
        }
        self.localization_frame();
        {
            // Before Start, so objects loaded this frame get Start before their first Update.
            profile_scope!("Assets.Update", Category::Scripts);
            self.assets_update();
        }
        {
            profile_scope!("WorldPartition.Scripts", Category::Scripts);
            self.world_partition_tick();
        }
        {
            profile_scope!("Scripts.Start", Category::Scripts);
            // OnEnable before Start for objects switched on since the last frame.
            self.sync_enabled();
            for i in 0..self.instances.len() {
                if !self.instances[i].started
                    && self.instances[i].enabled
                    && self.runnable(&self.instances[i])
                {
                    self.instances[i].started = true;
                    self.call(i, "Start", None);
                    self.flush_spawned();
                }
            }
        }

        {
            profile_scope!("ScriptGraphs.Start", Category::Scripts);
            self.graphs.start_phase(&self.lua);
            self.flush_spawned();
        }

        {
            // After Start (listeners added there hear this frame), before Update.
            profile_scope!("Input.Events", Category::Scripts);
            self.input_events(input_events);
            self.graphs.input_phase(&self.lua);
            self.flush_spawned();
        }
        {
            profile_scope!("UI.Events", Category::Ui);
            self.ui_events();
        }

        let steps = self.with_world(|w| w.physics.steps_due(time.delta));
        for _ in 0..steps {
            {
                profile_scope!("Scripts.FixedUpdate", Category::Scripts);
                self.call_all("FixedUpdate");
                let fixed = self.with_world(|w| w.physics.fixed_dt);
                self.graphs.fire_all(
                    &self.lua,
                    &crate::visual_script::compile::EventKind::FixedUpdate,
                    &[crate::visual_script::Value::Float(fixed)],
                );
                self.flush_spawned();
            }
            profile_scope!("Physics.Step", Category::Physics);
            let contacts = self.with_world(|w| w.physics.step(&w.scene));
            self.dispatch(&contacts);
            // Impulse Sources that fire on collision.
            self.with_world(|w| w.cameras.impulses.on_contacts(&w.scene, &contacts));
            self.flush_spawned();
            self.joint_breaks();
            let contacts_2d = self.with_world(|w| {
                w.physics2d.sync_active(&w.scene);
                w.physics2d.refresh_tilemaps(&w.scene);
                w.physics2d.step()
            });
            self.dispatch_2d(&contacts_2d);
        }
        {
            profile_scope!("Physics.SyncTransforms", Category::Physics);
            self.with_world(|w| {
                w.physics.sync_out(&mut w.scene);
                w.physics2d.sync_out(&mut w.scene);
            });
        }
        if crate::profiler::enabled() {
            self.with_world(|w| w.physics.report_profiler(steps));
        }
        if steps > 0 {
            let dt = steps as f32 * self.with_world(|w| w.physics.fixed_dt);
            self.update_cameras(crate::vcam::BrainUpdate::FixedUpdate, dt);
        }

        {
            profile_scope!("Scripts.Update", Category::Scripts);
            self.call_all("Update");
            // C# coroutines resume after Update.
            {
                let mut w = self
                    .lua
                    .app_data_mut::<World>()
                    .expect("world is installed");
                self.csharp.tick(&mut w);
            }
            self.flush_spawned();
        }
        {
            profile_scope!("ScriptGraphs.Update", Category::Scripts);
            let t = std::time::Instant::now();
            self.graphs.update_phase(&self.lua, time.delta);
            self.graph_update_time = t.elapsed();
            self.flush_spawned();
        }
        {
            // After the scripts and graphs (they set blackboard values), before navigation (a
            // Move To started now moves this frame).
            profile_scope!("BehaviourTrees.Update", Category::Scripts);
            self.update_behaviour_trees(time.delta);
        }
        {
            profile_scope!("Navigation.Update", Category::Navigation);
            self.with_world(|w| {
                w.navigation.update(&mut w.scene, time.delta);
                let logs = w.navigation.take_logs();
                w.logs
                    .extend(logs.into_iter().map(|l| (LogKind::Warning, l)));
            });
            self.dispatch_navigation_events();
        }
        {
            profile_scope!("Animation.Update", Category::Animation);
            self.animate(time.delta);
            self.with_world(|w| crate::splines::animate::update(&mut w.scene, time.delta));
        }
        {
            profile_scope!("Video.Update", Category::Others);
            self.update_video(time.delta);
        }
        {
            // Rigs run on the animated pose, before LateUpdate and rendering.
            profile_scope!("Animation.Rigging", Category::Animation);
            self.with_world(|w| {
                crate::rigging::evaluate(&mut w.scene, &mut w.rigging, time.delta, None)
            });
        }
        {
            // Ragdolls put their physics pose over the animation; cloth starts its next step.
            profile_scope!("Physics.Ragdoll+Cloth", Category::Physics);
            self.with_world(|w| {
                let World { physics, scene, .. } = w;
                physics.after_animation(scene, time.delta);
            });
        }
        {
            profile_scope!("Scripts.LateUpdate", Category::Scripts);
            self.call_all("LateUpdate");
            self.graphs.fire_all(
                &self.lua,
                &crate::visual_script::compile::EventKind::LateUpdate,
                &[],
            );
            self.flush_spawned();
        }
        self.update_cameras(crate::vcam::BrainUpdate::LateUpdate, time.delta);
        self.process_destroy();
        {
            profile_scope!("Network.Send", Category::Scripts);
            self.network_end();
        }
        self.localization_frame();

        *scene = self.with_world(|w| std::mem::take(&mut w.scene));
    }

    /// Virtual cameras and brains whose Update Method is `pass`.
    fn update_cameras(&mut self, pass: crate::vcam::BrainUpdate, dt: f32) {
        profile_scope!("Cameras.Update", Category::Others);
        self.with_world(|w| {
            let World {
                scene,
                physics,
                axes,
                input,
                input_system,
                cameras,
                logs,
                ..
            } = w;
            let size = input.screen_size;
            let aspect = if size.x > 0.0 && size.y > 0.0 {
                size.x / size.y
            } else {
                16.0 / 9.0
            };
            let axis = |name: &str| axes.axis(name).unwrap_or(0.0);
            let action = |name: &str| crate::vcam::solve::action_value(input_system, name);
            let cx = crate::vcam::SolveContext {
                dt,
                aspect,
                physics: Some(physics),
                axis: &axis,
                action: &action,
            };
            cameras.update(scene, pass, &cx);
            logs.extend(
                cameras
                    .take_logs()
                    .into_iter()
                    .map(|l| (LogKind::Warning, l)),
            );
        });
    }

    /// Start or stop Player Inputs to match the scene.
    fn input_sync(&mut self) {
        self.with_world(|w| {
            let World {
                input_system,
                scene,
                input,
                logs,
                ..
            } = w;
            input_system.sync_players(scene, input);
            logs.extend(input_system.logs.drain(..).map(|l| (LogKind::Warning, l)));
        });
    }

    /// Step the Input System with this frame's input.
    fn input_update(&mut self, dt: f32) -> Vec<crate::input::actions::SystemEvent> {
        self.with_world(|w| {
            let World {
                input_system,
                scene,
                input,
                logs,
                ..
            } = w;
            let events = input_system.update(scene, input, dt);
            logs.extend(input_system.logs.drain(..).map(|l| (LogKind::Warning, l)));
            events
        })
    }

    /// Gamepad motor speeds scripts asked for since the last call: (slot, low, high).
    pub fn take_rumble(&self) -> Option<Vec<(usize, f32, f32)>> {
        self.with_world(|w| w.input_system.take_rumble())
    }

    /// A playing Player Input as Inspector rows (index, scheme, devices, map, active).
    pub fn player_input_info(&self, e: Entity) -> Option<Vec<(String, String)>> {
        self.with_world(|w| {
            let p = w.input_system.player(e)?;
            let set = w.input_system.set(p.set)?;
            let map = p
                .map
                .and_then(|m| set.asset.maps.get(m))
                .map_or("<None>".to_owned(), |m| m.name.clone());
            let devices: Vec<String> = p.devices.iter().map(|d| d.name(&w.input)).collect();
            Some(vec![
                ("Player Index".into(), p.index.to_string()),
                (
                    "Control Scheme".into(),
                    p.scheme.clone().unwrap_or_else(|| "<None>".into()),
                ),
                ("Devices".into(), devices.join(", ")),
                ("Action Map".into(), map),
                ("Input Active".into(), p.active.to_string()),
            ])
        })
    }

    /// Stop every gamepad motor (Play stopping).
    pub fn reset_haptics(&self) {
        self.with_world(|w| w.input_system.reset_haptics());
    }

    /// Camera Brains and Virtual Cameras between frames (live camera, blends).
    pub fn with_cameras<R>(&self, f: impl FnOnce(&crate::vcam::CameraRig) -> R) -> R {
        self.with_world(|w| f(&w.cameras))
    }

    /// UI events (before FixedUpdate/Update, like an event system): hover/press/selection,
    /// widget input, then On Click / On Value Changed calls and Lua listeners.
    fn ui_events(&mut self) {
        let before = self.ui.selected;
        let mut events = Vec::new();
        {
            let Some(mut w) = self.lua.app_data_mut::<World>() else {
                return;
            };
            let w = &mut *w;
            if let Some(req) = w.ui_select_request.take() {
                events.extend(self.ui.select(&mut w.scene, req));
            }
            let mut pointer = w.input.ui_pointer(w.time.unscaled_delta);
            // Navigation through the Input System's UI map, else the keys and joystick.
            let asset = crate::ui::events::settings(&w.scene).actions_asset;
            match w.input_system.ui_actions(asset.as_deref()) {
                Some(a) => {
                    pointer.nav_from_actions = true;
                    pointer.navigate = a.navigate;
                    pointer.submit = a.submit;
                    pointer.cancel = a.cancel;
                }
                None => {
                    (pointer.navigate, pointer.submit, pointer.cancel) = w.input.ui_legacy_pad();
                }
            }
            events.extend(self.ui.update(&mut w.scene, &pointer));
            w.ui_selected = self.ui.selected;
        }
        // OnDeselect / OnSelect when the selection moved (pointer, navigation or a script),
        // before the clicks and value changes, like Unity's event system.
        let after = self.ui.selected;
        if before != after {
            if let Some(old) = before.filter(|e| self.with_world(|w| w.scene.contains(*e))) {
                self.ui_selection_changed(old, false);
            }
            if let Some(new) = after {
                self.ui_selection_changed(new, true);
            }
        }
        for ev in events {
            self.ui_event(ev);
        }
    }

    /// Call `method` on every running script of `target` that defines it, and run the Custom
    /// Event nodes named `method` in its Script Graphs.
    pub fn send_message(&mut self, target: Entity, method: &str) {
        self.send_message_with(target, method, None, "SendMessage");
    }

    /// Like [`ScriptHost::send_message`] with an argument (a graph's Custom Event gets it as its
    /// first argument); `what` names the caller in warnings (On Click, On Value Changed, Signal…).
    fn send_message_with(&mut self, target: Entity, method: &str, arg: Option<Value>, what: &str) {
        let ev = crate::visual_script::runtime::CustomEvent {
            target: Some(target),
            name: std::rc::Rc::from(method),
            args: arg
                .iter()
                .map(|v| crate::visual_script::lua_bridge::from_lua(&self.lua, v.clone()))
                .collect(),
        };
        let mut found = self.graphs.custom(&self.lua, &ev);
        if found {
            self.flush_spawned();
        }
        for i in 0..self.instances.len() {
            let inst = &self.instances[i];
            if inst.entity != target || !self.runnable(inst) {
                continue;
            }
            if let Some(cs) = inst.cs {
                if self.cs_message(cs, method, arg.as_ref()) {
                    found = true;
                    self.flush_spawned();
                }
                continue;
            }
            let defined = self
                .lua
                .registry_value::<Table>(&inst.table)
                .and_then(|t| t.get::<Option<Function>>(method))
                .is_ok_and(|f| f.is_some());
            if defined {
                found = true;
                self.call(i, method, arg.clone());
                self.flush_spawned();
            }
        }
        if !found {
            let name = self.with_world(|w| {
                if w.scene.contains(target) {
                    w.scene.name(target)
                } else {
                    "a destroyed object".into()
                }
            });
            self.log(
                LogKind::Warning,
                format!("{what}: no script on '{name}' has a method '{method}'"),
            );
        }
    }

    /// Call C# method `method` of instance `cs` by name, with a Lua argument (one C# can't take
    /// is left out). Whether the class has it.
    pub(crate) fn cs_message(&self, cs: csharp::CsRef, method: &str, arg: Option<&Value>) -> bool {
        let arg = arg.and_then(csharp::CsArg::from_lua);
        let mut w = self
            .lua
            .app_data_mut::<World>()
            .expect("world is installed");
        self.csharp.message(&mut w, cs, method, arg.as_ref())
    }

    /// Button hover/press state (the Game view and player tint buttons with it).
    pub fn ui_interaction(&self) -> &UiInteraction {
        &self.ui
    }

    /// Objects added between frames (an additively loaded scene): physics bodies, script
    /// instances and `Awake`, like `Instantiate`.
    pub fn add_objects(&mut self, scene: &mut Scene, roots: &[Entity]) {
        let scene_in = std::mem::take(scene);
        self.with_world(|w| {
            w.scene = scene_in;
            // One broad-phase update for all of them.
            w.physics.add_subtrees(&w.scene, roots);
            w.physics2d.add_subtrees(&w.scene, roots);
            w.spawned.extend_from_slice(roots);
        });
        self.flush_spawned();
        *scene = self.with_world(|w| std::mem::take(&mut w.scene));
    }

    /// [`ScriptHost::add_objects`] for World Partition objects: their bodies join the physics
    /// world in the next step (no extra broad-phase pass now).
    pub fn add_streamed_objects(&mut self, scene: &mut Scene, roots: &[Entity]) {
        let scene_in = std::mem::take(scene);
        self.with_world(|w| {
            w.scene = scene_in;
            w.physics.add_subtrees_deferred(&w.scene, roots);
            w.physics2d.add_subtrees(&w.scene, roots);
            w.spawned.extend_from_slice(roots);
        });
        self.flush_spawned();
        *scene = self.with_world(|w| std::mem::take(&mut w.scene));
    }

    /// Root objects marked with `DontDestroyOnLoad` that still exist in `scene`.
    pub fn dont_destroy_roots(&self, scene: &Scene) -> Vec<Entity> {
        let marked = self.with_world(|w| {
            w.dont_destroy.retain(|e| scene.contains(*e));
            w.dont_destroy.clone()
        });
        let mut roots: Vec<Entity> = marked
            .into_iter()
            .map(|mut e| {
                while let Some(p) = scene.parent(e) {
                    e = p;
                }
                e
            })
            .collect();
        roots.sort_unstable_by_key(|e| e.to_bits());
        roots.dedup();
        roots
    }

    /// Load the nav meshes of `scene` again (its active scene changed while playing).
    pub fn reload_navigation(&mut self, scene: &Scene) {
        let root = self.root.clone();
        let nav = crate::navigation::NavRuntime::load(scene, &root);
        self.with_world(|w| w.navigation = nav);
    }

    /// Remove objects between frames (an unloaded scene) with their bodies and scripts.
    pub fn remove_objects(&mut self, scene: &mut Scene, roots: &[Entity]) {
        let scene_in = std::mem::take(scene);
        self.with_world(|w| {
            w.scene = scene_in;
            w.destroy.extend_from_slice(roots);
        });
        self.process_destroy();
        *scene = self.with_world(|w| std::mem::take(&mut w.scene));
    }

    /// Remove objects queued by `Destroy` (with their children and physics bodies).
    fn process_destroy(&mut self) {
        self.remove_destroyed_scripts();
        let queued = self.with_world(|w| std::mem::take(&mut w.destroy));
        for e in queued {
            let subtree: Vec<Entity> = self.with_world(|w| {
                if !w.scene.contains(e) {
                    return Vec::new();
                }
                w.scene.subtree(e)
            });
            // Spawned network objects are despawned first (OnNetworkDespawn, clients told).
            self.network_destroying(&subtree);
            // OnDisable (scripts that were enabled), then OnDestroy (scripts that woke).
            self.destroying(&subtree);
            // Script Graphs get OnDestroy while the objects still exist.
            self.graphs.destroy(&self.lua, &subtree);
            let doomed: Vec<Entity> = self.with_world(|w| {
                if !w.scene.contains(e) {
                    return Vec::new();
                }
                let all = subtree;
                for x in &all {
                    w.physics.remove(*x);
                    w.physics2d.remove(*x);
                }
                w.scene.despawn(e);
                w.behaviour.forget(&all);
                all
            });
            for i in &self.instances {
                if let Some(cs) = i.cs.filter(|_| doomed.contains(&i.entity)) {
                    self.csharp.release(cs);
                }
            }
            self.instances.retain(|i| !doomed.contains(&i.entity));
            save_api::untrack(&self.lua, &doomed);
            unregister_script_instances(&self.lua, &doomed);
            ui_widgets_api::forget(&self.lua, &doomed);
            input_actions_api::forget(&self.lua, &doomed);
        }
    }

    pub fn take_logs(&self) -> Vec<(LogKind, String)> {
        self.with_world(|w| std::mem::take(&mut w.logs))
    }

    /// What scripts asked of the cursor (`Cursor.lockState`, `Cursor.visible`).
    pub fn cursor(&self) -> CursorState {
        self.with_world(|w| w.cursor)
    }

    /// Give the cursor back (the editor's Escape, a lost window focus): scripts then read
    /// `Cursor.lockState == CursorLockMode.None` and may lock it again on a click.
    pub fn release_cursor(&self) {
        self.with_world(|w| w.cursor = CursorState::default());
    }

    /// Scene loads and quit requests scripts made since the last call.
    pub fn take_requests(&self) -> Vec<Request> {
        self.with_world(|w| std::mem::take(&mut w.requests))
    }

    /// Physics counters for the Stats overlay.
    pub fn physics_counts(&self) -> (usize, usize) {
        self.with_world(|w| {
            (
                w.physics.body_count() + w.physics2d.body_count(),
                w.physics.collider_count() + w.physics2d.collider_count(),
            )
        })
    }

    /// Project Settings ▸ Physics the world was built with.
    pub fn physics_settings(&self) -> crate::settings::PhysicsSettings {
        self.with_world(|w| w.physics.settings().clone())
    }

    pub fn physics_config(&self) -> (Vec3, f32) {
        self.with_world(|w| (w.physics.gravity(), w.physics.fixed_dt))
    }

    /// The playing physics world (debug view, cloth stats).
    pub fn with_physics<R>(&self, f: impl FnOnce(&mut Physics) -> R) -> R {
        self.with_world(|w| f(&mut w.physics))
    }

    /// The playing physics world and scene together (ragdoll switches from the editor).
    pub fn with_physics_scene<R>(
        &mut self,
        scene: &mut Scene,
        f: impl FnOnce(&mut Physics, &mut Scene) -> R,
    ) -> R {
        let taken = std::mem::take(scene);
        let (r, back) = self.with_world(|w| {
            let mine = std::mem::replace(&mut w.scene, taken);
            let r = f(&mut w.physics, &mut w.scene);
            let played = std::mem::replace(&mut w.scene, mine);
            (r, played)
        });
        *scene = back;
        r
    }

    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    /// How long the last frame's Script Graph Update pass took (every graph's On Update,
    /// Delays and timers).
    pub fn graph_update_time(&self) -> std::time::Duration {
        self.graph_update_time
    }

    /// Script Graphs running on objects.
    pub fn graph_instance_count(&self) -> usize {
        self.graphs.instance_count()
    }

    /// The Script Graph debugger (graph, breakpoints, watched object; hits, errors, pause).
    pub fn graph_debugger(&mut self) -> &mut crate::visual_script::runtime::Debugger {
        &mut self.graphs.debug
    }

    /// Value on a pin of the watched graph instance, as text (the debugger's hover).
    pub fn graph_value(&self, node: u32, pin: &str, output: bool) -> Option<String> {
        self.with_world(|w| {
            self.graphs
                .value(w, node, pin, output)
                .map(|v| v.to_text(Some(&w.scene)))
        })
    }

    /// Variables of the watched graph instance (name, value as text).
    pub fn graph_variables(&self) -> Vec<(String, String)> {
        self.with_world(|w| {
            self.graphs
                .watched_variables(w)
                .into_iter()
                .map(|(n, v)| (n, v.to_text(Some(&w.scene))))
                .collect()
        })
    }

    /// Between frames: step the flow paused at a breakpoint to the next node (`step`), or let
    /// it run on.
    pub fn graph_continue(&mut self, scene: &mut Scene, step: bool) {
        let scene_in = std::mem::take(scene);
        self.with_world(|w| w.scene = scene_in);
        if step {
            self.graphs.step(&self.lua);
        } else {
            self.graphs.resume(&self.lua);
        }
        self.flush_spawned();
        *scene = self.with_world(|w| std::mem::take(&mut w.scene));
    }

    /// Object variables of an object (name, value), e.g. for tests and the Inspector.
    pub fn object_variables(&self, e: Entity) -> Vec<(String, crate::visual_script::Value)> {
        self.with_world(|w| {
            w.graph_vars
                .of(e)
                .into_iter()
                .map(|(n, v)| (n.to_string(), v))
                .collect()
        })
    }

    /// Custom events waiting (graphs' Send Custom Event, `gameObject:SendCustomEvent`):
    /// delivered to Custom Event nodes and to Lua functions of that name. True when any were.
    fn deliver_custom_events(&mut self) -> bool {
        let mut any = false;
        for _ in 0..256 {
            let events = self.with_world(|w| std::mem::take(&mut w.graph_events));
            if events.is_empty() {
                return any;
            }
            any = true;
            for ev in events {
                self.graphs.custom(&self.lua, &ev);
                self.custom_event_to_lua(&ev);
            }
        }
        self.with_world(|w| w.graph_events.clear());
        self.log(
            LogKind::Error,
            "Custom events kept sending more custom events; stopped".into(),
        );
        any
    }

    /// A custom event calls the function of its name on the target's Lua scripts (or every
    /// object's, when broadcast).
    fn custom_event_to_lua(&self, ev: &crate::visual_script::runtime::CustomEvent) {
        for inst in &self.instances {
            if ev.target.is_some_and(|t| t != inst.entity) || !self.runnable(inst) {
                continue;
            }
            let Ok(t) = self.lua.registry_value::<Table>(&inst.table) else {
                continue;
            };
            let Ok(Some(f)) = t.get::<Option<Function>>(&*ev.name) else {
                continue;
            };
            let mut args = vec![Value::Table(t)];
            for a in &ev.args {
                args.push(
                    crate::visual_script::lua_bridge::to_lua(&self.lua, a).unwrap_or(Value::Nil),
                );
            }
            if let Err(e) = f.call::<()>(mlua::MultiValue::from_vec(args)) {
                self.log(LogKind::Error, format!("{} ({}): {e}", inst.name, ev.name));
            }
        }
    }
}

/// Longest the game scripts of one frame (or of `Awake`/`Start` when the game starts) may run
/// before the running script is stopped with an error: `while true do end` in an `Update`
/// would otherwise freeze the editor (Play runs on its UI thread) or the game for good.
pub const FRAME_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Longest a script's top level may run when the editor reads its fields (on every save, on
/// the UI thread): `while true do end` there froze the editor, and again on every open.
pub const INSPECT_LIMIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Most memory a script's top level may take when the editor reads its fields.
const INSPECT_MEMORY: usize = 256 << 20;

/// Standard libraries game scripts get (`debug`, `io` and `package` are left out).
pub fn game_libs() -> mlua::StdLib {
    use mlua::StdLib as L;
    L::TABLE | L::STRING | L::MATH | L::UTF8 | L::COROUTINE | L::OS
}

/// A Lua state for game scripts: [`game_libs`], then [`sandbox`].
pub fn game_lua() -> Lua {
    let lua = Lua::new_with(game_libs(), mlua::LuaOptions::default())
        .expect("the safe standard libraries load");
    sandbox(&lua).expect("the sandbox chunk runs");
    lua
}

/// Take away what lets a game script touch the machine: files (`dofile`, `loadfile`), modules
/// (`require`), processes and the environment (`os` keeps only `time`, `clock`, `date` and
/// `difftime`), and precompiled chunks (`load` takes source text only: crafted bytecode can
/// crash Lua).
pub fn sandbox(lua: &Lua) -> mlua::Result<()> {
    lua.load(
        r#"
local os_, load_ = os, load
os = os_ and { time = os_.time, clock = os_.clock, date = os_.date, difftime = os_.difftime }
io, package, require, dofile, loadfile = nil, nil, nil, nil, nil
load = function(chunk, name, mode, ...) return load_(chunk, name, "t", ...) end
"#,
    )
    .set_name("=sandbox")
    .exec()
}

fn load_class(lua: &Lua, root: &std::path::Path, path: &str) -> Result<Table, String> {
    let source = crate::vfs::read_to_string(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
    // `--#if DEFINE` blocks of the Build Profile's scripting defines.
    let source =
        crate::build_profile::preprocess_lua(&source, &crate::build_profile::active_defines());
    eval_class(lua, &source, path)
}

fn eval_class(lua: &Lua, source: &str, path: &str) -> Result<Table, String> {
    match lua.load(source).set_name(path).eval::<Value>() {
        Ok(Value::Table(t)) => Ok(t),
        Ok(_) => Err(format!(
            "{path}: the script must end with `return <ClassTable>`"
        )),
        Err(e) => Err(format!("{e}")),
    }
}

fn to_lua(lua: &Lua, v: &ScriptValue) -> mlua::Result<Value> {
    Ok(match v {
        ScriptValue::Bool(b) => Value::Boolean(*b),
        ScriptValue::Number(n) => Value::Number(*n),
        ScriptValue::Text(s) => Value::String(lua.create_string(s)?),
        ScriptValue::Vector3(a) => mlua::IntoLua::into_lua(LVec3(glam::Vec3::from(*a)), lua)?,
        // An object field: its Transform when asked for one, else the object (components and
        // scripts through GetComponent); nil when empty.
        ScriptValue::Object(o) => match o.entity() {
            Some(e) if o.kind == "Transform" => {
                Value::UserData(lua.create_userdata(LTransform(e))?)
            }
            Some(e) => Value::UserData(lua.create_userdata(LGameObject(e))?),
            None => Value::Nil,
        },
    })
}

/// Public fields of a script (name, default) in declaration order, for the Inspector.
pub fn inspect_fields(source: &str, path: &str) -> Result<Vec<(String, ScriptValue)>, String> {
    inspect_script(source, path).map(|info| info.fields)
}

/// What the Inspector needs of a script: its public fields and their attributes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScriptInfo {
    /// Public fields (name, default) in declaration order.
    pub fields: Vec<(String, ScriptValue)>,
    /// `Header`, `Range`, … by field name (see [`attributes`]).
    pub attributes: HashMap<String, attributes::FieldAttributes>,
}

/// Public fields of a script with their Inspector attributes.
pub fn inspect_script(source: &str, path: &str) -> Result<ScriptInfo, String> {
    if csharp::is_csharp_path(path) {
        return Ok(csharp_fields::inspect(source, path));
    }
    let lua = game_lua();
    api::register(&lua).map_err(|e| e.to_string())?;
    // The top level runs here: an endless loop or a runaway table must not hang the editor.
    install_deadline(&lua);
    arm_deadline_for(&lua, Some(INSPECT_LIMIT));
    let _ = lua.set_memory_limit(lua.used_memory() + INSPECT_MEMORY);
    let class = eval_class(&lua, source, path)?;
    arm_deadline_for(&lua, None);
    let attributes = attributes::parse(&class);
    let mut fields = Vec::new();
    for pair in class.pairs::<Value, Value>() {
        let (k, v) = pair.map_err(|e| e.to_string())?;
        let Value::String(k) = k else { continue };
        let name = k.to_string_lossy();
        if name.starts_with('_') {
            continue;
        }
        let value = match v {
            Value::Boolean(b) => ScriptValue::Bool(b),
            Value::Integer(i) => ScriptValue::Number(i as f64),
            Value::Number(n) => ScriptValue::Number(n),
            Value::String(s) => ScriptValue::Text(s.to_string_lossy()),
            Value::Table(_) if api::vec3_in(&v).is_some() => {
                ScriptValue::Vector3(api::vec3_in(&v).unwrap_or_default().to_array())
            }
            Value::Table(t) => match t.raw_get::<Option<String>>(api::OBJECT_FIELD_KEY) {
                Ok(Some(kind)) => ScriptValue::Object(crate::scene::ObjectValue::none(kind)),
                _ => continue,
            },
            _ => continue,
        };
        fields.push((name, value));
    }
    // Declaration order: first `name =` in the source.
    fields.sort_by_key(|(name, _)| {
        source
            .find(&format!("{name} ="))
            .or_else(|| source.find(name.as_str()))
            .unwrap_or(usize::MAX)
    });
    Ok(ScriptInfo { fields, attributes })
}

/// Run a Lua `chunk` against script classes, for unit tests of script logic without a scene:
/// each `(name, source)` of `scripts` is evaluated like a script file and stored in the global
/// `name`; the chunk then runs with the game API registered (no world, so `Time`, `Input` and
/// objects are unavailable, but `Vector3`, `Mathf` and the classes' own functions work).
/// Returns the chunk's error, if any.
pub fn test_scripts(scripts: &[(&str, &str)], chunk: &str) -> Result<(), String> {
    let lua = game_lua();
    api::register(&lua).map_err(|e| e.to_string())?;
    for (name, source) in scripts {
        let class = eval_class(&lua, source, name)?;
        lua.globals().set(*name, class).map_err(|e| e.to_string())?;
    }
    lua.load(chunk)
        .set_name("test")
        .exec()
        .map_err(|e| e.to_string())
}

/// Starter script for Assets ▸ Create ▸ Script.
pub fn template(class_name: &str) -> String {
    format!(
        r#"-- {class_name}: attach to a GameObject. Plain values below are public fields shown in the Properties panel.
local {class_name} = {{
    speed = 1.0,
}}

-- Called once before the first frame.
function {class_name}:Start()
end

-- Called every frame.
function {class_name}:Update()
end

return {class_name}
"#
    )
}

#[cfg(test)]
mod assets_tests;
#[cfg(test)]
mod behaviour_tree_tests;
#[cfg(test)]
mod csharp_tests;
#[cfg(test)]
mod input_tests;
#[cfg(test)]
mod lua_overhead_bench;
#[cfg(test)]
mod material_tests;
#[cfg(test)]
mod network_tests;
#[cfg(test)]
mod scene_async_tests;
#[cfg(test)]
mod set_active_tests;
#[cfg(test)]
mod ships_bench;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tilemap_tests;
