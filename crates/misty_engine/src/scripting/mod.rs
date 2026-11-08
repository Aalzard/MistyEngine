//! Lua scripting with a MonoBehaviour-style lifecycle.
//!
//! A script file returns a class table. Plain values in it (numbers, booleans, strings, Vector3)
//! are public fields shown in the Inspector; functions are callbacks:
//! `Awake`, `Start`, `Update`, `FixedUpdate`, `LateUpdate`,
//! `OnCollisionEnter/Exit(other)`, `OnTriggerEnter/Exit(other)`, `OnControllerColliderHit(hit)`,
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

pub mod animator_api;
pub mod api;
pub mod app_api;
pub mod assets_api;
pub mod attributes;
pub mod audio_api;
pub mod camera_api;
pub mod cursor_api;
pub mod input_actions_api;
pub mod input_api;
pub mod light_probes_api;
pub mod localization_api;
pub mod material_api;
pub mod navigation_api;
pub mod network_api;
mod override_api;
pub mod particles_api;
pub mod physics2d_api;
pub mod physics_api;
pub mod render_settings_api;
pub mod rigging_api;
pub mod save_api;
mod save_host;
pub mod scene_api;
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
            cameras: Default::default(),
            video: crate::video::VideoPlayback::new(project_root_for_video),
            video_events: Vec::new(),
            cursor: CursorState::default(),
            network,
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
    started: bool,
}

pub struct ScriptHost {
    lua: Lua,
    root: PathBuf,
    classes: HashMap<String, Option<RegistryKey>>,
    instances: Vec<Instance>,
    /// UI button hover/press state.
    ui: UiInteraction,
    /// Network class info cache and settle state.
    net: network_api::NetHostState,
}

impl ScriptHost {
    /// Bytes allocated by the Lua state.
    pub fn lua_memory(&self) -> usize {
        self.lua.used_memory()
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
        let lua = Lua::new();
        let mut logs = Vec::new();
        if let Err(e) = api::register(&lua) {
            logs.push((LogKind::Error, format!("Script API failed to load: {e}")));
        }
        lua.set_app_data(crate::asset_groups::AssetSystem::auto(&project_root));
        let mut world = World::new(
            std::mem::take(scene),
            physics,
            physics2d,
            project_root.clone(),
            audio,
            settings,
        );
        logs.append(&mut world.logs);
        world.logs = logs;
        lua.set_app_data(world);
        let mut host = Self {
            lua,
            root: project_root,
            classes: HashMap::new(),
            instances: Vec::new(),
            ui: UiInteraction::default(),
            net: Default::default(),
        };
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
        for (e, list) in scripted {
            for (slot, script) in list.iter().enumerate() {
                host.instantiate(e, slot, script);
            }
        }
        // Player Inputs enable their default map before any Awake.
        host.input_sync();
        host.call_all("Awake");
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

    /// The Playable Directors of this game (between frames).
    pub fn with_timeline<R>(&self, f: impl FnOnce(&mut crate::timeline::DirectorPlayer) -> R) -> R {
        self.with_world(|w| f(&mut w.timeline))
    }

    /// Nav Mesh Agents of this game (between frames).
    pub fn with_navigation<R>(&self, f: impl FnOnce(&crate::navigation::NavRuntime) -> R) -> R {
        self.with_world(|w| f(&w.navigation))
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
                t.set(k.as_str(), to_lua(&self.lua, v)?)?;
            }
            t.set("gameObject", LGameObject(e))?;
            t.set("transform", LTransform(e))?;
            let meta = self.lua.create_table()?;
            meta.set("__index", class.clone())?;
            t.set_metatable(Some(meta))?;
            Ok(t)
        };
        let tracked = build().and_then(|t| {
            network_api::prepare_instance(&self.lua, &t, &class, &script_name, &info, networked)?;
            save_api::track(&self.lua, e, &key, &t)?;
            register_script_instance(&self.lua, e, &script.name(), &t)?;
            Ok(t)
        });
        match tracked.and_then(|t| self.lua.create_registry_value(t)) {
            Ok(table) => self.instances.push(Instance {
                entity: e,
                slot,
                name: script.name(),
                table,
                started: false,
            }),
            Err(err) => self.log(LogKind::Error, format!("{}: {err}", script.name())),
        }
    }

    /// Whether an instance should receive callbacks (object alive + active, script enabled).
    fn runnable(&self, i: &Instance) -> bool {
        self.with_world(|w| {
            w.scene.contains(i.entity)
                && w.scene.active_in_hierarchy(i.entity)
                && w.scene
                    .world
                    .get::<&Scripts>(i.entity)
                    .ok()
                    .and_then(|s| s.list.get(i.slot).map(|r| r.enabled))
                    .unwrap_or(false)
        })
    }

    fn call(&self, index: usize, name: &str, arg: Option<Value>) {
        let inst = &self.instances[index];
        let Ok(t) = self.lua.registry_value::<Table>(&inst.table) else {
            return;
        };
        let f = match t.get::<Option<Function>>(name) {
            Ok(Some(f)) => f,
            _ => return,
        };
        // Per script file and method, e.g. "Orbit.Update()".
        profile_scope!(format!("{}.{name}()", inst.name));
        let result = match arg {
            Some(a) => f.call::<()>((t, a)),
            None => f.call::<()>(t),
        };
        if let Err(e) = result {
            self.log(LogKind::Error, format!("{} ({name}): {e}", inst.name));
        }
    }

    fn call_all(&mut self, name: &str) {
        // Objects created during this pass join from the next one (after their Awake).
        for i in 0..self.instances.len() {
            if self.runnable(&self.instances[i]) {
                self.call(i, name, None);
                self.flush_spawned();
            }
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
        self.controller_hits();
        // Awake may instantiate more; stop runaway chains instead of hanging.
        for _ in 0..64 {
            let spawned = self.with_world(|w| std::mem::take(&mut w.spawned));
            if spawned.is_empty() {
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
                .walk()
                .into_iter()
                .map(|(e, _)| e)
                .filter(|e| w.scene.is_ancestor(root, *e))
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
        for i in first..self.instances.len() {
            if self.runnable(&self.instances[i]) {
                self.call(i, "Awake", None);
            }
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

    /// Objects with a runnable script instance defining `function`.
    fn entities_with(&self, function: &str) -> HashSet<Entity> {
        self.instances
            .iter()
            .filter(|i| {
                self.lua
                    .registry_value::<Table>(&i.table)
                    .ok()
                    .and_then(|t| t.get::<Option<Function>>(function).ok().flatten())
                    .is_some()
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
        let script_moves = self.entities_with("OnAnimatorMove");
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
                ..
            } = w;
            timeline.update(scene, audio, dt, unscaled)
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
                    target,
                    method,
                    parameter,
                } => {
                    let arg = parameter
                        .and_then(|p| self.lua.create_string(&p).ok())
                        .map(Value::String);
                    self.send_message_with(target, &method, arg, "Signal");
                }
                DirectorEvent::Stopped(e) => {
                    let arg = self
                        .lua
                        .create_userdata(timeline_api::LDirector(e))
                        .map(Value::UserData)
                        .ok();
                    for f in ui_widgets_api::listeners(&self.lua, e, "stopped") {
                        let result = match &arg {
                            Some(a) => f.call::<()>(a.clone()),
                            None => f.call::<()>(()),
                        };
                        if let Err(err) = result {
                            self.log(LogKind::Error, format!("stopped listener: {err}"));
                        }
                        self.flush_spawned();
                    }
                }
            }
        }
    }

    fn dispatch(&self, contacts: &[Contact]) {
        for c in contacts {
            let event = match (c.trigger, c.started) {
                (true, true) => "OnTriggerEnter",
                (true, false) => "OnTriggerExit",
                (false, true) => "OnCollisionEnter",
                (false, false) => "OnCollisionExit",
            };
            for (me, other) in [(c.a, c.b), (c.b, c.a)] {
                for (i, inst) in self.instances.iter().enumerate() {
                    if inst.entity == me && self.runnable(inst) {
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
    }

    /// `OnCollision*2D(collision)` / `OnTrigger*2D(other)` for 2D contacts.
    fn dispatch_2d(&mut self, contacts: &[Contact2D]) {
        for c in contacts {
            let base = match (c.trigger, c.phase) {
                (true, Phase::Enter) => "OnTriggerEnter2D",
                (true, Phase::Stay) => "OnTriggerStay2D",
                (true, Phase::Exit) => "OnTriggerExit2D",
                (false, Phase::Enter) => "OnCollisionEnter2D",
                (false, Phase::Stay) => "OnCollisionStay2D",
                (false, Phase::Exit) => "OnCollisionExit2D",
            };
            for (me, other) in [(c.a, c.b), (c.b, c.a)] {
                for i in 0..self.instances.len() {
                    if self.instances[i].entity != me || !self.runnable(&self.instances[i]) {
                        continue;
                    }
                    let arg = if c.trigger {
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
        let scene_in = std::mem::take(scene);
        self.with_world(|w| {
            w.scene = scene_in;
            w.time = *time;
            w.save.mark_time(time.time);
            // Axis smoothing runs on real time, so it works while the game is paused.
            w.axes.update(&input, time.unscaled_delta);
            w.input = input;
        });
        let input_events = {
            profile_scope!("Input.Update", Category::Scripts);
            self.input_update(time.unscaled_delta)
        };
        if let Some(w) = self.lua.app_data_ref::<World>()
            && let Err(e) = api::refresh_time(&self.lua, &w)
        {
            drop(w);
            self.log(LogKind::Error, format!("Time: {e}"));
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
            profile_scope!("Scripts.Start", Category::Scripts);
            for i in 0..self.instances.len() {
                if !self.instances[i].started && self.runnable(&self.instances[i]) {
                    self.instances[i].started = true;
                    self.call(i, "Start", None);
                    self.flush_spawned();
                }
            }
        }

        {
            // After Start (listeners added there hear this frame), before Update.
            profile_scope!("Input.Events", Category::Scripts);
            self.input_events(input_events);
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
            }
            profile_scope!("Physics.Step", Category::Physics);
            let contacts = self.with_world(|w| w.physics.step(&w.scene));
            self.dispatch(&contacts);
            self.flush_spawned();
            let contacts_2d = self.with_world(|w| {
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
        if steps > 0 {
            let dt = steps as f32 * self.with_world(|w| w.physics.fixed_dt);
            self.update_cameras(crate::vcam::BrainUpdate::FixedUpdate, dt);
        }

        {
            profile_scope!("Scripts.Update", Category::Scripts);
            self.call_all("Update");
        }
        {
            profile_scope!("Navigation.Update", Category::Navigation);
            self.with_world(|w| {
                w.navigation.update(&mut w.scene, time.delta);
                let logs = w.navigation.take_logs();
                w.logs
                    .extend(logs.into_iter().map(|l| (LogKind::Warning, l)));
            });
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
            profile_scope!("Scripts.LateUpdate", Category::Scripts);
            self.call_all("LateUpdate");
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
                cameras,
                ..
            } = w;
            let size = input.screen_size;
            let aspect = if size.x > 0.0 && size.y > 0.0 {
                size.x / size.y
            } else {
                16.0 / 9.0
            };
            let axis = |name: &str| axes.axis(name).unwrap_or(0.0);
            let cx = crate::vcam::SolveContext {
                dt,
                aspect,
                physics: Some(physics),
                axis: &axis,
            };
            cameras.update(scene, pass, &cx);
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
        let events = {
            let Some(mut w) = self.lua.app_data_mut::<World>() else {
                return;
            };
            let w = &mut *w;
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
            self.ui.update(&mut w.scene, &pointer)
        };
        for ev in events {
            self.ui_event(ev);
        }
    }

    /// Call `method` on every running script of `target` that defines it.
    pub fn send_message(&mut self, target: Entity, method: &str) {
        self.send_message_with(target, method, None, "SendMessage");
    }

    /// Like [`ScriptHost::send_message`] with an argument; `what` names the caller in warnings.
    fn send_message_with(&mut self, target: Entity, method: &str, arg: Option<Value>, what: &str) {
        let mut found = false;
        for i in 0..self.instances.len() {
            let inst = &self.instances[i];
            if inst.entity != target || !self.runnable(inst) {
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
            for r in roots {
                w.physics.add_subtree(&w.scene, *r);
                w.physics2d.add_subtree(&w.scene, *r);
                w.spawned.push(*r);
            }
        });
        self.flush_spawned();
        *scene = self.with_world(|w| std::mem::take(&mut w.scene));
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
        let queued = self.with_world(|w| std::mem::take(&mut w.destroy));
        for e in queued {
            let subtree: Vec<Entity> = self.with_world(|w| {
                if !w.scene.contains(e) {
                    return Vec::new();
                }
                w.scene
                    .walk()
                    .into_iter()
                    .map(|(x, _)| x)
                    .filter(|x| w.scene.is_ancestor(e, *x))
                    .collect()
            });
            // Spawned network objects are despawned first (OnNetworkDespawn, clients told).
            self.network_destroying(&subtree);
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
                all
            });
            self.instances.retain(|i| !doomed.contains(&i.entity));
            save_api::untrack(&self.lua, &doomed);
            unregister_script_instances(&self.lua, &doomed);
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

    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }
}

fn load_class(lua: &Lua, root: &std::path::Path, path: &str) -> Result<Table, String> {
    let source = crate::vfs::read_to_string(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
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
        ScriptValue::Vector3(a) => {
            Value::UserData(lua.create_userdata(LVec3(glam::Vec3::from(*a)))?)
        }
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
    let lua = Lua::new();
    api::register(&lua).map_err(|e| e.to_string())?;
    let class = eval_class(&lua, source, path)?;
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
            Value::UserData(ud) if ud.is::<LVec3>() => ScriptValue::Vector3(
                ud.borrow::<LVec3>()
                    .map_err(|e| e.to_string())?
                    .0
                    .to_array(),
            ),
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
    let lua = Lua::new();
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
        r#"-- {class_name}: attach to a GameObject. Plain values below are public fields shown in the Inspector.
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
mod input_tests;
#[cfg(test)]
mod material_tests;
#[cfg(test)]
mod network_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tilemap_tests;
