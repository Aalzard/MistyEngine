//! Lua scripting with a MonoBehaviour-style lifecycle.
//!
//! A script file returns a class table. Plain values in it (numbers, booleans, strings, Vector3)
//! are public fields shown in the Inspector; functions are callbacks:
//! `Awake`, `Start`, `Update`, `FixedUpdate`, `LateUpdate`,
//! `OnCollisionEnter/Exit(other)`, `OnTriggerEnter/Exit(other)`.
//!
//! ```lua
//! local Rotator = { speed = 90 }
//! function Rotator:Update()
//!     self.transform:Rotate(0, self.speed * Time.deltaTime, 0)
//! end
//! return Rotator
//! ```

pub mod api;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use glam::Vec2;
use hecs::Entity;
use mlua::{Function, Lua, RegistryKey, Table, Value};

use crate::physics::{Contact, Physics};
use crate::runtime::Time;
use crate::scene::{Scene, ScriptValue, Scripts};
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
}

impl Input {
    pub fn key(&self, k: &str) -> bool {
        self.held.contains(&k.to_lowercase())
    }
    pub fn key_down(&self, k: &str) -> bool {
        self.pressed.contains(&k.to_lowercase())
    }
    pub fn key_up(&self, k: &str) -> bool {
        self.released.contains(&k.to_lowercase())
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

    /// Default axes: Horizontal (A/D, ←/→), Vertical (W/S, ↑/↓), Mouse X / Mouse Y.
    pub fn axis(&self, name: &str) -> f32 {
        let pair = |pos: [&str; 2], neg: [&str; 2]| {
            let p = pos.iter().any(|k| self.key(k)) as i32 as f32;
            let n = neg.iter().any(|k| self.key(k)) as i32 as f32;
            p - n
        };
        match name {
            "Horizontal" => pair(["d", "right"], ["a", "left"]),
            "Vertical" => pair(["w", "up"], ["s", "down"]),
            "Mouse X" => self.mouse_delta.x * 0.1,
            "Mouse Y" => -self.mouse_delta.y * 0.1,
            _ => 0.0,
        }
    }
}

/// What scripts can reach during a frame (stored in the Lua state's app data).
pub struct World {
    pub scene: Scene,
    pub physics: Physics,
    pub time: Time,
    pub input: Input,
    pub logs: Vec<(LogKind, String)>,
    pub destroy: Vec<Entity>,
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
}

impl ScriptHost {
    /// Create script instances for every script in the scene and run `Awake`.
    /// `scene` is moved into the host's world for the duration of the call.
    pub fn start(scene: &mut Scene, physics: Physics, project_root: PathBuf) -> Self {
        let lua = Lua::new();
        let mut logs = Vec::new();
        if let Err(e) = api::register(&lua) {
            logs.push((LogKind::Error, format!("Script API failed to load: {e}")));
        }
        lua.set_app_data(World {
            scene: std::mem::take(scene),
            physics,
            time: Time::default(),
            input: Input::default(),
            logs,
            destroy: Vec::new(),
        });
        let mut host = Self {
            lua,
            root: project_root,
            classes: HashMap::new(),
            instances: Vec::new(),
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
        *scene = host.with_world(|w| std::mem::take(&mut w.scene));
        host
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
        let result = match arg {
            Some(a) => f.call::<()>((t, a)),
            None => f.call::<()>(t),
        };
        if let Err(e) = result {
            self.log(LogKind::Error, format!("{} ({name}): {e}", inst.name));
        }
    }

    fn call_all(&mut self, name: &str) {
        for i in 0..self.instances.len() {
            if self.runnable(&self.instances[i]) {
                self.call(i, name, None);
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

    /// One game frame: Start (first frame), FixedUpdate + physics steps, Update, LateUpdate, Destroy.
    pub fn update(&mut self, scene: &mut Scene, time: &Time, input: Input) {
        let scene_in = std::mem::take(scene);
        self.with_world(|w| {
            w.scene = scene_in;
            w.time = *time;
            w.input = input;
        });
        if let Some(w) = self.lua.app_data_ref::<World>()
            && let Err(e) = api::refresh_time(&self.lua, &w)
        {
            drop(w);
            self.log(LogKind::Error, format!("Time: {e}"));
        }

        for i in 0..self.instances.len() {
            if !self.instances[i].started && self.runnable(&self.instances[i]) {
                self.instances[i].started = true;
                self.call(i, "Start", None);
            }
        }

        let steps = self.with_world(|w| w.physics.steps_due(time.delta));
        for _ in 0..steps {
            self.call_all("FixedUpdate");
            let contacts = self.with_world(|w| w.physics.step(&w.scene));
            self.dispatch(&contacts);
        }
        self.with_world(|w| w.physics.sync_out(&mut w.scene));

        self.call_all("Update");
        self.call_all("LateUpdate");
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

    /// Physics counters for the Stats overlay.
    pub fn physics_counts(&self) -> (usize, usize) {
        self.with_world(|w| (w.physics.body_count(), w.physics.collider_count()))
    }

    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }
}

fn load_class(lua: &Lua, root: &std::path::Path, path: &str) -> Result<Table, String> {
    let source = std::fs::read_to_string(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
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
mod tests;
