//! Kittests of the Code Editor window: opening scripts from the Project window and the Console,
//! typing, saving, the dirty marker, find, prompts, reloads and diagnostics.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::text::Pos;
use crate::actions::{Action, AssetKind};
use crate::layout::Tab;
use crate::project::ProjectBrowser;
use crate::state::EditorState;

const ROTATOR: &str = "Assets/Scripts/Rotator.lua";

/// A temp copy of the sample project (tests never touch the real one).
fn sample_copy(tag: &str) -> (EditorState, PathBuf) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    let root = std::env::temp_dir().join(format!("misty_code_editor_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for dir in ["Assets", "ProjectSettings"] {
        crate::asset_ops::copy_all(&src.join(dir), &root.join(dir)).unwrap();
    }
    (EditorState::new(ProjectBrowser::open(root.clone())), root)
}

/// The Code Editor tab with its windows.
fn harness(s: EditorState) -> Harness<'static, EditorState> {
    let mut h = Harness::builder().with_size([900.0, 600.0]).build_ui_state(
        |ui, s: &mut EditorState| {
            super::show(ui, s);
            super::show_windows(ui.ctx(), s);
        },
        s,
    );
    h.run_steps(2);
    h
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

fn focus_tab_requested(s: &EditorState) -> bool {
    s.actions
        .iter()
        .any(|a| matches!(a, Action::FocusTab(Tab::CodeEditor)))
}

#[test]
fn double_click_in_project_opens_the_script() {
    let (mut s, root) = sample_copy("project");
    s.project.open_folder(root.join("Assets/Scripts"));
    let mut h = Harness::builder().with_size([800.0, 500.0]).build_ui_state(
        |ui, s: &mut EditorState| crate::panels::project::show(ui, s),
        s,
    );
    // Test frames are far apart in time: widen the double-click window, let the clock pass the
    // (doubled) triple-click window, then click twice one frame apart.
    h.ctx
        .options_mut(|o| o.input_options.max_double_click_delay = 5.0);
    h.run_steps(60);
    h.get_by_label("Rotator.lua").click();
    h.step();
    h.get_by_label("Rotator.lua").click();
    h.run();
    let s = h.state();
    assert_eq!(s.code_editor.docs.len(), 1, "opened in the Code Editor");
    assert_eq!(s.code_editor.docs[0].path, ROTATOR);
    assert!(focus_tab_requested(s));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn typing_marks_dirty_and_ctrl_s_saves() {
    let (mut s, root) = sample_copy("save");
    s.code_editor.open(ROTATOR, None).unwrap();
    let mut h = harness(s);
    h.get_by_label("Rotator.lua");
    h.event(Event::Text("-- edited".into()));
    h.run();
    h.get_by_label("Rotator.lua ●");
    assert!(h.state().code_editor.docs[0].dirty());

    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    h.run();
    let file = read(&root.join(ROTATOR));
    assert!(file.starts_with("-- edited"), "{file}");
    assert!(!h.state().code_editor.docs[0].dirty());
    h.get_by_label("Rotator.lua");
    assert!(h.query_by_label("Rotator.lua ●").is_none());

    // Undo makes it dirty again; undoing back to the saved text is clean.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert!(h.state().code_editor.docs[0].dirty());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.run();
    assert!(!h.state().code_editor.docs[0].dirty());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn keyboard_commands_and_chords_edit_the_text() {
    let (mut s, root) = sample_copy("keys");
    s.code_editor.open(ROTATOR, Some(1)).unwrap();
    let first = s.code_editor.docs[0].editor.buf.line(0).to_owned();
    let mut h = harness(s);
    // Ctrl+K, Ctrl+C comments the line; Ctrl+K, U uncomments it.
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    h.get_by_label_contains("Waiting for second key");
    h.key_press_modifiers(Modifiers::COMMAND, Key::C);
    h.run();
    assert_eq!(
        h.state().code_editor.docs[0].editor.buf.line(0),
        format!("-- {first}")
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.key_press(Key::U);
    h.run();
    assert_eq!(h.state().code_editor.docs[0].editor.buf.line(0), first);
    // Ctrl+D duplicates, Alt+Down moves it, Ctrl+Shift+L deletes.
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.run();
    let ed = &h.state().code_editor.docs[0].editor;
    assert_eq!(ed.buf.line(1), first);
    assert_eq!(ed.caret().line, 1);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::L);
    h.run();
    assert_ne!(h.state().code_editor.docs[0].editor.buf.line(1), first);
    // Ctrl+G opens Go To Line.
    h.key_press_modifiers(Modifiers::COMMAND, Key::G);
    h.run();
    h.get_by_label_contains("Line number (1 -");
    h.event(Event::Text("5".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().code_editor.docs[0].editor.caret().line, 4);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn find_bar_counts_and_steps_through_matches() {
    let (mut s, root) = sample_copy("find");
    s.code_editor.open(ROTATOR, None).unwrap();
    let text = s.code_editor.docs[0].editor.text();
    let expected = text.matches("speed").count();
    assert!(expected >= 2, "{text}");
    let mut h = harness(s);
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(2);
    h.event(Event::Text("speed".into()));
    h.run_steps(2);
    h.get_by_label(&format!("1 of {expected}"));
    let sel = h.state().code_editor.docs[0].editor.primary();
    assert_eq!(
        h.state().code_editor.docs[0]
            .editor
            .buf
            .slice(sel.start(), sel.end()),
        "speed"
    );
    h.key_press(Key::Enter);
    h.run_steps(2);
    h.get_by_label(&format!("2 of {expected}"));
    // Esc closes the bar.
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!h.state().code_editor.ui.find.open);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn clicking_a_console_error_opens_the_line() {
    let (mut s, root) = sample_copy("console");
    s.console.log(
        crate::console::LogLevel::Error,
        format!("Rotator (Update): [string \"{ROTATOR}\"]:6: attempt to index a nil value"),
    );
    let mut h = Harness::builder().with_size([800.0, 300.0]).build_ui_state(
        |ui, s: &mut EditorState| crate::panels::console::show(ui, s),
        s,
    );
    h.run_steps(2);
    h.get_by_label("Rotator.lua:6").click();
    h.run();
    let s = h.state();
    assert_eq!(s.code_editor.docs.len(), 1);
    assert_eq!(s.code_editor.docs[0].editor.caret().line, 5);
    assert!(focus_tab_requested(s));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn closing_a_modified_file_asks_to_save() {
    let (mut s, root) = sample_copy("close");
    s.code_editor.open(ROTATOR, None).unwrap();
    let before = read(&root.join(ROTATOR));
    let mut h = harness(s);
    h.event(Event::Text("x".into()));
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::F4);
    h.run_steps(2);
    h.get_by_label("Save changes?");
    h.get_by_label("Don't Save").click();
    h.run_steps(2);
    assert!(h.state().code_editor.docs.is_empty());
    assert_eq!(read(&root.join(ROTATOR)), before, "nothing written");

    // Quitting with unsaved scripts asks too, and Save writes them.
    let s = h.state_mut();
    s.code_editor.open(ROTATOR, None).unwrap();
    s.code_editor.docs[0].editor.type_text("--q");
    assert!(s.code_editor.cancel_close());
    h.run_steps(2);
    h.get_by_label("Save").click();
    h.run_steps(2);
    assert!(read(&root.join(ROTATOR)).starts_with("--q"));
    assert!(!h.state_mut().code_editor.cancel_close(), "confirmed once");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn files_changed_on_disk_reload_or_ask() {
    let (mut s, root) = sample_copy("disk");
    let ce = &mut s.code_editor;
    ce.open(ROTATOR, None).unwrap();
    let abs = root.join(ROTATOR);
    let later = |n| Instant::now() + Duration::from_secs(n);
    std::thread::sleep(Duration::from_millis(30));
    std::fs::write(&abs, "return {}\n").unwrap();
    bump_mtime(&abs, 5);
    ce.poll_disk(later(1));
    assert_eq!(
        ce.docs[0].editor.text(),
        "return {}\n",
        "clean files reload"
    );
    assert!(!ce.docs[0].dirty());
    // With unsaved edits it asks instead.
    ce.docs[0].editor.type_text("-- mine ");
    std::fs::write(&abs, "return { other = 1 }\n").unwrap();
    bump_mtime(&abs, 10);
    ce.poll_disk(later(2));
    assert!(ce.docs[0].disk_conflict);
    assert!(ce.docs[0].editor.text().starts_with("-- mine"));
    ce.reload(0);
    assert_eq!(ce.docs[0].editor.text(), "return { other = 1 }\n");
    std::fs::remove_dir_all(root).ok();
}

/// Move a file's modification time forward (file systems may round to whole seconds).
fn bump_mtime(p: &Path, secs: u64) {
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(std::time::SystemTime::now() + Duration::from_secs(secs))
        .unwrap();
}

#[test]
fn syntax_errors_show_after_a_pause() {
    let (mut s, root) = sample_copy("syntax");
    s.code_editor.open(ROTATOR, None).unwrap();
    s.code_editor.docs[0].editor.type_text("local = 1\n");
    let mut h = harness(s);
    // The check waits for typing to pause.
    let ce = &mut h.state_mut().code_editor;
    assert!(ce.docs[0].diagnostic.is_none());
    ce.tick(Instant::now());
    ce.tick(Instant::now() + Duration::from_secs(2));
    h.run();
    let d = h.state().code_editor.docs[0]
        .diagnostic
        .clone()
        .expect("syntax error");
    assert_eq!(d.line, 0);
    h.get_by_label_contains("⛔ Line 1:");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn new_scripts_open_and_follow_renames() {
    let (mut s, root) = sample_copy("new");
    s.project.open_folder(root.join("Assets/Scripts"));
    crate::commands::apply(&mut s, Action::CreateAsset(AssetKind::Script));
    let new = "Assets/Scripts/NewBehaviourScript.lua";
    assert_eq!(s.code_editor.docs[0].path, new);
    assert!(
        !s.code_editor.ui.focus_text,
        "the rename field keeps the keyboard"
    );
    crate::commands::apply(
        &mut s,
        Action::RenameAsset {
            path: root.join(new),
            name: "Mover".into(),
        },
    );
    assert_eq!(s.code_editor.docs[0].path, "Assets/Scripts/Mover.lua");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn f5_plays_and_completion_accepts_with_tab() {
    let (mut s, root) = sample_copy("play");
    s.code_editor.open(ROTATOR, None).unwrap();
    s.code_editor.docs[0].editor.set_caret(Pos::new(0, 0));
    let mut h = harness(s);
    for c in [
        "I", "n", "p", "u", "t", ".", "G", "e", "t", "K", "e", "y", "D",
    ] {
        h.event(Event::Text(c.into()));
    }
    h.run();
    assert!(h.state().code_editor.ui.completion.is_some(), "popup open");
    h.get_by_label_contains("GetKeyDown(name)");
    h.key_press(Key::Tab);
    h.run();
    assert!(
        h.state().code_editor.docs[0]
            .editor
            .buf
            .line(0)
            .starts_with("Input.GetKeyDown")
    );
    h.key_press(Key::F5);
    h.run();
    assert!(
        h.state()
            .actions
            .iter()
            .any(|a| matches!(a, Action::TogglePlay))
    );
    std::fs::remove_dir_all(root).ok();
}
