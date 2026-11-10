//! Lua `Terrain`: `Terrain.activeTerrain` / `Terrain.activeTerrains` and `GetComponent("Terrain")`
//! give a terrain with `SampleHeight(position)`, `GetPosition()`, `terrainData` (size and
//! resolutions), `gameObject` and `transform`.

use hecs::Entity;
use mlua::{Lua, Result, UserData, UserDataFields, UserDataMethods, Value};

use super::api::{LGameObject, LTransform, LVec3, alive, with_world};
use crate::scene::Terrain;
use crate::terrain;

#[derive(Clone, Copy)]
pub struct LTerrain(pub Entity);

/// The global `Terrain` table.
struct LTerrainStatics;

impl UserData for LTerrainStatics {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("activeTerrain", |lua, _| {
            with_world(lua, |w| terrain::active_terrain(&w.scene).map(LTerrain))
        });
        f.add_field_method_get("activeTerrains", |lua, _| {
            let list = with_world(lua, |w| {
                terrain::terrains(&w.scene)
                    .into_iter()
                    .map(|(e, _)| LTerrain(e))
                    .collect::<Vec<_>>()
            })?;
            lua.create_sequence_from(list)
        });
    }
}

impl UserData for LTerrain {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("gameObject", |_, t| Ok(LGameObject(t.0)));
        f.add_field_method_get("transform", |_, t| Ok(LTransform(t.0)));
        f.add_field_method_get("terrainData", |lua, t| {
            let Some(d) = with_world(lua, |w| terrain::data_of(&w.scene, t.0))? else {
                return Ok(Value::Nil);
            };
            let table = lua.create_table()?;
            table.set("size", LVec3(d.size))?;
            table.set("heightmapResolution", d.heightmap_resolution)?;
            table.set("alphamapResolution", d.alphamap_resolution)?;
            table.set("layerCount", d.layers.len())?;
            Ok(Value::Table(table))
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        // Height of the surface under a world position, relative to the terrain's position.
        m.add_method("SampleHeight", |lua, t, p: LVec3| {
            with_world(lua, |w| {
                terrain::sample_height(&w.scene, t.0, p.0).unwrap_or(0.0)
            })
        });
        m.add_method("GetPosition", |lua, t, ()| {
            with_world(lua, |w| {
                alive(w, t.0)?;
                Ok(LVec3(w.scene.world_position(t.0)))
            })?
        });
    }
}

/// `GetComponent("Terrain")`.
pub fn component(lua: &Lua, e: Entity, name: &str) -> Result<Option<Value>> {
    if name != "Terrain" || !with_world(lua, |w| w.scene.world.get::<&Terrain>(e).is_ok())? {
        return Ok(None);
    }
    Ok(Some(Value::UserData(lua.create_userdata(LTerrain(e))?)))
}

/// Register the `Terrain` global.
pub fn register(lua: &Lua) -> Result<()> {
    lua.globals().set("Terrain", LTerrainStatics)
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use crate::physics::Physics;
    use crate::runtime::Time;
    use crate::scene::{Scene, ScriptRef, Scripts, Terrain, Transform};
    use crate::scripting::{Input, LogKind, ScriptHost};
    use crate::terrain::{TerrainData, store};

    #[test]
    fn scripts_sample_the_active_terrain() {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("misty_terrain_script_{n}"));
        std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
        let src = r#"
local T = {}
function T:Start()
    local t = Terrain.activeTerrain
    local h = t:SampleHeight(Vector3.new(10, 99, 4))
    local same = self.gameObject:GetComponent("Terrain")
    Debug.Log(h, t.terrainData.size.x, t.terrainData.heightmapResolution,
        tostring(same.gameObject == t.gameObject), #Terrain.activeTerrains)
end
return T
"#;
        std::fs::write(root.join("Assets/Scripts/T.lua"), src).unwrap();
        // Rises 1 m per metre along x on a 33 m wide, 40 m tall terrain at x = 2.
        let mut d = TerrainData::new(33, Vec3::new(32.0, 40.0, 32.0), 32);
        for iz in 0..33 {
            for ix in 0..33 {
                d.set_height(ix, iz, ix as f32 / 40.0);
            }
        }
        let data_path = root.join("T.terrain").to_string_lossy().into_owned();
        store::insert(&data_path, d);

        let mut scene = Scene::new("T");
        let e = scene.spawn("Terrain", None);
        scene.set_transform(e, Transform::from_position(Vec3::new(2.0, 5.0, 0.0)));
        scene.add(
            e,
            Terrain {
                terrain_data: data_path,
                ..Default::default()
            },
        );
        scene.add(
            e,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Scripts/T.lua".into(),
                    ..Default::default()
                }],
            },
        );
        let physics = Physics::build(&scene, Vec3::ZERO);
        let mut host = ScriptHost::start(&mut scene, physics, root);
        let mut time = Time::default();
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
        let logs = host.take_logs();
        assert!(logs.iter().all(|(k, _)| *k != LogKind::Error), "{logs:?}");
        let texts: Vec<String> = logs.into_iter().map(|(_, t)| t).collect();
        assert_eq!(texts, ["8 32 33 true 1"], "{texts:?}");
    }

    /// A Terrain handle kept after `Destroy`: GetPosition raises a Lua error instead of
    /// panicking.
    #[test]
    fn terrain_of_a_destroyed_object_errors_in_lua() {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("misty_terrain_dead_{n}"));
        std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
        let src = r#"
local K = { frames = 0 }
function K:Start()
    local go = GameObject.Find("Terrain")
    self.t = go:GetComponent("Terrain")
    Destroy(go)
end
function K:Update()
    self.frames = self.frames + 1
    if self.frames == 3 then
        local t = self.t
        local ok, err = pcall(function() return t:GetPosition() end)
        Debug.Log("GetPosition", ok, string.find(tostring(err), "destroyed", 1, true) ~= nil)
    end
end
return K
"#;
        std::fs::write(root.join("Assets/Scripts/K.lua"), src).unwrap();
        let data_path = root.join("K.terrain").to_string_lossy().into_owned();
        store::insert(
            &data_path,
            TerrainData::new(33, Vec3::new(32.0, 40.0, 32.0), 32),
        );
        let mut scene = Scene::new("K");
        let e = scene.spawn("Terrain", None);
        scene.add(
            e,
            Terrain {
                terrain_data: data_path,
                ..Default::default()
            },
        );
        let k = scene.spawn("Keeper", None);
        scene.add(
            k,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Scripts/K.lua".into(),
                    ..Default::default()
                }],
            },
        );
        let physics = Physics::build(&scene, Vec3::ZERO);
        let mut host = ScriptHost::start(&mut scene, physics, root.clone());
        let mut time = Time::default();
        for _ in 0..5 {
            time.advance(1.0 / 60.0);
            host.update(&mut scene, &time, Input::default());
        }
        let texts: Vec<String> = host.take_logs().into_iter().map(|(_, t)| t).collect();
        assert!(texts.contains(&"GetPosition false true".to_owned()), "{texts:?}");
        let _ = std::fs::remove_dir_all(root);
    }
}
