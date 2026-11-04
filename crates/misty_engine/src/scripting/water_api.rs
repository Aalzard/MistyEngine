//! Lua `Water`: `Water.GetHeight(x, z)` / `Water.GetHeight(position)` return the height of the
//! water surface under a point (matching the drawn waves), or `nil` where there is no water.

use mlua::{Lua, Result, Value};

use super::api::{LVec3, with_world};

/// Register the global `Water` table.
pub fn register(lua: &Lua) -> Result<()> {
    let water = lua.create_table()?;
    water.set(
        "GetHeight",
        lua.create_function(|lua, (a, b): (Value, Option<f32>)| {
            let (x, z) = match (a, b) {
                (Value::UserData(u), _) => {
                    let p = u.borrow::<LVec3>()?.0;
                    (p.x, p.z)
                }
                (Value::Number(x), Some(z)) => (x as f32, z),
                (Value::Integer(x), Some(z)) => (x as f32, z),
                _ => {
                    return Err(mlua::Error::runtime(
                        "Water.GetHeight takes (x, z) or a Vector3",
                    ));
                }
            };
            with_world(lua, |w| crate::water::height_at(&w.scene, x, z))
        })?,
    )?;
    lua.globals().set("Water", water)
}

#[cfg(test)]
mod tests {
    use glam::{Vec2, Vec3};

    use crate::physics::Physics;
    use crate::scene::{Scene, ScriptRef, Scripts};
    use crate::scripting::ScriptHost;
    use crate::water::{WaterKind, spawn_water};

    #[test]
    fn get_height_matches_the_engine_query() {
        let dir = std::env::temp_dir().join("misty_water_api");
        std::fs::create_dir_all(dir.join("Assets")).unwrap();
        std::fs::write(
            dir.join("Assets/Probe.lua"),
            "local P = {}\nfunction P:Start()\n    local a = Water.GetHeight(3, 4)\n    local b = Water.GetHeight(Vector3.new(3, 100, 4))\n    assert(math.abs(a - b) < 0.05)\n    self.transform.position = Vector3.new(0, a, 0)\n    assert(Water.GetHeight(500, 0) == nil)\nend\nreturn P\n",
        )
        .unwrap();
        let mut s = Scene::new("w");
        spawn_water(
            &mut s,
            "Lake",
            WaterKind::Lake,
            Vec3::new(0.0, 2.0, 0.0),
            Vec2::new(40.0, 40.0),
        );
        let probe = s.spawn("Probe", None);
        s.add(
            probe,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Probe.lua".into(),
                    ..Default::default()
                }],
            },
        );
        let physics = Physics::build(&s, Vec3::ZERO);
        let mut host = ScriptHost::start(&mut s, physics, dir);
        host.update(&mut s, &Default::default(), Default::default());
        let logs = host.take_logs();
        assert!(logs.is_empty(), "{logs:?}");
        let y = s.world_position(probe).y;
        assert!((y - 2.0).abs() < 0.5, "{y}");
    }
}
