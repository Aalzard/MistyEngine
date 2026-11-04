//! Script lifecycle and API tests: each writes Lua files into a temp project and runs frames.

use std::path::PathBuf;

use glam::Vec3;

use super::*;
use crate::physics::Physics;
use crate::scene::{BoxCollider, MeshFilter, PrimitiveMesh, Rigidbody, ScriptRef, Transform};

fn project(files: &[(&str, &str)]) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_script_test_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    for (name, src) in files {
        std::fs::write(root.join("Assets/Scripts").join(name), src).unwrap();
    }
    root
}

fn attach(scene: &mut Scene, e: Entity, file: &str, fields: &[(&str, ScriptValue)]) {
    let mut list = scene
        .world
        .get::<&Scripts>(e)
        .map(|s| s.list.clone())
        .unwrap_or_default();
    list.push(ScriptRef {
        path: format!("Assets/Scripts/{file}"),
        enabled: true,
        fields: fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    });
    scene.add(e, Scripts { list });
}

fn run(scene: &mut Scene, root: PathBuf, frames: u32, input: Input) -> Vec<(LogKind, String)> {
    let physics = Physics::build(scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(scene, physics, root);
    let mut time = Time::default();
    for _ in 0..frames {
        time.advance(1.0 / 60.0);
        host.update(scene, &time, input.clone());
    }
    host.take_logs()
}

const ROTATOR: &str = r#"
local Rotator = { speed = 90, axis = Vector3.new(0, 1, 0), label = "spin", enabled_flag = true }
function Rotator:Start() Debug.Log("started", self.gameObject.name) end
function Rotator:Update()
    self.transform:Rotate(self.axis * (self.speed * Time.deltaTime))
end
return Rotator
"#;

#[test]
fn update_rotates_with_inspector_overrides_and_start_runs_once() {
    let root = project(&[("Rotator.lua", ROTATOR)]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Spinner", None);
    attach(
        &mut scene,
        e,
        "Rotator.lua",
        &[("speed", ScriptValue::Number(180.0))],
    );
    let logs = run(&mut scene, root, 30, Input::default());
    let yaw = scene.transform(e).euler().y;
    assert!((yaw - 90.0).abs() < 1.0, "180°/s for 0.5 s, got {yaw}");
    let starts = logs
        .iter()
        .filter(|(_, t)| t.starts_with("started"))
        .count();
    assert_eq!(starts, 1);
    assert!(
        logs.iter()
            .any(|(k, t)| *k == LogKind::Info && t == "started Spinner")
    );
}

#[test]
fn inspector_fields_in_declaration_order() {
    let fields = inspect_fields(ROTATOR, "Rotator.lua").unwrap();
    let names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["speed", "axis", "label", "enabled_flag"]);
    assert_eq!(fields[1].1, ScriptValue::Vector3([0.0, 1.0, 0.0]));
}

#[test]
fn input_axes_move_objects() {
    let mover = r#"
local Mover = { speed = 2 }
function Mover:Update()
    local h = Input.GetAxis("Horizontal")
    self.transform.position = self.transform.position + Vector3.new(h, 0, 0) * (self.speed * Time.deltaTime)
    if Input.GetKeyDown("space") then Debug.LogWarning("jump") end
end
return Mover
"#;
    let root = project(&[("Mover.lua", mover)]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Player", None);
    attach(&mut scene, e, "Mover.lua", &[]);
    let mut input = Input::default();
    input.held.insert("d".into());
    input.pressed.insert("space".into());
    let logs = run(&mut scene, root, 60, input);
    // GetAxis ramps up over 1/3 s (sensitivity 3), then holds 1: 50.5 frames' worth of 2/60 m.
    assert!((scene.world_position(e).x - 1.683).abs() < 0.01);
    assert!(
        logs.iter()
            .any(|(k, t)| *k == LogKind::Warning && t == "jump")
    );
}

#[test]
fn collisions_reach_scripts_and_destroy_removes_objects() {
    let script = r#"
local Breakable = {}
function Breakable:OnCollisionEnter(other)
    Debug.Log("hit by", other.name)
    Destroy(self.gameObject)
end
return Breakable
"#;
    let root = project(&[("Breakable.lua", script)]);
    let mut scene = Scene::new("t");
    let ground = scene.spawn("Ground", None);
    scene.add(
        ground,
        BoxCollider {
            size: Vec3::new(10.0, 1.0, 10.0),
            ..Default::default()
        },
    );
    let ball = scene.spawn("Ball", None);
    scene.set_transform(ball, Transform::from_position(Vec3::new(0.0, 2.0, 0.0)));
    scene.add(
        ball,
        MeshFilter {
            mesh: Some(PrimitiveMesh::Cube.into()),
        },
    );
    scene.add(ball, BoxCollider::default());
    scene.add(ball, Rigidbody::default());
    attach(&mut scene, ground, "Breakable.lua", &[]);
    let logs = run(&mut scene, root, 120, Input::default());
    assert!(logs.iter().any(|(_, t)| t == "hit by Ball"), "{logs:?}");
    assert!(!scene.contains(ground), "ground destroyed itself");
    assert!(scene.contains(ball));
}

#[test]
fn rigidbody_api_and_script_errors_are_reported() {
    let script = r#"
local Launcher = { power = 5 }
function Launcher:Start()
    self.body = self.gameObject:GetComponent("Rigidbody")
    self.body:AddForce(0, self.power, 0, ForceMode.VelocityChange)
end
function Launcher:Update()
    if Time.frameCount == 3 then nil_value.boom = 1 end
end
return Launcher
"#;
    let root = project(&[("Launcher.lua", script)]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Rocket", None);
    scene.add(e, BoxCollider::default());
    scene.add(
        e,
        Rigidbody {
            use_gravity: false,
            ..Default::default()
        },
    );
    attach(&mut scene, e, "Launcher.lua", &[]);
    let logs = run(&mut scene, root, 30, Input::default());
    let y = scene.world_position(e).y;
    assert!((y - 2.5).abs() < 0.15, "5 m/s for 0.5 s: {y}");
    let err = logs
        .iter()
        .find(|(k, _)| *k == LogKind::Error)
        .expect("runtime error logged");
    assert!(err.1.contains("Launcher (Update)"), "{}", err.1);
}

#[test]
fn missing_return_is_a_clear_error() {
    let root = project(&[("Broken.lua", "local X = {}")]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Thing", None);
    attach(&mut scene, e, "Broken.lua", &[]);
    let logs = run(&mut scene, root, 1, Input::default());
    assert!(
        logs.iter()
            .any(|(k, t)| *k == LogKind::Error && t.contains("must end with `return")),
        "{logs:?}"
    );
}

#[test]
fn lua_time_reflects_project_settings() {
    use crate::runtime::Runtime;
    use crate::settings::ProjectSettings;
    let probe = r#"
local Probe = {}
function Probe:Update() Debug.Log(string.format("time %.3f %.2f", Time.fixedDeltaTime, Time.timeScale)) end
return Probe
"#;
    let root = project(&[("Probe.lua", probe)]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Probe", None);
    attach(&mut scene, e, "Probe.lua", &[]);
    let mut settings = ProjectSettings::default();
    settings.physics.fixed_timestep = 0.05;
    settings.time.time_scale = 0.5;
    let mut runtime = Runtime::start(&mut scene, root, &settings);
    let mut time = Time::new(&settings.time);
    time.advance(1.0 / 60.0);
    runtime.update(&mut scene, &time, Input::default());
    let logs = runtime.take_logs();
    assert!(logs.iter().any(|(_, t)| t == "time 0.050 0.50"), "{logs:?}");
}

#[test]
fn instantiate_prefabs_and_objects_with_physics_and_scripts() {
    let spawner = r#"
local Spawner = {}
function Spawner:Start()
    local ball = Instantiate("Assets/Prefabs/Ball.prefab", Vector3.new(0, 5, 0))
    Debug.Log("spawned", ball.name)
    local copy = Instantiate(GameObject.Find("Template"))
    copy.transform.position = Vector3.new(3, 0, 0)
    Instantiate("Assets/Prefabs/Missing.prefab")
end
return Spawner
"#;
    let ball_script = r#"
local Ball = {}
function Ball:Awake() Debug.Log("awake", self.gameObject.name) end
return Ball
"#;
    let root = project(&[("Spawner.lua", spawner), ("Ball.lua", ball_script)]);

    // The prefab asset: a falling ball with a script.
    let mut asset = Scene::new("asset");
    let ball = asset.spawn("Ball", None);
    asset.add(ball, BoxCollider::default());
    asset.add(ball, Rigidbody::default());
    attach(&mut asset, ball, "Ball.lua", &[]);
    crate::scene::prefab::Prefab::from_scene(&asset, ball)
        .save(&root.join("Assets/Prefabs/Ball.prefab"))
        .unwrap();

    let mut scene = Scene::new("t");
    let s = scene.spawn("Spawner", None);
    attach(&mut scene, s, "Spawner.lua", &[]);
    let template = scene.spawn("Template", None);
    scene.spawn("Part", Some(template));
    let logs = run(&mut scene, root, 30, Input::default());

    assert!(
        logs.iter().any(|(_, t)| t == "awake Ball(Clone)"),
        "Awake runs on the new object: {logs:?}"
    );
    assert!(logs.iter().any(|(_, t)| t == "spawned Ball(Clone)"));
    assert!(
        logs.iter()
            .any(|(k, t)| *k == LogKind::Error && t.contains("Missing.prefab")),
        "{logs:?}"
    );
    let ball = scene.find_by_name("Ball(Clone)").unwrap();
    let y = scene.world_position(ball).y;
    assert!(
        y < 4.9,
        "the instance has a physics body and falls (y = {y})"
    );
    assert_eq!(
        scene.prefab_link(ball),
        None,
        "runtime copies are not linked"
    );
    let copy = scene.find_by_name("Template(Clone)").unwrap();
    assert_eq!(scene.children(copy).len(), 1);
    assert_eq!(scene.world_position(copy), Vec3::new(3.0, 0.0, 0.0));
}

#[test]
fn scene_manager_and_application_requests() {
    let root = project(&[(
        "Loader.lua",
        r#"
local Loader = {}
function Loader:Update()
    if Time.frameCount == 1 then
        Debug.Log(SceneManager.GetActiveScene().name)
        SceneManager.LoadScene("Level2")
        SceneManager.LoadScene(1)
        SceneManager.LoadScene("Extra", LoadSceneMode.Additive)
        SceneManager.UnloadSceneAsync("Extra")
        Application.Quit()
    elseif Time.frameCount == 2 then
        SceneManager.LoadScene(true)
    end
end
return Loader
"#,
    )]);
    let mut scene = Scene::new("Menu");
    let e = scene.spawn("Loader", None);
    attach(&mut scene, e, "Loader.lua", &[]);
    let physics = Physics::build(&scene, Vec3::ZERO);
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    time.advance(1.0 / 60.0);
    host.update(&mut scene, &time, Input::default());
    use crate::build::SceneRef;
    assert_eq!(
        host.take_requests(),
        [
            Request::LoadScene(SceneRef::Name("Level2".into())),
            Request::LoadScene(SceneRef::Index(1)),
            Request::LoadSceneAdditive(SceneRef::Name("Extra".into())),
            Request::UnloadScene(SceneRef::Name("Extra".into())),
            Request::Quit,
        ]
    );
    assert!(host.take_requests().is_empty(), "requests are taken once");
    time.advance(1.0 / 60.0);
    host.update(&mut scene, &time, Input::default());
    let logs = host.take_logs();
    assert!(logs.iter().any(|(_, t)| t == "Menu"), "{logs:?}");
    assert!(
        logs.iter()
            .any(|(k, t)| *k == LogKind::Error && t.contains("scene name or build index")),
        "{logs:?}"
    );
}

const JUKEBOX: &str = r#"
local Jukebox = {}
function Jukebox:Start()
    local src = self.gameObject:GetComponent("AudioSource")
    Debug.Log("awake playing", src.isPlaying, src.clip)
    src:Stop()
    Debug.Log("after stop", src.isPlaying)
    src.volume = 0.25
    src.pitch = 1.5
    src.loop = true
    src.clip = "Assets/Audio/Other.ogg"
    src:Play()
    Debug.Log("after play", src.isPlaying, src.volume, src.pitch, src.loop, src.clip)
    src:Pause()
    Debug.Log("after pause", src.isPlaying)
    src:UnPause()
    src:PlayOneShot("Assets/Audio/Shot.wav", 0.5)
    Debug.Log("none", self.gameObject:GetComponent("Rigidbody") == nil)
end
return Jukebox
"#;

#[test]
fn audio_source_api_against_the_silent_backend() {
    let root = project(&[("Jukebox.lua", JUKEBOX)]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Speaker", None);
    scene.add(
        e,
        crate::scene::AudioSource {
            clip: Some("Assets/Audio/Beep.wav".into()),
            ..Default::default()
        },
    );
    attach(&mut scene, e, "Jukebox.lua", &[]);
    let physics = Physics::build(&scene, Vec3::ZERO);
    let (audio, log) = Audio::null(root.clone());
    let mut host = ScriptHost::start_with_audio(&mut scene, physics, root.clone(), audio);
    let mut time = Time::default();
    time.advance(1.0 / 60.0);
    host.update(&mut scene, &time, Input::default());
    let logs: Vec<String> = host.take_logs().into_iter().map(|(_, t)| t).collect();
    assert_eq!(
        logs,
        [
            "awake playing true Assets/Audio/Beep.wav",
            "after stop false",
            "after play true 0.25 1.5 true Assets/Audio/Other.ogg",
            "after pause false",
            "none true",
        ]
    );
    let played: Vec<PathBuf> = log.lock().unwrap().played.clone();
    let clips: Vec<PathBuf> = ["Beep.wav", "Other.ogg", "Shot.wav"]
        .iter()
        .map(|f| root.join("Assets/Audio").join(f))
        .collect();
    assert_eq!(played, clips, "play on awake, Play(), PlayOneShot()");
    let src = scene.world.get::<&crate::scene::AudioSource>(e).unwrap();
    assert!(
        src.looping && src.volume == 0.25,
        "settings are written to the component"
    );
    drop(src);
    assert_eq!(host.with_audio(|a| a.voice_count()), 2);
}

const ANIMATOR: &str = r#"
local A = { frames = 0 }
function A:Start()
    self.anim = self.gameObject:GetComponent("Animation")
    Debug.Log("auto", self.anim.isPlaying, self.anim.clip)
    self.anim.speed = 2
end
function A:Update()
    self.frames = self.frames + 1
    if self.frames == 10 then
        self.anim:Stop()
        Debug.Log("stopped", self.anim.isPlaying, self.anim.time == 0)
        Debug.Log("missing", self.anim:Play("Nope"))
        Debug.Log("play", self.anim:Play("Rise"), self.anim:IsPlaying("Rise"))
    end
end
function A:Peak() Debug.Log("event Peak") end
return A
"#;

#[test]
fn animation_component_lua_api_and_events() {
    use crate::animation::{AnimationClip, AnimationEvent, Property};
    let root = project(&[("A.lua", ANIMATOR)]);
    let mut spin = AnimationClip::default();
    spin.set_key("", Property::Rotation(1), 0.0, 0.0);
    spin.set_key("", Property::Rotation(1), 1.0, 90.0);
    spin.events.push(AnimationEvent {
        time: 0.1,
        function: "Peak".into(),
    });
    spin.save(&root.join("Assets/Anim/Spin.anim")).unwrap();
    let mut rise = AnimationClip {
        looping: false,
        ..Default::default()
    };
    rise.set_key("", Property::Position(1), 0.0, 0.0);
    rise.set_key("", Property::Position(1), 0.5, 3.0);
    rise.save(&root.join("Assets/Anim/Rise.anim")).unwrap();

    let mut scene = Scene::new("t");
    let e = scene.spawn("Dancer", None);
    scene.add(
        e,
        crate::scene::Animation {
            clip: Some("Assets/Anim/Spin.anim".into()),
            clips: vec!["Assets/Anim/Rise.anim".into()],
            ..Default::default()
        },
    );
    attach(&mut scene, e, "A.lua", &[]);
    let logs = run(&mut scene, root, 60, Input::default());
    let texts: Vec<&str> = logs.iter().map(|(_, t)| t.as_str()).collect();
    assert!(texts.contains(&"auto true Spin"), "{texts:?}");
    assert!(texts.contains(&"event Peak"), "{texts:?}");
    assert!(texts.contains(&"stopped false true"), "{texts:?}");
    assert!(texts.contains(&"missing false"), "{texts:?}");
    assert!(texts.contains(&"play true true"), "{texts:?}");
    assert!(
        logs.iter()
            .any(|(k, t)| *k == LogKind::Warning && t.contains("Nope")),
        "missing clip warns"
    );
    // Rise (0.5 s at speed 2) finished long ago and holds its last pose; Spin stopped mid-way.
    let t = scene.transform(e);
    assert!((t.position.y - 3.0).abs() < 1e-4, "{}", t.position.y);
    assert!(t.euler().y > 0.0 && t.euler().y < 90.0);
}

const SPARKS: &str = r#"
local Sparks = {}
function Sparks:Start()
    local ps = self.gameObject:GetComponent("ParticleSystem")
    Debug.Log("start", ps.isPlaying, ps.particleCount)
    ps:Stop()
    ps:Emit(5)
    Debug.Log("emit", ps.isPlaying, ps.isStopped, ps.particleCount)
    self.ps = ps
end
function Sparks:Update()
    if Time.frameCount == 3 then
        self.ps:Clear()
        self.ps:Play()
        Debug.Log("play", self.ps.isPlaying, self.ps.particleCount)
        self.ps:Pause()
        Debug.Log("pause", self.ps.isPlaying, self.ps.isPaused)
        Debug.Log("none", self.gameObject:GetComponent("AudioSource") == nil)
    end
end
return Sparks
"#;

#[test]
fn particle_system_api_controls_the_emitter() {
    use crate::particles::{MinMax, ParticleEmitter, ParticleSystem};
    let root = project(&[("Sparks.lua", SPARKS)]);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Sparks", None);
    let mut ps = ParticleSystem::default();
    ps.main.start_lifetime = MinMax::constant(10.0);
    ps.emission.rate_over_time = MinMax::constant(0.0);
    scene.add(e, ps);
    attach(&mut scene, e, "Sparks.lua", &[]);
    let (audio, _) = Audio::null(root.clone());
    let settings = crate::settings::ProjectSettings::default();
    let mut rt = crate::runtime::Runtime::start_with_audio(&mut scene, root, &settings, audio);
    let mut time = Time::default();
    for _ in 0..4 {
        time.advance(1.0 / 60.0);
        rt.update(&mut scene, &time, Input::default());
    }
    let logs: Vec<String> = rt.take_logs().into_iter().map(|(_, t)| t).collect();
    assert_eq!(
        logs,
        [
            "start true 0",
            "emit false false 5",
            "play true 0",
            "pause false true",
            "none true",
        ]
    );
    let em = scene.world.get::<&ParticleEmitter>(e).unwrap();
    assert!(em.is_paused(), "paused emitter stays paused");
}
