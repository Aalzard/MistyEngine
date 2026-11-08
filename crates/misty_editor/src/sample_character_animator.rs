//! The CharacterAnimatorDemo sample in `SampleProject`: a jointed "Character" built from
//! primitives, driven by an Animator Controller with three layers.
//!
//! - Base Layer: a Locomotion 2D Freeform Directional blend tree (Idle, Walk, Run, WalkBack,
//!   StrafeLeft, StrafeRight on MoveX / MoveZ) whose clips move the character by root motion
//!   (through its Character Controller), and an "Emotes" sub-state machine (Bow, Cheer; its
//!   Entry picks Cheer when EmoteId is 1) entered on the Emote trigger and left through Exit.
//! - Upper Body: an override layer limited by the UpperBody Avatar Mask, playing Wave; the
//!   script fades its weight in and out.
//! - Breathing: an additive layer bending the spine a little.
//!
//! - "Buddy", a second rig with other bone names, axes and proportions, plays the Character's
//!   Wave retargeted through their humanoid Avatars (`Wave (Humanoid).anim`).
//!
//! `CharacterControl.lua`: WASD walks (Shift runs, damped SetFloat), Left / Right turn, Q waves,
//! 1 / 2 bow / cheer. Regenerate with `cargo test -p misty_editor -- --ignored
//! write_character_animator_demo`; the other test checks the files still match and plays it.

use std::path::{Path, PathBuf};

use glam::{Vec2, Vec3};
use misty_engine::animation::controller::{
    BlendChild, BlendTree, BlendType, Condition, ConditionMode, Layer, LayerBlending, Node,
    ParamKind, Parameter, State, StateMachine, SubMachine, Transition,
};
use misty_engine::animation::humanoid::humanoid_clip;
use misty_engine::animation::mask::path_within;
use misty_engine::animation::{
    AnimationClip, AnimatorController, Avatar, AvatarMask, Property, TangentMode,
};
use misty_engine::scene::{
    Animator, CharacterController, MeshFilter, MeshRenderer, PrimitiveMesh, RectTransform, Scene,
    ScriptRef, Scripts, Transform, UiText,
};

use crate::actions::{Action, Create};
use crate::project::ProjectBrowser;
use crate::state::EditorState;
use crate::ui_create::UiElement;

const SCENE: &str = "Assets/Scenes/CharacterAnimatorDemo.scene";
const DIR: &str = "Assets/Animations/Character";
const CONTROLLER: &str = "Assets/Animations/Character/Character.controller";
const MASK: &str = "Assets/Animations/Character/UpperBody.mask";
const SCRIPT: &str = "Assets/Scripts/CharacterControl.lua";
const CHARACTER_AVATAR: &str = "Assets/Animations/Character/Character.avatar";
const BUDDY_AVATAR: &str = "Assets/Animations/Character/Buddy.avatar";
const BUDDY_CONTROLLER: &str = "Assets/Animations/Character/Buddy.controller";
const HUMAN_WAVE: &str = "Assets/Animations/Character/Wave (Humanoid).anim";

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn anim(name: &str) -> String {
    format!("{DIR}/{name}.anim")
}

// Bone paths below the Character object.
const HIPS: &str = "Hips";
const SPINE: &str = "Hips/Spine";
const CHEST: &str = "Hips/Spine/Chest";
const HEAD: &str = "Hips/Spine/Chest/Head";
const L_ARM: &str = "Hips/Spine/Chest/LeftArm";
const L_FOREARM: &str = "Hips/Spine/Chest/LeftArm/LeftForearm";
const R_ARM: &str = "Hips/Spine/Chest/RightArm";
const R_FOREARM: &str = "Hips/Spine/Chest/RightArm/RightForearm";
const L_THIGH: &str = "Hips/LeftUpLeg";
const L_KNEE: &str = "Hips/LeftUpLeg/LeftLeg";
const R_THIGH: &str = "Hips/RightUpLeg";
const R_KNEE: &str = "Hips/RightUpLeg/RightLeg";

const RX: Property = Property::Rotation(0);
const RY: Property = Property::Rotation(1);
const RZ: Property = Property::Rotation(2);
const PX: Property = Property::Position(0);
const PY: Property = Property::Position(1);
const PZ: Property = Property::Position(2);

/// (bone path, property, keys).
type CurveSpec<'a> = (&'a str, Property, &'a [(f32, f32)]);
type Curves<'a> = &'a [CurveSpec<'a>];

/// A clip from (path, property, keys) curves; root travel (the object's own position) is
/// linear, everything else smooth.
fn clip(length: f32, looping: bool, curves: Curves) -> AnimationClip {
    let mut c = AnimationClip {
        length,
        looping,
        ..Default::default()
    };
    for (path, p, keys) in curves {
        for (t, v) in *keys {
            c.set_key(path, *p, *t, *v);
        }
        if path.is_empty() {
            let i = c.curves.len() - 1;
            for k in 0..keys.len() {
                c.curves[i].keys.set_tangent(k, TangentMode::Linear);
            }
        }
    }
    c
}

/// Keys of a two-step cycle of `length`: `a` at the start, middle and end… alternating with `b`.
fn cycle(length: f32, a: f32, b: f32) -> [(f32, f32); 3] {
    [(0.0, a), (length * 0.5, b), (length, a)]
}

/// Knee bend peaking at `at` (fraction of the cycle).
fn knee(length: f32, at: f32, bend: f32, rest: f32) -> Vec<(f32, f32)> {
    let mut k = vec![(0.0, rest), (length, rest)];
    let peak = at * length;
    for (t, v) in [(peak - 0.25 * length, rest), (peak, bend)] {
        if t > 0.0 && t < length {
            k.push((t, v));
        }
    }
    k.sort_by(|a, b| a.0.total_cmp(&b.0));
    k
}

fn idle() -> AnimationClip {
    clip(
        2.0,
        true,
        &[
            (HIPS, PY, &[(0.0, 0.95), (1.0, 0.94), (2.0, 0.95)]),
            (CHEST, RX, &[(0.0, 0.0), (1.0, 2.0), (2.0, 0.0)]),
            (L_ARM, RZ, &[(0.0, -4.0), (1.0, -7.0), (2.0, -4.0)]),
            (R_ARM, RZ, &[(0.0, 4.0), (1.0, 7.0), (2.0, 4.0)]),
        ],
    )
}

/// A stepping cycle: thighs swing ±`swing`, knees bend, arms swing ±`arms`, root travels `dx`,
/// `dz` over the clip. `back` steps backwards (thigh phases swapped).
#[allow(clippy::too_many_arguments)]
fn stride(
    length: f32,
    swing: f32,
    bend: f32,
    arms: f32,
    lean: f32,
    elbow: f32,
    travel: Vec2,
    back: bool,
) -> AnimationClip {
    let s = if back { -swing } else { swing };
    let lt = cycle(length, -s, s);
    let rt = cycle(length, s, -s);
    let la = cycle(length, arms, -arms);
    let ra = cycle(length, -arms, arms);
    let lk = knee(length, if back { 0.25 } else { 0.75 }, bend, 5.0);
    let rk = knee(length, if back { 0.75 } else { 0.25 }, bend, 5.0);
    let q = length * 0.25;
    let bob = [
        (0.0, 0.92),
        (q, 0.96),
        (2.0 * q, 0.92),
        (3.0 * q, 0.96),
        (length, 0.92),
    ];
    let root_x = [(0.0, 0.0), (length, travel.x)];
    let root_z = [(0.0, 0.0), (length, travel.y)];
    let lean_k = [(0.0, lean), (length, lean)];
    let elbow_k = [(0.0, elbow), (length, elbow)];
    let mut curves: Vec<CurveSpec> = vec![
        (HIPS, PY, &bob),
        (L_THIGH, RX, &lt),
        (R_THIGH, RX, &rt),
        (L_KNEE, RX, &lk),
        (R_KNEE, RX, &rk),
        (L_ARM, RX, &la),
        (R_ARM, RX, &ra),
        (CHEST, RX, &lean_k),
        (L_FOREARM, RX, &elbow_k),
        (R_FOREARM, RX, &elbow_k),
    ];
    if travel.x != 0.0 {
        curves.push(("", PX, &root_x));
    }
    if travel.y != 0.0 {
        curves.push(("", PZ, &root_z));
    }
    clip(length, true, &curves)
}

fn walk() -> AnimationClip {
    stride(
        1.0,
        25.0,
        40.0,
        20.0,
        0.0,
        -10.0,
        Vec2::new(0.0, 1.4),
        false,
    )
}

fn run() -> AnimationClip {
    stride(
        0.7,
        45.0,
        80.0,
        40.0,
        12.0,
        -70.0,
        Vec2::new(0.0, 2.45),
        false,
    )
}

fn walk_back() -> AnimationClip {
    stride(
        1.0,
        20.0,
        35.0,
        15.0,
        -3.0,
        -10.0,
        Vec2::new(0.0, -1.0),
        true,
    )
}

/// Side steps: the leading leg opens, the other follows; `dir` = -1 left, 1 right.
fn strafe(dir: f32) -> AnimationClip {
    let (lead, follow) = if dir < 0.0 {
        (L_THIGH, R_THIGH)
    } else {
        (R_THIGH, L_THIGH)
    };
    let open = [(0.0, 0.0), (0.25, 18.0 * dir), (0.5, 0.0), (1.0, 0.0)];
    let close = [(0.0, 0.0), (0.5, 0.0), (0.75, 10.0 * dir), (1.0, 0.0)];
    clip(
        1.0,
        true,
        &[
            ("", PX, &[(0.0, 0.0), (1.0, dir)]),
            (
                HIPS,
                PY,
                &[
                    (0.0, 0.94),
                    (0.25, 0.96),
                    (0.5, 0.93),
                    (0.75, 0.96),
                    (1.0, 0.94),
                ],
            ),
            (lead, RZ, &open),
            (follow, RZ, &close),
            (L_ARM, RZ, &[(0.0, -6.0), (1.0, -6.0)]),
            (R_ARM, RZ, &[(0.0, 6.0), (1.0, 6.0)]),
        ],
    )
}

/// The right arm raised, the forearm waving, the head turning a little.
fn wave() -> AnimationClip {
    clip(
        1.2,
        true,
        &[
            (R_ARM, RZ, &[(0.0, 150.0), (1.2, 150.0)]),
            (R_ARM, RX, &[(0.0, 0.0), (1.2, 0.0)]),
            (
                R_FOREARM,
                RZ,
                &[
                    (0.0, 10.0),
                    (0.3, 45.0),
                    (0.6, 10.0),
                    (0.9, 45.0),
                    (1.2, 10.0),
                ],
            ),
            (HEAD, RY, &[(0.0, 0.0), (0.6, 10.0), (1.2, 0.0)]),
        ],
    )
}

/// Additive: the spine bends forward and back, the chest rises a little.
fn breathe() -> AnimationClip {
    clip(
        3.0,
        true,
        &[
            (SPINE, RX, &[(0.0, 0.0), (1.5, -3.0), (3.0, 0.0)]),
            (CHEST, PY, &[(0.0, 0.25), (1.5, 0.262), (3.0, 0.25)]),
        ],
    )
}

fn bow() -> AnimationClip {
    clip(
        2.0,
        false,
        &[
            (
                SPINE,
                RX,
                &[(0.0, 0.0), (0.6, 55.0), (1.4, 55.0), (2.0, 0.0)],
            ),
            (
                HEAD,
                RX,
                &[(0.0, 0.0), (0.6, 15.0), (1.4, 15.0), (2.0, 0.0)],
            ),
            (
                L_ARM,
                RX,
                &[(0.0, 0.0), (0.6, -12.0), (1.4, -12.0), (2.0, 0.0)],
            ),
            (
                R_ARM,
                RX,
                &[(0.0, 0.0), (0.6, -12.0), (1.4, -12.0), (2.0, 0.0)],
            ),
        ],
    )
}

fn cheer() -> AnimationClip {
    clip(
        1.6,
        false,
        &[
            (
                L_ARM,
                RZ,
                &[(0.0, 0.0), (0.3, -160.0), (1.3, -160.0), (1.6, 0.0)],
            ),
            (
                R_ARM,
                RZ,
                &[(0.0, 0.0), (0.3, 160.0), (1.3, 160.0), (1.6, 0.0)],
            ),
            (
                HIPS,
                PY,
                &[
                    (0.0, 0.95),
                    (0.4, 1.25),
                    (0.7, 0.95),
                    (1.0, 1.25),
                    (1.3, 0.95),
                    (1.6, 0.95),
                ],
            ),
        ],
    )
}

fn clips() -> Vec<(String, AnimationClip)> {
    vec![
        (anim("Idle"), idle()),
        (anim("Walk"), walk()),
        (anim("Run"), run()),
        (anim("WalkBack"), walk_back()),
        (anim("StrafeLeft"), strafe(-1.0)),
        (anim("StrafeRight"), strafe(1.0)),
        (anim("Wave"), wave()),
        (anim("Breathe"), breathe()),
        (anim("Bow"), bow()),
        (anim("Cheer"), cheer()),
    ]
}

fn controller() -> AnimatorController {
    let param = |name: &str, kind| Parameter {
        name: name.into(),
        kind,
        default: 0.0,
    };
    let at = |name: &str, x: f32, y: f32| BlendChild {
        position: [x, y],
        ..BlendChild::clip(&anim(name))
    };
    let locomotion = BlendTree {
        blend_type: BlendType::FreeformDirectional2D,
        parameter: "MoveX".into(),
        parameter_y: "MoveZ".into(),
        children: vec![
            at("Idle", 0.0, 0.0),
            at("Walk", 0.0, 1.0),
            at("Run", 0.0, 2.0),
            at("WalkBack", 0.0, -1.0),
            at("StrafeLeft", -1.0, 0.0),
            at("StrafeRight", 1.0, 0.0),
        ],
        ..Default::default()
    };
    let emote = |name: &str, y: f32| State {
        name: name.into(),
        motion: Some(anim(name)),
        looping: false,
        machine: "Emotes".into(),
        tag: "Emote".into(),
        position: [0.0, y],
        ..Default::default()
    };
    let leave = |from: &str| Transition {
        has_exit_time: true,
        exit_time: 0.9,
        duration: 0.25,
        ..Transition::new(Node::state(from), Node::Exit)
    };
    let base = StateMachine {
        states: vec![
            State {
                name: "Locomotion".into(),
                blend_tree: Some(locomotion),
                position: [0.0, 0.0],
                ..Default::default()
            },
            emote("Bow", -70.0),
            emote("Cheer", 70.0),
        ],
        default_state: Some("Locomotion".into()),
        transitions: vec![
            Transition {
                has_exit_time: false,
                duration: 0.2,
                conditions: vec![Condition {
                    parameter: "Emote".into(),
                    mode: ConditionMode::If,
                    threshold: 0.0,
                }],
                ..Transition::new(Node::state("Locomotion"), Node::Machine("Emotes".into()))
            },
            Transition {
                conditions: vec![Condition {
                    parameter: "EmoteId".into(),
                    mode: ConditionMode::Equals,
                    threshold: 1.0,
                }],
                machine: "Emotes".into(),
                ..Transition::new(Node::Entry, Node::state("Cheer"))
            },
            leave("Bow"),
            leave("Cheer"),
        ],
        sub_machines: vec![SubMachine {
            path: "Emotes".into(),
            position: [300.0, 120.0],
            default_state: Some("Bow".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let single = |name: &str| StateMachine {
        states: vec![State {
            name: name.into(),
            motion: Some(anim(name)),
            ..Default::default()
        }],
        default_state: Some(name.into()),
        ..Default::default()
    };
    AnimatorController {
        parameters: vec![
            param("MoveX", ParamKind::Float),
            param("MoveZ", ParamKind::Float),
            param("Emote", ParamKind::Trigger),
            param("EmoteId", ParamKind::Int),
        ],
        base,
        layers: vec![
            Layer {
                name: "Upper Body".into(),
                weight: 0.0,
                mask: Some(MASK.into()),
                machine: single("Wave"),
                ..Default::default()
            },
            Layer {
                name: "Breathing".into(),
                blending: LayerBlending::Additive,
                machine: single("Breathe"),
                ..Default::default()
            },
        ],
    }
}

/// The upper body (spine and everything above it) of the character's skeleton.
fn mask(scene: &Scene) -> AvatarMask {
    let character = scene.find_by_name("Character").expect("character");
    let mut m = AvatarMask::default();
    m.import(&AvatarMask::skeleton(scene, character));
    for t in &mut m.transforms {
        t.active = path_within(&t.path, SPINE);
    }
    m
}

const CONTROL_LUA: &str = r#"-- CharacterControl: drives the character's Animator.
-- W / S walk forwards and back, A / D step sideways, Shift runs (a 2D blend tree; the clips'
-- root motion moves the character), Left / Right turn, Q waves (fades the Upper Body layer in
-- and out), 1 bows and 2 cheers (the Emotes sub-state machine).
local CharacterControl = {
    turnSpeed = 120, -- degrees per second
    damp = 0.15, -- seconds MoveX / MoveZ take to follow the keys
    stateText = "State Text",
}

function CharacterControl:Start()
    self._anim = self.gameObject:GetComponent("Animator")
    self._wave = 0
    self._waving = false
    local label = GameObject.Find(self.stateText)
    self._label = label and label:GetComponent("Text")
end

function CharacterControl:Update()
    local a = self._anim
    if not a then return end
    local dt = Time.deltaTime
    local x, z = 0, 0
    if Input.GetKey("w") then z = z + 1 end
    if Input.GetKey("s") then z = z - 1 end
    if Input.GetKey("d") then x = x + 1 end
    if Input.GetKey("a") then x = x - 1 end
    if z > 0 and Input.GetKey("left shift") then z = 2 end
    a:SetFloat("MoveX", x, self.damp, dt)
    a:SetFloat("MoveZ", z, self.damp, dt)

    local turn = 0
    if Input.GetKey("left") then turn = turn - 1 end
    if Input.GetKey("right") then turn = turn + 1 end
    if turn ~= 0 then self.transform:Rotate(0, turn * self.turnSpeed * dt, 0) end

    if Input.GetKeyDown("q") then self._waving = not self._waving end
    local target = self._waving and 1 or 0
    self._wave = self._wave + (target - self._wave) * math.min(1, dt * 4)
    a:SetLayerWeight(a:GetLayerIndex("Upper Body"), self._wave)

    if Input.GetKeyDown("1") then a:SetInteger("EmoteId", 0); a:SetTrigger("Emote") end
    if Input.GetKeyDown("2") then a:SetInteger("EmoteId", 1); a:SetTrigger("Emote") end

    if self._label then
        local info = a:GetCurrentAnimatorStateInfo(0)
        self._label.text = string.format("%s   Move (%.2f, %.2f)   Wave %.0f%%",
            info.fullPath, a:GetFloat("MoveX"), a:GetFloat("MoveZ"), self._wave * 100)
    end
end

return CharacterControl
"#;

fn create(s: &mut EditorState, what: Create) -> misty_engine::hecs::Entity {
    crate::commands::apply(s, Action::Create { what, parent: None });
    s.selection.active().expect("created object is selected")
}

/// A Text anchored to the top-left corner at `y` pixels down.
fn text(s: &mut EditorState, name: &str, y: f32, size: f32, body: &str) {
    let e = create(s, Create::Ui(UiElement::Text));
    s.scene.game_object_mut(e).name = name.into();
    let w = &mut s.scene.world;
    *w.get::<&mut RectTransform>(e).unwrap() = RectTransform {
        anchor_min: Vec2::new(0.0, 1.0),
        anchor_max: Vec2::new(0.0, 1.0),
        pivot: Vec2::new(0.0, 1.0),
        anchored_position: Vec2::new(20.0, -y),
        size_delta: Vec2::new(900.0, size * 1.6),
    };
    let mut t = w.get::<&mut UiText>(e).unwrap();
    t.text = body.into();
    t.font_size = size;
    t.color = [1.0; 4];
}

/// A bone (empty object) at `pos` under `parent`.
fn bone(
    scene: &mut Scene,
    name: &str,
    parent: misty_engine::hecs::Entity,
    pos: Vec3,
) -> misty_engine::hecs::Entity {
    let e = scene.spawn(name, Some(parent));
    scene.set_transform(e, Transform::from_position(pos));
    e
}

/// A visible box for a bone: a child "<bone> Mesh" at `offset` with `size`.
fn limb(
    scene: &mut Scene,
    parent: misty_engine::hecs::Entity,
    offset: Vec3,
    size: Vec3,
    shape: PrimitiveMesh,
    material: &str,
) {
    let name = format!("{} Mesh", scene.name(parent));
    let e = scene.spawn(&name, Some(parent));
    scene.set_transform(
        e,
        Transform {
            position: offset,
            scale: size,
            ..Default::default()
        },
    );
    scene.add(e, MeshFilter::primitive(shape));
    scene.add(
        e,
        MeshRenderer {
            material: Some(format!("Assets/Materials/{material}.mat")),
            ..Default::default()
        },
    );
}

/// The jointed character: Hips → Spine → Chest → Head / arms (with forearms), Hips → legs.
fn character(scene: &mut Scene) -> misty_engine::hecs::Entity {
    use PrimitiveMesh::{Capsule, Cube, Sphere};
    let root = scene.spawn("Character", None);
    scene.set_transform(
        root,
        Transform::from_position(Vec3::new(0.0, 0.0, 2.0)).with_euler(Vec3::new(0.0, 180.0, 0.0)),
    );
    let skin = "Rig Skin";
    let cloth = "Blue Steel";
    let hips = bone(scene, "Hips", root, Vec3::new(0.0, 0.95, 0.0));
    limb(
        scene,
        hips,
        Vec3::ZERO,
        Vec3::new(0.36, 0.2, 0.22),
        Cube,
        cloth,
    );
    let spine = bone(scene, "Spine", hips, Vec3::new(0.0, 0.1, 0.0));
    let chest = bone(scene, "Chest", spine, Vec3::new(0.0, 0.25, 0.0));
    limb(
        scene,
        chest,
        Vec3::new(0.0, 0.05, 0.0),
        Vec3::new(0.44, 0.42, 0.24),
        Cube,
        cloth,
    );
    let head = bone(scene, "Head", chest, Vec3::new(0.0, 0.3, 0.0));
    limb(
        scene,
        head,
        Vec3::new(0.0, 0.15, 0.0),
        Vec3::splat(0.26),
        Sphere,
        skin,
    );
    for (side, x) in [("Left", -0.28f32), ("Right", 0.28)] {
        let arm = bone(scene, &format!("{side}Arm"), chest, Vec3::new(x, 0.22, 0.0));
        limb(
            scene,
            arm,
            Vec3::new(0.0, -0.15, 0.0),
            Vec3::new(0.1, 0.16, 0.1),
            Capsule,
            skin,
        );
        let fore = bone(
            scene,
            &format!("{side}Forearm"),
            arm,
            Vec3::new(0.0, -0.3, 0.0),
        );
        limb(
            scene,
            fore,
            Vec3::new(0.0, -0.14, 0.0),
            Vec3::new(0.09, 0.15, 0.09),
            Capsule,
            skin,
        );
    }
    for (side, x) in [("Left", -0.11f32), ("Right", 0.11)] {
        let thigh = bone(
            scene,
            &format!("{side}UpLeg"),
            hips,
            Vec3::new(x, -0.05, 0.0),
        );
        limb(
            scene,
            thigh,
            Vec3::new(0.0, -0.22, 0.0),
            Vec3::new(0.14, 0.23, 0.14),
            Capsule,
            cloth,
        );
        let leg = bone(
            scene,
            &format!("{side}Leg"),
            thigh,
            Vec3::new(0.0, -0.44, 0.0),
        );
        limb(
            scene,
            leg,
            Vec3::new(0.0, -0.2, 0.0),
            Vec3::new(0.12, 0.21, 0.12),
            Capsule,
            skin,
        );
        let foot = bone(
            scene,
            &format!("{side}Foot"),
            leg,
            Vec3::new(0.0, -0.42, 0.05),
        );
        limb(
            scene,
            foot,
            Vec3::ZERO,
            Vec3::new(0.12, 0.06, 0.24),
            Cube,
            cloth,
        );
    }
    scene.add(
        root,
        Animator {
            controller: Some(CONTROLLER.into()),
            apply_root_motion: true,
            avatar: Some(CHARACTER_AVATAR.into()),
            ..Default::default()
        },
    );
    scene.add(
        root,
        CharacterController {
            center: Vec3::new(0.0, 0.9, 0.0),
            radius: 0.3,
            height: 1.8,
            ..Default::default()
        },
    );
    scene.add(
        root,
        Scripts {
            list: vec![ScriptRef {
                path: SCRIPT.into(),
                ..Default::default()
            }],
        },
    );
    root
}

/// A bone turned `rot_z` degrees about z (its children sit along its local +y when turned).
fn bone_z(
    scene: &mut Scene,
    name: &str,
    parent: misty_engine::hecs::Entity,
    pos: Vec3,
    rot_z: f32,
) -> misty_engine::hecs::Entity {
    let e = bone(scene, name, parent, pos);
    let t = scene.transform(e).with_euler(Vec3::new(0.0, 0.0, rot_z));
    scene.set_transform(e, t);
    e
}

/// "Buddy": a smaller rig named like other tools name bones ("pelvis", "upperarm_r"…), whose arm
/// bones are turned half a turn (children along local +y). Same rest pose as the Character.
fn buddy(scene: &mut Scene) -> misty_engine::hecs::Entity {
    use PrimitiveMesh::{Capsule, Cube, Sphere};
    let root = scene.spawn("Buddy", None);
    scene.set_transform(
        root,
        Transform::from_position(Vec3::new(2.2, 0.0, 2.5)).with_euler(Vec3::new(0.0, 180.0, 0.0)),
    );
    let body = "Stress Teal";
    let joint = "Rig Joint";
    let pelvis = bone(scene, "pelvis", root, Vec3::new(0.0, 0.66, 0.0));
    limb(
        scene,
        pelvis,
        Vec3::ZERO,
        Vec3::new(0.26, 0.14, 0.16),
        Cube,
        body,
    );
    let spine = bone(scene, "spine_01", pelvis, Vec3::new(0.0, 0.08, 0.0));
    let chest = bone(scene, "spine_02", spine, Vec3::new(0.0, 0.16, 0.0));
    limb(
        scene,
        chest,
        Vec3::new(0.0, 0.05, 0.0),
        Vec3::new(0.32, 0.3, 0.18),
        Cube,
        body,
    );
    let neck = bone(scene, "neck_01", chest, Vec3::new(0.0, 0.22, 0.0));
    let head = bone(scene, "head", neck, Vec3::new(0.0, 0.04, 0.0));
    limb(
        scene,
        head,
        Vec3::new(0.0, 0.1, 0.0),
        Vec3::splat(0.2),
        Sphere,
        joint,
    );
    for (side, x) in [("l", -1.0f32), ("r", 1.0)] {
        let arm = bone_z(
            scene,
            &format!("upperarm_{side}"),
            chest,
            Vec3::new(0.2 * x, 0.15, 0.0),
            180.0,
        );
        limb(
            scene,
            arm,
            Vec3::new(0.0, 0.11, 0.0),
            Vec3::new(0.08, 0.12, 0.08),
            Capsule,
            joint,
        );
        let fore = bone(
            scene,
            &format!("lowerarm_{side}"),
            arm,
            Vec3::new(0.0, 0.22, 0.0),
        );
        limb(
            scene,
            fore,
            Vec3::new(0.0, 0.1, 0.0),
            Vec3::new(0.07, 0.11, 0.07),
            Capsule,
            joint,
        );
        bone(
            scene,
            &format!("hand_{side}"),
            fore,
            Vec3::new(0.0, 0.2, 0.0),
        );
        let thigh = bone(
            scene,
            &format!("thigh_{side}"),
            pelvis,
            Vec3::new(0.08 * x, -0.03, 0.0),
        );
        limb(
            scene,
            thigh,
            Vec3::new(0.0, -0.16, 0.0),
            Vec3::new(0.1, 0.16, 0.1),
            Capsule,
            body,
        );
        let calf = bone(
            scene,
            &format!("calf_{side}"),
            thigh,
            Vec3::new(0.0, -0.32, 0.0),
        );
        limb(
            scene,
            calf,
            Vec3::new(0.0, -0.14, 0.0),
            Vec3::new(0.09, 0.15, 0.09),
            Capsule,
            joint,
        );
        let foot = bone(
            scene,
            &format!("foot_{side}"),
            calf,
            Vec3::new(0.0, -0.3, 0.04),
        );
        limb(
            scene,
            foot,
            Vec3::ZERO,
            Vec3::new(0.1, 0.05, 0.18),
            Cube,
            body,
        );
    }
    scene.add(
        root,
        Animator {
            controller: Some(BUDDY_CONTROLLER.into()),
            avatar: Some(BUDDY_AVATAR.into()),
            ..Default::default()
        },
    );
    root
}

/// Buddy plays the retargeted wave.
fn buddy_controller() -> AnimatorController {
    AnimatorController {
        base: StateMachine {
            states: vec![State {
                name: "Wave".into(),
                motion: Some(HUMAN_WAVE.into()),
                ..Default::default()
            }],
            default_state: Some("Wave".into()),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// The avatars of both rigs, mapped from the generated scene.
fn avatars(scene: &Scene) -> (Avatar, Avatar) {
    let c = scene.find_by_name("Character").expect("character");
    let b = scene.find_by_name("Buddy").expect("buddy");
    (Avatar::auto_map(scene, c), Avatar::auto_map(scene, b))
}

fn demo_scene() -> Scene {
    let mut s = EditorState::new(ProjectBrowser::open(
        std::env::temp_dir().join("misty_character_animator_demo_build"),
    ));
    s.scene = Scene::default_3d("CharacterAnimatorDemo");
    if let Some(cam) = s.scene.find_by_name("Main Camera") {
        s.scene.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 2.2, -4.5))
                .with_euler(Vec3::new(14.0, 0.0, 0.0)),
        );
    }
    let floor = s.scene.spawn("Floor", None);
    s.scene.set_transform(
        floor,
        Transform {
            scale: Vec3::new(3.0, 1.0, 3.0),
            ..Default::default()
        },
    );
    s.scene
        .add(floor, MeshFilter::primitive(PrimitiveMesh::Plane));
    s.scene.add(
        floor,
        MeshRenderer {
            material: Some("Assets/Materials/Checker.mat".into()),
            ..Default::default()
        },
    );
    // Cones around the start, to see the character travel.
    for (i, (x, z)) in [(-3.0, 6.0), (3.0, 6.0), (-3.0, -2.0), (3.0, -2.0)]
        .into_iter()
        .enumerate()
    {
        let e = s.scene.spawn(&format!("Marker {}", i + 1), None);
        s.scene.set_transform(
            e,
            Transform {
                position: Vec3::new(x, 0.3, z),
                scale: Vec3::new(0.3, 0.3, 0.3),
                ..Default::default()
            },
        );
        s.scene
            .add(e, MeshFilter::primitive(PrimitiveMesh::Cylinder));
        s.scene.add(
            e,
            MeshRenderer {
                material: Some("Assets/Materials/Traffic Cone.mat".into()),
                ..Default::default()
            },
        );
    }
    character(&mut s.scene);
    buddy(&mut s.scene);
    text(
        &mut s,
        "Hint Text",
        16.0,
        20.0,
        "WASD: walk / step   Shift: run   Left / Right: turn   Q: wave   1 / 2: bow / cheer",
    );
    text(&mut s, "State Text", 48.0, 24.0, "Base Layer.Locomotion");
    s.scene
}

/// A `.meta` with a fresh GUID unless the asset (or folder) has one.
fn ensure_meta(path: &Path) {
    if misty_engine::asset_db::meta::read_guid(path).is_none() {
        misty_engine::asset_db::meta::write_guid(path, misty_engine::asset_db::Guid::new())
            .unwrap();
    }
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_character_animator_demo() {
    let root = project();
    for (path, c) in clips() {
        c.save(&root.join(&path)).unwrap();
        ensure_meta(&root.join(&path));
    }
    ensure_meta(&root.join(DIR));
    controller().save(&root.join(CONTROLLER)).unwrap();
    ensure_meta(&root.join(CONTROLLER));
    let scene = demo_scene();
    mask(&scene).save(&root.join(MASK)).unwrap();
    ensure_meta(&root.join(MASK));
    let (character, buddy) = avatars(&scene);
    character.save(&root.join(CHARACTER_AVATAR)).unwrap();
    ensure_meta(&root.join(CHARACTER_AVATAR));
    buddy.save(&root.join(BUDDY_AVATAR)).unwrap();
    ensure_meta(&root.join(BUDDY_AVATAR));
    humanoid_clip(&wave(), &character, 30.0)
        .save(&root.join(HUMAN_WAVE))
        .unwrap();
    ensure_meta(&root.join(HUMAN_WAVE));
    buddy_controller()
        .save(&root.join(BUDDY_CONTROLLER))
        .unwrap();
    ensure_meta(&root.join(BUDDY_CONTROLLER));
    std::fs::write(root.join(SCRIPT), CONTROL_LUA).unwrap();
    ensure_meta(&root.join(SCRIPT));
    scene.save(&root.join(SCENE)).unwrap();
    ensure_meta(&root.join(SCENE));
}

#[test]
fn character_animator_demo_matches_its_generator_and_plays() {
    use misty_engine::audio::Audio;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::Input;

    let root = project();
    for (path, c) in clips() {
        crate::test_util::assert_close(&AnimationClip::load(&root.join(&path)).unwrap(), &c, &path);
    }
    crate::test_util::assert_close(
        &AnimatorController::load(&root.join(CONTROLLER)).unwrap(),
        &controller(),
        CONTROLLER,
    );
    assert_eq!(
        AvatarMask::load(&root.join(MASK)).unwrap(),
        mask(&demo_scene()),
        "regenerate with write_character_animator_demo"
    );
    let (character, buddy) = avatars(&demo_scene());
    assert_eq!(
        Avatar::load(&root.join(CHARACTER_AVATAR)).unwrap(),
        character
    );
    assert_eq!(Avatar::load(&root.join(BUDDY_AVATAR)).unwrap(), buddy);
    assert!(buddy.missing().is_empty(), "{:?}", buddy.missing());
    crate::test_util::assert_close(
        &AnimationClip::load(&root.join(HUMAN_WAVE)).unwrap(),
        &humanoid_clip(&wave(), &character, 30.0),
        HUMAN_WAVE,
    );
    crate::test_util::assert_close(
        &AnimatorController::load(&root.join(BUDDY_CONTROLLER)).unwrap(),
        &buddy_controller(),
        BUDDY_CONTROLLER,
    );
    assert_eq!(
        std::fs::read_to_string(root.join(SCRIPT))
            .unwrap()
            .replace("\r\n", "\n"),
        CONTROL_LUA
    );
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_character_animator_demo",
    );

    let (mut scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let mut rt = Runtime::start_with_audio(
        &mut scene,
        root.clone(),
        &Default::default(),
        Audio::null(root).0,
    );
    let hero = scene.find_by_name("Character").unwrap();
    let arm = scene.find_by_name("RightArm").unwrap();
    let mut time = Time::default();
    // `n` frames holding `held`; `press` goes down on the first one.
    let mut frames =
        |rt: &mut Runtime, scene: &mut Scene, n: usize, held: &[&str], press: &[&str]| {
            for i in 0..n {
                let mut input = Input::default();
                input.held.extend(held.iter().map(|k| (*k).to_owned()));
                if i == 0 {
                    for k in press {
                        input.pressed.insert((*k).to_owned());
                        input.held.insert((*k).to_owned());
                    }
                }
                time.advance(1.0 / 60.0);
                rt.update(scene, &time, input);
            }
        };
    let state = |rt: &Runtime, scene: &Scene| {
        rt.animator_status(scene, hero)
            .unwrap()
            .current
            .unwrap()
            .full_path
    };
    frames(&mut rt, &mut scene, 30, &[], &[]);
    assert_eq!(state(&rt, &scene), "Base Layer.Locomotion");
    let start = scene.transform(hero).position;
    assert!(
        (start - Vec3::new(0.0, 0.0, 2.0)).length() < 1e-3,
        "idle stays: {start}"
    );
    // W: root motion walks it forwards (towards the camera, -z).
    frames(&mut rt, &mut scene, 60, &["w"], &[]);
    let walked = start.z - scene.transform(hero).position.z;
    assert!(walked > 0.7 && walked < 1.6, "walked {walked}");
    // Shift: running covers more ground in the same time.
    let before = scene.transform(hero).position.z;
    frames(&mut rt, &mut scene, 60, &["w", "left shift"], &[]);
    let ran = before - scene.transform(hero).position.z;
    assert!(ran > walked * 1.5, "ran {ran} vs walked {walked}");
    frames(&mut rt, &mut scene, 40, &[], &[]);
    // Q fades the masked upper-body layer in: the right arm waves, the legs keep the base pose.
    frames(&mut rt, &mut scene, 90, &[], &["q"]);
    let st = rt.animator_status(&scene, hero).unwrap();
    assert!(st.layers[1].weight > 0.9, "{}", st.layers[1].weight);
    assert_eq!(st.layers[1].current.as_ref().unwrap().name, "Wave");
    let raised = scene.transform(arm).euler().z;
    assert!(raised > 120.0, "arm raised: {raised}");
    // 2: the Emotes sub-state machine's Entry picks Cheer; afterwards back to Locomotion.
    frames(&mut rt, &mut scene, 20, &[], &["2"]);
    assert_eq!(state(&rt, &scene), "Base Layer.Emotes.Cheer");
    frames(&mut rt, &mut scene, 120, &[], &[]);
    assert_eq!(state(&rt, &scene), "Base Layer.Locomotion");
    frames(&mut rt, &mut scene, 20, &[], &["1"]);
    assert_eq!(state(&rt, &scene), "Base Layer.Emotes.Bow");
    // Buddy's right arm (bones turned half a turn, other names) is raised by the retargeted
    // wave, like the Character's.
    let up = |upper: &str, lower: &str| {
        let u = scene.world_position(scene.find_by_name(upper).unwrap());
        let l = scene.world_position(scene.find_by_name(lower).unwrap());
        (l - u).normalize()
    };
    let buddy_arm = up("upperarm_r", "lowerarm_r");
    let hero_arm = up("RightArm", "RightForearm");
    assert!(buddy_arm.y > 0.4, "Buddy waves: {buddy_arm}");
    assert!(hero_arm.y > 0.4, "the Character waves too: {hero_arm}");
    let logs = rt.take_logs();
    assert!(logs.is_empty(), "{logs:?}");
}
