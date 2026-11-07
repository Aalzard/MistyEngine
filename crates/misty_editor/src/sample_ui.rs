//! The UIDemo sample scene: a score Text and a Button whose On Click calls
//! `ScoreKeeper:AddPoint` (Assets/Scripts/ScoreKeeper.lua). Built with the GameObject ▸ UI menu
//! commands. Regenerate with `cargo test -p misty_editor -- --ignored write_ui_demo`; the other
//! tests check the committed file still matches and play it headlessly.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::scene::{
    ButtonCall, MeshFilter, MeshRenderer, PrimitiveMesh, RectTransform, Scene, ScriptRef, Scripts,
    TextAnchor, Transform, UiButton, UiText,
};

use crate::actions::{Action, Create};
use crate::project::ProjectBrowser;
use crate::state::EditorState;
use crate::ui_create::UiElement;

const SCENE: &str = "Assets/Scenes/UIDemo.scene";

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn create(s: &mut EditorState, what: Create) -> misty_engine::hecs::Entity {
    crate::commands::apply(s, Action::Create { what, parent: None });
    s.selection.active().expect("created object is selected")
}

fn demo_scene() -> Scene {
    let mut s = EditorState::new(ProjectBrowser::open(
        std::env::temp_dir().join("misty_ui_demo_build"),
    ));
    s.scene = Scene::default_3d("UIDemo");
    if let Some(cam) = s.scene.find_by_name("Main Camera") {
        s.scene.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 1.5, -5.0))
                .with_euler(Vec3::new(10.0, 0.0, 0.0)),
        );
    }
    // A spinning cube behind the UI.
    let cube = s.scene.spawn("Cube", None);
    s.scene
        .add(cube, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.scene.add(cube, MeshRenderer::default());
    s.scene
        .set_transform(cube, Transform::from_position(Vec3::new(0.0, 1.0, 0.0)));
    s.scene.add(
        cube,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Rotator.lua".into(),
                ..Default::default()
            }],
        },
    );
    let keeper = s.scene.spawn("Score Keeper", None);
    s.scene.add(
        keeper,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/ScoreKeeper.lua".into(),
                ..Default::default()
            }],
        },
    );

    // Score text in the top-left corner.
    let text = create(&mut s, Create::Ui(UiElement::Text));
    s.scene.game_object_mut(text).name = "Score Text".into();
    let w = &mut s.scene.world;
    *w.get::<&mut RectTransform>(text).unwrap() = RectTransform {
        anchor_min: Vec2::new(0.0, 1.0),
        anchor_max: Vec2::new(0.0, 1.0),
        pivot: Vec2::new(0.0, 1.0),
        anchored_position: Vec2::new(20.0, -20.0),
        size_delta: Vec2::new(400.0, 50.0),
    };
    {
        let mut t = w.get::<&mut UiText>(text).unwrap();
        t.text = "Score: 0".into();
        t.font_size = 32.0;
        t.color = [1.0; 4];
    }

    // Button at the bottom centre.
    let button = create(&mut s, Create::Ui(UiElement::Button));
    s.scene.game_object_mut(button).name = "Add Point Button".into();
    let w = &mut s.scene.world;
    *w.get::<&mut RectTransform>(button).unwrap() = RectTransform {
        anchor_min: Vec2::new(0.5, 0.0),
        anchor_max: Vec2::new(0.5, 0.0),
        pivot: Vec2::new(0.5, 0.0),
        anchored_position: Vec2::new(0.0, 40.0),
        size_delta: Vec2::new(240.0, 60.0),
    };
    w.get::<&mut UiButton>(button).unwrap().on_click = vec![ButtonCall {
        target: Some(keeper),
        method: "AddPoint".into(),
    }];
    let label = s.scene.children(button)[0];
    {
        let mut t = s.scene.world.get::<&mut UiText>(label).unwrap();
        t.text = "Add Point".into();
        t.font_size = 26.0;
        t.alignment = TextAnchor::MiddleCenter;
    }
    // UIDemo predates the Event System object (the first Canvas now brings one); it plays the
    // same without it, so the committed scene stays as it was.
    if let Some(es) = s.scene.find_by_name("EventSystem") {
        s.scene.despawn(es);
    }
    s.scene
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_ui_demo() {
    demo_scene().save(&project().join(SCENE)).unwrap();
}

#[test]
fn ui_demo_matches_its_generator() {
    let text = std::fs::read_to_string(project().join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_ui_demo",
    );
    let (scene, report) = Scene::from_json("UIDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let button = scene.find_by_name("Add Point Button").unwrap();
    let call = &scene.world.get::<&UiButton>(button).unwrap().on_click[0];
    assert_eq!(
        call.target,
        scene.find_by_name("Score Keeper"),
        "target survives the file"
    );
}

/// Clicking the button while playing runs ScoreKeeper:AddPoint and updates the Text.
#[test]
fn ui_demo_button_click_updates_the_score() {
    use misty_engine::audio::Audio;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::{Input, LogKind};
    use misty_engine::settings::ProjectSettings;

    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let (audio, _) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root, &ProjectSettings::default(), audio);
    let screen = Vec2::new(800.0, 600.0);
    let button = scene.find_by_name("Add Point Button").unwrap();
    let centre = misty_engine::ui::layout(&scene, screen)
        .node(button)
        .unwrap()
        .screen
        .center();
    let mut time = Time::default();
    let mut frame = |scene: &mut Scene, down: bool, up: bool| {
        let input = Input {
            mouse_position: Some(centre),
            screen_size: screen,
            mouse_held: [down, false, false],
            mouse_pressed: [down, false, false],
            mouse_released: [up, false, false],
            ..Default::default()
        };
        time.advance(1.0 / 60.0);
        rt.update(scene, &time, input);
    };
    let label = scene.find_by_name("Score Text").unwrap();
    let score = |scene: &Scene| scene.world.get::<&UiText>(label).unwrap().text.clone();
    frame(&mut scene, false, false);
    assert_eq!(score(&scene), "Score: 0");
    for _ in 0..3 {
        frame(&mut scene, true, false);
        frame(&mut scene, false, true);
    }
    assert_eq!(score(&scene), "Score: 3");
    let errors: Vec<_> = rt
        .take_logs()
        .into_iter()
        .filter(|(k, _)| *k != LogKind::Info)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
}
