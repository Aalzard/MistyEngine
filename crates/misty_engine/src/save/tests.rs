//! Save system tests: a temp project played through `Runtime`, saved and loaded from Lua.

use std::path::PathBuf;

use glam::Vec3;
use serde_json::json;

use super::*;
use crate::audio::Audio;
use crate::runtime::{Runtime, Time};
use crate::scene::{BoxCollider, Light, Rigidbody, ScriptRef};
use crate::scripting::{Input, LogKind};
use crate::settings::ProjectSettings;

fn temp_root(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_save_{tag}_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    root
}

fn script(root: &std::path::Path, name: &str, src: &str) {
    std::fs::write(root.join("Assets/Scripts").join(name), src).unwrap();
}

fn attach(scene: &mut Scene, e: Entity, file: &str) {
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

const DRIVER: &str = r#"
local Driver = {}
function Driver:Update()
    local f = Time.frameCount
    if f == 1 then
        GameObject.Find("Mover").transform.position = Vector3.new(5, 1, 2)
        GameObject.Find("Lamp"):SetActive(false)
        Destroy(GameObject.Find("Doomed"))
        Instantiate("Assets/Prefabs/Coin.prefab", Vector3.new(1, 2, 3))
        GameObject.Find("Box"):GetComponent("Rigidbody").velocity = Vector3.new(0, 0, 4)
    elseif f == 2 then
        Debug.Log("save", tostring(SaveSystem.Save("a")), tostring(SaveSystem.Exists("a")))
    elseif f == 3 then
        GameObject.Find("Mover").transform.position = Vector3.new(-9, 0, 0)
        GameObject.Find("Lamp"):SetActive(true)
        Instantiate("Assets/Prefabs/Coin.prefab", Vector3.new(7, 7, 7))
        GameObject.Find("Box"):GetComponent("Rigidbody").velocity = Vector3.new(0, 0, -1)
    elseif f == 4 then
        Debug.Log("load", tostring(SaveSystem.Load("a")))
    end
end
return Driver
"#;

const COUNTER: &str = r#"
local Counter = { count = 0 }
function Counter:Update() self.count = self.count + 1 end
function Counter:OnSave(data)
    data.count = self.count
    data.tag = "c"
    data.pos = Vector3.new(1, 2, 3)
    data.list = { 1, 2, 3 }
    data.nested = { deep = { yes = true } }
    Debug.Log("saved", self.count)
end
function Counter:OnLoad(data)
    self.count = data.count
    Debug.Log("loaded", data.count, data.tag, math.floor(data.pos.y), #data.list, tostring(data.nested.deep.yes))
end
return Counter
"#;

const BOX: &str = r#"
local Box = {}
function Box:Start()
    local v = self.gameObject:GetComponent("Rigidbody").velocity.z
    Debug.Log("vel", math.floor(v + 0.5))
end
return Box
"#;

/// The test level: movable objects, a lamp, a doomed object, a counter and a moving box.
fn level(root: &std::path::Path) -> Scene {
    let mut s = Scene::new("Level");
    let driver = s.spawn("Driver", None);
    attach(&mut s, driver, "Driver.lua");
    for name in ["Mover", "Lamp", "Doomed", "Counter", "Box"] {
        let e = s.spawn(name, None);
        s.add(e, Saveable::default());
    }
    let lamp = s.find_by_name("Lamp").unwrap();
    s.add(lamp, Light::default());
    s.world.get::<&mut Saveable>(lamp).unwrap().components = vec!["Light.intensity".into()];
    let counter = s.find_by_name("Counter").unwrap();
    attach(&mut s, counter, "Counter.lua");
    let b = s.find_by_name("Box").unwrap();
    s.add(b, BoxCollider::default());
    s.add(
        b,
        Rigidbody {
            use_gravity: false,
            ..Default::default()
        },
    );
    attach(&mut s, b, "Box.lua");

    // A prefab with a Saveable, spawned at runtime.
    let mut asset = Scene::new("asset");
    let coin = asset.spawn("Coin", None);
    asset.add(coin, Saveable::default());
    crate::scene::prefab::Prefab::from_scene(&asset, coin)
        .save(&root.join("Assets/Prefabs/Coin.prefab"))
        .unwrap();
    s
}

fn play(root: &std::path::Path, scene: &mut Scene) -> Runtime {
    let (audio, _) = Audio::null(root.to_path_buf());
    Runtime::start_with_audio(
        scene,
        root.to_path_buf(),
        &ProjectSettings::default(),
        audio,
    )
}

#[test]
fn save_and_load_round_trip_through_a_reloaded_scene() {
    let root = temp_root("roundtrip");
    script(&root, "Driver.lua", DRIVER);
    script(&root, "Counter.lua", COUNTER);
    script(&root, "Box.lua", BOX);
    let rel = "Assets/Scenes/Level.scene";
    level(&root).save(&root.join(rel)).unwrap();
    let (mut scene, _) = Scene::load(&root.join(rel)).unwrap();
    scene.set_scene_path(0, rel);
    let mover_id = scene
        .world
        .get::<&Saveable>(scene.find_by_name("Mover").unwrap())
        .unwrap()
        .save_id
        .clone();

    let mut rt = play(&root, &mut scene);
    let mut time = Time::default();
    let mut logs = Vec::new();
    let mut frame = |rt: &mut Runtime, scene: &mut Scene| {
        time.advance(1.0 / 60.0);
        rt.update(scene, &time, Input::default());
        logs.extend(rt.take_logs());
    };
    frame(&mut rt, &mut scene); // 1: changes
    let lamp = scene.find_by_name("Lamp").unwrap();
    scene.world.get::<&mut Light>(lamp).unwrap().intensity = 3.0;
    frame(&mut rt, &mut scene); // 2: save
    scene.world.get::<&mut Light>(lamp).unwrap().intensity = 7.0;
    frame(&mut rt, &mut scene); // 3: more changes
    assert_eq!(
        scene
            .find_by_name("Lamp")
            .map(|l| scene.game_object(l).active),
        Some(true)
    );
    frame(&mut rt, &mut scene); // 4: load (scene reloaded after the frame)
    assert!(rt.take_scene_reloaded());
    frame(&mut rt, &mut scene); // 5: Start of the reloaded scene

    let problems: Vec<_> = logs.iter().filter(|(k, _)| *k != LogKind::Info).collect();
    assert!(problems.is_empty(), "{problems:?}");
    assert!(logs.iter().any(|(_, t)| t == "save true true"), "{logs:?}");
    assert!(logs.iter().any(|(_, t)| t == "load true"));

    let mover = scene.find_by_name("Mover").unwrap();
    assert_eq!(scene.transform(mover).position, Vec3::new(5.0, 1.0, 2.0));
    assert_eq!(
        scene.world.get::<&Saveable>(mover).unwrap().save_id,
        mover_id
    );
    let lamp = scene.find_by_name("Lamp").unwrap();
    assert!(!scene.game_object(lamp).active, "active state restored");
    assert_eq!(scene.world.get::<&Light>(lamp).unwrap().intensity, 3.0);
    assert!(scene.find_by_name("Doomed").is_none(), "stays destroyed");
    let coins: Vec<Entity> = scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| scene.name(*e) == "Coin(Clone)")
        .collect();
    assert_eq!(coins.len(), 1, "only the coin spawned before saving");
    assert_eq!(
        scene.world_position(coins[0]),
        Vec3::new(1.0, 2.0, 3.0),
        "spawned prefab recreated where it was"
    );
    // The counter's custom table came back through OnLoad.
    assert_eq!(
        logs.iter().filter(|(_, t)| t.starts_with("saved ")).count(),
        1
    );
    let count = logs
        .iter()
        .find_map(|(_, t)| t.strip_prefix("saved "))
        .unwrap()
        .to_owned();
    assert!(
        logs.iter()
            .any(|(_, t)| *t == format!("loaded {count} c 2 3 true")),
        "{logs:?}"
    );
    assert!(
        logs.iter().any(|(_, t)| t == "vel 4"),
        "rigidbody velocity restored: {logs:?}"
    );

    // The save is plain JSON with the expected parts.
    let folder = rt.save_folder();
    assert_eq!(folder, root.join(EDITOR_FOLDER));
    let v = read_slot_value(&folder, "a").unwrap();
    assert_eq!(v["format"], json!(FORMAT));
    assert_eq!(v["info"]["scene"], json!("Level"));
    assert_eq!(v["info"]["scene_path"], json!(rel));
    assert_eq!(v["destroyed"].as_array().unwrap().len(), 1);
    assert_eq!(
        v["spawned"][0]["prefab"],
        json!("Assets/Prefabs/Coin.prefab")
    );

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn slots_list_with_metadata_delete_and_compress() {
    let folder = temp_root("slots").join("saves");
    let mut a = SaveData {
        format: FORMAT,
        version: 3,
        info: SlotInfo {
            slot: "first".into(),
            time: 1_700_000_000,
            scene: "Level".into(),
            play_time: 75.0,
            version: 3,
            ..Default::default()
        },
        ..Default::default()
    };
    a.objects.insert(
        "id".into(),
        ObjectState {
            active: Some(false),
            ..Default::default()
        },
    );
    write_slot(&folder, "first", &a, false).unwrap();
    let mut b = a.clone();
    b.info.slot = "second/odd:name".into();
    b.info.time = 1_800_000_000;
    b.info.thumbnail = true;
    write_slot(&folder, "second/odd:name", &b, true).unwrap();
    std::fs::write(thumbnail_path(&folder, "second/odd:name"), b"png").unwrap();

    let bytes = std::fs::read(data_path(&folder, "second/odd:name")).unwrap();
    assert_eq!(&bytes[..2], &[0x1f, 0x8b], "compressed");
    assert!(data_path(&folder, "second/odd:name").ends_with("second_odd_name.save"));
    assert_eq!(read_slot(&folder, "second/odd:name", 3).unwrap(), b);

    let list = list_slots(&folder);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].slot, "second/odd:name", "newest first");
    assert!(list[0].thumbnail);
    assert_eq!(list[1].slot, "first");
    assert_eq!(list[1].scene, "Level");
    assert_eq!(list[1].play_time, 75.0);
    assert!(!list[1].thumbnail);
    assert!(list[1].size > 0);
    assert_eq!(format_time(1_700_000_000), "2023-11-14 22:13 UTC");
    assert_eq!(format_duration(75.0), "1:15");
    assert_eq!(format_duration(3725.0), "1:02:05");

    assert!(exists(&folder, "first"));
    assert!(delete_slot(&folder, "first"));
    assert!(!exists(&folder, "first"));
    assert!(!delete_slot(&folder, "first"));
    assert!(
        read_slot(&folder, "first", 3)
            .unwrap_err()
            .contains("no save")
    );
    assert_eq!(list_slots(&folder).len(), 1);
    std::fs::remove_dir_all(folder.parent().unwrap()).ok();
}

#[test]
fn migration_hooks_run_on_older_saves() {
    fn rename(save: &mut Value, from: u32) {
        if from < 2 {
            let gold = save["objects"]["p"]["scripts"]["s"]["coins"].take();
            save["objects"]["p"]["scripts"]["s"]["gold"] = gold;
        }
    }
    let mut v = json!({ "format": 1, "version": 1, "info": { "version": 1 },
        "objects": { "p": { "scripts": { "s": { "coins": 12 } } } } });
    assert!(migrate_with(&mut v, 2, &[rename]));
    assert_eq!(v["objects"]["p"]["scripts"]["s"]["gold"], json!(12));
    assert_eq!(version_of(&v), 2);
    assert_eq!(v["info"]["version"], json!(2));
    assert!(!migrate_with(&mut v, 2, &[rename]), "up to date");
    let data = from_value(v).unwrap();
    assert_eq!(data.version, 2);
    let newer = json!({ "format": FORMAT + 1 });
    assert!(from_value(newer).unwrap_err().contains("newer"));
}

/// `SaveSystem.OnMigrate(data, fromVersion)` updates a save written by an older game.
#[test]
fn lua_migration_hook_updates_old_saves() {
    let root = temp_root("luamigrate");
    script(
        &root,
        "Counter.lua",
        r#"
local Counter = { count = 0 }
function Counter:OnSave(data) data.count = 5 end
function Counter:OnLoad(data) Debug.Log("loaded", data.count, data.note) end
return Counter
"#,
    );
    script(
        &root,
        "Game.lua",
        r#"
local Game = {}
function Game:Awake()
    SaveSystem.version = self.v
    SaveSystem.OnMigrate = function(data, from)
        for _, o in pairs(data.objects) do
            if o.scripts then
                for _, s in pairs(o.scripts) do
                    s.count = s.count + 100
                    s.note = "from" .. from
                end
            end
        end
        return data
    end
end
function Game:Update()
    if Time.frameCount == 1 and self.v == 1 then SaveSystem.Save("m") end
    if Time.frameCount == 1 and self.v == 2 then SaveSystem.Load("m") end
end
return Game
"#,
    );
    let rel = "Assets/Scenes/M.scene";
    let mut s = Scene::new("M");
    let c = s.spawn("Counter", None);
    s.add(c, Saveable::default());
    attach(&mut s, c, "Counter.lua");
    let g = s.spawn("Game", None);
    attach(&mut s, g, "Game.lua");
    s.save(&root.join(rel)).unwrap();
    let mut logs = Vec::new();
    for v in [1.0, 2.0] {
        let (mut scene, _) = Scene::load(&root.join(rel)).unwrap();
        scene.set_scene_path(0, rel);
        let g = scene.find_by_name("Game").unwrap();
        scene.world.get::<&mut Scripts>(g).unwrap().list[0]
            .fields
            .insert("v".into(), crate::scene::ScriptValue::Number(v));
        let mut rt = play(&root, &mut scene);
        let mut time = Time::default();
        for _ in 0..3 {
            time.advance(1.0 / 60.0);
            rt.update(&mut scene, &time, Input::default());
            logs.extend(rt.take_logs());
        }
    }
    let problems: Vec<_> = logs.iter().filter(|(k, _)| *k != LogKind::Info).collect();
    assert!(problems.is_empty(), "{problems:?}");
    assert!(
        logs.iter().any(|(_, t)| t == "loaded 105 from1"),
        "{logs:?}"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn copies_and_prefab_instances_get_fresh_save_ids() {
    let mut s = Scene::new("t");
    let a = s.spawn("A", None);
    s.add(a, Saveable::default());
    let id = s.world.get::<&Saveable>(a).unwrap().save_id.clone();
    assert_eq!(id.len(), 32);
    let b = s.duplicate(a);
    let id_b = s.world.get::<&Saveable>(b).unwrap().save_id.clone();
    assert_ne!(id, id_b, "duplicate regenerates");
    // Copies made another way (paste, presets) are fixed by the duplicate check.
    let c = s.spawn("C", None);
    s.add(
        c,
        Saveable {
            save_id: id.clone(),
            ..Default::default()
        },
    );
    assert_eq!(fix_duplicate_ids(&mut s), 1);
    assert_eq!(
        s.world.get::<&Saveable>(a).unwrap().save_id,
        id,
        "first keeps it"
    );
    assert_ne!(s.world.get::<&Saveable>(c).unwrap().save_id, id);
    assert_eq!(fix_duplicate_ids(&mut s), 0);

    let prefab = crate::scene::prefab::Prefab::from_scene(&s, a);
    let i1 = s.instantiate_prefab(&prefab, Some("Assets/A.prefab"), None);
    let i2 = s.instantiate_prefab(&prefab, Some("Assets/A.prefab"), None);
    let sid = |e| s.world.get::<&Saveable>(e).unwrap().save_id.clone();
    assert_ne!(sid(i1), sid(i2));
    assert!(
        !s.has_prefab_overrides(i1, &prefab),
        "Save IDs are not overrides"
    );
}

#[test]
fn component_fields_round_trip() {
    let mut s = Scene::new("t");
    let e = s.spawn("L", None);
    s.add(e, Light::default());
    s.world.get::<&mut Light>(e).unwrap().intensity = 2.5;
    let v = read_component_field(&s, e, "Light.intensity").unwrap();
    assert_eq!(v, json!(2.5));
    let whole = read_component_field(&s, e, "Light").unwrap();
    s.world.get::<&mut Light>(e).unwrap().intensity = 9.0;
    assert!(write_component_field(&mut s, e, "Light", whole));
    assert_eq!(s.world.get::<&Light>(e).unwrap().intensity, 2.5);
    assert!(write_component_field(&mut s, e, "Light.range", json!(4.0)));
    assert_eq!(s.world.get::<&Light>(e).unwrap().range, 4.0);
    assert!(
        !write_component_field(&mut s, e, "Camera", json!({})),
        "missing component"
    );
    assert!(read_component_field(&s, e, "Nope").is_none());
}
