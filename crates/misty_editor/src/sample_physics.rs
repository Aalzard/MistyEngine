//! Procedural PhysicsDemo scene in `SampleProject`: a capsule character (Character Controller,
//! W/S walk, A/D turn, Space jump) with a raycast laser pointer, a self-closing hinged door, a
//! motorised turnstile, a chain of spring joints, steps, a ramp and pushable crates.
//! Regenerate with `cargo test -p misty_editor -- --ignored write_physics_demo`; the other test
//! checks the committed files still load and match.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::physics::layers;
use misty_engine::scene::{
    BoxCollider, CharacterController, FixedJoint, HingeJoint, Interpolation, JointLimits,
    JointMotor, JointSpring, MeshFilter, MeshRenderer, PrimitiveMesh, Rigidbody, Scene, ScriptRef,
    Scripts, SphereCollider, SpringJoint, Transform,
};

const LASER_MAT: &str = "Assets/Materials/Laser.mat";
const WOOD: &str = "Assets/Materials/Wood.mat";
const CHECKER: &str = "Assets/Materials/Checker.mat";
const RED: &str = "Assets/Materials/Red.mat";
const WALKER: &str = "Assets/Scripts/CharacterWalker.lua";
const LASER: &str = "Assets/Scripts/LaserPointer.lua";
const SCENE: &str = "Assets/Scenes/PhysicsDemo.scene";

/// Character Controller movement with gravity, jumping and pushing rigidbodies.
const CHARACTER_WALKER: &str = r#"-- CharacterWalker: W/S walk, A/D turn, Space jumps. Needs a Character Controller.
-- Rigidbodies it walks into are pushed (OnControllerColliderHit).
local CharacterWalker = {
    speed = 4,
    turnSpeed = 120,
    jumpSpeed = 6,
    gravity = -15,
    pushPower = 2,
}

function CharacterWalker:Start()
    self.cc = self.gameObject:GetComponent("CharacterController")
    self._vy = 0
end

function CharacterWalker:Update()
    local dt = Time.deltaTime
    self.transform:Rotate(0, Input.GetAxis("Horizontal") * self.turnSpeed * dt, 0)
    local move = self.transform.forward * (Input.GetAxis("Vertical") * self.speed)
    if self.cc.isGrounded then
        self._vy = -1
        if Input.GetKeyDown("space") then self._vy = self.jumpSpeed end
    end
    self._vy = self._vy + self.gravity * dt
    self.cc:Move((move + Vector3.new(0, self._vy, 0)) * dt)
end

function CharacterWalker:OnControllerColliderHit(hit)
    local body = hit.rigidbody
    if body == nil or hit.moveDirection.y < -0.3 then return end
    body.velocity = Vector3.new(hit.moveDirection.x, 0, hit.moveDirection.z) * self.pushPower
end

return CharacterWalker
"#;

/// Physics.Raycast pointer: a beam stretched to what it hits, a dot on the hit point.
const LASER_POINTER: &str = r#"-- LaserPointer: casts a ray forward (ignoring the Player layer), stretches a beam to the hit
-- and puts a dot there. Left click or E gives the Rigidbody it points at a shove.
local LaserPointer = {
    range = 25,
    pushForce = 6,
    height = 0.4,
    beam = "Laser Beam",
    dot = "Laser Dot",
}

function LaserPointer:Start()
    self._beam = GameObject.Find(self.beam).transform
    self._dot = GameObject.Find(self.dot)
    self._mask = Physics.DefaultRaycastLayers & ~LayerMask.GetMask("Player")
end

function LaserPointer:LateUpdate()
    local forward = self.transform.forward
    local origin = self.transform.position + Vector3.up * self.height + forward * 0.5
    local hit = Physics.Raycast(origin, forward, self.range, self._mask)
    local length = self.range
    if hit ~= nil then length = hit.distance end
    self._beam.position = origin + forward * (length / 2)
    self._beam.eulerAngles = self.transform.eulerAngles
    self._beam.localScale = Vector3.new(0.03, 0.03, length)
    self._dot:SetActive(hit ~= nil)
    if hit == nil then return end
    self._dot.transform.position = hit.point
    if (Input.GetMouseButtonDown(0) or Input.GetKeyDown("e")) and hit.rigidbody ~= nil then
        hit.rigidbody:AddForce(forward * self.pushForce, ForceMode.Impulse)
    end
end

return LaserPointer
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Glowing red, so the beam reads as light.
fn laser() -> Material {
    Material {
        color: [1.0, 0.1, 0.08, 1.0],
        emission_enabled: true,
        emission: [1.0, 0.12, 0.08],
        emission_intensity: 2.5,
        ..Default::default()
    }
}

/// Mesh object with a material; `collider` adds the primitive's matching collider.
fn shape(
    s: &mut Scene,
    name: &str,
    mesh: PrimitiveMesh,
    t: Transform,
    material: &str,
    collider: bool,
) -> misty_engine::hecs::Entity {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(mesh));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
    if collider {
        match mesh {
            PrimitiveMesh::Sphere => s.add(e, SphereCollider::default()),
            _ => s.add(e, BoxCollider::default()),
        }
    }
    e
}

fn scaled(pos: Vec3, scale: Vec3) -> Transform {
    let mut t = Transform::from_position(pos);
    t.scale = scale;
    t
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("PhysicsDemo");
    let cube = PrimitiveMesh::Cube;
    shape(
        &mut s,
        "Ground",
        cube,
        scaled(Vec3::new(0.0, -0.5, 0.0), Vec3::new(30.0, 1.0, 30.0)),
        CHECKER,
        true,
    );

    // Player: capsule + Character Controller on the Player layer, camera following behind.
    let player = shape(
        &mut s,
        "Player",
        PrimitiveMesh::Capsule,
        Transform::from_position(Vec3::new(0.0, 1.1, -6.0)),
        RED,
        false,
    );
    s.game_object_mut(player).layer = layers::name_to_layer("Player") as u8;
    s.add(player, CharacterController::default());
    s.add(
        player,
        Scripts {
            list: vec![
                ScriptRef {
                    path: WALKER.into(),
                    ..Default::default()
                },
                ScriptRef {
                    path: LASER.into(),
                    ..Default::default()
                },
            ],
        },
    );
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_parent(cam, Some(player), None, false);
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 2.5, -5.5))
                .with_euler(Vec3::new(15.0, 0.0, 0.0)),
        );
    }
    // Laser beam and dot: no colliders, on Ignore Raycast.
    let ignore = layers::IGNORE_RAYCAST_LAYER;
    let beam = shape(
        &mut s,
        "Laser Beam",
        cube,
        // Where the script puts it at the start: from the player to the door.
        scaled(Vec3::new(0.0, 1.5, -2.77), Vec3::new(0.03, 0.03, 5.46)),
        LASER_MAT,
        false,
    );
    let dot = shape(
        &mut s,
        "Laser Dot",
        PrimitiveMesh::Sphere,
        scaled(Vec3::new(0.0, 1.5, -0.04), Vec3::splat(0.12)),
        LASER_MAT,
        false,
    );
    s.game_object_mut(beam).layer = ignore;
    s.game_object_mut(dot).layer = ignore;

    // Door straight ahead: two posts and a self-closing door hinged on the left post.
    for x in [-0.65, 0.65] {
        shape(
            &mut s,
            "Door Post",
            cube,
            scaled(Vec3::new(x, 1.25, 0.0), Vec3::new(0.3, 2.5, 0.3)),
            WOOD,
            true,
        );
    }
    let door = shape(
        &mut s,
        "Door",
        cube,
        scaled(Vec3::new(0.0, 1.1, 0.0), Vec3::new(0.96, 2.1, 0.08)),
        WOOD,
        true,
    );
    s.add(
        door,
        Rigidbody {
            mass: 4.0,
            angular_drag: 1.0,
            ..Default::default()
        },
    );
    s.add(
        door,
        HingeJoint {
            anchor: Vec3::new(-0.5, 0.0, 0.0),
            axis: Vec3::Y,
            use_spring: true,
            spring: JointSpring {
                spring: 15.0,
                damper: 3.0,
                target_position: 0.0,
            },
            use_limits: true,
            limits: JointLimits {
                min: -100.0,
                max: 100.0,
            },
            ..Default::default()
        },
    );

    // Turnstile driven by a hinge motor.
    let turnstile = shape(
        &mut s,
        "Turnstile",
        cube,
        scaled(Vec3::new(4.0, 0.6, 2.0), Vec3::new(3.0, 0.2, 0.2)),
        RED,
        true,
    );
    s.add(
        turnstile,
        Rigidbody {
            mass: 10.0,
            ..Default::default()
        },
    );
    s.add(
        turnstile,
        HingeJoint {
            axis: Vec3::new(0.0, 1.0, 0.0),
            use_motor: true,
            motor: JointMotor {
                target_velocity: 45.0,
                force: 200.0,
            },
            ..Default::default()
        },
    );

    // Spring chain hanging from a fixed anchor.
    let anchor = shape(
        &mut s,
        "Chain Anchor",
        cube,
        scaled(Vec3::new(-6.0, 5.0, 3.0), Vec3::splat(0.4)),
        WOOD,
        true,
    );
    s.add(
        anchor,
        Rigidbody {
            is_kinematic: true,
            ..Default::default()
        },
    );
    let mut previous = anchor;
    for i in 0..5 {
        let link = shape(
            &mut s,
            &format!("Chain Link {}", i + 1),
            PrimitiveMesh::Sphere,
            scaled(
                Vec3::new(-6.0, 4.4 - i as f32 * 0.6, 3.0),
                Vec3::splat(0.35),
            ),
            RED,
            true,
        );
        s.add(
            link,
            Rigidbody {
                mass: 0.5,
                drag: 0.3,
                interpolation: Interpolation::Interpolate,
                ..Default::default()
            },
        );
        s.add(
            link,
            SpringJoint {
                connected_body: Some(previous),
                spring: 40.0,
                damper: 1.0,
                ..Default::default()
            },
        );
        previous = link;
    }
    // A weight glued under the last link.
    let weight = shape(
        &mut s,
        "Chain Weight",
        cube,
        scaled(Vec3::new(-6.0, 1.4, 3.0), Vec3::splat(0.4)),
        WOOD,
        true,
    );
    s.add(
        weight,
        Rigidbody {
            mass: 0.5,
            ..Default::default()
        },
    );
    s.add(
        weight,
        FixedJoint {
            connected_body: Some(previous),
            ..Default::default()
        },
    );

    // Steps (0.25 m each, under the 0.3 m Step Offset) and a 25° ramp.
    for i in 0..3 {
        let h = 0.25 * (i + 1) as f32;
        shape(
            &mut s,
            &format!("Step {}", i + 1),
            cube,
            scaled(
                Vec3::new(4.0, h / 2.0, -4.0 + i as f32 * 0.8),
                Vec3::new(2.0, h, 0.8),
            ),
            WOOD,
            true,
        );
    }
    shape(
        &mut s,
        "Ramp",
        cube,
        scaled(Vec3::new(-6.0, 1.0, -5.0), Vec3::new(2.0, 0.3, 5.0))
            .with_euler(Vec3::new(-25.0, 0.0, 0.0)),
        WOOD,
        true,
    );

    // Crates to push or shoot with the laser.
    for (i, p) in [(1.5, 0.4, 3.0), (2.4, 0.4, 3.4), (1.9, 1.2, 3.2)]
        .into_iter()
        .enumerate()
    {
        let c = shape(
            &mut s,
            &format!("Crate {}", i + 1),
            cube,
            scaled(Vec3::from(p), Vec3::splat(0.8)),
            WOOD,
            true,
        );
        s.add(
            c,
            Rigidbody {
                interpolation: Interpolation::Interpolate,
                ..Default::default()
            },
        );
    }
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_physics_demo() {
    let root = project();
    laser().save(&root.join(LASER_MAT)).unwrap();
    std::fs::write(root.join(WALKER), CHARACTER_WALKER).unwrap();
    std::fs::write(root.join(LASER), LASER_POINTER).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn physics_demo_matches_its_generator() {
    let root = project();
    crate::test_util::assert_close(
        &Material::load(&root.join(LASER_MAT)).unwrap(),
        &laser(),
        LASER_MAT,
    );
    for (path, text) in [(WALKER, CHARACTER_WALKER), (LASER, LASER_POINTER)] {
        let on_disk = std::fs::read_to_string(root.join(path)).unwrap();
        assert_eq!(on_disk.replace("\r\n", "\n"), text, "{path}");
    }
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    crate::test_util::assert_json_close(&scene.to_json(), &demo_scene().to_json(), SCENE);
}

/// The demo runs: the character walks into the door and opens it, the laser finds it, the
/// chain hangs.
#[test]
fn physics_demo_plays() {
    use misty_engine::runtime::Time;
    use misty_engine::scripting::{Input, LogKind, ScriptHost};
    let root = project();
    let mut scene = demo_scene();
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    let mut input = Input::default();
    input.held.insert("w".into());
    for _ in 0..150 {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, input.clone());
    }
    for (kind, text) in host.take_logs() {
        assert_ne!(kind, LogKind::Error, "{text}");
    }
    let player = scene.find_by_name("Player").unwrap();
    let p = scene.world_position(player);
    assert!(
        p.z > -3.0 && (p.y - 1.0).abs() < 0.2,
        "walked forward on the ground: {p}"
    );
    let dot = scene.find_by_name("Laser Dot").unwrap();
    assert!(
        scene.game_object(dot).active,
        "the laser hits something ahead"
    );
    let door = scene.find_by_name("Door").unwrap();
    let yaw = scene
        .world_rotation(door)
        .to_euler(glam::EulerRot::YXZ)
        .0
        .to_degrees();
    assert!(
        yaw.abs() > 10.0,
        "the character pushed the door open: {yaw}"
    );
    let link = scene.find_by_name("Chain Link 5").unwrap();
    assert!(
        scene.world_position(link).y > 0.5,
        "the chain hangs from its anchor"
    );
}
