//! Procedural InputDemo scene in `SampleProject`: a capsule character driven by the Input
//! Manager (`Input.GetAxis("Horizontal"/"Vertical")` eases in and out, `GetButtonDown("Jump")`
//! jumps; keys, arrows or a gamepad's left stick and Y button), a UI Text that names the
//! connected gamepad (`GamepadHint.lua`), and a spinning marker on the user layer "Hidden" that
//! the Main Camera's Culling Mask leaves out (it only shows in the Scene view). The writer also
//! names layer 8 "Hidden" in the project's Tags and Layers.
//! Regenerate with `cargo test -p misty_editor -- --ignored write_input_demo`; the other tests
//! check the committed files still match and play the scene with injected input.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    BoxCollider, Camera, CharacterController, MeshFilter, MeshRenderer, PrimitiveMesh,
    RectTransform, Scene, ScriptRef, Scripts, Transform, UiText,
};
use misty_engine::settings::ProjectSettings;

use crate::actions::{Action, Create};
use crate::project::ProjectBrowser;
use crate::state::EditorState;
use crate::ui_create::UiElement;

const SCENE: &str = "Assets/Scenes/InputDemo.scene";
const MOVER: &str = "Assets/Scripts/InputMover.lua";
const HINT: &str = "Assets/Scripts/GamepadHint.lua";
const ROTATOR: &str = "Assets/Scripts/Rotator.lua";
const CHECKER: &str = "Assets/Materials/Checker.mat";
const WOOD: &str = "Assets/Materials/Wood.mat";
const STEEL: &str = "Assets/Materials/Blue Steel.mat";
const LASER: &str = "Assets/Materials/Laser.mat";
/// User layer the camera doesn't render.
const HIDDEN_LAYER: usize = 8;

const MOVER_LUA: &str = r#"-- InputMover: moves a Character Controller with the Horizontal / Vertical axes (WASD, arrow
-- keys or a gamepad's left stick; GetAxis eases in and out) and jumps on the Jump button
-- (Space or joystick button 3). Axes are set up in Project Settings > Input Manager.
local InputMover = {
    speed = 5,
    jumpSpeed = 6,
    gravity = -15,
}

function InputMover:Start()
    self.cc = self.gameObject:GetComponent("CharacterController")
    self._vy = 0
end

function InputMover:Update()
    local dt = Time.deltaTime
    local move = Vector3.new(Input.GetAxis("Horizontal"), 0, Input.GetAxis("Vertical"))
    if move.magnitude > 1 then
        move = move.normalized
    end
    if self.cc.isGrounded then
        self._vy = -1
        if Input.GetButtonDown("Jump") then
            self._vy = self.jumpSpeed
        end
    end
    self._vy = self._vy + self.gravity * dt
    self.cc:Move((move * self.speed + Vector3.new(0, self._vy, 0)) * dt)
end

return InputMover
"#;

const HINT_LUA: &str = r#"-- GamepadHint: shows the controls in this object's Text, naming the first connected gamepad.
local GamepadHint = {}

function GamepadHint:Start()
    self.label = self.gameObject:GetComponent("Text")
end

function GamepadHint:Update()
    local pad = nil
    for _, name in ipairs(Input.GetJoystickNames()) do
        if name ~= "" then
            pad = name
            break
        end
    end
    local line = "No gamepad: plug one in to move with the left stick"
    if pad ~= nil then
        line = "Gamepad: " .. pad .. " (left stick moves, Y jumps)"
    end
    local text = "Move: WASD / arrow keys   Jump: Space\n" .. line
        .. "\nThe camera's Culling Mask hides the Hidden layer"
    if text ~= self._shown then
        self.label.text = text
        self._shown = text
    end
end

return GamepadHint
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn script(path: &str) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            ..Default::default()
        }],
    }
}

fn shape(s: &mut Scene, name: &str, mesh: PrimitiveMesh, t: Transform, mat: &str) -> Entity {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(mesh));
    s.add(
        e,
        MeshRenderer {
            material: Some(mat.into()),
            ..Default::default()
        },
    );
    e
}

fn block(s: &mut Scene, name: &str, pos: Vec3, scale: Vec3, mat: &str) -> Entity {
    let mut t = Transform::from_position(pos);
    t.scale = scale;
    let e = shape(s, name, PrimitiveMesh::Cube, t, mat);
    s.add(e, BoxCollider::default());
    e
}

fn demo_scene() -> Scene {
    let mut s = EditorState::new(ProjectBrowser::open(
        std::env::temp_dir().join("misty_input_demo_build"),
    ));
    s.scene = Scene::default_3d("InputDemo");
    let scene = &mut s.scene;
    if let Some(cam) = scene.find_by_name("Main Camera") {
        scene.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 7.0, -11.0))
                .with_euler(Vec3::new(32.0, 0.0, 0.0)),
        );
        scene.world.get::<&mut Camera>(cam).unwrap().culling_mask = !(1 << HIDDEN_LAYER);
    }
    block(
        scene,
        "Ground",
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(20.0, 1.0, 20.0),
        CHECKER,
    );
    // Steps to jump onto.
    for (i, (x, h)) in [(3.0, 0.6), (5.5, 1.2), (8.0, 1.8)].into_iter().enumerate() {
        block(
            scene,
            &format!("Step {}", i + 1),
            Vec3::new(x, h / 2.0, 3.0),
            Vec3::new(2.0, h, 2.0),
            WOOD,
        );
    }
    let player = shape(
        scene,
        "Player",
        PrimitiveMesh::Capsule,
        Transform::from_position(Vec3::new(0.0, 1.1, 0.0)),
        STEEL,
    );
    scene.game_object_mut(player).tag = "Player".into();
    scene.game_object_mut(player).layer = 5;
    scene.add(player, CharacterController::default());
    scene.add(player, script(MOVER));
    // Only the Scene view shows this one.
    let mut t = Transform::from_position(Vec3::new(-4.0, 1.5, 3.0));
    t.scale = Vec3::splat(0.8);
    let hidden = shape(scene, "Hidden Marker", PrimitiveMesh::Cube, t, LASER);
    scene.game_object_mut(hidden).layer = HIDDEN_LAYER as u8;
    scene.add(hidden, script(ROTATOR));

    // Controls and gamepad hint in the top-left corner.
    crate::commands::apply(
        &mut s,
        Action::Create {
            what: Create::Ui(UiElement::Text),
            parent: None,
        },
    );
    let text = s.selection.active().expect("created text is selected");
    s.scene.game_object_mut(text).name = "Hint Text".into();
    let w = &mut s.scene.world;
    *w.get::<&mut RectTransform>(text).unwrap() = RectTransform {
        anchor_min: Vec2::new(0.0, 1.0),
        anchor_max: Vec2::new(0.0, 1.0),
        pivot: Vec2::new(0.0, 1.0),
        anchored_position: Vec2::new(20.0, -20.0),
        size_delta: Vec2::new(900.0, 120.0),
    };
    {
        let mut t = w.get::<&mut UiText>(text).unwrap();
        t.text = "Move: WASD / arrow keys   Jump: Space".into();
        t.font_size = 26.0;
        t.color = [1.0; 4];
    }
    s.scene.add(text, script(HINT));
    s.scene
}

/// The sample project's settings with layer 8 named "Hidden".
fn settings(root: &std::path::Path) -> ProjectSettings {
    let mut p = ProjectSettings::load(root).unwrap();
    p.tags_and_layers.layers[HIDDEN_LAYER] = "Hidden".into();
    p
}

fn scripts() -> [(&'static str, &'static str); 2] {
    [(MOVER, MOVER_LUA), (HINT, HINT_LUA)]
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_input_demo() {
    let root = project();
    for (rel, src) in scripts() {
        std::fs::write(root.join(rel), src).unwrap();
    }
    settings(&root).save(&root).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn input_demo_matches_its_generator() {
    let root = project();
    for (rel, src) in scripts() {
        let on_disk = std::fs::read_to_string(root.join(rel)).unwrap();
        assert_eq!(on_disk.replace("\r\n", "\n"), src, "{rel}");
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert_eq!(
        text.replace("\r\n", "\n"),
        demo_scene().to_json(),
        "regenerate with write_input_demo"
    );
    let (_, report) = Scene::from_json("InputDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let p = ProjectSettings::load(&root).unwrap();
    assert_eq!(
        p.tags_and_layers.name_to_layer("Hidden"),
        HIDDEN_LAYER as i32
    );
}

/// Playing with injected keys and a fake gamepad: the character eases into motion, jumps, the
/// hint names the pad, and the game camera doesn't draw the Hidden layer.
#[test]
fn input_demo_plays_with_keys_and_a_gamepad() {
    use misty_engine::audio::Audio;
    use misty_engine::input::Joystick;
    use misty_engine::render_scene::{GameView, MaterialSource};
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::{Input, LogKind};

    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let settings = ProjectSettings::load(&root).unwrap();
    let (audio, _) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root, &settings, audio);
    let mut time = Time::new(&settings.time);
    let player = scene.find_by_name("Player").unwrap();
    let mut frame = |scene: &mut Scene, input: &Input| {
        time.advance(1.0 / 60.0);
        rt.update(scene, &time, input.clone());
        let errors: Vec<_> = rt
            .take_logs()
            .into_iter()
            .filter(|(k, _)| *k == LogKind::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
    };
    let idle = Input::default();
    for _ in 0..30 {
        frame(&mut scene, &idle);
    }
    let start = scene.world_position(player);
    assert!(start.y < 1.2, "landed: {start}");

    // Holding D: the first frame moves much less than full speed (GetAxis eases in).
    let mut right = Input::default();
    right.held.insert("d".into());
    frame(&mut scene, &right);
    let first = scene.world_position(player).x - start.x;
    assert!(first > 0.0 && first < 5.0 / 60.0 * 0.2, "{first}");
    for _ in 0..59 {
        frame(&mut scene, &right);
    }
    let moved = scene.world_position(player).x - start.x;
    assert!(
        moved > 3.5 && moved < 5.0,
        "about 5 m/s after easing in: {moved}"
    );

    // Space jumps.
    let mut jump = Input::default();
    jump.held.insert("space".into());
    jump.pressed.insert("space".into());
    let before = scene.world_position(player).y;
    frame(&mut scene, &jump);
    for _ in 0..12 {
        frame(&mut scene, &idle);
    }
    assert!(scene.world_position(player).y > before + 0.5, "jumped");
    for _ in 0..60 {
        frame(&mut scene, &idle);
    }

    // A fake gamepad: left stick pulled back (+1 = down) moves the character towards -z.
    let mut pad = Joystick {
        name: "Test Pad".into(),
        ..Default::default()
    };
    pad.axes[1] = 1.0;
    let stick = Input {
        joysticks: vec![pad],
        ..Default::default()
    };
    let z = scene.world_position(player).z;
    for _ in 0..30 {
        frame(&mut scene, &stick);
    }
    let dz = scene.world_position(player).z - z;
    assert!(dz < -2.0, "the stick moves it: {dz}");
    let hint = scene.find_by_name("Hint Text").unwrap();
    let text = scene.world.get::<&UiText>(hint).unwrap().text.clone();
    assert!(text.contains("Gamepad: Test Pad"), "{text}");

    // The game camera skips the Hidden layer.
    struct Plain;
    impl MaterialSource for Plain {
        fn params(&self, _: Option<&str>) -> misty_engine::render::MaterialParams {
            Default::default()
        }
    }
    let view = GameView::new(&scene, &Plain).unwrap();
    let renderers = scene
        .walk()
        .into_iter()
        .filter(|(e, _)| scene.world.get::<&MeshRenderer>(*e).is_ok())
        .count();
    assert_eq!(
        view.meshes.len(),
        renderers - 1,
        "Hidden Marker isn't drawn"
    );
}
