//! The SaveLocDemo sample scene: save games and localization together. Crates (Rigidbodies),
//! a player cube moved with WASD / arrow keys and a coin counter are Saveable; buttons Save,
//! Load and Delete one slot, add coins, push the crates and spawn coin prefabs; a dropdown
//! switches between English, Français, Español and العربية. Every label is a Localized Text
//! reading `Assets/Localization/SaveLocDemo.strings`.
//!
//! Scripts: `Assets/Scripts/SaveLocMenu.lua` (buttons, status, slot info),
//! `CoinCounter.lua` (OnSave / OnLoad), `CubeMover.lua`. Regenerate the scene, table and prefab
//! with `cargo test -p misty_editor -- --ignored write_saveloc_demo`; the other tests check the
//! committed files still match and play the scene headlessly.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::hecs::Entity;
use misty_engine::localization::{LocalizedText, StringTable};
use misty_engine::save::Saveable;
use misty_engine::scene::prefab::Prefab;
use misty_engine::scene::{
    BoxCollider, ButtonCall, CanvasScaler, HorizontalLayoutGroup, LayoutElement, LinearLayout,
    MeshFilter, MeshRenderer, Padding, PrimitiveMesh, RectTransform, Rigidbody, Scene, ScriptRef,
    Scripts, SphereCollider, TextAnchor, Transform, UiButton, UiDropdown, UiImage, UiText,
    VerticalLayoutGroup,
};
use misty_engine::ui::presets;

use crate::actions::{Action, Create};
use crate::project::ProjectBrowser;
use crate::state::EditorState;
use crate::ui_create::UiElement;

const SCENE: &str = "Assets/Scenes/SaveLocDemo.scene";
const TABLE: &str = "Assets/Localization/SaveLocDemo.strings";
const COIN: &str = "Assets/Prefabs/SaveLocCoin.prefab";

const PANEL: [f32; 4] = [0.10, 0.11, 0.14, 0.93];
const LIGHT: [f32; 4] = [0.93, 0.94, 0.96, 1.0];
const ACCENT: [f32; 4] = [1.0, 0.82, 0.35, 1.0];

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// The demo's String Table: key, English, French, Spanish, Arabic ("" = not translated yet).
const STRINGS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "title",
        "Save & Localization",
        "Sauvegarde et langues",
        "Guardado e idiomas",
        "الحفظ واللغات",
    ),
    ("button.save", "Save", "Sauvegarder", "Guardar", "حفظ"),
    ("button.load", "Load", "Charger", "Cargar", "تحميل"),
    ("button.delete", "Delete", "Supprimer", "Borrar", "حذف"),
    ("button.coin", "+1 Coin", "+1 pièce", "+1 moneda", "+1 عملة"),
    (
        "button.push",
        "Push Crates",
        "Pousser",
        "Empujar",
        "ادفع الصناديق",
    ),
    (
        "button.spawn",
        "Spawn Coin",
        "Nouvelle pièce",
        "Crear moneda",
        "أنشئ عملة",
    ),
    ("label.language", "Language", "Langue", "Idioma", "اللغة"),
    (
        "coins",
        "{0:plural:{} coin|{} coins}",
        "{0:plural:{} pièce|{} pièces}",
        "{0:plural:{} moneda|{} monedas}",
        "{0:plural:لا عملات|عملة واحدة|عملتان|{} عملات|{} عملة|{} عملة}",
    ),
    (
        "status.ready",
        "Move the cube with WASD or the arrow keys.",
        "Déplacez le cube avec ZQSD ou les flèches.",
        "Mueve el cubo con WASD o las flechas.",
        "حرّك المكعب بالأسهم.",
    ),
    (
        "status.saved",
        "Saved to slot \"{0}\".",
        "Sauvegardé dans « {0} ».",
        "Guardado en «{0}».",
        "تم الحفظ في «{0}».",
    ),
    (
        "status.loaded",
        "Loaded slot \"{0}\".",
        "« {0} » chargé.",
        "«{0}» cargado.",
        "تم تحميل «{0}».",
    ),
    (
        "status.deleted",
        "Deleted slot \"{0}\".",
        "« {0} » supprimé.",
        "«{0}» borrado.",
        "تم حذف «{0}».",
    ),
    (
        "status.nosave",
        "No save in slot \"{0}\" yet.",
        "Aucune sauvegarde dans « {0} ».",
        "No hay partida en «{0}».",
        "لا يوجد حفظ في «{0}».",
    ),
    (
        "slot.info",
        "Last save: {0}\n{1}, played {2}",
        "Dernière sauvegarde : {0}\n{1}, joué {2}",
        "Última partida: {0}\n{1}, jugado {2}",
        "آخر حفظ: {0}\n{1}، مدة اللعب {2}",
    ),
    (
        "slot.empty",
        "No save yet.",
        "Pas encore de sauvegarde.",
        "Aún no hay partida.",
        "لا يوجد حفظ بعد.",
    ),
    // Left untranslated in Spanish on purpose: the Tables window highlights it and the game
    // falls back to English.
    (
        "tip",
        "Saves keep the crates, the cube, your coins and spawned coins.",
        "Les sauvegardes gardent les caisses, le cube et vos pièces.",
        "",
        "الحفظ يحتفظ بالصناديق والمكعب والعملات.",
    ),
];

fn table() -> StringTable {
    let mut t = StringTable::new("SaveLocDemo", &["en", "fr", "es", "ar"]);
    for (key, en, fr, es, ar) in STRINGS {
        t.add_key(key);
        for (l, v) in [("en", en), ("fr", fr), ("es", es), ("ar", ar)] {
            t.set(key, l, v);
        }
    }
    t
}

/// A gold coin that rolls around (spawned by the Spawn Coin button).
fn coin_prefab() -> Prefab {
    let mut s = Scene::new("coin");
    let coin = s.spawn("Coin", None);
    s.set_transform(
        coin,
        Transform {
            scale: Vec3::splat(0.4),
            ..Default::default()
        },
    );
    s.add(coin, MeshFilter::primitive(PrimitiveMesh::Sphere));
    s.add(
        coin,
        MeshRenderer {
            material: Some("Assets/Materials/Stress Gold.mat".into()),
            ..Default::default()
        },
    );
    s.add(coin, SphereCollider::default());
    s.add(coin, Rigidbody::default());
    s.add(
        coin,
        Saveable {
            save_id: format!("{:032x}", 0xc01b_u128),
            ..Default::default()
        },
    );
    Prefab::from_scene(&s, coin)
}

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
            // Rows keep their height (their groups would make them flexible).
            flexible_height: Some(0.0),
            ..Default::default()
        },
    );
}

/// A Text driven by a Localized Text (the Text keeps the English string for Edit mode).
fn localize(
    s: &mut EditorState,
    text: Entity,
    key: &str,
    args: &[&str],
    size: f32,
    color: [f32; 4],
) {
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
        t.color = color;
    }
    s.scene.add(
        text,
        LocalizedText {
            table: "SaveLocDemo".into(),
            key: key.into(),
            args,
            ..Default::default()
        },
    );
}

fn text(s: &mut EditorState, parent: Entity, name: &str, key: &str, size: f32, h: f32) -> Entity {
    let t = create(s, UiElement::Text, Some(parent));
    rename(s, t, name);
    localize(s, t, key, &[], size, LIGHT);
    s.scene.world.get::<&mut UiText>(t).unwrap().alignment = TextAnchor::MiddleLeft;
    layout_size(s, t, None, Some(h), false);
    t
}

/// A row of buttons calling `method`s on `target`.
fn buttons(s: &mut EditorState, panel: Entity, name: &str, list: &[(&str, &str, &str, Entity)]) {
    let row = s.scene.spawn(name, Some(panel));
    s.scene.game_object_mut(row).layer = presets::UI_LAYER;
    s.scene.add(row, RectTransform::default());
    s.scene.add(
        row,
        HorizontalLayoutGroup(LinearLayout {
            spacing: 8.0,
            ..Default::default()
        }),
    );
    layout_size(s, row, None, Some(36.0), false);
    for (label, key, method, target) in list {
        let b = create(s, UiElement::Button, Some(row));
        rename(s, b, label);
        s.scene.world.get::<&mut UiButton>(b).unwrap().on_click = vec![ButtonCall {
            target: Some(*target),
            method: (*method).into(),
        }];
        let caption = s.scene.children(b)[0];
        localize(s, caption, key, &[], 16.0, [0.12, 0.12, 0.14, 1.0]);
        s.scene.world.get::<&mut UiText>(caption).unwrap().alignment = TextAnchor::MiddleCenter;
    }
}

fn script(path: &str) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            ..Default::default()
        }],
    }
}

/// Fixed Save IDs, so the committed scene doesn't change when regenerated.
fn saveable(s: &mut EditorState, e: Entity, n: u32) {
    s.scene.add(
        e,
        Saveable {
            save_id: format!("{:032x}", (0x5a7e_10c0_u128 << 96) | u128::from(n)),
            ..Default::default()
        },
    );
}

fn demo_scene() -> Scene {
    let mut s = EditorState::new(ProjectBrowser::open(
        std::env::temp_dir().join("misty_saveloc_demo_build"),
    ));
    s.scene = Scene::default_3d("SaveLocDemo");
    if let Some(cam) = s.scene.find_by_name("Main Camera") {
        s.scene.set_transform(
            cam,
            Transform::from_position(Vec3::new(-2.2, 6.5, -9.5))
                .with_euler(Vec3::new(32.0, 8.0, 0.0)),
        );
    }
    let floor = s.scene.spawn("Floor", None);
    s.scene.set_transform(
        floor,
        Transform {
            position: Vec3::new(0.0, -0.25, 0.0),
            scale: Vec3::new(12.0, 0.5, 9.0),
            ..Default::default()
        },
    );
    s.scene
        .add(floor, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.scene.add(
        floor,
        MeshRenderer {
            material: Some("Assets/Materials/Checker.mat".into()),
            ..Default::default()
        },
    );
    s.scene.add(floor, BoxCollider::default());

    // Three crates with physics and the player's cube: all Saveable.
    for (i, x) in [-1.5f32, 0.6, 2.7].into_iter().enumerate() {
        let c = s.scene.spawn(&format!("Crate {}", i + 1), None);
        s.scene.set_transform(
            c,
            Transform::from_position(Vec3::new(x, 0.5 + i as f32 * 0.02, 1.5))
                .with_euler(Vec3::new(0.0, 15.0 * i as f32, 0.0)),
        );
        s.scene.add(c, MeshFilter::primitive(PrimitiveMesh::Cube));
        s.scene.add(
            c,
            MeshRenderer {
                material: Some("Assets/Materials/AO Crate.mat".into()),
                ..Default::default()
            },
        );
        s.scene.add(c, BoxCollider::default());
        s.scene.add(c, Rigidbody::default());
        saveable(&mut s, c, i as u32 + 1);
    }
    let cube = s.scene.spawn("Player Cube", None);
    s.scene.set_transform(
        cube,
        Transform {
            position: Vec3::new(0.0, 0.4, -1.5),
            scale: Vec3::splat(0.8),
            ..Default::default()
        },
    );
    s.scene
        .add(cube, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.scene.add(
        cube,
        MeshRenderer {
            material: Some("Assets/Materials/Stress Teal.mat".into()),
            ..Default::default()
        },
    );
    s.scene.add(cube, script("Assets/Scripts/CubeMover.lua"));
    saveable(&mut s, cube, 10);

    // The coin counter keeps its count in saves with OnSave / OnLoad.
    let game = s.scene.spawn("Game", None);
    s.scene.add(game, script("Assets/Scripts/CoinCounter.lua"));
    saveable(&mut s, game, 20);
    {
        let mut sv = s.scene.world.get::<&mut Saveable>(game).unwrap();
        sv.save_transform = false;
        sv.save_active = false;
        sv.save_velocity = false;
    }
    let menu = s.scene.spawn("Menu", None);
    s.scene.add(menu, script("Assets/Scripts/SaveLocMenu.lua"));
    saveable(&mut s, menu, 30);
    {
        let mut sv = s.scene.world.get::<&mut Saveable>(menu).unwrap();
        sv.save_transform = false;
        sv.save_active = false;
        sv.save_velocity = false;
    }

    // The panel on the left.
    let canvas = create(&mut s, UiElement::Canvas, None);
    {
        let mut scaler = s.scene.world.get::<&mut CanvasScaler>(canvas).unwrap();
        scaler.reference_resolution = Vec2::new(1280.0, 720.0);
        scaler.match_width_or_height = 1.0;
    }
    let panel = create(&mut s, UiElement::Panel, Some(canvas));
    rename(&mut s, panel, "Save Panel");
    *s.scene.world.get::<&mut RectTransform>(panel).unwrap() = RectTransform {
        anchor_min: Vec2::new(0.0, 0.5),
        anchor_max: Vec2::new(0.0, 0.5),
        pivot: Vec2::new(0.0, 0.5),
        anchored_position: Vec2::new(24.0, 0.0),
        size_delta: Vec2::new(430.0, 470.0),
    };
    s.scene.world.get::<&mut UiImage>(panel).unwrap().color = PANEL;
    s.scene.add(
        panel,
        VerticalLayoutGroup(LinearLayout {
            padding: Padding {
                left: 18.0,
                right: 18.0,
                top: 16.0,
                bottom: 16.0,
            },
            spacing: 10.0,
            child_force_expand_height: false,
            ..Default::default()
        }),
    );
    let title = text(&mut s, panel, "Title", "title", 28.0, 40.0);
    s.scene.world.get::<&mut UiText>(title).unwrap().color = ACCENT;
    let coins = text(&mut s, panel, "Coins Text", "coins", 22.0, 30.0);
    {
        let mut l = s.scene.world.get::<&mut LocalizedText>(coins).unwrap();
        l.args = vec!["0".into()];
    }
    s.scene.world.get::<&mut UiText>(coins).unwrap().text = "0 coins".into();
    buttons(
        &mut s,
        panel,
        "Slot Buttons",
        &[
            ("Save Button", "button.save", "SaveGame", menu),
            ("Load Button", "button.load", "LoadGame", menu),
            ("Delete Button", "button.delete", "DeleteGame", menu),
        ],
    );
    buttons(
        &mut s,
        panel,
        "Play Buttons",
        &[
            ("Coin Button", "button.coin", "AddCoin", game),
            ("Push Button", "button.push", "PushCrates", menu),
            ("Spawn Button", "button.spawn", "SpawnCoin", menu),
        ],
    );

    // Language: label + dropdown.
    let row = s.scene.spawn("Language Row", Some(panel));
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
    layout_size(&mut s, row, None, Some(32.0), false);
    let label = create(&mut s, UiElement::Text, Some(row));
    rename(&mut s, label, "Language Label");
    localize(&mut s, label, "label.language", &[], 18.0, LIGHT);
    s.scene.world.get::<&mut UiText>(label).unwrap().alignment = TextAnchor::MiddleLeft;
    layout_size(&mut s, label, Some(110.0), None, false);
    let dropdown = create(&mut s, UiElement::Dropdown, Some(row));
    rename(&mut s, dropdown, "Language Dropdown");
    layout_size(&mut s, dropdown, None, None, true);
    {
        let mut d = s.scene.world.get::<&mut UiDropdown>(dropdown).unwrap();
        d.options = ["English", "Français", "Español", "العربية"]
            .map(String::from)
            .to_vec();
        d.value = 0;
        d.on_value_changed = vec![ButtonCall {
            target: Some(menu),
            method: "LanguageChanged".into(),
        }];
        if let Some(caption) = d.caption_text {
            drop(d);
            s.scene.world.get::<&mut UiText>(caption).unwrap().text = "English".into();
        }
    }

    let status = text(&mut s, panel, "Status", "status.ready", 16.0, 44.0);
    {
        let mut l = s.scene.world.get::<&mut LocalizedText>(status).unwrap();
        l.args = vec!["demo".into()];
    }
    let info = text(&mut s, panel, "Slot Info", "slot.empty", 15.0, 48.0);
    s.scene.world.get::<&mut UiText>(info).unwrap().color = [0.75, 0.78, 0.85, 1.0];
    let tip = text(&mut s, panel, "Tip", "tip", 14.0, 40.0);
    s.scene.world.get::<&mut UiText>(tip).unwrap().color = [0.6, 0.63, 0.7, 1.0];
    s.scene
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_saveloc_demo() {
    let root = project();
    demo_scene().save(&root.join(SCENE)).unwrap();
    table().save(&root.join(TABLE)).unwrap();
    coin_prefab().save(&root.join(COIN)).unwrap();
}

#[test]
fn saveloc_demo_matches_its_generator() {
    let root = project();
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_saveloc_demo",
    );
    let (scene, report) = Scene::from_json("SaveLocDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let t = std::fs::read_to_string(root.join(TABLE)).unwrap();
    crate::test_util::assert_json_close(
        &t,
        &table().to_json(),
        "regenerate with write_saveloc_demo",
    );
    let p = std::fs::read_to_string(root.join(COIN)).unwrap();
    crate::test_util::assert_json_close(
        &p,
        &coin_prefab().to_json(),
        "regenerate with write_saveloc_demo",
    );
    // Every Localized Text names a key of the table.
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
    assert!(n >= 12, "{n} localized texts");
    assert_eq!(table.missing(), [("tip".to_owned(), "es".to_owned())]);
}

/// Playing the demo: coins, Save, a language switch, more coins, Load (the scene reloads with
/// the saved coins, crates and cube, in the chosen language), Delete.
#[test]
fn saveloc_demo_plays() {
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
    let saves = std::env::temp_dir().join(format!("misty_saveloc_play_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&saves);
    let mut settings = ProjectSettings::default();
    settings.localization.startup = misty_engine::localization::StartupLocale::Default;
    let (audio, _) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root, &settings, audio);
    rt.set_save_folder(saves.clone());
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
    // A Text, or a button's caption.
    let shown = |scene: &Scene, name: &str| {
        let mut e = scene.find_by_name(name).unwrap();
        if scene.world.get::<&UiText>(e).is_err() {
            e = scene.children(e)[0];
        }
        scene.world.get::<&UiText>(e).unwrap().text.clone()
    };
    let press = |rt: &mut Runtime,
                 scene: &mut Scene,
                 name: &str,
                 frame: &mut dyn FnMut(&mut Runtime, &mut Scene, Input)| {
        for input in click(scene, name) {
            frame(rt, scene, input);
        }
    };
    frame(&mut rt, &mut scene, Input::default());
    assert_eq!(shown(&scene, "Coins Text"), "0 coins");
    assert_eq!(shown(&scene, "Slot Info"), "No save yet.");
    press(&mut rt, &mut scene, "Coin Button", &mut frame);
    assert_eq!(shown(&scene, "Coins Text"), "1 coin", "plural form");
    press(&mut rt, &mut scene, "Coin Button", &mut frame);
    press(&mut rt, &mut scene, "Save Button", &mut frame);
    assert_eq!(shown(&scene, "Status"), "Saved to slot \"demo\".");
    assert!(shown(&scene, "Slot Info").starts_with("Last save: "));
    let cube = scene.find_by_name("Player Cube").unwrap();
    let saved_cube = scene.world_position(cube);
    // Switch language (as the dropdown does) and keep playing.
    misty_engine::localization::set_session_locale("fr");
    frame(&mut rt, &mut scene, Input::default());
    assert_eq!(shown(&scene, "Coins Text"), "2 pièces");
    assert_eq!(shown(&scene, "Save Button"), "Sauvegarder");
    assert_eq!(
        shown(&scene, "Tip"),
        "Les sauvegardes gardent les caisses, le cube et vos pièces."
    );
    press(&mut rt, &mut scene, "Coin Button", &mut frame);
    for _ in 0..20 {
        frame(
            &mut rt,
            &mut scene,
            Input {
                held: ["right".to_owned()].into_iter().collect(),
                ..Default::default()
            },
        );
    }
    let cube = scene.find_by_name("Player Cube").unwrap();
    assert!(
        scene.world_position(cube).x > saved_cube.x + 0.5,
        "the cube moved"
    );
    press(&mut rt, &mut scene, "Spawn Button", &mut frame);
    assert!(scene.find_by_name("Coin(Clone)").is_some());
    assert_eq!(shown(&scene, "Coins Text"), "3 pièces");
    press(&mut rt, &mut scene, "Load Button", &mut frame);
    assert!(rt.take_scene_reloaded(), "Load reloads the scene");
    frame(&mut rt, &mut scene, Input::default());
    assert_eq!(
        shown(&scene, "Coins Text"),
        "2 pièces",
        "coins from the save, still French"
    );
    assert_eq!(shown(&scene, "Status"), "« demo » chargé.");
    let cube = scene.find_by_name("Player Cube").unwrap();
    assert!((scene.world_position(cube) - saved_cube).length() < 1e-3);
    assert!(
        scene.find_by_name("Coin(Clone)").is_none(),
        "spawned after saving"
    );
    // Spanish has no "tip": English is the fallback.
    misty_engine::localization::set_session_locale("es");
    frame(&mut rt, &mut scene, Input::default());
    assert_eq!(
        shown(&scene, "Tip"),
        "Saves keep the crates, the cube, your coins and spawned coins."
    );
    press(&mut rt, &mut scene, "Delete Button", &mut frame);
    assert_eq!(shown(&scene, "Status"), "«demo» borrado.");
    assert_eq!(shown(&scene, "Slot Info"), "Aún no hay partida.");
    assert!(!misty_engine::save::exists(&saves, "demo"));

    let problems: Vec<_> = logs.iter().filter(|(k, _)| *k != LogKind::Info).collect();
    assert!(problems.is_empty(), "{problems:?}");
    misty_engine::localization::reset_session();
    std::fs::remove_dir_all(saves).ok();
}
