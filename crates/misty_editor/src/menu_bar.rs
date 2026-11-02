//! Top menu bar: File, Edit, Assets, GameObject, Component, Window, Help.

use egui::Ui;

use crate::actions::{Action, LightKind, Primitive};
use crate::layout::Tab;
use crate::state::EditorState;

/// Menu entry with a right-aligned shortcut hint; pushes `action` when clicked.
fn item(ui: &mut Ui, state: &mut EditorState, label: &str, shortcut: &str, action: Action) {
    let mut button = egui::Button::new(label);
    if !shortcut.is_empty() {
        button = button.shortcut_text(shortcut);
    }
    if ui.add(button).clicked() {
        state.push(action);
        ui.close();
    }
}

/// Entry that only logs "not implemented yet" for now.
fn todo(ui: &mut Ui, state: &mut EditorState, label: &'static str, shortcut: &str) {
    item(ui, state, label, shortcut, Action::NotImplemented(label));
}

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| file_menu(ui, state));
        ui.menu_button("Edit", |ui| edit_menu(ui, state));
        ui.menu_button("Assets", |ui| assets_menu(ui, state));
        ui.menu_button("GameObject", |ui| gameobject_menu(ui, state));
        ui.menu_button("Component", |ui| component_menu(ui, state));
        ui.menu_button("Window", |ui| window_menu(ui, state));
        ui.menu_button("Help", |ui| help_menu(ui, state));
    });
}

fn file_menu(ui: &mut Ui, s: &mut EditorState) {
    item(ui, s, "New Scene", "Ctrl+N", Action::NewScene);
    todo(ui, s, "Open Scene", "Ctrl+O");
    ui.menu_button("Open Recent Scene", |ui| {
        ui.weak("(none)");
    });
    ui.separator();
    todo(ui, s, "Save", "Ctrl+S");
    todo(ui, s, "Save As...", "Ctrl+Shift+S");
    ui.separator();
    todo(ui, s, "New Project...", "");
    todo(ui, s, "Open Project...", "");
    ui.separator();
    todo(ui, s, "Build Settings...", "Ctrl+Shift+B");
    todo(ui, s, "Build And Run", "Ctrl+B");
    ui.separator();
    item(ui, s, "Exit", "", Action::Quit);
}

fn edit_menu(ui: &mut Ui, s: &mut EditorState) {
    todo(ui, s, "Undo", "Ctrl+Z");
    todo(ui, s, "Redo", "Ctrl+Y");
    ui.separator();
    todo(ui, s, "Select All", "Ctrl+A");
    item(ui, s, "Deselect All", "Shift+D", Action::DeselectAll);
    ui.separator();
    todo(ui, s, "Cut", "Ctrl+X");
    todo(ui, s, "Copy", "Ctrl+C");
    todo(ui, s, "Paste", "Ctrl+V");
    item(ui, s, "Duplicate", "Ctrl+D", Action::DuplicateSelected);
    todo(ui, s, "Rename", "F2");
    item(ui, s, "Delete", "Del", Action::DeleteSelected);
    ui.separator();
    item(ui, s, "Frame Selected", "F", Action::FrameSelected);
    todo(ui, s, "Lock View to Selected", "Shift+F");
    ui.separator();
    item(ui, s, "Play", "Ctrl+P", Action::TogglePlay);
    item(ui, s, "Pause", "Ctrl+Shift+P", Action::TogglePause);
    item(ui, s, "Step", "Ctrl+Alt+P", Action::Step);
    ui.separator();
    todo(ui, s, "Project Settings...", "");
    todo(ui, s, "Preferences...", "");
    todo(ui, s, "Shortcuts...", "");
}

fn assets_menu(ui: &mut Ui, s: &mut EditorState) {
    ui.menu_button("Create", |ui| {
        todo(ui, s, "Folder", "");
        ui.separator();
        todo(ui, s, "Script", "");
        todo(ui, s, "Scene", "");
        todo(ui, s, "Material", "");
    });
    todo(ui, s, "Show in Explorer", "");
    todo(ui, s, "Open", "");
    todo(ui, s, "Delete", "");
    todo(ui, s, "Rename", "");
    todo(ui, s, "Copy Path", "Alt+Ctrl+C");
    ui.separator();
    todo(ui, s, "Import New Asset...", "");
    if ui
        .add(egui::Button::new("Refresh").shortcut_text("Ctrl+R"))
        .clicked()
    {
        s.project.refresh();
        s.console.info("Asset database refreshed.");
        ui.close();
    }
}

/// Shared by the GameObject menu, the Hierarchy "+" button and its context menu.
pub fn create_menu_items(ui: &mut Ui, s: &mut EditorState) {
    item(ui, s, "Create Empty", "Ctrl+Shift+N", Action::CreateEmpty);
    ui.menu_button("3D Object", |ui| {
        for p in Primitive::ALL {
            item(ui, s, p.name(), "", Action::CreatePrimitive(p));
        }
    });
    ui.menu_button("Light", |ui| {
        for l in LightKind::ALL {
            item(ui, s, l.name(), "", Action::CreateLight(l));
        }
    });
    ui.menu_button("Audio", |ui| todo(ui, s, "Audio Source", ""));
    ui.menu_button("UI", |ui| todo(ui, s, "Canvas", ""));
    item(ui, s, "Camera", "", Action::CreateCamera);
}

fn gameobject_menu(ui: &mut Ui, s: &mut EditorState) {
    create_menu_items(ui, s);
    ui.separator();
    todo(ui, s, "Make Parent", "");
    todo(ui, s, "Clear Parent", "");
    ui.separator();
    todo(ui, s, "Move To View", "Ctrl+Alt+F");
    todo(ui, s, "Align With View", "Ctrl+Shift+F");
    ui.separator();
    todo(ui, s, "Toggle Active State", "Alt+Shift+A");
}

fn component_menu(ui: &mut Ui, s: &mut EditorState) {
    todo(ui, s, "Add...", "Ctrl+Shift+A");
    ui.separator();
    for cat in [
        "Mesh",
        "Effects",
        "Physics",
        "Audio",
        "Rendering",
        "Scripts",
        "UI",
    ] {
        ui.menu_button(cat, |ui| {
            ui.weak("(coming soon)");
        });
    }
}

fn window_menu(ui: &mut Ui, s: &mut EditorState) {
    ui.menu_button("Layouts", |ui| {
        item(ui, s, "Default", "", Action::ResetLayout);
    });
    ui.separator();
    ui.menu_button("General", |ui| {
        for tab in Tab::ALL {
            item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
        }
    });
    todo(ui, s, "Package Manager", "");
}

fn help_menu(ui: &mut Ui, s: &mut EditorState) {
    if ui.button("About Misty Engine").clicked() {
        s.console
            .info(concat!("Misty Engine ", env!("CARGO_PKG_VERSION")));
        ui.close();
    }
    todo(ui, s, "Manual", "");
    todo(ui, s, "Scripting Reference", "");
}
