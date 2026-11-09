//! The LocalizationDemo2 sample scene: a game UI in English, Türkçe and العربية. A dropdown
//! switches the locale; every label is a Localized Text reading
//! `Assets/Localization/LocalizationDemo2.strings`:
//! - Turkish casing: `{city:upper}` turns "istanbul" into "İSTANBUL" (dotted capital I).
//! - Arabic: joined letters, right-to-left lines with numbers and Latin words inside, six plural
//!   forms, and Texts that align to the right (Mirror for RTL).
//! - A TextMeshPro-like Text (rich text tags and an outline) in all three languages.
//!
//! Script: `Assets/Scripts/LocalizationDemo2.lua`. Regenerate the scene and table with
//! `cargo test -p misty_editor -- --ignored write_localization_demo2`; the other tests check
//! the committed files still match and play the scene headlessly.

use std::path::PathBuf;

use glam::Vec2;
use misty_engine::hecs::Entity;
use misty_engine::localization::{LocalizedText, StringTable};
use misty_engine::scene::{
    ButtonCall, CanvasScaler, HorizontalLayoutGroup, LayoutElement, LinearLayout, Padding,
    RectTransform, Scene, ScriptRef, Scripts, TextAnchor, UiButton, UiDropdown, UiImage, UiText,
    VerticalLayoutGroup,
};
use misty_engine::ui::presets;

use crate::actions::{Action, Create};
use crate::project::ProjectBrowser;
use crate::state::EditorState;
use crate::ui_create::UiElement;

const SCENE: &str = "Assets/Scenes/LocalizationDemo2.scene";
const TABLE: &str = "Assets/Localization/LocalizationDemo2.strings";
const SCRIPT: &str = "Assets/Scripts/LocalizationDemo2.lua";
const TABLE_NAME: &str = "LocalizationDemo2";

const PANEL: [f32; 4] = [0.09, 0.10, 0.13, 0.94];
const LIGHT: [f32; 4] = [0.93, 0.94, 0.96, 1.0];
const ACCENT: [f32; 4] = [1.0, 0.82, 0.35, 1.0];

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// The demo's String Table: key, English, Turkish, Arabic.
const STRINGS: &[(&str, &str, &str, &str)] = &[
    ("title", "Languages Demo", "Dil Demosu", "عرض اللغات"),
    (
        "greeting",
        "Welcome, {name}!",
        "Hoş geldin, {name}!",
        "أهلًا بك يا {name}!",
    ),
    (
        "city",
        "Next stop: {city:upper}",
        "Sıradaki durak: {city:upper}",
        "المحطة التالية: {city:upper}",
    ),
    (
        "stars",
        "{0:plural:{} star|{} stars}",
        "{0:plural:{} yıldız}",
        "{0:plural:لا نجوم|نجمة واحدة|نجمتان|{} نجوم|{} نجمة|{} نجمة}",
    ),
    (
        "version",
        "Version {0} is ready.",
        "Sürüm {0} hazır.",
        "الإصدار {0} جاهز.",
    ),
    (
        "hint",
        "Press Space to jump (2× for a double jump).",
        "Zıplamak için Space tuşuna basın (çift zıplama için 2×).",
        "اضغط Space للقفز (2× للقفز المزدوج).",
    ),
    (
        "rich",
        "<b>Bold</b>, <i>italic</i> and <color=#ffd25a>gold</color> text",
        "<b>Kalın</b>, <i>italik</i> ve <color=#ffd25a>altın</color> metin",
        "نص <b>عريض</b> و<i>مائل</i> و<color=#ffd25a>ذهبي</color>",
    ),
    ("button.star", "+1 Star", "+1 Yıldız", "+1 نجمة"),
    ("label.language", "Language", "Dil", "اللغة"),
];

fn table() -> StringTable {
    let mut t = StringTable::new(TABLE_NAME, &["en", "tr", "ar"]);
    for (key, en, tr, ar) in STRINGS {
        t.add_key(key);
        for (l, v) in [("en", en), ("tr", tr), ("ar", ar)] {
            t.set(key, l, v);
        }
    }
    t
}

fn create(s: &mut EditorState, el: UiElement, parent: Option<Entity>) -> Entity {
    let what = Create::Ui(el);
    crate::commands::apply(s, Action::Create { what, parent });
    s.selection.active().expect("created object is selected")
}

fn rename(s: &mut EditorState, e: Entity, name: &str) {
    s.scene.game_object_mut(e).name = name.into();
}

fn height(s: &mut EditorState, e: Entity, h: f32) {
    s.scene.add(
        e,
        LayoutElement {
            preferred_height: Some(h),
            flexible_height: Some(0.0),
            ..Default::default()
        },
    );
}

/// A Text driven by a Localized Text (the Text keeps the English string for Edit mode). Texts
/// mirror their alignment for right-to-left languages.
fn localize(s: &mut EditorState, text: Entity, key: &str, args: &[&str], size: f32) {
    let en = STRINGS
        .iter()
        .find(|r| r.0 == key)
        .map(|r| r.1)
        .unwrap_or(key);
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let shown = misty_engine::localization::format::format(
        en,
        &misty_engine::localization::Args::from_entries(&args),
        "en",
    );
    {
        let mut t = s.scene.world.get::<&mut UiText>(text).unwrap();
        t.text = shown;
        t.font_size = size;
        t.color = LIGHT;
        t.mirror_rtl = true;
    }
    s.scene.add(
        text,
        LocalizedText {
            table: TABLE_NAME.into(),
            key: key.into(),
            args,
            ..Default::default()
        },
    );
}

fn text(s: &mut EditorState, parent: Entity, name: &str, key: &str, args: &[&str]) -> Entity {
    let t = create(s, UiElement::Text, Some(parent));
    rename(s, t, name);
    localize(s, t, key, args, 20.0);
    s.scene.world.get::<&mut UiText>(t).unwrap().alignment = TextAnchor::MiddleLeft;
    height(s, t, 32.0);
    t
}

fn demo_scene() -> Scene {
    let mut s = EditorState::new(ProjectBrowser::open(
        std::env::temp_dir().join("misty_localization_demo2_build"),
    ));
    s.scene = Scene::default_3d("LocalizationDemo2");
    let game = s.scene.spawn("Game", None);
    s.scene.add(
        game,
        Scripts {
            list: vec![ScriptRef {
                path: SCRIPT.into(),
                ..Default::default()
            }],
        },
    );

    let canvas = create(&mut s, UiElement::Canvas, None);
    {
        let mut scaler = s.scene.world.get::<&mut CanvasScaler>(canvas).unwrap();
        scaler.reference_resolution = Vec2::new(1280.0, 720.0);
        scaler.match_width_or_height = 1.0;
    }
    let panel = create(&mut s, UiElement::Panel, Some(canvas));
    rename(&mut s, panel, "Panel");
    *s.scene.world.get::<&mut RectTransform>(panel).unwrap() = RectTransform {
        anchor_min: Vec2::splat(0.5),
        anchor_max: Vec2::splat(0.5),
        pivot: Vec2::splat(0.5),
        anchored_position: Vec2::ZERO,
        size_delta: Vec2::new(640.0, 520.0),
    };
    s.scene.world.get::<&mut UiImage>(panel).unwrap().color = PANEL;
    s.scene.add(
        panel,
        VerticalLayoutGroup(LinearLayout {
            padding: Padding {
                left: 28.0,
                right: 28.0,
                top: 22.0,
                bottom: 22.0,
            },
            spacing: 8.0,
            child_force_expand_height: false,
            ..Default::default()
        }),
    );
    let title = text(&mut s, panel, "Title", "title", &[]);
    {
        let mut t = s.scene.world.get::<&mut UiText>(title).unwrap();
        t.font_size = 34.0;
        t.color = ACCENT;
    }
    height(&mut s, title, 48.0);
    text(&mut s, panel, "Greeting", "greeting", &["name=Ayşe"]);
    text(&mut s, panel, "City", "city", &["city=istanbul"]);
    text(&mut s, panel, "Stars Text", "stars", &["3"]);
    text(&mut s, panel, "Version", "version", &["0.3.1"]);
    text(&mut s, panel, "Hint", "hint", &[]);
    // TextMeshPro-like: rich text tags, a larger size and an outline (distance field glyphs).
    let rich = text(&mut s, panel, "Rich Text", "rich", &[]);
    {
        let mut t = s.scene.world.get::<&mut UiText>(rich).unwrap();
        t.font_size = 24.0;
        t.effects.outline_width = 1.5;
        t.effects.outline_color = [0.05, 0.05, 0.08, 1.0];
    }
    height(&mut s, rich, 40.0);

    // "+1 Star" and the language dropdown on one row.
    let row = s.scene.spawn("Controls", Some(panel));
    s.scene.game_object_mut(row).layer = presets::UI_LAYER;
    s.scene.add(row, RectTransform::default());
    s.scene.add(
        row,
        HorizontalLayoutGroup(LinearLayout {
            spacing: 12.0,
            child_force_expand_width: false,
            ..Default::default()
        }),
    );
    height(&mut s, row, 40.0);
    let button = create(&mut s, UiElement::Button, Some(row));
    rename(&mut s, button, "Star Button");
    s.scene.world.get::<&mut UiButton>(button).unwrap().on_click = vec![ButtonCall {
        target: Some(game),
        method: "AddStar".into(),
    }];
    s.scene.add(
        button,
        LayoutElement {
            preferred_width: Some(150.0),
            ..Default::default()
        },
    );
    let caption = s.scene.children(button)[0];
    localize(&mut s, caption, "button.star", &[], 18.0);
    {
        let mut t = s.scene.world.get::<&mut UiText>(caption).unwrap();
        t.color = [0.12, 0.12, 0.14, 1.0];
        t.alignment = TextAnchor::MiddleCenter;
    }
    let label = create(&mut s, UiElement::Text, Some(row));
    rename(&mut s, label, "Language Label");
    localize(&mut s, label, "label.language", &[], 18.0);
    s.scene.world.get::<&mut UiText>(label).unwrap().alignment = TextAnchor::MiddleRight;
    s.scene.add(
        label,
        LayoutElement {
            preferred_width: Some(120.0),
            ..Default::default()
        },
    );
    let dropdown = create(&mut s, UiElement::Dropdown, Some(row));
    rename(&mut s, dropdown, "Language Dropdown");
    s.scene.add(
        dropdown,
        LayoutElement {
            preferred_width: Some(200.0),
            ..Default::default()
        },
    );
    {
        let mut d = s.scene.world.get::<&mut UiDropdown>(dropdown).unwrap();
        d.options = ["English", "Türkçe", "العربية"].map(String::from).to_vec();
        d.value = 0;
        d.on_value_changed = vec![ButtonCall {
            target: Some(game),
            method: "LanguageChanged".into(),
        }];
        if let Some(caption) = d.caption_text {
            drop(d);
            s.scene.world.get::<&mut UiText>(caption).unwrap().text = "English".into();
        }
    }
    s.scene
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_localization_demo2() {
    let root = project();
    demo_scene().save(&root.join(SCENE)).unwrap();
    table().save(&root.join(TABLE)).unwrap();
}

#[test]
fn localization_demo2_matches_its_generator() {
    let root = project();
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_localization_demo2",
    );
    let (scene, report) = Scene::from_json("LocalizationDemo2", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let t = std::fs::read_to_string(root.join(TABLE)).unwrap();
    crate::test_util::assert_json_close(
        &t,
        &table().to_json(),
        "regenerate with write_localization_demo2",
    );
    assert!(root.join(SCRIPT).exists(), "{SCRIPT}");
    // Every Localized Text names a key of the table, and every key is translated.
    let table = table();
    let mut n = 0;
    for (e, l) in scene.world.query::<(Entity, &LocalizedText)>().iter() {
        assert!(
            table.entry(&l.key).is_some(),
            "{}: {}",
            scene.name(e),
            l.key
        );
        n += 1;
    }
    assert!(n >= 9, "{n} localized texts");
    assert!(table.missing().is_empty(), "{:?}", table.missing());
}

/// Playing the demo: English, then Turkish (locale-aware upper case), then Arabic (shaped,
/// mirrored alignment, Arabic plural forms), switching as the dropdown does.
#[test]
fn localization_demo2_plays_in_three_languages() {
    use misty_engine::audio::Audio;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::{Input, LogKind};
    use misty_engine::settings::ProjectSettings;

    let _lock = misty_engine::localization::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    misty_engine::localization::reset_session();
    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    scene.set_scene_path(0, SCENE);
    let mut settings = ProjectSettings::default();
    settings.localization.startup = misty_engine::localization::StartupLocale::Default;
    let (audio, _) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root, &settings, audio);
    let screen = Vec2::new(1280.0, 720.0);
    let mut time = Time::default();
    let mut logs = Vec::new();
    let mut frame = |rt: &mut Runtime, scene: &mut Scene, input: Input| {
        time.advance(1.0 / 60.0);
        rt.update(
            scene,
            &time,
            Input {
                screen_size: screen,
                ..input
            },
        );
        logs.extend(rt.take_logs());
    };
    let shown = |scene: &Scene, name: &str| {
        let mut e = scene.find_by_name(name).unwrap();
        if scene.world.get::<&UiText>(e).is_err() {
            e = scene.children(e)[0];
        }
        scene.world.get::<&UiText>(e).unwrap().text.clone()
    };
    let click = |scene: &Scene, name: &str| {
        let e = scene.find_by_name(name).unwrap();
        let p = misty_engine::ui::layout(scene, screen)
            .node(e)
            .unwrap()
            .screen
            .center();
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
    frame(&mut rt, &mut scene, Input::default());
    assert_eq!(shown(&scene, "City"), "Next stop: ISTANBUL");
    assert_eq!(shown(&scene, "Stars Text"), "3 stars");
    assert_eq!(shown(&scene, "Greeting"), "Welcome, Ayşe!");

    misty_engine::localization::set_session_locale("tr");
    frame(&mut rt, &mut scene, Input::default());
    assert_eq!(
        shown(&scene, "City"),
        "Sıradaki durak: İSTANBUL",
        "Turkish dotted İ"
    );
    assert_eq!(shown(&scene, "Stars Text"), "3 yıldız");
    assert_eq!(shown(&scene, "Star Button"), "+1 Yıldız");

    misty_engine::localization::set_session_locale("ar");
    frame(&mut rt, &mut scene, Input::default());
    assert_eq!(shown(&scene, "Stars Text"), "3 نجوم", "Arabic few form");
    for input in click(&scene, "Star Button") {
        frame(&mut rt, &mut scene, input);
    }
    for _ in 0..8 {
        for input in click(&scene, "Star Button") {
            frame(&mut rt, &mut scene, input);
        }
    }
    assert_eq!(shown(&scene, "Stars Text"), "12 نجمة", "Arabic many form");
    assert_eq!(shown(&scene, "Version"), "الإصدار 0.3.1 جاهز.");
    // Right-to-left text takes the rich path (shaping) and mirrors its alignment.
    let hint = scene.find_by_name("Hint").unwrap();
    let t = scene.world.get::<&UiText>(hint).unwrap().clone();
    assert!(misty_engine::ui::rich::needs_rich(&t, &t.text));
    assert_eq!(
        misty_engine::ui::rich::alignment(&t, &t.text),
        TextAnchor::MiddleRight
    );

    let problems: Vec<_> = logs.iter().filter(|(k, _)| *k != LogKind::Info).collect();
    assert!(problems.is_empty(), "{problems:?}");
    misty_engine::localization::reset_session();
}
