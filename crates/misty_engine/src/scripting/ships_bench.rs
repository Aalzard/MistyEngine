//! 5,000 ships benchmark (ECS assessment, not committed).
use super::*;
use crate::physics::Physics;
use crate::scene::{ScriptRef, Transform};
use glam::{Quat, Vec3};
use std::time::Instant;

const SHIP: &str = r#"
local Ship = { vx = 1, vz = 0 }
function Ship:Update()
    local dt = Time.deltaTime
    local p = self.transform.position
    -- steer toward the origin, like ships circling a point
    local ax, az = -p.x * 0.1, -p.z * 0.1
    self.vx = self.vx + ax * dt
    self.vz = self.vz + az * dt
    local np = Vector3.new(p.x + self.vx * dt, p.y, p.z + self.vz * dt)
    self.transform.position = np
    self.transform:LookAt(Vector3.new(np.x + self.vx, np.y, np.z + self.vz))
end
return Ship
"#;

fn project() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("misty_ships_{}", std::process::id()));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts/Ship.lua"), SHIP).unwrap();
    root
}

fn spawn(scene: &mut Scene, n: usize, parented: bool, script: bool) -> Vec<Entity> {
    let fleet = parented.then(|| scene.spawn("Fleet", None));
    (0..n)
        .map(|i| {
            let e = scene.spawn("Ship", fleet);
            let a = i as f32 * 0.01;
            let r = 10.0 + (i % 100) as f32;
            scene.set_transform(
                e,
                Transform::from_position(Vec3::new(a.cos() * r, 0.0, a.sin() * r)),
            );
            if script {
                scene.add(
                    e,
                    Scripts {
                        list: vec![ScriptRef {
                            path: "Assets/Scripts/Ship.lua".into(),
                            enabled: true,
                            fields: Default::default(),
                        }],
                    },
                );
            }
            e
        })
        .collect()
}

fn time_host(scene: &mut Scene, frames: u32) -> f64 {
    let physics = Physics::build(scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(scene, physics, project());
    let mut time = Time::default();
    for _ in 0..10 {
        time.advance(1.0 / 60.0);
        host.update(scene, &time, Input::default());
    }
    let mut v = vec![];
    for _ in 0..frames {
        time.advance(1.0 / 60.0);
        let t = Instant::now();
        host.update(scene, &time, Input::default());
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

/// The same work as one Rust loop over the ECS (what an "ECS system" would cost).
fn time_system(scene: &mut Scene, ships: &[Entity], frames: u32) -> f64 {
    let mut vel: Vec<Vec3> = vec![Vec3::X; ships.len()];
    let mut v = vec![];
    for _ in 0..frames {
        let t = Instant::now();
        let dt = 1.0 / 60.0;
        for (i, &e) in ships.iter().enumerate() {
            let mut tr = scene.world.get::<&mut Transform>(e).unwrap();
            let p = tr.position;
            vel[i] += Vec3::new(-p.x * 0.1, 0.0, -p.z * 0.1) * dt;
            tr.position = p + vel[i] * dt;
            let f = vel[i].normalize_or_zero();
            tr.rotation = Quat::from_rotation_arc(Vec3::Z, f);
        }
        v.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

#[test]
#[ignore = "bench"]
fn ships_bench() {
    for &n in &[1000usize, 5000, 10000] {
        let mut s = Scene::new("b");
        spawn(&mut s, n, false, false);
        let empty = time_host(&mut s, 60);
        let mut s = Scene::new("b");
        spawn(&mut s, n, false, true);
        let lua_root = time_host(&mut s, 60);
        let mut s = Scene::new("b");
        spawn(&mut s, n, true, true);
        let lua_child = time_host(&mut s, 60);
        let mut s = Scene::new("b");
        let ships = spawn(&mut s, n, false, false);
        let sys = time_system(&mut s, &ships, 60);
        eprintln!(
            "n={n}: frame w/o scripts {empty:.2} ms | Lua MonoBehaviour-style {lua_root:.2} ms | same, ships under a parent {lua_child:.2} ms | Rust ECS loop {sys:.3} ms"
        );
    }
}
