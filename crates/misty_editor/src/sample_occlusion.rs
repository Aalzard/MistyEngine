//! OcclusionDemo sample scene in `SampleProject`: a 5 × 5 city of walled blocks (10 m walls, a few
//! with gates) with statues and crates in every courtyard, parked cars in the streets, a few
//! floating drones (dynamic, culled by the GPU Hi-Z pass) and an Occlusion Area over the streets.
//! The Main Camera tours the streets (`OcclusionTour.lua`); the Game view's Stats show how many
//! objects the baked occlusion data culls. Regenerate (scene, script and baked data) with
//! `cargo test -p misty_editor -- --ignored write_occlusion_demo`; the other tests check the
//! committed files still match and that culling cuts the draw count.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::occlusion::OcclusionSettings;
use misty_engine::scene::{
    MeshFilter, MeshRenderer, OcclusionArea, PrimitiveMesh, Scene, ScriptRef, ScriptValue, Scripts,
    Shadows, StaticFlags, Transform,
};

const SCENE: &str = "Assets/Scenes/OcclusionDemo.scene";
const TOUR: &str = "Assets/Scripts/OcclusionTour.lua";
const BOBBER: &str = "Assets/Scripts/VerticalBobber.lua";
// Existing sample materials.
const WALL: &str = "Assets/Materials/AO Concrete.mat";
const PLASTER: &str = "Assets/Materials/AO Plaster.mat";
const ROAD: &str = "Assets/Materials/Spline Asphalt.mat";
const GOLD: &str = "Assets/Materials/Stress Gold.mat";
const TEAL: &str = "Assets/Materials/Stress Teal.mat";
const CORAL: &str = "Assets/Materials/Stress Coral.mat";
const CRATE: &str = "Assets/Materials/AO Crate.mat";
const CAR: &str = "Assets/Materials/Car Paint.mat";
const ORB: &str = "Assets/Materials/Spline Orb.mat";

/// Blocks per side, block size, street width (metres).
const BLOCKS: i32 = 5;
const BLOCK: f32 = 18.0;
const STREET: f32 = 8.0;
const WALL_HEIGHT: f32 = 10.0;
const WALL_THICKNESS: f32 = 0.6;

const TOUR_SRC: &str = r#"-- OcclusionTour: drives the camera around the city's streets, looking ahead.
-- O toggles the camera's occlusion culling (compare the Stats).
local OcclusionTour = {
    speed = 7,          -- metres per second
    height = 1.8,
    lookAhead = 8,      -- metres ahead the camera looks
}

-- Corners of the loop, on street centre lines (x, z).
local points = {
    {-13, -65}, {-13, 13}, {39, 13}, {39, -39},
    {-39, -39}, {-39, 39}, {13, 39}, {13, -65},
}

function OcclusionTour:Start()
    self.lengths = {}
    self.total = 0
    for i = 1, #points do
        local a, b = points[i], points[i % #points + 1]
        local len = math.abs(b[1] - a[1]) + math.abs(b[2] - a[2])
        self.lengths[i] = len
        self.total = self.total + len
    end
    self.distance = 0
    self.camera = self.gameObject:GetComponent("Camera")
end

-- Point at distance `d` along the loop.
function OcclusionTour:At(d)
    d = d % self.total
    for i = 1, #points do
        local len = self.lengths[i]
        if d <= len then
            local a, b = points[i], points[i % #points + 1]
            local t = d / len
            return Vector3.new(a[1] + (b[1] - a[1]) * t, self.height, a[2] + (b[2] - a[2]) * t)
        end
        d = d - len
    end
    return Vector3.new(points[1][1], self.height, points[1][2])
end

function OcclusionTour:Update()
    self.distance = self.distance + self.speed * Time.deltaTime
    self.transform.position = self:At(self.distance)
    self.transform:LookAt(self:At(self.distance + self.lookAhead))
    if Input.GetKeyDown("o") and Camera.main then
        local cam = Camera.main
        cam.useOcclusionCulling = not cam.useOcclusionCulling
        Debug.Log("Occlusion culling: " .. tostring(cam.useOcclusionCulling))
    end
end

return OcclusionTour
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Deterministic 0..1 value per integer pair.
fn hash(x: i32, z: i32) -> f32 {
    let n = (x.wrapping_mul(73_856_093) ^ z.wrapping_mul(19_349_663)) as u32;
    (n.wrapping_mul(2_654_435_761) >> 8) as f32 / (1u32 << 24) as f32
}

fn scaled(position: Vec3, scale: Vec3) -> Transform {
    Transform {
        position,
        scale,
        ..Default::default()
    }
}

/// A static renderer (Occluder and Occludee Static unless `occluder` is false).
fn part(
    s: &mut Scene,
    name: &str,
    parent: Entity,
    kind: PrimitiveMesh,
    t: Transform,
    material: &str,
    occluder: bool,
) -> Entity {
    let e = s.spawn(name, Some(parent));
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(kind));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
    let go = &mut *s.game_object_mut(e);
    go.is_static = true;
    if !occluder {
        go.static_flags = StaticFlags::OCCLUDEE | StaticFlags::BATCHING;
    }
    e
}

/// Centre of block (i, j) on the ground.
fn block_center(i: i32, j: i32) -> Vec3 {
    let pitch = BLOCK + STREET;
    let off = (BLOCKS - 1) as f32 * 0.5;
    Vec3::new((i as f32 - off) * pitch, 0.0, (j as f32 - off) * pitch)
}

/// Four walls (one side may have a 3 m gate) and a courtyard of statues and crates.
fn block(s: &mut Scene, parent: Entity, i: i32, j: i32) {
    let c = block_center(i, j);
    let b = s.spawn(
        &format!("Block {}{}", (b'A' + i as u8) as char, j + 1),
        Some(parent),
    );
    s.set_transform(b, Transform::from_position(c));
    let (h, t, w) = (WALL_HEIGHT, WALL_THICKNESS, BLOCK);
    let y = h * 0.5;
    let material = if (i + j) % 2 == 0 { WALL } else { PLASTER };
    // Side 0..4: north, east, south, west. Some blocks get a gate on one side.
    let gate = (hash(i, j) < 0.45).then(|| (hash(j + 7, i + 3) * 4.0) as usize % 4);
    for side in 0..4 {
        let along_x = side % 2 == 0;
        let sign = if side < 2 { 1.0 } else { -1.0 };
        let normal = if along_x { Vec3::Z } else { Vec3::X } * sign;
        let center = normal * (w - t) * 0.5 + Vec3::Y * y;
        let size = |len: f32| match along_x {
            true => Vec3::new(len, h, t),
            false => Vec3::new(t, h, len),
        };
        let axis = if along_x { Vec3::X } else { Vec3::Z };
        if gate == Some(side) {
            let gap = 3.0;
            let piece = (w - gap) * 0.5;
            let off = axis * (gap + piece) * 0.5;
            for (k, d) in [(0, -1.0), (1, 1.0)] {
                let name = format!("Wall {side}{}", ["a", "b"][k]);
                let t = scaled(center + off * d, size(piece));
                part(s, &name, b, PrimitiveMesh::Cube, t, material, true);
            }
        } else {
            let t = scaled(center, size(w));
            part(
                s,
                &format!("Wall {side}"),
                b,
                PrimitiveMesh::Cube,
                t,
                material,
                true,
            );
        }
    }
    // Courtyard: a 3 × 3 grid of statues, pillars and crates.
    for gx in -1..=1 {
        for gz in -1..=1 {
            let p = Vec3::new(gx as f32 * 5.0, 0.0, gz as f32 * 5.0);
            let r = hash(i * 7 + gx + 11, j * 5 + gz + 3);
            let (kind, t, material, name) = match (r * 3.0) as i32 {
                0 => (
                    PrimitiveMesh::Sphere,
                    scaled(p + Vec3::Y * 1.0, Vec3::splat(2.0)),
                    GOLD,
                    "Statue",
                ),
                1 => (
                    PrimitiveMesh::Cylinder,
                    scaled(p + Vec3::Y * 1.5, Vec3::new(1.0, 1.5, 1.0)),
                    TEAL,
                    "Pillar",
                ),
                _ => (
                    PrimitiveMesh::Cube,
                    scaled(p + Vec3::Y * 0.6, Vec3::splat(1.2)),
                    if r > 0.85 { CORAL } else { CRATE },
                    "Crate",
                ),
            };
            part(s, name, b, kind, t, material, false);
        }
    }
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("OcclusionDemo");
    let start = Vec3::new(-13.0, 1.8, -65.0);
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(cam, Transform::from_position(start));
        s.add(
            cam,
            Scripts {
                list: vec![ScriptRef {
                    path: TOUR.into(),
                    ..Default::default()
                }],
            },
        );
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 30.0, 0.0))
                .with_euler(Vec3::new(55.0, -35.0, 0.0)),
        );
        let mut l = s.world.get::<&mut misty_engine::scene::Light>(sun).unwrap();
        l.shadows = Shadows::Hard;
    }
    let extent = BLOCKS as f32 * (BLOCK + STREET) + STREET;
    let ground = s.spawn("Ground", None);
    s.set_transform(
        ground,
        scaled(Vec3::ZERO, Vec3::new(extent / 10.0, 1.0, extent / 10.0)),
    );
    s.add(ground, MeshFilter::primitive(PrimitiveMesh::Plane));
    s.add(
        ground,
        MeshRenderer {
            material: Some(ROAD.into()),
            ..Default::default()
        },
    );
    s.game_object_mut(ground).is_static = true;

    let city = s.spawn("City", None);
    for i in 0..BLOCKS {
        for j in 0..BLOCKS {
            block(&mut s, city, i, j);
        }
    }

    // Parked cars on some street segments (between neighbouring blocks).
    let props = s.spawn("Parked Cars", None);
    let pitch = BLOCK + STREET;
    for line in 0..=BLOCKS {
        for seg in 0..BLOCKS {
            for along_x in [true, false] {
                let r = hash(line * 13 + 5, seg * 17 + if along_x { 1 } else { 2 });
                if r > 0.6 {
                    continue;
                }
                let off = (BLOCKS as f32) * 0.5;
                let street = (line as f32 - off) * pitch;
                let mid = (seg as f32 - (BLOCKS - 1) as f32 * 0.5) * pitch + (r - 0.3) * 8.0;
                let side = if r < 0.3 { -2.4 } else { 2.4 };
                let (p, size) = match along_x {
                    true => (Vec3::new(mid, 0.7, street + side), Vec3::new(4.0, 1.4, 1.8)),
                    false => (Vec3::new(street + side, 0.7, mid), Vec3::new(1.8, 1.4, 4.0)),
                };
                let t = scaled(p, size);
                part(&mut s, "Car", props, PrimitiveMesh::Cube, t, CAR, false);
            }
        }
    }

    // Floating drones in a few courtyards: dynamic, so only the GPU Hi-Z pass can cull them.
    let drones = s.spawn("Drones", None);
    for (k, (i, j)) in [(0, 0), (2, 1), (4, 3), (1, 4), (3, 2)]
        .into_iter()
        .enumerate()
    {
        let p = block_center(i, j) + Vec3::new(2.5, 3.0, -2.5);
        let e = s.spawn("Drone", Some(drones));
        s.set_transform(e, scaled(p, Vec3::splat(1.2)));
        s.add(e, MeshFilter::primitive(PrimitiveMesh::Sphere));
        s.add(
            e,
            MeshRenderer {
                material: Some(ORB.into()),
                ..Default::default()
            },
        );
        let mut script = ScriptRef {
            path: BOBBER.into(),
            ..Default::default()
        };
        script
            .fields
            .insert("speed".into(), ScriptValue::Number(0.6 + k as f64 * 0.15));
        s.add(e, Scripts { list: vec![script] });
    }

    // Where the camera can be: the streets and courtyards, up to 4 m.
    let area = s.spawn("Occlusion Area", None);
    s.set_transform(area, Transform::from_position(Vec3::new(0.0, 2.0, 0.0)));
    s.add(
        area,
        OcclusionArea {
            size: [extent, 3.6, extent],
            ..Default::default()
        },
    );
    let o = &mut s.render_settings.occlusion;
    o.smallest_occluder = 4.0;
    o.smallest_hole = 0.5;
    o.data = Some(OcclusionSettings::asset_path_for(SCENE));
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_occlusion_demo() {
    use misty_engine::occlusion::bake::{BakeInput, Progress, bake};
    let root = project();
    let scene = demo_scene();
    scene.save(&root.join(SCENE)).unwrap();
    std::fs::write(root.join(TOUR), TOUR_SRC).unwrap();
    let started = std::time::Instant::now();
    let data = bake(&BakeInput::from_scene(&scene), &Progress::default()).unwrap();
    let rel = scene.render_settings.occlusion.data.clone().unwrap();
    data.save(&root.join(rel)).unwrap();
    eprintln!(
        "baked {} cells, {} sets in {:.1} s",
        data.cell_count(),
        data.set_count(),
        started.elapsed().as_secs_f32()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use misty_engine::occlusion::runtime::OccludeeMap;
    use misty_engine::occlusion::{CellLookup, OcclusionData};

    fn committed() -> (Scene, OcclusionData) {
        let root = project();
        let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
        let (scene, report) = Scene::from_json("OcclusionDemo", &text).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let rel = scene.render_settings.occlusion.data.clone().unwrap();
        let data = OcclusionData::load(&root.join(rel)).unwrap();
        (scene, data)
    }

    #[test]
    fn occlusion_demo_matches_its_generator() {
        let root = project();
        let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
        assert!(
            text.replace("\r\n", "\n") == demo_scene().to_json(),
            "regenerate with write_occlusion_demo"
        );
        let src = std::fs::read_to_string(root.join(TOUR)).unwrap();
        assert_eq!(src.replace("\r\n", "\n"), TOUR_SRC);
        // The baked data belongs to this scene: every occludee matches.
        let (scene, data) = committed();
        let map = OccludeeMap::build(&scene, &data);
        assert_eq!(map.stale, 0);
        assert_eq!(map.map.len(), data.occludees.len());
        assert!(data.occludees.len() > 300, "{}", data.occludees.len());
        let p = &scene.render_settings.occlusion;
        assert_eq!(
            data.params,
            [p.smallest_occluder, p.smallest_hole, p.backface_threshold]
        );
    }

    /// Along the tour most occludees are culled; in the streets nothing is inside geometry.
    #[test]
    fn the_tour_culls_most_of_the_city() {
        let (_, data) = committed();
        let corners = [
            (-13.0, -65.0),
            (-13.0, 13.0),
            (39.0, 13.0),
            (39.0, -39.0),
            (-39.0, -39.0),
            (-39.0, 39.0),
            (13.0, 39.0),
            (13.0, -65.0),
        ];
        let total = data.occludees.len();
        let mut fractions = Vec::new();
        for k in 0..corners.len() {
            let (a, b) = (corners[k], corners[(k + 1) % corners.len()]);
            for step in 0..5 {
                let t = step as f32 / 5.0;
                let p = Vec3::new(a.0 + (b.0 - a.0) * t, 1.8, a.1 + (b.1 - a.1) * t);
                let CellLookup::Cell { set, .. } = data.lookup(p) else {
                    panic!("{p} has no cell");
                };
                fractions.push(1.0 - data.visible_count(set) as f32 / total as f32);
            }
        }
        let mean = fractions.iter().sum::<f32>() / fractions.len() as f32;
        assert!(mean > 0.5, "mean culled fraction {mean}");
    }

    /// The renderer draws fewer meshes with occlusion culling on (Stats counters).
    #[test]
    fn culling_reduces_draws_in_the_demo() {
        use misty_engine::render::Renderer;
        struct Plain;
        impl misty_engine::render_scene::MaterialSource for Plain {
            fn params(&self, _: Option<&str>) -> misty_engine::render::MaterialParams {
                Default::default()
            }
        }
        let (mut scene, _) = committed();
        // An absolute path: other tests change the global project root.
        let o = &mut scene.render_settings.occlusion;
        let rel = o.data.clone().unwrap();
        o.data = Some(project().join(rel).to_string_lossy().into_owned());
        let cam = scene.find_by_name("Main Camera").unwrap();
        // Looking up the first street.
        scene.set_transform(cam, Transform::from_position(Vec3::new(-13.0, 1.8, -60.0)));
        let Some(gpu) = crate::sample_render::headless() else {
            return;
        };
        let (device, queue) = &gpu;
        let mut stats = Vec::new();
        for on in [false, true] {
            scene
                .world
                .get::<&mut misty_engine::scene::Camera>(cam)
                .unwrap()
                .occlusion_culling = on;
            let mut cache = misty_engine::draw_cache::DrawCache::new();
            let view =
                misty_engine::render_scene::GameView::cached(&scene, &Plain, &mut cache).unwrap();
            let mut renderer = Renderer::new(device);
            let target = renderer.create_target(device, 320, 180);
            renderer.render(device, queue, &target, &view.frame());
            stats.push(renderer.stats());
        }
        let (off, on) = (stats[0], stats[1]);
        assert_eq!(off.occlusion_culled, 0);
        assert!(on.occlusion_culled > 100, "{on:?}");
        assert!(on.visible * 2 < off.visible, "off {off:?}\non {on:?}");
        assert!(on.triangles < off.triangles);
    }
}
