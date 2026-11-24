//! Hill Run (`SampleProject/Assets/HillRun`): a small two-player split-screen game made
//! only with C# scripts. Each player is a Character Controller capsule carrying the jointed
//! character of the CharacterAnimatorDemo (an Animator with Idle / Walk / Run on a Speed
//! parameter and foot IK in its IK pass), spawned by the Player Input Manager when a device
//! joins: the keyboard (any key) or a gamepad (any button). Players walk up a gentle ramp and
//! a flight of steps (a steep ramp blocks them), jump, and race for six spinning pickups
//! (triggers). The start menu, the pause menu (Esc / Start; `Time.timeScale = 0`) and the win
//! screen work with the keyboard and a gamepad through the Event System, and the selected
//! button is highlighted from `OnSelect`.
//!
//! Scripts (`Scripts/`): `GameManager.cs` (menus, joining, scores), `HillRunner.cs` (moving,
//! jumping, Player Input messages), `FootIK.cs` (`OnAnimatorIK`), `Pickup.cs`
//! (`OnTriggerEnter`) and `MenuButton.cs` (`OnSelect` / `OnDeselect`).
//!
//! Regenerate with `cargo test -p misty_editor -- --ignored write_hill_run`; the other tests
//! check the committed files still match and play the game with injected keys and gamepads
//! (they need the .NET SDK and are skipped without it).

use std::path::{Path, PathBuf};

use glam::{Quat, Vec2, Vec3};
use misty_engine::animation::controller::{
    Condition, ConditionMode, ParamKind, Parameter, State, StateMachine, Transition,
};
use misty_engine::animation::{AnimationClip, AnimatorController};
use misty_engine::hecs::Entity;
use misty_engine::input::actions::asset::{GAMEPAD, KEYBOARD_MOUSE, default_schemes};
use misty_engine::input::actions::{
    Action, ActionMap, ActionType, Binding, ControlType, InputActionAsset, PlayerInput,
    PlayerInputBehavior, PlayerInputManager,
};
use misty_engine::scene::{
    Animator, BoxCollider, Camera, Canvas, CanvasScaler, CharacterController, MeshFilter,
    MeshRenderer, ObjectValue, PrimitiveMesh, RectTransform, Scene, ScriptRef, ScriptValue,
    Scripts, SphereCollider, TextAnchor, Transform, UiImage, UiText,
};
use misty_engine::ui::presets;

const DIR: &str = "Assets/HillRun";
const SCENE: &str = "Assets/HillRun/HillRun.scene";
const PREFAB: &str = "Assets/HillRun/Runner.prefab";
const ACTIONS: &str = "Assets/HillRun/HillRun.inputactions";
const CONTROLLER: &str = "Assets/HillRun/Runner.controller";
const IDLE: &str = "Assets/HillRun/Runner Idle.anim";
const WALK: &str = "Assets/HillRun/Runner Walk.anim";
const RUN: &str = "Assets/HillRun/Runner Run.anim";
/// The CharacterAnimatorDemo's humanoid Avatar: the runner's body is the same rig.
const AVATAR: &str = "Assets/Animations/Character/Character.avatar";
const GAME_MANAGER: &str = "Assets/HillRun/Scripts/GameManager.cs";
const HILL_RUNNER: &str = "Assets/HillRun/Scripts/HillRunner.cs";
const FOOT_IK: &str = "Assets/HillRun/Scripts/FootIK.cs";
const PICKUP: &str = "Assets/HillRun/Scripts/Pickup.cs";
const MENU_BUTTON: &str = "Assets/HillRun/Scripts/MenuButton.cs";

/// Gentle ramp: 18°, walkable.
const RAMP_DEG: f32 = 18.0;
/// Steep ramp: 55°, steeper than the controller's slope limit (45°).
const STEEP_DEG: f32 = 55.0;
/// Height of each step (the controller climbs up to 0.3).
const STEP: f32 = 0.25;
/// Where the Body sits below the Runner, so the animated legs are a little bent and foot IK can
/// stretch them down a slope.
const BODY_DROP: f32 = 0.12;

const GAME_MANAGER_CS: &str = r#"using Misty;

/// Hill Run's game flow: the start menu (keyboard or gamepad through the Event System),
/// players joining through the Player Input Manager once Play is pressed (any key joins the
/// keyboard player, any button a gamepad player), split-screen cameras, scores, the pause menu
/// (Time.timeScale = 0) and the win screen.
public class GameManager : MonoBehaviour
{
    public static GameManager instance;

    [Header("Screens")]
    public GameObject menuPanel;
    public GameObject hud;
    public GameObject pausePanel;
    public GameObject winPanel;

    [Header("Buttons")]
    public Button playButton;
    public Button quitButton;
    public Button resumeButton;
    public Button menuButton;
    public Button againButton;

    [Header("Text")]
    public Text p1Score;
    public Text p2Score;
    public Text statusText;
    public Text winText;

    [Header("World")]
    public Camera lobbyCamera;
    public Transform spawn1;
    public Transform spawn2;

    [HideInInspector] public bool playing;
    [HideInInspector] public bool paused;
    [HideInInspector] public int pickupsLeft;

    readonly HillRunner[] runners = new HillRunner[2];

    void Awake() { instance = this; }

    void Start()
    {
        playButton.onClick.AddListener(StartGame);
        quitButton.onClick.AddListener(Application.Quit);
        resumeButton.onClick.AddListener(Resume);
        menuButton.onClick.AddListener(BackToMenu);
        againButton.onClick.AddListener(BackToMenu);
        Time.timeScale = 1f;
        menuPanel.SetActive(true);
        hud.SetActive(false);
        pausePanel.SetActive(false);
        winPanel.SetActive(false);
        EventSystem.current.SetSelectedGameObject(playButton.gameObject);
    }

    public void StartGame()
    {
        menuPanel.SetActive(false);
        hud.SetActive(true);
        playing = true;
        pickupsLeft = FindObjectsOfType<Pickup>().Length;
        EventSystem.current.SetSelectedGameObject(null);
        PlayerInputManager.instance.EnableJoining();
        Refresh();
    }

    // Sent by the Player Input Manager on this object.
    void OnPlayerJoined(PlayerInput player)
    {
        int i = player.playerIndex;
        var runner = player.GetComponent<HillRunner>();
        if (i < runners.Length) runners[i] = runner;
        player.transform.position = (i == 0 ? spawn1 : spawn2).position;
        player.gameObject.name = "Player " + (i + 1);
        runner.SetColor(i == 0 ? new Color(0.25f, 0.5f, 1f) : new Color(1f, 0.4f, 0.25f));
        lobbyCamera.enabled = false;
        if (PlayerInputManager.instance.playerCount >= runners.Length)
            PlayerInputManager.instance.DisableJoining();
        Refresh();
    }

    void OnPlayerLeft(PlayerInput player) { Refresh(); }

    public void TogglePause()
    {
        if (!playing) return;
        if (paused) Resume();
        else Pause();
    }

    public void Pause()
    {
        paused = true;
        Time.timeScale = 0f;
        pausePanel.SetActive(true);
        EventSystem.current.SetSelectedGameObject(resumeButton.gameObject);
    }

    public void Resume()
    {
        paused = false;
        Time.timeScale = 1f;
        pausePanel.SetActive(false);
        EventSystem.current.SetSelectedGameObject(null);
    }

    public void BackToMenu()
    {
        Time.timeScale = 1f;
        SceneManager.LoadScene("HillRun");
    }

    // A runner touched a pickup.
    public void Collected(HillRunner runner, int points)
    {
        if (!playing) return;
        runner.score += points;
        pickupsLeft--;
        Refresh();
        if (pickupsLeft <= 0) Win();
    }

    void Win()
    {
        playing = false;
        int a = Score(0), b = Score(1);
        winText.text = a == b ? $"Draw!  {a} - {b}" : $"Player {(a > b ? 1 : 2)} wins!  {a} - {b}";
        winPanel.SetActive(true);
        EventSystem.current.SetSelectedGameObject(againButton.gameObject);
    }

    int Score(int i) => runners[i] == null ? 0 : runners[i].score;

    void Refresh()
    {
        p1Score.text = runners[0] == null ? "P1: press a key" : "P1  " + Score(0);
        p2Score.text = runners[1] == null ? "P2: press a gamepad button" : "P2  " + Score(1);
        statusText.text = pickupsLeft + " pickups left";
    }
}
"#;

const HILL_RUNNER_CS: &str = r#"using Misty;

/// A Hill Run player (Runner.prefab, spawned by the Player Input Manager). Move walks (the
/// camera looks up the hill, so up is forward), Jump jumps while grounded, Pause opens the
/// pause menu: its Player Input sends OnJump / OnPause (Send Messages). The body turns towards
/// where it walks and its Animator hears the speed.
public class HillRunner : MonoBehaviour
{
    [Header("Movement")]
    public float walkSpeed = 4f;
    public float jumpHeight = 1.2f;
    public float gravity = -20f;
    public float turnSpeed = 720f;

    [Header("Parts")]
    public Transform body;
    public Animator animator;

    [HideInInspector] public int score;

    CharacterController controller;
    PlayerInput input;
    InputAction move;
    float verticalSpeed;
    bool jumpQueued;

    void Awake()
    {
        controller = GetComponent<CharacterController>();
        input = GetComponent<PlayerInput>();
    }

    void Start() { move = input.actions["Move"]; }

    void OnJump() { jumpQueued = true; }

    void OnPause()
    {
        if (GameManager.instance != null) GameManager.instance.TogglePause();
    }

    void Update()
    {
        float dt = Time.deltaTime;
        bool jump = jumpQueued;
        jumpQueued = false;
        if (dt <= 0f) return;
        bool canMove = GameManager.instance == null || GameManager.instance.playing;
        Vector2 stick = canMove ? move.ReadValue<Vector2>() : Vector2.zero;
        Vector3 direction = new Vector3(stick.x, 0f, stick.y);
        if (direction.magnitude > 1f) direction = direction.normalized;

        if (controller.isGrounded && verticalSpeed < 0f) verticalSpeed = -2f;
        if (jump && canMove && controller.isGrounded) verticalSpeed = Mathf.Sqrt(2f * jumpHeight * -gravity);
        verticalSpeed += gravity * dt;
        controller.Move((direction * walkSpeed + Vector3.up * verticalSpeed) * dt);

        if (direction.sqrMagnitude > 0.01f)
            body.rotation = Quaternion.RotateTowards(body.rotation, Quaternion.LookRotation(direction), turnSpeed * dt);
        Vector3 velocity = controller.velocity;
        velocity.y = 0f;
        animator.SetFloat("Speed", velocity.magnitude);
        animator.SetBool("Grounded", controller.isGrounded);
    }

    // Tints the shirt (the hips and chest boxes).
    public void SetColor(Color color)
    {
        foreach (var path in new[] { "Hips/Hips Mesh", "Hips/Spine/Chest/Chest Mesh" })
        {
            var part = body.Find(path);
            if (part != null) part.GetComponent<Renderer>().material.color = color;
        }
    }
}
"#;

const FOOT_IK_CS: &str = r#"using Misty;

/// Keeps the runner's feet on the ground (slopes, steps) in the Animator's IK pass: each
/// foot's animated position goes onto the ground a ray finds straight below it, and the head
/// looks a little ahead. The Runner controller's Base Layer has IK Pass on.
public class FootIK : MonoBehaviour
{
    public bool ikActive = true;
    [Tooltip("Height of the ankle above the sole")]
    public float footOffset = 0.03f;
    public float rayLength = 1.2f;
    public float lookAhead = 3f;

    Animator animator;
    CharacterController controller;

    void Start()
    {
        animator = GetComponent<Animator>();
        controller = GetComponentInParent<CharacterController>();
    }

    void OnAnimatorIK(int layerIndex)
    {
        bool grounded = ikActive && controller != null && controller.isGrounded;
        Foot(AvatarIKGoal.LeftFoot, grounded);
        Foot(AvatarIKGoal.RightFoot, grounded);
        Vector3 head = animator.GetBoneTransform(HumanBodyBones.Head).position;
        animator.SetLookAtPosition(head + transform.forward * lookAhead);
        animator.SetLookAtWeight(ikActive ? 0.6f : 0f, 0f, 1f, 0f, 0.5f);
    }

    void Foot(AvatarIKGoal goal, bool grounded)
    {
        float weight = 0f;
        Vector3 foot = animator.GetIKPosition(goal);
        if (grounded && Physics.Raycast(foot + Vector3.up * (rayLength * 0.5f), Vector3.down, out RaycastHit hit, rayLength))
        {
            animator.SetIKPosition(goal, hit.point + Vector3.up * footOffset);
            animator.SetIKRotation(goal, Quaternion.FromToRotation(Vector3.up, hit.normal) * transform.rotation);
            weight = 1f;
        }
        animator.SetIKPositionWeight(goal, weight);
        animator.SetIKRotationWeight(goal, weight);
    }
}
"#;

const PICKUP_CS: &str = r#"using Misty;

/// A spinning, bobbing pickup (a trigger): the first runner to touch it scores.
public class Pickup : MonoBehaviour
{
    public int points = 1;
    public float spinSpeed = 120f;
    public float bobHeight = 0.15f;

    Vector3 start;

    void Start() { start = transform.position; }

    void Update()
    {
        transform.Rotate(0f, spinSpeed * Time.deltaTime, 0f);
        transform.position = start + Vector3.up * (Mathf.Sin(Time.time * 3f) * bobHeight);
    }

    void OnTriggerEnter(Collider other)
    {
        var runner = other.GetComponent<HillRunner>();
        if (runner == null || GameManager.instance == null || !GameManager.instance.playing) return;
        GameManager.instance.Collected(runner, points);
        gameObject.SetActive(false);
    }
}
"#;

const MENU_BUTTON_CS: &str = r#"using Misty;

/// Highlights a menu button while it is selected (keyboard or gamepad navigation, or the
/// mouse): it grows a little and its label turns yellow.
public class MenuButton : MonoBehaviour, ISelectHandler, IDeselectHandler
{
    public Text label;
    public float selectedScale = 1.1f;

    public void OnSelect(BaseEventData eventData)
    {
        transform.localScale = Vector3.one * selectedScale;
        label.color = Color.yellow;
    }

    public void OnDeselect(BaseEventData eventData)
    {
        transform.localScale = Vector3.one;
        label.color = Color.white;
    }
}
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Move (WASD / arrows / left stick / d-pad), Jump (Space / South), Pause (Esc / Start).
pub fn actions() -> InputActionAsset {
    const KM: &str = KEYBOARD_MOUSE;
    const GP: &str = GAMEPAD;
    let composite = |name: &str, keys: [&str; 4]| {
        let parts: Vec<(&str, &str)> = ["up", "down", "left", "right"]
            .into_iter()
            .zip(keys)
            .collect();
        Binding::composite(name, "2DVector", &parts, KM)
    };
    let player = ActionMap {
        name: "Player".into(),
        id: String::new(),
        actions: vec![
            Action::new("Move", ActionType::Value, ControlType::Vector2)
                .with(composite(
                    "WASD",
                    [
                        "<Keyboard>/w",
                        "<Keyboard>/s",
                        "<Keyboard>/a",
                        "<Keyboard>/d",
                    ],
                ))
                .with(composite(
                    "Arrows",
                    [
                        "<Keyboard>/upArrow",
                        "<Keyboard>/downArrow",
                        "<Keyboard>/leftArrow",
                        "<Keyboard>/rightArrow",
                    ],
                ))
                .with(Binding {
                    processors: "stickDeadzone".into(),
                    ..Binding::to("<Gamepad>/leftStick", GP)
                })
                .with(Binding::to("<Gamepad>/dpad", GP)),
            Action::new("Jump", ActionType::Button, ControlType::Button)
                .with(Binding::to("<Keyboard>/space", KM))
                .with(Binding::to("<Gamepad>/buttonSouth", GP)),
            Action::new("Pause", ActionType::Button, ControlType::Button)
                .with(Binding::to("<Keyboard>/escape", KM))
                .with(Binding::to("<Gamepad>/start", GP)),
        ],
    };
    let mut a = InputActionAsset {
        name: "HillRun".into(),
        maps: vec![player],
        control_schemes: default_schemes().into_iter().take(2).collect(),
    };
    a.ensure_ids();
    a
}

// ---- the runner's animation -----------------------------------------------------------------

/// The CharacterAnimatorDemo's clips without their root travel (the Character Controller moves
/// the runner; the clips only move the limbs).
fn clips() -> [(&'static str, AnimationClip); 3] {
    use crate::sample_character_animator::{idle, stride};
    [
        (IDLE, idle()),
        (
            WALK,
            stride(1.0, 25.0, 40.0, 20.0, 0.0, -10.0, Vec2::ZERO, false),
        ),
        (
            RUN,
            stride(0.7, 45.0, 80.0, 40.0, 12.0, -70.0, Vec2::ZERO, false),
        ),
    ]
}

/// Idle ⇄ Walk ⇄ Run on Speed (m/s); the Base Layer's IK Pass calls FootIK.OnAnimatorIK.
pub fn controller() -> AnimatorController {
    let state = |name: &str, motion: &str, x: f32| State {
        name: name.into(),
        motion: Some(motion.into()),
        position: [x, 0.0],
        ..Default::default()
    };
    let go = |from: &str, to: &str, mode: ConditionMode, threshold: f32| Transition {
        has_exit_time: false,
        duration: 0.15,
        conditions: vec![Condition {
            parameter: "Speed".into(),
            mode,
            threshold,
        }],
        ..Transition::new(
            misty_engine::animation::controller::Node::state(from),
            misty_engine::animation::controller::Node::state(to),
        )
    };
    AnimatorController {
        parameters: vec![
            Parameter {
                name: "Speed".into(),
                kind: ParamKind::Float,
                default: 0.0,
            },
            Parameter {
                name: "Grounded".into(),
                kind: ParamKind::Bool,
                default: 1.0,
            },
        ],
        base: StateMachine {
            states: vec![
                state("Idle", IDLE, 0.0),
                state("Walk", WALK, 250.0),
                state("Run", RUN, 500.0),
            ],
            default_state: Some("Idle".into()),
            transitions: vec![
                go("Idle", "Walk", ConditionMode::Greater, 0.2),
                go("Walk", "Idle", ConditionMode::Less, 0.2),
                go("Walk", "Run", ConditionMode::Greater, 3.0),
                go("Run", "Walk", ConditionMode::Less, 3.0),
            ],
            ..Default::default()
        },
        base_ik_pass: true,
        ..Default::default()
    }
}

// ---- scene building helpers -----------------------------------------------------------------

fn script(path: &str, fields: &[(&str, ScriptValue)]) -> ScriptRef {
    ScriptRef {
        path: path.into(),
        fields: fields
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
        ..Default::default()
    }
}

/// An object field holding `kind` of `e`.
fn obj(e: Entity, kind: &str) -> ScriptValue {
    ScriptValue::Object(ObjectValue {
        object: Some(e.to_bits().get()),
        kind: kind.into(),
    })
}

fn add_script(s: &mut Scene, e: Entity, r: ScriptRef) {
    let mut list = s
        .world
        .get::<&Scripts>(e)
        .map(|l| l.list.clone())
        .unwrap_or_default();
    list.push(r);
    s.add(e, Scripts { list });
}

fn mesh(s: &mut Scene, e: Entity, shape: PrimitiveMesh, mat: &str) {
    s.add(e, MeshFilter::primitive(shape));
    s.add(
        e,
        MeshRenderer {
            material: Some(format!("Assets/Materials/{mat}.mat")),
            ..Default::default()
        },
    );
}

/// A box with a Box Collider.
fn block(s: &mut Scene, name: &str, parent: Option<Entity>, t: Transform, mat: &str) -> Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    mesh(s, e, PrimitiveMesh::Cube, mat);
    s.add(e, BoxCollider::default());
    e
}

fn sized(pos: Vec3, scale: Vec3) -> Transform {
    Transform {
        position: pos,
        scale,
        ..Default::default()
    }
}

/// A slab `width` wide, `length` long and 0.5 thick whose top surface starts at `foot` (its
/// lower edge's middle) and rises `deg` degrees towards +z.
#[allow(clippy::too_many_arguments)]
fn ramp(
    s: &mut Scene,
    name: &str,
    parent: Entity,
    foot: Vec3,
    deg: f32,
    width: f32,
    length: f32,
    mat: &str,
) -> Entity {
    let rot = Quat::from_rotation_x(-deg.to_radians());
    let along = rot * Vec3::Z;
    let up = rot * Vec3::Y;
    let centre = foot + along * (length * 0.5) - up * 0.25;
    let t = Transform {
        position: centre,
        scale: Vec3::new(width, 0.5, length),
        ..Default::default()
    }
    .with_euler(Vec3::new(-deg, 0.0, 0.0));
    block(s, name, Some(parent), t, mat)
}

fn rect(pos: Vec2, size: Vec2) -> RectTransform {
    RectTransform {
        anchored_position: pos,
        ..RectTransform::sized(size.x, size.y)
    }
}

/// A Text under `parent`.
fn text(
    s: &mut Scene,
    name: &str,
    parent: Entity,
    rt: RectTransform,
    value: &str,
    size: f32,
    align: TextAnchor,
) -> Entity {
    let t = presets::text(s, name, parent);
    *s.world.get::<&mut RectTransform>(t).unwrap() = rt;
    let mut c = s.world.get::<&mut UiText>(t).unwrap();
    c.text = value.into();
    c.font_size = size;
    c.color = [1.0; 4];
    c.alignment = align;
    t
}

/// A menu button with MenuButton.cs (highlight while selected).
fn button(s: &mut Scene, name: &str, parent: Entity, y: f32, label: &str) -> Entity {
    let b = presets::button(s, name, parent);
    *s.world.get::<&mut RectTransform>(b).unwrap() =
        rect(Vec2::new(0.0, y), Vec2::new(280.0, 56.0));
    s.world.get::<&mut UiImage>(b).unwrap().color = [0.16, 0.2, 0.28, 1.0];
    let t = s.children(b)[0];
    {
        let mut c = s.world.get::<&mut UiText>(t).unwrap();
        c.text = label.into();
        c.font_size = 26.0;
        c.color = [1.0; 4];
    }
    add_script(s, b, script(MENU_BUTTON, &[("label", obj(t, "Text"))]));
    b
}

/// A centred panel (hidden unless `active`) with a title.
fn panel(s: &mut Scene, name: &str, canvas: Entity, title: &str, active: bool) -> Entity {
    let p = presets::panel(s, name, canvas);
    s.world.get::<&mut UiImage>(p).unwrap().color = [0.05, 0.07, 0.1, 0.78];
    text(
        s,
        "Title",
        p,
        rect(Vec2::new(0.0, 150.0), Vec2::new(700.0, 80.0)),
        title,
        56.0,
        TextAnchor::MiddleCenter,
    );
    s.game_object_mut(p).active = active;
    p
}

// ---- the runner prefab ----------------------------------------------------------------------

fn bone(s: &mut Scene, name: &str, parent: Entity, pos: Vec3) -> Entity {
    crate::sample_character_animator::bone(s, name, parent, pos)
}

fn limb(s: &mut Scene, parent: Entity, offset: Vec3, size: Vec3, shape: PrimitiveMesh, mat: &str) {
    crate::sample_character_animator::limb(s, parent, offset, size, shape, mat);
}

/// The jointed body of the CharacterAnimatorDemo's Character (same bone names, so its Avatar
/// and clips fit), under `parent`.
fn body(s: &mut Scene, parent: Entity) -> Entity {
    use PrimitiveMesh::{Capsule, Cube, Sphere};
    let skin = "Rig Skin";
    let cloth = "Blue Steel";
    let root = s.spawn("Body", Some(parent));
    s.set_transform(
        root,
        Transform::from_position(Vec3::new(0.0, -BODY_DROP, 0.0)),
    );
    let hips = bone(s, "Hips", root, Vec3::new(0.0, 0.95, 0.0));
    limb(s, hips, Vec3::ZERO, Vec3::new(0.36, 0.2, 0.22), Cube, cloth);
    let spine = bone(s, "Spine", hips, Vec3::new(0.0, 0.1, 0.0));
    let chest = bone(s, "Chest", spine, Vec3::new(0.0, 0.25, 0.0));
    limb(
        s,
        chest,
        Vec3::new(0.0, 0.05, 0.0),
        Vec3::new(0.44, 0.42, 0.24),
        Cube,
        cloth,
    );
    let head = bone(s, "Head", chest, Vec3::new(0.0, 0.3, 0.0));
    limb(
        s,
        head,
        Vec3::new(0.0, 0.15, 0.0),
        Vec3::splat(0.26),
        Sphere,
        skin,
    );
    for (side, x) in [("Left", -0.28f32), ("Right", 0.28)] {
        let arm = bone(s, &format!("{side}Arm"), chest, Vec3::new(x, 0.22, 0.0));
        limb(
            s,
            arm,
            Vec3::new(0.0, -0.15, 0.0),
            Vec3::new(0.1, 0.16, 0.1),
            Capsule,
            skin,
        );
        let fore = bone(s, &format!("{side}Forearm"), arm, Vec3::new(0.0, -0.3, 0.0));
        limb(
            s,
            fore,
            Vec3::new(0.0, -0.14, 0.0),
            Vec3::new(0.09, 0.15, 0.09),
            Capsule,
            skin,
        );
    }
    for (side, x) in [("Left", -0.11f32), ("Right", 0.11)] {
        let thigh = bone(s, &format!("{side}UpLeg"), hips, Vec3::new(x, -0.05, 0.0));
        limb(
            s,
            thigh,
            Vec3::new(0.0, -0.22, 0.0),
            Vec3::new(0.14, 0.23, 0.14),
            Capsule,
            cloth,
        );
        let leg = bone(s, &format!("{side}Leg"), thigh, Vec3::new(0.0, -0.44, 0.0));
        limb(
            s,
            leg,
            Vec3::new(0.0, -0.2, 0.0),
            Vec3::new(0.12, 0.21, 0.12),
            Capsule,
            skin,
        );
        let foot = bone(s, &format!("{side}Foot"), leg, Vec3::new(0.0, -0.42, 0.05));
        limb(
            s,
            foot,
            Vec3::ZERO,
            Vec3::new(0.12, 0.06, 0.24),
            Cube,
            cloth,
        );
    }
    s.add(
        root,
        Animator {
            controller: Some(CONTROLLER.into()),
            avatar: Some(AVATAR.into()),
            ..Default::default()
        },
    );
    add_script(s, root, script(FOOT_IK, &[]));
    root
}

/// Runner: Character Controller, Player Input (Send Messages), HillRunner.cs, the Body and a
/// Camera following from behind and above (the Runner itself never turns).
fn runner_scene() -> (Scene, Entity) {
    let mut s = Scene::new("Runner");
    let root = s.spawn("Runner", None);
    s.add(
        root,
        CharacterController {
            center: Vec3::new(0.0, 0.9, 0.0),
            radius: 0.3,
            height: 1.8,
            step_offset: 0.3,
            slope_limit: 45.0,
            slope_speed: 0.5,
            ..Default::default()
        },
    );
    s.add(
        root,
        PlayerInput {
            actions: Some(ACTIONS.into()),
            default_map: "Player".into(),
            behavior: PlayerInputBehavior::SendMessages,
            ..Default::default()
        },
    );
    let b = body(&mut s, root);
    let cam = s.spawn("Camera", Some(root));
    s.set_transform(
        cam,
        Transform::from_position(Vec3::new(0.0, 4.0, -7.0)).with_euler(Vec3::new(22.0, 0.0, 0.0)),
    );
    s.add(cam, Camera::default());
    add_script(
        &mut s,
        root,
        script(
            HILL_RUNNER,
            &[
                ("body", obj(b, "Transform")),
                ("animator", obj(b, "Animator")),
            ],
        ),
    );
    (s, root)
}

fn runner_prefab() -> misty_engine::scene::prefab::Prefab {
    let (s, root) = runner_scene();
    misty_engine::scene::prefab::Prefab::from_scene(&s, root)
}

// ---- the level and the UI -------------------------------------------------------------------

/// Pickup positions: on the flat start, high up (jump for it), on the ramp, on the top, on the
/// steps' landing and beside the steep ramp.
pub const PICKUPS: [Vec3; 6] = [
    Vec3::new(-1.5, 0.8, 4.0),
    Vec3::new(4.0, 2.6, 0.0),
    Vec3::new(-1.5, 2.8, 13.0),
    Vec3::new(-1.5, 3.9, 21.0),
    Vec3::new(-7.0, 1.8, 10.3),
    Vec3::new(6.0, 0.8, 6.0),
];

/// Where the players start.
pub const SPAWNS: [Vec3; 2] = [Vec3::new(-1.5, 0.0, 0.0), Vec3::new(1.5, 0.0, 0.0)];

/// Height of the hill top (the ramp's upper end).
pub fn top_height() -> f32 {
    10.0 * RAMP_DEG.to_radians().sin()
}

/// Where the ramp's foot is (its lower edge, z) and how far it runs (z).
pub fn ramp_span() -> (f32, f32) {
    (7.0, 10.0 * RAMP_DEG.to_radians().cos())
}

fn level(s: &mut Scene) {
    let lvl = s.spawn("Level", None);
    block(
        s,
        "Ground",
        Some(lvl),
        sized(Vec3::new(0.0, -0.5, 15.0), Vec3::new(30.0, 1.0, 50.0)),
        "Builder Grass",
    );
    // The hill: a gentle ramp up to a plateau.
    let (foot, run) = ramp_span();
    let top = top_height();
    ramp(
        s,
        "Ramp",
        lvl,
        Vec3::new(0.0, 0.0, foot),
        RAMP_DEG,
        6.0,
        10.0,
        "Sandstone",
    );
    let plateau_start = foot + run;
    block(
        s,
        "Plateau",
        Some(lvl),
        sized(
            Vec3::new(0.0, top * 0.5, plateau_start + 4.0),
            Vec3::new(6.0, top, 8.0),
        ),
        "Sandstone",
    );
    // Steps on the left up to a landing.
    for i in 0..4 {
        let h = STEP * (i + 1) as f32;
        block(
            s,
            &format!("Step {}", i + 1),
            Some(lvl),
            sized(
                Vec3::new(-7.0, h * 0.5, 7.0 + i as f32 * 0.6),
                Vec3::new(3.0, h, 0.6),
            ),
            "Wood",
        );
    }
    block(
        s,
        "Landing",
        Some(lvl),
        sized(Vec3::new(-7.0, 0.5, 10.3), Vec3::new(3.0, 1.0, 2.4)),
        "Wood",
    );
    // A steep ramp on the right: too steep to walk up.
    ramp(
        s,
        "Steep Ramp",
        lvl,
        Vec3::new(7.0, 0.0, 9.0),
        STEEP_DEG,
        3.0,
        4.0,
        "Ruin Stone",
    );
    let pickups = s.spawn("Pickups", None);
    for (i, p) in PICKUPS.iter().enumerate() {
        let e = s.spawn(&format!("Pickup {}", i + 1), Some(pickups));
        s.set_transform(
            e,
            Transform {
                position: *p,
                scale: Vec3::splat(0.5),
                ..Default::default()
            }
            .with_euler(Vec3::new(0.0, 0.0, 45.0)),
        );
        mesh(s, e, PrimitiveMesh::Cube, "Stress Gold");
        s.add(
            e,
            SphereCollider {
                is_trigger: true,
                radius: 1.0,
                ..Default::default()
            },
        );
        add_script(s, e, script(PICKUP, &[]));
    }
}

fn hill_run_scene() -> Scene {
    let mut s = Scene::default_3d("HillRun");
    // The menu's camera: an overview until the players bring theirs.
    let cam = s.find_by_name("Main Camera").unwrap();
    s.game_object_mut(cam).name = "Lobby Camera".into();
    s.set_transform(
        cam,
        Transform::from_position(Vec3::new(0.0, 12.0, -12.0)).with_euler(Vec3::new(32.0, 0.0, 0.0)),
    );
    level(&mut s);
    let spawns: Vec<Entity> = SPAWNS
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let e = s.spawn(&format!("Spawn {}", i + 1), None);
            s.set_transform(e, Transform::from_position(*p));
            e
        })
        .collect();

    // UI.
    let canvas = presets::spawn(&mut s, "Canvas", None, RectTransform::stretched());
    s.add(canvas, Canvas::default());
    s.add(canvas, CanvasScaler::default());
    presets::event_system(&mut s, "EventSystem");

    let menu = panel(&mut s, "Menu Panel", canvas, "Hill Run", true);
    let play = button(&mut s, "Play Button", menu, 20.0, "Play");
    let quit = button(&mut s, "Quit Button", menu, -60.0, "Quit");
    text(
        &mut s,
        "Hint",
        menu,
        rect(Vec2::new(0.0, -170.0), Vec2::new(900.0, 80.0)),
        "Arrows / d-pad: choose   Enter / A: select\nPlayer 1: WASD, Space to jump   Player 2: gamepad, A to jump   Esc / Start: pause",
        20.0,
        TextAnchor::MiddleCenter,
    );

    let hud = presets::spawn(&mut s, "HUD", Some(canvas), RectTransform::stretched());
    let corner = |x: f32| RectTransform {
        anchor_min: Vec2::new(x, 1.0),
        anchor_max: Vec2::new(x, 1.0),
        pivot: Vec2::new(x, 1.0),
        anchored_position: Vec2::new(if x < 0.5 { 24.0 } else { -24.0 }, -20.0),
        size_delta: Vec2::new(520.0, 50.0),
    };
    let p1 = text(
        &mut s,
        "P1 Score",
        hud,
        corner(0.0),
        "P1: press a key",
        34.0,
        TextAnchor::UpperLeft,
    );
    let p2 = text(
        &mut s,
        "P2 Score",
        hud,
        corner(1.0),
        "P2: press a gamepad button",
        34.0,
        TextAnchor::UpperRight,
    );
    let status = text(
        &mut s,
        "Status",
        hud,
        RectTransform {
            anchor_min: Vec2::new(0.5, 0.0),
            anchor_max: Vec2::new(0.5, 0.0),
            pivot: Vec2::new(0.5, 0.0),
            anchored_position: Vec2::new(0.0, 20.0),
            size_delta: Vec2::new(600.0, 40.0),
        },
        "",
        26.0,
        TextAnchor::LowerCenter,
    );
    s.game_object_mut(hud).active = false;

    let pause = panel(&mut s, "Pause Panel", canvas, "Paused", false);
    let resume = button(&mut s, "Resume Button", pause, 20.0, "Resume");
    let back = button(&mut s, "Menu Button", pause, -60.0, "Main Menu");

    let win = panel(&mut s, "Win Panel", canvas, "Finished", false);
    let win_text = text(
        &mut s,
        "Win Text",
        win,
        rect(Vec2::new(0.0, 60.0), Vec2::new(800.0, 60.0)),
        "",
        40.0,
        TextAnchor::MiddleCenter,
    );
    let again = button(&mut s, "Again Button", win, -40.0, "Main Menu");

    let game = s.spawn("Game", None);
    s.add(
        game,
        PlayerInputManager {
            player_prefab: Some(PREFAB.into()),
            joining_enabled: false,
            max_player_count: 2,
            split_screen: true,
            ..Default::default()
        },
    );
    let fields = [
        ("menuPanel", obj(menu, "GameObject")),
        ("hud", obj(hud, "GameObject")),
        ("pausePanel", obj(pause, "GameObject")),
        ("winPanel", obj(win, "GameObject")),
        ("playButton", obj(play, "Button")),
        ("quitButton", obj(quit, "Button")),
        ("resumeButton", obj(resume, "Button")),
        ("menuButton", obj(back, "Button")),
        ("againButton", obj(again, "Button")),
        ("p1Score", obj(p1, "Text")),
        ("p2Score", obj(p2, "Text")),
        ("statusText", obj(status, "Text")),
        ("winText", obj(win_text, "Text")),
        ("lobbyCamera", obj(cam, "Camera")),
        ("spawn1", obj(spawns[0], "Transform")),
        ("spawn2", obj(spawns[1], "Transform")),
    ];
    add_script(&mut s, game, script(GAME_MANAGER, &fields));
    s
}

fn scripts() -> [(&'static str, &'static str); 5] {
    [
        (GAME_MANAGER, GAME_MANAGER_CS),
        (HILL_RUNNER, HILL_RUNNER_CS),
        (FOOT_IK, FOOT_IK_CS),
        (PICKUP, PICKUP_CS),
        (MENU_BUTTON, MENU_BUTTON_CS),
    ]
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
fn write_hill_run() {
    let root = project();
    let dir = root.join(DIR);
    std::fs::create_dir_all(dir.join("Scripts")).unwrap();
    ensure_meta(&dir);
    ensure_meta(&dir.join("Scripts"));
    for (rel, src) in scripts() {
        std::fs::write(root.join(rel), src).unwrap();
        ensure_meta(&root.join(rel));
    }
    std::fs::write(root.join(ACTIONS), actions().to_json() + "\n").unwrap();
    ensure_meta(&root.join(ACTIONS));
    for (rel, c) in clips() {
        c.save(&root.join(rel)).unwrap();
        ensure_meta(&root.join(rel));
    }
    controller().save(&root.join(CONTROLLER)).unwrap();
    ensure_meta(&root.join(CONTROLLER));
    runner_prefab().save(&root.join(PREFAB)).unwrap();
    ensure_meta(&root.join(PREFAB));
    hill_run_scene().save(&root.join(SCENE)).unwrap();
    ensure_meta(&root.join(SCENE));
}

#[test]
fn hill_run_matches_its_generator() {
    let root = project();
    const AGAIN: &str = "regenerate with write_hill_run";
    for (rel, src) in scripts() {
        let on_disk = std::fs::read_to_string(root.join(rel)).unwrap();
        assert_eq!(on_disk.replace("\r\n", "\n"), src, "{rel}: {AGAIN}");
    }
    let on_disk = std::fs::read_to_string(root.join(ACTIONS)).unwrap();
    assert_eq!(
        on_disk.replace("\r\n", "\n"),
        actions().to_json() + "\n",
        "{AGAIN}"
    );
    for (rel, c) in clips() {
        crate::test_util::assert_close(&AnimationClip::load(&root.join(rel)).unwrap(), &c, rel);
    }
    crate::test_util::assert_close(
        &AnimatorController::load(&root.join(CONTROLLER)).unwrap(),
        &controller(),
        CONTROLLER,
    );
    let prefab = std::fs::read_to_string(root.join(PREFAB)).unwrap();
    crate::test_util::assert_json_close(&prefab, &runner_prefab().to_json(), AGAIN);
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(&text, &hill_run_scene().to_json(), AGAIN);
    let (_, report) = Scene::from_json("HillRun", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // The scene is in the build, so "Main Menu" can load it by name.
    let build = misty_engine::build::BuildSettings::load(&root).unwrap();
    assert!(
        build.scenes.iter().any(|s| s.path == SCENE && s.enabled),
        "{SCENE} is in Build Settings"
    );
    // A build packs everything the game loads while playing.
    let deps = misty_engine::asset_deps::collect(&root, &[SCENE.to_owned()]);
    assert!(deps.missing.is_empty(), "{:?}", deps.missing);
    for rel in [
        PREFAB,
        ACTIONS,
        CONTROLLER,
        IDLE,
        WALK,
        RUN,
        AVATAR,
        FOOT_IK,
        HILL_RUNNER,
    ] {
        assert!(
            deps.files.contains(rel),
            "{rel} not packed: {:?}",
            deps.files
        );
    }
}

#[cfg(test)]
#[path = "sample_hill_run_tests.rs"]
mod play_tests;
