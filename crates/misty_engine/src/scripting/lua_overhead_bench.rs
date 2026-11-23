//! What one Lua script costs per frame, by kind of work (ignored: a benchmark).
//!
//! `cargo test --release -p misty_engine --lib lua_overhead_bench -- --ignored --nocapture`
//!
//! `MISTY_LUA_BENCH=empty,fields` runs only those cases, `MISTY_LUA_BENCH_FRAMES=600` times more
//! frames (for a profiler).
//!
//! `events_bench` (same command, other name) times what happens around the scripts in a busy
//! scene: objects spawned and destroyed every frame, and collision callbacks
//! (`MISTY_EVENTS_BENCH=prefab`: only the prefab spawner, for a profiler).
use super::*;
use crate::physics::Physics;
use crate::scene::{BoxCollider, Rigidbody, ScriptRef, Transform};
use glam::Vec3;
use std::time::Instant;

/// (case, objects, script). Each object runs one copy of the script.
const CASES: &[(&str, usize, &str)] = &[
    // The call itself: finding `Update` and calling it.
    (
        "empty",
        5000,
        r#"
local S = {}
function S:Update() end
return S
"#,
    ),
    // Script fields: defaults from the class, then the instance's own.
    (
        "fields",
        5000,
        r#"
local S = { speed = 2, hp = 100, t = 0 }
function S:Update()
    self.t = self.t + self.speed * Time.deltaTime
    if self.t > 1 then self.t = 0; self.hp = self.hp - 1 end
    if self.hp <= 0 then self.hp = 100 end
end
return S
"#,
    ),
    // The 5,000 ships of `ships_bench`: position get/set and LookAt.
    (
        "ships",
        5000,
        r#"
local Ship = { vx = 1, vz = 0 }
function Ship:Update()
    local dt = Time.deltaTime
    local p = self.transform.position
    local ax, az = -p.x * 0.1, -p.z * 0.1
    self.vx = self.vx + ax * dt
    self.vz = self.vz + az * dt
    local np = Vector3.new(p.x + self.vx * dt, p.y, p.z + self.vz * dt)
    self.transform.position = np
    self.transform:LookAt(Vector3.new(np.x + self.vx, np.y, np.z + self.vz))
end
return Ship
"#,
    ),
    // A typical small behaviour: input, Translate, Rotate, vector math.
    (
        "mover",
        5000,
        r#"
local M = { speed = 3, turn = 90 }
function M:Update()
    local dt = Time.deltaTime
    local h = Input.GetAxis("Horizontal")
    self.transform:Rotate(0, (h + 1) * self.turn * dt, 0)
    self.transform:Translate(Vector3.forward * (self.speed * dt))
    local p = self.transform.position
    if p.magnitude > 500 then self.transform.position = Vector3.zero end
end
return M
"#,
    ),
    // Update, FixedUpdate and LateUpdate on every object.
    (
        "three_callbacks",
        5000,
        r#"
local S = { n = 0 }
function S:Update() self.n = self.n + 1 end
function S:FixedUpdate() self.n = self.n + 1 end
function S:LateUpdate() self.n = self.n + 1 end
return S
"#,
    ),
    // Looking components up every frame instead of keeping them from Start.
    (
        "get_component",
        5000,
        r#"
local S = { n = 0 }
function S:Update()
    local t = self.gameObject:GetComponent("Transform")
    local me = self.gameObject:GetComponent("Bench")
    local none = self.gameObject:GetComponent("Rigidbody")
    if t and me and not none then self.n = self.n + 1 end
end
return S
"#,
    ),
    // One ray per object per frame (nothing to hit: the cost of asking).
    (
        "raycast",
        5000,
        r#"
local S = { n = 0 }
function S:Update()
    if Physics.Raycast(self.transform.position, Vector3.down, 2) then self.n = self.n + 1 end
end
return S
"#,
    ),
    // Plain Lua work in one script: what the endless-loop guard costs.
    (
        "pure_lua",
        1,
        r#"
local S = { sum = 0 }
function S:Update()
    local s = 0
    for i = 1, 200000 do
        s = s + (i % 7) * 0.5
    end
    self.sum = s
end
return S
"#,
    ),
];

fn project(script: &str) -> std::path::PathBuf {
    project_of(&[("Bench.lua", script)])
}

fn project_of(scripts: &[(&str, &str)]) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("misty_lua_bench_{}", std::process::id()));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    for (file, script) in scripts {
        std::fs::write(root.join("Assets/Scripts").join(file), script).unwrap();
    }
    root
}

fn script(scene: &mut Scene, e: Entity, file: &str) {
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: format!("Assets/Scripts/{file}"),
                enabled: true,
                fields: Default::default(),
            }],
        },
    );
}

fn spawn(scene: &mut Scene, n: usize) {
    for i in 0..n {
        // Own names: `spawn` looks through the siblings for a free one.
        let e = scene.spawn(&format!("Obj{i}"), None);
        let a = i as f32 * 0.01;
        let r = 10.0 + (i % 100) as f32;
        scene.set_transform(
            e,
            Transform::from_position(Vec3::new(a.cos() * r, 0.0, a.sin() * r)),
        );
        scene.add(
            e,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Scripts/Bench.lua".into(),
                    enabled: true,
                    fields: Default::default(),
                }],
            },
        );
    }
}

/// Median frame time in ms.
fn time_case(n: usize, script: &str, frames: usize) -> f64 {
    let mut scene = Scene::new("b");
    spawn(&mut scene, n);
    time_scene(scene, project(script), 10, frames)
}

/// Median frame time in ms of `scene` after `warm_up` frames.
fn time_scene(mut scene: Scene, root: std::path::PathBuf, warm_up: usize, frames: usize) -> f64 {
    let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    for _ in 0..warm_up {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
    }
    let mut v = Vec::with_capacity(frames);
    for _ in 0..frames {
        time.advance(1.0 / 60.0);
        let t = Instant::now();
        host.update(&mut scene, &time, Input::default());
        v.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let errs: Vec<_> = host
        .take_logs()
        .into_iter()
        .filter(|(k, _)| *k == LogKind::Error)
        .take(3)
        .collect();
    assert!(errs.is_empty(), "{errs:?}");
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

#[test]
#[ignore = "bench"]
fn lua_overhead_bench() {
    let only = std::env::var("MISTY_LUA_BENCH").unwrap_or_default();
    let frames = std::env::var("MISTY_LUA_BENCH_FRAMES")
        .ok()
        .and_then(|f| f.parse().ok())
        .unwrap_or(120);
    for (name, n, script) in CASES {
        if !only.is_empty() && !only.split(',').any(|o| o == *name) {
            continue;
        }
        let ms = time_case(*n, script, frames);
        eprintln!(
            "{name:>16}: {ms:7.3} ms/frame, {n} objects, {:6.0} ns each",
            ms * 1e6 / *n as f64
        );
    }
}

const IDLE: &str = r#"
local S = { n = 0 }
function S:Update() self.n = self.n + 1 end
return S
"#;

/// A bullet lives 20 frames, then destroys itself (the template named "Bullet" stays).
const BULLET: &str = r#"
local B = { age = 0 }
function B:Update()
    self.age = self.age + 1
    self.transform:Translate(0, 0, 0.1)
    if self.age > 20 and self.gameObject.name ~= "Bullet" then Destroy(self.gameObject) end
end
return B
"#;

const SPAWNER: &str = r#"
local S = { rate = 100 }
function S:Start() self.template = GameObject.Find("Bullet") end
function S:Update()
    for i = 1, self.rate do Instantiate(self.template) end
end
return S
"#;

const TOUCH: &str = r#"
local T = { hits = 0 }
function T:OnCollisionStay(other) self.hits = self.hits + 1 end
return T
"#;

/// `n` scripted objects that only count frames.
fn crowd(scene: &mut Scene, n: usize) {
    for i in 0..n {
        let e = scene.spawn(&format!("Idle{i}"), None);
        script(scene, e, "Idle.lua");
    }
}

/// 100 bullets spawned and 100 destroyed per frame (2,000 alive), next to `idle` other
/// scripted objects.
fn bullets(idle: usize, frames: usize) -> f64 {
    let mut scene = Scene::new("b");
    crowd(&mut scene, idle);
    let bullet = scene.spawn("Bullet", None);
    script(&mut scene, bullet, "Bullet.lua");
    let spawner = scene.spawn("Spawner", None);
    script(&mut scene, spawner, "Spawner.lua");
    let root = project_of(&[
        ("Idle.lua", IDLE),
        ("Bullet.lua", BULLET),
        ("Spawner.lua", SPAWNER),
    ]);
    // Past the 20 frames a bullet lives: as many go as come.
    time_scene(scene, root, 40, frames)
}

const PREFAB_SPAWNER: &str = r#"
local S = { rate = 100, n = 0 }
function S:Update()
    -- Apart from each other: colliders in one spot would time the physics pair search.
    self.n = self.n + 1
    for i = 1, self.rate do
        Instantiate("Assets/Prefabs/Bullet.prefab", Vector3.new(i * 3, 0, self.n * -3))
    end
end
return S
"#;

/// Like [`bullets`], with the bullets instantiated from a prefab file by its path.
fn prefab_bullets(idle: usize, frames: usize) -> f64 {
    let root = project_of(&[
        ("Idle.lua", IDLE),
        ("Bullet.lua", BULLET),
        ("Spawner.lua", PREFAB_SPAWNER),
    ]);
    let mut asset = Scene::new("p");
    let bullet = asset.spawn("Shot", None);
    asset.add(bullet, BoxCollider::default());
    script(&mut asset, bullet, "Bullet.lua");
    let tip = asset.spawn("Tip", Some(bullet));
    asset.set_transform(tip, Transform::from_position(Vec3::new(0.0, 0.0, 0.5)));
    crate::scene::prefab::Prefab::from_scene(&asset, bullet)
        .save(&root.join("Assets/Prefabs/Bullet.prefab"))
        .unwrap();
    let mut scene = Scene::new("b");
    crowd(&mut scene, idle);
    let spawner = scene.spawn("Spawner", None);
    script(&mut scene, spawner, "Spawner.lua");
    time_scene(scene, root, 40, frames)
}

/// `n` boxes resting on a floor, each with a script (`file`), next to `idle` other scripted
/// objects.
fn resting_boxes(n: usize, idle: usize, file: &str, frames: usize) -> f64 {
    let mut scene = Scene::new("b");
    crowd(&mut scene, idle);
    let floor = scene.spawn("Floor", None);
    scene.set_transform(floor, Transform::from_position(Vec3::new(0.0, -0.5, 0.0)));
    scene.add(
        floor,
        BoxCollider {
            size: Vec3::new(1000.0, 1.0, 1000.0),
            ..Default::default()
        },
    );
    for i in 0..n {
        let e = scene.spawn(&format!("Box{i}"), None);
        let (x, z) = ((i % 32) as f32 * 2.0, (i / 32) as f32 * 2.0);
        scene.set_transform(e, Transform::from_position(Vec3::new(x, 0.5, z)));
        scene.add(e, BoxCollider::default());
        scene.add(e, Rigidbody::default());
        script(&mut scene, e, file);
    }
    let root = project_of(&[("Idle.lua", IDLE), ("Touch.lua", TOUCH)]);
    time_scene(scene, root, 30, frames)
}

#[test]
#[ignore = "bench"]
fn events_bench() {
    let frames = 120;
    // For a profiler: only the prefab spawner, for longer.
    if std::env::var("MISTY_EVENTS_BENCH").is_ok_and(|v| v == "prefab") {
        let ms = prefab_bullets(0, 1200);
        eprintln!("prefab bullets: {ms:7.3} ms/frame");
        return;
    }
    for idle in [0, 5000] {
        let ms = bullets(idle, frames);
        eprintln!(
            "bullets (100 in, 100 out per frame, 2000 alive), {idle} idle scripts: {ms:7.3} ms/frame"
        );
    }
    for idle in [0, 5000] {
        let ms = prefab_bullets(idle, frames);
        eprintln!(
            "prefab bullets (100 in, 100 out per frame, 2000 alive), {idle} idle scripts: {ms:7.3} ms/frame"
        );
    }
    for idle in [0, 5000] {
        let quiet = resting_boxes(500, idle, "Idle.lua", frames);
        let stay = resting_boxes(500, idle, "Touch.lua", frames);
        eprintln!(
            "500 resting boxes, {idle} idle scripts: {quiet:7.3} ms/frame without OnCollisionStay, {stay:7.3} ms with"
        );
    }
}
