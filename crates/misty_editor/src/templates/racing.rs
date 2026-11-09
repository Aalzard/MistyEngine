//! The Racing template: an arcade racer on a 1.2 km closed circuit.
//!
//! - The track is a closed Auto Smooth spline (`Track`, Spline Extrude with the Road profile)
//!   with red and white kerbs on both sides (two more extruded splines), a start gantry and a
//!   chequered line, marker posts, tyre barriers outside the tight corners, trees, a grandstand
//!   and hills on the horizon.
//! - The car is a Rigidbody on four Wheel Colliders with three primitive-built bodies (Sport,
//!   Muscle, Kart) shown one at a time.
//! - Lua does the rest, in `Assets/Scripts` for users to edit: `RaceCar.lua` (driving, boost,
//!   downforce, top speed, reset to the track), `ChaseCamera.lua`, `LapTimer.lua` and
//!   `RaceSettings.lua` (car model, gravity, mass, grip and top speed while playing).
//!
//! Every texture is generated ([`super::textures`]): nothing downloaded, free to reuse.

use std::path::Path;

use glam::{Vec2, Vec3};
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::scene::render_settings::SunDisk;
use misty_engine::scene::{
    BoxCollider, Camera, Canvas, CanvasScaler, Interpolation, JointSpring, MeshRenderer,
    PrimitiveMesh, RectTransform, Rigidbody, Scene, ScriptRef, Scripts, TextAnchor, Transform,
    UiImage, UiText, WheelCollider,
};
use misty_engine::splines::components::ExtrudeProfile;
use misty_engine::splines::{Knot, Spline, SplineContainer, SplineExtrude};
use misty_engine::ui::presets;

use super::scenes::{self, mesh_in, scaled};
use super::textures;

pub const RACE_CAR: &str = "Assets/Scripts/RaceCar.lua";
pub const CHASE_CAMERA: &str = "Assets/Scripts/ChaseCamera.lua";
pub const LAP_TIMER: &str = "Assets/Scripts/LapTimer.lua";
pub const RACE_SETTINGS: &str = "Assets/Scripts/RaceSettings.lua";

/// The scripts, as shipped in `templates/racing/`.
pub const SCRIPTS: [(&str, &str); 4] = [
    (RACE_CAR, include_str!("racing/RaceCar.lua")),
    (CHASE_CAMERA, include_str!("racing/ChaseCamera.lua")),
    (LAP_TIMER, include_str!("racing/LapTimer.lua")),
    (RACE_SETTINGS, include_str!("racing/RaceSettings.lua")),
];

/// Car model objects under the car, in `RaceCar.lua`'s order.
pub const MODELS: [&str; 3] = ["Model Sport", "Model Muscle", "Model Kart"];

/// Road width (metres) and the kerbs on each side of it.
pub const ROAD_WIDTH: f32 = 14.0;
const KERB_WIDTH: f32 = 1.2;
/// Knots of the track spline.
const KNOTS: usize = 32;
/// The road and kerbs sit this far above the grass (no z-fighting).
const ROAD_LIFT: f32 = 0.02;
const KERB_LIFT: f32 = 0.03;
/// The grass: 2.4 km square, its top at y = 0.
const GROUND_SIZE: Vec3 = Vec3::new(2400.0, 1.0, 2400.0);
/// The car starts this far before the start line.
const GRID_GAP: f32 = 14.0;

/// Wheel radius and layout (x, z of each wheel object; front first, as `RaceCar.lua` lists
/// them). The car's origin rests at [`RIDE_HEIGHT`] (half way down the 0.3 m travel).
pub const WHEEL_RADIUS: f32 = 0.36;
pub const WHEELS: [(&str, f32, f32); 4] = [
    ("Wheel FL", -0.82, 1.35),
    ("Wheel FR", 0.82, 1.35),
    ("Wheel RL", -0.82, -1.3),
    ("Wheel RR", 0.82, -1.3),
];
pub const RIDE_HEIGHT: f32 = WHEEL_RADIUS + 0.15;

const GRASS_TEX: &str = "Assets/Textures/Grass.png";
const ASPHALT_TEX: &str = "Assets/Textures/Asphalt.png";
const KERB_TEX: &str = "Assets/Textures/Kerb.png";
const CHECKER_TEX: &str = "Assets/Textures/Checker.png";

const GRASS: &str = "Assets/Materials/Grass.mat";
const ASPHALT: &str = "Assets/Materials/Asphalt.mat";
const KERB: &str = "Assets/Materials/Kerb.mat";
const START_LINE: &str = "Assets/Materials/Start Line.mat";
const SPORT_RED: &str = "Assets/Materials/Sport Red.mat";
const MUSCLE_BLUE: &str = "Assets/Materials/Muscle Blue.mat";
const KART_YELLOW: &str = "Assets/Materials/Kart Yellow.mat";
const STRIPE: &str = "Assets/Materials/Stripe White.mat";
const CARBON: &str = "Assets/Materials/Carbon.mat";
const GLASS: &str = "Assets/Materials/Car Glass.mat";
const TYRE: &str = "Assets/Materials/Tyre.mat";
const RIM: &str = "Assets/Materials/Rim.mat";
const HEADLIGHT: &str = "Assets/Materials/Headlight.mat";
const TAILLIGHT: &str = "Assets/Materials/Tail Light.mat";
const POST: &str = "Assets/Materials/Post.mat";
const REFLECTOR: &str = "Assets/Materials/Reflector.mat";
const BARRIER: &str = "Assets/Materials/Barrier.mat";
const TRUNK: &str = "Assets/Materials/Tree Trunk.mat";
const LEAVES: &str = "Assets/Materials/Tree Leaves.mat";
const HILL: &str = "Assets/Materials/Hill.mat";
const CONCRETE: &str = "Assets/Materials/Concrete.mat";
const BANNER: &str = "Assets/Materials/Banner.mat";
const CROWD: &str = "Assets/Materials/Crowd.mat";

fn materials() -> Vec<(&'static str, Material)> {
    let m = |color: [f32; 3], metallic: f32, smoothness: f32| Material {
        color: [color[0], color[1], color[2], 1.0],
        metallic,
        smoothness,
        ..Default::default()
    };
    let textured = |tex: &str, color: [f32; 3], tiling: [f32; 2], smoothness: f32| Material {
        albedo_map: Some(tex.into()),
        tiling,
        ..m(color, 0.0, smoothness)
    };
    let glow = |color: [f32; 3], intensity: f32| Material {
        emission_enabled: true,
        emission: color,
        emission_intensity: intensity,
        ..m(color, 0.0, 0.8)
    };
    let grass_tiles = GROUND_SIZE.x / 16.0;
    vec![
        (
            GRASS,
            textured(
                GRASS_TEX,
                [0.62, 0.78, 0.45],
                [grass_tiles, grass_tiles],
                0.05,
            ),
        ),
        (ASPHALT, textured(ASPHALT_TEX, [1.0; 3], [1.0, 1.0], 0.3)),
        (KERB, textured(KERB_TEX, [1.0; 3], [1.0, 1.0], 0.35)),
        (START_LINE, textured(CHECKER_TEX, [1.0; 3], [7.0, 1.0], 0.3)),
        (SPORT_RED, m([0.82, 0.07, 0.06], 0.4, 0.85)),
        (MUSCLE_BLUE, m([0.07, 0.22, 0.75], 0.4, 0.85)),
        (KART_YELLOW, m([0.98, 0.72, 0.05], 0.2, 0.75)),
        (STRIPE, m([0.95, 0.95, 0.95], 0.2, 0.8)),
        (CARBON, m([0.05, 0.05, 0.06], 0.3, 0.6)),
        (GLASS, m([0.05, 0.08, 0.12], 0.3, 0.95)),
        (TYRE, m([0.04, 0.04, 0.045], 0.0, 0.25)),
        (RIM, m([0.78, 0.79, 0.8], 0.9, 0.75)),
        (HEADLIGHT, glow([1.0, 0.95, 0.85], 4.0)),
        (TAILLIGHT, glow([1.0, 0.06, 0.04], 3.0)),
        (POST, m([0.92, 0.92, 0.92], 0.0, 0.4)),
        (REFLECTOR, glow([1.0, 0.35, 0.05], 3.0)),
        (BARRIER, textured(KERB_TEX, [1.0; 3], [1.0, 1.0], 0.3)),
        (TRUNK, m([0.36, 0.25, 0.16], 0.0, 0.2)),
        (LEAVES, m([0.2, 0.45, 0.17], 0.0, 0.15)),
        (HILL, m([0.38, 0.5, 0.36], 0.0, 0.1)),
        (CONCRETE, m([0.6, 0.6, 0.62], 0.0, 0.3)),
        (BANNER, glow([0.1, 0.45, 1.0], 2.5)),
        (CROWD, m([0.75, 0.3, 0.25], 0.0, 0.3)),
    ]
}

/// Write the Racing template into `root` (a project folder with its settings).
pub(super) fn create(root: &Path) -> Result<(), String> {
    textures::save(root, GRASS_TEX, &textures::grass())?;
    textures::save(root, ASPHALT_TEX, &textures::asphalt())?;
    textures::save(root, KERB_TEX, &textures::kerb())?;
    textures::save(root, CHECKER_TEX, &textures::checker())?;
    for (rel, m) in materials() {
        m.save(&root.join(rel))?;
    }
    for (rel, source) in SCRIPTS {
        let path = root.join(rel);
        std::fs::write(&path, source).map_err(super::io_error("Could not write", &path))?;
    }
    super::save_scene(root, super::SCENE, &scene())
}

/// Point of the circuit's outline at angle `a` (radians) around the centre: a loop of about
/// 1.2 km with fast sweepers, an S and two tighter corners (60–80 m radius).
fn outline(a: f32) -> Vec3 {
    let r = 170.0
        * (1.0 + 0.2 * (2.0 * a).sin() + 0.12 * (3.0 * a + 0.6).cos() + 0.07 * (5.0 * a).sin());
    Vec3::new(r * a.cos(), 0.0, r * a.sin())
}

/// Where the start line goes: the middle of the longest straight.
const START_ANGLE: f32 = 187.3;

/// Horizontal direction of the outline at angle `a`.
fn outline_direction(a: f32) -> Vec3 {
    let h = 1e-3;
    (outline(a + h) - outline(a - h)).normalize()
}

/// The circuit's knots, every 1/[`KNOTS`] of a turn from the start line, moved `offset`
/// metres to the right of the driving direction (0: the centre line).
fn knots(offset: f32) -> Vec<Knot> {
    (0..KNOTS)
        .map(|i| {
            let a = START_ANGLE.to_radians() + std::f32::consts::TAU * i as f32 / KNOTS as f32;
            let right = Vec3::Y.cross(outline_direction(a));
            Knot::at(outline(a) + right * offset)
        })
        .collect()
}

/// The track's centre line: a closed Auto Smooth spline whose first knot is the start line.
pub fn track_spline() -> Spline {
    Spline::new(knots(0.0), true)
}

/// Yaw (degrees) of a horizontal direction.
fn yaw_of(dir: Vec3) -> f32 {
    dir.x.atan2(dir.z).to_degrees()
}

fn script(path: &str) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            ..Default::default()
        }],
    }
}

/// A Road-profile Spline Extrude of `spline`, `width` wide, `lift` above the grass.
fn extrude(s: &mut Scene, name: &str, spline: Spline, width: f32, lift: f32, material: &str) {
    let e = s.spawn(name, None);
    s.set_transform(e, Transform::from_position(Vec3::Y * lift));
    s.add(e, SplineContainer::with(spline));
    s.add_by_name(e, "Spline Extrude");
    *s.world.get::<&mut SplineExtrude>(e).unwrap() = SplineExtrude {
        profile: ExtrudeProfile::Road,
        width,
        segments_per_unit: 0.5,
        // The texture covers 12 m of road (kerbs: one red and one white band every 3 m).
        uv_tiling: if width > 5.0 { 1.0 / 12.0 } else { 1.0 / 3.0 },
        ..Default::default()
    };
    s.world.get::<&mut MeshRenderer>(e).unwrap().material = Some(material.into());
}

/// A static box with a collider.
fn block(s: &mut Scene, name: &str, parent: Option<Entity>, t: Transform, material: &str) {
    let e = mesh_in(s, name, parent, PrimitiveMesh::Cube, t, material);
    s.add(e, BoxCollider::default());
}

/// Stable pseudo-random 0..1 for prop placement.
fn jitter(i: usize, salt: u32) -> f32 {
    let mut h = (i as u32).wrapping_mul(2_654_435_761) ^ salt.wrapping_mul(40_503);
    h = (h ^ (h >> 15)).wrapping_mul(2_246_822_519);
    ((h ^ (h >> 13)) & 0xffff) as f32 / 65535.0
}

/// The circuit: road, kerbs, start line and gantry, and the props along it.
fn track(s: &mut Scene) {
    let centre = track_spline();
    extrude(s, "Track", centre.clone(), ROAD_WIDTH, ROAD_LIFT, ASPHALT);
    let kerb = ROAD_WIDTH * 0.5 + KERB_WIDTH * 0.5;
    for (name, side) in [("Kerb Left", -1.0), ("Kerb Right", 1.0)] {
        let spline = Spline::new(knots(side * kerb), true);
        extrude(s, name, spline, KERB_WIDTH, KERB_LIFT, KERB);
    }
    let path = centre.path();
    let at = |d: f32| {
        let t = path.t_at_distance(d.rem_euclid(path.length));
        let tangent = path.tangent(t);
        let forward = Vec3::new(tangent.x, 0.0, tangent.z).normalize_or_zero();
        (path.position(t), forward, Vec3::Y.cross(forward))
    };
    let half = ROAD_WIDTH * 0.5;

    // Start line and gantry.
    let (p, f, r) = at(0.0);
    let yaw = yaw_of(f);
    let turn = |t: Transform| t.with_euler(Vec3::new(0.0, yaw, 0.0));
    let start = s.spawn("Start", None);
    s.set_transform(start, turn(Transform::from_position(p)));
    mesh_in(
        s,
        "Start Line",
        Some(start),
        PrimitiveMesh::Cube,
        scaled(Vec3::Y * 0.035, Vec3::new(ROAD_WIDTH, 0.02, 2.0)),
        START_LINE,
    );
    for x in [-1.0, 1.0] {
        block(
            s,
            "Gantry Pillar",
            Some(start),
            scaled(
                Vec3::new(x * (half + 2.5), 3.5, 0.0),
                Vec3::new(0.8, 7.0, 0.8),
            ),
            CONCRETE,
        );
    }
    mesh_in(
        s,
        "Gantry Beam",
        Some(start),
        PrimitiveMesh::Cube,
        scaled(
            Vec3::new(0.0, 7.4, 0.0),
            Vec3::new(ROAD_WIDTH + 6.0, 1.4, 0.9),
        ),
        CONCRETE,
    );
    mesh_in(
        s,
        "Gantry Banner",
        Some(start),
        PrimitiveMesh::Cube,
        scaled(
            Vec3::new(0.0, 7.4, -0.47),
            Vec3::new(ROAD_WIDTH + 4.0, 0.8, 0.05),
        ),
        BANNER,
    );
    // A grandstand outside the start straight, facing the track.
    let stand = s.spawn("Grandstand", None);
    s.set_transform(
        stand,
        Transform::from_position(p - r * (half + 24.0)).with_euler(Vec3::new(0.0, yaw, 0.0)),
    );
    for (i, (h, depth)) in [(1.0, 9.0), (2.2, 6.5), (3.4, 4.0)].into_iter().enumerate() {
        let back = -(9.0 - depth) * 0.5;
        block(
            s,
            &format!("Tier {}", i + 1),
            Some(stand),
            scaled(Vec3::new(back, h * 0.5, 0.0), Vec3::new(depth, h, 60.0)),
            if i == 1 { CROWD } else { CONCRETE },
        );
    }
    mesh_in(
        s,
        "Roof",
        Some(stand),
        PrimitiveMesh::Cube,
        scaled(Vec3::new(-1.0, 8.0, 0.0), Vec3::new(11.0, 0.3, 62.0)),
        CONCRETE,
    );

    // Marker posts every 25 m on both sides: the sense of speed.
    let posts = s.spawn("Marker Posts", None);
    let mut d = 12.5;
    let mut n = 0;
    while d < path.length {
        let (p, _, r) = at(d);
        for side in [-1.0, 1.0] {
            n += 1;
            let base = p + r * side * (half + KERB_WIDTH + 2.5);
            let post = mesh_in(
                s,
                &format!("Post {n}"),
                Some(posts),
                PrimitiveMesh::Cylinder,
                scaled(base + Vec3::Y * 0.55, Vec3::new(0.18, 0.55, 0.18)),
                POST,
            );
            mesh_in(
                s,
                "Reflector",
                Some(post),
                PrimitiveMesh::Cube,
                scaled(Vec3::new(0.0, 0.7, 0.0), Vec3::new(1.05, 0.25, 1.05)),
                REFLECTOR,
            );
        }
        d += 25.0;
    }

    // Tyre barriers outside the tighter corners (radius under 120 m).
    let barriers = s.spawn("Barriers", None);
    let mut d = 0.0;
    let mut n = 0;
    while d < path.length {
        let (p, f, r) = at(d);
        let (_, ahead, _) = at(d + 4.0);
        // Turning right when the direction swings towards `r`.
        let turn = (ahead - f).dot(r) / 4.0;
        if turn.abs() > 1.0 / 120.0 {
            n += 1;
            let outside = -turn.signum();
            let pos = p + r * outside * (half + KERB_WIDTH + 9.0) + Vec3::Y * 0.5;
            block(
                s,
                &format!("Barrier {n}"),
                Some(barriers),
                scaled(pos, Vec3::new(0.9, 1.0, 5.6)).with_euler(Vec3::new(0.0, yaw_of(f), 0.0)),
                BARRIER,
            );
        }
        d += 6.0;
    }

    // Trees well clear of the road, a little irregular.
    let trees = s.spawn("Trees", None);
    let mut d = 6.0;
    let mut i = 0;
    while d < path.length {
        let (p, _, r) = at(d);
        let side = if i % 2 == 0 { 1.0 } else { -1.0 };
        let pos = p + r * side * (half + 18.0 + jitter(i, 1) * 26.0);
        i += 1;
        d += 22.0 + jitter(i, 2) * 10.0;
        if path.nearest(pos).distance < half + 14.0 {
            continue;
        }
        let height = 4.0 + jitter(i, 3) * 3.0;
        let tree = s.spawn(&format!("Tree {i}"), Some(trees));
        s.set_transform(tree, Transform::from_position(pos));
        let trunk = mesh_in(
            s,
            "Trunk",
            Some(tree),
            PrimitiveMesh::Cylinder,
            scaled(Vec3::Y * height * 0.5, Vec3::new(0.45, height * 0.5, 0.45)),
            TRUNK,
        );
        s.add(trunk, BoxCollider::default());
        let crown = 3.2 + jitter(i, 4) * 2.0;
        mesh_in(
            s,
            "Crown",
            Some(tree),
            PrimitiveMesh::Sphere,
            scaled(
                Vec3::Y * (height + crown * 0.3),
                Vec3::new(crown, crown * 1.2, crown),
            ),
            LEAVES,
        );
    }

    // Low hills on the horizon.
    let hills = s.spawn("Hills", None);
    for i in 0..12 {
        let a = std::f32::consts::TAU * (i as f32 + jitter(i, 5) * 0.6) / 12.0;
        let dist = 650.0 + jitter(i, 6) * 250.0;
        let w = 220.0 + jitter(i, 7) * 180.0;
        let h = 50.0 + jitter(i, 8) * 70.0;
        mesh_in(
            s,
            &format!("Hill {}", i + 1),
            Some(hills),
            PrimitiveMesh::Sphere,
            scaled(
                Vec3::new(a.cos() * dist, -h * 0.15, a.sin() * dist),
                Vec3::new(w, h, w * 0.8),
            ),
            HILL,
        );
    }
}

/// One car body: `parts` are (name, primitive, position, scale, euler, material).
type Part = (&'static str, PrimitiveMesh, Vec3, Vec3, Vec3, &'static str);

fn model(s: &mut Scene, car: Entity, name: &str, parts: &[Part], active: bool) {
    let m = s.spawn(name, Some(car));
    for (part, kind, pos, scale, euler, material) in parts {
        mesh_in(
            s,
            part,
            Some(m),
            *kind,
            scaled(*pos, *scale).with_euler(*euler),
            material,
        );
    }
    s.game_object_mut(m).active = active;
}

fn sport() -> Vec<Part> {
    let c = PrimitiveMesh::Cube;
    let z = Vec3::ZERO;
    let v = Vec3::new;
    vec![
        (
            "Body",
            c,
            v(0.0, 0.12, 0.0),
            v(1.9, 0.42, 4.4),
            z,
            SPORT_RED,
        ),
        (
            "Nose",
            c,
            v(0.0, 0.05, 2.3),
            v(1.8, 0.2, 0.9),
            v(10.0, 0.0, 0.0),
            SPORT_RED,
        ),
        ("Cabin", c, v(0.0, 0.5, -0.3), v(1.45, 0.4, 1.8), z, GLASS),
        (
            "Roof",
            c,
            v(0.0, 0.72, -0.35),
            v(1.4, 0.05, 1.4),
            z,
            SPORT_RED,
        ),
        (
            "Wing",
            c,
            v(0.0, 0.74, -2.05),
            v(1.9, 0.06, 0.45),
            z,
            CARBON,
        ),
        (
            "Wing Strut",
            c,
            v(-0.6, 0.5, -2.0),
            v(0.08, 0.4, 0.12),
            z,
            CARBON,
        ),
        (
            "Wing Strut",
            c,
            v(0.6, 0.5, -2.0),
            v(0.08, 0.4, 0.12),
            z,
            CARBON,
        ),
        (
            "Splitter",
            c,
            v(0.0, -0.08, 2.5),
            v(1.9, 0.04, 0.5),
            z,
            CARBON,
        ),
        (
            "Headlight",
            c,
            v(-0.6, 0.22, 2.2),
            v(0.42, 0.1, 0.04),
            z,
            HEADLIGHT,
        ),
        (
            "Headlight",
            c,
            v(0.6, 0.22, 2.2),
            v(0.42, 0.1, 0.04),
            z,
            HEADLIGHT,
        ),
        (
            "Tail Light",
            c,
            v(-0.62, 0.24, -2.2),
            v(0.4, 0.1, 0.04),
            z,
            TAILLIGHT,
        ),
        (
            "Tail Light",
            c,
            v(0.62, 0.24, -2.2),
            v(0.4, 0.1, 0.04),
            z,
            TAILLIGHT,
        ),
    ]
}

fn muscle() -> Vec<Part> {
    let c = PrimitiveMesh::Cube;
    let z = Vec3::ZERO;
    let v = Vec3::new;
    vec![
        (
            "Body",
            c,
            v(0.0, 0.18, 0.0),
            v(1.98, 0.56, 4.7),
            z,
            MUSCLE_BLUE,
        ),
        (
            "Stripe",
            c,
            v(-0.22, 0.465, 0.0),
            v(0.22, 0.02, 4.72),
            z,
            STRIPE,
        ),
        (
            "Stripe",
            c,
            v(0.22, 0.465, 0.0),
            v(0.22, 0.02, 4.72),
            z,
            STRIPE,
        ),
        (
            "Cabin",
            c,
            v(0.0, 0.68, -0.55),
            v(1.66, 0.45, 2.0),
            z,
            GLASS,
        ),
        (
            "Roof",
            c,
            v(0.0, 0.93, -0.6),
            v(1.62, 0.06, 1.6),
            z,
            MUSCLE_BLUE,
        ),
        (
            "Grille",
            c,
            v(0.0, 0.16, 2.36),
            v(1.6, 0.26, 0.04),
            z,
            CARBON,
        ),
        (
            "Bumper",
            c,
            v(0.0, -0.05, 2.38),
            v(2.0, 0.12, 0.12),
            z,
            CARBON,
        ),
        (
            "Headlight",
            c,
            v(-0.68, 0.24, 2.37),
            v(0.3, 0.16, 0.04),
            z,
            HEADLIGHT,
        ),
        (
            "Headlight",
            c,
            v(0.68, 0.24, 2.37),
            v(0.3, 0.16, 0.04),
            z,
            HEADLIGHT,
        ),
        (
            "Tail Light",
            c,
            v(-0.65, 0.3, -2.36),
            v(0.5, 0.12, 0.04),
            z,
            TAILLIGHT,
        ),
        (
            "Tail Light",
            c,
            v(0.65, 0.3, -2.36),
            v(0.5, 0.12, 0.04),
            z,
            TAILLIGHT,
        ),
    ]
}

fn kart() -> Vec<Part> {
    let c = PrimitiveMesh::Cube;
    let z = Vec3::ZERO;
    let v = Vec3::new;
    vec![
        (
            "Chassis",
            c,
            v(0.0, -0.12, 0.0),
            v(1.0, 0.12, 3.4),
            z,
            CARBON,
        ),
        (
            "Body",
            c,
            v(0.0, 0.06, 0.1),
            v(0.8, 0.3, 2.6),
            z,
            KART_YELLOW,
        ),
        (
            "Nose",
            c,
            v(0.0, 0.0, 1.8),
            v(0.5, 0.2, 1.0),
            z,
            KART_YELLOW,
        ),
        (
            "Front Wing",
            c,
            v(0.0, -0.15, 2.3),
            v(1.9, 0.05, 0.35),
            z,
            CARBON,
        ),
        (
            "Side Pod",
            c,
            v(-0.55, 0.0, -0.3),
            v(0.35, 0.25, 1.2),
            z,
            KART_YELLOW,
        ),
        (
            "Side Pod",
            c,
            v(0.55, 0.0, -0.3),
            v(0.35, 0.25, 1.2),
            z,
            KART_YELLOW,
        ),
        (
            "Rear Wing",
            c,
            v(0.0, 0.65, -1.75),
            v(1.6, 0.06, 0.4),
            z,
            KART_YELLOW,
        ),
        (
            "Wing Strut",
            c,
            v(0.0, 0.4, -1.7),
            v(0.1, 0.5, 0.1),
            z,
            CARBON,
        ),
        (
            "Roll Hoop",
            c,
            v(0.0, 0.55, -0.85),
            v(0.08, 0.45, 0.08),
            z,
            CARBON,
        ),
        (
            "Driver",
            c,
            v(0.0, 0.35, -0.5),
            v(0.45, 0.4, 0.4),
            z,
            CARBON,
        ),
        (
            "Helmet",
            PrimitiveMesh::Sphere,
            v(0.0, 0.68, -0.45),
            Vec3::splat(0.38),
            z,
            STRIPE,
        ),
    ]
}

/// The car at `position` facing `yaw`: Rigidbody and box collider on the root, the three
/// bodies (Sport shown), and four Wheel Colliders each turning a tyre model.
fn car(s: &mut Scene, position: Vec3, yaw: f32) -> Entity {
    let car = s.spawn("Car", None);
    s.set_transform(
        car,
        Transform::from_position(position + Vec3::Y * RIDE_HEIGHT)
            .with_euler(Vec3::new(0.0, yaw, 0.0)),
    );
    s.add(
        car,
        Rigidbody {
            mass: 1150.0,
            drag: 0.02,
            angular_drag: 0.5,
            interpolation: Interpolation::Interpolate,
            ..Default::default()
        },
    );
    s.add(
        car,
        // Low down: the centre of mass follows the collider, and a low one keeps the car on
        // its wheels in fast corners.
        BoxCollider {
            center: Vec3::new(0.0, 0.05, 0.0),
            size: Vec3::new(1.8, 0.5, 4.2),
            ..Default::default()
        },
    );
    s.add(car, script(RACE_CAR));
    model(s, car, MODELS[0], &sport(), true);
    model(s, car, MODELS[1], &muscle(), false);
    model(s, car, MODELS[2], &kart(), false);
    for (name, x, z) in WHEELS {
        let wheel_model = s.spawn(&format!("{name} Model"), Some(car));
        // Where the wheel rests: half way down its suspension.
        s.set_transform(
            wheel_model,
            Transform::from_position(Vec3::new(x, -0.15, z)),
        );
        let d = WHEEL_RADIUS * 2.0;
        mesh_in(
            s,
            "Tyre",
            Some(wheel_model),
            PrimitiveMesh::Cylinder,
            scaled(Vec3::ZERO, Vec3::new(d, 0.14, d)).with_euler(Vec3::new(0.0, 0.0, 90.0)),
            TYRE,
        );
        let side = x.signum();
        mesh_in(
            s,
            "Rim",
            Some(wheel_model),
            PrimitiveMesh::Cylinder,
            scaled(
                Vec3::new(side * 0.02, 0.0, 0.0),
                Vec3::new(0.46, 0.135, 0.46),
            )
            .with_euler(Vec3::new(0.0, 0.0, 90.0)),
            RIM,
        );
        // A bar across the rim, so the wheel is seen to roll.
        mesh_in(
            s,
            "Spoke",
            Some(wheel_model),
            PrimitiveMesh::Cube,
            scaled(Vec3::new(side * 0.14, 0.0, 0.0), Vec3::new(0.04, 0.58, 0.1)),
            CARBON,
        );
        let w = s.spawn(name, Some(car));
        s.set_transform(w, Transform::from_position(Vec3::new(x, 0.0, z)));
        s.add(
            w,
            WheelCollider {
                radius: WHEEL_RADIUS,
                suspension_spring: JointSpring {
                    spring: 42000.0,
                    damper: 5000.0,
                    target_position: 0.5,
                },
                // Forces above the ground contact: less body roll in fast corners.
                force_app_point_distance: 0.25,
                wheel_model: Some(wheel_model),
                ..Default::default()
            },
        );
    }
    car
}

/// A HUD text under `canvas`, anchored at `anchor` (also its pivot), `pos` from there.
#[allow(clippy::too_many_arguments)]
fn hud_text(
    s: &mut Scene,
    canvas: Entity,
    name: &str,
    text: &str,
    size: f32,
    anchor: Vec2,
    pos: Vec2,
    box_size: Vec2,
    align: TextAnchor,
) -> Entity {
    let rt = RectTransform {
        anchor_min: anchor,
        anchor_max: anchor,
        pivot: anchor,
        anchored_position: pos,
        size_delta: box_size,
    };
    let e = presets::spawn(s, name, Some(canvas), rt);
    s.add(
        e,
        UiText {
            text: text.into(),
            font_size: size,
            color: [1.0; 4],
            alignment: align,
            wrap: false,
            ..Default::default()
        },
    );
    e
}

/// The settings panel's text before RaceSettings.lua first writes it (the Sport's values).
const SETTINGS_TEXT: &str = concat!(
    "SETTINGS  ([Tab] hides)\n",
    "[C] Car: Sport\n",
    "[G] Gravity: Normal (1.00 g)\n",
    "[1/2] Mass: 1150 kg\n",
    "[3/4] Grip: 1.6\n",
    "[5/6] Top speed: 230 km/h\n",
    "\n",
    "[W/S] drive  [A/D] steer  [Shift] boost\n",
    "[Space] handbrake  [R] reset",
);

/// The HUD: lap and times top left, speed and boost bottom left, messages in the middle and
/// the settings panel top right.
fn hud(s: &mut Scene) {
    let canvas = presets::spawn(s, "HUD", None, RectTransform::stretched());
    s.add(canvas, Canvas::default());
    // Laid out for 1600 × 900, scaled to the screen.
    s.add(
        canvas,
        CanvasScaler {
            reference_resolution: Vec2::new(1600.0, 900.0),
            match_width_or_height: 0.5,
            ..Default::default()
        },
    );
    let top_left = Vec2::new(0.0, 1.0);
    let bottom_left = Vec2::ZERO;
    let left = TextAnchor::UpperLeft;
    hud_text(
        s,
        canvas,
        "Lap Text",
        "Cross the line to start",
        34.0,
        top_left,
        Vec2::new(24.0, -18.0),
        Vec2::new(700.0, 48.0),
        left,
    );
    hud_text(
        s,
        canvas,
        "Times Text",
        "LAST -:--.---   BEST -:--.---",
        20.0,
        top_left,
        Vec2::new(24.0, -66.0),
        Vec2::new(700.0, 30.0),
        left,
    );
    hud_text(
        s,
        canvas,
        "Speed Text",
        "0 km/h",
        48.0,
        bottom_left,
        Vec2::new(24.0, 60.0),
        Vec2::new(400.0, 60.0),
        TextAnchor::LowerLeft,
    );
    hud_text(
        s,
        canvas,
        "Boost Text",
        "BOOST ||||||||||",
        22.0,
        bottom_left,
        Vec2::new(24.0, 24.0),
        Vec2::new(400.0, 30.0),
        TextAnchor::LowerLeft,
    );
    hud_text(
        s,
        canvas,
        "Message Text",
        "",
        30.0,
        Vec2::new(0.5, 1.0),
        Vec2::new(0.0, -110.0),
        Vec2::new(800.0, 44.0),
        TextAnchor::MiddleCenter,
    );
    let top_right = Vec2::new(1.0, 1.0);
    let panel = presets::spawn(
        s,
        "Settings Panel",
        Some(canvas),
        RectTransform {
            anchor_min: top_right,
            anchor_max: top_right,
            pivot: top_right,
            anchored_position: Vec2::new(-16.0, -16.0),
            size_delta: Vec2::new(390.0, 238.0),
        },
    );
    s.add(
        panel,
        UiImage {
            color: [0.02, 0.03, 0.05, 0.55],
            ..Default::default()
        },
    );
    let text = hud_text(
        s,
        panel,
        "Settings Text",
        SETTINGS_TEXT,
        17.0,
        Vec2::new(0.0, 1.0),
        Vec2::new(14.0, -10.0),
        Vec2::new(370.0, 220.0),
        left,
    );
    s.world.get::<&mut UiText>(text).unwrap().line_spacing = 1.1;
}

/// The Racing scene: sun, sky, the circuit, the car on the grid behind the start line, the
/// chase camera behind it, the race scripts and the HUD.
pub fn scene() -> Scene {
    let mut s = Scene::default_3d("Main");
    scenes::sun(&mut s, Vec3::new(32.0, 140.0, 0.0), [1.0, 0.93, 0.82], 1.5);
    let sky = &mut s.render_settings.skybox;
    sky.sun_disk = SunDisk::HighQuality;
    sky.ground = [0.42, 0.5, 0.38];
    let ground = mesh_in(
        &mut s,
        "Ground",
        None,
        PrimitiveMesh::Cube,
        scaled(Vec3::new(0.0, -GROUND_SIZE.y * 0.5, 0.0), GROUND_SIZE),
        GRASS,
    );
    s.add(ground, BoxCollider::default());
    track(&mut s);

    let path = track_spline().path();
    let t = path.t_at_distance(path.length - GRID_GAP);
    let tangent = path.tangent(t);
    let yaw = yaw_of(Vec3::new(tangent.x, 0.0, tangent.z));
    let car_at = path.position(t);
    car(&mut s, car_at, yaw);

    // The chase camera starts where ChaseCamera.lua holds it: behind and above the car.
    if let Some(cam) = s.find_by_name("Main Camera") {
        let back = Vec3::new(tangent.x, 0.0, tangent.z).normalize();
        let pos = car_at + Vec3::Y * (RIDE_HEIGHT + 2.6) - back * 7.0;
        let look = car_at + Vec3::Y * (RIDE_HEIGHT + 1.0) - pos;
        let pitch = (-look.y)
            .atan2(Vec2::new(look.x, look.z).length())
            .to_degrees();
        s.set_transform(
            cam,
            Transform::from_position(pos).with_euler(Vec3::new(pitch, yaw, 0.0)),
        );
        if let Ok(mut c) = s.world.get::<&mut Camera>(cam) {
            c.far = 3000.0;
            c.fov = 62.0;
        }
        s.add(cam, script(CHASE_CAMERA));
    }
    let race = s.spawn("Race", None);
    s.add(
        race,
        Scripts {
            list: vec![
                ScriptRef {
                    path: LAP_TIMER.into(),
                    ..Default::default()
                },
                ScriptRef {
                    path: RACE_SETTINGS.into(),
                    ..Default::default()
                },
            ],
        },
    );
    hud(&mut s);

    let mut volume = scenes::sky_volume();
    // A light haze towards the hills.
    let f = &mut volume.fog;
    f.active = true;
    f.volumetric = false;
    f.max_fog_distance = 3000.0;
    f.attenuation_distance = 1800.0;
    f.base_height = 0.0;
    f.maximum_height = 150.0;
    scenes::add_volume(&mut s, volume);
    s
}
