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
pub mod audio_api;
pub mod camera_api;
pub mod input_api;
pub mod light_probes_api;
pub mod material_api;
pub mod navigation_api;
mod override_api;
pub mod particles_api;
pub mod physics2d_api;
pub mod physics_api;
pub mod render_settings_api;
pub mod rigging_api;
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
        }
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
    /// Project tags and layer names (`LayerMask.NameToLayer`, `FindWithTag`).
    pub tags_layers: crate::tags_layers::TagsAndLayers,
    /// Camera Brains and Virtual Cameras.
    pub cameras: crate::vcam::CameraRig,
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
        World {
            navigation,
            scene,
            physics,
            physics2d,
            time: Time::default(),
            input: Input::default(),
            logs: Vec::new(),
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
            tags_layers: settings.tags_and_layers.clone(),
            cameras: Default::default(),
        }
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
        world.logs = logs;
        lua.set_app_data(world);
        let mut host = Self {
            lua,
            root: project_root,
            classes: HashMap::new(),
            instances: Vec::new(),
            ui: UiInteraction::default(),
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
        host.call_all("Awake");
        host.flush_spawned();
        host.with_world(|w| {
            w.audio.play_on_awake(&w.scene, None);
            w.animation.play_automatically(&w.scene, None);
            w.timeline.play_on_awake(&w.scene, None);
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
        match build().and_then(|t| self.lua.create_registry_value(t)) {
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
        });
    }

    /// Advance animations (after Update, before LateUpdate) and call their events' functions on
    /// the animated object's scripts.
    fn animate(&mut self, dt: f32) {
        let events = self.with_world(|w| {
            let World {
                animation,
                animator,
                scene,
                ..
            } = w;
            // An Animator on the same object runs last, so it wins for shared properties.
            let mut events = animation.update(scene, dt);
            events.extend(animator.update(scene, dt));
            events
        });
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
            // Axis smoothing runs on real time, so it works while the game is paused.
            w.axes.update(&input, time.unscaled_delta);
            w.input = input;
        });
        if let Some(w) = self.lua.app_data_ref::<World>()
            && let Err(e) = api::refresh_time(&self.lua, &w)
        {
            drop(w);
            self.log(LogKind::Error, format!("Time: {e}"));
        }

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
            let pointer = w.input.ui_pointer(w.time.unscaled_delta);
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
            let doomed: Vec<Entity> = self.with_world(|w| {
                if !w.scene.contains(e) {
                    return Vec::new();
                }
                let all: Vec<Entity> = w
                    .scene
                    .walk()
                    .into_iter()
                    .map(|(x, _)| x)
                    .filter(|x| w.scene.is_ancestor(e, *x))
                    .collect();
                for x in &all {
                    w.physics.remove(*x);
                    w.physics2d.remove(*x);
                }
                w.scene.despawn(e);
                all
            });
            self.instances.retain(|i| !doomed.contains(&i.entity));
        }
    }

    pub fn take_logs(&self) -> Vec<(LogKind, String)> {
        self.with_world(|w| std::mem::take(&mut w.logs))
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
    let lua = Lua::new();
    api::register(&lua).map_err(|e| e.to_string())?;
    let class = eval_class(&lua, source, path)?;
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
    Ok(fields)
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
mod tests;
#[cfg(test)]
mod tilemap_tests;
