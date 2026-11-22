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

// ---- Round 81: bug hunt on the lifecycle ----------------------------------------------------

/// An object switched on by a script's Awake while Play starts wakes even when the start pass
/// already went by it (it comes first in the scene): it used to get no Awake, OnEnable or
/// Start at all.
#[test]
fn an_object_switched_on_by_an_earlier_awake_at_play_start_wakes() {
    let waker = r#"
local W = {}
function W:Awake() GameObject.Find("Panel"):SetActive(true) end
return W
"#;
    let root = project(&[("Waker", waker)]);
    let mut scene = Scene::new("t");
    let panel = scripted(&mut scene, "Panel", "Life", None);
    scene.game_object_mut(panel).active = false;
    scripted(&mut scene, "Waker", "Waker", None);
    let mut g = Game::start(scene, root.clone());
    assert_eq!(g.lines("Panel:"), names("Panel", &["Awake", "OnEnable"]));
    g.frames(1);
    assert_eq!(
        g.lines("Panel:"),
        names("Panel", &["Awake", "OnEnable", "Start"])
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Each OnEnable switching on the next object of a 40-long chain in one frame is not an
/// endless loop: every object wakes, with no Console error. The guard counted rounds (32),
/// and a link to an object the round had already gone by (earlier in the scene) takes one
/// round each, so the last links got no OnEnable or Start and the Console showed an error.
/// Objects down one hierarchy wake in the same round and pass either way.
#[test]
fn a_long_chain_of_onenable_switches_is_not_an_endless_loop() {
    let chain = r#"
local C = {}
function C:OnEnable()
    local n = tonumber(string.sub(self.gameObject.name, 2))
    Debug.Log("Chain:" .. n)
    local nxt = GameObject.Find("L" .. (n + 1))
    if nxt ~= nil then nxt:SetActive(true) end
    if self.transform.childCount > 0 then
        self.transform:GetChild(0).gameObject:SetActive(true)
    end
end
return C
"#;
    let ctl = controller(r#"    if f == 2 then GameObject.Find("L0"):SetActive(true) end"#);
    let root = project(&[("Chain", chain), ("Ctl", &ctl)]);
    let mut scene = Scene::new("t");
    scripted(&mut scene, "Ctl", "Ctl", None);
    // Later links come first in the scene: each link is switched on after the round passed it.
    for k in (0..40).rev() {
        let e = scripted(&mut scene, &format!("L{k}"), "Chain", None);
        scene.game_object_mut(e).active = false;
    }
    // And a 40-deep hierarchy under the last link.
    let mut parent = scene.find_by_name("L39");
    for k in 100..140 {
        let e = scripted(&mut scene, &format!("D{k}"), "Chain", parent);
        scene.game_object_mut(e).active = false;
        parent = Some(e);
    }
    let mut g = Game::start(scene, root.clone());
    g.frames(3);
    let woke = g.lines("Chain:");
    let want: Vec<String> = (0..40)
        .chain(100..140)
        .map(|k| format!("Chain:{k}"))
        .collect();
    assert_eq!(woke, want);
    let _ = std::fs::remove_dir_all(&root);
}

/// A firework: the rocket (Stop Action Destroy) bursts into a sub emitter when it dies. The
/// sub emitter's own Stop Action waits for its parent (it used to destroy itself when its own
/// short cycle ended, before the rocket died), and the rocket waits for the burst to finish
/// before it destroys itself and the sub emitter (it used to cut the burst off at once).
#[test]
fn stop_actions_wait_for_sub_emitters_and_their_parents() {
    use crate::particles::{SubEmitter, SubEmitterTrigger};
    let stopped = r#"
local S = {}
function S:OnParticleSystemStopped() Debug.Log(self.gameObject.name .. ":Stopped") end
return S
"#;
    let root = project(&[("S", stopped)]);
    let mut scene = Scene::new("t");
    // The rocket: one particle living 0.5 s; bursts into 8 sparks when it dies.
    let rocket = scene.spawn("Rocket", None);
    let mut s = one_shot(StopAction::Destroy);
    s.main.start_lifetime = crate::particles::MinMax::constant(0.5);
    s.emission.bursts[0].count = 1;
    s.sub_emitters.enabled = true;
    s.sub_emitters.list = vec![SubEmitter {
        trigger: SubEmitterTrigger::Death,
        emitter: "Sparks".into(),
        count: 8,
        ..Default::default()
    }];
    scene.add(rocket, s);
    // The sparks: a 0.1 s cycle of their own, living 0.3 s; its Stop Action calls back.
    let sparks = scripted(&mut scene, "Sparks", "S", Some(rocket));
    let mut sub = one_shot(StopAction::Callback);
    sub.main.duration = 0.1;
    sub.main.start_lifetime = crate::particles::MinMax::constant(0.3);
    sub.emission.bursts.clear();
    scene.add(sparks, sub);
    // 0.4 s: the rocket flies; the sparks' own cycle ended long ago.
    let (mut rt, mut logs) = run(&mut scene, root.clone(), 24);
    assert!(scene.contains(rocket) && scene.contains(sparks));
    assert!(logs.iter().all(|(_, t)| !t.contains("Stopped")), "{logs:?}");
    // 0.6 s: the rocket died and burst; the sparks fly while the rocket waits.
    let mut time = Time::default();
    time.advance(0.4);
    let mut step = |rt: &mut Runtime, scene: &mut Scene, n: u32, logs: &mut Vec<_>| {
        for _ in 0..n {
            time.advance(1.0 / 60.0);
            rt.update(scene, &time, Input::default());
            logs.extend(rt.take_logs());
        }
    };
    step(&mut rt, &mut scene, 12, &mut logs);
    let count = |sc: &Scene, e| sc.world.get::<&ParticleEmitter>(e).map_or(0, |m| m.count());
    assert!(scene.contains(rocket), "the rocket waits for its burst");
    assert_eq!(count(&scene, sparks), 8, "the burst is alive");
    // Long after: the sparks died, the sub emitter called back once, the rocket went.
    step(&mut rt, &mut scene, 60, &mut logs);
    assert!(!scene.contains(rocket) && !scene.contains(sparks));
    let calls: Vec<&String> = logs
        .iter()
        .inspect(|(k, t)| assert_ne!(*k, LogKind::Error, "{t}"))
        .map(|(_, t)| t)
        .filter(|t| t.contains("Stopped"))
        .collect();
    assert_eq!(calls, ["Sparks:Stopped"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// A script whose component starts disabled still wakes (Awake, not OnEnable or Start) when
/// its object is active, as in the familiar engine: it hears collisions, and its
/// OnCollisionEnter used to run on fields its Awake never set (a Console error).
#[test]
fn a_disabled_script_wakes_before_it_hears_collisions() {
    let counter = r#"
local H = {}
function H:Awake() self.hits = 0 Debug.Log("H:Awake") end
function H:OnEnable() Debug.Log("H:OnEnable") end
function H:Start() Debug.Log("H:Start") end
function H:OnCollisionEnter(o)
    self.hits = self.hits + 1
    Debug.Log("H:hit " .. self.hits)
end
function H:OnDestroy() Debug.Log("H:OnDestroy") end
return H
"#;
    let root = project(&[("H", counter)]);
    let mut scene = Scene::new("t");
    let floor = scene.spawn("Floor", None);
    scene.add(floor, BoxCollider::default());
    scene.add(
        floor,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/H.lua".into(),
                enabled: false,
                ..Default::default()
            }],
        },
    );
    let crate3 = scene.spawn("Crate", None);
    scene.set_transform(crate3, Transform::from_position(Vec3::new(0.0, 2.0, 0.0)));
    scene.add(crate3, BoxCollider::default());
    scene.add(crate3, Rigidbody::default());
    let mut g = Game::start(scene, root.clone());
    g.frames(90);
    g.host.shutdown(&mut g.scene);
    g.logs.extend(g.host.take_logs());
    assert_eq!(g.lines("H:"), ["H:Awake", "H:hit 1", "H:OnDestroy"]);
    let _ = std::fs::remove_dir_all(&root);
}

// ---- Round 83: `self.enabled` ----------------------------------------------------------------

/// `self.enabled` reads the script component's checkbox and sets it: `self.enabled = false`
/// calls OnDisable (after the callback, like SetActive) and stops Update; the Inspector
/// switching it back on calls OnEnable and `self.enabled` reads true again. Another script
/// reaches it through `GetComponent`.
#[test]
fn self_enabled_reads_and_sets_the_script_component() {
    let ticker = r#"
local T = { n = 0 }
function T:OnEnable() Debug.Log("T:OnEnable " .. tostring(self.enabled)) end
function T:OnDisable() Debug.Log("T:OnDisable " .. tostring(self.enabled)) end
function T:Update()
    self.n = self.n + 1
    Debug.Log("T:Update " .. tostring(self.enabled))
    if self.n == 2 then self.enabled = false end
end
return T
"#;
    let other = r#"
local O = { f = 0 }
function O:Update()
    self.f = self.f + 1
    local t = GameObject.Find("T"):GetComponent("T")
    if self.f == 3 then Debug.Log("O:sees " .. tostring(t.enabled)) end
    if self.f == 5 then t.enabled = true end
end
return O
"#;
    let root = project(&[("T", ticker), ("O", other)]);
    let mut scene = Scene::new("t");
    let t = scripted(&mut scene, "T", "T", None);
    scripted(&mut scene, "O", "O", None);
    let mut g = Game::start(scene, root.clone());
    g.frames(3);
    let enabled = |s: &Scene| s.world.get::<&Scripts>(t).unwrap().list[0].enabled;
    assert!(!enabled(&g.scene), "the component itself is disabled");
    g.scene.set_script_enabled(t, 0, true);
    g.frames(1);
    g.scene.set_script_enabled(t, 0, false);
    g.frames(2);
    assert!(enabled(&g.scene), "another script enabled it");
    assert_eq!(
        g.lines("T:"),
        [
            "T:OnEnable true",
            "T:Update true",
            "T:Update true",
            "T:OnDisable false",
            "T:OnEnable true",
            "T:Update true",
            "T:OnDisable false",
            "T:OnEnable true",
            "T:Update true",
        ]
    );
    assert_eq!(g.lines("O:"), ["O:sees false"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// A disabled script (which hears collisions, round 79) enables itself in OnCollisionEnter:
/// OnEnable follows, then Start and Update.
#[test]
fn a_disabled_script_enables_itself_in_on_collision_enter() {
    let waker = r#"
local H = {}
function H:Awake() Debug.Log("H:Awake " .. tostring(self.enabled)) end
function H:OnEnable() Debug.Log("H:OnEnable") end
function H:Start() Debug.Log("H:Start") end
function H:OnCollisionEnter(o)
    Debug.Log("H:hit " .. o.name)
    self.enabled = true
end
function H:Update()
    if not self.updated then self.updated = true Debug.Log("H:Update") end
end
return H
"#;
    let root = project(&[("H", waker)]);
    let mut scene = Scene::new("t");
    let floor = scene.spawn("Floor", None);
    scene.add(floor, BoxCollider::default());
    scene.add(
        floor,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/H.lua".into(),
                enabled: false,
                ..Default::default()
            }],
        },
    );
    let crate3 = scene.spawn("Crate", None);
    scene.set_transform(crate3, Transform::from_position(Vec3::new(0.0, 2.0, 0.0)));
    scene.add(crate3, BoxCollider::default());
    scene.add(crate3, Rigidbody::default());
    let mut g = Game::start(scene, root.clone());
    g.frames(90);
    assert_eq!(
        g.lines("H:"),
        [
            "H:Awake false",
            "H:hit Crate",
            "H:OnEnable",
            "H:Start",
            "H:Update"
        ]
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Fields and methods still work through the new `__index` (class methods, fields set from the
/// Inspector, new fields); a script with network RPCs keeps its network properties.
#[test]
fn self_enabled_keeps_fields_methods_and_network_properties() {
    let s = r#"
local S = { speed = 1 }
function S:Twice() return self.speed * 2 end
function S:PingServerRpc() end
function S:Start()
    self.extra = 5
    Debug.Log("S:" .. self:Twice() .. " " .. self.extra .. " " .. tostring(self.enabled)
        .. " " .. tostring(self.IsServer ~= nil))
end
return S
"#;
    let root = project(&[("S", s)]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("S", None);
    let mut r = ScriptRef {
        path: "Assets/Scripts/S.lua".into(),
        ..Default::default()
    };
    r.fields
        .insert("speed".into(), crate::scene::ScriptValue::Number(4.0));
    scene.add(e, Scripts { list: vec![r] });
    let mut g = Game::start(scene, root.clone());
    g.frames(1);
    assert_eq!(g.lines("S:"), ["S:8.0 5 true true"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// `ObjectField(kind)` declares an object slot: Properties lists it, a set one arrives as the
/// object (or its Transform), an unset one is nil.
#[test]
fn lua_object_fields_arrive_as_objects_or_nil() {
    let s = r#"
local S = { target = ObjectField("Transform"), thing = ObjectField(), unset = ObjectField("Rigidbody") }
function S:Start()
    Debug.Log("S:" .. self.thing.name .. " " .. self.target.position.x .. " " .. tostring(self.unset == nil))
end
return S
"#;
    let info = crate::scripting::inspect_script(s, "Assets/Scripts/S.lua").unwrap();
    let kinds: Vec<String> = info
        .fields
        .iter()
        .map(|(n, v)| match v {
            crate::scene::ScriptValue::Object(o) => format!("{n}:{}", o.kind),
            other => format!("{n}:{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        ["target:Transform", "thing:GameObject", "unset:Rigidbody"]
    );
    let root = project(&[("S", s)]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("S", None);
    let other = scene.spawn("Other", None);
    let mut t = scene.transform(other);
    t.position = Vec3::new(3.0, 0.0, 0.0);
    scene.set_transform(other, t);
    let obj = |kind: &str| {
        crate::scene::ScriptValue::Object(crate::scene::ObjectValue {
            object: Some(other.to_bits().get()),
            kind: kind.into(),
        })
    };
    let mut r = ScriptRef {
        path: "Assets/Scripts/S.lua".into(),
        ..Default::default()
    };
    r.fields.insert("target".into(), obj("Transform"));
    r.fields.insert("thing".into(), obj("GameObject"));
    scene.add(e, Scripts { list: vec![r] });
    let mut g = Game::start(scene, root.clone());
    g.frames(1);
    assert_eq!(g.lines("S:"), ["S:Other 3.0 true"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// `DontDestroyOnLoad` keeps an object (from its child too) and its script state through a
/// single scene load: the other objects unload, the new scene's objects wake up and its scene
/// becomes the active one.
#[test]
fn dont_destroy_on_load_keeps_objects_and_their_state_through_a_scene_load() {
    let keeper = r#"
local K = { n = 0 }
function K:Awake()
    Debug.Log("Keeper:Awake")
    DontDestroyOnLoad(self.transform:GetChild(0).gameObject)
end
function K:Update()
    self.n = self.n + 1
    if self.n == 12 then Debug.Log("Keeper:n=12") end
end
function K:OnDestroy() Debug.Log("Keeper:OnDestroy") end
return K
"#;
    let root = project(&[("Keeper", keeper)]);
    let mut next = Scene::new("Next");
    scripted(&mut next, "New", "Life", None);
    let rel = "Assets/Scenes/Next.scene";
    next.save(&root.join(rel)).unwrap();
    let text = std::fs::read_to_string(root.join(rel)).unwrap();

    let mut scene = Scene::new("First");
    let k = scripted(&mut scene, "Keeper", "Keeper", None);
    scene.spawn("Child", Some(k));
    scripted(&mut scene, "Old", "Life", None);
    let (audio, _log) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root.clone(), &Default::default(), audio);
    let mut time = Time::default();
    for _ in 0..10 {
        time.advance(1.0 / 60.0);
        rt.update(&mut scene, &time, Input::default());
    }
    assert!(rt.has_persistent_objects(&scene));
    rt.take_logs();
    rt.load_scene_single(&mut scene, rel, &text).unwrap();
    for _ in 0..2 {
        time.advance(1.0 / 60.0);
        rt.update(&mut scene, &time, Input::default());
    }
    let lines: Vec<String> = rt
        .take_logs()
        .into_iter()
        .inspect(|(kind, t)| assert_ne!(*kind, LogKind::Error, "{t}"))
        .map(|(_, t)| t)
        .collect();
    assert_eq!(
        lines,
        [
            "Old:OnDisable",
            "Old:OnDestroy",
            "New:Awake",
            "New:OnEnable",
            "New:Start",
            "Keeper:n=12",
        ],
        "{lines:?}"
    );
    assert!(scene.find_by_name("Old").is_none());
    assert!(scene.find_by_name("Child").is_some());
    assert_eq!(scene.scene_name(scene.active_scene()), "Next");
    let _ = std::fs::remove_dir_all(&root);
}
