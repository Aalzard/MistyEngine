//! Procedural PhysicsDemo2 scene in `SampleProject`: ragdolls, cloth, breakable joints and
//! physics materials.
//!
//! - **Dummy**: a jointed character of primitives playing an idle clip, made a ragdoll by the
//!   Ragdoll Wizard (`physics::ragdoll::create`). Left click throws a ball where the mouse
//!   points; a ball that hits the dummy switches it to physics (`Ragdoll.Enable`); R blends it
//!   back to the animation.
//! - **Flag**: a cloth flag (`Assets/Models/Flag.glb`, pinned along the pole by its vertex
//!   colours) streaming in a Wind Zone.
//! - **Bridge**: planks on Hinge Joints (and a Configurable Joint at the far end) with break
//!   forces; a heavy anvil falls on it and snaps it (`OnJointBreak` logs each plank).
//! - **Ramp**: an ice block slides off, a rubber block grips, a rubber ball bounces
//!   (`Ice.physicmat`, `Rubber.physicmat`).
//!
//! Regenerate with `cargo test -p misty_editor -- --ignored write_physics_demo2`; the other
//! tests check the committed files still load and match, and that the scene plays.

use std::path::{Path, PathBuf};

use glam::{Quat, Vec3};
use misty_engine::animation::{AnimationClip, Property};
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::physics::cloth::flag;
use misty_engine::physics::material::{Combine, PhysicsMaterial};
use misty_engine::physics::ragdoll::{self, RagdollOptions};
use misty_engine::scene::{
    Animation, BoxCollider, Cloth, ColorChannel, ConfigurableJoint, HingeJoint, Interpolation,
    JointMotion, MeshFilter, MeshRef, MeshRenderer, PrimitiveMesh, Ragdoll, Rigidbody, Scene,
    ScriptRef, Scripts, SkinnedMeshRenderer, SphereCollider, Transform, WindZone,
};

const SCENE: &str = "Assets/Scenes/PhysicsDemo2.scene";
const FLAG_MODEL: &str = "Assets/Models/Flag.glb";
const ICE: &str = "Assets/PhysicsMaterials/Ice.physicmat";
const RUBBER: &str = "Assets/PhysicsMaterials/Rubber.physicmat";
const MATERIALS_DIR: &str = "Assets/PhysicsMaterials";
const FLAG_MAT: &str = "Assets/Materials/Flag.mat";
const ICE_MAT: &str = "Assets/Materials/Ice.mat";
const RUBBER_MAT: &str = "Assets/Materials/Rubber.mat";
const DUMMY_MAT: &str = "Assets/Materials/Dummy.mat";
const IDLE: &str = "Assets/Animations/DummyIdle.anim";
const WOOD: &str = "Assets/Materials/Wood.mat";
const CHECKER: &str = "Assets/Materials/Checker.mat";
const RED: &str = "Assets/Materials/Red.mat";
const THROWER: &str = "Assets/Scripts/BallThrower.lua";
const BALL: &str = "Assets/Scripts/RagdollBall.lua";
const GET_UP: &str = "Assets/Scripts/RagdollGetUp.lua";
const PLANK: &str = "Assets/Scripts/BridgePlank.lua";

/// Bridge joints give way above this force (N): the planks' own weight is far below it, the
/// anvil's landing far above.
const PLANK_BREAK_FORCE: f32 = 6000.0;

const BALL_THROWER: &str = r#"-- BallThrower: left click throws a ball where the mouse points.
-- Hit the dummy to knock it over (R gets it back up), or the blocks on the ramp.
local BallThrower = {
    speed = 18,
    template = "Ball Template",
}

function BallThrower:Start()
    self._template = GameObject.Find(self.template)
end

function BallThrower:Update()
    if not Input.GetMouseButtonDown(0) or self._template == nil then return end
    local ray = Camera.main:ScreenPointToRay(Input.mousePosition)
    if ray == nil then return end
    local ball = Instantiate(self._template, ray.origin + ray.direction * 1.5)
    local body = ball:GetComponent("Rigidbody")
    body.isKinematic = false
    body.useGravity = true
    body.velocity = ray.direction * self.speed
end

return BallThrower
"#;

const RAGDOLL_BALL: &str = r#"-- RagdollBall: a ball that hits a ragdoll's bone switches the ragdoll to physics
-- and passes on a shove.
local RagdollBall = {
    shove = 6,
}

function RagdollBall:Start()
    self.body = self.gameObject:GetComponent("Rigidbody")
end

function RagdollBall:OnCollisionEnter(other)
    if self.body.isKinematic then return end
    if Ragdoll.Enable(other) then
        local bone = other:GetComponent("Rigidbody")
        local v = self.body.velocity
        if bone ~= nil then
            bone.velocity = bone.velocity + v.normalized * self.shove
        end
        Debug.Log("Knocked over by a ball")
    end
end

return RagdollBall
"#;

const RAGDOLL_GET_UP: &str = r#"-- RagdollGetUp: R blends the ragdoll back to its animation; K knocks it over.
local RagdollGetUp = {
    blendTime = 0.6,
}

function RagdollGetUp:Update()
    if Input.GetKeyDown("r") then
        Ragdoll.Disable(self.gameObject, self.blendTime)
    end
    if Input.GetKeyDown("k") then
        Ragdoll.Enable(self.gameObject)
    end
end

return RagdollGetUp
"#;

const BRIDGE_PLANK: &str = r#"-- BridgePlank: reports when its joint breaks (Break Force on the joint).
local BridgePlank = {}

function BridgePlank:OnJointBreak(force)
    Debug.Log("Bridge plank snapped:", self.gameObject.name, math.floor(force), "N")
end

return BridgePlank
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn color(c: [f32; 3], smooth: f32) -> Material {
    Material {
        color: [c[0], c[1], c[2], 1.0],
        smoothness: smooth,
        ..Default::default()
    }
}

/// The materials written by the generator: (path, material).
fn materials() -> Vec<(&'static str, Material)> {
    vec![
        (
            FLAG_MAT,
            Material {
                double_sided: true,
                ..color([0.55, 0.32, 0.85], 0.2)
            },
        ),
        (ICE_MAT, color([0.62, 0.86, 0.98], 0.9)),
        (RUBBER_MAT, color([0.12, 0.12, 0.13], 0.15)),
        (DUMMY_MAT, color([0.95, 0.62, 0.25], 0.35)),
    ]
}

fn ice() -> PhysicsMaterial {
    PhysicsMaterial {
        dynamic_friction: 0.02,
        static_friction: 0.05,
        bounciness: 0.0,
        friction_combine: Combine::Minimum,
        bounce_combine: Combine::Average,
    }
}

fn rubber() -> PhysicsMaterial {
    PhysicsMaterial {
        dynamic_friction: 1.0,
        static_friction: 1.2,
        bounciness: 0.75,
        friction_combine: Combine::Maximum,
        bounce_combine: Combine::Maximum,
    }
}

/// Dummy bone paths (below the Dummy) and their rest positions.
const BONES: [(&str, [f32; 3]); 17] = [
    ("Hips", [0.0, 1.0, 0.0]),
    ("Hips/Spine", [0.0, 0.15, 0.0]),
    ("Hips/Spine/Chest", [0.0, 0.2, 0.0]),
    ("Hips/Spine/Chest/Neck", [0.0, 0.3, 0.0]),
    ("Hips/Spine/Chest/Neck/Head", [0.0, 0.08, 0.0]),
    ("Hips/Spine/Chest/LeftUpperArm", [-0.27, 0.25, 0.0]),
    (
        "Hips/Spine/Chest/LeftUpperArm/LeftLowerArm",
        [0.0, -0.28, 0.0],
    ),
    (
        "Hips/Spine/Chest/LeftUpperArm/LeftLowerArm/LeftHand",
        [0.0, -0.27, 0.0],
    ),
    ("Hips/Spine/Chest/RightUpperArm", [0.27, 0.25, 0.0]),
    (
        "Hips/Spine/Chest/RightUpperArm/RightLowerArm",
        [0.0, -0.28, 0.0],
    ),
    (
        "Hips/Spine/Chest/RightUpperArm/RightLowerArm/RightHand",
        [0.0, -0.27, 0.0],
    ),
    ("Hips/LeftUpperLeg", [-0.1, -0.06, 0.0]),
    ("Hips/LeftUpperLeg/LeftLowerLeg", [0.0, -0.44, 0.0]),
    ("Hips/LeftUpperLeg/LeftLowerLeg/LeftFoot", [0.0, -0.42, 0.0]),
    ("Hips/RightUpperLeg", [0.1, -0.06, 0.0]),
    ("Hips/RightUpperLeg/RightLowerLeg", [0.0, -0.44, 0.0]),
    (
        "Hips/RightUpperLeg/RightLowerLeg/RightFoot",
        [0.0, -0.42, 0.0],
    ),
];

/// Visible part of a bone: (bone name, shape, offset, size).
const PARTS: [(&str, PrimitiveMesh, [f32; 3], [f32; 3]); 16] = [
    (
        "Hips",
        PrimitiveMesh::Cube,
        [0.0, 0.0, 0.0],
        [0.34, 0.2, 0.2],
    ),
    (
        "Chest",
        PrimitiveMesh::Cube,
        [0.0, 0.12, 0.0],
        [0.42, 0.38, 0.22],
    ),
    (
        "Head",
        PrimitiveMesh::Sphere,
        [0.0, 0.13, 0.0],
        [0.26, 0.26, 0.26],
    ),
    (
        "LeftUpperArm",
        PrimitiveMesh::Capsule,
        [0.0, -0.14, 0.0],
        [0.1, 0.14, 0.1],
    ),
    (
        "LeftLowerArm",
        PrimitiveMesh::Capsule,
        [0.0, -0.13, 0.0],
        [0.09, 0.13, 0.09],
    ),
    (
        "LeftHand",
        PrimitiveMesh::Sphere,
        [0.0, -0.03, 0.0],
        [0.09, 0.09, 0.09],
    ),
    (
        "RightUpperArm",
        PrimitiveMesh::Capsule,
        [0.0, -0.14, 0.0],
        [0.1, 0.14, 0.1],
    ),
    (
        "RightLowerArm",
        PrimitiveMesh::Capsule,
        [0.0, -0.13, 0.0],
        [0.09, 0.13, 0.09],
    ),
    (
        "RightHand",
        PrimitiveMesh::Sphere,
        [0.0, -0.03, 0.0],
        [0.09, 0.09, 0.09],
    ),
    (
        "LeftUpperLeg",
        PrimitiveMesh::Capsule,
        [0.0, -0.22, 0.0],
        [0.14, 0.22, 0.14],
    ),
    (
        "LeftLowerLeg",
        PrimitiveMesh::Capsule,
        [0.0, -0.21, 0.0],
        [0.12, 0.21, 0.12],
    ),
    (
        "LeftFoot",
        PrimitiveMesh::Cube,
        [0.0, -0.02, 0.06],
        [0.12, 0.08, 0.24],
    ),
    (
        "RightUpperLeg",
        PrimitiveMesh::Capsule,
        [0.0, -0.22, 0.0],
        [0.14, 0.22, 0.14],
    ),
    (
        "RightLowerLeg",
        PrimitiveMesh::Capsule,
        [0.0, -0.21, 0.0],
        [0.12, 0.21, 0.12],
    ),
    (
        "RightFoot",
        PrimitiveMesh::Cube,
        [0.0, -0.02, 0.06],
        [0.12, 0.08, 0.24],
    ),
    (
        "Spine",
        PrimitiveMesh::Cube,
        [0.0, 0.08, 0.0],
        [0.3, 0.16, 0.18],
    ),
];

/// The idle clip: every bone keyed at its rest pose (so blending back from the ragdoll restores
/// the whole body), the chest breathing and the arms swaying a little.
fn idle() -> AnimationClip {
    let mut c = AnimationClip {
        length: 2.0,
        looping: true,
        ..Default::default()
    };
    for (path, p) in BONES.iter() {
        for (i, v) in p.iter().enumerate() {
            for t in [0.0, 1.0, 2.0] {
                c.set_key(path, Property::Position(i as u8), t, *v);
            }
        }
        let name = path.rsplit('/').next().unwrap_or(path);
        let sway: [f32; 3] = match name {
            "Chest" => [3.0, 0.0, 0.0],
            "LeftUpperArm" => [0.0, 0.0, -5.0],
            "RightUpperArm" => [0.0, 0.0, 5.0],
            "Head" => [-2.0, 4.0, 0.0],
            _ => [0.0; 3],
        };
        for (i, s) in sway.iter().enumerate() {
            c.set_key(path, Property::Rotation(i as u8), 0.0, 0.0);
            c.set_key(path, Property::Rotation(i as u8), 1.0, *s);
            c.set_key(path, Property::Rotation(i as u8), 2.0, 0.0);
        }
    }
    c
}

fn mesh(s: &mut Scene, e: Entity, shape: PrimitiveMesh, material: &str) {
    s.add(e, MeshFilter::primitive(shape));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
}

/// A mesh object; `collider` adds a box (or sphere) collider.
fn shape(
    s: &mut Scene,
    name: &str,
    kind: PrimitiveMesh,
    t: Transform,
    material: &str,
    collider: bool,
) -> Entity {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    mesh(s, e, kind, material);
    if collider {
        match kind {
            PrimitiveMesh::Sphere => s.add(e, SphereCollider::default()),
            _ => s.add(e, BoxCollider::default()),
        }
    }
    e
}

fn placed(pos: Vec3, scale: Vec3, euler: Vec3) -> Transform {
    let mut t = Transform::from_position(pos).with_euler(euler);
    t.scale = scale;
    t
}

fn script(s: &mut Scene, e: Entity, path: &str) {
    s.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: path.into(),
                ..Default::default()
            }],
        },
    );
}

/// The dummy: bones, their visible parts, the idle clip and the wizard's ragdoll.
fn dummy(s: &mut Scene) -> Entity {
    let root = s.spawn("Dummy", None);
    s.set_transform(
        root,
        Transform::from_position(Vec3::new(0.0, 0.0, 0.0)).with_euler(Vec3::new(0.0, 180.0, 0.0)),
    );
    let mut made: Vec<(String, Entity)> = Vec::new();
    for (path, p) in BONES.iter() {
        let (parent, name) = match path.rsplit_once('/') {
            Some((parent, name)) => (
                made.iter()
                    .find(|(m, _)| m == parent)
                    .map(|(_, e)| *e)
                    .expect("parents come first"),
                name,
            ),
            None => (root, *path),
        };
        let e = s.spawn(name, Some(parent));
        s.set_transform(e, Transform::from_position(Vec3::from(*p)));
        made.push(((*path).to_owned(), e));
    }
    for (bone, kind, offset, size) in PARTS {
        let parent = made
            .iter()
            .find(|(p, _)| p.rsplit('/').next() == Some(bone))
            .map(|(_, e)| *e)
            .expect("bone exists");
        let e = s.spawn(&format!("{bone} Mesh"), Some(parent));
        s.set_transform(e, placed(Vec3::from(offset), Vec3::from(size), Vec3::ZERO));
        mesh(s, e, kind, DUMMY_MAT);
    }
    s.add(
        root,
        Animation {
            clip: Some(IDLE.into()),
            clips: vec![IDLE.into()],
            ..Default::default()
        },
    );
    let bones = ragdoll::auto_fill(s, root);
    ragdoll::create(
        s,
        root,
        &bones,
        &RagdollOptions {
            total_mass: 60.0,
            ..Default::default()
        },
    )
    .expect("the dummy has every bone");
    if let Ok(mut r) = s.world.get::<&mut Ragdoll>(root) {
        r.blend_time = 0.6;
    }
    script(s, root, GET_UP);
    root
}

/// Max distances from the flag's colour channel (red × 3 m: 0 pins the pole edge), for the
/// imported mesh's vertices (read from the project's `Flag.glb`, by its full path: other tests
/// move the global project root).
fn flag_max_distances() -> Vec<f32> {
    let abs = project()
        .join(FLAG_MODEL)
        .to_string_lossy()
        .replace('\\', "/");
    flag::max_distances_from_colors(&MeshRef::asset(&abs, 0), ColorChannel::Red, 3.0)
        .expect("write_physics_demo2 writes the flag first")
}

fn flag_pole(s: &mut Scene) -> Entity {
    let base = Vec3::new(-6.0, 0.0, 3.0);
    shape(
        s,
        "Flag Pole",
        PrimitiveMesh::Cylinder,
        placed(
            base + Vec3::new(0.0, 2.6, 0.0),
            Vec3::new(0.12, 2.6, 0.12),
            Vec3::ZERO,
        ),
        WOOD,
        true,
    );
    let f = s.spawn("Flag", None);
    s.set_transform(
        f,
        Transform::from_position(base + Vec3::new(0.07, 5.0, 0.0)),
    );
    s.add(
        f,
        SkinnedMeshRenderer {
            mesh: Some(MeshRef::asset(FLAG_MODEL, 0)),
            materials: vec![Some(FLAG_MAT.into())],
            update_when_offscreen: true,
            ..Default::default()
        },
    );
    s.add(
        f,
        Cloth {
            stretching_stiffness: 0.95,
            bending_stiffness: 0.3,
            damping: 0.15,
            wind_scale: 1.2,
            max_distances: flag_max_distances(),
            color_channel: ColorChannel::Red,
            color_scale: 3.0,
            ..Default::default()
        },
    );
    let wind = s.spawn("Wind Zone", None);
    // Blowing towards +X: the flag streams away from its pole.
    s.set_transform(
        wind,
        Transform::from_position(base + Vec3::new(0.0, 6.0, 0.0))
            .with_euler(Vec3::new(0.0, 90.0, 0.0)),
    );
    s.add(
        wind,
        WindZone {
            main: 1.0,
            turbulence: 0.8,
            pulse_magnitude: 0.8,
            pulse_frequency: 0.6,
            ..Default::default()
        },
    );
    f
}

/// Planks sagging between two pillars, hinged to each other, with break forces.
fn bridge(s: &mut Scene) {
    let (left, right, top) = (Vec3::new(3.0, 3.0, 3.0), Vec3::new(10.0, 3.0, 3.0), 3.0);
    for (name, x) in [
        ("Bridge Pillar L", left.x - 0.5),
        ("Bridge Pillar R", right.x + 0.5),
    ] {
        shape(
            s,
            name,
            PrimitiveMesh::Cube,
            placed(
                Vec3::new(x, top * 0.5, 3.0),
                Vec3::new(1.0, top, 2.0),
                Vec3::ZERO,
            ),
            CHECKER,
            true,
        );
    }
    let n = 9;
    let sag = 0.45;
    let point = |k: usize| {
        let t = k as f32 / n as f32;
        left.lerp(right, t) - Vec3::Y * (sag * 4.0 * t * (1.0 - t))
    };
    let mut prev: Option<Entity> = None;
    for k in 0..n {
        let (a, b) = (point(k), point(k + 1));
        let d = b - a;
        let angle = d.y.atan2(d.x).to_degrees();
        let plank = shape(
            s,
            &format!("Plank {}", k + 1),
            PrimitiveMesh::Cube,
            placed(
                (a + b) * 0.5,
                Vec3::new(d.length(), 0.15, 1.6),
                Vec3::new(0.0, 0.0, angle),
            ),
            WOOD,
            true,
        );
        s.add(
            plank,
            Rigidbody {
                mass: 10.0,
                interpolation: Interpolation::Interpolate,
                ..Default::default()
            },
        );
        s.add(
            plank,
            HingeJoint {
                connected_body: prev,
                anchor: Vec3::new(-0.5, 0.0, 0.0),
                axis: Vec3::Z,
                break_force: PLANK_BREAK_FORCE,
                ..Default::default()
            },
        );
        if k + 1 == n {
            // The far end: a Configurable Joint to the pillar, turning about Z only.
            s.add(
                plank,
                ConfigurableJoint {
                    anchor: Vec3::new(0.5, 0.0, 0.0),
                    axis: Vec3::Z,
                    secondary_axis: Vec3::Y,
                    x_motion: JointMotion::Locked,
                    y_motion: JointMotion::Locked,
                    z_motion: JointMotion::Locked,
                    angular_x_motion: JointMotion::Free,
                    angular_y_motion: JointMotion::Locked,
                    angular_z_motion: JointMotion::Locked,
                    break_force: PLANK_BREAK_FORCE,
                    ..Default::default()
                },
            );
        }
        script(s, plank, PLANK);
        prev = Some(plank);
    }
    let anvil = shape(
        s,
        "Anvil",
        PrimitiveMesh::Cube,
        placed(Vec3::new(6.5, 9.0, 3.0), Vec3::splat(0.8), Vec3::ZERO),
        RED,
        true,
    );
    s.add(
        anvil,
        Rigidbody {
            mass: 400.0,
            interpolation: Interpolation::Interpolate,
            ..Default::default()
        },
    );
}

/// The ramp with an ice block, a rubber block and a rubber ball.
fn ramp(s: &mut Scene) {
    let tilt = 20.0f32;
    let centre = Vec3::new(0.0, 1.0, 7.0);
    shape(
        s,
        "Ramp",
        PrimitiveMesh::Cube,
        placed(centre, Vec3::new(7.0, 0.3, 3.0), Vec3::new(0.0, 0.0, -tilt)),
        CHECKER,
        true,
    );
    let rot = Quat::from_rotation_z(-tilt.to_radians());
    let on_ramp = |x: f32, z: f32, h: f32| centre + rot * Vec3::new(x, 0.15 + h, z);
    for (name, mat, physics, z) in [
        ("Ice Block", ICE_MAT, ICE, -0.7),
        ("Rubber Block", RUBBER_MAT, RUBBER, 0.7),
    ] {
        let b = shape(
            s,
            name,
            PrimitiveMesh::Cube,
            placed(
                on_ramp(-2.6, z, 0.25),
                Vec3::splat(0.5),
                Vec3::new(0.0, 0.0, -tilt),
            ),
            mat,
            false,
        );
        s.add(
            b,
            BoxCollider {
                material: Some(physics.into()),
                ..Default::default()
            },
        );
        s.add(b, Rigidbody::default());
    }
    let ball = shape(
        s,
        "Rubber Ball",
        PrimitiveMesh::Sphere,
        placed(Vec3::new(2.5, 5.0, 4.5), Vec3::splat(0.5), Vec3::ZERO),
        RUBBER_MAT,
        false,
    );
    s.add(
        ball,
        SphereCollider {
            material: Some(RUBBER.into()),
            ..Default::default()
        },
    );
    s.add(ball, Rigidbody::default());
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("PhysicsDemo2");
    let cam = s.find_by_name("Main Camera").unwrap();
    s.set_transform(
        cam,
        Transform::from_position(Vec3::new(1.5, 4.5, -12.0)).with_euler(Vec3::new(12.0, 0.0, 0.0)),
    );
    script(&mut s, cam, THROWER);
    shape(
        &mut s,
        "Ground",
        PrimitiveMesh::Cube,
        placed(
            Vec3::new(0.0, -0.5, 0.0),
            Vec3::new(40.0, 1.0, 40.0),
            Vec3::ZERO,
        ),
        CHECKER,
        true,
    );
    dummy(&mut s);
    flag_pole(&mut s);
    bridge(&mut s);
    ramp(&mut s);
    // Thrown balls are copies of this one (parked out of sight, kinematic).
    let template = shape(
        &mut s,
        "Ball Template",
        PrimitiveMesh::Sphere,
        placed(Vec3::new(0.0, -50.0, 0.0), Vec3::splat(0.35), Vec3::ZERO),
        RED,
        true,
    );
    s.add(
        template,
        Rigidbody {
            mass: 4.0,
            is_kinematic: true,
            use_gravity: false,
            continuous: true,
            ..Default::default()
        },
    );
    script(&mut s, template, BALL);
    s
}

/// A `.meta` with a fresh GUID unless the asset (or folder) has one.
fn ensure_meta(path: &Path) {
    if misty_engine::asset_db::meta::read_guid(path).is_none() {
        misty_engine::asset_db::meta::write_guid(path, misty_engine::asset_db::Guid::new())
            .unwrap();
    }
}

const SCRIPTS: [(&str, &str); 4] = [
    (THROWER, BALL_THROWER),
    (BALL, RAGDOLL_BALL),
    (GET_UP, RAGDOLL_GET_UP),
    (PLANK, BRIDGE_PLANK),
];

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_physics_demo2() {
    let root = project();
    std::fs::write(root.join(FLAG_MODEL), flag::flag_glb()).unwrap();
    ensure_meta(&root.join(FLAG_MODEL));
    std::fs::create_dir_all(root.join(MATERIALS_DIR)).unwrap();
    ensure_meta(&root.join(MATERIALS_DIR));
    for (path, m) in [(ICE, ice()), (RUBBER, rubber())] {
        m.save(&root.join(path)).unwrap();
        ensure_meta(&root.join(path));
    }
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
        ensure_meta(&root.join(path));
    }
    idle().save(&root.join(IDLE)).unwrap();
    ensure_meta(&root.join(IDLE));
    for (path, text) in SCRIPTS {
        std::fs::write(root.join(path), text).unwrap();
        ensure_meta(&root.join(path));
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
    ensure_meta(&root.join(SCENE));
}

#[test]
fn physics_demo2_matches_its_generator() {
    let root = project();
    assert_eq!(
        std::fs::read(root.join(FLAG_MODEL)).unwrap(),
        flag::flag_glb(),
        "{FLAG_MODEL}: regenerate with write_physics_demo2"
    );
    for (path, m) in [(ICE, ice()), (RUBBER, rubber())] {
        assert_eq!(
            PhysicsMaterial::load(&root.join(path)).unwrap(),
            m,
            "{path}"
        );
    }
    for (path, m) in materials() {
        crate::test_util::assert_close(&Material::load(&root.join(path)).unwrap(), &m, path);
    }
    crate::test_util::assert_close(
        &AnimationClip::load(&root.join(IDLE)).unwrap(),
        &idle(),
        IDLE,
    );
    for (path, text) in SCRIPTS {
        let on_disk = std::fs::read_to_string(root.join(path)).unwrap();
        assert_eq!(on_disk.replace("\r\n", "\n"), text, "{path}");
    }
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    crate::test_util::assert_json_close(&scene.to_json(), &demo_scene().to_json(), SCENE);
}

/// The demo plays: the dummy stands on its animation, falls when its ragdoll is switched on
/// and gets back up; the anvil snaps the bridge; ice slides and rubber grips; the flag flies.
#[test]
fn physics_demo2_plays() {
    use misty_engine::runtime::Time;
    use misty_engine::scripting::{Input, LogKind, ScriptHost};
    let root = project();
    let mut scene = demo_scene();
    // Other tests move the global project root: point the assets the physics world loads
    // (Physics Materials, the flag mesh) at the sample project directly.
    let abs = |rel: &str| root.join(rel).to_string_lossy().replace('\\', "/");
    for c in scene.world.query_mut::<&mut BoxCollider>() {
        if let Some(m) = c.material.as_mut() {
            *m = abs(m);
        }
    }
    for c in scene.world.query_mut::<&mut SphereCollider>() {
        if let Some(m) = c.material.as_mut() {
            *m = abs(m);
        }
    }
    let flag_mesh = MeshRef::asset(&abs(FLAG_MODEL), 0);
    for r in scene.world.query_mut::<&mut SkinnedMeshRenderer>() {
        r.mesh = Some(flag_mesh.clone());
    }
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
    assert_eq!(physics.cloth_count(), 1);
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    let mut logs = Vec::new();
    let mut frame = |scene: &mut Scene, host: &mut ScriptHost, keys: &[&str]| {
        let mut input = Input::default();
        for k in keys {
            input.pressed.insert((*k).into());
        }
        time.advance(1.0 / 60.0);
        host.update(scene, &time, input);
        logs.extend(host.take_logs());
    };
    let head = scene.find_by_name("Head").unwrap();
    let ice_block = scene.find_by_name("Ice Block").unwrap();
    let rubber_block = scene.find_by_name("Rubber Block").unwrap();
    let (ice0, rubber0) = (
        scene.world_position(ice_block),
        scene.world_position(rubber_block),
    );
    for _ in 0..60 {
        frame(&mut scene, &mut host, &[]);
    }
    let standing = scene.world_position(head);
    assert!(standing.y > 1.6, "the animated dummy stands: {standing}");
    // K knocks it over.
    frame(&mut scene, &mut host, &["k"]);
    for _ in 0..150 {
        frame(&mut scene, &mut host, &[]);
    }
    let fallen = scene.world_position(head);
    assert!(fallen.y < 1.0, "the ragdoll fell: {fallen}");
    // R blends back to the idle clip.
    frame(&mut scene, &mut host, &["r"]);
    for _ in 0..60 {
        frame(&mut scene, &mut host, &[]);
    }
    let up = scene.world_position(head);
    assert!(up.y > 1.6, "back on its feet: {up}");

    for (kind, text) in &logs {
        assert_ne!(*kind, LogKind::Error, "{text}");
    }
    let snapped = logs
        .iter()
        .filter(|(_, t)| t.starts_with("Bridge plank snapped"))
        .count();
    assert!(snapped >= 1, "the anvil broke the bridge: {logs:?}");
    let anvil = scene.find_by_name("Anvil").unwrap();
    assert!(
        scene.world_position(anvil).y < 2.0,
        "the anvil fell through"
    );
    let slid = (scene.world_position(ice_block) - ice0).length();
    let held = (scene.world_position(rubber_block) - rubber0).length();
    assert!(slid > 2.0, "ice slides: {slid}");
    assert!(held < 0.15, "rubber grips: {held}");
    // The flag flies downwind (+X), off its pole.
    host.with_physics_scene(&mut scene, |p, s| p.finish_cloth(s));
    let flag = scene.find_by_name("Flag").unwrap();
    let pose = scene
        .world
        .get::<&misty_engine::physics::cloth::ClothPose>(flag)
        .expect("cloth simulated")
        .clone();
    let pole_x = scene.world_position(flag).x;
    let mean_x = pose.particles.iter().map(|p| p.x).sum::<f32>() / pose.particles.len() as f32;
    assert!(
        mean_x > pole_x + 0.6,
        "streaming out: {mean_x} vs pole {pole_x}"
    );
}
