//! Procedural VehicleDemo scene in `SampleProject`: a drivable car (Rigidbody body on four
//! Wheel Colliders with wheel models, `CarController.lua`: W/S throttle and brake, A/D steer,
//! Space handbrake, R resets), a follow camera (`CarFollowCamera.lua`), a ramp, speed bumps,
//! traffic cones and crates to knock over, a ring of barriers and a speed read-out.
//! Regenerate with `cargo test -p misty_editor -- --ignored write_vehicle_demo`; the other tests
//! check the committed files still match and drive the car with injected keys.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    BoxCollider, Interpolation, JointSpring, MeshFilter, MeshRenderer, PrimitiveMesh,
    RectTransform, Rigidbody, Scene, ScriptRef, Scripts, Transform, UiText, WheelCollider,
};

use crate::actions::{Action, Create};
use crate::project::ProjectBrowser;
use crate::state::EditorState;
use crate::ui_create::UiElement;

const SCENE: &str = "Assets/Scenes/VehicleDemo.scene";
const CONTROLLER: &str = "Assets/Scripts/CarController.lua";
const FOLLOW: &str = "Assets/Scripts/CarFollowCamera.lua";
const PAINT: &str = "Assets/Materials/Car Paint.mat";
const RUBBER: &str = "Assets/Materials/Tyre Rubber.mat";
const RIM: &str = "Assets/Materials/Wheel Rim.mat";
const GLASS: &str = "Assets/Materials/Car Glass.mat";
const HEADLIGHT: &str = "Assets/Materials/Headlight.mat";
const TAILLIGHT: &str = "Assets/Materials/Tail Light.mat";
const TRACK: &str = "Assets/Materials/Track Checker.mat";
const CONE: &str = "Assets/Materials/Traffic Cone.mat";
const WOOD: &str = "Assets/Materials/Wood.mat";
const PAVEMENT: &str = "Assets/Materials/Pavement.mat";
const STEEL: &str = "Assets/Materials/Blue Steel.mat";

/// Wheel radius and the car's wheel layout (x, z of each wheel object; front first).
const WHEEL_RADIUS: f32 = 0.36;
const WHEELS: [(&str, f32, f32); 4] = [
    ("Wheel FL", -0.82, 1.32),
    ("Wheel FR", 0.82, 1.32),
    ("Wheel RL", -0.82, -1.3),
    ("Wheel RR", 0.82, -1.3),
];
/// Car origin height above the ground at rest (wheel radius + half the 0.3 m travel).
const RIDE_HEIGHT: f32 = WHEEL_RADIUS + 0.15;

const CAR_CONTROLLER: &str = r#"-- CarController: W/S (or the arrow keys) throttle and brake, S reverses once stopped,
-- A/D steer, Space pulls the handbrake (rear wheels), R puts the car back on its wheels.
-- Drives the Wheel Colliders named below (rear-wheel drive) and shows the speed in a Text.
local CarController = {
    motorTorque = 1100,
    reverseTorque = 700,
    brakeTorque = 2500,
    handbrakeTorque = 6000,
    maxSteerAngle = 32,
    -- Steering eases towards the keys at this many degrees per second.
    steerSpeed = 140,
    -- Less steering at speed (degrees at 30 m/s).
    highSpeedSteerAngle = 10,
    frontLeft = "Wheel FL",
    frontRight = "Wheel FR",
    rearLeft = "Wheel RL",
    rearRight = "Wheel RR",
    speedText = "Speed Text",
}

local function wheel(name)
    local go = GameObject.Find(name)
    if go == nil then
        Debug.LogError("CarController: no wheel called " .. name)
        return nil
    end
    return go:GetComponent("WheelCollider")
end

function CarController:Start()
    self.body = self.gameObject:GetComponent("Rigidbody")
    self.front = { wheel(self.frontLeft), wheel(self.frontRight) }
    self.rear = { wheel(self.rearLeft), wheel(self.rearRight) }
    self._steer = 0
    local text = GameObject.Find(self.speedText)
    if text ~= nil then
        self.label = text:GetComponent("Text")
    end
end

local function key(a, b)
    return Input.GetKey(a) or Input.GetKey(b)
end

function CarController:FixedUpdate()
    local dt = Time.fixedDeltaTime
    local speed = Vector3.Dot(self.body.velocity, self.transform.forward)

    -- Steering: ease towards the keys, less of it at speed.
    local input = 0
    if key("a", "left") then input = input - 1 end
    if key("d", "right") then input = input + 1 end
    local t = Mathf.Clamp(math.abs(speed) / 30, 0, 1)
    local limit = Mathf.Lerp(self.maxSteerAngle, self.highSpeedSteerAngle, t)
    local target = input * limit
    local step = self.steerSpeed * dt
    if math.abs(target - self._steer) <= step then
        self._steer = target
    elseif target > self._steer then
        self._steer = self._steer + step
    else
        self._steer = self._steer - step
    end

    -- Throttle and brake: S brakes while rolling forward, then reverses (W likewise).
    local motor, brake = 0, 0
    if key("w", "up") then
        if speed < -0.5 then brake = self.brakeTorque else motor = self.motorTorque end
    elseif key("s", "down") then
        if speed > 0.5 then brake = self.brakeTorque else motor = -self.reverseTorque end
    else
        -- Engine braking, so the car slows down when coasting.
        brake = 40
    end
    local handbrake = Input.GetKey("space")

    for _, w in ipairs(self.front) do
        w.steerAngle = self._steer
        w.motorTorque = 0
        w.brakeTorque = brake
    end
    for _, w in ipairs(self.rear) do
        w.motorTorque = motor
        if handbrake then
            w.motorTorque = 0
            w.brakeTorque = self.handbrakeTorque
        else
            w.brakeTorque = brake
        end
    end
end

function CarController:Update()
    if Input.GetKeyDown("r") then
        -- Back on the wheels, facing the same way.
        local yaw = self.transform.eulerAngles.y
        self.transform.position = self.transform.position + Vector3.up * 1.5
        self.transform.eulerAngles = Vector3.new(0, yaw, 0)
        self.body.velocity = Vector3.zero
        self.body.angularVelocity = Vector3.zero
    end
    if self.label ~= nil then
        local kmh = math.floor(self.body.velocity.magnitude * 3.6 + 0.5)
        local text = kmh .. " km/h"
        if text ~= self._shown then
            self.label.text = text
            self._shown = text
        end
    end
end

return CarController
"#;

const FOLLOW_CAMERA: &str = r#"-- CarFollowCamera: stays behind and above the target, easing after it, and looks at it.
local CarFollowCamera = {
    target = "Car",
    distance = 7.5,
    height = 2.8,
    lookHeight = 0.8,
    -- How quickly the camera catches up (higher = stiffer).
    positionDamping = 4,
}

function CarFollowCamera:Start()
    self._target = GameObject.Find(self.target).transform
end

function CarFollowCamera:LateUpdate()
    local t = self._target
    -- Behind the car on the ground plane, so it doesn't swing when the car pitches.
    local back = Vector3.new(t.forward.x, 0, t.forward.z)
    if back.magnitude < 0.01 then back = Vector3.forward end
    back = back.normalized
    local wanted = t.position - back * self.distance + Vector3.up * self.height
    local k = Mathf.Clamp(Time.deltaTime * self.positionDamping, 0, 1)
    self.transform.position = Vector3.Lerp(self.transform.position, wanted, k)
    self.transform:LookAt(t.position + Vector3.up * self.lookHeight)
end

return CarFollowCamera
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
    let glow = |color: [f32; 3], intensity| Material {
        emission_enabled: true,
        emission: color,
        emission_intensity: intensity,
        ..m([color[0], color[1], color[2], 1.0], 0.0, 0.8)
    };
    [
        (PAINT, m([0.78, 0.09, 0.07, 1.0], 0.35, 0.8)),
        (RUBBER, m([0.05, 0.05, 0.055, 1.0], 0.0, 0.25)),
        (RIM, m([0.75, 0.76, 0.78, 1.0], 0.9, 0.7)),
        (GLASS, m([0.06, 0.09, 0.13, 1.0], 0.3, 0.95)),
        (HEADLIGHT, glow([1.0, 0.95, 0.82], 3.0)),
        (TAILLIGHT, glow([1.0, 0.08, 0.05], 2.0)),
        (
            TRACK,
            Material {
                albedo_map: Some("Assets/Textures/Checker.png".into()),
                tiling: [60.0, 60.0],
                ..m([0.85, 0.85, 0.85, 1.0], 0.0, 0.25)
            },
        ),
        (CONE, m([1.0, 0.38, 0.05, 1.0], 0.0, 0.45)),
    ]
}

fn script(path: &str) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            ..Default::default()
        }],
    }
}

fn scaled(position: Vec3, scale: Vec3) -> Transform {
    Transform {
        position,
        scale,
        ..Default::default()
    }
}

/// Mesh object (no collider) under `parent`.
fn mesh(
    s: &mut Scene,
    name: &str,
    parent: Option<Entity>,
    kind: PrimitiveMesh,
    t: Transform,
    material: &str,
) -> Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(kind));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
    e
}

/// Static box with a collider.
fn block(s: &mut Scene, name: &str, t: Transform, material: &str) -> Entity {
    let e = mesh(s, name, None, PrimitiveMesh::Cube, t, material);
    s.add(e, BoxCollider::default());
    e
}

/// The car: Rigidbody + box collider on the root, a body shell, cabin and lights, and four
/// Wheel Colliders each moving a tyre model.
fn car(s: &mut Scene) -> Entity {
    let car = s.spawn("Car", None);
    s.set_transform(
        car,
        Transform::from_position(Vec3::new(0.0, RIDE_HEIGHT, 0.0)),
    );
    s.add(
        car,
        Rigidbody {
            mass: 1200.0,
            drag: 0.05,
            angular_drag: 0.3,
            interpolation: Interpolation::Interpolate,
            ..Default::default()
        },
    );
    s.add(
        car,
        BoxCollider {
            center: Vec3::new(0.0, 0.5, 0.0),
            size: Vec3::new(1.8, 0.62, 4.2),
            ..Default::default()
        },
    );
    s.add(car, script(CONTROLLER));
    let cube = PrimitiveMesh::Cube;
    mesh(
        s,
        "Body",
        Some(car),
        cube,
        scaled(Vec3::new(0.0, 0.5, 0.0), Vec3::new(1.8, 0.55, 4.2)),
        PAINT,
    );
    mesh(
        s,
        "Cabin",
        Some(car),
        cube,
        scaled(Vec3::new(0.0, 1.0, -0.35), Vec3::new(1.5, 0.5, 2.0)),
        GLASS,
    );
    mesh(
        s,
        "Roof",
        Some(car),
        cube,
        scaled(Vec3::new(0.0, 1.27, -0.4), Vec3::new(1.46, 0.06, 1.7)),
        PAINT,
    );
    for x in [-0.62, 0.62] {
        mesh(
            s,
            "Headlight",
            Some(car),
            cube,
            scaled(Vec3::new(x, 0.55, 2.1), Vec3::new(0.38, 0.14, 0.04)),
            HEADLIGHT,
        );
        mesh(
            s,
            "Tail Light",
            Some(car),
            cube,
            scaled(Vec3::new(x, 0.6, -2.1), Vec3::new(0.38, 0.12, 0.04)),
            TAILLIGHT,
        );
    }
    for (name, x, z) in WHEELS {
        let model = s.spawn(&format!("{name} Model"), Some(car));
        // Where the wheel rests: half way down its suspension.
        s.set_transform(model, Transform::from_position(Vec3::new(x, -0.15, z)));
        let d = WHEEL_RADIUS * 2.0;
        mesh(
            s,
            "Tyre",
            Some(model),
            PrimitiveMesh::Cylinder,
            scaled(Vec3::ZERO, Vec3::new(d, 0.13, d)).with_euler(Vec3::new(0.0, 0.0, 90.0)),
            RUBBER,
        );
        // Rim with a bar across, so the wheel is seen to roll.
        let side = if x < 0.0 { -1.0 } else { 1.0 };
        mesh(
            s,
            "Rim",
            Some(model),
            PrimitiveMesh::Cylinder,
            scaled(
                Vec3::new(side * 0.02, 0.0, 0.0),
                Vec3::new(0.46, 0.125, 0.46),
            )
            .with_euler(Vec3::new(0.0, 0.0, 90.0)),
            RIM,
        );
        mesh(
            s,
            "Spoke",
            Some(model),
            cube,
            scaled(Vec3::new(side * 0.13, 0.0, 0.0), Vec3::new(0.04, 0.58, 0.1)),
            RIM,
        );
        let w = s.spawn(name, Some(car));
        s.set_transform(w, Transform::from_position(Vec3::new(x, 0.0, z)));
        s.add(
            w,
            WheelCollider {
                radius: WHEEL_RADIUS,
                suspension_spring: JointSpring {
                    spring: 35000.0,
                    damper: 4500.0,
                    target_position: 0.5,
                },
                // Forces a little above the ground: less body roll in corners.
                force_app_point_distance: 0.15,
                wheel_model: Some(model),
                ..Default::default()
            },
        );
    }
    car
}

fn demo_scene() -> Scene {
    let mut s = EditorState::new(ProjectBrowser::open(
        std::env::temp_dir().join("misty_vehicle_demo_build"),
    ));
    s.scene = Scene::default_3d("VehicleDemo");
    let scene = &mut s.scene;
    let cube = PrimitiveMesh::Cube;
    block(
        scene,
        "Ground",
        scaled(Vec3::new(0.0, -0.5, 20.0), Vec3::new(160.0, 1.0, 160.0)),
        TRACK,
    );
    // Barriers around the arena.
    for (name, pos, size) in [
        (
            "Barrier North",
            Vec3::new(0.0, 0.6, 90.0),
            Vec3::new(150.0, 1.2, 1.0),
        ),
        (
            "Barrier South",
            Vec3::new(0.0, 0.6, -50.0),
            Vec3::new(150.0, 1.2, 1.0),
        ),
        (
            "Barrier East",
            Vec3::new(75.0, 0.6, 20.0),
            Vec3::new(1.0, 1.2, 140.0),
        ),
        (
            "Barrier West",
            Vec3::new(-75.0, 0.6, 20.0),
            Vec3::new(1.0, 1.2, 140.0),
        ),
    ] {
        block(scene, name, scaled(pos, size), STEEL);
    }
    // A ramp straight ahead (15°, 2.4 m high) and a landing deck past it.
    block(
        scene,
        "Ramp",
        scaled(Vec3::new(0.0, 0.85, 30.0), Vec3::new(5.0, 0.4, 9.6))
            .with_euler(Vec3::new(-15.0, 0.0, 0.0)),
        PAVEMENT,
    );
    block(
        scene,
        "Ramp Deck",
        scaled(Vec3::new(0.0, 1.1, 38.6), Vec3::new(5.0, 2.2, 8.0)),
        PAVEMENT,
    );
    block(
        scene,
        "Ramp Down",
        scaled(Vec3::new(0.0, 0.85, 47.2), Vec3::new(5.0, 0.4, 9.6))
            .with_euler(Vec3::new(15.0, 0.0, 0.0)),
        PAVEMENT,
    );
    // Speed bumps on the left lane.
    for i in 0..4 {
        let z = 8.0 + i as f32 * 7.0;
        block(
            scene,
            &format!("Speed Bump {}", i + 1),
            scaled(Vec3::new(-12.0, 0.04, z), Vec3::new(6.0, 0.16, 0.7)),
            CONE,
        );
    }
    // A slalom of traffic cones on the right.
    for i in 0..7 {
        let z = 6.0 + i as f32 * 6.0;
        let x = 12.0 + if i % 2 == 0 { -1.5 } else { 1.5 };
        let c = mesh(
            scene,
            &format!("Cone {}", i + 1),
            None,
            PrimitiveMesh::Cylinder,
            scaled(Vec3::new(x, 0.35, z), Vec3::new(0.4, 0.35, 0.4)),
            CONE,
        );
        scene.add(c, BoxCollider::default());
        scene.add(
            c,
            Rigidbody {
                mass: 4.0,
                ..Default::default()
            },
        );
    }
    // A wall of crates to drive through.
    for row in 0..3 {
        for col in 0..4 {
            let p = Vec3::new(
                22.0 + col as f32 * 1.0 + if row % 2 == 1 { 0.5 } else { 0.0 },
                0.45 + row as f32 * 0.9,
                20.0,
            );
            let c = mesh(
                scene,
                &format!("Crate {}", row * 4 + col + 1),
                None,
                cube,
                scaled(p, Vec3::splat(0.9)),
                WOOD,
            );
            scene.add(c, BoxCollider::default());
            scene.add(
                c,
                Rigidbody {
                    mass: 25.0,
                    ..Default::default()
                },
            );
        }
    }
    car(scene);
    if let Some(cam) = scene.find_by_name("Main Camera") {
        // Where the follow script holds it: behind and above the car, looking at it.
        let pos = Vec3::new(0.0, RIDE_HEIGHT + 2.8, -7.5);
        let look = Vec3::new(0.0, RIDE_HEIGHT + 0.8, 0.0) - pos;
        let pitch = (-look.y).atan2(look.z).to_degrees();
        scene.set_transform(
            cam,
            Transform::from_position(pos).with_euler(Vec3::new(pitch, 0.0, 0.0)),
        );
        scene.add(cam, script(FOLLOW));
    }

    // Speed read-out and controls, top left.
    for (name, text, size, y) in [
        ("Speed Text", "0 km/h", 40.0, -16.0),
        (
            "Controls Text",
            "W/S throttle and brake   A/D steer   Space handbrake   R reset",
            20.0,
            -66.0,
        ),
    ] {
        crate::commands::apply(
            &mut s,
            Action::Create {
                what: Create::Ui(UiElement::Text),
                parent: None,
            },
        );
        let e = s.selection.active().expect("created text is selected");
        s.scene.game_object_mut(e).name = name.into();
        let w = &mut s.scene.world;
        *w.get::<&mut RectTransform>(e).unwrap() = RectTransform {
            anchor_min: Vec2::new(0.0, 1.0),
            anchor_max: Vec2::new(0.0, 1.0),
            pivot: Vec2::new(0.0, 1.0),
            anchored_position: Vec2::new(20.0, y),
            size_delta: Vec2::new(900.0, 60.0),
        };
        let mut t = w.get::<&mut UiText>(e).unwrap();
        t.text = text.into();
        t.font_size = size;
        t.color = [1.0; 4];
    }
    s.scene
}

fn scripts() -> [(&'static str, &'static str); 2] {
    [(CONTROLLER, CAR_CONTROLLER), (FOLLOW, FOLLOW_CAMERA)]
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_vehicle_demo() {
    let root = project();
    for (rel, m) in materials() {
        m.save(&root.join(rel)).unwrap();
    }
    for (rel, src) in scripts() {
        std::fs::write(root.join(rel), src).unwrap();
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn vehicle_demo_matches_its_generator() {
    let root = project();
    for (rel, m) in materials() {
        crate::test_util::assert_close(&Material::load(&root.join(rel)).unwrap(), &m, rel);
    }
    for (rel, src) in scripts() {
        let on_disk = std::fs::read_to_string(root.join(rel)).unwrap();
        assert_eq!(on_disk.replace("\r\n", "\n"), src, "{rel}");
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_vehicle_demo",
    );
    let (scene, report) = Scene::from_json("VehicleDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // Every wheel keeps its model after loading.
    for (name, _, _) in WHEELS {
        let w = scene.find_by_name(name).unwrap();
        let model = scene.world.get::<&WheelCollider>(w).unwrap().wheel_model;
        assert_eq!(model, scene.find_by_name(&format!("{name} Model")));
    }
}

/// Plays the demo with injected keys: the car settles on its wheels, drives forward on W,
/// turns on D, stops on S, and the camera follows.
#[test]
fn vehicle_demo_drives() {
    use misty_engine::runtime::Time;
    use misty_engine::scripting::{Input, LogKind, ScriptHost};
    let root = project();
    let mut scene = demo_scene();
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    let mut frames = |scene: &mut Scene, keys: &[&str], n: usize| {
        let mut input = Input::default();
        input.held.extend(keys.iter().map(|k| k.to_string()));
        for _ in 0..n {
            time.advance(1.0 / 60.0);
            host.update(scene, &time, input.clone());
        }
        for (kind, text) in host.take_logs() {
            assert_ne!(kind, LogKind::Error, "{text}");
        }
    };
    let car = scene.find_by_name("Car").unwrap();
    let wheel = scene.find_by_name("Wheel FL").unwrap();
    frames(&mut scene, &[], 60);
    let rest = scene.world_position(car);
    assert!(
        (rest.y - RIDE_HEIGHT).abs() < 0.05,
        "rests on its wheels: {rest}"
    );
    let model = scene.find_by_name("Wheel FL Model").unwrap();
    let wheel_y = scene.world_position(model).y;
    assert!(
        (wheel_y - WHEEL_RADIUS).abs() < 0.05,
        "tyre on the ground: {wheel_y}"
    );

    frames(&mut scene, &["w"], 120);
    let p = scene.world_position(car);
    assert!(p.z > rest.z + 5.0 && p.x.abs() < 1.0, "drove forward: {p}");
    let s = scene.world.get::<&WheelCollider>(wheel).unwrap().state;
    assert!(s.is_grounded && s.rpm > 50.0, "{s:?}");

    frames(&mut scene, &["w", "d"], 60);
    let heading = scene.world_rotation(car) * Vec3::Z;
    assert!(heading.x > 0.15, "turned right: {heading}");
    let speed_text = scene.find_by_name("Speed Text").unwrap();
    let shown = scene.world.get::<&UiText>(speed_text).unwrap().text.clone();
    assert!(shown.ends_with("km/h") && shown != "0 km/h", "{shown}");

    let speed = |scene: &Scene| {
        let v = scene.world.get::<&WheelCollider>(wheel).unwrap().state.rpm;
        v / 60.0 * std::f32::consts::TAU * WHEEL_RADIUS
    };
    let fast = speed(&scene);
    frames(&mut scene, &["s"], 30);
    let slower = speed(&scene);
    assert!(slower < fast * 0.8, "S brakes: {fast} → {slower}");
    // The handbrake locks the rear wheels and the car comes to a stop.
    frames(&mut scene, &["space"], 180);
    let after = scene.world_position(car);
    frames(&mut scene, &["space"], 30);
    let moved = (scene.world_position(car) - after).length();
    assert!(moved < 0.2, "stopped: moved {moved}");

    let cam = scene.find_by_name("Main Camera").unwrap();
    let gap = scene.world_position(cam) - scene.world_position(car);
    assert!(
        gap.length() < 12.0 && gap.y > 1.5,
        "the camera follows: {gap}"
    );
}
