//! Lua physics API tests: Physics queries, LayerMask, CharacterController and Rigidbody extras.

use std::path::PathBuf;

use glam::Vec3;
use hecs::Entity;

use crate::physics::Physics;
use crate::runtime::Time;
use crate::scene::{
    BoxCollider, CharacterController, Rigidbody, Scene, ScriptRef, Scripts, Transform,
};
use crate::scripting::{Input, LogKind, ScriptHost};

fn project(name: &str, source: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_physics_script_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts").join(name), source).unwrap();
    root
}

fn script(scene: &mut Scene, e: Entity, file: &str) {
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: format!("Assets/Scripts/{file}"),
                ..Default::default()
            }],
        },
    );
}

fn solid(scene: &mut Scene, name: &str, pos: Vec3, size: Vec3) -> Entity {
    let e = scene.spawn(name, None);
    let mut t = Transform::from_position(pos);
    t.scale = size;
    scene.set_transform(e, t);
    scene.add(e, BoxCollider::default());
    e
}

fn run(scene: &mut Scene, root: PathBuf, frames: u32) -> Vec<String> {
    let physics = Physics::build(scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(scene, physics, root);
    let mut time = Time::default();
    for _ in 0..frames {
        time.advance(1.0 / 60.0);
        host.update(scene, &time, Input::default());
    }
    let logs = host.take_logs();
    for (kind, text) in &logs {
        assert_ne!(*kind, LogKind::Error, "{text}");
    }
    logs.into_iter().map(|(_, t)| t).collect()
}

const PROBE: &str = r#"
local Probe = { frames = 0 }
function Probe:Start()
    local o = Vector3.new(0, 0.5, 0)
    local hit = Physics.Raycast(o, Vector3.forward, 100)
    Debug.Log("hit", hit.gameObject.name, string.format("%.2f", hit.distance),
        hit.collider.gameObject.name, hit.transform.name, hit.normal.z < -0.99,
        string.format("%.2f", hit.point.z), hit.rigidbody == nil)
    Debug.Log("short", Physics.Raycast(o, Vector3.forward, 2) == nil)
    Debug.Log("mask", LayerMask.GetMask("Default", "Water"), LayerMask.NameToLayer("Player"),
        LayerMask.LayerToName(5))
    Debug.Log("masked", Physics.Raycast(o, Vector3.forward, Mathf.Infinity,
        ~LayerMask.GetMask("Default")) == nil)
    Debug.Log("all", #Physics.RaycastAll(o, Vector3.forward, 100))
    local near = Physics.OverlapSphere(Vector3.new(0, 0.5, 5), 0.4)
    Debug.Log("overlap", #near, near[1].name)
    Debug.Log("sphere", Physics.SphereCast(o, 0.3, Vector3.forward, 100).gameObject.name)
    self.cc = self.gameObject:GetComponent("CharacterController")
    self.gameObject.layer = LayerMask.NameToLayer("Player")
    Debug.Log("layer", self.gameObject.layer)
end
function Probe:Update()
    self.cc:SimpleMove(Vector3.new(2, 0, 0))
    self.frames = self.frames + 1
    if self.frames == 30 then
        Debug.Log("grounded", self.cc.isGrounded, self.cc.velocity.x > 1.5, self.cc.height)
    end
end
function Probe:OnControllerColliderHit(hit)
    if hit.gameObject.name == "Wall" and not self.reported then
        self.reported = true
        Debug.Log("cc hit", hit.gameObject.name, hit.normal.x < -0.5)
    end
end
return Probe
"#;

#[test]
fn raycasts_layer_masks_and_character_controller_from_lua() {
    let root = project("Probe.lua", PROBE);
    let mut scene = Scene::new("t");
    solid(
        &mut scene,
        "Ground",
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(60.0, 1.0, 60.0),
    );
    solid(&mut scene, "Target", Vec3::new(0.0, 0.5, 5.0), Vec3::ONE);
    let water = solid(&mut scene, "Water", Vec3::new(0.0, 0.5, 8.0), Vec3::ONE);
    scene.game_object_mut(water).layer = 3;
    solid(
        &mut scene,
        "Wall",
        Vec3::new(13.0, 1.5, -10.0),
        Vec3::new(1.0, 3.0, 2.0),
    );
    let player = scene.spawn("Player", None);
    scene.set_transform(
        player,
        Transform::from_position(Vec3::new(10.0, 1.2, -10.0)),
    );
    scene.add(player, CharacterController::default());
    script(&mut scene, player, "Probe.lua");

    let logs = run(&mut scene, root, 120);
    let line = |prefix: &str| {
        logs.iter()
            .find(|l| l.starts_with(prefix))
            .unwrap_or_else(|| panic!("no '{prefix}' in {logs:?}"))
            .clone()
    };
    assert_eq!(line("hit "), "hit Target 4.50 Target Target true 4.50 true");
    assert_eq!(line("short"), "short true");
    assert_eq!(line("mask"), "mask 9 5 Player");
    assert_eq!(line("masked"), "masked false", "Water is still hit");
    assert_eq!(line("all"), "all 2");
    assert_eq!(line("overlap"), "overlap 1 Target");
    assert_eq!(line("sphere"), "sphere Target");
    assert_eq!(line("layer"), "layer 5");
    assert_eq!(line("grounded"), "grounded true true 2");
    assert_eq!(line("cc hit"), "cc hit Wall true");
    let x = scene.world_position(player).x;
    assert!(
        x > 11.0 && x < 12.5,
        "walked into the wall and stopped: {x}"
    );
}

const MOVER: &str = r#"
local Mover = {}
function Mover:Start()
    self.rb = self.gameObject:GetComponent("Rigidbody")
    self.rb.useGravity = false
    Debug.Log("flags", self.rb.useGravity, self.rb.isKinematic)
end
function Mover:FixedUpdate()
    self.rb:MovePosition(self.rb.position + Vector3.new(0, 0, 1) * Time.fixedDeltaTime)
    self.rb:MoveRotation(Vector3.new(0, 90, 0))
end
return Mover
"#;

#[test]
fn rigidbody_move_position_rotation_and_gravity_from_lua() {
    let root = project("Mover.lua", MOVER);
    let mut scene = Scene::new("t");
    let e = solid(&mut scene, "Mover", Vec3::new(0.0, 3.0, 0.0), Vec3::ONE);
    scene.add(e, Rigidbody::default());
    script(&mut scene, e, "Mover.lua");
    let logs = run(&mut scene, root, 60);
    assert!(logs.iter().any(|l| l == "flags false false"), "{logs:?}");
    let p = scene.world_position(e);
    assert!((p.y - 3.0).abs() < 0.01, "no gravity: {p}");
    assert!((p.z - 1.0).abs() < 0.1, "moved 1 m/s for 1 s: {p}");
    let yaw = scene.transform(e).euler().y;
    assert!((yaw - 90.0).abs() < 1.0, "{yaw}");
}

/// A Rigidbody handle kept after `Destroy`: its members raise a Lua error instead of
/// panicking (mlua re-raises Rust panics, which aborted the game).
#[test]
fn rigidbody_of_a_destroyed_object_errors_in_lua() {
    let src = r#"
local Keeper = { frames = 0 }
local function check(name, f)
    local ok, err = pcall(f)
    Debug.Log(name, ok, string.find(tostring(err), "destroyed", 1, true) ~= nil)
end
function Keeper:Start()
    local box = GameObject.Find("Box")
    self.rb = box:GetComponent("Rigidbody")
    Destroy(box)
end
function Keeper:Update()
    self.frames = self.frames + 1
    if self.frames == 3 then
        local rb = self.rb
        check("position", function() return rb.position end)
        check("MovePosition", function() rb:MovePosition(Vector3.new(1, 2, 3)) end)
        check("MoveRotation", function() rb:MoveRotation(Vector3.new(0, 90, 0)) end)
    end
end
return Keeper
"#;
    let root = project("Keeper.lua", src);
    let mut scene = Scene::new("t");
    let b = solid(&mut scene, "Box", Vec3::new(0.0, 5.0, 0.0), Vec3::ONE);
    scene.add(b, Rigidbody::default());
    let keeper = scene.spawn("Keeper", None);
    script(&mut scene, keeper, "Keeper.lua");
    let logs = run(&mut scene, root, 5);
    for name in ["position", "MovePosition", "MoveRotation"] {
        assert!(
            logs.contains(&format!("{name} false true")),
            "{name}: {logs:?}"
        );
    }
}
