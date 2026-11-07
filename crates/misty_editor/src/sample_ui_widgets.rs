//! The UIWidgetsDemo sample scene: a settings-menu panel laid out with layout groups — volume
//! Slider, fullscreen Toggle, name Input Field, quality Dropdown and a scroll list of level
//! buttons — with `Assets/Scripts/SettingsMenu.lua` showing the choices in a Text. Built with
//! the GameObject ▸ UI menu commands. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_ui_widgets_demo`; the other tests check the
//! committed file still matches and play it headlessly.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    ButtonCall, CanvasScaler, ContentSizeFitter, FitMode, HorizontalLayoutGroup, LayoutElement,
    LinearLayout, MeshFilter, MeshRenderer, Padding, PrimitiveMesh, RectTransform, Scene,
    ScriptRef, Scripts, TextAnchor, Transform, UiDropdown, UiImage, UiInputField, UiScrollRect,
    UiSlider, UiText, UiToggle, VerticalLayoutGroup,
};
use misty_engine::ui::presets;

use crate::actions::{Action, Create};
use crate::project::ProjectBrowser;
use crate::state::EditorState;
use crate::ui_create::UiElement;

const SCENE: &str = "Assets/Scenes/UIWidgetsDemo.scene";

const PANEL: [f32; 4] = [0.11, 0.12, 0.15, 0.94];
const LIGHT: [f32; 4] = [0.92, 0.93, 0.95, 1.0];

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Run a GameObject ▸ UI command with `parent` selected-as-parent; returns the new object.
fn create(s: &mut EditorState, el: UiElement, parent: Option<Entity>) -> Entity {
    let what = Create::Ui(el);
    crate::commands::apply(s, Action::Create { what, parent });
    s.selection.active().expect("created object is selected")
}

fn rename(s: &mut EditorState, e: Entity, name: &str) {
    s.scene.game_object_mut(e).name = name.into();
}

fn layout_size(s: &mut EditorState, e: Entity, w: Option<f32>, h: Option<f32>, flex: bool) {
    s.scene.add(
        e,
        LayoutElement {
            preferred_width: w,
            preferred_height: h,
            flexible_width: flex.then_some(1.0),
            ..Default::default()
        },
    );
}

fn style_text(s: &mut EditorState, e: Entity, text: &str, size: f32, align: TextAnchor) {
    let mut t = s.scene.world.get::<&mut UiText>(e).unwrap();
    t.text = text.into();
    t.font_size = size;
    t.color = LIGHT;
    t.alignment = align;
}

/// A labelled row (Horizontal Layout Group) holding `control`.
fn row(s: &mut EditorState, panel: Entity, label: &str, control: UiElement) -> Entity {
    let row = s.scene.spawn(&format!("{label} Row"), Some(panel));
    s.scene.game_object_mut(row).layer = presets::UI_LAYER;
    s.scene.add(row, RectTransform::default());
    s.scene.add(
        row,
        HorizontalLayoutGroup(LinearLayout {
            spacing: 10.0,
            child_force_expand_width: false,
            ..Default::default()
        }),
    );
    layout_size(s, row, None, Some(30.0), false);
    let text = create(s, UiElement::Text, Some(row));
    rename(s, text, &format!("{label} Label"));
    style_text(s, text, label, 18.0, TextAnchor::MiddleLeft);
    layout_size(s, text, Some(110.0), None, false);
    let c = create(s, control, Some(row));
    layout_size(s, c, None, None, true);
    c
}

fn demo_scene() -> Scene {
    let mut s = EditorState::new(ProjectBrowser::open(
        std::env::temp_dir().join("misty_ui_widgets_demo_build"),
    ));
    s.scene = Scene::default_3d("UIWidgetsDemo");
    if let Some(cam) = s.scene.find_by_name("Main Camera") {
        s.scene.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 1.5, -5.0))
                .with_euler(Vec3::new(10.0, 0.0, 0.0)),
        );
    }
    // A spinning cube behind the menu.
    let cube = s.scene.spawn("Cube", None);
    s.scene
        .add(cube, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.scene.add(cube, MeshRenderer::default());
    s.scene
        .set_transform(cube, Transform::from_position(Vec3::new(2.2, 1.0, 0.0)));
    s.scene.add(
        cube,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Rotator.lua".into(),
                ..Default::default()
            }],
        },
    );
    let menu = s.scene.spawn("Settings Menu", None);
    s.scene.add(
        menu,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/SettingsMenu.lua".into(),
                ..Default::default()
            }],
        },
    );

    // The panel: a vertical layout group on the left half of the screen.
    let canvas = create(&mut s, UiElement::Canvas, None);
    {
        // Scale with the screen height so the panel always fits.
        let mut scaler = s.scene.world.get::<&mut CanvasScaler>(canvas).unwrap();
        scaler.reference_resolution = Vec2::new(1280.0, 720.0);
        scaler.match_width_or_height = 1.0;
    }
    let panel = create(&mut s, UiElement::Panel, Some(canvas));
    rename(&mut s, panel, "Settings Panel");
    *s.scene.world.get::<&mut RectTransform>(panel).unwrap() = RectTransform {
        anchor_min: Vec2::new(0.0, 0.5),
        anchor_max: Vec2::new(0.0, 0.5),
        pivot: Vec2::new(0.0, 0.5),
        anchored_position: Vec2::new(30.0, 0.0),
        size_delta: Vec2::new(420.0, 540.0),
    };
    s.scene.world.get::<&mut UiImage>(panel).unwrap().color = PANEL;
    s.scene.add(
        panel,
        VerticalLayoutGroup(LinearLayout {
            padding: Padding {
                left: 20.0,
                right: 20.0,
                top: 16.0,
                bottom: 16.0,
            },
            spacing: 12.0,
            child_force_expand_height: false,
            ..Default::default()
        }),
    );

    let title = create(&mut s, UiElement::Text, Some(panel));
    rename(&mut s, title, "Title");
    style_text(&mut s, title, "Settings", 30.0, TextAnchor::MiddleLeft);
    layout_size(&mut s, title, None, Some(40.0), false);

    let volume = row(&mut s, panel, "Volume", UiElement::Slider);
    rename(&mut s, volume, "Volume Slider");
    {
        let mut sl = s.scene.world.get::<&mut UiSlider>(volume).unwrap();
        sl.max_value = 100.0;
        sl.whole_numbers = true;
        sl.value = 80.0;
    }
    let fullscreen = row(&mut s, panel, "Fullscreen", UiElement::Toggle);
    rename(&mut s, fullscreen, "Fullscreen Toggle");
    s.scene
        .world
        .get::<&mut UiToggle>(fullscreen)
        .unwrap()
        .is_on = false;
    if let Some(label) = s.scene.children(fullscreen).get(1).copied() {
        style_text(&mut s, label, "Full screen", 16.0, TextAnchor::MiddleLeft);
    }
    let name = row(&mut s, panel, "Name", UiElement::InputField);
    rename(&mut s, name, "Name Field");
    {
        let mut f = s.scene.world.get::<&mut UiInputField>(name).unwrap();
        f.character_limit = 16;
        f.on_end_edit = vec![ButtonCall {
            target: Some(menu),
            method: "NameEntered".into(),
        }];
        if let Some(ph) = f.placeholder {
            drop(f);
            s.scene.world.get::<&mut UiText>(ph).unwrap().text = "Player name...".into();
        }
    }
    let quality = row(&mut s, panel, "Quality", UiElement::Dropdown);
    rename(&mut s, quality, "Quality Dropdown");
    {
        let mut d = s.scene.world.get::<&mut UiDropdown>(quality).unwrap();
        d.options = ["Low", "Medium", "High", "Ultra"]
            .map(String::from)
            .to_vec();
        d.value = 2;
    }

    let levels_title = create(&mut s, UiElement::Text, Some(panel));
    rename(&mut s, levels_title, "Levels Label");
    style_text(&mut s, levels_title, "Levels", 18.0, TextAnchor::LowerLeft);
    layout_size(&mut s, levels_title, None, Some(24.0), false);

    // Scroll list: Content with a vertical layout group, sized by a Content Size Fitter.
    // (GameObject ▸ UI ▸ Scroll View adds a horizontal bar too; this list only scrolls down.)
    let list = presets::vertical_scroll_view(&mut s.scene, "Level List", panel);
    layout_size(&mut s, list, None, Some(190.0), false);
    let content = s
        .scene
        .world
        .get::<&UiScrollRect>(list)
        .unwrap()
        .content
        .unwrap();
    s.scene.add(
        content,
        VerticalLayoutGroup(LinearLayout {
            padding: Padding {
                left: 6.0,
                right: 6.0,
                top: 6.0,
                bottom: 6.0,
            },
            spacing: 4.0,
            child_force_expand_height: false,
            ..Default::default()
        }),
    );
    s.scene.add(
        content,
        ContentSizeFitter {
            vertical_fit: FitMode::PreferredSize,
            ..Default::default()
        },
    );
    for i in 1..=8 {
        // The parent must be a UI object for GameObject ▸ UI to put it there.
        let b = create(&mut s, UiElement::Button, Some(content));
        rename(&mut s, b, &format!("Level {i}"));
        layout_size(&mut s, b, None, Some(32.0), false);
        let label = s.scene.children(b)[0];
        s.scene.world.get::<&mut UiText>(label).unwrap().text = format!("Level {i}");
    }

    let summary = create(&mut s, UiElement::Text, Some(panel));
    rename(&mut s, summary, "Summary");
    style_text(&mut s, summary, "", 15.0, TextAnchor::MiddleLeft);
    layout_size(&mut s, summary, None, Some(44.0), false);
    s.scene
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_ui_widgets_demo() {
    demo_scene().save(&project().join(SCENE)).unwrap();
}

#[test]
fn ui_widgets_demo_matches_its_generator() {
    let text = std::fs::read_to_string(project().join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_ui_widgets_demo",
    );
    let (scene, report) = Scene::from_json("UIWidgetsDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let slider = scene.find_by_name("Volume Slider").unwrap();
    let fill = scene
        .world
        .get::<&UiSlider>(slider)
        .unwrap()
        .fill_rect
        .unwrap();
    assert!(
        scene.is_ancestor(slider, fill),
        "references survive the file"
    );
    assert!(scene.find_by_name("EventSystem").is_some());
}

/// Playing the scene: the summary follows the widgets.
#[test]
fn ui_widgets_demo_plays() {
    use misty_engine::audio::Audio;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::{Input, LogKind};
    use misty_engine::settings::ProjectSettings;

    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let (audio, _) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root, &ProjectSettings::default(), audio);
    let screen = Vec2::new(1280.0, 720.0);
    let mut time = Time::default();
    let mut frame = |scene: &mut Scene, input: Input| {
        time.advance(1.0 / 60.0);
        rt.update(
            scene,
            &time,
            Input {
                screen_size: screen,
                ..input
            },
        );
    };
    let click = |p: Vec2| {
        [
            Input {
                mouse_position: Some(p),
                mouse_held: [true, false, false],
                mouse_pressed: [true, false, false],
                ..Default::default()
            },
            Input {
                mouse_position: Some(p),
                mouse_released: [true, false, false],
                ..Default::default()
            },
        ]
    };
    let centre = |scene: &Scene, name: &str| {
        let e = scene.find_by_name(name).unwrap();
        misty_engine::ui::layout(scene, screen)
            .node(e)
            .unwrap()
            .screen
            .center()
    };
    let summary = |scene: &Scene| {
        let e = scene.find_by_name("Summary").unwrap();
        scene.world.get::<&UiText>(e).unwrap().text.clone()
    };
    frame(&mut scene, Input::default());
    assert_eq!(
        summary(&scene),
        "Volume 80  |  Fullscreen off  |  Level 1\nName (none)  |  Quality High"
    );
    for input in click(centre(&scene, "Fullscreen Toggle")) {
        frame(&mut scene, input);
    }
    for input in click(centre(&scene, "Level 3")) {
        frame(&mut scene, input);
    }
    for input in click(centre(&scene, "Name Field")) {
        frame(&mut scene, input);
    }
    frame(
        &mut scene,
        Input {
            text: "Ada".into(),
            ..Default::default()
        },
    );
    frame(
        &mut scene,
        Input {
            typed: vec!["return".into()],
            ..Default::default()
        },
    );
    assert_eq!(
        summary(&scene),
        "Volume 80  |  Fullscreen on  |  Level 3\nName Ada  |  Quality High"
    );
    let logs = rt.take_logs();
    let problems: Vec<_> = logs.iter().filter(|(k, _)| *k != LogKind::Info).collect();
    assert!(problems.is_empty(), "{problems:?}");
    assert!(logs.iter().any(|(_, t)| t == "Hello, Ada!"), "{logs:?}");
}
