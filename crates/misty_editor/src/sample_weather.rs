//! WeatherDemo sample scene in `SampleProject`: a country lane at the edge of an oak wood on
//! an overcast evening. A strip of asphalt (puddles) crosses the view, a timber shelter with
//! a pitched roof and a bench stands by a gravel path (dry under the roof), two street lamps
//! light the rain at night, boulders lie in the grass, and oaks, young oaks and birches (the
//! sample `Assets/Trees/*.tree` recipes, as terrain trees) close the view with a dark tree
//! line: rain shows against it. The ground is a small terrain (`WeatherDemo.terrain`: meadow
//! thatch with grass, a hand-painted gravel path). A global Weather volume shows Rain
//! (Preview in Edit Mode on); it is in manual, so its values in Properties are what you see
//! (its timeline holds the clear → light rain → thunderstorm → snow cycle, switched off). In
//! Play, keys switch the weather (`WeatherDemo.lua`): 1 Clear, 2 Drizzle, 3 Rain, 4 Downpour,
//! 5 Thunderstorm, 6 Snow, 7 Blizzard, 8 Sleet, L a lightning strike, N night / day, 0 back
//! to the Weather volume's own values.
//! Regenerate with `cargo test -p misty_editor -- --ignored write_weather_demo`; the other
//! tests check the committed files still match and that the scene plays.

use std::path::{Path, PathBuf};

use glam::{Vec2, Vec3};
use misty_engine::assets::Material;
use misty_engine::scene::{
    CloudPreset, Light, LightType, MeshFilter, MeshRenderer, PostProcessVolume, PrimitiveMesh,
    Scene, ScriptRef, Scripts, Shadows, Terrain, TerrainCollider, TonemapMode, Transform,
};
use misty_engine::terrain::auto_paint::{self, AutoPaintRules};
use misty_engine::terrain::brush::{self, Brush};
use misty_engine::terrain::trees::{TreeInstance, TreePrototype};
use misty_engine::terrain::{TerrainData, TerrainLayer, details, noise};
use misty_engine::weather::{Weather, WeatherPreset, WeatherTimeline};

const SCENE: &str = "Assets/Scenes/WeatherDemo.scene";
const DATA: &str = "Assets/Terrains/WeatherDemo.terrain";
const SCRIPT: &str = "Assets/Scripts/WeatherDemo.lua";
const ASPHALT: &str = "Assets/Materials/Weather Asphalt.mat";
const WOOD: &str = "Assets/Materials/Weather Wood.mat";
const ROOF: &str = "Assets/Materials/Weather Roof.mat";
const BARK: &str = "Assets/Materials/Weather Bark.mat";
const LEAVES: &str = "Assets/Materials/Weather Leaves.mat";
const STONE: &str = "Assets/Materials/Weather Stone.mat";
const IRON: &str = "Assets/Materials/Weather Iron.mat";
const LAMP: &str = "Assets/Materials/Weather Lamp.mat";
// Written by `sample_meadow` and `sample_terrain2`.
const GROUND: &str = "Assets/Textures/Meadow/MeadowThatch.png";
const GROUND_NORMAL: &str = "Assets/Textures/Meadow/MeadowThatchNormal.png";
const GROUND_HEIGHT: &str = "Assets/Textures/Meadow/MeadowThatchHeight.png";
const GRAVEL: &str = "Assets/Textures/Terrain/Scree.png";
const TIMBER: &str = "Assets/Trees/Textures/OakBark.png";
const TIMBER_NORMAL: &str = "Assets/Trees/Textures/OakBarkNormal.png";
/// Terrain tree recipes written by `sample_meadow`.
const TREES: [&str; 3] = [
    "Assets/Trees/Oak.tree",
    "Assets/Trees/Young Oak.tree",
    "Assets/Trees/Birch.tree",
];

const SIZE: Vec3 = Vec3::new(256.0, 24.0, 256.0);
const CORNER: Vec3 = Vec3::new(-128.0, -6.0, -128.0);
/// Height of the lane and the clearing (world y = 0).
const LEVEL: f32 = 0.25;

const LUA: &str = r#"-- WeatherDemo: keys switch the weather (4 s blends), L strikes lightning, N turns day into
-- night and back (the sun goes under the horizon: the lamps and the lightning light the
-- rain), 0 hands the weather back to the Weather volume (the values in its Properties).
-- While a key's weather is held, Properties shows "Script override" with a Clear button.
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
    self.night = false
    local sun = GameObject.Find("Directional Light")
    if sun then
        self.sun = sun.transform
        self.day = self.sun.eulerAngles
    end
    Debug.Log("Weather keys: 1 Clear, 2 Drizzle, 3 Rain, 4 Downpour, 5 Thunderstorm, 6 Snow, 7 Blizzard, 8 Sleet, L lightning, N night / day, 0 back to the volume")
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
    if Input.GetKeyDown("n") and self.sun then
        self.night = not self.night
        if self.night then
            -- The sun 28 degrees under the horizon: the moon and the night sky take over.
            self.sun.eulerAngles = Vector3.new(-28, self.day.y, 0)
        else
            self.sun.eulerAngles = self.day
        end
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

fn materials() -> [(&'static str, Material); 8] {
    let m = |color: [f32; 4], metallic, smoothness| Material {
        color,
        metallic,
        smoothness,
        ..Default::default()
    };
    [
        (ASPHALT, m([0.2, 0.2, 0.21, 1.0], 0.0, 0.14)),
        (
            WOOD,
            Material {
                albedo_map: Some(TIMBER.into()),
                normal_map: Some(TIMBER_NORMAL.into()),
                normal_scale: 0.8,
                tiling: [1.0, 2.0],
                ..m([0.78, 0.6, 0.45, 1.0], 0.0, 0.2)
            },
        ),
        (
            // Roof boards: dark, fine-grained and nearly flat, so the wet roof shines evenly
            // instead of in blotches.
            ROOF,
            Material {
                albedo_map: Some(TIMBER.into()),
                normal_map: Some(TIMBER_NORMAL.into()),
                normal_scale: 0.2,
                tiling: [10.0, 5.0],
                ..m([0.42, 0.33, 0.27, 1.0], 0.0, 0.25)
            },
        ),
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

/// 0..1: how much a spot (world x, z) belongs to the levelled lane and clearing.
fn levelled(x: f32, z: f32) -> f32 {
    let smooth = |t: f32| {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    // The lane runs along x at z = -4; the clearing with the shelter lies north of it.
    let lane = 1.0 - smooth(((z + 4.0).abs() - 4.0) / 5.0);
    let clearing = 1.0 - smooth((Vec2::new(x, z - 4.0).length() - 9.0) / 9.0);
    lane.max(clearing)
}

/// Normalised height at terrain-local (x, z) metres: gentle swells, a rise under the wood
/// behind the shelter, level ground under the lane and the clearing.
fn height(x: f32, z: f32) -> f32 {
    let (wx, wz) = (x + CORNER.x, z + CORNER.z);
    let swells = noise::fbm(x, z, 46.0, 3, 21) * 0.5 + 0.5;
    let bumps = noise::fbm(x, z, 9.0, 2, 5) * 0.5 + 0.5;
    let rise = ((wz - 14.0) / 90.0).clamp(0.0, 1.0);
    let wild = LEVEL - 0.02 + swells * 0.07 + bumps * 0.012 + rise * rise * 0.3;
    let k = levelled(wx, wz);
    (wild + (LEVEL - wild) * k).clamp(0.0, 1.0)
}

fn grass() -> details::DetailSettings {
    details::DetailSettings {
        enabled: true,
        rules: AutoPaintRules {
            enabled: true,
            slope: [0.0, 26.0],
            spread: 0.25,
            breakup: 0.3,
            breakup_scale: 14.0,
            ..Default::default()
        },
        on_layer: Some(0),
        density: 420.0,
        draw_distance: 70.0,
        // Unmown verge grass, shin high, darker and lusher than the summer meadow.
        height: [0.1, 0.32],
        width: 0.045,
        healthy_color: [0.36, 0.5, 0.17],
        dry_color: [0.6, 0.56, 0.33],
        wind_strength: 0.3,
        wind_speed: 1.1,
        seed_heads: 0.05,
        seed_color: [0.7, 0.64, 0.45],
        clumping: 0.65,
        bend: 0.85,
        translucency: 0.5,
        tip_dryness: 0.35,
        lod_distance: 6.5,
        patch_scale: 22.0,
        lush_color: [0.18, 0.3, 0.09],
        lush_amount: 0.7,
        dry_amount: 0.35,
        color_variation: 0.5,
        height_variation: 0.7,
        density_variation: 0.3,
        ground_tint: 1.0,
        cast_shadows: true,
        wind_direction: 70.0,
        gust_strength: 0.7,
        gust_scale: 16.0,
        turbulence: 0.4,
    }
}

/// The gravel path (terrain-local points): from the lane past the shelter into the wood.
fn path_points() -> Vec<Vec2> {
    (0..=70)
        .map(|i| {
            let t = i as f32 / 70.0;
            let z = -2.0 + t * 46.0;
            let x = 0.5 + (t * 3.4).sin() * 3.2 * t;
            Vec2::new(x - CORNER.x, z - CORNER.z)
        })
        .collect()
}

/// Trees near the camera (world x, z, prototype, height scale): they frame the shelter.
const NEAR_TREES: [(f32, f32, u16, f32); 7] = [
    (-10.5, 9.0, 0, 0.95),
    (10.0, 12.5, 1, 1.05),
    (-5.5, 19.0, 2, 1.0),
    (15.5, 3.5, 2, 0.9),
    (-17.0, 2.5, 1, 0.92),
    (4.5, 24.0, 0, 1.05),
    (-15.5, 20.0, 1, 1.1),
];

/// The terrain's trees (deterministic): the ones round the clearing, then the wood behind
/// it and loose trees along the lane; none on the lane, the clearing or the path.
fn trees(d: &TerrainData) -> Vec<TreeInstance> {
    let size = Vec2::new(d.size.x, d.size.z);
    let corner = Vec2::new(CORNER.x, CORNER.z);
    let path = path_points();
    let mut placed: Vec<Vec2> = Vec::new();
    let mut out = Vec::new();
    let tree = |world: Vec2, prototype: u16, scale: f32, n: i32| {
        let r = |salt| noise::hash(n, salt, 0x3ea7);
        TreeInstance {
            position: (world - corner) / size,
            prototype,
            rotation: r(4) * std::f32::consts::TAU,
            height_scale: scale,
            width_scale: (0.9 + 0.2 * r(5)) * scale.powf(0.3),
            color: r(6),
        }
    };
    for (i, (x, z, kind, scale)) in NEAR_TREES.into_iter().enumerate() {
        placed.push(Vec2::new(x, z));
        out.push(tree(Vec2::new(x, z), kind, scale, 1000 + i as i32));
    }
    for n in 0..900 {
        let r = |salt| noise::hash(n, salt, 0x51d3);
        let p = Vec2::new(r(1) * 236.0 - 118.0, r(2) * 236.0 - 118.0);
        // The wood is thick behind the shelter, thin in front of the camera and to the sides.
        let behind = ((p.y - 16.0) / 30.0).clamp(0.0, 1.0);
        let keep = 0.06 + 0.8 * behind;
        if r(3) > keep || levelled(p.x, p.y) > 0.02 {
            continue;
        }
        // Clear of the path, the camera's spot and the other trees.
        let local = p - corner;
        if path.iter().any(|q| q.distance(local) < 4.0) || p.distance(Vec2::new(-1.5, -6.4)) < 14.0
        {
            continue;
        }
        let gap = 6.5 + 4.0 * r(7);
        if placed.iter().any(|q| q.distance(p) < gap) {
            continue;
        }
        if misty_engine::terrain::trees::slope_at(d, local.x, local.y) > 20.0 {
            continue;
        }
        let kind = match (r(8) * 10.0) as u32 {
            0..=4 => 0,
            5..=7 => 1,
            _ => 2,
        };
        placed.push(p);
        out.push(tree(p, kind, 0.8 + 0.4 * r(9), n));
    }
    out
}

fn terrain_data() -> TerrainData {
    let mut d = TerrainData::new(257, SIZE, 256);
    let s = d.spacing();
    for iz in 0..d.heightmap_resolution {
        for ix in 0..d.heightmap_resolution {
            d.set_height(ix, iz, height(ix as f32 * s.x, iz as f32 * s.y));
        }
    }
    d.layers = vec![
        TerrainLayer {
            name: "Verge".into(),
            diffuse: Some(GROUND.into()),
            normal_map: Some(GROUND_NORMAL.into()),
            height_map: Some(GROUND_HEIGHT.into()),
            normal_scale: 1.0,
            tile_size: [2.2, 2.2],
            smoothness: 0.05,
            rules: AutoPaintRules {
                enabled: true,
                slope: [0.0, 90.0],
                ..Default::default()
            },
            ..Default::default()
        },
        // Painted by hand: the path.
        TerrainLayer {
            name: "Gravel".into(),
            diffuse: Some(GRAVEL.into()),
            tile_size: [3.0, 3.0],
            smoothness: 0.1,
            ..Default::default()
        },
    ];
    d.rule_seed = 38;
    auto_paint::generate(&mut d);
    for p in path_points() {
        let b = Brush {
            center: p,
            size: 3.4,
            strength: 1.0,
            falloff: 0.5,
        };
        brush::paint(&mut d, &b, 1.0, 1);
    }
    // Gravel under the lane and on its shoulders, and a pad under the shelter: no grass there.
    for i in 0..=150 {
        let b = Brush {
            center: Vec2::new(-75.0 + i as f32 - CORNER.x, -4.0 - CORNER.z),
            size: 8.0,
            strength: 1.0,
            falloff: 0.35,
        };
        brush::paint(&mut d, &b, 1.0, 1);
    }
    let pad = Brush {
        center: Vec2::new(-CORNER.x, 4.0 - CORNER.z),
        size: 10.0,
        strength: 1.0,
        falloff: 0.4,
    };
    brush::paint(&mut d, &pad, 1.0, 1);
    d.details = grass();
    details::generate(&mut d);
    d.trees.prototypes = TREES
        .into_iter()
        .map(|path| TreePrototype { tree: path.into() })
        .collect();
    // Placed on the heights as stored (16 bits), so the file and the generator agree.
    let stored = TerrainData {
        heights: d
            .heights
            .iter()
            .map(|h| (h * 65535.0).round() / 65535.0)
            .collect(),
        ..d.clone()
    };
    d.trees.instances = trees(&stored);
    d
}

fn ground(d: &TerrainData, x: f32, z: f32) -> f32 {
    CORNER.y + d.sample_height(x - CORNER.x, z - CORNER.z)
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
    v.color_adjustments.post_exposure = 1.0;
    v.ambient_occlusion.active = true;
    let c = &mut v.clouds;
    c.active = true;
    CloudPreset::Cloudy.apply(c);
    c.coverage = 0.4;
    v
}

/// The demo's weather volume: global, Rain, previewed while editing. It starts in manual:
/// the values in Properties are what you see. The demo cycle's states are there, with the
/// timeline off: Timeline ▸ Mode ▸ Sequence runs them.
pub fn weather() -> Weather {
    let mut w = Weather::with_preset(WeatherPreset::Rain);
    w.timeline = WeatherTimeline::demo_cycle();
    w.timeline.mode = misty_engine::weather::TimelineMode::Off;
    w.state.wind_direction = 70.0;
    w.surfaces.wet_time = 25.0;
    w.surfaces.snow_time = 40.0;
    w.surfaces.melt_time = 60.0;
    w.surfaces.dry_time = 60.0;
    w.surfaces.puddles = 0.5;
    w.preview_in_edit_mode = true;
    w
}

/// A street lamp at world (x, z): post, arm, a glowing globe and its light.
fn lamp(s: &mut Scene, n: usize, x: f32, z: f32) {
    mesh(
        s,
        &format!("Lamp {n} Post"),
        PrimitiveMesh::Cylinder,
        at(Vec3::new(x, 2.0, z), Vec3::new(0.12, 2.0, 0.12)),
        IRON,
    );
    mesh(
        s,
        &format!("Lamp {n} Arm"),
        PrimitiveMesh::Cube,
        at(Vec3::new(x, 3.95, z - 0.45), Vec3::new(0.07, 0.07, 1.0)),
        IRON,
    );
    mesh(
        s,
        &format!("Lamp {n} Globe"),
        PrimitiveMesh::Sphere,
        at(Vec3::new(x, 3.8, z - 0.95), Vec3::splat(0.34)),
        LAMP,
    );
    let light = s.spawn(&format!("Lamp {n} Light"), None);
    s.set_transform(light, Transform::from_position(Vec3::new(x, 3.6, z - 0.95)));
    s.add(
        light,
        Light {
            kind: LightType::Point,
            color: [1.0, 0.78, 0.5],
            intensity: 3.0,
            range: 14.0,
            ..Light::directional()
        },
    );
}

/// The demo's rocks (the sample rock prefabs): a boulder by the first lamp, stones along
/// the verge and an outcrop at the edge of the wood.
fn rocks(d: &TerrainData) -> Vec<crate::sample_rocks::Placed> {
    use glam::Quat;
    // (rock index in `sample_rocks::rocks()`, x, z, scale, yaw, sink)
    const SPOTS: [(usize, f32, f32, f32, f32, f32); 8] = [
        (2, -6.2, 0.6, 0.55, 40.0, 0.25),
        (4, -4.9, 1.6, 1.0, 110.0, 0.05),
        (5, -7.4, 1.9, 1.0, 10.0, 0.02),
        (0, 11.5, 6.0, 0.45, 200.0, 0.3),
        (4, 9.9, 5.2, 1.2, 60.0, 0.05),
        (3, -13.0, 13.0, 0.5, 300.0, 0.35),
        (1, 7.0, -8.6, 0.35, 150.0, 0.2),
        (5, 5.6, -8.0, 1.0, 80.0, 0.02),
    ];
    SPOTS
        .into_iter()
        .map(
            |(rock, x, z, scale, yaw, sink)| crate::sample_rocks::Placed {
                rock,
                position: Vec3::new(x, ground(d, x, z) - sink * scale, z),
                rotation: Quat::from_rotation_y(yaw.to_radians()),
                scale,
            },
        )
        .collect()
}

fn demo_scene(d: &TerrainData) -> Scene {
    let mut s = Scene::default_3d("WeatherDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(-1.5, 1.7, -6.4))
                .with_euler(Vec3::new(2.0, 12.0, 0.0)),
        );
        s.world
            .get::<&mut misty_engine::scene::Camera>(cam)
            .unwrap()
            .far = 1500.0;
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        // Afternoon sun from the side (storms dim it; N in Play takes it under the horizon).
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 10.0, 0.0))
                .with_euler(Vec3::new(34.0, -60.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.color = [1.0, 0.9, 0.78];
        l.intensity = 2.4;
        l.shadows = Shadows::Soft;
    }
    let terrain = s.spawn("Terrain", None);
    s.set_transform(terrain, Transform::from_position(CORNER));
    s.add(
        terrain,
        Terrain {
            terrain_data: DATA.into(),
            ..Default::default()
        },
    );
    s.add(terrain, TerrainCollider::default());
    // The lane: 140 m of asphalt (wet, puddles, snow) a few centimetres above the verge.
    mesh(
        &mut s,
        "Lane",
        PrimitiveMesh::Cube,
        at(Vec3::new(0.0, -0.02, -4.0), Vec3::new(140.0, 0.16, 5.2)),
        ASPHALT,
    );
    // A shelter: posts and a pitched roof (dry under it), a bench inside.
    for (i, (x, z)) in [(-3.0, 2.0), (3.0, 2.0), (-3.0, 6.0), (3.0, 6.0)]
        .into_iter()
        .enumerate()
    {
        mesh(
            &mut s,
            &format!("Shelter Post {}", i + 1),
            PrimitiveMesh::Cube,
            at(Vec3::new(x, 1.3, z), Vec3::new(0.18, 2.6, 0.18)),
            WOOD,
        );
    }
    for (name, x) in [("Shelter Beam Left", -3.0), ("Shelter Beam Right", 3.0)] {
        mesh(
            &mut s,
            name,
            PrimitiveMesh::Cube,
            at(Vec3::new(x, 2.6, 4.0), Vec3::new(0.16, 0.2, 4.6)),
            WOOD,
        );
    }
    for (name, z, tilt) in [
        ("Shelter Roof Front", 2.55, -24.0),
        ("Shelter Roof Back", 5.45, 24.0),
    ] {
        let mut t =
            Transform::from_position(Vec3::new(0.0, 3.3, z)).with_euler(Vec3::new(tilt, 0.0, 0.0));
        t.scale = Vec3::new(7.4, 0.1, 3.5);
        mesh(&mut s, name, PrimitiveMesh::Cube, t, ROOF);
    }
    mesh(
        &mut s,
        "Bench Seat",
        PrimitiveMesh::Cube,
        at(Vec3::new(0.0, 0.48, 5.0), Vec3::new(3.0, 0.08, 0.5)),
        WOOD,
    );
    for (name, x) in [("Bench Leg Left", -1.3), ("Bench Leg Right", 1.3)] {
        mesh(
            &mut s,
            name,
            PrimitiveMesh::Cube,
            at(Vec3::new(x, 0.22, 5.0), Vec3::new(0.1, 0.44, 0.45)),
            WOOD,
        );
    }
    // Street lamps on the far side of the lane: rain sparkles in their light at night.
    lamp(&mut s, 1, -3.5, -0.6);
    lamp(&mut s, 2, 16.0, -0.6);
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
    crate::sample_rocks::add_to_scene(&mut s, &rocks(d));
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
    let d = terrain_data();
    d.save(&root.join(DATA)).unwrap();
    ensure_meta(&root.join(DATA));
    misty_engine::asset_db::install(misty_engine::asset_db::GuidDatabase::read(&root));
    demo_scene(&d).save(&root.join(SCENE)).unwrap();
    ensure_meta(&root.join(SCENE));
}

#[test]
fn weather_demo_matches_its_generator() {
    let root = project();
    for rel in [
        GROUND,
        GROUND_NORMAL,
        GROUND_HEIGHT,
        GRAVEL,
        TIMBER,
        TIMBER_NORMAL,
    ] {
        assert!(root.join(rel).exists(), "{rel}");
    }
    for rel in TREES {
        assert!(
            root.join(rel).exists(),
            "{rel} (written by write_meadow_demo)"
        );
    }
    for (path, m) in materials() {
        crate::test_util::assert_close(&Material::load(&root.join(path)).unwrap(), &m, path);
    }
    let lua = std::fs::read_to_string(root.join(SCRIPT)).unwrap();
    assert_eq!(lua.replace("\r\n", "\n"), LUA, "{SCRIPT}");
    let on_disk = TerrainData::load(&root.join(DATA)).unwrap();
    let generated = terrain_data();
    assert_eq!(on_disk.layers, generated.layers);
    assert_eq!(on_disk.details, generated.details);
    assert_eq!(on_disk.size, generated.size);
    for (a, b) in on_disk.heights.iter().zip(&generated.heights) {
        assert!((a - b).abs() < 3.0 / 65535.0);
    }
    assert_eq!(on_disk.trees.prototypes, generated.trees.prototypes);
    assert_eq!(
        on_disk.trees.instances.len(),
        generated.trees.instances.len()
    );
    for (i, (a, b)) in on_disk
        .trees
        .instances
        .iter()
        .zip(&generated.trees.instances)
        .enumerate()
    {
        assert!(
            a.position.distance(b.position) < 1e-6 && a.prototype == b.prototype,
            "tree {i}: {a:?} vs {b:?}"
        );
    }
    // A wood behind the shelter, the lane and the clearing left open.
    assert!(
        generated.trees.instances.len() > 60,
        "{} trees",
        generated.trees.instances.len()
    );
    for t in &generated.trees.instances {
        let p = t.position * Vec2::new(SIZE.x, SIZE.z) + Vec2::new(CORNER.x, CORNER.z);
        assert!((p.y + 4.0).abs() > 4.0, "a tree on the lane at {p}");
    }
    // The lane and the shelter's floor are level with the world's y = 0.
    for (x, z) in [(0.0, -4.0), (30.0, -4.0), (0.0, 4.0), (-1.5, -6.4)] {
        assert!(ground(&generated, x, z).abs() < 0.05, "ground at {x}, {z}");
    }
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    crate::test_util::assert_json_close(
        &scene.to_json(),
        &demo_scene(&generated).to_json(),
        "regenerate with write_weather_demo",
    );
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
    // The terrain and the rocks' collider meshes by absolute path (other tests move the
    // project root).
    let terrain = scene.find_by_name("Terrain").unwrap();
    scene
        .world
        .get::<&mut Terrain>(terrain)
        .unwrap()
        .terrain_data = root.join(DATA).to_string_lossy().into_owned();
    for f in scene.world.query::<&mut MeshFilter>().iter() {
        if let Some(misty_engine::scene::MeshRef::Asset(key)) = &mut f.mesh
            && key.starts_with("Assets/")
        {
            let abs = root.join(key.as_str()).to_string_lossy().into_owned();
            *key = abs;
        }
    }
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::ZERO);
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    let mut step = |scene: &mut Scene, host: &mut ScriptHost, key: Option<&str>| {
        time.advance(1.0 / 30.0);
        misty_engine::weather::update_scene(scene, time.delta, Vec3::new(-1.5, 1.7, -6.4));
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
    // Manual: the volume's own Rain, no timeline running.
    assert_eq!(scene.weather.current.nearest_preset(), WeatherPreset::Rain);
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
    assert!(scene.find_by_name("Shelter Roof Front").is_some());
}
