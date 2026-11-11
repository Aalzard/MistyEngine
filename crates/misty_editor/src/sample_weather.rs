//! WeatherDemo sample scene in `SampleProject`: a small square at dusk with wet asphalt, a
//! wooden shelter (no rain under its roof), trees, a stone wall, a street lamp, and a global
//! Weather volume looping clear → light rain → thunderstorm → snow → clear (Preview in Edit
//! Mode on). In Play, keys switch the weather (`WeatherDemo.lua`): 1 Clear, 2 Drizzle, 3 Rain,
//! 4 Downpour, 5 Thunderstorm, 6 Snow, 7 Blizzard, 8 Sleet, L a lightning strike, 0 back to
//! the timeline. Regenerate with `cargo test -p misty_editor -- --ignored write_weather_demo`;
//! the other tests check the committed files still match and that the scene plays.

use std::path::{Path, PathBuf};

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::scene::{
    CloudPreset, Light, LightType, MeshFilter, MeshRenderer, PostProcessVolume, PrimitiveMesh,
    Scene, ScriptRef, Scripts, Shadows, TonemapMode, Transform,
};
use misty_engine::weather::{Weather, WeatherPreset, WeatherTimeline};

const SCENE: &str = "Assets/Scenes/WeatherDemo.scene";
const SCRIPT: &str = "Assets/Scripts/WeatherDemo.lua";
const ASPHALT: &str = "Assets/Materials/Weather Asphalt.mat";
const WOOD: &str = "Assets/Materials/Weather Wood.mat";
const BARK: &str = "Assets/Materials/Weather Bark.mat";
const LEAVES: &str = "Assets/Materials/Weather Leaves.mat";
const STONE: &str = "Assets/Materials/Weather Stone.mat";
const IRON: &str = "Assets/Materials/Weather Iron.mat";
const LAMP: &str = "Assets/Materials/Weather Lamp.mat";

const LUA: &str = r#"-- WeatherDemo: keys switch the weather (4 s blends), L strikes lightning, 0 hands the
-- weather back to the Weather volume's timeline.
local Demo = {}

local keys = {
    ["1"] = "Clear",
    ["2"] = "Drizzle",
    ["3"] = "Rain",
    ["4"] = "Downpour",
    ["5"] = "Thunderstorm",
    ["6"] = "Snow",
    ["7"] = "Blizzard",
    ["8"] = "Sleet",
}

function Demo:Start()
    self.blend = self.blend or 4
    Debug.Log("Weather keys: 1 Clear, 2 Drizzle, 3 Rain, 4 Downpour, 5 Thunderstorm, 6 Snow, 7 Blizzard, 8 Sleet, L lightning, 0 timeline")
end

function Demo:Update()
    for key, state in pairs(keys) do
        if Input.GetKeyDown(key) then
            Weather.SetState(state, self.blend)
        end
    end
    if Input.GetKeyDown("l") then
        Weather.Strike()
    end
    if Input.GetKeyDown("0") then
        Weather.ClearState(self.blend)
    end
end

function Demo:OnWeatherChanged(state, seconds)
    Debug.Log("Weather: " .. state)
end

return Demo
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn materials() -> [(&'static str, Material); 7] {
    let m = |color: [f32; 4], metallic, smoothness| Material {
        color,
        metallic,
        smoothness,
        ..Default::default()
    };
    [
        (ASPHALT, m([0.2, 0.2, 0.21, 1.0], 0.0, 0.12)),
        (WOOD, m([0.42, 0.29, 0.18, 1.0], 0.0, 0.2)),
        (BARK, m([0.3, 0.23, 0.17, 1.0], 0.0, 0.1)),
        (LEAVES, m([0.17, 0.3, 0.12, 1.0], 0.0, 0.25)),
        (STONE, m([0.5, 0.48, 0.45, 1.0], 0.0, 0.15)),
        (IRON, m([0.12, 0.12, 0.13, 1.0], 0.8, 0.45)),
        (
            LAMP,
            Material {
                emission_enabled: true,
                emission: [1.0, 0.75, 0.45],
                emission_intensity: 4.0,
                ..m([1.0, 0.85, 0.6, 1.0], 0.0, 0.6)
            },
        ),
    ]
}

fn mesh(s: &mut Scene, name: &str, kind: PrimitiveMesh, t: Transform, material: &str) {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(kind));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
}

fn at(position: Vec3, scale: Vec3) -> Transform {
    Transform {
        position,
        scale,
        ..Default::default()
    }
}

fn post() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Aces;
    v.bloom.active = true;
    v.bloom.intensity = 0.6;
    v.bloom.threshold = 1.0;
    v.color_adjustments.active = true;
    v.color_adjustments.post_exposure = 0.5;
    let c = &mut v.clouds;
    c.active = true;
    CloudPreset::Cloudy.apply(c);
    c.coverage = 0.4;
    v
}

/// The demo's weather volume: global, the looping demo cycle, previewed while editing.
pub fn weather() -> Weather {
    let mut w = Weather::with_preset(WeatherPreset::LightRain);
    w.timeline = WeatherTimeline::demo_cycle();
    w.state.wind_direction = 70.0;
    w.surfaces.wet_time = 25.0;
    w.surfaces.snow_time = 40.0;
    w.surfaces.melt_time = 60.0;
    w.surfaces.dry_time = 60.0;
    w.surfaces.puddles = 0.5;
    w.preview_in_edit_mode = true;
    w
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("WeatherDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(-1.5, 1.7, -9.0))
                .with_euler(Vec3::new(4.0, 12.0, 0.0)),
        );
        s.world
            .get::<&mut misty_engine::scene::Camera>(cam)
            .unwrap()
            .far = 1500.0;
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        // Low evening sun from the side, so rain streaks catch the light.
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 10.0, 0.0))
                .with_euler(Vec3::new(18.0, -60.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.color = [1.0, 0.85, 0.7];
        l.intensity = 1.6;
        l.shadows = Shadows::Soft;
    }
    // Ground: 60 m of asphalt (wet, puddles, snow).
    mesh(
        &mut s,
        "Ground",
        PrimitiveMesh::Plane,
        at(Vec3::ZERO, Vec3::new(6.0, 1.0, 6.0)),
        ASPHALT,
    );
    // A shelter: posts and a roof (dry under it).
    for (i, (x, z)) in [(-3.0, 2.0), (3.0, 2.0), (-3.0, 6.0), (3.0, 6.0)]
        .into_iter()
        .enumerate()
    {
        mesh(
            &mut s,
            &format!("Shelter Post {}", i + 1),
            PrimitiveMesh::Cylinder,
            at(Vec3::new(x, 1.4, z), Vec3::new(0.2, 1.4, 0.2)),
            WOOD,
        );
    }
    mesh(
        &mut s,
        "Shelter Roof",
        PrimitiveMesh::Cube,
        at(Vec3::new(0.0, 2.9, 4.0), Vec3::new(7.2, 0.2, 5.2)),
        WOOD,
    );
    mesh(
        &mut s,
        "Bench",
        PrimitiveMesh::Cube,
        at(Vec3::new(0.0, 0.25, 5.0), Vec3::new(3.0, 0.5, 0.6)),
        WOOD,
    );
    // Trees.
    for (i, (x, z, h)) in [(-9.0, 7.0, 1.0), (8.5, 9.0, 1.2), (-6.0, 15.0, 1.1)]
        .into_iter()
        .enumerate()
    {
        mesh(
            &mut s,
            &format!("Tree {} Trunk", i + 1),
            PrimitiveMesh::Cylinder,
            at(Vec3::new(x, 1.5 * h, z), Vec3::new(0.45, 1.5 * h, 0.45)),
            BARK,
        );
        mesh(
            &mut s,
            &format!("Tree {} Crown", i + 1),
            PrimitiveMesh::Sphere,
            at(Vec3::new(x, 3.6 * h, z), Vec3::splat(4.0 * h)),
            LEAVES,
        );
    }
    // A low stone wall and a boulder (snow settles on their tops).
    mesh(
        &mut s,
        "Stone Wall",
        PrimitiveMesh::Cube,
        at(Vec3::new(6.0, 0.5, 2.0), Vec3::new(0.8, 1.0, 7.0)),
        STONE,
    );
    mesh(
        &mut s,
        "Boulder",
        PrimitiveMesh::Sphere,
        at(Vec3::new(-5.0, 0.3, 0.5), Vec3::new(1.8, 1.2, 1.5)),
        STONE,
    );
    // A street lamp: rain sparkles in its light at night.
    mesh(
        &mut s,
        "Lamp Post",
        PrimitiveMesh::Cylinder,
        at(Vec3::new(-3.5, 2.0, -2.0), Vec3::new(0.12, 2.0, 0.12)),
        IRON,
    );
    mesh(
        &mut s,
        "Lamp Globe",
        PrimitiveMesh::Sphere,
        at(Vec3::new(-3.5, 4.1, -2.0), Vec3::splat(0.4)),
        LAMP,
    );
    let lamp = s.spawn("Lamp Light", None);
    s.set_transform(lamp, Transform::from_position(Vec3::new(-3.5, 3.8, -2.0)));
    s.add(
        lamp,
        Light {
            kind: LightType::Point,
            color: [1.0, 0.78, 0.5],
            intensity: 3.0,
            range: 14.0,
            ..Light::directional()
        },
    );
    let w = s.spawn("Weather", None);
    s.add(w, weather());
    let keys = s.spawn("Weather Keys", None);
    s.add(
        keys,
        Scripts {
            list: vec![ScriptRef {
                path: SCRIPT.into(),
                ..Default::default()
            }],
        },
    );
    let fx = s.spawn("Global Volume", None);
    s.add(fx, post());
    s
}

/// A meta file with a new GUID next to `asset` (only when it has none).
fn ensure_meta(asset: &Path) {
    if misty_engine::asset_db::meta::read_guid(asset).is_none() {
        misty_engine::asset_db::meta::write_guid(asset, misty_engine::asset_db::Guid::new())
            .unwrap();
    }
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_weather_demo() {
    let root = project();
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
        ensure_meta(&root.join(path));
    }
    std::fs::write(root.join(SCRIPT), LUA).unwrap();
    ensure_meta(&root.join(SCRIPT));
    demo_scene().save(&root.join(SCENE)).unwrap();
    ensure_meta(&root.join(SCENE));
}

#[test]
fn weather_demo_matches_its_generator() {
    let root = project();
    for (path, m) in materials() {
        crate::test_util::assert_close(&Material::load(&root.join(path)).unwrap(), &m, path);
    }
    let lua = std::fs::read_to_string(root.join(SCRIPT)).unwrap();
    assert_eq!(lua.replace("\r\n", "\n"), LUA, "{SCRIPT}");
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_weather_demo",
    );
    let (scene, report) = Scene::from_json("WeatherDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let vols = misty_engine::weather::volumes(&scene);
    assert_eq!(vols.len(), 1);
    let names: Vec<_> = vols[0]
        .1
        .timeline
        .entries
        .iter()
        .map(|e| e.preset)
        .collect();
    assert_eq!(
        names,
        [
            WeatherPreset::Clear,
            WeatherPreset::LightRain,
            WeatherPreset::Thunderstorm,
            WeatherPreset::Snow
        ]
    );
}

/// Playing: the timeline runs, and keys switch the weather through the Lua API.
#[test]
fn weather_demo_plays_and_keys_switch_the_weather() {
    use misty_engine::runtime::Time;
    use misty_engine::scripting::{Input, ScriptHost};
    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::ZERO);
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    let mut step = |scene: &mut Scene, host: &mut ScriptHost, key: Option<&str>| {
        time.advance(1.0 / 30.0);
        misty_engine::weather::update_scene(scene, time.delta, Vec3::new(0.0, 1.7, -9.0));
        let mut input = Input::default();
        if let Some(k) = key {
            input.pressed.insert(k.into());
            input.held.insert(k.into());
        }
        host.update(scene, &time, input);
        let logs: Vec<_> = host
            .take_logs()
            .into_iter()
            .filter(|(k, _)| *k != misty_engine::scripting::LogKind::Info)
            .collect();
        assert!(logs.is_empty(), "{logs:?}");
    };
    for _ in 0..30 {
        step(&mut scene, &mut host, None);
    }
    // The timeline starts Clear.
    assert_eq!(scene.weather.current.rain_density, 0.0);
    step(&mut scene, &mut host, Some("4"));
    for _ in 0..150 {
        step(&mut scene, &mut host, None);
    }
    assert_eq!(
        scene.weather.current.nearest_preset(),
        WeatherPreset::Downpour
    );
    step(&mut scene, &mut host, Some("7"));
    for _ in 0..150 {
        step(&mut scene, &mut host, None);
    }
    assert_eq!(
        scene.weather.current.nearest_preset(),
        WeatherPreset::Blizzard
    );
    step(&mut scene, &mut host, Some("0"));
    for _ in 0..150 {
        step(&mut scene, &mut host, None);
    }
    assert!(!scene.weather.scripted());
    // Rain under the shelter roof is the renderer's job; here the roof is a shadow caster.
    assert!(scene.find_by_name("Shelter Roof").is_some());
}
