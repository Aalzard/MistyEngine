//! Lua Tilemap and Light2D API tests.

use std::path::PathBuf;

use glam::{IVec2, Vec3};

use crate::light2d::Light2D;
use crate::physics::Physics;
use crate::runtime::Time;
use crate::scene::{Scene, ScriptRef, Scripts, Transform};
use crate::scripting::{Input, LogKind, ScriptHost};
use crate::tilemap::{Grid, Tilemap, TilemapRenderer};

const SCRIPT: &str = r#"
local Painter = {}
function Painter:Start()
    local map = self.gameObject:GetComponent("Tilemap")
    map:SetTile(2, 3, "Assets/Tiles/Ground.tile")
    map:SetTile(Vector3.new(-1, 0, 0), "Assets/Tiles/Water.tile")
    Debug.Log("get", map:GetTile(2, 3), map:GetTile(Vector3.new(-1, 0, 0)), map:GetTile(9, 9) == nil)
    Debug.Log("has", map:HasTile(2, 3), map:HasTile(0, 0), map:GetUsedTilesCount())
    local c = map:WorldToCell(Vector3.new(12.5, 3.2, 0))
    Debug.Log("cell", c.x, c.y)
    local w = map:CellToWorld(2, 3)
    local m = map:GetCellCenterWorld(2, 3)
    Debug.Log("world", w.x, w.y, m.x, m.y)
    map:SetTile(2, 3, nil)
    Debug.Log("erased", map:HasTile(2, 3), map:GetUsedTilesCount())
    map:ClearAllTiles()
    Debug.Log("cleared", map:GetUsedTilesCount())
    map:SetTile(5, 5, "Assets/Tiles/Ground.tile")
    local light = GameObject.Find("Torch"):GetComponent("Light2D")
    light.intensity = 2.5
    light.color = Color.new(1, 0.5, 0, 1)
    Debug.Log("light", light.intensity, light.pointLightOuterRadius, light.enabled)
end
return Painter
"#;

fn project() -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_tilemap_script_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts/Painter.lua"), SCRIPT).unwrap();
    root
}

#[test]
fn lua_paints_tiles_and_drives_2d_lights() {
    let root = project();
    let mut scene = Scene::new("Lua Tiles");
    let grid = scene.spawn("Grid", None);
    scene.add(grid, Grid::default());
    scene.set_transform(grid, Transform::from_position(Vec3::new(10.0, 0.0, 0.0)));
    let tm = scene.spawn("Tilemap", Some(grid));
    scene.add(tm, Tilemap::default());
    scene.add(tm, TilemapRenderer::default());
    scene.add(
        tm,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Painter.lua".into(),
                ..Default::default()
            }],
        },
    );
    let torch = scene.spawn("Torch", None);
    scene.add(torch, Light2D::default());

    let physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    time.advance(1.0 / 60.0);
    host.update(&mut scene, &time, Input::default());
    let logs = host.take_logs();
    for (kind, text) in &logs {
        assert_ne!(*kind, LogKind::Error, "{text}");
    }
    let text: Vec<String> = logs.into_iter().map(|(_, t)| t).collect();
    let expect = [
        "get Assets/Tiles/Ground.tile Assets/Tiles/Water.tile true",
        "has true false 2",
        "cell 2 3",
        "world 12 3 12.5 3.5",
        "erased false 1",
        "cleared 0",
        "light 2.5 5 true",
    ];
    for e in expect {
        assert!(text.iter().any(|t| t == e), "missing {e:?} in {text:?}");
    }
    let map = scene.world.get::<&Tilemap>(tm).unwrap();
    assert_eq!(
        map.tile_at(IVec2::new(5, 5)),
        Some("Assets/Tiles/Ground.tile")
    );
    let l = scene.world.get::<&Light2D>(torch).unwrap();
    assert_eq!((l.intensity, l.color), (2.5, [1.0, 0.5, 0.0]));
}
