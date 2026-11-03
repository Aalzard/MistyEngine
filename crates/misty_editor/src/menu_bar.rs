//! Top menu bar: File, Edit, Assets, GameObject, Component, Window, Help.

use egui::Ui;

use misty_engine::hecs::Entity;
use misty_engine::scene::registry;
use misty_engine::scene::{LightType, PrimitiveMesh};

use crate::actions::{Action, Create};
use crate::layout::{BuiltinLayout, Tab};
use crate::layouts::LayoutRef;
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
    item(ui, s, "Open Scene", "Ctrl+O", Action::OpenScene);
    ui.menu_button("Open Recent Scene", |ui| {
        let recent = s.files.settings.recent_scenes.clone();
        if recent.is_empty() {
            ui.weak("(none)");
        }
        for rel in recent {
            item(ui, s, &rel, "", Action::OpenScenePath(rel.clone()));
        }
    });
    ui.separator();
    item(ui, s, "Save", "Ctrl+S", Action::SaveScene);
    item(ui, s, "Save As...", "Ctrl+Shift+S", Action::SaveSceneAs);
    ui.separator();
    todo(ui, s, "New Project...", "");
    todo(ui, s, "Open Project...", "");
    ui.separator();
    item(
        ui,
        s,
        "Build Settings...",
        "Ctrl+Shift+B",
        Action::OpenBuildSettings,
    );
    item(ui, s, "Build And Run", "Ctrl+B", Action::BuildAndRun);
    ui.separator();
    item(ui, s, "Exit", "", Action::Quit);
}

fn edit_menu(ui: &mut Ui, s: &mut EditorState) {
    let undo = s.undo.undo_label().map(|l| format!("Undo {l}"));
    let redo = s.undo.redo_label().map(|l| format!("Redo {l}"));
    ui.add_enabled_ui(undo.is_some(), |ui| {
        item(
            ui,
            s,
            undo.as_deref().unwrap_or("Undo"),
            "Ctrl+Z",
            Action::Undo,
        )
    });
    ui.add_enabled_ui(redo.is_some(), |ui| {
        item(
            ui,
            s,
            redo.as_deref().unwrap_or("Redo"),
            "Ctrl+Y",
            Action::Redo,
        )
    });
    ui.separator();
    item(ui, s, "Select All", "Ctrl+A", Action::SelectAll);
    item(ui, s, "Deselect All", "Shift+D", Action::DeselectAll);
    ui.separator();
    item(ui, s, "Cut", "Ctrl+X", Action::CutObjects);
    item(ui, s, "Copy", "Ctrl+C", Action::CopyObjects);
    let paste = Action::PasteObjects {
        text: None,
        as_child: false,
    };
    item(ui, s, "Paste", "Ctrl+V", paste);
    item(ui, s, "Duplicate", "Ctrl+D", Action::DuplicateSelected);
    item(ui, s, "Rename", "F2", Action::RenameSelected);
    item(ui, s, "Delete", "Del", Action::DeleteSelected);
    ui.separator();
    item(ui, s, "Frame Selected", "F", Action::FrameSelected);
    let lock = if s.scene_view.lock_to_selection {
        "✔ Lock View to Selected"
    } else {
        "Lock View to Selected"
    };
    item(ui, s, lock, "Shift+F", Action::ToggleLockView);
    item(
        ui,
        s,
        "Align With View",
        "Ctrl+Shift+F",
        Action::AlignWithView,
    );
    ui.separator();
    item(ui, s, "Grid and Snap...", "", Action::OpenGridAndSnap);
    ui.separator();
    item(ui, s, "Play", "Ctrl+P", Action::TogglePlay);
    item(ui, s, "Pause", "Ctrl+Shift+P", Action::TogglePause);
    item(ui, s, "Step", "Ctrl+Alt+P", Action::Step);
    ui.separator();
    item(
        ui,
        s,
        "Project Settings...",
        "",
        Action::OpenProjectSettings,
    );
    todo(ui, s, "Preferences...", "");
    todo(ui, s, "Shortcuts...", "");
}

fn assets_menu(ui: &mut Ui, s: &mut EditorState) {
    ui.menu_button("Create", |ui| crate::panels::project::create_menu(ui, s));
    let selected = s.project.selected.clone();
    ui.add_enabled_ui(selected.is_some(), |ui| {
        let path = selected.clone().unwrap_or_default();
        item(
            ui,
            s,
            "Show in Explorer",
            "",
            Action::ShowInExplorer(path.clone()),
        );
        if ui.button("Rename").clicked() {
            s.project.start_rename(path.clone());
            ui.close();
        }
        item(
            ui,
            s,
            "Duplicate",
            "Ctrl+D",
            Action::DuplicateAsset(path.clone()),
        );
        if ui.button("Delete").clicked() {
            s.project.confirm_delete = Some(path);
            ui.close();
        }
    });
    ui.separator();
    todo(ui, s, "Import New Asset...", "");
    if ui
        .add(egui::Button::new("Refresh").shortcut_text("Ctrl+R"))
        .clicked()
    {
        s.refresh_assets();
        s.console.info("Asset database refreshed.");
        ui.close();
    }
}

/// Shared by the GameObject menu, the Hierarchy "+" button and its context menus.
/// `parent` = create as a child of this object.
pub fn create_menu_items(ui: &mut Ui, s: &mut EditorState, parent: Option<Entity>) {
    let create = |what| Action::Create { what, parent };
    let shortcut = if parent.is_none() { "Ctrl+Shift+N" } else { "" };
    item(ui, s, "Create Empty", shortcut, create(Create::Empty));
    ui.menu_button("3D Object", |ui| {
        for p in PrimitiveMesh::ALL {
            item(ui, s, p.name(), "", create(Create::Primitive(p)));
        }
        ui.separator();
        item(ui, s, "Terrain", "", create(Create::Terrain));
    });
    ui.menu_button("Effects", |ui| {
        item(ui, s, "Particle System", "", create(Create::ParticleSystem));
    });
    ui.menu_button("Light", |ui| {
        for l in LightType::ALL {
            let what = Create::Light(l);
            item(ui, s, what.default_name(), "", create(what));
        }
    });
    ui.menu_button("Audio", |ui| {
        item(ui, s, "Audio Source", "", create(Create::AudioSource));
    });
    ui.menu_button("UI", |ui| {
        for el in crate::ui_create::UiElement::ALL {
            item(ui, s, el.name(), "", create(Create::Ui(el)));
            if el.ends_group() {
                ui.separator();
            }
        }
    });
    item(ui, s, "Camera", "", create(Create::Camera));
}

fn gameobject_menu(ui: &mut Ui, s: &mut EditorState) {
    create_menu_items(ui, s, None);
    let child_parent = s.selection.active();
    ui.add_enabled_ui(child_parent.is_some(), |ui| {
        let action = Action::Create {
            what: Create::Empty,
            parent: child_parent,
        };
        item(ui, s, "Create Empty Child", "Alt+Shift+N", action);
    });
    item(
        ui,
        s,
        "Create Empty Parent",
        "Ctrl+Shift+G",
        Action::CreateEmptyParent,
    );
    ui.separator();
    item(ui, s, "Clear Parent", "", Action::ClearParent);
    item(
        ui,
        s,
        "Set as first sibling",
        "",
        Action::SetSibling { first: true },
    );
    item(
        ui,
        s,
        "Set as last sibling",
        "",
        Action::SetSibling { first: false },
    );
    ui.separator();
    item(ui, s, "Move To View", "Ctrl+Alt+F", Action::MoveToView);
    item(
        ui,
        s,
        "Align With View",
        "Ctrl+Shift+F",
        Action::AlignWithView,
    );
    item(
        ui,
        s,
        "Align View to Selected",
        "",
        Action::AlignViewToSelected,
    );
    ui.separator();
    item(
        ui,
        s,
        "Toggle Active State",
        "Alt+Shift+A",
        Action::ToggleActive,
    );
}

fn component_menu(ui: &mut Ui, s: &mut EditorState) {
    item(ui, s, "Add...", "Ctrl+Shift+A", Action::OpenAddComponent);
    ui.separator();
    let has_selection = !s.selection.is_empty();
    for cat in [
        "Mesh",
        "Effects",
        "Physics",
        "Navigation",
        "Audio",
        "Rendering",
        "Layout",
        "Miscellaneous",
        "Scripts",
        "UI",
    ] {
        ui.menu_button(cat, |ui| {
            let entries: Vec<_> = registry::components()
                .iter()
                .filter(|c| c.category == cat)
                .collect();
            if entries.is_empty() {
                ui.weak("(coming soon)");
            }
            for info in entries {
                ui.add_enabled_ui(has_selection, |ui| {
                    item(ui, s, info.name, "", Action::AddComponent(info.name))
                });
            }
        });
    }
}

fn window_menu(ui: &mut Ui, s: &mut EditorState) {
    ui.menu_button("Layouts", |ui| layouts_menu(ui, s));
    ui.separator();
    ui.menu_button("General", |ui| {
        for tab in Tab::ALL.into_iter().filter(|t| *t != Tab::Animation) {
            item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
        }
    });
    ui.menu_button("Animation", |ui| {
        for tab in [Tab::Animation, Tab::Animator] {
            item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
        }
    });
    ui.menu_button("Audio", |ui| {
        let tab = Tab::AudioMixer;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    ui.menu_button("Rendering", |ui| {
        let tab = Tab::Lighting;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    ui.menu_button("AI", |ui| {
        let tab = Tab::Navigation;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    todo(ui, s, "Package Manager", "");
}

/// Window ▸ Layouts, also used by the toolbar's Layout dropdown.
pub fn layouts_menu(ui: &mut Ui, s: &mut EditorState) {
    let current = s.layouts.current_name.clone();
    let entry = |ui: &mut Ui, s: &mut EditorState, name: &str, layout: LayoutRef| {
        if ui.selectable_label(name == current, name).clicked() {
            s.push(Action::LoadLayout(layout));
            ui.close();
        }
    };
    for b in BuiltinLayout::ALL {
        entry(ui, s, b.name(), LayoutRef::Builtin(b));
    }
    let names: Vec<String> = s.layouts.user.keys().cloned().collect();
    if !names.is_empty() {
        ui.separator();
    }
    for name in &names {
        entry(ui, s, name, LayoutRef::User(name.clone()));
    }
    ui.separator();
    if ui.button("Save Layout...").clicked() {
        s.layouts.save_prompt = Some(String::new());
        ui.close();
    }
    ui.add_enabled_ui(!names.is_empty(), |ui| {
        ui.menu_button("Delete Layout", |ui| {
            for name in &names {
                item(ui, s, name, "", Action::DeleteLayout(name.clone()));
            }
        });
    });
    item(ui, s, "Reset All Layouts", "", Action::ResetAllLayouts);
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
