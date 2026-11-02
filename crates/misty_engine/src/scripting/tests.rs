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
    assert!((scene.world_position(e).x - 2.0).abs() < 0.05);
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
            mesh: Some(PrimitiveMesh::Cube),
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
