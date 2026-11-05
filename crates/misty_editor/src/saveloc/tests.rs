//! Kittests of the Save Data and Localization Tables windows and the Localization page.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::localization::StringTable;
use misty_engine::save::{self, SaveData, SlotInfo};

use crate::project::ProjectBrowser;
use crate::state::EditorState;

fn is_input(n: &egui_kittest::kittest::AccessKitNode<'_>, value: &str) -> bool {
    n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some(value)
}

/// The text box showing `value`.
fn input<'a>(h: &'a Harness<'_, EditorState>, value: &'static str) -> egui_kittest::Node<'a> {
    h.get_by(move |n| is_input(n, value))
}

fn temp_root(tag: &str) -> std::path::PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_saveloc_{tag}_{n}"));
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    root
}

fn slot(name: &str, time: u64, scene: &str) -> SaveData {
    let mut d = SaveData {
        format: save::FORMAT,
        version: 1,
        info: SlotInfo {
            slot: name.into(),
            time,
            scene: scene.into(),
            play_time: 125.0,
            version: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    d.objects.insert("abc".into(), Default::default());
    d.destroyed.push("gone".into());
    d
}

#[test]
fn save_data_window_lists_views_and_deletes_slots() {
    let root = temp_root("savewin");
    let folder = root.join(save::EDITOR_FOLDER);
    save::write_slot(
        &folder,
        "older",
        &slot("older", 1_700_000_000, "Menu"),
        false,
    )
    .unwrap();
    save::write_slot(
        &folder,
        "newer",
        &slot("newer", 1_800_000_000, "Level 2"),
        true,
    )
    .unwrap();
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    super::save_data::open(&mut s);
    assert!(s.saveloc.save_data_open);
    let mut h = Harness::builder().with_size([700.0, 700.0]).build_ui_state(
        |ui, s: &mut EditorState| super::save_data::contents(ui, s),
        s,
    );
    h.run();
    // Opening selects the newest slot, so its details show next to the list.
    assert_eq!(
        h.state().saveloc.save_data.selected.as_deref(),
        Some("newer")
    );
    h.get_by_label("older");
    assert_eq!(h.get_all_by_label("Level 2").count(), 2, "list + details");
    h.get_by_label(&save::format_time(1_700_000_000));
    assert_eq!(
        h.get_all_by_label("2:05").count(),
        3,
        "play time column + details"
    );
    h.state_mut().saveloc.save_data.selected = None;
    h.run();

    // View: details and the save's JSON.
    h.get_all_by_label("View").next().unwrap().click();
    h.run();
    assert_eq!(
        h.state().saveloc.save_data.selected.as_deref(),
        Some("newer")
    );
    h.get_by_label("Show Contents");
    // The JSON is the value of a read-only text box.
    assert!(
        h.query_by(|n| n.role() == egui::accesskit::Role::MultilineTextInput
            && n.value().is_some_and(|v| v.contains("\"destroyed\"")))
            .is_some()
    );

    // Delete asks for a second click.
    h.get_all_by_label("Delete").last().unwrap().click();
    h.run();
    assert!(
        save::exists(&folder, "older"),
        "first click only arms Delete"
    );
    h.get_by_label("Confirm").click();
    h.run();
    assert!(!save::exists(&folder, "older"));
    assert!(save::exists(&folder, "newer"));
    assert!(h.query_by_label("older").is_none());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn save_data_window_without_saves_explains_how_to_make_one() {
    let root = temp_root("savewin_empty");
    let s = EditorState::new(ProjectBrowser::open(root.clone()));
    let mut h = Harness::builder().with_size([600.0, 300.0]).build_ui_state(
        |ui, s: &mut EditorState| super::save_data::contents(ui, s),
        s,
    );
    h.run();
    h.get_by_label("No saves yet.");
    std::fs::remove_dir_all(root).ok();
}

fn table_project(tag: &str) -> (std::path::PathBuf, String) {
    let root = temp_root(tag);
    let rel = "Assets/Localization/UI.strings".to_owned();
    let mut t = StringTable::new("UI", &["en", "fr"]);
    t.set("menu.play", "en", "Play");
    t.set("menu.play", "fr", "Jouer");
    t.set("menu.quit", "en", "Quit");
    t.save(&root.join(&rel)).unwrap();
    (root, rel)
}

#[test]
fn tables_window_edits_cells_adds_keys_and_highlights_missing() {
    let (root, rel) = table_project("tables");
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    super::tables::open(&mut s, &rel);
    let mut h = Harness::builder()
        .with_size([900.0, 600.0])
        .build_ui_state(|ui, s: &mut EditorState| super::tables::contents(ui, s), s);
    h.run();
    h.get_by_label("fr (1 missing)");
    input(&h, "Jouer");
    input(&h, "menu.quit");

    // Edit a cell: the file is saved right away.
    input(&h, "Jouer").focus();
    h.run();
    input(&h, "Jouer").type_text(" !");
    h.run();
    let saved = StringTable::load(&root.join(&rel)).unwrap();
    let fr = saved.get("menu.play", "fr").unwrap().to_owned();
    assert!(fr.contains('!') && fr.contains("Jouer"), "{fr}");

    // Add a key: a new row with every cell missing.
    h.state_mut().saveloc.tables.new_key = "menu.options".into();
    h.get_by_label("Add Key").click();
    h.run();
    input(&h, "menu.options");
    h.get_by_label("fr (2 missing)");
    h.get_by_label("en (1 missing)");
    let saved = StringTable::load(&root.join(&rel)).unwrap();
    assert!(saved.entry("menu.options").is_some());

    // Only Missing hides complete rows.
    h.get_by_label("Only Missing").click();
    h.run();
    assert!(h.query_by(|n| is_input(n, "menu.play")).is_none());
    input(&h, "menu.quit");
    h.get_by_label("Only Missing").click();
    h.run();

    // Remove a key.
    h.get_all_by_label("🗑").next().unwrap().click();
    h.run();
    let saved = StringTable::load(&root.join(&rel)).unwrap();
    assert!(saved.entry("menu.play").is_none());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn tables_window_imports_and_exports_csv() {
    let (root, rel) = table_project("tables_csv");
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    super::tables::open(&mut s, &rel);
    let out = root.join("UI.csv");
    super::tables::export_csv(&mut s, &out).unwrap();
    let csv = std::fs::read_to_string(&out).unwrap();
    assert!(csv.contains("menu.play,Play,Jouer"), "{csv}");
    let incoming = root.join("in.csv");
    std::fs::write(
        &incoming,
        "Key,en,fr,es\nmenu.quit,Quit,\"Quitter, vite\",Salir\nmenu.new,\"New\nline\",Nouveau,Nuevo\n",
    )
    .unwrap();
    let (added, changed) = super::tables::import_csv(&mut s, &incoming).unwrap();
    assert_eq!((added, changed), (1, 5));
    let mut h = Harness::builder()
        .with_size([1000.0, 600.0])
        .build_ui_state(|ui, s: &mut EditorState| super::tables::contents(ui, s), s);
    h.run();
    input(&h, "Quitter, vite");
    h.get_by_label("es (1 missing)");
    assert!(
        h.query_by_label_contains("fr (").is_none(),
        "fr complete now"
    );
    h.get_by_label_contains("Imported in.csv");
    let saved = StringTable::load(&root.join(&rel)).unwrap();
    assert_eq!(saved.get("menu.new", "en"), Some("New\nline"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn tables_window_creates_a_table_with_the_project_locales() {
    let root = temp_root("tables_new");
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    s.project_settings.localization.locales = vec![
        misty_engine::localization::Locale::new("en", "English"),
        misty_engine::localization::Locale::new("ar", "العربية"),
    ];
    super::tables::open_window(&mut s);
    let mut h = Harness::builder()
        .with_size([900.0, 400.0])
        .build_ui_state(|ui, s: &mut EditorState| super::tables::contents(ui, s), s);
    h.run();
    h.get_by_label("No String Table open.");
    h.state_mut().saveloc.tables.new_table = "Dialogue".into();
    h.get_by_label("Create Table").click();
    h.run();
    let t = StringTable::load(&root.join("Assets/Localization/Dialogue.strings")).unwrap();
    assert_eq!(t.locales, ["en", "ar"]);
    h.get_by_label_contains("Created Assets/Localization/Dialogue.strings");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn localization_settings_page_adds_locales() {
    let root = temp_root("locsettings");
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    crate::project_settings::open_at(&mut s, crate::project_settings::Category::Localization);
    let mut h = Harness::builder().with_size([800.0, 600.0]).build_ui_state(
        |ui, s: &mut EditorState| crate::project_settings::contents(ui, s),
        s,
    );
    h.run();
    h.get_by_label("Startup Locale");
    h.get_by_label("Add Locale").click();
    h.run();
    h.get_by_label("Français (fr)").click();
    h.run();
    let s = h.state();
    assert_eq!(s.project_settings.localization.locales.len(), 1);
    assert_eq!(s.project_settings.localization.locales[0].code, "fr");
    let saved = misty_engine::settings::ProjectSettings::load(&root).unwrap();
    assert_eq!(
        saved.localization.locales[0].name, "Français",
        "saved right away"
    );
    std::fs::remove_dir_all(root).ok();
}
