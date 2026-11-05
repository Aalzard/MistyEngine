//! `Assets` Lua API: LoadAsync callbacks, Await in Assets.Run, InstantiateAsync and Release.

use std::path::PathBuf;

use glam::Vec3;

use super::*;
use crate::asset_groups::{AssetEntry, AssetGroupSettings, AssetSystem, Provider};
use crate::physics::Physics;
use crate::scene::ScriptRef;

fn project(script: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_assets_lua_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts/Loader.lua"), script).unwrap();
    let mut asset = Scene::new("asset");
    let ghost = asset.spawn("Ghost", None);
    asset.spawn("Sheet", Some(ghost));
    crate::scene::prefab::Prefab::from_scene(&asset, ghost)
        .save(&root.join("Assets/Prefabs/Ghost.prefab"))
        .unwrap();
    let mut s = AssetGroupSettings::default();
    s.groups[0].entries.push(AssetEntry {
        guid: String::new(),
        path: "Assets/Prefabs/Ghost.prefab".into(),
        address: "Enemies/Ghost".into(),
        labels: vec!["spooky".into()],
    });
    s.save(&root).unwrap();
    root
}

fn host(script: &str) -> (ScriptHost, Scene) {
    let root = project(script);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Loader", None);
    scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Loader.lua".into(),
                enabled: true,
                fields: Default::default(),
            }],
        },
    );
    let physics = Physics::build(&scene, Vec3::ZERO);
    let host = ScriptHost::start(&mut scene, physics, root);
    (host, scene)
}

fn frames(host: &mut ScriptHost, scene: &mut Scene, n: u32) -> Vec<String> {
    let mut time = Time::default();
    for _ in 0..n {
        time.advance(1.0 / 60.0);
        host.update(scene, &time, Input::default());
    }
    host.take_logs().into_iter().map(|(_, t)| t).collect()
}

#[test]
fn load_async_callbacks_and_await_in_coroutines() {
    let script = r#"
local Loader = {}
function Loader:Start()
    local h = Assets.LoadAsync("Enemies/Ghost")
    Debug.Log("done at start", h.isDone, h.status)
    h:OnCompleted(function(h) Debug.Log("callback", h.status, h.result) end)
    Assets.Run(function()
        local all = Assets.LoadAsync("spooky"):Await()
        Debug.Log("awaited", #all.results)
        local inst = Assets.InstantiateAsync("Enemies/Ghost", self.transform):Await()
        Debug.Log("instance", inst.result.name, inst.result.transform.parent.name)
        local s = Assets.GetStats()
        Debug.Log("stats", s.loaded, s.handles)
        Assets.Release(inst)
        Assets.Release(all)
        Assets.Release(h)
        s = Assets.GetStats()
        Debug.Log("after", s.released, s.handles)
    end)
    local bad = Assets.LoadAsync("Nope")
    bad:OnCompleted(function(b) Debug.Log("bad", b.status) end)
    local ok, err = pcall(function() Assets.LoadAsync("spooky"):Await() end)
    Debug.Log("main await", ok)
end
return Loader
"#;
    let (mut host, mut scene) = host(script);
    let logs = frames(&mut host, &mut scene, 10);
    let has = |s: &str| logs.iter().any(|l| l == s);
    assert!(has("done at start false Loading"), "{logs:?}");
    assert!(
        has("callback Succeeded Assets/Prefabs/Ghost.prefab"),
        "{logs:?}"
    );
    assert!(has("awaited 1"), "{logs:?}");
    assert!(has("instance Ghost(Clone) Loader"), "{logs:?}");
    assert!(has("stats 4 5"), "{logs:?}");
    assert!(has("after 3 2"), "{logs:?}");
    assert!(has("bad Failed"), "{logs:?}");
    assert!(has("main await false"), "{logs:?}");
    // Releasing the instance handle destroyed the object.
    assert!(scene.find_by_name("Ghost(Clone)").is_none());
}

#[test]
fn release_by_object_and_editor_provider() {
    let script = r#"
local Loader = {}
function Loader:Start()
    Assets.InstantiateAsync("Enemies/Ghost"):OnCompleted(function(h)
        self.ghost = h.result
    end)
end
function Loader:Update()
    if self.ghost and not self.released then
        self.released = Assets.Release(self.ghost)
        Debug.Log("released", self.released, Assets.GetStats().handles)
    end
end
return Loader
"#;
    let (mut host, mut scene) = host(script);
    let root = host.root.clone();
    let settings = AssetGroupSettings::load(&root).unwrap();
    host.set_assets(AssetSystem::new(&root, Provider::Database(settings)));
    let logs = frames(&mut host, &mut scene, 6);
    assert!(logs.iter().any(|l| l == "released true 0"), "{logs:?}");
    assert!(scene.find_by_name("Ghost(Clone)").is_none());
    assert_eq!(host.with_assets(|s| s.stats().released), Some(1));
}
