//! What one Lua script costs per frame, by kind of work (ignored: a benchmark).
//!
//! `cargo test --release -p misty_engine --lib lua_overhead_bench -- --ignored --nocapture`
//!
//! `MISTY_LUA_BENCH=empty,fields` runs only those cases, `MISTY_LUA_BENCH_FRAMES=600` times more
//! frames (for a profiler).
use super::*;
use crate::physics::Physics;
use crate::scene::{ScriptRef, Transform};
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
    let root = std::env::temp_dir().join(format!("misty_lua_bench_{}", std::process::id()));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts/Bench.lua"), script).unwrap();
    root
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
    let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(&mut scene, physics, project(script));
    let mut time = Time::default();
    for _ in 0..10 {
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
