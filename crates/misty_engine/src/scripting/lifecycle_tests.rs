//! OnEnable / OnDisable / OnDestroy for Lua scripts, contacts reaching disabled scripts,
//! Particle System Stop Actions, and SetActive off and on within one frame (round 79).

use std::path::PathBuf;

use glam::Vec3;
use hecs::Entity;

use crate::audio::Audio;
use crate::particles::{ParticleEmitter, ParticleSystem, StopAction};
use crate::physics::Physics;
use crate::runtime::{Runtime, Time};
use crate::scene::{
    AudioSource, BoxCollider, BoxCollider2D, Rigidbody, Rigidbody2D, Scene, ScriptRef, Scripts,
    Transform,
};
use crate::scripting::{Input, LogKind, ScriptHost};
use crate::settings::ProjectSettings;

/// Logs every lifecycle callback as "Name:Callback".
const LIFE: &str = r#"
local Life = {}
local function log(self, what) Debug.Log(self.gameObject.name .. ":" .. what) end
function Life:Awake() log(self, "Awake") end
function Life:OnEnable() log(self, "OnEnable") end
function Life:Start() log(self, "Start") end
function Life:OnDisable() log(self, "OnDisable") end
function Life:OnDestroy() log(self, "OnDestroy") end
return Life
"#;

fn project(scripts: &[(&str, &str)]) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_lifecycle_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts/Life.lua"), LIFE).unwrap();
    for (name, source) in scripts {
        std::fs::write(root.join(format!("Assets/Scripts/{name}.lua")), source).unwrap();
    }
    root
}

fn scripted(scene: &mut Scene, name: &str, script: &str, parent: Option<Entity>) -> Entity {
    let e = scene.spawn(name, parent);
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: format!("Assets/Scripts/{script}.lua"),
                ..Default::default()
            }],
        },
    );
    e
}

/// A controller whose Update runs `body` with `f` = the frame number (1 = first Update).
fn controller(body: &str) -> String {
    format!(
        "local C = {{ f = 0 }}\nfunction C:Update()\n    self.f = self.f + 1\n    local f = self.f\n{body}\nend\nreturn C\n"
    )
}

struct Game {
    scene: Scene,
    host: ScriptHost,
    time: Time,
    logs: Vec<(LogKind, String)>,
}

impl Game {
    fn start(mut scene: Scene, root: PathBuf) -> Self {
        let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
        let host = ScriptHost::start(&mut scene, physics, root);
        let logs = host.take_logs();
        Self {
            scene,
            host,
            time: Time::default(),
            logs,
        }
    }

    fn frames(&mut self, n: u32) {
        for _ in 0..n {
            self.time.advance(1.0 / 60.0);
            self.host
                .update(&mut self.scene, &self.time, Input::default());
            self.logs.extend(self.host.take_logs());
        }
    }

    /// Info lines starting with `prefix`, in order (errors fail the test).
    fn lines(&self, prefix: &str) -> Vec<String> {
        for (k, t) in &self.logs {
            assert_ne!(*k, LogKind::Error, "{t}");
        }
        self.logs
            .iter()
            .filter(|(k, t)| *k == LogKind::Info && t.starts_with(prefix))
            .map(|(_, t)| t.clone())
            .collect()
    }
}

fn names(prefix: &str, list: &[&str]) -> Vec<String> {
    list.iter().map(|c| format!("{prefix}:{c}")).collect()
}

/// Awake, OnEnable, Start, then OnDisable and OnDestroy when the object is destroyed.
#[test]
fn callbacks_run_awake_onenable_start_then_ondisable_ondestroy() {
    let ctl = controller("    if f == 3 then Destroy(GameObject.Find(\"A\")) end");
    let root = project(&[("Ctl", &ctl)]);
    let mut scene = Scene::new("t");
    scripted(&mut scene, "A", "Life", None);
    scripted(&mut scene, "Ctl", "Ctl", None);
    let mut g = Game::start(scene, root.clone());
    assert_eq!(g.lines("A:"), names("A", &["Awake", "OnEnable"]));
    g.frames(5);
    assert_eq!(
        g.lines("A:"),
        names(
            "A",
            &["Awake", "OnEnable", "Start", "OnDisable", "OnDestroy"]
        )
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Switching a parent off reaches its children's scripts; switching an object off twice, or
/// off while its parent is already off, calls nothing more; switching the parent back on
/// leaves a child that is off itself alone. OnEnable and OnDisable always alternate.
#[test]
fn parent_switch_reaches_children_and_nothing_runs_twice() {
    let ctl = controller(
        r#"
    local p = GameObject.Find("P")
    local c = p.transform:Find("C").gameObject
    if f == 2 then p:SetActive(false) end
    if f == 3 then p:SetActive(false); c:SetActive(false) end
    if f == 4 then p:SetActive(true) end
    if f == 5 then c:SetActive(true) end
    if f == 6 then c:SetActive(true) end"#,
    );
    let root = project(&[("Ctl", &ctl)]);
    let mut scene = Scene::new("t");
    let p = scene.spawn("P", None);
    scripted(&mut scene, "C", "Life", Some(p));
    scripted(&mut scene, "Ctl", "Ctl", None);
    let mut g = Game::start(scene, root.clone());
    g.frames(8);
    assert_eq!(
        g.lines("C:"),
        names(
            "C",
            &["Awake", "OnEnable", "Start", "OnDisable", "OnEnable"]
        )
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// An object that starts inactive wakes when switched on: Awake, OnEnable, then Start on the
/// next frame (it never woke before round 79).
#[test]
fn an_object_switched_on_later_wakes_then_starts() {
    let ctl = controller(
        r#"
    if f == 2 then
        GameObject.Find("Holder").transform:Find("D").gameObject:SetActive(true)
        Debug.Log("D:switched on")
    end"#,
    );
    let root = project(&[("Ctl", &ctl)]);
    let mut scene = Scene::new("t");
    let holder = scene.spawn("Holder", None);
    let d = scripted(&mut scene, "D", "Life", Some(holder));
    scene.game_object_mut(d).active = false;
    scripted(&mut scene, "Ctl", "Ctl", None);
    let mut g = Game::start(scene, root.clone());
    g.frames(1);
    assert!(g.lines("D:").is_empty(), "inactive: nothing yet");
    g.frames(3);
    assert_eq!(
        g.lines("D:"),
        names("D", &["switched on", "Awake", "OnEnable", "Start"])
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// `SetActive(false)` then `SetActive(true)` in one callback is a real switch: OnDisable then
/// OnEnable (the familiar engine calls them inside SetActive).
#[test]
fn set_active_off_and_on_in_one_callback_disables_and_enables() {
    let ctl = controller(
        r#"
    if f == 2 then
        local x = GameObject.Find("X")
        x:SetActive(false)
        x:SetActive(true)
    end"#,
    );
    let root = project(&[("Ctl", &ctl)]);
    let mut scene = Scene::new("t");
    scripted(&mut scene, "X", "Life", None);
    scripted(&mut scene, "Ctl", "Ctl", None);
    let mut g = Game::start(scene, root.clone());
    g.frames(4);
    assert_eq!(
        g.lines("X:"),
        names(
            "X",
            &["Awake", "OnEnable", "Start", "OnDisable", "OnEnable"]
        )
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Stop (and a scene unload) disables every enabled script and destroys every woken one;
/// scripts already disabled get only OnDestroy, scripts that never woke get nothing.
#[test]
fn stop_disables_then_destroys() {
    let root = project(&[]);
    let mut scene = Scene::new("t");
    scripted(&mut scene, "On", "Life", None);
    let off = scripted(&mut scene, "Off", "Life", None);
    let never = scripted(&mut scene, "Never", "Life", None);
    scene.game_object_mut(never).active = false;
    let mut g = Game::start(scene, root.clone());
    g.frames(1);
    g.scene.set_active(off, false);
    g.frames(1);
    g.host.shutdown(&mut g.scene);
    g.logs.extend(g.host.take_logs());
    assert_eq!(
        g.lines("On:"),
        names(
            "On",
            &["Awake", "OnEnable", "Start", "OnDisable", "OnDestroy"]
        )
    );
    assert_eq!(
        g.lines("Off:"),
        names(
            "Off",
            &["Awake", "OnEnable", "Start", "OnDisable", "OnDestroy"]
        )
    );
    assert!(g.lines("Never:").is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// Disabling and enabling the script component (the Inspector's checkbox) calls OnDisable and
/// OnEnable; a disabled script gets no Update.
#[test]
fn the_script_component_switched_calls_ondisable_and_onenable() {
    let ticker = r#"
local T = {}
function T:OnEnable() Debug.Log("T:OnEnable") end
function T:OnDisable() Debug.Log("T:OnDisable") end
function T:Update() Debug.Log("T:Update") end
return T
"#;
    let root = project(&[("T", ticker)]);
    let mut scene = Scene::new("t");
    let t = scripted(&mut scene, "T", "T", None);
    let mut g = Game::start(scene, root.clone());
    g.frames(1);
    g.scene.set_script_enabled(t, 0, false);
    g.frames(2);
    g.scene.set_script_enabled(t, 0, true);
    g.frames(1);
    assert_eq!(
        g.lines("T:"),
        names(
            "T",
            &["OnEnable", "Update", "OnDisable", "OnEnable", "Update"]
        )
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// SetActive inside OnEnable / OnDisable: switching another object reaches it; an object that
/// switches itself off in OnEnable gets its OnDisable; two scripts switching each other back
/// for ever stop with an error instead of hanging or overflowing the stack.
#[test]
fn set_active_inside_onenable_and_ondisable() {
    let chain = r#"
local K = {}
function K:OnDisable()
    Debug.Log("K:OnDisable")
    GameObject.Find("L"):SetActive(false)
end
return K
"#;
    let shy = r#"
local S = { n = 0 }
function S:OnEnable()
    self.n = self.n + 1
    Debug.Log("S:OnEnable")
    if self.n == 2 then self.gameObject:SetActive(false) end
end
function S:OnDisable() Debug.Log("S:OnDisable") end
return S
"#;
    let pong = r#"
local P = {}
function P:OnEnable() self.gameObject:SetActive(false) end
function P:OnDisable() self.gameObject:SetActive(true) end
return P
"#;
    let ctl = controller(
        r#"
    if f == 2 then
        GameObject.Find("K"):SetActive(false)
        local s = GameObject.Find("S")
        s:SetActive(false)
        s:SetActive(true)
    end"#,
    );
    let root = project(&[("K", chain), ("S", shy), ("Pong", pong), ("Ctl", &ctl)]);
    let mut scene = Scene::new("t");
    scripted(&mut scene, "K", "K", None);
    scripted(&mut scene, "L", "Life", None);
    scripted(&mut scene, "S", "S", None);
    scripted(&mut scene, "Ctl", "Ctl", None);
    let mut g = Game::start(scene, root.clone());
    g.frames(3);
    assert_eq!(g.lines("K:"), ["K:OnDisable"]);
    assert_eq!(
        g.lines("L:"),
        names("L", &["Awake", "OnEnable", "Start", "OnDisable"])
    );
    assert_eq!(
        g.lines("S:"),
        names("S", &["OnEnable", "OnDisable", "OnEnable", "OnDisable"])
    );
    let s = g.scene.find_by_name("S").unwrap();
    assert!(!g.scene.game_object(s).active);

    // The endless pair: an error, then the game goes on.
    let pong = scripted(&mut g.scene, "Pong", "Pong", None);
    g.scene.game_object_mut(pong).active = false;
    g.host.add_objects(&mut g.scene, &[pong]);
    g.scene.set_active(pong, true);
    g.frames(2);
    let errors: Vec<&String> = g
        .logs
        .iter()
        .filter(|(k, _)| *k == LogKind::Error)
        .map(|(_, t)| t)
        .collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("kept switching"), "{errors:?}");
    let _ = std::fs::remove_dir_all(&root);
}

const HEAR: &str = r#"
local H = {}
function H:Update() Debug.Log("H:Update") end
function H:OnCollisionEnter(o) Debug.Log("H:OnCollisionEnter " .. o.name) end
function H:OnCollisionEnter2D(c) Debug.Log("H:OnCollisionEnter2D " .. c.gameObject.name) end
return H
"#;

/// A disabled script still hears collisions (3D and 2D), as in the familiar engine, but gets no
/// Update; a script on a switched-off object hears nothing.
#[test]
fn disabled_scripts_get_collision_events_in_3d_and_2d() {
    let root = project(&[("H", HEAR)]);
    let mut scene = Scene::new("t");
    let floor = scene.spawn("Floor", None);
    scene.add(floor, BoxCollider::default());
    let floor2 = scene.spawn("Floor2D", None);
    scene.set_transform(floor2, Transform::from_position(Vec3::new(20.0, 0.0, 0.0)));
    scene.add(floor2, BoxCollider2D::default());
    for (f, name) in [(floor, "Floor"), (floor2, "Floor2D")] {
        let _ = name;
        scene.add(
            f,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Scripts/H.lua".into(),
                    enabled: false,
                    ..Default::default()
                }],
            },
        );
    }
    let crate3 = scene.spawn("Crate", None);
    scene.set_transform(crate3, Transform::from_position(Vec3::new(0.0, 2.0, 0.0)));
    scene.add(crate3, BoxCollider::default());
    scene.add(crate3, Rigidbody::default());
    let crate2 = scene.spawn("Crate2D", None);
    scene.set_transform(crate2, Transform::from_position(Vec3::new(20.0, 2.0, 0.0)));
    scene.add(crate2, BoxCollider2D::default());
    scene.add(crate2, Rigidbody2D::default());
    let mut g = Game::start(scene, root.clone());
    g.frames(90);
    assert_eq!(
        g.lines("H:"),
        ["H:OnCollisionEnter Crate", "H:OnCollisionEnter2D Crate2D"],
        "contacts reach the disabled scripts; Update does not"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A script on a switched-off object hears no contacts (only disabled components do).
#[test]
fn switched_off_objects_get_no_collision_events() {
    let root = project(&[("H", HEAR)]);
    let mut scene = Scene::new("t");
    let holder = scene.spawn("Holder", None);
    let floor = scripted(&mut scene, "Floor", "H", Some(holder));
    scene.add(floor, BoxCollider::default());
    let crate3 = scene.spawn("Crate", None);
    scene.set_transform(crate3, Transform::from_position(Vec3::new(0.0, 2.0, 0.0)));
    scene.add(crate3, BoxCollider::default());
    scene.add(crate3, Rigidbody::default());
    let mut g = Game::start(scene, root.clone());
    g.frames(1);
    g.scene.set_active(holder, false);
    g.frames(90);
    assert_eq!(g.lines("H:"), ["H:Update"]);
    let _ = std::fs::remove_dir_all(&root);
}

// ---- Particle System Stop Action and the same-frame switch (through the Runtime) ----------

/// A one-shot: 5 particles at once living 0.2 s, a 0.3 s cycle.
fn one_shot(action: StopAction) -> ParticleSystem {
    let mut s = ParticleSystem::default();
    s.main.looping = false;
    s.main.duration = 0.3;
    s.main.start_lifetime = crate::particles::MinMax::constant(0.2);
    s.main.stop_action = action;
    s.emission.rate_over_time = crate::particles::MinMax::constant(0.0);
    s.emission.bursts = vec![crate::particles::Burst {
        count: 5,
        ..Default::default()
    }];
    s
}

fn run(scene: &mut Scene, root: PathBuf, frames: u32) -> (Runtime, Vec<(LogKind, String)>) {
    let (audio, _) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(scene, root, &ProjectSettings::default(), audio);
    let mut time = Time::default();
    let mut logs = Vec::new();
    for _ in 0..frames {
        time.advance(1.0 / 60.0);
        rt.update(scene, &time, Input::default());
        logs.extend(rt.take_logs());
    }
    (rt, logs)
}

/// Each Stop Action when a one-shot ends and its last particle dies: None leaves the object,
/// Disable switches it off (its scripts get OnDisable), Destroy removes it (OnDisable,
/// OnDestroy), Callback calls OnParticleSystemStopped once. Looping systems never stop.
#[test]
fn stop_actions_run_when_a_one_shot_finishes() {
    let stopped = r#"
local S = {}
function S:OnParticleSystemStopped() Debug.Log(self.gameObject.name .. ":Stopped") end
function S:OnDisable() Debug.Log(self.gameObject.name .. ":OnDisable") end
function S:OnDestroy() Debug.Log(self.gameObject.name .. ":OnDestroy") end
return S
"#;
    let root = project(&[("S", stopped)]);
    let mut scene = Scene::new("t");
    let mut objects = Vec::new();
    for (name, action) in [
        ("None", StopAction::None),
        ("Disable", StopAction::Disable),
        ("Destroy", StopAction::Destroy),
        ("Callback", StopAction::Callback),
    ] {
        let e = scripted(&mut scene, name, "S", None);
        scene.add(e, one_shot(action));
        objects.push(e);
    }
    let looping = scripted(&mut scene, "Looping", "S", None);
    let mut s = one_shot(StopAction::Destroy);
    s.main.looping = true;
    scene.add(looping, s);
    // Half-way the burst is alive: nothing has happened yet.
    let (mut rt, logs) = run(&mut scene, root.clone(), 6);
    let count = |sc: &Scene, e| sc.world.get::<&ParticleEmitter>(e).map_or(0, |m| m.count());
    assert_eq!(count(&scene, objects[0]), 5);
    assert!(logs.iter().all(|(_, t)| !t.contains(':')), "{logs:?}");
    let mut time = Time::default();
    let mut all = logs;
    for _ in 0..60 {
        time.advance(1.0 / 60.0);
        rt.update(&mut scene, &time, Input::default());
        all.extend(rt.take_logs());
    }
    let [none, disable, destroy, callback] = objects[..] else {
        unreachable!()
    };
    assert!(scene.contains(none) && scene.game_object(none).active);
    assert!(scene.contains(disable) && !scene.game_object(disable).active);
    assert!(!scene.contains(destroy));
    assert!(scene.contains(callback) && scene.game_object(callback).active);
    assert!(scene.contains(looping) && scene.game_object(looping).active);
    let mut text: Vec<&str> = all
        .iter()
        .inspect(|(k, t)| assert_ne!(*k, LogKind::Error, "{t}"))
        .map(|(_, t)| t.as_str())
        .collect();
    // Destroy goes at the end of the stop actions, like `Destroy` at the end of a frame.
    text.sort_by_key(|t| t.split(':').next().map(str::to_owned));
    assert_eq!(
        text,
        [
            "Callback:Stopped",
            "Destroy:OnDisable",
            "Destroy:OnDestroy",
            "Disable:OnDisable",
        ]
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Old scene and prefab files (no Stop Action) load as None; a set one survives a save.
#[test]
fn stop_action_saves_and_old_files_load_as_none() {
    let mut scene = Scene::new("t");
    let old = scene.spawn("Old", None);
    scene.add(old, ParticleSystem::default());
    let set = scene.spawn("Set", None);
    scene.add(set, one_shot(StopAction::Callback));
    let text = scene.to_json();
    assert!(
        !text.contains("Old\"") || text.matches("stop_action").count() == 1,
        "None is not written: {text}"
    );
    assert_eq!(text.matches("stop_action").count(), 1);
    let (back, _) = Scene::from_json("t", &text).unwrap();
    let action = |name: &str| {
        let e = back.find_by_name(name).unwrap();
        back.world
            .get::<&ParticleSystem>(e)
            .unwrap()
            .main
            .stop_action
    };
    assert_eq!(action("Old"), StopAction::None);
    assert_eq!(action("Set"), StopAction::Callback);
    // A main module written before round 79.
    let main: crate::particles::MainModule =
        serde_json::from_str(r#"{"duration": 2.0, "looping": false}"#).unwrap();
    assert_eq!(main.stop_action, StopAction::None);
    assert!(!main.looping);
}

/// SetActive(false) then SetActive(true) within one frame replays Play On Awake audio,
/// restarts particle systems and Visual Effects, like a switch across two frames does.
#[test]
fn set_active_off_and_on_in_one_frame_replays_play_on_awake() {
    let ctl = controller(
        r#"
    if f == 30 then
        local x = GameObject.Find("Pool")
        x:SetActive(false)
        x:SetActive(true)
    end"#,
    );
    let root = project(&[("Ctl", &ctl)]);
    let mut scene = Scene::new("t");
    let pool = scene.spawn("Pool", None);
    let fx = scene.spawn("Sparks", Some(pool));
    let mut s = ParticleSystem::default();
    s.main.looping = true;
    scene.add(fx, s);
    scene.add(
        pool,
        AudioSource {
            clip: Some("Assets/beep.wav".into()),
            play_on_awake: true,
            ..Default::default()
        },
    );
    scripted(&mut scene, "Ctl", "Ctl", None);
    let (audio, log) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root.clone(), &Default::default(), audio);
    let mut time = Time::default();
    let played = |log: &std::sync::Arc<std::sync::Mutex<crate::audio::NullLog>>| {
        log.lock().unwrap().played.len()
    };
    let playback = |sc: &Scene| sc.world.get::<&ParticleEmitter>(fx).unwrap().total_time;
    for _ in 0..29 {
        time.advance(1.0 / 60.0);
        rt.update(&mut scene, &time, Input::default());
    }
    assert_eq!(played(&log), 1, "played once at start");
    assert!(playback(&scene) > 0.4);
    time.advance(1.0 / 60.0);
    rt.update(&mut scene, &time, Input::default());
    assert_eq!(played(&log), 2, "the switch plays it again");
    assert!(
        playback(&scene) < 0.05,
        "the system starts over: {}",
        playback(&scene)
    );
    for _ in 0..10 {
        time.advance(1.0 / 60.0);
        rt.update(&mut scene, &time, Input::default());
    }
    assert_eq!(played(&log), 2, "nothing more while nothing switches");
    for (k, t) in rt.take_logs() {
        assert_ne!(k, LogKind::Error, "{t}");
    }
    let _ = std::fs::remove_dir_all(&root);
}
